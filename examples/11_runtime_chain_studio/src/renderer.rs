#![forbid(unsafe_code)]

//! Sleek 3D Kinetic Studio Renderer (`wgpu` + `winit`) with instanced faceted geometric actors
//! (crystals, cubes, monolith prisms), glowing studio floor grid, orbital camera, and a crisp
//! HUD showing live Runtime Chain wiring and ECS-only Mod Performance Telemetry.

use crate::components::Formation;
use crate::engine::{ActorRenderData, EcsPerfSummary};
use bamabi_host_sys::schedule::ScheduleSetMode;
use bytemuck::{Pod, Zeroable};
use glam::{EulerRot, Mat4, Quat, Vec3};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MAX_INSTANCES: usize = 34_000;
const MAX_UI_VERTICES: usize = 96_000;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct MeshVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ModelInstance {
    model_col0: [f32; 4],
    model_col1: [f32; 4],
    model_col2: [f32; 4],
    model_col3: [f32; 4],
    color: [f32; 4],
    emissive: f32,
    _pad: [f32; 3],
}

impl ModelInstance {
    fn from_mat4(m: Mat4, color: [f32; 4], emissive: f32) -> Self {
        let cols = m.to_cols_array_2d();
        Self {
            model_col0: cols[0],
            model_col1: cols[1],
            model_col2: cols[2],
            model_col3: cols[3],
            color,
            emissive,
            _pad: [0.0; 3],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct UiVertex {
    pos_ndc: [f32; 2],
    color: [f32; 4],
}

/// Interactive 3D orbital camera.
pub struct OrbitCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
    pub auto_orbit: bool,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            yaw: 0.62,
            pitch: 0.46,
            distance: 21.5,
            target: Vec3::new(0.0, 2.0, 0.0),
            auto_orbit: true,
        }
    }
}

impl OrbitCamera {
    pub fn eye(&self) -> Vec3 {
        let cp = self.pitch.cos();
        let sp = self.pitch.sin();
        let sy = self.yaw.sin();
        let cy = self.yaw.cos();
        self.target
            + Vec3::new(
                self.distance * cp * sy,
                self.distance * sp,
                self.distance * cp * cy,
            )
    }

    pub fn cycle_preset(&mut self, idx: u8) {
        match idx % 3 {
            0 => {
                self.yaw = 0.62;
                self.pitch = 0.46;
                self.distance = 21.5;
                self.target = Vec3::new(0.0, 2.0, 0.0);
            }
            1 => {
                self.yaw = 0.0;
                self.pitch = 1.18;
                self.distance = 24.0;
                self.target = Vec3::new(0.0, 1.0, 0.0);
            }
            _ => {
                self.yaw = 1.25;
                self.pitch = 0.22;
                self.distance = 17.5;
                self.target = Vec3::new(0.0, 2.6, 0.0);
            }
        }
    }
}

/// HUD metadata passed each frame to the renderer.
pub struct HudState<'a> {
    pub paused: bool,
    pub formation: Formation,
    pub chain_mode: ScheduleSetMode,
    pub chain_names: &'a [String],
    pub loaded_mod_count: usize,
    pub discovered_sys_count: usize,
    pub run_count: u32,
    pub speed_scale: f32,
    pub amplitude: f32,
    pub solo_chain: bool,
    pub latest_event: Option<&'a str>,
    pub perf: &'a EcsPerfSummary,
}

struct MeshBuffers {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    index_count: u32,
}

pub struct StudioRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    pipeline_3d: wgpu::RenderPipeline,
    pipeline_ui: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    mesh_crystal: MeshBuffers,
    mesh_cube: MeshBuffers,
    mesh_prism: MeshBuffers,
    instance_buffer: wgpu::Buffer,
    ui_vertex_buffer: wgpu::Buffer,
    static_floor: Vec<ModelInstance>,
}

const SHADER_3D: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct InstanceInput {
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(7) emissive: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) emissive: f32,
};

@vertex
fn vs_main(v: VertexInput, inst: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let world_pos4 = model * vec4<f32>(v.position, 1.0);

    let c0 = inst.m0.xyz;
    let c1 = inst.m1.xyz;
    let c2 = inst.m2.xyz;
    let n = vec3<f32>(
        v.normal.x / max(dot(c0, c0), 0.0001),
        v.normal.y / max(dot(c1, c1), 0.0001),
        v.normal.z / max(dot(c2, c2), 0.0001),
    );
    let world_n = normalize(c0 * n.x + c1 * n.y + c2 * n.z);

    var out: VertexOutput;
    out.clip_position = camera.view_proj * world_pos4;
    out.world_pos = world_pos4.xyz;
    out.world_normal = world_n;
    out.color = inst.color;
    out.emissive = inst.emissive;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let v = normalize(camera.camera_pos.xyz - in.world_pos);

    let key_dir = normalize(vec3<f32>(0.55, 0.92, 0.48));
    let fill_dir = normalize(vec3<f32>(-0.70, 0.35, -0.62));

    let diff_key = max(dot(n, key_dir), 0.0);
    let diff_fill = max(dot(n, fill_dir), 0.0) * 0.38;
    let hemi = mix(vec3<f32>(0.10, 0.13, 0.22), vec3<f32>(0.34, 0.40, 0.54), n.y * 0.5 + 0.5);

    let half_v = normalize(key_dir + v);
    let spec = pow(max(dot(n, half_v), 0.0), 36.0) * 0.42;
    let fresnel = pow(1.0 - max(dot(n, v), 0.0), 2.6) * 0.32;

    let lit = in.color.rgb * (hemi + vec3<f32>(1.0, 0.97, 0.94) * diff_key * 0.95 + vec3<f32>(0.42, 0.72, 1.0) * diff_fill);
    let glow = in.color.rgb * (in.emissive * 0.42);
    let final_rgb = lit + glow + vec3<f32>(spec) + in.color.rgb * fresnel * (0.6 + in.emissive * 0.4);

    // Subtle distance studio fog
    let dist = length(camera.camera_pos.xyz - in.world_pos);
    let fog_t = clamp((dist - 28.0) / 48.0, 0.0, 0.85);
    let fog_col = vec3<f32>(0.045, 0.055, 0.090);
    let rgb = mix(final_rgb, fog_col, fog_t);
    return vec4<f32>(min(rgb, vec3<f32>(1.0)), in.color.a);
}
"#;

const SHADER_UI: &str = r#"
struct UiIn {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct UiOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_ui(in: UiIn) -> UiOut {
    var out: UiOut;
    out.clip_pos = vec4<f32>(in.pos, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_ui(in: UiOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

impl StudioRenderer {
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|e| format!("create_surface failed: {e}"))?;

        let adapter =
            futures_executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .map_err(|e| format!("no suitable GPU adapter: {e}"))?;

        let (device, queue) =
            futures_executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("studio_device"),
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                ..Default::default()
            }))
            .map_err(|e| format!("request_device failed: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let depth_view = create_depth_view(&device, width, height);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera_uniform"),
            contents: bytemuck::bytes_of(&CameraUniform {
                view_proj: Mat4::IDENTITY.to_cols_array_2d(),
                camera_pos: [0.0, 8.0, 18.0, 1.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bg"),
            layout: &camera_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let shader_3d = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("studio_3d_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_3D.into()),
        });

        let pipeline_layout_3d = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("studio_3d_pl"),
            bind_group_layouts: &[Some(&camera_bgl)],
            immediate_size: 0,
        });

        let pipeline_3d = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("studio_3d_pipeline"),
            layout: Some(&pipeline_layout_3d),
            vertex: wgpu::VertexState {
                module: &shader_3d,
                entry_point: Some("vs_main"),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: core::mem::size_of::<MeshVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                offset: 0,
                                shader_location: 0,
                                format: wgpu::VertexFormat::Float32x3,
                            },
                            wgpu::VertexAttribute {
                                offset: 12,
                                shader_location: 1,
                                format: wgpu::VertexFormat::Float32x3,
                            },
                        ],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: core::mem::size_of::<ModelInstance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute {
                                offset: 0,
                                shader_location: 2,
                                format: wgpu::VertexFormat::Float32x4,
                            },
                            wgpu::VertexAttribute {
                                offset: 16,
                                shader_location: 3,
                                format: wgpu::VertexFormat::Float32x4,
                            },
                            wgpu::VertexAttribute {
                                offset: 32,
                                shader_location: 4,
                                format: wgpu::VertexFormat::Float32x4,
                            },
                            wgpu::VertexAttribute {
                                offset: 48,
                                shader_location: 5,
                                format: wgpu::VertexFormat::Float32x4,
                            },
                            wgpu::VertexAttribute {
                                offset: 64,
                                shader_location: 6,
                                format: wgpu::VertexFormat::Float32x4,
                            },
                            wgpu::VertexAttribute {
                                offset: 80,
                                shader_location: 7,
                                format: wgpu::VertexFormat::Float32,
                            },
                        ],
                    }),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_3d,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let shader_ui = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("studio_ui_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_UI.into()),
        });

        let pipeline_layout_ui = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("studio_ui_pl"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });

        let pipeline_ui = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("studio_ui_pipeline"),
            layout: Some(&pipeline_layout_ui),
            vertex: wgpu::VertexState {
                module: &shader_ui,
                entry_point: Some("vs_ui"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: core::mem::size_of::<UiVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_ui,
                entry_point: Some("fs_ui"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let mesh_crystal = upload_mesh(&device, "crystal", &build_octahedron_mesh());
        let mesh_cube = upload_mesh(&device, "cube", &build_cube_mesh());
        let mesh_prism = upload_mesh(&device, "prism", &build_prism_mesh());

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("studio_instances"),
            size: (MAX_INSTANCES * core::mem::size_of::<ModelInstance>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ui_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("studio_ui_vb"),
            size: (MAX_UI_VERTICES * core::mem::size_of::<UiVertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let static_floor = build_studio_floor();

        Ok(Self {
            surface,
            device,
            queue,
            config,
            depth_view,
            pipeline_3d,
            pipeline_ui,
            camera_buffer,
            camera_bind_group,
            mesh_crystal,
            mesh_cube,
            mesh_prism,
            instance_buffer,
            ui_vertex_buffer,
            static_floor,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, width, height);
    }

    pub fn render_frame(
        &self,
        camera: &OrbitCamera,
        actors: &[ActorRenderData],
        hud: &HudState<'_>,
    ) {
        let aspect = (self.config.width as f32) / (self.config.height.max(1) as f32);
        let eye = camera.eye();
        let view = glam::camera::rh::view::look_at_mat4(eye, camera.target, Vec3::Y);
        let proj =
            glam::camera::rh::proj::directx::perspective(46.0_f32.to_radians(), aspect, 0.1, 180.0);
        let vp = proj * view;

        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraUniform {
                view_proj: vp.to_cols_array_2d(),
                camera_pos: [eye.x, eye.y, eye.z, 1.0],
            }),
        );

        // Bucket instances by mesh shape (0 = crystal, 1 = cube, 2 = prism)
        let mut crystals = Vec::with_capacity(actors.len() / 2 + 16);
        let mut cubes = Vec::with_capacity(self.static_floor.len() + actors.len() / 2);
        let mut prisms = Vec::with_capacity(actors.len() / 3 + 16);

        cubes.extend_from_slice(&self.static_floor);

        // Adaptive base size so 256 actors look chunky and 10,000 actors stay readable
        let base_s = if actors.len() > 3000 {
            0.16
        } else if actors.len() > 800 {
            0.26
        } else {
            0.40
        };

        for a in actors {
            let xf = a.transform;
            let s = if a.is_spark { 0.8 } else { base_s };
            let rot = Quat::from_euler(EulerRot::YXZ, xf.ry, xf.rx, xf.rz);
            let scale = Vec3::new(xf.sx * s, xf.sy * s, xf.sz * s);
            let mat =
                Mat4::from_scale_rotation_translation(scale, rot, Vec3::new(xf.px, xf.py, xf.pz));
            let inst = ModelInstance::from_mat4(
                mat,
                [a.style.r, a.style.g, a.style.b, a.style.a],
                a.style.emissive,
            );
            match xf.shape % 3 {
                0 => crystals.push(inst),
                1 => cubes.push(inst),
                _ => prisms.push(inst),
            }
        }

        // Pack all 3 buckets into a single contiguous instance buffer upload
        let mut all_instances =
            Vec::with_capacity((crystals.len() + cubes.len() + prisms.len()).min(MAX_INSTANCES));
        let crystal_start = 0u32;
        let crystal_count = crystals.len().min(MAX_INSTANCES);
        all_instances.extend_from_slice(&crystals[..crystal_count]);

        let cube_start = all_instances.len() as u32;
        let cube_rem = MAX_INSTANCES.saturating_sub(all_instances.len());
        let cube_count = cubes.len().min(cube_rem);
        all_instances.extend_from_slice(&cubes[..cube_count]);

        let prism_start = all_instances.len() as u32;
        let prism_rem = MAX_INSTANCES.saturating_sub(all_instances.len());
        let prism_count = prisms.len().min(prism_rem);
        all_instances.extend_from_slice(&prisms[..prism_count]);

        if !all_instances.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&all_instances),
            );
        }

        // Build 2D HUD overlay geometry
        let ui_verts = build_hud_vertices(self.config.width as f32, self.config.height as f32, hud);
        let ui_count = ui_verts.len().min(MAX_UI_VERTICES);
        if ui_count > 0 {
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_verts[..ui_count]),
            );
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex)
            | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => tex,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("studio_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("studio_3d_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.032,
                            g: 0.040,
                            b: 0.068,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            pass.set_pipeline(&self.pipeline_3d);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));

            if crystal_count > 0 {
                pass.set_vertex_buffer(0, self.mesh_crystal.vb.slice(..));
                pass.set_index_buffer(self.mesh_crystal.ib.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(
                    0..self.mesh_crystal.index_count,
                    0,
                    crystal_start..(crystal_start + crystal_count as u32),
                );
            }
            if cube_count > 0 {
                pass.set_vertex_buffer(0, self.mesh_cube.vb.slice(..));
                pass.set_index_buffer(self.mesh_cube.ib.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(
                    0..self.mesh_cube.index_count,
                    0,
                    cube_start..(cube_start + cube_count as u32),
                );
            }
            if prism_count > 0 {
                pass.set_vertex_buffer(0, self.mesh_prism.vb.slice(..));
                pass.set_index_buffer(self.mesh_prism.ib.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(
                    0..self.mesh_prism.index_count,
                    0,
                    prism_start..(prism_start + prism_count as u32),
                );
            }
        }

        if ui_count > 0 {
            let mut ui_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("studio_ui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            ui_pass.set_pipeline(&self.pipeline_ui);
            ui_pass.set_vertex_buffer(0, self.ui_vertex_buffer.slice(..));
            ui_pass.draw(0..ui_count as u32, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
    }
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("studio_depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn upload_mesh(
    device: &wgpu::Device,
    label: &str,
    (verts, indices): &(Vec<MeshVertex>, Vec<u16>),
) -> MeshBuffers {
    let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{label}_vb")),
        contents: bytemuck::cast_slice(verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{label}_ib")),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    MeshBuffers {
        vb,
        ib,
        index_count: indices.len() as u32,
    }
}

/// Builds a crisp flat-shaded diamond octahedron (crystal) mesh.
fn build_octahedron_mesh() -> (Vec<MeshVertex>, Vec<u16>) {
    let top = Vec3::new(0.0, 1.18, 0.0);
    let bot = Vec3::new(0.0, -1.18, 0.0);
    let p = [
        Vec3::new(0.76, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 0.76),
        Vec3::new(-0.76, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -0.76),
    ];
    let mut verts = Vec::with_capacity(24);
    let mut idx = Vec::with_capacity(24);
    let mut push_tri = |a: Vec3, b: Vec3, c: Vec3| {
        let n = (b - a).cross(c - a).normalize();
        let base = verts.len() as u16;
        for pt in [a, b, c] {
            verts.push(MeshVertex {
                position: pt.to_array(),
                normal: n.to_array(),
            });
        }
        idx.extend_from_slice(&[base, base + 1, base + 2]);
    };
    for i in 0..4 {
        let next = (i + 1) % 4;
        push_tri(top, p[next], p[i]);
        push_tri(bot, p[i], p[next]);
    }
    (verts, idx)
}

/// Builds a crisp flat-shaded unit cube (`[-0.5, 0.5]^3`).
fn build_cube_mesh() -> (Vec<MeshVertex>, Vec<u16>) {
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, -0.5, 0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
            ],
        ),
    ];
    let mut verts = Vec::with_capacity(24);
    let mut idx = Vec::with_capacity(36);
    for (n, corners) in faces {
        let base = verts.len() as u16;
        for c in corners {
            verts.push(MeshVertex {
                position: c,
                normal: n,
            });
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (verts, idx)
}

/// Builds a sleek hexagonal monolith prism so yaw/pitch/roll spin is immediately legible!
fn build_prism_mesh() -> (Vec<MeshVertex>, Vec<u16>) {
    let mut verts = Vec::new();
    let mut idx = Vec::new();
    let sides = 6usize;
    let r_top = 0.42_f32;
    let r_bot = 0.62_f32;
    let h = 0.95_f32;

    let ring = |r: f32, y: f32| -> Vec<Vec3> {
        (0..sides)
            .map(|i| {
                let a = (i as f32) / (sides as f32) * core::f32::consts::TAU;
                Vec3::new(a.cos() * r, y, a.sin() * r)
            })
            .collect()
    };
    let top_ring = ring(r_top, h);
    let bot_ring = ring(r_bot, -h);

    let mut push_tri = |a: Vec3, b: Vec3, c: Vec3| {
        let n = (b - a).cross(c - a).normalize();
        let base = verts.len() as u16;
        for pt in [a, b, c] {
            verts.push(MeshVertex {
                position: pt.to_array(),
                normal: n.to_array(),
            });
        }
        idx.extend_from_slice(&[base, base + 1, base + 2]);
    };

    let top_c = Vec3::new(0.0, h + 0.20, 0.0);
    let bot_c = Vec3::new(0.0, -h, 0.0);
    for i in 0..sides {
        let next = (i + 1) % sides;
        push_tri(top_c, top_ring[next], top_ring[i]);
        push_tri(bot_c, bot_ring[i], bot_ring[next]);
        push_tri(bot_ring[i], top_ring[i], top_ring[next]);
        push_tri(bot_ring[i], top_ring[next], bot_ring[next]);
    }
    (verts, idx)
}

/// Builds the architectural dark-studio grid floor, subtle boundary ring, and origin axes.
fn build_studio_floor() -> Vec<ModelInstance> {
    let mut out = Vec::new();
    // Base dark slate floor slab
    out.push(ModelInstance::from_mat4(
        Mat4::from_scale_rotation_translation(
            Vec3::new(30.0, 0.14, 30.0),
            Quat::IDENTITY,
            Vec3::new(0.0, -0.09, 0.0),
        ),
        [0.06, 0.08, 0.125, 1.0],
        0.0,
    ));

    // Grid lines every 2 units across [-14, 14]
    for k in -7..=7 {
        let pos = (k as f32) * 2.0;
        let is_axis = k == 0;
        let thickness = if is_axis { 0.045 } else { 0.022 };
        let col_x = if is_axis {
            [0.35, 0.65, 0.95, 1.0]
        } else {
            [0.14, 0.20, 0.30, 1.0]
        };
        let em = if is_axis { 0.75 } else { 0.15 };

        // Line along X
        out.push(ModelInstance::from_mat4(
            Mat4::from_scale_rotation_translation(
                Vec3::new(28.0, 0.015, thickness),
                Quat::IDENTITY,
                Vec3::new(0.0, -0.01, pos),
            ),
            col_x,
            em,
        ));
        // Line along Z
        out.push(ModelInstance::from_mat4(
            Mat4::from_scale_rotation_translation(
                Vec3::new(thickness, 0.015, 28.0),
                Quat::IDENTITY,
                Vec3::new(pos, -0.01, 0.0),
            ),
            col_x,
            em,
        ));
    }

    // Subtle central pedestal hub
    out.push(ModelInstance::from_mat4(
        Mat4::from_scale_rotation_translation(
            Vec3::new(0.65, 0.08, 0.65),
            Quat::from_rotation_y(core::f32::consts::FRAC_PI_4),
            Vec3::new(0.0, 0.02, 0.0),
        ),
        [0.24, 0.80, 0.98, 1.0],
        1.1,
    ));

    out
}

// =============================================================================
// Crisp Pixel-Bitmap HUD Overlay (Runtime Chain + ECS Mod Perf Telemetry)
// =============================================================================

struct UiBuilder {
    w: f32,
    h: f32,
    verts: Vec<UiVertex>,
}

impl UiBuilder {
    fn new(w: f32, h: f32) -> Self {
        Self {
            w: w.max(1.0),
            h: h.max(1.0),
            verts: Vec::with_capacity(8192),
        }
    }

    fn px_to_ndc(&self, x: f32, y: f32) -> [f32; 2] {
        [(x / self.w) * 2.0 - 1.0, 1.0 - (y / self.h) * 2.0]
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let p0 = self.px_to_ndc(x, y);
        let p1 = self.px_to_ndc(x + w, y);
        let p2 = self.px_to_ndc(x + w, y + h);
        let p3 = self.px_to_ndc(x, y + h);
        self.verts.extend_from_slice(&[
            UiVertex { pos_ndc: p0, color },
            UiVertex { pos_ndc: p1, color },
            UiVertex { pos_ndc: p2, color },
            UiVertex { pos_ndc: p0, color },
            UiVertex { pos_ndc: p2, color },
            UiVertex { pos_ndc: p3, color },
        ]);
    }

    fn text(&mut self, x: f32, y: f32, scale: f32, color: [f32; 4], s: &str) {
        let mut cursor_x = x;
        for ch in s.chars() {
            if let Some(rows) = glyph_5x7(ch.to_ascii_uppercase()) {
                for (ry, bits) in rows.iter().enumerate() {
                    for rx in 0..5 {
                        if (bits >> (4 - rx)) & 1 == 1 {
                            self.rect(
                                cursor_x + (rx as f32) * scale,
                                y + (ry as f32) * scale,
                                scale,
                                scale,
                                color,
                            );
                        }
                    }
                }
            }
            cursor_x += 6.0 * scale;
        }
    }
}

fn build_hud_vertices(w: f32, h: f32, hud: &HudState<'_>) -> Vec<UiVertex> {
    let mut ui = UiBuilder::new(w, h);
    let panel_bg = [0.04, 0.06, 0.11, 0.80];
    let border = [0.22, 0.62, 0.88, 0.85];
    let white = [0.94, 0.96, 1.00, 1.0];
    let cyan = [0.35, 0.88, 1.00, 1.0];
    let gold = [1.00, 0.82, 0.36, 1.0];
    let mint = [0.42, 0.96, 0.72, 1.0];
    let muted = [0.64, 0.72, 0.84, 1.0];
    let alert = [1.00, 0.38, 0.36, 1.0];

    // Top Header Bar
    ui.rect(14.0, 12.0, (w - 28.0).max(200.0), 40.0, panel_bg);
    ui.rect(14.0, 12.0, (w - 28.0).max(200.0), 2.0, border);
    let state_tag = if hud.paused { "PAUSED" } else { "RUNNING" };
    let header = format!(
        "BAMABI RUNTIME CHAIN STUDIO  |  {}  |  RUN #{}  |  TICK {}  |  ACTORS {} (+{} SPARKS)  |  {}",
        state_tag,
        hud.run_count,
        hud.perf.tick,
        hud.perf.actor_count,
        hud.perf.spark_count,
        hud.formation.name().to_ascii_uppercase(),
    );
    ui.text(26.0, 25.0, 2.0, cyan, &header);

    // Left Panel: Active Runtime Chain & Loaded Mods
    let left_w = 390.0_f32.min(w * 0.42);
    let chain_rows = hud.chain_names.len().clamp(1, 10);
    let left_h = 112.0 + (chain_rows as f32) * 20.0;
    ui.rect(14.0, 62.0, left_w, left_h, panel_bg);
    ui.rect(14.0, 62.0, 3.0, left_h, cyan);
    ui.text(26.0, 72.0, 2.0, white, "ACTIVE RUNTIME CHAIN");

    let mode_str = match hud.chain_mode {
        ScheduleSetMode::Sequence => "SEQUENCE (.CHAIN)",
        ScheduleSetMode::SequenceWeak => "WEAK (.CHAIN_WEAK)",
        ScheduleSetMode::Parallel => "PARALLEL (DAG)",
    };
    ui.text(
        26.0,
        94.0,
        1.5,
        gold,
        &format!(
            "MODE: {} | SOLO: {}",
            mode_str,
            if hud.solo_chain { "ON" } else { "OFF" }
        ),
    );
    ui.text(
        26.0,
        112.0,
        1.5,
        muted,
        &format!(
            "MODS: {} | SYSTEMS: {} | REWIRE: {:.1} US",
            hud.loaded_mod_count, hud.discovered_sys_count, hud.perf.last_rewire_us
        ),
    );
    ui.text(
        26.0,
        128.0,
        1.5,
        muted,
        &format!("SPEED: {:.2}X  AMP: {:.2}X", hud.speed_scale, hud.amplitude),
    );

    if hud.chain_names.is_empty() {
        ui.text(
            26.0,
            152.0,
            1.5,
            muted,
            "(NO CHAIN - USE REPL: CHAIN A -> B)",
        );
    } else {
        for (i, name) in hud.chain_names.iter().take(10).enumerate() {
            let y = 152.0 + (i as f32) * 20.0;
            let arrow = if i == 0 { " " } else { ">" };
            ui.text(26.0, y, 1.6, mint, &format!("[{}] {} {}", i, arrow, name));
        }
    }

    // Right Panel: ECS-Only Mod Performance Telemetry (NO RENDERING!)
    let right_w = 480.0_f32.min(w * 0.48);
    let right_x = (w - right_w - 14.0).max(left_w + 26.0);
    let sys_rows = hud.perf.systems.len().min(10);
    let right_h = 128.0 + (sys_rows as f32) * 18.0;
    ui.rect(right_x, 62.0, right_w, right_h, panel_bg);
    ui.rect(right_x, 62.0, 3.0, right_h, mint);
    ui.text(right_x + 12.0, 72.0, 2.0, white, "ECS MOD PERF (NO RENDER)");
    ui.text(
        right_x + 12.0,
        94.0,
        1.5,
        mint,
        &format!(
            "ECS TICK: MEAN {:.1} US | P50 {:.1} | P99 {:.1}",
            hud.perf.ecs_tick_mean_us, hud.perf.ecs_tick_p50_us, hud.perf.ecs_tick_p99_us
        ),
    );
    ui.text(
        right_x + 12.0,
        112.0,
        1.5,
        cyan,
        &format!(
            "MOD SUM: P50 {} US | P99 {} US | {:.2} M ENTS/S",
            hud.perf.mod_sum_p50_us, hud.perf.mod_sum_p99_us, hud.perf.aggregate_m_ents_per_sec
        ),
    );
    ui.text(
        right_x + 12.0,
        134.0,
        1.4,
        gold,
        "SYSTEM               P50    P99   ENTS  CMDS  ST",
    );
    for (i, s) in hud.perf.systems.iter().take(10).enumerate() {
        let y = 152.0 + (i as f32) * 18.0;
        let short_name: String = s.system_name.chars().take(18).collect();
        let (st, col) = if s.quarantined {
            ("QUAR", alert)
        } else if s.breach_count > 0 {
            ("BRCH", gold)
        } else {
            ("OK", white)
        };
        let row = format!(
            "{:<18} {:>4}U {:>5}U {:>6} {:>5} {:>4}",
            short_name, s.p50_us, s.p99_us, s.matched_entities, s.commands_queued, st
        );
        ui.text(right_x + 12.0, y, 1.4, col, &row);
    }

    // Bottom Bar: Latest event + controls + REPL prompt reminder
    let bar_h = 46.0;
    let bar_y = (h - bar_h - 12.0).max(200.0);
    ui.rect(14.0, bar_y, (w - 28.0).max(200.0), bar_h, panel_bg);
    ui.rect(14.0, bar_y, (w - 28.0).max(200.0), 2.0, border);
    let ev_text = hud.latest_event.unwrap_or(
        "USE TERMINAL REPL: LOAD <PATH> | CHAIN A -> B -> C | PRESET POLYGLOT | BENCH 300 4000",
    );
    let ev_clipped: String = ev_text.chars().take(110).collect();
    ui.text(24.0, bar_y + 9.0, 1.5, gold, &ev_clipped);
    ui.text(
        24.0,
        bar_y + 27.0,
        1.4,
        muted,
        "[R] RESET  [SPACE] PAUSE  [.] STEP  [F] FORMATION  [1-6] PRESETS  [C] CAM  [DRAG/SCROLL] ORBIT  [Q] QUIT",
    );

    ui.verts
}

/// Minimal 5x7 uppercase bitmap font covering ASCII alphanumeric & punctuation.
fn glyph_5x7(ch: char) -> Option<[u8; 7]> {
    Some(match ch {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10011, 0b10101, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b01000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '|' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        '<' => [
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ],
        '>' => [
            0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
        ],
        '#' => [
            0b01010, 0b11111, 0b01010, 0b01010, 0b11111, 0b01010, 0b00000,
        ],
        '=' => [
            0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
        ' ' => return None,
        _ => [
            0b00000, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000, 0b00000,
        ],
    })
}
