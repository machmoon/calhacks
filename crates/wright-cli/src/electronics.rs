//! `wright run electronics "REQUEST" [-o DIR]`: the PCB path end to end in
//! one command, with Claude where judgement is needed and deterministic code
//! everywhere else (the KiCadRoutingTools `plan-pcb-placement` split,
//! `wright_electronics::placement`):
//!
//! 1. **spec**: one Claude call (`propose_spec`) picks a circuit template and
//!    its requirements, or writes a full circuit; `validate_spec` judges it,
//!    one repair turn on refusal.
//! 2. **groups**: one Claude call (`write_groups`) on the board context
//!    (KRT `board_context.py`) writes groups.json schema 1; lane C's
//!    `intent::resolve` judges it, one repair turn.
//! 3. **design**: schematic netlist, 12 placement seeds, the 6 cheapest
//!    quick-routed and ranked (KRT `judge_seeds.py`), the winner routed in
//!    full.
//! 4. **check**: the 15 electronics checks, kicad-cli ERC / DRC / parity on
//!    the files themselves.
//! 5. **build**: .kicad_pcb/.kicad_sch/.kicad_pro, BOM, board STEP with 3D
//!    models (`cad/board.step`), SVGs of the schematic and both copper
//!    layers, a GLB of the board (kicad-cli `pcb export glb`).
//! 6. **verify**: the claims re-measured (`placement::verify_claims`).
//!
//! Every stage is an event in `DIR/events.jsonl` (wright_core::Event, the
//! format the desktop replays) and `DIR/run.json` lists stages, checks and
//! artifacts. A stage that fails says so; nothing is shown as passing that
//! did not run. `--team` (in main.rs) runs the general agent team instead.

use anyhow::Context;
use serde_json::{json, Value};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;
use wright_core::{Domain, Event};
use wright_electronics::{placement, Electronics, Spec};

struct Rec {
    run: String,
    f: std::fs::File,
    out: PathBuf,
    stages: Vec<Value>,
    t0: Instant,
    /// The eval table's numbers (evals/electronics/run.sh reads them).
    metrics: Value,
}

impl Rec {
    fn emit(&mut self, e: Event) {
        if let Ok(l) = serde_json::to_string(&e) {
            let _ = writeln!(self.f, "{l}");
            let _ = self.f.flush();
        }
    }
    fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.out).unwrap_or(p).to_string_lossy().into_owned()
    }
    fn stage(&mut self, name: &str, input: Value) -> Instant {
        self.emit(Event::ToolCall { run: self.run.clone(), agent: "pipeline".into(), tool: name.into(), input });
        eprintln!("[{:5.1} s] {name} ...", self.t0.elapsed().as_secs_f64());
        Instant::now()
    }
    fn done(&mut self, name: &str, t: Instant, ok: bool, summary: String) {
        let s = t.elapsed().as_secs_f64();
        eprintln!("[{:5.1} s] {name}: {} ({s:.1} s) {summary}", self.t0.elapsed().as_secs_f64(), if ok { "ok" } else { "FAILED" });
        self.emit(Event::ToolResult { run: self.run.clone(), agent: "pipeline".into(), tool: name.into(), ok, summary: summary.clone() });
        self.stages.push(json!({"name": name, "ok": ok, "seconds": (s * 100.0).round() / 100.0, "summary": summary}));
    }
    fn artifact(&mut self, label: &str, p: &Path) {
        let path = self.rel(p);
        self.emit(Event::Artifact { run: self.run.clone(), label: label.into(), path });
    }
}

#[derive(Default)]
struct Usage {
    calls: u32,
    input: u64,
    output: u64,
}

/// One forced tool call; returns the tool input.
async fn ask(client: &wright_agents::Client, model: &wright_agents::agent::ModelCfg, system: &str, messages: Vec<Value>, tool: Value, u: &mut Usage) -> anyhow::Result<Value> {
    let name = tool["name"].as_str().unwrap_or("").to_string();
    let body = json!({
        "model": model.id, "max_tokens": model.max_tokens.max(8000),
        "system": system,
        "messages": messages,
        "tools": [tool],
        "tool_choice": {"type": "tool", "name": name},
        "output_config": {"effort": std::env::var("WRIGHT_ELECTRONICS_EFFORT").unwrap_or_else(|_| "medium".into())},
    });
    let resp = client.create(body).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    u.calls += 1;
    // the CLI transport's prompt is mostly cached: count every input kind
    u.input += ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"].iter().map(|k| resp["usage"][*k].as_u64().unwrap_or(0)).sum::<u64>();
    u.output += resp["usage"]["output_tokens"].as_u64().unwrap_or(0);
    resp["content"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|b| b["type"] == "tool_use" && b["name"] == name.as_str())
        .map(|b| b["input"].clone())
        .with_context(|| format!("the model did not call {name}: {}", resp["content"]))
}

/// Ask, validate, and give the model one repair turn with the error.
async fn ask_valid<T>(client: &wright_agents::Client, model: &wright_agents::agent::ModelCfg, system: &str, user: String, tool: Value, u: &mut Usage, check: impl Fn(&Value) -> Result<T, String>) -> anyhow::Result<(T, Value)> {
    let mut messages = vec![json!({"role": "user", "content": user})];
    let mut last = String::new();
    for attempt in 0..2 {
        let input = ask(client, model, system, messages.clone(), tool.clone(), u).await?;
        match check(&input) {
            Ok(t) => return Ok((t, input)),
            Err(e) => {
                eprintln!("  attempt {}: refused: {e}", attempt + 1);
                messages.push(json!({"role": "assistant", "content": format!("I called {} with {}", tool["name"], input)}));
                messages.push(json!({"role": "user", "content": format!("That was refused: {e}\nFix every problem and call {} again.", tool["name"])}));
                last = e;
            }
        }
    }
    anyhow::bail!("{} refused twice: {last}", tool["name"])
}

fn spec_system() -> String {
    format!(
        "You are the circuit engineer of an AI electronics team. Turn the person's request into a Wright electronics spec \
by calling propose_spec. Prefer a template when one fits: servo_power (TI TPS5450 buck, 5.5-36 V in, up to 5 A; \
vin_min_v/vin_max_v from the pack: 4S LiPo is 14.0-16.8 V, 3S 10.5-12.6 V, 6S 21.0-25.2 V; servos = number of servo \
headers, 1-8) or ldo (AMS1117 fixed 1.5/1.8/2.5/2.85/3.3/5.0 V). Otherwise write a full circuit in baseline CIRCUIT-SPEC \
JSON. Set max_board_mm and budget_usd only when the request states them; never invent a requirement. Copy the \
request text into `request`.\n\nSpec JSON Schema:\n{}",
        serde_json::to_string_pretty(&Electronics::spec_schema()).unwrap_or_default()
    )
}

fn spec_tool() -> Value {
    json!({"name": "propose_spec", "description": "The electronics spec for this request.", "input_schema": Electronics::spec_schema()})
}

pub fn run(request: &str, args: &[String]) -> anyhow::Result<()> {
    let flag = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let run_id = format!("electronics-{secs}");
    let out = PathBuf::from(flag("-o").unwrap_or_else(|| format!("runs/{run_id}")));
    std::fs::create_dir_all(&out)?;
    // absolute: kicad-cli runs with its cwd in the run dir, so a relative
    // `-o out/x` would double up inside it
    let out = std::fs::canonicalize(&out)?;
    let demo = args.iter().any(|a| a == "--demo") || wright_agents::roles::demo_from_env();
    let mut models = wright_agents::Models::from_env(demo).map_err(|e| anyhow::anyhow!(e))?;
    for (i, a) in args.iter().enumerate() {
        if a == "--model" {
            let v = args.get(i + 1).context("--model needs role=model[:effort]")?;
            let (role, m) = v.split_once('=').context("--model role=model")?;
            models.set(role, m).map_err(|e| anyhow::anyhow!("--model {v}: {e}"))?;
        }
    }
    let model = models.orchestrator.clone();
    let f = std::fs::OpenOptions::new().create(true).append(true).open(out.join("events.jsonl"))?;
    let mut rec = Rec { run: run_id.clone(), f, out: out.clone(), stages: vec![], t0: Instant::now(), metrics: Value::Null };
    rec.emit(Event::RunStarted { run: run_id.clone(), domain: "electronics".into(), request: request.into() });

    let rt = tokio::runtime::Runtime::new()?;
    let (tx, _rx) = wright_agents::channel();
    let transport = wright_agents::transport_from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    let client = wright_agents::Client::new(transport, wright_agents::Budget::from_env(), tx, run_id.clone());
    let mut u = Usage::default();

    // 1. spec
    let t = rec.stage("spec", json!({"model": model.id, "request": request}));
    // `--spec FILE`: a fixed spec (the eval runs one per fixture), no model call
    let fixed = flag("--spec").map(|f| -> anyhow::Result<(Spec, Value)> {
        let mut s: Spec = serde_json::from_str(&std::fs::read_to_string(&f).with_context(|| format!("--spec {f}"))?).with_context(|| format!("--spec {f}"))?;
        if s.request.trim().is_empty() {
            s.request = request.to_string();
        }
        Electronics::validate_spec(&s)?;
        Ok((s, Value::Null))
    });
    let r = if let Some(r) = fixed { r } else { rt.block_on(ask_valid(&client, &model, &spec_system(), request.to_string(), spec_tool(), &mut u, |v| {
        let mut s: Spec = serde_json::from_value(v.clone()).map_err(|e| format!("not a spec: {e}"))?;
        if s.request.trim().is_empty() {
            s.request = request.to_string();
        }
        Electronics::validate_spec(&s).map_err(|e| format!("{e:#}"))?;
        Ok(s)
    })) };
    let single_seed = args.iter().any(|a| a == "--single-seed");
    let mut spec = match r {
        Ok((s, _)) => s,
        Err(e) => {
            rec.done("spec", t, false, format!("{e:#}"));
            return finish(&mut rec, &u, None, false);
        }
    };
    let what = match (&spec.template, &spec.circuit) {
        (Some(t), _) => serde_json::to_string(t)?,
        _ => "agent-written circuit".into(),
    };
    rec.emit(Event::AgentMessage { run: run_id.clone(), agent: "circuit".into(), text: format!("spec: {what}") });
    std::fs::write(out.join("spec.json"), serde_json::to_string_pretty(&spec)?)?;
    rec.done("spec", t, true, what);

    // 2. groups
    let t = rec.stage("groups", json!({"model": model.id}));
    let pre = (|| -> anyhow::Result<_> {
        let c = wright_electronics::pipeline::circuit(&spec)?;
        let n = c.validate()?;
        let fps = wright_kicad::resolve_footprints(&n, &wright_kicad::kicad_share())?;
        let cur = wright_electronics::templates::net_currents(spec.template.as_ref(), &n);
        Ok((n, fps, cur))
    })();
    let (netlist, fps, cur) = match pre {
        Ok(x) => x,
        Err(e) => {
            rec.done("groups", t, false, format!("netlist / footprints: {e:#}"));
            return finish(&mut rec, &u, None, false);
        }
    };
    let ctx = placement::board_context(&netlist, &fps, &cur);
    std::fs::write(out.join("board_context.json"), serde_json::to_string_pretty(&ctx)?)?;
    let user = format!("Request: {request}\n\nBoard context (KiCadRoutingTools board_context.py fields):\n{}", serde_json::to_string(&ctx)?);
    // `--no-groups`: the placer derives decap groups itself (eval column b)
    let asked = if args.iter().any(|a| a == "--no-groups") { Err(anyhow::anyhow!("--no-groups: no agent groups")) } else { rt.block_on(ask_valid(&client, &model, &placement::groups_system(), user, placement::groups_tool(), &mut u, |v| placement::parse_groups(&netlist, v))) };
    match asked {
        Ok((g, _)) => {
            let line = g.groups.iter().map(|x| format!("{} [{}]", x.name, x.refs.join(" "))).collect::<Vec<_>>().join("; ");
            std::fs::write(out.join("groups.json"), serde_json::to_string_pretty(&g)?)?;
            rec.emit(Event::AgentMessage { run: run_id.clone(), agent: "layout".into(), text: format!("groups: {line}") });
            spec.groups = Some(g);
            rec.done("groups", t, true, line);
        }
        // the placer derives decap groups itself; say that the agent's step failed
        Err(e) => rec.done("groups", t, false, format!("{e:#}; placing with derived groups")),
    }
    // `--single-seed`: one annealing run, no seed judging (eval column b)
    spec.single_seed |= single_seed;
    std::fs::write(out.join("spec.json"), serde_json::to_string_pretty(&spec)?)?;

    // 3. design
    let t = rec.stage("place_route", if spec.single_seed { json!({"seeds": 1}) } else { json!({"seeds": 12, "judged": 6}) });
    let mut d = match Electronics::design(&spec) {
        Ok(d) => d,
        Err(e) => {
            rec.done("place_route", t, false, format!("{e:#}"));
            return finish(&mut rec, &u, None, false);
        }
    };
    std::fs::write(out.join("design.json"), serde_json::to_string_pretty(&d)?)?;
    let j = d.judge.as_ref().map(|j| format!("12 seeds, {} cheapest quick-routed, kept seed {}; ", j.candidates, j.kept_seed)).unwrap_or_default();
    let routed = d.route.routed.len() + d.route.poured.iter().filter(|n| !d.route.routed.contains(n)).count();
    rec.done("place_route", t, d.route.unrouted.is_empty(), format!("{j}{routed}/{} nets, {:.1} x {:.1} mm, {} vias", d.route.attempted.len(), d.board.outline.width().as_mm(), d.board.outline.height().as_mm(), d.board.vias.len()));

    // 4. check (kicad-cli inside)
    let t = rec.stage("check", json!({"checks": 15}));
    let mut fin = Electronics::check(&spec, &d);
    std::fs::write(out.join("checks.json"), serde_json::to_string_pretty(&fin)?)?;
    let failed = fin.failures().count();
    rec.emit(Event::Checks { run: run_id.clone(), passed: fin.outcomes.len() - failed, failed, outcomes: fin.outcomes.clone() });
    rec.emit(Event::Headlines { run: run_id.clone(), items: Electronics::headlines(&spec, &d) });
    rec.metrics = metrics(&d, &fin);
    let kc: Vec<String> = ["erc", "drc", "parity"].iter().filter_map(|c| fin.get(c)).map(|o| format!("{} {}", o.name, if o.passed() { "0" } else { "FAIL" })).collect();
    rec.done("check", t, true, format!("{} of {} pass; {}", fin.outcomes.len() - failed, fin.outcomes.len(), kc.join(", ")));

    // 5. build
    let t = rec.stage("build", json!({}));
    let arts = match Electronics::build(&d, &out) {
        Ok(a) => a,
        Err(e) => {
            rec.done("build", t, false, format!("{e:#}"));
            return finish(&mut rec, &u, Some(&fin), false);
        }
    };
    for (l, p) in &arts.files {
        rec.artifact(l, p);
    }
    let step = out.join("cad/board.step");
    rec.done("build", t, step.exists(), if step.exists() { "KiCad project, BOM, cad/board.step".into() } else { "board STEP not written (see cad/manifest.json)".into() });

    // 5b. renders
    let t = rec.stage("render", json!({}));
    let pcb = out.join("board.kicad_pcb");
    let sch = out.join("board.kicad_sch");
    let rdir = out.join("render");
    std::fs::create_dir_all(&rdir)?;
    let mut bad = vec![];
    // zones filled by kicad-cli (DRC --refill-zones --save-board) so the
    // copper views and the GLB show the pour
    let filled = match wright_kicad::fill_zones(&pcb, &rdir.join("board-filled.kicad_pcb")) {
        Ok(p) => p,
        Err(e) => {
            bad.push(format!("fill zones: {e:#}; views drawn without the pour"));
            pcb.clone()
        }
    };
    match wright_kicad::export_sch_svg(&sch, &rdir.join("schematic.svg")) {
        Ok(p) => rec.emit(Event::Render { run: run_id.clone(), view: "schematic".into(), path: rec.rel(&p) }),
        Err(e) => bad.push(format!("{e:#}")),
    }
    for (layer, name) in [("F.Cu", "copper_front"), ("B.Cu", "copper_back")] {
        match wright_kicad::export_pcb_svg_layers(&filled, &rdir.join(format!("{name}.svg")), &[layer, "Edge.Cuts"]) {
            Ok(p) => rec.emit(Event::Render { run: run_id.clone(), view: name.into(), path: rec.rel(&p) }),
            Err(e) => bad.push(format!("{e:#}")),
        }
    }
    match wright_kicad::export_glb(&filled, &out.join("cad/board.glb")) {
        Ok(p) => rec.artifact("board GLB (kicad-cli, 3D models)", &p),
        Err(e) => bad.push(format!("{e:#}")),
    }
    let missing = wright_electronics::pipeline::missing_models(&d.netlist, &d.board);
    let note = if missing.is_empty() { String::new() } else { format!("; no 3D model for {}", missing.join(", ")) };
    rec.done("render", t, bad.is_empty(), if bad.is_empty() { format!("render/schematic.svg, render/copper_front.svg, render/copper_back.svg, cad/board.glb{note}") } else { bad.join("; ") });

    // 5c. sourcing: the Creao Build Desk prices the BOM from live listings
    // (wright-creao loads its own key; nothing here reads or prints it),
    // every quote validated by apply_quotes, then `budget` is judged again
    // on the priced lines. Without an agent configured the stage says so and
    // the budget check stays failing.
    if !args.iter().any(|a| a == "--no-creao") {
        let t = rec.stage("source", json!({"bom_lines": wright_electronics::catalog::bom(&d.netlist, &d.board).lines.len()}));
        match rt.block_on(source(&run_id, request, &d, &fin, &out)) {
            Ok(None) => rec.done("source", t, false, "skipped: no Creao Build Desk agent configured (CREAO_APP_ID); BOM stays unpriced".into()),
            Ok(Some((bom, events, line))) => {
                for e in events {
                    rec.emit(e);
                }
                d.sourced = Some(bom);
                fin = Electronics::check(&spec, &d);
                std::fs::write(out.join("design.json"), serde_json::to_string_pretty(&d)?)?;
                std::fs::write(out.join("checks.json"), serde_json::to_string_pretty(&fin)?)?;
                let failed = fin.failures().count();
                rec.emit(Event::Checks { run: run_id.clone(), passed: fin.outcomes.len() - failed, failed, outcomes: fin.outcomes.clone() });
                let ok = fin.get("budget").is_some_and(|o| o.passed());
                rec.done("source", t, ok, line);
            }
            Err(e) => rec.done("source", t, false, format!("Creao Build Desk failed: {e:#}; BOM stays unpriced")),
        }
    }

    // 6. verifier
    let t = rec.stage("verify", json!({}));
    let drc = wright_electronics::checks::kicad_verdicts(&d).ok().and_then(|v| v.into_iter().find(|x| x.check == "drc"));
    let claims = placement::verify_claims(spec.groups.as_ref(), &d, drc.as_ref());
    std::fs::write(out.join("verifier.json"), serde_json::to_string_pretty(&claims)?)?;
    let held = claims.iter().filter(|c| c.holds).count();
    for c in claims.iter().filter(|c| !c.holds) {
        rec.emit(Event::AgentMessage { run: run_id.clone(), agent: "verifier".into(), text: format!("does not hold: {} ({})", c.claim, c.measured) });
    }
    rec.done("verify", t, held == claims.len(), format!("{held}/{} claims hold", claims.len()));

    // state.json in the harness's shape so `wright report RUN_DIR` reads it
    let state = json!({"domain": "electronics", "request": request, "spec": spec, "design": d, "report": fin});
    std::fs::write(out.join("state.json"), serde_json::to_string(&state)?)?;
    let accepted = wright_electronics::accept::accept(&d.netlist, &d.route, &wright_electronics::checks::kicad_verdicts(&d).unwrap_or_default());
    std::fs::write(out.join("accept.json"), serde_json::to_string_pretty(&json!({"board_accepted": accepted.accepted(), "nets_ok": accepted.nets_ok, "covered": accepted.covered, "pins_ok": accepted.pins_ok, "tools_ok": accepted.tools_ok, "why": accepted.why, "rule": "lean/Wright/Board.lean accept_iff"}))?)?;
    let artifacts: Vec<Value> = arts.files.iter().map(|(l, p)| json!({"label": l, "path": rec.rel(p)})).collect();
    let mut man = json!({"artifacts": artifacts, "board_accepted": accepted.accepted(), "models_missing": missing});
    man["renders"] = json!(["render/schematic.svg", "render/copper_front.svg", "render/copper_back.svg", "cad/board.glb"]);
    std::fs::write(out.join("artifacts.json"), serde_json::to_string_pretty(&man)?)?;
    let ok = fin.passed();
    finish(&mut rec, &u, Some(&fin), ok)
}

fn finish(rec: &mut Rec, u: &Usage, fin: Option<&wright_core::Finished>, passed: bool) -> anyhow::Result<()> {
    rec.emit(Event::Spend { run: rec.run.clone(), input_tokens: u.input, output_tokens: u.output, usd: 0.0, budget_usd: 0.0 });
    rec.emit(Event::RunFinished { run: rec.run.clone(), passed, objective: 0.0, objective_label: "board area mm2 (negated)".into() });
    let checks: Vec<Value> = fin.map(|f| f.outcomes.iter().map(|o| json!({"name": o.name, "passed": o.passed(), "margin": o.margin(), "detail": if o.detail.is_empty() { match &o.evidence { wright_core::Evidence::Missing { why } => format!("missing: {why}"), _ => String::new() } } else { o.detail.clone() }})).collect()).unwrap_or_default();
    let man = json!({
        "run": rec.run, "domain": "electronics", "passed": passed,
        "seconds": (rec.t0.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        "model_calls": u.calls, "input_tokens": u.input, "output_tokens": u.output,
        "stages": rec.stages, "checks": checks, "metrics": rec.metrics,
        "files": {"events": "events.jsonl", "spec": "spec.json", "groups": "groups.json", "design": "design.json", "checks": "checks.json", "verifier": "verifier.json", "accept": "accept.json", "artifacts": "artifacts.json", "state": "state.json"},
    });
    std::fs::write(rec.out.join("run.json"), serde_json::to_string_pretty(&man)?)?;
    if let Some(f) = fin {
        println!("{f}");
    }
    println!("{}: {} in {:.1} s, {} model calls ({} in / {} out tokens); files in {}", rec.run, if passed { "all checks pass" } else { "not all checks pass" }, rec.t0.elapsed().as_secs_f64(), u.calls, u.input, u.output, rec.out.display());
    Ok(())
}

/// One Build Desk run on the catalog BOM: `(priced BOM, events, summary)`,
/// or `None` when no agent is configured (main.rs `build_desk` does the
/// same for the agent harness).
async fn source(run_id: &str, request: &str, d: &wright_electronics::Design, fin: &wright_core::Finished, out: &Path) -> anyhow::Result<Option<(wright_core::Bom, Vec<Event>, String)>> {
    let dotenv = Path::new(".env");
    let env_text = std::fs::read_to_string(dotenv).unwrap_or_default();
    if wright_creao::config::dotenv_get(&env_text, "CREAO_APP_ID").is_none() && std::env::var("CREAO_APP_ID").is_err() {
        return Ok(None);
    }
    drop(env_text);
    let cfg = wright_creao::Config::load(dotenv)?;
    let client = wright_creao::Client::new(cfg, wright_creao::transport::Reqwest::new()?);
    let bom = wright_electronics::catalog::bom(&d.netlist, &d.board);
    let files = ["board.kicad_pcb", "board.kicad_sch", "bom.csv", "cad/board.step"].iter().map(|s| s.to_string()).collect();
    let packet = wright_creao::DesignPacket {
        vehicle: request.to_string(),
        domain: "electronics".into(),
        summary: format!("{request}. A {:.1} x {:.1} mm 2-layer PCB; price each BOM line (KiCad footprint and value given) from a live listing.", d.board.outline.width().as_mm(), d.board.outline.height().as_mm()),
        bom,
        checks: fin.outcomes.iter().map(Into::into).collect(),
        build_steps: vec![],
        files,
    };
    let dir = out.join("build_desk");
    let res = wright_creao::run_desk(&client, run_id, &packet, &dir, Default::default()).await?;
    let s = &res.sourcing;
    let line = format!("{} lines priced from live listings, {} rejected, {} still unpriced; ${:.2}", s.filled.len(), s.rejected.len(), s.still_unpriced.len(), s.total_after_usd);
    Ok(Some((s.bom.clone(), res.events, line)))
}

/// Board numbers for the A/B table: area, decap loops, routed share,
/// kicad-cli counts, and each power net's narrowest track against the
/// IPC-2221 width its current needs.
fn metrics(d: &wright_electronics::Design, fin: &wright_core::Finished) -> Value {
    let (w, h) = (d.board.outline.width().as_mm(), d.board.outline.height().as_mm());
    let loops: Vec<f64> = wright_place::decap::loops_nm(&d.board, &wright_place::decoupling_caps(&d.netlist)).iter().map(|x| x / 1e6).collect();
    let attempted = d.route.attempted.len();
    let done = d.route.attempted.iter().filter(|n| (d.route.routed.contains(n) || d.route.poured.contains(n)) && !d.route.unrouted.iter().any(|u| u.net == **n)).count();
    let v = wright_electronics::checks::kicad_verdicts(d).unwrap_or_default();
    let count = |c: &str| v.iter().find(|x| x.check == c && x.ran).map(|x| json!(x.errors + if c == "drc" { x.unconnected } else { 0 })).unwrap_or(Value::Null);
    let oz = d.board.rules.copper_oz;
    let power: Vec<Value> = d
        .net_current_a
        .iter()
        .enumerate()
        .filter(|(_, a)| **a >= 0.5)
        .map(|(i, a)| {
            let minw = d.board.tracks.iter().filter(|t| t.net == i).map(|t| t.width.as_mm()).fold(f64::INFINITY, f64::min);
            json!({"net": d.netlist.nets[i].name, "amps": a, "min_track_mm": if minw.is_finite() { json!(minw) } else { Value::Null }, "need_mm": wright_electronics::copper::width_for_mm(*a, oz), "poured": d.route.poured.contains(&i)})
        })
        .collect();
    json!({
        "outline_mm": [w, h], "area_mm2": w * h,
        "decap_loops_mm": loops, "decap_total_mm": loops.iter().sum::<f64>(), "decap_worst_mm": loops.iter().copied().fold(0.0, f64::max),
        "routed": done, "attempted": attempted, "routed_pct": if attempted > 0 { 100.0 * done as f64 / attempted as f64 } else { 0.0 },
        "vias": d.board.vias.len(), "copper_mm": d.board.tracks.iter().map(|t| t.a.dist(t.b)).sum::<f64>() / 1e6,
        "erc_errors": count("erc"), "drc_errors": count("drc"), "parity": count("parity"),
        "power": power, "checks_failed": fin.failures().map(|o| o.name.clone()).collect::<Vec<_>>(),
        "judge": d.judge.as_ref().map(|j| json!({"candidates": j.candidates, "kept_seed": j.kept_seed, "seconds": j.seconds})),
    })
}
