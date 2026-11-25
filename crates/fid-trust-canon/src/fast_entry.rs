//! Zero-allocation streaming entry-line parser and hasher (`inspect_entry_line`) using
//! reusable flat-arena scratch buffers ([`EntryScratch`]).
//!
//! Enforces the exact same RFC 8259 grammar, limits, duplicate-key rejection, lone-surrogate
//! rejection, numeric convention rules (`U64_FIELDS`, `MAX_SAFE_INTEGER`), and RFC 8785
//! canonicalisation + SHA-256 entry hash as [`crate::parse_with_limits`] +
//! [`crate::entry_hash_value`], while avoiding per-entry heap allocations so 1,000,000-entry
//! segments (`TRUST.CHAIN.LINKS` AC-3) verify in seconds with O(1) working memory.

use crate::entry::{ConventionRule, MAX_SAFE_INTEGER, U64_FIELDS};
use crate::number::format_number;
use crate::write::{cmp_utf16, write_string};
use crate::{CanonError, InvalidKind, Limits, MAX_DEPTH, MAX_INPUT_BYTES};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::borrow::Cow;
use std::fmt::Write as _;

const MAX_SAFE_U64: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy)]
enum Span {
    /// Raw UTF-8 slice directly inside the input `line` (no backslash escapes).
    Input(usize, usize),
    /// Unescaped UTF-8 slice inside `EntryScratch::escaped_bytes`.
    Escaped(usize, usize),
}

#[derive(Debug, Clone, Copy)]
enum ArenaNode {
    Null,
    Bool(bool),
    Number(f64),
    String(Span),
    /// Contiguous range `items[start..start + len]`.
    Array {
        start: usize,
        len: usize,
    },
    /// Contiguous range `members[start..start + len]`.
    Object {
        start: usize,
        len: usize,
    },
}

/// Reusable scratch arena for [`inspect_entry_line`]. Holding one instance across a whole
/// `journal/segment.jsonl` stream eliminates per-entry heap allocations.
#[derive(Debug, Default)]
pub struct EntryScratch {
    nodes: Vec<ArenaNode>,
    members: Vec<(Span, usize)>,
    items: Vec<usize>,
    level_members: Vec<Vec<(Span, usize)>>,
    level_items: Vec<Vec<usize>>,
    escaped_bytes: Vec<u8>,
    canon_buf: Vec<u8>,
    path_buf: String,
    sort_buf: Vec<usize>,
}

impl EntryScratch {
    /// Allocates an empty scratch arena with modest initial capacities.
    pub fn new() -> Self {
        EntryScratch {
            nodes: Vec::with_capacity(64),
            members: Vec::with_capacity(64),
            items: Vec::with_capacity(32),
            level_members: Vec::with_capacity(8),
            level_items: Vec::with_capacity(8),
            escaped_bytes: Vec::with_capacity(256),
            canon_buf: Vec::with_capacity(2048),
            path_buf: String::with_capacity(64),
            sort_buf: Vec::with_capacity(32),
        }
    }

    fn reset(&mut self) {
        self.nodes.clear();
        self.members.clear();
        self.items.clear();
        for v in &mut self.level_members {
            v.clear();
        }
        for v in &mut self.level_items {
            v.clear();
        }
        self.escaped_bytes.clear();
        self.canon_buf.clear();
        self.path_buf.clear();
        self.sort_buf.clear();
    }

    fn resolve<'a>(&'a self, input: &'a str, span: Span) -> &'a str {
        match span {
            Span::Input(s, e) => input.get(s..e).unwrap_or(""),
            Span::Escaped(s, e) => self.escaped_bytes.get(s..e).and_then(|b| std::str::from_utf8(b).ok()).unwrap_or(""),
        }
    }
}

/// Extracted header fields and recomputed entry hash from one `journal/segment.jsonl` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedEntry<'a> {
    /// Recomputed SHA-256 of the RFC 8785 canonical header omitting `"hash"`.
    pub recomputed_hash: Digest,
    /// Raw 32-byte SHA-256 matching `recomputed_hash.to_bytes()`.
    pub recomputed_bytes: [u8; 32],
    /// Raw string value of `header.hash`, if present and a JSON string.
    pub stored_hash: Option<Cow<'a, str>>,
    /// Raw string value of `header.prev`, if present and a JSON string.
    pub stored_prev: Option<Cow<'a, str>>,
    /// Raw string value of `header.seq`, if present and a JSON string.
    pub seq: Option<Cow<'a, str>>,
    /// Raw string value of `header.kind`, if present and a JSON string.
    pub kind: Option<Cow<'a, str>>,
    /// Raw string value of `header.stream`, if present and a JSON string.
    pub stream: Option<Cow<'a, str>>,
    /// True if `header.hash` was present with a non-string JSON type.
    pub hash_non_string: bool,
    /// True if `header.prev` was present with a non-string JSON type.
    pub prev_non_string: bool,
    /// True if `header.seq` was present with a non-string JSON type.
    pub seq_non_string: bool,
}

/// Classification of structural entry-line errors distinct from [`CanonError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectError {
    /// RFC 8259 / RFC 8785 / numeric-convention failure (`canon.invalid` or `canon.convention`).
    Canon(CanonError),
    /// Line parsed as valid JSON, but is not an object or lacks a valid `header` object.
    Structure { reason: &'static str, detail: &'static str },
}

impl From<CanonError> for InspectError {
    fn from(e: CanonError) -> Self {
        InspectError::Canon(e)
    }
}

/// Parses one JSONL entry line (`{"header": {...}, ...}` or a bare header object), checks all
/// RFC 8259/8785 and journal numeric conventions, recomputes the canonical entry hash omitting
/// `"hash"`, and extracts the chain-continuity header fields without per-entry heap allocations.
pub fn inspect_entry_line<'a>(
    line: &'a [u8],
    limits: Limits,
    scratch: &'a mut EntryScratch,
) -> Result<InspectedEntry<'a>, InspectError> {
    let max_bytes = limits.max_bytes.min(MAX_INPUT_BYTES);
    let max_depth = limits.max_depth.min(MAX_DEPTH);
    if line.len() > max_bytes {
        return Err(InspectError::Canon(CanonError::invalid(InvalidKind::TooLarge, max_bytes)));
    }
    let input = match std::str::from_utf8(line) {
        Ok(s) => s,
        Err(e) => return Err(InspectError::Canon(CanonError::invalid(InvalidKind::Utf8, e.valid_up_to()))),
    };

    // Single-pass zero-copy fast path for canonical ASCII `{"header":{...}}` lines.
    // Enforces every RFC 8259/8785, depth, duplicate-key, and numeric-convention rule;
    // any non-canonical feature, escape, non-ASCII byte, or error falls through to the
    // general arena parser below.
    if input.is_ascii() {
        if let Some(fast) = try_inspect_canonical_line(line, input, max_depth) {
            return Ok(fast);
        }
    }

    scratch.reset();
    let root_idx = {
        let mut p = ArenaParser { b: line, input, pos: 0, max_depth, s: scratch };
        p.skip_ws();
        let root = p.value(0)?;
        p.skip_ws();
        if p.pos != line.len() {
            return Err(InspectError::Canon(p.err(InvalidKind::Syntax)));
        }
        root
    };

    let Some(ArenaNode::Object { start: root_start, len: root_len }) = scratch.nodes.get(root_idx).copied() else {
        return Err(InspectError::Structure { reason: "entry_not_object", detail: "entry line must be a JSON object" });
    };

    // Locate header object: either `root["header"]` (standard CORE.JRN.ENTRY format) or `root`
    // directly when `seq` and `prev` sit at top level (CKPT bare-header format).
    let mut header_node_opt: Option<usize> = None;
    let mut has_seq = false;
    let mut has_prev = false;
    for i in root_start..root_start + root_len {
        let Some(&(kspan, vidx)) = scratch.members.get(i) else { break };
        match scratch.resolve(input, kspan) {
            "header" => header_node_opt = Some(vidx),
            "seq" => has_seq = true,
            "prev" => has_prev = true,
            _ => {}
        }
    }

    let header_idx = match header_node_opt {
        Some(idx) => match scratch.nodes.get(idx).copied() {
            Some(ArenaNode::Object { .. }) => idx,
            _ => {
                return Err(InspectError::Structure {
                    reason: "header_not_object",
                    detail: "entry header must be a JSON object",
                });
            }
        },
        None if has_seq && has_prev => root_idx,
        None => {
            return Err(InspectError::Structure { reason: "missing_header", detail: "entry missing header object" });
        }
    };

    // Check numeric conventions (`canon.convention`) on the header tree.
    check_conventions(scratch, input, header_idx, 0)?;

    // Canonicalise the header omitting top-level `"hash"` directly into `scratch.canon_buf`.
    let Some(ArenaNode::Object { start: h_start, len: h_len }) = scratch.nodes.get(header_idx).copied() else {
        return Err(InspectError::Structure {
            reason: "header_not_object",
            detail: "entry header must be a JSON object",
        });
    };
    write_arena_object(scratch, input, h_start, h_len, Some("hash"), 1)?;
    let recomputed_bytes: [u8; 32] = Sha256::digest(&scratch.canon_buf).into();
    let recomputed_hash = Digest::from_bytes(recomputed_bytes);

    // Extract continuity fields from the header.
    let mut stored_hash = None;
    let mut stored_prev = None;
    let mut seq = None;
    let mut kind = None;
    let mut stream = None;
    let mut hash_non_string = false;
    let mut prev_non_string = false;
    let mut seq_non_string = false;

    for i in h_start..h_start + h_len {
        let Some(&(kspan, vidx)) = scratch.members.get(i) else { break };
        let k = scratch.resolve(input, kspan);
        let node = scratch.nodes.get(vidx).copied();
        match k {
            "hash" => match node {
                Some(ArenaNode::String(sp)) => stored_hash = Some(span_to_cow(scratch, input, sp)),
                Some(_) => hash_non_string = true,
                None => {}
            },
            "prev" => match node {
                Some(ArenaNode::String(sp)) => stored_prev = Some(span_to_cow(scratch, input, sp)),
                Some(_) => prev_non_string = true,
                None => {}
            },
            "seq" => match node {
                Some(ArenaNode::String(sp)) => seq = Some(span_to_cow(scratch, input, sp)),
                Some(_) => seq_non_string = true,
                None => {}
            },
            "kind" => {
                if let Some(ArenaNode::String(sp)) = node {
                    kind = Some(span_to_cow(scratch, input, sp));
                }
            }
            "stream" => {
                if let Some(ArenaNode::String(sp)) = node {
                    stream = Some(span_to_cow(scratch, input, sp));
                }
            }
            _ => {}
        }
    }

    Ok(InspectedEntry {
        recomputed_hash,
        recomputed_bytes,
        stored_hash,
        stored_prev,
        seq,
        kind,
        stream,
        hash_non_string,
        prev_non_string,
        seq_non_string,
    })
}

fn span_to_cow<'a>(scratch: &'a EntryScratch, input: &'a str, sp: Span) -> Cow<'a, str> {
    match sp {
        Span::Input(s, e) => Cow::Borrowed(input.get(s..e).unwrap_or("")),
        Span::Escaped(s, e) => {
            Cow::Borrowed(scratch.escaped_bytes.get(s..e).and_then(|b| std::str::from_utf8(b).ok()).unwrap_or(""))
        }
    }
}

// ------------------------------------------------------------------ single-pass canonical fast path

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathCtx {
    Time,
    Command,
    Other,
    SkipConventions,
}

struct FastCanonScanner<'a> {
    b: &'a [u8],
    input: &'a str,
    pos: usize,
    max_depth: usize,
}

impl<'a> FastCanonScanner<'a> {
    #[inline]
    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    /// Scans an unescaped ASCII JSON string (`"..."`), returning its content slice (excluding
    /// quotes) or `None` if an escape (`\`), control byte (`< 0x20`), or syntax error is seen.
    #[inline]
    fn scan_string(&mut self) -> Option<&'a str> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.pos += 1;
        let start = self.pos;
        let bytes = self.b;
        while let Some(&c) = bytes.get(self.pos) {
            if c == b'"' {
                let end = self.pos;
                self.pos += 1;
                return self.input.get(start..end);
            }
            if c == b'\\' || c < 0x20 {
                return None;
            }
            self.pos += 1;
        }
        None
    }

    /// Scans a canonical safe-integer JSON number (`0` or `[-][1-9][0-9]*` with `|n| <= 2^53 - 1`,
    /// no fraction, no exponent, no `-0`). Any other numeric form returns `None` to fall back.
    #[inline]
    fn scan_canon_int(&mut self) -> Option<()> {
        let mut p = self.pos;
        let bytes = self.b;
        if bytes.get(p) == Some(&b'0') {
            p += 1;
            if matches!(bytes.get(p), Some(b'0'..=b'9' | b'.' | b'e' | b'E')) {
                return None;
            }
            self.pos = p;
            return Some(());
        }
        if bytes.get(p) == Some(&b'-') {
            p += 1;
        }
        let first = *bytes.get(p)?;
        if !(b'1'..=b'9').contains(&first) {
            return None;
        }
        let mut val = u64::from(first - b'0');
        p += 1;
        while let Some(&c) = bytes.get(p) {
            if c.is_ascii_digit() {
                val = val.checked_mul(10)?.checked_add(u64::from(c - b'0'))?;
                if val > MAX_SAFE_U64 {
                    return None;
                }
                p += 1;
            } else {
                break;
            }
        }
        if matches!(bytes.get(p), Some(b'.' | b'e' | b'E')) {
            return None;
        }
        self.pos = p;
        Some(())
    }

    fn scan_value(&mut self, depth: usize, ctx: PathCtx) -> Option<()> {
        match self.peek()? {
            b'"' => {
                self.scan_string()?;
                Some(())
            }
            b'{' => self.scan_object(depth + 1, ctx),
            b'[' => self.scan_array(depth + 1, ctx),
            b't' => {
                if self.b.get(self.pos..self.pos + 4) == Some(b"true") {
                    self.pos += 4;
                    Some(())
                } else {
                    None
                }
            }
            b'f' => {
                if self.b.get(self.pos..self.pos + 5) == Some(b"false") {
                    self.pos += 5;
                    Some(())
                } else {
                    None
                }
            }
            b'n' => {
                if self.b.get(self.pos..self.pos + 4) == Some(b"null") {
                    self.pos += 4;
                    Some(())
                } else {
                    None
                }
            }
            b'-' | b'0'..=b'9' => self.scan_canon_int(),
            _ => None,
        }
    }

    fn scan_array(&mut self, depth: usize, ctx: PathCtx) -> Option<()> {
        if depth > self.max_depth {
            return None;
        }
        self.pos += 1; // '['
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(());
        }
        let child_ctx = if ctx == PathCtx::SkipConventions { PathCtx::SkipConventions } else { PathCtx::Other };
        loop {
            self.scan_value(depth, child_ctx)?;
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Some(());
                }
                _ => return None,
            }
        }
    }

    fn scan_object(&mut self, depth: usize, ctx: PathCtx) -> Option<()> {
        if depth > self.max_depth {
            return None;
        }
        self.pos += 1; // '{'
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(());
        }
        let mut prev_key: Option<&[u8]> = None;
        loop {
            let key = self.scan_string()?;
            let kb = key.as_bytes();
            if let Some(pk) = prev_key {
                // ASCII-only line: byte comparison equals UTF-16 code-unit order and
                // strict inequality enforces both canonical key order and zero duplicates.
                if pk >= kb {
                    return None;
                }
            }
            prev_key = Some(kb);
            if self.peek() != Some(b':') {
                return None;
            }
            self.pos += 1;
            match (ctx, kb) {
                (PathCtx::Time, b"mono_ns") | (PathCtx::Command, b"seed") => {
                    let s = self.scan_string()?;
                    if !is_u64_decimal(s) {
                        return None;
                    }
                }
                (PathCtx::SkipConventions, _) => {
                    self.scan_value(depth, PathCtx::SkipConventions)?;
                }
                _ => {
                    self.scan_value(depth, PathCtx::Other)?;
                }
            }
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Some(());
                }
                _ => return None,
            }
        }
    }
}

/// Attempts zero-copy single-pass verification and hashing of a canonical ASCII
/// `{"header":{...}}` journal line. Returns `None` on any non-canonical construct,
/// escape, non-ASCII byte, convention error, or syntax error so the general arena parser
/// handles it identically.
fn try_inspect_canonical_line<'a>(line: &'a [u8], input: &'a str, max_depth: usize) -> Option<InspectedEntry<'a>> {
    const PREFIX: &[u8] = b"{\"header\":{";
    if max_depth < 2 || !line.starts_with(PREFIX) {
        return None;
    }

    let header_start = PREFIX.len() - 1; // index of '{' starting `header`
    let mut sc = FastCanonScanner { b: line, input, pos: PREFIX.len(), max_depth };

    let mut stored_hash = None;
    let mut stored_prev = None;
    let mut seq = None;
    let mut kind = None;
    let mut stream = None;
    let mut hash_non_string = false;
    let mut prev_non_string = false;
    let mut hash_omit: Option<(usize, usize)> = None;

    if sc.peek() == Some(b'}') {
        sc.pos += 1;
    } else {
        let mut prev_key: Option<&[u8]> = None;
        let mut member_idx: usize = 0;
        loop {
            let comma_pos = if member_idx > 0 { Some(sc.pos - 1) } else { None };
            let key_quote = sc.pos;
            let key = sc.scan_string()?;
            let kb = key.as_bytes();
            if let Some(pk) = prev_key {
                if pk >= kb {
                    return None;
                }
            }
            prev_key = Some(kb);
            if sc.peek() != Some(b':') {
                return None;
            }
            sc.pos += 1;

            match kb {
                b"hash" => {
                    if sc.peek() == Some(b'"') {
                        stored_hash = Some(Cow::Borrowed(sc.scan_string()?));
                    } else {
                        hash_non_string = true;
                        sc.scan_value(2, PathCtx::SkipConventions)?;
                    }
                    let val_end = sc.pos;
                    if let Some(cp) = comma_pos {
                        hash_omit = Some((cp, val_end));
                    } else if sc.peek() == Some(b',') {
                        hash_omit = Some((key_quote, val_end + 1));
                    } else {
                        hash_omit = Some((key_quote, val_end));
                    }
                }
                b"seq" => {
                    let s = sc.scan_string()?;
                    if !is_u64_decimal(s) {
                        return None;
                    }
                    seq = Some(Cow::Borrowed(s));
                }
                b"prev" => {
                    if sc.peek() == Some(b'"') {
                        stored_prev = Some(Cow::Borrowed(sc.scan_string()?));
                    } else {
                        prev_non_string = true;
                        sc.scan_value(2, PathCtx::Other)?;
                    }
                }
                b"kind" => {
                    if sc.peek() == Some(b'"') {
                        kind = Some(Cow::Borrowed(sc.scan_string()?));
                    } else {
                        sc.scan_value(2, PathCtx::Other)?;
                    }
                }
                b"stream" => {
                    if sc.peek() == Some(b'"') {
                        stream = Some(Cow::Borrowed(sc.scan_string()?));
                    } else {
                        sc.scan_value(2, PathCtx::Other)?;
                    }
                }
                b"time" => sc.scan_value(2, PathCtx::Time)?,
                b"command" => sc.scan_value(2, PathCtx::Command)?,
                _ => sc.scan_value(2, PathCtx::Other)?,
            }

            match sc.peek()? {
                b',' => {
                    sc.pos += 1;
                    member_idx += 1;
                }
                b'}' => {
                    sc.pos += 1;
                    break;
                }
                _ => return None,
            }
        }
    }
    let header_end = sc.pos;

    // Validate the closing outer wrapper (and any trailing canonical sibling fields such as `"payload"`).
    match sc.peek()? {
        b'}' => {
            sc.pos += 1;
        }
        b',' => {
            sc.pos += 1;
            let mut prev_outer: &[u8] = b"header";
            loop {
                let k = sc.scan_string()?.as_bytes();
                if prev_outer >= k {
                    return None;
                }
                prev_outer = k;
                if sc.peek() != Some(b':') {
                    return None;
                }
                sc.pos += 1;
                sc.scan_value(1, PathCtx::SkipConventions)?;
                match sc.peek()? {
                    b',' => sc.pos += 1,
                    b'}' => {
                        sc.pos += 1;
                        break;
                    }
                    _ => return None,
                }
            }
        }
        _ => return None,
    }
    if sc.pos != line.len() {
        return None;
    }

    // Header bytes are already canonical RFC 8785; hash directly from `line` without copying.
    let recomputed_bytes: [u8; 32] = match hash_omit {
        None => Sha256::digest(line.get(header_start..header_end)?).into(),
        Some((omit_start, omit_end)) => {
            let mut hasher = Sha256::new();
            hasher.update(line.get(header_start..omit_start)?);
            hasher.update(line.get(omit_end..header_end)?);
            hasher.finalize().into()
        }
    };
    let recomputed_hash = Digest::from_bytes(recomputed_bytes);

    Some(InspectedEntry {
        recomputed_hash,
        recomputed_bytes,
        stored_hash,
        stored_prev,
        seq,
        kind,
        stream,
        hash_non_string,
        prev_non_string,
        seq_non_string: false,
    })
}

// ------------------------------------------------------------------ parser

struct ArenaParser<'a> {
    b: &'a [u8],
    input: &'a str,
    pos: usize,
    max_depth: usize,
    s: &'a mut EntryScratch,
}

impl ArenaParser<'_> {
    fn err(&self, kind: InvalidKind) -> CanonError {
        CanonError::invalid(kind, self.pos)
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.pos += 1;
        }
    }

    fn eat(&mut self, want: u8) -> Result<(), CanonError> {
        if self.peek() == Some(want) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.err(InvalidKind::Syntax))
        }
    }

    fn enter(&self, depth: usize) -> Result<(), CanonError> {
        if depth > self.max_depth {
            Err(self.err(InvalidKind::TooDeep))
        } else {
            Ok(())
        }
    }

    fn push_node(&mut self, node: ArenaNode) -> usize {
        let idx = self.s.nodes.len();
        self.s.nodes.push(node);
        idx
    }

    fn value(&mut self, depth: usize) -> Result<usize, CanonError> {
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => {
                let sp = self.string()?;
                Ok(self.push_node(ArenaNode::String(sp)))
            }
            Some(b't') => self.literal(b"true", ArenaNode::Bool(true)),
            Some(b'f') => self.literal(b"false", ArenaNode::Bool(false)),
            Some(b'n') => self.literal(b"null", ArenaNode::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.err(InvalidKind::Syntax)),
        }
    }

    fn literal(&mut self, word: &[u8], node: ArenaNode) -> Result<usize, CanonError> {
        if self.b.get(self.pos..self.pos + word.len()) == Some(word) {
            self.pos += word.len();
            Ok(self.push_node(node))
        } else {
            Err(self.err(InvalidKind::Syntax))
        }
    }

    fn array(&mut self, depth: usize) -> Result<usize, CanonError> {
        self.enter(depth)?;
        self.pos += 1;
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(self.push_node(ArenaNode::Array { start: 0, len: 0 }));
        }
        while self.s.level_items.len() <= depth {
            self.s.level_items.push(Vec::new());
        }
        if let Some(buf) = self.s.level_items.get_mut(depth) {
            buf.clear();
        }
        loop {
            self.skip_ws();
            let v = self.value(depth)?;
            if let Some(buf) = self.s.level_items.get_mut(depth) {
                buf.push(v);
            }
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.err(InvalidKind::Syntax)),
            }
        }
        let start = self.s.items.len();
        let len = if let Some(buf) = self.s.level_items.get_mut(depth) {
            let n = buf.len();
            self.s.items.extend_from_slice(buf);
            buf.clear();
            n
        } else {
            0
        };
        Ok(self.push_node(ArenaNode::Array { start, len }))
    }

    fn object(&mut self, depth: usize) -> Result<usize, CanonError> {
        self.enter(depth)?;
        let obj_start = self.pos;
        self.pos += 1;
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(self.push_node(ArenaNode::Object { start: 0, len: 0 }));
        }
        while self.s.level_members.len() <= depth {
            self.s.level_members.push(Vec::new());
        }
        if let Some(buf) = self.s.level_members.get_mut(depth) {
            buf.clear();
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err(InvalidKind::Syntax));
            }
            let key = self.string()?;
            self.skip_ws();
            self.eat(b':')?;
            self.skip_ws();
            let val = self.value(depth)?;
            if let Some(buf) = self.s.level_members.get_mut(depth) {
                buf.push((key, val));
            }
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.err(InvalidKind::Syntax)),
            }
        }

        let start = self.s.members.len();
        let len = if let Some(buf) = self.s.level_members.get_mut(depth) {
            let n = buf.len();
            self.s.members.extend_from_slice(buf);
            buf.clear();
            n
        } else {
            0
        };

        // Fast duplicate-key check: O(n) when strictly sorted (canonical JSON), else O(n log n).
        let input = self.input;
        let members = &self.s.members[start..start + len];
        let escaped = &self.s.escaped_bytes;
        let key_str = |sp: Span| -> &str {
            match sp {
                Span::Input(a, b) => input.get(a..b).unwrap_or(""),
                Span::Escaped(a, b) => escaped.get(a..b).and_then(|x| std::str::from_utf8(x).ok()).unwrap_or(""),
            }
        };
        if !members.windows(2).all(|w| key_str(w[0].0) < key_str(w[1].0)) {
            self.s.sort_buf.clear();
            self.s.sort_buf.extend(0..len);
            let members_ref = &self.s.members[start..start + len];
            let escaped_ref = &self.s.escaped_bytes;
            let kstr = |sp: Span| -> &str {
                match sp {
                    Span::Input(a, b) => input.get(a..b).unwrap_or(""),
                    Span::Escaped(a, b) => {
                        escaped_ref.get(a..b).and_then(|x| std::str::from_utf8(x).ok()).unwrap_or("")
                    }
                }
            };
            self.s.sort_buf.sort_unstable_by(|&a, &b| kstr(members_ref[a].0).cmp(kstr(members_ref[b].0)));
            if self.s.sort_buf.windows(2).any(|w| kstr(members_ref[w[0]].0) == kstr(members_ref[w[1]].0)) {
                return Err(CanonError::invalid(InvalidKind::DuplicateKey, obj_start));
            }
        }

        Ok(self.push_node(ArenaNode::Object { start, len }))
    }

    fn hex4(&mut self) -> Result<u32, CanonError> {
        let mut cp = 0u32;
        for _ in 0..4 {
            let d = match self.peek() {
                Some(c @ b'0'..=b'9') => u32::from(c - b'0'),
                Some(c @ b'a'..=b'f') => u32::from(c - b'a') + 10,
                Some(c @ b'A'..=b'F') => u32::from(c - b'A') + 10,
                _ => return Err(self.err(InvalidKind::Syntax)),
            };
            cp = cp * 16 + d;
            self.pos += 1;
        }
        Ok(cp)
    }

    fn string(&mut self) -> Result<Span, CanonError> {
        self.pos += 1;
        let start = self.pos;
        let bytes = self.b;
        while let Some(&c) = bytes.get(self.pos) {
            if c == b'"' {
                let end = self.pos;
                self.pos += 1;
                return Ok(Span::Input(start, end));
            }
            if c == b'\\' {
                break;
            }
            if c < 0x20 {
                return Err(self.err(InvalidKind::Syntax));
            }
            self.pos += 1;
        }
        let esc_start = self.s.escaped_bytes.len();
        self.s.escaped_bytes.extend_from_slice(&bytes[start..self.pos]);
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err(InvalidKind::Syntax));
            };
            match c {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    let at = self.pos;
                    self.pos += 1;
                    let Some(e) = self.peek() else {
                        return Err(self.err(InvalidKind::Syntax));
                    };
                    self.pos += 1;
                    let short = match e {
                        b'"' => Some(b'"'),
                        b'\\' => Some(b'\\'),
                        b'/' => Some(b'/'),
                        b'b' => Some(0x08),
                        b'f' => Some(0x0c),
                        b'n' => Some(b'\n'),
                        b'r' => Some(b'\r'),
                        b't' => Some(b'\t'),
                        b'u' => None,
                        _ => return Err(CanonError::invalid(InvalidKind::Syntax, at)),
                    };
                    if let Some(byte) = short {
                        self.s.escaped_bytes.push(byte);
                        continue;
                    }
                    let unit = self.hex4()?;
                    let cp = if (0xD800..0xDC00).contains(&unit) {
                        if self.b.get(self.pos) != Some(&b'\\') || self.b.get(self.pos + 1) != Some(&b'u') {
                            return Err(CanonError::invalid(InvalidKind::LoneSurrogate, at));
                        }
                        self.pos += 2;
                        let low = self.hex4()?;
                        if !(0xDC00..0xE000).contains(&low) {
                            return Err(CanonError::invalid(InvalidKind::LoneSurrogate, at));
                        }
                        0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
                    } else if (0xDC00..0xE000).contains(&unit) {
                        return Err(CanonError::invalid(InvalidKind::LoneSurrogate, at));
                    } else {
                        unit
                    };
                    let Some(ch) = char::from_u32(cp) else {
                        return Err(CanonError::invalid(InvalidKind::LoneSurrogate, at));
                    };
                    let mut tmp = [0u8; 4];
                    self.s.escaped_bytes.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                }
                0x00..=0x1f => return Err(self.err(InvalidKind::Syntax)),
                _ => {
                    let span_start = self.pos;
                    self.pos += 1;
                    while let Some(&b) = bytes.get(self.pos) {
                        if b == b'"' || b == b'\\' || b < 0x20 {
                            break;
                        }
                        self.pos += 1;
                    }
                    self.s.escaped_bytes.extend_from_slice(&bytes[span_start..self.pos]);
                }
            }
        }
        let esc_end = self.s.escaped_bytes.len();
        if std::str::from_utf8(&self.s.escaped_bytes[esc_start..esc_end]).is_err() {
            return Err(self.err(InvalidKind::Utf8));
        }
        Ok(Span::Escaped(esc_start, esc_end))
    }

    fn digits(&mut self) -> usize {
        let from = self.pos;
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
        self.pos - from
    }

    fn number(&mut self) -> Result<usize, CanonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.err(InvalidKind::Syntax)),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return Err(self.err(InvalidKind::Syntax));
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(self.err(InvalidKind::Syntax));
            }
        }
        let text = self.input.get(start..self.pos).ok_or_else(|| CanonError::invalid(InvalidKind::Syntax, start))?;
        let v: f64 = text.parse().map_err(|_| CanonError::invalid(InvalidKind::Syntax, start))?;
        if !v.is_finite() {
            return Err(CanonError::invalid(InvalidKind::NumberRange, start));
        }
        Ok(self.push_node(ArenaNode::Number(v)))
    }
}

// ------------------------------------------------------------------ conventions & canonicalisation

fn is_u64_decimal(s: &str) -> bool {
    let plain = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    plain && s.parse::<u64>().is_ok()
}

fn push_pointer_segment(path: &mut String, name: &str) {
    path.push('/');
    if !name.bytes().any(|b| b == b'~' || b == b'/') {
        path.push_str(name);
        return;
    }
    for ch in name.chars() {
        match ch {
            '~' => path.push_str("~0"),
            '/' => path.push_str("~1"),
            other => path.push(other),
        }
    }
}

fn check_conventions(s: &mut EntryScratch, input: &str, node_idx: usize, depth: usize) -> Result<(), CanonError> {
    let u64_field = U64_FIELDS.contains(&s.path_buf.as_str());
    let Some(node) = s.nodes.get(node_idx).copied() else {
        return Ok(());
    };
    match node {
        ArenaNode::String(sp) if u64_field => {
            if !is_u64_decimal(s.resolve(input, sp)) {
                return Err(CanonError::Convention { path: s.path_buf.clone(), rule: ConventionRule::DecimalString });
            }
        }
        _ if u64_field => {
            return Err(CanonError::Convention { path: s.path_buf.clone(), rule: ConventionRule::DecimalString });
        }
        ArenaNode::Number(x) => {
            if x.fract() != 0.0 || x.abs() > MAX_SAFE_INTEGER {
                return Err(CanonError::Convention { path: s.path_buf.clone(), rule: ConventionRule::RawNumber });
            }
        }
        ArenaNode::Array { start, len } => {
            if depth + 1 > MAX_DEPTH {
                return Err(CanonError::invalid(InvalidKind::TooDeep, 0));
            }
            for i in 0..len {
                let Some(&child) = s.items.get(start + i) else { break };
                let prev_len = s.path_buf.len();
                s.path_buf.push('/');
                let _ = write!(&mut s.path_buf, "{i}");
                check_conventions(s, input, child, depth + 1)?;
                s.path_buf.truncate(prev_len);
            }
        }
        ArenaNode::Object { start, len } => {
            if depth + 1 > MAX_DEPTH {
                return Err(CanonError::invalid(InvalidKind::TooDeep, 0));
            }
            for i in 0..len {
                let Some(&(kspan, child)) = s.members.get(start + i) else { break };
                // Avoid borrowing `s` across recursion by copying/checking directly.
                let is_top_hash = depth == 0 && s.resolve(input, kspan) == "hash";
                if is_top_hash {
                    continue;
                }
                let prev_len = s.path_buf.len();
                match kspan {
                    Span::Input(a, b) => {
                        let name = input.get(a..b).unwrap_or("");
                        push_pointer_segment(&mut s.path_buf, name);
                    }
                    Span::Escaped(a, b) => {
                        let owned = s
                            .escaped_bytes
                            .get(a..b)
                            .and_then(|x| std::str::from_utf8(x).ok())
                            .unwrap_or("")
                            .to_owned();
                        push_pointer_segment(&mut s.path_buf, &owned);
                    }
                }
                check_conventions(s, input, child, depth + 1)?;
                s.path_buf.truncate(prev_len);
            }
        }
        ArenaNode::Null | ArenaNode::Bool(_) | ArenaNode::String(_) => {}
    }
    Ok(())
}

fn write_arena_object(
    s: &mut EntryScratch,
    input: &str,
    start: usize,
    len: usize,
    omit_key: Option<&str>,
    depth: usize,
) -> Result<(), CanonError> {
    if depth > MAX_DEPTH {
        return Err(CanonError::invalid(InvalidKind::TooDeep, 0));
    }

    // Check if already in strictly ascending UTF-16 key order (true for canonical journals).
    let mut prev: Option<Span> = None;
    let mut already_sorted = true;
    for i in 0..len {
        let Some(&(kspan, _)) = s.members.get(start + i) else { break };
        let k = s.resolve(input, kspan);
        if omit_key == Some(k) {
            continue;
        }
        if let Some(p) = prev {
            if cmp_utf16(s.resolve(input, p), k) != std::cmp::Ordering::Less {
                already_sorted = false;
                break;
            }
        }
        prev = Some(kspan);
    }

    s.canon_buf.push(b'{');
    if already_sorted {
        let mut first = true;
        for i in 0..len {
            let Some(&(kspan, child)) = s.members.get(start + i) else { break };
            if omit_key.is_some_and(|ok| s.resolve(input, kspan) == ok) {
                continue;
            }
            if !first {
                s.canon_buf.push(b',');
            }
            first = false;
            write_span_string(s, input, kspan);
            s.canon_buf.push(b':');
            write_arena_value(s, input, child, depth)?;
        }
    } else {
        let mut order: Vec<usize> = (0..len)
            .filter(|&i| {
                s.members
                    .get(start + i)
                    .is_some_and(|&(kspan, _)| !omit_key.is_some_and(|ok| s.resolve(input, kspan) == ok))
            })
            .collect();
        order.sort_by(|&a, &b| {
            let ka = s.members.get(start + a).map_or("", |&(sp, _)| s.resolve(input, sp));
            let kb = s.members.get(start + b).map_or("", |&(sp, _)| s.resolve(input, sp));
            cmp_utf16(ka, kb)
        });
        if order.windows(2).any(|w| {
            let ka = s.members.get(start + w[0]).map_or("", |&(sp, _)| s.resolve(input, sp));
            let kb = s.members.get(start + w[1]).map_or("", |&(sp, _)| s.resolve(input, sp));
            ka == kb
        }) {
            return Err(CanonError::invalid(InvalidKind::DuplicateKey, 0));
        }
        for (pos, i) in order.into_iter().enumerate() {
            let Some(&(kspan, child)) = s.members.get(start + i) else { break };
            if pos > 0 {
                s.canon_buf.push(b',');
            }
            write_span_string(s, input, kspan);
            s.canon_buf.push(b':');
            write_arena_value(s, input, child, depth)?;
        }
    }
    s.canon_buf.push(b'}');
    Ok(())
}

fn write_arena_value(s: &mut EntryScratch, input: &str, node_idx: usize, depth: usize) -> Result<(), CanonError> {
    let Some(node) = s.nodes.get(node_idx).copied() else {
        return Ok(());
    };
    match node {
        ArenaNode::Null => s.canon_buf.extend_from_slice(b"null"),
        ArenaNode::Bool(true) => s.canon_buf.extend_from_slice(b"true"),
        ArenaNode::Bool(false) => s.canon_buf.extend_from_slice(b"false"),
        ArenaNode::Number(x) => {
            let formatted = format_number(x)?;
            s.canon_buf.extend_from_slice(formatted.as_bytes());
        }
        ArenaNode::String(sp) => write_span_string(s, input, sp),
        ArenaNode::Array { start, len } => {
            if depth + 1 > MAX_DEPTH {
                return Err(CanonError::invalid(InvalidKind::TooDeep, 0));
            }
            s.canon_buf.push(b'[');
            for i in 0..len {
                if i > 0 {
                    s.canon_buf.push(b',');
                }
                let Some(&child) = s.items.get(start + i) else { break };
                write_arena_value(s, input, child, depth + 1)?;
            }
            s.canon_buf.push(b']');
        }
        ArenaNode::Object { start, len } => write_arena_object(s, input, start, len, None, depth + 1)?,
    }
    Ok(())
}

fn write_span_string(s: &mut EntryScratch, input: &str, sp: Span) {
    match sp {
        Span::Input(a, b) => {
            // An unescaped input span contains no `"`, `\` or `< 0x20`, so emit quotes + raw bytes directly.
            s.canon_buf.push(b'"');
            if let Some(slice) = input.as_bytes().get(a..b) {
                s.canon_buf.extend_from_slice(slice);
            }
            s.canon_buf.push(b'"');
        }
        Span::Escaped(a, b) => {
            let owned = s.escaped_bytes.get(a..b).and_then(|x| std::str::from_utf8(x).ok()).unwrap_or("").to_owned();
            write_string(&owned, &mut s.canon_buf);
        }
    }
}
