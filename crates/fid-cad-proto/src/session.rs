//! The host side of one load: handshake, message order and record-table validation
//! (REQ-IO.CAD.WORKER.PROTO-003, -004).

use crate::error::ProtoError;
use crate::frame::{FrameReader, ProtoLimits};
use crate::msg::{ProtoVersion, Record, RecordBody, RecordKind, Response};
use crate::wire::COST_TABLE_ENTRY;
use fid_io_reg::{Budget, IoError, LimitKind, Position};
use std::io::Read;

/// Checks a worker's version against the host's: the majors must be equal
/// (`protocol_mismatch` otherwise), and the minor in use is the lower of the two.
pub fn negotiate(host: ProtoVersion, worker: ProtoVersion) -> Result<ProtoVersion, ProtoError> {
    if host.major != worker.major {
        return Err(ProtoError::ProtocolMismatch { host, worker });
    }
    Ok(ProtoVersion { major: host.major, minor: host.minor.min(worker.minor) })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    AwaitHelloAck,
    Loading,
    Done,
}

/// Reads and validates the worker's responses for one load. Accepted order:
/// `HelloAck`, then any number of `Progress` and `Records`, then one `Report` or
/// `Failed`, then the end of the stream. Each record is checked against the session's
/// record table before it is added: every index must name an earlier record of an
/// allowed kind. `Report.records` must equal the table size. Records count against
/// [`ProtoLimits::records_cap`] and the budget's element limit.
#[derive(Debug)]
pub struct HostSession<R> {
    reader: FrameReader<R>,
    host: ProtoVersion,
    state: State,
    negotiated: Option<ProtoVersion>,
    table: Vec<RecordKind>,
    next_seq: u64,
}

impl<R: Read> HostSession<R> {
    /// A session reading the worker's stdout `inner`, for a host speaking `host`.
    pub fn new(inner: R, budget: Budget, limits: ProtoLimits, host: ProtoVersion) -> Self {
        HostSession {
            reader: FrameReader::new(inner, budget, limits),
            host,
            state: State::AwaitHelloAck,
            negotiated: None,
            table: Vec::new(),
            next_seq: 0,
        }
    }

    /// The negotiated version, once the handshake succeeded.
    pub fn negotiated(&self) -> Option<ProtoVersion> {
        self.negotiated
    }

    /// The kinds of all records accepted so far, in table order.
    pub fn table(&self) -> &[RecordKind] {
        &self.table
    }

    /// The frame reader (budget and byte counters).
    pub fn reader(&self) -> &FrameReader<R> {
        &self.reader
    }

    /// True once `Report` or `Failed` was accepted.
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// The next validated response, or `None` at the end of the stream after the final
    /// message. The end of the stream before the final message is `unexpected_eof`.
    pub fn next_response(&mut self) -> Result<Option<Response>, ProtoError> {
        let (frame, resp) = match self.reader.next_response()? {
            Some(x) => x,
            None if self.state == State::Done => return Ok(None),
            None => {
                let e = ProtoError::UnexpectedEof { frame: self.reader.frames_read() };
                return Err(self.reader.fail(e));
            }
        };
        match self.accept(frame, &resp) {
            Ok(()) => Ok(Some(resp)),
            Err(e) => Err(self.reader.fail(e)),
        }
    }

    fn accept(&mut self, frame: u64, resp: &Response) -> Result<(), ProtoError> {
        let invalid = |detail: &'static str| ProtoError::InvalidMessage { frame, detail };
        match (self.state, resp) {
            (State::AwaitHelloAck, Response::HelloAck { version, .. }) => {
                self.negotiated = Some(negotiate(self.host, *version)?);
                self.state = State::Loading;
                Ok(())
            }
            (State::AwaitHelloAck, _) => Err(invalid("expected_hello_ack")),
            (State::Loading, Response::HelloAck { .. }) => Err(invalid("unexpected_hello_ack")),
            (State::Loading, Response::Progress { .. }) => Ok(()),
            (State::Loading, Response::Records { seq, records }) => {
                if u64::from(*seq) != self.next_seq {
                    return Err(invalid("records_out_of_order"));
                }
                self.next_seq = self.next_seq.saturating_add(1);
                records.iter().try_for_each(|r| self.add(frame, r))
            }
            (State::Loading, Response::Report { records, .. }) => {
                if *records != self.table.len() as u64 {
                    return Err(invalid("report_count_mismatch"));
                }
                self.state = State::Done;
                Ok(())
            }
            (State::Loading, Response::Failed { .. }) => {
                self.state = State::Done;
                Ok(())
            }
            (State::Done, _) => Err(invalid("frame_after_end")),
        }
    }

    fn add(&mut self, frame: u64, r: &Record) -> Result<(), ProtoError> {
        let at = Position::byte(self.reader.budget().bytes_used());
        let cap = self.reader.limits().records_cap();
        if self.table.len() as u64 >= cap {
            return Err(ProtoError::Harness(IoError::LimitExceeded { limit: LimitKind::Elements, value: cap, at }));
        }
        if let RecordBody::Instance { parent, child, .. } = &r.body {
            if parent == child {
                return Err(ProtoError::InvalidMessage { frame, detail: "instance_self_reference" });
            }
        }
        let table = &self.table;
        r.body.try_for_each_reference(|index, allowed| {
            match usize::try_from(index).ok().and_then(|i| table.get(i)) {
                None => Err(ProtoError::InvalidMessage { frame, detail: "index_out_of_range" }),
                Some(k) if !allowed.contains(k) => {
                    Err(ProtoError::InvalidMessage { frame, detail: "index_wrong_kind" })
                }
                Some(_) => Ok(()),
            }
        })?;
        let budget = self.reader.budget_mut();
        budget.charge_elements(1, at)?;
        // The table grows by doubling: at most two bytes per entry, charged as eight.
        budget.charge_alloc(COST_TABLE_ENTRY, at)?;
        self.table.try_reserve(1).map_err(|_| IoError::Io { kind: std::io::ErrorKind::OutOfMemory, at })?;
        self.table.push(r.body.kind());
        Ok(())
    }
}
