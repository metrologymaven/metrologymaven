// Integration tests are test code: reading input data back here is allowed.
#[test]
fn points_round_trip() {
    let points: wgpu::Buffer = todo!();
    points.slice(..).map_async(wgpu::MapMode::Read, |_| {});
}
