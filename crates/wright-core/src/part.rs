//! Parts and bills of materials with their provenance.
//!
//! A catalog row is a claim read off a page: it carries the URL and the
//! date it was read. Fields a part's role depends on are required when the
//! catalog loads (a motor with no current rating is rejected then, not
//! treated as zero amps later -- the Python baseline's optimizer picked an ESC with no
//! mass because a missing `mass_g` read as 0 g).

use crate::units::Money;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub url: String,
    /// ISO date the page was read.
    pub retrieved: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BomLine {
    pub category: String,
    pub id: String,
    pub name: String,
    pub qty: f64,
    pub unit: String,
    /// None: nobody sells it at a stated price -- a failed budget check.
    pub unit_price: Option<Money>,
    pub url: String,
    pub note: String,
}

impl BomLine {
    pub fn total(&self) -> Option<Money> {
        self.unit_price.map(|p| p * self.qty)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bom {
    pub lines: Vec<BomLine>,
}

impl Bom {
    pub fn total(&self) -> Money {
        self.lines.iter().filter_map(BomLine::total).sum()
    }
    pub fn unpriced(&self) -> Vec<&BomLine> {
        self.lines.iter().filter(|l| l.unit_price.is_none()).collect()
    }
    pub fn csv(&self) -> String {
        let mut s = String::from("category,id,name,qty,unit,unit_price_usd,total_usd,url,note\n");
        let q = |t: &str| {
            if t.contains(',') || t.contains('"') {
                format!("\"{}\"", t.replace('"', "\"\""))
            } else {
                t.to_string()
            }
        };
        for l in &self.lines {
            let price = l.unit_price.map(|p| format!("{:.2}", p.as_usd())).unwrap_or_default();
            let total = l.total().map(|p| format!("{:.2}", p.as_usd())).unwrap_or_default();
            let _ = writeln!(
                s,
                "{},{},{},{},{},{},{},{},{}",
                q(&l.category),
                q(&l.id),
                q(&l.name),
                l.qty,
                q(&l.unit),
                price,
                total,
                q(&l.url),
                q(&l.note)
            );
        }
        s
    }
}
