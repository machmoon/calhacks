"""The shared case list for haskell/crosscheck.sh (JSON array on stdout).

"fn" is the oracle's function; "rust_fn" (if present) is the probe's name
for the same quantity when several Rust crates implement it.
"""
import json

cases = []
for z in range(0, 50001, 500):
    for rust in ("us76", "us76_balloon"):
        cases.append({"fn": "us76", "rust_fn": rust, "z_m": float(z)})
    if z <= 11000:
        # wright-air's ISA takes geometric altitude (converted to
        # geopotential since the 2026-10-24 cross-check finding).
        cases.append({"fn": "us76", "rust_fn": "us76_air", "z_m": float(z)})

gammas = (1.13, 1.2, 1.25, 1.3, 1.4)
for g in gammas:
    for m in (1.2, 1.5, 2.0, 3.0, 4.0, 5.0):
        cases.append({"fn": "nozzle_mach", "gamma": g, "mach": m})
    for e in (1.5, 2.0, 4.0, 8.0, 16.0, 40.0, 100.0):
        cases.append({"fn": "nozzle_area", "gamma": g, "area_ratio": e, "supersonic": True})
    for pe in (0.002, 0.01, 0.05):
        for pa in (0.0, 0.01):
            for e in (4.0, 10.0):
                cases.append({"fn": "nozzle_cf", "gamma": g, "pe_pc": pe, "pa_pc": pa, "area_ratio": e})
    for tc in (2800.0, 3400.0):
        for mm in (0.020, 0.024):
            cases.append({"fn": "nozzle_cstar", "gamma": g, "tc_k": tc, "molar_mass_kg_mol": mm})

for mb in (0.6, 1.0, 1.2):
    for mp in (0.5, 1.0, 2.0):
        for r in (0.9, 1.1, 1.3, 1.6):
            cases.append({"fn": "balloon", "radius_m": r, "balloon_kg": mb, "payload_kg": mp,
                          "cd": 0.3, "burst_diameter_m": 7.86})
        for v in (3.0, 5.0, 7.0):
            cases.append({"fn": "balloon_fill", "ascent_m_s": v, "balloon_kg": mb, "payload_kg": mp, "cd": 0.3})

# structures: the VTOL boom's bending and SP-8007 shell buckling
# (wright-air perf.rs), the rocket's tank walls, fins and bulkheads
# (wright-rocket domain.rs, vehicle.rs); mm -> the probe converts
for od, idm in ((21.0, 19.0), (16.0, 14.0), (12.0, 10.0), (30.0, 27.0)):
    for m in (5.0, 25.0, 120.0):
        cases.append({"fn": "tube_bending", "od_mm": od, "id_mm": idm, "moment_nm": m})
    for e in (70e9, 140e9):
        cases.append({"fn": "tube_buckling", "od_mm": od, "id_mm": idm, "modulus_pa": e})
for p in (1e6, 3e6, 6e6):
    for ri, ro in ((0.099, 0.101), (0.0735, 0.0762), (0.045, 0.0475)):
        cases.append({"fn": "lame", "p_pa": p, "ri_m": ri, "ro_m": ro})
for p in (101325.0, 50000.0, 20000.0):
    for a in (300.0, 340.0):
        for span, root, tip, t in ((0.10, 0.20, 0.10, 0.0030), (0.15, 0.25, 0.08, 0.0048), (0.08, 0.12, 0.06, 0.0024)):
            cases.append({"fn": "flutter", "a_m_s": a, "p_pa": p, "g_pa": 4.0e9, "span_m": span, "root_m": root, "tip_m": tip, "t_m": t})
for r in (0.05, 0.0762, 0.1):
    for p in (2e6, 4e6):
        cases.append({"fn": "bulkhead", "radius_m": r, "p_pa": p, "sf": 1.5, "allow_pa": 240e6})

# radio: the balloon's link budget (wright-balloon radio.rs); ground
# distances from 1 km so the Rust arcsine form stays away from its
# round-off edge at zero ground distance
for d in (1.0, 1000.0, 50e3, 200e3):
    for f in (144.39, 433.0, 915.0, 1280.0, 2400.0):
        cases.append({"fn": "fspl", "d_m": d, "f_mhz": f})
for g in (1e3, 1e4, 1e5, 4e5):
    for h1 in (0.0, 150.0):
        for h2 in (100.0, 1e4, 3e4):
            cases.append({"fn": "radio_geometry", "ground_m": g, "station_alt_m": h1, "payload_alt_m": h2})
for el in (-0.2, 0.0, 0.1, 0.5, 1.0, 1.5, 1.5707963):
    cases.append({"fn": "dipole_gain", "peak_dbi": 2.15, "el_rad": el})

# GNSS coexistence (wright-balloon radio.rs, used by analysis.rs): the
# payload's own transmitters against the L1 patch and the M10 front end;
# separations from 20 mm (inside the radian sphere at 144 MHz) to 0.5 m
for f in (144.39, 433.0, 868.0, 915.0, 1280.0, 2400.0, 5800.0):
    cases.append({"fn": "patch_rejection", "f_mhz": f})
    cases.append({"fn": "gnss_harmonics", "f_mhz": f})
    for sep in (0.02, 0.05, 0.15, 0.5):
        for tx in (10.0, 20.0, 30.0):
            cases.append({"fn": "gnss_coexist", "tx_dbm": tx, "gain_dbi": 2.15, "sep_m": sep, "f_mhz": f})
for f in (157.542, 196.9275, 225.06, 262.57, 315.084, 393.855, 525.14, 787.71):  # L1 / n
    cases.append({"fn": "gnss_harmonics", "f_mhz": f})
cases.append({"fn": "gnss_limits"})

# thermal (wright-balloon rules.rs warmer_power_sl_w, analysis.rs
# capacity_fraction, box_step): the lumped box step is called on bare
# numbers; the whole-flight ThermalModel::case still needs a Design
for h in (8.0, 10.0, 12.0, 18.0):
    cases.append({"fn": "warmer_power", "rated_h": h})
# the box's Euler step (analysis.rs box_step) iterated: heat capacity,
# R_th, heating, ambient, dt, steps, start
for cap, rth in ((400.0, 3.5), (900.0, 1.2), (2500.0, 6.0)):
    for p, amb, t0 in ((0.0, -55.0, 20.0), (3.0, -30.0, 5.0), (12.0, 15.0, -10.0)):
        for dt, n in ((5.0, 1), (5.0, 200), (30.0, 600)):
            cases.append({"fn": "box_thermal", "heat_cap_j_k": cap, "r_th_k_w": rth, "p_w": p,
                          "amb_c": amb, "dt_s": dt, "steps": n, "t0_c": t0})
for chem in ("lipo", "li_ion", "lithium_primary"):
    for t in (-50.0, -40.0, -30.0, -20.005, -20.0, -15.0, -10.0, -5.0, 0.0, 10.0, 21.0, 25.0, 40.0):
        cases.append({"fn": "capacity_fraction", "chemistry": chem, "t_c": t})

# rocket aero (wright-rocket aero.rs), recovery and closures (domain.rs,
# vehicle.rs)
for x in (0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0):
    for l, r in ((0.5, 0.05), (0.381, 0.0381), (0.76, 0.0762)):
        cases.append({"fn": "von_karman", "x_m": x * l, "l_m": l, "r_m": r})
for m in (0.3, 0.8, 0.9, 0.95, 1.0, 1.05, 1.13, 1.2, 1.4, 1.6, 2.0, 2.5, 3.0, 3.5):
    cases.append({"fn": "stagnation_cd", "mach": m})
    for fin in (3.0, 4.0, 5.0, 6.0):
        cases.append({"fn": "nose_cd", "mach": m, "fineness": fin})
for mass in (2.0, 8.0, 25.0):
    for d_in in (18.0, 36.0, 72.0, 120.0):
        for rho in (1.225, 0.9):
            cases.append({"fn": "descent_rate", "mass_kg": mass, "diameter_in": d_in, "cd": 1.5, "rho": rho})
for wall in (0.002, 0.003, 0.0048, 0.00635):
    cases.append({"fn": "closure_capacity", "wall_m": wall})
    for d in (0.075, 0.1, 0.15):
        for p in (2e6, 4e6, 6e6):
            cases.append({"fn": "closure_screws", "inner_d_m": d, "wall_m": wall, "meop_pa": p})

# PCB (Oracle.Pcb vs wright-eda; docs/electronics-plan.md section 4): one
# case per board and quantity. crosscheck.sh sets PCB_DIR to the boards the
# probe wrote (`wright-crosscheck gen-pcb`: SYNTHETIC copper on the real
# netlists of evals/electronics/fixtures until lanes C and D place and
# route them). pcb_clearance / pcb_ampacity / pcb_regulator compare with
# wright-electronics (lane E) copper::min_gap_nm, narrowest_tracks and
# checks::regulator_vout.
import glob, os
if os.environ.get("PCB_DIR"):
    for path in sorted(glob.glob(os.path.join(os.environ["PCB_DIR"], "*.json"))):
        with open(path) as f:
            board = json.load(f)
        # the setpoint only where there is a TPS5450 to read (elsewhere
        # both sides answer "no TPS5450", which the comparison counts as an
        # error, not an agreement)
        has_reg = any(p["value"].startswith("TPS5450") or p["name"].startswith("TPS5450") for p in board[0]["parts"])
        for fn in ("pcb_pads", "pcb_courtyard", "pcb_connectivity", "pcb_clearance", "pcb_ampacity") + (("pcb_regulator",) if has_reg else ()):
            cases.append({"fn": fn, "name": os.path.basename(path)[:-5], "board": board})

json.dump(cases, __import__("sys").stdout)
