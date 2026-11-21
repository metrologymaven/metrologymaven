// Golden cube draw pass (VIZ.RENDER.FRAME.GRAPH AC-1, shared with VIZ.RENDER.HEADLESS). Test
// data, not product code: vertices arrive in clip space (fixture cube.txt), so the shader maps
// them to pixels and writes the triangle's render ID and primitive index to the ID target.
// No bindings.

struct VsIn {
    @location(0) pos: vec4<f32>,
    @location(1) ids: vec2<u32>,
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) ids: vec2<u32>,
}

@vertex
fn vs(v: VsIn) -> VsOut {
    var o: VsOut;
    o.pos = v.pos;
    o.ids = v.ids;
    return o;
}

struct FsOut {
    @location(0) colour: vec4<f32>,
    @location(1) id: vec2<u32>,
}

@fragment
fn fs(i: VsOut) -> FsOut {
    var o: FsOut;
    o.colour = vec4<f32>(0.5, 0.5, 0.5, 1.0);
    o.id = i.ids;
    return o;
}
