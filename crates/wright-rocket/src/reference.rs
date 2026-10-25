//! The Berkeley reference: Space Enterprise at Berkeley's Eureka-2 with the
//! Lightbulb engine. Every published input below is cited in
//! `evals/rocket/reference.md`; everything else is Wright's own model.

use crate::engine::EngineSpec;
use crate::rules;
use crate::vehicle::VehicleParams;

/// Published numbers (sources in evals/rocket/reference.md).
pub struct Published;
impl Published {
    /// Chamber pressure 24 bar (arXiv 2401.07444 s2.1).
    pub const PC_PA: f64 = 24e5;
    /// Flows 1.14 kg/s LOX, 0.49 kg/s propane (arXiv 2401.07444 s2.1).
    pub const MDOT_OX: f64 = 1.14;
    pub const MDOT_FUEL: f64 = 0.49;
    /// Thrust 3 kN at sea level, nominal (arXiv 2401.07444 s2.1).
    pub const THRUST_N: f64 = 3000.0;
    /// "nominal, 14-second burn" (https://berkeleyse.org/ereg).
    pub const BURN_S: f64 = 14.0;
    /// 7.7 in diameter, 19.3 ft long (Eureka-1; https://berkeleyse.org/eureka1).
    pub const DIAMETER_M: f64 = 7.7 * 0.0254;
    pub const LENGTH_M: f64 = 19.3 * 0.3048;
    /// 120 lb fully fuelled (Eureka-1, Protolabs pre-flight article).
    pub const E1_LIFTOFF_KG: f64 = 120.0 * 0.453_592_37;
    /// Eureka-2 apogee 15,879 ft (https://www.youtube.com/watch?v=jMwTMRFgS2k).
    pub const APOGEE_M: f64 = 15_879.0 * 0.3048;
    /// Eureka-2 max Mach 0.88 (same source).
    pub const MAX_MACH: f64 = 0.88;
    /// Eureka-2 target 20,000 ft (Berkeley Engineering news, May 2023).
    pub const TARGET_M: f64 = 20_000.0 * 0.3048;
    /// FAR site, Mojave, ~2000 ft MSL.
    pub const SITE_ELEVATION_M: f64 = rules::FAR_SITE_ELEVATION_M;
}

pub fn of() -> f64 {
    Published::MDOT_OX / Published::MDOT_FUEL
}

/// Delivered Isp implied by the published thrust and flows, m/s.
pub fn published_isp() -> f64 {
    Published::THRUST_N / (Published::MDOT_OX + Published::MDOT_FUEL)
}

/// Eureka-2 as Wright reconstructs it from published inputs only. The
/// engine is sized for the published thrust at sea level; the c*
/// efficiency is *not* tuned to the published flows -- the gap is reported.
pub fn eureka2() -> VehicleParams {
    VehicleParams {
        engine: EngineSpec {
            oxidizer: "lox".into(),
            fuel: "propane".into(),
            pc: Published::PC_PA,
            of: of(),
            thrust: Published::THRUST_N,
            pa_design: 101_325.0,
            burn_time: Published::BURN_S,
            eta_cstar: None,
            // Injector manifold setpoints 480/460 psi at Pc 24 bar (eReg test
            // code, src/EReg/Config/*.h): dP/Pc = (480 psi - 24 bar)/24 bar.
            inj_dp: Some((480.0 * 6894.757 - Published::PC_PA) / Published::PC_PA),
            contraction: None,
        },
        tank_tube: "al6061_tube_6p0od_0p125w_72in".into(),
        airframe_tube: "wildman_g12_6in_60in".into(),
        nose: "wildman_fnc6_vk_5to1".into(),
        fin_material: "wildman_g10_g10_3_16".into(),
        fin_count: 4,
        fin_root: 0.40,
        fin_tip: 0.15,
        fin_span: 0.18,
        fin_sweep: 0.25,
        drogue: "fc_classic_elliptical_36in".into(),
        main: "fc_iris_ultra_compact_120in".into(),
        bottle: "airventuri_cf_tank_88cuft_4500".into(),
        bottles: 1,
        regulator: Some("ninja_pro_v3_reg_stainless".into()),
        valve: "valveman_cryo_ball_1_2_double_acting".into(),
        altimeters: vec!["featherweight_blue_raven".into(), "altus_easymini".into()],
        tracker: "featherweight_gps_tracker".into(),
        payload_kg: 0.0,
        // LOX tank 600 psi -> injector 480 psi: the injector-side eReg drops 20 %.
        feed_loss: 1.0 - 480.0 / 600.0,
        body_diameter: Some(Published::DIAMETER_M),
        extra_length: 0.0,
        measured_dry: None,
        // Tank setpoints LOX 600 psi, fuel 550 psi (eReg test-stand code,
        // github.com/Space-Enterprise-at-Berkeley/AutomaticElectronicPressureRegulator).
        tank_pressure: Some((600.0 * 6894.757, 550.0 * 6894.757)),
        ox_fill_k: None,
        dry_mass_factor: None,
        nose_cd_table: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::trajectory::{fly, Launch};
    use crate::vehicle::build;

    #[test]
    fn eureka2_flies() {
        let cat = Catalog::load().unwrap();
        let v = build(&eureka2(), &cat).unwrap();
        let m = v.mass_at(0.0);
        println!("dry {:.1} kg prop {:.1} kg liftoff {:.1} kg len {:.2} m cg {:.2}", v.dry_mass(), v.propellant_mass(), m.mass, v.length, m.cg);
        for it in &v.dry { println!("  {:<40} {:6.2} kg @ {:.2}", it.name, it.mass, it.x); }
        let e = &v.engine;
        println!("engine: Dt {:.1} mm De {:.1} mm eps {:.2} Dc {:.1} mm Lc {:.0} mm mdot {:.3} Isp_sl {:.1} s c* {:.0} Tc {:.0} h_t {:.0} mass {:.2}",
            e.throat_d*1e3, e.exit_d*1e3, e.area_ratio, e.chamber_d*1e3, e.chamber_cyl_len*1e3, e.mdot, e.isp(101325.0)/9.80665, e.c_star(), e.ideal.chamber.t_k, e.h_throat, e.mass);
        println!("pintle {:?}", e.injector);
        println!("tanks: ox {:.2} m fuel {:.2} m p_op {:.1} bar bh {:.1} mm; gn2 {:.2} kg; blowdown {:?}", v.ox_tank.length, v.fuel_tank.length, v.ox_tank.p_op/1e5, v.ox_tank.bulkhead_t*1e3, v.gn2_kg, v.blowdown());
        let l = Launch { elevation: Published::SITE_ELEVATION_M, rail_length: 50.0 * 0.3048, rail_tilt: 5f64.to_radians(), wind_10m: 3.0, thrust_scale: 1.0, drag_scale: 1.0 };
        let r = fly(&v, &l);
        println!("apogee {:.0} m mach {:.2} rail {:.1} m/s twr {:.2} margin {:.2}..{:.2} rail {:.2} maxq {:.0} accel {:.1} g aoa {:.1}",
            r.apogee_agl, r.max_mach, r.rail_exit_speed, r.liftoff_twr, r.margin_min, r.margin_max, r.margin_at_rail_exit, r.max_q, r.max_accel_g, r.max_aoa_deg);
        assert!(r.apogee_agl > 1000.0);
    }
}
