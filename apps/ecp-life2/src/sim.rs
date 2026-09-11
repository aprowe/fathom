//! GPU resources: buffers, pipelines, and the passes that make up a frame.
//!
//! Kept apart from `lib.rs` so that file reads as the app's *behaviour* and this one as
//! its *plumbing*.
//!
//! The pass order per substep is fixed and worth stating once:
//!
//! ```text
//! clear → count → scan_blocks → scan_sums → scan_add → scatter   rebuild the grid
//! force                                                          f, chase, and dE/dc
//! integrate                                                      move, and settle up
//! ```
//!
//! The grid is rebuilt from scratch every substep rather than repaired. At these
//! timesteps almost nothing changes cell, so repairing it would be cheaper — but a
//! counting sort of a hundred thousand particles is already a rounding error next to the
//! force pass, and an incrementally maintained grid is a structure that can be subtly
//! wrong for thousands of frames before anybody notices.

use fathom_core::wgpu;
use wgpu::util::DeviceExt;

use crate::landscape::{self, Landscape};
use crate::physics::Settings;
use crate::scenes::{self, Scene};

/// The workgroup size every compute shader declares.
pub const WORKGROUP: u32 = 256;

/// Finest subdivision of the world the cutoff slider can ask for.
///
/// Also the reason the prefix scan is exactly two levels deep: 256 blocks of 256 cells
/// is 65,536, so a third level would have nothing to scan.
pub const MAX_CELLS_1D: u32 = 256;
const MAX_CELLS: u32 = MAX_CELLS_1D * MAX_CELLS_1D;
const SCAN_BLOCKS: u32 = MAX_CELLS / WORKGROUP;
/// counts, offsets, cursors, then one running total per scan block.
const GRID_WORDS: u64 = (3 * MAX_CELLS + SCAN_BLOCKS) as u64;

// The prefix scan is exactly two levels deep, and only works out because these two hold.
// Checked here rather than in a test: a grid that does not tile is not a failing
// assertion at run time, it is a build that should not have happened.
const _: () = assert!(SCAN_BLOCKS * WORKGROUP == MAX_CELLS, "the block scan must tile the grid");
const _: () = assert!(SCAN_BLOCKS <= WORKGROUP, "the block totals need a scan of their own");

/// Matches `Uniforms` in `shaders/common.wgsl`. Field order and padding are load-bearing.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub center: [f32; 2],
    pub scale: [f32; 2],
    pub viewport: [f32; 2],
    pub dt: f32,
    pub time: f32,

    pub r_cut: f32,
    pub r_core: f32,
    pub k_rep: f32,
    pub lam: f32,
    pub chase_gain: f32,
    pub damping: f32,
    pub s0: f32,
    pub amp_sym: f32,
    pub amp_chase: f32,
    pub well_sigma: f32,
    pub rstar_min: f32,
    pub rstar_span: f32,

    pub point_size: f32,
    pub brightness: f32,
    pub probe_a: f32,
    pub probe_b: f32,
    pub cell_size: f32,

    pub n: u32,
    pub cells: u32,
    pub profile: u32,
    pub color_mode: u32,
    pub overlay: u32,
    pub _pad: [u32; 2],
}

impl Uniforms {
    /// Fill in the physics half from resolved settings, leaving the camera and render
    /// half to the caller.
    pub fn with_physics(mut self, s: &Settings, dt: f32) -> Self {
        self.dt = dt;
        self.r_cut = s.r_cut;
        self.r_core = s.r_core;
        self.k_rep = s.k_rep;
        self.lam = s.lam;
        self.chase_gain = s.chase_gain;
        self.damping = s.damping;
        self.s0 = s.s0;
        self.amp_sym = s.amp_sym;
        self.amp_chase = s.amp_chase;
        self.well_sigma = s.well_sigma;
        self.rstar_min = s.rstar_min;
        self.rstar_span = s.rstar_span;
        self.profile = s.profile.index();
        self.cells = s.cells();
        self.cell_size = 1.0 / s.cells() as f32;
        self
    }
}

const ACCUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The HDR buffer the particles accumulate into before tone mapping. Rebuilt on resize.
///
/// Still worth having with no trails to keep: additive blending of a hundred thousand
/// points runs far above 1.0 wherever the system has clumped, and clipping that to white
/// would lose exactly the structure worth looking at.
struct Accum {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

pub struct Sim {
    pub n: u32,
    pub scene: Scene,
    pub seed: u64,

    pos: wgpu::Buffer,
    vel: wgpu::Buffer,
    acc: wgpu::Buffer,
    lut: wgpu::Buffer,
    grid: wgpu::Buffer,
    sorted: wgpu::Buffer,
    uniforms: wgpu::Buffer,

    compute_layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    post_layout: wgpu::BindGroupLayout,

    compute_bind: wgpu::BindGroup,
    draw_bind: wgpu::BindGroup,
    uniform_bind: wgpu::BindGroup,
    overlay_bind: wgpu::BindGroup,

    grid_clear: wgpu::ComputePipeline,
    grid_count: wgpu::ComputePipeline,
    scan_blocks: wgpu::ComputePipeline,
    scan_sums: wgpu::ComputePipeline,
    scan_add: wgpu::ComputePipeline,
    scatter: wgpu::ComputePipeline,
    force: wgpu::ComputePipeline,
    integrate: wgpu::ComputePipeline,

    draw_particles: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,

    sampler: wgpu::Sampler,
    accum: Option<Accum>,
}

/// WGSL has no `#include`, so the shared declarations are pasted in here.
fn source(parts: &[&str]) -> String {
    parts.join("\n")
}

const COMMON: &str = include_str!("shaders/common.wgsl");
const LUT: &str = include_str!("shaders/lut.wgsl");
const GRID: &str = include_str!("shaders/grid.wgsl");

impl Sim {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        n: u32,
        scene: Scene,
        landscape: &Landscape,
    ) -> Self {
        let module = |label: &str, parts: &[&str]| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source(parts).into()),
            })
        };
        let grid_mod = module("ecp grid", &[COMMON, GRID, include_str!("shaders/grid_build.wgsl")]);
        let force_mod = module("ecp force", &[COMMON, LUT, GRID, include_str!("shaders/force.wgsl")]);
        let integrate_mod = module("ecp integrate", &[COMMON, include_str!("shaders/integrate.wgsl")]);
        let draw_mod = module("ecp draw", &[COMMON, include_str!("shaders/draw.wgsl")]);
        let blit_mod = module("ecp blit", &[COMMON, include_str!("shaders/blit.wgsl")]);
        let overlay_mod = module("ecp overlay", &[COMMON, LUT, include_str!("shaders/overlay.wgsl")]);

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

        // One layout for all eight compute pipelines. They use overlapping subsets of it,
        // and a layout per pass would be seven more places for a binding index to drift.
        let compute_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ecp compute layout"),
            entries: &[
                uniform_entry(wgpu::ShaderStages::COMPUTE),
                storage_entry(1, wgpu::ShaderStages::COMPUTE, false),
                storage_entry(2, wgpu::ShaderStages::COMPUTE, false),
                storage_entry(3, wgpu::ShaderStages::COMPUTE, false),
                storage_entry(4, wgpu::ShaderStages::COMPUTE, true),
                storage_entry(5, wgpu::ShaderStages::COMPUTE, false),
                storage_entry(6, wgpu::ShaderStages::COMPUTE, false),
            ],
        });
        // Read-only in the vertex stage: WebGPU forbids writable storage there, which is
        // exactly why the particle buffers are bound through a second layout for drawing.
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ecp draw layout"),
            entries: &[
                uniform_entry(wgpu::ShaderStages::VERTEX_FRAGMENT),
                storage_entry(1, wgpu::ShaderStages::VERTEX, true),
                storage_entry(2, wgpu::ShaderStages::VERTEX, true),
            ],
        });
        let uniform_only_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ecp uniform layout"),
            entries: &[uniform_entry(wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let overlay_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ecp overlay layout"),
            entries: &[
                uniform_entry(wgpu::ShaderStages::VERTEX_FRAGMENT),
                storage_entry(4, wgpu::ShaderStages::FRAGMENT, true),
            ],
        });
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ecp post layout"),
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
            label: Some("ecp uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Sized for the finest grid the cutoff allows and reused at every coarser one, so
        // moving the cutoff slider reallocates nothing.
        let grid = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ecp grid"),
            size: GRID_WORDS * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let lut = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ecp landscape"),
            size: (landscape::LUT_SIZE * landscape::LUT_SIZE) as u64
                * std::mem::size_of::<landscape::Entry>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let compute_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ecp compute pipeline layout"),
            bind_group_layouts: &[Some(&compute_layout)],
            immediate_size: 0,
        });
        let compute = |label: &str, module: &wgpu::ShaderModule, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&compute_pipeline_layout),
                module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let grid_clear = compute("ecp grid clear", &grid_mod, "clear");
        let grid_count = compute("ecp grid count", &grid_mod, "count");
        let scan_blocks = compute("ecp grid scan blocks", &grid_mod, "scan_blocks");
        let scan_sums = compute("ecp grid scan sums", &grid_mod, "scan_sums");
        let scan_add = compute("ecp grid scan add", &grid_mod, "scan_add");
        let scatter = compute("ecp grid scatter", &grid_mod, "scatter");
        let force = compute("ecp force", &force_mod, "main");
        let integrate = compute("ecp integrate", &integrate_mod, "main");

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
        let draw_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ecp draw pipeline layout"),
            bind_group_layouts: &[Some(&draw_layout)],
            immediate_size: 0,
        });
        let draw_particles = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ecp particles"),
            layout: Some(&draw_pipeline_layout),
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

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ecp blit pipeline layout"),
            bind_group_layouts: &[Some(&uniform_only_layout), Some(&post_layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ecp blit"),
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

        let overlay_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ecp overlay pipeline layout"),
            bind_group_layouts: &[Some(&overlay_layout)],
            immediate_size: 0,
        });
        let overlay = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ecp overlay"),
            layout: Some(&overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &overlay_mod,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &overlay_mod,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
            label: Some("ecp uniform bind"),
            layout: &uniform_only_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let overlay_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ecp overlay bind"),
            layout: &overlay_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: lut.as_entire_binding() },
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ecp accum sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let (pos, vel, acc, sorted) = allocate(device, scene, n, 0);
        let compute_bind =
            compute_bind_group(device, &compute_layout, &uniforms, &pos, &vel, &acc, &lut, &grid, &sorted);
        let draw_bind = draw_bind_group(device, &draw_layout, &uniforms, &pos, &vel);

        let sim = Self {
            n,
            scene,
            seed: 0,
            pos,
            vel,
            acc,
            lut,
            grid,
            sorted,
            uniforms,
            compute_layout,
            draw_layout,
            post_layout,
            compute_bind,
            draw_bind,
            uniform_bind,
            overlay_bind,
            grid_clear,
            grid_count,
            scan_blocks,
            scan_sums,
            scan_add,
            scatter,
            force,
            integrate,
            draw_particles,
            blit,
            overlay,
            sampler,
            accum: None,
        };
        sim.write_landscape(queue, landscape);
        sim
    }

    /// Upload a freshly baked landscape. Cheap enough to do on every randomise: it is a
    /// quarter of a megabyte and nothing else has to be rebuilt.
    pub fn write_landscape(&self, queue: &wgpu::Queue, landscape: &Landscape) {
        queue.write_buffer(&self.lut, 0, bytemuck::cast_slice(landscape.table().entries()));
    }

    /// Rebuild the particle buffers for a new scene, count, or seed.
    pub fn reseed(&mut self, device: &wgpu::Device, scene: Scene, n: u32, seed: u64) {
        let (pos, vel, acc, sorted) = allocate(device, scene, n, seed);
        self.compute_bind = compute_bind_group(
            device,
            &self.compute_layout,
            &self.uniforms,
            &pos,
            &vel,
            &acc,
            &self.lut,
            &self.grid,
            &sorted,
        );
        self.draw_bind = draw_bind_group(device, &self.draw_layout, &self.uniforms, &pos, &vel);
        self.pos = pos;
        self.vel = vel;
        self.acc = acc;
        self.sorted = sorted;
        self.n = n;
        self.scene = scene;
        self.seed = seed;
    }

    pub fn write_uniforms(&self, queue: &wgpu::Queue, uniforms: &Uniforms) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
    }

    /// One substep: rebuild the grid, find the forces, move and settle up.
    ///
    /// Each dispatch is its own pass, which is what gives the implicit barrier between
    /// them. They are strictly sequential — every one reads what the last one wrote.
    pub fn step(&self, encoder: &mut wgpu::CommandEncoder) {
        let particles = self.n.div_ceil(WORKGROUP);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("ecp step"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &self.compute_bind, &[]);

        pass.set_pipeline(&self.grid_clear);
        pass.dispatch_workgroups(SCAN_BLOCKS, 1, 1);
        pass.set_pipeline(&self.grid_count);
        pass.dispatch_workgroups(particles, 1, 1);
        pass.set_pipeline(&self.scan_blocks);
        pass.dispatch_workgroups(SCAN_BLOCKS, 1, 1);
        pass.set_pipeline(&self.scan_sums);
        pass.dispatch_workgroups(1, 1, 1);
        pass.set_pipeline(&self.scan_add);
        pass.dispatch_workgroups(SCAN_BLOCKS, 1, 1);
        pass.set_pipeline(&self.scatter);
        pass.dispatch_workgroups(particles, 1, 1);

        pass.set_pipeline(&self.force);
        pass.dispatch_workgroups(particles, 1, 1);
        pass.set_pipeline(&self.integrate);
        pass.dispatch_workgroups(particles, 1, 1);
    }

    /// The accumulation buffer must match the viewport, and the viewport is owned by the
    /// interface, so this is checked every frame rather than on a resize callback.
    fn ensure_accum(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if let Some(a) = &self.accum
            && a.width == width
            && a.height == height
        {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ecp accum"),
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
            label: Some("ecp accum bind"),
            layout: &self.post_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.accum = Some(Accum { view, bind_group, width, height });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        overlay: bool,
    ) {
        self.ensure_accum(device, width.max(1), height.max(1));
        let accum = self.accum.as_ref().expect("accum was just ensured");

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ecp accumulate"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &accum.view,
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

            pass.set_pipeline(&self.draw_particles);
            pass.set_bind_group(0, &self.draw_bind, &[]);
            pass.draw(0..6, 0..self.n);
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ecp blit"),
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

            // The instruments go over the tone-mapped image, in the same pass: they are
            // interface, so they must not be tone-mapped along with the simulation.
            if overlay {
                pass.set_pipeline(&self.overlay);
                pass.set_bind_group(0, &self.overlay_bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
    }

    /// Read the particle state back to the CPU. Test-only: the round trip stalls the
    /// pipeline, which is exactly what a conservation check wants and a frame does not.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_state(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
        (read_vec4(device, queue, &self.pos, self.n), read_vec4(device, queue, &self.vel, self.n))
    }

    /// Read the force buffer back: total force in `xy`, the chase alone in `zw`.
    ///
    /// The two are accumulated separately because the ledger needs the chase on its own,
    /// which also makes "how much of what is happening is the chase" a thing that can be
    /// measured rather than argued about.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_forces(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<[f32; 4]> {
        read_vec4(device, queue, &self.acc, self.n)
    }

    /// Overwrite the particle state, so a test can set up an exact configuration rather
    /// than hunting for a seed that happens to produce one.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_state(&self, queue: &wgpu::Queue, pos: &[[f32; 4]], vel: &[[f32; 4]]) {
        queue.write_buffer(&self.pos, 0, bytemuck::cast_slice(pos));
        queue.write_buffer(&self.vel, 0, bytemuck::cast_slice(vel));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_vec4(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    n: u32,
) -> Vec<[f32; 4]> {
    let size = (n as u64) * 16;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ecp readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
    queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = slice.get_mapped_range().expect("readback failed to map");
    let out = bytemuck::cast_slice::<u8, [f32; 4]>(&data).to_vec();
    drop(data);
    staging.unmap();
    out
}

fn allocate(
    device: &wgpu::Device,
    scene: Scene,
    n: u32,
    seed: u64,
) -> (wgpu::Buffer, wgpu::Buffer, wgpu::Buffer, wgpu::Buffer) {
    let (particles, velocities) = scenes::generate(scene, n as usize, seed);
    let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
    let pos = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ecp positions"),
        contents: bytemuck::cast_slice(&particles),
        usage: storage,
    });
    let vel = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ecp velocities"),
        contents: bytemuck::cast_slice(&velocities),
        usage: storage,
    });
    let acc = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ecp forces"),
        size: (n as u64) * 16,
        usage: storage,
        mapped_at_creation: false,
    });
    let sorted = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ecp cell order"),
        size: (n as u64) * 4,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    (pos, vel, acc, sorted)
}

#[allow(clippy::too_many_arguments)]
fn compute_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    pos: &wgpu::Buffer,
    vel: &wgpu::Buffer,
    acc: &wgpu::Buffer,
    lut: &wgpu::Buffer,
    grid: &wgpu::Buffer,
    sorted: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ecp compute bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: pos.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: vel.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: acc.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: lut.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: grid.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 6, resource: sorted.as_entire_binding() },
        ],
    })
}

fn draw_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    pos: &wgpu::Buffer,
    vel: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ecp draw bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: pos.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: vel.as_entire_binding() },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uniform_block_is_laid_out_the_way_wgsl_will_read_it() {
        // vec2 fields must sit on 8-byte boundaries and the whole block on 16, or the
        // shader silently reads the wrong field and the simulation misbehaves in a way
        // that looks like a physics bug.
        assert_eq!(std::mem::size_of::<Uniforms>() % 16, 0);
        assert_eq!(std::mem::offset_of!(Uniforms, center) % 8, 0);
        assert_eq!(std::mem::offset_of!(Uniforms, scale) % 8, 0);
        assert_eq!(std::mem::offset_of!(Uniforms, viewport) % 8, 0);
    }


}
