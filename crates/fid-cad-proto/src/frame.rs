//! Framing and the bounded frame reader (REQ-IO.CAD.WORKER.PROTO-002).

use crate::error::{Cap, EncodeError, ProtoError};
use crate::msg::{Request, Response};
use fid_io_reg::{BoundedSource, Budget, IoError, Position};
use std::io::{self, Read, Write};

/// Frame header length: `u32` little-endian payload length, then the `u8` kind.
pub const HEADER_LEN: usize = 5;
/// Default per-frame payload cap: 64 MiB.
pub const DEFAULT_FRAME_MAX: u32 = 64 * 1024 * 1024;
/// Default per-session cap on bytes received (headers and payloads): 8 GiB.
pub const DEFAULT_SESSION_MAX: u64 = 8 * 1024 * 1024 * 1024;
/// Default cap on records per session: 2^26 [unverified: to be tuned on the corpus].
pub const DEFAULT_RECORDS_MAX: u64 = 1 << 26;
/// Payload bytes are read in chunks of this size; the buffer grows only as bytes arrive.
const CHUNK: usize = 8192;

/// One frame: kind code and payload. The payload length is not stored; it is the
/// payload's length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Kind code (see [`crate::kind`]).
    pub kind: u8,
    /// Encoded payload.
    pub payload: Vec<u8>,
}

impl Frame {
    /// A frame; fails if the payload length does not fit the `u32` header.
    pub fn new(kind: u8, payload: Vec<u8>) -> Result<Frame, EncodeError> {
        u32::try_from(payload.len()).map_err(|_| EncodeError::TooLong)?;
        Ok(Frame { kind, payload })
    }

    /// Header and payload bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let len = u32::try_from(self.payload.len()).map_err(|_| EncodeError::TooLong)?;
        let mut out = Vec::with_capacity(self.payload.len().saturating_add(HEADER_LEN));
        out.extend_from_slice(&len.to_le_bytes());
        out.push(self.kind);
        out.extend_from_slice(&self.payload);
        Ok(out)
    }

    /// Writes header and payload to `w`.
    pub fn write_to<W: Write>(&self, w: &mut W) -> io::Result<()> {
        let len = u32::try_from(self.payload.len()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        w.write_all(&len.to_le_bytes())?;
        w.write_all(&[self.kind])?;
        w.write_all(&self.payload)
    }
}

/// Byte and record caps of one session. They may only be lowered from the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtoLimits {
    frame_max: u32,
    session_max: u64,
    records_max: u64,
}

impl ProtoLimits {
    /// The defaults: 64 MiB per frame, 8 GiB per session, 2^26 records.
    pub const DEFAULT: ProtoLimits =
        ProtoLimits { frame_max: DEFAULT_FRAME_MAX, session_max: DEFAULT_SESSION_MAX, records_max: DEFAULT_RECORDS_MAX };

    /// Lowers the per-frame payload cap (a higher value is ignored).
    pub fn frame_max(mut self, v: u32) -> Self {
        self.frame_max = self.frame_max.min(v);
        self
    }
    /// Lowers the per-session byte cap (a higher value is ignored).
    pub fn session_max(mut self, v: u64) -> Self {
        self.session_max = self.session_max.min(v);
        self
    }
    /// Lowers the per-session record cap (a higher value is ignored).
    pub fn records_max(mut self, v: u64) -> Self {
        self.records_max = self.records_max.min(v);
        self
    }

    /// Per-frame payload cap in bytes.
    pub fn frame_cap(&self) -> u32 {
        self.frame_max
    }
    /// Per-session byte cap.
    pub fn session_cap(&self) -> u64 {
        self.session_max
    }
    /// Per-session record cap.
    pub fn records_cap(&self) -> u64 {
        self.records_max
    }
}

impl Default for ProtoLimits {
    fn default() -> Self {
        ProtoLimits::DEFAULT
    }
}

/// Reads frames from a pipe through a [`BoundedSource`]. The declared length is checked
/// against both caps before any payload byte is read or any buffer grows, and the
/// payload buffer then grows only as bytes actually arrive, so a lying header costs at
/// most one chunk. One payload buffer is reused for every frame. After the first error
/// every later call returns that error again.
#[derive(Debug)]
pub struct FrameReader<R> {
    src: BoundedSource<R>,
    limits: ProtoLimits,
    session_bytes: u64,
    frames: u64,
    buf: Vec<u8>,
    kind: u8,
    frame_at: u64,
    failed: Option<ProtoError>,
}

impl<R: Read> FrameReader<R> {
    /// Reads from `inner`, charging `budget`, under `limits`.
    pub fn new(inner: R, budget: Budget, limits: ProtoLimits) -> Self {
        FrameReader {
            src: BoundedSource::new(inner, budget),
            limits,
            session_bytes: 0,
            frames: 0,
            buf: Vec::new(),
            kind: 0,
            frame_at: 0,
            failed: None,
        }
    }

    /// The budget.
    pub fn budget(&self) -> &Budget {
        self.src.budget()
    }
    /// The budget, for charges by the caller.
    pub fn budget_mut(&mut self) -> &mut Budget {
        self.src.budget_mut()
    }
    /// The limits.
    pub fn limits(&self) -> &ProtoLimits {
        &self.limits
    }
    /// Frames read completely so far (the number of the next frame).
    pub fn frames_read(&self) -> u64 {
        self.frames
    }
    /// Bytes of complete frames read so far.
    pub fn session_bytes(&self) -> u64 {
        self.session_bytes
    }

    fn fill(&mut self, out: &mut [u8]) -> Result<usize, ProtoError> {
        let mut got = 0;
        while got < out.len() {
            let Some(window) = out.get_mut(got..) else { break };
            let n = self.src.read_bounded(window)?;
            if n == 0 {
                break;
            }
            got = got.saturating_add(n);
        }
        Ok(got)
    }

    fn read_frame(&mut self) -> Result<bool, ProtoError> {
        let frame = self.frames;
        let mut header = [0u8; HEADER_LEN];
        let got = self.fill(&mut header)?;
        if got == 0 {
            return Ok(false);
        }
        if got < HEADER_LEN {
            return Err(ProtoError::UnexpectedEof { frame });
        }
        let [l0, l1, l2, l3, kind] = header;
        let len = u32::from_le_bytes([l0, l1, l2, l3]);
        if len > self.limits.frame_max {
            return Err(ProtoError::FrameTooLarge {
                frame,
                declared: u64::from(len),
                cap: Cap::Frame,
                limit: u64::from(self.limits.frame_max),
            });
        }
        let total = (HEADER_LEN as u64)
            .checked_add(u64::from(len))
            .and_then(|n| n.checked_add(self.session_bytes))
            .filter(|&t| t <= self.limits.session_max);
        let total = match total {
            Some(t) => t,
            None => {
                return Err(ProtoError::FrameTooLarge {
                    frame,
                    declared: u64::from(len),
                    cap: Cap::Session,
                    limit: self.limits.session_max,
                })
            }
        };
        let want = usize::try_from(len).map_err(|_| ProtoError::FrameTooLarge {
            frame,
            declared: u64::from(len),
            cap: Cap::Frame,
            limit: u64::from(self.limits.frame_max),
        })?;
        self.frame_at = self.src.offset().saturating_sub(HEADER_LEN as u64);
        self.buf.clear();
        let mut chunk = [0u8; CHUNK];
        while self.buf.len() < want {
            let step = (want - self.buf.len()).min(CHUNK);
            let Some(window) = chunk.get_mut(..step) else { return Err(ProtoError::UnexpectedEof { frame }) };
            let n = self.src.read_bounded(window)?;
            if n == 0 {
                return Err(ProtoError::UnexpectedEof { frame });
            }
            let needed = self.buf.len().saturating_add(n);
            let cap = self.buf.capacity();
            if needed > cap {
                let new_cap = needed.max(cap.saturating_mul(2)).min(want);
                let at = Position::byte(self.src.offset());
                self.src.budget_mut().charge_alloc((new_cap - cap) as u64, at)?;
                self.buf
                    .try_reserve_exact(new_cap - self.buf.len())
                    .map_err(|_| IoError::Io { kind: io::ErrorKind::OutOfMemory, at })?;
            }
            self.buf.extend_from_slice(chunk.get(..n).unwrap_or(&[]));
        }
        self.kind = kind;
        self.session_bytes = total;
        self.frames = frame.saturating_add(1);
        Ok(true)
    }

    fn guarded<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, ProtoError>) -> Result<T, ProtoError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        let out = f(self);
        if let Err(e) = &out {
            self.failed = Some(e.clone());
        }
        out
    }

    /// The next raw frame as `(frame number, kind, payload)`, or `None` at a clean end of
    /// input between frames.
    pub fn next_raw(&mut self) -> Result<Option<(u64, u8, &[u8])>, ProtoError> {
        let more = self.guarded(|r| r.read_frame())?;
        if !more {
            return Ok(None);
        }
        Ok(Some((self.frames.saturating_sub(1), self.kind, &self.buf)))
    }

    /// The next frame decoded as a [`Response`] (the host side), or `None` at a clean
    /// end of input.
    pub fn next_response(&mut self) -> Result<Option<(u64, Response)>, ProtoError> {
        self.guarded(|r| {
            if !r.read_frame()? {
                return Ok(None);
            }
            let frame = r.frames.saturating_sub(1);
            let at = Position::byte(r.frame_at);
            let resp = Response::decode(r.kind, &r.buf, frame, at, r.src.budget_mut())?;
            Ok(Some((frame, resp)))
        })
    }

    /// The next frame decoded as a [`Request`] (the worker side), or `None` at a clean
    /// end of input.
    pub fn next_request(&mut self) -> Result<Option<(u64, Request)>, ProtoError> {
        self.guarded(|r| {
            if !r.read_frame()? {
                return Ok(None);
            }
            let frame = r.frames.saturating_sub(1);
            let at = Position::byte(r.frame_at);
            let req = Request::decode(r.kind, &r.buf, frame, at, r.src.budget_mut())?;
            Ok(Some((frame, req)))
        })
    }

    /// Records a failure found by a caller (for example a session check), so later calls
    /// return it too.
    pub(crate) fn fail(&mut self, e: ProtoError) -> ProtoError {
        if self.failed.is_none() {
            self.failed = Some(e.clone());
        }
        e
    }
}
