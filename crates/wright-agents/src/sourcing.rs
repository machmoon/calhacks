//! Sourcing tools every domain gets for free: ranked catalog search,
//! alternates with an attribute-delta table, a BOM diff between the
//! incumbent and the candidate, and a strict id gate. Everything here is
//! deterministic and offline; it works on the `CatalogOption` rows a
//! domain's read-only `options` already returns and on `wright_core::Bom`,
//! so no domain crate changes. CREAO stays optional: a BOM its desk priced
//! (`wright_creao::desk::Sourcing::bom`) can be either side of [`diff`].
//!
//! Precedents (read in source, 2026-10-24):
//! * Filters and ranking: InvenTree `src/backend/InvenTree/part/api.py`,
//!   `PartFilter` -- boolean `has_stock` / `has_pricing` filters beside
//!   parametric ones, and `PartList.ordering_fields` (`in_stock`,
//!   `pricing_min`, ...) chosen by the caller. [`Query::stock`] mirrors
//!   wright-air's `StockPolicy` names so the agent uses one vocabulary.
//! * Which attributes make two rows "the same": KiBoM `kibom/component.py`,
//!   `Component.__eq__` / `compareField` (lower-cased string equality; a
//!   blank field matches only with `mergeBlankFields`, which we do not set)
//!   and KiCad `eeschema/python_scripts/bom_csv_grouped_by_value.py`
//!   `myEqu` (value, part, footprint, DNP). [`alternates`] lists every
//!   attribute that differs by that test.
//! * BOM diff: cargo `src/ops/cargo_update.rs`, `PackageDiff::diff`
//!   (group by key, `vec_subset` both ways, one-in/one-out is a change) and
//!   `PackageChangeKind` with its status words `Adding` / `Removing` /
//!   `Updating` and the `{previous} -> {new}` line. Parts have no version
//!   order, so Upgraded/Downgraded collapse into [`ChangeKind::Changed`]
//!   with the qty and price deltas spelled out.
//! * Id gate: rustc `compiler/rustc_span/src/edit_distance.rs`,
//!   `find_best_match_for_name` -- case-insensitive exact match first,
//!   then edit distance within `max(len, 3) / 3`, then the sorted-words
//!   fallback -- and `edit_distance_with_substrings` for the "nearest"
//!   list. Deviation: plain Levenshtein, not rustc's transposition-aware
//!   variant (catalog ids are long slugs; one transposition is one edit
//!   either way).

use crate::state::DesignState;
use crate::tools::DomainTools;
use serde::Deserialize;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use wright_core::{Bom, BomLine, CatalogOption};

// ------------------------------------------------------------------ stock

/// How long a builder waits for a row, read off the row. wright-air puts
/// its `Stock` label in `specs["stock"]`; rocket and balloon only set
/// `in_stock`. Ordered best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lead {
    InStock,
    /// Listed for sale with a price and no stock line.
    Unstated,
    /// Made to order, pre-order, backorder: weeks.
    Weeks,
    OutOfStock,
    Discontinued,
}

impl Lead {
    pub fn of(o: &CatalogOption) -> Lead {
        match o.specs.get("stock").map(|s| s.as_str()) {
            Some("in stock") => Lead::InStock,
            Some("stock unstated") => Lead::Unstated,
            Some("made to order") | Some("pre-order") | Some("backorder") => Lead::Weeks,
            Some("out of stock") => Lead::OutOfStock,
            Some("discontinued") => Lead::Discontinued,
            _ => match o.in_stock {
                Some(true) => Lead::InStock,
                None => Lead::Unstated,
                Some(false) => Lead::OutOfStock,
            },
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Lead::InStock => "in stock",
            Lead::Unstated => "stock unstated",
            Lead::Weeks => "weeks (made to order / pre-order / backorder)",
            Lead::OutOfStock => "OUT OF STOCK",
            Lead::Discontinued => "DISCONTINUED",
        }
    }
}

/// Which stock states a search may return; the names are wright-air's
/// `StockPolicy` (`spec.rs`) so one word means one thing everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockFilter {
    /// Only rows whose page said in stock.
    InStock,
    /// In stock, or priced with no stock line (the default).
    #[default]
    Listed,
    /// Also made to order, pre-order and backorder.
    LeadTime,
    /// Also out of stock. Discontinued rows are never returned.
    Restock,
}

impl StockFilter {
    pub fn accepts(self, lead: Lead) -> bool {
        match lead {
            Lead::InStock => true,
            Lead::Unstated => self != StockFilter::InStock,
            Lead::Weeks => matches!(self, StockFilter::LeadTime | StockFilter::Restock),
            Lead::OutOfStock => self == StockFilter::Restock,
            Lead::Discontinued => false,
        }
    }
    pub fn parse(s: &str) -> Option<StockFilter> {
        Some(match s.trim() {
            "" | "listed" => StockFilter::Listed,
            "in_stock" => StockFilter::InStock,
            "lead_time" => StockFilter::LeadTime,
            "restock" => StockFilter::Restock,
            _ => return None,
        })
    }
}

// ----------------------------------------------------------------- numbers

/// The first number in a spec string: "16" -> 16, "2-4S" -> 2,
/// "bore [5.0] mm" -> 5, "-3.5" -> -3.5. Rows write specs as text.
pub fn number(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let neg = b[i] == b'-' && i + 1 < b.len() && b[i + 1].is_ascii_digit();
        if neg || b[i].is_ascii_digit() {
            let start = i;
            i += usize::from(neg);
            let mut dot = false;
            while i < b.len() && (b[i].is_ascii_digit() || (b[i] == b'.' && !dot && i + 1 < b.len() && b[i + 1].is_ascii_digit())) {
                dot |= b[i] == b'.';
                i += 1;
            }
            if let Ok(x) = s[start..i].parse::<f64>() {
                return Some(x);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn money(x: f64) -> String {
    if x < 0.0 {
        format!("-${:.2}", -x)
    } else {
        format!("${x:.2}")
    }
}

fn signed(x: f64) -> String {
    if x < 0.0 {
        format!("-${:.2}", -x)
    } else {
        format!("+${x:.2}")
    }
}

fn num(x: f64) -> String {
    format!("{}", (x * 100.0).round() / 100.0)
}

// ------------------------------------------------------------------ search

/// `attr:min..max`; either bound may be missing (`mass_g:..60`,
/// `kv:800..`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Range {
    pub attr: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl Range {
    /// Parse `"kv:800..1200, mass_g:..60"`.
    pub fn parse_list(s: &str) -> Result<Vec<Range>, String> {
        let mut out = vec![];
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (attr, bounds) = part.split_once(':').ok_or_else(|| format!("range `{part}` is not `attr:min..max`"))?;
            let (lo, hi) = bounds.split_once("..").ok_or_else(|| format!("range `{part}` needs `..` between min and max (either may be empty)"))?;
            let bound = |t: &str| -> Result<Option<f64>, String> {
                let t = t.trim();
                if t.is_empty() {
                    Ok(None)
                } else {
                    t.parse::<f64>().map(Some).map_err(|_| format!("range `{part}`: `{t}` is not a number"))
                }
            };
            let (min, max) = (bound(lo)?, bound(hi)?);
            if let (Some(a), Some(b)) = (min, max) {
                if a > b {
                    return Err(format!("range `{part}`: min {a} is above max {b}"));
                }
            }
            out.push(Range { attr: attr.trim().to_string(), min, max });
        }
        Ok(out)
    }
    fn fits(&self, v: f64) -> bool {
        self.min.is_none_or(|m| v >= m) && self.max.is_none_or(|m| v <= m)
    }
    fn text(&self) -> String {
        match (self.min, self.max) {
            (Some(a), Some(b)) => format!("{a}..{b}"),
            (Some(a), None) => format!(">= {a}"),
            (None, Some(b)) => format!("<= {b}"),
            (None, None) => "any".into(),
        }
    }
}

/// Sort key the caller chooses, InvenTree's `ordering` (`-attr` for
/// descending). `price` sorts on the row price; anything else on a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
    pub attr: String,
    pub descending: bool,
}

impl Sort {
    pub fn parse(s: &str) -> Option<Sort> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let (descending, attr) = match s.strip_prefix('-') {
            Some(a) => (true, a),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        Some(Sort { attr: attr.to_string(), descending })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    /// Case-insensitive substring of id, label or any spec value.
    pub text: String,
    pub ranges: Vec<Range>,
    pub stock: StockFilter,
    pub sort: Option<Sort>,
    /// 0: no cap.
    pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub option: CatalogOption,
    pub lead: Lead,
    /// "why this row": one clause per filter it satisfied.
    pub why: Vec<String>,
}

/// How well `text` matches a row: exact id, id substring, label
/// substring, a spec value. Lower is better.
fn text_rank(o: &CatalogOption, text: &str) -> Option<u8> {
    if text.is_empty() {
        return Some(0);
    }
    let t = text.to_lowercase();
    let id = o.id.to_lowercase();
    if id == t {
        Some(0)
    } else if id.contains(&t) {
        Some(1)
    } else if o.label.to_lowercase().contains(&t) {
        Some(2)
    } else if o.specs.values().any(|v| v.to_lowercase().contains(&t)) {
        Some(3)
    } else {
        None
    }
}

fn sort_value(o: &CatalogOption, s: &Sort) -> Option<f64> {
    if s.attr == "price" || s.attr == "price_usd" {
        o.price_usd
    } else {
        o.specs.get(&s.attr).and_then(|v| number(v))
    }
}

/// Filter `rows` by `q` and rank them: stock class first (a part you can
/// buy today beats one on backorder), then how directly the text matched,
/// then price (unpriced last), then id. With `q.sort`, that attribute
/// leads (rows missing it last) and the rest breaks ties.
pub fn search(rows: &[CatalogOption], q: &Query) -> Vec<Hit> {
    let mut hits: Vec<(u8, Hit)> = rows
        .iter()
        .filter_map(|o| {
            let lead = Lead::of(o);
            if !q.stock.accepts(lead) {
                return None;
            }
            let rank = text_rank(o, &q.text)?;
            let mut why = vec![];
            if !q.text.is_empty() {
                why.push(match rank {
                    0 => format!("id is \"{}\"", q.text),
                    1 => format!("id contains \"{}\"", q.text),
                    2 => format!("label contains \"{}\"", q.text),
                    _ => format!("a spec mentions \"{}\"", q.text),
                });
            }
            for r in &q.ranges {
                let v = o.specs.get(&r.attr).and_then(|s| number(s))?;
                if !r.fits(v) {
                    return None;
                }
                why.push(format!("{} {} within {}", r.attr, num(v), r.text()));
            }
            why.push(lead.label().to_string());
            why.push(o.price_usd.map_or("unpriced".to_string(), money));
            Some((rank, Hit { option: o.clone(), lead, why }))
        })
        .collect();
    let price_key = |h: &Hit| h.option.price_usd.map(|p| (p * 100.0).round() as i64).unwrap_or(i64::MAX);
    hits.sort_by(|(ra, a), (rb, b)| {
        if let Some(s) = &q.sort {
            let (va, vb) = (sort_value(&a.option, s), sort_value(&b.option, s));
            let o = match (va, vb) {
                (Some(x), Some(y)) => {
                    let o = x.partial_cmp(&y).unwrap_or(Ordering::Equal);
                    if s.descending {
                        o.reverse()
                    } else {
                        o
                    }
                }
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        a.lead.cmp(&b.lead).then(ra.cmp(rb)).then(price_key(a).cmp(&price_key(b))).then(a.option.id.cmp(&b.option.id))
    });
    let mut out: Vec<Hit> = hits.into_iter().map(|(_, h)| h).collect();
    if q.limit > 0 {
        out.truncate(q.limit);
    }
    out
}

/// One line per hit: `id | label | key specs | price | stock -- why`.
pub fn render_hits(hits: &[Hit], total: usize, what: &str) -> String {
    if hits.is_empty() {
        return format!("No rows match {what}. Loosen a range, widen `stock`, or shorten the text.");
    }
    let mut s = format!("{} of {total} rows match {what}, best first:\n", hits.len());
    for h in hits {
        let specs: Vec<String> = h.option.specs.iter().filter(|(k, _)| k.as_str() != "stock").map(|(k, v)| format!("{k}={v}")).collect();
        let _ = writeln!(
            s,
            "- {} | {} | {} | {} | {} -- {}",
            h.option.id,
            h.option.label,
            specs.join(", "),
            h.option.price_usd.map_or("unpriced".into(), money),
            h.lead.label(),
            h.why.join("; ")
        );
    }
    s
}

// -------------------------------------------------------------- alternates

#[derive(Debug, Clone, PartialEq)]
pub struct Delta {
    pub attr: String,
    pub from: String,
    pub to: String,
    /// Relative change when both sides are numbers.
    pub pct: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Alternate {
    pub option: CatalogOption,
    pub lead: Lead,
    pub deltas: Vec<Delta>,
    pub price_delta_usd: Option<f64>,
}

/// KiBoM `compareField`: lower-cased equality, a blank never matches a
/// non-blank.
fn same_field(a: Option<&String>, b: Option<&String>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x.trim().to_lowercase() == y.trim().to_lowercase(),
        (None, None) => true,
        _ => false,
    }
}

/// Every other row that can take the same role as `incumbent_id` (the
/// same catalog path), with the attributes that differ, ranked: stock
/// class, then fewest differing attributes, then smallest price change.
/// Discontinued rows are left out. `Err` when the incumbent is not a row.
pub fn alternates(rows: &[CatalogOption], incumbent_id: &str, limit: usize) -> Result<(CatalogOption, Vec<Alternate>), String> {
    let inc = rows.iter().find(|o| o.id == incumbent_id).ok_or_else(|| format!("`{incumbent_id}` is not a row at this path"))?;
    let mut alts: Vec<Alternate> = rows
        .iter()
        .filter(|o| o.id != inc.id)
        .filter(|o| Lead::of(o) != Lead::Discontinued)
        .map(|o| {
            let mut keys: Vec<&String> = inc.specs.keys().chain(o.specs.keys()).filter(|k| k.as_str() != "stock").collect();
            keys.sort();
            keys.dedup();
            let deltas = keys
                .into_iter()
                .filter(|k| !same_field(inc.specs.get(*k), o.specs.get(*k)))
                .map(|k| {
                    let (from, to) = (inc.specs.get(k).cloned().unwrap_or_default(), o.specs.get(k).cloned().unwrap_or_default());
                    let pct = match (number(&from), number(&to)) {
                        (Some(a), Some(b)) if a != 0.0 => Some(100.0 * (b - a) / a.abs()),
                        _ => None,
                    };
                    Delta { attr: k.clone(), from, to, pct }
                })
                .collect();
            let price_delta_usd = match (inc.price_usd, o.price_usd) {
                (Some(a), Some(b)) => Some(b - a),
                _ => None,
            };
            Alternate { option: o.clone(), lead: Lead::of(o), deltas, price_delta_usd }
        })
        .collect();
    alts.sort_by(|a, b| {
        let pd = |x: &Alternate| x.price_delta_usd.map(|d| (d.abs() * 100.0).round() as i64).unwrap_or(i64::MAX);
        a.lead.cmp(&b.lead).then(a.deltas.len().cmp(&b.deltas.len())).then(pd(a).cmp(&pd(b))).then(a.option.id.cmp(&b.option.id))
    });
    if limit > 0 {
        alts.truncate(limit);
    }
    Ok((inc.clone(), alts))
}

/// The delta table: one line per alternate, `attr from -> to (+pct%)`
/// per differing attribute.
pub fn render_alternates(incumbent: &CatalogOption, alts: &[Alternate], total: usize) -> String {
    let mut s = format!(
        "incumbent {} | {} | {} | {}\n",
        incumbent.id,
        incumbent.label,
        incumbent.price_usd.map_or("unpriced".into(), money),
        Lead::of(incumbent).label()
    );
    if alts.is_empty() {
        s.push_str("no alternates at this path.\n");
        return s;
    }
    let _ = writeln!(s, "{} of {total} alternates, best first (stock, then fewest differences, then price change):", alts.len());
    for a in alts {
        let price = match (a.option.price_usd, a.price_delta_usd) {
            (Some(p), Some(d)) => format!("{} ({})", money(p), signed(d)),
            (Some(p), None) => money(p),
            (None, _) => "unpriced".into(),
        };
        let deltas: Vec<String> = a
            .deltas
            .iter()
            .map(|d| {
                let pct = d.pct.map(|p| format!(" ({}{:.0}%)", if p >= 0.0 { "+" } else { "" }, p)).unwrap_or_default();
                let blank = |t: &str| if t.is_empty() { "(none)".to_string() } else { t.to_string() };
                format!("{} {} -> {}{pct}", d.attr, blank(&d.from), blank(&d.to))
            })
            .collect();
        let _ = writeln!(
            s,
            "- {} | {} | {price} | {} | {}",
            a.option.id,
            a.option.label,
            a.lead.label(),
            if deltas.is_empty() { "identical specs".to_string() } else { deltas.join(", ") }
        );
    }
    s
}

// ---------------------------------------------------------------- bom diff

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
    Unchanged,
}

impl ChangeKind {
    /// Cargo's status words (`PackageChangeKind::status`).
    pub fn status(self) -> &'static str {
        match self {
            ChangeKind::Added => "Adding",
            ChangeKind::Removed => "Removing",
            ChangeKind::Changed => "Updating",
            ChangeKind::Unchanged => "Unchanged",
        }
    }
}

/// One BOM id's lines on one side, summed (a BOM may list an id twice).
#[derive(Debug, Clone)]
pub struct Side {
    pub name: String,
    pub qty: f64,
    pub unit: String,
    pub unit_price_usd: Option<f64>,
    pub total_usd: Option<f64>,
}

impl Side {
    fn of(lines: &[&BomLine]) -> Side {
        let qty: f64 = lines.iter().map(|l| l.qty).sum();
        let unpriced = lines.iter().any(|l| l.unit_price.is_none());
        let total = if unpriced { None } else { Some(lines.iter().filter_map(|l| l.total()).map(|m| m.as_usd()).sum()) };
        Side {
            name: lines[0].name.clone(),
            qty,
            unit: lines[0].unit.clone(),
            unit_price_usd: if unpriced { None } else { lines[0].unit_price.map(|p| p.as_usd()) },
            total_usd: total,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LineChange {
    pub kind: ChangeKind,
    pub id: String,
    pub before: Option<Side>,
    pub after: Option<Side>,
    /// `after - before` line totals; None when either side is unpriced.
    pub cost_delta_usd: Option<f64>,
    /// Stock of the row behind this id, when the catalog knows it.
    pub lead: Option<Lead>,
}

#[derive(Debug, Clone)]
pub struct BomDiff {
    pub changes: Vec<LineChange>,
    pub total_before_usd: f64,
    pub total_after_usd: f64,
    pub unpriced_before: usize,
    pub unpriced_after: usize,
}

impl BomDiff {
    pub fn count(&self, k: ChangeKind) -> usize {
        self.changes.iter().filter(|c| c.kind == k).count()
    }
    /// Ids the candidate adds or changes to that are not in stock: the
    /// stock regressions a swap introduces.
    pub fn stock_risks(&self) -> Vec<&LineChange> {
        self.changes.iter().filter(|c| matches!(c.kind, ChangeKind::Added | ChangeKind::Changed) && c.lead.is_some_and(|l| l != Lead::InStock)).collect()
    }
}

fn by_id(b: &Bom) -> BTreeMap<&str, Vec<&BomLine>> {
    let mut m: BTreeMap<&str, Vec<&BomLine>> = BTreeMap::new();
    for l in &b.lines {
        m.entry(l.id.as_str()).or_default().push(l);
    }
    m
}

/// Cargo's `PackageDiff::diff` keyed on the BOM id: ids only in
/// `incumbent` are removed, only in `candidate` added, in both with a
/// different qty or price changed. `lead` tells the stock of an id when
/// the catalog knows it (see [`leads_for`]).
pub fn diff(incumbent: &Bom, candidate: &Bom, lead: impl Fn(&str) -> Option<Lead>) -> BomDiff {
    let (a, b) = (by_id(incumbent), by_id(candidate));
    let mut ids: Vec<&str> = a.keys().chain(b.keys()).copied().collect();
    ids.sort_unstable();
    ids.dedup();
    let changes = ids
        .into_iter()
        .map(|id| {
            let before = a.get(id).map(|l| Side::of(l));
            let after = b.get(id).map(|l| Side::of(l));
            let kind = match (&before, &after) {
                (None, Some(_)) => ChangeKind::Added,
                (Some(_), None) => ChangeKind::Removed,
                (Some(x), Some(y)) => {
                    if (x.qty - y.qty).abs() > 1e-9 || x.unit_price_usd != y.unit_price_usd {
                        ChangeKind::Changed
                    } else {
                        ChangeKind::Unchanged
                    }
                }
                (None, None) => unreachable!("id came from one side"),
            };
            let cost_delta_usd = match (&before, &after) {
                (None, Some(y)) => y.total_usd,
                (Some(x), None) => x.total_usd.map(|t| -t),
                (Some(x), Some(y)) => match (x.total_usd, y.total_usd) {
                    (Some(p), Some(q)) => Some(q - p),
                    _ => None,
                },
                (None, None) => None,
            };
            LineChange { kind, id: id.to_string(), before, after, cost_delta_usd, lead: lead(id) }
        })
        .collect();
    BomDiff {
        changes,
        total_before_usd: incumbent.total().as_usd(),
        total_after_usd: candidate.total().as_usd(),
        unpriced_before: incumbent.unpriced().len(),
        unpriced_after: candidate.unpriced().len(),
    }
}

/// Cargo's change list: one status line per added/removed/changed id,
/// unchanged ids counted, then totals, unpriced lines and stock risks.
pub fn render_diff(d: &BomDiff, before_name: &str, after_name: &str) -> String {
    let mut s = format!("BOM diff: {before_name} -> {after_name}\n");
    let side = |x: &Side| {
        format!(
            "{} x {}{}",
            num(x.qty),
            x.name,
            match x.unit_price_usd {
                Some(p) => format!(" @ {}", money(p)),
                None => " (unpriced)".into(),
            }
        )
    };
    for c in &d.changes {
        let lead = c.lead.map(|l| format!(", {}", l.label())).unwrap_or_default();
        let delta = c.cost_delta_usd.map(|x| format!(" [{}]", signed(x))).unwrap_or_else(|| " [cost delta unknown: unpriced]".into());
        match (c.kind, &c.before, &c.after) {
            (ChangeKind::Added, _, Some(y)) => {
                let _ = writeln!(s, "  {:<9} {}: {}{delta}{lead}", c.kind.status(), c.id, side(y));
            }
            (ChangeKind::Removed, Some(x), _) => {
                let _ = writeln!(s, "  {:<9} {}: {}{delta}", c.kind.status(), c.id, side(x));
            }
            (ChangeKind::Changed, Some(x), Some(y)) => {
                let _ = writeln!(s, "  {:<9} {}: {} -> {}{delta}{lead}", c.kind.status(), c.id, side(x), side(y));
            }
            _ => {}
        }
    }
    let (added, removed, changed, unchanged) =
        (d.count(ChangeKind::Added), d.count(ChangeKind::Removed), d.count(ChangeKind::Changed), d.count(ChangeKind::Unchanged));
    if added + removed + changed == 0 {
        let _ = writeln!(s, "  no line changes ({unchanged} unchanged)");
    } else {
        let _ = writeln!(s, "  {added} added, {removed} removed, {changed} changed, {unchanged} unchanged");
    }
    let _ = writeln!(
        s,
        "  total {} -> {} ({}); unpriced lines {} -> {}",
        money(d.total_before_usd),
        money(d.total_after_usd),
        signed(d.total_after_usd - d.total_before_usd),
        d.unpriced_before,
        d.unpriced_after
    );
    let risks = d.stock_risks();
    if !risks.is_empty() {
        let names: Vec<String> = risks.iter().map(|c| format!("{} ({})", c.id, c.lead.map(Lead::label).unwrap_or(""))).collect();
        let _ = writeln!(s, "  stock: the candidate brings in {} part(s) not in stock: {}", risks.len(), names.join(", "));
    }
    s
}

// ---------------------------------------------------------------- id gate

/// Levenshtein distance in chars.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + usize::from(ca != cb));
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// rustc `edit_distance_with_substrings`: the distance less the length
/// difference, so a substring match scores low; half the difference
/// comes back when the lengths are comparable, all of it when one is more
/// than twice the other.
pub fn edit_distance_with_substrings(a: &str, b: &str) -> usize {
    let (la, lb) = (a.chars().count(), b.chars().count());
    let d = edit_distance(a, b);
    let diff = la.abs_diff(lb);
    let base = d.saturating_sub(diff);
    if la.max(lb) > 2 * la.min(lb) {
        base + diff
    } else {
        base + diff / 2
    }
}

fn sort_by_words(s: &str) -> String {
    let mut w: Vec<&str> = s.split('_').collect();
    w.sort_unstable();
    w.join("_")
}

/// rustc `find_best_match_for_name`: a case-insensitive exact match, else
/// the closest id within `max(len, 3) / 3` edits, else an id with the same
/// `_`-separated words in another order.
pub fn best_match<'a>(lookup: &str, candidates: impl IntoIterator<Item = &'a str> + Clone) -> Option<&'a str> {
    let upper = lookup.to_uppercase();
    if let Some(c) = candidates.clone().into_iter().find(|c| c.to_uppercase() == upper) {
        return Some(c);
    }
    let limit = lookup.chars().count().max(3) / 3;
    let mut best: Option<(usize, &str)> = None;
    for c in candidates.clone() {
        let d = edit_distance(lookup, c);
        if d <= limit && best.is_none_or(|(bd, bc)| d < bd || (d == bd && c < bc)) {
            best = Some((d, c));
        }
    }
    if let Some((_, c)) = best {
        return Some(c);
    }
    let words = sort_by_words(lookup);
    candidates.into_iter().find(|c| sort_by_words(c) == words)
}

/// The `n` closest ids by [`edit_distance_with_substrings`], closest
/// first; ties by id.
pub fn nearest<'a>(lookup: &str, candidates: impl IntoIterator<Item = &'a str>, n: usize) -> Vec<&'a str> {
    let l = lookup.to_lowercase();
    let mut v: Vec<(usize, &str)> = candidates.into_iter().map(|c| (edit_distance_with_substrings(&l, &c.to_lowercase()), c)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
    v.into_iter().take(n).map(|(_, c)| c).collect()
}

/// A design value for a catalog-backed path: a string id, a number
/// (wright-air's current-limit ids are numbers written as strings, review
/// 5 F8), or an object carrying `id` (a battery pack `{kind, id, count}`).
pub fn id_in(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Object(m) => m.get("id").and_then(id_in),
        _ => None,
    }
}

/// Whether `given` names one of `ids`; numbers compare as numbers so
/// `40` matches `"40.0"` and `"40"`.
pub fn id_accepted(given: &str, ids: &[String]) -> bool {
    if ids.iter().any(|i| i == given) {
        return true;
    }
    match given.parse::<f64>() {
        Ok(g) => ids.iter().any(|i| i.parse::<f64>().is_ok_and(|x| x == g)),
        Err(_) => false,
    }
}

pub fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |cur, k| cur.get(k))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub path: String,
    pub given: String,
    /// rustc's one best guess, when there is one within the threshold.
    pub best: Option<String>,
    pub nearest: Vec<String>,
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}` is not a catalog id at {}", self.given, self.path)?;
        if let Some(b) = &self.best {
            write!(f, "; did you mean `{b}`?")?;
        }
        if !self.nearest.is_empty() {
            write!(f, " Nearest ids: {}", self.nearest.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(", "))?;
        }
        write!(f, ". Call catalog or catalog_search at this path and use an id it returns")
    }
}

/// Every catalog-backed path `patch` sets whose id is not a row at that
/// path. A path whose catalog returns no rows is not judged (the domain
/// exposes nothing to judge against).
pub fn gate(patch: &Value, paths: &[&str], options: impl Fn(&str) -> Vec<CatalogOption>) -> Vec<Rejection> {
    let mut out = vec![];
    for path in paths {
        let Some(v) = get_path(patch, path) else { continue };
        let Some(given) = id_in(v) else { continue };
        let ids: Vec<String> = options(path).into_iter().map(|o| o.id).collect();
        if ids.is_empty() || id_accepted(&given, &ids) {
            continue;
        }
        let strs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let best = best_match(&given, strs.iter().copied()).map(str::to_string);
        let nearest = nearest(&given, strs.iter().copied(), 3).into_iter().filter(|n| Some(*n) != best.as_deref()).map(str::to_string).collect();
        out.push(Rejection { path: path.to_string(), given, best, nearest });
    }
    out
}

// --------------------------------------------------------------- workbench

/// Everything the harness needs to run the sourcing tools, so its own
/// glue is three match arms. `Err(Fail::Malformed)` is a schema error.
#[derive(Debug)]
pub enum Fail {
    Malformed(String),
    Err(String),
}

#[derive(Deserialize)]
struct SearchArgs {
    path: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    ranges: String,
    #[serde(default)]
    stock: String,
    #[serde(default)]
    sort: String,
}

#[derive(Deserialize)]
struct AlternatesArgs {
    path: String,
    #[serde(default)]
    id: String,
}

/// At most this many rows per answer (a tool result stays a few hundred
/// tokens, as the `catalog` tool's cap).
pub const MAX_ROWS: usize = 20;

fn args<T: serde::de::DeserializeOwned>(input: &Value) -> Result<T, Fail> {
    serde_json::from_value(input.clone()).map_err(|e| Fail::Malformed(format!("input does not match the tool schema: {e}")))
}

fn listing(domain: &dyn DomainTools) -> String {
    let paths = domain.catalog_paths();
    if paths.is_empty() {
        format!("the {} domain exposes no catalog lookups yet", domain.name())
    } else {
        paths.iter().map(|(p, d)| format!("- {p}: {d}")).collect::<Vec<_>>().join("\n")
    }
}

fn rows_at(domain: &dyn DomainTools, design: &Value, path: &str) -> Result<Vec<CatalogOption>, Fail> {
    if !domain.catalog_paths().iter().any(|(p, _)| *p == path) {
        return Err(Fail::Err(format!("`{path}` is not a catalog-backed path. Catalog-backed paths:\n{}", listing(domain))));
    }
    domain.options(design, path, None).map_err(|e| Fail::Err(format!("catalog lookup failed: {e:#}")))
}

/// Stock of every id the catalog knows, across all paths of `design`:
/// the `lead` for [`diff`].
pub fn leads_for(domain: &dyn DomainTools, design: &Value) -> BTreeMap<String, Lead> {
    let mut m = BTreeMap::new();
    for (path, _) in domain.catalog_paths() {
        if let Ok(rows) = domain.options(design, path, None) {
            for r in rows {
                let lead = Lead::of(&r);
                m.entry(r.id).and_modify(|l: &mut Lead| *l = (*l).min(lead)).or_insert(lead);
            }
        }
    }
    m
}

/// Run `catalog_search`, `alternates` or `bom_diff` against the shared
/// state; `None` for any other name.
pub fn workbench_call(domain: &dyn DomainTools, state: &DesignState, name: &str, input: &Value) -> Option<Result<String, Fail>> {
    Some(match name {
        "catalog_search" => catalog_search(domain, state, input),
        "alternates" => alternates_tool(domain, state, input),
        "bom_diff" => bom_diff(domain, state, input),
        _ => return None,
    })
}

fn design_of(state: &DesignState) -> Result<&Value, Fail> {
    state.design.as_ref().ok_or_else(|| Fail::Err("no design yet: call design first".into()))
}

fn catalog_search(domain: &dyn DomainTools, state: &DesignState, input: &Value) -> Result<String, Fail> {
    let a: SearchArgs = args(input)?;
    let path = a.path.trim();
    if path.is_empty() {
        return Ok(format!("catalog-backed design paths:\n{}", listing(domain)));
    }
    let design = design_of(state)?;
    let rows = rows_at(domain, design, path)?;
    let ranges = Range::parse_list(&a.ranges).map_err(Fail::Malformed)?;
    let stock = StockFilter::parse(&a.stock).ok_or_else(|| Fail::Malformed(format!("stock `{}` is not one of in_stock, listed, lead_time, restock", a.stock)))?;
    let q = Query { text: a.query.trim().to_string(), ranges, stock, sort: Sort::parse(&a.sort), limit: MAX_ROWS };
    let hits = search(&rows, &q);
    let total = search(&rows, &Query { limit: 0, ..q.clone() }).len();
    let mut what = format!("at {path}");
    if !q.text.is_empty() {
        what.push_str(&format!(" text \"{}\"", q.text));
    }
    for r in &q.ranges {
        what.push_str(&format!(" {} {}", r.attr, r.text()));
    }
    what.push_str(&format!(" stock={:?}", q.stock).to_lowercase());
    let mut out = render_hits(&hits, total, &what);
    if total > hits.len() {
        out.push_str(&format!("(showing {} of {total}; add a range or text to narrow)\n", hits.len()));
    }
    Ok(out)
}

fn alternates_tool(domain: &dyn DomainTools, state: &DesignState, input: &Value) -> Result<String, Fail> {
    let a: AlternatesArgs = args(input)?;
    let path = a.path.trim();
    let design = design_of(state)?;
    let rows = rows_at(domain, design, path)?;
    let id = if a.id.trim().is_empty() {
        get_path(design, path)
            .and_then(id_in)
            .ok_or_else(|| Fail::Err(format!("the design has no id at {path}; pass `id` explicitly")))?
    } else {
        a.id.trim().to_string()
    };
    let (inc, alts) = alternates(&rows, &id, MAX_ROWS).map_err(|e| {
        let strs: Vec<&str> = rows.iter().map(|o| o.id.as_str()).collect();
        let near = nearest(&id, strs, 3);
        Fail::Err(format!("{e}; nearest ids: {}", near.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")))
    })?;
    let total = rows.iter().filter(|o| o.id != inc.id && Lead::of(o) != Lead::Discontinued).count();
    Ok(render_alternates(&inc, &alts, total))
}

fn bom_diff(domain: &dyn DomainTools, state: &DesignState, input: &Value) -> Result<String, Fail> {
    #[derive(Deserialize)]
    struct Empty {}
    let _: Empty = args(input)?;
    let design = design_of(state)?;
    let Some(best) = &state.best else {
        return Err(Fail::Err("no incumbent yet: a design must pass check once (then edit it and call bom_diff to compare)".into()));
    };
    let before = domain.bom(&best.design).map_err(|e| Fail::Err(format!("bom of the incumbent failed: {e:#}")))?;
    let after = domain.bom(design).map_err(|e| Fail::Err(format!("bom of the current design failed: {e:#}")))?;
    let leads = leads_for(domain, design);
    let d = diff(&before, &after, |id| leads.get(id).copied());
    Ok(render_diff(&d, &format!("best verified design (round {})", best.round), "current design"))
}

/// The id gate for `edit_design`: a message to reject the patch with, or
/// `None` when every catalog id it sets is real.
pub fn gate_patch(domain: &dyn DomainTools, design: &Value, patch: &Value) -> Option<String> {
    let paths = domain.catalog_paths();
    let names: Vec<&str> = paths.iter().map(|(p, _)| *p).collect();
    let rej = gate(patch, &names, |p| domain.options(design, p, None).unwrap_or_default());
    if rej.is_empty() {
        None
    } else {
        Some(format!("edit rejected, nothing changed: {}", rej.iter().map(|r| r.to_string()).collect::<Vec<_>>().join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wright_core::units::Money;

    fn row(id: &str, label: &str, specs: &[(&str, &str)], price: Option<f64>, stock: &str) -> CatalogOption {
        let mut m: BTreeMap<String, String> = specs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        if !stock.is_empty() {
            m.insert("stock".into(), stock.into());
        }
        CatalogOption { id: id.into(), label: label.into(), specs: m, price_usd: price, in_stock: None, url: format!("https://x/{id}") }
    }

    /// The fake catalog: four lift motors and one that is discontinued.
    fn motors() -> Vec<CatalogOption> {
        vec![
            row("tmotor_mn3110_780", "T-Motor MN3110 780KV", &[("kv", "780"), ("mass_g", "81"), ("max_current_a", "20"), ("cells", "4-6S")], Some(49.9), "in stock"),
            row("tmotor_mn3110_470", "T-Motor MN3110 470KV", &[("kv", "470"), ("mass_g", "81"), ("max_current_a", "19"), ("cells", "6S")], Some(49.9), "backorder"),
            row("sunnysky_x2814_900", "SunnySky X2814 900KV", &[("kv", "900"), ("mass_g", "108"), ("max_current_a", "31"), ("cells", "3-4S")], Some(29.5), "stock unstated"),
            row("emax_eco_ii_2807_1300", "EMAX ECO II 2807 1300KV", &[("kv", "1300"), ("mass_g", "52"), ("max_current_a", "42")], Some(19.99), "out of stock"),
            row("old_motor_x", "Discontinued thing", &[("kv", "800"), ("mass_g", "80")], None, "discontinued"),
        ]
    }

    #[test]
    fn numbers_come_out_of_spec_text() {
        assert_eq!(number("16"), Some(16.0));
        assert_eq!(number("2-4S"), Some(2.0));
        assert_eq!(number("bore [5.0] mm, bolts [3]"), Some(5.0));
        assert_eq!(number("-3.5 deg"), Some(-3.5));
        assert_eq!(number("true"), None);
        assert_eq!(number("4S."), Some(4.0));
    }

    #[test]
    fn ranges_parse_and_reject_nonsense() {
        let r = Range::parse_list("kv:800..1200, mass_g:..60,max_current_a:30..").unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], Range { attr: "kv".into(), min: Some(800.0), max: Some(1200.0) });
        assert_eq!(r[1], Range { attr: "mass_g".into(), min: None, max: Some(60.0) });
        assert_eq!(r[2].text(), ">= 30");
        assert!(Range::parse_list("kv:1200..800").unwrap_err().contains("above max"));
        assert!(Range::parse_list("kv=800").unwrap_err().contains("attr:min..max"));
        assert!(Range::parse_list("kv:abc..").unwrap_err().contains("not a number"));
        assert!(Range::parse_list("").unwrap().is_empty());
    }

    #[test]
    fn search_filters_by_stock_and_ranges_then_ranks_by_lead_then_price() {
        let rows = motors();
        // default stock policy: in stock or unstated; the backorder and
        // out-of-stock rows are out, the discontinued one always is
        let hits = search(&rows, &Query::default());
        let ids: Vec<String> = hits.iter().map(|h| h.option.id.clone()).collect();
        assert_eq!(ids, ["tmotor_mn3110_780", "sunnysky_x2814_900"]);
        assert_eq!(hits[0].why, ["in stock", "$49.90"]);
        // widen to lead_time: the backorder row joins, after the in-stock one
        let q = Query { stock: StockFilter::LeadTime, ..Default::default() };
        let ids: Vec<String> = search(&rows, &q).iter().map(|h| h.option.id.clone()).collect();
        assert_eq!(ids, ["tmotor_mn3110_780", "sunnysky_x2814_900", "tmotor_mn3110_470"]);
        // a range: light motors, any stock but discontinued
        let q = Query { stock: StockFilter::Restock, ranges: Range::parse_list("mass_g:..90").unwrap(), ..Default::default() };
        let hits = search(&rows, &q);
        let ids: Vec<String> = hits.iter().map(|h| h.option.id.clone()).collect();
        assert_eq!(ids, ["tmotor_mn3110_780", "tmotor_mn3110_470", "emax_eco_ii_2807_1300"]);
        assert_eq!(hits[0].why[0], "mass_g 81 within <= 90");
        // a row missing the attribute is dropped, not treated as 0
        let q = Query { ranges: Range::parse_list("cells:..10").unwrap(), stock: StockFilter::Restock, ..Default::default() };
        assert!(search(&rows, &q).iter().all(|h| h.option.specs.contains_key("cells")));
    }

    #[test]
    fn search_text_ranks_id_over_label_over_spec_and_sort_overrides() {
        let rows = motors();
        let q = Query { text: "900".into(), stock: StockFilter::Restock, ..Default::default() };
        let hits = search(&rows, &q);
        assert_eq!(hits[0].option.id, "sunnysky_x2814_900");
        assert_eq!(hits[0].why[0], "id contains \"900\"");
        // kv descending puts the 1300 KV (out of stock) motor first
        let q = Query { stock: StockFilter::Restock, sort: Sort::parse("-kv"), ..Default::default() };
        let ids: Vec<String> = search(&rows, &q).iter().map(|h| h.option.id.clone()).collect();
        assert_eq!(ids, ["emax_eco_ii_2807_1300", "sunnysky_x2814_900", "tmotor_mn3110_780", "tmotor_mn3110_470"]);
        let q = Query { stock: StockFilter::Restock, sort: Sort::parse("price"), ..Default::default() };
        assert_eq!(search(&rows, &q)[0].option.id, "emax_eco_ii_2807_1300");
        assert_eq!(Sort::parse(""), None);
        assert_eq!(Sort::parse("+mass_g"), Some(Sort { attr: "mass_g".into(), descending: false }));
    }

    #[test]
    fn render_hits_says_why() {
        let rows = motors();
        let q = Query { ranges: Range::parse_list("kv:700..1000").unwrap(), ..Default::default() };
        let hits = search(&rows, &q);
        let text = render_hits(&hits, hits.len(), "at propulsion.lift_motor kv 700..1000");
        assert!(text.starts_with("2 of 2 rows match at propulsion.lift_motor kv 700..1000, best first:\n"));
        assert!(text.contains("- tmotor_mn3110_780 | T-Motor MN3110 780KV | cells=4-6S, kv=780, mass_g=81, max_current_a=20 | $49.90 | in stock -- kv 780 within 700..1000; in stock; $49.90"));
        assert!(render_hits(&[], 5, "at x").starts_with("No rows match at x."));
    }

    #[test]
    fn alternates_list_attribute_deltas_and_rank_by_stock_then_differences() {
        let rows = motors();
        let (inc, alts) = alternates(&rows, "tmotor_mn3110_780", 0).unwrap();
        assert_eq!(inc.id, "tmotor_mn3110_780");
        let ids: Vec<&str> = alts.iter().map(|a| a.option.id.as_str()).collect();
        // unstated beats backorder beats out of stock; discontinued is gone
        assert_eq!(ids, ["sunnysky_x2814_900", "tmotor_mn3110_470", "emax_eco_ii_2807_1300"]);
        let sunny = &alts[0];
        assert_eq!(sunny.deltas.len(), 4);
        let kv = sunny.deltas.iter().find(|d| d.attr == "kv").unwrap();
        assert_eq!((kv.from.as_str(), kv.to.as_str()), ("780", "900"));
        assert!((kv.pct.unwrap() - 15.38).abs() < 0.01);
        assert!((sunny.price_delta_usd.unwrap() + 20.4).abs() < 1e-9);
        // a blank vs a value is a difference (KiBoM compareField without mergeBlankFields)
        let emax = &alts[2];
        assert!(emax.deltas.iter().any(|d| d.attr == "cells" && d.from == "4-6S" && d.to.is_empty()));
        let text = render_alternates(&inc, &alts, 3);
        assert!(text.starts_with("incumbent tmotor_mn3110_780 | T-Motor MN3110 780KV | $49.90 | in stock\n"));
        assert!(text.contains("- sunnysky_x2814_900 | SunnySky X2814 900KV | $29.50 (-$20.40) | stock unstated | cells 4-6S -> 3-4S (-25%), kv 780 -> 900 (+15%), mass_g 81 -> 108 (+33%), max_current_a 20 -> 31 (+55%)"));
        assert!(text.contains("cells 4-6S -> (none)"));
        assert!(alternates(&rows, "nope", 0).is_err());
    }

    fn line(id: &str, qty: f64, price: Option<f64>) -> BomLine {
        BomLine { category: "motor".into(), id: id.into(), name: id.to_uppercase(), qty, unit: "ea".into(), unit_price: price.map(Money::usd), url: String::new(), note: String::new() }
    }

    #[test]
    fn bom_diff_classifies_like_cargo_and_sums_cost() {
        let before = Bom { lines: vec![line("a", 4.0, Some(10.0)), line("b", 1.0, Some(5.0)), line("c", 2.0, Some(1.0)), line("c", 1.0, Some(1.0))] };
        let after = Bom { lines: vec![line("a", 4.0, Some(10.0)), line("c", 3.0, Some(1.5)), line("d", 2.0, None), line("e", 1.0, Some(7.0))] };
        let lead = |id: &str| match id {
            "e" => Some(Lead::OutOfStock),
            "d" => Some(Lead::InStock),
            _ => None,
        };
        let d = diff(&before, &after, lead);
        let kinds: Vec<(&str, ChangeKind)> = d.changes.iter().map(|c| (c.id.as_str(), c.kind)).collect();
        assert_eq!(kinds, [("a", ChangeKind::Unchanged), ("b", ChangeKind::Removed), ("c", ChangeKind::Changed), ("d", ChangeKind::Added), ("e", ChangeKind::Added)]);
        let c = &d.changes[2];
        assert_eq!(c.before.as_ref().unwrap().qty, 3.0);
        assert_eq!(c.after.as_ref().unwrap().unit_price_usd, Some(1.5));
        assert!((c.cost_delta_usd.unwrap() - 1.5).abs() < 1e-9);
        assert_eq!(d.changes[1].cost_delta_usd, Some(-5.0));
        assert_eq!(d.changes[3].cost_delta_usd, None);
        assert_eq!(d.unpriced_after, 1);
        assert_eq!(d.stock_risks().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["e"]);
        let text = render_diff(&d, "best", "current");
        assert!(text.contains("  Removing  b: 1 x B @ $5.00 [-$5.00]"), "{text}");
        assert!(text.contains("  Updating  c: 3 x C @ $1.00 -> 3 x C @ $1.50 [+$1.50]"), "{text}");
        assert!(text.contains("  Adding    d: 2 x D (unpriced) [cost delta unknown: unpriced], in stock"), "{text}");
        assert!(text.contains("  Adding    e: 1 x E @ $7.00 [+$7.00], OUT OF STOCK"), "{text}");
        assert!(text.contains("2 added, 1 removed, 1 changed, 1 unchanged"), "{text}");
        assert!(text.contains("total $48.00 -> $51.50 (+$3.50); unpriced lines 0 -> 1"), "{text}");
        assert!(text.contains("stock: the candidate brings in 1 part(s) not in stock: e (OUT OF STOCK)"), "{text}");
        let same = diff(&before, &before, |_| None);
        assert!(render_diff(&same, "a", "a").contains("no line changes (3 unchanged)"));
    }

    #[test]
    fn best_match_follows_rustc() {
        let ids = ["tmotor_mn3110_780", "sunnysky_x2814_900", "emax_eco_ii_2807_1300"];
        assert_eq!(best_match("TMOTOR_MN3110_780", ids), Some("tmotor_mn3110_780"));
        assert_eq!(best_match("tmotor_mn3110_78", ids), Some("tmotor_mn3110_780"));
        // a third of the length is the limit: 9 edits on a 23-char lookup is too far
        assert_eq!(best_match("tmotor_mn3110_xxxxxxxxx", ids), None);
        // words in another order
        assert_eq!(best_match("900_x2814_sunnysky", ids), Some("sunnysky_x2814_900"));
        assert_eq!(best_match("zzz", ids), None);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        // substring scoring: a fragment of comparable length is close (half
        // the length difference comes back), a tiny fragment of a long id
        // is not (all of it does, rustc's "more than twice" rule)
        assert_eq!(edit_distance_with_substrings("mn3110_780", "tmotor_mn3110_780"), 3);
        assert_eq!(edit_distance_with_substrings("mn3110", "tmotor_mn3110_780"), 11);
        assert_eq!(nearest("mn3110_780", ids, 2)[0], "tmotor_mn3110_780");
    }

    #[test]
    fn gate_rejects_made_up_ids_with_suggestions_and_accepts_numbers() {
        let rows = motors();
        let options = |p: &str| if p == "propulsion.lift_motor" { rows.clone() } else if p == "propulsion.lift_current_limit_a" { vec![row("40", "40 A", &[], None, ""), row("42", "42 A", &[], None, "")] } else { vec![] };
        let paths = ["propulsion.lift_motor", "propulsion.lift_current_limit_a", "avionics.fc"];
        // a typo
        let rej = gate(&json!({"propulsion": {"lift_motor": "tmotor_mn3110_78o"}}), &paths, options);
        assert_eq!(rej.len(), 1);
        assert_eq!(rej[0].best.as_deref(), Some("tmotor_mn3110_780"));
        // the best guess is not repeated among the nearest
        assert_eq!(rej[0].nearest.len(), 2);
        assert_eq!(rej[0].nearest[0], "tmotor_mn3110_470");
        let msg = rej[0].to_string();
        assert!(msg.starts_with("`tmotor_mn3110_78o` is not a catalog id at propulsion.lift_motor; did you mean `tmotor_mn3110_780`? Nearest ids: `tmotor_mn3110_470`, "), "{msg}");
        // an invented id: no best guess, still the nearest three
        let rej = gate(&json!({"propulsion": {"lift_motor": "brushless_motor_2200kv"}}), &paths, options);
        assert_eq!(rej[0].best, None);
        assert_eq!(rej[0].nearest.len(), 3);
        // real ids pass; a number matches a numeric id string (F8); an
        // object carrying `id` is judged by it; a path with no catalog
        // rows is not judged; untouched paths are ignored
        assert!(gate(&json!({"propulsion": {"lift_motor": "sunnysky_x2814_900", "lift_current_limit_a": 40}}), &paths, options).is_empty());
        assert!(gate(&json!({"propulsion": {"lift_current_limit_a": 40.0}}), &paths, options).is_empty());
        assert_eq!(gate(&json!({"propulsion": {"lift_current_limit_a": 41}}), &paths, options)[0].given, "41");
        assert!(gate(&json!({"avionics": {"fc": "anything"}}), &paths, options).is_empty());
        assert!(gate(&json!({"wing": {"span_m": 2.0}}), &paths, options).is_empty());
        assert_eq!(id_in(&json!({"kind": "pack", "id": "p1", "count": 2})).as_deref(), Some("p1"));
        assert!(id_accepted("40", &["40.0".into()]));
        assert!(!id_accepted("40", &["400".into()]));
    }

    #[test]
    fn lead_reads_air_labels_then_in_stock() {
        assert_eq!(Lead::of(&row("a", "", &[], None, "pre-order")), Lead::Weeks);
        assert_eq!(Lead::of(&row("a", "", &[], None, "stock unstated")), Lead::Unstated);
        let mut o = row("a", "", &[], None, "");
        o.in_stock = Some(false);
        assert_eq!(Lead::of(&o), Lead::OutOfStock);
        o.in_stock = Some(true);
        assert_eq!(Lead::of(&o), Lead::InStock);
        assert!(StockFilter::InStock.accepts(Lead::InStock) && !StockFilter::InStock.accepts(Lead::Unstated));
        assert!(!StockFilter::Restock.accepts(Lead::Discontinued));
        assert_eq!(StockFilter::parse(""), Some(StockFilter::Listed));
        assert_eq!(StockFilter::parse("soon"), None);
    }
}
