"""Compare oracle and Rust outputs case by case: max relative difference
per (function, output), with the case where it occurs.

Fails (exit 1) on anything that is not a comparison that passed: a case
either side reports as an error (a domain feature off included), an output
that is null, non-numeric or present on one side only, a different number
of results than cases, or no comparison at all. Review 4 (finding 11): the
earlier version skipped all of these and passed when nothing was compared.
"""
import json, math, sys

# Tolerances: same law, same constants -> agree to round-off; different
# methods (published base pressures vs integrated) -> 1e-4.
TOL = {"us76": 2e-4, "nozzle_mach": 1e-9, "nozzle_area": 1e-6, "nozzle_cf": 1e-9,
       "nozzle_cstar": 1e-9, "balloon": 1e-9, "balloon_fill": 1e-6,
       # same formula, same constants -> round-off
       "tube_bending": 1e-9, "tube_buckling": 1e-9, "lame": 1e-9, "flutter": 1e-9,
       "bulkhead": 1e-9, "fspl": 1e-9, "dipole_gain": 1e-9,
       # GNSS, thermal, rocket aero and closures: same closed forms and the
       # same published constants on both sides -> round-off (the harmonic
       # and screw counts are integers and must agree exactly)
       "patch_rejection": 1e-9, "gnss_limits": 1e-9, "gnss_harmonics": 0.0, "gnss_coexist": 1e-9,
       "warmer_power": 1e-9, "box_thermal": 1e-9, "capacity_fraction": 1e-9,
       "von_karman": 1e-9, "stagnation_cd": 1e-9, "nose_cd": 1e-9, "descent_rate": 1e-9,
       "closure_capacity": 1e-9, "closure_screws": 0.0,
       # the elevation by arctangent (oracle) vs arcsine (Rust): the same
       # angle through two functions of a slant range with ~1e-10 of
       # round-off in it
       "radio_geometry": 1e-6,
       # PCB (Oracle.Pcb): positions and extents are integer nm and must
       # agree exactly; connectivity and counts are integers
       "pcb_pads": 0.0, "pcb_courtyard": 0.0, "pcb_connectivity": 0.0,
       # IPC-2221 current: same closed form -> round-off
       "pcb_ampacity": 1e-9, "pcb_regulator": 1e-9}

# Absolute tolerances (difference, not relative), in the output's unit:
# copper distances to +-1 nm.
ATOL = {"pcb_clearance": 1.0}


def is_num(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def main(paths):
    def load(p):
        with open(p) as f:
            return json.load(f)
    cases, hs, rs = (load(p) for p in paths)
    problems = []
    if not (len(cases) == len(hs) == len(rs)):
        problems.append(f"length mismatch: {len(cases)} cases, {len(hs)} oracle results, {len(rs)} rust results")
    worst = {}
    compared = 0
    # the outputs Rust reports for each function: one missing from any case
    # of that function (either side) is a failure; an output only the
    # oracle ever computes (a diagnostic such as geopotential height) is
    # listed, not compared
    rust_keys, oracle_only = {}, {}
    for c, r in zip(cases, rs):
        if isinstance(r, dict) and "error" not in r:
            fn = c["rust_fn"] + " vs " + c["fn"] if "rust_fn" in c else c["fn"]
            rust_keys.setdefault(fn, set()).update(k for k in r if k not in c and k != "error")
    for i, (c, h, r) in enumerate(zip(cases, hs, rs)):
        name = c["rust_fn"] + " vs " + c["fn"] if "rust_fn" in c else c["fn"]
        # a board case carries the whole board: show its name instead
        at = {kk: vv for kk, vv in c.items() if kk not in ("fn", "rust_fn", "board")}
        if not isinstance(r, dict) or not isinstance(h, dict):
            problems.append(f"case {i} {name}: result is not an object")
            continue
        if "error" in r or "error" in h:
            problems.append(f"case {i} {name}: error ({r.get('error') or h.get('error')})")
            continue
        if "board" in c:
            # per-item outputs differ from board to board: compare every
            # output of a quantity Rust reports, from either side
            fams = {k.partition(":")[0] for k in rust_keys.get(name, set())}
            outs = (set(r) | {k for k in h if k.partition(":")[0] in fams}) - set(c) - {"error"}
        else:
            outs = (set(r) | rust_keys.get(name, set())) - set(c) - {"error"}
        for k in set(h) - outs - set(c):
            oracle_only.setdefault(name, set()).add(k.partition(":")[0])
        if not outs:
            problems.append(f"case {i} {name}: no outputs")
        for k in sorted(outs):
            rv, hv = r.get(k), h.get(k)
            if not is_num(rv) or not is_num(hv):
                problems.append(f"case {i} {name} {k}: oracle {json.dumps(hv)} / rust {json.dumps(rv)} (missing, null or not a number)")
                continue
            if name in ATOL:
                d = abs(rv - hv)
            else:
                d = abs(rv - hv) / max(abs(hv), 1e-300)
            compared += 1
            # per-item outputs ("pad_x:U1#0") are grouped by quantity, the
            # item kept with the worst case
            fam, _, item = k.partition(":")
            key = (name, fam)
            where = dict(at, item=item) if item else at
            prev = worst.get(key)
            # NaN is the worst there is and stays the worst
            if prev is None or (not math.isnan(prev[0]) and (math.isnan(d) or d > prev[0])):
                worst[key] = (d, where, hv, rv)

    bad = 0
    print(f"{'rust fn vs oracle fn':<34} {'output':<16} {'max rel diff':>12}  tol    at (oracle / rust)")
    for (name, k), (d, at, hv, rv) in sorted(worst.items()):
        tol = ATOL.get(name, TOL.get(name, TOL["us76"] if name.startswith("us76") else 1e-6))
        over = not d <= tol  # NaN exceeds
        bad += over
        flag = "  <-- exceeds" if over else ""
        print(f"{name:<34} {k:<16} {d:12.3e}  {tol:.0e}  {json.dumps(at)} ({hv:.6g} / {rv:.6g}){flag}")
    for name, ks in sorted(oracle_only.items()):
        print(f"(not compared: oracle-only outputs of {name}: {', '.join(sorted(ks))})")
    for p in problems:
        print(f"FAIL {p}")
    if compared == 0:
        print("FAIL nothing was compared")
    print(f"{compared} comparisons, {bad} over tolerance, {len(problems)} problems")
    return 1 if (bad or problems or compared == 0) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:4]))
