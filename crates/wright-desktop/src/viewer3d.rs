//! The CAD view: shaded faces with black feature edges over them, an orbit
//! camera, selection, a section plane, an exploded view and a measure tool.
//!
//! Prior art, read and followed:
//! * GL inside egui: `examples/custom_3d_glow/src/main.rs` (emilk/egui
//!   0.37.0): shader program from two strings, `Arc<Mutex<..>>` handed to an
//!   `egui_glow::CallbackFn`, `destroy` on exit.
//! * CAD look, from FreeCAD's Part view provider
//!   (`src/Mod/Part/Gui/ViewProviderExt.cpp`, line ~490: "The correct order
//!   is Edges, Polygon offset, Faces", an `SoPolygonOffset` pushing the faces
//!   back so edges on them win the depth test) and its default colours
//!   (`src/Gui/ViewParams.cpp`: `DefaultShapeColor` 0xCCCCCC, i.e. 0.8 grey,
//!   `DefaultShapeLineColor` black). Edges are drawn as GL_LINES.
//! * Selection colours: FreeCAD's defaults, preselection (hover)
//!   rgb(10, 200, 255) and selection rgb(0, 171, 255)
//!   (`src/Gui/Selection/SelectionColors.h`, `highlightFallbackColor`,
//!   `selectionFallbackColor`).
//! * Orbit/pan/dolly gestures: three.js `OrbitControls` (left drag orbit,
//!   right or shift drag pan, wheel dolly), what the web studio used.
//!
//! Z is up (CAD and STL convention).

use std::sync::Arc;

use eframe::egui::{self, mutex::Mutex, Color32};
use eframe::{egui_glow, glow};
use glam::{Mat4, Vec3, Vec4};

use crate::scene::Scene;

pub const HOVER: Color32 = Color32::from_rgb(10, 200, 255);
pub const SELECT: Color32 = Color32::from_rgb(0, 171, 255);
pub const CLASH: Color32 = Color32::from_rgb(235, 40, 40);

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub target: Vec3,
    /// Azimuth around +Z from +X, radians.
    pub yaw: f32,
    /// Elevation above the XY plane, radians.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
}

impl Camera {
    pub fn fit(scene: &Scene) -> Self {
        let r = scene.radius();
        Camera { target: scene.center(), yaw: -2.2, pitch: 0.55, distance: r * 2.6, fov_y: 35f32.to_radians() }
    }

    pub fn eye(&self) -> Vec3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        self.target + Vec3::new(cp * cy, cp * sy, sp) * self.distance
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let near = (self.distance * 0.01).max(0.1);
        let far = self.distance * 20.0;
        glam::camera::rh::proj::opengl::perspective(self.fov_y, aspect.max(0.01), near, far)
            * glam::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Z)
    }

    /// A ray from the eye through `uv` (0..1 across the view, y down).
    pub fn ray(&self, uv: egui::Vec2, aspect: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(aspect).inverse();
        let a = inv * Vec4::new(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, -1.0, 1.0);
        let b = inv * Vec4::new(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 1.0, 1.0);
        let (a, b) = (a.truncate() / a.w, b.truncate() / b.w);
        (a, (b - a).normalize())
    }

    pub fn orbit(&mut self, d: egui::Vec2) {
        self.yaw -= d.x * 0.008;
        self.pitch = (self.pitch + d.y * 0.008).clamp(-1.55, 1.55);
    }

    pub fn pan(&mut self, d: egui::Vec2, view_height_px: f32) {
        let fwd = (self.target - self.eye()).normalize();
        let right = fwd.cross(Vec3::Z).normalize_or_zero();
        let up = right.cross(fwd);
        let per_px = 2.0 * self.distance * (self.fov_y * 0.5).tan() / view_height_px.max(1.0);
        self.target += (-right * d.x + up * d.y) * per_px;
    }

    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (-scroll * 0.002).exp()).max(1.0);
    }

    /// Standard views.
    pub fn preset(&mut self, name: &str) {
        let (yaw, pitch) = match name {
            "top" => (-std::f32::consts::FRAC_PI_2, 1.5499),
            "front" => (-std::f32::consts::FRAC_PI_2, 0.0),
            "side" => (0.0, 0.0),
            _ => (-2.2, 0.55),
        };
        self.yaw = yaw;
        self.pitch = pitch;
    }
}

const FACE_VS: &str = r#"
in vec3 a_pos;
in vec3 a_normal;
in float a_part;
in vec3 a_dir;
in float a_flag;
uniform mat4 u_mvp;
uniform float u_explode;
out vec3 v_normal;
out vec3 v_world;
out float v_part;
out float v_flag;
void main() {
    vec3 p = a_pos + a_dir * u_explode;
    v_normal = a_normal;
    v_world = p;
    v_part = a_part;
    v_flag = a_flag;
    gl_Position = u_mvp * vec4(p, 1.0);
}
"#;

const FACE_FS: &str = r#"
precision mediump float;
in vec3 v_normal;
in vec3 v_world;
in float v_part;
in float v_flag;
uniform vec3 u_eye;
uniform float u_selected;
uniform float u_hover;
uniform vec4 u_clip;
uniform float u_clip_on;
uniform vec3 u_select_rgb;
uniform vec3 u_hover_rgb;
out vec4 out_color;
vec3 palette(float i) {
    // CAD greys with a faint per-part tint, so neighbours read apart
    float h = fract(i * 0.61803398875);
    vec3 k = vec3(0.0, 0.33, 0.67);
    return vec3(0.76) + 0.05 * cos(6.28318 * (h + k));
}
void main() {
    if (u_clip_on > 0.5 && dot(v_world, u_clip.xyz) > u_clip.w) discard;
    vec3 n = normalize(v_normal);
    vec3 e = normalize(u_eye - v_world);
    bool back = dot(n, e) < 0.0;
    if (back) n = -n;
    vec3 base = palette(v_part);
    if (v_flag > 0.5) base = vec3(0.92, 0.16, 0.16);
    if (abs(v_part - u_selected) < 0.5) base = u_select_rgb;
    else if (abs(v_part - u_hover) < 0.5) base = mix(base, u_hover_rgb, 0.55);
    float diffuse = max(dot(n, e), 0.0);
    vec3 key = normalize(vec3(0.4, 0.3, 1.0));
    vec3 c = base * (0.30 + 0.55 * diffuse + 0.15 * max(dot(n, key), 0.0));
    // a cut face shows its inside: darker, so the section reads
    if (back && u_clip_on > 0.5) c = base * 0.35;
    out_color = vec4(c, 1.0);
}
"#;

const EDGE_VS: &str = r#"
in vec3 a_pos;
in float a_part;
in vec3 a_dir;
uniform mat4 u_mvp;
uniform float u_explode;
out vec3 v_world;
out float v_part;
void main() {
    vec3 p = a_pos + a_dir * u_explode;
    v_world = p;
    v_part = a_part;
    gl_Position = u_mvp * vec4(p, 1.0);
}
"#;

const EDGE_FS: &str = r#"
precision mediump float;
in vec3 v_world;
in float v_part;
uniform vec4 u_clip;
uniform float u_clip_on;
uniform float u_selected;
out vec4 out_color;
void main() {
    if (u_clip_on > 0.5 && dot(v_world, u_clip.xyz) > u_clip.w) discard;
    vec3 c = abs(v_part - u_selected) < 0.5 ? vec3(0.0, 0.25, 0.45) : vec3(0.04, 0.04, 0.05);
    out_color = vec4(c, 1.0);
}
"#;

unsafe fn program(gl: &glow::Context, vs: &str, fs: &str, attribs: &[&str]) -> Option<glow::Program> {
    use glow::HasContext as _;
    let program = gl.create_program().ok()?;
    let mut shaders = vec![];
    for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
        let s = gl.create_shader(kind).ok()?;
        gl.shader_source(s, &format!("#version 330\n{src}"));
        gl.compile_shader(s);
        if !gl.get_shader_compile_status(s) {
            eprintln!("wright-desktop: shader: {}", gl.get_shader_info_log(s));
            return None;
        }
        gl.attach_shader(program, s);
        shaders.push(s);
    }
    for (i, a) in attribs.iter().enumerate() {
        gl.bind_attrib_location(program, i as u32, a);
    }
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        eprintln!("wright-desktop: link: {}", gl.get_program_info_log(program));
        return None;
    }
    for s in shaders {
        gl.detach_shader(program, s);
        gl.delete_shader(s);
    }
    Some(program)
}

/// GPU side of the scene.
pub struct MeshRenderer {
    faces: glow::Program,
    edges: glow::Program,
    face_vao: glow::VertexArray,
    face_vbo: glow::Buffer,
    edge_vao: glow::VertexArray,
    edge_vbo: glow::Buffer,
    face_vertices: i32,
    edge_vertices: i32,
    uploaded: u64,
}

impl MeshRenderer {
    pub fn new(gl: &glow::Context) -> Option<Self> {
        use glow::HasContext as _;
        unsafe {
            let faces = program(gl, FACE_VS, FACE_FS, &["a_pos", "a_normal", "a_part", "a_dir", "a_flag"])?;
            let edges = program(gl, EDGE_VS, EDGE_FS, &["a_pos", "a_part", "a_dir"])?;
            Some(MeshRenderer {
                faces,
                edges,
                face_vao: gl.create_vertex_array().ok()?,
                face_vbo: gl.create_buffer().ok()?,
                edge_vao: gl.create_vertex_array().ok()?,
                edge_vbo: gl.create_buffer().ok()?,
                face_vertices: 0,
                edge_vertices: 0,
                uploaded: u64::MAX,
            })
        }
    }

    unsafe fn layout(gl: &glow::Context, vao: glow::VertexArray, vbo: glow::Buffer, data: &[f32], sizes: &[i32]) {
        use glow::HasContext as _;
        let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_ne_bytes()).collect();
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
        let stride: i32 = sizes.iter().sum::<i32>() * 4;
        let mut off = 0;
        for (i, s) in sizes.iter().enumerate() {
            gl.enable_vertex_attrib_array(i as u32);
            gl.vertex_attrib_pointer_f32(i as u32, *s, glow::FLOAT, false, stride, off);
            off += s * 4;
        }
        gl.bind_vertex_array(None);
    }

    fn upload(&mut self, gl: &glow::Context, scene: &Scene, flags: &[bool], generation: u64) {
        let dirs: Vec<Vec3> = (0..scene.parts.len()).map(|i| scene.explode_offset(i, 1.0)).collect();
        let mut f: Vec<f32> = Vec::with_capacity(scene.positions.len() * 11);
        for i in 0..scene.positions.len() {
            let part = scene.part_of_vertex[i] as usize;
            f.extend_from_slice(&scene.positions[i]);
            f.extend_from_slice(&scene.normals[i]);
            f.push(part as f32);
            f.extend_from_slice(&dirs[part].to_array());
            f.push(if flags.get(part).copied().unwrap_or(false) { 1.0 } else { 0.0 });
        }
        let mut e: Vec<f32> = Vec::with_capacity(scene.edge_positions.len() * 7);
        for i in 0..scene.edge_positions.len() {
            let part = scene.part_of_edge_vertex[i] as usize;
            e.extend_from_slice(&scene.edge_positions[i]);
            e.push(part as f32);
            e.extend_from_slice(&dirs[part].to_array());
        }
        unsafe {
            Self::layout(gl, self.face_vao, self.face_vbo, &f, &[3, 3, 1, 3, 1]);
            Self::layout(gl, self.edge_vao, self.edge_vbo, &e, &[3, 1, 3]);
        }
        self.face_vertices = scene.positions.len() as i32;
        self.edge_vertices = scene.edge_positions.len() as i32;
        self.uploaded = generation;
    }

    pub fn paint(&mut self, gl: &glow::Context, info: &egui::PaintCallbackInfo, frame: &Frame3d) {
        use glow::HasContext as _;
        if self.uploaded != frame.generation {
            let scene = frame.scene.lock();
            self.upload(gl, &scene, &frame.flags, frame.generation);
        }
        let vp = info.viewport_in_pixels();
        let rgb = |c: Color32| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0];
        unsafe {
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px);
            gl.clear_color(0.16, 0.17, 0.20, 1.0);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            if self.face_vertices == 0 {
                return;
            }
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::BLEND);
            let clip = frame.clip.unwrap_or((Vec3::Z, 0.0));
            // faces, pushed back so the edges on them win the depth test
            gl.enable(glow::POLYGON_OFFSET_FILL);
            gl.polygon_offset(1.0, 1.0);
            let p = self.faces;
            gl.use_program(Some(p));
            let u = |n: &str| gl.get_uniform_location(p, n);
            gl.uniform_matrix_4_f32_slice(u("u_mvp").as_ref(), false, &frame.mvp.to_cols_array());
            gl.uniform_3_f32(u("u_eye").as_ref(), frame.eye.x, frame.eye.y, frame.eye.z);
            gl.uniform_1_f32(u("u_selected").as_ref(), frame.selected);
            gl.uniform_1_f32(u("u_hover").as_ref(), frame.hover);
            gl.uniform_1_f32(u("u_explode").as_ref(), frame.explode);
            gl.uniform_4_f32(u("u_clip").as_ref(), clip.0.x, clip.0.y, clip.0.z, clip.1);
            gl.uniform_1_f32(u("u_clip_on").as_ref(), if frame.clip.is_some() { 1.0 } else { 0.0 });
            let [r, g, b] = rgb(SELECT);
            gl.uniform_3_f32(u("u_select_rgb").as_ref(), r, g, b);
            let [r, g, b] = rgb(HOVER);
            gl.uniform_3_f32(u("u_hover_rgb").as_ref(), r, g, b);
            gl.bind_vertex_array(Some(self.face_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, self.face_vertices);
            gl.disable(glow::POLYGON_OFFSET_FILL);
            if frame.show_edges && self.edge_vertices > 0 {
                let p = self.edges;
                gl.use_program(Some(p));
                let u = |n: &str| gl.get_uniform_location(p, n);
                gl.uniform_matrix_4_f32_slice(u("u_mvp").as_ref(), false, &frame.mvp.to_cols_array());
                gl.uniform_1_f32(u("u_explode").as_ref(), frame.explode);
                gl.uniform_1_f32(u("u_selected").as_ref(), frame.selected);
                gl.uniform_4_f32(u("u_clip").as_ref(), clip.0.x, clip.0.y, clip.0.z, clip.1);
                gl.uniform_1_f32(u("u_clip_on").as_ref(), if frame.clip.is_some() { 1.0 } else { 0.0 });
                gl.bind_vertex_array(Some(self.edge_vao));
                gl.draw_arrays(glow::LINES, 0, self.edge_vertices);
            }
            gl.bind_vertex_array(None);
            gl.disable(glow::DEPTH_TEST);
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        use glow::HasContext as _;
        unsafe {
            gl.delete_program(self.faces);
            gl.delete_program(self.edges);
            gl.delete_vertex_array(self.face_vao);
            gl.delete_vertex_array(self.edge_vao);
            gl.delete_buffer(self.face_vbo);
            gl.delete_buffer(self.edge_vbo);
        }
    }
}

/// What one paint needs.
pub struct Frame3d {
    pub scene: Arc<Mutex<Scene>>,
    pub flags: Vec<bool>,
    pub generation: u64,
    pub mvp: Mat4,
    pub eye: Vec3,
    pub selected: f32,
    pub hover: f32,
    pub explode: f32,
    pub clip: Option<(Vec3, f32)>,
    pub show_edges: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Measure,
}

/// A section plane: normal along an axis, at a fraction of the model's
/// extent along it; `flip` keeps the other side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    pub axis: usize,
    pub at: f32,
    pub flip: bool,
}

pub struct Viewer {
    pub camera: Camera,
    pub scene: Arc<Mutex<Scene>>,
    pub generation: u64,
    pub selected: Option<usize>,
    pub hover: Option<usize>,
    /// Parts drawn red (interference).
    pub flags: Vec<bool>,
    pub explode: f32,
    pub section: Option<Section>,
    pub show_edges: bool,
    pub tool: Tool,
    /// Measure: up to two picked surface points.
    pub measure: Vec<Vec3>,
    renderer: Option<Arc<Mutex<MeshRenderer>>>,
}

impl Viewer {
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        Viewer {
            camera: Camera::fit(&Scene::default()),
            scene: Arc::new(Mutex::new(Scene::default())),
            generation: 0,
            selected: None,
            hover: None,
            flags: vec![],
            explode: 0.0,
            section: None,
            show_edges: true,
            tool: Tool::Select,
            measure: vec![],
            renderer: gl.and_then(|gl| MeshRenderer::new(gl)).map(|r| Arc::new(Mutex::new(r))),
        }
    }

    pub fn set_scene(&mut self, scene: Scene) {
        self.camera = Camera::fit(&scene);
        self.flags = vec![false; scene.parts.len()];
        *self.scene.lock() = scene;
        self.generation += 1;
        self.selected = None;
        self.hover = None;
        self.measure.clear();
    }

    pub fn set_flags(&mut self, flags: Vec<bool>) {
        if flags != self.flags {
            self.flags = flags;
            self.generation += 1;
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        if let Some(r) = &self.renderer {
            r.lock().destroy(gl);
        }
    }

    /// The section plane in world terms: keep `n.p <= d`.
    pub fn clip(&self) -> Option<(Vec3, f32)> {
        let s = self.section?;
        let sc = self.scene.lock();
        let mut n = Vec3::ZERO;
        n[s.axis] = 1.0;
        let d = sc.min[s.axis] + (sc.max[s.axis] - sc.min[s.axis]) * s.at;
        Some(if s.flip { (-n, -d) } else { (n, d) })
    }

    /// World point to screen (inside `rect`), if in front of the camera.
    pub fn project(&self, p: Vec3, rect: egui::Rect) -> Option<egui::Pos2> {
        let c = self.camera.view_proj(rect.width() / rect.height().max(1.0)) * p.extend(1.0);
        if c.w <= 0.0 {
            return None;
        }
        let ndc = c.truncate() / c.w;
        Some(egui::pos2(rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(), rect.top() + (1.0 - ndc.y) * 0.5 * rect.height()))
    }

    pub fn show(&mut self, ui: &mut egui::Ui, rect: egui::Rect, interact: bool) -> egui::Response {
        let sense = if interact { egui::Sense::click_and_drag() } else { egui::Sense::hover() };
        let response = ui.allocate_rect(rect, sense);
        let aspect = rect.width() / rect.height().max(1.0);
        if interact {
            let shift = ui.input(|i| i.modifiers.shift);
            if response.dragged_by(egui::PointerButton::Primary) && !shift {
                self.camera.orbit(response.drag_delta());
            }
            if response.dragged_by(egui::PointerButton::Secondary)
                || response.dragged_by(egui::PointerButton::Middle)
                || (response.dragged_by(egui::PointerButton::Primary) && shift)
            {
                self.camera.pan(response.drag_delta(), rect.height());
            }
            if response.hovered() {
                let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
                if scroll != 0.0 {
                    self.camera.zoom(scroll);
                }
                if zoom != 1.0 {
                    self.camera.distance /= zoom;
                }
            }
            self.hover = if response.dragged() { self.hover } else { response.hover_pos().and_then(|p| self.pick(rect, p, aspect)).map(|h| h.0) };
            if response.clicked() {
                let hit = response.interact_pointer_pos().and_then(|p| self.pick(rect, p, aspect));
                match self.tool {
                    Tool::Select => self.selected = hit.map(|h| h.0),
                    Tool::Measure => {
                        if let Some((_, p)) = hit {
                            if self.measure.len() >= 2 {
                                self.measure.clear();
                            }
                            self.measure.push(p);
                        }
                    }
                }
            }
        }
        let Some(renderer) = self.renderer.clone() else {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "no OpenGL context: 3D view unavailable", egui::FontId::proportional(14.0), Color32::GRAY);
            return response;
        };
        let frame = Frame3d {
            scene: self.scene.clone(),
            flags: self.flags.clone(),
            generation: self.generation,
            mvp: self.camera.view_proj(aspect),
            eye: self.camera.eye(),
            selected: self.selected.map_or(-10.0, |s| s as f32),
            hover: self.hover.map_or(-10.0, |s| s as f32),
            explode: self.explode,
            clip: self.clip(),
            show_edges: self.show_edges,
        };
        ui.painter().add(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                renderer.lock().paint(painter.gl(), &info, &frame);
            })),
        });
        response
    }

    fn pick(&self, rect: egui::Rect, p: egui::Pos2, aspect: f32) -> Option<(usize, Vec3)> {
        let uv = (p - rect.min) / rect.size();
        let (o, d) = self.camera.ray(uv, aspect);
        let clip = self.clip();
        self.scene.lock().pick(o, d, self.explode, clip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_centre_ray_points_at_the_target() {
        let s = Scene { min: Vec3::splat(-10.0), max: Vec3::splat(10.0), ..Default::default() };
        let cam = Camera::fit(&s);
        let (o, d) = cam.ray(egui::vec2(0.5, 0.5), 1.5);
        assert!(d.dot((cam.target - o).normalize()) > 0.9999);
    }

    #[test]
    fn the_target_projects_to_the_view_centre() {
        let mut v = Viewer::new(None);
        let s = Scene { min: Vec3::splat(-10.0), max: Vec3::splat(10.0), ..Default::default() };
        v.camera = Camera::fit(&s);
        let r = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(400.0, 300.0));
        let p = v.project(v.camera.target, r).unwrap();
        assert!((p - r.center()).length() < 0.5, "{p:?}");
    }
}
