//! Matrix type and products (`KRN.NUM.LA.MAT`, REQ-001, REQ-002).

use super::{alloc, LaError};
use crate::err::{check_finite_slice, NumError};
use crate::red;

/// Largest number of rows or columns a [`Mat`] may have (REQ-KRN.NUM.LA.MAT-001).
pub const MAX_DIM: usize = 4096;

/// Upper bound on the heap bytes of one [`Mat`]: `MAX_DIM^2` elements of `f64` (128 MiB).
/// Every function in this module reserves its result and scratch fallibly with exact
/// capacity (no growth, no slack), and its total is at most two such matrices plus
/// `O(MAX_DIM)` vectors. A `Mat` never holds spare capacity, so [`Mat::heap_bytes`] is
/// exactly `rows * cols * size_of::<f64>()` and at most this bound.
pub const MAX_MAT_BYTES: usize = MAX_DIM * MAX_DIM * core::mem::size_of::<f64>();

/// A dense row-major `f64` matrix with `1..=MAX_DIM` rows and columns and finite entries.
/// The fields are private so that these invariants hold for every value of the type.
#[derive(Debug, Clone, PartialEq)]
pub struct Mat {
    rows: usize,
    cols: usize,
    data: Vec<f64>,
}

fn check_dims(op: &'static str, rows: usize, cols: usize) -> Result<(), NumError> {
    if rows == 0 || cols == 0 {
        return Err(LaError::BadShape.at(op));
    }
    if rows > MAX_DIM || cols > MAX_DIM {
        return Err(LaError::TooLarge.at(op));
    }
    Ok(())
}

impl Mat {
    /// Builds a matrix from row-major data. Check order: a zero dimension (`BadShape`),
    /// more than [`MAX_DIM`] rows or columns (`TooLarge`), `data.len() != rows * cols`
    /// (`BadShape`), then the first NaN or infinity by row-major index. If `data` has spare
    /// capacity it is copied into an exact-capacity buffer (`Allocation` if that fails).
    pub fn new(rows: usize, cols: usize, data: Vec<f64>) -> Result<Mat, NumError> {
        const OP: &str = "la.mat";
        check_dims(OP, rows, cols)?;
        if data.len() != rows * cols {
            return Err(LaError::BadShape.at(OP));
        }
        check_finite_slice(OP, &data)?;
        let data = if data.capacity() == data.len() {
            data
        } else {
            let mut exact = alloc(OP, data.len())?;
            exact.extend_from_slice(&data);
            exact
        };
        Ok(Mat { rows, cols, data })
    }

    /// Builds a matrix from a list of rows. Check order: no rows or an empty first row
    /// (`BadShape`), more than [`MAX_DIM`] rows or columns (`TooLarge`), a row whose length
    /// differs from the first (`BadShape`), then the first NaN or infinity by row-major index.
    pub fn from_rows<R: AsRef<[f64]>>(rows: &[R]) -> Result<Mat, NumError> {
        const OP: &str = "la.mat";
        let cols = rows.first().map_or(0, |r| r.as_ref().len());
        check_dims(OP, rows.len(), cols)?;
        if rows.iter().any(|r| r.as_ref().len() != cols) {
            return Err(LaError::BadShape.at(OP));
        }
        let mut data = alloc(OP, rows.len() * cols)?;
        for r in rows {
            data.extend_from_slice(r.as_ref());
        }
        check_finite_slice(OP, &data)?;
        Ok(Mat { rows: rows.len(), cols, data })
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// The row-major entries.
    pub fn data(&self) -> &[f64] {
        &self.data
    }

    /// Consumes the matrix and returns its row-major entries.
    pub fn into_data(self) -> Vec<f64> {
        self.data
    }

    /// Entry `(i, j)`; `BadShape` if out of range.
    pub fn get(&self, i: usize, j: usize) -> Result<f64, NumError> {
        if i >= self.rows || j >= self.cols {
            return Err(LaError::BadShape.at("la.get"));
        }
        Ok(self.data[i * self.cols + j])
    }

    /// Row `i` as a contiguous slice; `BadShape` if out of range.
    pub fn row(&self, i: usize) -> Result<&[f64], NumError> {
        if i >= self.rows {
            return Err(LaError::BadShape.at("la.row"));
        }
        Ok(self.row_unchecked(i))
    }

    /// Column `j` gathered into a new vector (ascending row index); `BadShape` if out of
    /// range.
    pub fn col(&self, j: usize) -> Result<Vec<f64>, NumError> {
        if j >= self.cols {
            return Err(LaError::BadShape.at("la.col"));
        }
        let mut v = alloc("la.col", self.rows)?;
        self.gather_col(j, &mut v);
        Ok(v)
    }

    /// The transpose (an exact copy; no arithmetic). `Allocation` if the buffer cannot be
    /// reserved.
    pub fn transpose(&self) -> Result<Mat, NumError> {
        let mut data = alloc("la.transpose", self.data.len())?;
        for j in 0..self.cols {
            for i in 0..self.rows {
                data.push(self.data[i * self.cols + j]);
            }
        }
        Ok(Mat { rows: self.cols, cols: self.rows, data })
    }

    /// Heap bytes held by the entries (`capacity * size_of::<f64>()`); the buffer has no
    /// spare capacity, so this is `rows * cols * size_of::<f64>()` <= [`MAX_MAT_BYTES`].
    pub fn heap_bytes(&self) -> usize {
        self.data.capacity() * core::mem::size_of::<f64>()
    }

    pub(crate) fn row_unchecked(&self, i: usize) -> &[f64] {
        &self.data[i * self.cols..(i + 1) * self.cols]
    }

    /// Clears `out` and fills it with column `j` (no reallocation if capacity suffices).
    pub(crate) fn gather_col(&self, j: usize, out: &mut Vec<f64>) {
        out.clear();
        for i in 0..self.rows {
            out.push(self.data[i * self.cols + j]);
        }
    }
}

/// Maps an error of [`red::dot`] on finite, equal-length operands (only an overflow is
/// possible) to an overflow of output element `k`.
fn overflow_at(op: &'static str, k: usize) -> impl FnOnce(NumError) -> NumError {
    move |_| NumError::Infinite { op, index: Some(k) }
}

/// `y = A x` with `y[i] = red::dot(A[i, ..], x)`. Errors: `x.len() != A.cols()`
/// (`BadShape`), the first NaN or infinity of `x`, then the first `i` whose dot overflows
/// (`Infinite { index: Some(i) }`).
pub fn mat_vec(a: &Mat, x: &[f64]) -> Result<Vec<f64>, NumError> {
    const OP: &str = "la.mat_vec";
    if x.len() != a.cols {
        return Err(LaError::BadShape.at(OP));
    }
    check_finite_slice(OP, x)?;
    let mut y = alloc(OP, a.rows)?;
    for i in 0..a.rows {
        y.push(red::dot(a.row_unchecked(i), x).map_err(overflow_at(OP, i))?);
    }
    Ok(y)
}

/// `y = A^T x` with `y[j] = red::dot(A[.., j], x)`, the column gathered into scratch first
/// so the dot sees ascending row order. Errors as [`mat_vec`] with `x.len() != A.rows()`.
pub fn mat_t_vec(a: &Mat, x: &[f64]) -> Result<Vec<f64>, NumError> {
    const OP: &str = "la.mat_t_vec";
    if x.len() != a.rows {
        return Err(LaError::BadShape.at(OP));
    }
    check_finite_slice(OP, x)?;
    let mut y = alloc(OP, a.cols)?;
    let mut col = alloc(OP, a.rows)?;
    for j in 0..a.cols {
        a.gather_col(j, &mut col);
        y.push(red::dot(&col, x).map_err(overflow_at(OP, j))?);
    }
    Ok(y)
}

/// `C = A B` with `C[i][j] = red::dot(A[i, ..], B[.., j])`. `B` is transposed once into
/// scratch so every column is contiguous; elements are computed in row-major order, so an
/// overflow names the lowest row-major index `i * B.cols() + j`. `A.cols() != B.rows()` is
/// `BadShape`.
pub fn mat_mul(a: &Mat, b: &Mat) -> Result<Mat, NumError> {
    const OP: &str = "la.mat_mul";
    if a.cols != b.rows {
        return Err(LaError::BadShape.at(OP));
    }
    let bt = b.transpose().map_err(|_| LaError::Allocation.at(OP))?;
    let mut data = alloc(OP, a.rows * b.cols)?;
    for i in 0..a.rows {
        let ai = a.row_unchecked(i);
        for j in 0..b.cols {
            data.push(red::dot(ai, bt.row_unchecked(j)).map_err(overflow_at(OP, i * b.cols + j))?);
        }
    }
    Ok(Mat { rows: a.rows, cols: b.cols, data })
}
