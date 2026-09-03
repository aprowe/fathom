//! The GPU handles an app is given.
//!
//! Deliberately thin: apps use `wgpu` types directly. The point of this struct is that
//! it is *the same* on both targets, so no app code branches on where it is running.

/// Device, queue, and the format the app should render into.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The surface format of the final target. Render pipelines should use this.
    pub format: wgpu::TextureFormat,
    pub adapter_info: wgpu::AdapterInfo,
}

impl Gpu {
    /// A one-line description of the adapter, for the interface's status readout.
    pub fn describe_adapter(&self) -> String {
        format!(
            "{} ({:?}, {:?})",
            self.adapter_info.name, self.adapter_info.device_type, self.adapter_info.backend
        )
    }
}
