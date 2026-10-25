"""Benchmark Wright's aero (crates/wright-air/src/aero.rs, a port of
AeroSandbox's AeroBuildup) against AeroBuildup itself on the same geometry.

    cargo run --release -p wright-air --example aero_audit DESIGN.json > wright_aero.json
    python -I aero_audit.py wright_aero.json OUT_DIR DATA_AIR_DIR

The geometry is vtol_sim.py's `airplane()` (the same AeroSandbox model the
sim flies), so the sim, this audit and Wright's own numbers share one design.
"""

import json
import os
import sys

import numpy as np

W_JSON, OUT, DATA = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(OUT, exist_ok=True)
WA = json.load(open(W_JSON))
inputs = os.path.join(OUT, "inputs.json")
json.dump(WA["sim"], open(inputs, "w"))
# vtol_sim.py's geometry section only (everything above its aero tables)
src = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "vtol_sim.py")).read()
geo = src.split("# ------------------------------------------------------------ aero tables")[0]
sys.argv = ["vtol_sim.py", inputs, OUT, DATA]
ns = {"__name__": "geo", "__file__": "vtol_sim.py"}
exec(compile(geo, "vtol_sim.py", "exec"), ns)
asb, airplane, X = ns["asb"], ns["airplane"], ns["X"]

V = WA["v"]
atm = asb.Atmosphere(altitude=X["air"]["altitude_m"])
full = airplane()
wing_only = asb.Airplane("wing", xyz_ref=X["mass"]["cg"], wings=[full.wings[0]], s_ref=full.s_ref, c_ref=full.c_ref, b_ref=full.b_ref)


def run(ap, alpha, v=V):
    op = asb.OperatingPoint(atmosphere=atm, velocity=v, alpha=alpha)
    return asb.AeroBuildup(ap, op, xyz_ref=X["mass"]["cg"]).run()


S = WA["area"]
rows = np.array(WA["wing"])
al = rows[:, 0]
r = run(wing_only, al)
wing = []
for i, a in enumerate(al):
    wing.append({"alpha": a, "CL_wright": rows[i, 1], "CL_asb": float(np.ravel(r["CL"])[i]),
                 "CD_wright": rows[i, 2] + rows[i, 3], "CD_asb": float(np.ravel(r["CD"])[i])})

comp = {}
for name, fus in (("pod", [f for f in full.fuselages if f.name == "pod"]), ("booms", [f for f in full.fuselages if f.name.startswith("boom")])):
    ap = asb.Airplane(name, xyz_ref=X["mass"]["cg"], fuselages=fus, s_ref=S, c_ref=full.c_ref, b_ref=full.b_ref)
    comp[name] = {"DoverQ_asb_m2": float(np.ravel(run(ap, 0.0)["CD"])[0]) * S, "DoverQ_wright_m2": WA["parasite_at_v"][name]}
tails = asb.Airplane("tails", xyz_ref=X["mass"]["cg"], wings=full.wings[1:], s_ref=S, c_ref=full.c_ref, b_ref=full.b_ref)
comp["tail_at_0"] = {"DoverQ_asb_m2": float(np.ravel(run(tails, 0.0)["CD"])[0]) * S, "DoverQ_wright_m2": WA["parasite_at_v"]["tail"]}

extra = sum(X["extra_drag_area_m2"][k] for k in ("motors", "stopped_props", "legs", "excrescence"))
level = []
grid = np.arange(-4, 14.01, 0.5)
for t in WA["trims"]:
    v = t["v"]
    q = 0.5 * WA["rho"] * v * v
    cl_need = t["weight_n"] / (q * S)
    rr = run(full, grid, v)
    cl, cd = np.ravel(rr["CL"]), np.ravel(rr["CD"])
    if cl_need > cl.max():
        level.append({"v": v, "note": "beyond AeroBuildup CLmax"})
        continue
    k = int(np.argmax(cl >= cl_need))
    f = (cl_need - cl[k - 1]) / (cl[k] - cl[k - 1])
    a = grid[k - 1] + f * 0.5
    cdv = cd[k - 1] + f * (cd[k] - cd[k - 1])
    d_asb = q * (cdv * S + extra)
    level.append({"v": v, "alpha_asb_untrimmed": a, "alpha_wright_trimmed": t["alpha_deg"], "drag_asb_n": d_asb, "drag_wright_n": t["drag_n"], "asb_over_wright": d_asb / t["drag_n"] - 1})

out = {"V_ms": V, "wing": wing, "components": comp, "level_flight": level,
       "note": "AeroBuildup level flight is untrimmed (elevator neutral, tail lift where alpha puts it); Wright's is trimmed. Motor bells, stopped props, legs and excrescence (no AeroBuildup component) are Wright's drag areas in both."}
json.dump(out, open(os.path.join(OUT, "aero_audit.json"), "w"), indent=2, default=float)
print("wing alone (alpha, CL wright/asb, CD wright/asb):")
for w in wing[::3]:
    print(f"  {w['alpha']:5.1f}  {w['CL_wright']:.3f} {w['CL_asb']:.3f}   {w['CD_wright']:.4f} {w['CD_asb']:.4f}")
print("components D/q m^2 (wright, asb):", {k: (round(v['DoverQ_wright_m2'], 5), round(v['DoverQ_asb_m2'], 5)) for k, v in comp.items()})
for l_ in level:
    print("level", {k: (round(v, 3) if isinstance(v, float) else v) for k, v in l_.items()})
