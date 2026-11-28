//! The named fixture set `fixtures/scan/edge/est_{sharp,round1,chamfer05,burr02}_{onesided,wrapped}`
//! (seeds 61 to 68) and its manifests.
//!
//! Every named fixture shares one sheet: thickness 1 mm, trim offset 0.5 mm, a straight 2 mm
//! nominal edge, 50 micrometre spacing, top extent 2 mm, bottom extent 1 mm, four stations and
//! the 30-degree tangency threshold. The profiles are sharp, round `r = 1 mm`, chamfer
//! `c = 0.5 mm` and burr `0.2 mm x 0.2 mm`. Each fixture has two variants with its seed: clean
//! (`sigma = 0`) and noisy (`sigma = NOISY_SIGMA_M`).
//!
//! The point data is not committed: a manifest pins the `GEO.TYPES` content digest of each
//! variant's cloud and mesh, and [`named_fixture`] regenerates the bytes. A manifest is one
//! canonical JSON text (keys in a fixed order, no insignificant whitespace except one line
//! feed after each top-level member and station; every float a string of the 16 lowercase
//! hexadecimal digits of its IEEE-754 bits, with `-0` written as `0`).

use core::f64::consts::FRAC_PI_6;
use fid_geo_types::digest::{digest_mesh, digest_point_cloud};

use super::{
    edge_fixture, Coverage, EdgeFixture, EdgeFixtureSpec, EdgeProfile, EfixError, NominalPath, StationFrame, GENERATOR,
};

/// The manifest format identifier.
pub const MANIFEST_FORMAT: &str = "fid.scan.edge.efix-manifest@1";
/// The standard deviation of every named fixture's noisy variant.
pub const NOISY_SIGMA_M: f64 = 10.0e-6;

/// A named fixture: its directory name under `fixtures/scan/edge/`, its spec (clean variant)
/// and its seed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NamedFixture {
    pub name: &'static str,
    pub spec: EdgeFixtureSpec,
    pub seed: u64,
}

const PROFILES: [EdgeProfile; 4] = [
    EdgeProfile::Sharp,
    EdgeProfile::Round { radius_m: 1.0e-3 },
    EdgeProfile::Chamfer { width_m: 0.5e-3 },
    EdgeProfile::Burr { height_m: 0.2e-3, width_m: 0.2e-3 },
];

const NAMES: [&str; 8] = [
    "est_sharp_onesided",
    "est_sharp_wrapped",
    "est_round1_onesided",
    "est_round1_wrapped",
    "est_chamfer05_onesided",
    "est_chamfer05_wrapped",
    "est_burr02_onesided",
    "est_burr02_wrapped",
];

/// The eight named fixtures, in seed order (61 to 68).
pub fn named_fixtures() -> Vec<NamedFixture> {
    let mut out = Vec::with_capacity(NAMES.len());
    let mut names = NAMES.iter();
    let mut seed = 61;
    for profile in PROFILES {
        for coverage in [Coverage::OneSided, Coverage::Wrapped] {
            let Some(name) = names.next() else { break };
            out.push(NamedFixture {
                name,
                spec: EdgeFixtureSpec {
                    profile,
                    thickness_m: 1.0e-3,
                    trim_offset_m: 0.5e-3,
                    coverage,
                    nominal_edge: NominalPath::Straight { length_m: 2.0e-3 },
                    spacing_m: 50.0e-6,
                    sigma_m: 0.0,
                    top_extent_m: 2.0e-3,
                    bottom_extent_m: 1.0e-3,
                    station_count: 4,
                    threshold_rad: FRAC_PI_6,
                },
                seed,
            });
            seed += 1;
        }
    }
    out
}

/// The named fixture `name` (for example `est_round1_onesided`) with noise `sigma_m`
/// (0 for the clean variant, [`NOISY_SIGMA_M`] for the noisy one).
pub fn named_fixture(name: &str, sigma_m: f64) -> Result<EdgeFixture, EfixError> {
    let Some(f) = named_fixtures().into_iter().find(|f| f.name == name) else {
        return Err(EfixError::OutOfRange { field: "name", reason: "not a named fixture" });
    };
    edge_fixture(&EdgeFixtureSpec { sigma_m, ..f.spec }, f.seed)
}

fn hex(x: f64) -> String {
    let x = if x == 0.0 { 0.0 } else { x };
    format!("\"{:016x}\"", x.to_bits())
}

fn vec3(v: [f64; 3]) -> String {
    format!("[{},{},{}]", hex(v[0]), hex(v[1]), hex(v[2]))
}

fn vec2(v: [f64; 2]) -> String {
    format!("[{},{}]", hex(v[0]), hex(v[1]))
}

fn profile_json(p: EdgeProfile) -> String {
    match p {
        EdgeProfile::Sharp => "{\"kind\":\"sharp\"}".to_string(),
        EdgeProfile::Round { radius_m } => format!("{{\"kind\":\"round\",\"radius_m\":{}}}", hex(radius_m)),
        EdgeProfile::Chamfer { width_m } => format!("{{\"kind\":\"chamfer\",\"width_m\":{}}}", hex(width_m)),
        EdgeProfile::Burr { height_m, width_m } => {
            format!("{{\"kind\":\"burr\",\"height_m\":{},\"width_m\":{}}}", hex(height_m), hex(width_m))
        }
    }
}

fn path_json(p: NominalPath) -> String {
    match p {
        NominalPath::Straight { length_m } => format!("{{\"kind\":\"straight\",\"length_m\":{}}}", hex(length_m)),
        NominalPath::Arc { radius_m, length_m } => {
            format!("{{\"kind\":\"arc\",\"radius_m\":{},\"length_m\":{}}}", hex(radius_m), hex(length_m))
        }
    }
}

fn spec_json(s: &EdgeFixtureSpec) -> String {
    let coverage = match s.coverage {
        Coverage::OneSided => "onesided",
        Coverage::Wrapped => "wrapped",
    };
    format!(
        "{{\"profile\":{},\"thickness_m\":{},\"trim_offset_m\":{},\"coverage\":\"{coverage}\",\"nominal_edge\":{},\
         \"spacing_m\":{},\"top_extent_m\":{},\"bottom_extent_m\":{},\"station_count\":{},\"threshold_rad\":{}}}",
        profile_json(s.profile),
        hex(s.thickness_m),
        hex(s.trim_offset_m),
        path_json(s.nominal_edge),
        hex(s.spacing_m),
        hex(s.top_extent_m),
        hex(s.bottom_extent_m),
        s.station_count,
        hex(s.threshold_rad),
    )
}

fn variant_json(f: &EdgeFixture) -> String {
    let (rows, cols) = f.grid();
    format!(
        "{{\"sigma_m\":{},\"rows\":{rows},\"cols\":{cols},\"points\":{},\"triangles\":{},\
         \"cloud_digest\":\"sha256:{}\",\"mesh_digest\":\"sha256:{}\"}}",
        hex(f.spec().sigma_m),
        f.cloud().len(),
        f.mesh().triangle_count(),
        digest_point_cloud(f.cloud()),
        digest_mesh(f.mesh()),
    )
}

fn frame_json(fr: &StationFrame) -> String {
    format!("\"p\":{},\"t\":{},\"n\":{},\"b\":{}", vec3(fr.p), vec3(fr.t), vec3(fr.n), vec3(fr.b))
}

/// The manifest of named fixture `f`: generator, seed, spec, the clean and noisy variants'
/// sizes and content digests, and the per-station truths. Generates both variants.
pub fn fixture_manifest(f: &NamedFixture) -> Result<String, EfixError> {
    let clean = edge_fixture(&EdgeFixtureSpec { sigma_m: 0.0, ..f.spec }, f.seed)?;
    let noisy = edge_fixture(&EdgeFixtureSpec { sigma_m: NOISY_SIGMA_M, ..f.spec }, f.seed)?;
    let mut out = format!(
        "{{\"format\":\"{MANIFEST_FORMAT}\",\n\"generator\":\"{GENERATOR}\",\n\"name\":\"{}\",\n\
         \"data\":\"synthetic\",\n\"license\":\"CC0-1.0\",\n\"seed\":{},\n\"spec\":{},\n\"variants\":[{},{}],\n\
         \"stations\":[",
        f.name,
        f.seed,
        spec_json(&clean.spec),
        variant_json(&clean),
        variant_json(&noisy),
    );
    for (i, st) in clean.truths().iter().enumerate() {
        let tang = match st.tangency_uv() {
            Some(uv) => vec2(uv),
            None => "null".to_string(),
        };
        let sep = if i == 0 { "\n" } else { ",\n" };
        out.push_str(&format!(
            "{sep}{{\"index\":{},\"s_m\":{},{},\"trim_offset_m\":{},\"tangency_uv\":{tang},\"silhouette_uv\":{}}}",
            st.index(),
            hex(st.s_m()),
            frame_json(&st.frame()),
            hex(st.trim_offset_m()),
            vec2(st.silhouette_uv()),
        ));
    }
    out.push_str("]}\n");
    Ok(out)
}
