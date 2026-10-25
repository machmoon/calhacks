//! From a requirements-only spec to a resolved design: enumerate the
//! discrete choices (balloon x chute x payload kits x wall), size the fill
//! by the CUSF rule for 5 m/s, screen on the cheap analysis, then run the
//! full check set on the best candidates until one passes.

use crate::catalog::{Catalog, LiftGas};
use crate::checks;
use crate::geometry;
use crate::model::{Bus, Design, Enclosure, Item, Spec, Tracker};
use crate::rules;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use wright_core::units::Speed;

const KITS_JSON: &str = include_str!("../../../data/balloon/kits.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Camera,
    Primary,
    Secondary,
    Backup,
}

/// A self-contained sub-assembly (a camera, a tracker chain) with its own
/// buses; bus indices in `items` are local to the kit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kit {
    pub id: String,
    pub slot: Slot,
    pub name: String,
    pub items: Vec<Item>,
    #[serde(default)]
    pub buses: Vec<Bus>,
    #[serde(default)]
    pub trackers: Vec<Tracker>,
    /// Ground gear this kit needs bought unless the team owns it.
    #[serde(default)]
    pub ground_needed: Vec<String>,
    #[serde(default)]
    pub needs_licence: bool,
}

pub fn kits() -> &'static Vec<Kit> {
    static K: OnceLock<Vec<Kit>> = OnceLock::new();
    K.get_or_init(|| serde_json::from_str(KITS_JSON).expect("data/balloon/kits.json"))
}

/// Line from balloon to chute to payload, m. ASSUMPTION: 3 m + 5 m.
pub const LINE_M: f64 = 8.0;
pub const LINE_ID: &str = "ashaway-dacron-30lb";
pub const INSULATION_ID: &str = "xps-foamular-150";
pub const WALLS_MM: [f64; 3] = [25.4, 38.1, 50.8];
/// Ascent rates the CUSF fill may be sized for (the check judges the result).
pub const FILL_TARGETS_MPS: [f64; 5] = [4.5, 5.0, 5.5, 6.0, 6.5];
/// Chemical hand warmers the search may pack (the thermal lever that needs
/// no battery; a 5 W thermostat pad is the backup-slot `heater-only` kit).
pub const WARMER_ID: &str = "hothands-hw";
pub const WARMER_COUNTS: [u32; 4] = [0, 2, 3, 4];

/// Add `n` hand warmers (no bus: they need no power).
pub fn with_warmers(d: &Design, n: u32) -> Design {
    let mut d = d.clone();
    if n > 0 {
        d.items.push(Item { part: WARMER_ID.into(), qty: n, bus: None, rail: None, on_after_landing: false, until_burst: false });
        d.notes.push(format!("Thermal: {n} HotHands hand warmers, opened at sealing on cold days"));
    }
    d
}

/// Set the thermal decisions a design declares: when its warmers are
/// opened, and its go/no-go launch window (the modelled window rounded
/// inward to whole degrees).
pub fn declare_thermal(spec: &Spec, d: &mut Design, cat: &Catalog) -> Result<()> {
    let m = crate::mass::masses(d, cat)?;
    let fl = crate::flight::simulate(spec, d, cat, &m)?;
    d.warmers_below_c = crate::analysis::choose_warmer_switch(spec, d, cat, &m, &fl)?;
    let [lo, hi] = crate::analysis::launch_range(spec)?;
    let w = crate::analysis::ThermalModel::new(d, cat, &m, &fl)?.window(spec, 0.5 * (lo + hi))?;
    d.launch_window_c = w.map(|[a, b]| [a.ceil(), b.floor()]);
    Ok(())
}

/// Merge kits into a design skeleton (no balloon yet).
pub fn assemble(label: &str, chosen: &[&Kit], owned_ground: &[String]) -> Design {
    let mut d = Design {
        label: label.into(),
        balloon: String::new(),
        gas: String::new(),
        cylinders: 1,
        neck_lift_g: 0.0,
        fill_target_mps: rules::ASCENT_TARGET_MPS,
        parachute: String::new(),
        line: LINE_ID.into(),
        line_m: LINE_M,
        enclosure: Enclosure { insulation: INSULATION_ID.into(), wall_mm: WALLS_MM[0], inner_mm: [0.0; 3], printed_tray: true },
        items: vec![],
        buses: vec![],
        trackers: vec![],
        buy_ground: vec![],
        printed: None,
        printed_allowance_g: 0.0,
        warmers_below_c: None,
        launch_window_c: None,
        notes: chosen.iter().map(|k| format!("{:?}: {}", k.slot, k.name)).collect(),
    };
    for k in chosen {
        let off = d.buses.len();
        d.buses.extend(k.buses.iter().cloned());
        d.items.extend(k.items.iter().cloned().map(|mut it| {
            it.bus = it.bus.map(|b| b + off);
            it
        }));
        d.trackers.extend(k.trackers.iter().cloned());
        for g in &k.ground_needed {
            if !owned_ground.contains(g) && !d.buy_ground.contains(g) {
                d.buy_ground.push(g.clone());
            }
        }
    }
    d
}

/// Pack the box and set the printed-rack allowance: the rack's solid
/// volume at PETG density (an upper bound until it is sliced).
pub fn size_enclosure(d: &mut Design, cat: &Catalog, wall: f64) -> Result<()> {
    d.enclosure.wall_mm = wall;
    let inst = geometry::instances(d, cat)?;
    let l = geometry::best_layout(&inst, wall, rules::PACK_GAP_MM, d.enclosure.printed_tray, &geometry::camera_ids(d, cat)).context("the parts do not fit a printable footprint")?;
    d.enclosure.inner_mm = l.inner_mm;
    if d.enclosure.printed_tray {
        let vol: f64 = geometry::rack_meshes(&l).iter().map(|(_, m)| m.volume()).sum();
        let petg = wright_geom::slice::material(geometry::PRINT_MATERIAL)?.density;
        d.printed_allowance_g = vol * 1e-3 * petg;
    }
    Ok(())
}

/// Fill to the CUSF neck lift for 5 m/s and buy enough cylinders.
pub fn size_fill(d: &mut Design, cat: &Catalog) -> Result<()> {
    let m = crate::mass::masses(d, cat)?;
    d.neck_lift_g = crate::flight::size_fill(d, cat, &m, Speed::mps(d.fill_target_mps))?;
    let gas = cat.gas(&d.gas)?;
    let balloon = cat.balloon(&d.balloon)?;
    let n = (d.neck_lift_g + balloon.mass_g) * 1e-3 / (rules::M_AIR - crate::physics::gas_molar_mass(gas.gas, gas.purity));
    let need = n * rules::R_UNIVERSAL * rules::CYL_REF_T_K / rules::CYL_REF_P_PA;
    d.cylinders = (need / (gas.cylinder_m3 * (1.0 - rules::CYL_UNUSABLE))).ceil().max(1.0) as u32;
    Ok(())
}

fn combos(spec: &Spec) -> Vec<Vec<&'static Kit>> {
    let ks = kits();
    let of = |s: Slot| -> Vec<&'static Kit> { ks.iter().filter(|k| k.slot == s && (!k.needs_licence || spec.licensed_ham)).collect() };
    let mut out = Vec::new();
    let cams = of(Slot::Camera);
    let prims = of(Slot::Primary);
    let secs = of(Slot::Secondary);
    let mut backs: Vec<Option<&Kit>> = vec![None];
    backs.extend(of(Slot::Backup).into_iter().map(Some));
    for c in &cams {
        for p in &prims {
            for s in &secs {
                for b in &backs {
                    let mut v = vec![*c, *p, *s];
                    if let Some(b) = b {
                        v.push(*b);
                    }
                    out.push(v);
                }
            }
        }
    }
    out
}

/// The search. Deterministic: candidates are enumerated in catalog order
/// and ranked by (passing, burst altitude, recovery probability, -cost).
pub fn search(spec: &Spec, cat: &Catalog) -> Result<Design> {
    search_under(spec, cat, &[])
}

/// A confirmed attack as the search uses it: the check it failed, under
/// the spec it produces (`wright_core::Condition`).
pub type Condition = wright_core::Condition<Spec>;

/// Ok when `d` passes `c.check` under `c.spec`; Err says how it does not.
pub fn passes_condition(d: &Design, c: &Condition, cat: &Catalog) -> std::result::Result<(), String> {
    let (f, _) = checks::evaluate(&c.spec, d, cat);
    match f.get(&c.check) {
        Some(o) if o.passed() => Ok(()),
        Some(o) => Err(format!("{} under {}: {o}", c.check, c.id)),
        None => Err(format!("{} under {}: no such check", c.check, c.id)),
    }
}

/// The search, every candidate also screened on each confirmed attack's
/// check under that attack's spec before the expensive steps (full checks,
/// slicing); a finalist must pass every check and every condition, or the
/// result is an error naming the conditions and the closest failure.
/// With no conditions this is [`search`]: the closest failing design is
/// returned when none passes.
pub fn search_under(spec: &Spec, cat: &Catalog, conditions: &[Condition]) -> Result<Design> {
    let gases: Vec<_> = cat.gases.iter().filter(|g| spec.allow_hydrogen || g.gas == LiftGas::Helium).collect();
    // every (kit set + warmers, wall) is one job; jobs run on all cores and
    // their results are merged in job order, so the search stays deterministic
    let mut jobs: Vec<(Design, f64)> = Vec::new();
    for kitset in combos(spec) {
        let label = kitset.iter().map(|k| k.id.as_str()).collect::<Vec<_>>().join("+");
        let base = assemble(&label, &kitset, &spec.owned_ground);
        for n in WARMER_COUNTS {
            let mut d = with_warmers(&base, n);
            if n > 0 {
                d.label = format!("{}+{n}xwarmer", d.label);
            }
            if !spec.must_fly.iter().all(|m| d.items.iter().any(|i| &i.part == m)) {
                continue;
            }
            for wall in WALLS_MM {
                jobs.push((d.clone(), wall));
            }
        }
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(jobs.len().max(1));
    let mut results: Vec<(usize, Vec<Screened>, Screen)> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                sc.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let j = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((base, wall)) = jobs.get(j) else { break };
                        let (ok, why) = screen_job(spec, cat, &gases, base, *wall, conditions);
                        out.push((j, ok, why));
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("search thread")).collect()
    });
    results.sort_by_key(|r| r.0);
    let mut screened: Vec<Screened> = Vec::new();
    // why candidates fail the screen, so an empty result can say which
    // requirement binds (a live run probed blindly for four rounds when the
    // answer was "the cheapest kit costs more than the budget")
    let mut why = Screen::default();
    for (_, ok, w) in results {
        screened.extend(ok);
        why.merge(&w);
    }
    screened.sort_by(|x, y| {
        y.0.partial_cmp(&x.0)
            .unwrap()
            .then(y.1.partial_cmp(&x.1).unwrap())
            .then(x.2.partial_cmp(&y.2).unwrap())
            .then(x.3.label.cmp(&y.3.label))
    });
    // slices land in a per-run scratch dir removed on drop (the old shared
    // dir grew with every run)
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("wright-balloon-slice"));
    let scratch = crate::build::Scratch::new()?;
    let mut best_fail: Option<(f64, Design)> = None;
    let mut condition_failures: Vec<String> = Vec::new();
    let fails_conditions = |d: &Design| -> Vec<String> { conditions.iter().filter_map(|c| passes_condition(d, c, cat).err()).collect() };
    for (_, _, _, mut d) in screened.into_iter().take(40) {
        d.printed = None;
        if declare_thermal(spec, &mut d, cat).is_err() {
            continue;
        }
        let (f, _) = checks::evaluate(spec, &d, cat);
        // a design whose only failure is the unsliced rack: slice it now
        let only_print = !f.passed() && f.failures().all(|o| o.name == "printed_parts");
        if f.passed() || only_print {
            let bad = fails_conditions(&d);
            if !bad.is_empty() {
                condition_failures.push(format!("{}: {}", d.label, bad.join("; ")));
                continue;
            }
        }
        if f.passed() {
            return Ok(d);
        }
        if only_print {
            if let Ok(p) = crate::build::slice_rack(&d, cat, &scratch.0) {
                d.printed = Some(p);
                size_fill(&mut d, cat)?;
                declare_thermal(spec, &mut d, cat)?;
                let (f2, _) = checks::evaluate(spec, &d, cat);
                let bad = fails_conditions(&d);
                if f2.passed() && bad.is_empty() {
                    return Ok(d);
                }
                if !bad.is_empty() {
                    condition_failures.push(format!("{}: {}", d.label, bad.join("; ")));
                }
            }
        }
        let v = f.violation(|_| 1.0);
        if best_fail.as_ref().is_none_or(|(bv, _)| v < *bv) {
            best_fail = Some((v, d));
        }
    }
    drop(scratch);
    if !conditions.is_empty() {
        let names = conditions.iter().map(|c| format!("{} under {} ({})", c.check, c.id, c.claim)).collect::<Vec<_>>().join(", ");
        let closest = match (&best_fail, condition_failures.first()) {
            (Some((_, d)), _) => {
                let (f, _) = checks::evaluate(spec, d, cat);
                let mut v: Vec<String> = f.failures().map(|o| o.name.clone()).collect();
                v.extend(fails_conditions(d));
                format!("closest: {} fails {}", d.label, v.join("; "))
            }
            (None, Some(c)) => format!("closest: {c}"),
            (None, None) => format!("no candidate passed the screen: {}", why.explain(spec)),
        };
        bail!("no balloon in the search passes every check and the confirmed attacks [{names}]; {closest}");
    }
    match best_fail {
        Some((_, d)) => Ok(d),
        None => bail!("no candidate passed the screen: {}", why.explain(spec)),
    }
}

/// Screen one kit set + warmers at one wall thickness over every balloon,
/// chute, gas and fill target: the flight and budget first, the thermal,
/// power and recovery analysis only for candidates that pass those.
fn screen_job(spec: &Spec, cat: &Catalog, gases: &[&crate::catalog::GasRow], base: &Design, wall: f64, conditions: &[Condition]) -> (Vec<Screened>, Screen) {
    let mut why = Screen::default();
    let mut ok = Vec::new();
    let mut base = base.clone();
    if size_enclosure(&mut base, cat, wall).is_err() {
        return (ok, why);
    }
    let Ok(layout) = geometry::layout(&base, cat) else { return (ok, why) };
    let has_warmers = base.items.iter().any(|i| i.part == WARMER_ID);
    for b in &cat.balloons {
        for c in &cat.parachutes {
            for g in gases {
                for target in FILL_TARGETS_MPS {
                    let mut d = base.clone();
                    d.fill_target_mps = target;
                    d.balloon = b.id.clone();
                    d.parachute = c.id.clone();
                    d.gas = g.id.clone();
                    if size_fill(&mut d, cat).is_err() {
                        continue;
                    }
                    let Ok(m) = crate::mass::masses(&d, cat) else { continue };
                    let Ok(fl) = crate::flight::simulate(spec, &d, cat, &m) else { continue };
                    let Some(burst) = fl.burst_altitude_m else { continue };
                    let bom = crate::bom(&d, cat);
                    let cost = bom.total().as_usd();
                    let mut fails = [
                        burst < spec.min_burst_altitude_m,
                        fl.landing_speed_mps > rules::LANDING_MAX_MPS,
                        !(rules::ASCENT_MIN_MPS..=rules::ASCENT_MAX_MPS).contains(&fl.mean_ascent_mps)
                            || !(rules::ASCENT_MIN_MPS..=rules::ASCENT_MAX_MPS).contains(&fl.launch_ascent_mps),
                        !bom.unpriced().is_empty() || spec.budget_usd.is_some_and(|b| cost > b),
                        false,
                        false,
                        false,
                    ];
                    if fails.iter().any(|f| *f) {
                        // the rest is not analysed for this candidate
                        why.note(&fails, cost, burst, 0.0, false);
                        continue;
                    }
                    if has_warmers {
                        let Ok(s) = crate::analysis::choose_warmer_switch(spec, &d, cat, &m, &fl) else { continue };
                        d.warmers_below_c = s;
                    }
                    let burst_t_s = fl.burst_t_s;
                    let Ok(a) = crate::analysis::analyse_with(spec, &d, cat, m, fl, layout.clone()) else { continue };
                    fails[4] = a.recovery_probability < spec.min_recovery_probability;
                    fails[5] = !a.buses.iter().chain(&a.self_powered).all(|x| x.have_wh >= x.need_wh);
                    fails[6] = !a.thermal_cases.iter().all(|c| c.passes(spec, &d, cat, burst_t_s));
                    why.note(&fails, cost, burst, a.recovery_probability, true);
                    if fails.iter().any(|f| *f) {
                        continue;
                    }
                    // the confirmed attacks, before any finalist work: the
                    // window this candidate would declare, then each check
                    // under its attack's spec
                    if !conditions.is_empty() {
                        let mut dc = d.clone();
                        if declare_thermal(spec, &mut dc, cat).is_err() || conditions.iter().any(|c| passes_condition(&dc, c, cat).is_err()) {
                            why.conditions += 1;
                            continue;
                        }
                    }
                    ok.push((burst, a.recovery_probability, cost, d));
                }
            }
        }
    }
    (ok, why)
}

/// A candidate that passed the screen: (burst m, recovery P, cost USD, design).
type Screened = (f64, f64, f64, Design);

/// Tallies of why candidates failed the screen.
#[derive(Default)]
struct Screen {
    n: usize,
    fails: [usize; 7],
    min_cost: f64,
    max_burst: f64,
    max_recovery: f64,
    /// failing only on budget
    only_budget: usize,
    /// candidates that passed flight and budget and were analysed for
    /// recovery, power and temperature
    analysed: usize,
    /// candidates that passed the screen but failed a confirmed attack
    conditions: usize,
}

const SCREEN_NAMES: [&str; 7] = ["burst altitude", "landing speed", "ascent rate 4-6 m/s", "budget (or an unpriced part)", "recovery probability", "power", "temperature over the launch range"];

impl Screen {
    fn note(&mut self, fails: &[bool; 7], cost: f64, burst: f64, recovery: f64, analysed: bool) {
        if self.n == 0 {
            self.min_cost = f64::INFINITY;
        }
        self.n += 1;
        self.analysed += analysed as usize;
        for (i, f) in fails.iter().enumerate() {
            if *f {
                self.fails[i] += 1;
            }
        }
        if fails[3] && fails.iter().filter(|f| **f).count() == 1 {
            self.only_budget += 1;
        }
        self.min_cost = self.min_cost.min(cost);
        self.max_burst = self.max_burst.max(burst);
        self.max_recovery = self.max_recovery.max(recovery);
    }

    fn merge(&mut self, o: &Screen) {
        if o.n == 0 {
            return;
        }
        if self.n == 0 {
            self.min_cost = f64::INFINITY;
        }
        self.n += o.n;
        self.analysed += o.analysed;
        self.conditions += o.conditions;
        for (a, b) in self.fails.iter_mut().zip(o.fails) {
            *a += b;
        }
        self.only_budget += o.only_budget;
        self.min_cost = self.min_cost.min(o.min_cost);
        self.max_burst = self.max_burst.max(o.max_burst);
        self.max_recovery = self.max_recovery.max(o.max_recovery);
    }

    fn explain(&self, spec: &Spec) -> String {
        if self.n == 0 {
            return "no balloon, parachute and gas combination could be sized for this payload".into();
        }
        let mut parts: Vec<String> = SCREEN_NAMES
            .iter()
            .zip(self.fails)
            .enumerate()
            .filter(|(_, (_, k))| *k > 0)
            .map(|(i, (name, k))| if i < 4 { format!("{name} fails {k} of {}", self.n) } else { format!("{name} fails {k} of the {} that pass flight and budget", self.analysed) })
            .collect();
        parts.push(format!(
            "cheapest candidate ${:.0} (budget {}), highest burst {:.0} m (need {:.0}), best recovery {:.2} (need {:.2})",
            self.min_cost, spec.budget_usd.map_or("none stated".into(), |b| format!("${b:.0}")), self.max_burst, spec.min_burst_altitude_m, self.max_recovery, spec.min_recovery_probability
        ));
        if self.conditions > 0 {
            parts.push(format!("{} passed the screen but failed a confirmed attack", self.conditions));
        }
        if self.only_budget > 0 {
            parts.push(format!("{} candidates fail on budget alone: raising the budget is the one change that admits them", self.only_budget));
        }
        parts.join("; ")
    }
}
