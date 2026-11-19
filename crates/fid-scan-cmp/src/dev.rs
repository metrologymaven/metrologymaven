//! Deviation method and per-point result types (spec node `SCAN.CMP.DEV.PRIM`, API part
//! only). The primitive itself (`deviation`, `deviation_batch`) is not built yet; the
//! builder of [`crate::field`] consumes [`DevResult`].

use core::fmt;

/// How a deviation is measured (REQ-SCAN.CMP.DEV-001). Its wire name is recorded in the
/// field provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Method {
    /// `(a - f) . n_f` at the closest-point foot `f` (SCAN.CMP.DEV.PRIM).
    Normal,
    /// Signed Euclidean distance to the closest point.
    Shortest,
}

impl Method {
    /// Every method, in declaration order.
    pub const ALL: [Method; 2] = [Method::Normal, Method::Shortest];

    /// The stable wire name: `normal` or `shortest`.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Normal => "normal",
            Method::Shortest => "shortest",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a point has no value (REQ-SCAN.CMP.DEV-002). The field stores only the status
/// `NO_MATCH`, not the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoMatchReason {
    /// No nominal point within the maximum search distance.
    BeyondDistance,
    /// The actual normal is further than the angle cutoff from the nominal normal.
    Angle,
    /// Method `normal` and the foot point lies on an open boundary of the nominal.
    OffEdge,
}

/// The result of the deviation primitive for one actual point: a signed deviation in
/// metres (positive on the material-outside side of the nominal), no match, or excluded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DevResult {
    /// A value of record in metres. Must be finite (`scan.cmp.non_finite` otherwise).
    Valid(f64),
    /// No value (REQ-SCAN.CMP.DEV-002).
    NoMatch(NoMatchReason),
    /// Not compared, for example a vertex used only by synthesized triangles
    /// (REQ-SCAN.CMP.DEV-004).
    Excluded,
}
