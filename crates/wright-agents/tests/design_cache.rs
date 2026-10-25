//! The memoized design search (cost + latency, 10/24 night): the same
//! (spec, regression cases) on the same binary is searched once, across
//! rounds and runs; a new regression case is a new search. Its own test
//! binary, because the switch is process-wide.
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use wright_agents::tools;
use wright_core::{Artifacts, Bom, CheckId, Condition, Domain, Finished, Report, Source};

static SEARCHES: AtomicUsize = AtomicUsize::new(0);

struct Counted;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum C {
    Ok,
}
impl CheckId for C {
    const ALL: &'static [Self] = &[C::Ok];
    fn name(self) -> &'static str {
        "ok"
    }
    fn describe(self) -> &'static str {
        "test"
    }
}
impl Domain for Counted {
    const NAME: &'static str = "design_cache_test";
    type Spec = Value;
    type Design = Value;
    type Check = C;
    fn spec_schema() -> Value {
        json!({"type": "object"})
    }
    fn design(s: &Value) -> anyhow::Result<Value> {
        SEARCHES.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"x": s["x"].as_f64().unwrap_or(0.0) * 2.0}))
    }
    fn check(_: &Value, _: &Value) -> Finished {
        let mut r = Report::<C>::new();
        r.margin(C::Ok, 1.0, "", Source::Geometry, "");
        r.finish()
    }
    fn bom(_: &Value) -> Bom {
        Bom::default()
    }
    fn build(_: &Value, _: &Path) -> anyhow::Result<Artifacts> {
        Ok(Artifacts::default())
    }
    fn objective(_: &Value) -> (f64, &'static str) {
        (0.0, "t")
    }
}

#[test]
fn the_same_search_runs_once_and_a_new_case_searches_again() {
    let dir = std::env::temp_dir().join(format!("wright-design-cache-test-{}", std::process::id()));
    std::env::set_var("WRIGHT_CACHE_DIR", &dir);
    let t = tools::<Counted>();
    let spec = json!({"x": 3.0});
    // off by default: every call searches
    t.design_under(&spec, &[]).unwrap();
    t.design_under(&spec, &[]).unwrap();
    assert_eq!(SEARCHES.load(Ordering::SeqCst), 2);
    wright_agents::tools::enable_design_cache(true);
    let a = t.design_under(&spec, &[]).unwrap();
    let b = t.design_under(&spec, &[]).unwrap();
    assert_eq!(a, b);
    assert_eq!(SEARCHES.load(Ordering::SeqCst), 3, "second identical search is a hit");
    let case = Condition { id: "r1".into(), check: "ok".into(), claim: "c".into(), spec: json!({"x": 3.0, "cold": true}) };
    t.design_under(&spec, std::slice::from_ref(&case)).unwrap();
    assert_eq!(SEARCHES.load(Ordering::SeqCst), 4, "a regression case changes the key");
    t.design_under(&spec, &[case]).unwrap();
    assert_eq!(SEARCHES.load(Ordering::SeqCst), 4);
    let _ = std::fs::remove_dir_all(dir);
}
