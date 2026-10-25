//! The Build Desk: Wright's hand-off from "checked design" to "a person can
//! order the parts and build it tonight".
//!
//! Wright's own agents and solvers stop at a design whose checks pass from
//! the catalog. Two jobs remain that need the live web, not a solver:
//! sourcing every BOM line at a current price, and writing the builder's
//! packet. A CREAO agent does both in its sandbox (web search, browser use,
//! HTML/PDF generation) and returns quotes plus a build page.
//!
//! Its quotes are claims, so Wright treats them like any catalog row:
//! [`apply_quotes`] validates each one deterministically (known BOM id, URL,
//! date, finite positive price) before it may price a line, and a line the
//! desk could not source stays unpriced -- the budget check still fails on
//! it. Removing the desk leaves uncatalogued parts unpriced and the design
//! unbuildable; a desk that invents a price is caught here, not trusted.

use crate::client::{Client, Poll, Run};
use crate::error::CreaoError;
use crate::transport::Transport;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use wright_core::units::Money;
use wright_core::{Bom, Event, Outcome};

/// The agent's name in agent.creao.ai and in Wright's event stream.
pub const AGENT_NAME: &str = "creao:build-desk";

/// A quote from the desk that differs from Wright's catalog price by more
/// than this fraction is reported as drift (the catalog price is kept).
/// Unsourced assumption: 25% is a judgement call for hobby-part price churn.
pub const DRIFT_FRACTION: f64 = 0.25;

/// Upper bound on a single hobby-vehicle part; a quote above it is rejected
/// as a parse or unit error. Unsourced assumption.
pub const MAX_UNIT_PRICE_USD: f64 = 20_000.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckLine {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

impl From<&Outcome> for CheckLine {
    fn from(o: &Outcome) -> Self {
        CheckLine { name: o.name.clone(), passed: o.passed(), detail: o.detail.clone() }
    }
}

/// What Wright sends: the checked design, in builder terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignPacket {
    /// e.g. "VTOL quadplane, 130 mi range".
    pub vehicle: String,
    /// `air` | `rocket` | `balloon`.
    pub domain: String,
    /// One paragraph: what it is, key numbers.
    pub summary: String,
    pub bom: Bom,
    pub checks: Vec<CheckLine>,
    pub build_steps: Vec<String>,
    /// Files Wright produced (STL/3MF, G-code, params) by label.
    pub files: Vec<String>,
}

impl DesignPacket {
    /// The agent's input form: two text fields, as created in docs/creao.md.
    pub fn inputs(&self) -> serde_json::Value {
        serde_json::json!({
            "vehicle": self.vehicle,
            "design_packet": serde_json::to_string(self).unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub bom_id: String,
    pub vendor: String,
    pub url: String,
    pub unit_price_usd: f64,
    #[serde(default)]
    pub in_stock: Option<bool>,
    /// ISO date the listing was read.
    pub retrieved: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Unsourced {
    pub bom_id: String,
    pub reason: String,
}

/// The JSON block the desk must end its reply with.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeskReply {
    #[serde(default)]
    pub quotes: Vec<Quote>,
    #[serde(default)]
    pub unsourced: Vec<Unsourced>,
    #[serde(default)]
    pub build_page: Option<String>,
    #[serde(default)]
    pub notes: String,
}

/// Pull the last ```json fenced block (or, failing that, the outermost
/// `{...}`) out of the agent's reply text.
pub fn parse_reply(text: &str) -> Result<DeskReply, CreaoError> {
    let fenced = text.rsplit_once("```json").and_then(|(_, rest)| rest.split_once("```").map(|(body, _)| body));
    let candidate = match fenced {
        Some(b) => b.trim().to_string(),
        None => match (text.find('{'), text.rfind('}')) {
            (Some(a), Some(b)) if b > a => text[a..=b].to_string(),
            _ => return Err(CreaoError::Decode("desk reply has no JSON block".into())),
        },
    };
    serde_json::from_str(&candidate).map_err(|e| CreaoError::Decode(format!("desk reply JSON: {e}")))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rejected {
    pub bom_id: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drift {
    pub bom_id: String,
    pub catalog_usd: f64,
    pub quote_usd: f64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sourcing {
    pub bom: Bom,
    /// BOM ids the desk priced that the catalog could not.
    pub filled: Vec<String>,
    pub drift: Vec<Drift>,
    pub rejected: Vec<Rejected>,
    /// Lines still without a price (the budget check fails on each).
    pub still_unpriced: Vec<String>,
    pub total_before_usd: f64,
    pub total_after_usd: f64,
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && s.chars().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Validate each quote and price the lines the catalog could not.
/// Never overwrites a catalog price (it reports drift instead), never
/// accepts a quote for a part the design does not contain.
pub fn apply_quotes(bom: &Bom, reply: &DeskReply) -> Sourcing {
    let mut out = bom.clone();
    let total_before_usd = bom.total().as_usd();
    let (mut filled, mut drift, mut rejected) = (vec![], vec![], vec![]);
    let mut seen = std::collections::HashSet::new();
    for q in &reply.quotes {
        let reject = |why: &str| Rejected { bom_id: q.bom_id.clone(), why: why.into() };
        if !seen.insert(q.bom_id.clone()) {
            rejected.push(reject("duplicate quote for this line (first kept)"));
            continue;
        }
        let Some(line) = out.lines.iter_mut().find(|l| l.id == q.bom_id) else {
            rejected.push(reject("no BOM line has this id"));
            continue;
        };
        if !(q.unit_price_usd.is_finite() && q.unit_price_usd > 0.0 && q.unit_price_usd <= MAX_UNIT_PRICE_USD) {
            rejected.push(reject("price is not a finite positive USD amount in range"));
            continue;
        }
        if !q.url.starts_with("https://") || q.url.len() < 12 || q.url.chars().any(char::is_whitespace) {
            rejected.push(reject("listing URL is not an https URL"));
            continue;
        }
        if !is_iso_date(&q.retrieved) {
            rejected.push(reject("retrieved is not an ISO date"));
            continue;
        }
        if q.in_stock == Some(false) {
            rejected.push(reject("listing says out of stock"));
            continue;
        }
        match line.unit_price {
            Some(p) => {
                let cat = p.as_usd();
                if (q.unit_price_usd - cat).abs() > DRIFT_FRACTION * cat {
                    drift.push(Drift { bom_id: q.bom_id.clone(), catalog_usd: cat, quote_usd: q.unit_price_usd, url: q.url.clone() });
                }
            }
            None => {
                line.unit_price = Some(Money::usd(q.unit_price_usd));
                line.url = q.url.clone();
                let tag = format!("price: CREAO Build Desk quote, {} ({})", q.vendor, q.retrieved);
                line.note = if line.note.is_empty() { tag } else { format!("{}; {tag}", line.note) };
                filled.push(q.bom_id.clone());
            }
        }
    }
    let still_unpriced = out.unpriced().iter().map(|l| l.id.clone()).collect();
    let total_after_usd = out.total().as_usd();
    Sourcing { bom: out, filled, drift, rejected, still_unpriced, total_before_usd, total_after_usd }
}

/// Everything one desk run produced.
#[derive(Debug, Clone)]
pub struct DeskOutcome {
    pub run: Run,
    pub reply: DeskReply,
    pub sourcing: Sourcing,
    /// Artifacts saved locally (label, path).
    pub files: Vec<(String, PathBuf)>,
    pub events: Vec<Event>,
}

/// A file name safe to join under `out_dir`: the artifact id's basename,
/// restricted to `[A-Za-z0-9._-]`, never `.`/`..`.
pub fn safe_name(id: &str) -> String {
    let base = id.rsplit(['/', '\\']).next().unwrap_or("");
    let s: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    let s = s.trim_start_matches('.').to_string();
    if s.is_empty() {
        "artifact".into()
    } else {
        s
    }
}

/// Send the design to the desk, wait, validate, save its artifacts.
/// The text that carries the fenced JSON block. An agent with a structured
/// output puts a short summary object in `result.text` and its full chat reply
/// (where the block is) in `result.chatText` (seen live 2026-10-24), so the
/// chat reply is preferred when it has a fenced block.
fn reply_text(run: &Run) -> String {
    let chat = run.raw.pointer("/result/chatText").and_then(|v| v.as_str());
    match chat {
        Some(c) if c.contains("```json") => c.to_string(),
        _ => run.text.clone().or_else(|| chat.map(str::to_string)).unwrap_or_default(),
    }
}

pub async fn run_desk<T: Transport>(
    client: &Client<T>,
    wright_run: &str,
    design: &DesignPacket,
    out_dir: &Path,
    poll: Poll,
) -> Result<DeskOutcome, CreaoError> {
    let mut events = vec![Event::ToolCall {
        run: wright_run.into(),
        agent: AGENT_NAME.into(),
        tool: "creao.run".into(),
        input: serde_json::json!({ "vehicle": design.vehicle, "bom_lines": design.bom.lines.len() }),
    }];
    let started = client.start(&design.inputs()).await?;
    let run = client.wait(&started.id, poll).await?;
    let reply = parse_reply(&reply_text(&run))?;
    let sourcing = apply_quotes(&design.bom, &reply);

    std::fs::create_dir_all(out_dir).map_err(|e| CreaoError::Transport(format!("mkdir {}: {e}", out_dir.display())))?;
    let mut files = vec![];
    for a in &run.artifacts {
        let bytes = client.download(a).await?;
        let path = out_dir.join(safe_name(&a.id));
        std::fs::write(&path, bytes).map_err(|e| CreaoError::Transport(format!("write {}: {e}", path.display())))?;
        let label = if a.title.is_empty() { safe_name(&a.id) } else { a.title.clone() };
        files.push((label, path));
    }
    let csv = out_dir.join("bom.sourced.csv");
    std::fs::write(&csv, sourcing.bom.csv()).map_err(|e| CreaoError::Transport(format!("write {}: {e}", csv.display())))?;
    files.push(("Sourced BOM".into(), csv));

    events.push(Event::ToolResult {
        run: wright_run.into(),
        agent: AGENT_NAME.into(),
        tool: "creao.run".into(),
        ok: sourcing.still_unpriced.is_empty() && sourcing.rejected.is_empty(),
        summary: format!(
            "CREAO run {}: priced {} line(s), {} rejected, {} drift, {} still unpriced; BOM ${:.2} -> ${:.2}{}",
            run.id,
            sourcing.filled.len(),
            sourcing.rejected.len(),
            sourcing.drift.len(),
            sourcing.still_unpriced.len(),
            sourcing.total_before_usd,
            sourcing.total_after_usd,
            run.credits.map(|c| format!(", {c} credits")).unwrap_or_default(),
        ),
    });
    if !reply.notes.is_empty() {
        events.push(Event::AgentMessage { run: wright_run.into(), agent: AGENT_NAME.into(), text: reply.notes.clone() });
    }
    for (label, path) in &files {
        events.push(Event::Artifact { run: wright_run.into(), label: label.clone(), path: path.display().to_string() });
    }
    Ok(DeskOutcome { run, reply, sourcing, files, events })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Poll;
    use crate::config::{ApiKey, Config};
    use crate::transport::Fake;
    use serde_json::json;
    use wright_core::BomLine;

    fn line(id: &str, price: Option<f64>) -> BomLine {
        BomLine {
            category: "propulsion".into(),
            id: id.into(),
            name: id.into(),
            qty: 4.0,
            unit: "ea".into(),
            unit_price: price.map(Money::usd),
            url: String::new(),
            note: String::new(),
        }
    }

    fn quote(id: &str, p: f64) -> Quote {
        Quote { bom_id: id.into(), vendor: "GetFPV".into(), url: format!("https://example.com/{id}"), unit_price_usd: p, in_stock: Some(true), retrieved: "2026-10-24".into() }
    }

    fn bom() -> Bom {
        Bom { lines: vec![line("motor", Some(30.0)), line("esc", None), line("servo", None)] }
    }

    #[test]
    fn fills_only_unpriced_lines_and_leaves_unsourced_failing() {
        let reply = DeskReply { quotes: vec![quote("esc", 25.0)], unsourced: vec![Unsourced { bom_id: "servo".into(), reason: "no listing".into() }], ..Default::default() };
        let s = apply_quotes(&bom(), &reply);
        assert_eq!(s.filled, vec!["esc"]);
        assert_eq!(s.still_unpriced, vec!["servo"]);
        assert!((s.total_after_usd - (120.0 + 100.0)).abs() < 1e-9);
        let esc = s.bom.lines.iter().find(|l| l.id == "esc").unwrap();
        assert!(esc.note.contains("CREAO"));
        assert_eq!(esc.url, "https://example.com/esc");
    }

    #[test]
    fn catalog_price_wins_and_drift_is_reported() {
        let s = apply_quotes(&bom(), &DeskReply { quotes: vec![quote("motor", 45.0)], ..Default::default() });
        assert_eq!(s.bom.lines[0].unit_price.unwrap().as_usd(), 30.0);
        assert_eq!(s.drift.len(), 1);
        let s = apply_quotes(&bom(), &DeskReply { quotes: vec![quote("motor", 32.0)], ..Default::default() });
        assert!(s.drift.is_empty());
    }

    #[test]
    fn invented_or_malformed_quotes_are_rejected() {
        let mut bad_url = quote("esc", 10.0);
        bad_url.url = "http://x.com/a".into();
        let mut bad_date = quote("esc", 10.0);
        bad_date.retrieved = "yesterday".into();
        let mut oos = quote("servo", 10.0);
        oos.in_stock = Some(false);
        let quotes = vec![quote("ghost", 5.0), quote("esc", f64::NAN), quote("esc", -1.0), bad_url, bad_date, oos, quote("esc", 1e9)];
        let s = apply_quotes(&bom(), &DeskReply { quotes, ..Default::default() });
        assert!(s.filled.is_empty(), "{:?}", s.filled);
        assert_eq!(s.still_unpriced.len(), 2);
        assert_eq!(s.rejected.len(), 7);
    }

    #[test]
    fn duplicate_quotes_keep_the_first() {
        let s = apply_quotes(&bom(), &DeskReply { quotes: vec![quote("esc", 20.0), quote("esc", 2.0)], ..Default::default() });
        assert_eq!(s.bom.lines[1].unit_price.unwrap().as_usd(), 20.0);
        assert_eq!(s.rejected.len(), 1);
    }

    #[test]
    fn parses_last_fenced_json_or_bare_object() {
        let t = "Here you go.\n```json\n{\"quotes\":[]}\n```\nmore\n```json\n{\"quotes\":[], \"notes\":\"final\"}\n```";
        assert_eq!(parse_reply(t).unwrap().notes, "final");
        assert_eq!(parse_reply("ok {\"notes\":\"bare\"} done").unwrap().notes, "bare");
        assert!(parse_reply("no json here").is_err());
    }

    #[test]
    fn structured_output_agents_reply_in_chat_text() {
        let chat = format!("Done.\n```json\n{}\n```", json!({"quotes": [quote("esc", 18.99)], "notes": "chat"}));
        let v = json!({"id": "11111111-1111-4111-8111-111111111111", "status": "completed",
            "result": {"text": "{\"lines_sourced\":1}", "chatText": chat}});
        let run = Run::from_json(v).unwrap();
        let reply = parse_reply(&reply_text(&run)).unwrap();
        assert_eq!((reply.quotes.len(), reply.notes.as_str()), (1, "chat"));
    }

    #[test]
    fn artifact_names_cannot_escape_out_dir() {
        assert_eq!(safe_name("workspaces/u/t/files/build.html"), "build.html");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name(".."), "artifact");
        assert_eq!(safe_name("a b$.pdf"), "a_b_.pdf");
    }

    #[tokio::test(start_paused = true)]
    async fn end_to_end_with_fake_creao() {
        const APP: &str = "00000000-0000-4000-8000-000000000000";
        const RUN: &str = "11111111-1111-4111-8111-111111111111";
        let reply_text = format!("Sourced.\n```json\n{}\n```", json!({
            "quotes": [quote("esc", 25.0), quote("servo", 12.5)],
            "unsourced": [], "build_page": "build.html", "notes": "2 lines sourced"
        }));
        let art = json!({"id": "workspaces/u/t/files/build.html", "title": "Build page", "type": "text/html",
            "url": format!("https://agent.creao.ai/api/v1/runs/{RUN}/artifacts/download?artifactId=x")});
        let fake = Fake::new()
            .reply(202, json!({"id": RUN, "status": "pending"}))
            .reply(200, json!({"id": RUN, "status": "completed", "result": {"text": reply_text, "artifacts": [art]}}))
            .reply_bytes(200, b"<html>build</html>");
        let client = Client::new(Config::new(ApiKey::new("capi_test").unwrap(), APP).unwrap(), fake);
        let design = DesignPacket {
            vehicle: "VTOL quadplane".into(),
            domain: "air".into(),
            summary: "s".into(),
            bom: bom(),
            checks: vec![],
            build_steps: vec!["print the pod".into()],
            files: vec![],
        };
        let dir = std::env::temp_dir().join(format!("wright-creao-test-{}", std::process::id()));
        let out = run_desk(&client, "r1", &design, &dir, Poll::default()).await.unwrap();
        assert!(out.sourcing.still_unpriced.is_empty());
        assert_eq!(std::fs::read(dir.join("build.html")).unwrap(), b"<html>build</html>");
        assert!(dir.join("bom.sourced.csv").exists());
        assert!(out.events.iter().any(|e| matches!(e, Event::ToolResult { ok: true, .. })));
        // The design went in as the agent's two form fields.
        let sent = client.transport.requests()[0].body.clone().unwrap();
        assert_eq!(sent["inputs"]["vehicle"], "VTOL quadplane");
        assert!(sent["inputs"]["design_packet"].as_str().unwrap().contains("\"esc\""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
