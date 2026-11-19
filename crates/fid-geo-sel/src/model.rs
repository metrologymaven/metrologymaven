//! Index-set selections, their digests and the stale-selection check (spec node
//! `GEO.SEL.MODEL`).
//!
//! # Index sets
//!
//! An [`IndexSet`] holds the members of a set of `u64` indexes into one [`Domain`] of an
//! element as half-open ranges `[start, end)` that are non-empty, ascending and coalesced
//! (no two ranges overlap or touch), so two sets with the same members have the same
//! ranges, encoding and digest (REQ-GEO.SEL.MODEL-001). Input lists may be in any order
//! and may repeat indexes; repeats are members once. Every constructor takes the element's
//! count in the domain and fails with [`SelError::IndexOutOfRange`] on an index not less
//! than it (REQ-GEO.SEL.MODEL-004). Memory is `O(ranges)`, not `O(members)`.
//!
//! # Index-set digest (REQ-GEO.SEL.MODEL-002)
//!
//! SHA-256 (the portable FIPS 180-4 code of `fid-kci`) over, all integers little-endian:
//!
//! ```text
//! "FIDSEL" | 0x01 | domain u8 | target element digest [32] | count u64 |
//! ranges as (start u64, end u64) pairs, ascending
//! ```
//!
//! `count` is the number of members. Domain bytes: points 1, triangles 2, faces 3,
//! edges 4 ([`Domain::code`]). The number of ranges is not written: it follows from the
//! length. Only integer operations are used (D0).
//!
//! # Serialised selection (REQ-GEO.SEL.MODEL-005)
//!
//! [`Selection::encode`] writes exactly the model's fields, little-endian:
//!
//! ```text
//! "FIDSLC" | 0x01 | target [16] | target_digest [32] | body tag u8
//! body tag 1, index set: domain u8 | n_ranges u64 | n_ranges x (start u64, end u64)
//! body tag 2, query:     len u32 | len query bytes          (len <= MAX_QUERY_BYTES)
//! ```
//!
//! [`Selection::decode`] accepts only that layout with canonical ranges and no trailing
//! bytes, so `decode(encode(s)) == s` and every accepted input re-encodes to itself. A
//! decoded selection is not bound-checked (the element is not at hand); [`apply`] checks
//! the bound against the element.
//!
//! # Domains per element kind
//!
//! [`domain_count`]: points of a point set or point cloud, and vertices of a mesh, are the
//! points domain; mesh triangles the triangles domain. Faces and edges belong to CAD body
//! topology, which `GEO.TYPES` does not expose (it is `GEO.CAD`): on a CAD body handle
//! their count is unknown here, so faces and edges index sets are built with a count the
//! caller takes from the topology, and [`apply`] checks only digest and domain for them.
//! Every other pairing is [`SelError::DomainMismatch`].

use core::fmt;
use fid_geo_types::{digest, ElementDigest, ElementKind, GeometryElement};
use fid_kci::kci::Sha256;
use fid_types::Digest;

/// The ASCII tag the index-set digest preimage starts with.
pub const DIGEST_TAG: [u8; 6] = *b"FIDSEL";

/// The version byte of the index-set digest preimage.
pub const DIGEST_VERSION: u8 = 1;

/// The ASCII tag a serialised selection starts with.
pub const SELECTION_TAG: [u8; 6] = *b"FIDSLC";

/// The version byte of the serialised selection.
pub const SELECTION_VERSION: u8 = 1;

/// The largest opaque query, in bytes (64 KiB).
pub const MAX_QUERY_BYTES: usize = 64 * 1024;

/// The fields of a serialised [`Selection`], in order. Nothing else is serialised.
pub const SELECTION_FIELDS: [&str; 3] = ["target", "target_digest", "body"];

/// The fields of a serialised index-set body, in order.
pub const INDEX_SET_FIELDS: [&str; 2] = ["domain", "ranges"];

/// The fields of a serialised query body.
pub const QUERY_FIELDS: [&str; 1] = ["query"];

const BODY_INDEX_SET: u8 = 1;
const BODY_QUERY: u8 = 2;

/// Which indexable part of an element an index set refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Domain {
    Points,
    Triangles,
    Faces,
    Edges,
}

impl Domain {
    /// Every domain, in code order.
    pub const ALL: [Domain; 4] = [Domain::Points, Domain::Triangles, Domain::Faces, Domain::Edges];

    /// The domain byte of the digest and the serialised form.
    pub const fn code(self) -> u8 {
        match self {
            Domain::Points => 1,
            Domain::Triangles => 2,
            Domain::Faces => 3,
            Domain::Edges => 4,
        }
    }

    /// The domain of a domain byte.
    pub const fn from_code(code: u8) -> Option<Domain> {
        match code {
            1 => Some(Domain::Points),
            2 => Some(Domain::Triangles),
            3 => Some(Domain::Faces),
            4 => Some(Domain::Edges),
            _ => None,
        }
    }

    /// The stable wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Domain::Points => "points",
            Domain::Triangles => "triangles",
            Domain::Faces => "faces",
            Domain::Edges => "edges",
        }
    }
}

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An opaque 128-bit element ID (CONSTITUTION §5 IDs are UUIDs; the ID type itself lives
/// in CORE, above this kernel crate, so it is carried here as its 16 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId([u8; 16]);

impl EntityId {
    /// The ID with these bytes.
    pub const fn from_bytes(id: [u8; 16]) -> EntityId {
        EntityId(id)
    }

    /// The 16 ID bytes.
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }
}

/// The SHA-256 digest of an index set bound to its target element digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SelectionDigest(pub [u8; 32]);

impl SelectionDigest {
    /// The 32 digest bytes.
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    /// The shared wire form, `sha256:` plus 64 lowercase hex digits.
    pub const fn to_digest(self) -> Digest {
        Digest::from_bytes(self.0)
    }
}

impl fmt::Display for SelectionDigest {
    /// 64 lowercase hex digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for SelectionDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SelectionDigest({self})")
    }
}

/// Every error of this module. Errors are values; nothing here panics (CONSTITUTION §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelError {
    /// The element's digest differs from the one the selection was made on
    /// (REQ-GEO.SEL.MODEL-003).
    StaleSelection { expected: ElementDigest, found: ElementDigest },
    /// `index` is not less than the element's `count` in the domain (REQ-GEO.SEL.MODEL-004).
    IndexOutOfRange { index: u64, count: u64 },
    /// The element kind has no such domain (or, for faces and edges, the selection is not
    /// on a CAD body).
    DomainMismatch { domain: Domain, kind: ElementKind },
    /// An explicit range is empty or reversed (`start >= end`).
    InvalidRange { start: u64, end: u64 },
    /// An explicit index list would have `count` entries, more than the caller's `limit`.
    TooManyIndices { count: u64, limit: u64 },
    /// An opaque query of `len` bytes exceeds [`MAX_QUERY_BYTES`].
    QueryTooLarge { len: u64 },
    /// [`apply`] was given a query selection; queries are evaluated by `GEO.SEL.QUERY`.
    NotIndexSet,
    /// A working allocation failed; nothing is created.
    AllocationFailed,
    /// A serialised selection is malformed at byte `offset`.
    Malformed { offset: u64, reason: &'static str },
}

impl SelError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            SelError::StaleSelection { .. } => "stale_selection",
            SelError::IndexOutOfRange { .. } => "index_out_of_range",
            SelError::DomainMismatch { .. } => "domain_mismatch",
            SelError::InvalidRange { .. } => "invalid_range",
            SelError::TooManyIndices { .. } => "too_many_indices",
            SelError::QueryTooLarge { .. } => "query_too_large",
            SelError::NotIndexSet => "not_index_set",
            SelError::AllocationFailed => "allocation_failed",
            SelError::Malformed { .. } => "malformed",
        }
    }
}

impl fmt::Display for SelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelError::StaleSelection { expected, found } => {
                write!(f, "stale selection: made on element {expected}, applied to {found}")
            }
            SelError::IndexOutOfRange { index, count } => {
                write!(f, "index {index} is out of range for {count} elements")
            }
            SelError::DomainMismatch { domain, kind } => write!(f, "a {kind} has no {domain} domain here"),
            SelError::InvalidRange { start, end } => write!(f, "range [{start}, {end}) is empty or reversed"),
            SelError::TooManyIndices { count, limit } => write!(f, "{count} indexes exceed the limit {limit}"),
            SelError::QueryTooLarge { len } => write!(f, "query of {len} bytes exceeds {MAX_QUERY_BYTES}"),
            SelError::NotIndexSet => f.write_str("the selection is a query, not an index set"),
            SelError::AllocationFailed => f.write_str("allocation failed"),
            SelError::Malformed { offset, reason } => write!(f, "malformed selection at byte {offset}: {reason}"),
        }
    }
}

impl core::error::Error for SelError {}

/// A `usize` as `u64`; lossless on every supported target, saturating otherwise.
fn u64_of(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Sorts ranges and merges every overlapping or touching pair, in place. Inputs are
/// non-empty (`start < end`). The result is the unique canonical form of their union.
fn normalise(ranges: &mut Vec<(u64, u64)>) {
    // Unstable sort is deterministic here: equal (start, end) tuples are indistinguishable.
    ranges.sort_unstable();
    // `cur` follows `kept` (the last range kept); it is folded into `kept` and dropped when
    // it overlaps or touches it.
    ranges.dedup_by(|cur, kept| {
        if cur.0 <= kept.1 {
            if cur.1 > kept.1 {
                kept.1 = cur.1;
            }
            true
        } else {
            false
        }
    });
    ranges.shrink_to_fit();
}

/// A set of unique `u64` indexes into one domain of an element, stored as ascending,
/// non-empty, coalesced half-open ranges. There is no `Default`: a set always has a domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexSet {
    domain: Domain,
    ranges: Vec<(u64, u64)>,
}

impl IndexSet {
    /// The empty set over `domain`.
    pub const fn empty(domain: Domain) -> IndexSet {
        IndexSet { domain, ranges: Vec::new() }
    }

    /// The set of the listed indexes, in any order, repeats allowed. Fails with
    /// [`SelError::IndexOutOfRange`] on the first index (in list order) `>= count`.
    pub fn from_indices(domain: Domain, indices: &[u64], count: u64) -> Result<IndexSet, SelError> {
        let mut b = IndexSetBuilder::new(domain, count);
        for &i in indices {
            b.push(i)?;
        }
        Ok(b.finish())
    }

    /// The union of explicit half-open ranges, in any order, overlaps allowed. Fails with
    /// [`SelError::InvalidRange`] on an empty or reversed range and with
    /// [`SelError::IndexOutOfRange`] (index `end - 1`) on a range reaching past `count`.
    pub fn from_ranges(domain: Domain, ranges: &[(u64, u64)], count: u64) -> Result<IndexSet, SelError> {
        let mut out = Vec::new();
        out.try_reserve_exact(ranges.len()).map_err(|_| SelError::AllocationFailed)?;
        for &(start, end) in ranges {
            if start >= end {
                return Err(SelError::InvalidRange { start, end });
            }
            if end > count {
                return Err(SelError::IndexOutOfRange { index: end - 1, count });
            }
            out.push((start, end));
        }
        normalise(&mut out);
        Ok(IndexSet { domain, ranges: out })
    }

    /// The domain the indexes refer to.
    pub fn domain(&self) -> Domain {
        self.domain
    }

    /// The canonical ranges: half-open, non-empty, ascending, coalesced.
    pub fn ranges(&self) -> &[(u64, u64)] {
        &self.ranges
    }

    /// The number of members (a fixed-order sum over the ranges).
    pub fn len(&self) -> u64 {
        // Canonical ranges are disjoint subsets of [0, 2^64), so the sum cannot overflow;
        // saturation only keeps the function total.
        self.ranges.iter().fold(0u64, |n, &(s, e)| n.saturating_add(e - s))
    }

    /// Whether the set has no members.
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Whether `index` is a member, by binary search over the ranges.
    pub fn contains(&self, index: u64) -> bool {
        // The first range whose end is beyond `index` is the only one that can hold it.
        let at = self.ranges.partition_point(|&(_, e)| e <= index);
        match self.ranges.get(at) {
            Some(&(s, _)) => s <= index,
            None => false,
        }
    }

    /// The bytes this set occupies: the struct plus the allocated capacity of its range
    /// vector. `O(ranges)`, independent of the number of members (AC-4).
    pub fn stored_bytes(&self) -> usize {
        core::mem::size_of::<IndexSet>() + self.ranges.capacity() * core::mem::size_of::<(u64, u64)>()
    }

    /// The largest member, if any.
    pub fn max_index(&self) -> Option<u64> {
        self.ranges.last().map(|&(_, e)| e - 1)
    }

    /// The members in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = u64> + '_ {
        self.ranges.iter().flat_map(|&(s, e)| s..e)
    }

    /// `Ok` if every member is less than `count`, else [`SelError::IndexOutOfRange`] for
    /// the largest member.
    pub fn check_bound(&self, count: u64) -> Result<(), SelError> {
        match self.max_index() {
            Some(index) if index >= count => Err(SelError::IndexOutOfRange { index, count }),
            _ => Ok(()),
        }
    }

    /// The members as an ascending explicit list, if there are at most `limit` of them;
    /// else [`SelError::TooManyIndices`] (nothing is allocated).
    pub fn to_indices(&self, limit: u64) -> Result<Vec<u64>, SelError> {
        let count = self.len();
        if count > limit {
            return Err(SelError::TooManyIndices { count, limit });
        }
        let n = usize::try_from(count).map_err(|_| SelError::AllocationFailed)?;
        let mut out = Vec::new();
        out.try_reserve_exact(n).map_err(|_| SelError::AllocationFailed)?;
        out.extend(self.iter());
        Ok(out)
    }

    /// Streams the canonical digest preimage (REQ-GEO.SEL.MODEL-002) to `sink`.
    fn write_preimage(&self, target: ElementDigest, sink: &mut dyn FnMut(&[u8])) {
        let mut head = [0u8; 48];
        head[..6].copy_from_slice(&DIGEST_TAG);
        head[6] = DIGEST_VERSION;
        head[7] = self.domain.code();
        head[8..40].copy_from_slice(&target.to_bytes());
        head[40..48].copy_from_slice(&self.len().to_le_bytes());
        sink(&head);
        for &(s, e) in &self.ranges {
            let mut pair = [0u8; 16];
            pair[..8].copy_from_slice(&s.to_le_bytes());
            pair[8..].copy_from_slice(&e.to_le_bytes());
            sink(&pair);
        }
    }

    /// The canonical encoding the digest is taken over, bound to `target`.
    pub fn encoding(&self, target: ElementDigest) -> Vec<u8> {
        let mut out = Vec::with_capacity(48 + 16 * self.ranges.len());
        self.write_preimage(target, &mut |b: &[u8]| out.extend_from_slice(b));
        out
    }

    /// SHA-256 of [`IndexSet::encoding`], streamed (no copy of the encoding is made).
    pub fn digest(&self, target: ElementDigest) -> SelectionDigest {
        let mut h = Sha256::new();
        self.write_preimage(target, &mut |b: &[u8]| h.update(b));
        SelectionDigest(h.finish())
    }
}

/// Builds an [`IndexSet`] from indexes pushed one at a time, in any order. Consecutive
/// pushes that extend a run are merged as they arrive, so ascending input uses memory in
/// proportion to its ranges, not its members.
#[derive(Debug, Clone)]
pub struct IndexSetBuilder {
    domain: Domain,
    count: u64,
    runs: Vec<(u64, u64)>,
}

impl IndexSetBuilder {
    /// A builder for indexes into a domain of `count` elements.
    pub fn new(domain: Domain, count: u64) -> IndexSetBuilder {
        IndexSetBuilder { domain, count, runs: Vec::new() }
    }

    /// Adds one index. Fails with [`SelError::IndexOutOfRange`] if `index >= count`, and
    /// with [`SelError::AllocationFailed`] if the run list cannot grow.
    pub fn push(&mut self, index: u64) -> Result<(), SelError> {
        if index >= self.count {
            return Err(SelError::IndexOutOfRange { index, count: self.count });
        }
        // `index < count <= u64::MAX`, so `index + 1` does not overflow.
        if let Some(last) = self.runs.last_mut() {
            if index == last.1 {
                last.1 = index + 1;
                return Ok(());
            }
            if last.0 <= index && index < last.1 {
                return Ok(());
            }
        }
        self.runs.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
        self.runs.push((index, index + 1));
        Ok(())
    }

    /// The canonical set of everything pushed.
    pub fn finish(self) -> IndexSet {
        let mut ranges = self.runs;
        normalise(&mut ranges);
        IndexSet { domain: self.domain, ranges }
    }
}

/// An opaque semantic query. Its language and evaluation belong to `GEO.SEL.QUERY`; this
/// model only carries its canonical bytes, at most [`MAX_QUERY_BYTES`] of them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SelectionQuery(Vec<u8>);

impl SelectionQuery {
    /// A query from its canonical bytes, or [`SelError::QueryTooLarge`].
    pub fn from_bytes(bytes: Vec<u8>) -> Result<SelectionQuery, SelError> {
        if bytes.len() > MAX_QUERY_BYTES {
            return Err(SelError::QueryTooLarge { len: u64_of(bytes.len()) });
        }
        Ok(SelectionQuery(bytes))
    }

    /// The canonical bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// What a selection holds: an index set or a semantic query.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SelectionBody {
    IndexSet(IndexSet),
    Query(SelectionQuery),
}

/// A selection on one element: its ID, the digest of the element it was made on, and an
/// index set or query. It holds no screen coordinates, camera parameters or GPU handles
/// (REQ-GEO.SEL.MODEL-005, CONSTITUTION C2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Selection {
    target: EntityId,
    target_digest: ElementDigest,
    body: SelectionBody,
}

/// The count of `element` in `domain`: `Ok(Some(n))` when the element defines it,
/// `Ok(None)` for faces and edges of a CAD body handle (topology is `GEO.CAD`'s), else
/// [`SelError::DomainMismatch`].
pub fn domain_count(element: &GeometryElement, domain: Domain) -> Result<Option<u64>, SelError> {
    match (element, domain) {
        (GeometryElement::PointSet(p), Domain::Points) => Ok(Some(p.len())),
        (GeometryElement::PointCloud(p), Domain::Points) => Ok(Some(p.len())),
        (GeometryElement::Mesh(m), Domain::Points) => Ok(Some(m.vertex_count())),
        (GeometryElement::Mesh(m), Domain::Triangles) => Ok(Some(m.triangle_count())),
        (GeometryElement::CadBody(_), Domain::Faces | Domain::Edges) => Ok(None),
        _ => Err(SelError::DomainMismatch { domain, kind: element.kind() }),
    }
}

impl Selection {
    /// A selection from its parts. An index-set body is assumed to be bound-checked by
    /// its constructor; [`apply`] checks it again against the element.
    pub fn new(target: EntityId, target_digest: ElementDigest, body: SelectionBody) -> Selection {
        Selection { target, target_digest, body }
    }

    /// An index-set selection on `element` (ID `target`): computes the element digest and
    /// checks every index against the element's count in `domain`. Faces and edges (CAD
    /// topology) need a count from `GEO.CAD`: build them with [`IndexSet::from_indices`]
    /// and [`Selection::new`]; here they fail with [`SelError::DomainMismatch`].
    pub fn on_element(
        target: EntityId,
        element: &GeometryElement,
        domain: Domain,
        indices: &[u64],
    ) -> Result<Selection, SelError> {
        let count = domain_count(element, domain)?
            .ok_or_else(|| SelError::DomainMismatch { domain, kind: element.kind() })?;
        let set = IndexSet::from_indices(domain, indices, count)?;
        Ok(Selection { target, target_digest: digest(element), body: SelectionBody::IndexSet(set) })
    }

    /// The target element ID.
    pub fn target(&self) -> EntityId {
        self.target
    }

    /// The digest of the element the selection was made on.
    pub fn target_digest(&self) -> ElementDigest {
        self.target_digest
    }

    /// The index set or query.
    pub fn body(&self) -> &SelectionBody {
        &self.body
    }

    /// The index-set digest bound to the target digest; `None` for a query.
    pub fn content_digest(&self) -> Option<SelectionDigest> {
        match &self.body {
            SelectionBody::IndexSet(s) => Some(s.digest(self.target_digest)),
            SelectionBody::Query(_) => None,
        }
    }

    /// The serialised form (see the module docs); exactly the model's fields.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&SELECTION_TAG);
        out.push(SELECTION_VERSION);
        out.extend_from_slice(&self.target.to_bytes());
        out.extend_from_slice(&self.target_digest.to_bytes());
        match &self.body {
            SelectionBody::IndexSet(s) => {
                out.push(BODY_INDEX_SET);
                out.push(s.domain.code());
                out.extend_from_slice(&u64_of(s.ranges.len()).to_le_bytes());
                for &(a, b) in &s.ranges {
                    out.extend_from_slice(&a.to_le_bytes());
                    out.extend_from_slice(&b.to_le_bytes());
                }
            }
            SelectionBody::Query(q) => {
                out.push(BODY_QUERY);
                // `from_bytes` bounds the length to MAX_QUERY_BYTES, far below u32::MAX.
                let len = u32::try_from(q.0.len()).unwrap_or(u32::MAX);
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(&q.0);
            }
        }
        out
    }

    /// Parses the serialised form. Rejects a wrong tag or version, an unknown body tag or
    /// domain, ranges that are empty, unsorted, overlapping or touching, a query longer
    /// than [`MAX_QUERY_BYTES`], truncation and trailing bytes.
    pub fn decode(bytes: &[u8]) -> Result<Selection, SelError> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(6)? != SELECTION_TAG {
            return Err(r.bad(0, "tag"));
        }
        if r.u8()? != SELECTION_VERSION {
            return Err(r.bad(6, "version"));
        }
        let target = EntityId(r.array::<16>()?);
        let target_digest = ElementDigest(r.array::<32>()?);
        let tag_at = r.at;
        let body = match r.u8()? {
            BODY_INDEX_SET => {
                let domain_at = r.at;
                let code = r.u8()?;
                let domain = Domain::from_code(code).ok_or_else(|| r.bad(domain_at, "domain"))?;
                let n_at = r.at;
                let n = r.u64()?;
                // Bound the allocation by the bytes actually present.
                let remaining = u64_of(bytes.len() - r.at);
                if n > remaining / 16 {
                    return Err(r.bad(n_at, "range count exceeds input"));
                }
                let n = usize::try_from(n).map_err(|_| r.bad(n_at, "range count"))?;
                let mut ranges: Vec<(u64, u64)> = Vec::new();
                ranges.try_reserve_exact(n).map_err(|_| SelError::AllocationFailed)?;
                for _ in 0..n {
                    let at = r.at;
                    let (s, e) = (r.u64()?, r.u64()?);
                    if s >= e {
                        return Err(r.bad(at, "empty or reversed range"));
                    }
                    if let Some(&(_, prev_end)) = ranges.last() {
                        if s <= prev_end {
                            return Err(r.bad(at, "ranges not ascending and coalesced"));
                        }
                    }
                    ranges.push((s, e));
                }
                SelectionBody::IndexSet(IndexSet { domain, ranges })
            }
            BODY_QUERY => {
                let len_at = r.at;
                let len = r.u32()?;
                let len = usize::try_from(len).map_err(|_| r.bad(len_at, "query length"))?;
                if len > MAX_QUERY_BYTES {
                    return Err(SelError::QueryTooLarge { len: u64_of(len) });
                }
                SelectionBody::Query(SelectionQuery(r.take(len)?.to_vec()))
            }
            _ => return Err(r.bad(tag_at, "body tag")),
        };
        if r.at != bytes.len() {
            return Err(r.bad(r.at, "trailing bytes"));
        }
        Ok(Selection { target, target_digest, body })
    }
}

/// A bounds-checked little-endian reader.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn bad(&self, at: usize, reason: &'static str) -> SelError {
        SelError::Malformed { offset: u64_of(at), reason }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SelError> {
        let end = match self.at.checked_add(n) {
            Some(e) if e <= self.bytes.len() => e,
            _ => return Err(self.bad(self.at, "truncated")),
        };
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], SelError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, SelError> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32, SelError> {
        Ok(u32::from_le_bytes(self.array::<4>()?))
    }

    fn u64(&mut self) -> Result<u64, SelError> {
        Ok(u64::from_le_bytes(self.array::<8>()?))
    }
}

/// Applies an index-set selection to `element` (REQ-GEO.SEL.MODEL-003). In order: a query
/// body fails with [`SelError::NotIndexSet`]; an element whose digest differs from the
/// stored target digest fails with [`SelError::StaleSelection`]; a domain the element does
/// not have fails with [`SelError::DomainMismatch`]; a member not less than the element's
/// count fails with [`SelError::IndexOutOfRange`] (decoded selections are not
/// bound-checked until here). Otherwise the members are returned.
pub fn apply<'s>(selection: &'s Selection, element: &GeometryElement) -> Result<&'s IndexSet, SelError> {
    let set = match &selection.body {
        SelectionBody::IndexSet(s) => s,
        SelectionBody::Query(_) => return Err(SelError::NotIndexSet),
    };
    let found = digest(element);
    if found != selection.target_digest {
        return Err(SelError::StaleSelection { expected: selection.target_digest, found });
    }
    if let Some(count) = domain_count(element, set.domain)? {
        set.check_bound(count)?;
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// AC-5 (compile-time half): exhaustive destructuring of every serialised struct. A
    /// field added to the model breaks this test until SELECTION_FIELDS, INDEX_SET_FIELDS
    /// and the encoding are reviewed with it.
    #[test]
    fn model_fields_are_exactly_the_serialised_ones() {
        let set = IndexSet::from_indices(Domain::Points, &[1, 2], 3).unwrap();
        let IndexSet { domain, ranges } = set.clone();
        assert_eq!((domain, ranges), (Domain::Points, vec![(1, 3)]));
        let s = Selection::new(EntityId([1; 16]), ElementDigest([2; 32]), SelectionBody::IndexSet(set));
        let Selection { target, target_digest, body } = s;
        let _ = (target, target_digest);
        match body {
            SelectionBody::IndexSet(IndexSet { domain: _, ranges: _ }) => {}
            SelectionBody::Query(SelectionQuery(_bytes)) => {}
        }
        assert_eq!(SELECTION_FIELDS, ["target", "target_digest", "body"]);
        assert_eq!(INDEX_SET_FIELDS, ["domain", "ranges"]);
        assert_eq!(QUERY_FIELDS, ["query"]);
    }

    #[test]
    fn normalise_merges_overlap_and_touch() {
        let mut r = vec![(5, 7), (0, 2), (2, 3), (6, 9), (1, 2), (20, 21)];
        normalise(&mut r);
        assert_eq!(r, vec![(0, 3), (5, 9), (20, 21)]);
    }

    #[test]
    fn domain_codes_round_trip() {
        for d in Domain::ALL {
            assert_eq!(Domain::from_code(d.code()), Some(d));
        }
        assert_eq!(Domain::from_code(0), None);
        assert_eq!(Domain::from_code(5), None);
    }
}
