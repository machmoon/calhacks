//! Offline tests of spec elicitation and the feasibility probe: a scripted
//! analyst, a toy domain whose range check fails above 250 km, no network.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use wright_agents::elicit::{self, Asker, ElicitConfig, HumanInput, NoHuman, Origin};
use wright_agents::fake::{reply, text, tool_use, ScriptedTransport};
use wright_agents::{channel, tools, Budget, Client};
use wright_core::{Artifacts, Bom, CheckId, Domain, Event, Finished, Report, Source};

/// A plane whose only physics is: range above 250 km fails on `cells_s` 6
/// (300 km passes on 8S), and cost is 1 USD per km of range.
struct Toy;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    requirements: Req,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Req {
    range_km: f64,
    payload_g: f64,
    #[serde(default)]
    budget_usd: Option<f64>,
    #[serde(default = "six")]
    cells_s: u32,
    #[serde(default = "fifteen")]
    hot_day_dk: f64,
}
fn six() -> u32 {
    6
}
fn fifteen() -> f64 {
    15.0
}

#[derive(Clone, Serialize, Deserialize)]
struct Design {
    range_km: f64,
    price_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum C {
    Range,
    Budget,
}
impl CheckId for C {
    const ALL: &'static [Self] = &[C::Range, C::Budget];
    fn name(self) -> &'static str {
        match self {
            C::Range => "range",
            C::Budget => "budget",
        }
    }
    fn describe(self) -> &'static str {
        ""
    }
}

impl Domain for Toy {
    const NAME: &'static str = "toy";
    type Spec = Spec;
    type Design = Design;
    type Check = C;
    fn spec_schema() -> Value {
        json!({"type": "object", "properties": {"requirements": {"type": "object", "properties": {
            "range_km": {"type": "number", "minimum": 0, "maximum": 1000},
            "payload_g": {"type": "number", "minimum": 0},
            "budget_usd": {"type": ["number", "null"]},
            "cells_s": {"type": "integer", "description": "default 6"},
            "hot_day_dk": {"type": "number", "description": "default 15"}
        }}}})
    }
    fn design(s: &Spec) -> anyhow::Result<Design> {
        let cap = if s.requirements.cells_s >= 8 { 400.0 } else { 250.0 };
        Ok(Design { range_km: s.requirements.range_km.min(cap), price_usd: s.requirements.range_km.min(cap) })
    }
    fn check(s: &Spec, d: &Design) -> Finished {
        let mut r = Report::<C>::new();
        r.margin(C::Range, d.range_km - s.requirements.range_km, "km", Source::Requirement, "range - required");
        match s.requirements.budget_usd {
            Some(b) => r.margin(C::Budget, b - d.price_usd, "USD", Source::Requirement, "budget - price"),
            None => r.margin(C::Budget, 0.0, "USD", Source::Requirement, "no budget stated"),
        }
        r.finish()
    }
    fn bom(_: &Design) -> Bom {
        Bom::default()
    }
    fn build(_: &Design, _: &Path) -> anyhow::Result<Artifacts> {
        Ok(Artifacts::default())
    }
    fn objective(d: &Design) -> (f64, &'static str) {
        (d.range_km, "range km")
    }
}

fn client(script: Vec<Value>) -> (Client, Arc<ScriptedTransport>, tokio::sync::broadcast::Receiver<Event>) {
    let t = Arc::new(ScriptedTransport::new(script));
    let (tx, rx) = channel();
    (Client::new(t.clone(), Budget::new(5.0, 45.0, None), tx, "elicit-test"), t, rx)
}

fn proposal(spec: Value, from_request: &[&str], assumptions: Value, questions: Value) -> Value {
    reply(
        "m",
        vec![tool_use(
            "t1",
            "propose_spec",
            json!({
                "reading": "a 300 km electric plane carrying 900 g",
                "spec_json": spec.to_string(),
                "from_request": from_request,
                "assumptions": assumptions,
                "questions": questions
            }),
        )],
        100,
        50,
    )
}

fn cfg(human: HumanInput) -> ElicitConfig {
    ElicitConfig::new(wright_agents::agent::ModelCfg::new("m", "low", 1000), human)
}

fn drain(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<Event> {
    let mut v = vec![];
    while let Ok(e) = rx.try_recv() {
        v.push(e);
    }
    v
}

#[tokio::test]
async fn unattended_run_sources_every_value_and_names_the_binding_requirement() {
    let spec = json!({"requirements": {"range_km": 300, "payload_g": 907.185, "budget_usd": 400, "hot_day_dk": 15}});
    let (client, t, mut rx) = client(vec![proposal(
        spec,
        &["requirements.range_km", "requirements.payload_g", "requirements.budget_usd"],
        json!([{"path": "requirements.hot_day_dk", "source": "convention", "note": "ISA+15 K hot day, the schema's documented default"}]),
        json!([{"path": "requirements.cells_s", "question": "Which pack voltage class?", "options_json": "[4, 6, 8]", "default_json": "6", "why": "range above 250 km needs 8S"}]),
    )]);
    let domain = tools::<Toy>();
    let e = elicit::elicit(&client, domain.as_ref(), "fly 300 miles... I mean 300 km with a 2 lb payload for under $400", &Value::Null, &cfg(HumanInput::Never), &mut NoHuman)
        .await
        .unwrap();

    // the analyst saw the normalised quantities, not raw text alone
    let sent = t.requests.lock().unwrap()[0].body.clone();
    let user = sent["messages"][0]["content"][0]["text"].as_str().unwrap();
    assert!(user.contains("2 lb = 907.185 g"), "{user}");
    assert!(user.contains("$400 = 400 USD"), "{user}");

    // locked = exactly from_request
    assert_eq!(e.stated, json!({"requirements": {"range_km": 300.0, "payload_g": 907.185, "budget_usd": 400.0}}));
    // the question was answered from its default and recorded as such
    assert_eq!(e.answered.len(), 1);
    assert_eq!(e.answered[0].source, Origin::UnattendedDefault);
    assert_eq!(e.spec["requirements"]["cells_s"], json!(6));
    // every non-locked leaf has a sourced assumption
    let paths: Vec<&str> = e.assumptions.iter().map(|a| a.path.as_str()).collect();
    assert!(paths.contains(&"requirements.hot_day_dk"), "{paths:?}");
    assert!(paths.contains(&"requirements.cells_s"), "{paths:?}");
    assert!(e.assumptions.iter().any(|a| a.source == Origin::Convention && a.note.contains("ISA+15")));

    // the probe: 300 km on 6S is infeasible; relaxing range alone passes
    assert!(!e.probe.feasible);
    assert_eq!(e.probe.failing, vec!["range".to_string()]);
    let b = e.probe.binding.as_ref().expect("a binding requirement");
    assert_eq!(b.path, "requirements.range_km");
    assert!(b.to <= 250.0 && b.to >= 200.0, "{b:?}");
    assert!(e.probe.designs_run <= 12);
    let block = e.assumptions_block();
    assert!(block.contains("binding requirement requirements.range_km = 300"), "{block}");

    // events for the desktop: the block as an elicit message, the probe as a tool pair
    let evs = drain(&mut rx);
    assert!(evs.iter().any(|ev| matches!(ev, Event::AgentMessage { agent, text, .. } if agent == "elicit" && text.contains("Assumptions (each with its source)"))));
    assert!(evs.iter().any(|ev| matches!(ev, Event::ToolResult { tool, ok: false, .. } if tool == "feasibility_probe")));

    // apply() installs locks and assumptions where the harness reads them
    let mut state = wright_agents::DesignState::new("toy", "req", Value::Null);
    e.apply(&mut state);
    assert_eq!(state.stated, e.stated);
    assert!(state.assumptions.iter().any(|a| a.contains("requirements.cells_s")));
}

struct Scripted(Vec<&'static str>, Arc<Mutex<Vec<String>>>);
impl Asker for Scripted {
    fn get_human_input(&mut self, prompt: &str) -> String {
        self.1.lock().unwrap().push(prompt.to_string());
        self.0.remove(0).to_string()
    }
}

#[tokio::test]
async fn a_person_s_answer_is_applied_and_makes_the_spec_feasible() {
    let spec = json!({"requirements": {"range_km": 300, "payload_g": 900}});
    let (client, _t, _rx) = client(vec![proposal(
        spec,
        &["requirements.range_km", "requirements.payload_g"],
        json!([]),
        json!([{"path": "requirements.cells_s", "question": "Which pack voltage class?", "options_json": "[4, 6, 8]", "default_json": "6", "why": "range above 250 km needs 8S"}]),
    )]);
    let prompts = Arc::new(Mutex::new(vec![]));
    let mut asker = Scripted(vec!["8"], prompts.clone());
    let e = elicit::elicit(&client, tools::<Toy>().as_ref(), "300 km with 900 g", &Value::Null, &cfg(HumanInput::Always), &mut asker).await.unwrap();
    assert!(prompts.lock().unwrap()[0].contains("Which pack voltage class?"));
    assert_eq!(e.spec["requirements"]["cells_s"], json!(8));
    assert_eq!(e.answered[0].source, Origin::Answer);
    assert!(e.probe.feasible, "{}", e.probe.summary());
    assert_eq!(e.probe.designs_run, 1);
    // the person's answer is an assumption the reviewer can read, sourced as an answer
    assert!(e.assumptions.iter().any(|a| a.path == "requirements.cells_s" && a.source == Origin::Answer));
}

#[tokio::test]
async fn a_rejected_spec_is_sent_back_with_the_reason_and_too_many_questions_are_refused() {
    let q = |p: &str| json!({"path": p, "question": "?", "options_json": "[]", "default_json": "1", "why": "w"});
    let (client, t, _rx) = client(vec![
        // 1: a field the domain does not have
        proposal(json!({"requirements": {"range_km": 100, "payload_g": 1, "wingspan_mm": 1200}}), &["requirements.range_km"], json!([]), json!([])),
        // 2: four questions
        proposal(json!({"requirements": {"range_km": 100, "payload_g": 1}}), &["requirements.range_km"], json!([]), json!([q("a"), q("b"), q("c"), q("d")])),
        // 3: fine
        proposal(json!({"requirements": {"range_km": 100, "payload_g": 1}}), &["requirements.range_km"], json!([]), json!([])),
    ]);
    let e = elicit::elicit(&client, tools::<Toy>().as_ref(), "100 km", &Value::Null, &cfg(HumanInput::Never), &mut NoHuman).await.unwrap();
    assert_eq!(t.calls(), 3);
    let reqs = t.requests.lock().unwrap();
    let r2 = reqs[1].body["messages"][2]["content"][0].clone();
    assert_eq!(r2["is_error"], json!(true));
    assert!(r2["content"].as_str().unwrap().contains("spec rejected"), "{r2}");
    let r3 = reqs[2].body["messages"][4]["content"][0].clone();
    assert!(r3["content"].as_str().unwrap().contains("at most 3"), "{r3}");
    // payload_g was written without a source: it is still listed, as a default
    assert!(e.assumptions.iter().any(|a| a.path == "requirements.payload_g" && a.source == Origin::SchemaDefault));
    assert!(e.probe.feasible);
}

#[tokio::test]
async fn an_analyst_that_never_proposes_is_an_error_not_a_spec() {
    let (client, _t, _rx) = client(vec![reply("m", vec![text("I would need more information.")], 10, 10)]);
    let r = elicit::elicit(&client, tools::<Toy>().as_ref(), "a plane", &Value::Null, &cfg(HumanInput::Never), &mut NoHuman).await;
    assert!(r.is_err());
}

#[test]
fn probe_alone_walks_the_ladder_until_one_relaxation_passes() {
    // 300 km on 6S with a budget that also fails: range is binding (price
    // follows range), and the probe needs no model to say so.
    let spec = json!({"requirements": {"range_km": 300, "payload_g": 1, "budget_usd": 100, "cells_s": 6}});
    let p = elicit::probe(tools::<Toy>().as_ref(), &spec, 12);
    assert!(!p.feasible);
    assert!(p.failing.contains(&"range".to_string()) && p.failing.contains(&"budget".to_string()));
    // 0.7x range (210) still fails the budget; the ladder walks on in the
    // direction that helped (0.5x, 0.33x) until 100 km fits both
    let b = p.binding.as_ref().expect("binding");
    assert_eq!((b.path.as_str(), b.to), ("requirements.range_km", 100.0));
    assert!(p.designs_run <= 12, "{}", p.designs_run);
    assert!(p.summary().starts_with("probe: infeasible as asked; failing range, budget; binding requirement requirements.range_km = 300 (100 passes)"), "{}", p.summary());
}
