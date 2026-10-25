//! Cut lists for tube and rod stock, and the cutting film.
//!
//! Nesting cuts into stock lengths is the 1D cutting-stock problem. Prior
//! art: OpenCutList (lairdubois/lairdubois-opencutlist-sketchup-extension),
//! `src/ladb_opencutlist/ruby/lib/bin_packing_1d/packerffd.rb`,
//! `PackerFFD#first_fit_decreasing`: boxes (cuts) longest first, each into
//! the first bin whose `current_leftover` holds it, else a new bin of
//! `base_bin_length`, else unplaced; the saw kerf is charged per cut by
//! `Bin#add`. Kept: that loop, "unplaced" for a cut longer than the bar.
//! Not taken: its dynamic-programming packer (`packerdp.rb`); with a dozen
//! cuts FFD is enough and is easy to check by hand.

use crate::render::{rgb, Camera, Renderer, Scene};
use crate::rocket::split_csv;
use crate::video::{fmt_minutes, Painter, Video, FPS};
use serde::Serialize;
use std::path::Path;
use wright_geom::v3;

#[derive(Clone, Debug, Serialize)]
pub struct StockKind {
    pub id: String,
    pub name: String,
    pub length_mm: f64,
    pub od_mm: f64,
    pub qty_bought: u32,
    pub material: String,
    pub kerf_mm: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Cut {
    pub label: String,
    pub length_mm: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Bar {
    pub stock: usize,
    pub index: u32,
    /// (cut, offset from the bar's end, mm)
    pub cuts: Vec<(Cut, f64)>,
    pub offcut_mm: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CutPlan {
    pub stocks: Vec<StockKind>,
    pub bars: Vec<Bar>,
    /// cuts that fit no stock length (longer than the bar)
    pub unplaced: Vec<(usize, Cut)>,
    pub minutes: f64,
}

/// First-fit decreasing, kerf per cut.
pub fn plan(groups: Vec<(StockKind, Vec<Cut>)>) -> CutPlan {
    let mut out = CutPlan { stocks: vec![], bars: vec![], unplaced: vec![], minutes: 0.0 };
    for (sk, mut cuts) in groups {
        let si = out.stocks.len();
        cuts.sort_by(|a, b| b.length_mm.total_cmp(&a.length_mm));
        let mut bars: Vec<Bar> = Vec::new();
        for c in cuts {
            if c.length_mm > sk.length_mm {
                out.unplaced.push((si, c));
                continue;
            }
            let need = c.length_mm + sk.kerf_mm;
            let slot = bars.iter_mut().find(|b| b.offcut_mm + 1e-9 >= need || (b.offcut_mm + 1e-9 >= c.length_mm && b.cuts.is_empty()));
            match slot {
                Some(b) => {
                    let off = sk.length_mm - b.offcut_mm;
                    b.offcut_mm = (b.offcut_mm - need).max(0.0);
                    b.cuts.push((c, off));
                }
                None => {
                    let n = bars.len() as u32;
                    let off = 0.0;
                    let rest = (sk.length_mm - need).max(0.0);
                    bars.push(Bar { stock: si, index: n + 1, cuts: vec![(c, off)], offcut_mm: rest });
                }
            }
        }
        out.minutes += bars.iter().map(|b| b.cuts.len()).sum::<usize>() as f64 * crate::rules::CUT_MIN;
        out.bars.extend(bars);
        out.stocks.push(sk);
    }
    out
}

impl CutPlan {
    pub fn csv(&self) -> String {
        let mut s = String::from("stock_id,stock,bar,cut,length_mm,from_end_mm,kerf_mm\n");
        for b in &self.bars {
            let sk = &self.stocks[b.stock];
            for (c, off) in &b.cuts {
                s.push_str(&format!("{},\"{}\",{},\"{}\",{:.1},{:.1},{:.1}\n", sk.id, sk.name.replace('"', "\"\""), b.index, c.label, c.length_mm, off, sk.kerf_mm));
            }
            s.push_str(&format!("{},\"{}\",{},\"offcut\",{:.1},,\n", sk.id, sk.name.replace('"', "\"\""), b.index, b.offcut_mm));
        }
        for (si, c) in &self.unplaced {
            s.push_str(&format!("{},\"{}\",,\"DOES NOT FIT: {}\",{:.1},,\n", self.stocks[*si].id, self.stocks[*si].name.replace('"', "\"\""), c.label, c.length_mm));
        }
        s
    }
    /// Bars needed vs bought, per stock: (stock, needed, bought).
    pub fn shortfalls(&self) -> Vec<(String, u32, u32)> {
        self.stocks.iter().enumerate().map(|(i, s)| (s.name.clone(), self.bars.iter().filter(|b| b.stock == i).count() as u32, s.qty_bought)).collect()
    }
}

/// Length of a stock from its name: "x 1 m", "1M", "x 24\"", "72\" cut", "60\"".
pub fn length_from_name(name: &str) -> Option<f64> {
    let l = name.to_ascii_lowercase();
    let chars: Vec<char> = l.chars().collect();
    let mut best = None;
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            let num: f64 = chars[i..j].iter().collect::<String>().parse().unwrap_or(0.0);
            let rest: String = chars[j..].iter().take(4).collect();
            let rest_t = rest.trim_start();
            let prev: String = chars[..i].iter().rev().take(3).collect::<String>().chars().rev().collect();
            if rest_t.starts_with("m ") || rest_t == "m" || rest_t.starts_with("m,") || (rest.starts_with('m') && !rest.starts_with("mm")) {
                best = Some(num * 1000.0);
            } else if (rest.starts_with('"') || rest.starts_with("in")) && (prev.contains('x') || rest.contains("cut") || num >= 12.0) {
                // a length in inches (diameters come first and are smaller)
                if num >= 12.0 || prev.contains('x') {
                    best = Some(num * 25.4);
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    best
}

fn od_from_name(name: &str) -> f64 {
    let l = name.to_ascii_lowercase();
    if let Some(p) = l.find("mm od") {
        let num: String = l[..p].chars().rev().skip_while(|c| *c == ' ').take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>().chars().rev().collect();
        if let Ok(v) = num.parse() {
            return v;
        }
    }
    if let Some(p) = l.find("\" od") {
        let num: String = l[..p].chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>().chars().rev().collect();
        if let Ok(v) = num.parse::<f64>() {
            return v * 25.4;
        }
    }
    if let Some(p) = l.find("mm x") {
        let num: String = l[..p].chars().rev().skip_while(|c| *c == ' ').take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>().chars().rev().collect();
        if let Ok(v) = num.parse() {
            return v;
        }
    }
    if l.contains(".500\"") || l.contains("0.5\"") {
        return 12.7;
    }
    10.0
}

/// Cuts the BOM names in its notes ("cut to 974, 974, 540, 540 mm").
pub fn from_bom(bom_csv: &str) -> Vec<(StockKind, Vec<Cut>)> {
    let mut out = Vec::new();
    for line in bom_csv.lines().skip(1) {
        let c = split_csv(line);
        if c.len() < 9 {
            continue;
        }
        let note = c[8].to_ascii_lowercase();
        let Some(p) = note.find("cut to ") else { continue };
        let lens: Vec<f64> = note[p + 7..].split(|ch: char| ch == ',' || ch.is_whitespace()).filter_map(|t| t.trim_end_matches("mm").parse().ok()).collect();
        if lens.is_empty() {
            continue;
        }
        let name = c[2].clone();
        let len = length_from_name(&name).unwrap_or(1000.0);
        let qty = c[3].parse::<f64>().unwrap_or(1.0).ceil() as u32;
        let material = if name.to_ascii_lowercase().contains("carbon") { "carbon fibre" } else { "tube" };
        let sk = StockKind { id: c[1].clone(), name: name.clone(), length_mm: len, od_mm: od_from_name(&name), qty_bought: qty, material: material.into(), kerf_mm: crate::rules::CARBON_KERF_MM };
        let short = c[1].split('_').take(2).collect::<Vec<_>>().join(" ");
        let cuts = lens.iter().enumerate().map(|(i, &l)| Cut { label: format!("{short} #{}", i + 1), length_mm: l }).collect();
        out.push((sk, cuts));
    }
    out
}

/// The rocket's bays and tanks cut from the tube stock (design sections).
pub fn rocket_cuts(design: &serde_json::Value, bom_csv: &str) -> Vec<(StockKind, Vec<Cut>)> {
    let v = &design["vehicle"];
    let Some(secs) = v["sections"].as_array() else { return vec![] };
    let p = &design["params"];
    let mut out = Vec::new();
    for (param, keys, kerf, material) in [
        ("tank_tube", &["lox_tank", "intertank", "fuel_tank"][..], crate::rules::SAW_KERF_MM, "6061-T6 tube"),
        ("airframe_tube", &["recovery", "pressurant", "engine_bay"][..], crate::rules::CARBON_KERF_MM, "G12 fibreglass tube"),
    ] {
        let id = p[param].as_str().unwrap_or("");
        let row = bom_csv.lines().map(split_csv).find(|c| c.len() > 3 && c[1] == id);
        let Some(row) = row else { continue };
        let name = row[2].clone();
        let len = length_from_name(&name).unwrap_or(1500.0);
        let cuts: Vec<Cut> = secs.iter().filter_map(|s| {
            let a = s.as_array()?;
            let k = a.first()?.as_str()?;
            let len = (a.get(2)?.as_f64()? - a.get(1)?.as_f64()?) * 1000.0;
            keys.contains(&k).then(|| Cut { label: k.replace('_', " "), length_mm: len })
        }).collect();
        if cuts.is_empty() {
            continue;
        }
        out.push((StockKind { id: id.into(), name: name.clone(), length_mm: len, od_mm: od_from_name(&name).max(150.0), qty_bought: row[3].parse::<f64>().unwrap_or(1.0).ceil() as u32, material: material.into(), kerf_mm: kerf }, cuts));
    }
    out
}

/// The cutting film: bars side by side, the saw stepping through the cuts,
/// each piece sliding off as it is cut.
pub fn film(r: &Renderer, painter: &Painter, plan: &CutPlan, title: &str, mp4: &Path, png: &Path) -> anyhow::Result<()> {
    let jobs: Vec<(usize, usize)> = plan.bars.iter().enumerate().flat_map(|(bi, b)| (0..b.cuts.len()).map(move |ci| (bi, ci))).collect();
    anyhow::ensure!(!jobs.is_empty(), "nothing to cut");
    // display radius: true OD, but at least 1 % of the longest bar so thin rods show
    let longest = plan.stocks.iter().map(|s| s.length_mm).fold(0.0, f64::max);
    let rad = |s: &StockKind| (s.od_mm / 2.0).max(0.008 * longest);
    let mut ys = Vec::new();
    let mut y = 0.0;
    for b in &plan.bars {
        let rr = rad(&plan.stocks[b.stock]);
        ys.push(y + rr);
        y += 2.0 * rr + 0.05 * longest + rr * 2.0;
    }
    let center = v3(longest / 2.0, y / 2.0, 0.0);
    let radius = 0.5 * (longest * longest + y * y).sqrt();
    let per = 24usize;
    let frames = (jobs.len() * per).clamp(150, 420);
    let mut video = Video::create(mp4)?;
    let mut last = Vec::new();
    for k in 0..=frames {
        let tt = k as f64 / frames as f64 * jobs.len() as f64;
        let done = tt.floor() as usize;
        let phase = tt - done as f64;
        let mut sc = Scene::default();
        sc.cuboid(v3(-0.05 * longest, -0.05 * longest, -0.04 * longest), v3(1.05 * longest, y + 0.02 * longest, -0.03 * longest), rgb([44, 46, 52]));
        for (bi, b) in plan.bars.iter().enumerate() {
            let sk = &plan.stocks[b.stock];
            let rr = rad(sk);
            let base = if sk.material.contains("carbon") { rgb([40, 40, 44]) } else if sk.material.contains("G12") { rgb([200, 205, 170]) } else { rgb([180, 186, 196]) };
            let piece = rgb([240, 128, 32]);
            let mut x_rest = 0.0;
            for (ci, (c, off)) in b.cuts.iter().enumerate() {
                let j = jobs.iter().position(|&q| q == (bi, ci)).unwrap();
                let cut_done = j < done || (j == done && phase > 0.6);
                let slide = if j < done { 1.0 } else if j == done && phase > 0.6 { (phase - 0.6) / 0.4 } else { 0.0 };
                let dy = -slide * (rr * 2.5);
                let c0 = v3(*off, ys[bi] + dy, 0.0);
                sc.cylinder(c0, c0 + v3(c.length_mm, 0.0, 0.0), rr, 24, if cut_done { piece } else { base });
                x_rest = off + c.length_mm + sk.kerf_mm;
            }
            if x_rest < sk.length_mm {
                sc.cylinder(v3(x_rest, ys[bi], 0.0), v3(sk.length_mm, ys[bi], 0.0), rr, 24, base);
            }
        }
        let cur = jobs[done.min(jobs.len() - 1)];
        let (bi, ci) = cur;
        let b = &plan.bars[bi];
        let sk = &plan.stocks[b.stock];
        let (c, off) = &b.cuts[ci];
        if k < frames {
            // the blade: a thin disc coming down at the cut's far end
            let x = off + c.length_mm + sk.kerf_mm / 2.0;
            let rr = rad(sk);
            let blade_r = (rr * 4.0).max(0.04 * longest);
            let down = if phase < 0.6 { phase / 0.6 } else { 1.0 - (phase - 0.6) / 0.4 };
            let zc = rr + blade_r * (1.0 - down) + 0.2 * rr;
            let zc = zc + blade_r * 0.0 - down * (rr * 1.2);
            sc.cylinder(v3(x - sk.kerf_mm.max(0.002 * longest) / 2.0, ys[bi], zc), v3(x + sk.kerf_mm.max(0.002 * longest) / 2.0, ys[bi], zc), blade_r, 40, rgb([210, 214, 222]));
        }
        let prog = k as f64 / frames as f64;
        let cam = Camera::orbit(center, Camera::fit_distance(radius * 0.8, 30.0), -80.0 + 16.0 * prog, 58.0);
        let mut img = r.draw(&sc, &cam);
        let done_cuts = done.min(jobs.len());
        painter.caption(
            &mut img,
            &format!("{title}: cut list"),
            &format!("bar {} of {}: {} {:.0} mm", b.index, sk.name.chars().take(48).collect::<String>(), c.label, c.length_mm),
            &[format!("cut {done_cuts}/{}", jobs.len()), format!("~{}", fmt_minutes(done_cuts as f64 * crate::rules::CUT_MIN)), format!("kerf {:.1} mm", sk.kerf_mm)],
            prog,
        );
        video.push(&img)?;
        last = img;
    }
    for _ in 0..FPS {
        video.push(&last)?;
    }
    video.finish()?;
    std::fs::write(png, crate::render::png(&last))?;
    Ok(())
}
