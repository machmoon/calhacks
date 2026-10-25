//! KiCad library reader: resolve each netlist part to a footprint in the
//! installed KiCad library, and parse that `.kicad_mod` into a
//! [`wright_eda::Footprint`] (pads, courtyard, 3D model, verbatim source).
//!
//! Replaces the Python baseline's `engine/silkscreen/kicadlib/` (`resolve.py`, `index.py`,
//! `footprint.py`) and the land-pattern generators of `footprints.py`: every
//! footprint here is KiCad's own file, never a re-drawn pattern (the baseline shipped
//! mirrored pin 1s and 0.1 mm SOT-223 gaps from its generated ones).
//!
//! Rules, from the baseline's `kicadlib/resolve.py`:
//! * An IC resolves by **exact** symbol name in `symbols/*.kicad_sym`, or a
//!   KiCad `x` placeholder name (`ATtiny85-20S` style libraries write
//!   `STM32F103C8Tx`), never by token similarity. Its footprint is the
//!   symbol's `Footprint` property (following `extends` to the parent).
//! * Passive sizes follow the baseline's `footprints.py for_passive` (bulk capacitance
//!   to an electrolytic, a microhenry inductor to a power inductor, a power
//!   Schottky to SMC), mapped to the KiCad library file the baseline measured from.
//! * Connector families follow the baseline's `kicadlib/connectors.py` (KLC F2/F3 names,
//!   read from the installed `.pretty`), for the two families the fixtures
//!   use; everything else is the `evals/electronics/packages.json` table.
//! * Failures are batched, like `Circuit::validate`: one error names every
//!   part that did not resolve and every pin whose pad the footprint lacks.
//!
//! Courtyard: the union of `F.CrtYd` graphics, with arcs bounded exactly
//! (the baseline's `kicad.py _courtyard_points` / `_arc_points`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use wright_eda::sexpr::Sexpr;
use wright_eda::{Footprint, Layer, Netlist, Nm, Pad, PadShape, Part, PartKind, Pt, Rect};

/// `evals/electronics/packages.json`, compiled in so a binary run from any
/// directory resolves the same way.
const PACKAGES_JSON: &str = include_str!("../../../evals/electronics/packages.json");

#[derive(serde::Deserialize)]
struct Tables {
    packages: BTreeMap<String, String>,
    devices: BTreeMap<String, String>,
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(PACKAGES_JSON).expect("evals/electronics/packages.json is valid JSON"))
}

/// IPC-7351 nominal courtyard excess, used when a footprint has no `F.CrtYd`.
const COURTYARD_EXCESS_MM: f64 = 0.25;

/// `<share>/footprints/<Lib>.pretty/<Name>.kicad_mod`.
pub fn footprint_path(share: &Path, lib_id: &str) -> anyhow::Result<PathBuf> {
    let (lib, name) = lib_id.split_once(':').ok_or_else(|| anyhow::anyhow!("footprint id {lib_id:?} is not Library:Name"))?;
    Ok(share.join("footprints").join(format!("{lib}.pretty")).join(format!("{name}.kicad_mod")))
}

/// Read and parse one library footprint.
pub fn load_footprint(share: &Path, lib_id: &str) -> anyhow::Result<Footprint> {
    let path = footprint_path(share, lib_id)?;
    let text = std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("footprint {lib_id}: cannot read {}: {e}", path.display()))?;
    parse_footprint(lib_id, &text)
}

fn pt_of(e: &Sexpr) -> Option<Pt> {
    Some(Pt::mm(e.num(1)?, e.num(2)?))
}

/// Parse `.kicad_mod` text. `lib_id` is recorded as the footprint's id.
pub fn parse_footprint(lib_id: &str, text: &str) -> anyhow::Result<Footprint> {
    let e = Sexpr::parse(text)?;
    anyhow::ensure!(matches!(e.head(), Some("footprint") | Some("module")), "{lib_id}: not a footprint file (head {:?})", e.head());
    let mut pads = Vec::new();
    for p in e.children("pad") {
        let number = p.text(1).unwrap_or("").to_string();
        let kind = p.text(2).unwrap_or("smd");
        let shape_tok = p.text(3).unwrap_or("rect");
        let at = p.child("at").ok_or_else(|| anyhow::anyhow!("{lib_id}: pad {number:?} has no (at)"))?;
        let size = p.child("size").ok_or_else(|| anyhow::anyhow!("{lib_id}: pad {number:?} has no (size)"))?;
        let shape = match shape_tok {
            "rect" => PadShape::Rect,
            "roundrect" => PadShape::RoundRect(p.child("roundrect_rratio").and_then(|r| r.num(1)).unwrap_or(0.25)),
            "circle" => PadShape::Circle,
            "oval" => PadShape::Oval,
            _ => PadShape::Other,
        };
        // (drill 1.0) | (drill oval 1.0 2.0) | (drill 1.0 (offset ..)): the
        // first number is the (minor) diameter.
        let drill_mm = p.child("drill").and_then(|d| d.items().iter().skip(1).find_map(|i| match i {
            Sexpr::Atom(a) => a.parse::<f64>().ok(),
            _ => None,
        }));
        let (drill, plated) = match kind {
            "thru_hole" => (drill_mm.map(Nm::mm), true),
            "np_thru_hole" => (drill_mm.map(Nm::mm), false),
            _ => (None, false),
        };
        // Same reading as wright_electronics::copper::source_pad_layers.
        let mut layers = Vec::new();
        for t in p.child("layers").map(|l| &l.items()[1..]).unwrap_or(&[]) {
            match t {
                Sexpr::Atom(t) | Sexpr::Str(t) => match t.as_str() {
                    "F.Cu" => layers.push(Layer::FCu),
                    "B.Cu" => layers.push(Layer::BCu),
                    "*.Cu" | "F&B.Cu" => layers.extend([Layer::FCu, Layer::BCu]),
                    _ => {}
                },
                _ => {}
            }
        }
        layers.sort();
        layers.dedup();
        pads.push(Pad {
            number,
            at: pt_of(at).ok_or_else(|| anyhow::anyhow!("{lib_id}: bad pad (at)"))?,
            rot_deg: at.num(3).unwrap_or(0.0),
            size: (Nm::mm(size.num(1).unwrap_or(0.0)), Nm::mm(size.num(2).unwrap_or(0.0))),
            shape,
            drill,
            plated,
            layers,
        });
    }
    let courtyard = courtyard_of(&e).unwrap_or_else(|| {
        let b = pads.iter().map(|p| p.local_bbox()).reduce(|a, b| a.union(&b)).unwrap_or_default();
        b.inflate(Nm::mm(COURTYARD_EXCESS_MM))
    });
    let model3d = e.child("model").and_then(|m| m.text(1)).map(str::to_string);
    Ok(Footprint { lib_id: lib_id.to_string(), pads, courtyard, model3d, source: text.to_string() })
}

/// Bounding box of every `F.CrtYd` graphic, or `None` if there is none.
fn courtyard_of(e: &Sexpr) -> Option<Rect> {
    let mut pts: Vec<Pt> = Vec::new();
    for g in e.items() {
        let on_crtyd = g.child("layer").and_then(|l| l.text(1)) == Some("F.CrtYd");
        if !on_crtyd {
            continue;
        }
        let get = |k: &str| g.child(k).and_then(pt_of);
        match g.head() {
            Some("fp_line") | Some("fp_rect") => pts.extend([get("start"), get("end")].into_iter().flatten()),
            Some("fp_poly") => {
                if let Some(ps) = g.child("pts") {
                    pts.extend(ps.children("xy").filter_map(pt_of));
                }
            }
            Some("fp_circle") => {
                if let (Some(c), Some(end)) = (get("center"), get("end")) {
                    let r = c.dist(end).round() as i64;
                    pts.push(Pt::new(c.x.0 - r, c.y.0 - r));
                    pts.push(Pt::new(c.x.0 + r, c.y.0 + r));
                }
            }
            Some("fp_arc") => {
                if let (Some(s), Some(m), Some(en)) = (get("start"), get("mid"), get("end")) {
                    pts.extend(arc_extent(s, m, en));
                }
            }
            _ => {}
        }
    }
    let first = *pts.first()?;
    Some(pts.iter().fold(Rect::from_corners(first, first), |r, p| r.union(&Rect::from_corners(*p, *p))))
}

/// The points that bound a three-point arc: its ends plus every axis extreme
/// the sweep passes (the baseline's `kicad.py _arc_points`). Collinear points fall back
/// to the three points.
fn arc_extent(s: Pt, m: Pt, e: Pt) -> Vec<Pt> {
    let f = |p: Pt| (p.x.0 as f64, p.y.0 as f64);
    let ((ax, ay), (bx, by), (cx, cy)) = (f(s), f(m), f(e));
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-6 {
        return vec![s, m, e];
    }
    let ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
    let r = (ax - ux).hypot(ay - uy);
    let ang = |x: f64, y: f64| (y - uy).atan2(x - ux).rem_euclid(std::f64::consts::TAU);
    let (a0, am, a1) = (ang(ax, ay), ang(bx, by), ang(cx, cy));
    // Sweep from a0 increasing; it passes the mid point or goes the other way.
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
    let increasing = ccw(a0, am) <= ccw(a0, a1);
    let mut out = vec![s, e];
    for k in 0..4 {
        let t = k as f64 * std::f64::consts::FRAC_PI_2;
        let inside = if increasing { ccw(a0, t) <= ccw(a0, a1) } else { ccw(a1, t) <= ccw(a1, a0) };
        if inside {
            out.push(Pt::new((ux + r * t.cos()).round() as i64, (uy + r * t.sin()).round() as i64));
        }
    }
    out
}

// ---------------------------------------------------------------- symbols

/// One top-level symbol in an installed `.kicad_sym`.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolEntry {
    /// `Lib:Name`.
    pub lib_id: String,
    /// The `Footprint` property (after following `extends`), `""` if none.
    pub footprint: String,
}

struct SymIndex {
    /// Symbol name -> (library file, byte offset of its `(symbol`).
    by_name: HashMap<String, Vec<(PathBuf, usize)>>,
}

/// Every top-level symbol name in `<share>/symbols`, read once per process.
/// A top-level symbol is a line `\t(symbol "NAME"` (KiCad's own one-tab
/// indent); sub-units are two tabs deep and are skipped.
fn sym_index(share: &Path) -> Arc<SymIndex> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<SymIndex>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = share.to_path_buf();
    if let Some(i) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return i.clone();
    }
    let mut by_name: HashMap<String, Vec<(PathBuf, usize)>> = HashMap::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(share.join("symbols"))
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "kicad_sym")).collect())
        .unwrap_or_default();
    files.sort();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let mut off = 0;
        for line in text.split_inclusive('\n') {
            if let Some(rest) = line.strip_prefix("\t(symbol \"") {
                if let Some(end) = rest.find('"') {
                    by_name.entry(rest[..end].to_string()).or_default().push((f.clone(), off + 1));
                }
            }
            off += line.len();
        }
    }
    let idx = Arc::new(SymIndex { by_name });
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(key, idx.clone());
    idx
}

/// The end (exclusive) of the s-expression list starting at `start`.
fn list_end(b: &[u8], start: usize) -> Option<usize> {
    let (mut depth, mut i, mut in_str) = (0usize, start, false);
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

fn read_symbol(file: &Path, off: usize) -> Option<Sexpr> {
    let text = std::fs::read_to_string(file).ok()?;
    let end = list_end(text.as_bytes(), off)?;
    Sexpr::parse(&text[off..end]).ok()
}

fn property(sym: &Sexpr, key: &str) -> Option<String> {
    sym.children("property").find(|p| p.text(1) == Some(key)).and_then(|p| p.text(2)).map(str::to_string)
}

fn entry_for(idx: &SymIndex, name: &str, file: &Path, off: usize) -> Option<SymbolEntry> {
    let lib = file.file_stem()?.to_str()?.to_string();
    let mut sym = read_symbol(file, off)?;
    let mut footprint = property(&sym, "Footprint").unwrap_or_default();
    // Follow `extends` (same library) while the footprint is empty.
    for _ in 0..8 {
        if !footprint.is_empty() {
            break;
        }
        let Some(parent) = sym.child("extends").and_then(|x| x.text(1)).map(str::to_string) else { break };
        let Some((pf, po)) = idx.by_name.get(&parent).and_then(|v| v.iter().find(|(f, _)| f == file)).cloned() else { break };
        sym = read_symbol(&pf, po)?;
        footprint = property(&sym, "Footprint").unwrap_or_default();
    }
    Some(SymbolEntry { lib_id: format!("{lib}:{name}"), footprint })
}

/// KiCad `x` placeholder: an `x` after a digit or capital stands for one
/// character (the baseline's `resolve.py _placeholder_match`).
fn placeholder_match(name: &str, part: &str) -> bool {
    let n: Vec<char> = name.chars().collect();
    let p: Vec<char> = part.trim().chars().collect();
    if n.len() != p.len() {
        return false;
    }
    let mut any = false;
    for i in 0..n.len() {
        let wild = n[i] == 'x' && i > 0 && (n[i - 1].is_ascii_digit() || n[i - 1].is_ascii_uppercase());
        if wild {
            any = true;
            continue;
        }
        if !n[i].eq_ignore_ascii_case(&p[i]) {
            return false;
        }
    }
    any
}

/// The library symbol for a part number: exact name, else a placeholder
/// name; among several, the one naming a footprint, then by `lib_id`.
pub fn find_symbol(share: &Path, part_number: &str) -> Option<SymbolEntry> {
    let idx = sym_index(share);
    let mut hits: Vec<SymbolEntry> = Vec::new();
    if let Some(v) = idx.by_name.get(part_number) {
        hits.extend(v.iter().filter_map(|(f, o)| entry_for(&idx, part_number, f, *o)));
    }
    if hits.is_empty() {
        let mut names: Vec<&String> = idx.by_name.keys().filter(|n| n.eq_ignore_ascii_case(part_number.trim()) || placeholder_match(n, part_number)).collect();
        names.sort();
        for n in names {
            hits.extend(idx.by_name[n].iter().filter_map(|(f, o)| entry_for(&idx, n, f, *o)));
        }
    }
    hits.sort_by(|a, b| (a.footprint.is_empty(), &a.lib_id).cmp(&(b.footprint.is_empty(), &b.lib_id)));
    hits.into_iter().next()
}

// ---------------------------------------------------------------- passives

/// `'100nF' -> 1e-7` for unit letter `f` (the baseline's `footprints.py _parse_value`):
/// first token only, SI prefix p/n/u/µ/m.
pub fn parse_value(value: &str, unit_letter: char) -> Option<f64> {
    let tok = value.split_whitespace().next()?.to_lowercase();
    let tok = tok.trim_end_matches(unit_letter);
    let (num, mult) = match tok.chars().last()? {
        'p' => (&tok[..tok.len() - 1], 1e-12),
        'n' => (&tok[..tok.len() - 1], 1e-9),
        'u' => (&tok[..tok.len() - 1], 1e-6),
        'µ' => (&tok[..tok.len() - 'µ'.len_utf8()], 1e-6),
        'm' => (&tok[..tok.len() - 1], 1e-3),
        _ => (tok, 1.0),
    };
    num.trim().parse::<f64>().ok().map(|v| v * mult)
}

/// The baseline's `footprints.py _is_power_diode`: a current of 2 A or more in the value,
/// or a power Schottky part-number stem.
fn is_power_diode(value: &str) -> bool {
    const STEMS: [&str; 13] = ["ss3", "ss5", "b340", "b540", "b560", "sk3", "sk5", "mbrs3", "mbrs5", "stps3", "stps5", "sr540", "sr560"];
    let t = value.to_lowercase().replace(' ', "");
    if STEMS.iter().any(|s| t.contains(s)) {
        return true;
    }
    // "<number>a" not followed by a letter
    let b = t.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'a' && !b.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic()) {
            let start = t[..i].rfind(|ch: char| !(ch.is_ascii_digit() || ch == '.')).map_or(0, |k| k + 1);
            if let Ok(v) = t[start..i].parse::<f64>() {
                if v >= 2.0 {
                    return true;
                }
            }
        }
    }
    false
}

fn chip(kind: PartKind, imperial: &str) -> Option<String> {
    let metric = match imperial {
        "0201" => "0603",
        "0402" => "1005",
        "0603" => "1608",
        "0805" => "2012",
        "1206" => "3216",
        "1210" => "3225",
        "1812" => "4532",
        "2010" => "5025",
        "2512" => "6332",
        _ => return None,
    };
    let (lib, p) = match kind {
        PartKind::Resistor => ("Resistor_SMD", "R"),
        PartKind::Capacitor => ("Capacitor_SMD", "C"),
        PartKind::Inductor | PartKind::Ferrite => ("Inductor_SMD", "L"),
        PartKind::Diode => ("Diode_SMD", "D"),
        PartKind::Led => ("LED_SMD", "LED"),
        _ => return None,
    };
    Some(format!("{lib}:{p}_{imperial}_{metric}Metric"))
}

fn is_led(part: &Part) -> bool {
    part.kind == PartKind::Led || (part.kind == PartKind::Diode && part.value.to_lowercase().contains("led"))
}

/// Footprint for a passive with no package (the baseline's `footprints.py for_passive`).
fn passive_default(part: &Part) -> String {
    match part.kind {
        PartKind::Capacitor => match parse_value(&part.value, 'f') {
            // 1e-9 relative slack, as the baseline: no MLCC reaches 100 uF.
            Some(f) if f >= 100e-6 * (1.0 - 1e-9) => "Capacitor_SMD:CP_Elec_10x10".into(),
            Some(f) if f >= 10e-6 * (1.0 - 1e-9) => "Capacitor_SMD:C_1206_3216Metric".into(),
            Some(f) if f >= 1e-6 * (1.0 - 1e-9) => "Capacitor_SMD:C_0805_2012Metric".into(),
            _ => "Capacitor_SMD:C_0603_1608Metric".into(),
        },
        PartKind::Inductor => match parse_value(&part.value, 'h') {
            Some(h) if h >= 1e-6 * (1.0 - 1e-9) => "Inductor_SMD:L_Bourns_SRR1260".into(),
            _ => "Inductor_SMD:L_0805_2012Metric".into(),
        },
        PartKind::Diode if is_led(part) => "LED_SMD:LED_0603_1608Metric".into(),
        // Deviation from the baseline (0603 chip diode): SOD-123 is the package
        // small-signal diodes (1N4148W, BAT54) actually ship in.
        PartKind::Diode if !is_power_diode(&part.value) => "Diode_SMD:D_SOD-123".into(),
        PartKind::Diode => "Diode_SMD:D_SMC".into(),
        PartKind::Led => "LED_SMD:LED_0603_1608Metric".into(),
        PartKind::Ferrite => "Inductor_SMD:L_0603_1608Metric".into(),
        _ => "Resistor_SMD:R_0603_1608Metric".into(),
    }
}

/// Connector families by KiCad's KLC names (the baseline's `kicadlib/connectors.py`).
fn family(package: &str) -> Option<String> {
    let num = |s: &str| s.parse::<u32>().ok().filter(|n| (1..=40).contains(n));
    if let Some(n) = package.strip_prefix("PinHeader_1x").and_then(|r| r.strip_suffix("_P2.54mm")).and_then(num) {
        return Some(format!("Connector_PinHeader_2.54mm:PinHeader_1x{n:02}_P2.54mm_Vertical"));
    }
    if let Some(n) = package.strip_prefix("JST_PH_").and_then(|r| r.strip_suffix('P')).and_then(num) {
        return Some(format!("Connector_JST:JST_PH_B{n}B-PH-K_1x{n:02}_P2.00mm_Vertical"));
    }
    None
}

/// The KiCad footprint id for one part, or the reason there is none.
pub fn footprint_id(part: &Part, share: &Path) -> Result<String, String> {
    let t = tables();
    if let Some(pkg) = part.package.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        if pkg.contains(':') {
            return Ok(pkg.to_string());
        }
        if part.kind.is_passive() {
            if let Some(id) = chip(if is_led(part) { PartKind::Led } else { part.kind }, pkg) {
                return Ok(id);
            }
        }
        if let Some(id) = t.packages.get(pkg).cloned().or_else(|| family(pkg)) {
            return Ok(id);
        }
        return Err(format!("{} ({}): package {pkg:?} is not a KiCad footprint id (Lib:Name), a chip size, or a known package ({})", part.refdes, part.name, t.packages.keys().cloned().collect::<Vec<_>>().join(", ")));
    }
    if part.kind.is_passive() {
        return Ok(passive_default(part));
    }
    if part.kind != PartKind::Ic {
        return Err(format!("{} ({}): a {:?} needs a package (a KiCad footprint id or one of: {})", part.refdes, part.name, part.kind, t.packages.keys().cloned().collect::<Vec<_>>().join(", ")));
    }
    let keys: Vec<&str> = std::iter::once(part.name.as_str()).chain(part.mpn.as_deref()).collect();
    for k in &keys {
        if let Some(id) = t.devices.get(*k) {
            return Ok(id.clone());
        }
    }
    for k in &keys {
        if let Some(sym) = find_symbol(share, k) {
            if !sym.footprint.is_empty() {
                return Ok(sym.footprint);
            }
            return Err(format!("{} ({}): KiCad's symbol {} names no footprint; give the device a package", part.refdes, part.name, sym.lib_id));
        }
    }
    Err(format!("{} ({}): no KiCad symbol is named {:?} (exact or x-placeholder match only); give the device a package (a KiCad footprint id)", part.refdes, part.name, part.name))
}

/// Footprint for every part, in part order; every failure batched.
pub fn resolve_footprints(netlist: &Netlist, share: &Path) -> anyhow::Result<Vec<Footprint>> {
    let mut problems = Vec::new();
    let mut out = Vec::new();
    let mut cache: HashMap<String, Footprint> = HashMap::new();
    for part in &netlist.parts {
        let id = match footprint_id(part, share) {
            Ok(id) => id,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        let fp = match cache.get(&id) {
            Some(f) => f.clone(),
            None => match load_footprint(share, &id) {
                Ok(f) => {
                    cache.insert(id.clone(), f.clone());
                    f
                }
                Err(e) => {
                    problems.push(format!("{} ({}): {e}", part.refdes, part.name));
                    continue;
                }
            },
        };
        let missing: Vec<String> = part.pins.iter().filter(|pin| !fp.pads.iter().any(|p| p.number == pin.pad)).map(|pin| format!("{}={}", pin.name, pin.pad)).collect();
        if !missing.is_empty() {
            let mut have: Vec<&str> = fp.pads.iter().map(|p| p.number.as_str()).filter(|n| !n.is_empty()).collect();
            have.sort();
            have.dedup();
            problems.push(format!("{} ({}): footprint {} has no pad for pins {} (its pads: {})", part.refdes, part.name, id, missing.join(", "), have.join(" ")));
        }
        out.push(fp);
    }
    if !problems.is_empty() {
        anyhow::bail!("{} footprint problem(s): {}", problems.len(), problems.join("; "));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_like_baseline() {
        assert!((parse_value("100nF", 'f').unwrap() - 1e-7).abs() < 1e-18);
        assert!((parse_value("220uF 16V", 'f').unwrap() - 2.2e-4).abs() < 1e-15);
        assert!((parse_value("15uH", 'h').unwrap() - 1.5e-5).abs() < 1e-15);
        assert!((parse_value("10u", 'f').unwrap() - 10e-6).abs() < 1e-15);
        assert_eq!(parse_value("LED", 'f'), None);
    }

    #[test]
    fn power_diodes_and_placeholders() {
        assert!(is_power_diode("SS54 5A 40V"));
        assert!(is_power_diode("3A"));
        assert!(!is_power_diode("1N4148"));
        assert!(!is_power_diode("LED"));
        assert!(placeholder_match("STM32F103C8Tx", "STM32F103C8T6"));
        assert!(!placeholder_match("STM32F103C8Tx", "STM32F103CBT6"));
        assert!(!placeholder_match("NE555D", "NE555D"));
    }

    #[test]
    fn families_follow_klc_names() {
        assert_eq!(family("PinHeader_1x03_P2.54mm").unwrap(), "Connector_PinHeader_2.54mm:PinHeader_1x03_P2.54mm_Vertical");
        assert_eq!(family("JST_PH_2P").unwrap(), "Connector_JST:JST_PH_B2B-PH-K_1x02_P2.00mm_Vertical");
        assert_eq!(family("XT30_2P"), None);
    }

    #[test]
    fn arc_extent_includes_the_swept_extreme() {
        // Half circle radius 1 mm from (-1,0) through (0,-1) to (1,0): top at y=-1.
        let pts = arc_extent(Pt::mm(-1.0, 0.0), Pt::mm(0.0, -1.0), Pt::mm(1.0, 0.0));
        assert!(pts.iter().any(|p| p.y == Nm::mm(-1.0)));
        assert!(!pts.iter().any(|p| p.y == Nm::mm(1.0)));
    }

    #[test]
    fn synthetic_footprint_parses_pads_and_courtyard() {
        let t = r#"(footprint "X" (layer "F.Cu")
            (fp_rect (start -2 -1) (end 2 1) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
            (fp_arc (start -2 1) (mid 0 3) (end 2 1) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
            (pad "1" thru_hole circle (at -1 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask"))
            (pad "2" smd roundrect (at 1 0 90) (size 0.8 0.95) (layers "F.Cu") (roundrect_rratio 0.2))
            (pad "" np_thru_hole circle (at 0 0) (size 1 1) (drill oval 1 1.5) (layers "*.Cu"))
            (model "${KICAD10_3DMODEL_DIR}/X.step"))"#;
        let f = parse_footprint("L:X", t).unwrap();
        assert_eq!(f.pads.len(), 3);
        assert_eq!(f.pads[0].drill, Some(Nm::mm(1.0)));
        assert!(f.pads[0].plated);
        assert_eq!(f.pads[1].shape, PadShape::RoundRect(0.2));
        assert_eq!(f.pads[0].layers, [Layer::FCu, Layer::BCu]);
        assert_eq!(f.pads[1].layers, [Layer::FCu]);
        assert_eq!(f.pads[1].rot_deg, 90.0);
        assert_eq!((f.pads[2].drill, f.pads[2].plated), (Some(Nm::mm(1.0)), false));
        // The arc (centre (0,1), r 2) bulges to y = 3.
        assert_eq!(f.courtyard, Rect::from_corners(Pt::mm(-2.0, -1.0), Pt::mm(2.0, 3.0)));
        assert_eq!(f.model3d.as_deref(), Some("${KICAD10_3DMODEL_DIR}/X.step"));
    }
}
