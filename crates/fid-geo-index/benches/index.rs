//! Spatial-index performance and memory benchmark harness (spec node `GEO.INDEX.BENCH`).
//!
//! Measures `PointIndex` (`GEO.INDEX.KDTREE`) and `PrimitiveBvh` (`GEO.INDEX.BVH`) over two
//! seeded synthetic distributions (`uniform` and anisotropic `scan_strip`) generated with the
//! `fid-num` Philox4x32-10 counter RNG (`KRN.NUM.RNG`). Timings use `std::time::Instant`
//! outside the kernel library crate; memory overhead is reported from each index's own
//! exact `memory_bytes()` accounting.
//!
//! # CLI usage
//! ```text
//! cargo bench -p fid-geo-index --bench index -- \
//!     [--tier small|medium|large|all] \
//!     [--profile <id>] [--threads <n>] [--seed <u64>] \
//!     [--n <points>] [--queries <count>] [--bvh-triangles <count>] [--bvh-queries <count>] \
//!     [--slowdown-factor <factor>] [--inject-slowdown-ns <ns>] [--latency-budget-ns <ns>] \
//!     [--out <path>] [--json]
//! ```
//!
//! # Pass / fail rules
//! - Fails non-zero if any point-index tier exceeds `32.0` bytes per point (REQ-001).
//! - Under `--profile reference`, marks `"regression": true` and exits non-zero if any
//!   tier's median `k = 16` latency exceeds the budget (`10_000 ns` = 10 us by default,
//!   REQ-003, AC-3).
//! - Always emits the JSON report before returning a non-zero exit code so CI jobs and
//!   regression tests can inspect the measured figures.

use std::fmt::Write as FmtWrite;
use std::hint::black_box;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use fid_geo_index::{PointIndex, PrimitiveBvh, MAX_POINTS, MAX_TRIANGLES};
use fid_geo_types::{FrameRef, PointBuffer};
use fid_num::rng::{Address, Cursor};

const DEFAULT_SEED: u64 = 31;
const DEFAULT_PROFILE: &str = "ci-default";
const REFERENCE_PROFILE: &str = "reference";
const MAX_BYTES_PER_POINT: f64 = 32.0;
const DEFAULT_LATENCY_BUDGET_NS: u64 = 10_000;
const DEFAULT_QUERIES: usize = 100_000;
const DEFAULT_BVH_QUERIES: usize = 10_000;
const KNN_K: usize = 16;
const FRAME: FrameRef = FrameRef::from_bytes([1; 16]);

const SMALL_N: usize = 1_000_000;
const MEDIUM_N: usize = 10_000_000;
const LARGE_N: usize = 100_000_000;

const MAX_CLI_ARGS: usize = 64;
const MAX_QUERIES: usize = 10_000_000;
const MAX_SLOWDOWN_FACTOR: u32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TierKind {
    Small,
    Medium,
    Large,
    Custom,
}

impl TierKind {
    fn as_str(self) -> &'static str {
        match self {
            TierKind::Small => "small",
            TierKind::Medium => "medium",
            TierKind::Large => "large",
            TierKind::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Distribution {
    Uniform,
    ScanStrip,
}

impl Distribution {
    fn as_str(self) -> &'static str {
        match self {
            Distribution::Uniform => "uniform",
            Distribution::ScanStrip => "scan_strip",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) tiers: Vec<(TierKind, usize)>,
    pub(crate) queries: usize,
    bvh_triangles_override: Option<usize>,
    bvh_queries: usize,
    pub(crate) seed: u64,
    threads: usize,
    profile: String,
    json: bool,
    out: Option<PathBuf>,
    slowdown_factor: u32,
    inject_slowdown_ns: u64,
    latency_budget_ns: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct Seeds {
    base: u64,
    uniform_points: u64,
    uniform_queries: u64,
    scan_strip_points: u64,
    scan_strip_queries: u64,
    bvh_uniform: u64,
    bvh_scan_strip: u64,
}

impl Seeds {
    fn from_base(base: u64) -> Self {
        Seeds {
            base,
            uniform_points: base,
            uniform_queries: base.wrapping_add(1),
            scan_strip_points: base.wrapping_add(2),
            scan_strip_queries: base.wrapping_add(3),
            bvh_uniform: base.wrapping_add(4),
            bvh_scan_strip: base.wrapping_add(5),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BvhTierResult {
    pub(crate) triangles: usize,
    pub(crate) memory_bytes: u64,
    bytes_per_triangle: f64,
    build_ms: f64,
    pub(crate) nearest_box_median_ns: u64,
    pub(crate) nearest_box_p99_ns: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct TierResult {
    tier: TierKind,
    distribution: Distribution,
    pub(crate) n: usize,
    queries: usize,
    memory_bytes: u64,
    pub(crate) bytes_per_point: f64,
    build_ms: f64,
    pub(crate) knn16_median_ns: u64,
    pub(crate) knn16_p99_ns: u64,
    pub(crate) bvh: Option<BvhTierResult>,
}

#[derive(Debug, Clone)]
pub(crate) struct BenchReport {
    profile: String,
    threads: usize,
    version: String,
    seeds: Seeds,
    pub(crate) tiers: Vec<TierResult>,
    pub(crate) regression: bool,
}

fn parse_usize(flag: &str, raw: &str) -> Result<usize, String> {
    raw.parse::<usize>().map_err(|_| format!("{flag}: invalid unsigned integer {raw:?}"))
}

fn parse_u64(flag: &str, raw: &str) -> Result<u64, String> {
    raw.parse::<u64>().map_err(|_| format!("{flag}: invalid u64 {raw:?}"))
}

fn parse_u32(flag: &str, raw: &str) -> Result<u32, String> {
    raw.parse::<u32>().map_err(|_| format!("{flag}: invalid u32 {raw:?}"))
}

pub(crate) fn parse_args(args: &[String]) -> Result<Config, String> {
    if args.len() > MAX_CLI_ARGS {
        return Err("too many CLI arguments".to_string());
    }
    let mut tier_arg: Option<String> = None;
    let mut n_override: Option<usize> = None;
    let mut queries = DEFAULT_QUERIES;
    let mut bvh_triangles_override: Option<usize> = None;
    let mut bvh_queries = DEFAULT_BVH_QUERIES;
    let mut seed = DEFAULT_SEED;
    let mut threads = 1usize;
    let mut profile = DEFAULT_PROFILE.to_string();
    let mut json = false;
    let mut out: Option<PathBuf> = None;
    let mut slowdown_factor = 1u32;
    let mut inject_slowdown_ns = 0u64;
    let mut latency_budget_ns = DEFAULT_LATENCY_BUDGET_NS;

    let mut i = 0usize;
    while i < args.len() {
        let arg = args.get(i).map(String::as_str).unwrap_or("");
        match arg {
            "--bench" => {
                // Passed automatically by `cargo bench`.
            }
            "--json" => json = true,
            "--tier" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--tier requires a value".to_string())?;
                tier_arg = Some(val.clone());
            }
            "--n" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--n requires a value".to_string())?;
                let n = parse_usize("--n", val)?;
                if n < KNN_K || (n as u64) > MAX_POINTS {
                    return Err(format!("--n must be in {KNN_K}..={MAX_POINTS}"));
                }
                n_override = Some(n);
            }
            "--queries" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--queries requires a value".to_string())?;
                let q = parse_usize("--queries", val)?;
                if q == 0 || q > MAX_QUERIES {
                    return Err(format!("--queries must be in 1..={MAX_QUERIES}"));
                }
                queries = q;
            }
            "--bvh-triangles" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--bvh-triangles requires a value".to_string())?;
                let t = parse_usize("--bvh-triangles", val)?;
                if (t as u64) > MAX_TRIANGLES {
                    return Err(format!("--bvh-triangles must be <= {MAX_TRIANGLES}"));
                }
                bvh_triangles_override = Some(t);
            }
            "--bvh-queries" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--bvh-queries requires a value".to_string())?;
                let q = parse_usize("--bvh-queries", val)?;
                if q == 0 || q > MAX_QUERIES {
                    return Err(format!("--bvh-queries must be in 1..={MAX_QUERIES}"));
                }
                bvh_queries = q;
            }
            "--seed" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--seed requires a value".to_string())?;
                seed = parse_u64("--seed", val)?;
            }
            "--threads" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--threads requires a value".to_string())?;
                threads = parse_usize("--threads", val)?;
                if threads == 0 {
                    return Err("--threads must be >= 1".to_string());
                }
            }
            "--profile" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--profile requires a value".to_string())?;
                if val.is_empty() {
                    return Err("--profile must not be empty".to_string());
                }
                profile = val.clone();
            }
            "--out" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--out requires a value".to_string())?;
                if val.is_empty() {
                    return Err("--out must not be empty".to_string());
                }
                out = Some(PathBuf::from(val));
            }
            "--slowdown-factor" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--slowdown-factor requires a value".to_string())?;
                slowdown_factor = parse_u32("--slowdown-factor", val)?;
                if slowdown_factor == 0 || slowdown_factor > MAX_SLOWDOWN_FACTOR {
                    return Err(format!("--slowdown-factor must be in 1..={MAX_SLOWDOWN_FACTOR}"));
                }
            }
            "--inject-slowdown-ns" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--inject-slowdown-ns requires a value".to_string())?;
                inject_slowdown_ns = parse_u64("--inject-slowdown-ns", val)?;
            }
            "--latency-budget-ns" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| "--latency-budget-ns requires a value".to_string())?;
                latency_budget_ns = parse_u64("--latency-budget-ns", val)?;
            }
            other => return Err(format!("unknown option {other:?}")),
        }
        i += 1;
    }

    let tiers = if let Some(n) = n_override {
        vec![(TierKind::Custom, n)]
    } else {
        match tier_arg.as_deref().unwrap_or("small") {
            "small" => vec![(TierKind::Small, SMALL_N)],
            "medium" => vec![(TierKind::Medium, MEDIUM_N)],
            "large" => vec![(TierKind::Large, LARGE_N)],
            "all" => vec![(TierKind::Small, SMALL_N), (TierKind::Medium, MEDIUM_N), (TierKind::Large, LARGE_N)],
            other => return Err(format!("unknown tier {other:?}; expected small, medium, large, or all")),
        }
    };

    Ok(Config {
        tiers,
        queries,
        bvh_triangles_override,
        bvh_queries,
        seed,
        threads,
        profile,
        json,
        out,
        slowdown_factor,
        inject_slowdown_ns,
        latency_budget_ns,
    })
}

fn draw_point(dist: Distribution, cur: &mut Cursor) -> [f64; 3] {
    let u = cur.uniform_f64();
    let v = cur.uniform_f64();
    let w = cur.uniform_f64();
    match dist {
        Distribution::Uniform => [u, v, w],
        Distribution::ScanStrip => {
            // Anisotropic laser-scan strip: 1.0 m along X, 0.05 m swath across Y, ~2 mm ripple in Z.
            let ripple = (u * 8.0) - (u * 8.0).floor() - 0.5;
            [u, v * 0.05, (w - 0.5) * 0.002 + 0.0005 * ripple]
        }
    }
}

fn generate_points(dist: Distribution, n: usize, seed: u64, stream: u64) -> Result<PointBuffer, String> {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut zs = Vec::new();
    xs.try_reserve_exact(n).map_err(|_| format!("allocation failed for {n} x coordinates"))?;
    ys.try_reserve_exact(n).map_err(|_| format!("allocation failed for {n} y coordinates"))?;
    zs.try_reserve_exact(n).map_err(|_| format!("allocation failed for {n} z coordinates"))?;

    let mut cur = Cursor::new(Address { seed, stream }, 0);
    for _ in 0..n {
        let [x, y, z] = draw_point(dist, &mut cur);
        xs.push(x);
        ys.push(y);
        zs.push(z);
    }
    Ok(PointBuffer::new(xs, ys, zs))
}

fn generate_queries(
    dist: Distribution,
    pts: &PointBuffer,
    count: usize,
    seed: u64,
    stream: u64,
) -> Result<Vec<[f64; 3]>, String> {
    let mut qs = Vec::new();
    qs.try_reserve_exact(count).map_err(|_| format!("allocation failed for {count} queries"))?;
    let n = pts.len();
    let mut cur = Cursor::new(Address { seed, stream }, 0);
    for i in 0..count {
        if i % 5 == 0 && n > 0 {
            let idx = cur.below(n).map_err(|e| e.to_string())?;
            let idx_us = usize::try_from(idx).unwrap_or(0);
            if let (Some(&x), Some(&y), Some(&z)) = (pts.x.get(idx_us), pts.y.get(idx_us), pts.z.get(idx_us)) {
                qs.push([x, y, z]);
                continue;
            }
        }
        let u = cur.uniform_f64();
        let v = cur.uniform_f64();
        let w = cur.uniform_f64();
        let q = match dist {
            Distribution::Uniform => [u * 1.1 - 0.05, v * 1.1 - 0.05, w * 1.1 - 0.05],
            Distribution::ScanStrip => [u * 1.02 - 0.01, v * 0.06 - 0.005, (w - 0.5) * 0.004],
        };
        qs.push(q);
    }
    Ok(qs)
}

fn percentile(sorted: &[u64], num: usize, den: usize) -> u64 {
    if sorted.is_empty() || den == 0 {
        return 0;
    }
    let last = sorted.len().saturating_sub(1);
    let idx = (sorted.len().saturating_mul(num) / den).min(last);
    sorted.get(idx).copied().unwrap_or(0)
}

fn bench_bvh(
    dist: Distribution,
    tri_count: usize,
    query_count: usize,
    seed: u64,
    threads: usize,
) -> Result<BvhTierResult, String> {
    let mut tris = Vec::new();
    tris.try_reserve_exact(tri_count).map_err(|_| format!("allocation failed for {tri_count} triangles"))?;
    let mut payload = Vec::new();
    payload.try_reserve_exact(tri_count).map_err(|_| format!("allocation failed for {tri_count} payload IDs"))?;

    let edge = match dist {
        Distribution::Uniform => 0.002,
        Distribution::ScanStrip => 0.0005,
    };
    let mut cur = Cursor::new(Address { seed, stream: 10 }, 0);
    for idx in 0..tri_count {
        let [x, y, z] = draw_point(dist, &mut cur);
        tris.push([[x, y, z], [x + edge, y, z], [x, y + edge, z + edge * 0.25]]);
        payload.push(idx as u64);
    }

    let build_start = Instant::now();
    let bvh = PrimitiveBvh::build_triangles(FRAME, &tris, &payload, threads)
        .map_err(|e| format!("PrimitiveBvh::build_triangles failed: {e}"))?;
    let build_ms = build_start.elapsed().as_secs_f64() * 1_000.0;
    let memory_bytes = bvh.memory_bytes();
    let bytes_per_triangle = memory_bytes as f64 / (tri_count as f64);

    let mut q_cur = Cursor::new(Address { seed: seed.wrapping_add(100), stream: 11 }, 0);
    let mut latencies = Vec::new();
    latencies
        .try_reserve_exact(query_count)
        .map_err(|_| format!("allocation failed for {query_count} BVH latencies"))?;
    for _ in 0..query_count {
        let q = draw_point(dist, &mut q_cur);
        let t0 = Instant::now();
        let nb = bvh.nearest_box(q).map_err(|e| format!("PrimitiveBvh::nearest_box failed: {e}"))?;
        black_box(nb);
        let dt = t0.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        latencies.push(dt);
    }
    latencies.sort_unstable();

    Ok(BvhTierResult {
        triangles: tri_count,
        memory_bytes,
        bytes_per_triangle,
        build_ms,
        nearest_box_median_ns: percentile(&latencies, 1, 2),
        nearest_box_p99_ns: percentile(&latencies, 99, 100),
    })
}

fn run_tier(tier: TierKind, dist: Distribution, n: usize, cfg: &Config, seeds: &Seeds) -> Result<TierResult, String> {
    let (pt_seed, q_seed, bvh_seed, stream_base) = match dist {
        Distribution::Uniform => (seeds.uniform_points, seeds.uniform_queries, seeds.bvh_uniform, 1u64),
        Distribution::ScanStrip => (seeds.scan_strip_points, seeds.scan_strip_queries, seeds.bvh_scan_strip, 2u64),
    };

    let pts = generate_points(dist, n, pt_seed, stream_base)?;
    let queries = generate_queries(dist, &pts, cfg.queries, q_seed, stream_base.wrapping_add(10))?;

    let build_start = Instant::now();
    let index = PointIndex::build(&pts, cfg.threads).map_err(|e| format!("PointIndex::build failed: {e}"))?;
    let build_ms = build_start.elapsed().as_secs_f64() * 1_000.0;

    let memory_bytes = index.memory_bytes();
    let bytes_per_point = memory_bytes as f64 / (n as f64);

    let mut latencies = Vec::new();
    latencies.try_reserve_exact(queries.len()).map_err(|_| "allocation failed for latency samples".to_string())?;
    for &q in &queries {
        let t0 = Instant::now();
        for _ in 0..cfg.slowdown_factor {
            let res = index.knn(q, KNN_K).map_err(|e| format!("PointIndex::knn failed: {e}"))?;
            black_box(res.len());
            if let Some(first) = res.first() {
                black_box(first.dist2);
            }
        }
        let elapsed_ns = t0.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        latencies.push(elapsed_ns.saturating_add(cfg.inject_slowdown_ns));
    }
    latencies.sort_unstable();

    let knn16_median_ns = percentile(&latencies, 1, 2);
    let knn16_p99_ns = percentile(&latencies, 99, 100);

    // Drop the point cloud and k-d tree before building the BVH to keep peak RSS bounded.
    drop(index);
    drop(pts);
    drop(queries);

    let bvh_triangles = cfg.bvh_triangles_override.unwrap_or_else(|| (n / 10).max(16).min(MAX_TRIANGLES as usize));
    let bvh = if bvh_triangles > 0 {
        let bq = cfg.bvh_queries.min(cfg.queries.max(1));
        Some(bench_bvh(dist, bvh_triangles, bq, bvh_seed, cfg.threads)?)
    } else {
        None
    };

    Ok(TierResult {
        tier,
        distribution: dist,
        n,
        queries: cfg.queries,
        memory_bytes,
        bytes_per_point,
        build_ms,
        knn16_median_ns,
        knn16_p99_ns,
        bvh,
    })
}

fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_f64(out: &mut String, x: f64) -> Result<(), String> {
    if !x.is_finite() {
        return Err("non-finite floating-point measurement".to_string());
    }
    let _ = write!(out, "{x:.6}");
    Ok(())
}

fn render_json(report: &BenchReport) -> Result<String, String> {
    let mut s = String::new();
    s.push_str("{\"profile\":");
    push_json_str(&mut s, &report.profile);
    let _ = write!(s, ",\"regression\":{}", report.regression);
    let _ = write!(
        s,
        ",\"seeds\":{{\"base\":{},\"bvh_scan_strip\":{},\"bvh_uniform\":{},\"scan_strip_points\":{},\"scan_strip_queries\":{},\"uniform_points\":{},\"uniform_queries\":{}}}",
        report.seeds.base,
        report.seeds.bvh_scan_strip,
        report.seeds.bvh_uniform,
        report.seeds.scan_strip_points,
        report.seeds.scan_strip_queries,
        report.seeds.uniform_points,
        report.seeds.uniform_queries
    );
    let _ = write!(s, ",\"threads\":{},\"tiers\":[", report.threads);
    for (idx, t) in report.tiers.iter().enumerate() {
        if idx > 0 {
            s.push(',');
        }
        s.push_str("{\"build_ms\":");
        push_f64(&mut s, t.build_ms)?;
        if let Some(b) = &t.bvh {
            s.push_str(",\"bvh\":{\"build_ms\":");
            push_f64(&mut s, b.build_ms)?;
            s.push_str(",\"bytes_per_triangle\":");
            push_f64(&mut s, b.bytes_per_triangle)?;
            let _ = write!(
                s,
                ",\"memory_bytes\":{},\"nearest_box_median_ns\":{},\"nearest_box_p99_ns\":{},\"triangles\":{}}}",
                b.memory_bytes, b.nearest_box_median_ns, b.nearest_box_p99_ns, b.triangles
            );
        }
        s.push_str(",\"bytes_per_point\":");
        push_f64(&mut s, t.bytes_per_point)?;
        s.push_str(",\"distribution\":");
        push_json_str(&mut s, t.distribution.as_str());
        let _ = write!(
            s,
            ",\"knn16_median_ns\":{},\"knn16_p99_ns\":{},\"memory_bytes\":{},\"n\":{},\"queries\":{},\"tier\":",
            t.knn16_median_ns, t.knn16_p99_ns, t.memory_bytes, t.n, t.queries
        );
        push_json_str(&mut s, t.tier.as_str());
        s.push('}');
    }
    s.push_str("],\"version\":");
    push_json_str(&mut s, &report.version);
    s.push('}');
    Ok(s)
}

fn render_human(report: &BenchReport) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "fid-geo-index {} | profile={} threads={} seed={} regression={}",
        report.version, report.profile, report.threads, report.seeds.base, report.regression
    );
    for t in &report.tiers {
        let _ = writeln!(
            s,
            "  tier={} dist={} n={} bpp={:.3} build={:.2}ms knn16_med={}ns knn16_p99={}ns",
            t.tier.as_str(),
            t.distribution.as_str(),
            t.n,
            t.bytes_per_point,
            t.build_ms,
            t.knn16_median_ns,
            t.knn16_p99_ns
        );
    }
    s
}

pub(crate) fn execute(cfg: &Config) -> Result<(BenchReport, Vec<String>), String> {
    let seeds = Seeds::from_base(cfg.seed);
    let mut tiers = Vec::new();
    for &(tier_kind, n) in &cfg.tiers {
        for dist in [Distribution::Uniform, Distribution::ScanStrip] {
            tiers.push(run_tier(tier_kind, dist, n, cfg, &seeds)?);
        }
    }

    let mut failures = Vec::new();
    let mut latency_regression = false;
    let is_reference = cfg.profile == REFERENCE_PROFILE;

    for t in &tiers {
        if t.bytes_per_point > MAX_BYTES_PER_POINT {
            failures.push(format!(
                "tier {} ({}): bytes_per_point {:.6} exceeds limit {:.1}",
                t.tier.as_str(),
                t.distribution.as_str(),
                t.bytes_per_point,
                MAX_BYTES_PER_POINT
            ));
        }
        if is_reference && t.knn16_median_ns > cfg.latency_budget_ns {
            latency_regression = true;
            failures.push(format!(
                "tier {} ({}): knn16_median_ns {} exceeds reference budget {} ns",
                t.tier.as_str(),
                t.distribution.as_str(),
                t.knn16_median_ns,
                cfg.latency_budget_ns
            ));
        }
    }

    let report = BenchReport {
        profile: cfg.profile.clone(),
        threads: cfg.threads,
        version: env!("CARGO_PKG_VERSION").to_string(),
        seeds,
        tiers,
        regression: latency_regression,
    };
    Ok((report, failures))
}

pub(crate) fn run_cli(raw_args: &[String]) -> (ExitCode, String, String) {
    let cfg = match parse_args(raw_args) {
        Ok(c) => c,
        Err(msg) => return (ExitCode::FAILURE, String::new(), format!("error: {msg}\n")),
    };

    let (report, failures) = match execute(&cfg) {
        Ok(res) => res,
        Err(msg) => return (ExitCode::FAILURE, String::new(), format!("error: {msg}\n")),
    };

    let json = match render_json(&report) {
        Ok(j) => j,
        Err(msg) => return (ExitCode::FAILURE, String::new(), format!("error: {msg}\n")),
    };

    if let Some(path) = &cfg.out {
        if let Err(e) = std::fs::write(path, format!("{json}\n")) {
            return (ExitCode::FAILURE, String::new(), format!("error: failed to write {}: {e}\n", path.display()));
        }
    }

    let stdout = if cfg.json { format!("{json}\n") } else { render_human(&report) };
    let mut stderr = String::new();
    for f in &failures {
        let _ = writeln!(stderr, "FAIL: {f}");
    }

    let code = if failures.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    (code, stdout, stderr)
}

fn main() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let (code, stdout, stderr) = run_cli(&raw_args);
    if !stdout.is_empty() {
        let _ = write!(std::io::stdout(), "{stdout}");
    }
    if !stderr.is_empty() {
        let _ = write!(std::io::stderr(), "{stderr}");
    }
    code
}
