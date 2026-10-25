//! Routing congestion, KiCadRoutingTools v0.23.7 `py_placer/placement/
//! simple.py` (`_net_demand`, `_capacity`, `_over2`; weight 5 in
//! `place_simple.py`): a 0.5 mm grid; each routed net's spanning-tree
//! length spread evenly over its pins' box grown by half a track pitch
//! (0.33 mm); each cell's capacity is its free area on every copper layer
//! (pad copper grown by half the clearance taken out; drilled pads on both
//! layers) divided by the pitch; the cost is the squared overflow summed
//! over cells. Kept incrementally: a move takes its parts' pads and its
//! nets' demand out and puts them back at the new poses.

use crate::geom::R;

pub const CELL_MM: f64 = 0.5;
pub const TRACK_PITCH_MM: f64 = 0.33;

#[derive(Debug, Clone)]
pub struct Grid {
    pub x0: f64,
    pub y0: f64,
    pub nx: usize,
    pub ny: usize,
    dem: Vec<f64>,
    cap: Vec<f64>,
    layers: f64,
}

impl Grid {
    pub fn new(area: R, layers: u8) -> Grid {
        let nx = ((area.w() / CELL_MM).ceil() as usize).max(1);
        let ny = ((area.h() / CELL_MM).ceil() as usize).max(1);
        let layers = f64::from(layers.max(1));
        Grid { x0: area.x0, y0: area.y0, nx, ny, dem: vec![0.0; nx * ny], cap: vec![layers * CELL_MM * CELL_MM; nx * ny], layers }
    }
    pub fn reset(&mut self) {
        self.dem.iter_mut().for_each(|d| *d = 0.0);
        let full = self.layers * CELL_MM * CELL_MM;
        self.cap.iter_mut().for_each(|c| *c = full);
    }
    fn over2(&self, i: usize) -> f64 {
        let o = self.dem[i] - self.cap[i].max(0.0) / TRACK_PITCH_MM;
        if o > 0.0 {
            o * o
        } else {
            0.0
        }
    }
    /// Add `per_area` x overlap area of `r` to every cell's demand (`demand`)
    /// or take it from the capacity; returns the change in the cost.
    pub fn add(&mut self, r: &R, per_area: f64, demand: bool) -> f64 {
        if per_area == 0.0 {
            return 0.0;
        }
        let cx0 = (((r.x0 - self.x0) / CELL_MM).floor().max(0.0)) as usize;
        let cy0 = (((r.y0 - self.y0) / CELL_MM).floor().max(0.0)) as usize;
        let cx1 = ((((r.x1 - self.x0) / CELL_MM).ceil()).max(0.0) as usize).min(self.nx);
        let cy1 = ((((r.y1 - self.y0) / CELL_MM).ceil()).max(0.0) as usize).min(self.ny);
        let mut d = 0.0;
        for cy in cy0..cy1 {
            let (y0, y1) = (self.y0 + cy as f64 * CELL_MM, self.y0 + (cy + 1) as f64 * CELL_MM);
            let oy = r.y1.min(y1) - r.y0.max(y0);
            if oy <= 0.0 {
                continue;
            }
            for cx in cx0..cx1 {
                let (x0, x1) = (self.x0 + cx as f64 * CELL_MM, self.x0 + (cx + 1) as f64 * CELL_MM);
                let ox = r.x1.min(x1) - r.x0.max(x0);
                if ox <= 0.0 {
                    continue;
                }
                let i = cy * self.nx + cx;
                let before = self.over2(i);
                if demand {
                    self.dem[i] += per_area * ox * oy;
                } else {
                    self.cap[i] -= per_area * ox * oy;
                }
                d += self.over2(i) - before;
            }
        }
        d
    }
    pub fn total(&self) -> f64 {
        (0..self.dem.len()).map(|i| self.over2(i)).sum()
    }
}
