// Overlay pass that nobody added to shaders/manifest.toml.
@group(0) @binding(0) var<uniform> tint: vec4<f32>;

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return tint;
}
