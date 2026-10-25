"""Contour pictures and mode-shape frames for wright-fea.

    python render.py scene.json

scene.json (written by crates/wright-fea/src/render.rs):
    nodes  [[x, y, z], ...] (m)        faces [[i, j, k(, l)], ...] (0-based)
    value  [per node] (contour, already in the unit of `label`)
    disp   [[ux, uy, uz], ...] or null  scale  display magnification
    title, label, view [elev, azim], out (png path)
    frames  0 for one picture; n > 0 writes out % k for k in 0..n with the
            displacement scaled by sin(2 pi k / n) (a mode shape cycling)

Painter's-algorithm 3D polygons are matplotlib's own (mpl_toolkits.mplot3d
art3d.Poly3DCollection); the colour map is viridis-like "turbo" as in most FE
post-processors (ParaView / GraphiX default rainbow), one colour per face from
its nodes' mean.
"""
import json
import math
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from mpl_toolkits.mplot3d.art3d import Poly3DCollection  # noqa: E402


def draw(sc, amp, out):
    P = np.array(sc["nodes"], dtype=float)
    if sc.get("disp") is not None:
        P = P + amp * sc["scale"] * np.array(sc["disp"], dtype=float)
    V = np.array(sc["value"], dtype=float)
    faces = sc["faces"]
    vmin, vmax = sc.get("vmin", float(V.min())), sc.get("vmax", float(V.max()))
    if vmax <= vmin:
        vmax = vmin + 1e-12
    cmap = plt.get_cmap("turbo")
    polys = [P[f] for f in faces]
    cols = [cmap((float(np.mean(V[f])) - vmin) / (vmax - vmin)) for f in faces]
    fig = plt.figure(figsize=(10, 6), dpi=130)
    ax = fig.add_subplot(111, projection="3d")
    pc = Poly3DCollection(polys, facecolors=cols, edgecolors=(0, 0, 0, 0.08), linewidths=0.2)
    ax.add_collection3d(pc)
    B = np.array(sc["bounds"], dtype=float) if sc.get("bounds") else np.array([P.min(0), P.max(0)])
    c = B.mean(0)
    span = np.maximum(B[1] - B[0], 1e-9)
    ax.set_xlim(c[0] - span[0] / 2, c[0] + span[0] / 2)
    ax.set_ylim(c[1] - span[1] / 2, c[1] + span[1] / 2)
    ax.set_zlim(c[2] - span[2] / 2, c[2] + span[2] / 2)
    ax.set_proj_type("ortho")
    ax.set_box_aspect(tuple(np.maximum(span / span.max(), 0.04)), zoom=0.9)
    ax.view_init(*sc.get("view", [25, -60]))
    ax.set_axis_off()
    sm = plt.cm.ScalarMappable(cmap=cmap, norm=plt.Normalize(vmin, vmax))
    cb = fig.colorbar(sm, ax=ax, shrink=0.7, pad=0.02)
    cb.set_label(sc["label"])
    ax.set_title(sc["title"] + ("\n" + sc["note"] if sc.get("note") else ""), fontsize=10)
    fig.canvas.draw()
    img = np.asarray(fig.canvas.buffer_rgba())[..., :3].copy()
    plt.close(fig)
    return img


def ink_box(imgs, pad=12):
    """3D axes leave wide margins around a slender part: the box holding
    every non-white pixel of every image, even-sized for H.264."""
    m = np.zeros(imgs[0].shape[:2], dtype=bool)
    for img in imgs:
        m |= (img < 245).any(axis=2)
    ys, xs = np.where(m)
    if not len(ys):
        return 0, imgs[0].shape[0], 0, imgs[0].shape[1]
    y0, y1 = max(ys.min() - pad, 0), min(ys.max() + pad, m.shape[0])
    x0, x1 = max(xs.min() - pad, 0), min(xs.max() + pad, m.shape[1])
    return y0, y0 + (y1 - y0) // 2 * 2, x0, x0 + (x1 - x0) // 2 * 2


def main():
    sc = json.load(open(sys.argv[1]))
    n = int(sc.get("frames", 0))
    if n <= 0:
        img = draw(sc, 1.0, sc["out"])
        y0, y1, x0, x1 = ink_box([img])
        plt.imsave(sc["out"], img[y0:y1, x0:x1])
        return
    # fixed axes for every frame: the bounds of both extremes
    P = np.array(sc["nodes"], dtype=float)
    D = np.array(sc["disp"], dtype=float) * sc["scale"]
    lo = np.minimum((P + D).min(0), (P - D).min(0))
    hi = np.maximum((P + D).max(0), (P - D).max(0))
    sc["bounds"] = [lo.tolist(), hi.tolist()]
    imgs = [draw(sc, math.sin(2 * math.pi * k / n), None) for k in range(n)]
    y0, y1, x0, x1 = ink_box(imgs)
    for k, img in enumerate(imgs):
        plt.imsave(sc["out"] % k, img[y0:y1, x0:x1])


if __name__ == "__main__":
    main()
