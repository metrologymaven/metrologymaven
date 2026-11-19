//! Message types and their fixed-schema encoding (REQ-IO.CAD.WORKER.PROTO-001, -003, -005).

use crate::error::{EncodeError, ProtoError};
use crate::frame::Frame;
use crate::wire::{Decoder, Writer, COST_RECORD, COST_SLOT};
use fid_io_reg::{Budget, Position, UntrustedText};
use std::fmt;

/// A protocol version. Host and worker must share the major version; the minor version
/// in use is the lower of the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtoVersion {
    /// Incompatible changes bump this.
    pub major: u16,
    /// Compatible additions bump this.
    pub minor: u16,
}

impl ProtoVersion {
    /// The version this crate speaks.
    pub const CURRENT: ProtoVersion = ProtoVersion { major: 1, minor: 0 };

    /// The major version as `"<major>.x"`, the form protocol-mismatch errors report.
    pub fn major_label(&self) -> String {
        format!("{}.x", self.major)
    }
}

impl fmt::Display for ProtoVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Text received from the worker (reader names, notes, record names, failure detail).
/// It is untrusted (CONSTITUTION C6): like [`UntrustedText`] it offers no `&str` view, no
/// `Display` and no conversion into `String`. The only way out is
/// [`WireText::to_untrusted`]; the raw text is used by this crate's encoder only.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct WireText(Box<str>);

impl WireText {
    /// Wraps text (the worker side builds messages with this).
    pub fn new(text: &str) -> WireText {
        WireText(Box::from(text))
    }

    /// The text as the harness's untrusted-text type.
    pub fn to_untrusted(&self) -> UntrustedText {
        UntrustedText::new(&self.0)
    }

    /// Length in UTF-8 bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True for the empty string.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for WireText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_untrusted(), f)
    }
}

/// Declares a closed wire enum with stable one-byte codes.
macro_rules! wire_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $var:ident = $val:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($(#[$vm])* $var),+ }

        impl $name {
            /// Every value, in code order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];

            /// The wire code.
            pub fn code(self) -> u8 {
                match self { $($name::$var => $val),+ }
            }

            /// The value for a wire code, if the code is known.
            pub fn from_code(code: u8) -> Option<$name> {
                match code { $($val => Some($name::$var),)+ _ => None }
            }
        }
    };
}

wire_enum! {
    /// Geometric kind of a surface record.
    SurfaceKind { Plane = 0, Cylinder = 1, Cone = 2, Sphere = 3, Torus = 4, BSpline = 5, Other = 6 }
}
wire_enum! {
    /// Geometric kind of a curve record.
    CurveKind { Line = 0, Circle = 1, Ellipse = 2, BSpline = 3, Other = 4 }
}
wire_enum! {
    /// Kind of a PMI item.
    PmiKind { Dimension = 0, Tolerance = 1, Datum = 2, Note = 3 }
}
wire_enum! {
    /// Why the worker skipped a source entity.
    SkipReason { Unsupported = 0, Malformed = 1, Filtered = 2 }
}
wire_enum! {
    /// Failure class of a `Failed` response.
    FailCode { Parse = 0, Unsupported = 1, Internal = 2, OutOfMemory = 3 }
}
wire_enum! {
    /// Kind of a record, its wire tag, and the type an index field may point at.
    RecordKind {
        Surface = 0, Curve = 1, Pcurve = 2, Vertex = 3, Edge = 4, Coedge = 5, Loop = 6, Face = 7,
        Shell = 8, Body = 9, Product = 10, Instance = 11, PmiItem = 12, IgesEntity = 13, Skipped = 14,
    }
}

/// Where a record came from in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceRef {
    /// A STEP entity instance number (`#n`).
    Step(u64),
    /// An IGES directory-entry sequence number.
    Iges(u32),
}

/// One typed record of a `Records` batch. Index fields are positions in the session's
/// record table (every record received so far, counted from 0 across batches); a
/// [`crate::HostSession`] accepts only indices to earlier records of the named kind.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Source entity.
    pub source: SourceRef,
    /// Optional name taken from the file (untrusted).
    pub name: Option<WireText>,
    /// The typed content.
    pub body: RecordBody,
}

/// The content of a [`Record`]; the comment on each index field names the kind it must
/// point at.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordBody {
    /// Surface geometry.
    Surface {
        /// Kind.
        kind: SurfaceKind,
        /// Kind-specific parameters.
        params: Vec<f64>,
    },
    /// Curve geometry.
    Curve {
        /// Kind.
        kind: CurveKind,
        /// Kind-specific parameters.
        params: Vec<f64>,
    },
    /// Parameter-space curve.
    Pcurve {
        /// Surface index.
        surface: u32,
        /// Parameters.
        params: Vec<f64>,
    },
    /// Topological vertex.
    Vertex {
        /// Point, model units.
        point: [f64; 3],
    },
    /// Topological edge.
    Edge {
        /// Curve index.
        curve: u32,
        /// Start vertex index.
        start: u32,
        /// End vertex index.
        end: u32,
    },
    /// Oriented use of an edge in a loop.
    Coedge {
        /// Edge index.
        edge: u32,
        /// Opposite to the edge direction.
        reversed: bool,
        /// Optional pcurve index.
        pcurve: Option<u32>,
    },
    /// Face boundary loop.
    Loop {
        /// Coedge indices.
        coedges: Vec<u32>,
    },
    /// Topological face.
    Face {
        /// Surface index.
        surface: u32,
        /// Normal agrees with the surface.
        same_sense: bool,
        /// Loop indices.
        loops: Vec<u32>,
    },
    /// Shell.
    Shell {
        /// Closed shell.
        closed: bool,
        /// Face indices.
        faces: Vec<u32>,
    },
    /// Body.
    Body {
        /// Shell indices.
        shells: Vec<u32>,
    },
    /// Assembly product.
    Product {
        /// Body indices.
        bodies: Vec<u32>,
    },
    /// Placement of a product in a parent product.
    Instance {
        /// Parent product index.
        parent: u32,
        /// Child product index.
        child: u32,
        /// Row-major 3x4 placement matrix.
        transform: [f64; 12],
    },
    /// PMI item.
    PmiItem {
        /// Kind.
        kind: PmiKind,
        /// Optional nominal value.
        value: Option<f64>,
        /// Face, edge or vertex indices.
        targets: Vec<u32>,
    },
    /// Raw IGES entity.
    IgesEntity {
        /// Entity type number.
        entity_type: u16,
        /// Form number.
        form: u16,
        /// Parameter data.
        params: Vec<f64>,
    },
    /// A source entity the worker did not convert.
    Skipped {
        /// Why.
        reason: SkipReason,
    },
}

impl RecordBody {
    /// The record kind.
    pub fn kind(&self) -> RecordKind {
        match self {
            RecordBody::Surface { .. } => RecordKind::Surface,
            RecordBody::Curve { .. } => RecordKind::Curve,
            RecordBody::Pcurve { .. } => RecordKind::Pcurve,
            RecordBody::Vertex { .. } => RecordKind::Vertex,
            RecordBody::Edge { .. } => RecordKind::Edge,
            RecordBody::Coedge { .. } => RecordKind::Coedge,
            RecordBody::Loop { .. } => RecordKind::Loop,
            RecordBody::Face { .. } => RecordKind::Face,
            RecordBody::Shell { .. } => RecordKind::Shell,
            RecordBody::Body { .. } => RecordKind::Body,
            RecordBody::Product { .. } => RecordKind::Product,
            RecordBody::Instance { .. } => RecordKind::Instance,
            RecordBody::PmiItem { .. } => RecordKind::PmiItem,
            RecordBody::IgesEntity { .. } => RecordKind::IgesEntity,
            RecordBody::Skipped { .. } => RecordKind::Skipped,
        }
    }

    /// Calls `f` on every index field with the kinds it may point at, in wire order,
    /// stopping at the first error. Allocates nothing, so validating a record costs no
    /// memory beyond the record itself.
    pub fn try_for_each_reference<E>(
        &self,
        mut f: impl FnMut(u32, &'static [RecordKind]) -> Result<(), E>,
    ) -> Result<(), E> {
        const SURFACE: &[RecordKind] = &[RecordKind::Surface];
        const CURVE: &[RecordKind] = &[RecordKind::Curve];
        const PCURVE: &[RecordKind] = &[RecordKind::Pcurve];
        const VERTEX: &[RecordKind] = &[RecordKind::Vertex];
        const EDGE: &[RecordKind] = &[RecordKind::Edge];
        const COEDGE: &[RecordKind] = &[RecordKind::Coedge];
        const LOOP: &[RecordKind] = &[RecordKind::Loop];
        const FACE: &[RecordKind] = &[RecordKind::Face];
        const SHELL: &[RecordKind] = &[RecordKind::Shell];
        const BODY: &[RecordKind] = &[RecordKind::Body];
        const PRODUCT: &[RecordKind] = &[RecordKind::Product];
        const PMI_TARGET: &[RecordKind] = &[RecordKind::Face, RecordKind::Edge, RecordKind::Vertex];
        match self {
            RecordBody::Surface { .. }
            | RecordBody::Curve { .. }
            | RecordBody::Vertex { .. }
            | RecordBody::IgesEntity { .. }
            | RecordBody::Skipped { .. } => Ok(()),
            RecordBody::Pcurve { surface, .. } => f(*surface, SURFACE),
            RecordBody::Edge { curve, start, end } => {
                f(*curve, CURVE)?;
                f(*start, VERTEX)?;
                f(*end, VERTEX)
            }
            RecordBody::Coedge { edge, pcurve, .. } => {
                f(*edge, EDGE)?;
                match pcurve {
                    Some(p) => f(*p, PCURVE),
                    None => Ok(()),
                }
            }
            RecordBody::Loop { coedges } => coedges.iter().try_for_each(|&i| f(i, COEDGE)),
            RecordBody::Face { surface, loops, .. } => {
                f(*surface, SURFACE)?;
                loops.iter().try_for_each(|&i| f(i, LOOP))
            }
            RecordBody::Shell { faces, .. } => faces.iter().try_for_each(|&i| f(i, FACE)),
            RecordBody::Body { shells } => shells.iter().try_for_each(|&i| f(i, SHELL)),
            RecordBody::Product { bodies } => bodies.iter().try_for_each(|&i| f(i, BODY)),
            RecordBody::Instance { parent, child, .. } => {
                f(*parent, PRODUCT)?;
                f(*child, PRODUCT)
            }
            RecordBody::PmiItem { targets, .. } => targets.iter().try_for_each(|&i| f(i, PMI_TARGET)),
        }
    }
}

/// Options of a STEP load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StepOptions {
    /// Also convert PMI.
    pub pmi: bool,
}

/// Options of an IGES load (none yet; encoded as zero bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct IgesOptions {}

/// Host to worker. No request names a file-system path or network address: the source
/// travels as bytes (REQ-IO.CAD.WORKER.PROTO-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Opens the session with the host's version.
    Hello {
        /// Host version.
        version: ProtoVersion,
    },
    /// Load a STEP file given as bytes.
    LoadStep {
        /// The whole source file.
        bytes: Vec<u8>,
        /// Options.
        options: StepOptions,
    },
    /// Load an IGES file given as bytes.
    LoadIges {
        /// The whole source file.
        bytes: Vec<u8>,
        /// Options.
        options: IgesOptions,
    },
    /// Ends the session; the worker exits.
    Shutdown,
}

/// Worker to host. Every one is hostile input to the host.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// Answers `Hello` with the worker's version and reader identity.
    HelloAck {
        /// Worker version.
        version: ProtoVersion,
        /// Reader name (untrusted).
        reader: WireText,
        /// Reader version (untrusted).
        reader_version: WireText,
    },
    /// Progress of the current load; `done <= total`.
    Progress {
        /// Units done.
        done: u64,
        /// Units in total.
        total: u64,
    },
    /// A batch of records; `seq` counts batches from 0.
    Records {
        /// Batch number.
        seq: u32,
        /// Records, in table order.
        records: Vec<Record>,
    },
    /// Final message of a successful load.
    Report {
        /// Records sent in total (must equal the session's table size).
        records: u64,
        /// Source entities skipped.
        skipped: u64,
        /// Notes (untrusted).
        notes: Vec<WireText>,
    },
    /// Final message of a failed load.
    Failed {
        /// Failure class.
        code: FailCode,
        /// Detail (untrusted).
        detail: WireText,
    },
}

/// Frame kind codes.
pub mod kind {
    /// `Request::Hello`.
    pub const HELLO: u8 = 0x01;
    /// `Request::LoadStep`.
    pub const LOAD_STEP: u8 = 0x02;
    /// `Request::LoadIges`.
    pub const LOAD_IGES: u8 = 0x03;
    /// `Request::Shutdown`.
    pub const SHUTDOWN: u8 = 0x04;
    /// `Response::HelloAck`.
    pub const HELLO_ACK: u8 = 0x81;
    /// `Response::Progress`.
    pub const PROGRESS: u8 = 0x82;
    /// `Response::Records`.
    pub const RECORDS: u8 = 0x83;
    /// `Response::Report`.
    pub const REPORT: u8 = 0x84;
    /// `Response::Failed`.
    pub const FAILED: u8 = 0x85;
}

fn put_version(w: &mut Writer, v: ProtoVersion) {
    w.u16(v.major);
    w.u16(v.minor);
}

fn put_text(w: &mut Writer, t: &WireText) -> Result<(), EncodeError> {
    w.bytes(t.0.as_bytes())
}

fn put_record(w: &mut Writer, r: &Record) -> Result<(), EncodeError> {
    w.u8(r.body.kind().code());
    match r.source {
        SourceRef::Step(n) => {
            w.u8(0);
            w.u64(n);
        }
        SourceRef::Iges(n) => {
            w.u8(1);
            w.u32(n);
        }
    }
    match &r.name {
        None => w.u8(0),
        Some(t) => {
            w.u8(1);
            put_text(w, t)?;
        }
    }
    match &r.body {
        RecordBody::Surface { kind, params } => {
            w.u8(kind.code());
            w.f64s(params)?;
        }
        RecordBody::Curve { kind, params } => {
            w.u8(kind.code());
            w.f64s(params)?;
        }
        RecordBody::Pcurve { surface, params } => {
            w.u32(*surface);
            w.f64s(params)?;
        }
        RecordBody::Vertex { point } => point.iter().try_for_each(|&x| w.f64(x))?,
        RecordBody::Edge { curve, start, end } => {
            w.u32(*curve);
            w.u32(*start);
            w.u32(*end);
        }
        RecordBody::Coedge { edge, reversed, pcurve } => {
            w.u32(*edge);
            w.bool(*reversed);
            match pcurve {
                None => w.u8(0),
                Some(p) => {
                    w.u8(1);
                    w.u32(*p);
                }
            }
        }
        RecordBody::Loop { coedges } => w.u32s(coedges)?,
        RecordBody::Face { surface, same_sense, loops } => {
            w.u32(*surface);
            w.bool(*same_sense);
            w.u32s(loops)?;
        }
        RecordBody::Shell { closed, faces } => {
            w.bool(*closed);
            w.u32s(faces)?;
        }
        RecordBody::Body { shells } => w.u32s(shells)?,
        RecordBody::Product { bodies } => w.u32s(bodies)?,
        RecordBody::Instance { parent, child, transform } => {
            w.u32(*parent);
            w.u32(*child);
            transform.iter().try_for_each(|&x| w.f64(x))?;
        }
        RecordBody::PmiItem { kind, value, targets } => {
            w.u8(kind.code());
            match value {
                None => w.u8(0),
                Some(v) => {
                    w.u8(1);
                    w.f64(*v)?;
                }
            }
            w.u32s(targets)?;
        }
        RecordBody::IgesEntity { entity_type, form, params } => {
            w.u16(*entity_type);
            w.u16(*form);
            w.f64s(params)?;
        }
        RecordBody::Skipped { reason } => w.u8(reason.code()),
    }
    Ok(())
}

impl Request {
    /// The frame kind code.
    pub fn kind(&self) -> u8 {
        match self {
            Request::Hello { .. } => kind::HELLO,
            Request::LoadStep { .. } => kind::LOAD_STEP,
            Request::LoadIges { .. } => kind::LOAD_IGES,
            Request::Shutdown => kind::SHUTDOWN,
        }
    }

    /// Encodes this request as a frame.
    pub fn to_frame(&self) -> Result<Frame, EncodeError> {
        let mut w = Writer::default();
        match self {
            Request::Hello { version } => put_version(&mut w, *version),
            Request::LoadStep { bytes, options } => {
                w.bool(options.pmi);
                w.bytes(bytes)?;
            }
            Request::LoadIges { bytes, options: IgesOptions {} } => w.bytes(bytes)?,
            Request::Shutdown => {}
        }
        Frame::new(self.kind(), w.buf)
    }

    /// Encodes this request as frame bytes (header and payload).
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        self.to_frame()?.to_bytes()
    }

    /// Decodes a request payload of frame kind `kind` (the worker side). `frame` is the
    /// frame number for errors; tracked allocation is charged to `budget` at `at`.
    pub fn decode(kind: u8, payload: &[u8], frame: u64, at: Position, budget: &mut Budget) -> Result<Self, ProtoError> {
        let mut d = Decoder::new(payload, frame, at, budget);
        let out = match kind {
            kind::HELLO => Request::Hello { version: version(&mut d)? },
            kind::LOAD_STEP => {
                let pmi = d.bool()?;
                Request::LoadStep { bytes: d.bytes()?, options: StepOptions { pmi } }
            }
            kind::LOAD_IGES => Request::LoadIges { bytes: d.bytes()?, options: IgesOptions {} },
            kind::SHUTDOWN => Request::Shutdown,
            _ => return Err(d.invalid("unknown_request_kind")),
        };
        d.finish()?;
        Ok(out)
    }
}

impl Response {
    /// The frame kind code.
    pub fn kind(&self) -> u8 {
        match self {
            Response::HelloAck { .. } => kind::HELLO_ACK,
            Response::Progress { .. } => kind::PROGRESS,
            Response::Records { .. } => kind::RECORDS,
            Response::Report { .. } => kind::REPORT,
            Response::Failed { .. } => kind::FAILED,
        }
    }

    /// Encodes this response as a frame (the worker side).
    pub fn to_frame(&self) -> Result<Frame, EncodeError> {
        let mut w = Writer::default();
        match self {
            Response::HelloAck { version, reader, reader_version } => {
                put_version(&mut w, *version);
                put_text(&mut w, reader)?;
                put_text(&mut w, reader_version)?;
            }
            Response::Progress { done, total } => {
                if done > total {
                    return Err(EncodeError::ProgressPastTotal);
                }
                w.u64(*done);
                w.u64(*total);
            }
            Response::Records { seq, records } => {
                w.u32(*seq);
                w.len(records.len())?;
                records.iter().try_for_each(|r| put_record(&mut w, r))?;
            }
            Response::Report { records, skipped, notes } => {
                w.u64(*records);
                w.u64(*skipped);
                w.len(notes.len())?;
                notes.iter().try_for_each(|t| put_text(&mut w, t))?;
            }
            Response::Failed { code, detail } => {
                w.u8(code.code());
                put_text(&mut w, detail)?;
            }
        }
        Frame::new(self.kind(), w.buf)
    }

    /// Encodes this response as frame bytes (header and payload).
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        self.to_frame()?.to_bytes()
    }

    /// Decodes and validates a response payload of frame kind `kind`: schema, enum
    /// ranges, finite floats, UTF-8 text, `done <= total`, no trailing bytes. Index
    /// ranges need the session's table and are checked by [`crate::HostSession`].
    pub fn decode(kind: u8, payload: &[u8], frame: u64, at: Position, budget: &mut Budget) -> Result<Self, ProtoError> {
        let mut d = Decoder::new(payload, frame, at, budget);
        let out = match kind {
            kind::HELLO_ACK => {
                let version = version(&mut d)?;
                let reader = WireText(d.text()?);
                Response::HelloAck { version, reader, reader_version: WireText(d.text()?) }
            }
            kind::PROGRESS => {
                let done = d.u64()?;
                let total = d.u64()?;
                if done > total {
                    return Err(d.invalid("progress_past_total"));
                }
                Response::Progress { done, total }
            }
            kind::RECORDS => {
                let seq = d.u32()?;
                // The smallest record is 8 bytes: tag, source tag, 4-byte IGES source,
                // name tag and a 1-byte body (`Skipped`).
                let n = d.count(8, COST_RECORD)?;
                let mut records = Vec::with_capacity(n);
                for _ in 0..n {
                    records.push(record(&mut d)?);
                }
                Response::Records { seq, records }
            }
            kind::REPORT => {
                let records = d.u64()?;
                let skipped = d.u64()?;
                let n = d.count(4, COST_SLOT)?;
                let mut notes = Vec::with_capacity(n);
                for _ in 0..n {
                    notes.push(WireText(d.text()?));
                }
                Response::Report { records, skipped, notes }
            }
            kind::FAILED => {
                let code = FailCode::from_code(d.u8()?).ok_or_else(|| d.invalid("unknown_fail_code"))?;
                Response::Failed { code, detail: WireText(d.text()?) }
            }
            _ => return Err(d.invalid("unknown_response_kind")),
        };
        d.finish()?;
        Ok(out)
    }
}

fn version(d: &mut Decoder<'_, '_>) -> Result<ProtoVersion, ProtoError> {
    let major = d.u16()?;
    Ok(ProtoVersion { major, minor: d.u16()? })
}

fn opt_u32(d: &mut Decoder<'_, '_>) -> Result<Option<u32>, ProtoError> {
    Ok(if d.present()? { Some(d.u32()?) } else { None })
}

fn record(d: &mut Decoder<'_, '_>) -> Result<Record, ProtoError> {
    let tag = RecordKind::from_code(d.u8()?).ok_or_else(|| d.invalid("unknown_record_kind"))?;
    let source = match d.u8()? {
        0 => SourceRef::Step(d.u64()?),
        1 => SourceRef::Iges(d.u32()?),
        _ => return Err(d.invalid("unknown_source_kind")),
    };
    let name = if d.present()? { Some(WireText(d.text()?)) } else { None };
    let body = match tag {
        RecordKind::Surface => {
            let kind = SurfaceKind::from_code(d.u8()?).ok_or_else(|| d.invalid("unknown_surface_kind"))?;
            RecordBody::Surface { kind, params: d.f64s()? }
        }
        RecordKind::Curve => {
            let kind = CurveKind::from_code(d.u8()?).ok_or_else(|| d.invalid("unknown_curve_kind"))?;
            RecordBody::Curve { kind, params: d.f64s()? }
        }
        RecordKind::Pcurve => {
            let surface = d.u32()?;
            RecordBody::Pcurve { surface, params: d.f64s()? }
        }
        RecordKind::Vertex => RecordBody::Vertex { point: d.f64_array()? },
        RecordKind::Edge => {
            let curve = d.u32()?;
            let start = d.u32()?;
            RecordBody::Edge { curve, start, end: d.u32()? }
        }
        RecordKind::Coedge => {
            let edge = d.u32()?;
            let reversed = d.bool()?;
            RecordBody::Coedge { edge, reversed, pcurve: opt_u32(d)? }
        }
        RecordKind::Loop => RecordBody::Loop { coedges: d.u32s()? },
        RecordKind::Face => {
            let surface = d.u32()?;
            let same_sense = d.bool()?;
            RecordBody::Face { surface, same_sense, loops: d.u32s()? }
        }
        RecordKind::Shell => {
            let closed = d.bool()?;
            RecordBody::Shell { closed, faces: d.u32s()? }
        }
        RecordKind::Body => RecordBody::Body { shells: d.u32s()? },
        RecordKind::Product => RecordBody::Product { bodies: d.u32s()? },
        RecordKind::Instance => {
            let parent = d.u32()?;
            let child = d.u32()?;
            RecordBody::Instance { parent, child, transform: d.f64_array()? }
        }
        RecordKind::PmiItem => {
            let kind = PmiKind::from_code(d.u8()?).ok_or_else(|| d.invalid("unknown_pmi_kind"))?;
            let value = if d.present()? { Some(d.f64()?) } else { None };
            RecordBody::PmiItem { kind, value, targets: d.u32s()? }
        }
        RecordKind::IgesEntity => {
            let entity_type = d.u16()?;
            let form = d.u16()?;
            RecordBody::IgesEntity { entity_type, form, params: d.f64s()? }
        }
        RecordKind::Skipped => {
            let reason = SkipReason::from_code(d.u8()?).ok_or_else(|| d.invalid("unknown_skip_reason"))?;
            RecordBody::Skipped { reason }
        }
    };
    Ok(Record { source, name, body })
}
