//! The exact legality pass, integer nanometres: every courtyard at least
//! `spacing` from every other (KiCad's courtyard rule, `DRC_TEST_PROVIDER_
//! COURTYARD_CLEARANCE`, with the spacing on top) and inside the outline.
//! The Python baseline's `simple/anneal.py` `legal` / `_nearest_legal` (from upstream
//! KiCadRoutingTools `py_placer/placement/legality.py`): the anneal's float
//! state is only a proposal; this decides.

use wright_eda::{Nm, Placed, Pt, Rect};

/// Two courtyards closer than `spacing` (Chebyshev gap per axis; touching
/// at exactly `spacing` is legal).
pub fn too_close(a: &Rect, b: &Rect, spacing: i64) -> bool {
    a.inflate(Nm(spacing)).overlaps(b)
}

/// Pairs (i < j) of parts closer than `spacing`.
pub fn conflicts(parts: &[Placed], spacing: i64) -> Vec<(usize, usize)> {
    let r: Vec<Rect> = parts.iter().map(Placed::courtyard_abs).collect();
    let mut out = Vec::new();
    for i in 0..r.len() {
        for j in i + 1..r.len() {
            if parts[i].side == parts[j].side && too_close(&r[i], &r[j], spacing) {
                out.push((i, j));
            }
        }
    }
    out
}

/// Courtyard pairs whose interiors intersect (spacing 0): the number the
/// `courtyard_clearance` check and the report count.
pub fn courtyard_overlaps(parts: &[Placed]) -> usize {
    conflicts(parts, 0).len()
}

/// How the exact pass may move a part: its allowed quarter turns (empty =
/// any) and, for a part seated on an edge, the one axis it slides along
/// (`Some(true)` = along x, `Some(false)` = along y).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Freedom {
    pub turns: Vec<i32>,
    pub slide: Option<bool>,
}

fn legal_one(parts: &[Placed], i: usize, spacing: i64, inside: Option<&Rect>, walls: &[Rect]) -> bool {
    let r = parts[i].courtyard_abs();
    if let Some(o) = inside {
        if r.min.x < o.min.x || r.min.y < o.min.y || r.max.x > o.max.x || r.max.y > o.max.y {
            return false;
        }
    }
    walls.iter().all(|w| !too_close(&r, w, spacing)) && parts.iter().enumerate().all(|(j, p)| j == i || p.side != parts[i].side || !too_close(&r, &p.courtyard_abs(), spacing))
}

/// Move part `i` to the nearest legal pose: rings of `step` out to `reach`,
/// nearest offset first, its own turn then the other allowed turns, along
/// its edge only when seated. Upstream's `nearest_legal` (the
/// baseline's `simple/anneal.py _nearest_legal`). On failure the part is left where it
/// was.
pub fn reseat(parts: &mut [Placed], i: usize, spacing: i64, inside: Option<&Rect>, walls: &[Rect], free: &Freedom, (step, reach): (i64, i64)) -> bool {
    let was = (parts[i].at, parts[i].rot);
    let mut turns = vec![was.1];
    let allowed: Vec<i32> = if free.turns.is_empty() { (0..4).collect() } else { free.turns.clone() };
    for k in 1..4 {
        let q = (was.1 + k) % 4;
        if allowed.contains(&q) {
            turns.push(q);
        }
    }
    let rings = reach / step;
    for ring in 0..=rings {
        let mut offs = Vec::new();
        for dx in -ring..=ring {
            for dy in -ring..=ring {
                let ok = match free.slide {
                    Some(true) => dy == 0,
                    Some(false) => dx == 0,
                    None => true,
                };
                if ok && dx.abs().max(dy.abs()) == ring {
                    offs.push((dx * dx + dy * dy, dx, dy));
                }
            }
        }
        offs.sort();
        for &(_, dx, dy) in &offs {
            for &q in &turns {
                parts[i].at = was.0 + Pt::new(dx * step, dy * step);
                parts[i].rot = q;
                if legal_one(parts, i, spacing, inside, walls) {
                    return true;
                }
            }
        }
    }
    parts[i].at = was.0;
    parts[i].rot = was.1;
    false
}

/// Every illegal part reseated, in part order (locked parts never move);
/// `false` when one could not be.
pub fn legalize(parts: &mut [Placed], spacing: i64, inside: Option<&Rect>, walls: &[Rect], free: &[Freedom], step: i64, reach: i64) -> bool {
    let mut ok = true;
    for i in 0..parts.len() {
        if parts[i].locked {
            continue;
        }
        if !legal_one(parts, i, spacing, inside, walls) && !reseat(parts, i, spacing, inside, walls, &free[i], (step, reach)) {
            ok = false;
        }
    }
    ok && (0..parts.len()).all(|i| legal_one(parts, i, spacing, inside, walls))
}
