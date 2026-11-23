//! AC-3 (performance, offline): seeded large journal segment verification (`large-1m.zip`).
//!
//! Generates a deterministic Result Package containing `FID_CHAIN_LARGE_ENTRIES` journal entries
//! (default 2 000 for fast debug `cargo test`; set `FID_CHAIN_LARGE_ENTRIES=1000000` in release
//! mode for the full 1 000 000-entry, ~1 KiB median header size AC-3 benchmark targeting < 30 s).
//!
//! Streams the segment directly to a temporary ZIP on disk so generation and verification both
//! stay bounded in memory (independent of segment size except the compact sorted hash vector).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_chain::links::{check_package, ChainVerdict, ZERO_DIGEST};
use fid_pkg::manifest::{
    hash_reader, EvidenceTier, Manifest, Member, Statement, FORMAT_NAME, JSON_MEDIA_TYPE, MANIFEST_PATH, STATEMENT_PATH,
};
use fid_pkg::{crc32, PkgPath, FORMAT_VERSION};
use fid_trust_canon::digest_canonical;
use fid_types::Digest;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_ENTRIES: u64 = 2_000;
const BUDGET_SECS: f64 = 30.0;

struct Cleanup(Vec<PathBuf>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = fs::remove_file(p);
        }
    }
}

/// Deterministic 1 KiB-class canonical journal entry header (exact RFC 8785 key order, no
/// whitespace, matching `CORE.JRN.ENTRY` + `TRUST.CHAIN.CANON`).
///
/// Computing the header bytes directly in canonical RFC 8785 order makes generating 1 000 000
/// entries take ~1 s instead of minutes, while `check_package` independently parses and
/// re-canonicalises every entry through `fid-trust-canon`.
fn write_canonical_entry(out: &mut impl Write, seq: u64, prev: Digest, pad: &str) -> Digest {
    let mono_ns = seq * 1000;
    let body_without_hash = if seq == 1 {
        format!(
            r#"{{"ai_assistance":"none","branch":"main","kind":"session_header","logical_parent":null,"manifests":{{"kernel":"sha256:{k}","registry":"sha256:{r}"}},"prev":"{prev}","principal":{{"commit":"sha256:{p}","kind":"script"}},"schema_version":1,"seq":"1","session":{{"app_version":"0.0.1","note":"{pad}","session_id":"large-1m-seed-42"}},"stream":"main","time":{{"mono_ns":"{mono_ns}","sync":"test","utc":"2026-01-01T00:00:00Z"}}}}"#,
            k = "11".repeat(32),
            r = "22".repeat(32),
            p = "33".repeat(32),
        )
    } else {
        format!(
            r#"{{"ai_assistance":"none","branch":"main","command":{{"algorithms":["fit.circle.ls@1.0.0"],"args_commit":"sha256:{a:064x}","class":"R1","determinism":"D0","events_commit":"sha256:{e:064x}","id":"gdt.fit_circle","inputs":[{{"digest":"sha256:{i:064x}","tag":"input"}}],"major":1,"note":"{pad}","outputs":[{{"digest":"sha256:{o:064x}"}}],"seed":"{seq}"}},"kind":"command","logical_parent":null,"manifests":{{"kernel":"sha256:{k}","registry":"sha256:{r}"}},"prev":"{prev}","principal":{{"commit":"sha256:{p}","kind":"script"}},"schema_version":1,"seq":"{seq}","stream":"main","time":{{"mono_ns":"{mono_ns}","sync":"test","utc":"2026-01-01T00:00:00Z"}}}}"#,
            a = seq.wrapping_mul(0x9E37_79B9_7F4A_7C15),
            e = seq.wrapping_mul(0xBF58_476D_1CE4_E5B9),
            i = seq.wrapping_mul(0x94D0_49BB_1331_11EB),
            o = seq.wrapping_mul(0xD6E8_FEB8_6659_FD93),
            k = "11".repeat(32),
            r = "22".repeat(32),
            p = "33".repeat(32),
        )
    };
    let hash = digest_canonical(body_without_hash.as_bytes());
    // Insert `"hash":"sha256:..."` in exact UTF-16 sorted key order (between "command"/"branch"
    // and "kind") or anywhere inside header since the verifier sorts all keys. Putting it in
    // canonical order keeps the segment RFC 8785 canonical too.
    let split_at = body_without_hash.find(",\"kind\":").expect("kind field present");
    out.write_all(b"{\"header\":").unwrap();
    out.write_all(&body_without_hash.as_bytes()[..split_at]).unwrap();
    write!(out, ",\"hash\":\"{hash}\"").unwrap();
    out.write_all(&body_without_hash.as_bytes()[split_at..]).unwrap();
    out.write_all(b"}\n").unwrap();
    hash
}

/// Computes the exact compressed size of RFC 1951 stored-block deflate over `len` bytes.
fn stored_deflate_len(len: u64) -> u64 {
    if len == 0 {
        return 5;
    }
    let blocks = len.div_ceil(65_535);
    len + blocks * 5
}

/// Streams `src` as RFC 1951 stored-block deflate chunks into `dst`.
fn copy_stored_deflate(src: &mut impl Read, dst: &mut impl Write, total_len: u64) {
    if total_len == 0 {
        dst.write_all(&[1, 0, 0, 0xFF, 0xFF]).unwrap();
        return;
    }
    let mut buf = vec![0u8; 65_535];
    let mut remaining = total_len;
    while remaining > 0 {
        let want = remaining.min(65_535) as usize;
        src.read_exact(&mut buf[..want]).unwrap();
        remaining -= want as u64;
        let last = u8::from(remaining == 0);
        let l16 = want as u16;
        let n16 = !l16;
        let mut hdr = [0u8; 5];
        hdr[0] = last;
        hdr[1..3].copy_from_slice(&l16.to_le_bytes());
        hdr[3..5].copy_from_slice(&n16.to_le_bytes());
        dst.write_all(&hdr).unwrap();
        dst.write_all(&buf[..want]).unwrap();
    }
}

/// Computes CRC-32 by streaming `path` in 64 KiB chunks.
fn file_crc32(path: &Path) -> u32 {
    let mut f = File::open(path).unwrap();
    let mut buf = vec![0u8; 64 * 1024];
    let mut crc = 0u32;
    loop {
        let n = f.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        crc = crc32_combine_chunk(crc, &buf[..n]);
    }
    crc
}

/// Table-driven incremental IEEE CRC-32 matching `fid_pkg::crc32`.
const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

fn crc32_combine_chunk(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        c = CRC_TABLE[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

struct ZipRec {
    name: String,
    crc: u32,
    csize: u64,
    usize: u64,
    offset: u64,
}

fn write_local_header(out: &mut impl Write, name: &str, crc: u32, csize: u64, usize: u64) -> u64 {
    assert!(csize <= u64::from(u32::MAX) && usize <= u64::from(u32::MAX), "use zip64 for >4 GiB entries");
    let mut h = [0u8; 30];
    h[0..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
    h[4..6].copy_from_slice(&20u16.to_le_bytes()); // version needed 2.0
    h[6..8].copy_from_slice(&0x0800u16.to_le_bytes()); // UTF-8
    h[8..10].copy_from_slice(&8u16.to_le_bytes()); // deflate
    h[10..12].copy_from_slice(&0u16.to_le_bytes()); // time
    h[12..14].copy_from_slice(&0x0021u16.to_le_bytes()); // 1980-01-01
    h[14..18].copy_from_slice(&crc.to_le_bytes());
    h[18..22].copy_from_slice(&(csize as u32).to_le_bytes());
    h[22..26].copy_from_slice(&(usize as u32).to_le_bytes());
    h[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
    h[28..30].copy_from_slice(&0u16.to_le_bytes());
    out.write_all(&h).unwrap();
    out.write_all(name.as_bytes()).unwrap();
    30 + name.len() as u64
}

fn write_central_and_eocd(out: &mut impl Write, recs: &[ZipRec], cd_offset: u64) {
    let mut cd_size: u64 = 0;
    for r in recs {
        assert!(r.offset <= u64::from(u32::MAX));
        let mut h = [0u8; 46];
        h[0..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
        h[4..6].copy_from_slice(&0x0314u16.to_le_bytes()); // Unix, 2.0
        h[6..8].copy_from_slice(&20u16.to_le_bytes());
        h[8..10].copy_from_slice(&0x0800u16.to_le_bytes());
        h[10..12].copy_from_slice(&8u16.to_le_bytes());
        h[12..14].copy_from_slice(&0u16.to_le_bytes());
        h[14..16].copy_from_slice(&0x0021u16.to_le_bytes());
        h[16..20].copy_from_slice(&r.crc.to_le_bytes());
        h[20..24].copy_from_slice(&(r.csize as u32).to_le_bytes());
        h[24..28].copy_from_slice(&(r.usize as u32).to_le_bytes());
        h[28..30].copy_from_slice(&(r.name.len() as u16).to_le_bytes());
        h[30..32].copy_from_slice(&0u16.to_le_bytes());
        h[32..34].copy_from_slice(&0u16.to_le_bytes());
        h[34..36].copy_from_slice(&0u16.to_le_bytes());
        h[36..38].copy_from_slice(&0u16.to_le_bytes());
        h[38..42].copy_from_slice(&(0o100_644u32 << 16).to_le_bytes());
        h[42..46].copy_from_slice(&(r.offset as u32).to_le_bytes());
        out.write_all(&h).unwrap();
        out.write_all(r.name.as_bytes()).unwrap();
        cd_size += 46 + r.name.len() as u64;
    }
    let mut eocd = [0u8; 22];
    eocd[0..4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
    eocd[4..6].copy_from_slice(&0u16.to_le_bytes());
    eocd[6..8].copy_from_slice(&0u16.to_le_bytes());
    eocd[8..10].copy_from_slice(&(recs.len() as u16).to_le_bytes());
    eocd[10..12].copy_from_slice(&(recs.len() as u16).to_le_bytes());
    eocd[12..16].copy_from_slice(&(cd_size as u32).to_le_bytes());
    eocd[16..20].copy_from_slice(&(cd_offset as u32).to_le_bytes());
    eocd[20..22].copy_from_slice(&0u16.to_le_bytes());
    out.write_all(&eocd).unwrap();
}

#[test]
fn ac3_large_segment_verifies_under_budget() {
    let n: u64 = std::env::var("FID_CHAIN_LARGE_ENTRIES").ok().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_ENTRIES);

    let pid = std::process::id();
    let seg_path = std::env::temp_dir().join(format!("fid-chain-large-{n}-{pid}.jsonl"));
    let zip_path = std::env::temp_dir().join(format!("fid-chain-large-{n}-{pid}.zip"));
    let _cleanup = Cleanup(vec![seg_path.clone(), zip_path.clone()]);

    // Command entry header is ~1 100 bytes (~1 KiB median header size, REQ-006).
    let pad = "x".repeat(16);

    let gen_start = Instant::now();
    {
        let mut w = BufWriter::with_capacity(256 * 1024, File::create(&seg_path).unwrap());
        let mut prev = ZERO_DIGEST;
        for seq in 1..=n {
            prev = write_canonical_entry(&mut w, seq, prev, &pad);
        }
        w.flush().unwrap();
    }

    let (seg_len, seg_sha) = hash_reader(&mut File::open(&seg_path).unwrap()).unwrap();
    let seg_crc = file_crc32(&seg_path);
    let avg_entry_bytes = seg_len / n.max(1);
    assert!(avg_entry_bytes >= 1000, "expected ~1 KiB header size, got avg {avg_entry_bytes} bytes");

    let manifest = Manifest {
        format: FORMAT_NAME.to_owned(),
        format_version: FORMAT_VERSION,
        created_by: "fid-chain large-1m seeded generator".to_owned(),
        evidence_tier: EvidenceTier::Results,
        profile_digest: Digest::from_bytes([0xaa; 32]),
        kernel_manifest_digest: Digest::from_bytes([0xbb; 32]),
        members: vec![Member {
            path: PkgPath::parse("journal/segment.jsonl").unwrap(),
            media_type: "application/x-ndjson".to_owned(),
            length: seg_len,
            sha256: seg_sha,
        }],
    };
    let manifest_bytes = manifest.to_canonical_bytes().unwrap();
    let statement_bytes = Statement::for_manifest(&manifest, &manifest_bytes, 0).to_canonical_bytes().unwrap();

    // Stream-write the valid Result Package ZIP directly to disk.
    {
        let mut out = BufWriter::with_capacity(256 * 1024, File::create(&zip_path).unwrap());
        let mut recs = Vec::with_capacity(3);
        let mut offset = 0u64;

        // 1. manifest.json
        let m_crc = crc32(&manifest_bytes);
        let m_usize = manifest_bytes.len() as u64;
        let m_csize = stored_deflate_len(m_usize);
        offset += write_local_header(&mut out, MANIFEST_PATH, m_crc, m_csize, m_usize);
        copy_stored_deflate(&mut manifest_bytes.as_slice(), &mut out, m_usize);
        recs.push(ZipRec { name: MANIFEST_PATH.to_owned(), crc: m_crc, csize: m_csize, usize: m_usize, offset: 0 });
        offset += m_csize;

        // 2. journal/segment.jsonl
        let s_off = offset;
        let s_csize = stored_deflate_len(seg_len);
        offset += write_local_header(&mut out, "journal/segment.jsonl", seg_crc, s_csize, seg_len);
        let mut seg_reader = BufReader::with_capacity(256 * 1024, File::open(&seg_path).unwrap());
        copy_stored_deflate(&mut seg_reader, &mut out, seg_len);
        recs.push(ZipRec {
            name: "journal/segment.jsonl".to_owned(),
            crc: seg_crc,
            csize: s_csize,
            usize: seg_len,
            offset: s_off,
        });
        offset += s_csize;

        // 3. statement.json
        let st_off = offset;
        let st_crc = crc32(&statement_bytes);
        let st_usize = statement_bytes.len() as u64;
        let st_csize = stored_deflate_len(st_usize);
        offset += write_local_header(&mut out, STATEMENT_PATH, st_crc, st_csize, st_usize);
        copy_stored_deflate(&mut statement_bytes.as_slice(), &mut out, st_usize);
        recs.push(ZipRec {
            name: STATEMENT_PATH.to_owned(),
            crc: st_crc,
            csize: st_csize,
            usize: st_usize,
            offset: st_off,
        });
        offset += st_csize;

        write_central_and_eocd(&mut out, &recs, offset);
        out.flush().unwrap();
    }
    let _ = fs::remove_file(&seg_path);
    let gen_secs = gen_start.elapsed().as_secs_f64();

    // Time ONLY the offline package verification (REQ-TRUST.CHAIN.LINKS-006 / AC-3).
    let verify_start = Instant::now();
    let f = File::open(&zip_path).unwrap();
    let report = check_package(f, None).expect("check_package on large segment");
    let verify_secs = verify_start.elapsed().as_secs_f64();

    eprintln!(
        "large_1m: n={n}, avg_entry={avg_entry_bytes} B, seg={:.1} MiB, gen={gen_secs:.2}s, verify={verify_secs:.2}s",
        seg_len as f64 / (1024.0 * 1024.0)
    );

    assert_eq!(report.verdict, ChainVerdict::Valid, "findings: {:?}", report.findings);
    assert_eq!(report.entries_verified, n);
    assert!(report.findings.is_empty());
    assert!(
        verify_secs < BUDGET_SECS,
        "verification of {n} entries took {verify_secs:.2}s, exceeding {BUDGET_SECS:.0}s budget"
    );
    let _ = JSON_MEDIA_TYPE;
}
