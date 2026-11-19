//! The CAD worker protocol (spec node `IO.CAD.WORKER.PROTO`): the typed, versioned
//! messages the document process (host) and the isolated CAD worker exchange over the
//! worker's stdin and stdout. Shared by host and worker; no I/O beyond the
//! [`std::io::Read`] and [`std::io::Write`] traits, no `unsafe`, no panics.
//!
//! Every byte from the worker is hostile. [`FrameReader`] checks each declared frame
//! length against the per-frame and per-session caps of [`ProtoLimits`] before it reads
//! or allocates anything, reads through the `IO.REG.LIMITS` [`fid_io_reg::BoundedSource`],
//! and charges every buffer growth and every decoded item to the [`fid_io_reg::Budget`].
//! [`Response::decode`] applies the fixed schema; [`HostSession`] then checks message
//! order, the handshake ([`negotiate`]) and every record index against the session's
//! record table. Text from the worker is [`WireText`], convertible only to the harness's
//! [`fid_io_reg::UntrustedText`].
//!
//! # Wire format (version 1.0)
//!
//! A frame is a `u32` little-endian payload length, a `u8` kind, then the payload.
//! Payload primitives: integers little-endian; `bool` one byte `0` or `1`; `f64` as the
//! little-endian IEEE 754 bits, finite only; `Option<T>` a tag byte `0` (absent) or `1`
//! then `T`; `bytes` and `text` a `u32` length then the bytes (text must be UTF-8);
//! `Vec<T>` a `u32` count then the items. Nothing else: no padding, no maps, no
//! self-description, so each message has exactly one encoding.
//!
//! | Kind | Message | Payload |
//! |---|---|---|
//! | `0x01` | `Hello` | major `u16`, minor `u16` |
//! | `0x02` | `LoadStep` | pmi `bool`, source `bytes` |
//! | `0x03` | `LoadIges` | source `bytes` |
//! | `0x04` | `Shutdown` | empty |
//! | `0x81` | `HelloAck` | major `u16`, minor `u16`, reader `text`, reader_version `text` |
//! | `0x82` | `Progress` | done `u64`, total `u64` (done ≤ total) |
//! | `0x83` | `Records` | seq `u32`, `Vec<Record>` |
//! | `0x84` | `Report` | records `u64`, skipped `u64`, notes `Vec<text>` |
//! | `0x85` | `Failed` | code `u8` ([`FailCode`]), detail `text` |
//!
//! A record is its [`RecordKind`] code (`u8`), a source (`u8` `0` then STEP entity
//! `u64`, or `1` then IGES directory entry `u32`), a name `Option<text>`, then by kind:
//!
//! | Code | Kind | Fields |
//! |---|---|---|
//! | 0 | Surface | [`SurfaceKind`] `u8`, params `Vec<f64>` |
//! | 1 | Curve | [`CurveKind`] `u8`, params `Vec<f64>` |
//! | 2 | Pcurve | surface `u32`, params `Vec<f64>` |
//! | 3 | Vertex | x, y, z `f64` |
//! | 4 | Edge | curve, start, end `u32` |
//! | 5 | Coedge | edge `u32`, reversed `bool`, pcurve `Option<u32>` |
//! | 6 | Loop | coedges `Vec<u32>` |
//! | 7 | Face | surface `u32`, same_sense `bool`, loops `Vec<u32>` |
//! | 8 | Shell | closed `bool`, faces `Vec<u32>` |
//! | 9 | Body | shells `Vec<u32>` |
//! | 10 | Product | bodies `Vec<u32>` |
//! | 11 | Instance | parent `u32`, child `u32`, transform 12 × `f64` |
//! | 12 | PmiItem | [`PmiKind`] `u8`, value `Option<f64>`, targets `Vec<u32>` |
//! | 13 | IgesEntity | entity_type `u16`, form `u16`, params `Vec<f64>` |
//! | 14 | Skipped | [`SkipReason`] `u8` |
//!
//! No message carries a file-system path or network address: the source file travels
//! as `bytes` inside the load request (REQ-IO.CAD.WORKER.PROTO-001).

mod error;
mod frame;
mod msg;
mod session;
mod wire;

pub use error::{Cap, CrashReason, EncodeError, ProtoError};
pub use frame::{
    Frame, FrameReader, ProtoLimits, RawFrame, DEFAULT_FRAME_MAX, DEFAULT_RECORDS_MAX, DEFAULT_SESSION_MAX,
    HEADER_LEN,
};
pub use msg::{
    kind, CurveKind, FailCode, IgesOptions, PmiKind, ProtoVersion, Record, RecordBody, RecordKind, Request, Response,
    SkipReason, SourceRef, StepOptions, SurfaceKind, WireText,
};
pub use session::{negotiate, HostSession};
