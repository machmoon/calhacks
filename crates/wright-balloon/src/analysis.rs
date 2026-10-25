//! Thermal, power, links and recovery over one simulated flight. Each
//! number here is what a check reads; `checks.rs` only compares.

use crate::atmosphere;
use crate::catalog::{Catalog, Chemistry, LinkKind, Part};
use crate::flight::{self, Flight, Phase};
use crate::geometry::{self, Layout};
use crate::mass::{self, Masses};
use crate::model::{Design, Spec, TrackerClass};
use crate::radio;
use crate::rules;
use anyhow::{Context, Result};
use serde::Serialize;
use wright_core::units::Length;

#[derive(Debug, Clone, Serialize)]
pub struct BusEnergy {
    pub name: String,
    pub need_wh: f64,
    pub have_wh: f64,
    /// Coldest cell temperature while the bus is on, C.
    pub cell_min_c: f64,
    pub capacity_fraction: f64,
    /// Hours the bus lasts at its flight load.
    pub flight_endurance_h: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackerLink {
    pub name: String,
    pub class: TrackerClass,
    pub link: Option<LinkKind>,
    /// Worst margin over every flight sample above 1 km AGL, dB (None: no ground station).
    pub worst_margin_db: Option<f64>,
    pub worst_at_range_km: f64,
    pub max_range_km: f64,
    /// Lowest altitude AGL on the descent at which the link still closed, m.
    pub last_heard_agl_m: Option<f64>,
    pub works_through_window: bool,
    pub p_recover: f64,
    pub why: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Coexist {
    pub gps: String,
    pub tx: String,
    pub separation_m: f64,
    pub at_gnss_dbm: f64,
    pub limit_dbm: f64,
    pub mechanism: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub masses: Masses,
    pub flight: Flight,
    pub layout: Layout,
    pub box_min_c: f64,
    pub box_max_c: f64,
    pub ambient_min_c: f64,
    pub buses: Vec<BusEnergy>,
    /// Self-powered parts: (part, need Wh, have Wh).
    pub self_powered: Vec<BusEnergy>,
    pub trackers: Vec<TrackerLink>,
    pub coexistence: Vec<Coexist>,
    pub gas_needed_m3: f64,
    pub gas_have_m3: f64,
    pub landing_km: f64,
    pub recovery_probability: f64,
    /// The declared launch range the thermal numbers cover, C.
    pub launch_range_c: [f64; 2],
    /// Each ground temperature the thermal model ran at.
    pub thermal_cases: Vec<ThermalCase>,
}

/// Capacity fraction at a cell temperature, linear between table points.
///
/// LiPo: ASSUMPTION built from typical hobby LiPo data sheets (discharge
/// rated to -20 C; ~50 % at -20 C; none usable at -40 C). Lithium primary
/// (Energizer L91): Energizer L91 product data sheet, capacity vs
/// temperature curve.
pub fn capacity_fraction(chem: Chemistry, t_c: f64) -> f64 {
    let tab = match chem {
        Chemistry::Lipo | Chemistry::LiIon => rules::LIPO_CAPACITY,
        Chemistry::LithiumPrimary => rules::L91_CAPACITY,
    };
    if t_c <= tab[0].0 {
        return tab[0].1;
    }
    for w in tab.windows(2) {
        if t_c <= w[1].0 {
            let f = (t_c - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + f * (w[1].1 - w[0].1);
        }
    }
    tab[tab.len() - 1].1
}

fn usable(chem: Chemistry) -> f64 {
    match chem {
        Chemistry::Lipo | Chemistry::LiIon => rules::LIPO_USABLE,
        Chemistry::LithiumPrimary => 1.0,
    }
}

/// One ground-temperature case of the box thermal and energy model.
#[derive(Debug, Clone, Serialize)]
pub struct ThermalCase {
    pub ground_c: f64,
    /// Whether the design's hand warmers are opened at this temperature.
    pub warmers_on: bool,
    pub box_min_c: f64,
    pub box_max_c: f64,
    /// Heat the warmers gave over flight + window, Wh.
    pub warmer_wh: f64,
    pub buses: Vec<BusEnergy>,
    pub self_powered: Vec<BusEnergy>,
}

impl ThermalCase {
    /// Every part inside its rating, every battery covers its need and a
    /// camera meeting the mission records to burst: the thermal checks.
    pub fn passes(&self, spec: &Spec, d: &Design, cat: &Catalog, burst_t_s: f64) -> bool {
        let temps_ok = rated_parts(d).iter().all(|id| cat.part(id).is_ok_and(|p| self.box_min_c >= p.temp_min_c && self.box_max_c <= p.temp_max_c));
        let energy_ok = self.buses.iter().chain(&self.self_powered).all(|b| b.have_wh >= b.need_wh);
        let to_burst_h = burst_t_s / 3600.0;
        let imaging_ok = d.items.iter().any(|it| {
            let Ok(p) = cat.part(&it.part) else { return false };
            let Some(cam) = &p.camera else { return false };
            if (spec.needs_video && !cam.video) || (spec.needs_photos && !cam.photo && !cam.video) {
                return false;
            }
            let e = if p.self_powered {
                self.self_powered.iter().find(|b| b.name == p.id).map(|b| b.flight_endurance_h)
            } else {
                it.bus.and_then(|bi| self.buses.get(bi)).map(|b| b.flight_endurance_h)
            };
            e.is_some_and(|e| e >= to_burst_h)
        });
        temps_ok && energy_ok && imaging_ok
    }
}

/// Every part id the temperature check covers (items, batteries, rails).
pub fn rated_parts(d: &Design) -> Vec<&str> {
    let mut parts: Vec<&str> = d.items.iter().map(|i| i.part.as_str()).collect();
    parts.extend(d.buses.iter().map(|b| b.battery.as_str()));
    parts.extend(d.buses.iter().flat_map(|b| b.rails.iter().map(|x| x.as_str())));
    parts
}

/// The declared launch range [low, high], C; an error if it is not a range.
pub fn launch_range(spec: &Spec) -> Result<[f64; 2]> {
    let r = spec.launch_temp_range_c.unwrap_or(rules::LAUNCH_TEMP_RANGE_C);
    anyhow::ensure!(r[0].is_finite() && r[1].is_finite() && r[0] <= r[1], "launch_temp_range_c {:?} is not a range [low, high]", r);
    Ok(r)
}

/// Whether a forecast ground temperature is one the design may launch at:
/// inside the spec's declared range or inside the design's declared
/// launch window. Outside both it is a no-go day, not a design case.
pub fn forecast_is_launch_day(spec: &Spec, d: &Design, g: f64) -> Result<bool> {
    let [lo, hi] = launch_range(spec)?;
    Ok((lo..=hi).contains(&g) || d.launch_window_c.is_some_and(|[a, b]| (a..=b).contains(&g)))
}

/// The ground temperatures the thermal checks are evaluated at: both ends
/// of the declared range, the forecast when it is a launch day (see
/// `forecast_is_launch_day`), and both sides of the warmer switch-over
/// when that lies inside the range.
pub fn thermal_points(spec: &Spec, d: &Design) -> Result<Vec<f64>> {
    let [lo, hi] = launch_range(spec)?;
    let mut v = vec![lo, hi];
    if let Some(g) = spec.ground_temperature_c {
        if forecast_is_launch_day(spec, d, g)? {
            v.push(g);
        }
    }
    if let Some(s) = d.warmers_below_c.filter(|s| *s > lo && *s <= hi) {
        // just below (warmers opened) and at (not opened) the switch
        v.push(s - rules::WINDOW_TOL_C);
        v.push(s);
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.dedup();
    Ok(v)
}

/// One step of the box's environment over the flight and window.
struct Env {
    dt: f64,
    /// Air temperature, C (None: on the ground, the case's ground temperature).
    amb_c: Option<f64>,
    rho: f64,
    p_ratio: f64,
    landed: bool,
    descending: bool,
    flying: bool,
}

/// One item's electrical load, resolved from the catalog once (the same
/// rules as `loads_at`).
struct ItemLoad {
    watts: f64,
    self_powered: bool,
    /// Battery-side bus and the rail efficiency the load is divided by.
    bus: Option<(usize, f64)>,
    thermostat_c: Option<f64>,
    on_after_landing: bool,
    until_burst: bool,
}

/// One forward-Euler step of the payload box's lumped capacitance,
/// `T' = T + (P - (T - T_amb) / R_th) dt / C`: the update inside
/// `ThermalModel::case`, on bare numbers. Only the difference `T - T_amb`
/// enters, so the temperatures may be in C or K (`case` carries C).
pub fn box_step(heat_cap_j_per_k: f64, r_th_k_per_w: f64, p_w: f64, t_amb_k: f64, dt_s: f64, t_k: f64) -> f64 {
    t_k + (p_w - (t_k - t_amb_k) / r_th_k_per_w) * dt_s / heat_cap_j_per_k
}

/// The precomputed inputs every thermal case shares.
pub struct ThermalModel<'a> {
    d: &'a Design,
    cat: &'a Catalog,
    env: Vec<Env>,
    loads: Vec<ItemLoad>,
    r_cond: f64,
    a_out: f64,
    heat_cap: f64,
    /// Sea-level heat of all warmers at full activation, W, and their spec.
    warmers: Option<(f64, f64, f64)>,
    burst_t_s: f64,
}

impl<'a> ThermalModel<'a> {
    pub fn new(d: &'a Design, cat: &'a Catalog, masses: &Masses, fl: &Flight) -> Result<Self> {
        let ins = cat.insulation(&d.enclosure.insulation)?;
        let o = mass::outer_mm(d);
        let i = d.enclosure.inner_mm;
        let area = |s: [f64; 3]| 2.0 * (s[0] * s[1] + s[1] * s[2] + s[0] * s[2]) * 1e-6;
        let (a_out, a_in) = (area(o), area(i));
        let a_cond = (a_out * a_in).sqrt();
        let r_cond = d.enclosure.wall_mm * 1e-3 / (ins.k_w_mk * a_cond);
        let heat_cap = (masses.package().as_kg() - masses.foam_g * 1e-3) * rules::CP_PAYLOAD;
        let rho0 = rules::US76_P0_PA / (rules::US76_R_AIR * rules::US76_T0_K);
        // powered up and sealed on the ground before launch
        let mut env = vec![Env { dt: rules::PRELAUNCH_MIN * 60.0, amb_c: None, rho: rho0, p_ratio: 1.0, landed: false, descending: false, flying: false }];
        let mut prev_t = 0.0;
        for s in &fl.samples {
            let dt = s.t_s - prev_t;
            prev_t = s.t_s;
            let landed = s.phase == Phase::Landed;
            if landed {
                env.push(Env { dt, amb_c: None, rho: rho0, p_ratio: 1.0, landed, descending: false, flying: false });
            } else {
                let a = atmosphere::at(Length::m(s.z_m));
                env.push(Env {
                    dt,
                    amb_c: Some(a.temperature.as_celsius()),
                    rho: a.density.as_kg_m3(),
                    p_ratio: a.pressure.as_pa() / rules::US76_P0_PA,
                    landed,
                    descending: s.phase == Phase::Descent,
                    flying: s.phase == Phase::Ascent,
                });
            }
        }
        let mut warmers: Option<(f64, f64, f64)> = None;
        for it in &d.items {
            let p = cat.part(&it.part)?;
            if let Some(w) = &p.warmer {
                let (sum, h, act) = warmers.unwrap_or((0.0, w.rated_h, w.activation_min));
                warmers = Some((sum + rules::warmer_power_sl_w(w.rated_h) * it.qty as f64, h.min(w.rated_h), act.max(w.activation_min)));
            }
        }
        let mut loads = Vec::with_capacity(d.items.len());
        for it in &d.items {
            let part = cat.part(&it.part)?;
            let bus = match it.bus.filter(|b| *b < d.buses.len()) {
                Some(b) if !part.self_powered => {
                    let eff = match it.rail.and_then(|r| d.buses[b].rails.get(r)) {
                        Some(r) => cat.part(r)?.regulator.as_ref().map(|r| r.efficiency).unwrap_or(1.0),
                        None => 1.0,
                    };
                    Some((b, eff))
                }
                _ => None,
            };
            loads.push(ItemLoad { watts: part.power_w * it.qty as f64, self_powered: part.self_powered, bus, thermostat_c: part.thermostat_c, on_after_landing: it.on_after_landing, until_burst: it.until_burst });
        }
        Ok(ThermalModel { d, cat, env, loads, r_cond, a_out, heat_cap, warmers, burst_t_s: fl.burst_t_s })
    }

    /// `loads_at` over the resolved loads, into reused buffers.
    fn loads(&self, landed: bool, descending: bool, box_c: f64, bus: &mut [f64], own: &mut [f64]) {
        bus.iter_mut().for_each(|x| *x = 0.0);
        own.iter_mut().for_each(|x| *x = 0.0);
        for (k, l) in self.loads.iter().enumerate() {
            if (landed && !l.on_after_landing) || ((landed || descending) && l.until_burst) {
                continue;
            }
            if l.thermostat_c.is_some_and(|set| box_c >= set) {
                continue;
            }
            if l.self_powered {
                own[k] = l.watts;
            } else if let Some((b, eff)) = l.bus {
                bus[b] += l.watts / eff;
            }
        }
    }

    /// Whether the design opens its warmers on a day this cold.
    pub fn warmers_on(&self, ground_c: f64) -> bool {
        self.warmers.is_some() && self.d.warmers_below_c.is_none_or(|s| ground_c < s)
    }

    /// The box over prelaunch, flight and the recovery window on a day
    /// with this ground temperature.
    pub fn case(&self, ground_c: f64) -> Result<ThermalCase> {
        let (d, cat) = (self.d, self.cat);
        let rho0 = rules::US76_P0_PA / (rules::US76_R_AIR * rules::US76_T0_K);
        let warm_on = self.warmers_on(ground_c);
        let mut temp = rules::PACK_TEMP_C;
        let mut box_max = temp;
        let mut box_min = temp;
        let mut bus_wh = vec![0.0; d.buses.len()];
        let mut own_wh = vec![0.0; d.items.len()];
        // ascent energy per bus / item (for endurance), and coldest cell while on
        let mut bus_flight_wh = vec![0.0; d.buses.len()];
        let mut own_flight_wh = vec![0.0; d.items.len()];
        let mut bus_cold = vec![f64::INFINITY; d.buses.len()];
        let mut own_cold = vec![f64::INFINITY; d.items.len()];
        let mut warmer_wh = 0.0;
        let mut t_since_open = 0.0;
        let mut bus = vec![0.0; d.buses.len()];
        let mut own = vec![0.0; d.items.len()];
        for e in &self.env {
            let t_amb = e.amb_c.unwrap_or(ground_c);
            let h = rules::H_EXT_SL * (e.rho / rho0).sqrt();
            let r_th = self.r_cond + 1.0 / (h * self.a_out);
            let n = (e.dt / 5.0).ceil().max(1.0) as usize;
            let sub = e.dt / n as f64;
            for _ in 0..n {
                self.loads(e.landed, e.descending, temp, &mut bus, &mut own);
                let mut p: f64 = bus.iter().sum::<f64>() + own.iter().sum::<f64>();
                if let (true, Some((w_sl, rated_h, act_min))) = (warm_on, self.warmers) {
                    let ramp = if act_min > 0.0 { (t_since_open / (act_min * 60.0)).min(1.0) } else { 1.0 };
                    let alive = t_since_open < rated_h * 3600.0;
                    let w = if alive { w_sl * ramp * e.p_ratio.powf(rules::WARMER_PRESSURE_EXPONENT) } else { 0.0 };
                    warmer_wh += w * sub / 3600.0;
                    p += w;
                }
                t_since_open += sub;
                for (k, w) in bus.iter().enumerate() {
                    bus_wh[k] += w * sub / 3600.0;
                    if e.flying {
                        bus_flight_wh[k] += w * sub / 3600.0;
                    }
                    if *w > 0.0 {
                        bus_cold[k] = bus_cold[k].min(temp);
                    }
                }
                for (k, w) in own.iter().enumerate() {
                    own_wh[k] += w * sub / 3600.0;
                    if e.flying {
                        own_flight_wh[k] += w * sub / 3600.0;
                    }
                    if *w > 0.0 {
                        own_cold[k] = own_cold[k].min(temp);
                    }
                }
                temp = box_step(self.heat_cap, r_th, p, t_amb, sub, temp);
                box_max = box_max.max(temp);
                box_min = box_min.min(temp);
            }
        }
        // endurance is judged at the ascent's average draw
        let flight_h = (self.burst_t_s / 3600.0).max(1e-9);
        let mut buses = Vec::new();
        for (bi, b) in d.buses.iter().enumerate() {
            let bat = cat.part(&b.battery)?;
            let spec_b = bat.battery.as_ref().with_context(|| format!("{} is not a battery", b.battery))?;
            let cell_min = if bus_cold[bi].is_finite() { bus_cold[bi] } else { ground_c };
            let frac = capacity_fraction(spec_b.chemistry, cell_min);
            let nominal = spec_b.v_nom * spec_b.capacity_ah * (b.series * b.parallel) as f64;
            let have = nominal * frac * usable(spec_b.chemistry);
            let pf = bus_flight_wh[bi] / flight_h;
            buses.push(BusEnergy {
                name: b.name.clone(),
                need_wh: bus_wh[bi],
                have_wh: have,
                cell_min_c: cell_min,
                capacity_fraction: frac,
                flight_endurance_h: if pf > 0.0 { have / pf } else { f64::INFINITY },
            });
        }
        let mut selfp = Vec::new();
        for (k, it) in d.items.iter().enumerate() {
            let part = cat.part(&it.part)?;
            if !part.self_powered {
                continue;
            }
            let bspec = part.battery.as_ref().with_context(|| format!("{} has no battery spec", part.id))?;
            let cell_min = if own_cold[k].is_finite() { own_cold[k] } else { ground_c };
            let frac = capacity_fraction(bspec.chemistry, cell_min);
            // a device's runtime figure already spends its whole battery
            let have = bspec.v_nom * bspec.capacity_ah * frac * it.qty as f64;
            let pf = own_flight_wh[k] / flight_h;
            selfp.push(BusEnergy {
                name: part.id.clone(),
                need_wh: own_wh[k],
                have_wh: have,
                cell_min_c: cell_min,
                capacity_fraction: frac,
                flight_endurance_h: if pf > 0.0 { have / pf } else { f64::INFINITY },
            });
        }
        Ok(ThermalCase { ground_c, warmers_on: warm_on, box_min_c: box_min, box_max_c: box_max, warmer_wh, buses, self_powered: selfp })
    }

    /// The launch window: the widest contiguous ground-temperature interval
    /// around `inside` on which every thermal check passes, edges bisected
    /// to `rules::WINDOW_TOL_C`. None when `inside` itself fails.
    pub fn window(&self, spec: &Spec, inside: f64) -> Result<Option<[f64; 2]>> {
        let ok = |t: f64| -> Result<bool> { Ok(self.case(t)?.passes(spec, self.d, self.cat, self.burst_t_s)) };
        if !ok(inside)? {
            return Ok(None);
        }
        let [smin, smax] = rules::WINDOW_SWEEP_C;
        let edge = |dir: f64| -> Result<f64> {
            let mut good = inside;
            loop {
                let next = good + dir * rules::WINDOW_STEP_C;
                if (dir < 0.0 && next < smin) || (dir > 0.0 && next > smax) {
                    return Ok(good);
                }
                if !ok(next)? {
                    let mut bad = next;
                    while (bad - good).abs() > rules::WINDOW_TOL_C {
                        let mid = 0.5 * (good + bad);
                        if ok(mid)? {
                            good = mid;
                        } else {
                            bad = mid;
                        }
                    }
                    return Ok(good);
                }
                good = next;
            }
        };
        let lo = edge(-1.0)?;
        let hi = edge(1.0)?;
        Ok(Some([lo, hi]))
    }
}

/// The design's launch window for this spec (see `ThermalModel::window`),
/// searched outward from the middle of the declared range.
pub fn launch_window(spec: &Spec, d: &Design, cat: &Catalog) -> Result<Option<[f64; 2]>> {
    let masses = mass::masses(d, cat)?;
    let fl = flight::simulate(spec, d, cat, &masses)?;
    let [lo, hi] = launch_range(spec)?;
    ThermalModel::new(d, cat, &masses, &fl)?.window(spec, 0.5 * (lo + hi))
}

/// The warmer switch-over a design should declare: the lowest whole degree
/// in the declared range at and above which the box passes without its
/// warmers, provided it also passes with them just below. None: no warmers,
/// or no switch-over works (then warmers are opened on every day).
pub fn choose_warmer_switch(spec: &Spec, d: &Design, cat: &Catalog, masses: &Masses, fl: &Flight) -> Result<Option<f64>> {
    let d_off = Design { warmers_below_c: Some(f64::NEG_INFINITY), ..d.clone() };
    let d_on = Design { warmers_below_c: None, ..d.clone() };
    let off = ThermalModel::new(&d_off, cat, masses, fl)?;
    if off.warmers.is_none() {
        return Ok(None);
    }
    let on = ThermalModel::new(&d_on, cat, masses, fl)?;
    let [lo, hi] = launch_range(spec)?;
    let pass = |m: &ThermalModel, t: f64| -> Result<bool> { Ok(m.case(t)?.passes(spec, d, cat, fl.burst_t_s)) };
    // the lowest whole degree at which the box passes with the warmers
    // left shut (colder days only get colder: bisect on whole degrees)
    let mut good = hi.floor();
    if !pass(&off, good)? {
        return Ok(None);
    }
    if pass(&off, lo)? {
        // never needed in the range: keep them shut on every day in it
        return Ok(Some(lo));
    }
    let mut bad = lo;
    while good - bad > 1.0 {
        let mid = (0.5 * (good + bad)).floor().max(bad + 1.0);
        if pass(&off, mid)? {
            good = mid;
        } else {
            bad = mid;
        }
    }
    let s = good;
    Ok(if pass(&on, s - rules::WINDOW_TOL_C)? { Some(s) } else { None })
}

/// Link margin booked for a flight sample at which a tracker is off, dead
/// or has no GPS: nothing reaches the ground.
pub const NO_LINK_DB: f64 = -100.0;

pub fn analyse(spec: &Spec, d: &Design, cat: &Catalog) -> Result<Analysis> {
    let masses = mass::masses(d, cat)?;
    let fl = flight::simulate(spec, d, cat, &masses)?;
    let layout = geometry::layout(d, cat)?;
    analyse_with(spec, d, cat, masses, fl, layout)
}

/// `analyse` from a mass breakdown, flight and packed layout the caller
/// already has for this design (the search computes them once).
pub fn analyse_with(spec: &Spec, d: &Design, cat: &Catalog, masses: Masses, fl: Flight, layout: Layout) -> Result<Analysis> {
    let ground_c = spec.ground_temperature_c.unwrap_or(rules::US76_T0_K - 273.15);

    // ------------------------------------------- thermal and energy, per case
    // every thermal number below is the worst over the declared launch
    // range's ends (and the forecast and warmer switch-over inside it)
    let model = ThermalModel::new(d, cat, &masses, &fl)?;
    let launch_range_c = launch_range(spec)?;
    let mut thermal_cases = Vec::new();
    for t in thermal_points(spec, d)? {
        thermal_cases.push(model.case(t)?);
    }
    let box_min = thermal_cases.iter().map(|c| c.box_min_c).fold(f64::INFINITY, f64::min);
    let box_max = thermal_cases.iter().map(|c| c.box_max_c).fold(f64::NEG_INFINITY, f64::max);
    let worst = |pick: &dyn Fn(&ThermalCase) -> &Vec<BusEnergy>| -> Vec<BusEnergy> {
        let n = pick(&thermal_cases[0]).len();
        (0..n)
            .map(|k| {
                thermal_cases
                    .iter()
                    .map(|c| &pick(c)[k])
                    .min_by(|x, y| (x.have_wh - x.need_wh).partial_cmp(&(y.have_wh - y.need_wh)).unwrap_or(std::cmp::Ordering::Equal).then(x.flight_endurance_h.partial_cmp(&y.flight_endurance_h).unwrap_or(std::cmp::Ordering::Equal)))
                    .expect("at least one case")
                    .clone()
            })
            .collect()
    };
    let buses = worst(&|c: &ThermalCase| &c.buses);
    let selfp = worst(&|c: &ThermalCase| &c.self_powered);
    let ambient_min = fl
        .samples
        .iter()
        .filter(|s| s.phase != Phase::Landed)
        .map(|s| atmosphere::at(Length::m(s.z_m)).temperature.as_celsius())
        .fold(launch_range_c[0].min(ground_c), f64::min);

    // ------------------------------------------------------------ links
    let (lx, ly) = fl.landing_xy_m;
    let land_dist = (lx * lx + ly * ly).sqrt();
    // chase car: on the line from launch to the landing, CHASE_OFFSET short of it
    let k = if land_dist > 0.0 { (land_dist - rules::CHASE_OFFSET_KM * 1e3).max(0.0) / land_dist } else { 0.0 };
    let chase = (lx * k, ly * k);
    let z0 = spec.launch.elevation_m;
    let mut trackers = Vec::new();
    for tr in &d.trackers {
        let radio_part = cat.part(&tr.radio)?;
        let rs = radio_part.radio.as_ref();
        let mut t = TrackerLink {
            name: tr.name.clone(),
            class: tr.class,
            link: rs.map(|r| r.link),
            worst_margin_db: None,
            worst_at_range_km: 0.0,
            max_range_km: 0.0,
            last_heard_agl_m: None,
            works_through_window: false,
            p_recover: 0.0,
            why: String::new(),
        };
        let Some(rs) = rs else {
            t.why = format!("{} has no radio spec", tr.radio);
            trackers.push(t);
            continue;
        };
        // a tracker reports only while its radio AND its GPS are in the
        // design, switched on in that phase and on a battery that lasts
        let alive = |id: &str| -> Option<&crate::model::Item> {
            let it = d.items.iter().find(|it| it.part == id)?;
            let p = cat.part(id).ok()?;
            let energy = if p.self_powered {
                selfp.iter().any(|b| b.name == p.id && b.have_wh >= b.need_wh)
            } else {
                it.bus.and_then(|bi| buses.get(bi)).is_some_and(|b| b.have_wh >= b.need_wh)
            };
            energy.then_some(it)
        };
        let radio_it = alive(&tr.radio);
        let gps_part = cat.part(&tr.gps).ok().filter(|g| g.gps.is_some() || g.radio.as_ref().is_some_and(|r| r.link == LinkKind::Cellular));
        let gps_it = gps_part.and(alive(&tr.gps));
        let on = |ph: Phase| -> bool {
            let on_in = |it: &crate::model::Item| match ph {
                Phase::Ascent => true,
                Phase::Descent => !it.until_burst,
                Phase::Landed => it.on_after_landing && !it.until_burst,
            };
            radio_it.is_some_and(on_in) && gps_it.is_some_and(on_in)
        };
        let gps_part_temp_ok = gps_part.is_some_and(|g| box_min >= g.temp_min_c && box_max <= g.temp_max_c);
        let temp_ok = box_min >= radio_part.temp_min_c && box_max <= radio_part.temp_max_c && gps_part_temp_ok;
        t.works_through_window = on(Phase::Landed) && temp_ok;
        let dead_why = if radio_it.is_none() {
            format!("{} missing or its battery runs flat", tr.radio)
        } else if gps_part.is_none() {
            format!("{} is not a GPS", tr.gps)
        } else if gps_it.is_none() {
            format!("GPS {} missing from the payload or its battery runs flat", tr.gps)
        } else {
            String::new()
        };
        let base_p = match rs.link {
            LinkKind::Aprs => rules::P_TRACKER_APRS,
            LinkKind::Lora => rules::P_TRACKER_LORA,
            LinkKind::Elrs => rules::P_TRACKER_ELRS,
            LinkKind::Cellular => rules::P_TRACKER_PHONE,
            LinkKind::Video => 0.0,
        };
        if rs.link == LinkKind::Cellular {
            // a phone reports through the cell network once landed
            t.p_recover = if t.works_through_window { base_p } else { 0.0 };
            t.why = if t.works_through_window {
                "phone alive at landing".into()
            } else if !dead_why.is_empty() {
                dead_why
            } else {
                format!("phone off after landing or out of its {}..{} C range (box {:.0}..{:.0} C)", radio_part.temp_min_c, radio_part.temp_max_c, box_min, box_max)
            };
            t.last_heard_agl_m = t.works_through_window.then_some(0.0);
            trackers.push(t);
            continue;
        }
        let Some(gid) = &tr.ground else {
            t.why = "no ground station for this link".into();
            trackers.push(t);
            continue;
        };
        let gs = cat.station(gid)?;
        let mut worst = f64::INFINITY;
        let mut last_heard: Option<f64> = None;
        for s in fl.samples.iter().filter(|s| s.phase != Phase::Landed) {
            let agl = s.z_m - z0;
            if !on(s.phase) {
                // switched off or dead in this phase: no position reaches the ground
                if agl >= 1000.0 && NO_LINK_DB < worst {
                    worst = NO_LINK_DB;
                    t.worst_at_range_km = 0.0;
                }
                continue;
            }
            let mut best = f64::NEG_INFINITY;
            let mut best_range = 0.0;
            for (sx, sy) in [(0.0, 0.0), chase] {
                let gd = ((s.x_m - sx).powi(2) + (s.y_m - sy).powi(2)).sqrt();
                let (slant, el, vis) = radio::geometry(gd, gs.antenna_height_m, agl.max(rules::LANDED_ANTENNA_M));
                t.max_range_km = t.max_range_km.max(slant / 1e3);
                if !vis {
                    continue;
                }
                let g_air = radio::dipole_gain_dbi(rs.antenna_gain_dbi, el);
                let g_gnd = radio::dipole_gain_dbi(gs.antenna_gain_dbi, el);
                let loss = radio::fspl_db(slant, rs.freq_mhz);
                let down = rs.tx_dbm + g_air + g_gnd - loss - gs.rx_sens_dbm;
                let m = if rs.link == LinkKind::Elrs {
                    let up = gs.tx_dbm.unwrap_or(f64::NEG_INFINITY) + g_air + g_gnd - loss - rs.sens_dbm.unwrap_or(0.0);
                    down.min(up)
                } else {
                    down
                } - rules::LINK_FADE_MARGIN_DB;
                if m > best {
                    best = m;
                    best_range = slant / 1e3;
                }
            }
            if agl >= 1000.0 && best < worst {
                worst = best;
                t.worst_at_range_km = best_range;
            }
            if s.phase == Phase::Descent && best >= 0.0 {
                last_heard = Some(last_heard.map_or(agl, |h: f64| h.min(agl)));
            }
        }
        t.worst_margin_db = Some(if worst.is_finite() { worst } else { NO_LINK_DB });
        t.last_heard_agl_m = last_heard;
        // locating: a last fix within 1 km AGL leaves a short low-level drift to search
        let p_loc = match last_heard {
            Some(h) if h <= 1000.0 => 1.0,
            Some(h) => 1000.0 / h,
            None => 0.0,
        };
        let alive_to_landing = temp_ok && on(Phase::Descent);
        t.p_recover = if alive_to_landing { base_p * p_loc } else { 0.0 };
        t.why = format!(
            "last heard {} AGL; {}",
            last_heard.map_or("never on descent".into(), |h| format!("{h:.0} m")),
            if alive_to_landing {
                "alive to landing".to_string()
            } else if !dead_why.is_empty() {
                dead_why.clone()
            } else if !on(Phase::Descent) {
                "radio or GPS switched off on the descent".to_string()
            } else {
                "out of temperature range".to_string()
            }
        );
        trackers.push(t);
    }
    let p_none: f64 = trackers.iter().map(|t| 1.0 - t.p_recover).product();
    let recovery_probability = rules::P_SITE * (1.0 - p_none);

    // ------------------------------------------------------ coexistence
    let mut coexistence = Vec::new();
    for tr in &d.trackers {
        let Ok(gps) = cat.part(&tr.gps) else { continue };
        if gps.gps.is_none() {
            continue;
        }
        // a phone's GNSS is used only on the ground and its front end is
        // unpublished: no in-flight coexistence pair is booked for it
        if gps.radio.as_ref().is_some_and(|r| r.link == LinkKind::Cellular) {
            continue;
        }
        let Some(pg) = geometry::antenna_point(d, cat, &layout, &gps.id, true) else { continue };
        for it in &d.items {
            let p: &Part = cat.part(&it.part)?;
            let Some(r) = &p.radio else { continue };
            // a tracker module's own GPS and radio were laid out by its maker
            if p.id == gps.id {
                continue;
            }
            let Some(pt) = geometry::antenna_point(d, cat, &layout, &p.id, false) else { continue };
            let sep = (pt - pg).norm() * 1e-3;
            let coupled = r.tx_dbm + r.antenna_gain_dbi - radio::fspl_db(sep, r.freq_mhz);
            let block = coupled - radio::patch_rejection_db(r.freq_mhz);
            coexistence.push(Coexist {
                gps: gps.id.clone(),
                tx: p.id.clone(),
                separation_m: sep,
                at_gnss_dbm: block,
                limit_dbm: radio::gnss_blocking_limit_dbm(),
                mechanism: format!("fundamental {:.0} MHz through patch rejection {:.0} dB", r.freq_mhz, radio::patch_rejection_db(r.freq_mhz)),
            });
            for n in radio::harmonics_in_l1(r.freq_mhz) {
                // ASSUMPTION: harmonics 60 dB under the carrier.
                coexistence.push(Coexist {
                    gps: gps.id.clone(),
                    tx: p.id.clone(),
                    separation_m: sep,
                    at_gnss_dbm: coupled - 60.0,
                    limit_dbm: radio::gnss_inband_limit_dbm(),
                    mechanism: format!("harmonic {n} in L1"),
                });
            }
        }
    }

    // -------------------------------------------------------------- gas
    let gas = cat.gas(&d.gas)?;
    let balloon = cat.balloon(&d.balloon)?;
    let n = (d.neck_lift_g + balloon.mass_g) * 1e-3 / (rules::M_AIR - crate::physics::gas_molar_mass(gas.gas, gas.purity));
    let gas_needed = n * rules::R_UNIVERSAL * rules::CYL_REF_T_K / rules::CYL_REF_P_PA;
    let gas_have = d.cylinders as f64 * gas.cylinder_m3 * (1.0 - rules::CYL_UNUSABLE);

    Ok(Analysis {
        masses,
        landing_km: land_dist / 1e3,
        flight: fl,
        layout,
        box_min_c: box_min,
        box_max_c: box_max,
        ambient_min_c: ambient_min,
        buses,
        self_powered: selfp,
        trackers,
        coexistence,
        gas_needed_m3: gas_needed,
        gas_have_m3: gas_have,
        recovery_probability,
        launch_range_c,
        thermal_cases,
    })
}

#[cfg(test)]
mod tests {
    use super::box_step;

    #[test]
    fn box_step_is_the_euler_update() {
        // C = 100 J/K, R = 2 K/W, P = 5 W, ambient 250 K, box at 260 K:
        // dT/dt = 5 - 10/2 = 0, so one step leaves it put
        assert_eq!(box_step(100.0, 2.0, 5.0, 250.0, 10.0, 260.0), 260.0);
        // box at 250 K: dT = 5 * 10 / 100 = 0.5
        assert!((box_step(100.0, 2.0, 5.0, 250.0, 10.0, 250.0) - 250.5).abs() < 1e-12);
        // unpowered, 20 K above ambient: -(20 / 2) * 10 / 100 = -1
        assert!((box_step(100.0, 2.0, 0.0, 250.0, 10.0, 270.0) - 269.0).abs() < 1e-12);
    }
}
