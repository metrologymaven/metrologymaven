---
id: IO.CLOUD.E57.XML
title: Hardened E57 XML section reader and writer
level: 4
parent: IO.CLOUD.E57
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-io-e57]
satisfies: [REQ-IO.CLOUD.E57-001]
covers: []
depends_on: [IO.CLOUD.E57.PHYS, IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CLOUD.E57.XML: Hardened E57 XML section reader and writer

## Purpose

Parses the E57 XML section into a typed element tree (structures, vectors, compressed vectors, integers, scaled integers, floats, strings, blobs) and writes it back. The parser is hardened: no DTDs, no external or custom entities, bounded depth, element count and string length. It exposes the scans (`data3D`) with their pose, point-field prototype and binary-section offsets, and counts `images2D`.

## Scope

**In:**
- Hardened XML parsing (pure-Rust parser on the D-001 allowlist [unverified: crate choice]), rejecting DOCTYPE and non-predefined entities
- E57 element types and attributes (`type`, `minimum`, `maximum`, `scale`, `offset`, `precision`, `fileOffset`, `recordCount`)
- `data3D` items: guid, name, description (untrusted), pose (quaternion w,x,y,z and translation), point-field prototype, codecs, cartesian/spherical bounds, intensity and colour limits, `fileOffset` and `recordCount` of the points section
- `images2D` count; unknown extension elements kept as counted, untrusted metadata
- Writer: deterministic XML for the scans the exporter writes

**Out:**
- Binary data (`IO.CLOUD.E57.CVEC`), pose application and point mapping (`IO.CLOUD.E57.READ`)

## Requirements

- **REQ-IO.CLOUD.E57.XML-001** When the XML section is parsed, the reader shall return one `ScanDesc` per `data3D` child, in document order, with its pose, prototype and points-section offset and record count.
- **REQ-IO.CLOUD.E57.XML-002** If the XML contains a DOCTYPE declaration or a non-predefined entity reference, then parsing shall fail with `malformed{reason: xml_forbidden_construct}`.
- **REQ-IO.CLOUD.E57.XML-003** If element depth exceeds 64, element count exceeds 10^6 or a text node exceeds 1 MiB, then parsing shall fail with `limit_exceeded` naming the bound.
- **REQ-IO.CLOUD.E57.XML-004** If a pose quaternion's norm differs from 1 by more than 1e-6, then the reader shall report a warning and pass the quaternion unnormalised to the caller, which decides; all string values shall be untrusted text.
- **REQ-IO.CLOUD.E57.XML-005** XML written by the writer and parsed back shall yield an equal element tree, and writing the same tree twice shall give byte-identical XML.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cloud/e57/xml/two_scans.xml` (two `data3D`, one with Cartesian double fields, one with spherical scaled-integer fields), when parsed, then two `ScanDesc` with poses, prototypes and record counts are returned in order.
- **AC-2** (error path) Given `fixtures/io/cloud/e57/xml/doctype.xml`, then `malformed{reason: xml_forbidden_construct}`.
- **AC-3** (error path) Given a generated XML with depth 65, then `limit_exceeded{bound: depth}`.
- **AC-4** Given a tree built by the writer for 3 scans, then write → parse → equal, and two writes are byte-identical.
- **AC-5** Given the XML fuzz target run for its CI budget, then no panic occurs.

## Design

- **Registry commands:** none (library).
- **Data structures:** `E57Node` enum; `ScanDesc { guid: Untrusted, name: Untrusted, pose: Option<Pose { q: [f64; 4], t: [f64; 3] }>, prototype: Vec<FieldDesc>, codecs, points: SectionRef { offset, records }, bounds, limits }`; `FieldDesc { name, kind: Integer{min,max} | ScaledInteger{min,max,scale,offset} | Float{precision, min?, max?} | String }`.
- **Algorithm:** streaming pull parser over `LogicalReader` bytes of the XML section; build typed nodes with depth guard; reals parsed with correct rounding.
- **Errors:** `malformed{reason}`, `limit_exceeded{bound}`.
- **Determinism:** D0.
- **Performance budget:** XML of 10^4 scans in < 1 s [unverified until measured].
- **Crate:** `fid-io-e57`.

## Test fixtures

Hand-written XML sections under `fixtures/io/cloud/e57/xml/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-e57`, parse the E57 XML section (read via the physical layer) with a hardened pure-Rust XML parser: reject DOCTYPE and non-predefined entities (`malformed{reason: xml_forbidden_construct}`), cap depth 64, elements 10^6, text 1 MiB (`limit_exceeded{bound}`). Build typed E57 nodes and return `ScanDesc` per `data3D` (guid, name, pose quaternion + translation, field prototype with integer/scaled-integer/float/string kinds, codecs, bounds, limits, points section offset and record count), count `images2D`; strings are untrusted text; warn when a quaternion norm is off by > 1e-6. Add a deterministic writer whose output parses back equal. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Fuzz target. Tests AC-1..AC-5.

## Open questions

- Built in `fid-io-e57` (module `xml`) on the shared hardened reader `fid-xml`. Fail-closed choices made at build time, each a static `malformed` reason: namespace declarations only on the root (extension namespaces are root prefixes); attributes not in the type's list (or in any namespace) refused; `Vector` children must be named `vectorChild`; a present `pose` needs both `rotation` and `translation`; `cartesianBounds` and `sphericalBounds` need all six `Float` values, ordered (azimuth excepted); limit pairs must be of one kind; prototype fields must be `Integer`, `ScaledInteger`, `Float` or `String` (no nested prototypes); a BOM and U+007F to U+009F in strings and namespace names are refused. Real files may need some of these relaxed once a corpus file is available [unverified].
- Absent attributes and empty values are kept as absent (`None`), not filled with the format defaults (scale 1, offset 0, full integer range, double precision); `IO.CLOUD.E57.CVEC` applies the defaults.
- The writer's `scan_tree` writes no `coordinateMetadata` and no `creationDateTime` (a timestamp would break byte-identical output); whether ASTM E2807 requires them is for `IO.CLOUD.E57.WRITE` [unverified].
- AC-5: the cargo-fuzz target `crates/fid-io-e57/fuzz` (`xml_section`) is not run by PR CI (the `io-fuzz` workflow runs the `fid-io-corp` targets); CI runs the in-tree deterministic mutation test (3,000 mutated sections, accepted ones must round-trip), as for `IO.CLOUD.LAS.HDR`.
- `fid-xml` accepts a QName whose local part is a Name but not an NCName (`a:1x`); this reader refuses it (`element_name`). Whether `IO.QIF.XML.PARSE` should refuse it too is for that node.

## Children

_Leaf: none._
