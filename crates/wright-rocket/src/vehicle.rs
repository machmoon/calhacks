//! Vehicle layout, tanks, pressurant, mass build-up, CG and pitch inertia.
//!
//! Layout from the nose tip aft (the common student liquid stack, and the
//! order SEB describes for Eureka-1 at https://berkeleyse.org/eureka1):
//! nose cone | recovery + avionics bay | pressurant bay | LOX tank |
//! intertank | fuel tank | engine bay + fin can. Tanks are 6061-T6 tube and
//! carry flight loads (structural tanks); the other bays are G12 fibreglass.

use crate::aero::AeroBody;
use crate::catalog::Catalog;
use crate::engine::{self, Engine, EngineSpec};
use crate::propellant::reactant;
use crate::rules;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Choices that define a vehicle. `design` fills these from a spec;
/// the reference reproduction fills them from published numbers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleParams {
    pub engine: EngineSpec,
    pub tank_tube: String,
    pub airframe_tube: String,
    pub nose: String,
    pub fin_material: String,
    pub fin_count: usize,
    pub fin_root: f64,
    pub fin_tip: f64,
    pub fin_span: f64,
    pub fin_sweep: f64,
    pub drogue: String,
    pub main: String,
    pub bottle: String,
    pub bottles: usize,
    pub regulator: Option<String>,
    pub valve: String,
    pub altimeters: Vec<String>,
    pub tracker: String,
    pub payload_kg: f64,
    /// Fraction of tank pressure lost in lines/valves before the injector
    /// (Wright allowance, 10 %; unsourced).
    pub feed_loss: f64,
    /// Body diameter override (m) for a vehicle with published diameter
    /// different from catalog stock (the reference reproduction).
    pub body_diameter: Option<f64>,
    /// Extra length (m) to match a published overall length.
    pub extra_length: f64,
    /// Measured dry mass (kg) and dry CG from the nose tip (m), which
    /// override the estimate when a team has weighed the built vehicle.
    pub measured_dry: Option<(f64, f64)>,
    /// Published tank pressures (ox, fuel), Pa, overriding the rule
    /// Pc (1 + dP_inj) / (1 - feed_loss).
    #[serde(default)]
    pub tank_pressure: Option<(f64, f64)>,
    /// Self-pressurised oxidiser only: the run tank's bulk temperature at
    /// launch, K (None: `rules::N2O_FILL_NOMINAL_K`). Sizing always uses the
    /// nominal temperature; this changes only the flown blowdown, so the
    /// checks can fly the same vehicle on a cold and a hot day.
    #[serde(default)]
    pub ox_fill_k: Option<f64>,
    /// Multiplies the estimated dry mass (CG unchanged): a calibration
    /// from a flown vehicle, e.g. the eval's Eureka-2 fit. None = 1.
    #[serde(default)]
    pub dry_mass_factor: Option<f64>,
    /// Nose pressure drag `[mach, cd]` from CFD on this nose, replacing the
    /// fineness-scaled TR-R-100 curve (see `aero::AeroBody::nose_cd_table`).
    #[serde(default)]
    pub nose_cd_table: Option<Vec<[f64; 2]>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub mass: f64,
    /// CG from the nose tip, m.
    pub x: f64,
    /// Axial extent, m (for its own pitch inertia).
    pub len: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tank {
    pub name: String,
    /// Forward end from nose tip, m.
    pub x_fwd: f64,
    pub inner_d: f64,
    pub wall: f64,
    /// Cylindrical length between bulkheads, m.
    pub length: f64,
    pub bulkhead_t: f64,
    pub propellant_kg: f64,
    pub density: f64,
    /// Maximum expected operating pressure (relief setting), Pa.
    pub meop: f64,
    /// Regulated operating pressure, Pa.
    pub p_op: f64,
    /// Radial 1/4-20 screws retaining each bulkhead (see `closure_screws`).
    #[serde(default)]
    pub closure_screws: usize,
}

/// Peak bending stress (Pa) of a flat circular bulkhead of radius `a`,
/// thickness `t`, uniform pressure `p`, edge free to rotate (simply
/// supported), from a numerical solution of the axisymmetric Kirchhoff
/// plate equation for the slope theta(r) -- theta'' + theta'/r - theta/r^2
/// = -Q(r)/D with Q = p r / 2, theta(0) = 0, M_r(a) = 0 -- on a 200-cell
/// finite-difference grid. Derived here independently of the closed-form
/// Roark expression the bulkhead is sized with (`bulkhead_thickness`); the
/// two agree to < 1 % (test), so this checks the sizing arithmetic and
/// thickness rounding, not a different edge condition.
pub fn plate_stress_fd(a: f64, t: f64, p: f64) -> f64 {
    let nu = 0.33;
    let n = 200usize;
    let h = a / n as f64;
    // Unknowns theta_1..theta_n (theta_0 = 0); D cancels in the stress, use D = 1.
    let m = n;
    let mut mat = vec![vec![0.0; m]; m];
    let mut rhs = vec![0.0; m];
    for i in 1..n {
        let r = i as f64 * h;
        let row = i - 1;
        let (lo, mid, hi) = (1.0 / (h * h) - 1.0 / (2.0 * h * r), -2.0 / (h * h) - 1.0 / (r * r), 1.0 / (h * h) + 1.0 / (2.0 * h * r));
        if i >= 2 {
            mat[row][i - 2] = lo;
        }
        mat[row][i - 1] = mid;
        mat[row][i] = hi;
        rhs[row] = -p * r / 2.0;
    }
    // Edge: theta' + nu theta / a = 0 (second-order backward difference).
    mat[m - 1][m - 1] = 3.0 / (2.0 * h) + nu / a;
    mat[m - 1][m - 2] = -4.0 / (2.0 * h);
    mat[m - 1][m - 3] = 1.0 / (2.0 * h);
    // Gaussian elimination with partial pivoting (small dense system).
    for c in 0..m {
        let piv = (c..m).max_by(|&x, &y| mat[x][c].abs().total_cmp(&mat[y][c].abs())).unwrap_or(c);
        mat.swap(c, piv);
        rhs.swap(c, piv);
        let pivot_row = mat[c].clone();
        for r2 in c + 1..m {
            let f = mat[r2][c] / pivot_row[c];
            if f != 0.0 {
                for (x, pv) in mat[r2][c..].iter_mut().zip(&pivot_row[c..]) {
                    *x -= f * pv;
                }
                rhs[r2] -= f * rhs[c];
            }
        }
    }
    let mut th = vec![0.0; m];
    for c in (0..m).rev() {
        let s: f64 = (c + 1..m).map(|k| mat[c][k] * th[k]).sum();
        th[c] = (rhs[c] - s) / mat[c][c];
    }
    let theta = |i: usize| if i == 0 { 0.0 } else { th[i - 1] };
    // Moments (per unit D): M_r = -(theta' + nu theta/r), M_t = -(theta/r + nu theta').
    let mut mmax: f64 = 0.0;
    for i in 1..n {
        let r = i as f64 * h;
        let d1 = (theta(i + 1) - theta(i - 1)) / (2.0 * h);
        mmax = mmax.max((d1 + nu * theta(i) / r).abs()).max((theta(i) / r + nu * d1).abs());
    }
    // Centre: M_r = M_t = (1 + nu) theta'(0).
    mmax = mmax.max(((1.0 + nu) * theta(1) / h).abs());
    // sigma = 6 M / t^2 with M = D * m; D = E t^3 / (12 (1 - nu^2)) cancels:
    // the solved theta is theta_true * D, so m above is the moment itself.
    6.0 * mmax / (t * t)
}

/// Screws per bulkhead: the fewest (even, >= 6) that keep wall bearing at
/// proof (1.5 x MEOP) below the 6061 bearing yield. The check
/// (`closure_capacity`) loads them at burst and tests screw shear, bearing
/// and tear-out on ultimate.
pub fn closure_screws(inner_d: f64, wall: f64, meop: f64) -> usize {
    let f = 1.5 * meop * PI * inner_d * inner_d / 4.0;
    let per = rules::CLOSURE_SCREW_D_M * wall * rules::AL6061_BEARING_YIELD_PA;
    let n = (f / per).ceil().max(6.0) as usize;
    n + n % 2
}

/// Per-screw capacity on ultimate (N) by mode: (screw shear, wall bearing,
/// wall tear-out over two planes of length e).
pub fn closure_capacity(wall: f64) -> (f64, f64, f64) {
    let d = rules::CLOSURE_SCREW_D_M;
    (
        rules::STEEL_SHEAR_OVER_TENSILE * rules::A574_TENSILE_PA * rules::CLOSURE_SCREW_ROOT_AREA_M2,
        d * wall * rules::AL6061_BEARING_ULT_PA,
        2.0 * rules::CLOSURE_EDGE_OVER_D * d * wall * rules::AL6061_SHEAR_ULT_PA,
    )
}

impl Tank {
    pub fn volume(&self) -> f64 {
        PI * self.inner_d.powi(2) / 4.0 * self.length
    }
    /// Liquid CG and mass with `frac` of the load left (settled aft).
    pub fn liquid(&self, frac: f64) -> (f64, f64) {
        let m = self.propellant_kg * frac;
        let h = m / self.density / (PI * self.inner_d.powi(2) / 4.0);
        let aft = self.x_fwd + self.bulkhead_t + self.length;
        (m, aft - h / 2.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vehicle {
    pub params: VehicleParams,
    pub engine: Engine,
    pub diameter: f64,
    pub length: f64,
    pub nose_length: f64,
    pub dry: Vec<Item>,
    pub ox_tank: Tank,
    pub fuel_tank: Tank,
    /// Pressurant: bottle volume total (m^3), fill pressure (Pa), GN2 mass.
    pub bottle_volume: f64,
    pub bottle_p0: f64,
    pub gn2_kg: f64,
    pub burn_time: f64,
    pub aero: AeroBody,
    /// Section boundaries from the tip, for geometry: (name, x0, x1).
    pub sections: Vec<(String, f64, f64)>,
    /// Self-pressurised oxidiser: the flown blowdown (thrust, flows and
    /// tank state against time). None: regulated feed, constant thrust.
    #[serde(default)]
    pub profile: Option<crate::engine::BurnProfile>,
    /// Cached dry sums (m, sum m x, sum m (x^2 + l^2/12)).
    #[serde(skip)]
    pub dry_sums: std::sync::OnceLock<(f64, f64, f64)>,
}

/// Mass, CG and pitch inertia about the CG.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MassState {
    pub mass: f64,
    pub cg: f64,
    pub inertia: f64,
}

/// Bulkhead thickness for a flat simply-supported circular plate (Roark,
/// Formulas for Stress and Strain, Table 11.2 case 10a: sigma_max =
/// 3(3+nu) p a^2 / (8 t^2)), sized at `sf` x `p` against `allow`.
pub fn bulkhead_thickness(radius: f64, p: f64, sf: f64, allow: f64) -> f64 {
    let nu = 0.33;
    radius * (3.0 * (3.0 + nu) * p * sf / (8.0 * allow)).sqrt()
}

pub fn build(params: &VehicleParams, cat: &Catalog) -> anyhow::Result<Vehicle> {
    let eng = engine::design(&params.engine)?;
    let ox = reactant(&params.engine.oxidizer)?;
    let fuel = reactant(&params.engine.fuel)?;
    let tube = cat.get(&params.tank_tube).ok_or_else(|| anyhow::anyhow!("no tank tube {}", params.tank_tube))?;
    let af = cat.get(&params.airframe_tube).ok_or_else(|| anyhow::anyhow!("no airframe tube {}", params.airframe_tube))?;
    let nose = cat.get(&params.nose).ok_or_else(|| anyhow::anyhow!("no nose {}", params.nose))?;
    let bottle = cat.get(&params.bottle).ok_or_else(|| anyhow::anyhow!("no bottle {}", params.bottle))?;
    let tube_od = tube.f("od_mm").unwrap() * 1e-3;
    let tube_wall = tube.f("wall_mm").unwrap() * 1e-3;
    let af_od = af.f("od_mm").unwrap() * 1e-3;
    let af_id = af.f("id_mm").unwrap() * 1e-3;
    let diameter = params.body_diameter.unwrap_or(af_od.max(tube_od));
    let nose_len = nose.f("length_mm").unwrap() * 1e-3 * diameter / af_od;

    // Propellant loads: burn time x flow, plus residuals.
    let t_b = params.engine.burn_time;
    let id = tube_od - 2.0 * tube_wall;
    let area = PI * id * id / 4.0;
    let bh = |p: f64| ((bulkhead_thickness(id / 2.0, p, rules::FAR_BURST_SF_METAL, rules::AL6061_ULT_PA) * 1e3).ceil() / 1e3).max(6e-3);
    let mk_tank = |name: &str, m: f64, rho: f64, p_op: f64| Tank {
        name: name.into(),
        x_fwd: 0.0,
        inner_d: id,
        wall: tube_wall,
        length: m / rho / (1.0 - rules::ULLAGE) / area,
        bulkhead_t: bh(1.1 * p_op),
        propellant_kg: m,
        density: rho,
        meop: 1.1 * p_op,
        p_op,
        closure_screws: closure_screws(id, tube_wall, 1.1 * p_op),
    };
    let (mut ox_tank, mut fuel_tank, profile, burn_time) = if ox.self_pressurizing {
        // Self-pressurised run tank: N2O load from the design flow and burn
        // time; volume so the same load is still (1 - ULLAGE) liquid by
        // volume at the hot end of the fill band (liquid N2O swells ~12 %
        // from 20 to 30 C; a liquid-full tank is a hydraulic-lock hazard).
        let tri = eng.triplet().ok_or_else(|| anyhow::anyhow!("self-pressurised oxidiser needs the triplet injector"))?;
        let m_ox = eng.mdot_ox * t_b * (1.0 + rules::RESIDUAL);
        let hot = crate::nitrous::sat(rules::N2O_FILL_HOT_K)?;
        let nom = crate::nitrous::sat(rules::N2O_FILL_NOMINAL_K)?;
        let vol = m_ox / (hot.rho_l * (1.0 - rules::ULLAGE) + hot.rho_v * rules::ULLAGE);
        let p_f = params.tank_pressure.map(|p| p.1).unwrap_or(tri.fuel_tank_p);
        let nominal = crate::engine::n2o_burn(&eng, vol, m_ox, rules::N2O_FILL_NOMINAL_K, p_f, params.feed_loss)?;
        let m_f = nominal.f_used[nominal.f_used.len() - 1] * (1.0 + rules::RESIDUAL);
        let flown = match params.ox_fill_k {
            Some(k) if k != rules::N2O_FILL_NOMINAL_K => crate::engine::n2o_burn(&eng, vol, m_ox, k, p_f, params.feed_loss)?,
            _ => nominal,
        };
        let ox_t = Tank {
            name: format!("{} run tank", ox.id.to_uppercase()),
            x_fwd: 0.0,
            inner_d: id,
            wall: tube_wall,
            length: vol / area,
            // Relief at the vapour pressure of the hot end of the fill band.
            bulkhead_t: bh(hot.p),
            propellant_kg: m_ox,
            density: nom.rho_l,
            meop: hot.p,
            p_op: nom.p,
            closure_screws: closure_screws(id, tube_wall, hot.p),
        };
        let tb = flown.t_end;
        (ox_t, mk_tank("fuel tank", m_f, fuel.density_kg_m3, p_f), Some(flown), tb)
    } else {
        let m_ox = eng.mdot_ox * t_b * (1.0 + rules::RESIDUAL);
        let m_f = eng.mdot_fuel * t_b * (1.0 + rules::RESIDUAL);
        // Tank pressures: Pc (1 + dP_inj) through the feed losses; MEOP is the
        // relief setting 10 % above the regulator setpoint (Wright choice).
        let p_rule = params.engine.pc * (1.0 + params.engine.inj_dp.unwrap_or(rules::INJ_DP_DESIGN)) / (1.0 - params.feed_loss);
        let (p_ox, p_fu) = params.tank_pressure.unwrap_or((p_rule, p_rule));
        (mk_tank("LOX tank", m_ox, ox.density_kg_m3, p_ox), mk_tank("fuel tank", m_f, fuel.density_kg_m3, p_fu), None, t_b)
    };

    // Pressurant: the bottle's fill. What it must deliver is checked by
    // `pressurant_requirement` (closed form) and sized by `blowdown`.
    // Fill pressure: the bottle's rating or what the gas source reaches,
    // whichever is lower (GN2 cylinders for LOX, HPA air for N2O; rules).
    let source_psi = if ox.self_pressurizing { rules::HPA_FILL_PSI } else { rules::GN2_FILL_PSI };
    let p0 = bottle.f("pressure_psi").unwrap().min(source_psi) * 6894.757;
    let bottle_v = bottle.f("volume_l").unwrap() * 1e-3 * params.bottles as f64;
    // Fill mass at 293 K with the NIST compressibility.
    let gn2 = p0 * bottle_v / (rules::n2_z(p0) * rules::GN2_R * 293.0);

    // Bays (lengths in metres; Wright allowances, unsourced, stated).
    let recovery_len = 0.80; // main + drogue + harnesses + avionics sled
    // Bay: bottles stacked with 20 mm gaps, 50 mm fore, 150 mm aft for the
    // regulators; a bottle with no published length uses a 2x-diameter-of-
    // a-sphere estimate (and fails the geometry check for missing dims).
    let one_len = bottle.f("length_mm").map(|l| l * 1e-3).unwrap_or(2.0 * (bottle_v / params.bottles as f64).cbrt());
    let bottle_len = 0.05 + params.bottles as f64 * (one_len + 0.02) + 0.15;
    let intertank = 0.15;
    let engine_bay = (eng.length() + 0.20).max(0.6);

    let mut x = nose_len;
    let mut sections = vec![("nose".to_string(), 0.0, nose_len)];
    let mut items = Vec::new();
    let mut push = |n: &str, m: f64, xc: f64, l: f64| items.push(Item { name: n.into(), mass: m, x: xc, len: l });
    // Nose: catalog mass (mean of the listed range) at 2/3 of its length (thin conical shell CG).
    let nose_m = match (nose.f("mass_kg_min"), nose.f("mass_kg_max"), nose.f("mass_kg")) {
        (Some(a), Some(b), _) => 0.5 * (a + b),
        (_, _, Some(m)) => m,
        _ => anyhow::bail!("nose cone {} has no mass", nose.id()),
    };
    push("nose cone", nose_m * (diameter / af_od).powi(2), nose_len * 0.66, nose_len);
    let g12_rho = rules::FIBERGLASS_DENSITY;
    let g12_shell = |l: f64| PI * (af_od.powi(2) - af_id.powi(2)) / 4.0 * l * g12_rho * (diameter / af_od).powi(2);
    // Recovery/avionics bay.
    let rec_x0 = x;
    push("recovery+avionics bay tube", g12_shell(recovery_len), x + recovery_len / 2.0, recovery_len);
    let drogue = cat.get(&params.drogue).ok_or_else(|| anyhow::anyhow!("no drogue {}", params.drogue))?;
    let main = cat.get(&params.main).ok_or_else(|| anyhow::anyhow!("no main {}", params.main))?;
    push("main parachute", main.f("mass_g").unwrap() * 1e-3, x + 0.25, 0.3);
    push("drogue parachute", drogue.f("mass_g").unwrap() * 1e-3, x + 0.55, 0.2);
    push("shock cords (2 x kevlar)", 2.0 * 0.35, x + 0.4, 0.4);
    push("avionics, batteries, wiring", rules::AVIONICS_KG, x + 0.70, 0.2);
    push("bulkheads + couplers (recovery)", 1.2, x + 0.4, recovery_len);
    if params.payload_kg > 0.0 {
        push("payload", params.payload_kg, x + 0.70, 0.15);
    }
    x += recovery_len;
    sections.push(("recovery".into(), rec_x0, x));
    // Pressurant bay.
    let pb0 = x;
    push("pressurant bay tube", g12_shell(bottle_len), x + bottle_len / 2.0, bottle_len);
    let b_mass = bottle.f("mass_kg").unwrap_or(f64::NAN);
    anyhow::ensure!(b_mass.is_finite(), "bottle {} has no mass in the catalog", bottle.id());
    push("GN2 bottles", b_mass * params.bottles as f64, x + bottle_len / 2.0, bottle_len);
    let reg_mass = params.regulator.as_ref().and_then(|r| cat.get(r)).and_then(|r| r.f("mass_g")).map(|g| g * 1e-3).unwrap_or(rules::REGULATOR_MASS_ALLOWANCE_KG);
    push("regulators (2) + pressurant plumbing", 2.0 * reg_mass + 0.8, x + bottle_len - 0.05, 0.1);
    x += bottle_len;
    sections.push(("pressurant".into(), pb0, x));
    // LOX tank.
    ox_tank.x_fwd = x;
    let tank_shell = |t: &Tank| PI * (tube_od.powi(2) - t.inner_d.powi(2)) / 4.0 * (t.length + 2.0 * t.bulkhead_t) * rules::AL6061_DENSITY;
    let bh_mass = |t: &Tank| 2.0 * PI * t.inner_d.powi(2) / 4.0 * t.bulkhead_t * rules::AL6061_DENSITY;
    let lox_len = ox_tank.length + 2.0 * ox_tank.bulkhead_t;
    push("LOX tank wall", tank_shell(&ox_tank), x + lox_len / 2.0, lox_len);
    push("LOX tank bulkheads", bh_mass(&ox_tank), x + lox_len / 2.0, lox_len);
    sections.push(("lox_tank".into(), x, x + lox_len));
    x += lox_len;
    let it0 = x;
    push("intertank (Al) + LOX downcomer", PI * (tube_od.powi(2) - id.powi(2)) / 4.0 * intertank * rules::AL6061_DENSITY + 0.5, x + intertank / 2.0, intertank);
    x += intertank;
    sections.push(("intertank".into(), it0, x));
    fuel_tank.x_fwd = x;
    let f_len = fuel_tank.length + 2.0 * fuel_tank.bulkhead_t;
    push("fuel tank wall", tank_shell(&fuel_tank), x + f_len / 2.0, f_len);
    push("fuel tank bulkheads", bh_mass(&fuel_tank), x + f_len / 2.0, f_len);
    sections.push(("fuel_tank".into(), x, x + f_len));
    x += f_len;
    // Engine bay + fin can.
    let eb0 = x;
    push("engine bay tube", g12_shell(engine_bay), x + engine_bay / 2.0, engine_bay);
    push("main valves (2) + actuators + feed lines", 2.0 * rules::PLUMBING_PER_CIRCUIT_KG, x + 0.12, 0.2);
    push("thrust structure", rules::THRUST_STRUCTURE_KG_PER_KN * params.engine.thrust / 1000.0, x + 0.2, 0.1);
    let eng_x0 = x + engine_bay - eng.length();
    push("engine", eng.mass, eng_x0 + eng.length() * 0.4, eng.length());
    x += engine_bay + params.extra_length;
    sections.push(("engine_bay".into(), eb0, x));
    let length = x;
    // Fins: G10 flush with the aft end.
    let fin = cat.get(&params.fin_material).ok_or_else(|| anyhow::anyhow!("no fin material {}", params.fin_material))?;
    let fin_t = fin.f("thickness_mm").unwrap() * 1e-3;
    let fin_area = 0.5 * (params.fin_root + params.fin_tip) * params.fin_span;
    let fin_le = length - params.fin_root;
    push(
        "fins (G10) + fillets",
        params.fin_count as f64 * fin_area * fin_t * rules::FIBERGLASS_DENSITY * 1.15,
        fin_le + params.fin_root * 0.55,
        params.fin_root,
    );
    let aero = AeroBody {
        diameter,
        nose_length: nose_len,
        body_length: length - nose_len,
        fin_count: params.fin_count,
        fin_root: params.fin_root,
        fin_tip: params.fin_tip,
        fin_span: params.fin_span,
        fin_sweep: params.fin_sweep,
        fin_thickness: fin_t,
        fin_le_x: fin_le,
        roughness: 20e-6,
        nose_cd_table: params.nose_cd_table.clone(),
        cache: Default::default(),
    };
    let mut dry = items;
    // GN2 gas itself, in the bottles at the start.
    dry.push(Item { name: "GN2 (initial charge)".into(), mass: gn2, x: pb0 + bottle_len / 2.0, len: bottle_len });
    if let Some((m, cg)) = params.measured_dry {
        // A weighed vehicle overrides the estimate: scale masses and shift
        // CGs so the totals match what the team measured.
        let (m0, cg0) = totals(&dry);
        for it in &mut dry {
            it.mass *= m / m0;
        }
        // Shift CGs so the total CG lands on the measured one.
        for it in &mut dry {
            it.x += cg - cg0;
        }
    }
    // A weighed vehicle needs no calibration.
    if let (Some(k), None) = (params.dry_mass_factor, params.measured_dry) {
        anyhow::ensure!(k.is_finite() && k > 0.0, "dry_mass_factor {k} must be positive");
        for it in &mut dry {
            it.mass *= k;
        }
    }
    Ok(Vehicle {
        params: params.clone(),
        engine: eng,
        diameter,
        length,
        nose_length: nose_len,
        dry,
        ox_tank,
        fuel_tank,
        bottle_volume: bottle_v,
        bottle_p0: p0,
        gn2_kg: gn2,
        burn_time,
        aero,
        sections,
        profile,
        dry_sums: Default::default(),
    })
}

fn totals(items: &[Item]) -> (f64, f64) {
    let m: f64 = items.iter().map(|i| i.mass).sum();
    (m, items.iter().map(|i| i.mass * i.x).sum::<f64>() / m)
}

impl Vehicle {
    pub fn dry_mass(&self) -> f64 {
        totals(&self.dry).0
    }
    pub fn propellant_mass(&self) -> f64 {
        self.ox_tank.propellant_kg + self.fuel_tank.propellant_kg
    }
    /// Propellant that leaves through the engine over the burn, kg.
    pub fn propellant_burned(&self) -> f64 {
        match &self.profile {
            None => self.engine.mdot * self.burn_time,
            Some(p) => p.ox_used[p.ox_used.len() - 1] + p.f_used[p.f_used.len() - 1],
        }
    }
    /// Mass properties at time `t` after ignition (propellant drains
    /// linearly over the burn; residuals stay).
    pub fn mass_at(&self, t: f64) -> MassState {
        let (frac_o, frac_f) = match &self.profile {
            None => {
                let used = (t / self.burn_time).clamp(0.0, 1.0);
                let frac = 1.0 - used / (1.0 + rules::RESIDUAL);
                (frac, frac)
            }
            Some(p) => (
                1.0 - p.at(&p.ox_used, t) / self.ox_tank.propellant_kg,
                1.0 - p.at(&p.f_used, t) / self.fuel_tank.propellant_kg,
            ),
        };
        let (mut m, mut mx, mut mxx) = *self.dry_sums.get_or_init(|| {
            self.dry.iter().fold((0.0, 0.0, 0.0), |(a, b, c), i| (a + i.mass, b + i.mass * i.x, c + i.mass * (i.x * i.x + i.len * i.len / 12.0)))
        });
        for (tank, frac) in [(&self.ox_tank, frac_o), (&self.fuel_tank, frac_f)] {
            let (mi, xi) = tank.liquid(frac);
            let h = mi / tank.density / (PI * tank.inner_d.powi(2) / 4.0);
            m += mi;
            mx += mi * xi;
            mxx += mi * (xi * xi + h * h / 12.0);
        }
        let cg = mx / m;
        let r = self.diameter / 2.0;
        // Parallel axis: sum m (x - cg)^2 = sum m x^2 - M cg^2.
        let inertia = mxx - m * cg * cg + m * r * r / 4.0;
        MassState { mass: m, cg, inertia }
    }
    /// Thrust (N) at time `t` after ignition and ambient pressure `pa`.
    /// Regulated feed: the design thrust until burnout. Self-pressurised:
    /// the blowdown's Pc(t) through the fixed nozzle, F = Cf_vac Pc At - pa Ae.
    pub fn thrust_at(&self, t: f64, pa: f64) -> f64 {
        if t >= self.burn_time {
            return 0.0;
        }
        match &self.profile {
            None => self.engine.thrust(pa),
            Some(p) => (self.engine.cf_vac() * p.at(&p.pc, t) * self.engine.throat_area() - pa * self.engine.exit_area()).max(0.0),
        }
    }
    /// Total propellant flow (kg/s) at time `t`.
    pub fn mdot_at(&self, t: f64) -> f64 {
        if t >= self.burn_time {
            return 0.0;
        }
        match &self.profile {
            None => self.engine.mdot,
            Some(p) => p.at(&p.mdot_ox, t) + p.at(&p.mdot_f, t),
        }
    }
    /// The pressure the GN2 regulators must hold: both tanks on a regulated
    /// feed, the fuel tank alone when the oxidiser self-pressurises.
    pub fn regulated_p(&self) -> f64 {
        match self.profile {
            None => self.ox_tank.p_op.max(self.fuel_tank.p_op),
            Some(_) => self.fuel_tank.p_op,
        }
    }
    /// GN2 left in the bottles (kg) and bottle pressure (Pa) at end of
    /// burn, by a time-stepped adiabatic blowdown that feeds both tanks at
    /// their regulated pressure while propellant leaves. Independent of the
    /// sizing formula. Returns None if the bottle drops below the tank
    /// pressure (regulator dropout) before burnout.
    pub fn blowdown(&self) -> (f64, f64, bool) {
        let g = rules::GN2_GAMMA;
        let n = 400;
        let dt = self.burn_time / n as f64;
        let mut m = self.gn2_kg;
        let m0 = m;
        let mut p = self.bottle_p0;
        let mut ok = true;
        // Initial ullage already at p_op (pre-pressurised on the pad from GSE).
        let q_ox = self.engine.mdot_ox / self.ox_tank.density; // m^3/s displaced
        let q_f = self.engine.mdot_fuel / self.fuel_tank.density;
        for k in 0..n {
            // Gas leaves the bottle at the bottle's own (isentropically
            // falling) temperature: ~170 K by the end of a 310 bar blowdown,
            // so each cubic metre of ullage takes 1.7x the mass it would at
            // 293 K (hostile review 2026-10-24). The LOX side cools further
            // to the 150 K ullage assumption.
            let t_gas = 293.0 * (p / self.bottle_p0).powf((g - 1.0) / g);
            let dm = match &self.profile {
                None => self.ox_tank.p_op * q_ox * dt / (rules::GN2_R * t_gas.min(150.0)) + self.fuel_tank.p_op * q_f * dt / (rules::GN2_R * t_gas),
                // Fuel side only; the N2O run tank pressurises itself.
                Some(p) => self.fuel_tank.p_op * p.at(&p.mdot_f, (k as f64 + 0.5) * dt) / self.fuel_tank.density * dt / (rules::GN2_R * t_gas),
            };
            m -= dm;
            if m <= 0.0 {
                return (0.0, 0.0, false);
            }
            // Adiabatic (isentropic) bottle: p ~ rho^gamma.
            p = self.bottle_p0 * (m / m0).powf(g);
            // Regulator needs ~ 1.15 x outlet upstream to hold setpoint (assumed dropout).
            if p < 1.15 * self.regulated_p() {
                ok = false;
            }
        }
        (m, p, ok)
    }
}

/// Independent pressurant check (closed form, not the time-stepped
/// blowdown the search sizes with): usable GN2 = fill mass minus what an
/// isentropic bottle still holds at the regulator dropout pressure
/// (m_res = m0 (p_min/p0)^(1/gamma)); required GN2 = every regulated tank's
/// displaced propellant volume filled at its setpoint with gas at the
/// bottle's end temperature T0 (p_min/p0)^((gamma-1)/gamma) (150 K on the
/// LOX side). Returns (usable/required - 1, required kg, usable kg, end T).
pub fn pressurant_requirement(v: &Vehicle) -> (f64, f64, f64, f64) {
    let g = rules::GN2_GAMMA;
    let p_min = 1.15 * v.regulated_p();
    let r = p_min / v.bottle_p0;
    let t_end = 293.0 * r.powf((g - 1.0) / g);
    let usable = v.gn2_kg * (1.0 - r.powf(1.0 / g));
    let req = match &v.profile {
        None => {
            let vo = v.engine.mdot_ox * v.burn_time / v.ox_tank.density;
            let vf = v.engine.mdot_fuel * v.burn_time / v.fuel_tank.density;
            v.ox_tank.p_op * vo / (rules::GN2_R * t_end.min(150.0)) + v.fuel_tank.p_op * vf / (rules::GN2_R * t_end)
        }
        Some(p) => v.fuel_tank.p_op * (p.f_used[p.f_used.len() - 1] / v.fuel_tank.density) / (rules::GN2_R * t_end),
    };
    (usable / req - 1.0, req, usable, t_end)
}

/// Propellant + pressurant check figure: minimum ratio of bottle pressure to
/// regulator-required inlet over the burn (>= 1 passes).
pub fn pressurant_margin(v: &Vehicle) -> f64 {
    let (_, p_end, _) = v.blowdown();
    p_end / (1.15 * v.regulated_p()) - 1.0
}

#[cfg(test)]
mod plate_tests {
    #[test]
    fn fd_plate_agrees_with_roark_simply_supported() {
        let (a, t, p) = (0.073, 0.008, 1.0e7);
        let roark = 3.0 * (3.0 + 0.33) * p * a * a / (8.0 * t * t);
        let fd = super::plate_stress_fd(a, t, p);
        assert!((fd / roark - 1.0).abs() < 0.01, "fd {fd} roark {roark}");
    }
}

