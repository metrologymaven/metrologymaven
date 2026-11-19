// Copies the depth attachment into a buffer that the picking code reads back.
@group(0) @binding(0) var depth_in: texture_depth_2d;
@group(0) @binding(1) var<storage, read_write> depth_out: array<f32>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let d = textureLoad(depth_in, vec2<i32>(g.xy), 0);
    depth_out[g.y * 64u + g.x] = d;
}
