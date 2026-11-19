//! Frame-time statistics: nearest-rank 95th percentile and the 33 ms pass criterion
//! (REQ-VIZ.SHELL.SPKTREE-002, -003).

/// Pass threshold: every measured p95 must be strictly under this, in milliseconds.
pub const THRESHOLD_MS: f64 = 33.0;
/// Largest number of samples accepted in one series.
pub const MAX_SAMPLES: usize = 2_000_000;

/// Why a series cannot be summarised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatsError {
    /// No samples.
    Empty,
    /// More than [`MAX_SAMPLES`] samples.
    TooMany,
    /// A sample is NaN, infinite or negative (0-based index).
    BadSample(usize),
}

/// Summary of one series of frame times in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    /// Number of samples.
    pub n: usize,
    /// Smallest sample.
    pub min: f64,
    /// Nearest-rank median (rank ceil(n / 2)).
    pub median: f64,
    /// Nearest-rank 95th percentile (rank ceil(0.95 n)).
    pub p95: f64,
    /// Largest sample.
    pub max: f64,
}

impl Summary {
    /// Whether the p95 is strictly under [`THRESHOLD_MS`].
    pub fn passes(&self) -> bool {
        self.p95 < THRESHOLD_MS
    }
}

/// Summarises a series. Samples must be finite and non-negative.
pub fn summarise(samples: &[f64]) -> Result<Summary, StatsError> {
    if samples.is_empty() {
        return Err(StatsError::Empty);
    }
    if samples.len() > MAX_SAMPLES {
        return Err(StatsError::TooMany);
    }
    if let Some(i) = samples.iter().position(|s| !s.is_finite() || *s < 0.0) {
        return Err(StatsError::BadSample(i));
    }
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let at = |rank: usize| v.get(rank.saturating_sub(1)).copied().unwrap_or(0.0);
    Ok(Summary {
        n,
        min: at(1),
        median: at(n.div_ceil(2)),
        p95: at((95 * n).div_ceil(100)),
        max: at(n),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_hand_labelled() {
        let one_to_100: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = summarise(&one_to_100).unwrap();
        assert_eq!((s.n, s.min, s.median, s.p95, s.max), (100, 1.0, 50.0, 95.0, 100.0));
        // n = 20: rank ceil(19) = 19.
        let twenty: Vec<f64> = (1..=20).rev().map(f64::from).collect();
        assert_eq!(summarise(&twenty).unwrap().p95, 19.0);
        // n = 21: rank ceil(19.95) = 20.
        let s = summarise(&(1..=21).map(f64::from).collect::<Vec<_>>()).unwrap();
        assert_eq!((s.median, s.p95), (11.0, 20.0));
        assert_eq!(summarise(&[7.5]).unwrap().p95, 7.5);
    }

    #[test]
    fn threshold_is_strict() {
        assert!(summarise(&[32.999]).unwrap().passes());
        assert!(!summarise(&[33.0]).unwrap().passes());
    }

    #[test]
    fn rejects_bad_series() {
        assert_eq!(summarise(&[]), Err(StatsError::Empty));
        assert_eq!(summarise(&[1.0, f64::NAN]), Err(StatsError::BadSample(1)));
        assert_eq!(summarise(&[f64::INFINITY]), Err(StatsError::BadSample(0)));
        assert_eq!(summarise(&[-0.5]), Err(StatsError::BadSample(0)));
    }
}
