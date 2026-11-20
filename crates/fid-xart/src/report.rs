//! Compares digest tables and writes the report (REQ-001 to REQ-005).
//!
//! - **WASM across hosts (gates).** The reference host is the first matrix host with a WASM
//!   table. Every other WASM table must name the same artifact SHA-256 and the same result
//!   digest for every call; each difference is a failure naming the host, call and algorithm.
//! - **Unavailable hosts (gates).** A matrix host without a WASM table is `not_run`; it is a
//!   failure unless the host is optional. No WASM table at all is a failure.
//! - **Failed hosts (gates).** A host whose job failed leaves a `failed-<host>` marker; each
//!   one is a failure `host_failed: <host>`, optional or not, table or not (defence in depth:
//!   a host that ran and then failed never passes silently).
//! - **Native artifacts (measured only).** Per native table and algorithm, the number of calls
//!   and the number whose result digest equals the reference WASM digest. Never a failure while
//!   `native_gate = measured-only`.
//! - **Patch releases (gates).** With release kind `patch`, the reference WASM table must have
//!   the previous release's call-set digest and the same result digest for every call.
//!
//! Inputs that do not fit together are errors, not failures: tables over different call sets
//! or listing different calls, a host not in the matrix, two WASM tables for one host, two
//! native tables with the same host and artifact label, a patch release without a previous
//! table, or a previous table for any other release kind.
//!
//! The report is canonical JSON (keys in byte order, every value a string, no trailing
//! newline). Hosts appear in matrix order, native tables sorted by host then artifact label,
//! algorithms by ID, so the same inputs in any order give the same bytes (AC-4).

use std::collections::BTreeMap;

use fid_kci::kci::json::Json;

use crate::json::{self, obj, s};
use crate::matrix::Matrix;
use crate::table::{DigestTable, Kind};
use crate::XartError;

/// The `schema` member of a report.
pub const REPORT_SCHEMA: &str = "fid-xart-report@1";
/// The native gate policy this tool applies (changing it is a steering-group decision).
pub const NATIVE_GATE: &str = "measured-only";

/// Release kind of the candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// Not a release (pull requests, nightly runs).
    None,
    Patch,
    Minor,
    Major,
}

impl Release {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Release::None => "none",
            Release::Patch => "patch",
            Release::Minor => "minor",
            Release::Major => "major",
        }
    }

    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<Release> {
        match s {
            "none" => Some(Release::None),
            "patch" => Some(Release::Patch),
            "minor" => Some(Release::Minor),
            "major" => Some(Release::Major),
            _ => None,
        }
    }
}

/// The comparison's verdict and report bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// The failures, in report order; the gate passes when there are none.
    pub failures: Vec<String>,
    /// Canonical report bytes.
    pub report: Vec<u8>,
}

impl Comparison {
    /// True when the gate passes.
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

fn check_inputs(
    matrix: &Matrix,
    tables: &[DigestTable],
    failed: &[String],
    release: Release,
    previous: Option<&DigestTable>,
) -> Result<(), XartError> {
    let bad = |r: String| Err(XartError::Inputs(r));
    let Some(first) = tables.first() else {
        return bad("no tables".to_string());
    };
    for t in tables {
        if t.call_set_digest != first.call_set_digest {
            return bad(format!("{} {} on {}: another call set", t.kind.as_str(), t.artifact, t.host));
        }
        if !t.same_calls(first) {
            return bad(format!("{} {} on {}: rows list other calls", t.kind.as_str(), t.artifact, t.host));
        }
        if matrix.host(&t.host).is_none() {
            return bad(format!("host {} is not in the matrix", t.host));
        }
    }
    if let Some(h) = failed.iter().find(|h| matrix.host(h).is_none()) {
        return bad(format!("failed host {h} is not in the matrix"));
    }
    let mut seen = BTreeMap::new();
    for t in tables {
        let key = match t.kind {
            Kind::Wasm => (Kind::Wasm, t.host.as_str(), ""),
            Kind::Native => (Kind::Native, t.host.as_str(), t.artifact.as_str()),
        };
        if seen.insert(key, ()).is_some() {
            return bad(format!("two {} tables for host {} artifact {}", t.kind.as_str(), t.host, t.artifact));
        }
    }
    match (release, previous) {
        (Release::Patch, None) => bad("a patch release needs the previous release's table".to_string()),
        (Release::Patch, Some(p)) if p.kind != Kind::Wasm => bad("the previous table is not a WASM table".to_string()),
        (Release::Patch, Some(p)) if p.call_set_digest == first.call_set_digest && !p.same_calls(first) => {
            bad("the previous table lists other calls for the same call set".to_string())
        }
        (Release::Patch, Some(_)) | (_, None) => Ok(()),
        (_, Some(_)) => bad("a previous table is compared only for a patch release".to_string()),
    }
}

/// Compares the tables (see the module docs).
pub fn compare(
    matrix: &Matrix,
    tables: &[DigestTable],
    failed: &[String],
    release: Release,
    previous: Option<&DigestTable>,
) -> Result<Comparison, XartError> {
    check_inputs(matrix, tables, failed, release, previous)?;
    let wasm = |host: &str| tables.iter().find(|t| t.kind == Kind::Wasm && t.host == host);
    let reference = matrix.hosts.iter().find_map(|h| wasm(&h.name));
    let mut failures = Vec::new();
    if reference.is_none() {
        failures.push("no_wasm_table".to_string());
    }

    let mut hosts = Vec::new();
    for h in &matrix.hosts {
        let mut entry = vec![
            ("host", s(&h.name)),
            ("optional", s(if h.optional { "true" } else { "false" })),
            ("runner", s(&h.runner)),
        ];
        let (Some(t), Some(r)) = (wasm(&h.name), reference) else {
            entry.push(("status", s("not_run")));
            hosts.push(obj(entry));
            if !h.optional {
                failures.push(format!("host_not_run: {}", h.name));
            }
            continue;
        };
        entry.push(("status", s("run")));
        entry.push(("artifact_sha256", s(t.artifact_sha256.as_str())));
        hosts.push(obj(entry));
        if t.host == r.host {
            continue;
        }
        if t.artifact_sha256 != r.artifact_sha256 {
            failures.push(format!(
                "wasm_artifact_mismatch: host {} ran {}, host {} ran {}",
                t.host,
                t.artifact_sha256.as_str(),
                r.host,
                r.artifact_sha256.as_str()
            ));
        }
        for (a, b) in t.rows.iter().zip(&r.rows) {
            if a.result_digest != b.result_digest {
                failures.push(format!(
                    "wasm_digest_mismatch: host {} call {} ({}) algorithm {} differs from host {}",
                    t.host, a.call, a.name, a.algorithm, r.host
                ));
            }
        }
    }

    for h in &matrix.hosts {
        if failed.contains(&h.name) {
            failures.push(format!("host_failed: {}", h.name));
        }
    }

    let mut natives: Vec<&DigestTable> = tables.iter().filter(|t| t.kind == Kind::Native).collect();
    natives.sort_by(|a, b| (&a.host, &a.artifact).cmp(&(&b.host, &b.artifact)));
    let mut native = Vec::new();
    for t in natives {
        let mut counts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        if let Some(r) = reference {
            for (a, b) in t.rows.iter().zip(&r.rows) {
                let c = counts.entry(a.algorithm.as_str()).or_insert((0, 0));
                c.0 += 1;
                c.1 += usize::from(a.result_digest == b.result_digest);
            }
        }
        let algorithms = counts
            .into_iter()
            .map(|(id, (calls, matches))| {
                obj(vec![("algorithm", s(id)), ("calls", s(&calls.to_string())), ("matches", s(&matches.to_string()))])
            })
            .collect();
        native.push(obj(vec![
            ("algorithms", Json::Arr(algorithms)),
            ("artifact", s(&t.artifact)),
            ("artifact_sha256", s(t.artifact_sha256.as_str())),
            ("host", s(&t.host)),
        ]));
    }

    let mut rel = vec![("kind", s(release.as_str()))];
    let status = match (release, previous) {
        (Release::Patch, Some(p)) => {
            rel.push(("previous_artifact_sha256", s(p.artifact_sha256.as_str())));
            match reference {
                None => "fail",
                Some(r) if p.call_set_digest != r.call_set_digest => {
                    failures.push(format!("patch_call_set_mismatch: previous {}", p.call_set_digest.as_str()));
                    "fail"
                }
                Some(r) => {
                    let before = failures.len();
                    for (a, b) in r.rows.iter().zip(&p.rows) {
                        if a.result_digest != b.result_digest {
                            failures.push(format!(
                                "patch_digest_mismatch: call {} ({}) algorithm {}",
                                a.call, a.name, a.algorithm
                            ));
                        }
                    }
                    if failures.len() == before {
                        "pass"
                    } else {
                        "fail"
                    }
                }
            }
        }
        _ => "not_checked",
    };
    rel.push(("status", s(status)));

    let call_set = tables.first().map(|t| t.call_set_digest.as_str().to_string()).unwrap_or_default();
    let mut top = vec![
        ("call_set_digest", s(&call_set)),
        ("failures", Json::Arr(failures.iter().map(|f| s(f)).collect())),
        ("gate", s(if failures.is_empty() { "pass" } else { "fail" })),
        ("hosts", Json::Arr(hosts)),
        ("native", Json::Arr(native)),
        ("native_gate", s(NATIVE_GATE)),
        ("release", obj(rel)),
        ("schema", s(REPORT_SCHEMA)),
    ];
    if let Some(r) = reference {
        top.push(("wasm_artifact_sha256", s(r.artifact_sha256.as_str())));
    }
    let report = json::write(&obj(top)).map_err(XartError::Inputs)?;
    Ok(Comparison { failures, report })
}
