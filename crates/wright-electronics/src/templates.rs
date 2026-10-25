//! Circuit families sized from requirements. The servo power board is
//! baseline `aircraft/electronics.py:257 servo_board_spec` (TI SLVS757 TPS5450
//! application circuit), the same topology as
//! `evals/electronics/fixtures/servo_power_6v5a.json` with three
//! differences, each a fix:
//!
//! * every part names its KiCad library footprint (the Python baseline left passives to its
//!   own generated `silkscreen:*` patterns), sized for the voltage and
//!   current: 1210 50 V input ceramics (>= 1.5 x the 4S maximum), an 8 x 10
//!   electrolytic at the output, a 12 x 12 shielded inductor, SMC for the
//!   5 A Schottky, the XT30 PCB plug;
//! * the catch diode's cathode is on SW. KiCad `Device:D` / `D_Schottky`
//!   number the cathode pin `1` and `Diode_SMD:D_SMC` pad 1 is the band;
//!   the Python baseline's fixture puts `D_CATCH.1` on GND, which on KiCad's footprint is
//!   a reversed catch diode (VIN shorted through the high-side switch);
//! * R2 is the nearest E96 value to 10 k * 1.221 / (Vout - 1.221), not a
//!   2-decimal float, and the setpoint check recomputes Vout from it.
//!
//! The LDO is baseline `scripts/design_quality.py`'s selftest circuit
//! (`fixtures/ldo.json`) with an input and an output header so the board
//! has something to plug into.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wright_eda::ir::{Device, Passive};
use wright_eda::{Circuit, Netlist};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Template {
    /// Buck servo rail from the flight pack.
    ServoPower { vin_min_v: f64, vin_max_v: f64, vout_v: f64, iout_a: f64, servos: u32 },
    /// Linear regulator rail.
    Ldo { vin_v: f64, vout_v: f64, iout_a: f64 },
}

/// TPS5450 internal reference, SLVS757 section 7.3 ("1.221 V").
pub const TPS5450_VREF: f64 = 1.221;
/// TPS5450 upper feedback resistor, SLVS757 application circuit.
pub const TPS5450_R1_OHM: f64 = 10_000.0;
/// TPS5450 output from its divider, SLVS757: Vout = 1.221 x (1 + R1/R2).
pub fn tps5450_vout(r1_ohm: f64, r2_ohm: f64) -> f64 {
    TPS5450_VREF * (1.0 + r1_ohm / r2_ohm)
}

/// Converter efficiency used to size the input current (ASSUMPTION: TI
/// SLVS757 efficiency curves sit near 90 % at 12 V in / 5 V 3 A; 0.85 is
/// the conservative figure, so the input current is over- not under-stated).
pub const BUCK_EFFICIENCY: f64 = 0.85;
/// Design current for a signal net, A (servo PWM, feedback, bootstrap).
pub const SIGNAL_A: f64 = 0.1;

/// baseline `aircraft/electronics.py` `SERVOS` order (the fixture's headers).
const SERVO_NAMES: [&str; 5] = ["aileron_l", "aileron_r", "elevator", "rudder_l", "rudder_r"];

pub const FP_HEADER: &str = "Connector_PinHeader_2.54mm:PinHeader_1x0{n}_P2.54mm_Vertical";
pub const FP_XT30: &str = "Connector_AMASS:AMASS_XT30PW-M_1x02_P2.50mm_Horizontal";
pub const FP_POWERPAD8: &str = "Package_SO:TI_SO-PowerPAD-8_ThermalVias";
pub const FP_SOT223: &str = "Package_TO_SOT_SMD:SOT-223-3_TabPin2";
pub const FP_R0603: &str = "Resistor_SMD:R_0603_1608Metric";
pub const FP_C0603: &str = "Capacitor_SMD:C_0603_1608Metric";
pub const FP_C1206: &str = "Capacitor_SMD:C_1206_3216Metric";
pub const FP_C1210: &str = "Capacitor_SMD:C_1210_3225Metric";
pub const FP_CP8X10: &str = "Capacitor_SMD:CP_Elec_8x10";
pub const FP_L12X12: &str = "Inductor_SMD:L_Bourns_SRP1245A";
pub const FP_SMC: &str = "Diode_SMD:D_SMC";

fn header(n: usize) -> String {
    FP_HEADER.replace("{n}", &n.to_string())
}

fn device(kind: &str, package: &str, pins: &[(&str, &str)], nc: &[&str], mpn: Option<&str>) -> Device {
    Device {
        kind: Some(kind.into()),
        package: Some(package.into()),
        pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        no_connect: nc.iter().map(|s| s.to_string()).collect(),
        mpn: mpn.map(str::to_string),
    }
}

fn passive(kind: &str, value: &str, package: &str) -> Passive {
    Passive { kind: kind.into(), value: value.into(), package: Some(package.into()) }
}

/// The E96 series (IEC 60063), one decade.
const E96: [f64; 96] = [
    1.00, 1.02, 1.05, 1.07, 1.10, 1.13, 1.15, 1.18, 1.21, 1.24, 1.27, 1.30, 1.33, 1.37, 1.40, 1.43, 1.47, 1.50, 1.54, 1.58, 1.62, 1.65, 1.69, 1.74, 1.78, 1.82, 1.87, 1.91, 1.96, 2.00, 2.05, 2.10, 2.15, 2.21, 2.26, 2.32, 2.37, 2.43, 2.49, 2.55, 2.61, 2.67, 2.74, 2.80, 2.87, 2.94, 3.01, 3.09, 3.16, 3.24, 3.32, 3.40, 3.48, 3.57, 3.65, 3.74, 3.83, 3.92, 4.02, 4.12, 4.22, 4.32, 4.42, 4.53, 4.64, 4.75, 4.87, 4.99, 5.11, 5.23, 5.36, 5.49, 5.62, 5.76, 5.90, 6.04, 6.19, 6.34, 6.49, 6.65, 6.81, 6.98, 7.15, 7.32, 7.50, 7.68, 7.87, 8.06, 8.25, 8.45, 8.66, 8.87, 9.09, 9.31, 9.53, 9.76,
];

/// Nearest E96 value in ohms.
pub fn e96(ohm: f64) -> f64 {
    let dec = 10f64.powf(ohm.log10().floor());
    let m = ohm / dec;
    let mut best = (f64::INFINITY, 1.0);
    for v in E96.iter().copied().chain([10.0]) {
        let d = (v / m).ln().abs();
        if d < best.0 {
            best = (d, v);
        }
    }
    best.1 * dec
}

/// Resistor value text (`2.55k`, `10k`, `470`).
pub fn ohm_text(ohm: f64) -> String {
    let (v, suffix) = if ohm >= 1e6 { (ohm / 1e6, "M") } else if ohm >= 1e3 { (ohm / 1e3, "k") } else { (ohm, "") };
    let mut s = format!("{v:.3}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    format!("{s}{suffix}")
}

/// Parse a resistor value (`2.55k`, `5k1`, `10k`, `470R`, `1M`) to ohms.
pub fn parse_ohm(text: &str) -> Option<f64> {
    let t = text.split_whitespace().next()?.trim_end_matches(['R', 'r', 'Ω']).trim_end_matches("ohm");
    let (mult, letter) = [('k', 1e3), ('K', 1e3), ('M', 1e6), ('m', 1e-3)]
        .iter()
        .find(|(c, _)| t.contains(*c))
        .map(|(c, m)| (*m, Some(*c)))
        .unwrap_or((1.0, None));
    let num = match letter {
        // `5k1` = 5.1k, `2.55k` = 2.55k
        Some(c) => {
            let (a, b) = t.split_once(c)?;
            if b.is_empty() {
                a.to_string()
            } else {
                format!("{a}.{b}")
            }
        }
        None => t.to_string(),
    };
    num.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0).map(|v| v * mult)
}

/// The circuit a template expands to.
pub fn expand(t: &Template) -> anyhow::Result<Circuit> {
    match *t {
        Template::ServoPower { vin_min_v, vin_max_v, vout_v, iout_a, servos } => servo_power(vin_min_v, vin_max_v, vout_v, iout_a, servos),
        Template::Ldo { vin_v, vout_v, iout_a } => ldo(vin_v, vout_v, iout_a),
    }
}

/// Requirements a template states, for the checks.
pub fn validate(t: &Template) -> anyhow::Result<()> {
    match *t {
        Template::ServoPower { vin_min_v, vin_max_v, vout_v, iout_a, servos } => {
            anyhow::ensure!(vin_min_v.is_finite() && vin_max_v.is_finite() && vout_v.is_finite() && iout_a.is_finite(), "servo_power: every requirement must be a finite number");
            anyhow::ensure!(vin_min_v > 0.0 && vin_max_v >= vin_min_v, "servo_power: need 0 < vin_min_v <= vin_max_v (got {vin_min_v}, {vin_max_v})");
            anyhow::ensure!(vout_v > TPS5450_VREF, "servo_power: vout_v {vout_v} V is below the TPS5450 reference {TPS5450_VREF} V");
            anyhow::ensure!(vout_v < vin_min_v, "servo_power: a buck cannot make {vout_v} V from {vin_min_v} V");
            anyhow::ensure!(iout_a > 0.0, "servo_power: iout_a must be > 0");
            anyhow::ensure!((1..=8).contains(&servos), "servo_power: 1..=8 servos (got {servos}); the signal header is 1x0(n+1) up to 1x09");
        }
        Template::Ldo { vin_v, vout_v, iout_a } => {
            anyhow::ensure!(vin_v.is_finite() && vout_v.is_finite() && iout_a.is_finite() && vin_v > vout_v && vout_v > 0.0 && iout_a > 0.0, "ldo: need vin_v > vout_v > 0 and iout_a > 0");
        }
    }
    Ok(())
}

fn servo_power(vin_min_v: f64, vin_max_v: f64, vout_v: f64, iout_a: f64, servos: u32) -> anyhow::Result<Circuit> {
    validate(&Template::ServoPower { vin_min_v, vin_max_v, vout_v, iout_a, servos })?;
    let r2 = e96(TPS5450_R1_OHM * TPS5450_VREF / (vout_v - TPS5450_VREF));
    let mut c = Circuit::default();
    let d = &mut c.devices;
    d.insert(
        "TPS5450DDA".into(),
        device("ic", FP_POWERPAD8, &[("BOOT", "1"), ("NC1", "2"), ("NC2", "3"), ("VSENSE", "4"), ("ENA", "5"), ("GND", "6"), ("VIN", "7"), ("PH", "8"), ("PAD", "9")], &["NC1", "NC2", "ENA"], Some("TPS5450DDAR")),
    );
    d.insert("J_BAT".into(), device("connector", FP_XT30, &[("1", "1"), ("2", "2")], &[], Some("XT30PW-M")));
    let sig_pins: Vec<(String, String)> = (1..=servos as usize + 1).map(|i| (i.to_string(), i.to_string())).collect();
    let sig_refs: Vec<(&str, &str)> = sig_pins.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    d.insert("J_SIG".into(), device("connector", &header(servos as usize + 1), &sig_refs, &[], None));
    let p = &mut c.passives;
    // TI SLVS757 Figure 10-1 values; ratings chosen for the 4S maximum.
    p.insert("C_IN1".into(), passive("capacitor", "10uF 50V X7R", FP_C1210));
    p.insert("C_IN2".into(), passive("capacitor", "10uF 50V X7R", FP_C1210));
    p.insert("C_BOOT".into(), passive("capacitor", "10nF 50V X7R", FP_C0603));
    p.insert("L_BUCK".into(), passive("inductor", "15uH", FP_L12X12));
    p.insert("D_CATCH".into(), passive("diode", "SS54 5A 40V", FP_SMC));
    p.insert("C_OUT1".into(), passive("capacitor", "220uF 16V", FP_CP8X10));
    p.insert("C_OUT2".into(), passive("capacitor", "10uF 16V X7R", FP_C1206));
    p.insert("R_FB1".into(), passive("resistor", &ohm_text(TPS5450_R1_OHM), FP_R0603));
    p.insert("R_FB2".into(), passive("resistor", &ohm_text(r2), FP_R0603));
    let out = rail_name(vout_v);
    let mut nets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut net = |n: &str, ts: &[&str]| nets.insert(n.to_string(), ts.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    net("VBAT", &["J_BAT.1", "TPS5450DDA.VIN", "C_IN1.1", "C_IN2.1"]);
    // D_CATCH.1 is the cathode (KiCad Device:D_Schottky pin 1 = K).
    net("GND", &["J_BAT.2", "TPS5450DDA.GND", "TPS5450DDA.PAD", "C_IN1.2", "C_IN2.2", "D_CATCH.2", "C_OUT1.2", "C_OUT2.2", "R_FB2.2", &format!("J_SIG.{}", servos + 1)]);
    net("SW", &["TPS5450DDA.PH", "C_BOOT.2", "L_BUCK.1", "D_CATCH.1"]);
    net("BOOT", &["TPS5450DDA.BOOT", "C_BOOT.1"]);
    net(&out, &["L_BUCK.2", "C_OUT1.1", "C_OUT2.1", "R_FB1.1"]);
    net("VSENSE", &["TPS5450DDA.VSENSE", "R_FB1.2", "R_FB2.1"]);
    for i in 0..servos as usize {
        let name = SERVO_NAMES.get(i).map(|s| s.to_string()).unwrap_or_else(|| format!("servo{}", i + 1));
        let h = format!("J_{}", name.to_ascii_uppercase());
        c.devices.insert(h.clone(), device("connector", &header(3), &[("1", "1"), ("2", "2"), ("3", "3")], &[], None));
        nets.insert(format!("SIG_{}", name.to_ascii_uppercase()), vec![format!("J_SIG.{}", i + 1), format!("{h}.1")]);
        nets.get_mut(&out).unwrap().push(format!("{h}.2"));
        nets.get_mut("GND").unwrap().push(format!("{h}.3"));
    }
    c.nets = nets;
    let _ = vin_max_v;
    Ok(c)
}

/// KiCad-style rail name: `+6V`, `+3V3`, `+6V5`.
pub fn rail_name(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s.split_once('.') {
        Some((a, b)) => format!("+{a}V{b}"),
        None => format!("+{s}V"),
    }
}

fn ldo(vin_v: f64, vout_v: f64, iout_a: f64) -> anyhow::Result<Circuit> {
    validate(&Template::Ldo { vin_v, vout_v, iout_a })?;
    // AMS1117 is fixed-output in 1.5/1.8/2.5/2.85/3.3/5.0 V (AMS1117 datasheet, "ordering information").
    let fixed = [1.5, 1.8, 2.5, 2.85, 3.3, 5.0];
    let v = fixed.iter().copied().find(|f| (f - vout_v).abs() < 1e-6).ok_or_else(|| anyhow::anyhow!("ldo: AMS1117 comes in {fixed:?} V; no {vout_v} V part"))?;
    let part = format!("AMS1117-{v}");
    let mut c = Circuit::default();
    c.devices.insert(part.clone(), device("ic", FP_SOT223, &[("GND", "1"), ("VOUT", "2"), ("VIN", "3")], &[], Some(&part)));
    c.devices.insert("J_IN".into(), device("connector", &header(2), &[("1", "1"), ("2", "2")], &[], None));
    c.devices.insert("J_OUT".into(), device("connector", &header(2), &[("1", "1"), ("2", "2")], &[], None));
    c.passives.insert("c_in".into(), passive("capacitor", "10uF 25V X7R", FP_C1206));
    c.passives.insert("c_out".into(), passive("capacitor", "22uF 10V X7R", FP_C1206));
    let out = rail_name(v);
    c.nets.insert("VIN".into(), vec![format!("{part}.VIN"), "c_in.1".into(), "J_IN.1".into()]);
    c.nets.insert("GND".into(), vec![format!("{part}.GND"), "c_in.2".into(), "c_out.2".into(), "J_IN.2".into(), "J_OUT.2".into()]);
    c.nets.insert(out, vec![format!("{part}.VOUT"), "c_out.1".into(), "J_OUT.1".into()]);
    Ok(c)
}

/// Per-net design current, A, in `netlist.nets` order. Template circuits
/// carry the load: servo rail and its return at `iout_a`, the switch node
/// at the inductor's DC current, VBAT at Vout*Iout/(Vin_min*eta). Every
/// other net (and every net of an agent-proposed circuit) is a signal at
/// [`SIGNAL_A`]; a power-role net of an agent circuit gets 0.5 A, stated
/// in the ampacity check's detail.
pub fn net_currents(t: Option<&Template>, netlist: &Netlist) -> Vec<f64> {
    netlist
        .nets
        .iter()
        .map(|n| match t {
            Some(Template::ServoPower { vin_min_v, vout_v, iout_a, .. }) => {
                let out = rail_name(*vout_v);
                match n.name.as_str() {
                    "VBAT" => vout_v * iout_a / (vin_min_v * BUCK_EFFICIENCY),
                    "SW" | "GND" => *iout_a,
                    s if s == out => *iout_a,
                    _ => SIGNAL_A,
                }
            }
            Some(Template::Ldo { iout_a, .. }) => match n.role {
                wright_eda::ir::NetRole::Signal => SIGNAL_A,
                _ => *iout_a,
            },
            None => match n.role {
                wright_eda::ir::NetRole::Signal => SIGNAL_A,
                _ => 0.5,
            },
        })
        .collect()
}

/// The output rail's name for a template.
pub fn output_net(t: &Template) -> String {
    match *t {
        Template::ServoPower { vout_v, .. } | Template::Ldo { vout_v, .. } => rail_name(vout_v),
    }
}

pub fn spec_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["request"],
        "properties": {
            "request": {"type": "string"},
            "template": {"type": ["object", "null"], "description": "servo_power {vin_min_v, vin_max_v, vout_v, iout_a, servos} or ldo {vin_v, vout_v, iout_a}, tagged by kind",
                "properties": {
                    "kind": {"enum": ["servo_power", "ldo"]},
                    "vin_min_v": {"type": "number", "exclusiveMinimum": 0},
                    "vin_max_v": {"type": "number", "exclusiveMinimum": 0, "maximum": 36},
                    "vin_v": {"type": "number", "exclusiveMinimum": 0},
                    "vout_v": {"type": "number", "exclusiveMinimum": 0},
                    "iout_a": {"type": "number", "exclusiveMinimum": 0, "maximum": 10},
                    "servos": {"type": "integer", "minimum": 1, "maximum": 8}
                }},
            "circuit": {"type": ["object", "null"], "description": "baseline CIRCUIT-SPEC: devices{name:{kind,package,pins{name:pad},no_connect[]}}, passives{name:{type,value}}, nets{name:[PART.PIN]}"},
            "max_board_mm": {"type": ["array", "null"], "items": {"type": "number", "exclusiveMinimum": 0}, "minItems": 2, "maxItems": 2},
            "budget_usd": {"type": ["number", "null"], "exclusiveMinimum": 0}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn servo() -> Template {
        Template::ServoPower { vin_min_v: 14.0, vin_max_v: 16.8, vout_v: 6.0, iout_a: 5.0, servos: 5 }
    }

    #[test]
    fn e96_and_ohm_text_round_trip() {
        assert_eq!(ohm_text(e96(2554.9)), "2.55k");
        assert_eq!(ohm_text(10_000.0), "10k");
        assert_eq!(parse_ohm("2.55k"), Some(2550.0));
        assert_eq!(parse_ohm("5k1"), Some(5100.0));
        assert_eq!(parse_ohm("470R"), Some(470.0));
        assert_eq!(parse_ohm("10k 1%"), Some(10_000.0));
        assert_eq!(parse_ohm("abc"), None);
    }

    /// Same parts, same nets as the fixture (the Python baseline's, with every diode moved
    /// to KiCad pin 1 = K on 10/24; the Python baseline had the catch diode reversed).
    #[test]
    fn servo_template_matches_the_baseline_fixture_topology() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/electronics/fixtures/servo_power_6v5a.json");
        let baseline = Circuit::from_json(&std::fs::read_to_string(dir).unwrap()).unwrap();
        let ours = expand(&servo()).unwrap();
        assert_eq!(ours.devices.keys().collect::<Vec<_>>(), baseline.devices.keys().collect::<Vec<_>>());
        assert_eq!(ours.passives.keys().collect::<Vec<_>>(), baseline.passives.keys().collect::<Vec<_>>());
        assert_eq!(ours.nets.keys().collect::<Vec<_>>(), baseline.nets.keys().collect::<Vec<_>>());
        for (name, terms) in &baseline.nets {
            let mut a: Vec<String> = terms.clone();
            let mut b = ours.nets[name].clone();
            a.sort();
            b.sort();
            assert_eq!(a, b, "net {name}");
        }
        assert_eq!(ours.passives["R_FB2"].value, "2.55k");
        assert!(ours.nets["SW"].contains(&"D_CATCH.1".to_string()), "cathode on the switch node");
        ours.validate().unwrap();
    }

    #[test]
    fn currents_follow_the_load() {
        let n = expand(&servo()).unwrap().validate().unwrap();
        let i = net_currents(Some(&servo()), &n);
        let at = |name: &str| i[n.net_index(name).unwrap()];
        assert_eq!(at("+6V"), 5.0);
        assert_eq!(at("SW"), 5.0);
        assert!((at("VBAT") - 6.0 * 5.0 / (14.0 * 0.85)).abs() < 1e-12);
        assert_eq!(at("SIG_ELEVATOR"), SIGNAL_A);
        assert_eq!(output_net(&servo()), "+6V");
    }

    #[test]
    fn ldo_template_validates_and_refuses_odd_voltages() {
        let t = Template::Ldo { vin_v: 5.0, vout_v: 3.3, iout_a: 0.5 };
        let n = expand(&t).unwrap().validate().unwrap();
        assert!(n.net_index("+3V3").is_some());
        assert_eq!(output_net(&t), "+3V3");
        assert_eq!(rail_name(5.0), "+5V");
        assert_eq!(rail_name(2.85), "+2V85");
        assert!(expand(&Template::Ldo { vin_v: 5.0, vout_v: 3.0, iout_a: 0.5 }).is_err());
        assert!(validate(&Template::ServoPower { vin_min_v: 5.0, vin_max_v: 16.8, vout_v: 6.0, iout_a: 5.0, servos: 5 }).is_err());
    }
}
