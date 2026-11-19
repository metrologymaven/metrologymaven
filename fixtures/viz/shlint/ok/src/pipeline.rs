//! Pipeline set-up of the fixture crate: shader text comes only from listed modules.

pub fn points_module(device: &wgpu::Device) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("points"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/points.wgsl").into()),
    })
}

pub fn depth_module(device: &wgpu::Device) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::include_wgsl!("../shaders/depth_resolve.wgsl"))
}
