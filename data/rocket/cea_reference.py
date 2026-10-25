"""Generate NASA CEA reference rocket outputs for validating the Rust Gibbs solver.

Uses the official NASA CEA Python bindings (PyPI `cea`, source github.com/nasa/cea),
following the API exactly as in the repo's docs/source/examples/rocket/example8.rst
(RP-1311 Example 8). Equilibrium, infinite-area combustor (IAC).

Run:  <venv>/bin/python -I cea_reference.py > out.json
"""
import json
import sys

import numpy as np
import cea

R_ATM = 1.01325  # bar


def run(name, fuel, oxid, of, pc, t_fuel, t_oxid, pi_p=None, supar=None, subar=None,
        extra_fuel=None):
    """extra_fuel = (species, weight_fraction_of_fuel, T_K) for a 2-component fuel."""
    reac_names = [fuel, oxid]
    T_reactant = [t_fuel, t_oxid]
    fuel_w = [1.0, 0.0]
    oxid_w = [0.0, 1.0]
    if extra_fuel:
        sp, wf, tx = extra_fuel
        reac_names.append(sp)
        T_reactant.append(tx)
        fuel_w = [1.0 - wf, 0.0, wf]
        oxid_w.append(0.0)
    T_reactant = np.array(T_reactant)
    fuel_w = np.array(fuel_w)
    oxid_w = np.array(oxid_w)
    reac = cea.Mixture(reac_names)
    prod = cea.Mixture(reac_names, products_from_reactants=True)
    solver = cea.RocketSolver(prod, reactants=reac)
    sol = cea.RocketSolution(solver)
    w = reac.of_ratio_to_weights(oxid_w, fuel_w, of)
    hc = reac.calc_property(cea.ENTHALPY, w, T_reactant) / cea.R
    if pi_p is None:
        pi_p = [pc / R_ATM]
    kw = dict(hc=hc, iac=True)
    if supar:
        kw["supar"] = supar
    if subar:
        kw["subar"] = subar
    solver.solve(sol, w, pc, pi_p, **kw)
    n = sol.num_pts
    stations = []
    for i in range(n):
        stations.append({
            "P_bar": float(sol.P[i]),
            "T_K": float(sol.T[i]),
            "M_1_over_n": float(sol.M[i]),
            "MW": float(sol.MW[i]),
            "gamma_s": float(sol.gamma_s[i]),
            "cp_eq_kJ_kgK": float(sol.cp_eq[i]),
            "son_vel_m_s": float(sol.sonic_velocity[i]),
            "mach": float(sol.Mach[i]),
            "ae_at": float(sol.ae_at[i]),
            "c_star_m_s": float(sol.c_star[i]),
            "Cf": float(sol.coefficient_of_thrust[i]),
            "Isp_m_s": float(sol.Isp[i]),
            "Isp_vac_m_s": float(sol.Isp_vacuum[i]),
        })
    mf = {k: [float(x) for x in v] for k, v in sol.mole_fractions.items()
          if np.any(np.asarray(v) > 5e-6)}
    return {
        "name": name,
        "inputs": {
            "fuel": fuel, "T_fuel_K": t_fuel,
            "oxidizer": oxid, "T_oxid_K": t_oxid,
            "O_F": of, "Pc_bar": pc, "pi_p": list(pi_p),
            "subar": subar, "supar": supar,
            "extra_fuel": extra_fuel,
            "model": "equilibrium, IAC",
        },
        "station_order": "chamber, throat, then pi_p exits (pc/pe), then subar, then supar",
        "stations": stations,
        "mole_fractions_gt_5e-6": mf,
    }


def main():
    cases = []
    # RP-1311 Example 8 reproduction (published output in example8.rst)
    cases.append(run("H2(L)/O2(L) RP-1311 Example 8", "H2(L)", "O2(L)", 5.55157, 53.3172,
                     20.27, 90.17, pi_p=[10.0, 100.0, 1000.0], subar=[1.58],
                     supar=[25.0, 50.0, 75.0]))
    for pc in (20.0, 30.0):
        for of in (1.3, 1.5):
            cases.append(run(f"LOX/ethanol Pc={pc} O/F={of}", "C2H5OH(L)", "O2(L)", of, pc,
                             298.15, 90.17))
    cases.append(run("LOX/RP-1 Pc=68.9 O/F=2.4", "RP-1", "O2(L)", 2.4, 68.9, 298.15, 90.17))
    # thermo.inp has no liquid IPA; C3H8O,2propanol is the GAS species (heat of
    # vaporisation not included -> Tc slightly high vs liquid IPA).
    cases.append(run("N2O(g)/IPA(g) Pc=30 O/F=3.0", "C3H8O,2propanol", "N2O", 3.0, 30.0,
                     298.15, 298.15))
    cases.append(run("N2O(g)/ethanol(L) Pc=30 O/F=4.0", "C2H5OH(L)", "N2O", 4.0, 30.0,
                     298.15, 298.15))
    cases.append(run("LOX/CH4(L) Pc=30 O/F=3.0", "CH4(L)", "O2(L)", 3.0, 30.0, 111.643, 90.17))
    # LOX/propane: the Berkeley SEB Lightbulb engine (Pc 24 bar, O/F 1.14/0.49 = 2.33,
    # arXiv 2401.07444) and a 30 bar design point. C3H8(L) at its CEA-assigned 231.076 K.
    cases.append(run("LOX/C3H8(L) Pc=24 O/F=2.33", "C3H8(L)", "O2(L)", 2.33, 24.0, 231.076, 90.17))
    cases.append(run("LOX/C3H8(L) Pc=30 O/F=2.6", "C3H8(L)", "O2(L)", 2.6, 30.0, 231.076, 90.17))
    # Cross-checks against braeunig.us/space/propel.htm table (Pc=68 atm, pe=1 atm, SL Isp)
    pc68 = 68.0 * R_ATM
    cases.append(run("Braeunig LOX/LH2 O/F=5.00", "H2(L)", "O2(L)", 5.00, pc68, 20.27, 90.17))
    cases.append(run("Braeunig LOX/CH4(L) O/F=2.77", "CH4(L)", "O2(L)", 2.77, pc68, 111.643, 90.17))
    cases.append(run("Braeunig LOX/RP-1 O/F=2.29", "RP-1", "O2(L)", 2.29, pc68, 298.15, 90.17))
    cases.append(run("Braeunig LOX/75% ethanol+25% water O/F=1.29", "C2H5OH(L)", "O2(L)", 1.29,
                     pc68, 298.15, 90.17, extra_fuel=("H2O(L)", 0.25, 298.15)))
    json.dump({"cea_version": cea.__version__, "cases": cases}, sys.stdout, indent=1)


if __name__ == "__main__":
    main()
