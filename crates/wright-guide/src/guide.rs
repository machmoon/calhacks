//! The build guide: one `guide.html` a person can follow at the bench, with
//! its images and files in folders beside it (so the folder zips, prints and
//! opens from `file://`).
//!
//! Shape taken from GitBuilding (gitlab.com/gitbuilding/gitbuilding, the
//! Open Know-How documentation tool for open hardware):
//! `gitbuilding/buildup/partlist.py` -- the bill of materials is divided
//! into one heading per part category (`bom_md(divide=True)`), it is also
//! offered as CSV (`bom_csv`), and every step page lists the parts that step
//! uses. Wright adds what GitBuilding does not have: the prints as sliced
//! plates with real time and grams, the check report, and a renders-first
//! assembly where each step's new parts are highlighted.
//!
//! Honesty rules, from the product brief: failures come first; a line with
//! no price says "unpriced" and is left out of the total, which says so; a
//! link to a shop's homepage is marked; a printed part nobody sliced is
//! listed as "not sliced"; a part no assembly step mentions gets a step of
//! its own rather than being left out.

use crate::render::{rasterize, Options, View};
use anyhow::Context as _;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use wright_core::check::{Evidence, Source};
use wright_core::{Bom, Finished};
use wright_geom::slice::Sliced;
use wright_geom::{Part, PartKind};

/// One assembly step: the parts it adds, and what to do.
#[derive(Debug, Clone, Default)]
pub struct Step {
    pub title: String,
    /// Names of the parts this step adds (highlighted in its picture).
    pub parts: Vec<String>,
    /// Plain instructions. Empty: the guide writes a default line.
    pub text: String,
}

/// Content for a domain-provided section.
#[derive(Debug, Clone)]
pub enum Block {
    Para(String),
    /// Tick boxes (a pre-flight list).
    Checklist(Vec<String>),
    Numbered(Vec<String>),
    Table { head: Vec<String>, rows: Vec<Vec<String>> },
    /// A file copied beside the guide and linked (params, harness, notice).
    File { label: String, path: PathBuf },
    /// Trusted HTML from the domain, inserted as is.
    Html(String),
}

#[derive(Debug, Clone)]
pub struct Section {
    pub title: String,
    pub blocks: Vec<Block>,
}

impl Section {
    pub fn new(title: impl Into<String>, blocks: Vec<Block>) -> Self {
        Self { title: title.into(), blocks }
    }
    /// A titled checklist, e.g. `Section::checklist("Before the first flight", [...])`.
    pub fn checklist<S: Into<String>>(title: impl Into<String>, items: impl IntoIterator<Item = S>) -> Self {
        Self::new(title, vec![Block::Checklist(items.into_iter().map(Into::into).collect())])
    }
}

/// Everything the guide shows. Build with [`Guide::new`] and set the rest.
#[derive(Debug, Clone)]
pub struct Guide<'a> {
    pub title: String,
    /// One or two plain sentences under the title.
    pub summary: String,
    pub parts: &'a [Part],
    pub bom: &'a Bom,
    pub checks: &'a Finished,
    /// One per print plate: the part's name and its slicer result.
    pub plates: Vec<(String, Sliced)>,
    /// Explicit assembly order; `None` uses [`default_steps`].
    pub steps: Option<Vec<Step>>,
    /// A wiring diagram (SVG or PNG), shown in "Wiring".
    pub wiring: Option<PathBuf>,
    /// The domain's last checklist: "Before the first flight" (air),
    /// "Before launch" (rocket), "Before release" (balloon).
    pub before_use: Option<Section>,
    /// Files to hand over (ArduPilot .param, harness, notices).
    pub files: Vec<(String, PathBuf)>,
    /// Anything else, placed before "Checks".
    pub extra_sections: Vec<Section>,
    /// Printed pieces laid out for the printer bed rather than in the
    /// vehicle's frame (cut wing panels): listed and thumbnailed under
    /// "What you'll print", never drawn into the assembly pictures.
    pub print_parts: Vec<Part>,
    /// Parts whose printed/bought kind the build did not state and no
    /// G-code settles: listed as "not sliced" with that said, never as
    /// "bought" (review 4, finding 13).
    pub kind_unknown: Vec<String>,
}

impl<'a> Guide<'a> {
    pub fn new(title: impl Into<String>, parts: &'a [Part], bom: &'a Bom, checks: &'a Finished) -> Self {
        Self {
            title: title.into(),
            summary: String::new(),
            parts,
            bom,
            checks,
            plates: Vec::new(),
            steps: None,
            wiring: None,
            before_use: None,
            files: Vec::new(),
            extra_sections: Vec::new(),
            print_parts: Vec::new(),
            kind_unknown: Vec::new(),
        }
    }
}

// --- default assembly order -------------------------------------------------

/// Assembly groups in build order, matched on words in a part's name. The
/// battery and the propellers go last on purpose: the common practice is to
/// power up and check direction and surfaces with props off, and to connect
/// the pack only at the end.
const GROUPS: &[(&str, &[&str])] = &[
    ("Main structure", &["fuselage", "body", "pod", "frame", "airframe", "hull", "chassis", "gondola", "coupler", "bulkhead", "centering", "tube", "box", "spine", "keel"]),
    ("Wings, booms and tail", &["wing", "boom", "tail", "fin", "stab", "spar", "canard", "rudder", "elevator", "aileron", "elevon", "flap", "nose", "cone", "strut", "arm", "leg", "rail", "lug"]),
    ("Engine and motors", &["motor", "esc", "engine", "nozzle", "chamber", "injector", "grain", "casing", "mount", "igniter", "tank", "valve"]),
    ("Servos and linkages", &["servo", "linkage", "pushrod", "horn", "clevis"]),
    ("Electronics", &["fc", "flight", "controller", "autopilot", "receiver", "rx", "gps", "gnss", "telemetry", "vtx", "camera", "tracker", "radio", "altimeter", "avionics", "computer", "sensor", "aprs", "lora", "elrs", "antenna", "pdb", "bec", "logger", "board"]),
    ("Recovery", &["parachute", "chute", "recovery", "drogue", "main", "cord", "harness", "deployment", "ejection", "swivel", "line"]),
    ("Envelope and lift gas", &["balloon", "envelope", "neck", "filler"]),
    ("Battery", &["battery", "pack", "lipo", "liion", "cell", "cells"]),
    ("Propellers (last, after the checks)", &["prop", "propeller", "props"]),
];
const OTHER: &str = "Remaining parts";

fn group_of(name: &str) -> usize {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    // the last group whose word appears wins ties toward later groups only
    // when no earlier one matched: scan in order, first match
    for (gi, (_, keys)) in GROUPS.iter().enumerate() {
        let hit = words.iter().any(|w| {
            keys.iter().any(|k| if k.len() <= 3 { w == k } else { w.starts_with(k) || (w.len() > 4 && w.trim_end_matches('s') == *k) })
        });
        if hit {
            return gi;
        }
    }
    GROUPS.len() // OTHER, placed before the battery and props
}

/// A default order from part names and kinds: structure, flying surfaces,
/// motors, servos, electronics, recovery, battery, propellers. Within a group
/// printed parts come before bought ones. Domains that know better pass
/// explicit steps.
pub fn default_steps(parts: &[Part]) -> Vec<Step> {
    // OTHER goes before "Battery" so the battery stays near the end
    let battery = GROUPS.iter().position(|g| g.0 == "Battery").unwrap_or(GROUPS.len());
    let rank = |g: usize| -> usize {
        if g == GROUPS.len() {
            2 * battery - 1
        } else if g >= battery {
            2 * g + 1
        } else {
            2 * g
        }
    };
    let mut buckets: Vec<(usize, usize, Vec<&Part>)> = Vec::new();
    for p in parts {
        let g = group_of(&p.name);
        match buckets.iter_mut().find(|b| b.0 == g) {
            Some(b) => b.2.push(p),
            None => buckets.push((g, rank(g), vec![p])),
        }
    }
    buckets.sort_by_key(|b| b.1);
    let mut out = Vec::new();
    for (g, _, mut ps) in buckets {
        ps.sort_by_key(|p| (p.kind != PartKind::Printed, p.name.clone()));
        let title = if g == GROUPS.len() { OTHER.to_string() } else { GROUPS[g].0.to_string() };
        // big groups split so each picture says something
        for chunk in ps.chunks(6) {
            out.push(Step { title: title.clone(), parts: chunk.iter().map(|p| p.name.clone()).collect(), text: String::new() });
        }
    }
    out
}

/// Steps with every part covered once: names no part has are reported, and
/// parts no step adds get a final step.
fn complete_steps(parts: &[Part], steps: Vec<Step>) -> (Vec<Step>, Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    let mut unknown = Vec::new();
    let mut out = Vec::new();
    for mut s in steps {
        s.parts.retain(|n| {
            if !parts.iter().any(|p| &p.name == n) {
                unknown.push(n.clone());
                false
            } else {
                seen.insert(n.clone())
            }
        });
        out.push(s);
    }
    let left: Vec<String> = parts.iter().filter(|p| !seen.contains(&p.name)).map(|p| p.name.clone()).collect();
    if !left.is_empty() {
        out.push(Step {
            title: "Parts no step mentioned".into(),
            parts: left,
            text: "The design lists these parts but no assembly step adds them. Fit them where the picture shows.".into(),
        });
    }
    (out, unknown)
}

// --- formatting -------------------------------------------------------------

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn slug(s: &str) -> String {
    let mut o: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while o.contains("--") {
        o = o.replace("--", "-");
    }
    let o = o.trim_matches('-').to_string();
    if o.is_empty() { "part".into() } else { o }
}

fn num(v: f64) -> String {
    let a = v.abs();
    if a >= 1000.0 {
        format!("{v:.0}")
    } else if a >= 100.0 {
        format!("{v:.1}")
    } else if a >= 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    }
}

fn usd(v: f64) -> String {
    format!("${v:.2}")
}

fn duration(secs: f64) -> String {
    let m = (secs / 60.0).round() as u64;
    if m >= 60 {
        format!("{} h {:02} min", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}

fn source_name(s: Source) -> &'static str {
    match s {
        Source::Physics => "physics model",
        Source::Geometry => "measured on the geometry",
        Source::Rating => "part rating",
        Source::Requirement => "your requirement",
        Source::Slicer => "slicer output",
        Source::Simulation => "simulator",
    }
}

/// "https://shop.example.com/" or no path at all: a homepage, not a product.
fn is_homepage(url: &str) -> bool {
    let rest = url.split_once("://").map(|x| x.1).unwrap_or(url);
    match rest.split_once('/') {
        None => true,
        Some((_, path)) => path.trim_matches('/').is_empty(),
    }
}

fn link(url: &str, text: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        format!("<a href=\"{}\">{}</a>", esc(url), esc(text))
    } else {
        esc(text)
    }
}

/// The print recipe as PrusaSlicer recorded it in the G-code's config
/// block (`; key = value` lines at the end of the file).
fn recipe_from_gcode(path: &Path) -> Option<String> {
    let text = std::fs::read(path).ok()?;
    let tail = &text[text.len().saturating_sub(512 * 1024)..];
    let tail = String::from_utf8_lossy(tail);
    let get = |k: &str| {
        let pre = format!("; {k} = ");
        tail.lines().rev().find_map(|l| l.strip_prefix(pre.as_str()).map(|v| v.trim().to_string()))
    };
    let layer = get("layer_height");
    let layer = layer.map(|l| format!(", {l} mm layers")).unwrap_or_default();
    if get("spiral_vase").as_deref() == Some("1") {
        return Some(format!("vase mode (one wall){layer}"));
    }
    let walls = get("perimeters")?;
    let infill = get("fill_density").unwrap_or_else(|| "?".into());
    Some(format!("{walls} walls, {infill} infill{layer}"))
}

/// Which bed the part fits as it sits (wright-geom `printer_that_fits`).
fn fits_note(mesh: &wright_geom::Mesh) -> String {
    let s = mesh.bounds().size();
    match wright_geom::slice::printer_that_fits(mesh) {
        Some(p) => format!("{:.0}&times;{:.0}&times;{:.0} mm; smallest bed that fits: {}", s.x, s.y, s.z, esc(p.name)),
        None => format!("<span class=\"tag bad\">fits no listed printer</span> {:.0}&times;{:.0}&times;{:.0} mm: split it", s.x, s.y, s.z),
    }
}

// --- writer -----------------------------------------------------------------

struct Out<'p> {
    dir: &'p Path,
}

impl Out<'_> {
    fn img(&self, name: &str, png: &[u8]) -> anyhow::Result<String> {
        let rel = format!("img/{name}.png");
        std::fs::write(self.dir.join(&rel), png).with_context(|| format!("writing {rel}"))?;
        Ok(rel)
    }
    /// Copy `src` into `sub/` unless it is already inside the guide folder;
    /// return the path relative to the guide.
    fn file(&self, sub: &str, src: &Path) -> anyhow::Result<String> {
        if let (Ok(abs), Ok(dir)) = (src.canonicalize(), self.dir.canonicalize()) {
            if let Ok(rel) = abs.strip_prefix(&dir) {
                return Ok(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        let name = src.file_name().context("file has no name")?.to_string_lossy().to_string();
        let rel = format!("{sub}/{name}");
        std::fs::create_dir_all(self.dir.join(sub))?;
        std::fs::copy(src, self.dir.join(&rel)).with_context(|| format!("copying {}", src.display()))?;
        Ok(rel)
    }
}

/// Write `guide.html` (plus `img/`, `prints/`, `files/`, `bom.csv`) into
/// `out_dir` and return the HTML's path.
pub fn write_guide(g: &Guide, out_dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(out_dir.join("img"))?;
    std::fs::create_dir_all(out_dir.join("prints"))?;
    let out = Out { dir: out_dir };
    let opts = |view: View, w: u32, h: u32| Options::new(view, w, h).fit_parts(g.parts);

    let total_checks = g.checks.outcomes.len();
    let failing = g.checks.failures().count();
    let unpriced = g.bom.unpriced().len();
    let print_secs: f64 = g.plates.iter().map(|p| p.1.seconds).sum();
    let print_grams: f64 = g.plates.iter().map(|p| p.1.grams).sum();

    let mut h = String::new();
    let _ = write!(
        h,
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{t} - build guide</title><style>{CSS}</style></head><body><main>",
        t = esc(&g.title)
    );

    // ---- cover
    let cover = out.img("cover-iso", &rasterize(g.parts, &opts(View::Iso, 1200, 760)).png())?;
    let _ = write!(h, "<header class=\"cover\"><p class=\"kicker\">Build guide</p><h1>{}</h1>", esc(&g.title));
    if !g.summary.is_empty() {
        let _ = write!(h, "<p class=\"lede\">{}</p>", esc(&g.summary));
    }
    if failing > 0 {
        let _ = write!(
            h,
            "<p class=\"banner bad\"><b>Not ready to build.</b> {failing} of {total_checks} checks fail. \
             See <a href=\"#checks\">Checks</a> before you buy anything.</p>"
        );
    } else if total_checks == 0 {
        h.push_str("<p class=\"banner bad\"><b>No checks were run.</b> Nothing here has been shown to work.</p>");
    } else {
        let _ = write!(h, "<p class=\"banner ok\"><b>All {total_checks} checks pass.</b> The numbers are in <a href=\"#checks\">Checks</a>.</p>");
    }
    let _ = write!(h, "<img class=\"hero\" src=\"{cover}\" alt=\"The assembled vehicle\">");
    h.push_str("<dl class=\"facts\">");
    let cost = if unpriced > 0 {
        format!("{} <small>+ {unpriced} unpriced</small>", usd(g.bom.total().as_usd()))
    } else {
        usd(g.bom.total().as_usd())
    };
    let facts = [
        ("Parts to buy", format!("{} lines, {cost}", g.bom.lines.len())),
        ("Print", {
            let n_printed = g.parts.iter().chain(&g.print_parts).filter(|p| p.kind == PartKind::Printed).count();
            let sliced = g.plates.len();
            if sliced == 0 {
                format!("{n_printed} printed parts, none sliced")
            } else if sliced >= n_printed {
                // every printed part sliced (plates can also be printed tools
                // such as jigs, which are not parts of the vehicle)
                let plates = if sliced == 1 { "1 plate".to_string() } else { format!("{sliced} plates") };
                format!("{plates}: {}, {:.0} g", duration(print_secs), print_grams)
            } else {
                format!("{sliced} of {n_printed} printed parts sliced: {}, {:.0} g", duration(print_secs), print_grams)
            }
        }),
        ("Checks", format!("{} pass, {failing} fail", total_checks - failing)),
        ("Parts in the model", g.parts.len().to_string()),
    ];
    for (k, v) in facts {
        let _ = write!(h, "<div><dt>{k}</dt><dd>{v}</dd></div>");
    }
    h.push_str("</dl><div class=\"views\">");
    for view in [View::Top, View::Front, View::Side] {
        let p = out.img(&format!("view-{}", view.name()), &rasterize(g.parts, &opts(view, 600, 400)).png())?;
        let _ = write!(h, "<figure><img src=\"{p}\" alt=\"{n} view\"><figcaption>{n}</figcaption></figure>", n = view.name());
    }
    h.push_str("</div><p class=\"legend\"><span class=\"chip printed\"></span>printed <span class=\"chip bought\"></span>bought <span class=\"chip new\"></span>added in this step</p>");
    h.push_str("<nav><a href=\"#buy\">Buy</a><a href=\"#print\">Print</a><a href=\"#assembly\">Assembly</a><a href=\"#wiring\">Wiring</a>");
    for s in &g.extra_sections {
        let _ = write!(h, "<a href=\"#{}\">{}</a>", slug(&s.title), esc(&s.title));
    }
    h.push_str("<a href=\"#checks\">Checks</a>");
    if let Some(b) = &g.before_use {
        let _ = write!(h, "<a href=\"#before\">{}</a>", esc(&b.title));
    }
    h.push_str("</nav></header>");

    // ---- buy
    std::fs::write(out_dir.join("bom.csv"), g.bom.csv())?;
    h.push_str("<section id=\"buy\"><h2>What you'll buy</h2>");
    if g.bom.lines.is_empty() {
        h.push_str("<p class=\"warn\">The design has no parts list.</p>");
    } else {
        h.push_str("<table class=\"bom\"><thead><tr><th>Part</th><th class=\"n\">Qty</th><th class=\"n\">Each</th><th class=\"n\">Total</th><th>Note</th></tr></thead><tbody>");
        let mut cats: Vec<&str> = Vec::new();
        for l in &g.bom.lines {
            if !cats.contains(&l.category.as_str()) {
                cats.push(&l.category);
            }
        }
        for cat in cats {
            let _ = write!(h, "<tr class=\"cat\"><th colspan=\"5\">{}</th></tr>", esc(cat));
            for l in g.bom.lines.iter().filter(|l| l.category == cat) {
                let mut flags = String::new();
                if l.url.is_empty() {
                    flags.push_str("<span class=\"tag bad\">no link</span> ");
                } else if is_homepage(&l.url) {
                    flags.push_str("<span class=\"tag bad\">link is a shop homepage</span> ");
                }
                let q = if l.qty.fract() == 0.0 { format!("{:.0}", l.qty) } else { num(l.qty) };
                let qty = if l.unit.is_empty() || l.unit == "ea" { q } else { format!("{q} {}", esc(&l.unit)) };
                let (each, total) = match (l.unit_price, l.total()) {
                    (Some(p), Some(t)) => (usd(p.as_usd()), usd(t.as_usd())),
                    _ => ("<span class=\"tag bad\">unpriced</span>".to_string(), "&ndash;".to_string()),
                };
                let _ = write!(
                    h,
                    "<tr><td>{}<div class=\"id\">{}</div></td><td class=\"n\">{qty}</td><td class=\"n\">{each}</td><td class=\"n\">{total}</td><td>{flags}{}</td></tr>",
                    link(&l.url, &l.name),
                    esc(&l.id),
                    esc(&l.note)
                );
            }
        }
        let _ = write!(h, "</tbody><tfoot><tr><th>Total</th><td></td><td></td><td class=\"n\"><b>{}</b></td><td>", usd(g.bom.total().as_usd()));
        if unpriced > 0 {
            let _ = write!(h, "<span class=\"tag bad\">plus {unpriced} unpriced line{}, not counted</span>", if unpriced == 1 { "" } else { "s" });
        }
        h.push_str("</td></tr></tfoot></table>");
        h.push_str("<p class=\"small\">Prices are as listed on each linked page when the catalog was read; check them before you order. <a href=\"bom.csv\">bom.csv</a></p>");
    }
    h.push_str("</section>");

    // ---- print
    h.push_str("<section id=\"print\"><h2>What you'll print</h2>");
    let printed: Vec<&Part> = g.parts.iter().chain(&g.print_parts).filter(|p| p.kind == PartKind::Printed).collect();
    if printed.is_empty() && g.plates.is_empty() {
        h.push_str("<p>Nothing in this design is printed.</p>");
    } else {
        h.push_str("<table class=\"prints\"><thead><tr><th></th><th>Part</th><th>Printer</th><th>Material</th><th>Settings</th><th class=\"n\">Time</th><th class=\"n\">Filament</th><th>Files</th></tr></thead><tbody>");
        for (name, s) in &g.plates {
            let part = g.parts.iter().chain(&g.print_parts).find(|p| &p.name == name);
            let thumb = match part {
                Some(p) => {
                    let rel = out.img(&format!("print-{}", slug(name)), &rasterize(std::slice::from_ref(p), &Options::new(View::Iso, 240, 180)).png())?;
                    format!("<img class=\"thumb\" src=\"{rel}\" alt=\"\">")
                }
                None => String::new(),
            };
            let gcode = out.file("prints", &s.gcode)?;
            let mut files = format!("<a href=\"{}\">G-code</a>", esc(&gcode));
            if let Some(p) = part {
                let rel = format!("prints/{}.stl", slug(name));
                p.mesh.write_stl(&out_dir.join(&rel))?;
                let _ = write!(files, " &middot; <a href=\"{rel}\">STL</a>");
            }
            let mut printer = if s.generic_gcode {
                format!("{}<div class=\"small\">generic G-code (time and grams are right; send through your printer's own slicer)</div>", esc(&s.printer))
            } else {
                esc(&s.printer)
            };
            if let Some(p) = part {
                let _ = write!(printer, "<div class=\"small\">{}</div>", fits_note(&p.mesh));
            }
            let recipe = recipe_from_gcode(&s.gcode).unwrap_or_else(|| "see the G-code".into());
            let _ = write!(
                h,
                "<tr><td>{thumb}</td><td><b>{}</b></td><td>{printer}</td><td>{}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{:.0} g</td><td>{files}</td></tr>",
                esc(name),
                esc(part.map(|p| p.material.as_str()).unwrap_or("?")),
                esc(&recipe),
                duration(s.seconds),
                s.grams
            );
        }
        for p in printed.iter().filter(|p| !g.plates.iter().any(|(n, _)| n == &p.name)) {
            let rel = out.img(&format!("print-{}", slug(&p.name)), &rasterize(std::slice::from_ref(*p), &Options::new(View::Iso, 240, 180)).png())?;
            let fit = fits_note(&p.mesh);
            let stl = format!("prints/{}.stl", slug(&p.name));
            p.mesh.write_stl(&out_dir.join(&stl))?;
            let _ = write!(
                h,
                "<tr class=\"unsliced\"><td><img class=\"thumb\" src=\"{rel}\" alt=\"\"></td><td><b>{}</b></td><td><div class=\"small\">{fit}</div></td><td>{}</td><td colspan=\"3\"><span class=\"tag bad\">not sliced</span> {}</td><td><a href=\"{stl}\">STL</a></td></tr>",
                esc(&p.name),
                esc(&p.material),
                if g.kind_unknown.contains(&p.name) {
                    "the build did not say whether this part is printed or bought; if you print it, slice the STL yourself"
                } else {
                    "no print time or grams yet; slice the STL yourself"
                }
            );
        }
        let _ = write!(
            h,
            "</tbody><tfoot><tr><th></th><th>Total</th><td></td><td></td><td></td><td class=\"n\"><b>{}</b></td><td class=\"n\"><b>{:.0} g</b></td><td></td></tr></tfoot></table>",
            duration(print_secs),
            print_grams
        );
    }
    h.push_str("</section>");

    // ---- assembly
    let (steps, unknown) = complete_steps(g.parts, g.steps.clone().unwrap_or_else(|| default_steps(g.parts)));
    h.push_str("<section id=\"assembly\"><h2>Assembly</h2>");
    if !unknown.is_empty() {
        let _ = write!(h, "<p class=\"warn\">The steps name parts the design does not have: {}.</p>", esc(&unknown.join(", ")));
    }
    let mut so_far: Vec<Part> = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        for n in &s.parts {
            if let Some(p) = g.parts.iter().find(|p| &p.name == n) {
                so_far.push(p.clone());
            }
        }
        // framed on the parts so far: the first steps' small parts stay legible
        let png = rasterize(&so_far, &Options::new(View::Iso, 900, 560).highlight(s.parts.iter().cloned())).png();
        let rel = out.img(&format!("step-{:02}", i + 1), &png)?;
        let _ = write!(h, "<article class=\"step\"><h3><span class=\"num\">{}</span> {}</h3><div class=\"stepbody\"><img src=\"{rel}\" alt=\"Step {} picture\"><div>", i + 1, esc(&s.title), i + 1);
        if !s.text.is_empty() {
            let _ = write!(h, "<p>{}</p>", esc(&s.text));
        }
        h.push_str("<p class=\"need\">You need:</p><ul class=\"uses\">");
        for n in &s.parts {
            let Some(p) = g.parts.iter().find(|p| &p.name == n) else { continue };
            let what = match p.kind {
                PartKind::Printed if g.kind_unknown.contains(&p.name) => "printed or bought: the build did not say; not sliced".to_string(),
                PartKind::Printed => format!("printed, {}", esc(&p.material)),
                PartKind::Bought => {
                    let line = g.bom.lines.iter().find(|l| l.id == p.material);
                    match line {
                        Some(l) => format!("bought: {}", link(&l.url, &l.name)),
                        None => format!("bought: {}", esc(&p.material)),
                    }
                }
            };
            let joins = p.touches.iter().filter(|t| g.parts.iter().any(|q| &q.name == *t)).cloned().collect::<Vec<_>>();
            let joins = if joins.is_empty() { String::new() } else { format!(" &middot; fits against {}", esc(&joins.join(", "))) };
            let _ = write!(h, "<li><b>{}</b> <span class=\"small\">({what}){joins}</span></li>", esc(&p.name));
        }
        h.push_str("</ul></div></div></article>");
    }
    h.push_str("</section>");

    // ---- wiring
    h.push_str("<section id=\"wiring\"><h2>Wiring</h2>");
    match &g.wiring {
        Some(p) => {
            let rel = out.file("files", p)?;
            let _ = write!(h, "<a href=\"{r}\"><img class=\"wiring\" src=\"{r}\" alt=\"Wiring diagram\"></a>", r = esc(&rel));
        }
        None => h.push_str("<p class=\"warn\">No wiring diagram was produced for this design.</p>"),
    }
    if !g.files.is_empty() {
        h.push_str("<h3>Files to load</h3><ul>");
        for (label, p) in &g.files {
            let rel = out.file("files", p)?;
            let _ = write!(h, "<li><a href=\"{}\">{}</a></li>", esc(&rel), esc(label));
        }
        h.push_str("</ul>");
    }
    h.push_str("</section>");

    // ---- extra
    for s in &g.extra_sections {
        section(&mut h, &out, s, &slug(&s.title))?;
    }

    // ---- checks, failures first
    let _ = write!(h, "<section id=\"checks\"><h2>Checks</h2><p>{total_checks} checks, <b>{failing} failing</b>. A check with no evidence counts as a failure.</p>");
    h.push_str("<table class=\"checks\"><thead><tr><th></th><th>Check</th><th class=\"n\">Margin</th><th>What it means</th><th>Source</th></tr></thead><tbody>");
    let mut order: Vec<&wright_core::Outcome> = g.checks.outcomes.iter().collect();
    order.sort_by_key(|o| o.passed());
    for o in order {
        let (mark, cls) = if o.passed() { ("pass", "ok") } else { ("FAIL", "bad") };
        let (margin, detail) = match &o.evidence {
            Evidence::Margin { value, unit } => (format!("{}{} {}", if *value >= 0.0 { "+" } else { "" }, num(*value), esc(unit)), esc(&o.detail)),
            Evidence::Missing { why } => ("no evidence".into(), format!("Not measured: {}", esc(why))),
        };
        let _ = write!(
            h,
            "<tr class=\"{cls}\"><td><span class=\"tag {cls}\">{mark}</span></td><td><code>{}</code></td><td class=\"n\">{margin}</td><td>{detail}</td><td class=\"small\">{}</td></tr>",
            esc(&o.name),
            source_name(o.source)
        );
    }
    h.push_str("</tbody></table></section>");

    // ---- before first use
    match &g.before_use {
        Some(s) => section(&mut h, &out, s, "before")?,
        None => h.push_str("<section id=\"before\"><h2>Before the first use</h2><p class=\"warn\">This design came without a pre-flight checklist. Write one before you fly, launch or release it.</p></section>"),
    }

    h.push_str("<footer>Made by Wright. Renders and numbers come from the design files beside this page.</footer></main></body></html>\n");
    let path = out_dir.join("guide.html");
    std::fs::write(&path, h)?;
    Ok(path)
}

fn section(h: &mut String, out: &Out, s: &Section, id: &str) -> anyhow::Result<()> {
    let _ = write!(h, "<section id=\"{}\"><h2>{}</h2>", esc(id), esc(&s.title));
    for b in &s.blocks {
        match b {
            Block::Para(t) => {
                let _ = write!(h, "<p>{}</p>", esc(t));
            }
            Block::Checklist(items) => {
                h.push_str("<ul class=\"checklist\">");
                for it in items {
                    let _ = write!(h, "<li><span class=\"box\"></span>{}</li>", esc(it));
                }
                h.push_str("</ul>");
            }
            Block::Numbered(items) => {
                h.push_str("<ol>");
                for it in items {
                    let _ = write!(h, "<li>{}</li>", esc(it));
                }
                h.push_str("</ol>");
            }
            Block::Table { head, rows } => {
                h.push_str("<table><thead><tr>");
                for c in head {
                    let _ = write!(h, "<th>{}</th>", esc(c));
                }
                h.push_str("</tr></thead><tbody>");
                for r in rows {
                    h.push_str("<tr>");
                    for c in r {
                        let _ = write!(h, "<td>{}</td>", esc(c));
                    }
                    h.push_str("</tr>");
                }
                h.push_str("</tbody></table>");
            }
            Block::File { label, path } => {
                let rel = out.file("files", path)?;
                let _ = write!(h, "<p><a class=\"file\" href=\"{}\">{}</a></p>", esc(&rel), esc(label));
            }
            Block::Html(raw) => h.push_str(raw),
        }
    }
    h.push_str("</section>");
    Ok(())
}

const CSS: &str = r#"
:root{--ink:#1d2129;--muted:#5d6470;--line:#dfe3e8;--bg:#fff;--soft:#f5f7f9;--ok:#1d7a46;--okbg:#e6f4ec;--bad:#b42318;--badbg:#fdecea;--accent:#f08020;--printed:#78a6d6;--bought:#606670}
*{box-sizing:border-box}
html{background:var(--bg)}
body{margin:0;color:var(--ink);background:var(--bg);font:15px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif}
main{max-width:1000px;margin:0 auto;padding:32px 16px 64px}
h1{font-size:34px;line-height:1.15;margin:4px 0 8px}
h2{font-size:24px;margin:48px 0 12px;padding-bottom:6px;border-bottom:2px solid var(--ink)}
h3{font-size:18px;margin:24px 0 8px}
a{color:#0b5cad}
.kicker{text-transform:uppercase;letter-spacing:.08em;font-size:12px;color:var(--muted);margin:0}
.lede{font-size:17px;color:var(--muted);margin:0 0 12px}
.banner{padding:10px 14px;border-radius:6px;margin:12px 0}
.banner.ok{background:var(--okbg);color:var(--ok)}
.banner.bad{background:var(--badbg);color:var(--bad)}
.hero{width:100%;height:auto;display:block;border:1px solid var(--line);border-radius:8px}
.facts{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:8px;margin:16px 0}
.facts div{background:var(--soft);border-radius:6px;padding:8px 12px}
.facts dt{font-size:12px;color:var(--muted);text-transform:uppercase;letter-spacing:.05em}
.facts dd{margin:0;font-weight:600}
.views{display:grid;grid-template-columns:repeat(3,1fr);gap:8px}
.views figure{margin:0}
.views img{width:100%;height:auto;border:1px solid var(--line);border-radius:6px;display:block}
figcaption{font-size:12px;color:var(--muted);text-align:center}
.legend{font-size:13px;color:var(--muted)}
.chip{display:inline-block;width:12px;height:12px;border-radius:3px;vertical-align:-1px;margin:0 4px 0 10px}
.chip.printed{background:var(--printed)}.chip.bought{background:var(--bought)}.chip.new{background:var(--accent)}
nav{display:flex;flex-wrap:wrap;gap:6px;margin:16px 0 0}
nav a{padding:4px 10px;border:1px solid var(--line);border-radius:999px;text-decoration:none;color:var(--ink);font-size:14px}
table{width:100%;border-collapse:collapse;font-size:14px}
th,td{text-align:left;padding:6px 8px;border-bottom:1px solid var(--line);vertical-align:top}
thead th{font-size:12px;color:var(--muted);text-transform:uppercase;letter-spacing:.04em;border-bottom:1px solid var(--ink)}
tfoot th,tfoot td{border-top:1px solid var(--ink);border-bottom:none}
tr.cat th{background:var(--soft);font-size:13px}
.n{text-align:right;white-space:nowrap;font-variant-numeric:tabular-nums}
.id{font-size:12px;color:var(--muted)}
.small{font-size:12px;color:var(--muted)}
.tag{display:inline-block;font-size:11px;font-weight:700;padding:1px 6px;border-radius:4px;text-transform:uppercase;letter-spacing:.03em;white-space:nowrap}
.tag.ok{background:var(--okbg);color:var(--ok)}.tag.bad{background:var(--badbg);color:var(--bad)}
.warn{background:var(--badbg);color:var(--bad);padding:8px 12px;border-radius:6px}
.thumb{width:120px;height:auto;display:block}
.step{border:1px solid var(--line);border-radius:8px;padding:12px 16px;margin:12px 0;break-inside:avoid}
.step h3{margin:0 0 8px;display:flex;align-items:center;gap:10px}
.num{display:inline-flex;align-items:center;justify-content:center;width:28px;height:28px;border-radius:50%;background:var(--ink);color:#fff;font-size:14px;flex:none}
.stepbody{display:grid;grid-template-columns:3fr 2fr;gap:16px;align-items:start}
.stepbody img{width:100%;height:auto;display:block}
.stepbody p{margin:0 0 8px}
.need{font-weight:600}
.uses{margin:0;padding-left:18px}
.wiring{max-width:100%;height:auto;border:1px solid var(--line);border-radius:6px}
.checks tr.bad td{background:#fff8f7}
code{font:13px ui-monospace,SFMono-Regular,Menlo,monospace}
.checklist{list-style:none;padding:0}
.checklist li{display:flex;gap:10px;align-items:flex-start;padding:6px 0;border-bottom:1px solid var(--line)}
.box{flex:none;width:16px;height:16px;border:2px solid var(--ink);border-radius:3px;margin-top:3px}
footer{margin-top:48px;font-size:12px;color:var(--muted)}
@media (max-width:700px){.views{grid-template-columns:1fr}.stepbody{grid-template-columns:1fr}.prints th:nth-child(3),.prints td:nth-child(3){display:none}}
@media print{main{max-width:none;padding:0}nav{display:none}a{color:inherit;text-decoration:none}
 h2{break-before:page}#buy h2{break-before:auto}tr,.step,figure{break-inside:avoid}
 .banner,.tag,.warn,.chip,.num,tr.cat th,.facts div{-webkit-print-color-adjust:exact;print-color-adjust:exact}
 @page{margin:14mm}}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use wright_core::check::{CheckId, Report};
    use wright_core::units::Money;
    use wright_core::BomLine;
    use wright_geom::{shapes::cuboid, v3};

    fn part(name: &str, kind: PartKind) -> Part {
        Part { name: name.into(), kind, material: "pla".into(), mesh: cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0)), touches: vec![] }
    }

    #[test]
    fn default_order_puts_structure_first_and_battery_and_props_last() {
        let parts = [part("prop_1", PartKind::Bought), part("battery", PartKind::Bought), part("left_wing", PartKind::Printed), part("fuselage", PartKind::Printed), part("gizmo", PartKind::Bought), part("receiver", PartKind::Bought)];
        let steps = default_steps(&parts);
        let order: Vec<&str> = steps.iter().map(|s| s.parts[0].as_str()).collect();
        assert_eq!(order, ["fuselage", "left_wing", "receiver", "gizmo", "battery", "prop_1"]);
    }

    #[test]
    fn every_part_lands_in_a_step_and_unknown_names_are_reported() {
        let parts = [part("a", PartKind::Printed), part("b", PartKind::Bought)];
        let (steps, unknown) = complete_steps(&parts, vec![Step { title: "one".into(), parts: vec!["a".into(), "ghost".into()], text: String::new() }]);
        assert_eq!(unknown, ["ghost"]);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].parts, ["b"]);
    }

    #[test]
    fn homepages_are_spotted() {
        assert!(is_homepage("https://www.getfpv.com/"));
        assert!(is_homepage("https://www.getfpv.com"));
        assert!(!is_homepage("https://www.getfpv.com/t-motor-f60.html"));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum C {
        Fits,
        Hover,
    }
    impl CheckId for C {
        const ALL: &'static [Self] = &[C::Fits, C::Hover];
        fn name(self) -> &'static str {
            match self {
                C::Fits => "fits",
                C::Hover => "hover_current",
            }
        }
        fn describe(self) -> &'static str {
            "test"
        }
    }

    #[test]
    fn a_guide_shows_failures_first_and_unpriced_lines() {
        let parts = vec![part("fuselage", PartKind::Printed), part("battery", PartKind::Bought)];
        let bom = Bom {
            lines: vec![
                BomLine { category: "power".into(), id: "pla".into(), name: "Pack".into(), qty: 1.0, unit: "ea".into(), unit_price: None, url: "https://shop.example/".into(), note: String::new() },
                BomLine { category: "motors".into(), id: "m".into(), name: "Motor <x>".into(), qty: 4.0, unit: "ea".into(), unit_price: Some(Money::usd(20.0)), url: "https://shop.example/m".into(), note: String::new() },
            ],
        };
        let mut r = Report::<C>::new();
        r.margin(C::Fits, 2.0, "mm", Source::Geometry, "clear");
        let checks = r.finish();
        let mut g = Guide::new("Test & build", &parts, &bom, &checks);
        g.before_use = Some(Section::checklist("Before the first flight", ["Props off"]));
        let dir = std::env::temp_dir().join(format!("wright-guide-test-{}", std::process::id()));
        let path = write_guide(&g, &dir).unwrap();
        let html = std::fs::read_to_string(&path).unwrap();
        assert!(html.contains("Test &amp; build") && html.contains("Motor &lt;x&gt;"));
        assert!(html.contains("unpriced") && html.contains("link is a shop homepage"));
        assert!(html.contains("$80.00"));
        assert!(html.contains("Not ready to build"));
        // the failing (never evaluated) check is listed before the passing one
        let (f, p) = (html.find("<code>hover_current</code>").unwrap(), html.find("<code>fits</code>").unwrap());
        assert!(f < p);
        assert!(html.contains("not sliced"));
        assert!(html.contains("Before the first flight") && html.contains("Props off"));
        assert!(dir.join("img/step-01.png").exists() && dir.join("bom.csv").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
