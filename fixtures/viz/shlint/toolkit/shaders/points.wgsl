// Points pass of the fixture crate: kernel data in, colour and ID out.
struct Camera {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> points: array<vec4<f32>>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) id: u32,
}

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    var r: VsOut;
    r.pos = camera.view_proj * points[i];
    r.id = i;
    return r;
}

struct FsOut {
    @location(0) colour: vec4<f32>,
    @location(1) id: u32,
}

@fragment
fn fs_main(v: VsOut) -> FsOut {
    var o: FsOut;
    o.colour = vec4<f32>(1.0, 1.0, 1.0, 1.0);
    o.id = v.id;
    return o;
}
