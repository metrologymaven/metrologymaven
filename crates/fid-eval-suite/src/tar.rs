//! Bounded, read-only listing of a ustar start-state archive (AGENT.EVAL.CHECK).
//!
//! Nothing is written to disk: the check only needs the path and digest of every regular
//! file. The listing fails closed on anything an extractor could resolve differently from
//! the check:
//!
//! - an absolute path, a `..` segment, a backslash or a drive prefix;
//! - a link, or any entry type other than a regular file or a directory;
//! - two entries with the same [`fold`]ed path (a repeated path, where extraction keeps the
//!   last copy, or names that differ only in ASCII case or trailing dots and spaces, which
//!   land on the same file on Windows and macOS);
//! - a bad header checksum, a base-256 number, truncated data, more than [`MAX_ENTRIES`]
//!   entries, or any non-zero byte after the end-of-archive block.

use crate::file_digest;
use fid_types::Digest;
use std::collections::BTreeMap;

const BLOCK: usize = 512;
/// Most entries accepted in one archive.
pub(crate) const MAX_ENTRIES: usize = 100_000;

/// One regular file of an archive: normalised `/`-joined path, its [`fold`]ed form and
/// the SHA-256 of its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TarFile {
    pub path: String,
    pub folded: String,
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
/// name is absolute, has a `..` segment, a backslash or a `:` (drive prefix), is empty, or
/// has a segment that folds to nothing (only dots and spaces).
pub(crate) fn normalise(name: &str) -> Option<String> {
    if name.starts_with('/') || name.contains('\\') {
        return None;
    }
    let segs: Vec<&str> = name.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if segs.is_empty() || segs.iter().any(|s| *s == ".." || s.contains(':') || fold_segment(s).is_empty()) {
        return None;
    }
    Some(segs.join("/"))
}

fn fold_segment(s: &str) -> String {
    s.trim_end_matches(['.', ' ']).to_ascii_lowercase()
}

/// The form two paths are compared in: each segment ASCII-lowercased with trailing dots and
/// spaces removed, as case-insensitive file systems and Windows resolve them.
pub(crate) fn fold(path: &str) -> String {
    path.split('/').map(fold_segment).collect::<Vec<_>>().join("/")
}

/// Lists the regular files of a ustar archive. Errors are one-line reasons.
pub(crate) fn list(bytes: &[u8]) -> Result<Vec<TarFile>, String> {
    list_with_limit(bytes, MAX_ENTRIES)
}

fn list_with_limit(bytes: &[u8], max_entries: usize) -> Result<Vec<TarFile>, String> {
    let mut out = Vec::new();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
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
            if bytes[off..].iter().any(|&c| c != 0) {
                return Err(format!("non-zero data after the end-of-archive block at offset {off}"));
            }
            break;
        }
        entries += 1;
        if entries > max_entries {
            return Err(format!("more than {max_entries} entries"));
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
        let folded = fold(&path);
        if let Some(first) = seen.insert(folded.clone(), path.clone()) {
            return Err(if first == path {
                format!("entry path {path:?} appears more than once")
            } else {
                format!("entry paths {first:?} and {path:?} name the same file on a case-insensitive file system")
            });
        }
        if typ != b'5' {
            out.push(TarFile { path, folded, digest: file_digest(&bytes[start..end]) });
        }
        off = padded.min(bytes.len());
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    //! Hostile archives, built header by header from the ustar layout (hand-labelled).
    use super::*;

    fn set_checksum(h: &mut [u8]) {
        h[148..156].fill(b' ');
        let sum: u32 = h.iter().map(|&c| u32::from(c)).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    }

    /// A ustar header for `name` with `size`, `typ`, and a correct checksum.
    fn header(name: &str, size: usize, typ: u8) -> Vec<u8> {
        let mut h = vec![0u8; BLOCK];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[108..115].copy_from_slice(b"0000000");
        h[116..123].copy_from_slice(b"0000000");
        h[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
        h[136..147].copy_from_slice(b"00000000000");
        h[156] = typ;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        set_checksum(&mut h);
        h
    }

    fn file(name: &str, data: &[u8]) -> Vec<u8> {
        let mut v = header(name, data.len(), b'0');
        v.extend_from_slice(data);
        v.resize(v.len() + (BLOCK - data.len() % BLOCK) % BLOCK, 0);
        v
    }

    fn archive(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut v: Vec<u8> = parts.concat();
        v.extend_from_slice(&[0u8; 2 * BLOCK]);
        v
    }

    fn err(bytes: &[u8]) -> String {
        list(bytes).expect_err("archive must be rejected")
    }

    #[test]
    fn good_archive_lists_files() {
        let a = archive(&[header("in/", 0, b'5'), file("in/a.txt", b"a\n"), file("./b.txt", b"b\n")]);
        let got = list(&a).unwrap();
        let paths: Vec<&str> = got.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["in/a.txt", "b.txt"]);
        assert_eq!(got[0].digest, file_digest(b"a\n"));
    }

    #[test]
    fn duplicate_path_is_rejected() {
        let a = archive(&[file("in/x.json", b"harmless\n"), file("in/x.json", b"oracle\n")]);
        assert!(err(&a).contains("more than once"));
        let a = archive(&[file("in/x.json", b"1\n"), file("./in//x.json", b"2\n")]);
        assert!(err(&a).contains("more than once"));
    }

    #[test]
    fn case_and_trailing_dot_collisions_are_rejected() {
        let a = archive(&[file("in/a.txt", b"1\n"), file("IN/A.TXT", b"2\n")]);
        assert!(err(&a).contains("case-insensitive"));
        let a = archive(&[file("in/a.txt", b"1\n"), file("in./a.txt ", b"2\n")]);
        assert!(err(&a).contains("case-insensitive"));
        assert_eq!(fold("IN./Oracle.JSON. "), "in/oracle.json");
        assert!(normalise("in/... /x").is_none());
    }

    #[test]
    fn truncated_data_is_rejected() {
        let mut a = header("in/a.txt", 2000, b'0');
        a.extend_from_slice(&[b'x'; 100]);
        assert!(err(&a).contains("truncated entry data"));
        assert!(err(&header("in/a.txt", 0, b'0')[..300]).contains("truncated header"));
    }

    #[test]
    fn bad_checksum_is_rejected() {
        let mut h = header("in/a.txt", 0, b'0');
        h[0] = b'j';
        assert!(err(&archive(&[h])).contains("checksum"));
    }

    #[test]
    fn base256_size_is_rejected() {
        let mut h = header("in/a.txt", 0, b'0');
        h[124..136].fill(0);
        h[124] = 0x80;
        h[135] = 1;
        set_checksum(&mut h);
        assert!(err(&archive(&[h])).contains("base-256"));
    }

    #[test]
    fn too_many_entries_are_rejected() {
        let parts: Vec<Vec<u8>> = (0..4).map(|i| file(&format!("f{i}"), b"")).collect();
        let a = archive(&parts);
        assert_eq!(list_with_limit(&a, 4).unwrap().len(), 4);
        assert!(list_with_limit(&a, 3).unwrap_err().contains("more than 3 entries"));
    }

    #[test]
    fn data_after_end_block_is_rejected() {
        let mut a = archive(&[file("in/a.txt", b"a\n")]);
        a.extend_from_slice(&file("in/hidden.json", b"oracle\n"));
        assert!(err(&a).contains("after the end-of-archive block"));
    }

    #[test]
    fn unsafe_entries_are_rejected() {
        assert!(err(&archive(&[file("../x", b"")])).contains("unsafe"));
        assert!(err(&archive(&[file("/x", b"")])).contains("unsafe"));
        assert!(err(&archive(&[file("C:x", b"")])).contains("unsafe"));
        assert!(err(&archive(&[file("in\\x", b"")])).contains("unsafe"));
        assert!(err(&archive(&[header("in/l", 0, b'2')])).contains("link"));
        assert!(err(&archive(&[header("in/f", 0, b'6')])).contains("unsupported"));
    }
}
