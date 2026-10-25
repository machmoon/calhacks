//! Ideal rocket performance from equilibrium thermochemistry: the CEA
//! "rocket" problem with an infinite-area combustor (RP-1311 Part I ch. 6).
//!
//! * Chamber: HP equilibrium at Pc with the reactants' enthalpy.
//! * Expansion: isentropic. Shifting = SP equilibrium at each pressure;
//!   frozen = chamber composition held, T from s(T, P) = s_c.
//! * Throat: CEA (rocket.f90) iterates Pc/Pt until the flow speed equals the
//!   equilibrium sonic speed. We find the same point as the maximum of the
//!   mass flux G = rho u over P -- the two are the same condition
//!   (d(rho u)/dP = 0 <=> u = a_s), and the maximum needs no derivative
//!   matrices. Our G is accurate to second order in the pressure error.
//! * c* = Pc / G_t; Ae/At = G_t / G_e; Isp = u_e (+ (pe - pa)/G_e);
//!   Cf = Isp / c*.  gamma_s = (d ln P / d ln rho)_s by central difference.

use crate::equilibrium::{Equilibrium, Problem, State};
use crate::propellant::Mix;
use crate::thermo::{products, R};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Flow {
    Shifting,
    Frozen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Station {
    pub p_pa: f64,
    pub t_k: f64,
    pub molar_mass_kg_mol: f64,
    pub density_kg_m3: f64,
    pub velocity_m_s: f64,
    /// Mass flux rho u, kg/(m^2 s).
    pub mass_flux: f64,
    pub cp_frozen_j_kg_k: f64,
    /// Isentropic exponent; `None` where it was not computed (stations
    /// other than the chamber). Never NaN: a NaN serialises as JSON `null`
    /// and the design then cannot be read back (found in a live agent run).
    pub gamma_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Performance {
    pub pc_pa: f64,
    pub of: f64,
    pub flow: Flow,
    pub chamber: Station,
    pub throat: Station,
    pub exit: Station,
    pub c_star_m_s: f64,
    pub area_ratio: f64,
    /// Thrust coefficient at the exit pressure (pa = pe).
    pub cf_opt: f64,
    pub isp_opt_m_s: f64,
    pub isp_vac_m_s: f64,
    pub cf_vac: f64,
    /// Mole fractions in the chamber above 1e-5.
    pub chamber_composition: Vec<(String, f64)>,
}

impl Performance {
    /// Isp (m/s) at ambient pressure `pa` for this nozzle.
    pub fn isp_at(&self, pa: f64) -> f64 {
        self.exit.velocity_m_s + (self.exit.p_pa - pa) / self.exit.mass_flux
    }
    /// Thrust coefficient at ambient pressure `pa`.
    pub fn cf_at(&self, pa: f64) -> f64 {
        self.isp_at(pa) / self.c_star_m_s
    }
}

/// Where the nozzle ends.
#[derive(Debug, Clone, Copy)]
pub enum Exit {
    Pressure(f64),
    AreaRatio(f64),
}

struct Expander<'a> {
    eq: &'a Equilibrium,
    chamber: State,
    h_c: f64,
    s_c: f64,
    flow: Flow,
}

impl Expander<'_> {
    fn at(&self, p: f64, warm: &State) -> anyhow::Result<State> {
        match self.flow {
            Flow::Shifting => self.eq.solve(Problem::Sp { s: self.s_c, p }, warm),
            Flow::Frozen => Ok(self.eq.frozen_at_entropy(&self.chamber, self.s_c, p)),
        }
    }
    fn station(&self, st: &State, gamma: bool) -> anyhow::Result<Station> {
        let h = self.eq.enthalpy(st);
        let u = (2.0 * (self.h_c - h)).max(0.0).sqrt();
        let rho = st.density();
        let gamma_s = if gamma { Some(self.gamma_s(st)?) } else { None };
        Ok(Station {
            p_pa: st.p,
            t_k: st.t,
            molar_mass_kg_mol: st.molar_mass(),
            density_kg_m3: rho,
            velocity_m_s: u,
            mass_flux: rho * u,
            cp_frozen_j_kg_k: self.eq.cp_frozen(st),
            gamma_s,
        })
    }
    fn gamma_s(&self, st: &State) -> anyhow::Result<f64> {
        let d = 1e-3;
        let s = self.eq.entropy(st);
        let (hi, lo) = match self.flow {
            Flow::Shifting => (
                self.eq.solve(Problem::Sp { s, p: st.p * (1.0 + d) }, st)?,
                self.eq.solve(Problem::Sp { s, p: st.p * (1.0 - d) }, st)?,
            ),
            Flow::Frozen => (
                self.eq.frozen_at_entropy(st, s, st.p * (1.0 + d)),
                self.eq.frozen_at_entropy(st, s, st.p * (1.0 - d)),
            ),
        };
        Ok(((1.0 + d) / (1.0 - d)).ln() / (hi.density() / lo.density()).ln())
    }
}

/// Run the rocket problem.
pub fn rocket(mix: &Mix, of: f64, pc: f64, exit: Exit, flow: Flow) -> anyhow::Result<Performance> {
    let b0 = mix.elements();
    let eq = Equilibrium::new(&products(), &b0);
    let h0 = mix.enthalpy();
    let chamber = eq.solve(Problem::Hp { h: h0, p: pc }, &eq.initial(pc))?;
    let s_c = eq.entropy(&chamber);
    let x = Expander { eq: &eq, chamber: chamber.clone(), h_c: h0, s_c, flow };

    // Throat: golden-section maximum of G over ln P in [Pc/3, Pc/1.3].
    let g_of = |lnp: f64, warm: &State| -> anyhow::Result<(f64, State)> {
        let st = x.at(lnp.exp(), warm)?;
        let h = eq.enthalpy(&st);
        let u = (2.0 * (h0 - h)).max(0.0).sqrt();
        Ok((st.density() * u, st))
    };
    let phi = (5f64.sqrt() - 1.0) / 2.0;
    let (mut a, mut b) = ((pc / 3.0).ln(), (pc / 1.3).ln());
    let mut c = b - phi * (b - a);
    let mut d = a + phi * (b - a);
    let (mut gc, mut sc) = g_of(c, &chamber)?;
    let (mut gd, mut sd) = g_of(d, &chamber)?;
    for _ in 0..40 {
        if gc > gd {
            b = d;
            d = c;
            gd = gc;
            sd = sc.clone();
            c = b - phi * (b - a);
            (gc, sc) = g_of(c, &sd)?;
        } else {
            a = c;
            c = d;
            gc = gd;
            sc = sd.clone();
            d = a + phi * (b - a);
            (gd, sd) = g_of(d, &sc)?;
        }
    }
    let throat_state = if gc > gd { sc } else { sd };
    let throat = x.station(&throat_state, true)?;
    let c_star = pc / throat.mass_flux;

    let exit_state = match exit {
        Exit::Pressure(pe) => x.at(pe, &throat_state)?,
        Exit::AreaRatio(eps) => {
            anyhow::ensure!(eps > 1.0, "supersonic area ratio must exceed 1");
            // Bisection on ln P below the throat: G_t / G(P) = eps.
            let (mut lo, mut hi) = ((throat.p_pa * 1e-5).ln(), throat.p_pa.ln());
            let mut warm = throat_state.clone();
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                let (g, st) = g_of(mid, &warm)?;
                if throat.mass_flux / g > eps {
                    lo = mid;
                } else {
                    hi = mid;
                }
                warm = st;
                if hi - lo < 1e-10 {
                    break;
                }
            }
            x.at((0.5 * (lo + hi)).exp(), &warm)?
        }
    };
    let exit_st = x.station(&exit_state, false)?;
    let chamber_st = Station {
        p_pa: pc,
        t_k: chamber.t,
        molar_mass_kg_mol: chamber.molar_mass(),
        density_kg_m3: chamber.density(),
        velocity_m_s: 0.0,
        mass_flux: 0.0,
        cp_frozen_j_kg_k: eq.cp_frozen(&chamber),
        gamma_s: Some(x.gamma_s(&chamber)?),
    };
    let area_ratio = throat.mass_flux / exit_st.mass_flux;
    let isp_opt = exit_st.velocity_m_s;
    let isp_vac = isp_opt + exit_st.p_pa / exit_st.mass_flux;
    let mut comp: Vec<(String, f64)> = eq
        .species
        .iter()
        .zip(&chamber.nj)
        .map(|(s, n)| (s.name.clone(), n / chamber.n))
        .filter(|(_, x)| *x > 1e-5)
        .collect();
    comp.sort_by(|a, b| b.1.total_cmp(&a.1));
    let _ = R;
    Ok(Performance {
        pc_pa: pc,
        of,
        flow,
        chamber: chamber_st,
        throat,
        exit: exit_st,
        c_star_m_s: c_star,
        area_ratio,
        cf_opt: isp_opt / c_star,
        isp_opt_m_s: isp_opt,
        isp_vac_m_s: isp_vac,
        cf_vac: isp_vac / c_star,
        chamber_composition: comp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::propellant::reactant;
    use serde_json::Value;

    const CEA: &str = include_str!("../../../data/rocket/cea_validation.json");

    fn case(name: &str) -> Value {
        let v: Value = serde_json::from_str(CEA).unwrap();
        v["cea_generated_cases"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap().clone()
    }

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs()
    }

    /// Run one CEA case and compare. Tolerances: 0.1 % (Tc, M, c*, Isp),
    /// 0.2 % (gamma_s, Ae/At). Observed errors are below 0.07 %.
    fn check(name: &str, ox: &str, fuel: &[(&str, f64)]) -> (Performance, Value) {
        let c = case(name);
        let i = &c["inputs"];
        let of = i["O_F"].as_f64().unwrap();
        let pc = i["Pc_bar"].as_f64().unwrap() * 1e5;
        let fuel: Vec<_> = fuel.iter().map(|(id, w)| (reactant(id).unwrap(), *w)).collect();
        let mix = Mix::bipropellant(&reactant(ox).unwrap(), &fuel, of);
        let p = rocket(&mix, of, pc, Exit::Pressure(101_325.0), Flow::Shifting).unwrap();
        let s = &c["stations"];
        let ch = &s[0];
        let ex = &s[2];
        let f = |v: &Value, k: &str| v[k].as_f64().unwrap();
        let errs = [
            ("Tc", rel(p.chamber.t_k, f(ch, "T_K")), 0.001),
            ("M", rel(p.chamber.molar_mass_kg_mol * 1e3, f(ch, "MW")), 0.001),
            ("gamma_s", rel(p.chamber.gamma_s.unwrap_or(f64::NAN), f(ch, "gamma_s")), 0.002),
            ("c*", rel(p.c_star_m_s, f(ch, "c_star_m_s")), 0.001),
            ("Isp", rel(p.isp_opt_m_s, f(ex, "Isp_m_s")), 0.001),
            ("Isp_vac", rel(p.isp_vac_m_s, f(ex, "Isp_vac_m_s")), 0.001),
            ("Ae/At", rel(p.area_ratio, f(ex, "ae_at")), 0.002),
        ];
        for (k, e, tol) in errs {
            assert!(e < tol, "{name}: {k} off by {:.3} % (tol {:.1} %)", e * 100.0, tol * 100.0);
        }
        (p, c)
    }

    #[test]
    fn lox_ethanol_matches_cea() {
        check("LOX/ethanol Pc=20.0 O/F=1.3", "lox", &[("ethanol", 1.0)]);
        check("LOX/ethanol Pc=30.0 O/F=1.5", "lox", &[("ethanol", 1.0)]);
    }
    #[test]
    fn lox_propane_matches_cea() {
        // The Berkeley SEB Lightbulb point: Pc 24 bar, O/F 2.33.
        check("LOX/C3H8(L) Pc=24 O/F=2.33", "lox", &[("propane", 1.0)]);
        check("LOX/C3H8(L) Pc=30 O/F=2.6", "lox", &[("propane", 1.0)]);
    }
    #[test]
    fn lox_rp1_matches_cea() {
        check("LOX/RP-1 Pc=68.9 O/F=2.4", "lox", &[("rp1", 1.0)]);
    }
    #[test]
    fn lox_methane_matches_cea() {
        check("LOX/CH4(L) Pc=30 O/F=3.0", "lox", &[("methane", 1.0)]);
    }
    #[test]
    fn n2o_ethanol_matches_cea() {
        check("N2O(g)/ethanol(L) Pc=30 O/F=4.0", "n2o_gas", &[("ethanol", 1.0)]);
    }
    #[test]
    fn lox_ethanol_water_matches_cea() {
        check("Braeunig LOX/75% ethanol+25% water O/F=1.29", "lox", &[("ethanol", 0.75), ("water", 0.25)]);
    }
    #[test]
    fn hydrogen_oxygen_rp1311_example8_chamber() {
        // Published NASA output (example8.rst): Tc 3383.845 K, c* 2332.34 m/s.
        let of = 5.55157;
        let mix = Mix::bipropellant(&reactant("lox").unwrap(), &[(reactant("hydrogen").unwrap(), 1.0)], of);
        let p = rocket(&mix, of, 53.3172e5, Exit::Pressure(53.3172e5 / 10.0), Flow::Shifting).unwrap();
        assert!(rel(p.chamber.t_k, 3383.845) < 0.003, "{}", p.chamber.t_k);
        assert!(rel(p.c_star_m_s, 2332.34) < 0.003, "{}", p.c_star_m_s);
        // pc/pe = 10 column: Ae/At 2.350, Isp 2967.386 m/s, Isp_vac 3515.549 m/s.
        assert!(rel(p.area_ratio, 2.350) < 0.01, "{}", p.area_ratio);
        assert!(rel(p.isp_opt_m_s, 2967.386) < 0.004, "{}", p.isp_opt_m_s);
        assert!(rel(p.isp_vac_m_s, 3515.549) < 0.004, "{}", p.isp_vac_m_s);
    }
}

/// Wright vs NASA CEA 3.3.4 on the cases in `data/rocket/cea_validation.json`.
pub fn validation_table() -> anyhow::Result<Vec<serde_json::Value>> {
    use crate::propellant::reactant;
    let v: serde_json::Value = serde_json::from_str(include_str!("../../../data/rocket/cea_validation.json"))?;
    type Case<'a> = (&'a str, &'a str, &'a [(&'a str, f64)]);
    let cases: &[Case] = &[
        ("LOX/ethanol Pc=20.0 O/F=1.3", "lox", &[("ethanol", 1.0)]),
        ("LOX/ethanol Pc=30.0 O/F=1.5", "lox", &[("ethanol", 1.0)]),
        ("Braeunig LOX/75% ethanol+25% water O/F=1.29", "lox", &[("ethanol", 0.75), ("water", 0.25)]),
        ("LOX/C3H8(L) Pc=24 O/F=2.33", "lox", &[("propane", 1.0)]),
        ("LOX/C3H8(L) Pc=30 O/F=2.6", "lox", &[("propane", 1.0)]),
        ("LOX/RP-1 Pc=68.9 O/F=2.4", "lox", &[("rp1", 1.0)]),
        ("LOX/CH4(L) Pc=30 O/F=3.0", "lox", &[("methane", 1.0)]),
        ("N2O(g)/ethanol(L) Pc=30 O/F=4.0", "n2o_gas", &[("ethanol", 1.0)]),
        ("N2O(g)/IPA(g) Pc=30 O/F=3.0", "n2o_gas", &[("ipa_gas", 1.0)]),
    ];
    let mut out = Vec::new();
    for (name, ox, fuel) in cases {
        let c = v["cea_generated_cases"].as_array().unwrap().iter().find(|c| c["name"] == *name)
            .ok_or_else(|| anyhow::anyhow!("case {name} missing"))?;
        let of = c["inputs"]["O_F"].as_f64().unwrap();
        let pc = c["inputs"]["Pc_bar"].as_f64().unwrap() * 1e5;
        let fuel: Vec<_> = fuel.iter().map(|(id, w)| (reactant(id).unwrap(), *w)).collect();
        let mix = Mix::bipropellant(&reactant(ox)?, &fuel, of);
        let p = rocket(&mix, of, pc, Exit::Pressure(101_325.0), Flow::Shifting)?;
        let ch = &c["stations"][0];
        let ex = &c["stations"][2];
        let g = |s: &serde_json::Value, k: &str| s[k].as_f64().unwrap();
        out.push(serde_json::json!({
            "case": name,
            "Tc_K": [p.chamber.t_k, g(ch, "T_K")],
            "M_g_mol": [p.chamber.molar_mass_kg_mol * 1e3, g(ch, "MW")],
            "gamma_s": [p.chamber.gamma_s, g(ch, "gamma_s")],
            "c_star_m_s": [p.c_star_m_s, g(ch, "c_star_m_s")],
            "Isp_1atm_m_s": [p.isp_opt_m_s, g(ex, "Isp_m_s")],
            "Isp_vac_m_s": [p.isp_vac_m_s, g(ex, "Isp_vac_m_s")],
            "Ae_At": [p.area_ratio, g(ex, "ae_at")],
        }));
    }
    Ok(out)
}

#[cfg(test)]
mod table {
    #[test]
    fn print_validation() {
        for r in super::validation_table().unwrap() {
            let mut s = format!("{:<46}", r["case"].as_str().unwrap());
            for k in ["Tc_K", "M_g_mol", "gamma_s", "c_star_m_s", "Isp_1atm_m_s", "Isp_vac_m_s", "Ae_At"] {
                let a = r[k][0].as_f64().unwrap();
                let b = r[k][1].as_f64().unwrap();
                s += &format!(" {k} {a:.4}/{b:.4} ({:+.2}%)", (a - b) / b * 100.0);
            }
            println!("{s}");
        }
    }
}
