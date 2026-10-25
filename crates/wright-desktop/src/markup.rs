//! Drawing on the current view, sent to the agents as `Event::Feedback`.
//!
//! The stroke model is egui's painting demo
//! (`crates/egui_demo_lib/src/demo/painting.rs`, emilk/egui 0.37.0): lines
//! stored in normalised 0..1 canvas coordinates, a new line on each press,
//! drawn as `Shape::line`. The tool set and keys are the web studio's
//! (`crates/wright-studio/web/markup.js`), which took them from excalidraw
//! (`packages/excalidraw/components/Tools.tsx`: freedraw P, arrow A,
//! eraser E) and tldraw (`useTools.tsx`: highlight Shift+D); the eraser
//! removes whole strokes it touches, as both editors' erasers do.
//!
//! What the agents get is what the person saw: the app takes a screenshot of
//! the view with the strokes on it (`Context::request_screenshot`, as in
//! egui's `examples/screenshot`), crops it to the view, and saves it as a
//! PNG under `<run dir>/feedback/`; the event carries that relative path,
//! which the harness's inbox resolves against the run's out dir.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Highlight,
    Arrow,
    Eraser,
}

pub const COLORS: [(&str, Color32); 5] = [
    ("vermillion", Color32::from_rgb(0xff, 0x4f, 0x1f)),
    ("yellow", Color32::from_rgb(0xff, 0xd6, 0x0a)),
    ("sky", Color32::from_rgb(0x2e, 0xa8, 0xff)),
    ("white", Color32::WHITE),
    ("black", Color32::from_rgb(0x11, 0x11, 0x11)),
];

#[derive(Debug, Clone)]
pub struct Line {
    pub tool: Tool,
    pub color: Color32,
    /// 0..1 across the canvas.
    pub points: Vec<Pos2>,
}

#[derive(Debug, Clone)]
pub struct Markup {
    pub on: bool,
    pub tool: Tool,
    pub color: Color32,
    pub lines: Vec<Line>,
    drawing: bool,
    pub note: String,
    /// The canvas rect last frame (what a screenshot is cropped to).
    pub rect: Option<Rect>,
}

impl Default for Markup {
    fn default() -> Self {
        Markup { on: false, tool: Tool::Pen, color: COLORS[0].1, lines: vec![], drawing: false, note: String::new(), rect: None }
    }
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t - p).length()
}

impl Markup {
    pub fn has_ink(&self) -> bool {
        self.lines.iter().any(|l| l.points.len() >= 2)
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.drawing = false;
    }

    pub fn undo(&mut self) {
        self.lines.pop();
        self.drawing = false;
    }

    /// Keys while marking up: P pen, Shift+D highlight, A arrow, E eraser,
    /// Cmd/Ctrl+Z undo, Esc leave. Ignored while a text field has focus.
    pub fn keys(&mut self, ctx: &egui::Context) {
        if !self.on || ctx.egui_wants_keyboard_input() {
            return;
        }
        ctx.input(|i| {
            if i.key_pressed(egui::Key::P) {
                self.tool = Tool::Pen;
            }
            if i.key_pressed(egui::Key::D) && i.modifiers.shift {
                self.tool = Tool::Highlight;
            }
            if i.key_pressed(egui::Key::A) {
                self.tool = Tool::Arrow;
            }
            if i.key_pressed(egui::Key::E) {
                self.tool = Tool::Eraser;
            }
            if i.key_pressed(egui::Key::Escape) {
                self.on = false;
            }
        });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            self.undo();
        }
    }

    /// The canvas over `rect`: takes the pointer when on, and always paints
    /// the strokes (so they stay visible over the view until cleared).
    pub fn canvas(&mut self, ui: &mut egui::Ui, rect: Rect) {
        self.rect = Some(rect);
        let to_screen = egui::emath::RectTransform::from_to(Rect::from_min_size(Pos2::ZERO, egui::vec2(1.0, 1.0)), rect);
        let from_screen = to_screen.inverse();
        if self.on {
            let response = ui.interact(rect, ui.scope_id().with("markup-canvas"), egui::Sense::drag());
            if let Some(p) = response.interact_pointer_pos() {
                let c = from_screen * p;
                match self.tool {
                    Tool::Eraser => {
                        let r = 10.0 / rect.width().max(1.0);
                        self.lines.retain(|l| {
                            !l.points.windows(2).any(|w| dist_to_segment(c, w[0], w[1]) < r)
                                && !(l.points.len() == 1 && (l.points[0] - c).length() < r)
                        });
                    }
                    Tool::Arrow => {
                        if !self.drawing {
                            self.lines.push(Line { tool: Tool::Arrow, color: self.color, points: vec![c, c] });
                            self.drawing = true;
                        } else if let Some(l) = self.lines.last_mut() {
                            l.points[1] = c;
                        }
                    }
                    Tool::Pen | Tool::Highlight => {
                        if !self.drawing {
                            self.lines.push(Line { tool: self.tool, color: self.color, points: vec![] });
                            self.drawing = true;
                        }
                        let line = self.lines.last_mut().expect("just pushed");
                        if line.points.last() != Some(&c) {
                            line.points.push(c);
                        }
                    }
                }
            } else {
                self.drawing = false;
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let painter = ui.painter_at(rect);
        for l in &self.lines {
            let pts: Vec<Pos2> = l.points.iter().map(|p| to_screen * *p).collect();
            if pts.len() < 2 {
                continue;
            }
            match l.tool {
                Tool::Pen => {
                    painter.add(Shape::line(pts, Stroke::new(4.0, l.color)));
                }
                Tool::Highlight => {
                    painter.add(Shape::line(pts, Stroke::new(16.0, l.color.gamma_multiply(0.45))));
                }
                Tool::Arrow | Tool::Eraser => painter.arrow(pts[0], pts[1] - pts[0], Stroke::new(4.0, l.color)),
            }
        }
    }

    pub fn toolbar(&mut self, ui: &mut egui::Ui) {
        for (t, label, key) in [
            (Tool::Pen, "Pen", "P"),
            (Tool::Highlight, "Highlight", "Shift+D"),
            (Tool::Arrow, "Arrow", "A"),
            (Tool::Eraser, "Eraser", "E"),
        ] {
            ui.selectable_value(&mut self.tool, t, label).on_hover_text(key);
        }
        ui.separator();
        for (name, c) in COLORS {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
            ui.painter().circle_filled(rect.center(), 7.0, c);
            if self.color == c {
                ui.painter().circle_stroke(rect.center(), 9.0, Stroke::new(2.0, ui.visuals().strong_text_color()));
            }
            if resp.on_hover_text(name).clicked() {
                self.color = c;
            }
        }
        ui.separator();
        if ui.button("Undo").on_hover_text("Cmd+Z").clicked() {
            self.undo();
        }
        if ui.button("Clear").clicked() {
            self.clear();
        }
    }
}

/// Crop a screenshot to `rect` (points) and encode it as PNG.
pub fn crop_png(image: &egui::ColorImage, rect: Rect, pixels_per_point: f32) -> anyhow::Result<Vec<u8>> {
    let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(image.size[0] as f32, image.size[1] as f32) / pixels_per_point);
    let r = rect.intersect(screen);
    let region = image.region(&r, Some(pixels_per_point));
    let (w, h) = (region.size[0] as u32, region.size[1] as u32);
    let raw: Vec<u8> = region.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let buf = image::RgbaImage::from_raw(w, h, raw).ok_or_else(|| anyhow::anyhow!("screenshot region {w}x{h} is empty"))?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(buf).write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

/// A filename-safe run id or view name (the studio's `slug`).
pub fn slug(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '_' })
        .take(48)
        .collect();
    if out.is_empty() { "x".into() } else { out }
}

/// Save a sketch under `<root>/feedback/`; returns (path relative to
/// `root` -- what the event carries --, absolute path).
pub fn save_sketch(root: &Path, run: &str, view: &str, png: &[u8]) -> anyhow::Result<(String, PathBuf)> {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let rel = format!("feedback/{}-{}-{ms}.png", slug(run), slug(view));
    let abs = root.join(&rel);
    if let Some(d) = abs.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&abs, png)?;
    Ok((rel, abs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cropped_screenshot_is_a_png_of_the_view() {
        let mut img = egui::ColorImage::filled([200, 100], Color32::BLACK);
        img.pixels[50 * 200 + 120] = Color32::RED;
        let png = crop_png(&img, Rect::from_min_size(Pos2::new(50.0, 25.0), egui::vec2(50.0, 25.0)), 2.0).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let back = image::load_from_memory(&png).unwrap();
        assert_eq!((back.width(), back.height()), (100, 50));
        assert_eq!(back.to_rgba8().get_pixel(20, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn sketches_land_under_feedback() {
        let dir = std::env::temp_dir().join(format!("wright-desktop-test-{}", std::process::id()));
        let (rel, abs) = save_sketch(&dir, "aircraft-1", "3d iso", b"\x89PNG").unwrap();
        assert!(rel.starts_with("feedback/aircraft-1-3d_iso-"));
        assert!(abs.is_file());
        let _ = std::fs::remove_dir_all(dir);
    }
}
