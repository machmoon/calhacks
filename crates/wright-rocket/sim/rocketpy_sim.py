"""Fly a Wright rocket design in RocketPy (6-DOF) and render what it did.

    python -I rocketpy_sim.py <input.json> <out_dir>

input.json is written by wright-rocket `sim::sim_input`. Writes into out_dir:
result.json (RocketPy vs Wright apogees and scores), flight.csv, plots (png),
flight.mp4 (trajectory + attitude + stability animation) and manifest.json
({kind:"sim", title, items:[{label, path, media, summary, metrics}]}).

The RocketPy objects follow its liquid-motor example (docs/examples, and
rocketpy/motors/liquid_motor.py, tank.py, rocket/rocket.py in 1.13.0):
LiquidMotor + MassFlowRateBasedTank on CylindricalTank geometry, Rocket in
nose_to_tail coordinates, von Karman nose, trapezoidal fins, two parachutes.
"""
import json
import math
import os
import shutil
import sys

import numpy as np
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib import animation  # noqa: E402
from rocketpy import (  # noqa: E402
    CylindricalTank,
    Environment,
    Flight,
    Fluid,
    LiquidMotor,
    MassFlowRateBasedTank,
    Rocket,
)

FT = 0.3048


def far_score(apogee, contract):
    # FAR-OUT 2027 7.3 (same as wright-rocket rules::far_altitude_score)
    return max(0.0, 1000.0 - 2000.0 * abs(apogee / contract - 1.0))


def environment(cfg, w10):
    s, w = cfg["site"], cfg["wind"]
    env = Environment(gravity=s["gravity_m_s2"], latitude=s["latitude"], longitude=s["longitude"], elevation=s["elevation_m"])
    hs = np.concatenate([[0.0, 1.0], np.geomspace(2.0, 40000.0, 120)])
    speed = w10 * (np.maximum(hs, 1.0) / 10.0) ** w["power_law_exponent"]
    # wind blowing toward heading `toward` (deg from north): u east, v north
    to = math.radians(w["toward_heading_deg"])
    u = [[s["elevation_m"] + h, sp * math.sin(to)] for h, sp in zip(hs, speed)]
    v = [[s["elevation_m"] + h, sp * math.cos(to)] for h, sp in zip(hs, speed)]
    env.set_atmospheric_model(type="custom_atmosphere", wind_u=u, wind_v=v)
    return env


def rocket(cfg, thrust_scale=1.0, drag_scale=1.0, chutes=True):
    r, m = cfg["rocket"], cfg["motor"]
    L = r["length_m"]
    tb = m["burn_time_s"]
    thrust = [[t, f * thrust_scale] for t, f in m["thrust_vac_n"]]
    motor = LiquidMotor(
        thrust_source=thrust,
        dry_mass=0.0,
        dry_inertia=(0.0, 0.0, 0.0),
        nozzle_radius=m["nozzle_exit_radius_m"],
        center_of_dry_mass_position=0.0,
        nozzle_position=0.0,
        burn_time=tb,
        coordinate_system_orientation="nozzle_to_combustion_chamber",
        reference_pressure=0.0,
    )
    gas = Fluid(name="ullage gas", density=1.0)
    for tk in m["tanks"]:
        ts = np.array([p[0] for p in tk["mass_kg"]])
        ms = np.array([p[1] for p in tk["mass_kg"]])
        mdot = np.maximum(-np.gradient(ms, ts), 0.0)
        # scale so RocketPy's integral of the outflow equals Wright's drained mass
        drained = ms[0] - ms[-1]
        integ = np.trapezoid(mdot, ts)
        if integ > 0:
            mdot *= drained / integ
        tank = MassFlowRateBasedTank(
            name=tk["name"],
            geometry=CylindricalTank(radius=tk["inner_radius_m"], height=tk["length_m"], spherical_caps=False),
            flux_time=tb,
            liquid=Fluid(name=tk["name"] + " liquid", density=tk["density_kg_m3"]),
            gas=gas,
            initial_liquid_mass=float(ms[0]),
            initial_gas_mass=0.0,
            liquid_mass_flow_rate_in=0.0,
            gas_mass_flow_rate_in=0.0,
            liquid_mass_flow_rate_out=[[float(t), float(x)] for t, x in zip(ts, mdot)],
            gas_mass_flow_rate_out=0.0,
        )
        motor.add_tank(tank, position=L - tk["center_from_nose_m"])
    d = cfg["drag"]
    rk = Rocket(
        radius=r["radius_m"],
        mass=r["dry_mass_kg"],
        inertia=tuple(r["dry_inertia"]),
        power_off_drag=[[mm, c * drag_scale] for mm, c in d["power_off"]],
        power_on_drag=[[mm, c * drag_scale] for mm, c in d["power_on"]],
        center_of_mass_without_motor=r["dry_cg_from_nose_m"],
        coordinate_system_orientation="nose_to_tail",
    )
    rk.add_motor(motor, position=L)
    rk.add_nose(length=r["nose_length_m"], kind=r["nose_kind"], position=0.0)
    f = cfg["fins"]
    rk.add_trapezoidal_fins(n=f["n"], root_chord=f["root_m"], tip_chord=f["tip_m"], span=f["span_m"], position=f["le_from_nose_m"], sweep_length=f["sweep_m"])
    if chutes:
        rec = cfg["recovery"]
        rk.add_parachute("Drogue", cd_s=rec["drogue_cd_s_m2"], trigger="apogee", sampling_rate=100, lag=1.0)
        rk.add_parachute("Main", cd_s=rec["main_cd_s_m2"], trigger=rec["main_deploy_agl_m"], sampling_rate=100, lag=1.0)
    return rk


def fly(cfg, w10, ts=1.0, ds=1.0, full=False):
    rail = cfg["rail"]
    return Flight(
        rocket=rocket(cfg, ts, ds, chutes=full),
        environment=environment(cfg, w10),
        rail_length=rail["length_m"],
        inclination=rail["inclination_deg"],
        heading=rail["heading_deg"],
        terminate_on_apogee=not full,
        max_time=900,
    )


def main():
    cfg = json.load(open(sys.argv[1]))
    out = sys.argv[2]
    os.makedirs(out, exist_ok=True)
    elev = cfg["site"]["elevation_m"]
    contract = cfg["contract_agl_m"]
    winds = cfg["wind"]["speeds_10m_m_s"]

    # Dispersion set: the same 9 cases Wright flies (wind x thrust x drag).
    rows = []
    wd = {r["case"]: r["apogee_agl_m"] for r in cfg["wright"]["dispersion"]}
    wlist = list(wd.values())
    i = 0
    for w in winds:
        for ts, ds in cfg["dispersion"]:
            f = fly(cfg, w, ts, ds)
            a = f.apogee - elev
            rows.append({"wind_10m_m_s": w, "thrust_scale": ts, "drag_scale": ds, "rocketpy_apogee_agl_m": a, "wright_apogee_agl_m": wlist[i], "rocketpy_max_mach": f.max_mach_number})
            i += 1
    rp_score = float(np.mean([far_score(r["rocketpy_apogee_agl_m"], contract) for r in rows]))
    w_score = float(np.mean([far_score(r["wright_apogee_agl_m"], contract) for r in rows]))

    # Nominal (mid wind) full flight with recovery, for the plots and film.
    nom = fly(cfg, winds[1], full=True)
    t_apo = nom.apogee_time
    t_end = nom.t_final
    t_rail = nom.out_of_rail_time
    W = cfg["wright"]
    res = {
        "rocketpy_version": __import__("importlib.metadata").metadata.version("rocketpy"),
        "name": cfg["name"],
        "contract_agl_m": contract,
        "nominal": {
            "rocketpy_apogee_agl_m": nom.apogee - elev,
            "wright_apogee_agl_m": W["nominal_apogee_agl_m"],
            "apogee_diff_pct": (nom.apogee - elev - W["nominal_apogee_agl_m"]) / W["nominal_apogee_agl_m"] * 100,
            "rocketpy_max_mach": nom.max_mach_number,
            "wright_max_mach": W["nominal_max_mach"],
            "rocketpy_rail_exit_m_s": nom.out_of_rail_velocity,
            "wright_rail_exit_m_s": W["rail_exit_m_s"],
            "rocketpy_stability_at_rail_exit_cal": nom.out_of_rail_stability_margin,
            "wright_stability_at_rail_exit_cal": W["margin_at_rail_exit_cal"],
            "rocketpy_min_stability_cal": nom.min_stability_margin,
            "apogee_time_s": t_apo,
            "landing_time_s": t_end,
            "drift_at_landing_m": float(math.hypot(nom.x(t_end), nom.y(t_end))),
        },
        "dispersion": rows,
        "far_score_mean": {"rocketpy": rp_score, "wright": w_score},
    }
    json.dump(res, open(os.path.join(out, "result.json"), "w"), indent=1)

    # Time series (CSV) of the nominal flight.
    ts = np.linspace(0.0, t_end, 1500)
    ascent = ts[ts <= t_apo]
    cols = {
        "t_s": ts,
        "x_east_m": [nom.x(t) for t in ts],
        "y_north_m": [nom.y(t) for t in ts],
        "altitude_agl_m": [nom.z(t) - elev for t in ts],
        "vz_m_s": [nom.vz(t) for t in ts],
        "mach": [nom.mach_number(t) for t in ts],
        "attitude_deg": [nom.attitude_angle(t) for t in ts],
        "aoa_deg": [nom.angle_of_attack(t) for t in ts],
        "stability_cal": [nom.stability_margin(t) for t in ts],
    }
    with open(os.path.join(out, "flight.csv"), "w") as fh:
        fh.write(",".join(cols) + "\n")
        for k in range(len(ts)):
            fh.write(",".join(f"{cols[c][k]:.6g}" for c in cols) + "\n")

    wt = np.array(W["trace"]) if W["trace"] else np.zeros((0, 6))
    # Static plots.
    figs = []
    fig, ax = plt.subplots(1, 2, figsize=(11, 4.5))
    ax[0].plot(ts / 1, np.array(cols["altitude_agl_m"]) / FT, label="RocketPy 6-DOF")
    if len(wt):
        ax[0].plot(wt[:, 0], wt[:, 2] / FT, "--", label="Wright trajectory.rs (3-DOF)")
    ax[0].axhline(contract / FT, color="gray", lw=0.8, ls=":", label="contract")
    ax[0].set(xlabel="time, s", ylabel="altitude AGL, ft", title="Altitude (nominal, mid wind)")
    ax[0].legend()
    ax[1].plot(np.array(cols["x_east_m"]) / FT, np.array(cols["altitude_agl_m"]) / FT, label="RocketPy (incl. descent)")
    if len(wt):
        ax[1].plot(wt[:, 1] / FT, wt[:, 2] / FT, "--", label="Wright (ascent)")
    ax[1].set(xlabel="downrange east, ft", ylabel="altitude AGL, ft", title="Trajectory")
    ax[1].legend()
    fig.tight_layout()
    fig.savefig(os.path.join(out, "trajectory.png"), dpi=110)
    plt.close(fig)
    figs.append(("Trajectory: RocketPy vs Wright", "trajectory.png", f"apogee RocketPy {res['nominal']['rocketpy_apogee_agl_m'] / FT:,.0f} ft vs Wright {W['nominal_apogee_agl_m'] / FT:,.0f} ft ({res['nominal']['apogee_diff_pct']:+.1f} %)", {"rocketpy_apogee_ft": res["nominal"]["rocketpy_apogee_agl_m"] / FT, "wright_apogee_ft": W["nominal_apogee_agl_m"] / FT}))

    fig, ax = plt.subplots(1, 3, figsize=(14, 4))
    ax[0].plot(ascent, [nom.attitude_angle(t) for t in ascent], label="attitude")
    ax[0].plot(ascent, [nom.path_angle(t) for t in ascent], "--", label="flight path")
    ax[0].set(xlabel="time, s", ylabel="deg from horizontal", title="Attitude (ascent)")
    ax[0].legend()
    ax[1].plot(ascent, [nom.angle_of_attack(t) for t in ascent])
    ax[1].set(xlabel="time, s", ylabel="deg", title="Angle of attack")
    ax[2].plot(ascent, [nom.stability_margin(t) for t in ascent], label="RocketPy")
    if len(wt):
        ax[2].plot(wt[:, 0], wt[:, 5], "--", label="Wright")
    ax[2].axhline(1.5, color="gray", lw=0.8, ls=":")
    ax[2].set(xlabel="time, s", ylabel="cal", title="Stability margin")
    ax[2].legend()
    fig.tight_layout()
    fig.savefig(os.path.join(out, "attitude_stability.png"), dpi=110)
    plt.close(fig)
    figs.append(("Attitude, angle of attack, stability", "attitude_stability.png", f"stability at rail exit RocketPy {nom.out_of_rail_stability_margin:.2f} cal vs Wright {W['margin_at_rail_exit_cal']:.2f} cal; min {nom.min_stability_margin:.2f} cal", {"rocketpy_rail_exit_cal": nom.out_of_rail_stability_margin, "wright_rail_exit_cal": W["margin_at_rail_exit_cal"]}))

    fig, ax = plt.subplots(figsize=(7, 4))
    k = np.arange(len(rows))
    ax.bar(k - 0.2, [r["rocketpy_apogee_agl_m"] / FT for r in rows], 0.4, label="RocketPy")
    ax.bar(k + 0.2, [r["wright_apogee_agl_m"] / FT for r in rows], 0.4, label="Wright")
    ax.axhline(contract / FT, color="gray", lw=0.8, ls=":")
    ax.set_xticks(k, [f"{r['wind_10m_m_s']:.0f} m/s\nT{r['thrust_scale']} D{r['drag_scale']}" for r in rows], fontsize=7)
    ax.set(ylabel="apogee AGL, ft", title=f"Dispersion: FAR score RocketPy {rp_score:.0f}, Wright {w_score:.0f}")
    ax.set_ylim(min(r["rocketpy_apogee_agl_m"] for r in rows) / FT * 0.8, None)
    ax.legend()
    fig.tight_layout()
    fig.savefig(os.path.join(out, "dispersion.png"), dpi=110)
    plt.close(fig)
    figs.append(("Dispersion apogees", "dispersion.png", f"mean FAR-OUT score RocketPy {rp_score:.0f} vs Wright {w_score:.0f} over the same 9 cases", {"rocketpy_far_score": rp_score, "wright_far_score": w_score}))

    # Animation: trajectory with the rocket drawn at its attitude, plus
    # altitude and stability traces filling in.
    items = []
    ff = shutil.which("ffmpeg")
    if ff:
        plt.rcParams["animation.ffmpeg_path"] = ff
        fps = 30
        n = int(min(t_end, 20.0 + t_apo) * fps / 2)  # 2x speed
        tt = np.linspace(0.0, min(t_end, t_apo + 20.0), n)
        X = np.array([nom.x(t) for t in tt]) / FT
        Z = np.array([nom.z(t) - elev for t in tt]) / FT
        fig = plt.figure(figsize=(12, 6.75), dpi=100)
        g = fig.add_gridspec(2, 2, width_ratios=[1.3, 1])
        a0 = fig.add_subplot(g[:, 0])
        a1 = fig.add_subplot(g[0, 1])
        a2 = fig.add_subplot(g[1, 1])
        a0.set(xlim=(min(X.min(), -500), max(X.max(), 500) * 1.1), ylim=(0, Z.max() * 1.08), xlabel="downrange east, ft", ylabel="altitude AGL, ft")
        a0.axhline(contract / FT, color="gray", ls=":", lw=0.8)
        if len(wt):
            a0.plot(wt[:, 1] / FT, wt[:, 2] / FT, color="tab:orange", lw=1, ls="--", label="Wright 3-DOF")
        (path,) = a0.plot([], [], color="tab:blue", lw=1.5, label="RocketPy 6-DOF")
        (body,) = a0.plot([], [], color="k", lw=3)
        a0.legend(loc="upper left")
        title = a0.set_title("")
        a1.set(xlim=(0, tt[-1]), ylim=(0, Z.max() * 1.08), ylabel="altitude, ft")
        (alt,) = a1.plot([], [])
        a2.set(xlim=(0, tt[-1]), ylabel="stability, cal / AoA, deg", xlabel="time, s")
        sm = np.array([nom.stability_margin(t) if t <= t_apo else np.nan for t in tt])
        aoa = np.array([nom.angle_of_attack(t) if t <= t_apo else np.nan for t in tt])
        a2.set_ylim(min(0, np.nanmin(aoa)), max(np.nanmax(sm), np.nanmax(aoa)) * 1.1)
        (stab,) = a2.plot([], [], label="stability margin")
        (aa,) = a2.plot([], [], label="angle of attack")
        a2.legend(loc="upper right")
        span = (Z.max() * 0.04)

        def frame(k):
            t = tt[k]
            path.set_data(X[: k + 1], Z[: k + 1])
            th = math.radians(nom.attitude_angle(t))
            body.set_data([X[k] - span * math.cos(th) * 0.5, X[k] + span * math.cos(th) * 0.5], [Z[k] - span * math.sin(th) * 0.5, Z[k] + span * math.sin(th) * 0.5])
            alt.set_data(tt[: k + 1], Z[: k + 1])
            stab.set_data(tt[: k + 1], sm[: k + 1])
            aa.set_data(tt[: k + 1], aoa[: k + 1])
            phase = "boost" if t < cfg["motor"]["burn_time_s"] else ("coast" if t < t_apo else "drogue")
            title.set_text(f"{cfg['name']}  t = {t:5.1f} s  {phase}  alt {Z[k]:,.0f} ft  Mach {nom.mach_number(t):.2f}")
            return path, body, alt, stab, aa, title

        anim = animation.FuncAnimation(fig, frame, frames=n, blit=False)
        anim.save(os.path.join(out, "flight.mp4"), writer=animation.FFMpegWriter(fps=fps, bitrate=2400))
        plt.close(fig)
        items.append({"label": "Flight animation (RocketPy 6-DOF)", "path": "flight.mp4", "media": "mp4", "summary": f"nominal flight at mid wind, 2x speed: boost, coast to {res['nominal']['rocketpy_apogee_agl_m'] / FT:,.0f} ft AGL, drogue", "metrics": {"apogee_ft": res["nominal"]["rocketpy_apogee_agl_m"] / FT, "max_mach": nom.max_mach_number}})

    for label, path, summ, met in figs:
        items.append({"label": label, "path": path, "media": "png", "summary": summ, "metrics": met})
    items.append({"label": "Nominal flight time series", "path": "flight.csv", "media": "csv", "summary": "t, position, Mach, attitude, angle of attack, stability margin (RocketPy)", "metrics": {}})
    items.append({"label": "RocketPy vs Wright numbers", "path": "result.json", "media": "json", "summary": f"nominal apogee {res['nominal']['apogee_diff_pct']:+.1f} % vs Wright; FAR score {rp_score:.0f} vs {w_score:.0f}", "metrics": {"apogee_diff_pct": res["nominal"]["apogee_diff_pct"]}})
    items.append({"label": "RocketPy input (from Wright's design)", "path": os.path.basename(sys.argv[1]), "media": "json", "summary": "masses, tanks, thrust curve, CD(M), fins, recovery, site", "metrics": {}})
    json.dump({"kind": "sim", "title": f"6-DOF flight, RocketPy {res['rocketpy_version']}: {cfg['name']}", "items": items}, open(os.path.join(out, "manifest.json"), "w"), indent=1)
    print(json.dumps({"nominal": res["nominal"], "far_score_mean": res["far_score_mean"]}, indent=1))


if __name__ == "__main__":
    main()
