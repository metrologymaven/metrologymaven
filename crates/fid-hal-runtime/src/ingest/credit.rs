//! Credit ledger for data-plane flow control (spec leaf `HAL.ACQ.INGEST.CREDIT`,
//! REQ-HAL.ACQ.INGEST-001).
//!
//! A slow consumer must slow the source down instead of losing samples. The runtime grants a
//! source credit in two units at once, bytes and batches; the source may send only while it
//! holds credit, and every admitted batch is held "in flight" until a downstream stage
//! acknowledges it as stamped or rejected, which returns its credit.
//!
//! # Accounting
//!
//! The ledger keeps two amounts, each in bytes and in batches:
//!
//! - `outstanding`: credit the source holds and may spend;
//! - `in_flight`: credit spent on admitted batches not yet acknowledged. Each such batch is
//!   kept individually, keyed by its admitted [`SeqRange`] with its byte size, in admission
//!   order; there are never more of them than `window.batches`.
//!
//! Invariant after every call: `outstanding + in_flight <= window` in both units, where
//! `in_flight` is exactly the sum over the batches still held downstream. So `outstanding`
//! never exceeds the window (REQ-003) and the bytes and batches held downstream never exceed
//! it either: credit returns only for a batch that was admitted and is still in flight, and
//! only once. All arithmetic is on unsigned integers; nothing can go negative.
//!
//! - [`CreditLedger::grant`] refills `outstanding` to `window - in_flight` while
//!   [`StreamState::Active`]; it grants nothing while stopping or stopped (REQ-004).
//! - [`CreditLedger::admit`] accepts a batch only if its bytes and one batch both fit in
//!   `outstanding` and then deducts both (REQ-001); otherwise it refuses with
//!   [`CREDIT_EXCEEDED`] and reports the batch's sequence range as a [`Gap`] with reason
//!   [`GapReason::CreditExhausted`], changing nothing (REQ-002). While stopping, credit the
//!   source already holds can still be spent, so batches already sent under credit drain
//!   instead of being dropped.
//! - [`CreditLedger::ack`] names the batch by its admitted sequence range and byte size. It
//!   must match a batch in flight exactly; a duplicate acknowledgement, one for a batch never
//!   admitted (including any acknowledgement before the first admission), or one with a
//!   different byte size is refused with `credit_unknown_ack` and changes nothing. A matching
//!   acknowledgement releases that batch and, while active, returns its credit to
//!   `outstanding`, capped at `window - in_flight` (REQ-003). While stopping or stopped it
//!   releases the batch but returns nothing (REQ-004).
//! - [`CreditLedger::begin_stop`] moves `Active` to `Stopping`; [`CreditLedger::finish_stop`]
//!   moves to `Stopped` and withdraws all outstanding credit, so a stopped stream admits
//!   nothing.
//!
//! # Determinism (REQ-005, D0)
//!
//! A pure integer state machine with no clock reads, no hashing and no floating point; the
//! in-flight batches live in a `Vec` in admission order and an acknowledgement releases the
//! first exact match. The same sequence of calls gives the same states and outputs on every
//! platform.

use fid_hal_idl::stamp::{Gap, GapReason, SeqRange};
use std::fmt;

/// The error code of a refused batch (REQ-HAL.ACQ.INGEST.CREDIT-002).
pub const CREDIT_EXCEEDED: &str = "credit_exceeded";

/// Default byte window: 64 MiB (REQ-003, "64 MB" read as 64 × 2^20 bytes) [unverified until
/// tuned with `HAL.ACQ.INGEST.BENCH`].
pub const DEFAULT_WINDOW_BYTES: u64 = 64 * 1024 * 1024;

/// Default batch window: 16 batches (REQ-003) [unverified until tuned].
pub const DEFAULT_WINDOW_BATCHES: u32 = 16;

/// The configured credit cap of a stream. Both units are enforced (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CreditWindow {
    pub bytes: u64,
    pub batches: u32,
}

impl Default for CreditWindow {
    fn default() -> Self {
        CreditWindow { bytes: DEFAULT_WINDOW_BYTES, batches: DEFAULT_WINDOW_BATCHES }
    }
}

/// An amount of credit: held, in flight, granted or returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Credit {
    pub bytes: u64,
    pub batches: u32,
}

impl Credit {
    /// No credit.
    pub const ZERO: Credit = Credit { bytes: 0, batches: 0 };

    /// Whether both units are zero.
    pub fn is_zero(&self) -> bool {
        self.bytes == 0 && self.batches == 0
    }
}

/// The stream's state as the ledger sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamState {
    /// Credit is granted and returned.
    Active,
    /// Stop requested: no new credit; credit already held may still be spent.
    Stopping,
    /// Terminal: no credit held, nothing admitted.
    Stopped,
}

impl StreamState {
    /// The lowercase name, as used in traces.
    pub fn as_str(self) -> &'static str {
        match self {
            StreamState::Active => "active",
            StreamState::Stopping => "stopping",
            StreamState::Stopped => "stopped",
        }
    }
}

impl fmt::Display for StreamState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The outcome of [`CreditLedger::admit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Admit {
    /// The batch fitted; its credit is now in flight.
    Accepted,
    /// Refused with [`CREDIT_EXCEEDED`]: the gap covering the batch's sequence range, with
    /// reason [`GapReason::CreditExhausted`].
    Refused(Gap),
}

impl Admit {
    /// The error code of a refusal; `None` when accepted.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Admit::Accepted => None,
            Admit::Refused(_) => Some(CREDIT_EXCEEDED),
        }
    }
}

/// Why a ledger call was rejected outright (as opposed to a batch being refused for credit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreditError {
    /// A window with zero bytes or zero batches could never admit a batch
    /// (`credit_invalid_window`).
    InvalidWindow { window: CreditWindow },
    /// A sequence range whose first number is after its last (`credit_invalid_range`); the
    /// ledger cannot report it as a gap.
    InvalidRange { range: SeqRange },
    /// An acknowledgement that matches no batch in flight by sequence range and byte size
    /// (`credit_unknown_ack`): a duplicate, a batch never admitted, or a wrong size. Nothing
    /// changes.
    UnknownAck { range: SeqRange, bytes: u64 },
}

impl CreditError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CreditError::InvalidWindow { .. } => "credit_invalid_window",
            CreditError::InvalidRange { .. } => "credit_invalid_range",
            CreditError::UnknownAck { .. } => CREDIT_UNKNOWN_ACK,
        }
    }
}

/// The error code of an acknowledgement that matches no batch in flight.
pub const CREDIT_UNKNOWN_ACK: &str = "credit_unknown_ack";

/// A batch admitted and not yet acknowledged: its sequence range and byte size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InFlightBatch {
    pub range: SeqRange,
    pub bytes: u64,
}

impl fmt::Display for CreditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CreditError::InvalidWindow { window } => {
                write!(f, "{}: window {} bytes / {} batches", self.code(), window.bytes, window.batches)
            }
            CreditError::InvalidRange { range } => {
                write!(f, "{}: range {}..={}", self.code(), range.first, range.last)
            }
            CreditError::UnknownAck { range, bytes } => {
                write!(f, "{}: no batch {}..={} of {} bytes in flight", self.code(), range.first, range.last, bytes)
            }
        }
    }
}

impl std::error::Error for CreditError {}

/// The per-stream credit ledger. Starts `Active` with no credit; the first [`grant`] opens the
/// window.
///
/// [`grant`]: CreditLedger::grant
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CreditLedger {
    window: CreditWindow,
    outstanding: Credit,
    in_flight: Credit,
    /// The batches behind `in_flight`, in admission order; at most `window.batches` of them.
    held: Vec<InFlightBatch>,
    state: StreamState,
}

impl CreditLedger {
    /// A ledger for `window`; `credit_invalid_window` if either unit is zero.
    pub fn new(window: CreditWindow) -> Result<Self, CreditError> {
        if window.bytes == 0 || window.batches == 0 {
            return Err(CreditError::InvalidWindow { window });
        }
        Ok(CreditLedger {
            window,
            outstanding: Credit::ZERO,
            in_flight: Credit::ZERO,
            held: Vec::new(),
            state: StreamState::Active,
        })
    }

    /// The configured window.
    pub fn window(&self) -> CreditWindow {
        self.window
    }

    /// Credit the source holds and may spend.
    pub fn outstanding(&self) -> Credit {
        self.outstanding
    }

    /// Credit spent on admitted batches not yet acknowledged.
    pub fn in_flight(&self) -> Credit {
        self.in_flight
    }

    /// The batches admitted and not yet acknowledged, in admission order.
    pub fn held(&self) -> &[InFlightBatch] {
        &self.held
    }

    /// The stream state.
    pub fn state(&self) -> StreamState {
        self.state
    }

    /// The most `outstanding` may be: the window less what is in flight.
    fn cap(&self) -> Credit {
        Credit {
            bytes: self.window.bytes.saturating_sub(self.in_flight.bytes),
            batches: self.window.batches.saturating_sub(self.in_flight.batches),
        }
    }

    /// Refills outstanding credit to the window less what is in flight and returns the increase.
    /// Grants nothing (returns [`Credit::ZERO`]) while stopping or stopped (REQ-004).
    pub fn grant(&mut self) -> Credit {
        if self.state != StreamState::Active {
            return Credit::ZERO;
        }
        let cap = self.cap();
        let granted = Credit {
            bytes: cap.bytes.saturating_sub(self.outstanding.bytes),
            batches: cap.batches.saturating_sub(self.outstanding.batches),
        };
        self.outstanding = Credit {
            bytes: self.outstanding.bytes.saturating_add(granted.bytes),
            batches: self.outstanding.batches.saturating_add(granted.batches),
        };
        granted
    }

    /// Offers a batch of `batch_bytes` bytes covering sequence numbers `seq`. Accepted only if
    /// both its bytes and one batch fit in the outstanding credit, which are then moved to in
    /// flight (REQ-001); otherwise refused with [`CREDIT_EXCEEDED`] and the gap
    /// `{seq.first, seq.last, CREDIT_EXHAUSTED}`, leaving the ledger unchanged (REQ-002).
    pub fn admit(&mut self, batch_bytes: u64, seq: SeqRange) -> Result<Admit, CreditError> {
        if seq.first > seq.last {
            return Err(CreditError::InvalidRange { range: seq });
        }
        if self.outstanding.batches == 0 || self.outstanding.bytes < batch_bytes {
            return Ok(Admit::Refused(Gap { from: seq.first, to: seq.last, reason: GapReason::CreditExhausted }));
        }
        self.outstanding.bytes -= batch_bytes;
        self.outstanding.batches -= 1;
        // Cannot saturate: outstanding + in_flight <= window, and the amounts only moved.
        self.in_flight.bytes = self.in_flight.bytes.saturating_add(batch_bytes);
        self.in_flight.batches = self.in_flight.batches.saturating_add(1);
        // At most window.batches entries: each one holds a batch credit.
        self.held.push(InFlightBatch { range: seq, bytes: batch_bytes });
        Ok(Admit::Accepted)
    }

    /// Acknowledges the batch admitted as `seq` with `batch_bytes` bytes as stamped or rejected
    /// downstream. It must match a batch in flight exactly; otherwise `credit_unknown_ack` and
    /// nothing changes (a duplicate, a batch never admitted, or a different byte size). A match
    /// releases the batch and, while active, returns its credit, capped so that outstanding
    /// never exceeds the window less what is in flight (REQ-003). Returns the credit given back
    /// to the source; [`Credit::ZERO`] while stopping or stopped (REQ-004).
    pub fn ack(&mut self, batch_bytes: u64, seq: SeqRange) -> Result<Credit, CreditError> {
        let wanted = InFlightBatch { range: seq, bytes: batch_bytes };
        let Some(pos) = self.held.iter().position(|b| *b == wanted) else {
            return Err(CreditError::UnknownAck { range: seq, bytes: batch_bytes });
        };
        self.held.remove(pos);
        // Cannot saturate: in_flight is exactly the sum over `held`, which contained this batch.
        self.in_flight.bytes = self.in_flight.bytes.saturating_sub(batch_bytes);
        self.in_flight.batches = self.in_flight.batches.saturating_sub(1);
        if self.state != StreamState::Active {
            return Ok(Credit::ZERO);
        }
        let cap = self.cap();
        let bytes = self.outstanding.bytes.saturating_add(batch_bytes).min(cap.bytes);
        let batches = self.outstanding.batches.saturating_add(1).min(cap.batches);
        let returned = Credit {
            bytes: bytes.saturating_sub(self.outstanding.bytes),
            batches: batches.saturating_sub(self.outstanding.batches),
        };
        self.outstanding = Credit {
            bytes: self.outstanding.bytes.saturating_add(returned.bytes),
            batches: self.outstanding.batches.saturating_add(returned.batches),
        };
        Ok(returned)
    }

    /// Requests a stop: `Active` becomes `Stopping`. Idempotent; a stopped stream stays stopped.
    pub fn begin_stop(&mut self) {
        if self.state == StreamState::Active {
            self.state = StreamState::Stopping;
        }
    }

    /// Completes the stop: the stream becomes `Stopped` and all outstanding credit is withdrawn.
    pub fn finish_stop(&mut self) {
        self.state = StreamState::Stopped;
        self.outstanding = Credit::ZERO;
    }
}
