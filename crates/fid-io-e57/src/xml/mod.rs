//! The E57 XML section: a hardened reader into a typed element tree, the scan descriptions
//! (`data3D`) taken from it, and a deterministic writer (spec node IO.CLOUD.E57.XML).
//!
//! # Reading
//!
//! [`parse_slice`] reads XML bytes and [`read_section`] the XML section of an opened E57 file
//! (through [`crate::LogicalReader`], so every page checksum is verified). Both stream the
//! bytes through the shared hardened pull reader `fid-xml`, which has no DTD support at all:
//! a DOCTYPE, any other markup declaration and any reference to an entity other than the
//! five predefined ones fail with `malformed{reason: xml_forbidden_construct}` before
//! anything is resolved (REQ-002). A byte order mark is refused (`xml_bom`): the section is
//! UTF-8 without one. The bounds (REQ-003) are element depth [`MAX_DEPTH`], element count
//! [`MAX_ELEMENTS`] and text length [`MAX_TEXT_LEN`]; going past one fails with
//! `limit_exceeded{bound}`, as do the caller's byte, element, allocation and wall-time budgets.
//!
//! The tree ([`E57Tree`], [`E57Node`]) holds every element of the section in document order,
//! each attribute exactly as present (an absent `minimum`, `scale`, `precision` and so on is
//! `None`, never a filled-in default), and every string as untrusted [`XmlText`]. The reader
//! fails closed (`malformed{reason}`, with a static reason code and the input position) on:
//! a root other than a `Structure` named `e57Root` in [`E57_NAMESPACE`]; a namespace
//! declaration anywhere but on the root, or the E57 namespace bound to a prefix, or two
//! prefixes bound to one namespace; an element in no or an undeclared namespace, or whose local name is not an NCName; a missing or
//! unknown `type`; an attribute that does not apply to the type (attributes in a namespace
//! included); a malformed number (strict XML Schema lexical forms; reals are parsed correctly
//! rounded and must be finite); `minimum > maximum`, a value outside its range, a zero or
//! non-finite scale; a single-precision float beyond the `f32` range; a duplicate key in a
//! `Structure`; a `Vector` child not named `vectorChild`, or of a different type from its
//! siblings unless `allowHeterogeneousChildren="1"`; a `CompressedVector` without exactly one
//! `prototype` and at most one `codecs` (a `Vector`); text inside a container; a child inside
//! a leaf; a control character (U+007F to U+009F) in a string.
//!
//! [`E57Xml::scans`] gives one [`ScanDesc`] per `data3D` child in document order (REQ-001),
//! [`E57Xml::images2d`] the `images2D` count, and [`E57Xml::extension_elements`] the number
//! of elements in an extension namespace and [`E57Xml::unknown_elements`] the number of
//! unprefixed elements whose name is not in [`E57_NAMES`] (both kept in the tree as
//! untrusted metadata). The root must hold `formatName` [`FORMAT_NAME`] and `versionMajor` 1. A pose
//! quaternion whose norm differs from 1 by more than [`QUATERNION_TOL`] is passed on
//! unnormalised with a [`XmlWarning::QuaternionNotUnit`] carrying the computed norm (REQ-004);
//! an empty `data3D` gives [`XmlWarning::NoScans`].
//!
//! # Writing
//!
//! [`scan_tree`] builds the tree the exporter writes for a list of scans, and [`write_tree`]
//! serialises any tree to one fixed byte form (UTF-8, LF line ends, two-space indentation,
//! attributes in a fixed order, reals in Rust's shortest round-trip `{:e}` form). The writer
//! checks the tree against the same rules as the reader first, so whatever it writes parses
//! back to an equal tree, and the same tree always gives the same bytes (REQ-005). A tree the
//! reader accepted round-trips unless escaping takes a string or the root tag past the
//! reader's size bound; the writer then refuses with [`WriteError::LimitExceeded`].
//!
//! Element names, attribute names, the namespace name and the `type` values follow the
//! public descriptions of ASTM E2807 (cited by designator only) and libE57Format's output and
//! are \[unverified\] against the standard text and a real corpus file.

mod lex;
mod read;
mod scan;
mod tree;
mod write;

pub use lex::E57_NAMES;
pub use read::{parse_slice, parse_slice_with, read_section, E57Xml, XmlLimits, RETAINED_NODE_COST};
pub use scan::{
    CartesianBounds, ColorLimits, FieldDesc, FieldKind, Pose, Scalar, ScalarRange, ScanDesc, SectionRef,
    SphericalBounds,
};
pub use tree::{Bound, E57Node, E57Tree, Key, Namespace, NodeType, Precision, XmlSectionError, XmlWarning};
pub use write::{scan_tree, write_tree, WriteError, FORMAT_NAME};

pub use fid_xml::XmlText;

/// The E57 XML namespace name \[unverified against ASTM E2807\].
pub const E57_NAMESPACE: &str = "http://www.astm.org/COMMIT/E57/2010-e57-v1.0";
/// Deepest element nesting accepted (the root is depth 1).
pub const MAX_DEPTH: u32 = 64;
/// Most elements accepted in one XML section.
pub const MAX_ELEMENTS: u64 = 1_000_000;
/// Longest text node (and markup token) accepted, in bytes: 1 MiB.
pub const MAX_TEXT_LEN: usize = 1 << 20;
/// A pose quaternion whose norm differs from 1 by more than this gets a warning.
pub const QUATERNION_TOL: f64 = 1e-6;
