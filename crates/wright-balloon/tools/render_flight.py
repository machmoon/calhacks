"""Animate a balloon flight written by wright_balloon::sim.

usage: python -I render_flight.py tracks.csv comparison.json OUT_DIR

Writes OUT_DIR/flight.mp4 (matplotlib FFMpegWriter, system ffmpeg) and
OUT_DIR/flight.png. Plain axes: ground track (lon/lat, equal-distance
aspect) beside altitude against time, one marker per track moving in time.
"""
import csv
import json
import math
import sys
from collections import defaultdict

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib.animation import FFMpegWriter, FuncAnimation  # noqa: E402

tracks_csv, comparison_json, out = sys.argv[1], sys.argv[2], sys.argv[3]
cmp = json.load(open(comparison_json))
tracks = defaultdict(list)
for r in csv.DictReader(open(tracks_csv)):
    tracks[r["track"]].append((float(r["t_s"]), float(r["lat"]), float(r["lon"]), float(r["alt_m"]), r["phase"]))

STYLE = {
    "tawhiri_api": dict(color="#1f77b4", ls="-", lw=2.4),
    "wright_gfs": dict(color="#d62728", ls="-", lw=1.8),
    "tawhiri_port_gfs": dict(color="#2ca02c", ls="--", lw=1.4),
    "wright_assumed": dict(color="#7f7f7f", ls=":", lw=1.4),
}
order = [k for k in STYLE if k in tracks]
launch = cmp["launch"]
lat0 = launch["lat"]
# towns for orientation (approximate centres)
TOWNS = [("Bakersfield", 35.373, -119.019), ("Fresno", 36.738, -119.787), ("Visalia", 36.330, -119.292), ("Hanford", 36.327, -119.646), ("Lost Hills", 35.616, -119.694)]

lats = [p[1] for k in order for p in tracks[k]] + [lat0]
lons = [p[2] for k in order for p in tracks[k]] + [launch["lon"]]
# a square box in km around every track, so an east-west flight is not a
# sliver
k = math.cos(math.radians(lat0))
clat, clon = (min(lats) + max(lats)) / 2, (min(lons) + max(lons)) / 2
half = 0.55 * max(max(lats) - min(lats), (max(lons) - min(lons)) * k) + 0.1
la0, la1, lo0, lo1 = clat - half, clat + half, clon - half / k, clon + half / k
t_end = max(p[0] for k in order for p in tracks[k])
z_max = max(p[3] for k in order for p in tracks[k])

fig, (ax, az) = plt.subplots(1, 2, figsize=(12.8, 7.2), dpi=100, gridspec_kw=dict(width_ratios=[1.15, 1]))
ax.set_xlim(lo0, lo1)
ax.set_ylim(la0, la1)
ax.set_aspect(1 / math.cos(math.radians(lat0)))
ax.set_xlabel("longitude, deg E")
ax.set_ylabel("latitude, deg N")
ax.grid(alpha=0.3)
for name, la, lo in TOWNS:
    if la0 < la < la1 and lo0 < lo < lo1:
        ax.plot(lo, la, "s", color="#444", ms=4)
        ax.annotate(name, (lo, la), xytext=(4, 4), textcoords="offset points", fontsize=8, color="#444")
ax.plot(launch["lon"], launch["lat"], "k^", ms=9, label="launch")
az.set_xlim(0, t_end / 60)
az.set_ylim(0, z_max * 1.05 / 1000)
az.set_xlabel("time after launch, min")
az.set_ylabel("altitude, km")
az.grid(alpha=0.3)

lines, dots, zlines, zdots = {}, {}, {}, {}
for k in order:
    st = STYLE[k]
    label = cmp["tracks"][k]["label"]
    lines[k], = ax.plot([], [], color=st["color"], ls=st["ls"], lw=st["lw"], label=label)
    dots[k], = ax.plot([], [], "o", color=st["color"], ms=7)
    zlines[k], = az.plot([], [], color=st["color"], ls=st["ls"], lw=st["lw"])
    zdots[k], = az.plot([], [], "o", color=st["color"], ms=6)
ax.legend(loc="lower left", fontsize=7.5, framealpha=0.9)
title = fig.suptitle("")
w = cmp["tracks"]["wright_gfs"]
sub = (f"GFS {cmp['gfs_dataset']} via Tawhiri ({cmp['endpoint']}); launch {launch['datetime']}; "
       f"Wright lands {w['landing_km_from_launch']:.0f} km at {w['landing_bearing_deg']:.0f} deg, "
       f"Tawhiri's model on the same winds {cmp['landing_separation_km']['wright_gfs_vs_tawhiri_port']:.1f} km away")
fig.text(0.5, 0.01, sub, ha="center", fontsize=8.5, color="#333")


def upto(k, t):
    pts = [p for p in tracks[k] if p[0] <= t]
    return pts or tracks[k][:1]


N = 300
times = [t_end * i / (N - 1) for i in range(N)]


def frame(i):
    t = times[i]
    lead = None
    for k in order:
        pts = upto(k, t)
        lines[k].set_data([p[2] for p in pts], [p[1] for p in pts])
        dots[k].set_data([pts[-1][2]], [pts[-1][1]])
        zlines[k].set_data([p[0] / 60 for p in pts], [p[3] / 1000 for p in pts])
        zdots[k].set_data([pts[-1][0] / 60], [pts[-1][3] / 1000])
        if k == "wright_gfs":
            lead = pts[-1]
    phase = "landed" if t >= tracks["wright_gfs"][-1][0] else lead[4]
    title.set_text(f"Balloon flight on real winds: t = {t / 60:5.1f} min, Wright model {lead[3] / 1000:5.1f} km ({phase})")
    return []


anim = FuncAnimation(fig, frame, frames=N, blit=False)
anim.save(f"{out}/flight.mp4", writer=FFMpegWriter(fps=30, bitrate=2400, codec="libx264", extra_args=["-pix_fmt", "yuv420p"]))
frame(N - 1)
fig.savefig(f"{out}/flight.png")
print(f"wrote {out}/flight.mp4 and flight.png")
