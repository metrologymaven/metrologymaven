//! A bounded, fail-closed reader for POSIX ustar start-state archives.
//!
//! Accepted: 512-byte headers with magic `ustar\0` and version `00`, a valid header checksum,
//! octal sizes, type `0` (or NUL) regular files and type `5` directories, the `prefix` field,
//! and an end-of-archive zero block followed only by zero bytes. Everything else (symbolic and
//! hard links, devices, FIFOs, pax and GNU extension headers, base-256 numbers, truncation,
//! trailing data) is refused. Path checks are the caller's ([`crate::path::check_rel`]).

/// Largest accepted number of entries.
pub(crate) const MAX_ENTRIES: usize = 100_000;

const BLOCK: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Dir,
}

#[derive(Debug, Clone)]
pub(crate) struct Entry<'a> {
    /// The path as stored, a directory's trailing `/` removed.
    pub path: String,
    pub kind: Kind,
    pub data: &'a [u8],
}

fn field(h: &[u8], a: usize, b: usize) -> &[u8] {
    h.get(a..b).unwrap_or(&[])
}

/// A NUL-terminated text field; bytes after the first NUL must all be NUL.
fn text(f: &[u8], what: &str) -> Result<String, String> {
    let end = f.iter().position(|&c| c == 0).unwrap_or(f.len());
    let (s, rest) = f.split_at(end);
    if rest.iter().any(|&c| c != 0) {
        return Err(format!("{what}: data after the terminating NUL"));
    }
    String::from_utf8(s.to_vec()).map_err(|_| format!("{what}: not UTF-8"))
}

/// An octal number field: optional leading spaces, octal digits, then only spaces or NULs.
fn octal(f: &[u8], what: &str) -> Result<u64, String> {
    if f.first().is_some_and(|&c| c & 0x80 != 0) {
        return Err(format!("{what}: base-256 numbers are not accepted"));
    }
    let bad = || format!("{what}: not an octal number");
    let mut v: u64 = 0;
    let mut seen = false;
    let mut done = false;
    for &c in f {
        match c {
            b'0'..=b'7' if !done => {
                v = v.checked_mul(8).and_then(|v| v.checked_add(u64::from(c - b'0'))).ok_or_else(bad)?;
                seen = true;
            }
            b' ' if !seen => {}
            b' ' | 0 => done = true,
            _ => return Err(bad()),
        }
    }
    if seen {
        Ok(v)
    } else {
        Err(bad())
    }
}

/// Parses the whole archive into entries, or names the first problem.
pub(crate) fn parse(bytes: &[u8]) -> Result<Vec<Entry<'_>>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    loop {
        let header = bytes.get(pos..pos + BLOCK).ok_or("archive ends without an end-of-archive block")?;
        if header.iter().all(|&c| c == 0) {
            let rest = bytes.get(pos..).unwrap_or(&[]);
            if rest.iter().any(|&c| c != 0) {
                return Err("data after the end-of-archive block".to_string());
            }
            return Ok(out);
        }
        if out.len() >= MAX_ENTRIES {
            return Err(format!("more than {MAX_ENTRIES} entries"));
        }
        let at = format!("entry {}", out.len());
        let stored = octal(field(header, 148, 156), &format!("{at}: checksum"))?;
        let sum: u64 = header
            .iter()
            .enumerate()
            .map(|(i, &c)| if (148..156).contains(&i) { u64::from(b' ') } else { u64::from(c) })
            .sum();
        if sum != stored {
            return Err(format!("{at}: header checksum mismatch"));
        }
        if field(header, 257, 263) != b"ustar\0" || field(header, 263, 265) != b"00" {
            return Err(format!("{at}: not a POSIX ustar header"));
        }
        let name = text(field(header, 0, 100), &format!("{at}: name"))?;
        let prefix = text(field(header, 345, 500), &format!("{at}: prefix"))?;
        let mut path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let size = octal(field(header, 124, 136), &format!("{at}: size"))?;
        let kind = match field(header, 156, 157) {
            b"0" | b"\0" => Kind::File,
            b"5" => Kind::Dir,
            other => {
                let t = other.first().map_or('?', |&c| char::from(c));
                return Err(format!("{at} ({path}): entry type {t:?} is not a regular file or directory"));
            }
        };
        if kind == Kind::Dir {
            if size != 0 {
                return Err(format!("{at} ({path}): directory with data"));
            }
            if let Some(p) = path.strip_suffix('/') {
                path = p.to_string();
            }
        }
        let size = usize::try_from(size).map_err(|_| format!("{at}: size too large"))?;
        let start = pos + BLOCK;
        let end = start.checked_add(size).ok_or_else(|| format!("{at}: size too large"))?;
        let data = bytes.get(start..end).ok_or_else(|| format!("{at} ({path}): truncated data"))?;
        let padded = size.div_ceil(BLOCK).checked_mul(BLOCK).ok_or_else(|| format!("{at}: size too large"))?;
        pos = start + padded;
        if pos > bytes.len() {
            return Err(format!("{at} ({path}): truncated padding"));
        }
        out.push(Entry { path, kind, data });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn octal_fields() {
        assert_eq!(octal(b"0000644\0", "x").unwrap(), 0o644);
        assert_eq!(octal(b"  17 \0\0\0", "x").unwrap(), 0o17);
        assert!(octal(b"0008\0", "x").is_err());
        assert!(octal(b"\0\0\0", "x").is_err());
        assert!(octal(b"1 2\0", "x").is_err());
        assert!(octal(&[0x80, 0, 0, 1], "x").is_err());
    }

    #[test]
    fn empty_and_short_archives() {
        assert!(parse(&[0u8; 1024]).unwrap().is_empty());
        assert!(parse(&[]).is_err());
        assert!(parse(&[0u8; 100]).is_err());
    }
}
