//! Bounded, read-only listing of a ustar start-state archive (AGENT.EVAL.CHECK).
//!
//! Nothing is written to disk: the check only needs the path and digest of every regular
//! file. Entries with an absolute path, a `..` segment, a backslash or a drive prefix are
//! rejected, and so are links and every entry type other than a regular file or a
//! directory (fail closed: a link could reach files the static check cannot see).

use crate::file_digest;
use fid_types::Digest;

const BLOCK: usize = 512;
/// Most entries accepted in one archive.
pub(crate) const MAX_ENTRIES: usize = 100_000;

/// One regular file of an archive: normalised `/`-joined path and SHA-256 of its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TarFile {
    pub path: String,
    pub digest: Digest,
}

/// Bytes up to the first NUL.
fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(i) => &b[..i],
        None => b,
    }
}

/// An octal header field (NUL- or space-padded). Base-256 fields are not supported.
fn octal(b: &[u8], what: &str) -> Result<u64, String> {
    if b.first().is_some_and(|c| c & 0x80 != 0) {
        return Err(format!("{what}: base-256 numbers are not supported"));
    }
    let mut n: u64 = 0;
    for &c in cstr(b).iter().filter(|c| **c != b' ') {
        if !(b'0'..=b'7').contains(&c) {
            return Err(format!("{what}: not an octal number"));
        }
        n = n
            .checked_mul(8)
            .and_then(|n| n.checked_add(u64::from(c - b'0')))
            .ok_or_else(|| format!("{what}: too large"))?;
    }
    Ok(n)
}

/// The normalised path of an entry name: `.` and empty segments dropped. `None` if the
/// name is absolute, has a `..` segment, a backslash or a `:` (drive prefix), or is empty.
pub(crate) fn normalise(name: &str) -> Option<String> {
    if name.starts_with('/') || name.contains('\\') {
        return None;
    }
    let segs: Vec<&str> = name.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if segs.is_empty() || segs.iter().any(|s| *s == ".." || s.contains(':')) {
        return None;
    }
    Some(segs.join("/"))
}

/// Lists the regular files of a ustar archive. Errors are one-line reasons.
pub(crate) fn list(bytes: &[u8]) -> Result<Vec<TarFile>, String> {
    let mut out = Vec::new();
    let mut off = 0usize;
    let mut entries = 0usize;
    loop {
        if off == bytes.len() {
            break;
        }
        let Some(h) = bytes.get(off..off + BLOCK) else {
            return Err("truncated header".to_string());
        };
        if h.iter().all(|&c| c == 0) {
            break;
        }
        entries += 1;
        if entries > MAX_ENTRIES {
            return Err(format!("more than {MAX_ENTRIES} entries"));
        }
        let stored = octal(&h[148..156], "checksum")?;
        let sum: u64 = h
            .iter()
            .enumerate()
            .map(|(i, &c)| if (148..156).contains(&i) { u64::from(b' ') } else { u64::from(c) })
            .sum();
        if sum != stored {
            return Err(format!("header checksum mismatch at offset {off}"));
        }
        let name = cstr(&h[0..100]);
        let prefix = if &h[257..262] == b"ustar" { cstr(&h[345..500]) } else { &[][..] };
        let mut full = Vec::with_capacity(prefix.len() + 1 + name.len());
        if !prefix.is_empty() {
            full.extend_from_slice(prefix);
            full.push(b'/');
        }
        full.extend_from_slice(name);
        let full = String::from_utf8(full).map_err(|_| "entry name is not UTF-8".to_string())?;
        let size = usize::try_from(octal(&h[124..136], "size")?).map_err(|_| "entry too large".to_string())?;
        let start = off + BLOCK;
        let end =
            start.checked_add(size).filter(|e| *e <= bytes.len()).ok_or_else(|| "truncated entry data".to_string())?;
        let padded = end.checked_add((BLOCK - size % BLOCK) % BLOCK).ok_or_else(|| "entry too large".to_string())?;
        let typ = h[156];
        match typ {
            b'0' | 0 | b'5' => {}
            b'1' | b'2' => return Err(format!("link entry {full:?} is not allowed")),
            other => return Err(format!("entry {full:?} has unsupported type {:?}", char::from(other))),
        }
        let path = normalise(&full).ok_or_else(|| format!("unsafe entry path {full:?}"))?;
        if typ != b'5' {
            out.push(TarFile { path, digest: file_digest(&bytes[start..end]) });
        }
        off = padded.min(bytes.len());
    }
    Ok(out)
}
