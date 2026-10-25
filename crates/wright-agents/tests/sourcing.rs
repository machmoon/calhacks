//! The sourcing tools (lane 8, src/sourcing.rs) through the whole harness,
//! offline: a toy domain with a three-row fake catalog, a scripted
//! transport, no network.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use wright_agents::fake::{reply, tool_use, ScriptedTransport};
use wright_agents::{channel, harness, tools, Budget, Client, RunConfig, StopReason};
use wright_core::units::Money;
use wright_core::{Artifacts, Bom, BomLine, CatalogOption, CheckId, Domain, Finished, Report, Source};

struct Toy;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    budget_usd: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Design {
    motor: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum C {
    Budget,
}

impl CheckId for C {
    const ALL: &'static [Self] = &[C::Budget];
    fn name(self) -> &'static str {
        "budget"
    }
    fn describe(self) -> &'static str {
        "price within budget"
    }
}

/// (id, kv, mass_g, price, stock label)
const ROWS: &[(&str, &str, &str, f64, &str)] = &[
    ("m_780", "780", "81", 50.0, "in stock"),
    ("m_900", "900", "108", 30.0, "stock unstated"),
    ("m_470", "470", "81", 50.0, "backorder"),
];

fn rows() -> Vec<CatalogOption> {
    ROWS.iter()
        .map(|(id, kv, mass, price, stock)| {
            let specs: BTreeMap<String, String> = [("kv", *kv), ("mass_g", *mass), ("stock", *stock)].into_iter().map(|(k, v)| (k.into(), v.into())).collect();
            CatalogOption { id: id.to_string(), label: format!("Motor {kv}KV"), specs, price_usd: Some(*price), in_stock: None, url: format!("https://x/{id}") }
        })
        .collect()
}

fn price(id: &str) -> Option<f64> {
    ROWS.iter().find(|r| r.0 == id).map(|r| r.3)
}

impl Domain for Toy {
    const NAME: &'static str = "toy";
    type Spec = Spec;
    type Design = Design;
    type Check = C;
    fn spec_schema() -> Value {
        json!({"type": "object", "properties": {"budget_usd": {"type": "number"}}})
    }
    fn design(_: &Spec) -> anyhow::Result<Design> {
        Ok(Design { motor: "m_780".into() })
    }
    fn check(s: &Spec, d: &Design) -> Finished {
        let mut r = Report::<C>::new();
        r.margin(C::Budget, s.budget_usd - Self::bom(d).total().as_usd(), "USD", Source::Requirement, "budget - price");
        r.finish()
    }
    fn bom(d: &Design) -> Bom {
        Bom {
            lines: vec![BomLine {
                category: "motor".into(),
                id: d.motor.clone(),
                name: format!("Motor {}", d.motor),
                qty: 4.0,
                unit: "ea".into(),
                unit_price: price(&d.motor).map(Money::usd),
                url: String::new(),
                note: String::new(),
            }],
        }
    }
    fn build(_: &Design, _: &Path) -> anyhow::Result<Artifacts> {
        Ok(Artifacts::default())
    }
    fn objective(_: &Design) -> (f64, &'static str) {
        (1.0, "one")
    }
    fn catalog_paths() -> Vec<(&'static str, &'static str)> {
        vec![("motor", "lift motor")]
    }
    fn options(_: &Design, path: &str, _: Option<&str>) -> Vec<CatalogOption> {
        if path == "motor" {
            rows()
        } else {
            vec![]
        }
    }
}

const M: &str = "claude-sonnet-5-5";

fn result_text(req: &Value) -> (bool, String) {
    let msgs = req["messages"].as_array().unwrap();
    let block = &msgs.last().unwrap()["content"].as_array().unwrap()[0];
    assert_eq!(block["type"], "tool_result");
    (block["is_error"].as_bool().unwrap_or(false), block["content"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn sourcing_tools_search_rank_gate_and_diff_offline() {
    let script = vec![
        reply(M, vec![tool_use("o1", "design", json!({}))], 1000, 50),
        reply(M, vec![tool_use("o2", "catalog_search", json!({"path": "motor", "query": "", "ranges": "kv:850..", "stock": "listed", "sort": ""}))], 1000, 50),
        reply(M, vec![tool_use("o3", "catalog_search", json!({"path": "motor", "query": "", "ranges": "", "stock": "lead_time", "sort": "price"}))], 1000, 50),
        reply(M, vec![tool_use("o4", "alternates", json!({"path": "motor", "id": ""}))], 1000, 50),
        reply(M, vec![tool_use("o5", "edit_design", json!({"patch_json": "{\"motor\": \"m_78o\"}", "why": "typo"}))], 1000, 50),
        reply(M, vec![tool_use("o6", "edit_design", json!({"patch_json": "{\"motor\": \"m_900\"}", "why": "cheaper"}))], 1000, 50),
        reply(M, vec![tool_use("o7", "bom_diff", json!({}))], 1000, 50),
        reply(M, vec![tool_use("o8", "check", json!({}))], 1000, 50),
        reply(M, vec![tool_use("o9", "submit", json!({"summary": "m_900"}))], 1000, 50),
        reply(M, vec![tool_use("r1", "verdict", json!({"summary": "fine"}))], 1000, 50),
    ];
    let transport = Arc::new(ScriptedTransport::new(script));
    let (tx, _rx) = channel();
    let client = Client::new(transport.clone(), Budget::new(1.0, 45.0, None), tx, "run-s");
    let cfg = RunConfig::new("run-s", "a motor under $400", json!({"budget_usd": 400.0}));

    let out = harness::run(client, tools::<Toy>(), cfg).await;
    assert_eq!(out.stop, StopReason::Passed, "{:?}", out.stop);
    assert_eq!(out.state.design.as_ref().unwrap()["motor"], "m_900");

    let reqs = transport.requests.lock().unwrap();
    // o2: a kv range keeps only the 900 KV row, with why it matched
    let (err, t) = result_text(&reqs[2].body);
    assert!(!err, "{t}");
    assert!(t.starts_with("1 of 1 rows match at motor kv >= 850 stock=listed, best first:\n"), "{t}");
    assert!(t.contains("- m_900 | Motor 900KV | kv=900, mass_g=108 | $30.00 | stock unstated -- kv 900 within >= 850; stock unstated; $30.00"), "{t}");
    // o3: lead_time admits the backorder row; sort by price leads
    let (_, t) = result_text(&reqs[3].body);
    let order: Vec<usize> = ["m_900", "m_780", "m_470"].iter().map(|id| t.find(&format!("- {id} ")).unwrap()).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{t}");
    // o4: alternates of the current choice with the delta table
    let (_, t) = result_text(&reqs[4].body);
    assert!(t.starts_with("incumbent m_780 | Motor 780KV | $50.00 | in stock\n"), "{t}");
    assert!(t.contains("- m_900 | Motor 900KV | $30.00 (-$20.00) | stock unstated | kv 780 -> 900 (+15%), mass_g 81 -> 108 (+33%)"), "{t}");
    assert!(t.contains("- m_470 | Motor 470KV | $50.00 (+$0.00) | weeks (made to order / pre-order / backorder) | kv 780 -> 470 (-40%)"), "{t}");
    // o5: the made-up id is rejected with the nearest real one (review 5, F9)
    // (the trace layer may wrap an error as a JSON `{"error": ...}` block,
    // so judge the text, not the flag)
    let (_, t) = result_text(&reqs[5].body);
    assert!(t.contains("edit rejected, nothing changed: `m_78o` is not a catalog id at motor; did you mean `m_780`?"), "{t}");
    assert!(t.contains("Nearest ids: `m_470`, `m_900`"), "{t}");
    // o6 accepted; o7: the diff against the incumbent (round 1's passing design)
    let (err, _) = result_text(&reqs[6].body);
    assert!(!err);
    let (err, t) = result_text(&reqs[7].body);
    assert!(!err, "{t}");
    assert!(t.starts_with("BOM diff: best verified design (round 1) -> current design\n"), "{t}");
    assert!(t.contains("  Adding    m_900: 4 x Motor m_900 @ $30.00 [+$120.00], stock unstated"), "{t}");
    assert!(t.contains("  Removing  m_780: 4 x Motor m_780 @ $50.00 [-$200.00]"), "{t}");
    assert!(t.contains("1 added, 1 removed, 0 changed, 0 unchanged"), "{t}");
    assert!(t.contains("total $200.00 -> $120.00 (-$80.00); unpriced lines 0 -> 0"), "{t}");
    assert!(t.contains("stock: the candidate brings in 1 part(s) not in stock: m_900 (stock unstated)"), "{t}");
}
