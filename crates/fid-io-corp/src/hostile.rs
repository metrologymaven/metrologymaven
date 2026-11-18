//! Hostile corpus generator. Spec node `IO.CORP.HOSTILE`; CI binary `corpus-hostile`.
//!
//! For every accepted, in-repository file of every corpus manifest, the mutators of the
//! format's plan make small hostile variants (truncation, seeded bit flips, inflated declared
//! counts, deep XML nesting, XML billion laughs, overlapping ZIP bombs). Each variant gets the
//! typed error a reader must return: the fixed [`Mutator::default_error`] table, or a
//! per-format override with a recorded reason. Variants are written under one output
//! directory, next to a generated manifest `<format-id>.toml` per format whose entries are
//! tagged `hostile` and expect `reject{error}`, so the output directory is itself a corpus.
//!
//! Determinism D2: the only randomness is the Philox counter-based RNG of `fid-num`, keyed by
//! the seed and by a stream derived from (file digest, mutator, index). No clock, no
//! environment. All variants are built in memory; nothing is written outside the output
//! directory, and the total output is capped.
//!
//! Plans, `plan.toml`:
//!
//! ```toml
//! [[format]]
//! id = "mesh.stl"
//! tags = ["binary"]                 # descriptor tags: xml, tree, zip, container select extras
//! header_len = 84                   # optional structure boundary for truncation
//! count = { le = 4, at = 80 }       # or { zip_entries = true } or { after = ' n="' }
//! flip = { start = 81, end = 84 }   # optional byte range for bit flips, or { xml_root_name = true }
//!
//! [[format.override]]
//! mutator = "bitflip"
//! error = "truncated_file"
//! reason = "why this format's reader returns another code"
//! ```

use crate::check::{load_manifests, read_capped, regular_file_in};
use crate::error::CorpError;
use crate::manifest::{Expect, FormatId, Location};
use fid_num::rng::{Address, Cursor};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Bit-flip variants per input file.
pub const BITFLIPS: u32 = 3;
/// Depth of the element chain inserted by [`Mutator::Nest`]; above the `IO.REG.LIMITS`
/// default depth of 64.
pub const NEST_DEPTH: usize = 256;
/// Largest bomb on disk (REQ-IO.CORP.HOSTILE-004).
pub const MAX_BOMB_BYTES: usize = 1 << 20;
/// Default cap on everything one run writes.
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
/// Default cap on one input file.
pub const DEFAULT_MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
/// Length-258 runs in the bomb's deflate kernel, and central-directory entries sharing it.
const BOMB_RUNS: usize = 40_000;
const BOMB_ENTRIES: usize = 7_000;
const BIG_DECIMAL: &[u8] = b"18446744073709551615";

/// A hostile mutation. Declaration order is the order variants are generated in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mutator {
    /// Cut the file at 1/4, 1/2 and 3/4 of its length and at the plan's header boundary.
    Truncate,
    /// Flip one seeded bit in the plan's flip range ([`BITFLIPS`] variants).
    BitFlip,
    /// Set the plan's declared count field to its largest value.
    InflateCount,
    /// Insert a [`NEST_DEPTH`]-deep element chain after the XML root start tag.
    Nest,
    /// Insert a ten-level nested-entity DOCTYPE and a reference to its top entity.
    BillionLaughs,
    /// Replace the file by a ZIP whose entries all share one deflate stream.
    ZipBomb,
}

impl Mutator {
    pub const ALL: [Mutator; 6] = [
        Mutator::Truncate,
        Mutator::BitFlip,
        Mutator::InflateCount,
        Mutator::Nest,
        Mutator::BillionLaughs,
        Mutator::ZipBomb,
    ];

    /// Stable snake_case name, used in plans, file names and tags.
    pub fn as_str(self) -> &'static str {
        match self {
            Mutator::Truncate => "truncate",
            Mutator::BitFlip => "bitflip",
            Mutator::InflateCount => "inflate_count",
            Mutator::Nest => "nest",
            Mutator::BillionLaughs => "billion_laughs",
            Mutator::ZipBomb => "zip_bomb",
        }
    }

    pub fn parse(s: &str) -> Option<Mutator> {
        Mutator::ALL.into_iter().find(|m| m.as_str() == s)
    }

    /// The fixed mutator → error table.
    pub fn default_error(self) -> &'static str {
        match self {
            Mutator::Truncate => "truncated_file",
            Mutator::BitFlip => "malformed",
            Mutator::InflateCount | Mutator::Nest | Mutator::ZipBomb => "limit_exceeded",
            Mutator::BillionLaughs => "xml_forbidden_construct",
        }
    }
}

/// Mutators selected by descriptor tags. Truncation, bit flips and count inflation are always
/// on (REQ-IO.CORP.HOSTILE-001); `xml` adds nesting and billion laughs, `tree` adds nesting,
/// `zip` or `container` add the ZIP bomb. Other tags are ignored.
pub fn mutators_for_tags<S: AsRef<str>>(tags: &[S]) -> Vec<Mutator> {
    let has = |t: &str| tags.iter().any(|x| x.as_ref() == t);
    let mut set = BTreeSet::from([Mutator::Truncate, Mutator::BitFlip, Mutator::InflateCount]);
    if has("xml") {
        set.insert(Mutator::Nest);
        set.insert(Mutator::BillionLaughs);
    }
    if has("tree") {
        set.insert(Mutator::Nest);
    }
    if has("zip") || has("container") {
        set.insert(Mutator::ZipBomb);
    }
    set.into_iter().collect()
}

/// Where a format declares a count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountField {
    /// Unsigned little-endian integer of `width` bytes (1..=8) at byte `at`.
    Le { at: u64, width: u8 },
    /// Both entry counts of the ZIP end-of-central-directory record.
    ZipEntries,
    /// The ASCII decimal right after the first occurrence of this text.
    After(String),
}

/// Which bytes a bit flip may hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlipRange {
    /// `[start, end)`, `end` clipped to the file length (`None`: to the end).
    Bytes { start: u64, end: Option<u64> },
    /// The name of the XML root element.
    XmlRootName,
}

/// A per-format expected-error override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub mutator: Mutator,
    pub error: String,
    pub reason: String,
}

/// The hostile plan of one format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutatorPlan {
    pub format: FormatId,
    pub tags: Vec<String>,
    pub mutators: Vec<Mutator>,
    pub overrides: Vec<Override>,
    pub header_len: Option<u64>,
    pub count: CountField,
    pub flip: FlipRange,
}

impl MutatorPlan {
    /// The expected error of `m` and, when overridden, the reason.
    pub fn expected(&self, m: Mutator) -> (&str, Option<&str>) {
        match self.overrides.iter().find(|o| o.mutator == m) {
            Some(o) => (o.error.as_str(), Some(o.reason.as_str())),
            None => (m.default_error(), None),
        }
    }
}

fn is_code(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn bad(at: &str, detail: impl Into<String>) -> CorpError {
    CorpError::malformed(at, detail)
}

fn only_keys(t: &toml::Table, allowed: &[&str], at: &str) -> Result<(), CorpError> {
    match t.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(bad(at, format!("unknown key {k:?}"))),
        None => Ok(()),
    }
}

fn get_str<'a>(t: &'a toml::Table, key: &str, at: &str) -> Result<&'a str, CorpError> {
    match t.get(key) {
        Some(toml::Value::String(s)) => Ok(s),
        _ => Err(bad(at, format!("{key} must be a string"))),
    }
}

fn get_u64(t: &toml::Table, key: &str, at: &str) -> Result<Option<u64>, CorpError> {
    match t.get(key) {
        None => Ok(None),
        Some(toml::Value::Integer(n)) => {
            u64::try_from(*n).map(Some).map_err(|_| bad(at, format!("{key} must not be negative")))
        }
        Some(_) => Err(bad(at, format!("{key} must be an integer"))),
    }
}

fn get_table<'a>(t: &'a toml::Table, key: &str, at: &str) -> Result<Option<&'a toml::Table>, CorpError> {
    match t.get(key) {
        None => Ok(None),
        Some(toml::Value::Table(x)) => Ok(Some(x)),
        Some(_) => Err(bad(at, format!("{key} must be a table"))),
    }
}

fn parse_count(t: &toml::Table, at: &str) -> Result<CountField, CorpError> {
    if t.contains_key("le") {
        only_keys(t, &["le", "at"], at)?;
        let width = get_u64(t, "le", at)?.filter(|w| (1..=8).contains(w));
        let at_ = get_u64(t, "at", at)?;
        return match (width, at_) {
            (Some(w), Some(a)) => Ok(CountField::Le { at: a, width: w as u8 }),
            _ => Err(bad(at, "count.le must be 1..=8 and count.at an offset")),
        };
    }
    if t.contains_key("zip_entries") {
        only_keys(t, &["zip_entries"], at)?;
        return match t.get("zip_entries") {
            Some(toml::Value::Boolean(true)) => Ok(CountField::ZipEntries),
            _ => Err(bad(at, "count.zip_entries must be true")),
        };
    }
    only_keys(t, &["after"], at)?;
    let after = get_str(t, "after", at)?;
    if after.is_empty() {
        return Err(bad(at, "count.after must not be empty"));
    }
    Ok(CountField::After(after.to_string()))
}

fn parse_flip(t: Option<&toml::Table>, at: &str) -> Result<FlipRange, CorpError> {
    let Some(t) = t else { return Ok(FlipRange::Bytes { start: 0, end: None }) };
    if t.contains_key("xml_root_name") {
        only_keys(t, &["xml_root_name"], at)?;
        return match t.get("xml_root_name") {
            Some(toml::Value::Boolean(true)) => Ok(FlipRange::XmlRootName),
            _ => Err(bad(at, "flip.xml_root_name must be true")),
        };
    }
    only_keys(t, &["start", "end"], at)?;
    let start = get_u64(t, "start", at)?.unwrap_or(0);
    let end = get_u64(t, "end", at)?;
    if end.is_some_and(|e| e <= start) {
        return Err(bad(at, "flip.end must be greater than flip.start"));
    }
    Ok(FlipRange::Bytes { start, end })
}

fn parse_plan(t: &toml::Table, at: &str) -> Result<MutatorPlan, CorpError> {
    only_keys(t, &["id", "tags", "header_len", "count", "flip", "override"], at)?;
    let id = get_str(t, "id", at)?;
    let format = FormatId::parse(id).ok_or_else(|| bad(at, format!("bad format ID {id:?}")))?;
    let at = format!("{at} ({format})");
    let at = at.as_str();
    let mut tags = Vec::new();
    match t.get("tags") {
        None => {}
        Some(toml::Value::Array(items)) => {
            for item in items {
                match item {
                    toml::Value::String(s) if is_code(s) => tags.push(s.clone()),
                    _ => return Err(bad(at, "tags must be lowercase identifiers")),
                }
            }
        }
        Some(_) => return Err(bad(at, "tags must be an array")),
    }
    let mutators = mutators_for_tags(&tags);
    let header_len = get_u64(t, "header_len", at)?;
    // Every format must have an oversized-declaration variant (REQ-IO.CORP.HOSTILE-001).
    let count = parse_count(get_table(t, "count", at)?.ok_or_else(|| bad(at, "missing count"))?, at)?;
    let flip = parse_flip(get_table(t, "flip", at)?, at)?;
    let mut overrides: Vec<Override> = Vec::new();
    match t.get("override") {
        None => {}
        Some(toml::Value::Array(items)) => {
            for item in items {
                let toml::Value::Table(o) = item else { return Err(bad(at, "override must be [[format.override]]")) };
                only_keys(o, &["mutator", "error", "reason"], at)?;
                let name = get_str(o, "mutator", at)?;
                let mutator = Mutator::parse(name)
                    .filter(|m| mutators.contains(m))
                    .ok_or_else(|| bad(at, format!("override of mutator {name:?}, which the plan does not use")))?;
                if overrides.iter().any(|x| x.mutator == mutator) {
                    return Err(bad(at, format!("mutator {name:?} overridden twice")));
                }
                let error = get_str(o, "error", at)?;
                if !is_code(error) {
                    return Err(bad(at, format!("bad error code {error:?}")));
                }
                let reason = get_str(o, "reason", at)?;
                if reason.trim().is_empty() {
                    return Err(bad(at, "an override needs a reason"));
                }
                overrides.push(Override { mutator, error: error.to_string(), reason: reason.to_string() });
            }
        }
        Some(_) => return Err(bad(at, "override must be an array of tables")),
    }
    Ok(MutatorPlan { format, tags, mutators, overrides, header_len, count, flip })
}

/// Parses a plan file into plans by format. Errors are `malformed` (a bad plan).
pub fn parse_plans(text: &str) -> Result<BTreeMap<FormatId, MutatorPlan>, CorpError> {
    const AT: &str = "hostile plan";
    let root: toml::Table = text.parse().map_err(|e: toml::de::Error| bad(AT, e.message()))?;
    only_keys(&root, &["format"], AT)?;
    let mut plans = BTreeMap::new();
    match root.get("format") {
        None => {}
        Some(toml::Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                let at = format!("{AT} format {}", i + 1);
                let toml::Value::Table(t) = item else { return Err(bad(&at, "format must be [[format]]")) };
                let plan = parse_plan(t, &at)?;
                if plans.contains_key(&plan.format) {
                    return Err(bad(&at, format!("format {} planned twice", plan.format)));
                }
                plans.insert(plan.format.clone(), plan);
            }
        }
        Some(_) => return Err(bad(AT, "format must be an array of tables ([[format]])")),
    }
    Ok(plans)
}

/// One generated variant, in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub mutator: Mutator,
    pub index: u32,
    pub bytes: Vec<u8>,
}

fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

/// The RNG stream of (file digest, mutator, index): the first 8 bytes, big-endian, of
/// SHA-256 over `"<digest>\0<mutator>\0<index>"`.
pub fn rng_stream(digest: &Digest, m: Mutator, index: u32) -> u64 {
    let h = Sha256::digest(format!("{digest}\0{}\0{index}", m.as_str()).as_bytes());
    let mut b = [0u8; 8];
    b.copy_from_slice(&h[..8]);
    u64::from_be_bytes(b)
}

fn usize_of(n: u64, what: &str) -> Result<usize, CorpError> {
    usize::try_from(n).map_err(|_| bad(what, "offset does not fit"))
}

/// Truncation lengths: 1/4, 1/2, 3/4 of `n`, then `header_len`; zero, full and repeated
/// lengths are dropped.
pub fn truncate_lengths(n: usize, header_len: Option<u64>) -> Vec<usize> {
    let mut cuts = vec![n / 4, n / 2, n / 4 * 3 + (n % 4) * 3 / 4];
    if let Some(h) = header_len.and_then(|h| usize::try_from(h).ok()) {
        cuts.push(h);
    }
    let mut out: Vec<usize> = Vec::new();
    for c in cuts {
        if c > 0 && c < n && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

fn find_from(data: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    if from > data.len() {
        return None;
    }
    data[from..].windows(pat.len()).position(|w| w == pat).map(|p| p + from)
}

/// The XML root element: (index of its `<`, end of its name, index of the `>` ending its
/// start tag). Skips a UTF-8 BOM, the XML declaration, processing instructions, comments and
/// a DOCTYPE. An empty-element root (`<a/>`) is refused.
pub fn find_xml_root(data: &[u8]) -> Result<(usize, usize, usize), CorpError> {
    const AT: &str = "xml root";
    let none = || bad(AT, "no XML root element");
    let mut i = if data.starts_with(b"\xef\xbb\xbf") { 3 } else { 0 };
    loop {
        while data.get(i).is_some_and(|c| b" \t\r\n".contains(c)) {
            i += 1;
        }
        let rest = data.get(i..).ok_or_else(none)?;
        if rest.starts_with(b"<?") {
            i = find_from(data, b"?>", i + 2).ok_or_else(none)? + 2;
        } else if rest.starts_with(b"<!--") {
            i = find_from(data, b"-->", i + 4).ok_or_else(none)? + 3;
        } else if rest.starts_with(b"<!") {
            let gt = find_from(data, b">", i + 2).ok_or_else(none)?;
            let j = match find_from(data, b"[", i + 2) {
                Some(br) if br < gt => find_from(data, b"]>", br).ok_or_else(none)? + 1,
                _ => gt,
            };
            i = j + 1;
        } else if rest.first() == Some(&b'<') {
            break;
        } else {
            return Err(none());
        }
    }
    let mut j = i + 1;
    while data.get(j).is_some_and(|c| !b" \t\r\n/>".contains(c)) {
        j += 1;
    }
    if j == i + 1 {
        return Err(none());
    }
    let mut k = j;
    let mut quote = None;
    while let Some(&c) = data.get(k) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'>' => break,
            None => {}
        }
        k += 1;
    }
    if k >= data.len() || data[k - 1] == b'/' {
        return Err(bad(AT, "the root start tag is unterminated or an empty-element tag"));
    }
    Ok((i, j, k))
}

fn bitflip(plan: &MutatorPlan, data: &[u8], digest: &Digest, seed: u64, index: u32) -> Result<Vec<u8>, CorpError> {
    let (start, end) = match &plan.flip {
        FlipRange::XmlRootName => {
            let (lt, name_end, _) = find_xml_root(data)?;
            (lt + 1, name_end)
        }
        FlipRange::Bytes { start, end } => {
            let start = usize_of(*start, "flip.start")?;
            let end = match end {
                Some(e) => usize_of(*e, "flip.end")?.min(data.len()),
                None => data.len(),
            };
            (start, end)
        }
    };
    if start >= end {
        return Err(bad(plan.format.as_str(), "flip range is empty for this file"));
    }
    let mut c = Cursor::new(Address { seed, stream: rng_stream(digest, Mutator::BitFlip, index) }, 0);
    let rng = |e: fid_num::NumError| bad("rng", e.to_string());
    let pos = start + usize_of(c.below((end - start) as u64).map_err(rng)?, "flip")?;
    let bit = c.below(8).map_err(rng)?;
    let mut out = data.to_vec();
    out[pos] ^= 1u8 << bit;
    Ok(out)
}

fn inflate_count(plan: &MutatorPlan, data: &[u8]) -> Result<Vec<u8>, CorpError> {
    let at = plan.format.as_str();
    let mut out = data.to_vec();
    match &plan.count {
        CountField::Le { at: off, width } => {
            let s = usize_of(*off, "count.at")?;
            let e = s.checked_add(usize::from(*width)).filter(|&e| e <= data.len());
            let e = e.ok_or_else(|| bad(at, "count field lies beyond the end of the file"))?;
            out[s..e].fill(0xff);
        }
        CountField::ZipEntries => {
            let last = data.len().checked_sub(22).ok_or_else(|| bad(at, "no ZIP end-of-central-directory"))?;
            let lo = last.saturating_sub(65_535);
            let p = (lo..=last)
                .rev()
                .find(|&p| data[p..].starts_with(b"PK\x05\x06"))
                .ok_or_else(|| bad(at, "no ZIP end-of-central-directory"))?;
            out[p + 8..p + 12].fill(0xff);
        }
        CountField::After(pat) => {
            let p = find_from(data, pat.as_bytes(), 0).ok_or_else(|| bad(at, "count text not found"))?;
            let s = p + pat.len();
            let mut e = s;
            while data.get(e).is_some_and(u8::is_ascii_digit) {
                e += 1;
            }
            if e == s {
                return Err(bad(at, "no decimal after the count text"));
            }
            out.truncate(s);
            out.extend_from_slice(BIG_DECIMAL);
            out.extend_from_slice(&data[e..]);
        }
    }
    Ok(out)
}

fn nest(data: &[u8]) -> Result<Vec<u8>, CorpError> {
    let (_, _, k) = find_xml_root(data)?;
    let mut out = Vec::with_capacity(data.len() + 7 * NEST_DEPTH);
    out.extend_from_slice(&data[..=k]);
    for _ in 0..NEST_DEPTH {
        out.extend_from_slice(b"<x>");
    }
    for _ in 0..NEST_DEPTH {
        out.extend_from_slice(b"</x>");
    }
    out.extend_from_slice(&data[k + 1..]);
    Ok(out)
}

fn billion_laughs(data: &[u8]) -> Result<Vec<u8>, CorpError> {
    let (i, j, k) = find_xml_root(data)?;
    let mut dt = b"<!DOCTYPE ".to_vec();
    dt.extend_from_slice(&data[i + 1..j]);
    dt.extend_from_slice(b" [\n<!ENTITY a0 \"lol\">\n");
    for n in 1..10 {
        dt.extend_from_slice(format!("<!ENTITY a{n} \"").as_bytes());
        for _ in 0..10 {
            dt.extend_from_slice(format!("&a{};", n - 1).as_bytes());
        }
        dt.extend_from_slice(b"\">\n");
    }
    dt.extend_from_slice(b"]>\n");
    let mut out = Vec::with_capacity(data.len() + dt.len() + 4);
    out.extend_from_slice(&data[..i]);
    out.extend_from_slice(&dt);
    out.extend_from_slice(&data[i..=k]);
    out.extend_from_slice(b"&a9;");
    out.extend_from_slice(&data[k + 1..]);
    Ok(out)
}

/// LSB-first bit writer for deflate.
struct Bits {
    out: Vec<u8>,
    acc: u8,
    n: u8,
}

impl Bits {
    fn put_bit(&mut self, bit: u8) {
        self.acc |= (bit & 1) << self.n;
        self.n += 1;
        if self.n == 8 {
            self.out.push(self.acc);
            self.acc = 0;
            self.n = 0;
        }
    }

    /// A Huffman code, most significant bit first.
    fn code(&mut self, c: u32, len: u32) {
        for k in (0..len).rev() {
            self.put_bit(((c >> k) & 1) as u8);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc);
        }
        self.out
    }
}

fn crc32_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (i, slot) in t.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    t
}

/// CRC-32 (ZIP) of `n` zero bytes.
fn crc32_zeros(n: usize) -> u32 {
    let t = crc32_table();
    let mut c = 0xFFFF_FFFFu32;
    for _ in 0..n {
        c = t[(c & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// The ZIP bomb: one local entry whose fixed-Huffman deflate stream inflates to
/// 1 + 258 × 40,000 zero bytes, and 7,000 central-directory entries that all point at it, so
/// the declared (and real) total exceeds the 64 GiB default byte limit while the file stays
/// near 0.5 MiB. Sizes are honest: each entry inflates to exactly its declared size and CRC.
pub fn zip_bomb() -> Vec<u8> {
    let mut b = Bits { out: Vec::with_capacity(70_000), acc: 0, n: 0 };
    b.put_bit(1); // BFINAL
    b.put_bit(1); // BTYPE = 01 (fixed Huffman), LSB first
    b.put_bit(0);
    b.code(0x30, 8); // literal 0
    for _ in 0..BOMB_RUNS {
        b.code(0b1100_0101, 8); // length code 285: 258
        b.code(0, 5); // distance code 0: 1
    }
    b.code(0, 7); // end of block
    let stream = b.finish();
    let size = 1 + 258 * BOMB_RUNS;
    let crc = crc32_zeros(size);
    let (csize, usize_) = (stream.len() as u32, size as u32);
    let mut out = Vec::with_capacity(stream.len() + 60 * BOMB_ENTRIES + 128);
    let name = b"3D/3dmodel.model";
    let le16 = |o: &mut Vec<u8>, v: u16| o.extend_from_slice(&v.to_le_bytes());
    let le32 = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    le32(&mut out, 0x0403_4B50);
    for v in [20, 0, 8, 0, 0x21] {
        le16(&mut out, v);
    }
    for v in [crc, csize, usize_] {
        le32(&mut out, v);
    }
    le16(&mut out, name.len() as u16);
    le16(&mut out, 0);
    out.extend_from_slice(name);
    out.extend_from_slice(&stream);
    let cd_off = out.len() as u32;
    for i in 0..BOMB_ENTRIES {
        let nm = format!("b{i:05}");
        le32(&mut out, 0x0201_4B50);
        for v in [20, 20, 0, 8, 0, 0x21] {
            le16(&mut out, v);
        }
        for v in [crc, csize, usize_] {
            le32(&mut out, v);
        }
        for v in [nm.len() as u16, 0, 0, 0, 0] {
            le16(&mut out, v);
        }
        le32(&mut out, 0); // external attributes
        le32(&mut out, 0); // local header offset: every entry shares entry 0
        out.extend_from_slice(nm.as_bytes());
    }
    let cd_len = out.len() as u32 - cd_off;
    le32(&mut out, 0x0605_4B50);
    for v in [0, 0, BOMB_ENTRIES as u16, BOMB_ENTRIES as u16] {
        le16(&mut out, v);
    }
    le32(&mut out, cd_len);
    le32(&mut out, cd_off);
    le16(&mut out, 0);
    out
}

/// All variants of one input, in mutator order. `digest` is the input's SHA-256.
pub fn generate(plan: &MutatorPlan, data: &[u8], digest: &Digest, seed: u64) -> Result<Vec<Variant>, CorpError> {
    let mut out = Vec::new();
    for &m in &plan.mutators {
        match m {
            Mutator::Truncate => {
                for (i, n) in truncate_lengths(data.len(), plan.header_len).into_iter().enumerate() {
                    out.push(Variant { mutator: m, index: i as u32, bytes: data[..n].to_vec() });
                }
            }
            Mutator::BitFlip => {
                for i in 0..BITFLIPS {
                    out.push(Variant { mutator: m, index: i, bytes: bitflip(plan, data, digest, seed, i)? });
                }
            }
            Mutator::InflateCount => out.push(Variant { mutator: m, index: 0, bytes: inflate_count(plan, data)? }),
            Mutator::Nest => out.push(Variant { mutator: m, index: 0, bytes: nest(data)? }),
            Mutator::BillionLaughs => out.push(Variant { mutator: m, index: 0, bytes: billion_laughs(data)? }),
            Mutator::ZipBomb => {
                let bytes = zip_bomb();
                if bytes.len() > MAX_BOMB_BYTES {
                    return Err(bad(plan.format.as_str(), "bomb larger than 1 MiB"));
                }
                out.push(Variant { mutator: m, index: 0, bytes });
            }
        }
    }
    Ok(out)
}

/// Options of [`run_hostile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostileOptions {
    pub seed: u64,
    /// Cap on all bytes written (variants and manifests); exceeding it is `limit_exceeded`.
    pub max_total_bytes: u64,
    /// Cap on one input file.
    pub max_input_bytes: u64,
}

impl HostileOptions {
    pub fn new(seed: u64) -> Self {
        HostileOptions { seed, max_total_bytes: DEFAULT_MAX_TOTAL_BYTES, max_input_bytes: DEFAULT_MAX_INPUT_BYTES }
    }
}

/// One generated manifest entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostileEntry {
    /// Path relative to the output directory: `<format-id>/<file>`.
    pub path: String,
    /// The input's path in the corpus.
    pub input: String,
    pub mutator: Mutator,
    pub index: u32,
    pub sha256: Digest,
    pub licence: String,
    pub error: String,
    /// The override reason, when the error is not the table default.
    pub reason: Option<String>,
    pub bytes: u64,
}

/// What a run produced: entries by format, in generation order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostileReport {
    pub formats: BTreeMap<FormatId, Vec<HostileEntry>>,
    pub bytes_written: u64,
}

fn file_ext(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && (1..=8).contains(&ext.len())
                && ext.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) =>
        {
            ext
        }
        _ => "bin",
    }
}

fn toml_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn source_text(format: &FormatId, e: &HostileEntry, seed: u64) -> String {
    let mut s = format!(
        "hostile variant of {format}: {} by corpus-hostile ({} #{}, seed {seed})",
        e.input,
        e.mutator.as_str(),
        e.index
    );
    if let Some(r) = &e.reason {
        let _ = write!(s, "; expected error overridden: {r}");
    }
    s
}

/// The generated manifest of one format, in the `IO.CORP.MANIF` schema.
pub fn render_manifest(format: &FormatId, entries: &[HostileEntry], seed: u64) -> String {
    let mut out = format!("# Generated by corpus-hostile (IO.CORP.HOSTILE); do not edit.\nformat = \"{format}\"\n");
    for e in entries {
        let _ = write!(
            out,
            "\n[[entry]]\npath = {}\nsource = {}\nlicence = {}\nsha256 = \"{}\"\n\
             expect = {{ reject = {{ error = \"{}\" }} }}\ntags = [\"hostile\", \"{}\"]\n",
            toml_str(&e.path),
            toml_str(&source_text(format, e, seed)),
            toml_str(&e.licence),
            e.sha256,
            e.error,
            e.mutator.as_str()
        );
    }
    out
}

fn limit(detail: String) -> CorpError {
    CorpError::LimitExceeded { detail }
}

/// Writes `bytes` to `dir/name`, refusing a symlink at the target and charging the cap.
fn write_capped(dir: &Path, name: &str, bytes: &[u8], written: &mut u64, cap: u64) -> Result<(), CorpError> {
    let n = bytes.len() as u64;
    if written.saturating_add(n) > cap {
        return Err(limit(format!("output would exceed {cap} bytes at {name}")));
    }
    let path = dir.join(name);
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
        return Err(CorpError::malformed(name, "output target exists and is not a file"));
    }
    fs::write(&path, bytes).map_err(|e| CorpError::malformed(name, format!("cannot write: {e}")))?;
    *written += n;
    Ok(())
}

fn ensure_dir(path: &Path) -> Result<(), CorpError> {
    let at = path.display().to_string();
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(CorpError::malformed(at, "output directory is a symlink or not a directory")),
        Err(_) => fs::create_dir(path).map_err(|e| CorpError::malformed(at, format!("cannot create: {e}"))),
    }
}

/// Generates the hostile corpus of every manifest in `corpus` into `out`.
///
/// Inputs are the entries stored in the repository (`path`) that expect `accept` and are not
/// tagged `hostile`. Every manifest's format needs a plan (`malformed` otherwise). Variants
/// whose digest was already produced are skipped, so the output never repeats a digest.
/// Writes only `out/<format-id>.toml` and files in `out/<format-id>/`, where `out` and those
/// directories must not be symlinks; stops with `limit_exceeded` before the total would pass
/// `max_total_bytes`.
pub fn run_hostile(
    corpus: &Path,
    plans: &BTreeMap<FormatId, MutatorPlan>,
    out: &Path,
    opts: &HostileOptions,
) -> Result<HostileReport, CorpError> {
    let (manifests, errors) = load_manifests(corpus);
    if let Some(e) = errors.into_iter().next() {
        return Err(e);
    }
    let mut report = HostileReport::default();
    let mut generated: BTreeMap<FormatId, Vec<(HostileEntry, Vec<u8>)>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut planned = 0u64;
    for (format, m) in &manifests {
        let plan = plans.get(format).ok_or_else(|| bad(format.as_str(), "no hostile plan for this format"))?;
        let list = generated.entry(format.clone()).or_default();
        for e in &m.entries {
            let (Location::Path(rel), Expect::Accept { .. }) = (&e.location, &e.expect) else { continue };
            if e.tags.iter().any(|t| t == "hostile") {
                continue;
            }
            let at = format!("{format}: {rel}");
            let data = regular_file_in(corpus, rel)
                .and_then(|p| read_capped(&p, opts.max_input_bytes))
                .map_err(|d| CorpError::Io { format: format.to_string(), entry: rel.clone(), detail: d })?;
            let digest = sha256(&data);
            if digest != e.sha256 {
                return Err(CorpError::DigestMismatch {
                    format: format.to_string(),
                    entry: rel.clone(),
                    expected: Box::new(e.sha256),
                    actual: Box::new(digest),
                });
            }
            let ext = file_ext(rel);
            for v in generate(plan, &data, &digest, opts.seed).map_err(|x| bad(&at, x.to_string()))? {
                let sha = sha256(&v.bytes);
                if !seen.insert(sha) {
                    continue;
                }
                planned = planned.saturating_add(v.bytes.len() as u64);
                if planned > opts.max_total_bytes {
                    return Err(limit(format!("output would exceed {} bytes at {at}", opts.max_total_bytes)));
                }
                let (error, reason) = plan.expected(v.mutator);
                let name = format!("{}-{}-{}.{ext}", &digest.hex()[..16], v.mutator.as_str(), v.index);
                let entry = HostileEntry {
                    path: format!("{format}/{name}"),
                    input: rel.clone(),
                    mutator: v.mutator,
                    index: v.index,
                    sha256: sha,
                    licence: e.licence.clone(),
                    error: error.to_string(),
                    reason: reason.map(str::to_string),
                    bytes: v.bytes.len() as u64,
                };
                list.push((entry, v.bytes));
            }
        }
    }
    // Everything is generated and within the cap: now write.
    ensure_dir(out)?;
    for (format, list) in generated {
        let dir = out.join(format.as_str());
        ensure_dir(&dir)?;
        let mut entries = Vec::with_capacity(list.len());
        for (entry, bytes) in list {
            let name = entry.path.rsplit('/').next().unwrap_or_default().to_string();
            write_capped(&dir, &name, &bytes, &mut report.bytes_written, opts.max_total_bytes)?;
            entries.push(entry);
        }
        let text = render_manifest(&format, &entries, opts.seed);
        write_capped(out, &format!("{format}.toml"), text.as_bytes(), &mut report.bytes_written, opts.max_total_bytes)?;
        report.formats.insert(format, entries);
    }
    Ok(report)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn mutator_names_round_trip() {
        for m in Mutator::ALL {
            assert_eq!(Mutator::parse(m.as_str()), Some(m));
        }
        assert_eq!(Mutator::parse("nope"), None);
    }

    #[test]
    fn tags_select_mutators() {
        let base = [Mutator::Truncate, Mutator::BitFlip, Mutator::InflateCount];
        assert_eq!(mutators_for_tags::<&str>(&[]), base);
        assert_eq!(mutators_for_tags(&["xml"]).len(), 5);
        assert_eq!(mutators_for_tags(&["tree"]), [base.as_slice(), &[Mutator::Nest]].concat());
        assert_eq!(mutators_for_tags(&["container"]), [base.as_slice(), &[Mutator::ZipBomb]].concat());
    }

    #[test]
    fn truncation_lengths() {
        assert_eq!(truncate_lengths(684, Some(84)), [171, 342, 513, 84]);
        assert_eq!(truncate_lengths(448, Some(48)), [112, 224, 336, 48]);
        assert_eq!(truncate_lengths(2, Some(1)), [1]);
        assert!(truncate_lengths(0, None).is_empty());
    }

    #[test]
    fn xml_root() {
        let x = b"\xef\xbb\xbf<?xml version=\"1.0\"?>\n<!-- c -->\n<!DOCTYPE a [<!ENTITY e \"x\">]>\n<a b='>'>t</a>";
        let (i, j, k) = find_xml_root(x).unwrap();
        assert_eq!(&x[i..j], b"<a");
        assert_eq!(&x[k - 3..=k], b"'>'>");
        assert!(find_xml_root(b"<a/>").is_err());
        assert!(find_xml_root(b"text").is_err());
        assert!(find_xml_root(b"<a b=\"").is_err());
    }

    #[test]
    fn crc_of_zeros() {
        // zlib.crc32(bytes(1)) and zlib.crc32(bytes(1000)).
        assert_eq!(crc32_zeros(0), 0);
        assert_eq!(crc32_zeros(1), 0xD202_EF8D);
        assert_eq!(crc32_zeros(1000), 0x060B_1780);
    }

    #[test]
    fn toml_strings_escape() {
        assert_eq!(toml_str("a\"b\\c\n"), "\"a\\\"b\\\\c\\u000A\"");
    }

    #[test]
    fn extensions() {
        assert_eq!(file_ext("files/a.stl"), "stl");
        assert_eq!(file_ext("a.3mf"), "3mf");
        assert_eq!(file_ext("a.STL"), "bin");
        assert_eq!(file_ext(".hidden"), "bin");
        assert_eq!(file_ext("noext"), "bin");
    }
}
