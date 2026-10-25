//! The circuit IR: what an agent (or a template) proposes, and the validated
//! netlist every later stage reads.
//!
//! [`Circuit`] is wire-compatible with the Python baseline's CIRCUIT-SPEC JSON
//! (`engine/silkscreen/netlist.py:501 parse_circuit_spec`): `devices` keyed by
//! part number with a `pins` name->number map and `no_connect` names,
//! `passives` keyed by name with `type`/`value`, and `nets` mapping a net name
//! to `"PART.PIN"` terminals. So the baseline's fixtures (`evals/electronics/fixtures/`)
//! load unchanged. [`Circuit::validate`] collects **every** failure into one
//! [`ValidationError`] (the baseline's batched repair prompt, `netlist.py:88`), and
//! adds the rule the baseline enforced only in its in-loop ERC (`verify/circuit.py`):
//! every pin is on a net or declared no-connect.
//!
//! Reference designators are assigned here, once, in a fixed order (the baseline's
//! `CircuitSpec.assign_refs`): the schematic and the board both read
//! [`Part::refdes`], so they cannot disagree about what `C3` is.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A proposed circuit, as an agent writes it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Circuit {
    #[serde(default)]
    pub devices: BTreeMap<String, Device>,
    #[serde(default)]
    pub passives: BTreeMap<String, Passive>,
    #[serde(default)]
    pub nets: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Device {
    /// `ic` (default), `connector`, `battery`, `switch`, `testpoint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// A package name (`PinHeader_1x03_P2.54mm`, `XT30_2P`) or a KiCad
    /// footprint id (`Package_SO:TI_SO-PowerPAD-8`). ICs may omit it; the
    /// library lane resolves it from the part number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Pin name -> pad number.
    pub pins: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub no_connect: Vec<String>,
    /// Optional manufacturer part number (BOM).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpn: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Passive {
    /// `resistor`, `capacitor`, `inductor`, `diode`, `led`, `ferrite`.
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
    /// Optional imperial size (`0603`) or KiCad footprint id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

/// `PART.PIN` with the pin as a *name* for devices, `1`/`2` for passives.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PinRef {
    pub part: usize,
    /// Pad number on the footprint (devices: mapped from the pin name).
    pub pad: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Ic,
    Connector,
    Battery,
    Switch,
    Testpoint,
    Resistor,
    Capacitor,
    Inductor,
    Diode,
    Led,
    Ferrite,
}

impl PartKind {
    /// KiCad / IEEE 315 reference prefix.
    pub fn prefix(self) -> &'static str {
        match self {
            PartKind::Ic => "U",
            PartKind::Connector => "J",
            PartKind::Battery => "BT",
            PartKind::Switch => "SW",
            PartKind::Testpoint => "TP",
            PartKind::Resistor => "R",
            PartKind::Capacitor => "C",
            PartKind::Inductor => "L",
            PartKind::Diode | PartKind::Led => "D",
            PartKind::Ferrite => "FB",
        }
    }
    pub fn is_passive(self) -> bool {
        matches!(self, PartKind::Resistor | PartKind::Capacitor | PartKind::Inductor | PartKind::Diode | PartKind::Led | PartKind::Ferrite)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub name: String,
    pub pad: String,
    /// Index into [`Netlist::nets`]; `None` only when `no_connect`.
    pub net: Option<usize>,
    pub no_connect: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// `U1`, `C3`: assigned once by [`Circuit::validate`].
    pub refdes: String,
    /// The key the circuit used (`TPS5450DDA`, `C_IN1`).
    pub name: String,
    pub kind: PartKind,
    /// Value text (`10uF`, `TPS5450DDA`).
    pub value: String,
    pub package: Option<String>,
    pub mpn: Option<String>,
    pub pins: Vec<Pin>,
}

/// How a net is treated by width, placement weight and pour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetRole {
    Ground,
    Power,
    Signal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Net {
    pub name: String,
    pub role: NetRole,
    pub pins: Vec<PinRef>,
}

/// A validated circuit: every terminal resolved, refs assigned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Netlist {
    pub parts: Vec<Part>,
    pub nets: Vec<Net>,
}

impl Netlist {
    pub fn part(&self, refdes: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.refdes == refdes)
    }
    pub fn net_index(&self, name: &str) -> Option<usize> {
        self.nets.iter().position(|n| n.name == name)
    }
}

/// Every problem in a proposal, batched for one repair prompt.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[error("circuit has {} problem(s): {}", .problems.len(), .problems.join("; "))]
pub struct ValidationError {
    pub problems: Vec<String>,
}

/// The baseline's `netlist.py` `_IDENT_RE`.
fn ident_ok(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_ascii_alphabetic())
        && s.chars().all(|ch| ch.is_ascii_alphanumeric() || "_./+-".contains(ch))
}

/// Net role from its name, the baseline's `schematic.py net_class` (ground first).
pub fn net_role(name: &str) -> NetRole {
    let n = name.trim_start_matches('/').to_ascii_uppercase();
    if matches!(n.as_str(), "GND" | "AGND" | "DGND" | "PGND" | "GNDA" | "GNDD" | "VSS" | "0V") || n.starts_with("GND") {
        NetRole::Ground
    } else if n.starts_with('+') || n.starts_with("VBAT") || n.starts_with("VBUS") || n.starts_with("VIN") || n.starts_with("VCC") || n.starts_with("VDD") || n.starts_with("V+") || n == "VSYS" || n == "VOUT" {
        NetRole::Power
    } else {
        NetRole::Signal
    }
}

fn device_kind(d: &Device) -> Result<PartKind, String> {
    Ok(match d.kind.as_deref().unwrap_or("ic") {
        "ic" => PartKind::Ic,
        "connector" => PartKind::Connector,
        "battery" => PartKind::Battery,
        "switch" => PartKind::Switch,
        "testpoint" => PartKind::Testpoint,
        k => return Err(format!("unknown device kind {k:?} (ic, connector, battery, switch, testpoint)")),
    })
}

fn passive_kind(p: &Passive) -> Result<PartKind, String> {
    Ok(match p.kind.as_str() {
        "resistor" => PartKind::Resistor,
        "capacitor" => PartKind::Capacitor,
        "inductor" => PartKind::Inductor,
        "diode" => PartKind::Diode,
        "led" => PartKind::Led,
        "ferrite" => PartKind::Ferrite,
        k => return Err(format!("unknown passive type {k:?} (resistor, capacitor, inductor, diode, led, ferrite)")),
    })
}

impl Circuit {
    pub fn from_json(text: &str) -> anyhow::Result<Circuit> {
        // The baseline tolerated fenced model output (`netlist.py:463 _strip_code_fence`).
        let t = text.trim();
        let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
        let t = t.strip_suffix("```").unwrap_or(t);
        Ok(serde_json::from_str(t.trim())?)
    }

    /// Resolve every terminal, assign reference designators, and check that
    /// every pin is on exactly one net or declared no-connect.
    pub fn validate(&self) -> Result<Netlist, ValidationError> {
        let mut problems = Vec::new();
        let mut parts: Vec<Part> = Vec::new();
        let mut by_name: BTreeMap<&str, usize> = BTreeMap::new();
        // Devices first, then passives, each in key order: deterministic refs.
        for (name, d) in &self.devices {
            if !ident_ok(name) {
                problems.push(format!("device name {name:?} is not an identifier"));
            }
            let kind = device_kind(d).unwrap_or_else(|e| {
                problems.push(format!("{name}: {e}"));
                PartKind::Ic
            });
            if d.pins.is_empty() {
                problems.push(format!("{name}: device has no pins"));
            }
            let mut seen_pad = BTreeSet::new();
            for (pin, pad) in &d.pins {
                if !seen_pad.insert(pad.as_str()) {
                    problems.push(format!("{name}: pad {pad} is mapped by two pin names (one is {pin})"));
                }
            }
            for nc in &d.no_connect {
                if !d.pins.contains_key(nc) {
                    problems.push(format!("{name}: no_connect names {nc:?}, which is not one of its pins"));
                }
            }
            let pins = d.pins.iter().map(|(n, pad)| Pin { name: n.clone(), pad: pad.clone(), net: None, no_connect: d.no_connect.contains(n) }).collect();
            by_name.insert(name, parts.len());
            parts.push(Part { refdes: String::new(), name: name.clone(), kind, value: name.clone(), package: d.package.clone(), mpn: d.mpn.clone(), pins });
        }
        for (name, p) in &self.passives {
            if !ident_ok(name) {
                problems.push(format!("passive name {name:?} is not an identifier"));
            }
            if by_name.contains_key(name.as_str()) {
                problems.push(format!("{name} is both a device and a passive"));
            }
            let kind = passive_kind(p).unwrap_or_else(|e| {
                problems.push(format!("{name}: {e}"));
                PartKind::Resistor
            });
            let pins = ["1", "2"].iter().map(|n| Pin { name: n.to_string(), pad: n.to_string(), net: None, no_connect: false }).collect();
            by_name.insert(name, parts.len());
            parts.push(Part { refdes: String::new(), name: name.clone(), kind, value: p.value.clone(), package: p.package.clone(), mpn: None, pins });
        }
        // Reference designators: per prefix, in part order.
        let mut counters: BTreeMap<&str, usize> = BTreeMap::new();
        for p in parts.iter_mut() {
            let c = counters.entry(p.kind.prefix()).or_insert(0);
            *c += 1;
            p.refdes = format!("{}{}", p.kind.prefix(), c);
        }
        let mut nets = Vec::new();
        for (net_name, terms) in &self.nets {
            if terms.len() < 2 {
                problems.push(format!("net {net_name} has {} terminal(s); a net joins at least two pins", terms.len()));
            }
            let idx = nets.len();
            let mut pins = Vec::new();
            for t in terms {
                let Some((part, pin)) = t.rsplit_once('.') else {
                    problems.push(format!("net {net_name}: terminal {t:?} is not PART.PIN"));
                    continue;
                };
                let Some(&pi) = by_name.get(part) else {
                    problems.push(format!("net {net_name}: {t:?} names no part {part:?}"));
                    continue;
                };
                let Some(pin_i) = parts[pi].pins.iter().position(|p| p.name == pin || (parts[pi].kind.is_passive() && p.pad == pin)) else {
                    problems.push(format!("net {net_name}: {part} has no pin {pin:?}"));
                    continue;
                };
                let slot = &mut parts[pi].pins[pin_i];
                if slot.no_connect {
                    problems.push(format!("net {net_name}: {t} is declared no_connect"));
                }
                if let Some(other) = slot.net {
                    if other != idx {
                        problems.push(format!("{t} is on two nets ({} and {net_name})", self.nets.keys().nth(other).map(String::as_str).unwrap_or("?")));
                    }
                }
                slot.net = Some(idx);
                pins.push(PinRef { part: pi, pad: slot.pad.clone() });
            }
            nets.push(Net { name: net_name.clone(), role: net_role(net_name), pins });
        }
        for p in &parts {
            for pin in &p.pins {
                if pin.net.is_none() && !pin.no_connect {
                    problems.push(format!("{}.{} ({}) is on no net and not declared no_connect", p.name, pin.name, p.refdes));
                }
            }
        }
        if problems.is_empty() {
            Ok(Netlist { parts, nets })
        } else {
            Err(ValidationError { problems })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LDO: &str = r#"{"devices": {"AMS1117-3.3": {"pins": {"GND": "1", "OUT": "2", "IN": "3"}}},
      "passives": {"c_in": {"type": "capacitor", "value": "10uF"}, "c_out": {"type": "capacitor", "value": "22uF"}},
      "nets": {"VIN": ["AMS1117-3.3.IN", "c_in.1"], "GND": ["AMS1117-3.3.GND", "c_in.2", "c_out.2"], "+3V3": ["AMS1117-3.3.OUT", "c_out.1"]}}"#;

    #[test]
    fn baseline_fixture_validates_with_stable_refs() {
        let n = Circuit::from_json(LDO).unwrap().validate().unwrap();
        let refs: Vec<_> = n.parts.iter().map(|p| p.refdes.as_str()).collect();
        assert_eq!(refs, ["U1", "C1", "C2"]);
        assert_eq!(n.nets[n.net_index("GND").unwrap()].role, NetRole::Ground);
        assert_eq!(n.nets[n.net_index("+3V3").unwrap()].role, NetRole::Power);
    }

    #[test]
    fn every_problem_is_batched_and_floating_pins_fail() {
        let mut c = Circuit::from_json(LDO).unwrap();
        c.nets.remove("+3V3");
        c.nets.insert("BAD".into(), vec!["nope.1".into(), "c_in.9".into()]);
        let e = c.validate().unwrap_err();
        assert!(e.problems.iter().any(|p| p.contains("names no part")), "{e}");
        assert!(e.problems.iter().any(|p| p.contains("has no pin")), "{e}");
        assert!(e.problems.iter().any(|p| p.contains("AMS1117-3.3.OUT") && p.contains("no net")), "{e}");
    }

    #[test]
    fn every_baseline_fixture_on_disk_validates() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/electronics/fixtures");
        let mut n = 0;
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "json") {
                let c = Circuit::from_json(&std::fs::read_to_string(&p).unwrap()).unwrap();
                c.validate().unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                n += 1;
            }
        }
        assert!(n >= 7, "expected the 7 baseline fixtures, found {n}");
    }
}
