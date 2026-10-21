//! wgpu renderer (feature `gpu`).
//!
//! Deliberately simple: one shader; opaque pipelines (one per city decal layer, differing only
//! in depth bias), then blended glass; a 4×-MSAA colour target with a reversed-Z float depth
//! buffer. Each car body has its own vertex buffer, rewritten only when the body deformed;
//! wheel meshes are shared per car id; every draw's model matrix lives in one uniform buffer
//! addressed with dynamic offsets. Ground, barriers and traffic are one mesh each; the city is
//! one mesh per decal layer, uploaded once.

use std::collections::BTreeMap;
use std::path::Path;

use glam::{Mat4, Vec3};
/// The wgpu version this renderer is built against (apps create surfaces with it).
pub use wgpu;
use wgpu::util::DeviceExt;
use wp_contracts::EntityId;

use crate::camera::CameraView;
use crate::city::DECAL_LAYERS;
use crate::lighting::MAX_SPOT_LIGHTS;
use crate::scene::Scene;
use crate::skin::StaticMesh;
use crate::{Shade, Vertex};

/// Depth buffer format.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Bytes between per-draw uniform slots (the WebGPU minimum dynamic-offset alignment).
const OBJECT_STRIDE: u64 = 256;
/// Half size of the ground plane, m (fog hides its edge).
const GROUND_HALF_M: f32 = 2000.0;
/// Ground plane height under a city, m: below its zone ground (-0.05 m) and roads (0 m), as in
/// the browser. Without a city the ground is the z = 0 driving plane.
const CITY_GROUND_Z: f32 = -0.2;

#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct GlobalsRaw {
    view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky_color: [f32; 4],
    ground_color: [f32; 4],
    fog: [f32; 4],
    /// x: night factor, y: number of spot lights used.
    night: [f32; 4],
    lights: [SpotRaw; MAX_SPOT_LIGHTS],
}

/// One spot light as the shader reads it.
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct SpotRaw {
    /// xyz position, w range (m).
    pos_range: [f32; 4],
    /// xyz direction, w cosine of the outer cone.
    dir_outer: [f32; 4],
    /// rgb colour, w cosine of the inner cone.
    color_inner: [f32; 4],
}

/// A mesh on the GPU.
struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

impl GpuMesh {
    fn new(device: &wgpu::Device, label: &str, vertices: &[Vertex], indices: &[u32]) -> Self {
        Self {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(vertices),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            }),
            index_count: indices.len() as u32,
        }
    }

    /// Replace the contents, reusing the buffers when the new data fits.
    fn rewrite(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[Vertex],
        indices: &[u32],
    ) {
        let vb: &[u8] = bytemuck::cast_slice(vertices);
        let ib: &[u8] = bytemuck::cast_slice(indices);
        if vb.len() as u64 > self.vertices.size() || ib.len() as u64 > self.indices.size() {
            // Grow with headroom so a slowly growing mesh is not reallocated every frame.
            let mut v = vertices.to_vec();
            v.resize(vertices.len() * 2, Vertex::default());
            let mut i = indices.to_vec();
            i.resize(indices.len() * 2, 0);
            *self = Self::new(device, "dynamic", &v, &i);
        } else {
            queue.write_buffer(&self.vertices, 0, vb);
            queue.write_buffer(&self.indices, 0, ib);
        }
        self.index_count = indices.len() as u32;
    }
}

/// A car body on the GPU: one vertex buffer, separate opaque and glass index buffers.
struct GpuCar {
    revision: u64,
    opaque: GpuMesh,
    glass: Option<wgpu::Buffer>,
    glass_count: u32,
}

/// Size-dependent render targets.
struct Targets {
    size: (u32, u32),
    depth: wgpu::TextureView,
    msaa: Option<wgpu::TextureView>,
}

/// What one draw call uses.
enum DrawMesh {
    Ground,
    City(usize),
    Boxes,
    Traffic,
    CarOpaque(EntityId),
    CarGlass(EntityId),
    Wheel(String, usize),
}

/// Draws a [`Scene`] (lit by [`Scene::lighting`] and its nearest headlights) into any colour
/// target of the format given at creation.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    sample_count: u32,
    /// Opaque pipelines by city decal layer (layer `l` is pulled `l` steps toward the camera).
    opaque: Vec<wgpu::RenderPipeline>,
    glass: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    object_layout: wgpu::BindGroupLayout,
    objects: wgpu::Buffer,
    objects_group: wgpu::BindGroup,
    object_slots: u64,
    ground: GpuMesh,
    boxes: Option<(u64, GpuMesh)>,
    city: Option<(u64, Vec<Option<GpuMesh>>)>,
    traffic: Option<GpuMesh>,
    cars: BTreeMap<EntityId, GpuCar>,
    wheels: BTreeMap<String, Vec<Option<GpuMesh>>>,
    targets: Option<Targets>,
}

impl Renderer {
    /// Create a renderer drawing into `format` targets with `sample_count` MSAA (1 or 4).
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wp_render shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniform_layout = |dynamic: bool, size: u64| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: dynamic,
                        min_binding_size: wgpu::BufferSize::new(size),
                    },
                    count: None,
                }],
            })
        };
        let globals_layout = uniform_layout(false, std::mem::size_of::<GlobalsRaw>() as u64);
        let object_layout = uniform_layout(true, 64);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wp_render layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&object_layout)],
            ..Default::default()
        });
        let vertex_layout = Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3,
                1 => Float32x4,
                2 => Float32,
                3 => Float32x3
            ],
        });
        let pipeline = |label: &str,
                        blend: wgpu::BlendState,
                        depth_write: bool,
                        bias: wgpu::DepthBiasState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: std::slice::from_ref(&vertex_layout),
                },
                primitive: wgpu::PrimitiveState {
                    // Mesh parts are single-sided shells seen from both sides (open doors).
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    // Reversed Z (`CameraView::view_proj`): nearer is greater.
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // Same offsets as the browser's polygon offset per layer, positive for reversed Z.
        let opaque = (0..DECAL_LAYERS)
            .map(|l| {
                let bias = wgpu::DepthBiasState {
                    constant: 4 * l as i32,
                    slope_scale: l as f32,
                    clamp: 0.0,
                };
                pipeline("wp_render opaque", wgpu::BlendState::REPLACE, true, bias)
            })
            .collect();
        let glass = pipeline(
            "wp_render glass",
            wgpu::BlendState::ALPHA_BLENDING,
            false,
            Default::default(),
        );

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("wp_render globals"),
            size: std::mem::size_of::<GlobalsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wp_render globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let (objects, objects_group) = object_buffer(device, &object_layout, 64);

        let ground_color = [0.11, 0.115, 0.12, 1.0];
        let shade = Shade::Ground as u32 as f32;
        let h = GROUND_HALF_M;
        let ground_vertices: Vec<Vertex> = [[-h, -h], [h, -h], [h, h], [-h, h]]
            .iter()
            .map(|&[x, y]| Vertex {
                pos: [x, y, 0.0],
                color: ground_color,
                shade,
                normal: [0.0; 3],
            })
            .collect();
        let ground = GpuMesh::new(device, "ground", &ground_vertices, &[0, 1, 2, 0, 2, 3]);

        Self {
            device: device.clone(),
            queue: queue.clone(),
            format,
            sample_count,
            opaque,
            glass,
            globals,
            globals_group,
            object_layout,
            objects,
            objects_group,
            object_slots: 64,
            ground,
            boxes: None,
            city: None,
            traffic: None,
            cars: BTreeMap::new(),
            wheels: BTreeMap::new(),
            targets: None,
        }
    }

    /// Colour format this renderer was built for.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Draw `scene` from `camera` into `target` (`size` in pixels, matching the target).
    pub fn render(
        &mut self,
        scene: &Scene,
        camera: &CameraView,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let size = (size.0.max(1), size.1.max(1));
        self.ensure_targets(size);
        self.upload_scene(scene);

        let l = *scene.lighting();
        let mut lights = [SpotRaw::default(); MAX_SPOT_LIGHTS];
        let near = scene.lights_near(camera.eye);
        for (raw, s) in lights.iter_mut().zip(&near) {
            *raw = SpotRaw {
                pos_range: [s.pos[0], s.pos[1], s.pos[2], s.range_m],
                dir_outer: [s.dir[0], s.dir[1], s.dir[2], s.cos_outer],
                color_inner: [s.color[0], s.color[1], s.color[2], s.cos_inner],
            };
        }
        let sun = Vec3::from(l.sun_dir).normalize_or(Vec3::Z);
        let globals = GlobalsRaw {
            view_proj: camera
                .view_proj(size.0 as f32 / size.1 as f32)
                .to_cols_array_2d(),
            camera_pos: camera.eye.extend(1.0).to_array(),
            sun_dir: sun.extend(0.0).to_array(),
            sun_color: rgb4(l.sun_color),
            sky_color: rgb4(l.sky_color),
            ground_color: rgb4(l.ground_color),
            fog: [
                l.fog_color[0],
                l.fog_color[1],
                l.fog_color[2],
                l.fog_density,
            ],
            night: [l.night, near.len() as f32, 0.0, 0.0],
            lights,
        };
        self.queue
            .write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        // Draw list: opaque first, glass last (blended over everything opaque).
        let mut draws: Vec<(DrawMesh, Mat4)> = Vec::new();
        // The ground follows the camera in 10 m steps so it never runs out.
        let snap = (camera.eye.truncate() / 10.0).round() * 10.0;
        let ground_z = if scene.city().is_some() {
            CITY_GROUND_Z
        } else {
            0.0
        };
        draws.push((
            DrawMesh::Ground,
            Mat4::from_translation(snap.extend(ground_z)),
        ));
        if let Some((_, layers)) = &self.city {
            for (l, m) in layers.iter().enumerate() {
                if m.is_some() {
                    draws.push((DrawMesh::City(l), Mat4::IDENTITY));
                }
            }
        }
        if self.boxes.as_ref().is_some_and(|(_, m)| m.index_count > 0) {
            draws.push((DrawMesh::Boxes, Mat4::IDENTITY));
        }
        if self.traffic.as_ref().is_some_and(|m| m.index_count > 0) {
            draws.push((DrawMesh::Traffic, Mat4::IDENTITY));
        }
        for car in scene.cars() {
            draws.push((DrawMesh::CarOpaque(car.entity), car.to_world));
            if self.wheels.contains_key(&car.car_id) {
                for (i, w) in car.wheels.iter().enumerate() {
                    if w.visible {
                        draws.push((
                            DrawMesh::Wheel(car.car_id.clone(), i),
                            car.to_world * w.to_chassis,
                        ));
                    }
                }
            }
        }
        for car in scene.cars() {
            draws.push((DrawMesh::CarGlass(car.entity), car.to_world));
        }

        self.ensure_object_slots(draws.len() as u64);
        let mut slots = vec![0u8; draws.len() * OBJECT_STRIDE as usize];
        for (i, (_, m)) in draws.iter().enumerate() {
            let at = i * OBJECT_STRIDE as usize;
            slots[at..at + 64].copy_from_slice(bytemuck::cast_slice(&m.to_cols_array()));
        }
        self.queue.write_buffer(&self.objects, 0, &slots);

        let targets = self.targets.as_ref().expect("ensured above");
        let (view, resolve) = match &targets.msaa {
            Some(msaa) => (msaa, Some(target)),
            None => (target, None),
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wp_render frame"),
            });
        {
            let fog = l.fog_color;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wp_render main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: fog[0] as f64,
                            g: fog[1] as f64,
                            b: fog[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        // Reversed Z: the far plane is 0.
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            // Index into `opaque`, or `DECAL_LAYERS` for glass.
            let mut bound: Option<usize> = None;
            for (i, (mesh, _)) in draws.iter().enumerate() {
                let (vertices, indices, count) = match mesh {
                    DrawMesh::Ground => (
                        &self.ground.vertices,
                        &self.ground.indices,
                        self.ground.index_count,
                    ),
                    DrawMesh::City(l) => {
                        let (_, layers) = self.city.as_ref().expect("checked when listed");
                        let m = layers[*l].as_ref().expect("checked when listed");
                        (&m.vertices, &m.indices, m.index_count)
                    }
                    DrawMesh::Boxes => {
                        let (_, m) = self.boxes.as_ref().expect("checked when listed");
                        (&m.vertices, &m.indices, m.index_count)
                    }
                    DrawMesh::Traffic => {
                        let m = self.traffic.as_ref().expect("checked when listed");
                        (&m.vertices, &m.indices, m.index_count)
                    }
                    DrawMesh::CarOpaque(id) => {
                        let c = &self.cars[id];
                        (&c.opaque.vertices, &c.opaque.indices, c.opaque.index_count)
                    }
                    DrawMesh::CarGlass(id) => {
                        let c = &self.cars[id];
                        let Some(glass) = &c.glass else { continue };
                        (&c.opaque.vertices, glass, c.glass_count)
                    }
                    DrawMesh::Wheel(car_id, w) => {
                        let Some(m) = self.wheels[car_id][*w].as_ref() else {
                            continue;
                        };
                        (&m.vertices, &m.indices, m.index_count)
                    }
                };
                if count == 0 {
                    continue;
                }
                let want = match mesh {
                    DrawMesh::CarGlass(_) => DECAL_LAYERS,
                    DrawMesh::City(l) => *l,
                    _ => 0,
                };
                if bound != Some(want) {
                    bound = Some(want);
                    pass.set_pipeline(self.opaque.get(want).unwrap_or(&self.glass));
                }
                pass.set_bind_group(1, &self.objects_group, &[(i as u64 * OBJECT_STRIDE) as u32]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..count, 0, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
    }

    fn ensure_targets(&mut self, size: (u32, u32)) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let texture = |label: &str, format: wgpu::TextureFormat| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: self.sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        self.targets = Some(Targets {
            size,
            depth: texture("wp_render depth", DEPTH_FORMAT),
            msaa: (self.sample_count > 1).then(|| texture("wp_render msaa", self.format)),
        });
    }

    fn ensure_object_slots(&mut self, needed: u64) {
        if needed <= self.object_slots {
            return;
        }
        let slots = needed.next_power_of_two();
        let (buffer, group) = object_buffer(&self.device, &self.object_layout, slots);
        self.objects = buffer;
        self.objects_group = group;
        self.object_slots = slots;
    }

    /// Create, update or drop GPU copies of the scene's meshes.
    fn upload_scene(&mut self, scene: &Scene) {
        let city_rev = scene.city_revision();
        if self.city.as_ref().map(|(r, _)| *r) != Some(city_rev) {
            let layers = scene
                .city()
                .map(|c| {
                    c.layers
                        .iter()
                        .map(|m| gpu_static(&self.device, m))
                        .collect()
                })
                .unwrap_or_default();
            self.city = Some((city_rev, layers));
        }
        let t = scene.traffic_mesh();
        match &mut self.traffic {
            Some(m) => m.rewrite(&self.device, &self.queue, &t.vertices, &t.indices),
            None if !t.indices.is_empty() => {
                self.traffic = Some(GpuMesh::new(
                    &self.device,
                    "traffic",
                    &t.vertices,
                    &t.indices,
                ));
            }
            None => {}
        }
        let rev = scene.static_revision();
        if self.boxes.as_ref().map(|(r, _)| *r) != Some(rev) {
            let m = scene.static_mesh();
            self.boxes = Some((
                rev,
                GpuMesh::new(&self.device, "boxes", &m.vertices, &m.indices),
            ));
        }
        self.cars.retain(|id, _| scene.car(*id).is_some());
        for car in scene.cars() {
            match self.cars.get_mut(&car.entity) {
                Some(gpu) if gpu.revision == car.body_revision => {}
                Some(gpu) => {
                    self.queue.write_buffer(
                        &gpu.opaque.vertices,
                        0,
                        bytemuck::cast_slice(car.body.vertices()),
                    );
                    gpu.revision = car.body_revision;
                }
                None => {
                    let body = &car.body;
                    let glass = (!body.glass_indices().is_empty()).then(|| {
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("car glass"),
                                contents: bytemuck::cast_slice(body.glass_indices()),
                                usage: wgpu::BufferUsages::INDEX,
                            })
                    });
                    self.cars.insert(
                        car.entity,
                        GpuCar {
                            revision: car.body_revision,
                            opaque: GpuMesh::new(
                                &self.device,
                                "car body",
                                body.vertices(),
                                body.opaque_indices(),
                            ),
                            glass,
                            glass_count: body.glass_indices().len() as u32,
                        },
                    );
                }
            }
            if !self.wheels.contains_key(&car.car_id) {
                if let Some(meshes) = scene.wheel_meshes(&car.car_id) {
                    let gpu = meshes.iter().map(|m| gpu_static(&self.device, m)).collect();
                    self.wheels.insert(car.car_id.clone(), gpu);
                }
            }
        }
    }
}

fn gpu_static(device: &wgpu::Device, m: &StaticMesh) -> Option<GpuMesh> {
    (!m.indices.is_empty()).then(|| GpuMesh::new(device, "static", &m.vertices, &m.indices))
}

fn rgb4(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

fn object_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    slots: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wp_render objects"),
        size: slots * OBJECT_STRIDE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("wp_render objects"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buffer,
                offset: 0,
                size: wgpu::BufferSize::new(64),
            }),
        }],
    });
    (buffer, group)
}

/// Headless rendering into a texture that is read back to the CPU (screenshots, CI images).
pub struct Offscreen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture: wgpu::Texture,
    readback: wgpu::Buffer,
    size: (u32, u32),
    padded_row: u32,
    /// The renderer drawing into the offscreen texture.
    pub renderer: Renderer,
}

impl Offscreen {
    /// Open any adapter (a software one such as lavapipe is fine) and allocate a
    /// `width × height` RGBA8 sRGB target.
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| format!("no GPU device: {e}"))?;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen readback"),
            size: padded_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let renderer = Renderer::new(&device, &queue, format, 4);
        Ok(Self {
            device,
            queue,
            texture,
            readback,
            size: (width, height),
            padded_row,
            renderer,
        })
    }

    /// Render and return tightly packed RGBA8 (sRGB) pixels, top row first.
    pub fn render(&mut self, scene: &Scene, camera: &CameraView) -> Result<Vec<u8>, String> {
        let view = self.texture.create_view(&Default::default());
        self.renderer.render(scene, camera, &view, self.size);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.size.1),
                },
            },
            wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| format!("GPU poll: {e}"))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| format!("GPU readback: {e}"))?;
        let row = self.size.0 as usize * 4;
        let mut out = Vec::with_capacity(row * self.size.1 as usize);
        for y in 0..self.size.1 as usize {
            let at = y * self.padded_row as usize;
            out.extend_from_slice(&data[at..at + row]);
        }
        drop(data);
        self.readback.unmap();
        Ok(out)
    }

    /// Target size in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
}

/// Write RGBA8 sRGB pixels as a PNG.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(rgba).map_err(|e| e.to_string())
}
