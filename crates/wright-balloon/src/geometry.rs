//! The payload enclosure as one geometric model (wright-geom): insulation
//! panels (the lid is the top panel), a printed PETG rack of shelves and
//! corner posts, every board and battery as a solid at its packed place,
//! and ports for the camera and the antenna leads. The physics reads its
//! sizes (foam mass, wall area, antenna separations) and the checks run
//! all-pairs interference over it; `build()` writes and slices the same
//! meshes.

use crate::catalog::{Catalog, LinkKind, Role};
use crate::model::Design;
use anyhow::Result;
use serde::Serialize;
use wright_geom::{shapes, v3, Mesh, Part, PartKind, V3};

/// Shelf plate thickness, mm, and the margin kept clear for corner posts.
pub const SHELF_MM: f64 = 3.0;
pub const POST_R_MM: f64 = 5.0;
pub const POST_MARGIN_MM: f64 = 12.0;
/// Printer the rack is sliced for (wright-geom `slice::PRINTERS`).
pub const PRINTER: &str = "bambu_p1s";
pub const PRINT_MATERIAL: &str = "petg";

#[derive(Debug, Clone, Serialize)]
pub struct Placed {
    /// Unique instance name ("esp32-devkit#2").
    pub name: String,
    pub part: String,
    pub min: [f64; 3],
    pub size: [f64; 3],
}

#[derive(Debug, Clone, Serialize)]
pub struct Layout {
    pub inner_mm: [f64; 3],
    pub placed: Vec<Placed>,
    /// Bottom z of each shelf (printed rack only).
    pub shelves: Vec<f64>,
}

/// Every physical instance that goes in the box: items, batteries, regulators.
pub fn instances(d: &Design, cat: &Catalog) -> Result<Vec<(String, String, [f64; 3])>> {
    let mut out = Vec::new();
    let mut push = |id: &str, n: u32| -> Result<()> {
        let p = cat.part(id)?;
        for k in 0..n {
            out.push((format!("{id}#{}", k + 1), id.to_string(), p.dims_mm));
        }
        Ok(())
    };
    for it in &d.items {
        push(&it.part, it.qty)?;
    }
    for (bi, b) in d.buses.iter().enumerate() {
        let _ = bi;
        push(&b.battery, b.series * b.parallel)?;
        for r in &b.rails {
            push(r, 1)?;
        }
    }
    // Same part on two buses: keep names unique.
    let mut seen = std::collections::BTreeMap::<String, u32>::new();
    for (name, _, _) in out.iter_mut() {
        let c = seen.entry(name.clone()).or_insert(0);
        *c += 1;
        if *c > 1 {
            name.push_str(&format!("-{c}"));
        }
    }
    Ok(out)
}

/// Shelf-pack instances (lying flat: largest dims horizontal) into a
/// footprint `w` x `dd`, layers bottom-up, tallest first. Deterministic.
/// None if some part is wider than the usable footprint. Parts in `front`
/// (cameras) are packed first and end flush against the +x wall, which
/// carries their view ports (lens on the short face, looking +x).
pub fn pack(inst: &[(String, String, [f64; 3])], w: f64, dd: f64, gap: f64, rack: bool, front: &[String]) -> Option<Layout> {
    let m = if rack { POST_MARGIN_MM } else { gap };
    let (x0, x1, y0, y1) = (m, w - m, m, dd - m);
    let mut items: Vec<(String, String, [f64; 3])> = inst
        .iter()
        .map(|(n, p, d)| {
            let mut s = *d;
            s.sort_by(|a, b| b.partial_cmp(a).unwrap());
            (n.clone(), p.clone(), s)
        })
        .collect();
    let is_front = |part: &String| front.contains(part);
    items.sort_by(|a, b| is_front(&b.1).cmp(&is_front(&a.1)).then(b.2[2].partial_cmp(&a.2[2]).unwrap()).then(b.2[0].partial_cmp(&a.2[0]).unwrap()).then(a.0.cmp(&b.0)));
    let mut placed = Vec::new();
    let mut shelves = Vec::new();
    let mut layer_z = 0.0;
    let mut rest = items;
    while !rest.is_empty() {
        let floor = layer_z + if rack { SHELF_MM } else { 0.0 };
        if rack {
            shelves.push(layer_z);
        }
        let (mut cx, mut cy, mut row_d, mut layer_h) = (x0, y0, 0.0_f64, 0.0_f64);
        let mut left = Vec::new();
        for (name, part, s) in rest {
            // try as-is, then rotated 90 degrees in plan
            let mut fit = None;
            for (sx, sy) in [(s[0], s[1]), (s[1], s[0])] {
                if sx > x1 - x0 || sy > y1 - y0 {
                    continue;
                }
                if cx + sx <= x1 && cy + sy <= y1 {
                    fit = Some((sx, sy, cx, cy));
                    break;
                }
                if cy + row_d + gap + sy <= y1 && x0 + sx <= x1 {
                    fit = Some((sx, sy, x0, cy + row_d + gap));
                    break;
                }
            }
            match fit {
                Some((sx, sy, px, py)) => {
                    if py != cy {
                        cy = py;
                        row_d = 0.0;
                    }
                    placed.push(Placed { name, part, min: [px, py, floor], size: [sx, sy, s[2]] });
                    cx = px + sx + gap;
                    row_d = row_d.max(sy);
                    layer_h = layer_h.max(s[2]);
                }
                None => {
                    if (s[0] > x1 - x0 || s[1] > y1 - y0) && (s[1] > x1 - x0 || s[0] > y1 - y0) {
                        return None;
                    }
                    left.push((name, part, s));
                }
            }
        }
        layer_z = floor + layer_h + gap;
        rest = left;
    }
    // mirror in x so the first part of each row is at the +x wall, then
    // bring the front parts flush with it (their y keeps them clear of the
    // corner posts)
    for pl in placed.iter_mut() {
        pl.min[0] = w - pl.min[0] - pl.size[0];
        if is_front(&pl.part) && pl.min[0] + pl.size[0] >= w - m - 1e-9 {
            pl.min[0] = w - pl.size[0];
        }
    }
    Some(Layout { inner_mm: [w, dd, layer_z], placed, shelves })
}

/// The footprint that packs the parts into the least outer surface (heat
/// loss and foam mass both scale with it), within the printer bed.
pub fn best_layout(inst: &[(String, String, [f64; 3])], wall: f64, gap: f64, rack: bool, front: &[String]) -> Option<Layout> {
    let bed = 250.0;
    let mut best: Option<(f64, Layout)> = None;
    let mut w = 80.0;
    while w <= bed {
        let mut dd = 80.0;
        while dd <= w {
            if let Some(l) = pack(inst, w, dd, gap, rack, front) {
                let o = [l.inner_mm[0] + 2.0 * wall, l.inner_mm[1] + 2.0 * wall, l.inner_mm[2] + 2.0 * wall];
                let area = 2.0 * (o[0] * o[1] + o[1] * o[2] + o[0] * o[2]);
                if best.as_ref().is_none_or(|(a, _)| area < *a - 1e-9) {
                    best = Some((area, l));
                }
            }
            dd += 10.0;
        }
        w += 10.0;
    }
    best.map(|(_, l)| l)
}

/// The layout of a resolved design: its stated inner box, packed again.
/// Parts that do not fit stay outside the box, where the interference
/// check finds them in the walls.
pub fn layout(d: &Design, cat: &Catalog) -> Result<Layout> {
    let inst = instances(d, cat)?;
    let [w, dd, h] = d.enclosure.inner_mm;
    match pack(&inst, w, dd, crate::rules::PACK_GAP_MM, d.enclosure.printed_tray, &camera_ids(d, cat)) {
        Some(mut l) => {
            l.inner_mm = [w, dd, h];
            Ok(l)
        }
        None => {
            // Put everything at the origin: it will collide, and the check says so.
            let placed = inst.into_iter().map(|(name, part, s)| Placed { name, part, min: [0.0; 3], size: s }).collect();
            Ok(Layout { inner_mm: [w, dd, h], placed, shelves: vec![] })
        }
    }
}

/// Camera part ids in the design (packed against the port wall).
pub fn camera_ids(d: &Design, cat: &Catalog) -> Vec<String> {
    d.items.iter().filter(|it| cat.part(&it.part).is_ok_and(|p| p.role == Role::Camera)).map(|it| it.part.clone()).collect()
}

/// What a camera sees through its port in the +x wall.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    /// Lens (camera +x face) to the inner face of the wall, mm.
    pub gap_mm: f64,
    /// Half-angle of the clear cone from the lens through the port, deg.
    pub clear_half_deg: f64,
    /// Placed solids inside the port's line of sight.
    pub blocked_by: Vec<String>,
}

/// The cone a camera's port is cut to, from `rules::PORT_R_MM` at the
/// inner face flaring at `rules::VIEW_HALF_DEG` through the wall.
pub fn port_outer_r(wall_mm: f64) -> f64 {
    crate::rules::PORT_R_MM + wall_mm * crate::rules::VIEW_HALF_DEG.to_radians().tan()
}

/// The view of the camera instance `name` through its port.
pub fn camera_view(d: &Design, l: &Layout, name: &str) -> Option<View> {
    let pl = l.placed.iter().find(|p| p.name == name)?;
    let w = l.inner_mm[0];
    let t = d.enclosure.wall_mm;
    let (yc, zc) = (pl.min[1] + pl.size[1] / 2.0, pl.min[2] + pl.size[2] / 2.0);
    let face = pl.min[0] + pl.size[0];
    let gap = (w - face).max(0.0);
    let r0 = crate::rules::PORT_R_MM;
    let inner = if gap > 1e-9 { (r0 / gap).atan() } else { std::f64::consts::FRAC_PI_2 };
    let outer = (port_outer_r(t) / (gap + t)).atan();
    let blocked_by = l
        .placed
        .iter()
        .filter(|q| q.name != pl.name)
        .filter(|q| q.min[0] + q.size[0] > face + 1e-9 && q.min[0] < w - 1e-9)
        .filter(|q| q.min[1] < yc + r0 && q.min[1] + q.size[1] > yc - r0 && q.min[2] < zc + r0 && q.min[2] + q.size[2] > zc - r0)
        .map(|q| q.name.clone())
        .collect();
    Some(View { gap_mm: gap, clear_half_deg: inner.min(outer).to_degrees(), blocked_by })
}

/// A cone frustum along x from (x0, r0) to (x1, r1) about (y, z).
fn frustum_x(x0: f64, x1: f64, y: f64, z: f64, r0: f64, r1: f64, n: usize) -> Mesh {
    let ring = |x: f64, r: f64| -> Vec<V3> {
        (0..n)
            .map(|i| {
                let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                v3(x, y + r * a.cos(), z + r * a.sin())
            })
            .collect()
    };
    shapes::loft(&[ring(x0, r0), ring(x1, r1)])
}

fn bx(min: [f64; 3], size: [f64; 3]) -> Mesh {
    shapes::cuboid(v3(min[0], min[1], min[2]), v3(min[0] + size[0], min[1] + size[1], min[2] + size[2]))
}

/// Printed rack: shelves and corner posts (z from 0 to the top shelf).
pub fn rack_meshes(l: &Layout) -> Vec<(String, Mesh)> {
    let [w, dd, _] = l.inner_mm;
    let mut out = Vec::new();
    for (k, z) in l.shelves.iter().enumerate() {
        let mut plate = bx([0.0, 0.0, *z], [w, dd, SHELF_MM]);
        // a wiring pass-through in each shelf
        plate = plate.subtract(&shapes::cylinder(v3(w / 2.0, POST_MARGIN_MM / 2.0, z - 1.0), v3(w / 2.0, POST_MARGIN_MM / 2.0, z + SHELF_MM + 1.0), 4.0, 12));
        out.push((format!("shelf{}", k + 1), plate));
    }
    if l.shelves.len() > 1 {
        let top = *l.shelves.last().unwrap();
        for (i, (x, y)) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)].into_iter().enumerate() {
            let px = if x > 0.0 { POST_MARGIN_MM / 2.0 } else { w - POST_MARGIN_MM / 2.0 };
            let py = if y > 0.0 { POST_MARGIN_MM / 2.0 } else { dd - POST_MARGIN_MM / 2.0 };
            // posts run between shelves, so they never overlap a plate
            for (k, z) in l.shelves.iter().enumerate().skip(1) {
                let z0 = l.shelves[k - 1] + SHELF_MM;
                let _ = top;
                out.push((format!("post{}-{}", i + 1, k), shapes::cylinder(v3(px, py, z0), v3(px, py, *z), POST_R_MM, 12)));
            }
        }
    }
    out
}

/// Where each antenna is, mm in box coordinates: the GPS patch under the
/// lid above its board; radios with leads hang 50 mm below the box; a
/// phone's antenna is the phone.
pub fn antenna_point(d: &Design, cat: &Catalog, l: &Layout, part: &str, as_gps: bool) -> Option<V3> {
    let p = cat.part(part).ok()?;
    let pl = l.placed.iter().find(|q| q.part == part)?;
    let c = v3(pl.min[0] + pl.size[0] / 2.0, pl.min[1] + pl.size[1] / 2.0, pl.min[2] + pl.size[2] / 2.0);
    let t = d.enclosure.wall_mm;
    if as_gps {
        return Some(v3(c.x, c.y, l.inner_mm[2] + t));
    }
    match (p.role, p.radio.as_ref().map(|r| r.link)) {
        (_, Some(LinkKind::Cellular)) => Some(c),
        (_, Some(_)) => Some(v3(l.inner_mm[0] / 2.0, l.inner_mm[1] / 2.0, -t - 50.0)),
        _ => Some(c),
    }
}

/// Every solid of the payload, for all-pairs interference.
pub fn solids(d: &Design, cat: &Catalog, l: &Layout) -> Vec<Part> {
    let [w, dd, h] = l.inner_mm;
    let t = d.enclosure.wall_mm;
    let mut parts = Vec::new();
    let rack = rack_meshes(l);
    let rack_names: Vec<String> = rack.iter().map(|(n, _)| n.clone()).collect();
    // insulation panels; bottom and top span the outer footprint
    let mut camera_ports = Vec::new();
    for pl in &l.placed {
        if cat.part(&pl.part).map(|p| p.role == Role::Camera).unwrap_or(false) {
            camera_ports.push((pl.min[1] + pl.size[1] / 2.0, pl.min[2] + pl.size[2] / 2.0));
        }
    }
    let mut px = bx([w, 0.0, 0.0], [t, dd, h]);
    let slope = crate::rules::VIEW_HALF_DEG.to_radians().tan();
    for (y, z) in &camera_ports {
        let r0 = crate::rules::PORT_R_MM;
        px = px.subtract(&frustum_x(w - 1.0, w + t + 1.0, *y, *z, r0 - slope, r0 + (t + 1.0) * slope, 16));
    }
    let mut bottom = bx([-t, -t, -t], [w + 2.0 * t, dd + 2.0 * t, t]);
    let n_leads = d.trackers.len().max(1);
    for k in 0..n_leads {
        let x = w * (k as f64 + 1.0) / (n_leads as f64 + 1.0);
        bottom = bottom.subtract(&shapes::cylinder(v3(x, dd / 2.0, -t - 1.0), v3(x, dd / 2.0, 1.0), 4.0, 12));
    }
    let panels = [
        ("wall_bottom", bottom),
        ("wall_lid", bx([-t, -t, h], [w + 2.0 * t, dd + 2.0 * t, t])),
        ("wall_-x", bx([-t, -t, 0.0], [t, dd + 2.0 * t, h])),
        ("wall_+x", px.union(&bx([w, -t, 0.0], [t, t, h])).union(&bx([w, dd, 0.0], [t, t, h]))),
        ("wall_-y", bx([0.0, -t, 0.0], [w, t, h])),
        ("wall_+y", bx([0.0, dd, 0.0], [w, t, h])),
    ];
    let panel_names: Vec<String> = panels.iter().map(|(n, _)| n.to_string()).collect();
    for (n, m) in panels {
        parts.push(Part { name: n.into(), kind: PartKind::Bought, material: d.enclosure.insulation.clone(), mesh: m, touches: panel_names.clone() });
    }
    for (n, m) in rack {
        let mut touches = rack_names.clone();
        touches.extend(panel_names.iter().cloned());
        parts.push(Part { name: n, kind: PartKind::Printed, material: PRINT_MATERIAL.into(), mesh: m, touches });
    }
    for pl in &l.placed {
        parts.push(Part { name: pl.name.clone(), kind: PartKind::Bought, material: pl.part.clone(), mesh: bx(pl.min, pl.size), touches: rack_names.clone() });
    }
    parts
}

/// The whole flight train at true scale, mm, z up, payload box at the
/// origin: the balloon at its launch diameter (from the design's neck lift
/// and the balloon's mass, sea-level air), the line, the parachute canopy
/// hung a quarter of the way down the line, and the payload enclosure. It
/// is a picture of the vehicle, not a model anything is sized from.
pub fn flight_train(d: &Design, cat: &Catalog) -> Result<Vec<Part>> {
    let balloon = cat.balloon(&d.balloon)?;
    let gas = cat.gas(&d.gas)?;
    let chute = cat.chute(&d.parachute)?;
    // a design whose fill was never sized (the reference before the eval
    // fills it) is drawn at the fill its own target ascent rate would give
    let neck_lift_g = if d.neck_lift_g > 0.0 {
        d.neck_lift_g
    } else {
        let m = crate::mass::masses(d, cat)?;
        crate::flight::size_fill(d, cat, &m, wright_core::units::Speed::mps(d.fill_target_mps))?
    };
    let gross = wright_core::units::Mass::g(neck_lift_g + balloon.mass_g);
    let per_mol = crate::rules::M_AIR - crate::physics::gas_molar_mass(gas.gas, gas.purity);
    let air0 = crate::atmosphere::at(wright_core::units::Length::m(0.0));
    let launch_d_mm = crate::physics::sphere_diameter(crate::physics::volume_at(gross.as_kg() / per_mol, &air0)).as_m() * 1000.0;
    let [w, dd, h] = crate::mass::outer_mm(d);
    let t = d.enclosure.wall_mm;
    let line_mm = d.line_m * 1000.0;
    let line_r = 1.5;
    let box_top = h - t;
    let neck_mm = 0.1 * launch_d_mm; // latex neck below the bulb
    let bulb_c = box_top + line_mm + neck_mm + launch_d_mm / 2.0;
    let r = launch_d_mm / 2.0;
    // teardrop: neck tube, then a sphere traced from the bottom up
    let mut profile = vec![(0.0, box_top + line_mm), (0.03 * launch_d_mm, box_top + line_mm), (0.03 * launch_d_mm, bulb_c - r + 0.02 * r)];
    for k in 1..24 {
        let a = std::f64::consts::PI * k as f64 / 24.0;
        profile.push((r * a.sin(), bulb_c - r * a.cos()));
    }
    profile.push((0.0, bulb_c + r));
    let balloon_mesh = shapes::revolve(&profile, 32).translate(v3(w / 2.0, dd / 2.0, 0.0));
    // inflated canopy: a hemisphere of ASSUMPTION 0.7 x the maker's nominal
    // diameter (flat-to-inflated for round chutes), open end down
    let cr = 0.7 * chute.diameter_m * 1000.0 / 2.0;
    let chute_c = box_top + 0.75 * line_mm;
    let mut cp = vec![(cr, chute_c), (cr, chute_c + 0.03 * cr)];
    for k in 1..12 {
        let a = std::f64::consts::FRAC_PI_2 * k as f64 / 12.0;
        cp.push((cr * a.cos(), chute_c + cr * a.sin()));
    }
    cp.push((0.0, chute_c + cr));
    let chute_mesh = shapes::revolve(&cp, 32).translate(v3(w / 2.0, dd / 2.0, 0.0));
    let line_mesh = shapes::cylinder(v3(w / 2.0, dd / 2.0, box_top), v3(w / 2.0, dd / 2.0, box_top + line_mm), line_r, 8);
    let enclosure = bx([-t, -t, -t], [w, dd, h]);
    let names: Vec<String> = ["balloon", "line", "parachute", "enclosure"].iter().map(|s| s.to_string()).collect();
    Ok(vec![
        Part { name: "balloon".into(), kind: PartKind::Bought, material: d.balloon.clone(), mesh: balloon_mesh, touches: names.clone() },
        Part { name: "line".into(), kind: PartKind::Bought, material: d.line.clone(), mesh: line_mesh, touches: names.clone() },
        Part { name: "parachute".into(), kind: PartKind::Bought, material: d.parachute.clone(), mesh: chute_mesh, touches: names.clone() },
        Part { name: "enclosure".into(), kind: PartKind::Bought, material: d.enclosure.insulation.clone(), mesh: enclosure, touches: names },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flight_train_is_a_true_scale_train() {
        let d = crate::reference::pat_design();
        let cat = crate::catalog::catalog();
        let parts = flight_train(&d, cat).unwrap();
        assert_eq!(parts.len(), 4);
        for p in &parts {
            assert!(!p.mesh.is_empty() && p.mesh.is_finite(), "{} is empty or not finite", p.name);
        }
        let b = parts.iter().find(|p| p.name == "balloon").unwrap().mesh.bounds();
        let dia = b.size().x;
        // a 2000 g latex balloon lifting a 1.5 kg train inflates to 2-3 m
        assert!(dia > 1800.0 && dia < 3500.0, "launch diameter {dia} mm");
        let (lo, hi) = parts.iter().map(|p| p.mesh.bounds()).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), b| (lo.min(b.min.z), hi.max(b.max.z)));
        let height = hi - lo;
        let expect = crate::mass::outer_mm(&d)[2] + d.line_m * 1000.0 + 1.1 * dia;
        assert!((height - expect).abs() < 0.05 * expect, "train height {height} vs {expect}");
    }

    #[test]
    fn packs_without_overlap() {
        let inst: Vec<_> = (0..6).map(|i| (format!("b{i}"), "b".to_string(), [60.0, 40.0, 15.0])).collect();
        let l = pack(&inst, 150.0, 120.0, 8.0, true, &[]).unwrap();
        for (i, a) in l.placed.iter().enumerate() {
            for b in &l.placed[i + 1..] {
                let sep = (0..3).any(|k| a.min[k] + a.size[k] <= b.min[k] + 1e-9 || b.min[k] + b.size[k] <= a.min[k] + 1e-9);
                assert!(sep, "{} overlaps {}", a.name, b.name);
            }
            assert!(a.min[0] + a.size[0] <= 150.0 && a.min[1] + a.size[1] <= 120.0);
        }
    }
}
