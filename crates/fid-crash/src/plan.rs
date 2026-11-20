//! Seeded kill schedule (REQ-CORE.STO.SQL.CRASH-002, -005).
//!
//! The delay before kill `k` of a run with seed `s` is a pure function of `(s, k)`: SplitMix64
//! of `s` and the counter `k`, reduced to `0..=max_delay_us`. A failure names the seed and the
//! [`KillInstant`], and re-running that one instant ([`KillPlan::instant`]) needs no replay of
//! the kills before it.

use std::fmt;

/// One kill: its number in the run (from 0) and the delay, in microseconds, between the
/// writer reporting ready (or, see [`KillInstant::from_spawn`], being spawned) and the kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KillInstant {
    pub kill: u64,
    pub delay_us: u64,
}

impl fmt::Display for KillInstant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kill, self.delay_us)
    }
}

impl KillInstant {
    /// Whether the delay is counted from the writer's spawn rather than from its `READY`
    /// line: every fourth kill (kill numbers 3, 7, 11, ...), so kills also land in the
    /// writer's start-up (store open, WAL recovery, lock takeover). A pure function of the
    /// kill number, so a printed instant reproduces with the same reference point.
    pub fn from_spawn(&self) -> bool {
        self.kill % 4 == 3
    }

    /// Parses `<kill>:<delay_us>`, the form [`KillInstant`]'s `Display` prints.
    pub fn parse(s: &str) -> Option<KillInstant> {
        let (k, d) = s.split_once(':')?;
        Some(KillInstant { kill: k.parse().ok()?, delay_us: d.parse().ok()? })
    }
}

/// The seeded, counter-based kill schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KillPlan {
    pub seed: u64,
    pub max_delay_us: u64,
}

/// SplitMix64 output for state `x` (Steele, Lea and Flood, 2014).
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl KillPlan {
    /// The instant of kill number `kill`.
    pub fn instant(&self, kill: u64) -> KillInstant {
        let r = splitmix64(splitmix64(self.seed) ^ kill);
        // saturating_add keeps the divisor at least 1.
        KillInstant { kill, delay_us: r % self.max_delay_us.saturating_add(1) }
    }
}
