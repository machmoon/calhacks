//! The electronics domain (lane E of docs/electronics-plan.md): Wright's
//! "AI electronics engineer". An agent writes a [`Spec`] (a template with
//! requirements, or a full circuit in the Python baseline's CIRCUIT-SPEC JSON); `design`
//! runs the deterministic pipeline (validate -> footprints -> place ->
//! route); `check` judges it with the closed [`checks::Check`] set; `build`
//! writes the KiCad project, runs kicad-cli, and exports STEP for CAD.
//! Replaces baseline `agents/pipeline.py` + `board.py build_board/route_board`
//! + `aircraft/electronics.py`.

pub mod accept;
pub mod catalog;
pub mod checks;
pub mod copper;
pub mod pipeline;
pub mod placement;
pub mod templates;

pub use checks::Check;

use serde::{Deserialize, Serialize};
use std::path::Path;
use wright_core::{Artifacts, Bom, Domain, Finished, Headline};
use wright_eda::{Board, Circuit, Netlist, RouteReport};

/// What a person or agent asks for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    /// The request in words ("servo power board for the VTOL: 6 V 5 A from 4S").
    pub request: String,
    /// A known circuit family sized from requirements (preferred).
    #[serde(default)]
    pub template: Option<templates::Template>,
    /// Or a full circuit the agent proposed (baseline CIRCUIT-SPEC JSON).
    #[serde(default)]
    pub circuit: Option<Circuit>,
    /// Largest board the vehicle bay takes, mm (w, h).
    #[serde(default)]
    pub max_board_mm: Option<(f64, f64)>,
    #[serde(default)]
    pub budget_usd: Option<f64>,
    /// The layout agent's functional groups (KiCadRoutingTools groups.json
    /// schema 1, `wright_place::intent::GroupsDoc`); `None` = the placer
    /// derives decap groups itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<wright_place::intent::GroupsDoc>,
    /// Fixed poses, proximity claims, templates (`wright_place::intent::Intent`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<wright_place::intent::Intent>,
    /// Place once and route the cheapest placement, skipping the seed judge
    /// (the A/B baseline). Default: judge the candidates by quick-routing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub single_seed: bool,
}

/// Fully resolved: every part placed, every net routed or reported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Design {
    pub circuit: Circuit,
    pub netlist: Netlist,
    pub board: Board,
    pub route: RouteReport,
    pub place: wright_place::PlaceReport,
    /// Per-net design current, A (from the template; agent circuits: 0.5 A
    /// signal default, ASSUMPTION stated in the check detail).
    #[serde(default)]
    pub net_current_a: Vec<f64>,
    /// The seed judge's ranking (KRT judge_seeds.py), when it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge: Option<pipeline::JudgeReport>,
    /// The BOM as priced by the sourcing worker (Creao Build Desk, every
    /// quote validated by `wright_creao::apply_quotes`); `budget` judges it
    /// only when its lines are this design's catalog lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sourced: Option<wright_core::Bom>,
}

pub struct Electronics;

impl Domain for Electronics {
    const NAME: &'static str = "electronics";
    type Spec = Spec;
    type Design = Design;
    type Check = Check;

    fn spec_schema() -> serde_json::Value {
        templates::spec_schema()
    }

    fn design(spec: &Spec) -> anyhow::Result<Design> {
        pipeline::design(spec)
    }

    fn validate_spec(spec: &Spec) -> anyhow::Result<()> {
        match (&spec.template, &spec.circuit) {
            (Some(t), _) => templates::validate(t)?,
            (None, Some(c)) => {
                c.validate()?;
            }
            (None, None) => anyhow::bail!("spec names neither a template nor a circuit"),
        }
        if let Some((w, h)) = spec.max_board_mm {
            anyhow::ensure!(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite(), "max_board_mm must be two positive numbers");
        }
        Ok(())
    }

    /// The divider is sized from the requested Vout: under a changed
    /// requirement every design made for the old one fails it.
    fn consistency_checks() -> &'static [&'static str] {
        &["regulator_setpoint"]
    }

    fn check(spec: &Spec, design: &Design) -> Finished {
        checks::check(spec, design)
    }

    fn bom(design: &Design) -> Bom {
        catalog::bom(&design.netlist, &design.board)
    }

    fn headlines(spec: &Spec, d: &Design) -> Vec<Headline> {
        use wright_core::Bound;
        let h = |label: &str, value: f64, unit: &str, limit: Option<f64>, bound: Bound, check: &str| Headline { label: label.into(), value, unit: unit.into(), limit, bound, check: Some(check.into()) };
        let (w, ht) = (d.board.outline.width().as_mm(), d.board.outline.height().as_mm());
        let mut out = vec![h("Board", w * ht, "mm2", None, Bound::Info, "board_fits_bay")];
        if let Some((bw, bh)) = spec.max_board_mm {
            out.push(h("Board long side", w.max(ht), "mm", Some(bw.max(bh)), Bound::AtMost, "board_fits_bay"));
        }
        out.push(h("Routed", d.route.completion() * 100.0, "%", Some(100.0), Bound::AtLeast, "routed"));
        let drc = checks::kicad_verdicts(d).ok().and_then(|v| v.into_iter().find(|x| x.check == "drc" && x.ran)).map(|x| (x.errors + x.unconnected) as f64).unwrap_or(f64::NAN);
        out.push(h("DRC errors", drc, "", Some(0.0), Bound::AtMost, "drc"));
        let bom = catalog::bom(&d.netlist, &d.board);
        let cost = if bom.unpriced().is_empty() { bom.total().as_usd() } else { f64::NAN };
        out.push(h("Cost", cost, "USD", spec.budget_usd, if spec.budget_usd.is_some() { Bound::AtMost } else { Bound::Info }, "budget"));
        out
    }

    fn build(design: &Design, out: &Path) -> anyhow::Result<Artifacts> {
        pipeline::build(design, out)
    }

    fn objective(design: &Design) -> (f64, &'static str) {
        // Smaller boards are better among passing designs.
        let a = design.board.outline.width().as_mm() * design.board.outline.height().as_mm();
        (-a, "board area mm2 (negated)")
    }
}
