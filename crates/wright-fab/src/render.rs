//! Headless GPU renderer for the fabrication videos: perspective camera,
//! per-pixel lighting, 4x MSAA, read back to RGBA for ffmpeg or PNG.
//!
//! Prior art, read before writing this: gfx-rs/wgpu
//! `examples/features/src/render_to_texture/mod.rs` (trunk, API = wgpu 30):
//! render into a texture with `RENDER_ATTACHMENT | COPY_SRC`, copy it to a
//! `MAP_READ` staging buffer with `copy_texture_to_buffer`, `map_async`,
//! `device.poll(PollType::wait_indefinitely())`, read the mapped range.
//! Kept: that sequence. Added, each for a stated reason:
//! * a multisampled colour + depth target resolved into the copied texture
//!   (the example draws one triangle with no depth; a part needs both);
//! * vertex buffers re-created per frame: every frame of a material-removal
//!   or print timelapse is a different mesh;
//! * bytes-per-row padded to 256 (`COPY_BYTES_PER_ROW_ALIGNMENT`), which the
//!   example skips because 512x4 happens to be aligned.
//!
//! Why this and not `wright_guide::render` (our z-buffer, tinyrenderer
//! port): that one is orthographic, flat-shaded at 2x2 supersampling on the
//! CPU, made for assembly drawings. A timelapse is hundreds of 720p frames
//! of a moving camera; it needs perspective, smooth lighting and speed.
//! When no GPU adapter exists (a container without Vulkan), [`Renderer::new`]
//! fails and callers fall back to `wright_guide::render` for stills.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use wright_geom::{Mesh, V3};

pub const W: u32 = 1280;
pub const H: u32 = 720;
const SAMPLES: u32 = 4;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    light: [f32; 4],
}

/// sRGB 0-255 to linear RGBA.
pub fn rgb(c: [u8; 3]) -> [f32; 4] {
    let f = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    [f(c[0]), f(c[1]), f(c[2]), 1.0]
}

/// Triangle soup with flat normals (CSG meshes carry no smooth normals).
#[derive(Default, Clone)]
pub struct Scene {
    pub verts: Vec<Vertex>,
}

impl Scene {
    pub fn tri(&mut self, a: V3, b: V3, c: V3, color: [f32; 4]) {
        let n = (b - a).cross(c - a);
        let l = n.norm();
        let n = if l > 0.0 { n * (1.0 / l) } else { V3 { x: 0.0, y: 0.0, z: 1.0 } };
        let nn = [n.x as f32, n.y as f32, n.z as f32];
        for p in [a, b, c] {
            self.verts.push(Vertex { pos: [p.x as f32, p.y as f32, p.z as f32], normal: nn, color });
        }
    }
    /// A triangle with per-vertex normals (smooth surfaces of revolution).
    pub fn tri_n(&mut self, p: [V3; 3], n: [V3; 3], color: [f32; 4]) {
        for i in 0..3 {
            self.verts.push(Vertex {
                pos: [p[i].x as f32, p[i].y as f32, p[i].z as f32],
                normal: [n[i].x as f32, n[i].y as f32, n[i].z as f32],
                color,
            });
        }
    }
    pub fn quad(&mut self, a: V3, b: V3, c: V3, d: V3, color: [f32; 4]) {
        self.tri(a, b, c, color);
        self.tri(a, c, d, color);
    }
    pub fn mesh(&mut self, m: &Mesh, color: [f32; 4]) {
        for t in m.triangles() {
            self.tri(t[0], t[1], t[2], color);
        }
    }
    pub fn mesh_at(&mut self, m: &Mesh, offset: V3, color: [f32; 4]) {
        for t in m.triangles() {
            self.tri(t[0] + offset, t[1] + offset, t[2] + offset, color);
        }
    }
    /// An axis-aligned box.
    pub fn cuboid(&mut self, min: V3, max: V3, color: [f32; 4]) {
        let c = |i: u8| V3 {
            x: if i & 1 == 0 { min.x } else { max.x },
            y: if i & 2 == 0 { min.y } else { max.y },
            z: if i & 4 == 0 { min.z } else { max.z },
        };
        for f in [[0u8, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            self.quad(c(f[0]), c(f[1]), c(f[2]), c(f[3]), color);
        }
    }
    /// A cylinder between two points (tools, spindles, drills, tubes).
    pub fn cylinder(&mut self, p0: V3, p1: V3, r: f64, n: usize, color: [f32; 4]) {
        let axis = (p1 - p0).unit();
        let helper = if axis.z.abs() < 0.9 { V3 { x: 0.0, y: 0.0, z: 1.0 } } else { V3 { x: 1.0, y: 0.0, z: 0.0 } };
        let u = axis.cross(helper).unit();
        let v = axis.cross(u);
        let ring = |i: usize| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            u * (r * a.cos()) + v * (r * a.sin())
        };
        for i in 0..n {
            let (a, b) = (ring(i), ring(i + 1));
            self.tri_n([p0 + a, p0 + b, p1 + b], [a, b, b], color);
            self.tri_n([p0 + a, p1 + b, p1 + a], [a, b, a], color);
            self.tri(p0, p0 + b, p0 + a, color);
            self.tri(p1, p1 + a, p1 + b, color);
        }
    }
    pub fn extend(&mut self, o: &Scene) {
        self.verts.extend_from_slice(&o.verts);
    }
    pub fn bounds(&self) -> Option<(V3, V3)> {
        let mut it = self.verts.iter();
        let f = it.next()?;
        let p = |v: &Vertex| V3 { x: v.pos[0] as f64, y: v.pos[1] as f64, z: v.pos[2] as f64 };
        let (mut lo, mut hi) = (p(f), p(f));
        for v in it {
            lo = lo.min(p(v));
            hi = hi.max(p(v));
        }
        Some((lo, hi))
    }
}

/// A perspective camera looking at `target` from `eye`, z up.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: V3,
    pub target: V3,
    pub fov_y_deg: f64,
}

impl Camera {
    /// Orbit `target` at `dist`, azimuth/elevation in degrees.
    pub fn orbit(target: V3, dist: f64, az_deg: f64, el_deg: f64) -> Self {
        let (az, el) = (az_deg.to_radians(), el_deg.to_radians());
        let eye = target + V3 { x: dist * el.cos() * az.cos(), y: dist * el.cos() * az.sin(), z: dist * el.sin() };
        Camera { eye, target, fov_y_deg: 30.0 }
    }
    /// Distance at which a sphere of `radius` fills the frame.
    pub fn fit_distance(radius: f64, fov_y_deg: f64) -> f64 {
        // the vertical field is the narrower one at 16:9
        radius / (fov_y_deg.to_radians() / 2.0).sin() * 1.05
    }
    fn view_proj(&self, near: f64, far: f64) -> [[f32; 4]; 4] {
        let f = (self.target - self.eye).unit();
        let up = if f.z.abs() > 0.999 { V3 { x: 0.0, y: 1.0, z: 0.0 } } else { V3 { x: 0.0, y: 0.0, z: 1.0 } };
        let s = f.cross(up).unit();
        let u = s.cross(f);
        let e = self.eye;
        // view (right-handed, camera looks down -z)
        let view = [
            [s.x, u.x, -f.x, 0.0],
            [s.y, u.y, -f.y, 0.0],
            [s.z, u.z, -f.z, 0.0],
            [-s.dot(e), -u.dot(e), f.dot(e), 1.0],
        ];
        // wgpu clip space: z in [0, 1]
        let aspect = W as f64 / H as f64;
        let t = 1.0 / (self.fov_y_deg.to_radians() / 2.0).tan();
        let proj = [
            [t / aspect, 0.0, 0.0, 0.0],
            [0.0, t, 0.0, 0.0],
            [0.0, 0.0, far / (near - far), -1.0],
            [0.0, 0.0, near * far / (near - far), 0.0],
        ];
        let mut m = [[0f32; 4]; 4];
        for c in 0..4 {
            for r in 0..4 {
                let mut acc = 0.0;
                for k in 0..4 {
                    acc += proj[k][r] * view[c][k];
                }
                m[c][r] = acc as f32;
            }
        }
        m
    }
}

pub struct GpuMesh {
    buf: wgpu::Buffer,
    pub verts: u32,
    lo: V3,
    hi: V3,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolve: wgpu::Texture,
    staging: wgpu::Buffer,
    pub background: [f64; 3],
}

const SHADER: &str = r#"
struct U { view_proj: mat4x4<f32>, eye: vec4<f32>, light: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) world: vec3<f32>, @location(1) n: vec3<f32>, @location(2) color: vec4<f32> };
@vertex fn vs_main(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>) -> VsOut {
  var o: VsOut;
  o.pos = u.view_proj * vec4<f32>(p, 1.0);
  o.world = p; o.n = n; o.color = c;
  return o;
}
@fragment fn fs_main(i: VsOut, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {
  var n = normalize(i.n);
  let v = normalize(u.eye.xyz - i.world);
  if (dot(n, v) < 0.0) { n = -n; }
  let l = normalize(u.light.xyz);
  let l2 = normalize(vec3<f32>(-l.x, -l.y, 0.4));
  let sky = vec3<f32>(0.62, 0.68, 0.78);
  let ground = vec3<f32>(0.22, 0.20, 0.18);
  let hemi = mix(ground, sky, 0.5 + 0.5 * n.z);
  let diff = max(dot(n, l), 0.0);
  let fill = max(dot(n, l2), 0.0);
  let h = normalize(l + v);
  let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.35 * i.color.a;
  let rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * 0.12;
  let base = i.color.rgb;
  let c = base * (0.38 * hemi + 0.85 * diff + 0.25 * fill) + vec3<f32>(spec + rim);
  return vec4<f32>(c, 1.0);
}
"#;

impl Renderer {
    pub fn new() -> anyhow::Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| anyhow::anyhow!("no GPU adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits { max_buffer_size: 1 << 30, ..wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()) },
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bind_layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        let tex = |format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let msaa = tex(FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let depth = tex(wgpu::TextureFormat::Depth32Float, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let resolve = tex(FORMAT, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (padded_row() * H) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Renderer { device, queue, pipeline, bind_layout, msaa, depth, resolve, staging, background: [0.010, 0.012, 0.016] })
    }

    /// Upload a scene once (a print's every extrusion) to draw prefixes of
    /// it frame after frame without sending it again.
    pub fn upload(&self, scene: &Scene) -> Option<GpuMesh> {
        let (lo, hi) = scene.bounds()?;
        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&scene.verts), usage: wgpu::BufferUsages::VERTEX });
        Some(GpuMesh { buf, verts: scene.verts.len() as u32, lo, hi })
    }

    /// Render `scene` and return tightly packed RGBA8 (sRGB), W x H.
    pub fn draw(&self, scene: &Scene, cam: &Camera) -> Vec<u8> {
        self.draw_with(&[], scene, cam)
    }

    /// Render the first `n` vertices of each uploaded mesh plus `scene`.
    pub fn draw_with(&self, statics: &[(&GpuMesh, u32)], scene: &Scene, cam: &Camera) -> Vec<u8> {
        let mut b = scene.bounds();
        for (m, _) in statics {
            b = Some(match b {
                Some((lo, hi)) => (lo.min(m.lo), hi.max(m.hi)),
                None => (m.lo, m.hi),
            });
        }
        let (near, far) = match b {
            Some((lo, hi)) => {
                let c = (lo + hi) * 0.5;
                let r = (hi - lo).norm() * 0.5 + 1.0;
                let d = (c - cam.eye).norm();
                (((d - r) * 0.5).max(r * 0.002).max(0.5), d + r * 1.5)
            }
            None => (1.0, 1000.0),
        };
        let u = Uniforms {
            view_proj: cam.view_proj(near, far),
            eye: [cam.eye.x as f32, cam.eye.y as f32, cam.eye.z as f32, 1.0],
            light: {
                // key light from over the camera's right shoulder
                let f = (cam.target - cam.eye).unit();
                let s = f.cross(V3 { x: 0.0, y: 0.0, z: 1.0 }).unit();
                let l = (s * 0.6 - f * 0.6 + V3 { x: 0.0, y: 0.0, z: 1.0 }).unit();
                [l.x as f32, l.y as f32, l.z as f32, 0.0]
            },
        };
        let ubuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bind_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let vbuf = (!scene.verts.is_empty()).then(|| {
            self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&scene.verts), usage: wgpu::BufferUsages::VERTEX })
        });
        let resolve_view = self.resolve.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let bg = self.background;
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa,
                    depth_slice: None,
                    resolve_target: Some(&resolve_view),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0], g: bg[1], b: bg[2], a: 1.0 }), store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            for (m, n) in statics {
                let n = (*n).min(m.verts);
                if n > 0 {
                    pass.set_vertex_buffer(0, m.buf.slice(..));
                    pass.draw(0..n, 0..1);
                }
            }
            if let Some(vb) = &vbuf {
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(0..scene.verts.len() as u32, 0..1);
            }
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.resolve, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row()), rows_per_image: Some(H) },
            },
            wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));
        let slice = self.staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let _ = rx.recv();
        let mut out = Vec::with_capacity((W * H * 4) as usize);
        {
            let view = slice.get_mapped_range().expect("mapped");
            for row in 0..H as usize {
                let s = row * padded_row() as usize;
                out.extend_from_slice(&view[s..s + (W * 4) as usize]);
            }
        }
        self.staging.unmap();
        out
    }
}

fn padded_row() -> u32 {
    let a = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    (W * 4).div_ceil(a) * a
}

/// RGBA -> PNG bytes (via wright-guide's encoder, RGB).
pub fn png(rgba: &[u8]) -> Vec<u8> {
    let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    wright_guide::render::encode_png(W, H, &rgb)
}
