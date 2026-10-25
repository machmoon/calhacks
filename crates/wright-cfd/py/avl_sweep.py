"""Alpha sweep of an AVL geometry through OptVL (github.com/joanibal/OptVL).

Called by wright-cfd as `python -I avl_sweep.py case.json out.json`.
API used, as in OptVL `optvl/optvl_class.py` (v2.5.0):
  OVLSolver(geo_file=...), set_parameter("Mach", M), set_variable("alpha", deg),
  execute_run(), get_total_forces() (CL, CD, CDi, CDff, Cm, e),
  get_stab_derivs() ("dCL/dalpha" per rad, "neutral point"),
  get_strip_forces() (per-surface "Y LE", "chord", "CL strip", "lift dist"),
  avl.cpoml(False) + get_cp_data() (surface mesh and Cp, as `plot_cp` does).
Nothing here computes aerodynamics; it only moves AVL's numbers into JSON.
"""
import json
import sys

import numpy as np
from optvl import OVLSolver


def f(x):
    a = np.asarray(x, dtype=float)
    if a.ndim > 0:
        return None
    x = float(a)
    return x if np.isfinite(x) else None


def main(case_path, out_path):
    case = json.load(open(case_path))
    ovl = OVLSolver(geo_file=case["avl_file"], debug=False)
    ovl.set_parameter("Mach", case.get("mach", 0.0))
    rows = []
    for a in case["alphas"]:
        ovl.set_variable("alpha", a)
        ovl.execute_run()
        t = ovl.get_total_forces()
        row = {"alpha": a}
        for k in ("CL", "CD", "CDi", "CDff", "Cm", "e", "CLff"):
            row[k] = f(t.get(k, float("nan")))
        sd = ovl.get_stab_derivs()
        row["CLa"] = f(sd.get("dCL/dalpha", float("nan")))
        row["Cma"] = f(sd.get("dCm/dalpha", float("nan")))
        row["Xnp"] = f(sd.get("neutral point", float("nan")))
        strips = ovl.get_strip_forces()
        row["strips"] = {
            name: {
                "y": [f(v) for v in s["Y LE"]],
                "chord": [f(v) for v in s["chord"]],
                "cl": [f(v) for v in s["CL strip"]],
                "lift_dist": [f(v) for v in s["lift dist"]],
            }
            for name, s in strips.items()
        }
        if case.get("cp", True):
            ovl.avl.cpoml(False)
            xyz, cp = ovl.get_cp_data()
            row["surfaces"] = [
                {"xyz": np.asarray(x).tolist(), "cp": np.asarray(c).tolist()} for x, c in zip(xyz, cp)
            ]
        rows.append(row)
    ref = ovl.get_reference_data()
    json.dump({"reference": {k: f(v) for k, v in ref.items() if np.asarray(v).ndim == 0}, "rows": rows}, open(out_path, "w"))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
