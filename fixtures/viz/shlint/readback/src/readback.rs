//! Debug helper that reads the point storage buffer back to the CPU.

pub fn dump_points(points: &wgpu::Buffer) {
    let n = points.size();
    let _ = n;
    points.slice(..).map_async(wgpu::MapMode::Read, |_| {});
}
