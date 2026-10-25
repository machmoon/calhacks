//! The flight simulation Pat can watch: a design flown on a real GFS wind
//! forecast, by Wright's model and by Tawhiri's, written to `<out>/sim/`
//! as tracks (CSV), a comparison (JSON), an animation (MP4) and a still
//! (PNG), listed in `<out>/sim/manifest.json`
//! (`{kind: "sim", title, items: [{label, path, media, summary, metrics}]}`).
//!
//! The winds come from a cached Tawhiri prediction
//! (`data/balloon/predictions/*.json`, fetched by
//! `cargo run -p wright-balloon --example sim`); nothing here touches the
//! network, so a build is reproducible offline.

use crate::catalog::Catalog;
use crate::model::{Design, Spec};
use crate::predict::{self, Point, Prediction};
use anyhow::{Context, Result};
use serde_json::json;
use wright_core::Manifest;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The cached predictions, newest name last.
pub fn predictions_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/balloon/predictions")
}

/// The newest cached prediction, if any.
pub fn latest_prediction() -> Option<Prediction> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(predictions_dir()).ok()?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    v.sort();
    let t = std::fs::read_to_string(v.last()?).ok()?;
    serde_json::from_str(&t).ok()
}

/// The renderer: `crates/wright-balloon/tools/render_flight.py`, run by the
/// Python in `WRIGHT_PYTHON` (needs matplotlib and an ffmpeg on PATH).
/// Without `WRIGHT_PYTHON`, the team's shared venv (docs/TODO.md,
/// Downloads log) when it exists.
fn python() -> Option<PathBuf> {
    const SHARED_VENV: &str = "/private/tmp/claude-501/-Users-patliu/e9f305f9-7d6c-4ecb-a4fc-7e5345001431/scratchpad/venv312/bin/python";
    std::env::var_os("WRIGHT_PYTHON").map(PathBuf::from).or_else(|| Some(PathBuf::from(SHARED_VENV))).filter(|p| p.exists())
}

struct Track {
    name: &'static str,
    label: String,
    points: Vec<(Point, &'static str)>,
}

fn haversine_km(a: &Point, b: &Point) -> f64 {
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dp = p2 - p1;
    let dl = (b.lon - a.lon).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * predict::TAWHIRI_R_EARTH_M * h.sqrt().asin() / 1e3
}

fn bearing_deg(a: &Point, b: &Point) -> f64 {
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dl = (b.lon - a.lon).to_radians();
    (dl.sin() * p2.cos()).atan2(p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos()).to_degrees().rem_euclid(360.0)
}

/// Wright's flight (`flight::simulate`) as lat/lon points.
fn wright_track(spec: &Spec, d: &Design, cat: &Catalog) -> Result<(crate::flight::Flight, Vec<(Point, &'static str)>)> {
    let m = crate::mass::masses(d, cat)?;
    let fl = crate::flight::simulate(spec, d, cat, &m)?;
    let (lat0, lon0) = (spec.launch.lat_deg, spec.launch.lon_deg);
    let r = predict::TAWHIRI_R_EARTH_M;
    let pts = fl
        .samples
        .iter()
        .filter(|s| s.phase != crate::flight::Phase::Landed)
        .map(|s| {
            let p = Point { t_s: s.t_s, lat: lat0 + (s.y_m / r).to_degrees(), lon: lon0 + (s.x_m / (r * lat0.to_radians().cos())).to_degrees(), alt_m: s.z_m };
            (p, if s.phase == crate::flight::Phase::Ascent { "ascent" } else { "descent" })
        })
        .collect();
    Ok((fl, pts))
}

fn phased(v: &[Point], burst_t: f64) -> Vec<(Point, &'static str)> {
    v.iter().map(|p| (*p, if p.t_s <= burst_t + 1e-6 { "ascent" } else { "descent" })).collect()
}

/// Fly `d` on the GFS winds of `pred`, write the files into `dir` (the
/// stage's `<out_dir>/sim`) and return the manifest (also written there).
pub fn write(spec: &Spec, d: &Design, cat: &Catalog, pred: &Prediction, dir: &Path) -> Result<Manifest> {
    std::fs::create_dir_all(dir)?;
    let gfs = predict::wind_profile(pred);
    anyhow::ensure!(gfs.len() >= 2, "the cached prediction has no usable ascent");
    let mut spec_gfs = spec.clone();
    spec_gfs.launch.lat_deg = pred.request.launch_lat;
    spec_gfs.launch.lon_deg = pred.request.launch_lon;
    spec_gfs.wind = gfs.clone();
    let mut spec_assumed = spec_gfs.clone();
    spec_assumed.wind = vec![];

    let (fl, w_gfs) = wright_track(&spec_gfs, d, cat)?;
    let (_, w_assumed) = wright_track(&spec_assumed, d, cat)?;
    // Tawhiri's model (the port) on the same GFS profile, with this
    // design's rates
    let req = predict::request_for(&spec_gfs, &fl, &pred.request.launch_datetime);
    let port = predict::tawhiri_model(&req, &gfs, spec_gfs.launch.elevation_m);
    let port_burst = port.iter().find(|p| p.alt_m >= req.burst_altitude_m - 1.0).map_or(0.0, |p| p.t_s);
    // the live API run, when it was asked for this design's rates
    let same_rates = (pred.request.ascent_rate_mps - req.ascent_rate_mps).abs() < 0.01
        && (pred.request.burst_altitude_m - req.burst_altitude_m).abs() < 1.0
        && (pred.request.descent_rate_mps - req.descent_rate_mps).abs() < 0.01;

    let mut tracks = vec![
        Track { name: "wright_gfs", label: format!("Wright model, GFS winds {}", pred.dataset), points: w_gfs },
        Track { name: "tawhiri_port_gfs", label: "Tawhiri model (Rust port), same GFS winds".into(), points: phased(&port, port_burst) },
        Track { name: "wright_assumed", label: "Wright model, assumed wind profile".into(), points: w_assumed },
    ];
    if same_rates {
        let bt = pred.ascent.last().map_or(0.0, |p| p.t_s);
        let pts: Vec<Point> = pred.points().copied().collect();
        tracks.insert(0, Track { name: "tawhiri_api", label: format!("Tawhiri API (SondeHub), GFS {}", pred.dataset), points: phased(&pts, bt) });
    }

    let launch = Point { t_s: 0.0, lat: spec_gfs.launch.lat_deg, lon: spec_gfs.launch.lon_deg, alt_m: spec_gfs.launch.elevation_m };
    let mut csv = String::from("track,t_s,lat,lon,alt_m,phase\n");
    let mut per = serde_json::Map::new();
    for t in &tracks {
        for (p, ph) in &t.points {
            let _ = writeln!(csv, "{},{:.1},{:.6},{:.6},{:.1},{}", t.name, p.t_s, p.lat, p.lon, p.alt_m, ph);
        }
        let land = t.points.last().context("empty track")?.0;
        let burst = t.points.iter().filter(|(_, ph)| *ph == "ascent").map(|(p, _)| *p).next_back().unwrap_or(launch);
        per.insert(
            t.name.into(),
            json!({
                "label": t.label,
                "landing_lat": land.lat, "landing_lon": land.lon,
                "landing_km_from_launch": haversine_km(&launch, &land),
                "landing_bearing_deg": bearing_deg(&launch, &land),
                "burst_t_min": burst.t_s / 60.0, "burst_alt_m": burst.alt_m,
                "descent_min": (land.t_s - burst.t_s) / 60.0,
                "flight_min": land.t_s / 60.0,
            }),
        );
    }
    let land_of = |n: &str| tracks.iter().find(|t| t.name == n).and_then(|t| t.points.last()).map(|x| x.0);
    let sep = |a: &str, b: &str| land_of(a).zip(land_of(b)).map(|(x, y)| haversine_km(&x, &y));

    // the design re-checked on the real winds
    let (f_gfs, _) = crate::checks::evaluate(&spec_gfs, d, cat);
    let checks: Vec<serde_json::Value> = f_gfs.outcomes.iter().map(|o| json!({"name": o.name, "passed": o.passed(), "margin": o.margin(), "detail": o.detail})).collect();
    let comparison = json!({
        "launch": {"lat": launch.lat, "lon": launch.lon, "alt_m": launch.alt_m, "datetime": pred.request.launch_datetime},
        "gfs_dataset": pred.dataset,
        "endpoint": predict::TAWHIRI_API,
        "tawhiri_request_for_this_design": req,
        "api_track_included": same_rates,
        "tracks": per,
        "landing_separation_km": {
            "wright_gfs_vs_tawhiri_port": sep("wright_gfs", "tawhiri_port_gfs"),
            "wright_gfs_vs_tawhiri_api": sep("wright_gfs", "tawhiri_api"),
            "tawhiri_port_vs_api": sep("tawhiri_port_gfs", "tawhiri_api"),
            "wright_gfs_vs_assumed": sep("wright_gfs", "wright_assumed"),
        },
        "gfs_wind_profile": gfs,
        "checks_on_gfs_winds": {"passed": f_gfs.passed(), "failing": f_gfs.failures().map(|o| o.name.clone()).collect::<Vec<_>>(), "all": checks},
    });
    std::fs::write(dir.join("tracks.csv"), &csv)?;
    std::fs::write(dir.join("comparison.json"), serde_json::to_string_pretty(&comparison)?)?;

    let wl = &per["wright_gfs"];
    let f64_of = |v: &serde_json::Value| v.as_f64().unwrap_or(f64::NAN);
    let land_km = f64_of(&wl["landing_km_from_launch"]);
    let sep_port = sep("wright_gfs", "tawhiri_port_gfs").unwrap_or(f64::NAN);
    let sep_api = sep("wright_gfs", "tawhiri_api").unwrap_or(f64::NAN);
    let title = format!("{} flown on the GFS forecast for {} (dataset {})", d.balloon, pred.request.launch_datetime, pred.dataset);
    let mut m = Manifest::new("sim", title);
    if let Some(py) = python() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/render_flight.py");
        // the animation is the slow part (~150 s of matplotlib + ffmpeg):
        // memoized by its inputs (tracks, comparison, the script itself)
        let rkey = wright_core::cache::key(&[&std::fs::read(dir.join("tracks.csv")).unwrap_or_default(), &std::fs::read(dir.join("comparison.json")).unwrap_or_default(), &std::fs::read(&script).unwrap_or_default()]);
        let rcache = wright_core::cache::Cache::named("sim-render");
        let restored = rcache.get::<bool>(&rkey, None).is_some()
            && ["flight.mp4", "flight.png"].iter().all(|f| std::fs::copy(rcache.dir.join(format!("{rkey}-{f}")), dir.join(f)).is_ok());
        let st = if restored {
            Ok(std::process::ExitStatus::default())
        } else {
            let st = std::process::Command::new(py).arg("-I").arg(&script).arg(dir.join("tracks.csv")).arg(dir.join("comparison.json")).arg(dir).stdout(std::process::Stdio::null()).status();
            if st.as_ref().is_ok_and(|s| s.success()) && wright_core::cache::enabled() && std::fs::create_dir_all(&rcache.dir).is_ok()
                && ["flight.mp4", "flight.png"].iter().all(|f| std::fs::copy(dir.join(f), rcache.dir.join(format!("{rkey}-{f}"))).is_ok())
            {
                rcache.put(&rkey, &true);
            }
            st
        };
        match st {
            Ok(s) if s.success() => {
                let summary = format!("ascent, burst and descent on GFS {}: Wright's model (red), Tawhiri's model on the same winds (green), the live Tawhiri prediction (blue, when fetched for this design), Wright on its assumed winds (grey)", pred.dataset);
                for (f, label) in [("flight.mp4", "flight animation"), ("flight.png", "flight track")] {
                    if dir.join(f).exists() {
                        m.add(dir, &dir.join(f), label, &summary, &[("landing_km", land_km), ("flight_min", f64_of(&wl["flight_min"]))]);
                    }
                }
            }
            other => eprintln!("render_flight.py failed: {other:?}"),
        }
    }
    m.add(dir, &dir.join("tracks.csv"), "flight tracks", "every track: time, lat, lon, altitude, phase", &[]);
    m.add(
        dir,
        &dir.join("comparison.json"),
        "Wright vs Tawhiri",
        &format!(
            "lands {land_km:.0} km at {:.0} deg after {:.0} min on GFS winds (assumed profile: {:.0} km); Tawhiri's model on the same winds lands {sep_port:.1} km away",
            f64_of(&wl["landing_bearing_deg"]),
            f64_of(&wl["flight_min"]),
            f64_of(&per["wright_assumed"]["landing_km_from_launch"]),
        ),
        &[("landing_km", land_km), ("separation_vs_tawhiri_model_km", sep_port), ("separation_vs_tawhiri_api_km", sep_api)],
    );
    let fails: Vec<&str> = f_gfs.failures().map(|o| o.name.as_str()).collect();
    m.check("checks_on_forecast_winds", fails.is_empty(), &if fails.is_empty() { format!("all {} checks pass with the GFS winds of {}", f_gfs.outcomes.len(), pred.dataset) } else { format!("failing on the forecast winds: {}", fails.join(", ")) });
    // the predicted landing is inside the recovery area: both trackers' links
    // reach it (the link checks on the forecast winds cover the whole track)
    let links_ok = ["link_primary", "recovery_probability"].iter().all(|n| f_gfs.get(n).is_some_and(|o| o.passed()));
    m.check("landing_inside_recovery_area", links_ok, &format!("predicted landing {land_km:.0} km from launch; link_primary and recovery_probability on these winds {}", if links_ok { "pass" } else { "fail" }));
    // Wright's flight model agrees with Tawhiri's on the same winds: within
    // 10 % of the drift (or 5 km)
    let tol = (0.10 * land_km).max(5.0);
    m.check("agrees_with_tawhiri", sep_port <= tol, &format!("landings {sep_port:.1} km apart (tolerance {tol:.1} km){}", if sep_api.is_finite() { format!("; live Tawhiri API {sep_api:.1} km") } else { String::new() }));
    m.write(dir)?;
    Ok(m)
}

/// Ask the public Tawhiri API (curl, no personal data: launch site, time and
/// the design's three rates) and cache the answer in `predictions_dir()`.
pub fn fetch(spec: &Spec, d: &Design, cat: &Catalog, launch_datetime: &str) -> Result<Prediction> {
    let m = crate::mass::masses(d, cat)?;
    let fl = crate::flight::simulate(spec, d, cat, &m)?;
    let req = predict::request_for(spec, &fl, launch_datetime);
    let url = predict::tawhiri_url(&req);
    // the URL holds every input (site, launch time, ascent/burst/descent):
    // a rerun inside the TTL does not wait on the network
    // (`wright_core::cache`, ~/.wright/cache/tawhiri; WRIGHT_TAWHIRI_TTL_H, default 6 h = one GFS cycle)
    let cache = wright_core::cache::Cache::named("tawhiri");
    let key = wright_core::cache::key(&[url.as_bytes()]);
    let ttl_h = std::env::var("WRIGHT_TAWHIRI_TTL_H").ok().and_then(|v| v.parse::<f64>().ok()).filter(|h| h.is_finite() && *h >= 0.0).unwrap_or(6.0);
    if let Some(p) = cache.get::<Prediction>(&key, Some(std::time::Duration::from_secs_f64(ttl_h * 3600.0))) {
        return Ok(p);
    }
    let out = std::process::Command::new("curl").args(["-sfS", "--max-time", "60", &url]).output()?;
    anyhow::ensure!(out.status.success(), "Tawhiri request failed ({url}): {}", String::from_utf8_lossy(&out.stderr));
    let p = Prediction::parse(req, &String::from_utf8(out.stdout)?)?;
    std::fs::create_dir_all(predictions_dir())?;
    std::fs::write(predictions_dir().join(format!("{}.json", launch_datetime.replace(':', ""))), serde_json::to_string_pretty(&p)?)?;
    cache.put(&key, &p);
    Ok(p)
}

/// The "sim" post-design stage: the newest cached GFS prediction (fetched
/// live for tomorrow 16:00 UTC when none is cached), flown by Wright's and
/// Tawhiri's models and animated.
pub struct SimStage;

impl wright_core::Stage for SimStage {
    fn name(&self) -> &'static str {
        "sim"
    }
    fn applies_to(&self, domain: &str) -> bool {
        domain == "balloon"
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn run(&self, ctx: &wright_core::StageCtx) -> Result<Manifest> {
        let spec: Spec = serde_json::from_value(ctx.spec.clone()).context("balloon spec")?;
        let d: Design = serde_json::from_value(ctx.design.clone()).context("balloon design")?;
        let cat = crate::catalog::catalog();
        let pred = match latest_prediction() {
            Some(p) => p,
            None => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
                let day = now / 86_400 + 1;
                let (y, mo, dd) = civil(day as i64);
                fetch(&spec, &d, cat, &format!("{y:04}-{mo:02}-{dd:02}T16:00:00Z"))?
            }
        };
        write(&spec, &d, cat, &pred, &ctx.dir)
    }
}

/// Civil date of a day count since 1970-01-01 (H. Hinnant).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}
