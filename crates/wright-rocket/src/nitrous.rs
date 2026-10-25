//! Self-pressurised nitrous oxide: saturation properties, the run-tank
//! blowdown, and two-phase injector flow.
//!
//! Real student N2O engines do not regulate the oxidiser: liquid N2O sits on
//! its own vapour (~50 bar at 20 C) and the tank cools and loses pressure as
//! liquid boils to refill the ullage. Wright models that the way the
//! self-pressurising-tank literature does:
//!
//! * properties: the NIST WebBook saturation table (Lemmon & Span EOS),
//!   `data/rocket/n2o_saturation.json`, interpolated linearly in T;
//! * tank: the single-node *equilibrium* model -- liquid and vapour stay
//!   saturated at one temperature; mass and internal energy leave with the
//!   liquid; T is solved from (V/m, U/m) each step (the model reviewed by
//!   Zimmerman, Waxman, Cantwell & Zilliac, AIAA 2013-4045, and used by e.g.
//!   the McGill Rocket Team's model, arXiv:2302.06725 sec. 3). Known bias:
//!   it misses the initial pressure dip of real tanks (non-equilibrium);
//! * injector: Dyer et al.'s non-homogeneous non-equilibrium (NHNE) model,
//!   G = Cd [k/(1+k) G_SPI + 1/(1+k) G_HEM], k = sqrt((Pi-Po)/(Psat-Po)),
//!   with HEM held at its maximum past the critical pressure, as written in
//!   Nino & Razavi, "Design of Two-Phase Injectors ...", AIAA 2019-4154,
//!   eqs (1)-(4) (<4 % error on Waxman's N2O cold-flow data, unchoked).
//!
//! No large open-source project ships this model (GitHub/web search
//! 2026-10-24 found single-team theses and scripts only), so the equations
//! come from the papers above, not from code.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const N2O_SATURATION_JSON: &str = include_str!("../../../data/rocket/n2o_saturation.json");

/// Saturated state at one temperature, SI (Pa, kg/m^3, J/kg, J/kg K).
#[derive(Debug, Clone, Copy)]
pub struct Sat {
    pub t: f64,
    pub p: f64,
    pub rho_l: f64,
    pub rho_v: f64,
    pub u_l: f64,
    pub u_v: f64,
    pub h_l: f64,
    pub h_v: f64,
    pub s_l: f64,
    pub s_v: f64,
}

#[derive(Deserialize)]
struct File {
    columns: Vec<String>,
    rows: Vec<Vec<f64>>,
}

fn table() -> &'static Vec<Sat> {
    static T: OnceLock<Vec<Sat>> = OnceLock::new();
    T.get_or_init(|| {
        let f: File = serde_json::from_str(N2O_SATURATION_JSON).expect("n2o_saturation.json parses");
        let want = ["T_K", "p_bar", "rho_l", "rho_v", "u_l", "u_v", "h_l", "h_v", "s_l", "s_v"];
        assert_eq!(f.columns, want, "n2o_saturation.json columns");
        let mut rows: Vec<Sat> = f
            .rows
            .iter()
            .map(|r| Sat { t: r[0], p: r[1] * 1e5, rho_l: r[2], rho_v: r[3], u_l: r[4] * 1e3, u_v: r[5] * 1e3, h_l: r[6] * 1e3, h_v: r[7] * 1e3, s_l: r[8] * 1e3, s_v: r[9] * 1e3 })
            .collect();
        rows.sort_by(|a, b| a.t.total_cmp(&b.t));
        rows
    })
}

/// Temperature range covered by the table, K.
pub fn t_range() -> (f64, f64) {
    let t = table();
    (t[0].t, t[t.len() - 1].t)
}

/// Saturated state at `t` (K); Err outside the table (never extrapolated).
pub fn sat(t: f64) -> anyhow::Result<Sat> {
    let tab = table();
    let (lo, hi) = t_range();
    anyhow::ensure!(t.is_finite() && t >= lo && t <= hi, "N2O temperature {t:.2} K outside the NIST table {lo}-{hi} K");
    let i = tab.partition_point(|s| s.t <= t).clamp(1, tab.len() - 1);
    let (a, b) = (&tab[i - 1], &tab[i]);
    let w = (t - a.t) / (b.t - a.t);
    let l = |x: f64, y: f64| x + w * (y - x);
    Ok(Sat {
        t,
        p: l(a.p, b.p),
        rho_l: l(a.rho_l, b.rho_l),
        rho_v: l(a.rho_v, b.rho_v),
        u_l: l(a.u_l, b.u_l),
        u_v: l(a.u_v, b.u_v),
        h_l: l(a.h_l, b.h_l),
        h_v: l(a.h_v, b.h_v),
        s_l: l(a.s_l, b.s_l),
        s_v: l(a.s_v, b.s_v),
    })
}

/// Saturation temperature at pressure `p` (Pa): the exact inverse of the
/// table's piecewise-linear p(T).
pub fn t_sat(p: f64) -> anyhow::Result<f64> {
    let tab = table();
    let (first, last) = (&tab[0], &tab[tab.len() - 1]);
    anyhow::ensure!(p.is_finite() && p >= first.p && p <= last.p, "N2O pressure {:.2} bar outside the NIST table {:.1}-{:.1} bar", p / 1e5, first.p / 1e5, last.p / 1e5);
    let i = tab.partition_point(|s| s.p <= p).clamp(1, tab.len() - 1);
    let (a, b) = (&tab[i - 1], &tab[i]);
    Ok(a.t + (p - a.p) / (b.p - a.p) * (b.t - a.t))
}

/// Vapour pressure at `t`, Pa.
pub fn p_sat(t: f64) -> anyhow::Result<f64> {
    Ok(sat(t)?.p)
}

/// HEM mass flux (without Cd) from saturated liquid at `t_up` expanding
/// isentropically to `p` (Pa): G = rho_2 sqrt(2 (h_1 - h_2)).
fn g_hem_at(up: &Sat, p: f64) -> anyhow::Result<f64> {
    if p >= up.p {
        return Ok(0.0);
    }
    let d = sat(t_sat(p)?)?;
    let x = ((up.s_l - d.s_l) / (d.s_v - d.s_l)).clamp(0.0, 1.0);
    let h2 = d.h_l + x * (d.h_v - d.h_l);
    let rho2 = 1.0 / ((1.0 - x) / d.rho_l + x / d.rho_v);
    Ok(rho2 * (2.0 * (up.h_l - h2).max(0.0)).sqrt())
}

/// Dyer NHNE mass flux (kg/m^2 s) of saturated liquid N2O at tank
/// temperature `t_up`, injector inlet pressure `p_in` and downstream
/// (chamber) pressure `p_out`, discharge coefficient `cd`.
pub fn dyer_flux(t_up: f64, p_in: f64, p_out: f64, cd: f64) -> anyhow::Result<f64> {
    if p_in <= p_out {
        return Ok(0.0);
    }
    let up = sat(t_up)?;
    let g_spi = (2.0 * up.rho_l * (p_in - p_out)).sqrt();
    // HEM, choked: the maximum over downstream pressures from p_out up to
    // Psat (G_HEM(p) rises then falls; golden-section on the single peak,
    // never below the value at p_out).
    let top = up.p.min(p_in);
    let mut g_hem = 0.0;
    if p_out < top {
        let r = 0.618_033_988_75;
        let (mut a, mut b) = (p_out, top);
        let mut c = b - r * (b - a);
        let mut d = a + r * (b - a);
        let (mut fc, mut fd) = (g_hem_at(&up, c)?, g_hem_at(&up, d)?);
        for _ in 0..18 {
            if fc > fd {
                b = d;
                d = c;
                fd = fc;
                c = b - r * (b - a);
                fc = g_hem_at(&up, c)?;
            } else {
                a = c;
                c = d;
                fc = fd;
                d = a + r * (b - a);
                fd = g_hem_at(&up, d)?;
            }
        }
        g_hem = g_hem_at(&up, p_out)?.max(fc).max(fd);
    }
    let k = ((p_in - p_out) / (up.p - p_out).max(1.0)).sqrt();
    Ok(cd * (k / (1.0 + k) * g_spi + 1.0 / (1.0 + k) * g_hem))
}

/// The run tank's thermodynamic state: total N2O mass and internal energy in
/// a fixed volume.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RunTank {
    pub volume: f64,
    pub mass: f64,
    pub energy: f64,
}

impl RunTank {
    /// Fill `mass` kg into `volume` m^3 at saturation temperature `t`.
    pub fn fill(volume: f64, mass: f64, t: f64) -> anyhow::Result<Self> {
        let s = sat(t)?;
        let v = volume / mass;
        let x = (v - 1.0 / s.rho_l) / (1.0 / s.rho_v - 1.0 / s.rho_l);
        anyhow::ensure!(x > 0.0, "N2O tank liquid-full at {:.1} C: {:.2} kg does not fit {:.1} L as liquid + vapour", t - 273.15, mass, volume * 1e3);
        anyhow::ensure!(x < 1.0, "N2O tank holds only vapour at {:.1} C", t - 273.15);
        Ok(Self { volume, mass, energy: mass * (s.u_l + x * (s.u_v - s.u_l)) })
    }
    /// Equilibrium temperature and vapour quality x (mass fraction of
    /// vapour); x >= 1 means the liquid is gone.
    pub fn state(&self) -> anyhow::Result<(Sat, f64)> {
        let v = self.volume / self.mass;
        let u = self.energy / self.mass;
        let q = |s: &Sat| (v - 1.0 / s.rho_l) / (1.0 / s.rho_v - 1.0 / s.rho_l);
        let f = |t: f64| -> anyhow::Result<f64> {
            let s = sat(t)?;
            Ok(s.u_l + q(&s) * (s.u_v - s.u_l) - u)
        };
        let (mut lo, mut hi) = t_range();
        anyhow::ensure!(f(lo)? < 0.0 && f(hi)? > 0.0, "N2O tank state (v {v:.5} m^3/kg, u {:.1} kJ/kg) outside the table", u / 1e3);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if f(mid)? < 0.0 { lo = mid } else { hi = mid }
        }
        let s = sat(0.5 * (lo + hi))?;
        Ok((s, q(&s)))
    }
    /// Liquid leaves at the bottom: remove `dm` kg of saturated liquid.
    pub fn drain(&mut self, dm: f64, h_l: f64) {
        self.mass -= dm;
        self.energy -= dm * h_l;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vapour_pressure_matches_nist_points() {
        // NIST WebBook rows: 293.03 K 50.382 bar; 278 K ~35.2 bar (277.94 K 35.233 bar).
        assert!((p_sat(293.03).unwrap() / 1e5 - 50.382).abs() < 1e-6);
        let p5 = p_sat(278.15).unwrap() / 1e5;
        assert!((p5 - 35.4).abs() < 0.3, "{p5}");
        assert!((t_sat(50.382e5).unwrap() - 293.03).abs() < 1e-3);
        assert!(sat(200.0).is_err() && sat(320.0).is_err());
    }

    #[test]
    fn dyer_lies_between_hem_and_spi() {
        // 20 C saturated liquid into 25 bar: flux below SPI, above zero.
        let t = 293.15;
        let p_in = p_sat(t).unwrap();
        let g = dyer_flux(t, p_in, 25e5, 1.0).unwrap();
        let spi = (2.0 * sat(t).unwrap().rho_l * (p_in - 25e5)).sqrt();
        assert!(g > 0.3 * spi && g < spi, "{g} vs SPI {spi}");
    }

    #[test]
    fn draining_liquid_cools_and_depressurises_the_tank() {
        let mut tank = RunTank::fill(0.02, 14.0, 293.15).unwrap();
        let (s0, x0) = tank.state().unwrap();
        assert!((s0.t - 293.15).abs() < 0.01 && x0 > 0.0 && x0 < 0.1, "{x0}");
        for _ in 0..50 {
            let (s, _) = tank.state().unwrap();
            tank.drain(0.1, s.h_l);
        }
        let (s1, x1) = tank.state().unwrap();
        assert!(s1.t < s0.t - 3.0 && s1.p < s0.p - 4e5, "T {} p {}", s1.t, s1.p / 1e5);
        assert!(x1 > x0);
    }
}
