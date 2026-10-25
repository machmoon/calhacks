//! Budget guards, ported from the review's probes: each test failed before
//! the fix it names (one authorized call past the run limit, free responses
//! without `usage`, concurrent calls on one client, lost ledger updates,
//! NaN limits).

use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use wright_agents::budget::{BudgetError, Ledger};
use wright_agents::client::{ApiError, BoxFut, CallError, Provider, Request, Transport};
use wright_agents::fake::{reply, text, ScriptedTransport};
use wright_agents::price::Usage;
use wright_agents::{channel, Budget, Client};
use wright_core::Event;

const SONNET: &str = "claude-sonnet-5-5";

fn body() -> Value {
    json!({"model": SONNET, "max_tokens": 8000, "messages": [{"role": "user", "content": "x"}]})
}

fn client(t: Arc<dyn Transport>, b: Budget) -> (Client, tokio::sync::broadcast::Receiver<Event>) {
    let (tx, rx) = channel();
    (Client::new(t, b, tx, "run-1"), rx)
}

/// The Claude API (server-side fallbacks on) answering every request with
/// the given model, as `fallbacks: "default"` may.
struct AnswersAs {
    model: &'static str,
    seen: Mutex<Vec<Request>>,
}
impl Transport for AnswersAs {
    fn provider(&self) -> Provider {
        Provider::Anthropic
    }
    fn send<'a>(&'a self, req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        self.seen.lock().unwrap().push(req.clone());
        Box::pin(async move { Ok(reply(self.model, vec![text("hi")], 1000, 8000)) })
    }
}

#[tokio::test]
async fn fallback_to_a_dearer_model_cannot_cross_the_run_limit() {
    // Sonnet 5.5 worst case ~$0.08 fits $0.10, but with fallbacks the
    // answer may come from Opus 4.8 at 5/25 (~$0.205), so it is refused.
    let t = Arc::new(AnswersAs { model: "claude-opus-4-8", seen: Mutex::new(vec![]) });
    let (c, _rx) = client(t.clone(), Budget::new(0.10, 45.0, None));
    let r = c.create(body()).await;
    assert!(matches!(r, Err(CallError::Budget(BudgetError::Run { .. }))), "{r:?}");
    assert!(t.seen.lock().unwrap().is_empty(), "refused call was sent");
    assert!(c.spent() <= 0.10);
    assert_eq!(c.budget.lock().unwrap().reserved, 0.0);

    // With room for the dearest target the call goes out with fallbacks and
    // is booked at the model that answered, within what was authorized.
    let (c, _rx) = client(t.clone(), Budget::new(0.25, 45.0, None));
    c.create(body()).await.unwrap();
    assert_eq!(t.seen.lock().unwrap()[0].body["fallbacks"], "default");
    let opus = Usage { input: 1000, output: 8000, ..Default::default() }.usd("claude-opus-4-8");
    assert!((c.spent() - opus).abs() < 1e-12 && c.spent() <= 0.25);
}

#[tokio::test]
async fn a_response_without_usage_books_the_worst_case() {
    let mut n = 0;
    let t = Arc::new(ScriptedTransport::from_fn(move |_| {
        n += 1;
        if n > 50 {
            return None;
        }
        let mut r = json!({"type": "message", "model": SONNET, "content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn"});
        // Alternate missing and malformed usage.
        if n % 2 == 0 {
            r["usage"] = json!({"input_tokens": "many", "output_tokens": 3});
        }
        Some(r)
    }));
    let (c, mut rx) = client(t, Budget::new(0.10, 45.0, None));
    let mut ok = 0;
    for _ in 0..50 {
        if c.create(body()).await.is_ok() {
            ok += 1;
        }
    }
    assert_eq!(ok, 1, "{ok} calls of up to $0.08 accepted under a $0.10 limit");
    let worst = wright_agents::price::worst_case_usd(SONNET, &body());
    assert!((c.spent() - worst).abs() < 1e-12, "spent {}", c.spent());
    let mut said = false;
    while let Ok(e) = rx.try_recv() {
        if let Event::AgentMessage { agent, text, .. } = e {
            said |= agent == "budget" && text.contains("no readable usage");
        }
    }
    assert!(said, "missing usage was not reported");
}

struct Slow;
impl Transport for Slow {
    fn provider(&self) -> Provider {
        Provider::Fake
    }
    fn send<'a>(&'a self, _req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(reply(SONNET, vec![text("hi")], 1000, 8000))
        })
    }
}

// current_thread still interleaves the four calls at the transport await,
// which is where the unreserved check used to let all of them through.
#[tokio::test]
async fn concurrent_calls_on_one_client_cannot_cross_the_run_limit() {
    let (c, _rx) = client(Arc::new(Slow), Budget::new(0.10, 45.0, None));
    let hs: Vec<_> = (0..4)
        .map(|_| {
            let c = c.clone();
            tokio::spawn(async move { c.create(body()).await.is_ok() })
        })
        .collect();
    let mut ok = 0;
    for h in hs {
        if h.await.unwrap() {
            ok += 1;
        }
    }
    assert_eq!(ok, 1, "{ok} of 4 concurrent calls accepted");
    assert!(c.spent() <= 0.10, "spent {}", c.spent());
    assert_eq!(c.budget.lock().unwrap().reserved, 0.0, "settled call left a reservation");
}

struct Fails;
impl Transport for Fails {
    fn provider(&self) -> Provider {
        Provider::Fake
    }
    fn send<'a>(&'a self, _req: &'a Request) -> BoxFut<'a, Result<Value, ApiError>> {
        Box::pin(async move { Err(ApiError::Http { status: 500, body: "boom".into() }) })
    }
}

#[tokio::test]
async fn a_failed_call_releases_its_reservation() {
    let (c, _rx) = client(Arc::new(Fails), Budget::new(0.10, 45.0, None));
    for _ in 0..3 {
        assert!(matches!(c.create(body()).await, Err(CallError::Api(_))));
    }
    let b = c.budget.lock().unwrap();
    assert_eq!((b.reserved, b.run_spent), (0.0, 0.0));
}

#[test]
fn ledger_keeps_every_update_across_writers() {
    let dir = std::env::temp_dir().join(format!("wright-budget-it-{}", std::process::id()));
    let path = dir.join("ledger.json");
    let _ = std::fs::remove_dir_all(&dir);
    let hs: Vec<_> = (0..4)
        .map(|_| {
            let p = path.clone();
            std::thread::spawn(move || {
                let mut b = Budget::new(1e9, 1e9, Some(p));
                for _ in 0..100 {
                    b.record(SONNET, &Usage { output: 1000, ..Default::default() }, true).unwrap();
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    let l = Ledger::load(&path).unwrap();
    assert_eq!(l.calls, 400);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nan_budget_refuses() {
    let b = Budget::new("nan".parse().unwrap(), 45.0, None);
    assert!(matches!(b.authorize(SONNET, &json!({"max_tokens": 1_000_000}), false), Err(BudgetError::Config(_))));
}
