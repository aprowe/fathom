//! GPU resources for the gravity simulation: buffers, pipelines, and the frame's passes.
//!
//! Kept apart from `lib.rs` so the `App` implementation reads as the app's *behaviour*
//! and this reads as its *plumbing*.

use fathom_core::wgpu;
use wgpu::util::DeviceExt;

use crate::scenes::{self, Scene};

/// The workgroup size the compute shaders declare. Body counts are kept a multiple of
/// it so no dispatch has a partly idle final group.
pub const WORKGROUP: u32 = 256;

/// Matches `Uniforms` in `shaders/common.wgsl`. Field order and padding are load-bearing.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub center: [f32; 2],
    pub scale: [f32; 2],
    pub well: [f32; 4],
    pub viewport: [f32; 2],
    pub dt: f32,
    pub g: f32,
    pub softening: f32,
    pub point_size: f32,
    pub brightness: f32,
    pub n: u32,
    pub color_mode: u32,
    pub launch_mass: f32,
    pub launch_on: f32,
    /// Brings `launch` to a 16-byte boundary, as WGSL requires of a `vec4`.
    pub _pad: u32,
    pub launch: [f32; 4],
}

/// Room kept past the scene's bodies for ones launched by hand. Once it is full, the
/// oldest launched body is replaced rather than the newest refused.
pub const LAUNCH_SLOTS: u32 = 64;

const ACCUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The HDR buffer the bodies accumulate into, plus its bind group. Rebuilt on resize.
struct Accum {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

pub struct Sim {
    /// Live bodies: the scene's, plus any launched since.
    pub n: u32,
    /// How many bodies the scene started with; launched ones sit after these.
    pub base: u32,
    launched: u32,
    pub scene: Scene,
    pub seed: u64,

    bodies: wgpu::Buffer,
    vel: wgpu::Buffer,
    accel: wgpu::Buffer,
    uniforms: wgpu::Buffer,

    compute_layout: wgpu::BindGroupLayout,
    body_layout: wgpu::BindGroupLayout,
    post_layout: wgpu::BindGroupLayout,

    compute_bind: wgpu::BindGroup,
    body_bind: wgpu::BindGroup,
    uniform_bind: wgpu::BindGroup,

    force: wgpu::ComputePipeline,
    integrate: wgpu::ComputePipeline,
    draw_bodies: wgpu::RenderPipeline,
    fade: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,

    sampler: wgpu::Sampler,
    accum: Option<Accum>,
}

/// WGSL has no `#include`, so the shared uniform declaration is pasted in here.
fn source(body: &str) -> String {
    format!("{}\n{}", include_str!("shaders/common.wgsl"), body)
}

impl Sim {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, n: u32, scene: Scene) -> Self {
        let force_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gravity force"),
            source: wgpu::ShaderSource::Wgsl(source(include_str!("shaders/force.wgsl")).into()),
        });
        let integrate_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gravity integrate"),
            source: wgpu::ShaderSource::Wgsl(source(include_str!("shaders/integrate.wgsl")).into()),
        });
        let draw_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gravity draw"),
            source: wgpu::ShaderSource::Wgsl(source(include_str!("shaders/draw.wgsl")).into()),
        });
        let fade_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gravity fade"),
            source: wgpu::ShaderSource::Wgsl(source(include_str!("shaders/fade.wgsl")).into()),
        });
        let blit_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gravity blit"),
            source: wgpu::ShaderSource::Wgsl(source(include_str!("shaders/blit.wgsl")).into()),
        });

        let uniform_entry = |visibility| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let storage_entry = |binding, visibility, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };

        let compute_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gravity compute layout"),
            entries: &[
                uniform_entry(wgpu::ShaderStages::COMPUTE),
                storage_entry(1, wgpu::ShaderStages::COMPUTE, false),
                storage_entry(2, wgpu::ShaderStages::COMPUTE, false),
                storage_entry(3, wgpu::ShaderStages::COMPUTE, false),
            ],
        });
        let body_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gravity body layout"),
            entries: &[
                uniform_entry(wgpu::ShaderStages::VERTEX_FRAGMENT),
                storage_entry(1, wgpu::ShaderStages::VERTEX, true),
                storage_entry(2, wgpu::ShaderStages::VERTEX, true),
            ],
        });
        let uniform_only_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gravity uniform layout"),
            entries: &[uniform_entry(wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gravity post layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gravity uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let compute_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gravity compute pipeline layout"),
            bind_group_layouts: &[Some(&compute_layout)],
            immediate_size: 0,
        });
        let force = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gravity force"),
            layout: Some(&compute_pipeline_layout),
            module: &force_mod,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let integrate = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gravity integrate"),
            layout: Some(&compute_pipeline_layout),
            module: &integrate_mod,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let body_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gravity body pipeline layout"),
            bind_group_layouts: &[Some(&body_layout)],
            immediate_size: 0,
        });
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let draw_bodies = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gravity bodies"),
            layout: Some(&body_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &draw_mod,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &draw_mod,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: ACCUM_FORMAT,
                    blend: Some(additive),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // src * 0 + dst * blend_constant: multiplies whatever is already there.
        let multiply_by_constant = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::Constant,
            operation: wgpu::BlendOperation::Add,
        };
        let fade_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gravity fade pipeline layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let fade = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gravity fade"),
            layout: Some(&fade_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &fade_mod,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &fade_mod,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: ACCUM_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: multiply_by_constant,
                        alpha: multiply_by_constant,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gravity blit pipeline layout"),
            bind_group_layouts: &[Some(&uniform_only_layout), Some(&post_layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gravity blit"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_mod,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blit_mod,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let uniform_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gravity uniform bind"),
            layout: &uniform_only_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gravity accum sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Placeholder buffers, immediately replaced by `reseed` below. Creating them
        // here keeps every field initialised without an `Option` on the hot path.
        let (bodies, vel, accel) = allocate(device, queue, scene, n, 0);
        let compute_bind = compute_bind_group(device, &compute_layout, &uniforms, &bodies, &vel, &accel);
        let body_bind = body_bind_group(device, &body_layout, &uniforms, &bodies, &vel);

        Self {
            n,
            base: n,
            launched: 0,
            scene,
            seed: 0,
            bodies,
            vel,
            accel,
            uniforms,
            compute_layout,
            body_layout,
            post_layout,
            compute_bind,
            body_bind,
            uniform_bind,
            force,
            integrate,
            draw_bodies,
            fade,
            blit,
            sampler,
            accum: None,
        }
    }

    /// Rebuild the body buffers for a new scene, count, or seed.
    pub fn reseed(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: Scene, n: u32, seed: u64) {
        let (bodies, vel, accel) = allocate(device, queue, scene, n, seed);
        self.compute_bind =
            compute_bind_group(device, &self.compute_layout, &self.uniforms, &bodies, &vel, &accel);
        self.body_bind = body_bind_group(device, &self.body_layout, &self.uniforms, &bodies, &vel);
        self.bodies = bodies;
        self.vel = vel;
        self.accel = accel;
        self.n = n;
        self.base = n;
        self.launched = 0;
        self.scene = scene;
        self.seed = seed;
        // Old trails belong to the old scene.
        self.accum = None;
    }

    /// Add one body at `pos` moving at `vel`. The buffers keep [`LAUNCH_SLOTS`] spare
    /// entries past the scene, so this is two small writes and no reallocation.
    pub fn launch(&mut self, queue: &wgpu::Queue, pos: [f32; 2], mass: f32, vel: [f32; 2]) {
        let slot = self.base + self.launched % LAUNCH_SLOTS;
        self.launched = self.launched.wrapping_add(1);
        let body: [f32; 4] = [pos[0], pos[1], mass, 0.0];
        queue.write_buffer(&self.bodies, u64::from(slot) * 16, bytemuck::bytes_of(&body));
        queue.write_buffer(&self.vel, u64::from(slot) * 8, bytemuck::bytes_of(&vel));
        self.n = self.n.max(slot + 1);
    }

    pub fn write_uniforms(&self, queue: &wgpu::Queue, uniforms: &Uniforms) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
    }

    /// Force, then integrate. Two dispatches, one barrier between them (implicit at the
    /// pass boundary).
    pub fn step(&self, encoder: &mut wgpu::CommandEncoder) {
        let groups = self.n.div_ceil(WORKGROUP);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gravity step"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &self.compute_bind, &[]);
        pass.set_pipeline(&self.force);
        pass.dispatch_workgroups(groups, 1, 1);
        pass.set_pipeline(&self.integrate);
        pass.dispatch_workgroups(groups, 1, 1);
    }

    /// The accumulation buffer must match the viewport, and the viewport is owned by
    /// the interface, so this is checked every frame rather than on a resize callback.
    fn ensure_accum(&mut self, device: &wgpu::Device, width: u32, height: u32) -> bool {
        if let Some(a) = &self.accum {
            if a.width == width && a.height == height {
                return false;
            }
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gravity accum"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ACCUM_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gravity accum bind"),
            layout: &self.post_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.accum = Some(Accum { view, bind_group, width, height });
        true
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        trails: bool,
        trail_fade: f32,
    ) {
        let recreated = self.ensure_accum(device, width.max(1), height.max(1));
        let accum = self.accum.as_ref().expect("accum was just ensured");

        {
            // A freshly created target has undefined contents, so the first frame after a
            // resize must clear rather than load.
            let load = if trails && !recreated {
                wgpu::LoadOp::Load
            } else {
                wgpu::LoadOp::Clear(wgpu::Color::BLACK)
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gravity accumulate"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &accum.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if matches!(load, wgpu::LoadOp::Load) {
                let f = trail_fade.clamp(0.0, 0.999) as f64;
                pass.set_pipeline(&self.fade);
                pass.set_blend_constant(wgpu::Color { r: f, g: f, b: f, a: f });
                pass.draw(0..3, 0..1);
            }

            pass.set_pipeline(&self.draw_bodies);
            pass.set_bind_group(0, &self.body_bind, &[]);
            pass.draw(0..6, 0..self.n);
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gravity blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blit);
            pass.set_bind_group(0, &self.uniform_bind, &[]);
            pass.set_bind_group(1, &accum.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Drop the trail buffer, so the next frame starts clean.
    pub fn clear_trails(&mut self) {
        self.accum = None;
    }

    /// Read the bodies back to the CPU. Test-only: the round trip stalls the pipeline.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_accelerations(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<[f32; 2]> {
        let size = (self.n as u64) * 8;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("accel readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.accel, 0, &staging, 0, size);
        queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = slice.get_mapped_range().expect("accel buffer failed to map");
        let out = bytemuck::cast_slice::<u8, [f32; 2]>(&data).to_vec();
        drop(data);
        staging.unmap();
        out
    }
}

fn allocate(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: Scene,
    n: u32,
    seed: u64,
) -> (wgpu::Buffer, wgpu::Buffer, wgpu::Buffer) {
    let (mut bodies, mut vels) = scenes::generate(scene, n as usize, seed);
    // The spare slots are massless until launched into, so they pull on nothing; they
    // are also past `n`, so the shaders never visit them.
    bodies.resize(bodies.len() + LAUNCH_SLOTS as usize, [0.0; 4]);
    vels.resize(vels.len() + LAUNCH_SLOTS as usize, [0.0; 2]);
    let bodies_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("gravity bodies"),
        contents: bytemuck::cast_slice(&bodies),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let vel_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("gravity velocities"),
        contents: bytemuck::cast_slice(&vels),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let accel_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gravity accelerations"),
        size: u64::from(n + LAUNCH_SLOTS) * 8,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let _ = queue;
    (bodies_buf, vel_buf, accel_buf)
}

fn compute_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    bodies: &wgpu::Buffer,
    vel: &wgpu::Buffer,
    accel: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gravity compute bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: bodies.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: vel.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: accel.as_entire_binding() },
        ],
    })
}

fn body_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    bodies: &wgpu::Buffer,
    vel: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gravity body bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: bodies.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: vel.as_entire_binding() },
        ],
    })
}
