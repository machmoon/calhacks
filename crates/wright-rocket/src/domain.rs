//! `wright_rocket::Rocket`: the `wright_core::Domain` for student liquid
//! rockets. `design` accepts a requirements-only spec (competition,
//! contract apogee, propellants, budget) and resolves every choice by
//! rule and search; `check` rebuilds the vehicle from the design's choices
//! and re-flies it, so no stored number is trusted.

use crate::catalog::Catalog;
use crate::engine::{self, EngineSpec};
use crate::propellant::reactant;
use crate::rules::{self, FT};
use crate::trajectory::{fly, FlightResult, Launch};
use crate::vehicle::{self, Vehicle, VehicleParams};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use wright_core::units::{Money, G0};
use wright_core::{Bom, BomLine, CheckId, Domain, Finished, Report, Source};

/// The competition whose rules bound the checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Competition {
    /// FAR-OUT 2026-27 rules v4.1, 2027 TWR rule (Mojave FAR site).
    #[default]
    FarOut2027,
    /// ESRA IREC 2025 DTEG v1.1.6.
    Irec2025,
}

/// Numeric limits of one competition, each with its section.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    pub rail_exit_min_mps: f64,
    pub twr_min: f64,
    pub stability_min_cal: f64,
    pub stability_max_cal: f64,
    pub drogue_mps: (f64, f64),
    pub main_max_mps: f64,
    pub burst_sf_metal: f64,
    pub burst_sf_composite: f64,
    pub wind_max_mps: f64,
    pub elevation_m: f64,
    pub rail_m: f64,
}

impl Competition {
    pub fn limits(self) -> Limits {
        match self {
            Competition::FarOut2027 => Limits {
                rail_exit_min_mps: rules::FAR_RAIL_EXIT_MIN_MPS,
                twr_min: rules::FAR_TWR_MIN,
                stability_min_cal: rules::FAR_STABILITY_MIN_CAL,
                stability_max_cal: rules::OVERSTABLE_MAX_CAL,
                drogue_mps: (rules::FAR_DROGUE_MIN_MPS, rules::FAR_DROGUE_MAX_MPS),
                main_max_mps: rules::FAR_MAIN_MAX_MPS,
                burst_sf_metal: rules::FAR_BURST_SF_METAL,
                burst_sf_composite: rules::FAR_BURST_SF_COMPOSITE,
                wind_max_mps: rules::FAR_WIND_MAX_MPS,
                elevation_m: rules::FAR_SITE_ELEVATION_M,
                rail_m: rules::FAR_RAIL_20FT_M,
            },
            // IREC DTEG 2025: rail >= 100 ft/s (10.2.1), no TWR rule (1.0 =
            // must lift off), >= 1.5 cal and <= 4 static (10.3.1, 10.4.1),
            // drogue 20-40 m/s (6.3.3), main < 11 m/s (6.4.3), burst 2x metal
            // / 3x composite (6.17.4.1, 6.17.5.1), 1400 m (6.3.2), 5.2 m rail
            // (11.1.2). Wind 20 mph assumed (not in the DTEG excerpt read).
            Competition::Irec2025 => Limits {
                rail_exit_min_mps: 100.0 * FT,
                twr_min: 1.0,
                stability_min_cal: 1.5,
                stability_max_cal: 4.0,
                drogue_mps: (20.0, 40.0),
                main_max_mps: 11.0,
                burst_sf_metal: 2.0,
                burst_sf_composite: 3.0,
                wind_max_mps: 20.0 * 0.44704,
                elevation_m: 1400.0,
                rail_m: 5.2,
            },
        }
    }
}

/// What a person asks for. Everything except the first five fields may be
/// left out; `pinned` fixes the whole vehicle (used to reproduce a
/// published rocket).
fn lower_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = <String as serde::Deserialize>::deserialize(d)?;
    Ok(s.trim().to_ascii_lowercase())
}

/// Oxidiser id: lower case, and the names people use for nitrous mapped to
/// the liquid, self-pressurised `n2o` row ("n2o_gas" is the gas-phase CEA
/// record, kept only for the thermochemistry validation; a flight tank
/// holds liquid).
fn ox_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = lower_id(d)?;
    Ok(match s.as_str() {
        "n2o_gas" | "nitrous" | "nitrous_oxide" | "n2o(l)" => "n2o".into(),
        _ => s,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RocketSpec {
    pub name: String,
    #[serde(default)]
    pub competition: Competition,
    /// Contract (target) apogee above ground, ft.
    pub contract_apogee_ft: f64,
    /// Propellant ids from data/rocket/propellants.json (case-insensitive:
    /// normalised to the catalog's lower case on the way in, so "LOX" cannot
    /// slip past the price lookup and leave the propellant unpriced).
    #[serde(deserialize_with = "ox_id")]
    pub oxidizer: String,
    #[serde(deserialize_with = "lower_id")]
    pub fuel: String,
    /// The person's budget, USD. None = no budget stated: cost is reported
    /// as information and the budget check measures nothing against it
    /// (Pat 2026-10-24: "ignore budget"; same rule as wright-air's
    /// `checks.rs` Budget with `budget_usd: None`).
    #[serde(default)]
    pub budget_usd: Option<f64>,
    #[serde(default)]
    pub payload_kg: f64,
    #[serde(default)]
    pub rail_length_m: Option<f64>,
    /// Weighed dry mass (kg) and CG from the nose tip (m); overrides the
    /// estimate.
    #[serde(default)]
    pub measured_dry: Option<(f64, f64)>,
    /// c* efficiency measured on a test stand (overrides the 0.90 default).
    #[serde(default)]
    pub eta_cstar: Option<f64>,
    /// Nose pressure drag `[mach, cd]` rows from CFD on this nose (the cfd
    /// stage's SU2 sweep, `rocket_su2.json` `nose_points` mach /
    /// cd_pressure), replacing the fineness-scaled TR-R-100 curve in the
    /// drag build-up. Rising Mach, CD on the body cross-section.
    #[serde(default)]
    pub nose_cd_table: Option<Vec<[f64; 2]>>,
    #[serde(default)]
    pub pinned: Option<VehicleParams>,
    /// Calibration: multiply Wright's dry-mass estimate by this (from a
    /// flown vehicle). None = the uncalibrated build-up.
    #[serde(default)]
    pub dry_mass_factor: Option<f64>,
}

impl RocketSpec {
    /// Reject a spec outside the envelope Wright is validated for, with the
    /// rule that bounds it (rules.rs "Spec envelope"). Called by `design`
    /// and `design_under`; the same bounds are in `spec_schema`.
    pub fn validate(&self) -> anyhow::Result<()> {
        let mut bad: Vec<String> = Vec::new();
        let ft = self.contract_apogee_ft;
        match self.competition {
            Competition::FarOut2027 => {
                let (a, b) = (rules::FAR_GROUP_A_FT, rules::FAR_GROUP_B_FT);
                if !((ft >= a.0 && ft <= a.1) || (ft >= b.0 && ft <= b.1)) {
                    bad.push(format!("contract_apogee_ft {ft} is in no FAR-OUT group: A {:.0}-{:.0} ft or B {:.0}-{:.0} ft (FAR-OUT 7.3; Group C is outside Wright's envelope)", a.0, a.1, b.0, b.1));
                }
            }
            Competition::Irec2025 => {
                if !rules::IREC_CATEGORIES_FT.iter().any(|c| (ft - c).abs() <= 1.0) {
                    bad.push(format!("contract_apogee_ft {ft}: IREC liquid categories are 10,000 or 30,000 ft (IREC Rules v1.6 sec. 2.0)"));
                }
            }
        }
        if let Some(b) = self.budget_usd {
            if !(b > 0.0 && b <= rules::BUDGET_MAX_USD) {
                bad.push(format!("budget_usd {b} outside (0, {}]", rules::BUDGET_MAX_USD));
            }
        }
        let (p0, p1) = rules::PAYLOAD_RANGE_KG;
        if !(self.payload_kg >= p0 && self.payload_kg <= p1) {
            bad.push(format!("payload_kg {} outside {p0}-{p1} kg", self.payload_kg));
        }
        if let Some(r) = self.rail_length_m {
            let (lo, hi, rule) = match self.competition {
                Competition::FarOut2027 => (rules::FAR_RAIL_RANGE_M.0, rules::FAR_RAIL_RANGE_M.1, "FAR's pad has 10-60 ft rails (FAR-OUT 5.4.13)"),
                Competition::Irec2025 => (rules::IREC_RAIL_RANGE_M.0, rules::IREC_RAIL_RANGE_M.1, "IREC requires >= 5.2 m (DTEG 11.1.2)"),
            };
            if !(r >= lo - 1e-6 && r <= hi + 1e-6) {
                bad.push(format!("rail_length_m {r} outside {lo:.2}-{hi:.2} m: {rule}"));
            }
        }
        if let Some(e) = self.eta_cstar {
            let (lo, hi) = rules::ETA_CSTAR_RANGE;
            if !(e >= lo && e <= hi) {
                bad.push(format!("eta_cstar {e} outside {lo}-{hi} (above 1 is impossible; SEB demonstrated 0.71)"));
            }
        }
        if let Some(t) = &self.nose_cd_table {
            let (lo, hi) = rules::NOSE_CD_RANGE;
            if t.len() < 2 {
                bad.push("nose_cd_table needs at least 2 [mach, cd] rows".into());
            }
            if t.windows(2).any(|w| w[1][0] <= w[0][0]) {
                bad.push("nose_cd_table Mach must rise".into());
            }
            if t.iter().any(|r| !(r[0] >= 0.0 && r[0] <= 10.0) || !(r[1] >= lo && r[1] <= hi)) {
                bad.push(format!("nose_cd_table rows outside Mach 0-10, cd {lo}-{hi} (a blunt face is {:.2} at M 2)", crate::aero::stagnation_cd(2.0)));
            }
        }
        if let Some(k) = self.dry_mass_factor {
            let (lo, hi) = rules::DRY_MASS_FACTOR_RANGE;
            if !(k >= lo && k <= hi) {
                bad.push(format!("dry_mass_factor {k} outside {lo}-{hi}"));
            }
        }
        if let Some((m, cg)) = self.measured_dry {
            let (m0, m1) = rules::MEASURED_DRY_KG;
            let (c0, c1) = rules::MEASURED_CG_M;
            if !(m >= m0 && m <= m1 && cg > c0 && cg <= c1) {
                bad.push(format!("measured_dry ({m} kg, {cg} m) outside {m0}-{m1} kg, CG {c0}-{c1} m"));
            }
        }
        anyhow::ensure!(bad.is_empty(), "spec outside Wright's envelope: {}", bad.join("; "));
        Ok(())
    }
    /// The dry-mass calibration this spec designs and checks with (the
    /// Eureka-2 calibration unless the spec states one).
    pub fn dry_mass_factor(&self) -> f64 {
        self.dry_mass_factor.unwrap_or(rules::EUREKA2_DRY_MASS_FACTOR)
    }
    pub fn launch(&self, wind: f64) -> Launch {
        let l = self.competition.limits();
        Launch {
            elevation: l.elevation_m,
            rail_length: self.rail_length_m.unwrap_or(l.rail_m),
            rail_tilt: rules::FAR_LAUNCH_ANGLE_FROM_VERTICAL_DEG.to_radians(),
            wind_10m: wind,
            thrust_scale: 1.0,
            drag_scale: 1.0,
        }
    }
    pub fn contract_m(&self) -> f64 {
        self.contract_apogee_ft * FT
    }
}

/// Engine start: a pyrotechnic igniter lit first, then main valves, LOX lead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartSequence {
    /// Solid igniter burn duration, s: an Estes E12-0 motor, 2.60 s
    /// (estesrockets.com, catalog row estes_e12_0).
    pub igniter_burn_s: f64,
    /// Igniter-confirm delay before the LOX main opens, s.
    pub lox_open_s: f64,
    /// Fuel main opens this long after the igniter is lit, s (LOX lead).
    pub fuel_open_s: f64,
    /// Pneumatic ball valve stroke (90 deg), s (assumed).
    pub valve_stroke_s: f64,
    /// Feed line from valve to injector: length (m), inner diameter (m).
    pub line_len_m: f64,
    pub line_id_m: f64,
}

impl Default for StartSequence {
    fn default() -> Self {
        Self { igniter_burn_s: 2.6, lox_open_s: 1.0, fuel_open_s: 1.25, valve_stroke_s: 0.3, line_len_m: 0.4, line_id_m: 0.0102 }
    }
}

impl StartSequence {
    /// Arrival time of each propellant at the injector face: valve opens,
    /// half the stroke to flowing (assumed), then the line + manifold fills
    /// at the steady volumetric flow.
    pub fn arrivals(&self, v: &Vehicle) -> (f64, f64) {
        let line_vol = PI * self.line_id_m.powi(2) / 4.0 * self.line_len_m;
        let q_ox = v.engine.mdot_ox / v.ox_tank.density;
        let q_f = v.engine.mdot_fuel / v.fuel_tank.density;
        (self.lox_open_s + self.valve_stroke_s / 2.0 + line_vol / q_ox, self.fuel_open_s + self.valve_stroke_s / 2.0 + line_vol / q_f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RocketDesign {
    pub params: VehicleParams,
    pub vehicle: Vehicle,
    pub start: StartSequence,
    pub nominal: FlightResult,
    /// (label, apogee AGL m) for the wind/thrust/drag dispersion set.
    pub dispersion: Vec<(String, f64)>,
    pub apogee_band_m: (f64, f64),
    /// The contract apogee AGL (m) the design was made for.
    pub contract_m: f64,
    /// The competition and rail the design was made for (inputs, kept so
    /// `objective`, which sees only the design, can re-fly it; `check`
    /// fails apogee_accuracy if they differ from the spec it is given).
    #[serde(default)]
    pub competition: Competition,
    #[serde(default)]
    pub rail_length_m: Option<f64>,
    pub search_log: Vec<String>,
}

impl RocketDesign {
    /// The launch inputs recorded in the design, as a spec.
    pub fn launch_spec(&self) -> RocketSpec {
        RocketSpec {
            name: String::new(),
            competition: self.competition,
            contract_apogee_ft: self.contract_m / FT,
            oxidizer: self.params.engine.oxidizer.clone(),
            fuel: self.params.engine.fuel.clone(),
            budget_usd: None,
            payload_kg: self.params.payload_kg,
            rail_length_m: self.rail_length_m,
            measured_dry: None,
            eta_cstar: None,
            nose_cd_table: self.params.nose_cd_table.clone(),
            pinned: None,
            dry_mass_factor: None,
        }
    }
}

/// Rebuild and re-fly a design from its inputs (`params`) only: the
/// stored `vehicle`, `nominal`, `dispersion` and `apogee_band_m` are
/// derived and agent-editable, so nothing reported reads them (harness
/// review 2026-10-24: an edited dispersion moved the score 838 -> 1000 and
/// an edited nominal showed a 3278 ft/s rail exit).
pub struct Reflown {
    pub vehicle: Vehicle,
    pub nominal: FlightResult,
    pub dispersion: Vec<(String, f64)>,
}

pub fn reflown(spec: &RocketSpec, d: &RocketDesign) -> anyhow::Result<Reflown> {
    let cat = Catalog::load()?;
    let vehicle = vehicle::build(&d.params, &cat)?;
    let nominal = fly(&vehicle, &spec.launch(wind_cases(spec)[1]));
    let dispersion = dispersion(spec, &vehicle);
    Ok(Reflown { vehicle, nominal, dispersion })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RocketCheck {
    StabilityMin,
    StabilityMax,
    RailExitSpeed,
    ThrustToWeight,
    MaxAcceleration,
    FinFlutter,
    ChamberStress,
    TankBurst,
    TankProof,
    WallTemperature,
    AblativeChar,
    InjectorStiffness,
    InjectorMomentum,
    StartSequence,
    NozzleSeparation,
    PressurantMargin,
    RegulatorRange,
    ValveRating,
    DrogueDescent,
    MainDescent,
    ApogeeAccuracy,
    SpecInputs,
    PartsFit,
    PartsOrderable,
    Budget,
    IspBound,
}

impl CheckId for RocketCheck {
    const ALL: &'static [Self] = &[
        Self::StabilityMin,
        Self::StabilityMax,
        Self::RailExitSpeed,
        Self::ThrustToWeight,
        Self::MaxAcceleration,
        Self::FinFlutter,
        Self::ChamberStress,
        Self::TankBurst,
        Self::TankProof,
        Self::WallTemperature,
        Self::AblativeChar,
        Self::InjectorStiffness,
        Self::InjectorMomentum,
        Self::StartSequence,
        Self::NozzleSeparation,
        Self::PressurantMargin,
        Self::RegulatorRange,
        Self::ValveRating,
        Self::DrogueDescent,
        Self::MainDescent,
        Self::ApogeeAccuracy,
        Self::SpecInputs,
        Self::PartsFit,
        Self::PartsOrderable,
        Self::Budget,
        Self::IspBound,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::StabilityMin => "stability_min",
            Self::StabilityMax => "stability_max",
            Self::RailExitSpeed => "rail_exit_speed",
            Self::ThrustToWeight => "thrust_to_weight",
            Self::MaxAcceleration => "max_acceleration",
            Self::FinFlutter => "fin_flutter",
            Self::ChamberStress => "chamber_stress",
            Self::TankBurst => "tank_burst",
            Self::TankProof => "tank_proof",
            Self::WallTemperature => "wall_temperature",
            Self::AblativeChar => "ablative_char",
            Self::InjectorStiffness => "injector_stiffness",
            Self::InjectorMomentum => "injector_momentum",
            Self::StartSequence => "start_sequence",
            Self::NozzleSeparation => "nozzle_separation",
            Self::PressurantMargin => "pressurant_margin",
            Self::RegulatorRange => "regulator_range",
            Self::ValveRating => "valve_rating",
            Self::DrogueDescent => "drogue_descent",
            Self::MainDescent => "main_descent",
            Self::ApogeeAccuracy => "apogee_accuracy",
            Self::SpecInputs => "spec_inputs",
            Self::PartsFit => "parts_fit",
            Self::PartsOrderable => "parts_orderable",
            Self::Budget => "budget",
            Self::IspBound => "isp_bound",
        }
    }
    fn describe(self) -> &'static str {
        match self {
            Self::StabilityMin => "static margin from rail exit to burnout >= competition minimum (FAR-OUT 5.1.5 / IREC 10.3.1), every wind case",
            Self::StabilityMax => "static margin at rail exit <= over-stability limit (IREC DTEG 10.4.1, 4 cal)",
            Self::RailExitSpeed => "rail exit speed >= competition minimum (FAR-OUT 5.1.4 85 ft/s / IREC 10.2.1 100 ft/s), worst wind",
            Self::ThrustToWeight => "liftoff thrust/weight at site pressure >= competition minimum (FAR-OUT 2027: 8)",
            Self::MaxAcceleration => "peak axial acceleration <= 15 g (Wright structural allowance, unsourced)",
            Self::FinFlutter => "fin flutter speed (NACA TN 4197 eq 18; Apogee Peak of Flight #615 correction of #291) >= 1.2 x speed at max-Q",
            Self::ChamberStress => "Lame thick-wall hoop stress in the chamber case at burst SF x Pc <= ultimate",
            Self::TankBurst => "von Mises of Lame's hoop, axial and radial stresses at the bore of each tank at burst SF x MEOP <= ultimate (FAR-OUT 5.2.2)",
            Self::TankProof => "tank wall and bulkhead (finite-difference plate solution) stress at 1.5 x MEOP <= yield (FAR-OUT 5.7.1 proof, no yield); bulkhead closure screws (shear, bearing, tear-out) carry burst SF x MEOP on ultimate",
            Self::WallTemperature => "aluminium case <= 120 C at burnout: FD transient through liner (chamber and throat), Bartz h",
            Self::AblativeChar => "char front (phenolic >= pyrolysis T) stays inside the liner, chamber and throat",
            Self::InjectorStiffness => "injector dP/Pc >= 15 % on both circuits with drilled orifices (Huzel & Huang eq 4-40): LOX at end of burn; N2O at every instant of the 5/20/30 C blowdowns (Dyer two-phase flow)",
            Self::InjectorMomentum => "LOX pintle: total momentum ratio within 0.5-2 (Wright band); N2O O-F-O triplet: resultant momentum angle |beta| <= 5 deg over every flown blowdown with the paired N2O holes +/-0.05 mm off size (Huzel & Huang eq 4-41)",
            Self::StartSequence => "igniter still burning >= 0.5 s after both propellants reach the injector",
            Self::NozzleSeparation => "pe/pa >= 0.4 at the launch site (Summerfield criterion, Sutton RPE sec. 3.4)",
            Self::PressurantMargin => "usable GN2 (isentropic bottle down to 1.15 x regulated pressure) >= gas needed to fill the displaced propellant volume at the bottle's end temperature; closed form, independent of the time-stepped sizing (fuel tank only when N2O self-pressurises)",
            Self::RegulatorRange => "tank operating pressure inside the regulator's published outlet range",
            Self::ValveRating => "each tank's MEOP <= the published rating of every component on it (main, relief, check, fill/vent valves, hose) and inside the relief's setting range",
            Self::DrogueDescent => "drogue descent rate inside competition band from main deploy to apogee (cap 30k ft)",
            Self::MainDescent => "main descent rate at the ground <= competition maximum",
            Self::ApogeeAccuracy => "nominal apogee, flown at the spec's launch inputs, within 5 % of the spec's contract altitude (Wright requirement)",
            Self::SpecInputs => "the launch inputs the design records (contract, competition, rail) are the spec's (copy consistency, not physics)",
            Self::PartsOrderable => "every BOM line's catalog row is priced and not marked out of stock on its page (unstated stock accepted)",
            Self::PartsFit => "all-pairs interference of the placed parts (wright-geom), plus internal parts inside the airframe ID",
            Self::Budget => "BOM total <= budget and every line priced",
            Self::IspBound => "delivered vacuum Isp <= the ideal (shifting-equilibrium) vacuum Isp, and 0 < c* efficiency <= 1: no engine beats its own thermochemistry",
        }
    }
}

/// The rocket domain.
pub struct Rocket;

/// Engine stock: phenolic rod for liner + nozzle, 6061 bar for case and injector.
/// Engine stock is chosen per design from the catalog (`engine_stock`):
/// the smallest phenolic rod covering the liner/nozzle OD and the smallest
/// 6061 round covering the case OD (a calibrated, heavier vehicle needs a
/// bigger engine than the 3.5 in rod / 4 in bar the first designs used).
pub struct EngineStock<'a> {
    pub rod: Option<&'a crate::catalog::Row>,
    pub bar: Option<&'a crate::catalog::Row>,
    /// Required diameters, mm.
    pub liner_od_mm: f64,
    pub case_od_mm: f64,
}

pub fn engine_stock<'a>(cat: &'a Catalog, e: &crate::engine::Engine) -> EngineStock<'a> {
    let liner_od_mm = (e.chamber_d + 2.0 * e.chamber_wall[0].thickness).max(e.throat_d + 2.0 * e.throat_wall[0].thickness).max(e.exit_d + 0.012) * 1e3;
    let case_od_mm = e.outer_d() * 1e3;
    let pick = |pred: &dyn Fn(&crate::catalog::Row) -> bool, need: f64| {
        let mut rows: Vec<_> = cat.rows.iter().filter(|r| pred(r) && r.f("diameter_mm").is_some() && r.price().is_some()).collect();
        rows.sort_by(|a, b| a.f("diameter_mm").unwrap().total_cmp(&b.f("diameter_mm").unwrap()));
        rows.iter().find(|r| r.f("diameter_mm").unwrap() >= need).or(rows.last()).copied()
    };
    EngineStock {
        rod: pick(&|r| r.role() == "machining" && r.id().contains("phenolic_rod"), liner_od_mm),
        bar: pick(&|r| r.role() == "machining" && r.id().starts_with("al6061_round"), case_od_mm),
        liner_od_mm,
        case_od_mm,
    }
}

/// Descent rate (m/s) of `mass` under a chute of `diameter_in` and `cd` at density `rho`.
pub fn descent_rate(mass: f64, diameter_in: f64, cd: f64, rho: f64) -> f64 {
    let a = PI * (diameter_in * 0.0254 / 2.0).powi(2);
    (2.0 * mass * G0 / (rho * cd * a)).sqrt()
}

/// NACA TN 4197 flutter boundary (Martin 1958, eq 18, p.14):
/// (Vf/a)^2 = G / ([39.3 AR^3 / ((t/c)^3 (AR + 2))] ((lambda + 1)/2) (p/p0)),
/// 39.3 in psi with p0 = 14.696 psi, i.e. a constant 39.3/14.696 = 2.674
/// with p and G in the same units:
/// Vf = a sqrt(G / (2.674 AR^3 p (lambda + 1) / (2 (AR + 2) (t/c)^3))).
/// Apogee Peak of Flight #291 printed 1.337 with the same /2, which
/// overstates Vf by sqrt(2); Apogee corrected it in Peak of Flight #615
/// (J. K. Bennett, Dec 2023). Wright used the #291 form until the hostile
/// review of 2026-10-24.
pub fn flutter_speed(a: f64, p: f64, g_shear: f64, span: f64, root: f64, tip: f64, t: f64) -> f64 {
    let area = 0.5 * (root + tip) * span;
    let ar = span * span / area;
    let lam = tip / root;
    let denom = (39.3 / 14.696) * ar.powi(3) * p * (lam + 1.0) / (2.0 * (ar + 2.0) * (t / root).powi(3));
    a * (g_shear / denom).sqrt()
}

/// G10/FR4 in-plane shear modulus, Pa. No G-10 datasheet publishes one
/// (Gund and Plastics International list E and shear *strength* only).
/// The nearest primary number is G12 = 6.8 GPa measured ultrasonically on
/// the sister laminate NEMA G-11CR (Ledbetter & Austin, NBSIR 83-1690,
/// Table II p.424, nvlpubs.nist.gov); Bennett (Peak of Flight #615) cites
/// direct G10/G12 measurements near 775 ksi (5.3 GPa). Wright uses 4 GPa,
/// below both (static moduli run under ultrasonic ones; assumption).
pub const G10_SHEAR_PA: f64 = 4.0e9;

/// (flutter speed, speed at max-Q), m/s, for fins of thickness `t` on a flight.
fn flutter_at(lim: &Limits, p: &VehicleParams, t: f64, f: &FlightResult) -> (f64, f64) {
    let air = crate::atmosphere::at(lim.elevation_m + f.max_q_alt_agl);
    let vq = (2.0 * f.max_q / air.rho).sqrt();
    (flutter_speed(air.a, air.p, G10_SHEAR_PA, p.fin_span, p.fin_root, p.fin_tip, t), vq)
}
/// Bottles are sized by the time-stepped blowdown (`vehicle::blowdown`)
/// with this margin on end pressure over the regulator dropout, and must
/// also clear the closed-form requirement by 2 %: the two models disagree
/// by 3-10 % on single 4500 psi bottles (all gas at the end temperature vs
/// each parcel at its own), so no pressure margin alone covered both. The
/// pressurant_margin check therefore cannot fail on a searched design; it
/// still fails pinned or edited ones (stated in evals/rocket/result.md).
/// Margin history: 25 % (Wright
/// choice, raised from 25 % to 45 % when the calibrated, heavier designs
/// fell 3-10 % short of the check; not tuned per design).
pub const PRESSURANT_SIZING_MARGIN: f64 = 0.45;
/// Most pressurant bottles the search stacks in the bay.
pub const MAX_BOTTLES: usize = 10;
/// Peak axial acceleration allowance, g (unsourced).
pub const MAX_ACCEL_G: f64 = 15.0;

fn wind_cases(spec: &RocketSpec) -> [f64; 3] {
    let w = spec.competition.limits().wind_max_mps;
    [0.0, w / 2.0, w]
}

/// Mass on the chutes: the vehicle after burnout (residuals and remaining GN2 aboard).
fn recovery_mass(v: &Vehicle) -> f64 {
    v.mass_at(v.burn_time + 1.0).mass
}

fn pick_chutes(cat: &Catalog, spec: &RocketSpec, v: &Vehicle, apogee_agl: f64) -> (String, String) {
    let lim = spec.competition.limits();
    let m = recovery_mass(v);
    let elev = lim.elevation_m;
    let rho_top = crate::atmosphere::at(elev + apogee_agl.min(30_000.0 * FT)).rho;
    let rho_low = crate::atmosphere::at(elev + rules::MAIN_DEPLOY_AGL_M).rho;
    let rho_gnd = crate::atmosphere::at(elev).rho;
    let mut chutes: Vec<_> = cat.role("parachute").filter(|c| c.orderable()).collect();
    chutes.sort_by(|a, b| a.price().unwrap_or(1e9).total_cmp(&b.price().unwrap_or(1e9)));
    let drogue = chutes
        .iter()
        .filter(|c| {
            let (d, cd) = (c.f("diameter_in").unwrap(), c.f("cd").unwrap());
            descent_rate(m, d, cd, rho_top) <= lim.drogue_mps.1 * 0.95 && descent_rate(m, d, cd, rho_low) >= lim.drogue_mps.0 * 1.05
        })
        .min_by(|a, b| (a.f("diameter_in").unwrap()).total_cmp(&b.f("diameter_in").unwrap()))
        .map(|c| c.id().to_string())
        .unwrap_or_else(|| "fc_classic_elliptical_36in".into());
    let main = chutes
        .iter()
        .filter(|c| descent_rate(m, c.f("diameter_in").unwrap(), c.f("cd").unwrap(), rho_gnd) <= lim.main_max_mps * 0.95)
        .filter(|c| c.in_stock())
        .min_by(|a, b| a.f("diameter_in").unwrap().total_cmp(&b.f("diameter_in").unwrap()))
        .map(|c| c.id().to_string())
        .unwrap_or_else(|| "fc_iris_ultra_compact_168in".into());
    (drogue, main)
}

/// Fly the dispersion set: winds x thrust +-5 % x drag +-10 %.
pub fn dispersion(spec: &RocketSpec, v: &Vehicle) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for w in wind_cases(spec) {
        for (ts, ds) in [(1.0, 1.0), (0.95, 1.10), (1.05, 0.90)] {
            let mut l = spec.launch(w);
            l.thrust_scale = ts;
            l.drag_scale = ds;
            let r = fly(v, &l);
            out.push((format!("wind {:.1} m/s, thrust x{ts}, drag x{ds}", w), r.apogee_agl));
        }
    }
    out
}

fn fins_for(d: f64, s: f64) -> (f64, f64, f64, f64) {
    // Clipped delta family scaled by s: root 2.0 d, tip 0.8 d, span s d, LE sweep 1.0 s d.
    (2.0 * d, 0.8 * d, s * d, 1.0 * s * d)
}

/// If the chamber liner would not fit the largest priced phenolic rod,
/// lower the contraction ratio until it does (not below
/// `rules::CONTRACTION_MIN`); a liner that fits keeps the correlation.
fn fit_contraction(params: &mut VehicleParams, cat: &Catalog) -> anyhow::Result<()> {
    let v = vehicle::build(params, cat)?;
    let e = &v.engine;
    let st = engine_stock(cat, e);
    let largest = cat.rows.iter().filter(|r| r.role() == "machining" && r.id().contains("phenolic_rod") && r.price().is_some()).filter_map(|r| r.f("diameter_mm")).fold(0.0_f64, f64::max);
    let chamber_blank = (e.chamber_d + 2.0 * e.chamber_wall[0].thickness) * 1e3;
    if st.rod.and_then(|r| r.f("diameter_mm")).is_some_and(|d| d >= st.liner_od_mm) || largest <= 0.0 || chamber_blank <= largest {
        return Ok(());
    }
    // Chamber bore that leaves the liner inside the rod with 0.5 mm to spare.
    let dc = (largest - 0.5) * 1e-3 - 2.0 * e.chamber_wall[0].thickness;
    let cr = (dc / e.throat_d).powi(2);
    if cr < e.contraction {
        params.engine.contraction = Some(cr.max(rules::CONTRACTION_MIN));
    }
    Ok(())
}

/// The requirements-only design search.
pub fn search(spec: &RocketSpec, cat: &Catalog) -> anyhow::Result<RocketDesign> {
    let lim = spec.competition.limits();
    let ox = reactant(&spec.oxidizer)?;
    let fuel = reactant(&spec.fuel)?;
    let pa_site = crate::atmosphere::at(lim.elevation_m).p;
    // Self-pressurised N2O: run-tank blowdown, unlike-doublet injector,
    // GN2-regulated fuel. Refused with cryogenic fuels (see engine::design).
    let selfp = ox.self_pressurizing;
    anyhow::ensure!(
        !(selfp && fuel.cryogenic),
        "{} with cryogenic {} is not modelled: a room-temperature N2O run tank beside a cryogenic fuel tank (N2O freezes at 182 K) has no student precedent Wright could find; pick propane, ethanol, ipa or rp1",
        ox.id,
        fuel.id
    );
    let mut log = Vec::new();
    let mut best: Option<(f64, f64, f64, RocketDesign)> = None;
    let reg = cat
        .role("regulator")
        .filter(|r| r.orderable())
        .max_by(|a, b| a.f("outlet_psi_max").unwrap().total_cmp(&b.f("outlet_psi_max").unwrap()))
        .ok_or_else(|| anyhow::anyhow!("no regulator in catalog"))?;
    let reg_max = reg.f("outlet_psi_max").unwrap() * 6894.757;
    let reg_min = reg.f("outlet_psi_min").unwrap_or(0.0) * 6894.757;
    let feed_loss = 0.10;
    // N2O at 20 C sits at ~50 bar, so its engines run lower Pc than the
    // regulator-limited LOX ones.
    let pcs: &[f64] = if selfp { &[15.0, 17.5, 20.0, 22.5, 25.0, 27.5, 30.0] } else { &[20.0, 25.0, 30.0, 32.5, 35.0, 37.5] };
    // Under-expanded nozzles (exit at 1.25-2x site pressure) trade a few
    // percent of Isp for an exit that fits the largest priced phenolic rod
    // (4.5 in): the calibrated, heavier vehicles need 8+ kN engines.
    let pes: &[f64] = &[1.0, 1.25, 1.5, 2.0];
    let cands: Vec<(f64, f64)> = pcs.iter().flat_map(|&p| pes.iter().map(move |&f| (p, f))).collect();
    'cand: for (pc_bar, pe_factor) in cands {
        // A candidate whose vehicle does not build (e.g. an N2O tank that
        // would be liquid-full) is dropped, not the whole search.
        macro_rules! build_or_skip {
            ($p:expr) => {
                match vehicle::build(&$p, cat) {
                    Ok(v) => v,
                    Err(e) => {
                        log.push(format!("Pc {pc_bar} bar pe/pa {pe_factor}: vehicle does not build ({e:#}); candidate dropped"));
                        continue 'cand;
                    }
                }
            };
        }
        let pc = pc_bar * 1e5;
        // pe_factor > 1: nozzle exit pressure above site ambient (shorter,
        // narrower nozzle; trades ~1 % Isp for stock fit).
        let pa_design = pa_site * pe_factor;
        // The regulated tank(s): both for LOX; the fuel tank alone for N2O.
        let p_op = if selfp { engine::n2o_fuel_tank_pressure(pc)? } else { pc * (1.0 + rules::INJ_DP_DESIGN) / (1.0 - feed_loss) };
        if p_op > reg_max * 0.98 || p_op < reg_min * 1.02 {
            log.push(format!("Pc {pc_bar} bar: tank {:.0} psi outside regulator outlet range, skipped", p_op / 6894.757));
            continue;
        }
        // The pintle's total momentum ratio with one regulator on both tanks
        // (equal injector drops) is TMR = O/F sqrt(rho_fuel / rho_ox), set by
        // O/F alone; cap O/F where it stays in the band (5 % inside). Costs
        // nothing for LOX/propane (optimum ~2.4, cap ~2.7) and forces N2O/
        // ethanol fuel-rich (cap ~1.8): a live run picked N2O and the search,
        // tuned on LOX, returned TMR 3.7 (2026-10-24).
        // The N2O doublet has no such coupling (its angles absorb the
        // momentum ratio), so N2O takes its Isp optimum (~4 with ethanol).
        let tmr_max = rules::PINTLE_TMR_RANGE.1 * 0.95;
        let of_cap = tmr_max / (fuel.density_kg_m3 / ox.density_kg_m3).sqrt();
        let (of_lo, of_hi) = if selfp { (1.0, 9.0) } else { (1.0_f64.min(of_cap), 3.6_f64.min(of_cap)) };
        let of = engine::best_of(&ox, &fuel, pc, pa_site, of_lo, of_hi)?;
        // A measured c* efficiency also enters the schema/headlines via the engine spec.
        let d = 0.15672;
        let mut thrust = 4000.0;
        let mut s_fin = 1.0;
        let mut burn = 6.0;
        let mut params = VehicleParams {
            engine: EngineSpec { oxidizer: spec.oxidizer.clone(), fuel: spec.fuel.clone(), pc, of, thrust, pa_design, burn_time: burn, eta_cstar: spec.eta_cstar, inj_dp: None , contraction: None },
            tank_tube: "al6061_tube_6p0od_0p125w_72in".into(),
            airframe_tube: "wildman_g12_6in_60in".into(),
            nose: "wildman_fnc6_vk_5to1".into(),
            fin_material: "wildman_g10_g10_3_16".into(),
            fin_count: 4,
            fin_root: 0.0,
            fin_tip: 0.0,
            fin_span: 0.0,
            fin_sweep: 0.0,
            drogue: "fc_classic_elliptical_24in".into(),
            main: "fc_iris_ultra_compact_96in".into(),
            bottle: String::new(),
            bottles: 1,
            regulator: Some(reg.id().to_string()),
            valve: cat.first_orderable(&MAIN_VALVES),
            altimeters: vec![cat.first_orderable(&["featherweight_blue_raven", "altus_telemega"]), cat.first_orderable(&["altus_easymini", "eggtimer_quasar", "altus_telemega"])],
            tracker: "featherweight_gps_tracker".into(),
            payload_kg: spec.payload_kg,
            feed_loss,
            body_diameter: None,
            extra_length: 0.0,
            measured_dry: spec.measured_dry,
            tank_pressure: None,
            ox_fill_k: None,
            dry_mass_factor: Some(spec.dry_mass_factor()),
            nose_cd_table: spec.nose_cd_table.clone(),
        };
        let set_fins = |p: &mut VehicleParams, s: f64| {
            let (r, t, sp, sw) = fins_for(d, s);
            p.fin_root = r;
            p.fin_tip = t;
            p.fin_span = sp;
            p.fin_sweep = sw;
        };
        set_fins(&mut params, s_fin);
        // Bottles: cheapest (type, count) whose blowdown margin passes,
        // re-chosen each outer iteration as tank volumes change.
        // Only bottles with a published mass, price and size that fits the
        // airframe bore with 10 mm radial clearance for lines.
        let bore = cat.get("wildman_g12_6in_60in").and_then(|r| r.f("id_mm")).unwrap_or(152.4);
        let mut bottles: Vec<_> = cat
            .role("pressurant_tank")
            .filter(|b| b.orderable())
            .filter(|b| b.f("mass_kg").is_some() && b.price().is_some() && b.f("length_mm").is_some() && b.f("diameter_mm").is_some_and(|d| d <= bore - 20.0))
            .collect();
        bottles.sort_by(|a, b| a.price().unwrap().total_cmp(&b.price().unwrap()));
        let choose_bottle = |p: &mut VehicleParams| -> anyhow::Result<()> {
            let mut opts = Vec::new();
            for b in &bottles {
                for n in 1..=MAX_BOTTLES {
                    let mut q = p.clone();
                    q.bottle = b.id().to_string();
                    q.bottles = n;
                    if let Ok(v) = vehicle::build(&q, cat) {
                        if vehicle::pressurant_margin(&v) >= PRESSURANT_SIZING_MARGIN && vehicle::pressurant_requirement(&v).0 >= 0.02 {
                            opts.push((b.price().unwrap() * n as f64 + 50.0 * n as f64, q.bottle.clone(), n));
                        }
                    }
                }
            }
            match opts.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)) {
                Some(o) => {
                    p.bottle = o.1;
                    p.bottles = o.2;
                    Ok(())
                }
                None => {
                    // Nothing feeds this (possibly over-sized) iterate: carry
                    // the largest set forward and let the burn shrink; the
                    // final pressurant check decides.
                    let big = bottles.iter().max_by(|a, b| a.f("volume_l").unwrap().total_cmp(&b.f("volume_l").unwrap())).ok_or_else(|| anyhow::anyhow!("no usable bottles"))?;
                    p.bottle = big.id().to_string();
                    p.bottles = MAX_BOTTLES;
                    Err(anyhow::anyhow!("no bottle set feeds the tanks"))
                }
            }
        };
        params.bottle = bottles.first().map(|b| b.id().to_string()).unwrap_or_default();
        let mut ok = true;
        for it in 0..5 {
            if let Err(e) = choose_bottle(&mut params) {
                log.push(format!("Pc {pc_bar} iter {it}: {e}; carrying 6 x {}", params.bottle));
                if it == 4 {
                    ok = false;
                }
            }
            // Burn time for the contract apogee at mid wind.
            let target = spec.contract_m();
            let (mut lo, mut hi) = (if selfp { 0.3 } else { 2.0 }, 60.0);
            for _ in 0..28 {
                let mid = 0.5 * (lo + hi);
                params.engine.burn_time = mid;
                let v = build_or_skip!(params);
                let r = fly(&v, &spec.launch(wind_cases(spec)[1]));
                if r.apogee_agl < target { lo = mid } else { hi = mid }
            }
            burn = 0.5 * (lo + hi);
            params.engine.burn_time = burn;
            // Thrust for the TWR rule with 10 % margin (and >= 1.3 g for IREC).
            let v = build_or_skip!(params);
            let m0 = v.mass_at(0.0).mass;
            // ...and enough to leave the rail at the rule's speed: on a rail
            // of length L, v^2 = 2 (T/W - 1) g L, so T/W = 1 + v^2 / (2 g L),
            // +15 % for drag, friction and the launch angle. IREC's 100 ft/s
            // off 5.2 m needs ~10; the old floor of 5 made IREC unreachable
            // (agents "fixed" it by lengthening the rail; live run 2026-10-24).
            let rail = spec.rail_length_m.unwrap_or(lim.rail_m);
            let twr_rail = 1.0 + lim.rail_exit_min_mps.powi(2) / (2.0 * G0 * rail);
            let mut twr_target = (lim.twr_min * 1.10).max(5.0).max(twr_rail * 1.15);
            // N2O: liftoff thrust falls with the tank temperature; size for
            // the cold end of the fill band so the rail and T/W rules hold
            // on a cold morning too.
            // The hot end then accelerates hardest, and at IREC (100 ft/s
            // off 5.2 m) the window between the cold rail exit and the hot
            // 15 g allowance is narrow, so instead of the 15 % rule of thumb
            // size from the flown cold-day rail exit: (T/W - 1) scales with
            // v_exit^2 on a fixed rail; aim 4 % above the rule.
            if selfp {
                let mut cold = params.clone();
                cold.ox_fill_k = Some(rules::N2O_FILL_COLD_K);
                let vc = build_or_skip!(cold);
                let fc = fly(&vc, &spec.launch(lim.wind_max_mps));
                let ratio = (vc.thrust_at(0.0, pa_site) / v.thrust_at(0.0, pa_site)).min(1.0);
                let cold_twr = |k: f64| (1.0 + (fc.liftoff_twr - 1.0) * (k * lim.rail_exit_min_mps / fc.rail_exit_speed).powi(2)).max(lim.twr_min * 1.05);
                twr_target = cold_twr(1.04) / ratio;
                // Then the hot end: peak acceleration scales with T/W; if
                // the 4 % rail margin would push the hot day past 98 % of the
                // allowance, give margin back (down to 1 % on the rail).
                let mut hot = params.clone();
                hot.ox_fill_k = Some(rules::N2O_FILL_HOT_K);
                let fh = fly(&build_or_skip!(hot), &spec.launch(wind_cases(spec)[1]));
                let twr_now = v.thrust_at(0.0, pa_site) / (m0 * G0);
                let twr_hot_cap = twr_now * 0.98 * MAX_ACCEL_G / fh.max_accel_g;
                twr_target = twr_target.min(twr_hot_cap).max(cold_twr(1.01) / ratio);
            }
            let new_thrust = params.engine.thrust * (twr_target * m0 * G0 / v.thrust_at(0.0, pa_site));
            if !(new_thrust.is_finite() && new_thrust > 0.0 && new_thrust < 1e6) {
                // e.g. the cold-day N2O feed cannot sustain combustion at
                // this Pc: the cold/nominal thrust ratio is 0.
                log.push(format!("Pc {pc_bar} bar pe/pa {pe_factor} iter {it}: no finite thrust meets T/W and rail exit (cold-day thrust {:.0} N); candidate dropped", v.thrust_at(0.0, pa_site)));
                ok = false;
                break;
            }
            thrust = new_thrust;
            params.engine.thrust = thrust;
            // Fins: smallest scale whose minimum margin through the burn
            // (all winds) is >= stability_min + 0.3.
            let need = lim.stability_min_cal + 0.3;
            let (mut lo, mut hi) = (0.3, 3.0);
            for _ in 0..20 {
                let mid = 0.5 * (lo + hi);
                set_fins(&mut params, mid);
                let v = build_or_skip!(params);
                let m = wind_cases(spec).iter().map(|&w| fly(&v, &spec.launch(w)).margin_min).fold(f64::MAX, f64::min);
                if m < need { lo = mid } else { hi = mid }
            }
            s_fin = hi;
            set_fins(&mut params, s_fin);
            // Liner to stock: lower Ac/At within Huzel & Huang's range if needed.
            if let Err(e) = fit_contraction(&mut params, cat) {
                log.push(format!("Pc {pc_bar} bar pe/pa {pe_factor}: {e:#}; candidate dropped"));
                continue 'cand;
            }
            // Fin stock: the thinnest G10 sheet whose flutter speed clears
            // 1.2 x the max-Q speed with 10 % to spare on the nominal flight
            // (thicker if none does: the check then reports it).
            let mut sheets: Vec<_> = cat.role("fin_material").filter_map(|r| r.f("thickness_mm").map(|t| (t, r.id().to_string()))).collect();
            sheets.sort_by(|a, b| a.0.total_cmp(&b.0));
            let vq_flight = fly(&build_or_skip!(params), &spec.launch(wind_cases(spec)[1]));
            if let Some((_, id)) = sheets.iter().find(|(t, _)| {
                let (vf, vq) = flutter_at(&lim, &params, t * 1e-3, &vq_flight);
                vf >= 1.2 * 1.10 * vq
            }).or(sheets.last()) {
                params.fin_material = id.clone();
            }
            log.push(format!("Pc {pc_bar} bar pe/pa {pe_factor} O/F {of:.2} iter {it}: burn {burn:.2} s, thrust {thrust:.0} N, fin scale {s_fin:.2}, m0 {m0:.1} kg"));
        }
        if !ok {
            continue;
        }
        // The last iteration resized thrust and fins after its burn-time
        // bisection; re-hit the contract apogee with burn time alone (the
        // N2O blowdown's thrust curve made the loop end up to 5 % short).
        {
            let target = spec.contract_m();
            let (mut lo, mut hi) = (0.5 * params.engine.burn_time, 2.0 * params.engine.burn_time);
            for _ in 0..24 {
                let mid = 0.5 * (lo + hi);
                params.engine.burn_time = mid;
                let v = build_or_skip!(params);
                if fly(&v, &spec.launch(wind_cases(spec)[1])).apogee_agl < target { lo = mid } else { hi = mid }
            }
            params.engine.burn_time = 0.5 * (lo + hi);
        }
        if let Err(e) = fit_contraction(&mut params, cat) {
            log.push(format!("Pc {pc_bar} bar pe/pa {pe_factor}: {e:#}; candidate dropped"));
            continue;
        }
        // The longer or shorter burn changes the fuel the pressurant must
        // displace: re-choose the bottles for the final burn.
        if let Err(e) = choose_bottle(&mut params) {
            log.push(format!("Pc {pc_bar} bar pe/pa {pe_factor}: after the final burn {e}; candidate dropped"));
            continue;
        }
        // Chutes for the resulting apogee.
        let v = build_or_skip!(params);
        let nominal = fly(&v, &spec.launch(wind_cases(spec)[1]));
        let (dr, mn) = pick_chutes(cat, spec, &v, nominal.apogee_agl);
        params.drogue = dr;
        params.main = mn;
        let d = match finish(spec, cat, params.clone(), StartSequence::default(), log.clone()) {
            Ok(d) => d,
            Err(e) => {
                log.push(format!("Pc {pc_bar} bar pe/pa {pe_factor}: final vehicle does not build ({e:#}); candidate dropped"));
                continue;
            }
        };
        let fin = Rocket::check(spec, &d);
        let viol = fin.violation(|_| 1.0);
        let cost = Rocket::bom(&d).total().as_usd();
        log.push(format!("Pc {pc_bar} bar pe/pa {pe_factor}: {} failing checks, BOM ${cost:.0}, score {:.0}", fin.failures().count(), far_score_of(&d.dispersion, spec.contract_m())));
        let score = far_score_of(&d.dispersion, spec.contract_m());
        if best.as_ref().is_none_or(|b| ranks_before((viol, score, cost), (b.0, b.1, b.2))) {
            best = Some((viol, score, cost, d));
        }
    }
    let (_, _, _, mut d) = best.ok_or_else(|| anyhow::anyhow!("no chamber pressure produced a design"))?;
    d.search_log = log;
    Ok(d)
}

/// Checks that compare the design's copy of the spec's inputs with the
/// spec (not physics): a requirement-change attack makes them unpassable by
/// construction, so the harness must not count them as confirmed attacks
/// (review 5 F1; same as wright-air `checks::CONSISTENCY_CHECKS`).
pub const CONSISTENCY_CHECKS: &[&str] = &["spec_inputs"];

/// Main propellant valves in order of preference; the search takes the
/// first one orderable (review 5 F4: the double-acting valve went out of
/// stock).
pub const MAIN_VALVES: [&str; 3] = ["valveman_cryo_ball_1_2_double_acting", "valveman_cryo_ball_1_2_spring_return", "valveman_cryo_ball_3_4_double_acting"];

/// Candidate order: least check violation, then the higher FAR-OUT score
/// (scores within half a point tie: the dispersion mean is not that
/// precise), then the lower BOM. Cost is only a tie-break; the budget,
/// when stated, is a check like any other (in the violation).
pub fn ranks_before(a: (f64, f64, f64), b: (f64, f64, f64)) -> bool {
    let (va, sa, ca) = a;
    let (vb, sb, cb) = b;
    if (va - vb).abs() > 1e-9 {
        return va < vb;
    }
    if (sa - sb).abs() > 0.5 {
        return sa > sb;
    }
    ca < cb
}

/// Build, fly nominal + dispersion, and package a design from fixed params.
pub fn finish(spec: &RocketSpec, cat: &Catalog, params: VehicleParams, start: StartSequence, log: Vec<String>) -> anyhow::Result<RocketDesign> {
    let v = vehicle::build(&params, cat)?;
    let nominal = fly(&v, &spec.launch(wind_cases(spec)[1]));
    let disp = dispersion(spec, &v);
    let lo = disp.iter().map(|d| d.1).fold(f64::MAX, f64::min);
    let hi = disp.iter().map(|d| d.1).fold(f64::MIN, f64::max);
    Ok(RocketDesign { params, vehicle: v, start, nominal, dispersion: disp, apogee_band_m: (lo, hi), contract_m: spec.contract_m(), competition: spec.competition, rail_length_m: spec.rail_length_m, search_log: log })
}

/// Lame hoop stress at the bore of a thick cylinder, Pa.
pub fn lame_hoop(p: f64, ri: f64, ro: f64) -> f64 {
    p * (ro * ro + ri * ri) / (ro * ro - ri * ri)
}

/// Von Mises stress at the bore of a closed-end thick cylinder from Lame's
/// three principal stresses: hoop p(ro^2+ri^2)/(ro^2-ri^2), radial -p,
/// axial (end caps) p ri^2/(ro^2-ri^2) (Roark 8th ed. Table 13.5 case 1c;
/// Shigley eq 3-49/3-50). FEA worker, 2026-10-24: hoop alone overstates the
/// equivalent stress by ~9 % on the tanks.
pub fn lame_von_mises(p: f64, ri: f64, ro: f64) -> (f64, f64, f64, f64) {
    let k = ro * ro - ri * ri;
    let hoop = p * (ro * ro + ri * ri) / k;
    let axial = p * ri * ri / k;
    let radial = -p;
    let vm = (((hoop - axial).powi(2) + (axial - radial).powi(2) + (radial - hoop).powi(2)) / 2.0).sqrt();
    (vm, hoop, axial, radial)
}

fn run_checks(spec: &RocketSpec, d: &RocketDesign, cat: &Catalog, r: &mut Report<RocketCheck>) {
    use RocketCheck as C;
    let lim = spec.competition.limits();
    let v = match vehicle::build(&d.params, cat) {
        Ok(v) => v,
        Err(e) => {
            for &c in RocketCheck::ALL {
                r.missing(c, Source::Physics, format!("vehicle does not build: {e}"));
            }
            return;
        }
    };
    let e = &v.engine;
    let flights: Vec<(f64, FlightResult)> = wind_cases(spec).iter().map(|&w| (w, fly(&v, &spec.launch(w)))).collect();
    let nominal = &flights[1].1;
    // Self-pressurised N2O: the same vehicle flown with the run tank at the
    // cold and hot ends of the fill band (vapour pressure, so thrust, Pc,
    // O/F and injector drops all move with it).
    let selfp = v.profile.is_some();
    let variants: Vec<(String, Result<Vehicle, String>)> = if selfp {
        [rules::N2O_FILL_COLD_K, rules::N2O_FILL_HOT_K]
            .iter()
            .map(|&k| {
                let mut p = d.params.clone();
                p.ox_fill_k = Some(k);
                (format!("N2O at {:.0} C", k - 273.15), vehicle::build(&p, cat).map_err(|e| format!("{e:#}")))
            })
            .collect()
    } else {
        vec![]
    };
    let profiles: Vec<(String, &crate::engine::BurnProfile)> = v
        .profile
        .iter()
        .map(|p| (format!("N2O at {:.0} C", p.fill_k - 273.15), p))
        .chain(variants.iter().filter_map(|(l, x)| x.as_ref().ok().and_then(|x| x.profile.as_ref()).map(|p| (l.clone(), p))))
        .collect();
    for (label, x) in &variants {
        match x {
            Ok(vx) => {
                // Worst wind for the rail; the stability and acceleration
                // rules hold on every flight day.
                let f = fly(vx, &spec.launch(lim.wind_max_mps));
                r.margin(C::RailExitSpeed, f.rail_exit_speed - lim.rail_exit_min_mps, "m/s", Source::Physics, format!("{:.1} m/s ({:.0} ft/s), {label}, wind {:.1}; limit {:.0} ft/s", f.rail_exit_speed, f.rail_exit_speed / FT, lim.wind_max_mps, lim.rail_exit_min_mps / FT));
                r.margin(C::ThrustToWeight, f.liftoff_twr - lim.twr_min, "", Source::Physics, format!("T/W {:.2}, {label}; limit {:.1}", f.liftoff_twr, lim.twr_min));
                r.margin(C::StabilityMin, f.margin_min - lim.stability_min_cal, "cal", Source::Physics, format!("min {:.2} cal, {label}; limit {:.1}", f.margin_min, lim.stability_min_cal));
                r.margin(C::MaxAcceleration, MAX_ACCEL_G - f.max_accel_g, "g", Source::Physics, format!("{:.1} g, {label}", f.max_accel_g));
            }
            Err(err) => {
                for c in [C::RailExitSpeed, C::ThrustToWeight, C::StabilityMin, C::MaxAcceleration, C::InjectorStiffness, C::InjectorMomentum, C::NozzleSeparation, C::ChamberStress] {
                    r.missing(c, Source::Physics, format!("{label}: the vehicle does not fly: {err}"));
                }
            }
        }
    }
    // Chamber pressure extremes over every flown burn (constant for LOX).
    let pc_max = profiles.iter().flat_map(|(_, p)| p.pc.iter().copied()).fold(e.spec.pc, f64::max);
    let pc_min = profiles.iter().flat_map(|(_, p)| p.pc.iter().copied()).fold(e.spec.pc, f64::min);
    let burn_max = variants.iter().filter_map(|(_, x)| x.as_ref().ok().map(|x| x.burn_time)).fold(v.burn_time, f64::max);

    // Stability through the burn, every wind.
    for (w, f) in &flights {
        r.margin(C::StabilityMin, f.margin_min - lim.stability_min_cal, "cal", Source::Physics, format!("min {:.2} cal (rail exit {:.2}) at wind {:.1} m/s; limit {:.1}", f.margin_min, f.margin_at_rail_exit, w, lim.stability_min_cal));
        r.margin(C::StabilityMax, lim.stability_max_cal - f.margin_at_rail_exit, "cal", Source::Physics, format!("rail exit {:.2} cal; limit {:.1}", f.margin_at_rail_exit, lim.stability_max_cal));
        r.margin(C::RailExitSpeed, f.rail_exit_speed - lim.rail_exit_min_mps, "m/s", Source::Physics, format!("{:.1} m/s ({:.0} ft/s) at wind {:.1}; limit {:.0} ft/s", f.rail_exit_speed, f.rail_exit_speed / FT, w, lim.rail_exit_min_mps / FT));
        r.margin(C::MaxAcceleration, MAX_ACCEL_G - f.max_accel_g, "g", Source::Physics, format!("{:.1} g", f.max_accel_g));
    }
    r.margin(C::ThrustToWeight, nominal.liftoff_twr - lim.twr_min, "", Source::Physics, format!("T/W {:.2} at {:.0} m site pressure; limit {:.1}", nominal.liftoff_twr, lim.elevation_m, lim.twr_min));

    // Fin flutter at max-Q, nominal flight and (N2O) every fill temperature.
    match cat.get(&d.params.fin_material).and_then(|f| f.f("thickness_mm")) {
        Some(t_mm) => {
            let extra: Vec<(String, FlightResult)> = variants.iter().filter_map(|(l, x)| x.as_ref().ok().map(|vx| (l.clone(), fly(vx, &spec.launch(wind_cases(spec)[1]))))).collect();
            for (label, f) in std::iter::once(("nominal".to_string(), nominal.clone())).chain(extra) {
                let (vf, vq) = flutter_at(&lim, &d.params, t_mm * 1e-3, &f);
                r.margin(C::FinFlutter, vf / (1.2 * vq) - 1.0, "", Source::Physics, format!("{label}: flutter {:.0} m/s vs 1.2 x {:.0} m/s at max-Q ({:.1} mm G10)", vf, vq, t_mm));
            }
        }
        None => r.missing(C::FinFlutter, Source::Physics, "fin material not in catalog or has no thickness"),
    }

    // Chamber case: Lame at burst SF x Pc against ultimate.
    let ri = e.chamber_d / 2.0 + e.chamber_wall[0].thickness;
    let ro = ri + e.chamber_wall[1].thickness;
    let s = lame_hoop(lim.burst_sf_metal * pc_max, ri, ro);
    r.margin(C::ChamberStress, rules::AL6061_ULT_PA / s - 1.0, "", Source::Physics, format!("hoop {:.0} MPa at {:.1}x Pc {:.1} bar (highest flown) vs Ftu {:.0} MPa", s / 1e6, lim.burst_sf_metal, pc_max / 1e5, rules::AL6061_ULT_PA / 1e6));

    // Tanks: burst (Lame vs ultimate) and proof (wall + bulkhead vs yield).
    for t in [&v.ox_tank, &v.fuel_tank] {
        let (ri, ro) = (t.inner_d / 2.0, t.inner_d / 2.0 + t.wall);
        let (s, hoop, ax, rad) = lame_von_mises(lim.burst_sf_metal * t.meop, ri, ro);
        r.margin(C::TankBurst, rules::AL6061_ULT_PA / s - 1.0, "", Source::Physics, format!("{}: von Mises {:.0} MPa (hoop {:.0}, axial {:.0}, radial {:.0}) at {:.1}x MEOP {:.1} bar", t.name, s / 1e6, hoop / 1e6, ax / 1e6, rad / 1e6, lim.burst_sf_metal, t.meop / 1e5));
        let pp = 1.5 * t.meop;
        let (wall, wall_hoop, _, _) = lame_von_mises(pp, ri, ro);
        // Bulkhead: the numerical plate solution (not the sizing formula).
        let bh = vehicle::plate_stress_fd(ri, t.bulkhead_t, pp);
        let worst = wall.max(bh);
        // The wall comes from the catalog tube (not sized). The bulkhead is
        // checked with the finite-difference plate solution, derived apart
        // from the Roark expression it is sized with.
        r.margin(C::TankProof, rules::AL6061_YIELD_PA / worst - 1.0, "", Source::Physics, format!("{}: proof {:.1} bar (tank alone, ports plugged) -> wall von Mises {:.0} MPa (hoop {:.0}), bulkhead {:.0} MPa vs Fty {:.0}", t.name, pp / 1e5, wall / 1e6, wall_hoop / 1e6, bh / 1e6, rules::AL6061_YIELD_PA / 1e6));
        // Closure: the bulkhead's pressure load at burst SF x MEOP carried by
        // its radial screws, each mode on ultimate (screw shear, wall
        // bearing, tear-out); sized on bearing yield at proof.
        let n = t.closure_screws;
        if n == 0 {
            r.missing(C::TankProof, Source::Physics, format!("{}: no bulkhead closure", t.name));
        } else {
            let f = lim.burst_sf_metal * t.meop * PI * ri * ri / n as f64;
            let (shear, bearing, tear) = vehicle::closure_capacity(t.wall);
            let cap = shear.min(bearing).min(tear);
            let mode = if cap == shear { "screw shear" } else if cap == bearing { "wall bearing" } else { "tear-out" };
            r.margin(C::TankProof, cap / f - 1.0, "", Source::Physics, format!("{}: bulkhead closure {n} x 1/4-20 screws at {:.1}x MEOP: {:.1} kN per screw vs {:.1} kN ({mode})", t.name, lim.burst_sf_metal, f / 1e3, cap / 1e3));
        }
    }

    // Thermal: FD transient, chamber and throat.
    // Bartz h scales as Pc^0.8 (Huzel & Huang eq 4-13): the highest flown Pc
    // held for the longest flown burn (conservative pairing; both 1 for LOX).
    let h_scale = (pc_max / e.spec.pc).powf(0.8);
    for (label, wall, h, taw) in [("chamber", &e.chamber_wall, e.h_chamber * h_scale, e.t_aw_chamber), ("throat", &e.throat_wall, e.h_throat * h_scale, e.t_aw_throat)] {
        let (xs, t) = engine::wall_transient(wall, h, taw, 293.0, burn_max);
        let (char_d, al) = engine::wall_result(wall, &xs, &t);
        r.margin(C::WallTemperature, rules::AL6061_MAX_CASE_K - al, "K", Source::Physics, format!("{label}: Al case peaks {:.0} C (limit {:.0} C), h {:.0} W/m2K, Taw {:.0} K", al - 273.15, rules::AL6061_MAX_CASE_K - 273.15, h, taw));
        let liner = wall[0].thickness;
        r.margin(C::AblativeChar, (liner - char_d) * 1e3, "mm", Source::Physics, format!("{label}: char {:.1} mm of {:.1} mm liner", char_d * 1e3, liner * 1e3));
    }

    // Injector. Regulated (LOX pintle): at end of burn, the bottle-fed
    // regulator holds p_op, drilled holes. Self-pressurised (N2O doublet):
    // every sample of every flown blowdown, Dyer flow through drilled holes.
    match e.triplet() {
        None => {
            match engine::operating_point(e, v.ox_tank.p_op, v.fuel_tank.p_op, d.params.feed_loss) {
                Ok((pc, so, sf)) => {
                    r.margin(C::InjectorStiffness, so.min(sf) - rules::INJ_DP_MIN, "", Source::Physics, format!("Pc {:.1} bar with drilled orifices; dP/Pc ox {:.0} %, fuel {:.0} %", pc / 1e5, so * 100.0, sf * 100.0));
                }
                Err(err) => r.missing(C::InjectorStiffness, Source::Physics, format!("{err}")),
            }
            match e.pintle() {
                Some(pin) => {
                    let (lo, hi) = rules::PINTLE_TMR_RANGE;
                    r.margin(C::InjectorMomentum, (pin.tmr - lo).min(hi - pin.tmr), "", Source::Physics, format!("pintle TMR {:.2} (band {lo}-{hi})", pin.tmr));
                }
                None => r.missing(C::InjectorMomentum, Source::Physics, "engine has no injector"),
            }
        }
        Some(tri) => match reactant(&d.params.engine.fuel) {
            Ok(fuel) => {
                let loss = d.params.feed_loss;
                let p_f = v.fuel_tank.p_op;
                for (label, p) in &profiles {
                    // Worst stiffness and worst resultant angle over the burn.
                    let mut worst = (f64::MAX, 0.0, 0.0, 0.0, 0.0);
                    let mut beta_worst: (f64, f64, f64) = (0.0, 0.0, 0.0);
                    for i in 0..p.pc.len() {
                        let pc = p.pc[i];
                        let so = (p.p_tank[i] * (1.0 - loss) - pc) / pc;
                        let sf = (p_f * (1.0 - loss) - pc) / pc;
                        if so.min(sf) < worst.0 {
                            worst = (so.min(sf), i as f64 * p.dt, pc, so, sf);
                        }
                        let rho_o = crate::nitrous::sat(p.t_tank[i]).map(|s| s.rho_l).unwrap_or(f64::NAN);
                        let b = tri.beta_deg_tolerance(p.mdot_ox[i], p.mdot_f[i], rho_o, fuel.density_kg_m3, rules::ORIFICE_TOL_M);
                        if b.is_nan() || b.abs() > beta_worst.0.abs() {
                            beta_worst = (b, i as f64 * p.dt, p.mdot_ox[i] / p.mdot_f[i]);
                        }
                    }
                    r.margin(C::InjectorStiffness, worst.0 - rules::INJ_DP_MIN, "", Source::Physics, format!("{label}: worst at t {:.1} s, Pc {:.1} bar, dP/Pc N2O {:.0} %, fuel {:.0} % (Dyer two-phase flow, drilled holes)", worst.1, worst.2 / 1e5, worst.3 * 100.0, worst.4 * 100.0));
                    r.margin(C::InjectorMomentum, rules::INJ_BETA_MAX_DEG - beta_worst.0.abs(), "deg", Source::Physics, format!("{label}: O-F-O triplet resultant angle {:+.2} deg with the N2O holes +/-{:.2} mm off size, at t {:.1} s (O/F {:.2}); limit {:.0} deg (Huzel & Huang eq 4-41)", beta_worst.0, rules::ORIFICE_TOL_M * 1e3, beta_worst.1, beta_worst.2, rules::INJ_BETA_MAX_DEG));
                }
            }
            Err(err) => {
                r.missing(C::InjectorStiffness, Source::Physics, format!("{err}"));
                r.missing(C::InjectorMomentum, Source::Physics, format!("{err}"));
            }
        },
    }

    // Start sequence.
    let (a_ox, a_f) = d.start.arrivals(&v);
    let last = a_ox.max(a_f);
    r.margin(C::StartSequence, d.start.igniter_burn_s - (last + 0.5), "s", Source::Physics, format!("LOX at {:.2} s, fuel at {:.2} s, igniter out at {:.2} s", a_ox, a_f, d.start.igniter_burn_s));

    // Nozzle separation at the site.
    // pe scales with Pc through the fixed nozzle: the lowest flown Pc.
    let pa = crate::atmosphere::at(lim.elevation_m).p;
    let pe = e.ideal.exit.p_pa * pc_min / e.spec.pc;
    r.margin(C::NozzleSeparation, pe / pa - 0.4, "", Source::Physics, format!("pe {:.2} bar at Pc {:.1} bar vs pa {:.2} bar", pe / 1e5, pc_min / 1e5, pa / 1e5));

    // Pressurant.
    let (_, p_end, _) = v.blowdown();
    let fed = if selfp { "fuel tank only; the N2O run tank self-pressurises" } else { "both tanks" };
    let _ = p_end;
    let (pm, req, usable, t_end) = vehicle::pressurant_requirement(&v);
    r.margin(C::PressurantMargin, pm, "", Source::Physics, format!("usable {} {:.2} kg (down to {:.0} bar, gas at {:.0} K) vs {:.2} kg needed ({} x {}; {fed})", if selfp { "compressed air" } else { "GN2" }, usable, 1.15 * v.regulated_p() / 1e5, t_end, req, d.params.bottles, d.params.bottle));

    // Ratings.
    match d.params.regulator.as_ref().and_then(|id| cat.get(id)) {
        Some(reg) => {
            let hi = reg.f("outlet_psi_max").unwrap() * 6894.757;
            match reg.f("outlet_psi_min") {
                Some(lo) => {
                    let lo = lo * 6894.757;
                    // The regulated tank: LOX (both tanks share the setpoint
                    // unless published pressures differ) or the fuel tank
                    // under a self-pressurised N2O feed.
                    let p = if selfp { v.fuel_tank.p_op } else { v.ox_tank.p_op };
                    r.margin(C::RegulatorRange, ((p - lo).min(hi - p)) / 1e5, "bar", Source::Rating, format!("tank {:.1} bar in {} outlet {:.1}-{:.1} bar", p / 1e5, reg.id(), lo / 1e5, hi / 1e5));
                }
                None => r.missing(C::RegulatorRange, Source::Rating, format!("{} lists no minimum outlet pressure", reg.id())),
            }
        }
        None => r.missing(C::RegulatorRange, Source::Rating, "no regulator chosen or not in catalog"),
    }
    // Every component that sees a tank's MEOP: main valve, relief valve
    // body and setting range, check valve, PTFE hose. (Tanks are proof
    // tested alone with ports plugged; components see MEOP only.)
    let relief_for = |meop: f64| if meop / 6894.757 <= 750.0 { "nuvair_relief_rv2mf_a" } else { "nuvair_relief_rv2mf_b" };
    for t in [&v.ox_tank, &v.fuel_tank] {
        let psi = t.meop / 6894.757;
        let mut parts: Vec<(&str, String)> = vec![("main valve", d.params.valve.clone()), ("relief valve", relief_for(t.meop).to_string()), ("check valve", "superlok_check_1_4mnpt_0p3psi".into())];
        // Room-temperature circuits get PTFE hose and the Superlok
        // fill/vent ball valve; the LOX circuit uses stainless tube and the
        // cryogenic main-valve model for fill/vent (the hose is rated to
        // -100 F, the Superlok valve to 50 F).
        if selfp || t.name == "fuel tank" {
            parts.push(("feed hose", "raceflux_ptfe_an4_per_ft".into()));
            parts.push(("fill/vent valve", "superlok_ball_1_4fnpt_3000".into()));
        }
        for (role, id) in parts {
            match cat.get(&id).and_then(|row| row.f("max_pressure_psi")) {
                Some(rating) => r.margin(C::ValveRating, (rating - psi) * 6894.757 / 1e5, "bar", Source::Rating, format!("{}: {role} {id} rated {rating:.0} psi vs MEOP {psi:.0} psi", t.name)),
                None => r.missing(C::ValveRating, Source::Rating, format!("{}: {role} {id} has no published pressure rating", t.name)),
            }
        }
        if let Some(row) = cat.get(relief_for(t.meop)) {
            match (row.f("set_psi_min"), row.f("set_psi_max")) {
                (Some(lo), Some(hi)) => r.margin(C::ValveRating, (psi - lo).min(hi - psi) * 6894.757 / 1e5, "bar", Source::Rating, format!("{}: relief set at MEOP {psi:.0} psi within {} range {lo:.0}-{hi:.0} psi", t.name, row.id())),
                _ => r.missing(C::ValveRating, Source::Rating, format!("{} lists no setting range", row.id())),
            }
        }
    }

    // Recovery.
    let m = recovery_mass(&v);
    match (cat.get(&d.params.drogue), cat.get(&d.params.main)) {
        (Some(dr), Some(mn)) => {
            let top = lim.elevation_m + nominal.apogee_agl.min(30_000.0 * FT);
            let low = lim.elevation_m + rules::MAIN_DEPLOY_AGL_M;
            let v_top = descent_rate(m, dr.f("diameter_in").unwrap(), dr.f("cd").unwrap(), crate::atmosphere::at(top).rho);
            let v_low = descent_rate(m, dr.f("diameter_in").unwrap(), dr.f("cd").unwrap(), crate::atmosphere::at(low).rho);
            r.margin(C::DrogueDescent, (v_low - lim.drogue_mps.0).min(lim.drogue_mps.1 - v_top), "m/s", Source::Physics, format!("{:.1} kg: {:.1} m/s at apogee, {:.1} m/s at main deploy; band {:.1}-{:.1}", m, v_top, v_low, lim.drogue_mps.0, lim.drogue_mps.1));
            let v_g = descent_rate(m, mn.f("diameter_in").unwrap(), mn.f("cd").unwrap(), crate::atmosphere::at(lim.elevation_m).rho);
            r.margin(C::MainDescent, lim.main_max_mps - v_g, "m/s", Source::Physics, format!("{:.1} m/s ({:.1} ft/s) at the ground; limit {:.1} ft/s", v_g, v_g / FT, lim.main_max_mps / FT));
        }
        _ => {
            r.missing(C::DrogueDescent, Source::Physics, "drogue not in catalog");
            r.missing(C::MainDescent, Source::Physics, "main not in catalog");
        }
    }

    // The design's recorded launch inputs must be this spec's (objective
    // re-flies with them). A copy-consistency check of its own (review 5
    // F1): a requirement attack makes it unpassable by construction, so it
    // is listed in CONSISTENCY_CHECKS, and apogee_accuracy stays physics.
    if (d.contract_m - spec.contract_m()).abs() > 1e-6 || d.competition != spec.competition || d.rail_length_m != spec.rail_length_m {
        r.missing(C::SpecInputs, Source::Requirement, format!("design records contract {:.0} ft, {:?}, rail {:?}; the spec says {:.0} ft, {:?}, rail {:?}", d.contract_m / FT, d.competition, d.rail_length_m, spec.contract_apogee_ft, spec.competition, spec.rail_length_m));
    }
    else {
        r.margin(C::SpecInputs, 0.0, "", Source::Requirement, "the design records the spec's contract, competition and rail");
    }
    // Apogee accuracy.
    let err = (nominal.apogee_agl / spec.contract_m() - 1.0).abs();
    r.margin(C::ApogeeAccuracy, 0.05 - err, "", Source::Requirement, format!("nominal {:.0} m ({:.0} ft) vs contract {:.0} ft", nominal.apogee_agl, nominal.apogee_agl / FT, spec.contract_apogee_ft));

    // Stock fit: the engine must be machinable from the stock bought.
    let st = engine_stock(cat, e);
    match (st.rod, st.bar) {
        (Some(rod), Some(bar)) => {
            let (rd, bd) = (rod.f("diameter_mm").unwrap_or(f64::NAN), bar.f("diameter_mm").unwrap_or(f64::NAN));
            r.margin(C::PartsFit, (rd - st.liner_od_mm).min(bd - st.case_od_mm), "mm", Source::Geometry, format!("liner/nozzle OD {:.1} mm from {:.1} mm phenolic rod ({}); case OD {:.1} mm from {:.1} mm 6061 bar ({})", st.liner_od_mm, rd, rod.id(), st.case_od_mm, bd, bar.id()));
        }
        _ => r.missing(C::PartsFit, Source::Geometry, "no priced phenolic rod or 6061 round with a published diameter in the catalog"),
    }

    // Geometry: all pairs.
    match crate::geometry::fit_report(&v, cat) {
        Ok(list) => {
            if list.is_empty() {
                r.margin(C::PartsFit, 0.0, "mm^3", Source::Geometry, "no overlapping pairs");
            }
            for (a, b, vol) in list {
                r.margin(C::PartsFit, -vol, "mm^3", Source::Geometry, format!("{a} overlaps {b}"));
            }
        }
        Err(err) => r.missing(C::PartsFit, Source::Geometry, format!("{err}")),
    }

    // Delivered performance cannot exceed the ideal: a c* efficiency above 1
    // (accepted from a spec before 2026-10-24) made Isp free.
    let eta = e.eta_cstar;
    if !(eta > 0.0 && eta <= 1.0) {
        r.missing(C::IspBound, Source::Physics, format!("c* efficiency {eta} is outside (0, 1]"));
    } else {
        let isp_vac = total_impulse(&v, 0.0) / v.propellant_burned();
        let ideal = e.ideal.isp_vac_m_s;
        r.margin(C::IspBound, 1.0 - isp_vac / ideal, "", Source::Physics, format!("delivered vacuum Isp {:.1} s vs ideal {:.1} s at the design O/F (eta c* {:.2})", isp_vac / G0, ideal / G0, eta));
    }

    // Budget from exactly the BOM rows.
    // Stock (review 5 F4): every catalog line orderable today.
    {
        let bom = Rocket::bom(d);
        let refused: Vec<String> = bom
            .lines
            .iter()
            .filter_map(|l| cat.get(&l.id).filter(|row| !row.in_stock()).map(|row| format!("{} (out of stock at {} on {})", l.id, row.url(), row.s("retrieved").unwrap_or("?"))))
            .collect();
        let text = if refused.is_empty() { format!("{} BOM lines, none marked out of stock", bom.lines.len()) } else { format!("out of stock: {}", refused.join(", ")) };
        r.margin(C::PartsOrderable, -(refused.len() as f64), "parts", Source::Rating, text);
    }
    // With no budget stated the check measures nothing against one (margin
    // 0, cost and every unpriced line with its reason in the detail), as
    // wright-air's Budget does with `budget_usd: None`.
    let bom = Rocket::bom(d);
    let unpriced = bom.unpriced();
    let priced: f64 = bom.lines.iter().filter_map(|l| l.unit_price.map(|u| u.as_usd() * l.qty)).sum();
    let unpriced_list = unpriced.iter().map(|l| format!("{} ({})", l.id, l.note)).collect::<Vec<_>>().join(", ");
    match spec.budget_usd {
        None => {
            let extra = if unpriced.is_empty() { String::new() } else { format!("; unpriced: {unpriced_list}") };
            r.margin(C::Budget, 0.0, "USD", Source::Requirement, format!("no budget stated: cost is information only, priced lines ${priced:.0}{extra}"));
        }
        Some(b) if !unpriced.is_empty() => {
            r.missing(C::Budget, Source::Requirement, format!("priced lines ${priced:.0} of budget ${b:.0}; unpriced: {unpriced_list}"));
        }
        Some(b) => {
            let total = bom.total().as_usd();
            r.margin(C::Budget, b - total, "USD", Source::Requirement, format!("BOM ${total:.0} vs budget ${b:.0}"));
        }
    }
}

fn line(cat: &Catalog, category: &str, id: &str, qty: f64, note: &str) -> BomLine {
    match cat.get(id) {
        Some(r) => BomLine {
            category: category.into(),
            id: id.into(),
            name: r.name().into(),
            qty,
            unit: r.unit().into(),
            unit_price: r.price().map(Money::usd),
            url: r.url().into(),
            note: note.into(),
        },
        None => BomLine { category: category.into(), id: id.into(), name: id.into(), qty, unit: "each".into(), unit_price: None, url: String::new(), note: format!("not in catalog; {note}") },
    }
}

pub fn bom_for(d: &RocketDesign, cat: &Catalog) -> Bom {
    // Quantities come from the vehicle rebuilt from the parameters, never
    // from the stored (editable) one.
    let rebuilt = match vehicle::build(&d.params, cat) {
        Ok(v) => v,
        Err(e) => return Bom { lines: vec![BomLine { category: "error".into(), id: "vehicle".into(), name: "vehicle does not build".into(), qty: 1.0, unit: "each".into(), unit_price: None, url: String::new(), note: format!("{e:#}") }] },
    };
    let v = &rebuilt;
    let p = &d.params;
    let mut lines = Vec::new();
    let sec = |n: &str| v.sections.iter().filter(|s| s.0 == n).map(|s| s.2 - s.1).sum::<f64>();
    let al_len = sec("lox_tank") + sec("intertank") + sec("fuel_tank");
    let tube_len = cat.get(&p.tank_tube).and_then(|r| r.f("length_m")).unwrap_or(1.8288);
    lines.push(line(cat, "structure", &p.tank_tube, (al_len / tube_len).ceil(), &format!("{:.2} m of tank/intertank tube", al_len)));
    let g12 = sec("recovery") + sec("pressurant") + sec("engine_bay");
    let g12_len = cat.get(&p.airframe_tube).and_then(|r| r.f("length_m")).unwrap_or(1.524);
    lines.push(line(cat, "structure", &p.airframe_tube, (g12 / g12_len).ceil(), &format!("{:.2} m of G12 bays", g12)));
    lines.push(line(cat, "structure", "wildman_g12ct_6in_coupler_12in", 3.0, "nose/recovery, recovery/pressurant, fuel tank/engine bay joints"));
    lines.push(line(cat, "structure", &p.nose, 1.0, ""));
    let fin_area = p.fin_count as f64 * 0.5 * (p.fin_root + p.fin_tip) * p.fin_span;
    let sheet = cat.get(&p.fin_material).and_then(|r| r.f("area_m2")).unwrap_or(0.0929);
    lines.push(line(cat, "structure", &p.fin_material, (fin_area * 1.5 / sheet).ceil(), &format!("{:.3} m^2 of fins + 50 % nesting waste", fin_area)));
    lines.push(line(cat, "structure", "wildman_bp_6_0_bulkplate", 3.0, "recovery bulkheads (plywood)"));
    lines.push(line(cat, "structure", "wildman_cr_6_0_3_0_centering_ring", 2.0, "engine bay centering rings"));
    lines.push(line(cat, "launch", "madcow_rg15b_1515_rail_guides", 1.0, "1515 rail guides (pair)"));
    lines.push(line(cat, "recovery", &p.drogue, 1.0, "drogue"));
    lines.push(line(cat, "recovery", &p.main, 1.0, "main"));
    lines.push(line(cat, "recovery", "wildman_kevlar_1_2_30ft", 2.0, "shock cords"));
    lines.push(line(cat, "recovery", "wildman_eyebolt_3_8_male", 4.0, ""));
    lines.push(line(cat, "recovery", "wildman_shear_pins_2_56_10pk", 1.0, ""));
    for a in &p.altimeters {
        lines.push(line(cat, "avionics", a, 1.0, "redundant dual-deploy altimeters"));
    }
    lines.push(line(cat, "avionics", &p.tracker, 1.0, "GPS tracker"));
    let selfp = v.profile.is_some();
    lines.push(line(cat, "feed", &p.bottle, p.bottles as f64, if selfp { "GN2 pressurant (fuel tank)" } else { "GN2 pressurant" }));
    if let Some(reg) = &p.regulator {
        let (n, note) = if selfp { (1.0, "fuel tank regulator (N2O self-pressurises)") } else { (2.0, "one regulator per tank") };
        lines.push(line(cat, "feed", reg, n, note));
    }
    lines.push(line(cat, "feed", &p.valve, 2.0, &format!("{} and fuel main valves", if selfp { "N2O" } else { "LOX" })));
    lines.push(line(cat, "feed", "superlok_check_1_4mnpt_0p3psi", 2.0, if selfp { "fuel pressurant and N2O fill-line check valves" } else { "pressurant check valves (no propellant backflow)" }));
    // One relief per tank, each in the range that covers its own MEOP.
    for t in [&v.ox_tank, &v.fuel_tank] {
        let meop_psi = t.meop / 6894.757;
        let relief = if meop_psi <= 750.0 { "nuvair_relief_rv2mf_a" } else { "nuvair_relief_rv2mf_b" };
        lines.push(line(cat, "feed", relief, 1.0, &format!("{} relief set at MEOP {:.0} psi", t.name, meop_psi)));
    }
    // Start, actuation and ground interfaces (hostile review 2026-10-24:
    // the BOM had no igniter, no way to open the valves, no fill/vent).
    lines.push(line(cat, "engine", "estes_e12_0", 1.0, "igniter: E12-0 motor (2.6 s burn) with its starter; pack of 2 (one spare)"));
    lines.push(line(cat, "feed", "aignep_5_2_double_solenoid_12vdc", 2.0, "pilot solenoids for the two main valve actuators"));
    lines.push(line(cat, "feed", "hk_army_hstl_68_4500", 1.0, "actuation air bottle, 800 psi output (compressed air, valve actuators only)"));
    let lp = cat.role("regulator_lp").find(|r| r.orderable() && r.f("inlet_psi_max").unwrap_or(0.0) >= 800.0).map(|r| r.id().to_string()).unwrap_or_else(|| "balystik_hpr800c_v3".into());
    lines.push(line(cat, "feed", &lp, 1.0, "actuation second stage 800 -> 100 psi (solenoids rated 145 psi)"));
    lines.push(line(cat, "avionics", "arduino_nano_a000005", 1.0, "valve sequencer"));
    lines.push(line(cat, "avionics", "addicore_relay_4ch", 1.0, "solenoid drivers"));
    if selfp {
        lines.push(line(cat, "feed", "superlok_ball_1_4fnpt_3000", 4.0, "fill and vent valves, N2O and fuel tanks"));
    } else {
        lines.push(line(cat, "feed", &p.valve, 2.0, "LOX fill and vent valves (cryogenic)"));
        lines.push(line(cat, "feed", "superlok_ball_1_4fnpt_3000", 2.0, "fuel fill and vent valves"));
    }
    lines.push(line(cat, "feed", "tube_316_1_4x035_20ft", 1.0, "pressurant and fill/vent lines"));
    lines.push(line(cat, "feed", "superlok_smc_4_4n", 12.0, "tube-to-NPT connectors (allowance)"));
    lines.push(line(cat, "feed", "superlok_sut_4", 4.0, "tees (allowance)"));
    lines.push(line(cat, "feed", "raceflux_ptfe_an4_per_ft", 6.0, if selfp { "N2O and fuel feed hose, ft" } else { "fuel feed hose, ft (LOX side: stainless tube)" }));
    let screws: usize = [&v.ox_tank, &v.fuel_tank].iter().map(|t| 2 * t.closure_screws).sum();
    lines.push(line(cat, "structure", "shcs_1_4_20x1_2_100pk", (screws as f64 / 100.0).ceil(), &format!("{screws} radial bulkhead closure screws")));
    let st = engine_stock(cat, &v.engine);
    let bar_id = st.bar.map(|r| r.id().to_string()).unwrap_or_else(|| "engine_case_bar_unavailable".into());
    let rod_id = st.rod.map(|r| r.id().to_string()).unwrap_or_else(|| "engine_liner_rod_unavailable".into());
    lines.push(line(cat, "engine", &bar_id, 2.0, "chamber case and injector stock"));
    let rod_ft = ((v.engine.length() + 0.05) / 0.3048).ceil();
    lines.push(line(cat, "engine", &rod_id, rod_ft, "liner, converging section and nozzle machined from solid rod (ft)"));
    // Propellants: LOX bought at 2x load (chill-down and boil-off allowance,
    // unsourced); nitrous at 1.5x (fill-line and vent losses, unsourced).
    match p.engine.oxidizer.as_str() {
        "lox" => {
            let lox_l = v.ox_tank.propellant_kg / v.ox_tank.density * 1e3 * 2.0;
            lines.push(line(cat, "propellant", "lox_weldstar_refill_per_liter", lox_l.ceil(), "2x load for chill-down/boil-off"));
        }
        "n2o" => {
            let lb = v.ox_tank.propellant_kg / 0.45359237 * 1.5;
            // SK Speed's rate drops at 9 lb and up.
            let id = if lb.ceil() >= 9.0 { "n2o_fill_skspeed_per_lb" } else { "n2o_fill_skspeed_per_lb_small" };
            lines.push(line(cat, "propellant", id, lb.ceil(), "1.5x load for fill and top-off vent losses (unsourced allowance)"));
            lines.push(line(cat, "propellant", "n2o_fill_skspeed_surcharge", 1.0, "per-fill surcharge"));
        }
        other => lines.push(line(cat, "propellant", &format!("{other}_oxidizer_unpriced"), 1.0, "no priced source for this oxidizer")),
    }
    let fuel_gal = v.fuel_tank.propellant_kg / v.fuel_tank.density * 264.172 * 1.2;
    // (row, gallons per unit): kerosene comes in 5 gal cans.
    let (fuel_id, per_unit, note) = match p.engine.fuel.as_str() {
        "propane" => ("propane_us_residential_avg".to_string(), 1.0, "load + 20 %"),
        "ethanol" => ("ethanol_190_1gal".to_string(), 1.0, "load + 20 %"),
        "ipa" => ("ipa_998_1gal_laballey".to_string(), 1.0, "load + 20 %"),
        "rp1" => ("kerosene_k1_5gal".to_string(), 5.0, "load + 20 %; RP-1 has no retail source, K-1 kerosene is the substitute student teams buy (Isp modelled with the RP-1 record)"),
        // Liquid methane is sold by bulk contract or quote only (no retail
        // page found 2026-10-24), so the line stays unpriced and the budget
        // check says so instead of reading it as free.
        "methane" => ("liquid_methane_unpriced".to_string(), 1.0, "no retail price: LNG/liquid methane is sold by bulk contract or quote only (searched 2026-10-24); get a quote"),
        other => (format!("{other}_fuel_unpriced"), 1.0, "no priced source for this fuel"),
    };
    lines.push(line(cat, "propellant", &fuel_id, (fuel_gal / per_unit).ceil(), note));
    let gn2_ft3 = v.gn2_kg / 1.165 * 35.3147 * 2.0;
    let _ = gn2_ft3;
    if selfp {
        // Fuel-side pressurant and actuation: compressed air (no LOX aboard).
        lines.push(line(cat, "propellant", "hpa_fill_pbsports", (2 * p.bottles + 2) as f64, "compressed air (HPA) to 4500 psi: pressurant bottles twice (pad + flight), actuation bottle twice"));
    } else {
        // LOX: nitrogen only, to 4500 psi: no public price (rules::GN2_FILL_PSI).
        let fill_ft3 = v.bottle_volume * 35.3147 * (v.bottle_p0 / 101_325.0);
        lines.push(line(cat, "propellant", "gn2_fill_4500psi_quote", 2.0, &format!("nitrogen to 4500 psi for the pressurant bottles, two fills of {fill_ft3:.0} cu ft: no public price (6K cylinders and boosters are quote-only; paintball 'nitrogen' is air; the priced 2000 psi cylinder cannot reach the fill) -- get a gas-supplier quote")));
        lines.push(line(cat, "propellant", "hpa_fill_pbsports", 2.0, "compressed air (HPA) for the actuation bottle, twice"));
    }
    Bom { lines }
}

impl Domain for Rocket {
    const NAME: &'static str = "rocket";
    type Spec = RocketSpec;
    type Design = RocketDesign;
    type Check = RocketCheck;

    fn spec_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["name", "contract_apogee_ft", "oxidizer", "fuel"],
            "properties": {
                "name": {"type": "string"},
                "competition": {"enum": ["FarOut2027", "Irec2025"], "default": "FarOut2027"},
                "contract_apogee_ft": {"type": "number", "minimum": 3000, "maximum": 50000, "description": "target apogee above ground level, ft. FarOut2027: Group A 3,000-15,000 or Group B 20,000-50,000 (FAR-OUT rules 7.3 p.75; Group C refused). Irec2025: exactly 10,000 or 30,000 (IREC Rules v1.6 sec. 2.0, https://www.soundingrocket.org/uploads/9/0/6/4/9064598/irec_rules_and_requirements_document_v_1.6_.pdf)"},
                "oxidizer": {"enum": ["lox", "n2o"], "description": "lox: GN2-regulated tanks and a pintle injector. n2o: liquid nitrous in a self-pressurised run tank (vapour pressure 35-63 bar over the 5-30 C fill band, falling as it blows down), an O-F-O triplet injector and an air-pressurised regulated fuel tank; the design is checked at 5, 20 and 30 C. N2O is not offered with cryogenic fuels."},
                "fuel": {"enum": ["propane", "ethanol", "ipa", "rp1"], "description": "rp1 is priced as K-1 kerosene (no retail RP-1). Liquid methane is modelled but left out: no retail price exists (bulk LNG by quote), so its budget line would be unpriced."},
                "budget_usd": {"type": "number", "exclusiveMinimum": 0, "maximum": 100000, "description": "USD, optional: only when the person states one (otherwise cost is reported for information and the budget check has nothing to hold it to). Max $100k: above the largest published student liquid-program cost (SEB Eureka-1, $65-75k development, https://engineering.berkeley.edu/news/2023/05/a-eureka-1-moment/)"},
                "payload_kg": {"type": "number", "default": 0, "minimum": 0, "maximum": 20, "description": "kg. IREC's minimum payload is 2.0 kg (Rules v1.6 sec. 2.0); 20 kg is Wright's envelope (unsourced)"},
                "rail_length_m": {"type": "number", "minimum": 3.048, "maximum": 18.288, "description": "launch rail length, m. Leave unset at a competition: the rules fix it (IREC >= 5.2 m, DTEG 11.1.2, https://www.soundingrocket.org/uploads/9/0/6/4/9064598/2025-irec_dteg_v1.1.6_02-14-25.pdf; FAR 20 ft). FAR's pad has 10-60 ft rails (FAR-OUT 5.4.13 p.46): 3.05-18.3 m there, 5.2-18.3 m at IREC. Site elevation, wind and temperatures are fixed by the competition, not spec fields."},
                "measured_dry": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "[weighed dry mass kg (5-500), dry CG from nose tip m (0-20)] (Wright envelope)"},
                "eta_cstar": {"type": "number", "minimum": 0.5, "maximum": 1, "description": "c* efficiency measured on a test stand, 0.5-1 (above 1 is impossible; SEB's Eureka-2 implies 0.71; default 0.90 a priori)"},
                "nose_cd_table": {"type": "array", "minItems": 2, "items": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2}, "description": "[[mach, cd], ...] nose pressure drag on the body cross-section from CFD on this nose (the cfd stage's rocket_su2.json nose_points: mach, cd_pressure), rising Mach, cd 0-1.2. Replaces the fineness-scaled NASA TR-R-100 von Karman curve (OpenRocket's scaling) in the drag build-up"},
                "pinned": {"type": "object", "description": "a fully specified VehicleParams (skips the search)"},
                "dry_mass_factor": {"type": "number", "minimum": 0.5, "maximum": 3.0, "description": "multiply Wright's dry-mass build-up by this calibration from a flown vehicle. Default 1.86, the Eureka-2 calibration (evals/rocket/reference.md sec. 7); 1 = the uncalibrated build-up"}
            }
        })
    }

    /// The envelope (`RocketSpec::validate`) plus the pinned vehicle's c*
    /// efficiency and the propellant pair: everything a spec can get wrong
    /// before a design is attempted.
    /// Only `spec_inputs`: the design's recorded contract/competition/rail
    /// against the spec (review 5 F1). `apogee_accuracy` is physics.
    fn consistency_checks() -> &'static [&'static str] {
        CONSISTENCY_CHECKS
    }

    fn validate_spec(spec: &RocketSpec) -> anyhow::Result<()> {
        spec.validate()?;
        if let Some(eta) = spec.pinned.as_ref().and_then(|p| p.engine.eta_cstar) {
            anyhow::ensure!(eta > 0.0 && eta <= 1.0, "pinned eta_cstar {eta} is not a c* efficiency: it must be in (0, 1] (a measured value is delivered c* / ideal c*)");
        }
        let ox = reactant(&spec.oxidizer)?;
        let fuel = reactant(&spec.fuel)?;
        anyhow::ensure!(ox.role == "oxidizer" && fuel.role == "fuel", "'{}' is not an oxidizer or '{}' is not a fuel", spec.oxidizer, spec.fuel);
        anyhow::ensure!(!(ox.self_pressurizing && fuel.cryogenic), "{} with cryogenic {} is not modelled: a room-temperature N2O run tank beside a cryogenic fuel tank has no student precedent Wright could find", ox.id, fuel.id);
        Ok(())
    }

    fn design(spec: &RocketSpec) -> anyhow::Result<RocketDesign> {
        Self::validate_spec(spec)?;
        let cat = Catalog::load()?;
        match &spec.pinned {
            Some(p) => {
                let mut p = p.clone();
                if spec.measured_dry.is_some() {
                    p.measured_dry = spec.measured_dry;
                }
                if spec.nose_cd_table.is_some() {
                    p.nose_cd_table = spec.nose_cd_table.clone();
                }
                if p.dry_mass_factor.is_none() {
                    p.dry_mass_factor = Some(spec.dry_mass_factor());
                }
                finish(spec, &cat, p, StartSequence::default(), vec!["pinned vehicle".into()])
            }
            None => search(spec, &cat),
        }
    }

    /// Design so that the result also passes every confirmed attack
    /// (`conditions`: the spec with one reviewer patch applied, and the
    /// check it broke). Wright's search sizes every candidate to the spec it
    /// is given, so a candidate set made only for `spec` would never answer
    /// e.g. a shorter rail: the candidates are the searches for `spec` and
    /// for each condition's spec, each re-stamped to `spec`'s launch inputs
    /// and re-flown (`finish`). Each candidate is screened on the
    /// conditions' checks first (cheap: one check run per condition), then
    /// must pass every check of `spec`; the best survivor by `ranks_before`
    /// (score, then cost) wins. If none
    /// survives, the error names the conditions and the closest candidate's
    /// failures. Same rule as wright-air's `search::design_under`.
    fn design_under(spec: &RocketSpec, conditions: &[wright_core::Condition<RocketSpec>]) -> anyhow::Result<RocketDesign> {
        if conditions.is_empty() {
            return Self::design(spec);
        }
        Self::validate_spec(spec)?;
        for c in conditions {
            Self::validate_spec(&c.spec).map_err(|e| anyhow::anyhow!("condition {} ({}): {e}", c.id, c.claim))?;
        }
        for c in conditions {
            anyhow::ensure!(RocketCheck::ALL.iter().any(|k| k.name() == c.check), "condition {} names unknown check '{}'", c.id, c.check);
        }
        let cat = Catalog::load()?;
        let mut sizing: Vec<(String, RocketSpec)> = vec![("the spec".into(), spec.clone())];
        for c in conditions {
            sizing.push((format!("condition {}", c.id), c.spec.clone()));
        }
        let mut survivors: Vec<(f64, f64, RocketDesign)> = Vec::new();
        let mut closest: Option<(usize, String)> = None;
        let mut tried: Vec<String> = Vec::new();
        let mut note = |n: usize, what: String| {
            tried.push(what.clone());
            if closest.as_ref().is_none_or(|(m, _)| n < *m) {
                closest = Some((n, what));
            }
        };
        for (label, s) in &sizing {
            let d0 = match Self::design(s) {
                Ok(d) => d,
                Err(e) => {
                    note(usize::MAX - 1, format!("sized for {label}: no design ({e:#})"));
                    continue;
                }
            };
            let d = match finish(spec, &cat, d0.params.clone(), d0.start.clone(), d0.search_log.clone()) {
                Ok(d) => d,
                Err(e) => {
                    note(usize::MAX - 1, format!("sized for {label}: does not rebuild ({e:#})"));
                    continue;
                }
            };
            // Screen: each condition's broken check, under its spec.
            let screen_fails: Vec<String> = conditions
                .iter()
                .filter_map(|c| {
                    let fin = Self::check(&c.spec, &d);
                    match fin.get(&c.check) {
                        Some(o) if o.passed() => None,
                        Some(o) => Some(format!("{} ({}): {o}", c.id, c.claim)),
                        None => Some(format!("{}: check {} not reported", c.id, c.check)),
                    }
                })
                .collect();
            if !screen_fails.is_empty() {
                note(conditions.len() + screen_fails.len(), format!("sized for {label}: {}", screen_fails.join("; ")));
                continue;
            }
            // Finalist: every check of the person's spec.
            let fin = Self::check(spec, &d);
            if !fin.passed() {
                let f: Vec<String> = fin.failures().map(|o| o.to_string()).collect();
                note(f.len(), format!("sized for {label}: passes the attacks but fails {}", f.join("; ")));
                continue;
            }
            survivors.push((mean_far_score(&d, spec), Self::bom(&d).total().as_usd(), d));
        }
        let pick = survivors.into_iter().reduce(|a, b| if ranks_before((0.0, b.0, b.1), (0.0, a.0, a.1)) { b } else { a });
        match pick {
            Some((_, _, mut d)) => {
                d.search_log.push(format!("design_under: passes the spec and {} confirmed attack(s): {}", conditions.len(), conditions.iter().map(|c| format!("{} ({})", c.id, c.check)).collect::<Vec<_>>().join(", ")));
                Ok(d)
            }
            None => {
                let closest = closest.map(|c| c.1).unwrap_or_else(|| "none built".into());
                anyhow::bail!(
                    "no design passes the spec and the confirmed attacks [{}]; closest: {}; all candidates: {}",
                    conditions.iter().map(|c| format!("{} on {}: {}", c.id, c.check, c.claim)).collect::<Vec<_>>().join("; "),
                    closest,
                    tried.join(" | ")
                )
            }
        }
    }

    fn check(spec: &RocketSpec, design: &RocketDesign) -> Finished {
        let mut r = Report::<RocketCheck>::new();
        match Catalog::load() {
            Ok(cat) => run_checks(spec, design, &cat, &mut r),
            Err(e) => {
                for &c in RocketCheck::ALL {
                    r.missing(c, Source::Rating, format!("catalog failed to load: {e}"));
                }
            }
        }
        r.finish()
    }

    fn bom(design: &RocketDesign) -> Bom {
        match Catalog::load() {
            Ok(cat) => bom_for(design, &cat),
            Err(e) => Bom { lines: vec![BomLine { category: "error".into(), id: "catalog".into(), name: format!("catalog failed: {e}"), qty: 1.0, unit: "each".into(), unit_price: None, url: String::new(), note: String::new() }] },
        }
    }

    fn build(design: &RocketDesign, out: &std::path::Path) -> anyhow::Result<wright_core::Artifacts> {
        crate::geometry::build(design, out)
    }

    fn catalog_paths() -> Vec<(&'static str, &'static str)> {
        crate::options::paths()
    }

    /// Catalog rows the field at `path` accepts (filtered on id or label,
    /// case-insensitive); the design does not narrow them.
    fn options(_design: &RocketDesign, path: &str, query: Option<&str>) -> Vec<wright_core::CatalogOption> {
        match Catalog::load() {
            Ok(cat) => crate::options::options(&cat, path, query),
            Err(_) => vec![],
        }
    }

    fn objective(design: &RocketDesign) -> (f64, &'static str) {
        let label = "FAR-OUT altitude score, mean over wind/thrust/drag dispersion (max 1000), re-flown from the design's parameters";
        match reflown(&design.launch_spec(), design) {
            Ok(r) => (far_score_of(&r.dispersion, design.contract_m), label),
            // A design that does not build scores nothing.
            Err(_) => (0.0, label),
        }
    }

    fn headlines(spec: &RocketSpec, d: &RocketDesign) -> Vec<wright_core::Headline> {
        use wright_core::{Bound, Headline};
        let lim = spec.competition.limits();
        let pa = crate::atmosphere::at(lim.elevation_m).p;
        let h = |label: &str, value: f64, unit: &str, limit: Option<f64>, bound: Bound, check: Option<&str>| Headline {
            label: label.into(), value, unit: unit.into(), limit, bound, check: check.map(String::from),
        };
        let r = match reflown(spec, d) {
            Ok(r) => r,
            // Nothing to show for a design that does not build (check says why).
            Err(_) => return vec![],
        };
        let (n, v) = (&r.nominal, &r.vehicle);
        let lo = r.dispersion.iter().map(|x| x.1).fold(f64::MAX, f64::min);
        let hi = r.dispersion.iter().map(|x| x.1).fold(f64::MIN, f64::max);
        vec![
            h("Apogee (nominal)", n.apogee_agl / FT, "ft", Some(spec.contract_apogee_ft), Bound::Info, Some("apogee_accuracy")),
            h("Apogee band low", lo / FT, "ft", None, Bound::Info, None),
            h("Apogee band high", hi / FT, "ft", None, Bound::Info, None),
            // The same vehicle with Wright's uncalibrated dry-mass build-up
            // (x1.0 instead of the Eureka-2 calibration): information only.
            h(&format!("Apogee, uncalibrated mass (design uses x{:.2})", d.params.dry_mass_factor.unwrap_or(1.0)), {
                let mut q = d.params.clone();
                q.dry_mass_factor = Some(1.0);
                Catalog::load().ok().and_then(|cat| vehicle::build(&q, &cat).ok()).map(|vu| fly(&vu, &spec.launch(wind_cases(spec)[1])).apogee_agl / FT).unwrap_or(f64::NAN)
            }, "ft", None, Bound::Info, None),
            h("Stability at rail exit", n.margin_at_rail_exit, "cal", Some(lim.stability_min_cal), Bound::AtLeast, Some("stability_min")),
            h("Rail exit speed", n.rail_exit_speed / FT, "ft/s", Some(lim.rail_exit_min_mps / FT), Bound::AtLeast, Some("rail_exit_speed")),
            h("Thrust/weight", n.liftoff_twr, "", Some(lim.twr_min), Bound::AtLeast, Some("thrust_to_weight")),
            h("Isp at site", total_impulse(v, pa) / v.propellant_burned() / G0, "s", None, Bound::Info, None),
            // FAR-OUT and IREC liquid categories set no impulse class limit.
            h("Total impulse", total_impulse(v, pa), "N s", None, Bound::Info, None),
            h("Cost", Rocket::bom(d).total().as_usd(), "USD", spec.budget_usd, if spec.budget_usd.is_some() { Bound::AtMost } else { Bound::Info }, Some("budget")),
        ]
    }
}

/// Total impulse (N s) at ambient `pa`: thrust integrated over the flown
/// burn (constant thrust x burn time for a regulated feed).
pub fn total_impulse(v: &Vehicle, pa: f64) -> f64 {
    match &v.profile {
        None => v.engine.thrust(pa) * v.burn_time,
        Some(_) => {
            let n = 400;
            let dt = v.burn_time / n as f64;
            (0..n).map(|i| v.thrust_at((i as f64 + 0.5) * dt, pa) * dt).sum()
        }
    }
}

/// FAR-OUT altitude score averaged over a dispersion set.
pub fn far_score_of(dispersion: &[(String, f64)], contract_m: f64) -> f64 {
    dispersion.iter().map(|(_, a)| rules::far_altitude_score(*a, contract_m)).sum::<f64>() / dispersion.len() as f64
}

/// FAR-OUT altitude score of a design, re-flown from its parameters at the
/// spec's launch conditions (0 if it does not build).
pub fn mean_far_score(d: &RocketDesign, spec: &RocketSpec) -> f64 {
    reflown(spec, d).map(|r| far_score_of(&r.dispersion, spec.contract_m())).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{reference_spec, requirements_spec};

    #[test]
    fn requirements_only_spec_designs_a_passing_rocket() {
        let spec = requirements_spec();
        let d = Rocket::design(&spec).unwrap();
        let fin = Rocket::check(&spec, &d);
        // No budget stated: every check passes under the Eureka-2 mass
        // calibration, and the budget check carries the cost and the
        // unpriced GN2 line with its reason as information.
        assert!(fin.passed(), "{fin}");
        let b = fin.get("budget").unwrap().to_string();
        assert!(b.contains("no budget stated") && b.contains("gn2_fill_4500psi_quote"), "{b}");
        // A stated budget is held: nitrogen at 4500 psi has no public
        // price, so the line is unpriced with its reason and the check fails.
        let stated = RocketSpec { budget_usd: Some(10_000.0), ..spec.clone() };
        let o = Rocket::check(&stated, &d);
        let s = o.get("budget").unwrap();
        assert!(!s.passed() && s.to_string().contains("gn2_fill_4500psi_quote") && s.to_string().contains("priced lines $"), "{s}");
        assert_eq!(d.params.dry_mass_factor, Some(rules::EUREKA2_DRY_MASS_FACTOR));
        // The objective is re-flown from the parameters, not read from storage.
        let mut edited = d.clone();
        edited.dispersion = vec![("edited".into(), d.contract_m)];
        assert_eq!(Rocket::objective(&edited).0, Rocket::objective(&d).0);
        assert!(!Rocket::headlines(&spec, &d).is_empty());
    }

    #[test]
    fn design_is_deterministic() {
        let spec = requirements_spec();
        let a = serde_json::to_string(&Rocket::design(&spec).unwrap().params).unwrap();
        let b = serde_json::to_string(&Rocket::design(&spec).unwrap().params).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn eureka2_reconstruction_fails_far_out_2027_thrust_to_weight() {
        let spec = RocketSpec { pinned: Some(crate::reference::eureka2()), ..reference_spec(50.0 * FT) };
        let d = Rocket::design(&spec).unwrap();
        let fin = Rocket::check(&spec, &d);
        assert!(!fin.get("thrust_to_weight").unwrap().passed());
    }

    /// Review 5 F4: the search buys only orderable parts, and a design
    /// carrying an out-of-stock part fails parts_orderable naming it.
    #[test]
    fn out_of_stock_part_fails_parts_orderable() {
        let spec = requirements_spec();
        let d = Rocket::design(&spec).unwrap();
        let fin = Rocket::check(&spec, &d);
        assert!(fin.get("parts_orderable").unwrap().passed(), "{fin}");
        let mut e = d.clone();
        e.params.valve = "valveman_cryo_ball_1_2_double_acting".into();
        let o = Rocket::check(&spec, &e);
        let po = o.get("parts_orderable").unwrap();
        assert!(!po.passed() && po.to_string().contains("valveman_cryo_ball_1_2_double_acting"), "{po}");
        // Review 5 F1: a moved contract fails the copy check (listed as
        // consistency) and apogee_accuracy is judged on the spec's contract.
        let moved = RocketSpec { contract_apogee_ft: 25_000.0, ..spec.clone() };
        let m = Rocket::check(&moved, &d);
        assert!(!m.get("spec_inputs").unwrap().passed());
        assert!(m.get("apogee_accuracy").unwrap().margin().is_some(), "apogee_accuracy is measured, not missing");
        assert_eq!(<Rocket as Domain>::consistency_checks(), &["spec_inputs"]);
        assert!(RocketCheck::ALL.iter().any(|c| c.name() == "spec_inputs"));
    }

    /// Lame von Mises at the bore: below hoop for a thin closed-end tube,
    /// and equal to the textbook closed form sqrt(3) p ro^2 / (ro^2 - ri^2).
    #[test]
    fn tank_wall_von_mises_from_lame() {
        let (p, ri, ro) = (100e5, 0.0730, 0.0762);
        let (vm, hoop, ax, rad) = lame_von_mises(p, ri, ro);
        assert!((hoop - lame_hoop(p, ri, ro)).abs() < 1e-6 && rad == -p && ax > 0.0);
        let closed = 3f64.sqrt() * p * ro * ro / (ro * ro - ri * ri);
        assert!((vm / closed - 1.0).abs() < 1e-12, "{vm} vs {closed}");
        let ratio = hoop / vm;
        assert!(ratio > 1.05 && ratio < 1.15, "hoop overstates by {ratio}");
    }

    #[test]
    fn unpriced_line_fails_budget() {
        let spec = RocketSpec { budget_usd: Some(50_000.0), ..requirements_spec() };
        let mut d = Rocket::design(&spec).unwrap();
        d.params.valve = "no_such_valve".into();
        let fin = Rocket::check(&spec, &d);
        assert!(!fin.get("budget").unwrap().passed());
        assert!(!fin.get("valve_rating").unwrap().passed());
    }

    /// The envelope refuses implausible specs with the rule, and the stored
    /// rocket cases agree with it: every accepted regression patch gives a
    /// valid spec at some competition; the rejected IREC 3 m rail is outside.
    #[test]
    fn envelope_refuses_implausible_specs_and_matches_stored_cases() {
        let ok = requirements_spec();
        assert!(Rocket::validate_spec(&ok).is_ok());
        for (patch, word) in [
            (serde_json::json!({"contract_apogee_ft": 17000.0}), "FAR-OUT group"),
            (serde_json::json!({"contract_apogee_ft": 120000.0}), "FAR-OUT group"),
            (serde_json::json!({"competition": "Irec2025", "contract_apogee_ft": 20000.0}), "IREC liquid categories"),
            (serde_json::json!({"competition": "Irec2025", "contract_apogee_ft": 10000.0, "rail_length_m": 3.0}), "DTEG 11.1.2"),
            (serde_json::json!({"rail_length_m": 1.0}), "FAR-OUT 5.4.13"),
            (serde_json::json!({"budget_usd": -5.0}), "budget_usd"),
            (serde_json::json!({"eta_cstar": 1.3}), "eta_cstar"),
            (serde_json::json!({"payload_kg": 200.0}), "payload_kg"),
            (serde_json::json!({"oxidizer": "n2o", "fuel": "methane"}), "not modelled"),
        ] {
            let mut v = serde_json::to_value(&ok).unwrap();
            for (k, x) in patch.as_object().unwrap() {
                v[k] = x.clone();
            }
            let s: RocketSpec = serde_json::from_value(v).unwrap();
            let e = Rocket::validate_spec(&s).unwrap_err().to_string();
            assert!(e.contains(word), "{patch}: {e}");
        }
        // A 10 ft FAR rail is inside the envelope (the pad has one).
        let short = RocketSpec { rail_length_m: Some(10.0 * FT), ..requirements_spec() };
        assert!(Rocket::validate_spec(&short).is_ok());
        // Stored cases.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let bases = [requirements_spec(), RocketSpec { competition: Competition::Irec2025, contract_apogee_ft: 10_000.0, ..requirements_spec() }];
        let apply = |base: &RocketSpec, patch: &serde_json::Value| -> Option<RocketSpec> {
            let mut v = serde_json::to_value(base).ok()?;
            for (k, x) in patch.as_object()? {
                v[k] = x.clone();
            }
            serde_json::from_value(v).ok()
        };
        for (file, accepted) in [("regressions-rocket.jsonl", true), ("regressions-rejected.jsonl", false)] {
            let text = std::fs::read_to_string(root.join(file)).unwrap_or_default();
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let r: serde_json::Value = serde_json::from_str(line).unwrap();
                if r["domain"] != "rocket" {
                    continue;
                }
                let valid_somewhere = bases.iter().any(|b| apply(b, &r["spec_patch"]).is_some_and(|s| Rocket::validate_spec(&s).is_ok()));
                if accepted {
                    assert!(valid_somewhere, "stored regression {} is outside the envelope everywhere", r["id"]);
                }
            }
        }
        // The rejected IREC case: its 3 m rail is outside IREC's envelope.
        let irec3 = apply(&bases[1], &serde_json::json!({"rail_length_m": 3.0})).unwrap();
        assert!(Rocket::validate_spec(&irec3).is_err());
    }

    #[test]
    fn all_lists_every_check_once() {
        let mut v = RocketCheck::ALL.to_vec();
        v.dedup();
        assert_eq!(v.len(), 26);
        let names: std::collections::BTreeSet<_> = RocketCheck::ALL.iter().map(|c| c.name()).collect();
        assert_eq!(names.len(), RocketCheck::ALL.len());
    }
}
