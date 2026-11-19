//! Fixture crate for fid-shlint (VIZ.RENDER.FRAME.SHLINT AC-1 to AC-4). Never compiled.
//! Mentions of points.slice(..).map_async(wgpu::MapMode::Read, ..) in comments do not count.

mod frame;

pub use frame::Frame;

/// Reads the ID attachment back for picking: a declared ID target.
pub fn read_ids<'a>(id_readback: &'a wgpu::Buffer) {
    id_readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
}

/// Reads the resolved depth back: `depth_out` is a depth target in the manifest.
pub fn read_depth(f: &Frame) {
    f.depth_out.map_async(wgpu::MapMode::Read, 0..64, |_| {});
}

/// Mapping input data for writing is an upload, not a readback.
pub fn upload(points: &wgpu::Buffer) {
    points.slice(..).map_async(wgpu::MapMode::Write, |_| {});
}

/* A block comment /* with a nested one */ that says points.map_async(Read). */
pub const NOTE: &str = "points.slice(..).map_async(wgpu::MapMode::Read, cb)";
pub const RAW: &str = r#"camera.map_async(MapMode::Read) "quoted" "#;
pub const QUOTE: char = '"';
pub const BYTES: &[u8] = b"points.map_async(";

#[cfg(test)]
mod tests {
    #[test]
    fn reads_input_back_in_a_test() {
        let points: &wgpu::Buffer = todo!();
        points.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    }
}
