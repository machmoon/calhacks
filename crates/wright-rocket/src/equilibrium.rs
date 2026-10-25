//! Chemical equilibrium of ideal-gas products by Gibbs energy minimisation.
//!
//! The method is Gordon & McBride, NASA RP-1311 Part I (1994), section 2:
//! Newton iteration on the Lagrangian of g = sum mu_j n_j subject to element
//! balance, solving for the Lagrange multipliers pi_i, d ln n and d ln T
//! (the reduced system, eqs 2.24-2.28), then recovering d ln n_j from
//! eq 2.18. The matrix layout, the damping factor (RP-1311 eqs 3.1-3.3) and
//! the trace-species threshold follow NASA CEA's open source:
//! `source/equilibrium.f90` (`EqSolver_assemble_matrix`,
//! `EqSolver_get_solution_vars`, `EqSolver_compute_damped_update_factor`)
//! at github.com/nasa/cea commit 2f79a647e857.
//!
//! Deviation, stated: gas-phase products only (no condensed species such
//! as C(gr) or H2O(L)). Every propellant pair Wright designs with runs well
//! on the oxidiser side of soot formation; `Equilibrium::solve` returns an
//! error rather than a wrong answer if it fails to converge.
//!
//! Units: n_j in mol per kg of mixture, h_j = H_j/RT, s_j = S_j/R,
//! P in Pa (the polynomials' standard state is 1 bar).

// Index loops mirror the RP-1311 / CEA matrix equations term by term.
#![allow(clippy::needless_range_loop, clippy::manual_memcpy)]

use crate::thermo::{Species, P_REF, R};
use std::collections::BTreeMap;

/// What is held fixed.
#[derive(Debug, Clone, Copy)]
pub enum Problem {
    /// Temperature (K) and pressure (Pa).
    Tp { t: f64, p: f64 },
    /// Specific enthalpy (J/kg) and pressure (Pa): adiabatic combustion.
    Hp { h: f64, p: f64 },
    /// Specific entropy (J/kg/K) and pressure (Pa): isentropic expansion.
    Sp { s: f64, p: f64 },
}

/// RP-1311 / CEA trace threshold: a species with ln(n_j/n) below -SIZE is
/// held at zero in the sums but its ln n_j keeps iterating (CEA `tsize`).
const SIZE: f64 = 18.420_681;

/// The product species allowed for one set of elements.
#[derive(Debug, Clone)]
pub struct Equilibrium {
    pub species: Vec<Species>,
    pub elements: Vec<String>,
    /// a[j][i]: atoms of element i in species j.
    a: Vec<Vec<f64>>,
    /// b0[i]: mol of element i per kg of mixture.
    pub b0: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct State {
    pub t: f64,
    pub p: f64,
    /// mol/kg per species, aligned with `Equilibrium::species`.
    pub nj: Vec<f64>,
    ln_nj: Vec<f64>,
    /// Total gas moles, mol/kg.
    pub n: f64,
    pub iterations: usize,
}

impl State {
    /// Molecular weight, kg/mol (CEA's "M, (1/n)").
    pub fn molar_mass(&self) -> f64 {
        1.0 / self.n
    }
    /// Density, kg/m^3 (ideal gas).
    pub fn density(&self) -> f64 {
        self.p / (self.n * R * self.t)
    }
    pub fn mole_fraction(&self, eq: &Equilibrium, name: &str) -> f64 {
        eq.species.iter().position(|s| s.name == name).map(|j| self.nj[j] / self.n).unwrap_or(0.0)
    }
}

fn solve_linear(mut m: Vec<Vec<f64>>) -> Option<Vec<f64>> {
    // Gaussian elimination with partial pivoting on an augmented matrix.
    let n = m.len();
    for c in 0..n {
        let piv = (c..n).max_by(|&x, &y| m[x][c].abs().total_cmp(&m[y][c].abs()))?;
        if m[piv][c].abs() < 1e-300 {
            return None;
        }
        m.swap(c, piv);
        for r in c + 1..n {
            let f = m[r][c] / m[c][c];
            if f != 0.0 {
                for k in c..=n {
                    m[r][k] -= f * m[c][k];
                }
            }
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = m[r][n];
        for k in r + 1..n {
            s -= m[r][k] * x[k];
        }
        x[r] = s / m[r][r];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

impl Equilibrium {
    /// Products drawn from `all` whose elements are all present in `b0`.
    pub fn new(all: &[Species], b0: &BTreeMap<String, f64>) -> Self {
        let elements: Vec<String> = b0.iter().filter(|(_, v)| **v > 0.0).map(|(k, _)| k.clone()).collect();
        let species: Vec<Species> = all
            .iter()
            .filter(|s| s.elements.keys().all(|e| elements.contains(e)))
            .cloned()
            .collect();
        let a = species
            .iter()
            .map(|s| elements.iter().map(|e| *s.elements.get(e).unwrap_or(&0.0)).collect())
            .collect();
        let b0 = elements.iter().map(|e| b0[e]).collect();
        Self { species, elements, a, b0 }
    }

    /// Mixture enthalpy, J/kg, at the state's composition.
    pub fn enthalpy(&self, st: &State) -> f64 {
        self.species.iter().zip(&st.nj).map(|(s, n)| n * s.h_rt(st.t)).sum::<f64>() * R * st.t
    }
    /// Mixture entropy, J/(kg K), including mixing and pressure terms.
    pub fn entropy(&self, st: &State) -> f64 {
        let lnp = (st.p / P_REF).ln();
        self.species
            .iter()
            .zip(&st.nj)
            .filter(|(_, n)| **n > 0.0)
            .map(|(s, n)| n * (s.s_r(st.t) - (n / st.n).ln() - lnp))
            .sum::<f64>()
            * R
    }
    /// Frozen specific heat, J/(kg K).
    pub fn cp_frozen(&self, st: &State) -> f64 {
        self.species.iter().zip(&st.nj).map(|(s, n)| n * s.cp_r(st.t)).sum::<f64>() * R
    }

    /// The initial estimate CEA uses (theory.rst): n = 0.1 kmol/kg spread
    /// evenly, T = 3800 K.
    pub fn initial(&self, p: f64) -> State {
        let ng = self.species.len();
        let n = 100.0;
        State { t: 3800.0, p, nj: vec![n / ng as f64; ng], ln_nj: vec![(n / ng as f64).ln(); ng], n, iterations: 0 }
    }

    /// Solve one problem from a starting state (pass the previous solution
    /// to warm-start an expansion).
    pub fn solve(&self, problem: Problem, start: &State) -> anyhow::Result<State> {
        let ne = self.elements.len();
        let ng = self.species.len();
        let (p, const_t) = match problem {
            Problem::Tp { p, .. } => (p, true),
            Problem::Hp { p, .. } | Problem::Sp { p, .. } => (p, false),
        };
        let mut st = start.clone();
        st.p = p;
        if let Problem::Tp { t, .. } = problem {
            st.t = t;
        }
        let lnp = (p / P_REF).ln();
        for iter in 0..500 {
            let t = st.t;
            let ln_n = st.n.ln();
            let h: Vec<f64> = self.species.iter().map(|s| s.h_rt(t)).collect();
            let s0: Vec<f64> = self.species.iter().map(|s| s.s_r(t)).collect();
            let cp: Vec<f64> = self.species.iter().map(|s| s.cp_r(t)).collect();
            // Effective amounts: trace species are zero in the sums.
            let nj: Vec<f64> = st
                .ln_nj
                .iter()
                .map(|&l| if l - ln_n < -SIZE { 0.0 } else { l.exp() })
                .collect();
            let mu: Vec<f64> = (0..ng).map(|j| h[j] - s0[j] + st.ln_nj[j] - ln_n + lnp).collect();
            let sum_n: f64 = nj.iter().sum();
            let b: Vec<f64> = (0..ne).map(|i| (0..ng).map(|j| self.a[j][i] * nj[j]).sum()).collect();
            let dim = ne + 1 + usize::from(!const_t);
            let mut g = vec![vec![0.0; dim + 1]; dim];
            // Element rows (RP-1311 eq 2.24).
            for i in 0..ne {
                for k in 0..ne {
                    g[i][k] = (0..ng).map(|j| self.a[j][i] * self.a[j][k] * nj[j]).sum();
                }
                g[i][ne] = b[i];
                if !const_t {
                    g[i][ne + 1] = (0..ng).map(|j| self.a[j][i] * nj[j] * h[j]).sum();
                }
                g[i][dim] = self.b0[i] - b[i] + (0..ng).map(|j| self.a[j][i] * nj[j] * mu[j]).sum::<f64>();
            }
            // Total-moles row (eq 2.26).
            for k in 0..ne {
                g[ne][k] = b[k];
            }
            g[ne][ne] = sum_n - st.n;
            if !const_t {
                g[ne][ne + 1] = (0..ng).map(|j| nj[j] * h[j]).sum();
            }
            g[ne][dim] = st.n - sum_n + (0..ng).map(|j| nj[j] * mu[j]).sum::<f64>();
            // Energy row: eq 2.27 (HP) or eq 2.28 (SP).
            if !const_t {
                let r = ne + 1;
                let (w, resid): (Vec<f64>, f64) = match problem {
                    Problem::Hp { h: h0, .. } => {
                        let w: Vec<f64> = (0..ng).map(|j| nj[j] * h[j]).collect();
                        let resid = h0 / (R * t) - w.iter().sum::<f64>();
                        (w, resid)
                    }
                    Problem::Sp { s: sv, .. } => {
                        // tmp = n_j (h_j - mu_j) = n_j (s_j - ln(n_j/n) - ln P)
                        let w: Vec<f64> = (0..ng).map(|j| nj[j] * (h[j] - mu[j])).collect();
                        let resid = sv / R - w.iter().sum::<f64>() + (st.n - sum_n);
                        (w, resid)
                    }
                    Problem::Tp { .. } => unreachable!(),
                };
                for k in 0..ne {
                    g[r][k] = (0..ng).map(|j| w[j] * self.a[j][k]).sum();
                }
                g[r][ne] = w.iter().sum();
                g[r][ne + 1] = (0..ng).map(|j| nj[j] * cp[j] + w[j] * h[j]).sum();
                g[r][dim] = resid + (0..ng).map(|j| w[j] * mu[j]).sum::<f64>();
            }
            let x = solve_linear(g).ok_or_else(|| anyhow::anyhow!("singular equilibrium matrix"))?;
            let pi = &x[..ne];
            let dln_n = x[ne];
            let dln_t = if const_t { 0.0 } else { x[ne + 1] };
            // eq 2.18
            let dln_nj: Vec<f64> = (0..ng)
                .map(|j| -mu[j] + dln_n + (0..ne).map(|i| self.a[j][i] * pi[i]).sum::<f64>() + dln_t * h[j])
                .collect();
            // Damping, RP-1311 eqs 3.1-3.3 (CEA compute_damped_update_factor).
            const FACTOR: f64 = -9.210_340_4; // ln(1e-4)
            let mut l1_denom = (5.0 * dln_t.abs()).max(5.0 * dln_n.abs());
            let mut lambda2: f64 = 1.0;
            for j in 0..ng {
                if dln_nj[j] > 0.0 {
                    if st.ln_nj[j] - ln_n + SIZE <= 0.0 {
                        let d = (dln_nj[j] - dln_n).abs();
                        if d >= SIZE + FACTOR {
                            lambda2 = lambda2.min((FACTOR - st.ln_nj[j] + ln_n).abs() / d);
                        }
                    } else if dln_nj[j] > l1_denom {
                        l1_denom = dln_nj[j];
                    }
                }
            }
            let lambda1 = if l1_denom > 2.0 { 2.0 / l1_denom } else { 1.0 };
            let lambda = 1.0f64.min(lambda1).min(lambda2);
            for j in 0..ng {
                st.ln_nj[j] = (st.ln_nj[j] + lambda * dln_nj[j]).max(-700.0);
            }
            st.n = (ln_n + lambda * dln_n).exp();
            st.t = (t.ln() + lambda * dln_t).exp();
            // Convergence (RP-1311 section 3.3): relative changes.
            let tol = 0.5e-5;
            let conv_n = (0..ng).all(|j| nj[j] * dln_nj[j].abs() / sum_n <= tol)
                && st.n * dln_n.abs() / sum_n <= tol
                && dln_t.abs() <= 1e-4
                && (0..ne).all(|i| (self.b0[i] - b[i]).abs() <= 1e-6 * self.b0.iter().cloned().fold(0.0, f64::max));
            if conv_n && lambda == 1.0 {
                st.iterations = iter + 1;
                st.nj = st.ln_nj.iter().map(|&l| if l - st.n.ln() < -SIZE { 0.0 } else { l.exp() }).collect();
                return Ok(st);
            }
            if !st.t.is_finite() || st.t < 50.0 || st.t > 8000.0 {
                anyhow::bail!("equilibrium diverged (T = {})", st.t);
            }
        }
        anyhow::bail!("equilibrium did not converge in 500 iterations")
    }

    /// Temperature at fixed composition (frozen flow) for a given entropy
    /// and pressure, by Newton on T. Returns a state with the same n_j.
    pub fn frozen_at_entropy(&self, from: &State, s: f64, p: f64) -> State {
        let mut st = from.clone();
        st.p = p;
        for _ in 0..100 {
            let f = self.entropy(&st) - s;
            let dsdt = self.cp_frozen(&st) / st.t;
            let dt = -f / dsdt;
            st.t = (st.t + dt).max(50.0);
            if dt.abs() < 1e-7 * st.t {
                break;
            }
        }
        st
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thermo::products;

    #[test]
    fn hydrogen_oxygen_tp_conserves_elements() {
        let mut b0 = BTreeMap::new();
        // 1 kg of stoichiometric H2/O2: 2 H2 + O2 -> 2 H2O, 36.03 g per 2 mol H2O.
        let mol = 1000.0 / 36.03056;
        b0.insert("H".into(), 4.0 * mol);
        b0.insert("O".into(), 2.0 * mol);
        let eq = Equilibrium::new(&products(), &b0);
        let st = eq.solve(Problem::Tp { t: 3000.0, p: 1e6 }, &eq.initial(1e6)).unwrap();
        let h: f64 = eq.species.iter().zip(&st.nj).map(|(s, n)| n * s.elements.get("H").unwrap_or(&0.0)).sum();
        assert!((h - 4.0 * mol).abs() / (4.0 * mol) < 1e-5);
        // Water dominates at 3000 K, 10 bar but dissociation is real.
        let x = st.mole_fraction(&eq, "H2O");
        assert!(x > 0.6 && x < 0.95, "{x}");
    }
}
