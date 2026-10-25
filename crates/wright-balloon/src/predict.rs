//! Flight prediction on real (GFS) winds with CUSF's Tawhiri, the predictor
//! behind predict.sondehub.org (github.com/cuspaceflight/tawhiri, GPL-3).
//!
//! Three pieces, each following Tawhiri's source at commit 668b44e:
//! * [`tawhiri_url`] / [`Prediction::parse`]: the public API
//!   (`https://api.v2.sondehub.org/tawhiri`, `standard_profile`; parameters
//!   per tawhiri.readthedocs.io/en/latest/api.html). The caller fetches.
//! * [`wind_profile`]: the GFS wind Tawhiri flew the ascent through,
//!   recovered by inverting `tawhiri/models.py::make_wind_velocity`
//!   (`dlat = 180/pi * v / R`, `dlng = 180/pi * u / (R cos lat)`,
//!   `R = 6371009 + alt`). The ascent model is wind + constant climb, so the
//!   lateral motion between two ascent points is the wind there.
//! * [`tawhiri_model`]: a port of `models.py::standard_profile`
//!   (`make_constant_ascent`, `make_drag_descent` with its NASA Glenn
//!   atmosphere and `drag_coefficient = sea_level_descent_rate * 1.1045`)
//!   integrated by `solver.pyx::rk4` (dt 60 s, termination bisected to 1 %).
//!   It runs offline on a wind profile, so Wright's own flight model
//!   (`flight::simulate`) can be compared with Tawhiri's on the same winds.

use crate::atmosphere;
use crate::model::{Spec, WindPoint};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use wright_core::units::Length;

/// The public Tawhiri endpoint (SondeHub). No personal data is sent: the
/// launch coordinates, time and the three rates.
pub const TAWHIRI_API: &str = "https://api.v2.sondehub.org/tawhiri";
/// `models.py::make_wind_velocity`: Earth radius used by Tawhiri, m.
pub const TAWHIRI_R_EARTH_M: f64 = 6_371_009.0;
/// `solver.pyx::rk4` default step, s.
pub const TAWHIRI_DT_S: f64 = 60.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Point {
    /// Seconds after launch.
    pub t_s: f64,
    pub lat: f64,
    /// Degrees east, -180..180.
    pub lon: f64,
    pub alt_m: f64,
}

/// What Tawhiri was asked: the flight Wright's model predicts, reduced to
/// Tawhiri's three numbers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub launch_lat: f64,
    pub launch_lon: f64,
    pub launch_alt_m: f64,
    /// RFC 3339, UTC.
    pub launch_datetime: String,
    pub ascent_rate_mps: f64,
    pub burst_altitude_m: f64,
    /// Descent rate at sea level, m/s (Tawhiri scales it by 1/sqrt(rho)).
    pub descent_rate_mps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub request: Request,
    /// The GFS run Tawhiri used (`request.dataset`).
    pub dataset: String,
    pub ascent: Vec<Point>,
    pub descent: Vec<Point>,
}

/// The API URL for `r` (Tawhiri wants longitude 0..360).
pub fn tawhiri_url(r: &Request) -> String {
    format!(
        "{TAWHIRI_API}?profile=standard_profile&launch_latitude={:.5}&launch_longitude={:.5}&launch_altitude={:.0}&launch_datetime={}&ascent_rate={:.3}&burst_altitude={:.0}&descent_rate={:.3}",
        r.launch_lat,
        r.launch_lon.rem_euclid(360.0),
        r.launch_alt_m,
        r.launch_datetime,
        r.ascent_rate_mps,
        r.burst_altitude_m,
        r.descent_rate_mps
    )
}

/// Seconds since the Unix epoch of an RFC 3339 UTC time
/// (`YYYY-MM-DDTHH:MM:SS[.fff]Z`), without a date crate.
pub fn unix_s(s: &str) -> Result<f64> {
    let b = s.trim_end_matches('Z');
    let (date, time) = b.split_once('T').context("no T in datetime")?;
    let d: Vec<i64> = date.split('-').map(|x| x.parse()).collect::<std::result::Result<_, _>>()?;
    let t: Vec<f64> = time.split(':').map(|x| x.parse()).collect::<std::result::Result<_, _>>()?;
    if d.len() != 3 || t.len() != 3 {
        bail!("bad datetime {s}");
    }
    // days from civil (H. Hinnant's algorithm)
    let (y, m, dd) = (if d[1] <= 2 { d[0] - 1 } else { d[0] }, d[1], d[2]);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + dd - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Ok(days as f64 * 86_400.0 + t[0] * 3600.0 + t[1] * 60.0 + t[2])
}

impl Prediction {
    /// Parse the API's JSON (`prediction[stage].trajectory`).
    pub fn parse(request: Request, json: &str) -> Result<Prediction> {
        let v: serde_json::Value = serde_json::from_str(json)?;
        if let Some(e) = v.get("error") {
            bail!("Tawhiri error: {e}");
        }
        let t0 = unix_s(&request.launch_datetime)?;
        let stage = |name: &str| -> Result<Vec<Point>> {
            let st = v["prediction"].as_array().context("no prediction")?.iter().find(|s| s["stage"] == name).with_context(|| format!("no {name} stage"))?;
            st["trajectory"]
                .as_array()
                .context("no trajectory")?
                .iter()
                .map(|p| {
                    let lon = p["longitude"].as_f64().context("longitude")?;
                    Ok(Point {
                        t_s: unix_s(p["datetime"].as_str().context("datetime")?)? - t0,
                        lat: p["latitude"].as_f64().context("latitude")?,
                        lon: if lon > 180.0 { lon - 360.0 } else { lon },
                        alt_m: p["altitude"].as_f64().context("altitude")?,
                    })
                })
                .collect()
        };
        let dataset = v["request"]["dataset"].as_str().unwrap_or("?").to_string();
        Ok(Prediction { request, dataset, ascent: stage("ascent")?, descent: stage("descent")? })
    }

    pub fn landing(&self) -> Point {
        *self.descent.last().unwrap_or(&self.ascent[self.ascent.len() - 1])
    }

    pub fn points(&self) -> impl Iterator<Item = &Point> {
        self.ascent.iter().chain(self.descent.iter().skip(1))
    }
}

/// East and north of (`lat0`, `lon0`), m, on Tawhiri's sphere.
pub fn local_xy(lat0: f64, lon0: f64, p: &Point) -> (f64, f64) {
    let r = TAWHIRI_R_EARTH_M;
    let x = (p.lon - lon0).to_radians() * r * lat0.to_radians().cos();
    let y = (p.lat - lat0).to_radians() * r;
    (x, y)
}

/// The GFS wind profile Tawhiri flew the ascent through, inverted from its
/// own wind model (see the module docs). One point per ascent step,
/// strictly increasing in altitude.
pub fn wind_profile(p: &Prediction) -> Vec<WindPoint> {
    let mut out: Vec<WindPoint> = Vec::new();
    for w in p.ascent.windows(2) {
        let (a, b) = (w[0], w[1]);
        let dt = b.t_s - a.t_s;
        if dt <= 1.0 {
            continue;
        }
        let alt = 0.5 * (a.alt_m + b.alt_m);
        let lat = 0.5 * (a.lat + b.lat);
        let r = TAWHIRI_R_EARTH_M + alt;
        let v = (b.lat - a.lat).to_radians() * r / dt;
        let mut dlon = b.lon - a.lon;
        if dlon > 180.0 {
            dlon -= 360.0;
        } else if dlon < -180.0 {
            dlon += 360.0;
        }
        let u = dlon.to_radians() * r * lat.to_radians().cos() / dt;
        let speed = (u * u + v * v).sqrt();
        let toward = u.atan2(v).to_degrees().rem_euclid(360.0);
        if out.last().is_none_or(|l| alt > l.alt_m) {
            out.push(WindPoint { alt_m: alt, speed_mps: speed, toward_deg: toward });
        }
    }
    out
}

/// (u east, v north) m/s at `z` on a profile: linear in the components
/// (not in speed and bearing, so a wind that turns through north does not
/// spin the long way round).
pub fn wind_uv(profile: &[WindPoint], z: f64) -> (f64, f64) {
    let uv = |w: &WindPoint| {
        let r = w.toward_deg.to_radians();
        (w.speed_mps * r.sin(), w.speed_mps * r.cos())
    };
    if profile.is_empty() {
        return (0.0, 0.0);
    }
    if z <= profile[0].alt_m {
        return uv(&profile[0]);
    }
    let last = &profile[profile.len() - 1];
    if z >= last.alt_m {
        return uv(last);
    }
    let i = profile.windows(2).position(|w| z >= w[0].alt_m && z <= w[1].alt_m).unwrap_or(0);
    let (a, b) = (uv(&profile[i]), uv(&profile[i + 1]));
    let f = (z - profile[i].alt_m) / (profile[i + 1].alt_m - profile[i].alt_m);
    (a.0 + f * (b.0 - a.0), a.1 + f * (b.1 - a.1))
}

/// `models.py::make_drag_descent::density`: the NASA Glenn atmosphere
/// (grc.nasa.gov/WWW/K-12/airplane/atmosmet.html), kg/m^3.
pub fn nasa_density(alt: f64) -> f64 {
    let (temp, pressure) = if alt > 25_000.0 {
        let t = -131.21 + 0.00299 * alt;
        (t, 2.488 * ((t + 273.1) / 216.6).powf(-11.388))
    } else if alt > 11_000.0 {
        (-56.46, 22.65 * (1.73 - 0.000157 * alt).exp())
    } else {
        let t = 15.04 - 0.00649 * alt;
        (t, 101.29 * ((t + 273.1) / 288.08).powf(5.256))
    };
    pressure / (0.2869 * (temp + 273.1))
}

/// Tawhiri's `standard_profile` (constant ascent, then drag descent, both
/// plus the wind) integrated by its RK4, on `wind` instead of the GFS grid.
/// Terminates at `ground_m` (Tawhiri uses its elevation dataset).
pub fn tawhiri_model(r: &Request, wind: &[WindPoint], ground_m: f64) -> Vec<Point> {
    let drag_coefficient = r.descent_rate_mps * 1.1045;
    let f = |up: bool, y: [f64; 3]| -> [f64; 3] {
        let (u, v) = wind_uv(wind, y[2]);
        let rr = TAWHIRI_R_EARTH_M + y[2];
        let dlat = v / rr * 180.0 / std::f64::consts::PI;
        let dlng = u / (rr * y[0].to_radians().cos()) * 180.0 / std::f64::consts::PI;
        let dalt = if up { r.ascent_rate_mps } else { -drag_coefficient / nasa_density(y[2]).sqrt() };
        [dlat, dlng, dalt]
    };
    let add = |a: [f64; 3], k: f64, b: [f64; 3]| [a[0] + k * b[0], a[1] + k * b[1], a[2] + k * b[2]];
    let mut out = vec![Point { t_s: 0.0, lat: r.launch_lat, lon: r.launch_lon, alt_m: r.launch_alt_m }];
    let (mut t, mut y) = (0.0, [r.launch_lat, r.launch_lon, r.launch_alt_m]);
    for up in [true, false] {
        let done = |y: [f64; 3]| if up { y[2] >= r.burst_altitude_m } else { y[2] <= ground_m };
        let dt = TAWHIRI_DT_S;
        loop {
            let k1 = f(up, y);
            let k2 = f(up, add(y, dt / 2.0, k1));
            let k3 = f(up, add(y, dt / 2.0, k2));
            let k4 = f(up, add(y, dt, k3));
            let mut y2 = add(y, dt / 6.0, k1);
            y2 = add(y2, dt / 3.0, k2);
            y2 = add(y2, dt / 3.0, k3);
            y2 = add(y2, dt / 6.0, k4);
            if done(y2) {
                // solver.pyx: bisect the step to 1 % for the crossing
                let (mut lo, mut hi) = (0.0, 1.0);
                let mut ym = y2;
                let mut tm = t + dt;
                while hi - lo > 0.01 {
                    let mid = 0.5 * (lo + hi);
                    ym = [y[0] + mid * (y2[0] - y[0]), y[1] + mid * (y2[1] - y[1]), y[2] + mid * (y2[2] - y[2])];
                    tm = t + mid * dt;
                    if done(ym) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                t = tm;
                y = ym;
                out.push(Point { t_s: t, lat: y[0], lon: y[1], alt_m: y[2] });
                break;
            }
            t += dt;
            y = y2;
            out.push(Point { t_s: t, lat: y[0], lon: y[1], alt_m: y[2] });
            if t > 12.0 * 3600.0 {
                break;
            }
        }
    }
    out
}

/// Tawhiri's request for the flight Wright's model predicts: mean ascent
/// rate, burst altitude, and the chute's descent rate at sea level (the
/// rate Wright computes at the launch elevation, scaled to US76 sea-level
/// density as Tawhiri's 1/sqrt(rho) model expects).
pub fn request_for(spec: &Spec, fl: &crate::flight::Flight, launch_datetime: &str) -> Request {
    let rho0 = atmosphere::at(Length::m(0.0)).density.as_kg_m3();
    let rho_l = atmosphere::at(Length::m(spec.launch.elevation_m)).density.as_kg_m3();
    Request {
        launch_lat: spec.launch.lat_deg,
        launch_lon: spec.launch.lon_deg,
        launch_alt_m: spec.launch.elevation_m,
        launch_datetime: launch_datetime.to_string(),
        ascent_rate_mps: fl.mean_ascent_mps,
        burst_altitude_m: fl.burst_altitude_m.unwrap_or(0.0),
        descent_rate_mps: fl.landing_speed_mps * (rho_l / rho0).sqrt(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_time_matches_known_epochs() {
        assert_eq!(unix_s("1970-01-01T00:00:00Z").unwrap(), 0.0);
        assert_eq!(unix_s("2026-10-25T16:00:00Z").unwrap(), 1_792_944_000.0);
    }

    #[test]
    fn nasa_density_is_sea_level_at_zero() {
        // Tawhiri's 1.1045 is sqrt of this density
        assert!((nasa_density(0.0).sqrt() - 1.1045).abs() < 0.005, "{}", nasa_density(0.0));
    }

    #[test]
    fn wind_round_trips_through_tawhiri_model() {
        // fly the port on a known profile, recover it from the ascent leg
        let wind = vec![
            WindPoint { alt_m: 0.0, speed_mps: 5.0, toward_deg: 90.0 },
            WindPoint { alt_m: 12_000.0, speed_mps: 30.0, toward_deg: 60.0 },
            WindPoint { alt_m: 30_000.0, speed_mps: 8.0, toward_deg: 270.0 },
        ];
        let r = Request { launch_lat: 35.6, launch_lon: -119.7, launch_alt_m: 80.0, launch_datetime: "2026-10-25T16:00:00Z".into(), ascent_rate_mps: 5.0, burst_altitude_m: 30_000.0, descent_rate_mps: 5.0 };
        let pts = tawhiri_model(&r, &wind, 80.0);
        let k = pts.iter().position(|p| p.alt_m >= 29_900.0).unwrap();
        let p = Prediction { request: r, dataset: "test".into(), ascent: pts[..=k].to_vec(), descent: pts[k..].to_vec() };
        let back = wind_profile(&p);
        let (u, v) = wind_uv(&back, 12_000.0);
        let (u0, v0) = wind_uv(&wind, 12_000.0);
        assert!((u - u0).abs() < 1.0 && (v - v0).abs() < 1.0, "({u},{v}) vs ({u0},{v0})");
    }
}
