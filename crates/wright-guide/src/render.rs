//! A small software renderer: orthographic views of a part set, flat shaded,
//! hidden surfaces removed with a z-buffer, written as PNG.
//!
//! Prior art: ssloy/tinyrenderer (`our_gl.cpp`, `rasterize`): per triangle,
//! walk the screen bounding box clipped to the image, take each pixel's
//! barycentric coordinates, skip it when one is negative, interpolate depth
//! from them and keep the fragment only if it is nearer than the z-buffer.
//! Kept: that loop and its names (`bc`, `zbuffer`). Deviations, each for a
//! stated reason:
//! * orthographic only (`View`), no perspective divide: the guide's pictures
//!   are assembly drawings, where parallel lines stay parallel;
//! * no back-face culling (tinyrenderer drops `det < 1`): meshes out of CSG
//!   booleans are not guaranteed consistently wound, and the z-buffer gives
//!   the right answer either way; shading is two-sided instead;
//! * pixel centres at +0.5 and 2x2 supersampling, so thin booms do not
//!   alias away;
//! * an outline pass after rasterising (part boundaries, depth jumps and
//!   creases), the usual look of a technical illustration;
//! * an "x-ray" for highlighted parts: a second z-buffer holds the nearest
//!   highlighted fragment, so a battery inside a fuselage still shows through
//!   it in its assembly step instead of vanishing.
//!
//! Frame: any right-handed frame with z up (wright-air uses x aft,
//! y starboard; wright-rocket z along the body). `Front` looks along +x,
//! `Side` along +y, `Top` down -z with -x at the top of the image.

use wright_geom::{v3, Aabb, Part, PartKind, V3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Iso,
    Top,
    Front,
    Side,
}

impl View {
    pub const ALL: [View; 4] = [View::Iso, View::Top, View::Front, View::Side];
    pub fn name(self) -> &'static str {
        match self {
            View::Iso => "iso",
            View::Top => "top",
            View::Front => "front",
            View::Side => "side",
        }
    }
    /// (view direction, right, up), unit vectors.
    fn basis(self) -> (V3, V3, V3) {
        let (d, hint) = match self {
            View::Iso => (v3(1.0, 1.0, -1.0), v3(0.0, 0.0, 1.0)),
            View::Top => (v3(0.0, 0.0, -1.0), v3(-1.0, 0.0, 0.0)),
            View::Front => (v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)),
            View::Side => (v3(0.0, 1.0, 0.0), v3(0.0, 0.0, 1.0)),
        };
        let d = d.unit();
        let right = d.cross(hint).unit();
        (d, right, right.cross(d).unit())
    }
}

/// Colours, sRGB.
pub const BACKGROUND: [u8; 3] = [255, 255, 255];
pub const PRINTED: [u8; 3] = [120, 166, 214];
pub const BOUGHT: [u8; 3] = [96, 102, 112];
pub const HIGHLIGHT: [u8; 3] = [240, 128, 32];
const HIDDEN_NEW: [u8; 3] = [120, 40, 0];
const OUTLINE: [u8; 3] = [34, 38, 46];

/// What to draw and how.
#[derive(Debug, Clone)]
pub struct Options {
    pub view: View,
    pub width: u32,
    pub height: u32,
    /// Part names drawn in [`HIGHLIGHT`] (seen through anything in front of
    /// them); every other part is ghosted. Empty: plain colours by kind.
    pub highlight: Vec<String>,
    /// Points to fit in the image; empty fits the parts drawn. Pass the
    /// whole vehicle ([`Options::fit_parts`]) to keep a series of step
    /// images aligned.
    pub fit: Vec<V3>,
}

impl Options {
    pub fn new(view: View, width: u32, height: u32) -> Self {
        Self { view, width, height, highlight: Vec::new(), fit: Vec::new() }
    }
    pub fn highlight<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.highlight = names.into_iter().map(Into::into).collect();
        self
    }
    /// Fit a box's eight corners.
    pub fn frame(mut self, b: Aabb) -> Self {
        self.fit = (0..8u8)
            .map(|i| {
                v3(
                    if i & 1 == 0 { b.min.x } else { b.max.x },
                    if i & 2 == 0 { b.min.y } else { b.max.y },
                    if i & 4 == 0 { b.min.z } else { b.max.z },
                )
            })
            .collect();
        self
    }
    /// Fit these parts' vertices (the tightest framing of a whole vehicle).
    pub fn fit_parts(mut self, parts: &[Part]) -> Self {
        self.fit = vertices(parts);
        self
    }
}

/// A rendered image with what is under every pixel.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGB, row-major from the top-left.
    pub rgb: Vec<u8>,
    /// Index into the part slice of the visible part per pixel (-1: none),
    /// at output resolution (the majority of its supersamples). A studio
    /// can use it to name the part a person circled.
    pub part: Vec<i32>,
}

impl Frame {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let i = 3 * (y * self.width + x) as usize;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }
    pub fn part_at(&self, x: u32, y: u32) -> Option<usize> {
        let v = self.part[(y * self.width + x) as usize];
        (v >= 0).then_some(v as usize)
    }
    pub fn png(&self) -> Vec<u8> {
        encode_png(self.width, self.height, &self.rgb)
    }
}

/// Render `parts` and return PNG bytes.
pub fn render(parts: &[Part], view: View, size: (u32, u32)) -> Vec<u8> {
    rasterize(parts, &Options::new(view, size.0, size.1)).png()
}

/// Render with options (highlight, fixed frame) and return PNG bytes.
pub fn render_with(parts: &[Part], opts: &Options) -> Vec<u8> {
    rasterize(parts, opts).png()
}

/// Bounds of every non-empty part, or `None` if nothing has geometry.
pub fn bounds(parts: &[Part]) -> Option<Aabb> {
    let mut b: Option<Aabb> = None;
    for p in parts.iter().filter(|p| !p.mesh.is_empty()) {
        let pb = p.mesh.bounds();
        b = Some(match b {
            None => pb,
            Some(a) => Aabb { min: a.min.min(pb.min), max: a.max.max(pb.max) },
        });
    }
    b
}

fn vertices(parts: &[Part]) -> Vec<V3> {
    parts.iter().flat_map(|p| p.mesh.polygons.iter().flat_map(|q| q.vertices.iter().copied())).collect()
}

const SS: u32 = 2; // supersampling per axis
const MARGIN: f64 = 0.06; // fraction of the image left blank around the frame

struct Buffers {
    w: usize,
    h: usize,
    zbuffer: Vec<f64>,
    id: Vec<i32>,
    shade: Vec<f32>,
    normal: Vec<V3>,
    // nearest highlighted fragment, regardless of what is in front of it
    hz: Vec<f64>,
    hid: Vec<i32>,
    hshade: Vec<f32>,
    // nearest highlighted fragment of a *different* part behind that one
    // (a flight controller inside a fuselage added in the same step)
    hz2: Vec<f64>,
}

/// Rasterise to a [`Frame`] (the PNG-free core; tests read it directly).
pub fn rasterize(parts: &[Part], opts: &Options) -> Frame {
    let (w, h) = ((opts.width * SS) as usize, (opts.height * SS) as usize);
    let n = w * h;
    let mut buf = Buffers {
        w,
        h,
        zbuffer: vec![f64::INFINITY; n],
        id: vec![-1; n],
        shade: vec![0.0; n],
        normal: vec![v3(0.0, 0.0, 0.0); n],
        hz: vec![f64::INFINITY; n],
        hid: vec![-1; n],
        hshade: vec![0.0; n],
        hz2: vec![f64::INFINITY; n],
    };
    let (d, right, up) = opts.view.basis();
    // a fixed light in view space: over the viewer's left shoulder
    let light = (up * 0.6 - right * 0.45 - d * 0.75).unit();
    let own;
    let fit = if opts.fit.is_empty() {
        own = vertices(parts);
        &own
    } else {
        &opts.fit
    };
    let mut depth_extent = 1.0;
    if !fit.is_empty() {
        // fit the points' projection
        let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
        let (mut bmin, mut bmax) = (fit[0], fit[0]);
        for &c in fit {
            let (sx, sy) = (c.dot(right), c.dot(up));
            lo = (lo.0.min(sx), lo.1.min(sy));
            hi = (hi.0.max(sx), hi.1.max(sy));
            bmin = bmin.min(c);
            bmax = bmax.max(c);
        }
        depth_extent = (bmax - bmin).norm().max(1e-9);
        let (wf, hf) = (w as f64, h as f64);
        let span = ((hi.0 - lo.0).max(1e-9), (hi.1 - lo.1).max(1e-9));
        let scale = ((wf * (1.0 - 2.0 * MARGIN)) / span.0).min((hf * (1.0 - 2.0 * MARGIN)) / span.1);
        let (cx, cy) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
        let to_screen = |p: V3| -> (f64, f64, f64) {
            (wf / 2.0 + (p.dot(right) - cx) * scale, hf / 2.0 - (p.dot(up) - cy) * scale, p.dot(d))
        };
        for (pi, part) in parts.iter().enumerate() {
            let hl = opts.highlight.contains(&part.name);
            for tri in part.mesh.triangles() {
                let nrm = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
                if nrm.norm() <= 0.0 {
                    continue;
                }
                let mut nrm = nrm.unit();
                if nrm.dot(d) > 0.0 {
                    nrm = -nrm; // two-sided: face the viewer
                }
                let shade = (0.38 + 0.62 * nrm.dot(light).max(0.0)) as f32;
                let s = tri.map(to_screen);
                raster_triangle(&mut buf, s, pi as i32, shade, nrm, hl);
            }
        }
    }
    resolve(parts, opts, &buf, depth_extent)
}

/// tinyrenderer's `rasterize`: bounding box, barycentric test, z-buffer.
fn raster_triangle(buf: &mut Buffers, s: [(f64, f64, f64); 3], id: i32, shade: f32, nrm: V3, hl: bool) {
    let [(ax, ay, az), (bx, by, bz), (cx, cy, cz)] = s;
    let det = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
    if det.abs() < 1e-12 {
        return; // edge-on: covers no area
    }
    let x0 = ax.min(bx).min(cx).floor().max(0.0) as usize;
    let x1 = (ax.max(bx).max(cx).ceil() as isize).min(buf.w as isize - 1);
    let y0 = ay.min(by).min(cy).floor().max(0.0) as usize;
    let y1 = (ay.max(by).max(cy).ceil() as isize).min(buf.h as isize - 1);
    if x1 < 0 || y1 < 0 {
        return;
    }
    for y in y0..=y1 as usize {
        let py = y as f64 + 0.5;
        for x in x0..=x1 as usize {
            let px = x as f64 + 0.5;
            // barycentric coordinates of the pixel centre
            let b1 = ((px - ax) * (cy - ay) - (py - ay) * (cx - ax)) / det;
            let b2 = ((bx - ax) * (py - ay) - (by - ay) * (px - ax)) / det;
            let b0 = 1.0 - b1 - b2;
            const E: f64 = -1e-9; // shared edges: no cracks
            if b0 < E || b1 < E || b2 < E {
                continue;
            }
            let z = b0 * az + b1 * bz + b2 * cz;
            let i = y * buf.w + x;
            if z < buf.zbuffer[i] {
                buf.zbuffer[i] = z;
                buf.id[i] = id;
                buf.shade[i] = shade;
                buf.normal[i] = nrm;
            }
            if hl {
                if z < buf.hz[i] {
                    if buf.hid[i] != id {
                        buf.hz2[i] = buf.hz[i]; // the old nearest is now behind
                    }
                    buf.hz[i] = z;
                    buf.hid[i] = id;
                    buf.hshade[i] = shade;
                } else if buf.hid[i] != id && z < buf.hz2[i] {
                    buf.hz2[i] = z;
                }
            }
        }
    }
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| a[k] as f32 * (1.0 - t) + b[k] as f32 * t)
}

fn scale(c: [u8; 3], s: f32) -> [f32; 3] {
    c.map(|v| v as f32 * s)
}

/// Colour, outline and downsample.
fn resolve(parts: &[Part], opts: &Options, buf: &Buffers, depth_extent: f64) -> Frame {
    let (w, h) = (buf.w, buf.h);
    let any_hl = !opts.highlight.is_empty();
    let is_hl: Vec<bool> = parts.iter().map(|p| opts.highlight.contains(&p.name)).collect();
    let jump = depth_extent * 0.02;
    let mut hi = vec![[0f32; 3]; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let id = buf.id[i];
            let mut c = if id < 0 {
                BACKGROUND.map(|v| v as f32)
            } else {
                let p = &parts[id as usize];
                let base = match p.kind {
                    PartKind::Printed => PRINTED,
                    PartKind::Bought => BOUGHT,
                };
                let s = buf.shade[i];
                if is_hl[id as usize] {
                    scale(HIGHLIGHT, 0.55 + 0.45 * s)
                } else if any_hl {
                    // ghosted context
                    let lit = scale(base, s);
                    [0, 1, 2].map(|k| lit[k] * 0.3 + 255.0 * 0.7)
                } else {
                    scale(base, s)
                }
            };
            // x-ray: a highlighted part behind the visible surface
            if any_hl && buf.hz[i].is_finite() && (id < 0 || !is_hl[id as usize]) {
                let hc = scale(HIGHLIGHT, 0.55 + 0.45 * buf.hshade[i]);
                c = [0, 1, 2].map(|k| c[k] * 0.45 + hc[k] * 0.55);
            } else if any_hl && id >= 0 && is_hl[id as usize] && buf.hz2[i].is_finite() {
                // a new part inside another new part: darker, seen through
                c = [0, 1, 2].map(|k| c[k] * 0.55 + HIDDEN_NEW[k] as f32 * 0.45);
            }
            // outline: a neighbour on another part, across a depth jump, or
            // across a crease
            let mut edge = false;
            if id >= 0 {
                for (dx, dy) in [(1isize, 0isize), (0, 1), (-1, 0), (0, -1)] {
                    let (nx, ny) = (x as isize + dx, y as isize + dy);
                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    let other = buf.id[j];
                    if other != id {
                        // draw the boundary on the nearer side only
                        if other < 0 || buf.zbuffer[i] <= buf.zbuffer[j] {
                            edge = true;
                        }
                    } else if (buf.zbuffer[j] - buf.zbuffer[i]) > jump || buf.normal[i].dot(buf.normal[j]) < 0.82 {
                        edge = true;
                    }
                }
            }
            if edge {
                let o = if any_hl && !is_hl[id as usize] { mix(OUTLINE, BACKGROUND, 0.6) } else { OUTLINE.map(|v| v as f32) };
                c = o;
            }
            hi[i] = c;
        }
    }
    // box-filter downsample
    let (ow, oh) = (opts.width as usize, opts.height as usize);
    let mut rgb = vec![0u8; ow * oh * 3];
    let mut part = vec![-1i32; ow * oh];
    for y in 0..oh {
        for x in 0..ow {
            let mut acc = [0f32; 3];
            let mut votes: Vec<i32> = Vec::with_capacity((SS * SS) as usize);
            for sy in 0..SS as usize {
                for sx in 0..SS as usize {
                    let i = (y * SS as usize + sy) * w + x * SS as usize + sx;
                    for k in 0..3 {
                        acc[k] += hi[i][k];
                    }
                    votes.push(buf.id[i]);
                }
            }
            let nss = (SS * SS) as f32;
            for k in 0..3 {
                rgb[3 * (y * ow + x) + k] = (acc[k] / nss).round().clamp(0.0, 255.0) as u8;
            }
            votes.sort_unstable();
            let mut best = (votes[0], 0);
            let mut run = (votes[0], 0);
            for &v in &votes {
                run = if v == run.0 { (v, run.1 + 1) } else { (v, 1) };
                if run.1 > best.1 {
                    best = run;
                }
            }
            part[y * ow + x] = best.0;
        }
    }
    Frame { width: opts.width, height: opts.height, rgb, part }
}

/// RGB8 to PNG with the `png` crate.
pub fn encode_png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Best);
        let mut wr = enc.write_header().expect("PNG header to memory");
        wr.write_image_data(rgb).expect("PNG data to memory");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_geom::shapes::cuboid;

    fn part(name: &str, kind: PartKind, min: V3, max: V3) -> Part {
        Part { name: name.into(), kind, material: "pla".into(), mesh: cuboid(min, max), touches: vec![] }
    }

    #[test]
    fn a_cube_from_the_front_is_a_centred_square() {
        let c = part("cube", PartKind::Printed, v3(-10.0, -10.0, -10.0), v3(10.0, 10.0, 10.0));
        let f = rasterize(&[c], &Options::new(View::Front, 100, 100));
        // the square fills 88 % of the frame: pixels 6..94
        assert_eq!(f.part_at(50, 50), Some(0));
        assert_eq!(f.part_at(8, 8), Some(0));
        assert_eq!(f.part_at(91, 91), Some(0));
        assert_eq!(f.part_at(3, 50), None);
        assert_eq!(f.part_at(50, 97), None);
        assert_eq!(f.pixel(1, 1), BACKGROUND);
        let covered = f.part.iter().filter(|&&p| p == 0).count();
        assert!((covered as i64 - 88 * 88).abs() < 200, "{covered}");
        // the face is lit, and drawn in the printed colour's hue
        let [r, g, b] = f.pixel(50, 50);
        assert!(b > r && b > g, "{r} {g} {b}");
    }

    #[test]
    fn an_iso_cube_is_a_hexagon() {
        let c = part("cube", PartKind::Bought, v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0));
        let f = rasterize(&[c], &Options::new(View::Iso, 120, 120));
        // centre covered, the bounding square's corners empty (a hexagon)
        assert_eq!(f.part_at(60, 60), Some(0));
        for (x, y) in [(18, 12), (102, 12), (18, 108), (102, 108)] {
            assert_eq!(f.part_at(x, y), None, "corner {x},{y}");
        }
        // three faces at three different shades
        let mut shades: Vec<[u8; 3]> = [(60, 30), (35, 75), (85, 75)].iter().map(|&(x, y)| f.pixel(x, y)).collect();
        shades.dedup();
        assert_eq!(shades.len(), 3, "{shades:?}");
    }

    #[test]
    fn a_part_behind_another_is_not_drawn() {
        // a battery inside a fuselage, from the side: the painter's renderer
        // in the prototype drew it through the skin
        let fuse = part("fuselage", PartKind::Printed, v3(0.0, -20.0, -20.0), v3(200.0, 20.0, 20.0));
        let batt = part("battery", PartKind::Bought, v3(50.0, -10.0, -10.0), v3(120.0, 10.0, 10.0));
        // battery listed last, so a painter's order would draw it on top
        let parts = [fuse, batt];
        for view in View::ALL {
            let f = rasterize(&parts, &Options::new(view, 160, 160));
            assert!(f.part.iter().all(|&p| p != 1), "battery visible in {view:?}");
            assert!(f.part.contains(&0));
        }
        // ...unless highlighted, when it shows through as an x-ray tint
        let f = rasterize(&parts, &Options::new(View::Side, 160, 160).highlight(["battery"]));
        assert!(f.part.iter().all(|&p| p != 1));
        let centre = f.pixel(80, 80);
        assert!(centre[0] > centre[2] + 40, "no orange tint: {centre:?}");
    }

    #[test]
    fn a_part_in_front_hides_the_one_behind_regardless_of_order() {
        let near = part("near", PartKind::Bought, v3(-5.0, -5.0, 0.0), v3(5.0, 5.0, 10.0));
        let far = part("far", PartKind::Printed, v3(-20.0, -20.0, -30.0), v3(20.0, 20.0, -20.0));
        for parts in [[near.clone(), far.clone()], [far.clone(), near.clone()]] {
            let f = rasterize(&parts, &Options::new(View::Top, 100, 100));
            let ni = parts.iter().position(|p| p.name == "near").unwrap();
            assert_eq!(f.part_at(50, 50), Some(ni));
            assert_eq!(f.part_at(15, 15), Some(1 - ni));
        }
    }

    #[test]
    fn png_bytes_have_the_signature() {
        let c = part("cube", PartKind::Printed, v3(0.0, 0.0, 0.0), v3(1.0, 1.0, 1.0));
        let png = render(&[c], View::Iso, (32, 24));
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // and nothing to draw is a blank image, not a panic
        assert_eq!(&render(&[], View::Top, (8, 8))[..8], b"\x89PNG\r\n\x1a\n");
    }
}
