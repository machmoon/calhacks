//! The fallback that always returns a legal placement: deterministic
//! next-fit-decreasing-height shelf packing, ported from the Python baseline's
//! `engine/silkscreen/packing.py:213 _shelf_pack` (parts sorted by height,
//! laid left to right, a new shelf when the next one would pass the target
//! width, which starts at sqrt of the total stepped area and never below
//! the widest part).
//!
//! Two changes from the baseline, both stated: each part that may turn is laid
//! landscape (wider than tall) first, which is what makes NFDH's shelves
//! tight; and a handful of target widths are tried and the smallest
//! bounding area kept, rather than one. The baseline's fallback did not rotate
//! (`packing.py` "Fallback does not rotate parts").
//!
//! Exact in integer nm: shelves are stepped by courtyard + spacing, so no
//! two courtyards come closer than `spacing`.

use wright_eda::{Footprint, Pt};

#[derive(Debug, Clone, PartialEq)]
pub struct Shelf {
    /// Origin and quarter turns per part, in input order.
    pub poses: Vec<(Pt, i32)>,
    /// Extent of the stepped boxes (courtyards + spacing), nm.
    pub w: i64,
    pub h: i64,
}

fn pack_at(fps: &[Footprint], turns: &[i32], spacing: i64, target_w: i64) -> Shelf {
    let rects: Vec<_> = fps.iter().zip(turns).map(|(f, &q)| f.courtyard.rot90(q)).collect();
    let mut order: Vec<usize> = (0..fps.len()).collect();
    // Height descending, then width descending, then input order: total.
    order.sort_by_key(|&i| (std::cmp::Reverse(rects[i].height().0), std::cmp::Reverse(rects[i].width().0), i));
    let mut poses = vec![(Pt::default(), 0); fps.len()];
    let (mut cx, mut sy, mut sh, mut used_w) = (0i64, 0i64, 0i64, 0i64);
    for i in order {
        let r = rects[i];
        let (sw, sth) = (r.width().0 + spacing, r.height().0 + spacing);
        if cx > 0 && cx + sw > target_w {
            sy += sh;
            cx = 0;
            sh = 0;
        }
        // Courtyard min corner at (cx, sy) + spacing/2.
        let corner = Pt::new(cx + spacing / 2, sy + spacing / 2);
        poses[i] = (corner - r.min, turns[i]);
        cx += sw;
        sh = sh.max(sth);
        used_w = used_w.max(cx);
    }
    Shelf { poses, w: used_w, h: sy + sh }
}

/// Shelf-pack `fps`. `can_turn[i]` allows a quarter turn to lay part `i`
/// landscape; `max_w` caps the shelf width (a fixed outline).
pub fn shelf_pack(fps: &[Footprint], can_turn: &[bool], spacing: i64, max_w: Option<i64>) -> Shelf {
    if fps.is_empty() {
        return Shelf { poses: Vec::new(), w: 0, h: 0 };
    }
    let turns: Vec<i32> = fps
        .iter()
        .zip(can_turn)
        .map(|(f, &t)| i32::from(t && f.courtyard.height() > f.courtyard.width()))
        .collect();
    let widest = fps.iter().zip(&turns).map(|(f, &q)| f.courtyard.rot90(q).width().0 + spacing).max().unwrap_or(0);
    let area: f64 = fps.iter().map(|f| (f.courtyard.width().0 + spacing) as f64 * (f.courtyard.height().0 + spacing) as f64).sum();
    let base = area.sqrt() as i64;
    let mut best: Option<Shelf> = None;
    // The baseline's sqrt(area) target and a spread around it; the smallest
    // bounding area wins, ties to the squarer board, then the earlier try.
    for k in [100, 85, 115, 70, 130, 150, 175, 200] {
        let mut t = (base * k / 100).max(widest);
        if let Some(m) = max_w {
            t = t.min(m).max(widest);
        }
        let s = pack_at(fps, &turns, spacing, t);
        let key = |s: &Shelf| (s.w as i128 * s.h as i128, (s.w - s.h).abs());
        if best.as_ref().is_none_or(|b| key(&s) < key(b)) {
            best = Some(s);
        }
    }
    best.unwrap_or_else(|| pack_at(fps, &turns, spacing, widest))
}
