//! Canonical problem form and its checked builder (REQ-KRN.SOLV.CONIC.WRAP-001).

use super::ConicError;

/// A sparse matrix in compressed sparse column form: column `j` holds the entries
/// `rowval[colptr[j]..colptr[j + 1]]` with values `nzval[..]` at the same positions. Row
/// indices are strictly increasing within each column (sorted, no duplicates).
#[derive(Debug, Clone, PartialEq)]
pub struct Csc {
    /// Number of rows.
    pub nrows: usize,
    /// Number of columns.
    pub ncols: usize,
    /// Column pointers, length `ncols + 1`, starting at 0, nondecreasing.
    pub colptr: Vec<usize>,
    /// Row index of every stored entry.
    pub rowval: Vec<usize>,
    /// Value of every stored entry.
    pub nzval: Vec<f64>,
}

impl Csc {
    /// A matrix from its parts; the builder checks them.
    pub fn new(nrows: usize, ncols: usize, colptr: Vec<usize>, rowval: Vec<usize>, nzval: Vec<f64>) -> Csc {
        Csc { nrows, ncols, colptr, rowval, nzval }
    }

    /// The `nrows x ncols` matrix with no stored entries.
    pub fn zeros(nrows: usize, ncols: usize) -> Csc {
        Csc { nrows, ncols, colptr: vec![0; ncols.saturating_add(1)], rowval: Vec::new(), nzval: Vec::new() }
    }

    /// Number of stored entries.
    pub fn nnz(&self) -> usize {
        self.rowval.len()
    }

    /// Whether the parts form a valid CSC matrix with finite values (`upper`: every entry on
    /// or above the diagonal). Returns `false` on any inconsistency; never panics.
    fn is_valid(&self, upper: bool) -> bool {
        let Some(len) = self.ncols.checked_add(1) else { return false };
        if self.colptr.len() != len || self.colptr.first() != Some(&0) {
            return false;
        }
        if self.colptr.last() != Some(&self.rowval.len()) || self.rowval.len() != self.nzval.len() {
            return false;
        }
        for (col, w) in self.colptr.windows(2).enumerate() {
            let (lo, hi) = match w {
                [lo, hi] => (*lo, *hi),
                _ => return false,
            };
            let Some(rows) = self.rowval.get(lo..hi) else { return false };
            let mut prev: Option<usize> = None;
            for &row in rows {
                if row >= self.nrows || (upper && row > col) || prev.is_some_and(|p| row <= p) {
                    return false;
                }
                prev = Some(row);
            }
        }
        true
    }

    fn all_finite(&self) -> bool {
        self.nzval.iter().all(|v| v.is_finite())
    }
}

/// One cone of the product cone `K`, with its dimension (rows of `A` it covers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cone {
    /// `s = 0` (equality rows).
    Zero(usize),
    /// `s >= 0` componentwise.
    Nonneg(usize),
    /// Second-order cone `s_0 >= ||(s_1, .., s_{d-1})||`, dimension `d >= 2`.
    Soc(usize),
}

impl Cone {
    /// Rows the cone covers.
    pub fn dim(&self) -> usize {
        match *self {
            Cone::Zero(d) | Cone::Nonneg(d) | Cone::Soc(d) => d,
        }
    }
}

/// Problem class, as checked by the entry points (REQ-KRN.SOLV.CONIC.WRAP-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemClass {
    /// Linear program: no `P`, no second-order cone (`solve_lp`).
    Lp,
    /// Convex quadratic program: no second-order cone (`solve_qp`).
    Qp,
    /// Second-order cone program: no `P` (`solve_socp`).
    Socp,
}

/// A validated problem `minimise 1/2 x'Px + q'x subject to Ax + s = b, s in K`, with `P`
/// upper triangular (optional), `A` of size `m x n` and `K` the product of `cones` in order.
/// Built only by [`ConicProblemBuilder::build`].
#[derive(Debug, Clone, PartialEq)]
pub struct ConicProblem {
    pub(crate) p: Option<Csc>,
    pub(crate) q: Vec<f64>,
    pub(crate) a: Csc,
    pub(crate) b: Vec<f64>,
    pub(crate) cones: Vec<Cone>,
}

impl ConicProblem {
    /// Number of variables `n`.
    pub fn n(&self) -> usize {
        self.q.len()
    }
    /// Number of constraint rows `m`.
    pub fn m(&self) -> usize {
        self.b.len()
    }
    /// The upper triangle of `P`, if present.
    pub fn p(&self) -> Option<&Csc> {
        self.p.as_ref()
    }
    /// The linear cost `q`.
    pub fn q(&self) -> &[f64] {
        &self.q
    }
    /// The constraint matrix `A`.
    pub fn a(&self) -> &Csc {
        &self.a
    }
    /// The right-hand side `b`.
    pub fn b(&self) -> &[f64] {
        &self.b
    }
    /// The cones of `K`, in order.
    pub fn cones(&self) -> &[Cone] {
        &self.cones
    }
    /// Whether any cone is a second-order cone.
    pub fn has_soc(&self) -> bool {
        self.cones.iter().any(|c| matches!(c, Cone::Soc(_)))
    }
}

/// Checked builder of a [`ConicProblem`] (REQ-KRN.SOLV.CONIC.WRAP-001).
#[derive(Debug, Clone)]
pub struct ConicProblemBuilder {
    p: Option<Csc>,
    q: Vec<f64>,
    a: Csc,
    b: Vec<f64>,
    cones: Vec<Cone>,
}

impl ConicProblemBuilder {
    /// A problem without a quadratic term.
    pub fn new(q: Vec<f64>, a: Csc, b: Vec<f64>, cones: Vec<Cone>) -> ConicProblemBuilder {
        ConicProblemBuilder { p: None, q, a, b, cones }
    }

    /// Sets the upper triangle of `P` (entries on or above the diagonal only).
    pub fn p(mut self, p: Csc) -> ConicProblemBuilder {
        self.p = Some(p);
        self
    }

    /// Validates the problem. Fields are checked in the order `q`, `A`, `b`, `cones`, `P`;
    /// the first failing one is named in [`ConicError::InvalidProblem`]:
    /// - `q`: empty, or a non-finite entry;
    /// - `A`: not a valid CSC matrix (pointers, lengths, row range, unsorted or duplicate
    ///   rows), column count other than `q`'s length, or a non-finite entry;
    /// - `b`: length other than the row count of `A`, or a non-finite entry;
    /// - `cones`: a cone of dimension 0, a second-order cone of dimension below 2, or
    ///   dimensions that do not sum to the row count of `A`;
    /// - `P`: not a valid CSC matrix, not `n x n`, an entry below the diagonal, or a
    ///   non-finite entry.
    pub fn build(self) -> Result<ConicProblem, ConicError> {
        let ConicProblemBuilder { p, q, a, b, cones } = self;
        let invalid = |field: &'static str| ConicError::InvalidProblem { field };
        let n = q.len();
        if n == 0 || !q.iter().all(|v| v.is_finite()) {
            return Err(invalid("q"));
        }
        if !a.is_valid(false) || a.ncols != n || !a.all_finite() {
            return Err(invalid("A"));
        }
        if b.len() != a.nrows || !b.iter().all(|v| v.is_finite()) {
            return Err(invalid("b"));
        }
        let mut rows = 0usize;
        for c in &cones {
            let d = c.dim();
            let too_small = match c {
                Cone::Soc(_) => d < 2,
                Cone::Zero(_) | Cone::Nonneg(_) => d == 0,
            };
            if too_small {
                return Err(invalid("cones"));
            }
            rows = rows.checked_add(d).ok_or(invalid("cones"))?;
        }
        if rows != a.nrows {
            return Err(invalid("cones"));
        }
        if let Some(p) = &p {
            if !p.is_valid(true) || p.nrows != n || p.ncols != n || !p.all_finite() {
                return Err(invalid("P"));
            }
        }
        Ok(ConicProblem { p, q, a, b, cones })
    }
}
