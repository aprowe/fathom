//! The render path, driven headlessly into an offscreen texture.
//!
//! `conservation.rs` builds every pipeline, so it already proves the shaders compile.
//! What it never does is *draw*, and the drawing side has failure modes of its own that
//! validation catches only at submit time: an attachment format that disagrees with the
//! pipeline, a bind group belonging to the wrong layout, a viewport of zero.
//!
//! So this renders actual frames and looks at the pixels. Checking that something was
//! drawn — rather than only that nothing panicked — is what makes it a test of the
//! render path instead of a test that wgpu can be called.

use ecp_life2::landscape::Landscape;
use ecp_life2::scenes::Scene;
use ecp_life2::sim::{Sim, Uniforms};
use fathom_core::wgpu;

const W: u32 = 256;
const H: u32 = 192;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("ecp-life render test"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))
    .ok()
}

/// Render `count` frames and return the last one as RGBA rows.
fn frames(overlay: bool, count: usize) -> Option<Vec<[u8; 4]>> {
    let (device, queue) = device()?;
    let landscape = Landscape::new(2, 3);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut sim = Sim::new(&device, &queue, format, 4096, Scene::Soup, &landscape);

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    // A camera that frames the unit world, written by hand rather than taken from a
    // `Camera`: the test is about the render path, not about the framework's defaults.
    sim.write_uniforms(
        &queue,
        &Uniforms {
            center: [0.0, 0.0],
            scale: [1.6, 1.6 * W as f32 / H as f32],
            viewport: [W as f32, H as f32],
            point_size: 2.0,
            brightness: 1.5,
            probe_a: 0.4,
            probe_b: 3.1,
            n: 4096,
            overlay: u32::from(overlay),
            ..Uniforms::default()
        },
    );

    for _ in 0..count {
        let mut encoder = device.create_command_encoder(&Default::default());
        sim.step(&mut encoder);
        sim.render(&device, &mut encoder, &view, W, H, overlay);
        queue.submit(Some(encoder.finish()));
    }

    // 256 pixels of RGBA is exactly the 256-byte row alignment a copy wants, which is why
    // the test is this wide.
    let bytes = (W * H * 4) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("frame readback"),
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(W * 4),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );
    queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = slice.get_mapped_range().expect("frame failed to map");
    let out = bytemuck::cast_slice::<u8, [u8; 4]>(&data).to_vec();
    drop(data);
    staging.unmap();
    Some(out)
}

fn brightness(px: &[u8; 4]) -> u32 {
    px[0] as u32 + px[1] as u32 + px[2] as u32
}

#[test]
fn a_frame_comes_out_with_particles_in_it() {
    let Some(pixels) = frames(false, 3) else {
        eprintln!("no GPU adapter; skipping the render check");
        return;
    };
    assert_eq!(pixels.len(), (W * H) as usize);

    // The blit lays down a near-black ground everywhere, so every pixel must be opaque
    // and none may be pure black: a fully black frame means the blit never ran.
    assert!(pixels.iter().all(|p| p[3] == 255), "the frame has transparent pixels");
    let ground = pixels.iter().filter(|p| brightness(p) < 8).count();
    assert!(ground * 4 < pixels.len(), "the frame is mostly pure black");

    // And something must be brighter than the ground, or nothing was drawn on it.
    let lit = pixels.iter().filter(|p| brightness(p) > 40).count();
    assert!(lit > 50, "only {lit} pixels are lit; the particles did not draw");
}

/// The instruments are drawn into the bottom corners by a pass of their own. If the
/// overlay pipeline were mis-bound, the frame would come back looking exactly like one
/// without it — silently, since nothing errors.
#[test]
fn the_instruments_change_the_bottom_corners_and_nothing_else() {
    let Some(without) = frames(false, 2) else {
        eprintln!("no GPU adapter; skipping the overlay check");
        return;
    };
    let with = frames(true, 2).expect("adapter vanished mid-test");

    let differing = |rows: std::ops::Range<u32>| {
        let mut n = 0;
        for y in rows {
            for x in 0..W {
                let i = (y * W + x) as usize;
                if with[i] != without[i] {
                    n += 1;
                }
            }
        }
        n
    };

    // The panels are sized from the viewport and sit above a 14px margin, so on a frame
    // this size they occupy roughly the bottom third.
    assert!(differing(H / 2..H) > 500, "the instruments drew nothing");
    assert_eq!(differing(0..H / 3), 0, "the instruments drew outside their corners");
}
