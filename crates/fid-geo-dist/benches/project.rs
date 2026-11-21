//! GEO.DIST.BATCH AC-6 (advisory until the reference machine exists): times the batch
//! projection of a seeded point set onto a subdivided icosphere.
//!
//! Run: `cargo bench -p fid-geo-dist --bench project`. Environment: `FID_BENCH_POINTS`
//! (default 10 000 000), `FID_BENCH_SUB` (subdivisions per icosahedron edge, default 224, about
//! 1 000 000 triangles) and `FID_BENCH_THREADS` (default: available parallelism). The target
//! is under 10 s (REQ-GEO.DIST.BATCH-005). This is not a test: it prints and never fails on time.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_dist::{project_batch, summary, BatchConfig, MeshTarget, ProjectionOptions, SignTarget};
use fid_geo_index::PrimitiveBvh;
use fid_geo_types::{FrameRef, Mesh, PointSet};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn env(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn sqrt(x: f64) -> f64 {
    fid_num::f::sqrt(x).unwrap()
}

struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn uni(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * ((self.next() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0))
    }
}

fn sphere(n: u64) -> (Vec<[f64; 3]>, Vec<[u64; 3]>) {
    let phi = (1.0 + sqrt(5.0)) / 2.0;
    let v = [
        [-1.0, phi, 0.0],
        [1.0, phi, 0.0],
        [-1.0, -phi, 0.0],
        [1.0, -phi, 0.0],
        [0.0, -1.0, phi],
        [0.0, 1.0, phi],
        [0.0, -1.0, -phi],
        [0.0, 1.0, -phi],
        [phi, 0.0, -1.0],
        [phi, 0.0, 1.0],
        [-phi, 0.0, -1.0],
        [-phi, 0.0, 1.0],
    ];
    let f: [[usize; 3]; 20] = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let fn_ = n as f64;
    let (mut verts, mut tris) = (Vec::new(), Vec::new());
    for face in f {
        let (a, b, c): ([f64; 3], [f64; 3], [f64; 3]) = (v[face[0]], v[face[1]], v[face[2]]);
        let base = verts.len() as u64;
        for i in 0..=n {
            for j in 0..=(n - i) {
                let (wa, wb, wc) = ((n - i - j) as f64, i as f64, j as f64);
                let p: [f64; 3] = std::array::from_fn(|k| ((a[k] * wa + b[k] * wb) + c[k] * wc) / fn_);
                let len = sqrt((p[0] * p[0] + p[1] * p[1]) + p[2] * p[2]);
                verts.push([p[0] / len, p[1] / len, p[2] / len]);
            }
        }
        let idx = |i: u64, j: u64| base + i * (n + 1) - (i * (i.wrapping_sub(1))) / 2 + j;
        for i in 0..n {
            for j in 0..(n - i) {
                tris.push([idx(i, j), idx(i + 1, j), idx(i, j + 1)]);
                if j + 1 < n - i {
                    tris.push([idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)]);
                }
            }
        }
    }
    (verts, tris)
}

fn main() {
    let points = env("FID_BENCH_POINTS", 10_000_000);
    let sub = env("FID_BENCH_SUB", 224) as u64;
    let threads = env("FID_BENCH_THREADS", std::thread::available_parallelism().map_or(1, |n| n.get())).clamp(1, 256);
    let frame = FrameRef::from_bytes([3; 16]);
    let (verts, tris) = sphere(sub);
    let mesh = Mesh::from_points(frame, &verts, &tris).unwrap();
    let payload: Vec<u64> = (0..tris.len() as u64).collect();
    let bvh = PrimitiveBvh::build(&mesh, &payload, threads).unwrap();
    let target = SignTarget::new(MeshTarget::new(&mesh, &bvh).unwrap()).unwrap();
    let mut g = SplitMix(51);
    let pts: Vec<[f64; 3]> = (0..points)
        .map(|_| {
            let d = [g.uni(-1.0, 1.0), g.uni(-1.0, 1.0), g.uni(-1.0, 1.0)];
            let s = g.uni(0.97, 1.03) / sqrt((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]);
            [d[0] * s, d[1] * s, d[2] * s]
        })
        .collect();
    let set = PointSet::from_points(frame, &pts).unwrap();
    let config = BatchConfig { threads, ..BatchConfig::default() };
    let start = Instant::now();
    let r = project_batch(&set, &target, &ProjectionOptions::default(), &config, &AtomicBool::new(false)).unwrap();
    let secs = start.elapsed().as_secs_f64();
    let s = summary(&r).unwrap();
    println!(
        "GEO.DIST.BATCH AC-6: {points} points onto {} triangles, {threads} threads: {secs:.2} s ({}; target < 10 s, advisory); hits {}",
        tris.len(),
        if secs < 10.0 { "within" } else { "over" },
        s.hits
    );
}
