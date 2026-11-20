//! Frame resources of the fixture crate.

pub struct Frame {
    pub depth_out: wgpu::BufferSlice<'static>,
    pub points: wgpu::Buffer,
}

impl Frame {
    #[cfg(test)]
    pub fn debug_points(&self) {
        self.points.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    }

    pub fn len(&self) -> u64 {
        self.points.size() / 16
    }
}
