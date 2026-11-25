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
//! - `in_flight`: credit spent on admitted batches not yet acknowledged.
//!
//! Invariant after every call: `outstanding + in_flight <= window` in both units, so
//! `outstanding` never exceeds the window (REQ-003) and nothing downstream ever holds more than
//! the window. All arithmetic is on unsigned integers, saturating where it could overflow and
//! capped by the window; nothing can go negative.
//!
//! - [`CreditLedger::grant`] refills `outstanding` to `window - in_flight` while
//!   [`StreamState::Active`]; it grants nothing while stopping or stopped (REQ-004).
//! - [`CreditLedger::admit`] accepts a batch only if its bytes and one batch both fit in
//!   `outstanding` and then deducts both (REQ-001); otherwise it refuses with
//!   [`CREDIT_EXCEEDED`] and reports the batch's sequence range as a [`Gap`] with reason
//!   [`GapReason::CreditExhausted`], changing nothing (REQ-002). While stopping, credit the
//!   source already holds can still be spent, so batches already sent under credit drain
//!   instead of being dropped.
//! - [`CreditLedger::ack`] releases one batch from `in_flight` (floored at zero, so a duplicate
//!   or spurious acknowledgement cannot underflow) and, while active, returns its credit to
//!   `outstanding`, capped at `window - in_flight` (REQ-003). While stopping or stopped an
//!   acknowledgement returns nothing (REQ-004).
//! - [`CreditLedger::begin_stop`] moves `Active` to `Stopping`; [`CreditLedger::finish_stop`]
//!   moves to `Stopped` and withdraws all outstanding credit, so a stopped stream admits
//!   nothing.
//!
//! # Determinism (REQ-005, D0)
//!
//! A pure integer state machine with no clock reads, no allocation and no floating point: the
//! same sequence of calls gives the same states and outputs on every platform.

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
}

impl CreditError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CreditError::InvalidWindow { .. } => "credit_invalid_window",
            CreditError::InvalidRange { .. } => "credit_invalid_range",
        }
    }
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
    state: StreamState,
}

impl CreditLedger {
    /// A ledger for `window`; `credit_invalid_window` if either unit is zero.
    pub fn new(window: CreditWindow) -> Result<Self, CreditError> {
        if window.bytes == 0 || window.batches == 0 {
            return Err(CreditError::InvalidWindow { window });
        }
        Ok(CreditLedger { window, outstanding: Credit::ZERO, in_flight: Credit::ZERO, state: StreamState::Active })
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
        Ok(Admit::Accepted)
    }

    /// Acknowledges one batch of `batch_bytes` bytes as stamped or rejected downstream. Releases
    /// it from in flight (floored at zero) and, while active, returns its credit, capped so
    /// that outstanding never exceeds the window less what is in flight (REQ-003). Returns the
    /// credit actually given back to the source; [`Credit::ZERO`] while stopping or stopped
    /// (REQ-004).
    pub fn ack(&mut self, batch_bytes: u64) -> Credit {
        self.in_flight.bytes = self.in_flight.bytes.saturating_sub(batch_bytes);
        self.in_flight.batches = self.in_flight.batches.saturating_sub(1);
        if self.state != StreamState::Active {
            return Credit::ZERO;
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
        returned
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
