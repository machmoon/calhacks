"""Run: <venv312>/bin/python -I evals/rocket/cantera_bench.py . evals/rocket/cantera_bench.json

Cantera 3.2 rocket performance on the same cases as Wright's CEA table.

Same reactant enthalpies as Wright (data/rocket/propellants.json, copied
from NASA CEA thermo.inp); products: every gas species in Cantera's
nasa_gas.yaml made of C/H/O/N. Chamber HP equilibrium, shifting SP
expansion; throat where the mass flux peaks (u = equilibrium sound speed).
"""
import json, sys, time
import numpy as np, cantera as ct
from scipy.optimize import minimize_scalar

root = sys.argv[1]
props = {r["id"]: r for r in json.load(open(f"{root}/data/rocket/propellants.json"))["rows"]}
ref = {c["name"]: c for c in json.load(open(f"{root}/data/rocket/cea_validation.json"))["cea_generated_cases"]}
wright = {c["case"]: c for c in json.load(open(f"{root}/evals/rocket/result.json"))["cea_validation"]}
cases = [
    ("LOX/ethanol Pc=20.0 O/F=1.3", "lox", [("ethanol", 1.0)]),
    ("LOX/ethanol Pc=30.0 O/F=1.5", "lox", [("ethanol", 1.0)]),
    ("Braeunig LOX/75% ethanol+25% water O/F=1.29", "lox", [("ethanol", 0.75), ("water", 0.25)]),
    ("LOX/C3H8(L) Pc=24 O/F=2.33", "lox", [("propane", 1.0)]),
    ("LOX/C3H8(L) Pc=30 O/F=2.6", "lox", [("propane", 1.0)]),
    ("LOX/RP-1 Pc=68.9 O/F=2.4", "lox", [("rp1", 1.0)]),
    ("LOX/CH4(L) Pc=30 O/F=3.0", "lox", [("methane", 1.0)]),
    ("N2O(g)/ethanol(L) Pc=30 O/F=4.0", "n2o_gas", [("ethanol", 1.0)]),
    ("N2O(g)/IPA(g) Pc=30 O/F=3.0", "n2o_gas", [("ipa_gas", 1.0)]),
]
allsp = ct.Species.list_from_file("nasa_gas.yaml")
sp = [s for s in allsp if set(s.composition) <= {"C", "H", "O", "N"}]
gas = ct.Solution(thermo="ideal-gas", species=sp)

def mix(ox, fuels, of):
    parts = [(props[ox], of / (1 + of))] + [(props[f], w / (1 + of)) for f, w in fuels]
    el, h = {}, 0.0
    for r, mf in parts:
        mol = mf / (r["molar_mass_g"] / 1e3)  # mol per kg mixture
        h += mol * r["enthalpy_j_mol"]
        for e, n in r["formula"].items():
            el[e] = el.get(e, 0.0) + mol * n
    return el, h

def perf(ox, fuels, of, pc, pe=101325.0):
    el, h0 = mix(ox, fuels, of)
    gas.TPX = 3000, pc, {e: n for e, n in el.items()}
    gas.equilibrate("TP")
    gas.HP = h0, pc
    gas.equilibrate("HP")
    tc, mw, s0 = gas.T, gas.mean_molecular_weight, gas.s
    def at(p):
        gas.SP = s0, p
        gas.equilibrate("SP")
        u = np.sqrt(max(2 * (h0 - gas.h), 0.0))
        return u, gas.density
    r = minimize_scalar(lambda p: -np.prod(at(p)), bounds=(0.4 * pc, 0.8 * pc), method="bounded", options={"xatol": 1.0})
    flux_t = -r.fun
    ue, rhoe = at(pe)
    cstar = pc / flux_t
    ae = flux_t / (rhoe * ue)
    return dict(Tc_K=tc, M_g_mol=mw, c_star_m_s=cstar, Isp_1atm_m_s=ue, Isp_vac_m_s=ue + pe / (rhoe * ue), Ae_At=ae)

print(f"Cantera {ct.__version__}, {len(sp)} gas species from nasa_gas.yaml")
keys = ["Tc_K", "M_g_mol", "c_star_m_s", "Isp_1atm_m_s", "Isp_vac_m_s", "Ae_At"]
out, worst = [], {"wright": 0.0, "cantera": 0.0}
t_total = 0.0
for name, ox, fuels in cases:
    c = ref[name]
    t0 = time.time()
    p = perf(ox, fuels, c["inputs"]["O_F"], c["inputs"]["Pc_bar"] * 1e5)
    t_total += time.time() - t0
    row = {"case": name}
    for k in keys:
        cea = wright[name][k][1]
        w = wright[name][k][0]
        row[k] = {"cea": cea, "wright": w, "cantera": p[k], "wright_err_pct": (w - cea) / cea * 100, "cantera_err_pct": (p[k] - cea) / cea * 100}
        worst["wright"] = max(worst["wright"], abs(row[k]["wright_err_pct"]))
        worst["cantera"] = max(worst["cantera"], abs(row[k]["cantera_err_pct"]))
    out.append(row)
    print(name, " ".join(f"{k} W{row[k]['wright_err_pct']:+.3f}% C{row[k]['cantera_err_pct']:+.3f}%" for k in keys))
print("worst |err| vs NASA CEA 3.3.4:", worst, f"cantera time {t_total:.1f}s")
json.dump({"cantera_version": ct.__version__, "species": len(sp), "rows": out, "worst_abs_err_pct": worst}, open(sys.argv[2], "w"), indent=1)
