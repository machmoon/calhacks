//! Wright's shared contract: units, checks that cannot pass by omission,
//! sourced parts, and the [`Domain`] trait every vehicle family implements
//! (aircraft in `wright-air`, rockets in `wright-rocket`, balloons in
//! `wright-balloon`). The agents (`wright-agents`) see every domain only
//! through this trait, so one harness designs all three.

/// Content-addressed on-disk result cache (ccache-style LRU trim); slicer, search, forecast memoization.
pub mod cache;
pub mod check;
pub mod event;
pub mod part;
pub mod shard;
pub mod stage;
pub mod units;

pub use event::{Bound, Event, Headline, PlanStatus, PlanTask};
pub use check::{CheckId, Evidence, Finished, Outcome, Report, Source};
pub use part::{Bom, BomLine, Provenance};
pub use stage::{Item, Manifest, Stage, StageCheck, StageCtx, StageRun, Stages};


use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};

/// Files a design run wrote (CAD, sliced prints, BOM, harness, params...).
/// A path may be a directory (everything under it belongs to the label).
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct Artifacts {
    pub files: Vec<(String, PathBuf)>,
    /// What a mesh file (or every mesh under a directory) is, when the
    /// build knows: the guide then never has to guess printed vs bought
    /// from file names (review 4, finding 13).
    #[serde(default)]
    pub kinds: Vec<(PathBuf, ArtifactKind)>,
}

/// What a mesh artifact is to the person building it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// A part of the vehicle that is 3D printed.
    PrintedPart,
    /// A part of the vehicle that is bought (tube, motor, tank).
    BoughtPart,
    /// The whole vehicle in its own frame: what pictures are drawn from.
    Assembly,
    /// A printed tool (jig, fixture): printed, not part of the vehicle.
    PrintedTool,
}

impl Artifacts {
    pub fn add(&mut self, label: impl Into<String>, path: impl Into<PathBuf>) {
        self.files.push((label.into(), path.into()));
    }
    /// Add a file (or directory) and say what it is.
    pub fn add_kind(&mut self, label: impl Into<String>, path: impl Into<PathBuf>, kind: ArtifactKind) {
        let path = path.into();
        self.kinds.push((path.clone(), kind));
        self.files.push((label.into(), path));
    }
    /// The declared kind of `path`, or of the nearest declared directory
    /// above it.
    pub fn kind_of(&self, path: &Path) -> Option<ArtifactKind> {
        self.kinds
            .iter()
            .filter(|(p, _)| path.starts_with(p))
            .max_by_key(|(p, _)| p.components().count())
            .map(|(_, k)| *k)
    }
}

/// One catalog row an open design choice can take: what the agents' read-only
/// `catalog` tool returns, so they pick ids instead of guessing them.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct CatalogOption {
    pub id: String,
    pub label: String,
    /// A few key specs as text ("od_mm": "16", "mass_g": "42").
    #[serde(default)]
    pub specs: std::collections::BTreeMap<String, String>,
    pub price_usd: Option<f64>,
    pub in_stock: Option<bool>,
    #[serde(default)]
    pub url: String,
}

/// One condition a design must also pass under: a confirmed regression
/// case (a reviewer's attack the host ran), as the full spec it produces.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Condition<S> {
    /// The regression id (`{run}-r{round}-{n}`).
    pub id: String,
    /// The check the attack failed: the one to screen candidates on.
    pub check: String,
    /// The reviewer's claim, for messages.
    pub claim: String,
    /// The working spec with the attack's patch applied.
    pub spec: S,
}

/// One vehicle family. `Spec` is what a person (or an agent) asks for and
/// may leave choices open; `Design` is fully resolved -- every part named,
/// every dimension set -- and is the only thing that gets evaluated, so an
/// unresolved choice can never be "evaluated as zero".
pub trait Domain {
    /// Stable name: "aircraft", "rocket", "balloon".
    const NAME: &'static str;
    type Spec: Serialize + DeserializeOwned + Clone + Send + Sync;
    type Design: Serialize + DeserializeOwned + Clone + Send + Sync;
    type Check: CheckId;

    /// JSON Schema of `Spec`, for agents' tool definitions.
    fn spec_schema() -> serde_json::Value;

    /// Resolve every open choice by rule and search; deterministic.
    fn design(spec: &Self::Spec) -> anyhow::Result<Self::Design>;

    /// Like [`Domain::design`], but the design must also pass `check`
    /// under each of `conditions`: the confirmed regression cases (a 1000 m
    /// field, a cold morning), each a full spec. A domain whose search can
    /// screen candidates against them overrides this; the default ignores
    /// them and designs from `spec` alone (the harness still judges every
    /// condition afterwards).
    fn design_under(spec: &Self::Spec, conditions: &[Condition<Self::Spec>]) -> anyhow::Result<Self::Design> {
        let _ = conditions;
        Self::design(spec)
    }

    /// The domain's own spec validation, beyond what the schema's types
    /// and bounds say: combinations that cannot occur (an ISA+45 K day at a
    /// 3500 m field), requirements no design can meet. The harness runs it
    /// on every spec it accepts, on every reviewer attack (a patched spec
    /// it refuses is rejected, not confirmed), and on every stored
    /// regression case (one it refuses is set aside). Default: accept.
    fn validate_spec(spec: &Self::Spec) -> anyhow::Result<()> {
        let _ = spec;
        Ok(())
    }

    /// Checks that compare the design with the spec it was made for (the
    /// design's copy of the requirements, a contract figure): under an
    /// attack's perturbed spec they fail for every design, since no design
    /// can match a spec it was not made from. The harness refuses attacks
    /// that predict one of these fails and never judges a stored regression
    /// case on one (review 5, F1). Default: none.
    fn consistency_checks() -> &'static [&'static str] {
        &[]
    }

    /// Run every check on a resolved design. Never fails silently: an
    /// analysis that cannot run reports its checks as missing.
    fn check(spec: &Self::Spec, design: &Self::Design) -> Finished;

    /// The bill of materials, every line priced and sourced (an unpriced line
    /// is a failed budget check, never a free part).
    fn bom(design: &Self::Design) -> Bom;

    /// Write CAD, prints, wiring and anything else a builder needs.
    fn build(design: &Self::Design, out: &Path) -> anyhow::Result<Artifacts>;

    /// One number to maximise among passing designs, with its label
    /// ("range km", "apogee m", "burst altitude m").
    fn objective(design: &Self::Design) -> (f64, &'static str);

    /// The three or four numbers a builder asks for first (endurance, cost,
    /// mass, print time; apogee; burst altitude), each against its
    /// requirement. Shown at the top of the desktop app.
    fn headlines(_spec: &Self::Spec, _design: &Self::Design) -> Vec<Headline> {
        Vec::new()
    }

    /// The design paths whose value is a catalog id, each with a line on
    /// what it chooses (`("structure.boom_tube", "carbon boom tube")`).
    fn catalog_paths() -> Vec<(&'static str, &'static str)> {
        Vec::new()
    }

    /// Catalog rows the choice at `path` of `design` can take, filtered by
    /// a free-text `query` (case-insensitive match on id or label). Read
    /// only. The default (no catalog lookups) returns nothing.
    fn options(_design: &Self::Design, _path: &str, _query: Option<&str>) -> Vec<CatalogOption> {
        Vec::new()
    }

    /// The same rows when no design exists yet (the spec is still being
    /// written, or the search found nothing). A domain whose catalog does
    /// not depend on the design returns `Some`; the default `None` keeps
    /// the agents' catalog tool closed until `design` succeeds.
    fn options_before_design(_path: &str, _query: Option<&str>) -> Option<Vec<CatalogOption>> {
        None
    }
}
