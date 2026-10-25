//! Liquid engine design: delivered performance, chamber and nozzle
//! geometry, pintle injector, Bartz gas-side heat transfer, and a transient
//! multilayer wall conduction model for an ablative-lined aluminium case
//! with a graphite throat (the Lightbulb architecture of Berkeley SEB,
//! https://berkeleyse.org/lightbulb-ablative-engine).
//!
//! Sizing and checking are deliberately different models: the chamber wall
//! is sized by thin-wall hoop stress and checked by Lame thick-wall stress;
//! the liner is sized from a semi-infinite conduction estimate and checked
//! by a finite-difference transient through the actual layer stack; the
//! injector is sized at the nominal point with continuous orifice areas and
//! checked at the end-of-burn tank pressure with drill-rounded orifices.

use crate::performance::{rocket, Exit, Flow, Performance};
use crate::propellant::{reactant, Mix, Reactant};
use crate::rules;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSpec {
    pub oxidizer: String,
    pub fuel: String,
    /// Chamber pressure, Pa.
    pub pc: f64,
    pub of: f64,
    /// Thrust at `pa_design`, N.
    pub thrust: f64,
    /// Ambient pressure the nozzle is sized for (pe = pa_design), Pa.
    pub pa_design: f64,
    pub burn_time: f64,
    /// c* efficiency override (None: `rules::ETA_CSTAR`). The reference
    /// reproduction sets it from the published thrust and flows.
    #[serde(default)]
    pub eta_cstar: Option<f64>,
    /// Injector dP/Pc override (None: `rules::INJ_DP_DESIGN`).
    #[serde(default)]
    pub inj_dp: Option<f64>,
    /// Chamber contraction ratio Ac/At override (None: the Humble
    /// correlation `rules::contraction_ratio`). Huzel & Huang SP-125 p.88:
    /// "For pressurized-gas propellant feed, low-thrust engine systems
    /// contraction area ratio values of 2 to 5 have been used", optimised
    /// over factors including "(6) Space envelope (7) Ease of
    /// manufacturing" -- the search lowers it (not below
    /// `rules::CONTRACTION_MIN`) when the chamber liner would not fit the
    /// largest priced phenolic stock.
    #[serde(default)]
    pub contraction: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pintle {
    /// Pintle shaft outer diameter at the tip, m.
    pub pintle_d: f64,
    /// Radial oxidiser holes in the pintle tip.
    pub ox_holes: usize,
    pub ox_hole_d: f64,
    /// Fuel annulus radial gap around the pintle, m.
    pub fuel_gap: f64,
    pub dp_ox: f64,
    pub dp_fuel: f64,
    pub v_ox: f64,
    pub v_fuel: f64,
    /// Total momentum ratio (radial ox / axial fuel).
    pub tmr: f64,
}

/// Unlike-impinging triplet (O-F-O) for a self-pressurised (two-phase)
/// oxidiser: `elements` triplets, each two N2O jets at +/- `ox_angle_deg`
/// impinging symmetrically on one axial fuel jet. Huzel & Huang SP-125
/// p.125: a doublet's resultant angle "will vary with mixture ratio", and
/// "two streams of one propellant impinging symmetrically on one stream of
/// the other propellant will eliminate the change of vector angle beta, as
/// a result of mixture-ratio variations". A blowdown N2O feed swings O/F by
/// ~2x through the burn and the fill-temperature band, and an unlike
/// doublet sized for beta = 0 at 20 C reached -15.7 deg on a 5 C morning
/// (Wright's first N2O design, 2026-10-24), so N2O engines use the triplet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Triplet {
    pub elements: usize,
    /// Two of these per element.
    pub ox_hole_d: f64,
    /// One of these per element, on the element axis.
    pub fuel_hole_d: f64,
    /// Each N2O jet's angle to the axis (= its included angle with the
    /// axial fuel jet), deg.
    pub ox_angle_deg: f64,
    /// Design-point drops, Pa.
    pub dp_ox: f64,
    pub dp_fuel: f64,
    /// Regulated fuel tank pressure the injector was sized for, Pa.
    pub fuel_tank_p: f64,
    /// N2O bulk temperature at the design point, K.
    pub fill_k: f64,
}

impl Triplet {
    pub fn ox_area(&self) -> f64 {
        2.0 * self.elements as f64 * PI * self.ox_hole_d.powi(2) / 4.0
    }
    pub fn fuel_area(&self) -> f64 {
        self.elements as f64 * PI * self.fuel_hole_d.powi(2) / 4.0
    }
    /// Resultant momentum angle of one element from the axis, deg, for
    /// flows `mo`, `mf` (kg/s) at densities `rho_o`, `rho_f`: Huzel & Huang
    /// eq 4-41 / Sutton & Biblarz eq 8-6 summed over the three jets, with
    /// V = W / (rho A) through the drilled area (H&H eq 4-39). The two N2O
    /// jets carry equal flow at +/- angle, so it is zero at every O/F.
    /// Resultant angle (deg) with the element's two N2O holes off by
    /// `tol` in opposite directions (one +tol, one -tol): the flow splits by
    /// area at the same drop, so the larger hole carries more momentum and
    /// the jets no longer cancel. Same momentum sum as `beta_deg`.
    pub fn beta_deg_tolerance(&self, mo: f64, mf: f64, rho_o: f64, rho_f: f64, tol: f64) -> f64 {
        let n = self.elements as f64;
        let a1 = PI * (self.ox_hole_d + tol).powi(2) / 4.0;
        let a2 = PI * (self.ox_hole_d - tol).max(0.0).powi(2) / 4.0;
        let a_f1 = PI * self.fuel_hole_d.powi(2) / 4.0;
        let mo_el = mo / n;
        let (m1, m2) = (mo_el * a1 / (a1 + a2), mo_el * a2 / (a1 + a2));
        let (mom1, mom2) = (m1 * m1 / (rho_o * a1), if a2 > 0.0 { m2 * m2 / (rho_o * a2) } else { 0.0 });
        let mf1 = mf / n;
        let mom_f = mf1 * mf1 / (rho_f * a_f1);
        let g = self.ox_angle_deg.to_radians();
        (mom1 * g.sin() - mom2 * g.sin()).atan2(mom1 * g.cos() + mom2 * g.cos() + mom_f).to_degrees()
    }
    pub fn beta_deg(&self, mo: f64, mf: f64, rho_o: f64, rho_f: f64) -> f64 {
        let n = self.elements as f64;
        let a_o1 = PI * self.ox_hole_d.powi(2) / 4.0;
        let a_f1 = PI * self.fuel_hole_d.powi(2) / 4.0;
        let (mo1, mf1) = (mo / (2.0 * n), mf / n);
        let mom_o = mo1 * mo1 / (rho_o * a_o1);
        let mom_f = mf1 * mf1 / (rho_f * a_f1);
        let g = self.ox_angle_deg.to_radians();
        let transverse = mom_o * g.sin() + mom_o * (-g).sin();
        let axial = 2.0 * mom_o * g.cos() + mom_f;
        transverse.atan2(axial).to_degrees()
    }
}

/// The injector: a pintle for regulated (LOX) feeds, an unlike triplet for
/// self-pressurised N2O.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Injector {
    Pintle(Pintle),
    Triplet(Triplet),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub material: Material,
    pub thickness: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Material {
    Phenolic,
    Graphite,
    Al6061,
}

impl Material {
    /// (k W/m K, rho kg/m^3, cp J/kg K). Graphite: isomolded, k ~ 80, cp
    /// rising with T; 1500 used (high-T mean). Aluminium 6061-T6: k 167,
    /// cp 896 (ASM/matweb). Phenolic: `rules`.
    pub fn props(self) -> (f64, f64, f64) {
        match self {
            Material::Phenolic => (rules::PHENOLIC_K, rules::PHENOLIC_DENSITY, rules::PHENOLIC_CP),
            Material::Graphite => (80.0, 1800.0, 1500.0),
            Material::Al6061 => (167.0, rules::AL6061_DENSITY, 896.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Engine {
    pub spec: EngineSpec,
    pub ideal: Performance,
    pub eta_cstar: f64,
    /// Conical divergence factor.
    pub lambda: f64,
    pub mdot: f64,
    pub mdot_ox: f64,
    pub mdot_fuel: f64,
    pub throat_d: f64,
    pub exit_d: f64,
    pub area_ratio: f64,
    pub chamber_d: f64,
    pub contraction: f64,
    pub lstar: f64,
    pub chamber_cyl_len: f64,
    pub converging_len: f64,
    pub diverging_len: f64,
    pub injector: Injector,
    /// Ideal c* (m/s) against O/F at the design Pc, for engines whose O/F
    /// moves during the burn (self-pressurised N2O); empty otherwise.
    #[serde(default)]
    pub cstar_of: Vec<(f64, f64)>,
    /// Hot-gas wall stack in the cylindrical chamber (inside -> out).
    pub chamber_wall: Vec<Layer>,
    /// Stack at the throat.
    pub throat_wall: Vec<Layer>,
    /// Bartz gas-side coefficients, W/m^2 K.
    pub h_throat: f64,
    pub h_chamber: f64,
    pub t_aw_throat: f64,
    pub t_aw_chamber: f64,
    pub mass: f64,
}

impl Engine {
    pub fn length(&self) -> f64 {
        // Injector head 40 mm + chamber + nozzle.
        0.04 + self.chamber_cyl_len + self.converging_len + self.diverging_len
    }
    pub fn throat_area(&self) -> f64 {
        PI * self.throat_d.powi(2) / 4.0
    }
    pub fn exit_area(&self) -> f64 {
        PI * self.exit_d.powi(2) / 4.0
    }
    pub fn outer_d(&self) -> f64 {
        self.chamber_d + 2.0 * self.chamber_wall.iter().map(|l| l.thickness).sum::<f64>()
    }
    /// Delivered c*, m/s.
    pub fn c_star(&self) -> f64 {
        self.eta_cstar * self.ideal.c_star_m_s
    }
    /// Delivered thrust coefficient at ambient pressure `pa`.
    pub fn cf(&self, pa: f64) -> f64 {
        let ex = &self.ideal.exit;
        self.lambda * ex.velocity_m_s / self.ideal.c_star_m_s + (ex.p_pa - pa) * self.area_ratio / self.spec.pc
    }
    /// Thrust (N) at ambient pressure `pa`.
    pub fn thrust(&self, pa: f64) -> f64 {
        self.cf(pa) * self.spec.pc * self.throat_area()
    }
    /// Delivered Isp, m/s.
    pub fn isp(&self, pa: f64) -> f64 {
        self.thrust(pa) / self.mdot
    }
    pub fn pintle(&self) -> Option<&Pintle> {
        match &self.injector {
            Injector::Pintle(p) => Some(p),
            Injector::Triplet(_) => None,
        }
    }
    pub fn triplet(&self) -> Option<&Triplet> {
        match &self.injector {
            Injector::Triplet(d) => Some(d),
            Injector::Pintle(_) => None,
        }
    }
    /// Delivered c* at mixture ratio `of` (m/s): the design value scaled by
    /// the ideal c*(O/F) curve at the design Pc (Err when none was computed).
    pub fn c_star_at(&self, of: f64) -> anyhow::Result<f64> {
        let t = &self.cstar_of;
        anyhow::ensure!(t.len() >= 2, "engine has no c*(O/F) table");
        let of = of.clamp(t[0].0, t[t.len() - 1].0);
        let i = t.partition_point(|p| p.0 <= of).clamp(1, t.len() - 1);
        let (a, b) = (t[i - 1], t[i]);
        Ok(self.eta_cstar * (a.1 + (of - a.0) / (b.0 - a.0) * (b.1 - a.1)))
    }
    /// Vacuum thrust coefficient (delivered), the nozzle being fixed.
    pub fn cf_vac(&self) -> f64 {
        self.cf(0.0)
    }
}

/// Ideal c* against O/F at chamber pressure `pc`, 1.0-9.0 in 0.25 steps
/// (points where the equilibrium solve fails are left out). Cached per
/// propellant pair and Pc: the search rebuilds engines thousands of times.
pub fn cstar_table(ox: &Reactant, fuel: &Reactant, pc: f64) -> anyhow::Result<Vec<(f64, f64)>> {
    use std::collections::BTreeMap;
    use std::sync::{Mutex, OnceLock};
    type Table = BTreeMap<(String, String, u64), Vec<(f64, f64)>>;
    static CACHE: OnceLock<Mutex<Table>> = OnceLock::new();
    let key = (ox.id.clone(), fuel.id.clone(), pc.to_bits());
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(t) = cache.lock().map_err(|_| anyhow::anyhow!("c* cache poisoned"))?.get(&key) {
        return Ok(t.clone());
    }
    let mut t = Vec::new();
    for i in 0..=32 {
        let of = 1.0 + 0.25 * i as f64;
        let mix = Mix::bipropellant(ox, &[(fuel.clone(), 1.0)], of);
        if let Ok(p) = rocket(&mix, of, pc, Exit::Pressure(1e5), Flow::Shifting) {
            if p.c_star_m_s.is_finite() {
                t.push((of, p.c_star_m_s));
            }
        }
    }
    anyhow::ensure!(t.len() >= 2, "no c* points for {}/{}", ox.id, fuel.id);
    cache.lock().map_err(|_| anyhow::anyhow!("c* cache poisoned"))?.insert(key, t.clone());
    Ok(t)
}

/// Regulated fuel tank pressure for a self-pressurised N2O engine at design
/// Pc `pc`. Pc follows the N2O vapour pressure, so the fuel side is sized
/// for the hot end of the fill band: Pc_hot ~ pc Psat(hot)/Psat(nominal),
/// then the usual (1 + dP_design)/(1 - feed loss). An estimate for sizing
/// only; the checks fly the hot, nominal and cold blowdowns.
pub fn n2o_fuel_tank_pressure(pc: f64) -> anyhow::Result<f64> {
    let k = crate::nitrous::p_sat(rules::N2O_FILL_HOT_K)? / crate::nitrous::p_sat(rules::N2O_FILL_NOMINAL_K)?;
    Ok(pc * k * (1.0 + rules::INJ_DP_DESIGN) / (1.0 - rules::FEED_LOSS))
}

/// One operating point of a self-pressurised N2O engine.
#[derive(Debug, Clone, Copy)]
pub struct N2oPoint {
    pub pc: f64,
    pub mdot_ox: f64,
    pub mdot_f: f64,
}

/// Chamber pressure where the N2O flow (Dyer, from saturated liquid at
/// `t_tank`, inlet `p_tank_ox (1 - loss)`) plus the fuel flow (SPI from the
/// regulated `p_tank_f (1 - loss)`) equals what the throat passes at the
/// delivered c*(O/F): Pc At / c* = mdot. Bisection (the residual rises
/// monotonically with Pc).
pub fn n2o_operating_point(e: &Engine, t_tank: f64, p_tank_ox: f64, p_tank_f: f64, loss: f64) -> anyhow::Result<N2oPoint> {
    let d = e.triplet().ok_or_else(|| anyhow::anyhow!("not a self-pressurised (triplet) engine"))?;
    let fuel = reactant(&e.spec.fuel)?;
    let (pin_o, pin_f) = (p_tank_ox * (1.0 - loss), p_tank_f * (1.0 - loss));
    let (a_o, a_f, at) = (d.ox_area(), d.fuel_area(), e.throat_area());
    let flows = |pc: f64| -> anyhow::Result<(f64, f64)> {
        let mo = a_o * crate::nitrous::dyer_flux(t_tank, pin_o, pc, rules::N2O_ORIFICE_CD)?;
        let mf = rules::ORIFICE_CD * a_f * (2.0 * fuel.density_kg_m3 * (pin_f - pc).max(0.0)).sqrt();
        Ok((mo, mf))
    };
    let resid = |pc: f64| -> anyhow::Result<f64> {
        let (mo, mf) = flows(pc)?;
        let of = if mf > 0.0 { mo / mf } else { 1e3 };
        Ok(pc * at / e.c_star_at(of)? - (mo + mf))
    };
    let p_floor = crate::nitrous::sat(crate::nitrous::t_range().0)?.p * 1.001;
    let (mut lo, mut hi) = (p_floor, pin_o.min(pin_f));
    anyhow::ensure!(hi > lo && resid(lo)? < 0.0, "N2O feed cannot sustain combustion: injector inlets {:.1}/{:.1} bar", pin_o / 1e5, pin_f / 1e5);
    for _ in 0..32 {
        let mid = 0.5 * (lo + hi);
        if resid(mid)? < 0.0 { lo = mid } else { hi = mid }
    }
    let pc = 0.5 * (lo + hi);
    let (mdot_ox, mdot_f) = flows(pc)?;
    Ok(N2oPoint { pc, mdot_ox, mdot_f })
}

/// A burn sampled every `dt` seconds from ignition to liquid-oxidiser
/// depletion (the vapour-only tail is not flown: its thrust is neglected
/// and the vapour stays aboard as residual mass).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnProfile {
    pub fill_k: f64,
    pub dt: f64,
    pub t_end: f64,
    pub pc: Vec<f64>,
    pub mdot_ox: Vec<f64>,
    pub mdot_f: Vec<f64>,
    pub t_tank: Vec<f64>,
    pub p_tank: Vec<f64>,
    /// Cumulative mass used, kg, at each sample.
    pub ox_used: Vec<f64>,
    pub f_used: Vec<f64>,
}

impl BurnProfile {
    /// Linear interpolation of a sampled series at time `t` (clamped).
    pub fn at(&self, series: &[f64], t: f64) -> f64 {
        let n = series.len();
        let x = (t / self.dt).clamp(0.0, (n - 1) as f64);
        let i = (x.floor() as usize).min(n.saturating_sub(2));
        let w = x - i as f64;
        series[i] + w * (series[(i + 1).min(n - 1)] - series[i])
    }
}

/// Fly the self-pressurised N2O blowdown of `e` from a run tank filled with
/// `ox_mass` kg in `volume` m^3 at `fill_k`, fuel regulated at `p_tank_f`.
/// Memoised: the search rebuilds the same vehicle many times while it
/// sizes fins and bottles, and the blowdown is a pure function of these
/// inputs (keyed on their exact bits, so results are identical either way).
pub fn n2o_burn(e: &Engine, volume: f64, ox_mass: f64, fill_k: f64, p_tank_f: f64, loss: f64) -> anyhow::Result<BurnProfile> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Key = (String, String, Vec<u64>);
    static MEMO: OnceLock<Mutex<HashMap<Key, BurnProfile>>> = OnceLock::new();
    let t = e.triplet().ok_or_else(|| anyhow::anyhow!("not a self-pressurised (triplet) engine"))?;
    let nums = [e.throat_area(), t.ox_area(), t.fuel_area(), e.eta_cstar, e.spec.pc, volume, ox_mass, fill_k, p_tank_f, loss];
    let key: Key = (e.spec.oxidizer.clone(), e.spec.fuel.clone(), nums.iter().map(|x| x.to_bits()).collect());
    let memo = MEMO.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(p) = memo.lock().map_err(|_| anyhow::anyhow!("blowdown memo poisoned"))?.get(&key) {
        return Ok(p.clone());
    }
    let p = n2o_burn_uncached(e, volume, ox_mass, fill_k, p_tank_f, loss)?;
    let mut m = memo.lock().map_err(|_| anyhow::anyhow!("blowdown memo poisoned"))?;
    if m.len() > 20_000 {
        m.clear();
    }
    m.insert(key, p.clone());
    Ok(p)
}

fn n2o_burn_uncached(e: &Engine, volume: f64, ox_mass: f64, fill_k: f64, p_tank_f: f64, loss: f64) -> anyhow::Result<BurnProfile> {
    let dt = 0.2;
    let mut tank = crate::nitrous::RunTank::fill(volume, ox_mass, fill_k)?;
    let mut p = BurnProfile { fill_k, dt, t_end: 0.0, pc: vec![], mdot_ox: vec![], mdot_f: vec![], t_tank: vec![], p_tank: vec![], ox_used: vec![], f_used: vec![] };
    let (mut used_o, mut used_f) = (0.0, 0.0);
    for i in 0..1500 {
        let (s, x) = tank.state()?;
        if x >= 1.0 || tank.mass <= 0.0 {
            anyhow::ensure!(i > 0, "N2O tank holds no liquid at {:.1} C", fill_k - 273.15);
            // Close the series at burnout: the last flows held to t_end.
            for v in [&mut p.pc, &mut p.mdot_ox, &mut p.mdot_f, &mut p.t_tank, &mut p.p_tank] {
                let last = v[v.len() - 1];
                v.push(last);
            }
            p.ox_used.push(used_o);
            p.f_used.push(used_f);
            p.t_end = i as f64 * dt;
            return Ok(p);
        }
        let op = n2o_operating_point(e, s.t, s.p, p_tank_f, loss)?;
        p.pc.push(op.pc);
        p.mdot_ox.push(op.mdot_ox);
        p.mdot_f.push(op.mdot_f);
        p.t_tank.push(s.t);
        p.p_tank.push(s.p);
        p.ox_used.push(used_o);
        p.f_used.push(used_f);
        // Liquid left in the tank bounds this step's draw: the last step is
        // partial and ends the burn (without this the state solve can creep
        // toward x = 1 while flows were still being recorded).
        let liquid = tank.mass * (1.0 - x);
        if op.mdot_ox * dt >= liquid {
            let h = liquid / op.mdot_ox;
            used_o += liquid;
            used_f += op.mdot_f * h;
            for v in [&mut p.pc, &mut p.mdot_ox, &mut p.mdot_f, &mut p.t_tank, &mut p.p_tank] {
                let last = v[v.len() - 1];
                v.push(last);
            }
            p.ox_used.push(used_o);
            p.f_used.push(used_f);
            p.t_end = i as f64 * dt + h;
            return Ok(p);
        }
        tank.drain(op.mdot_ox * dt, s.h_l);
        used_o += op.mdot_ox * dt;
        used_f += op.mdot_f * dt;
    }
    anyhow::bail!("N2O blowdown did not empty in 300 s")
}

/// O/F that maximises ideal shifting Isp at `pc` expanding to `pa`
/// (scan 0.05 steps then refine). Uses the full equilibrium solver.
pub fn best_of(ox: &Reactant, fuel: &Reactant, pc: f64, pa: f64, lo: f64, hi: f64) -> anyhow::Result<f64> {
    let isp = |of: f64| -> anyhow::Result<f64> {
        let mix = Mix::bipropellant(ox, &[(fuel.clone(), 1.0)], of);
        Ok(rocket(&mix, of, pc, Exit::Pressure(pa), Flow::Shifting)?.isp_opt_m_s)
    };
    let mut best = (lo, f64::MIN);
    let mut of = lo;
    while of <= hi + 1e-9 {
        let v = isp(of)?;
        if v > best.1 {
            best = (of, v);
        }
        of += 0.1;
    }
    Ok((best.0 * 100.0).round() / 100.0)
}

/// Bartz (1957) convective coefficient at local area ratio `a_ratio`
/// (A/At) and Mach `m`: Huzel & Huang SP-125 eqs 4-13 (h_g), 4-14 (sigma),
/// 4-15 (Pr = 4g/(9g-5)), 4-16 (mu = 46.6e-10 M^0.5 T^0.6 lb/in-s, T in R),
/// pp.100-101, in SI (pc g/c* -> pc/c* in kg/m^2 s).
#[allow(clippy::too_many_arguments)]
pub fn bartz(pc: f64, c_star: f64, dt: f64, rc_throat: f64, t0: f64, molar_mass: f64, gamma: f64, cp: f64, a_ratio: f64, m: f64, tw: f64) -> f64 {
    // Bartz viscosity mu = 46.6e-10 M^0.5 T^0.6 [lb/(in s), M lb/lbmol, T in R].
    let mu = 46.6e-10 * (molar_mass * 1e3).powf(0.5) * (t0 * 1.8).powf(0.6) * 17.858;
    let pr = 4.0 * gamma / (9.0 * gamma - 5.0);
    let k = 1.0 + (gamma - 1.0) / 2.0 * m * m;
    let sigma = 1.0 / ((0.5 * tw / t0 * k + 0.5).powf(0.68) * k.powf(0.12));
    0.026 / dt.powf(0.2) * (mu.powf(0.2) * cp / pr.powf(0.6)) * (pc / c_star).powf(0.8) * (dt / rc_throat).powf(0.1)
        * (1.0 / a_ratio).powf(0.9)
        * sigma
}

/// Adiabatic wall temperature with turbulent recovery factor Pr^(1/3).
pub fn t_aw(t0: f64, gamma: f64, m: f64) -> f64 {
    let pr = 4.0 * gamma / (9.0 * gamma - 5.0);
    let r = pr.powf(1.0 / 3.0);
    let k = (gamma - 1.0) / 2.0 * m * m;
    t0 * (1.0 + r * k) / (1.0 + k)
}

/// Transient 1-D conduction through `layers` (hot side first) for `t_end`
/// seconds: convection h to `t_gas` on the hot face, adiabatic outer face
/// (conservative: no soak-out). Explicit finite differences (FTCS) with a
/// stable step. Returns the temperature at every node and node positions.
pub fn wall_transient(layers: &[Layer], h: f64, t_gas: f64, t_init: f64, t_end: f64) -> (Vec<f64>, Vec<f64>) {
    // Nodes: ~0.25 mm cells, at least 4 per layer.
    let mut xs = Vec::new();
    let mut props = Vec::new();
    let mut x = 0.0;
    for l in layers {
        let n = ((l.thickness / 0.25e-3).ceil() as usize).max(4);
        let dx = l.thickness / n as f64;
        for _ in 0..n {
            xs.push(x + dx / 2.0);
            props.push((l.material.props(), dx));
            x += dx;
        }
    }
    let n = xs.len();
    let mut t = vec![t_init; n];
    // Stable step: dt < 0.5 min(rho cp dx^2 / k), and the convective cell.
    let mut dt = f64::MAX;
    for &((k, rho, cp), dx) in &props {
        dt = dt.min(0.4 * rho * cp * dx * dx / k);
        dt = dt.min(0.4 * rho * cp * dx / (h + 2.0 * k / dx));
    }
    let steps = (t_end / dt).ceil() as usize;
    let dt = t_end / steps as f64;
    let mut q = vec![0.0; n + 1];
    for _ in 0..steps {
        // Interface fluxes (series resistance between cell centres).
        let ((k0, _, _), dx0) = props[0];
        q[0] = (t_gas - t[0]) / (1.0 / h + dx0 / (2.0 * k0));
        for i in 1..n {
            let ((ka, _, _), dxa) = props[i - 1];
            let ((kb, _, _), dxb) = props[i];
            q[i] = (t[i - 1] - t[i]) / (dxa / (2.0 * ka) + dxb / (2.0 * kb));
        }
        q[n] = 0.0;
        for i in 0..n {
            let ((_, rho, cp), dx) = props[i];
            t[i] += dt * (q[i] - q[i + 1]) / (rho * cp * dx);
        }
    }
    (xs, t)
}

/// Depth into the stack where phenolic reached its pyrolysis temperature
/// (the char front), m; and the hottest aluminium temperature, K.
pub fn wall_result(layers: &[Layer], xs: &[f64], t: &[f64]) -> (f64, f64) {
    let mut char_depth: f64 = 0.0;
    let mut al_max: f64 = 0.0;
    let mut x0 = 0.0;
    let mut bounds = Vec::new();
    for l in layers {
        bounds.push((x0, x0 + l.thickness, l.material));
        x0 += l.thickness;
    }
    for (x, temp) in xs.iter().zip(t) {
        let mat = bounds.iter().find(|b| *x >= b.0 && *x < b.1).map(|b| b.2).unwrap_or(Material::Al6061);
        if mat == Material::Phenolic && *temp >= rules::PHENOLIC_PYROLYSIS_K {
            char_depth = char_depth.max(*x);
        }
        if mat == Material::Al6061 {
            al_max = al_max.max(*temp);
        }
    }
    (char_depth, al_max)
}

/// Inverse complementary error function by bisection (0 < y < 2).
fn erfc_inv(y: f64) -> f64 {
    let erfc = |x: f64| -> f64 {
        // Abramowitz & Stegun 7.1.26, |err| < 1.5e-7.
        let s = x.signum();
        let x = x.abs();
        let t = 1.0 / (1.0 + 0.327_591_1 * x);
        let p = t * (0.254_829_592 + t * (-0.284_496_736 + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
        let e = 1.0 - p * (-x * x).exp();
        1.0 - s * e
    };
    let (mut lo, mut hi) = (-5.0, 5.0);
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if erfc(mid) > y { lo = mid } else { hi = mid }
    }
    0.5 * (lo + hi)
}

/// Size the O-F-O triplet at the nominal point: N2O saturated at
/// `N2O_FILL_NOMINAL_K` entering at Psat (1 - feed loss), fuel from the
/// regulated tank (`n2o_fuel_tank_pressure`), both into the design Pc.
fn design_triplet(e: &Engine, _ox: &Reactant, fuel: &Reactant) -> anyhow::Result<Triplet> {
    let pc = e.spec.pc;
    let t0 = rules::N2O_FILL_NOMINAL_K;
    let s0 = crate::nitrous::sat(t0)?;
    let pin_o = s0.p * (1.0 - rules::FEED_LOSS);
    let p_f = n2o_fuel_tank_pressure(pc)?;
    let pin_f = p_f * (1.0 - rules::FEED_LOSS);
    anyhow::ensure!(pin_o > pc && pin_f > pc, "Pc {:.1} bar is above the N2O injector inlet {:.1} bar at {:.0} C", pc / 1e5, pin_o / 1e5, t0 - 273.15);
    let g_o = crate::nitrous::dyer_flux(t0, pin_o, pc, rules::N2O_ORIFICE_CD)?;
    let a_o = e.mdot_ox / g_o;
    let a_f = e.mdot_fuel / (rules::ORIFICE_CD * (2.0 * fuel.density_kg_m3 * (pin_f - pc)).sqrt());
    // Elements: fewest (from 3) whose N2O orifices are within the Dyer
    // model's validated size; every hole drilled to 0.1 mm.
    let mut n = 3usize;
    while (4.0 * a_o / (2.0 * PI * n as f64)).sqrt() > rules::N2O_HOLE_MAX_M && n < 200 {
        n += 1;
    }
    let drill = |a: f64, holes: f64| ((4.0 * a / (PI * holes)).sqrt() * 1e4).round().max(1.0) / 1e4;
    Ok(Triplet {
        elements: n,
        ox_hole_d: drill(a_o, 2.0 * n as f64),
        fuel_hole_d: drill(a_f, n as f64),
        ox_angle_deg: rules::IMPINGEMENT_ANGLE_DEG,
        dp_ox: pin_o - pc,
        dp_fuel: pin_f - pc,
        fuel_tank_p: p_f,
        fill_k: t0,
    })
}

pub fn design(spec: &EngineSpec) -> anyhow::Result<Engine> {
    let ox = reactant(&spec.oxidizer)?;
    let fuel = reactant(&spec.fuel)?;
    let mix = Mix::bipropellant(&ox, &[(fuel.clone(), 1.0)], spec.of);
    let ideal = rocket(&mix, spec.of, spec.pc, Exit::Pressure(spec.pa_design), Flow::Shifting)?;
    let lambda = (1.0 + rules::NOZZLE_HALF_ANGLE_DEG.to_radians().cos()) / 2.0;
    let eta = spec.eta_cstar.unwrap_or(rules::ETA_CSTAR);
    anyhow::ensure!(eta > 0.0 && eta <= 1.0, "c* efficiency {eta} outside (0, 1]");
    let mut e = Engine {
        spec: spec.clone(),
        area_ratio: ideal.area_ratio,
        ideal,
        eta_cstar: eta,
        lambda,
        mdot: 0.0,
        mdot_ox: 0.0,
        mdot_fuel: 0.0,
        throat_d: 0.0,
        exit_d: 0.0,
        chamber_d: 0.0,
        contraction: 0.0,
        lstar: rules::LSTAR_DESIGN_M,
        chamber_cyl_len: 0.0,
        converging_len: 0.0,
        diverging_len: 0.0,
        injector: Injector::Pintle(Pintle { pintle_d: 0.0, ox_holes: 0, ox_hole_d: 0.0, fuel_gap: 0.0, dp_ox: 0.0, dp_fuel: 0.0, v_ox: 0.0, v_fuel: 0.0, tmr: 0.0 }),
        cstar_of: vec![],
        chamber_wall: vec![],
        throat_wall: vec![],
        h_throat: 0.0,
        h_chamber: 0.0,
        t_aw_throat: 0.0,
        t_aw_chamber: 0.0,
        mass: 0.0,
    };
    // Throat from F = Cf pc At at the design ambient.
    let at = spec.thrust / (e.cf(spec.pa_design) * spec.pc);
    e.throat_d = (4.0 * at / PI).sqrt();
    e.exit_d = e.throat_d * e.area_ratio.sqrt();
    e.mdot = spec.pc * at / e.c_star();
    e.mdot_ox = e.mdot * spec.of / (1.0 + spec.of);
    e.mdot_fuel = e.mdot / (1.0 + spec.of);
    // Chamber: contraction ratio correlation, L* volume.
    e.contraction = spec.contraction.unwrap_or_else(|| rules::contraction_ratio(e.throat_d));
    anyhow::ensure!(e.contraction >= rules::CONTRACTION_MIN && e.contraction.is_finite(), "contraction ratio {:.2} below {} (Huzel & Huang's range for pressure-fed engines is 2-5)", e.contraction, rules::CONTRACTION_MIN);
    let ac = e.contraction * at;
    e.chamber_d = (4.0 * ac / PI).sqrt();
    let conv = rules::CONV_HALF_ANGLE_DEG.to_radians();
    let (rc, rt) = (e.chamber_d / 2.0, e.throat_d / 2.0);
    e.converging_len = (rc - rt) / conv.tan();
    let v_conv = PI / 3.0 * e.converging_len * (rc * rc + rc * rt + rt * rt);
    let v_c = e.lstar * at;
    e.chamber_cyl_len = ((v_c - v_conv) / ac).max(0.5 * e.chamber_d);
    e.diverging_len = (e.exit_d / 2.0 - rt) / rules::NOZZLE_HALF_ANGLE_DEG.to_radians().tan();

    e.injector = if ox.self_pressurizing {
        anyhow::ensure!(!fuel.cryogenic, "{} with cryogenic {} is not modelled: a room-temperature N2O run tank beside a cryogenic fuel tank (N2O freezes at 182 K) has no student precedent Wright could find", ox.id, fuel.id);
        e.cstar_of = cstar_table(&ox, &fuel, spec.pc)?;
        Injector::Triplet(design_triplet(&e, &ox, &fuel)?)
    } else {
        // Pintle injector: LOX radially out of the pintle tip, fuel axial through the annulus.
        let dp = spec.inj_dp.unwrap_or(rules::INJ_DP_DESIGN) * spec.pc;
        let a_ox = e.mdot_ox / (rules::ORIFICE_CD * (2.0 * ox.density_kg_m3 * dp).sqrt());
        let a_f = e.mdot_fuel / (rules::ORIFICE_CD * (2.0 * fuel.density_kg_m3 * dp).sqrt());
        // Pintle diameter ~ chamber/4 (TRW practice Dc/Dp 3-5, Dressler & Bauer AIAA 2000-3871).
        let pintle_d = e.chamber_d / 4.0;
        // Ox holes: 1.0 mm minimum drill, as many as fit at 2x pitch around the tip.
        let max_holes = ((PI * pintle_d) / (2.0 * 1.2e-3)).floor() as usize;
        let mut n = 8usize;
        let mut d = (4.0 * a_ox / (PI * n as f64)).sqrt();
        while d > 2.0e-3 && n < max_holes {
            n += 2;
            d = (4.0 * a_ox / (PI * n as f64)).sqrt();
        }
        // Drill to the nearest 0.1 mm (what a shop does).
        let d_drill = (d * 1e4).round() / 1e4;
        // Annulus gap: A = pi (Dp + g) g.
        let gap = (-pintle_d + (pintle_d * pintle_d + 4.0 * a_f / PI).sqrt()) / 2.0;
        let v_ox = rules::ORIFICE_CD * (2.0 * dp / ox.density_kg_m3).sqrt();
        let v_f = rules::ORIFICE_CD * (2.0 * dp / fuel.density_kg_m3).sqrt();
        Injector::Pintle(Pintle {
            pintle_d,
            ox_holes: n,
            ox_hole_d: d_drill,
            fuel_gap: gap,
            dp_ox: dp,
            dp_fuel: dp,
            v_ox,
            v_fuel: v_f,
            tmr: (e.mdot_ox * v_ox) / (e.mdot_fuel * v_f),
        })
    };

    // Gas-side heat transfer (Bartz) at the throat and the chamber.
    let g = e.ideal.chamber.gamma_s.unwrap_or(f64::NAN);
    let mw = e.ideal.chamber.molar_mass_kg_mol;
    let cp = e.ideal.chamber.cp_frozen_j_kg_k;
    let t0 = e.ideal.chamber.t_k;
    let rc_curv = 1.5 * rt;
    e.t_aw_throat = t_aw(t0, g, 1.0);
    // Chamber Mach from the subsonic area-Mach relation at Ac/At.
    let mut mc = 0.01;
    for _ in 0..200 {
        let f = crate::ideal::area_ratio_from_mach(mc, g) - e.contraction;
        let df = (crate::ideal::area_ratio_from_mach(mc + 1e-6, g) - crate::ideal::area_ratio_from_mach(mc, g)) / 1e-6;
        mc = (mc - f / df).clamp(1e-4, 0.99);
    }
    e.t_aw_chamber = t_aw(t0, g, mc);
    let tw_guess = 1000.0;
    e.h_throat = bartz(spec.pc, e.c_star(), e.throat_d, rc_curv, t0, mw, g, cp, 1.0, 1.0, tw_guess);
    e.h_chamber = bartz(spec.pc, e.c_star(), e.throat_d, rc_curv, t0, mw, g, cp, e.contraction, mc, tw_guess);

    // Liner sizing: semi-infinite char depth for a surface held at the char
    // temperature (Incropera eq 5.57), times the ablative SF, plus 1 mm.
    let (k, rho, cpp) = Material::Phenolic.props();
    let alpha = k / (rho * cpp);
    let theta = (rules::PHENOLIC_PYROLYSIS_K - 293.0) / (rules::PHENOLIC_SURFACE_K - 293.0);
    let x_char = 2.0 * (alpha * spec.burn_time).sqrt() * erfc_inv(theta);
    let liner = (x_char * rules::ABLATIVE_SF + 1.0e-3).max(3.0e-3);
    // Case: thin-wall hoop at burst SF (metallic chamber) on ultimate.
    let r_case = rc + liner;
    let t_case = (rules::FAR_BURST_SF_METAL * spec.pc * r_case / rules::AL6061_ULT_PA).max(3.0e-3);
    let t_case = (t_case * 1e3).ceil() / 1e3;
    e.chamber_wall = vec![
        Layer { material: Material::Phenolic, thickness: (liner * 1e3).ceil() / 1e3 },
        Layer { material: Material::Al6061, thickness: t_case },
    ];
    // Throat: phenolic (SEB also ran phenolic nozzles; no graphite stock has
    // a published price in the catalog), 1.5x the chamber liner because the
    // throat sees the highest Bartz coefficient.
    e.throat_wall = vec![
        Layer { material: Material::Phenolic, thickness: ((1.5 * liner) * 1e3).ceil() / 1e3 },
        Layer { material: Material::Al6061, thickness: t_case },
    ];
    // Mass: case shell over chamber+converging, liner, graphite, injector head, nozzle extension (phenolic).
    let shell = |d_in: f64, t: f64, len: f64, rho: f64| PI * (d_in + t) * t * len * rho;
    let len_c = e.chamber_cyl_len + e.converging_len;
    let liner_t = e.chamber_wall[0].thickness;
    let m_liner = shell(e.chamber_d, liner_t, len_c, rules::PHENOLIC_DENSITY);
    let m_case = shell(e.chamber_d + 2.0 * liner_t, t_case, len_c + 0.02, rules::AL6061_DENSITY);
    let m_graphite = shell(e.throat_d, e.throat_wall[0].thickness - liner_t, 3.0 * e.throat_d, rules::PHENOLIC_DENSITY);
    let mean_div_d = (e.throat_d + e.exit_d) / 2.0;
    let m_div = shell(mean_div_d, 6e-3, e.diverging_len, rules::PHENOLIC_DENSITY);
    let head_d = e.outer_d();
    let m_head = PI * head_d * head_d / 4.0 * 0.03 * rules::AL6061_DENSITY * 0.6;
    e.mass = m_liner + m_case + m_graphite + m_div + m_head;
    Ok(e)
}

/// Injector stiffness at a tank pressure `p_tank` with the drilled ox
/// holes: solve the chamber pressure that balances flow through orifices
/// and throat (Pc = mdot c* / At) by fixed point; returns (pc, dp_ox/pc,
/// dp_fuel/pc). Feed losses are a fraction `loss` of tank pressure.
pub fn operating_point(e: &Engine, p_tank_ox: f64, p_tank_f: f64, loss: f64) -> anyhow::Result<(f64, f64, f64)> {
    let ox = reactant(&e.spec.oxidizer)?;
    let fuel = reactant(&e.spec.fuel)?;
    let pin = e.pintle().ok_or_else(|| anyhow::anyhow!("operating_point is the regulated pintle model; use n2o_operating_point"))?;
    let a_ox = pin.ox_holes as f64 * PI * pin.ox_hole_d.powi(2) / 4.0;
    let a_f = PI * (pin.pintle_d + pin.fuel_gap) * pin.fuel_gap;
    let at = e.throat_area();
    let mut pc = e.spec.pc;
    for _ in 0..200 {
        let pin_o = p_tank_ox * (1.0 - loss);
        let pin_f = p_tank_f * (1.0 - loss);
        let mo = rules::ORIFICE_CD * a_ox * (2.0 * ox.density_kg_m3 * (pin_o - pc).max(0.0)).sqrt();
        let mf = rules::ORIFICE_CD * a_f * (2.0 * fuel.density_kg_m3 * (pin_f - pc).max(0.0)).sqrt();
        let pc_new = (mo + mf) * e.c_star() / at;
        if (pc_new - pc).abs() < 1.0 {
            pc = pc_new;
            break;
        }
        pc = 0.5 * pc + 0.5 * pc_new;
    }
    let pin_o = p_tank_ox * (1.0 - loss);
    let pin_f = p_tank_f * (1.0 - loss);
    Ok((pc, (pin_o - pc) / pc, (pin_f - pc) / pc))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn lightbulb() -> EngineSpec {
        EngineSpec { oxidizer: "lox".into(), fuel: "propane".into(), pc: 24e5, of: 2.33, thrust: 3000.0, pa_design: 94_000.0, burn_time: 14.0, eta_cstar: None, inj_dp: None, contraction: None }
    }

    #[test]
    fn lightbulb_class_engine_is_sane() {
        let e = design(&lightbulb()).unwrap();
        // Throat ~ 30-45 mm for 3 kN at 24 bar.
        assert!(e.throat_d > 0.025 && e.throat_d < 0.05, "{}", e.throat_d);
        assert!((e.thrust(94_000.0) - 3000.0).abs() < 1.0);
        assert!(e.mdot > 1.0 && e.mdot < 1.6, "{}", e.mdot);
        let tmr = e.pintle().unwrap().tmr;
        assert!(tmr > 0.3 && tmr < 5.0, "{tmr}");
        // Bartz throat h for ~24 bar LOX/HC: order 3-8 kW/m^2K.
        assert!(e.h_throat > 2000.0 && e.h_throat < 12000.0, "{}", e.h_throat);
    }

    #[test]
    fn operating_point_recovers_design_pc() {
        let e = design(&lightbulb()).unwrap();
        let ptank = e.spec.pc * (1.0 + rules::INJ_DP_DESIGN) / 0.9;
        let (pc, so, sf) = operating_point(&e, ptank, ptank, 0.1).unwrap();
        // Drill rounding moves Pc a little.
        assert!((pc - e.spec.pc).abs() / e.spec.pc < 0.08, "{pc}");
        assert!(so > 0.1 && sf > 0.1);
    }

    #[test]
    fn transient_heats_the_hot_face_first() {
        let layers = vec![
            Layer { material: Material::Phenolic, thickness: 8e-3 },
            Layer { material: Material::Al6061, thickness: 4e-3 },
        ];
        let (xs, t) = wall_transient(&layers, 2000.0, 3000.0, 293.0, 14.0);
        assert!(t[0] > t[t.len() - 1]);
        let (char_d, al) = wall_result(&layers, &xs, &t);
        assert!(char_d > 0.5e-3 && char_d < 8e-3, "{char_d}");
        assert!(al < 400.0, "{al}");
    }
}
