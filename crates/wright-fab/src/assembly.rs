//! Assembly animation (parts fly in, in build order) and the turntable.
//!
//! Parts and positions come from the build's own STLs, in the vehicle
//! frame: one STL per part when the build writes them (rocket, balloon),
//! else the assembly STL split into its connected pieces (aircraft writes
//! only `cad/assembly.stl`; its pieces were separate meshes before the
//! build merged them, so connectivity recovers them, unnamed).
//! Order: `wright_guide::default_steps`, the guide's own assembly order
//! (structure, flying surfaces, motors, servos, electronics, recovery,
//! battery, props; printed before bought in a group), so the film and the
//! printed guide agree. Unnamed pieces go largest first.

use crate::render::{rgb, Camera, Renderer, Scene};
use crate::video::{Painter, Video, FPS};
use std::collections::HashMap;
use std::path::Path;
use wright_geom::{v3, Mesh, Part, PartKind, V3};

pub struct Loaded {
    pub parts: Vec<Part>,
    /// Steps: (title, part indices).
    pub steps: Vec<(String, Vec<usize>)>,
    pub named: bool,
    pub source: String,
}

fn stl_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("stl"))).collect();
    v.sort();
    v
}

fn is_machined(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    !l.contains("bay") && ["bulkhead", "engine", "liner", "injector", "case", "nozzle"].iter().any(|k| l.contains(k))
}

pub fn color_of(p: &Part) -> [f32; 4] {
    if is_machined(&p.name) {
        rgb([196, 202, 212])
    } else if p.kind == PartKind::Printed {
        rgb([120, 166, 214])
    } else {
        rgb([112, 118, 128])
    }
}

/// Split a mesh into connected pieces (shared vertices, 1 um grid).
pub fn components(m: &Mesh) -> Vec<Mesh> {
    let tris = m.triangles();
    let key = |p: V3| ((p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64, (p.z * 1000.0).round() as i64);
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut parent: Vec<usize> = Vec::new();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let mut tv = Vec::with_capacity(tris.len());
    for t in &tris {
        let mut vs = [0usize; 3];
        for (i, p) in t.iter().enumerate() {
            let n = ids.len();
            let id = *ids.entry(key(*p)).or_insert(n);
            if id == parent.len() {
                parent.push(id);
            }
            vs[i] = id;
        }
        for i in 1..3 {
            let (a, b) = (find(&mut parent, vs[0]), find(&mut parent, vs[i]));
            if a != b {
                parent[a] = b;
            }
        }
        tv.push(vs[0]);
    }
    let mut groups: HashMap<usize, Vec<[V3; 3]>> = HashMap::new();
    for (t, v) in tris.iter().zip(tv) {
        let r = find(&mut parent, v);
        groups.entry(r).or_default().push(*t);
    }
    let mut out: Vec<Mesh> = groups.into_values().map(Mesh::from_triangles).collect();
    out.sort_by(|a, b| {
        let va = a.bounds().size();
        let vb = b.bounds().size();
        (vb.x * vb.y * vb.z).total_cmp(&(va.x * va.y * va.z))
    });
    out
}

pub fn load(build_dir: &Path) -> anyhow::Result<Loaded> {
    let mut files = stl_files(build_dir);
    files.extend(stl_files(&build_dir.join("cad")));
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (asm, singles): (Vec<_>, Vec<_>) = files.into_iter().filter(|p| {
        let s = stem(p).to_ascii_lowercase();
        !(s.contains("jig") || s.contains("fixture"))
    }).partition(|p| stem(p).to_ascii_lowercase().contains("assembly"));
    let printed = |s: &str| build_dir.join("print").join(format!("{s}.gcode")).exists() || build_dir.join(format!("{s}.gcode")).exists();
    if !singles.is_empty() {
        let mut parts = Vec::new();
        for f in &singles {
            let s = stem(f);
            let mesh = Mesh::read_stl(f)?;
            if mesh.is_empty() {
                continue;
            }
            let name = s.replace('_', " ");
            let kind = if printed(&s) { PartKind::Printed } else { PartKind::Bought };
            parts.push(Part { name, kind, material: String::new(), mesh, touches: vec![] });
        }
        let steps = wright_guide::default_steps(&parts);
        let steps = steps.into_iter().map(|st| (st.title, st.parts.iter().filter_map(|n| parts.iter().position(|p| &p.name == n)).collect())).collect();
        return Ok(Loaded { parts, steps, named: true, source: format!("{} part STLs in the vehicle frame", singles.len()) });
    }
    let a = asm.first().ok_or_else(|| anyhow::anyhow!("no STL to assemble in {}", build_dir.display()))?;
    let mesh = Mesh::read_stl(a)?;
    let comps = components(&mesh);
    let parts: Vec<Part> = comps.into_iter().enumerate().map(|(i, m)| Part { name: format!("piece {}", i + 1), kind: PartKind::Bought, material: String::new(), mesh: m, touches: vec![] }).collect();
    // a few pieces per step, largest first
    let mut steps = Vec::new();
    let per = (parts.len() / 12).max(1);
    for (k, chunk) in (0..parts.len()).collect::<Vec<_>>().chunks(per).enumerate() {
        steps.push((format!("step {}", k + 1), chunk.to_vec()));
    }
    let source = format!("{} split into {} connected pieces", a.file_name().unwrap().to_string_lossy(), comps_len(&steps));
    Ok(Loaded { parts, steps, named: false, source })
}

fn comps_len(steps: &[(String, Vec<usize>)]) -> usize {
    steps.iter().map(|s| s.1.len()).sum()
}

/// Lay a tall, thin vehicle (a rocket) on its side for the camera.
pub fn orient(parts: &mut [Part]) -> bool {
    let b = wright_guide::render::bounds(parts);
    let Some(b) = b else { return false };
    let s = b.size();
    if s.z > 2.5 * s.x.max(s.y) {
        for p in parts.iter_mut() {
            p.mesh = p.mesh.map(|q| v3(q.z, q.y, -q.x));
        }
        return true;
    }
    false
}

fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn film(r: &Renderer, painter: &Painter, l: &Loaded, title: &str, mp4: &Path, png: &Path) -> anyhow::Result<usize> {
    let mut parts = l.parts.clone();
    orient(&mut parts);
    let b = wright_guide::render::bounds(&parts).ok_or_else(|| anyhow::anyhow!("no geometry"))?;
    let center = (b.min + b.max) * 0.5;
    let radius = (b.max - b.min).norm() * 0.5;
    let n_steps = l.steps.len().max(1);
    let per = (420 / n_steps).clamp(18, 45);
    let frames = per * n_steps + 20;
    let tris: Vec<Vec<[V3; 3]>> = parts.iter().map(|p| p.mesh.triangles()).collect();
    let centers: Vec<V3> = parts.iter().map(|p| {
        let bb = p.mesh.bounds();
        (bb.min + bb.max) * 0.5
    }).collect();
    let mut start = vec![usize::MAX; parts.len()];
    for (k, (_, idx)) in l.steps.iter().enumerate() {
        for (j, &i) in idx.iter().enumerate() {
            start[i] = k * per + j * (per / (idx.len() + 1)).min(6);
        }
    }
    let mut video = Video::create(mp4)?;
    let mut last = Vec::new();
    let fly = (per as f64 * 0.8).max(14.0);
    for f in 0..=frames {
        let mut sc = Scene::default();
        sc.cuboid(v3(b.min.x - 0.15 * radius, b.min.y - 0.15 * radius, b.min.z - 0.03 * radius), v3(b.max.x + 0.15 * radius, b.max.y + 0.15 * radius, b.min.z - 0.01 * radius), rgb([36, 38, 44]));
        for (i, p) in parts.iter().enumerate() {
            if start[i] == usize::MAX || f < start[i] {
                continue;
            }
            let t = (f - start[i]) as f64 / fly;
            let mut dir = centers[i] - center;
            dir.z = dir.z.max(0.0) + 0.6 * radius;
            let dir = if dir.norm() > 1e-9 { dir.unit() } else { v3(0.0, 0.0, 1.0) };
            let off = dir * ((1.0 - ease(t)) * 0.9 * radius);
            let c = if t < 1.0 { rgb([240, 128, 32]) } else { color_of(p) };
            for tr in &tris[i] {
                sc.tri(tr[0] + off, tr[1] + off, tr[2] + off, c);
            }
        }
        let prog = f as f64 / frames as f64;
        let cam = Camera::orbit(center, Camera::fit_distance(radius * 0.95, 30.0), 25.0 + 70.0 * prog, 28.0);
        let mut img = r.draw(&sc, &cam);
        let k = (f / per).min(n_steps - 1);
        let (st_title, idx) = &l.steps[k];
        let names: Vec<&str> = idx.iter().take(4).map(|&i| parts[i].name.as_str()).collect();
        let more = if idx.len() > 4 { format!(" +{}", idx.len() - 4) } else { String::new() };
        painter.caption(&mut img, &format!("{title}: assembly"), &format!("step {}/{}: {} -- {}{}", k + 1, n_steps, st_title, names.join(", "), more), &[format!("{} parts", parts.len()), l.source.chars().take(44).collect()], prog);
        video.push(&img)?;
        last = img;
    }
    for _ in 0..FPS {
        video.push(&last)?;
    }
    video.finish()?;
    std::fs::write(png, crate::render::png(&last))?;
    Ok(parts.len())
}

/// A turntable (or, for a vehicle laid on its side, a slow roll about its
/// long axis with the camera circling half way).
pub fn turntable(r: &Renderer, painter: &Painter, l: &Loaded, title: &str, mp4: &Path, png: &Path) -> anyhow::Result<()> {
    let mut parts = l.parts.clone();
    let long = orient(&mut parts);
    let b = wright_guide::render::bounds(&parts).ok_or_else(|| anyhow::anyhow!("no geometry"))?;
    let center = (b.min + b.max) * 0.5;
    let radius = (b.max - b.min).norm() * 0.5;
    let mut base = Scene::default();
    for p in &parts {
        base.mesh(&p.mesh, color_of(p));
    }
    let frames = 240;
    let mut video = Video::create(mp4)?;
    let mut hero = Vec::new();
    for f in 0..frames {
        let prog = f as f64 / frames as f64;
        let (sc, cam) = if long {
            let a = std::f64::consts::TAU * prog;
            let mut sc = Scene::default();
            for vtx in &base.verts {
                let mut v = *vtx;
                let (y, z) = (v.pos[1] as f64 - center.y, v.pos[2] as f64 - center.z);
                v.pos[1] = (center.y + y * a.cos() - z * a.sin()) as f32;
                v.pos[2] = (center.z + y * a.sin() + z * a.cos()) as f32;
                let (ny, nz) = (v.normal[1] as f64, v.normal[2] as f64);
                v.normal[1] = (ny * a.cos() - nz * a.sin()) as f32;
                v.normal[2] = (ny * a.sin() + nz * a.cos()) as f32;
                sc.verts.push(v);
            }
            (sc, Camera::orbit(center, Camera::fit_distance(radius * 0.62, 30.0), 75.0 + 30.0 * (std::f64::consts::TAU * prog).sin(), 14.0))
        } else {
            (base.clone(), Camera::orbit(center, Camera::fit_distance(radius * 0.9, 30.0), 360.0 * prog + 30.0, 24.0))
        };
        let mut img = r.draw(&sc, &cam);
        if f == frames / 8 {
            hero = img.clone();
        }
        painter.caption(&mut img, title, &format!("{} parts", parts.len()), &[], prog);
        video.push(&img)?;
    }
    video.finish()?;
    if long {
        // a long, thin vehicle: the hero is a 3/4 close-up of the aft end
        // (fins, nozzle), plus a wide shot framed on its length
        let len = b.max.x - b.min.x;
        let dia = (b.max.y - b.min.y).max(b.max.z - b.min.z);
        let aft = v3(b.min.x + 0.12 * len, center.y, center.z);
        let r_aft = (0.16 * len).max(1.2 * dia);
        hero = r.draw(&base, &Camera::orbit(aft, Camera::fit_distance(r_aft, 30.0), 215.0, 22.0));
        let hfov = 2.0 * ((15.0f64).to_radians().tan() * crate::render::W as f64 / crate::render::H as f64).atan();
        let d = 0.5 * len / (hfov / 2.0).tan() * 1.3;
        let wide = r.draw(&base, &Camera::orbit(center, d, 84.0, 10.0));
        if let Some(dir) = png.parent() {
            std::fs::write(dir.join("render_wide.png"), crate::render::png(&wide))?;
        }
    }
    std::fs::write(png, crate::render::png(&hero))?;
    Ok(())
}
