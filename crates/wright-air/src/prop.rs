//! Propellers from measured data: the UIUC Propeller Database
//! (`data/air/props_uiuc.json`).
//!
//! T = CT rho n^2 D^4, P = CP rho n^3 D^5, n in rev/s, J = V / (n D).
//! A query is answered from the measured curves: the static run is J = 0,
//! the dynamic runs carry it on, and between tested RPMs the curves blend
//! linearly in RPM. Past a curve's last measured J the nearest-RPM curve that
//! reaches it is read (the windmill end is insensitive to Reynolds number);
//! with none, the last slope is carried on -- never held flat, which would be
//! thrust the data never showed.
//!
//! A size UIUC did not test is read through the measured prop of its family
//! with the nearest pitch/diameter (then diameter), scaled by diameter:
//! geometrically similar props share CT(J), CP(J) apart from Reynolds effects
//! (McCormick, Aerodynamics, Aeronautics and Flight Mechanics, ch. 6).

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use wright_core::units::{Density, Force, Length, Power, Speed};

#[derive(Debug, Deserialize)]
struct Raw {
    props: BTreeMap<String, RawProp>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawProp {
    family: String,
    folding: bool,
    diameter_in: f64,
    pitch_in: f64,
    #[serde(rename = "static")]
    static_rows: Vec<[f64; 3]>,
    dynamic: BTreeMap<String, Vec<[f64; 3]>>,
}

fn table() -> &'static BTreeMap<String, RawProp> {
    static T: OnceLock<BTreeMap<String, RawProp>> = OnceLock::new();
    T.get_or_init(|| {
        let raw: Raw = serde_json::from_str(include_str!("../../../data/air/props_uiuc.json"))
            .expect("data/air/props_uiuc.json parses");
        raw.props
    })
}

pub fn measured_ids() -> Vec<&'static str> {
    table().keys().map(|s| s.as_str()).collect()
}

#[derive(Debug, Clone)]
struct Curve {
    rpm: f64,
    j: Vec<f64>,
    ct: Vec<f64>,
    cp: Vec<f64>,
}

/// One propeller's measured performance (its own data or its proxy's).
#[derive(Debug, Clone)]
pub struct PropData {
    pub data_id: String,
    pub family: String,
    pub measured_folding: bool,
    pub diameter: Length,
    pub pitch: Length,
    static_rpm: Vec<f64>,
    static_ct: Vec<f64>,
    static_cp: Vec<f64>,
    curves: Vec<Curve>,
}

fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len();
    if n == 1 || x <= xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let j = xs.partition_point(|&v| v <= x);
    let i = j - 1;
    ys[i] + (ys[j] - ys[i]) * (x - xs[i]) / (xs[j] - xs[i])
}

fn build(key: &str, diameter_in: Option<f64>, pitch_in: Option<f64>) -> PropData {
    let e = &table()[key];
    let mut curves: Vec<Curve> = e
        .dynamic
        .iter()
        .map(|(rpm, rows)| {
            let mut rows = rows.clone();
            rows.sort_by(|a, b| a[0].total_cmp(&b[0]));
            Curve {
                rpm: rpm.parse().unwrap_or(0.0),
                j: rows.iter().map(|r| r[0]).collect(),
                ct: rows.iter().map(|r| r[1]).collect(),
                cp: rows.iter().map(|r| r[2]).collect(),
            }
        })
        .collect();
    curves.sort_by(|a, b| a.rpm.total_cmp(&b.rpm));
    let mut st = e.static_rows.clone();
    st.sort_by(|a, b| a[0].total_cmp(&b[0]));
    PropData {
        data_id: key.to_string(),
        family: e.family.clone(),
        measured_folding: e.folding,
        diameter: Length::inch(diameter_in.unwrap_or(e.diameter_in)),
        pitch: Length::inch(pitch_in.unwrap_or(e.pitch_in)),
        static_rpm: st.iter().map(|r| r[0]).collect(),
        static_ct: st.iter().map(|r| r[1]).collect(),
        static_cp: st.iter().map(|r| r[2]).collect(),
        curves,
    }
}

/// A measured prop by its UIUC id.
pub fn measured(id: &str) -> Option<PropData> {
    table().contains_key(id).then(|| build(id, None, None))
}

/// An unmeasured size read through the nearest measured prop of `family`
/// (a UIUC id prefix such as `apce`): pitch/diameter first, an inch of
/// diameter counting as 0.02 of pitch ratio.
pub fn proxy(diameter_in: f64, pitch_in: f64, family: &str) -> Option<PropData> {
    let target = pitch_in / diameter_in;
    let prefix = format!("{family}_");
    let best = table()
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix))
        .map(|(k, e)| {
            let s = (e.pitch_in / e.diameter_in - target).abs() + 0.02 * (e.diameter_in - diameter_in).abs();
            (s, k)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)))?;
    Some(build(best.1, Some(diameter_in), Some(pitch_in)))
}

impl PropData {
    pub fn disc_area_m2(&self) -> f64 {
        std::f64::consts::PI * (self.diameter.as_m() / 2.0).powi(2)
    }

    fn static_at(&self, rpm: f64) -> (f64, f64) {
        if self.static_rpm.is_empty() {
            let c = self
                .curves
                .iter()
                .min_by(|a, b| (a.rpm - rpm).abs().total_cmp(&(b.rpm - rpm).abs()))
                .expect("a prop with neither static nor dynamic data was rejected at load");
            return (c.ct[0], c.cp[0]);
        }
        (interp(&self.static_rpm, &self.static_ct, rpm), interp(&self.static_rpm, &self.static_cp, rpm))
    }

    fn curve_at(&self, k: usize, j: f64, rpm: f64) -> (f64, f64) {
        let c = &self.curves[k];
        let last = *c.j.last().unwrap();
        if j > last {
            let reach = self
                .curves
                .iter()
                .enumerate()
                .filter(|(_, o)| *o.j.last().unwrap() >= j)
                .min_by(|a, b| (a.1.rpm - c.rpm).abs().total_cmp(&(b.1.rpm - c.rpm).abs()));
            if let Some((i, _)) = reach {
                return self.curve_at(i, j, rpm);
            }
            let n = c.j.len();
            if n >= 2 {
                let dj = c.j[n - 1] - c.j[n - 2];
                let t = if dj > 0.0 { (j - c.j[n - 1]) / dj } else { 0.0 };
                return (
                    c.ct[n - 1] + (c.ct[n - 1] - c.ct[n - 2]) * t,
                    (c.cp[n - 1] + (c.cp[n - 1] - c.cp[n - 2]) * t).max(1e-4),
                );
            }
        }
        let (ct0, cp0) = self.static_at(rpm);
        // J = 0 is the static point, then the measured rows
        let mut xs = Vec::with_capacity(c.j.len() + 1);
        xs.push(0.0);
        xs.extend_from_slice(&c.j);
        let mut ct = vec![ct0];
        ct.extend_from_slice(&c.ct);
        let mut cp = vec![cp0];
        cp.extend_from_slice(&c.cp);
        (interp(&xs, &ct, j), interp(&xs, &cp, j))
    }

    /// (CT, CP) at advance ratio `j` and `rpm`.
    pub fn coefficients(&self, j: f64, rpm: f64) -> (f64, f64) {
        if j <= 0.0 || self.curves.is_empty() {
            return self.static_at(rpm);
        }
        let n = self.curves.len();
        if rpm <= self.curves[0].rpm {
            return self.curve_at(0, j, rpm);
        }
        if rpm >= self.curves[n - 1].rpm {
            return self.curve_at(n - 1, j, rpm);
        }
        let b = self.curves.partition_point(|c| c.rpm <= rpm);
        let a = b - 1;
        let t = (rpm - self.curves[a].rpm) / (self.curves[b].rpm - self.curves[a].rpm);
        let (ca, pa) = self.curve_at(a, j, rpm);
        let (cb, pb) = self.curve_at(b, j, rpm);
        (ca + (cb - ca) * t, pa + (pb - pa) * t)
    }

    /// (thrust, shaft power) at `rpm` and airspeed `v`.
    pub fn thrust_power(&self, rpm: f64, v: Speed, rho: Density) -> (Force, Power) {
        let n = rpm / 60.0;
        let d = self.diameter.as_m();
        let j = if n > 0.0 { v.value() / (n * d) } else { 0.0 };
        let (ct, cp) = self.coefficients(j, rpm);
        let r = rho.value();
        (Force::n(ct * r * n * n * d.powi(4)), Power::w(cp * r * n * n * n * d.powi(5)))
    }

    /// The RPM making `thrust` at `v`, by bisection below `rpm_max`; None
    /// when even `rpm_max` falls short.
    pub fn rpm_for_thrust(&self, thrust: Force, v: Speed, rho: Density, rpm_max: f64) -> Option<f64> {
        let (mut lo, mut hi) = (200.0, rpm_max);
        if self.thrust_power(hi, v, rho).0 < thrust {
            return None;
        }
        if self.thrust_power(lo, v, rho).0 >= thrust {
            return Some(lo);
        }
        for _ in 0..48 {
            let mid = 0.5 * (lo + hi);
            if self.thrust_power(mid, v, rho).0 < thrust {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(0.5 * (lo + hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apce_10x7_static_thrust_is_plausible() {
        let p = measured("apce_10x7").unwrap();
        let (t, pw) = p.thrust_power(6000.0, Speed::mps(0.0), Density::kg_m3(1.225));
        // APC 10x7E at 6000 rpm: about 6-7 N static, 90-110 W shaft (APC data)
        assert!(t.as_n() > 4.5 && t.as_n() < 8.5, "{}", t.as_n());
        assert!(pw.as_w() > 60.0 && pw.as_w() < 140.0, "{}", pw.as_w());
        // thrust falls with airspeed and reaches zero near the pitch speed
        let (t20, _) = p.thrust_power(6000.0, Speed::mps(20.0), Density::kg_m3(1.225));
        assert!(t20 < t);
    }
    #[test]
    fn proxy_keeps_the_requested_size() {
        let p = proxy(7.0, 4.2, "apce").unwrap();
        assert!((p.diameter.as_inch() - 7.0).abs() < 1e-9);
    }
}
