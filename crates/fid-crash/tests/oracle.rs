//! The synthetic chain and kill plan against the independent Python oracle
//! (`fixtures/core/sto/sql/crash/make_chain.py`), bounded parsing of stored rows, and the
//! mapping of failures to the v3 bus error catalogue.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_crash::chain::{blob_bytes, chain, MAX_ROW_BYTES};
use fid_crash::{CrashFailure, FailureKind, KillInstant, KillPlan, SyntheticRow};

/// REQ-001: every row, its canonical bytes and its hash equal the oracle's.
#[test]
fn chain_matches_the_python_oracle() {
    let fx = fixture();
    assert_eq!(text(field(&fx, "format")), fid_crash::chain::ROW_FORMAT);
    let want = arr(field(&fx, "rows"));
    let got = chain(want.len() as u64);
    assert_eq!(got.len(), 40);
    for (row, w) in got.iter().zip(want) {
        assert_eq!(row.seq, num(field(w, "seq")));
        assert_eq!(String::from_utf8(row.canonical()).unwrap(), text(field(w, "canonical")), "row {}", row.seq);
        assert_eq!(row.hash().as_str(), text(field(w, "hash")), "row {}", row.seq);
        assert_eq!(blob_bytes(row.seq).map_or(0, |b| b.len() as u64), num(field(w, "blob_len")));
    }
    // Each row links to the previous one's hash.
    for pair in got.windows(2) {
        assert_eq!(pair[1].prev_hash, Some(pair[0].hash()));
    }
    assert_eq!(got[0].prev_hash, None);
}

/// REQ-002 / REQ-005: the kill schedule is the oracle's, bounded, and instants round-trip.
#[test]
fn kill_plan_matches_the_oracle() {
    let fx = fixture();
    let kp = field(&fx, "kill_plan");
    let plan = KillPlan { seed: num(field(kp, "seed")), max_delay_us: num(field(kp, "max_delay_us")) };
    for (k, d) in arr(field(kp, "delays_us")).iter().enumerate() {
        assert_eq!(plan.instant(k as u64), KillInstant { kill: k as u64, delay_us: num(d) });
    }
    assert_eq!(fid_crash::plan::splitmix64(0), 0xe220_a839_7b1d_cdaf, "published SplitMix64 vector");
    let other = KillPlan { seed: 2, ..plan };
    assert_ne!(
        (0..16).map(|k| plan.instant(k)).collect::<Vec<_>>(),
        (0..16).map(|k| other.instant(k)).collect::<Vec<_>>()
    );
    for k in 0..10_000 {
        assert!(plan.instant(k).delay_us <= plan.max_delay_us);
    }
    let i = KillInstant { kill: 812, delay_us: 4711 };
    assert_eq!(KillInstant::parse(&i.to_string()), Some(i));
    for bad in ["", "1", "1:", ":2", "a:b", "1:2:3", "-1:2", "99999999999999999999:1"] {
        assert_eq!(KillInstant::parse(bad), None, "{bad:?}");
    }
    let edge = KillPlan { seed: 7, max_delay_us: u64::MAX };
    let _ = edge.instant(u64::MAX);
    assert_eq!(KillPlan { seed: 7, max_delay_us: 0 }.instant(5).delay_us, 0);
    for k in 0..64 {
        let inst = KillInstant { kill: k, delay_us: 0 };
        assert!(!(inst.from_spawn() && inst.from_first_ack()), "kill {k} matched both spawn and first_ack");
    }
    assert!(KillInstant { kill: 0, delay_us: 0 }.from_first_ack());
    assert!(KillInstant { kill: 8, delay_us: 0 }.from_first_ack());
    assert!(KillInstant { kill: 3, delay_us: 0 }.from_spawn());
}

/// Stored rows are accepted only in their exact canonical form; anything else is refused
/// without panicking, and oversized input is refused before parsing.
#[test]
fn row_parsing_is_strict_and_bounded() {
    for row in chain(12) {
        assert_eq!(SyntheticRow::parse(&row.canonical()).unwrap(), row);
    }
    let r = SyntheticRow::new(1, None);
    let canon = String::from_utf8(r.canonical()).unwrap();
    let mut bad: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"null".to_vec(),
        b"[]".to_vec(),
        b"{}".to_vec(),
        vec![0xff, 0xfe, 0x00],
        canon.replace(',', ", ").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":\"01\"").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":\"0\"").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":1").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":\"99999999999999999999\"").into_bytes(),
        canon.replace("\"seq\":\"1\"", "\"seq\":\"1\",\"x\":null").into_bytes(),
        canon.replace("fid-crash-row@1", "fid-crash-row@2").into_bytes(),
        canon.replace("\"prev\":null", "\"prev\":\"sha256:00\"").into_bytes(),
        canon.replace("\"prev\":null", "\"prev\":{\"a\":[[[[]]]]}").into_bytes(),
        format!("{canon}{canon}").into_bytes(),
        "[".repeat(100_000).into_bytes(),
        vec![b' '; MAX_ROW_BYTES + 1],
    ];
    // Byte-for-byte mutations of a real row.
    for i in 0..canon.len() {
        let mut b = canon.clone().into_bytes();
        b[i] ^= 0x01;
        bad.push(b);
    }
    for b in &bad {
        if let Ok(row) = SyntheticRow::parse(b) {
            // A mutation may still be a canonical row (a flipped hex digit), but then it is a
            // different row, which the chain check refuses.
            assert_ne!(row, r, "mutation {:?} parsed as the original", String::from_utf8_lossy(b));
            assert_eq!(row.canonical(), *b);
        }
    }
}

/// Every failure maps to `internal` with a reason of the frozen v3 catalogue, never to a
/// success value.
#[test]
fn failures_map_to_v3_reasons() {
    for kind in [
        FailureKind::ChainBroken,
        FailureKind::AckMissing,
        FailureKind::BlobMissing,
        FailureKind::BlobCorrupt,
        FailureKind::StoreRefused,
    ] {
        let f = CrashFailure::new(kind, "detail").at(1, KillInstant { kill: 3, delay_us: 9 });
        assert_refused(&f, kind.code());
        let shown = f.to_string();
        assert!(shown.starts_with(kind.code()) && shown.contains("seed 1") && shown.contains("instant 3:9"), "{shown}");
        let json = String::from_utf8(f.to_bus_error().to_json().unwrap()).unwrap();
        assert!(json.contains("\"code\":\"internal\""), "{json}");
    }
}

/// The test hooks: fid-store's commit probe and this crate's feature that forwards it, each
/// with the crate whose `[features]` table may declare it.
const HOOKS: [(&str, &str); 2] = [("test-probe", "fid-store"), ("fault-inject", "fid-crash")];

/// The table a TOML line opens, without brackets or quotes (`[[bin]]` gives `bin`).
fn header(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with('[') {
        return None;
    }
    let inner = t.trim_start_matches('[');
    let end = inner.find(']')?;
    Some(inner[..end].trim().replace([' ', '"', '\''], ""))
}

fn is_dev_section(section: &str) -> bool {
    section == "dev-dependencies"
        || section.starts_with("dev-dependencies.")
        || (section.starts_with("target.") && section.contains(".dev-dependencies"))
}

/// Lines of the manifest of crate `owner` that name a test hook outside a dev-dependency
/// table. The only other place allowed is the owning crate's `[features]` table, on the line
/// that declares the hook itself (`test-probe = []`, `fault-inject = ["fid-store/test-probe"]`).
fn violations(text: &str, owner: &str) -> Vec<String> {
    let mut section = String::new();
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(h) = header(line) {
            section = h;
            continue;
        }
        let code = line.split('#').next().unwrap_or("");
        if !HOOKS.iter().any(|(h, _)| code.contains(h)) {
            continue;
        }
        let declaration = section == "features"
            && HOOKS.iter().any(|(h, o)| {
                *o == owner && code.trim_start().strip_prefix(h).is_some_and(|r| r.trim_start().starts_with('='))
            });
        if !(is_dev_section(&section) || declaration) {
            out.push(format!("line {}: [{section}] {}", i + 1, line.trim()));
        }
    }
    out
}

fn manifests(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if p.is_dir() {
            if name != "target" && !name.starts_with('.') {
                manifests(&p, out);
            }
        } else if name == "Cargo.toml" {
            out.push(p);
        }
    }
}

/// Test hooks stay out of normal builds: no manifest of the workspace (the root with its
/// `[workspace.dependencies]`, every crate, nested manifests such as `fuzz/`, vendored
/// crates) enables `fault-inject` or fid-store's `test-probe` outside `[dev-dependencies]`,
/// including through another crate's `[features]`. CI also checks the resolved release
/// graph (`cargo tree -e normal,build -i fid-store`, rust.yml).
#[test]
fn fault_injection_is_test_only() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![root.join("Cargo.toml")];
    manifests(&root.join("crates"), &mut files);
    manifests(&root.join("vendor"), &mut files);
    assert!(files.len() > 50, "found only {} manifests", files.len());
    assert!(files.iter().any(|f| f.parent().is_some_and(|d| d.ends_with("fuzz"))), "no nested fuzz manifest seen");
    let mut bad = Vec::new();
    let mut dev_uses = 0;
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let owner = f.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        for v in violations(&text, &owner) {
            bad.push(format!("{}: {v}", f.display()));
        }
        dev_uses += text.lines().filter(|l| HOOKS.iter().any(|(h, _)| l.contains(h)) && l.contains("features")).count();
    }
    assert!(bad.is_empty(), "test hooks enabled outside [dev-dependencies]:\n{}", bad.join("\n"));
    assert!(dev_uses >= 2, "the scan saw the known dev-dependency uses");
}

#[test]
fn the_hook_scan_catches_every_non_dev_form() {
    let bad = [
        "[dependencies]\nfid-store = { path = \"x\", features = [\"test-probe\"] }\n",
        "[dependencies.fid-store]\npath = \"x\"\nfeatures = [\"test-probe\"]\n",
        "[target.'cfg(unix)'.dependencies]\nfid-crash = { path = \"x\", features = [\"fault-inject\"] }\n",
        "[build-dependencies]\nfid-crash = { path = \"x\", features = [\"fault-inject\"] }\n",
        "[workspace.dependencies]\nfid-store = { path = \"x\", features = [\"test-probe\"] }\n",
        "[features]\ndefault = [\"fid-store/test-probe\"]\n",
        "[features]\nhooks = [\"fid-crash/fault-inject\"]\n",
    ];
    for text in bad {
        assert_eq!(violations(text, "fid-other").len(), 1, "{text}");
    }
    let good = [
        "[dev-dependencies]\nfid-store = { path = \"x\", features = [\"test-probe\"] }\n",
        "[dev-dependencies.fid-crash]\nfeatures = [\"fault-inject\"]\n",
        "[target.'cfg(unix)'.dev-dependencies]\nfid-store = { path = \"x\", features = [\"test-probe\"] }\n",
        "[dependencies]\n# a comment naming test-probe\nfid-store = { path = \"x\" }\n",
    ];
    for text in good {
        assert!(violations(text, "fid-other").is_empty(), "{text}");
    }
    assert!(violations("[features]\ntest-probe = []\n", "fid-store").is_empty());
    assert!(violations("[features]\nfault-inject = [\"fid-store/test-probe\"]\n", "fid-crash").is_empty());
    assert_eq!(violations("[features]\ntest-probe = []\n", "fid-other").len(), 1);
    assert_eq!(violations("[features]\ndefault = [\"fault-inject\"]\n", "fid-crash").len(), 1);
    assert_eq!(violations("[features]\ndefault = [\"test-probe\"]\n", "fid-store").len(), 1);
}
