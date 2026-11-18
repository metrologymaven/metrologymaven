//! Generated house-rules blocks inside Markdown files. Spec node `ECO.RULES.BLOCK`.
//!
//! A block is the begin marker line, the body lines, and the end marker line:
//!
//! ```text
//! <!-- house-rules:begin block=<id> source=<repo-id>@<40-hex rev> sha256=<64-hex> -->
//! ...body...
//! <!-- house-rules:end block=<id> -->
//! ```
//!
//! The digest is the SHA-256 of the body as UTF-8 with CRLF normalised to LF, nothing added
//! or removed ([`digest`]). [`render`] writes a block, [`find_blocks`] finds and parses every
//! block in a text. Whether a digest mismatch fails a check is policy and belongs to the
//! caller (ECO.RULES.CHECK): [`Block::is_intact`] only reports it.
//!
//! Lines are split on LF; one trailing CR on a line is part of its terminator, so files with
//! CRLF endings parse to the same blocks and digests. Any line whose text (after leading spaces
//! and tabs) starts with `<!-- house-rules:` is a marker line and must be a valid begin or end
//! marker, otherwise parsing fails with `bad-marker`: the parser fails closed.

use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::fmt;

/// Text that makes a line a marker line (after leading spaces and tabs).
pub const MARKER_PREFIX: &str = "<!-- house-rules:";
const BEGIN_PREFIX: &str = "<!-- house-rules:begin block=";
const END_PREFIX: &str = "<!-- house-rules:end block=";
const SUFFIX: &str = " -->";
/// Longest accepted repository ID, in bytes.
pub const MAX_REPO_ID: usize = 100;

/// One generated block found in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Block ID, `^[a-z][a-z0-9-]{1,31}$`.
    pub id: String,
    /// Canonical source repository ID (the part before `@`).
    pub source_repo: String,
    /// Canonical source revision, 40 lowercase hex digits.
    pub source_rev: String,
    /// Digest recorded in the begin marker.
    pub sha256: Digest,
    /// Every line between the markers, joined with their original line endings; the
    /// terminator of the last body line is not part of the body.
    pub body: String,
    /// 1-based line number of the begin marker.
    pub begin_line: usize,
    /// 1-based line number of the end marker.
    pub end_line: usize,
}

impl Block {
    /// The digest of the body as it is now.
    pub fn computed_digest(&self) -> Digest {
        digest(&self.body)
    }

    /// Whether the body still matches the recorded digest.
    pub fn is_intact(&self) -> bool {
        self.computed_digest() == self.sha256
    }
}

/// Kind of a parse error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockErrorKind {
    /// An end marker without an open block, or a begin marker never closed.
    Unbalanced,
    /// A begin marker while a block is open.
    Nested,
    /// An end marker whose block ID differs from the open block's.
    IdMismatch,
    /// A marker line that is not a valid begin or end marker.
    BadMarker,
}

impl BlockErrorKind {
    /// The stable wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            BlockErrorKind::Unbalanced => "unbalanced",
            BlockErrorKind::Nested => "nested",
            BlockErrorKind::IdMismatch => "id-mismatch",
            BlockErrorKind::BadMarker => "bad-marker",
        }
    }
}

/// A parse error with the 1-based line number it refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockError {
    /// What went wrong.
    pub kind: BlockErrorKind,
    /// 1-based line number: the offending marker, or for a begin never closed, that begin.
    pub line: usize,
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at line {}", self.kind.as_str(), self.line)
    }
}

impl std::error::Error for BlockError {}

/// Why [`render`] refused its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// The block ID does not match `^[a-z][a-z0-9-]{1,31}$`.
    BadBlockId,
    /// The repository ID is empty, too long, or uses characters outside `[A-Za-z0-9._/-]`.
    BadSourceRepo,
    /// The revision is not 40 lowercase hex digits.
    BadSourceRev,
    /// Body line `line` (1-based) is a marker line.
    MarkerInBody {
        /// 1-based line within the body.
        line: usize,
    },
    /// The body ends with CR, which parsing would take as part of a line ending.
    TrailingCr,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::BadBlockId => f.write_str("bad block id"),
            RenderError::BadSourceRepo => f.write_str("bad source repository id"),
            RenderError::BadSourceRev => f.write_str("bad source revision"),
            RenderError::MarkerInBody { line } => write!(f, "marker line in body at line {line}"),
            RenderError::TrailingCr => f.write_str("body ends with CR"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Whether `s` is a valid block ID, `^[a-z][a-z0-9-]{1,31}$`.
pub fn is_valid_block_id(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=32).contains(&b.len())
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// Whether `s` is a valid repository ID: 1 to 100 bytes of `[A-Za-z0-9._/-]`, starting with a
/// letter or digit.
pub fn is_valid_repo_id(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=MAX_REPO_ID).contains(&b.len())
        && b.first().is_some_and(u8::is_ascii_alphanumeric)
        && b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'/' | b'-'))
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether `s` is a valid source revision, 40 lowercase hex digits.
pub fn is_valid_rev(s: &str) -> bool {
    is_lower_hex(s, 40)
}

/// SHA-256 of `body` as UTF-8 with every CRLF replaced by LF (REQ-ECO.RULES.BLOCK-002).
pub fn digest(body: &str) -> Digest {
    let normalised = body.replace("\r\n", "\n");
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(normalised.as_bytes()));
    Digest::from_bytes(out)
}

/// Removes one trailing CR (part of a CRLF line ending).
fn strip_cr(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

/// Whether a line (without its terminator) is a marker line.
fn is_marker_line(line: &str) -> bool {
    line.trim_start_matches([' ', '\t']).starts_with(MARKER_PREFIX)
}

/// Renders a block: begin marker, body, end marker, each ending in LF.
///
/// Parsing the result with [`find_blocks`] returns the same ID, repository, revision, digest
/// and body (REQ-ECO.RULES.BLOCK-004).
pub fn render(block_id: &str, source_repo: &str, source_rev: &str, body: &str) -> Result<String, RenderError> {
    if !is_valid_block_id(block_id) {
        return Err(RenderError::BadBlockId);
    }
    if !is_valid_repo_id(source_repo) {
        return Err(RenderError::BadSourceRepo);
    }
    if !is_valid_rev(source_rev) {
        return Err(RenderError::BadSourceRev);
    }
    if let Some(i) = body.split('\n').position(|l| is_marker_line(strip_cr(l))) {
        return Err(RenderError::MarkerInBody { line: i + 1 });
    }
    if body.ends_with('\r') {
        return Err(RenderError::TrailingCr);
    }
    let sha = digest(body);
    Ok(format!(
        "{BEGIN_PREFIX}{block_id} source={source_repo}@{source_rev} sha256={}{SUFFIX}\n{body}\n{END_PREFIX}{block_id}{SUFFIX}\n",
        sha.hex()
    ))
}

struct Begin<'a> {
    id: &'a str,
    repo: &'a str,
    rev: &'a str,
    sha: Digest,
}

fn parse_begin(line: &str) -> Option<Begin<'_>> {
    let rest = line.strip_prefix(BEGIN_PREFIX)?.strip_suffix(SUFFIX)?;
    let (id, rest) = rest.split_once(" source=")?;
    let (source, hex) = rest.rsplit_once(" sha256=")?;
    let (repo, rev) = source.rsplit_once('@')?;
    if !is_valid_block_id(id) || !is_valid_repo_id(repo) || !is_valid_rev(rev) || !is_lower_hex(hex, 64) {
        return None;
    }
    let sha = Digest::parse(&format!("sha256:{hex}")).ok()?;
    Some(Begin { id, repo, rev, sha })
}

fn parse_end(line: &str) -> Option<&str> {
    let id = line.strip_prefix(END_PREFIX)?.strip_suffix(SUFFIX)?;
    is_valid_block_id(id).then_some(id)
}

/// Finds and parses every block in `text`, in order.
///
/// Fails with a typed error naming the 1-based line when markers are unbalanced, nested,
/// carry different block IDs, or are malformed (REQ-ECO.RULES.BLOCK-003). Text outside blocks
/// is ignored. A digest mismatch is not an error here; see [`Block::is_intact`].
pub fn find_blocks(text: &str) -> Result<Vec<Block>, BlockError> {
    let mut blocks = Vec::new();
    // (begin line number, parsed begin, byte offset where the body starts)
    let mut open: Option<(usize, Begin<'_>, usize)> = None;
    let mut offset = 0usize;
    for (i, raw) in text.split('\n').enumerate() {
        let n = i + 1;
        let line_start = offset;
        offset += raw.len() + 1;
        let line = strip_cr(raw);
        if !is_marker_line(line) {
            continue;
        }
        let err = |kind| BlockError { kind, line: n };
        if let Some(begin) = parse_begin(line) {
            if open.is_some() {
                return Err(err(BlockErrorKind::Nested));
            }
            open = Some((n, begin, offset));
        } else if let Some(end_id) = parse_end(line) {
            let Some((begin_line, begin, body_start)) = open.take() else {
                return Err(err(BlockErrorKind::Unbalanced));
            };
            if end_id != begin.id {
                return Err(err(BlockErrorKind::IdMismatch));
            }
            let body = if line_start > body_start {
                // Exclude the LF ending the last body line, then its CR if any.
                strip_cr(text.get(body_start..line_start - 1).unwrap_or_default())
            } else {
                ""
            };
            blocks.push(Block {
                id: begin.id.to_string(),
                source_repo: begin.repo.to_string(),
                source_rev: begin.rev.to_string(),
                sha256: begin.sha,
                body: body.to_string(),
                begin_line,
                end_line: n,
            });
        } else {
            return Err(err(BlockErrorKind::BadMarker));
        }
    }
    match open {
        Some((begin_line, _, _)) => Err(BlockError { kind: BlockErrorKind::Unbalanced, line: begin_line }),
        None => Ok(blocks),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REV: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn ids() {
        for ok in ["ab", "core", "a-1", "z234567890123456789012345678901"] {
            assert!(is_valid_block_id(ok), "{ok}");
        }
        for bad in ["", "a", "1a", "-a", "Ab", "a_b", "a b", "a23456789012345678901234567890123", "é1"] {
            assert!(!is_valid_block_id(bad), "{bad:?}");
        }
        assert!(is_valid_repo_id("Org.Name/repo_1-x"));
        for bad in ["", "/a", ".a", "a@b", "a b", "ä"] {
            assert!(!is_valid_repo_id(bad), "{bad:?}");
        }
        assert!(!is_valid_repo_id(&"x".repeat(101)));
        assert!(!is_valid_rev(&REV.to_uppercase()));
    }

    #[test]
    fn empty_body_digest() {
        assert_eq!(digest("").hex(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn render_refuses_bad_input() {
        assert_eq!(render("A", "r", REV, ""), Err(RenderError::BadBlockId));
        assert_eq!(render("core", "", REV, ""), Err(RenderError::BadSourceRepo));
        assert_eq!(render("core", "r", "abc", ""), Err(RenderError::BadSourceRev));
        assert_eq!(render("core", "r", REV, "a\n  <!-- house-rules:x"), Err(RenderError::MarkerInBody { line: 2 }));
        assert_eq!(render("core", "r", REV, "a\r"), Err(RenderError::TrailingCr));
    }
}
