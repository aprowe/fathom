//! The "it computes" test: one dispatch of the tiled O(n^2) force kernel, checked
//! against a plain CPU implementation of the same sum.
//!
//! The GPU version is tiled through workgroup storage, which is exactly the kind of
//! optimisation that can be subtly wrong — an off-by-one on the tile bounds silently
//! drops or double-counts bodies, and the simulation still *looks* plausible. This
//! catches that.
//!
//! Skipped with a message when no adapter is available, so it does not fail CI on a
//! machine with no GPU.

use fathom_core::wgpu;
use gravity::scenes::{self, Scene};
use gravity::sim::{Sim, Uniforms};

const N: u32 = 256;
const G: f32 = 1.3;
const SOFTENING: f32 = 0.03;
const SEED: u64 = 9;

fn cpu_accelerations(bodies: &[[f32; 4]]) -> Vec<[f32; 2]> {
    let eps2 = SOFTENING * SOFTENING;
    bodies
        .iter()
        .map(|me| {
            let mut acc = [0.0f32; 2];
            for other in bodies {
                let d = [other[0] - me[0], other[1] - me[1]];
                let r2 = d[0] * d[0] + d[1] * d[1] + eps2;
                let inv_r = 1.0 / r2.sqrt();
                let f = other[2] * inv_r * inv_r * inv_r;
                acc[0] += d[0] * f;
                acc[1] += d[1] * f;
            }
            [acc[0] * G, acc[1] * G]
        })
        .collect()
}

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
        label: Some("gravity test"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))
    .ok()
}

#[test]
fn the_tiled_gpu_force_kernel_agrees_with_a_cpu_sum() {
    let Some((device, queue)) = device() else {
        eprintln!("skipped: no GPU adapter available");
        return;
    };

    let mut sim = Sim::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, N, Scene::Disc);
    sim.reseed(&device, &queue, Scene::Disc, N, SEED);

    sim.write_uniforms(
        &queue,
        &Uniforms {
            // dt = 0 so the integrate pass cannot move anything before the readback.
            dt: 0.0,
            g: G,
            softening: SOFTENING,
            n: N,
            scale: [1.0, 1.0],
            ..Default::default()
        },
    );

    let mut encoder = device.create_command_encoder(&Default::default());
    sim.step(&mut encoder);
    queue.submit(Some(encoder.finish()));

    let gpu = sim.read_accelerations(&device, &queue);
    let (bodies, _) = scenes::generate(Scene::Disc, N as usize, SEED);
    let cpu = cpu_accelerations(&bodies);

    assert_eq!(gpu.len(), cpu.len());
    let scale = cpu
        .iter()
        .flat_map(|a| [a[0].abs(), a[1].abs()])
        .fold(1e-6f32, f32::max);

    for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
        let dx = (g[0] - c[0]).abs() / scale;
        let dy = (g[1] - c[1]).abs() / scale;
        assert!(
            dx < 1e-3 && dy < 1e-3,
            "body {i}: gpu {g:?} vs cpu {c:?} (relative error {dx}, {dy})"
        );
    }
}
