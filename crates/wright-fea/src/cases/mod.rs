//! Per-domain cases, and what every case reports: pictures and films into
//! the stage's manifest, and a closed-form vs FEA row per quantity into
//! `comparison.json` / `comparison.csv`.

#[cfg(feature = "air")]
pub mod air;
pub mod balloon;
pub mod rocket;

use crate::render::{contour, mode_animation, Picture};
use crate::results::Solved;
use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use wright_core::Manifest;

/// One quantity both ways.
#[derive(Debug, Clone, Serialize)]
pub struct Compare {
    pub part: String,
    pub case: String,
    pub quantity: String,
    pub closed_form: f64,
    pub fea: f64,
    /// fea / closed_form.
    pub ratio: f64,
    pub note: String,
}

pub struct Run {
    /// The stage directory (`<out_dir>/fea`).
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub compare: Vec<Compare>,
    /// Draw pictures and films (off in quick runs).
    pub media: bool,
}

impl Run {
    pub fn new(dir: &Path, title: &str) -> Self {
        Run { dir: dir.to_path_buf(), manifest: Manifest::new("fea", title), compare: vec![], media: true }
    }
    pub fn work(&self, sub: &str) -> PathBuf {
        self.dir.join("work").join(sub)
    }
    pub fn cmp(&mut self, part: &str, case: &str, q: &str, closed_form: f64, fea: f64, note: &str) {
        self.compare.push(Compare { part: part.into(), case: case.into(), quantity: q.into(), closed_form, fea, ratio: fea / closed_form, note: note.into() });
    }
    /// A contour picture of a static result, listed in the manifest.
    pub fn picture(&mut self, s: &Solved, field: &str, file: &str, label: &str, summary: &str, view: [f64; 2], metrics: &[(&str, f64)]) -> Result<()> {
        let path = self.dir.join(file);
        if self.media {
            contour(s, field, &Picture { title: label, note: "CalculiX 2.23", view, scale: None }, &path)?;
            self.manifest.add(&self.dir, &path, label, summary, metrics);
        } else {
            self.numbers(file, label, summary, metrics)?;
        }
        Ok(())
    }
    /// A mode-shape film, listed in the manifest.
    pub fn film(&mut self, s: &Solved, mode: usize, file: &str, label: &str, summary: &str, view: [f64; 2], metrics: &[(&str, f64)]) -> Result<()> {
        let path = self.dir.join(file);
        if self.media {
            mode_animation(s, mode, &Picture { title: label, note: "CalculiX 2.23 *FREQUENCY", view, scale: None }, &path, 24)?;
            self.manifest.add(&self.dir, &path, label, summary, metrics);
        } else {
            self.numbers(file, label, summary, metrics)?;
        }
        Ok(())
    }
    /// Numbers only: a small JSON item.
    pub fn numbers(&mut self, file: &str, label: &str, summary: &str, metrics: &[(&str, f64)]) -> Result<()> {
        let path = self.dir.join(Path::new(file).with_extension("json"));
        let m: serde_json::Map<String, serde_json::Value> = metrics.iter().filter(|(_, v)| v.is_finite()).map(|(k, v)| (k.to_string(), serde_json::json!(v))).collect();
        std::fs::write(&path, serde_json::to_string_pretty(&serde_json::json!({"label": label, "summary": summary, "metrics": m}))?)?;
        self.manifest.add(&self.dir, &path, label, summary, metrics);
        Ok(())
    }
    /// Write comparison.json / .csv, list them, and return the manifest.
    pub fn finish(mut self) -> Result<Manifest> {
        let j = self.dir.join("comparison.json");
        std::fs::write(&j, serde_json::to_string_pretty(&self.compare)?)?;
        let mut csv = String::from("part,case,quantity,closed_form,fea,ratio,note\n");
        for c in &self.compare {
            csv.push_str(&format!("{},{},{},{:.6},{:.6},{:.4},\"{}\"\n", c.part, c.case, c.quantity, c.closed_form, c.fea, c.ratio, c.note.replace('"', "'")));
        }
        let c = self.dir.join("comparison.csv");
        std::fs::write(&c, csv)?;
        let n = self.compare.len() as f64;
        self.manifest.add(&self.dir, &c, "FEA vs closed form", "every quantity both ways: the domain's closed-form number and CalculiX's", &[("rows", n)]);
        // the decks and .frd files run to 100+ MB on a shared, small disk:
        // dropped unless WRIGHT_FEA_KEEP_WORK is set
        if std::env::var_os("WRIGHT_FEA_KEEP_WORK").is_none() {
            let _ = std::fs::remove_dir_all(self.dir.join("work"));
        }
        Ok(self.manifest)
    }
}

/// The FEA stage for `wright_core::Stages`.
pub struct FeaStage;

impl wright_core::Stage for FeaStage {
    fn name(&self) -> &'static str {
        "fea"
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn applies_to(&self, domain: &str) -> bool {
        matches!(domain, "rocket" | "balloon") || (cfg!(feature = "air") && domain == "aircraft")
    }
    fn run(&self, ctx: &wright_core::StageCtx) -> Result<Manifest> {
        match ctx.domain.as_str() {
            #[cfg(feature = "air")]
            "aircraft" => air::run(&serde_json::from_value(ctx.design.clone())?, &ctx.dir, true),
            "rocket" => rocket::run(&ctx.design, &ctx.dir, true),
            "balloon" => balloon::run(&ctx.design, &ctx.out_dir, &ctx.dir, true),
            d => anyhow::bail!("no FEA cases for {d}"),
        }
    }
}
