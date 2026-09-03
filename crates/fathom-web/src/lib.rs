//! The web host: a fathom app rendering into a `<canvas>` through WebGPU.
//!
//! This crate is empty on native targets. All it does is own a surface and marshal
//! between JavaScript and [`fathom_core::Runner`] — the loop itself lives in the core,
//! shared with the native host, which is what keeps the two targets from drifting.
//!
//! JavaScript drives the frame loop (via `requestAnimationFrame`) rather than Rust, so
//! the interface can pause, throttle, or tear down the app without fighting a loop it
//! does not own.

#![cfg(target_arch = "wasm32")]

use fathom_core::{App, Gpu, Runner, Viewport, wgpu};
use web_sys::HtmlCanvasElement;

/// Everything the JS-facing wrapper needs. `export_app!` generates that wrapper.
pub struct WebHost<A: App> {
    runner: Runner<A>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    adapter: wgpu::Adapter,
}

impl<A: App> WebHost<A> {
    pub async fn create(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("could not create a WebGPU surface: {e}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|e| {
                format!(
                    "no WebGPU adapter available ({e}). WebGPU needs Chrome or Edge 113+, \
                     Safari 26+, or Firefox with dom.webgpu.enabled."
                )
            })?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fathom"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("could not create a GPU device: {e}"))?;

        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| "the adapter cannot present to this canvas".to_string())?;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        surface.configure(&device, &config);

        let gpu = Gpu {
            device,
            queue,
            format: config.format,
            adapter_info: adapter.get_info(),
        };
        let runner = Runner::new(gpu, Viewport::new(width, height, 1.0));

        Ok(Self { runner, surface, config, adapter })
    }

    pub fn descriptor_json(&self) -> String {
        serde_json::to_string(&Runner::<A>::descriptor()).unwrap_or_else(|_| "{}".into())
    }

    pub fn stats_json(&self) -> String {
        serde_json::to_string(&self.runner.stats()).unwrap_or_else(|_| "{}".into())
    }

    pub fn adapter_description(&self) -> String {
        self.runner.gpu().describe_adapter()
    }

    pub fn param_byte_len(&self) -> usize {
        self.runner.param_byte_len()
    }

    pub fn set_viewport(&mut self, width: u32, height: u32, dpr: f32) {
        let (width, height) = (width.max(1), height.max(1));
        if self.config.width != width || self.config.height != height {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.runner.gpu().device, &self.config);
        }
        self.runner.resize(Viewport::new(width, height, dpr));
    }

    pub fn write_params(&mut self, bytes: &[u8]) {
        self.runner.write_params(bytes);
    }

    pub fn input(&mut self, json: &str) {
        match serde_json::from_str(json) {
            Ok(event) => self.runner.input(event),
            Err(e) => log::warn!("dropped malformed input event: {e}"),
        }
    }

    pub fn command(&mut self, name: &str, args_json: &str) {
        let value = serde_json::from_str(args_json).unwrap_or(serde_json::Value::Null);
        self.runner.command(name, value);
    }

    pub fn frame(&mut self, now_ms: f64) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            // Outdated or lost usually means the canvas was resized underneath us;
            // reconfiguring and skipping one frame is the cheapest recovery.
            other => {
                log::warn!("surface unavailable this frame: {other:?}");
                self.surface.configure(&self.runner.gpu().device, &self.config);
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.runner.frame(now_ms, &view);
        self.runner.gpu().queue.present(frame);
    }

    /// Kept so the adapter outlives the surface without a warning.
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }
}

/// Generate the JavaScript-facing class for an app.
///
/// The app crate needs `wasm-bindgen`, `wasm-bindgen-futures` and `web-sys` (with the
/// `HtmlCanvasElement` feature) as wasm-target dependencies; the generated bindings
/// refer to them by absolute path.
///
/// ```ignore
/// #[cfg(target_arch = "wasm32")]
/// fathom_web::export_app!(Gravity);
/// ```
#[macro_export]
macro_rules! export_app {
    ($app:ty) => {
        /// The app, as JavaScript sees it.
        #[::wasm_bindgen::prelude::wasm_bindgen]
        pub struct FathomApp {
            inner: $crate::WebHost<$app>,
        }

        #[::wasm_bindgen::prelude::wasm_bindgen]
        impl FathomApp {
            /// Attach to a canvas and set the app up. Rejects with a readable message if
            /// WebGPU is unavailable, which is what the interface shows the user.
            pub async fn create(
                canvas: ::web_sys::HtmlCanvasElement,
            ) -> ::std::result::Result<FathomApp, ::wasm_bindgen::JsValue> {
                ::std::panic::set_hook(::std::boxed::Box::new(
                    ::console_error_panic_hook::hook,
                ));
                let _ = ::console_log::init_with_level(::log::Level::Warn);
                match $crate::WebHost::<$app>::create(canvas).await {
                    Ok(inner) => Ok(FathomApp { inner }),
                    Err(e) => Err(::wasm_bindgen::JsValue::from_str(&e)),
                }
            }

            /// The parameter and command schema, as JSON.
            pub fn descriptor(&self) -> String {
                self.inner.descriptor_json()
            }

            /// Size in bytes of the parameter block the interface mirrors.
            #[::wasm_bindgen::prelude::wasm_bindgen(js_name = paramByteLength)]
            pub fn param_byte_length(&self) -> usize {
                self.inner.param_byte_len()
            }

            #[::wasm_bindgen::prelude::wasm_bindgen(js_name = adapterInfo)]
            pub fn adapter_info(&self) -> String {
                self.inner.adapter_description()
            }

            #[::wasm_bindgen::prelude::wasm_bindgen(js_name = setViewport)]
            pub fn set_viewport(&mut self, width: u32, height: u32, dpr: f32) {
                self.inner.set_viewport(width, height, dpr);
            }

            #[::wasm_bindgen::prelude::wasm_bindgen(js_name = writeParams)]
            pub fn write_params(&mut self, bytes: &[u8]) {
                self.inner.write_params(bytes);
            }

            pub fn input(&mut self, json: &str) {
                self.inner.input(json);
            }

            pub fn command(&mut self, name: &str, args: &str) {
                self.inner.command(name, args);
            }

            pub fn frame(&mut self, now_ms: f64) {
                self.inner.frame(now_ms);
            }

            pub fn stats(&self) -> String {
                self.inner.stats_json()
            }
        }
    };
}
