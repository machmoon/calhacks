//! Pictures of results: the exterior faces of the solved (expanded) mesh,
//! coloured by a nodal field, drawn by `py/render.py` (matplotlib); mode
//! shapes as frames joined by ffmpeg into an mp4.

use crate::results::{von_mises, Solved};
use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Exterior faces (corner node ids) of the frd's solid elements: a face
/// used by exactly one element is on the outside.
pub fn exterior_faces(s: &Solved) -> Vec<Vec<usize>> {
    let mut count: HashMap<Vec<usize>, (usize, Vec<usize>)> = HashMap::new();
    for (ty, ns) in &s.frd.elements {
        let table: &[&[usize]] = match ty {
            4 => &[&[0, 1, 2, 3], &[4, 7, 6, 5], &[0, 4, 5, 1], &[1, 5, 6, 2], &[2, 6, 7, 3], &[3, 7, 4, 0]],
            6 => &[&[0, 1, 2], &[0, 3, 1], &[1, 3, 2], &[2, 3, 0]],
            _ => continue,
        };
        for face in table {
            if face.iter().any(|&i| i >= ns.len()) {
                continue;
            }
            let f: Vec<usize> = face.iter().map(|&i| ns[i]).collect();
            let mut key = f.clone();
            key.sort_unstable();
            count.entry(key).or_insert((0, f)).0 += 1;
        }
    }
    let mut out: Vec<Vec<usize>> = count.into_values().filter(|(c, _)| *c == 1).map(|(_, f)| f).collect();
    out.sort();
    out
}

pub struct Picture<'a> {
    pub title: &'a str,
    pub note: &'a str,
    pub view: [f64; 2],
    /// Displacement magnification shown (1 = true scale); None: automatic
    /// (largest displacement drawn at 8 % of the part's size).
    pub scale: Option<f64>,
}

fn scene(s: &Solved, field: &str, mode: usize, pic: &Picture, out: &str, frames: usize) -> serde_json::Value {
    let faces = exterior_faces(s);
    let mut ids: Vec<usize> = faces.iter().flatten().copied().collect();
    ids.sort_unstable();
    ids.dedup();
    let index: HashMap<usize, usize> = ids.iter().enumerate().map(|(i, &n)| (n, i)).collect();
    let nodes: Vec<[f64; 3]> = ids.iter().map(|n| s.frd.nodes[n]).collect();
    let disp = s.nth("DISP", mode);
    let d: Vec<[f64; 3]> = ids.iter().map(|n| disp.and_then(|b| b.data.get(n)).map(|v| [v[0], v[1], v[2]]).unwrap_or([0.0; 3])).collect();
    let (value, label): (Vec<f64>, &str) = match field {
        "von_mises" => {
            let st = s.nth("STRESS", mode);
            (ids.iter().map(|n| st.and_then(|b| b.data.get(n)).map(|v| von_mises(v) / 1e6).unwrap_or(0.0)).collect(), "von Mises stress, MPa")
        }
        "disp_mm" => (d.iter().map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() * 1e3).collect(), "displacement, mm"),
        _ => (d.iter().map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()).collect(), "relative modal displacement"),
    };
    let size = {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &nodes {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max)
    };
    let dmax = d.iter().map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()).fold(0.0, f64::max);
    let scale = pic.scale.unwrap_or(if dmax > 0.0 { 0.08 * size / dmax } else { 1.0 });
    let faces: Vec<Vec<usize>> = faces.iter().map(|f| f.iter().map(|n| index[n]).collect()).collect();
    let note = if scale > 1.5 { format!("{}  (deformation x{:.0})", pic.note, scale) } else { pic.note.to_string() };
    json!({"nodes": nodes, "faces": faces, "value": value, "disp": d, "scale": scale, "title": pic.title,
           "label": label, "view": pic.view, "out": out, "frames": frames, "note": note})
}

/// A contour PNG of `field` ("von_mises" or "disp_mm") on the deformed shape.
pub fn contour(s: &Solved, field: &str, pic: &Picture, out: &Path) -> Result<()> {
    let sc = scene(s, field, 0, pic, &out.to_string_lossy(), 0);
    let j = out.with_extension("scene.json");
    std::fs::write(&j, serde_json::to_string(&sc)?)?;
    crate::tools::run_python(&[&crate::tools::script("render.py").to_string_lossy(), &j.to_string_lossy()], "render.py")?;
    let _ = std::fs::remove_file(&j);
    Ok(())
}

/// An mp4 of mode `mode` (0-based) cycling, plus its first frame as a PNG
/// poster next to it.
pub fn mode_animation(s: &Solved, mode: usize, pic: &Picture, out_mp4: &Path, frames: usize) -> Result<()> {
    let tmp = out_mp4.with_extension("frames");
    std::fs::create_dir_all(&tmp)?;
    let pattern = tmp.join("f%03d.png");
    let sc = scene(s, "mode", mode, pic, &pattern.to_string_lossy(), frames);
    let j = tmp.join("scene.json");
    std::fs::write(&j, serde_json::to_string(&sc)?)?;
    crate::tools::run_python(&[&crate::tools::script("render.py").to_string_lossy(), &j.to_string_lossy()], "render.py frames")?;
    let ff = crate::tools::ffmpeg()?;
    crate::tools::run(
        Command::new(ff).args(["-y", "-loglevel", "error", "-framerate", "20", "-stream_loop", "2", "-i"]).arg(&pattern).args(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart"]).arg(out_mp4),
        "ffmpeg",
    )?;
    std::fs::copy(tmp.join("f005.png"), out_mp4.with_extension("png")).ok();
    std::fs::remove_dir_all(&tmp).ok();
    Ok(())
}
