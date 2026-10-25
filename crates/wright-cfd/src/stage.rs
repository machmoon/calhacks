//! The pipeline stage (`wright_core::Stage`): "cfd".
//!
//! * aircraft: AVL on wing + tail + pod at the best-range speed; checks the
//!   static margin AVL finds against wright-air's own band
//!   (`rules::STATIC_MARGIN_MIN/MAX`) and how far the analytic lift slope and
//!   induced drag sit from AVL's.
//! * rocket: SU2 on the nose (pressure drag vs Mach) and the nozzle (Cf);
//!   checks that the build-up's CD is not below the CFD-informed one at the
//!   flight's Mach numbers (an optimistic drag model flies too high) and that
//!   the nozzle delivers the Cf the engine model assumes.
//! * balloon: nothing (no lifting surface or supersonic flow to solve).

use crate::{rocket, vtol, Manifest};
use anyhow::Context;
use wright_core::stage::{Stage, StageCtx};

pub struct CfdStage;

/// Mach numbers for the rocket stage: subsonic to past the flight's peak.
pub const ROCKET_MACHS: [f64; 7] = [0.8, 0.95, 1.05, 1.15, 1.3, 1.6, 2.0];

impl Stage for CfdStage {
    fn name(&self) -> &'static str {
        "cfd"
    }
    fn applies_to(&self, domain: &str) -> bool {
        matches!(domain, "aircraft" | "rocket")
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn run(&self, ctx: &StageCtx) -> anyhow::Result<Manifest> {
        match ctx.domain.as_str() {
            "aircraft" => aircraft(ctx),
            "rocket" => rocket_stage(ctx),
            d => anyhow::bail!("cfd has nothing to run for domain {d:?}"),
        }
    }
}

fn threads() -> usize {
    std::env::var("WRIGHT_CFD_THREADS").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

fn aircraft(ctx: &StageCtx) -> anyhow::Result<Manifest> {
    let v = ctx.design.get("design").unwrap_or(&ctx.design);
    let design: wright_air::spec::Design = serde_json::from_value(v.clone()).context("reading the aircraft design")?;
    let ev = wright_air::checks::evaluate(&design, wright_air::powertrain::Factors::NOMINAL).map_err(|e| anyhow::anyhow!(e))?;
    let speed = ev.perf.best_range.as_ref().map(|c| c.speed.value()).unwrap_or(17.0);
    let mut man = Manifest::new("cfd", format!("CFD: {}", design.name));
    let c = vtol::run(&design, ev.perf.range_km, speed, &ctx.dir, &mut man)?;
    use wright_air::rules::{STATIC_MARGIN_MAX, STATIC_MARGIN_MIN};
    let sm = c.static_margin_avl;
    man.check(
        "cfd_static_margin",
        (STATIC_MARGIN_MIN..=STATIC_MARGIN_MAX).contains(&sm),
        &format!("AVL neutral point {:.0} mm, CG {:.0} mm: static margin {:.1} % MAC (band {:.0}-{:.0} %; analytic model says {:.1} %)", c.x_np_avl_m * 1e3, c.x_cg_m * 1e3, 100.0 * sm, 100.0 * STATIC_MARGIN_MIN, 100.0 * STATIC_MARGIN_MAX, 100.0 * c.static_margin_analytic),
    );
    let dcla = (c.cl_alpha_wing_analytic - c.cl_alpha_wing_avl) / c.cl_alpha_wing_avl;
    man.check("cfd_cl_alpha", dcla.abs() <= 0.10, &format!("wing CL_alpha analytic {:.2} vs AVL {:.2} /rad ({:+.1} %, limit 10 %)", c.cl_alpha_wing_analytic, c.cl_alpha_wing_avl, 100.0 * dcla));
    // the analytic induced drag must not be optimistic against AVL's
    man.check(
        "cfd_induced_drag",
        c.induced_drag_analytic_n >= 0.95 * c.induced_drag_avl_n,
        &format!("cruise induced drag analytic {:.2} N vs AVL {:.2} N (analytic may not be more than 5 % below AVL)", c.induced_drag_analytic_n, c.induced_drag_avl_n),
    );
    Ok(man)
}

fn rocket_stage(ctx: &StageCtx) -> anyhow::Result<Manifest> {
    let veh = ctx.design.get("vehicle").unwrap_or(&ctx.design);
    let aero: wright_rocket::aero::AeroBody = serde_json::from_value(veh["aero"].clone()).context("design.vehicle.aero")?;
    let engine: wright_rocket::engine::Engine = serde_json::from_value(veh["engine"].clone()).context("design.vehicle.engine")?;
    let mut man = Manifest::new("cfd", "CFD: rocket nose and nozzle");
    let (rows, nz) = rocket::run(&aero, &engine, &ROCKET_MACHS, &ctx.dir, &mut man, threads())?;
    let max_mach = ctx.design.pointer("/nominal/max_mach").and_then(|v| v.as_f64()).unwrap_or(2.0);
    let worst = rows.iter().filter(|r| r.mach <= max_mach + 0.1).map(|r| (r.cd_wright - r.cd_with_su2_nose) / r.cd_with_su2_nose).fold(f64::INFINITY, f64::min);
    man.check(
        "cfd_drag_not_optimistic",
        worst >= -0.02,
        &format!("over M <= {:.2}, the build-up CD is at worst {:+.1} % against the CD with SU2's nose drag (limit -2 %)", max_mach, 100.0 * worst),
    );
    if let Ok(full) = serde_json::from_value::<wright_rocket::RocketDesign>(ctx.design.clone()) {
        let a = rocket::apogee_effect(&full, &rows);
        let json = ctx.dir.join("rocket_apogee_effect.json");
        std::fs::write(&json, serde_json::to_vec_pretty(&a)?)?;
        man.add(&ctx.dir, &json, "Apogee with the SU2 nose drag (bound)", &format!("apogee {:.0} m today; {:.0} m with all drag x{:.3} (the CD ratio at peak Mach {:.2})", a.apogee_today_m, a.apogee_bound_m, a.drag_scale_at_max_mach, a.max_mach), &[("apogee_today_m", a.apogee_today_m), ("apogee_bound_m", a.apogee_bound_m)]);
    }
    let d = (nz.cf_vac_su2 - nz.cf_vac_lambda_model) / nz.cf_vac_lambda_model;
    man.check("cfd_nozzle_cf", d >= -0.01, &format!("vacuum Cf SU2 {:.4} vs engine model {:.4} ({:+.2} %, limit -1 %)", nz.cf_vac_su2, nz.cf_vac_lambda_model, 100.0 * d));
    Ok(man)
}
