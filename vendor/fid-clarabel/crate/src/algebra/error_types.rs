// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
/// Error type returned by matrix concatenation operations.
#[derive(Debug)]
pub enum MatrixConcatenationError {
    /// Indicates inputs have incompatible dimension
    IncompatibleDimension,
}

impl std::fmt::Display for MatrixConcatenationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatrixConcatenationError::IncompatibleDimension => write!(f, "Incompatible dimensions"),
        }
    }
}

impl std::error::Error for MatrixConcatenationError {}

#[derive(Debug)]
/// Error type returned by sparse matrix assembly operations.
pub enum SparseFormatError {
    /// Matrix dimension fields and/or array lengths are incompatible
    IncompatibleDimension,
    /// Data is not sorted by row index within each column
    BadRowOrdering,
    /// Row value exceeds the matrix row dimension
    BadRowval,
    /// Matrix column pointer values are defective
    BadColptr,
    /// Operation on matrices that have mismatching sparsity patterns
    SparsityMismatch,
}

impl std::fmt::Display for SparseFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SparseFormatError::IncompatibleDimension => write!(
                f,
                "Matrix dimension fields and/or array lengths are incompatible"
            ),
            SparseFormatError::BadRowOrdering => {
                write!(f, "Data is not sorted by row index within each column")
            }
            SparseFormatError::BadRowval => write!(f, "Row value exceeds the matrix row dimension"),
            SparseFormatError::BadColptr => write!(f, "Bad column pointer values"),
            SparseFormatError::SparsityMismatch => write!(f, "sparsity pattern mismatch"),
        }
    }
}

impl std::error::Error for SparseFormatError {}

/// Error type returned by BLAS-like dense factorization routines.  Errors
/// return the internal BLAS error codes.
#[allow(clippy::upper_case_acronyms)]
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum DenseFactorizationError {
    IncompatibleDimension,
    Eigen(i32),
    SVD(i32),
    Cholesky(i32),
    LU(i32),
}

impl std::fmt::Display for DenseFactorizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DenseFactorizationError::IncompatibleDimension => write!(
                f,
                "Matrix dimension fields and/or array lengths are incompatible"
            ),
            DenseFactorizationError::Eigen(_) => write!(f, "Eigendecomposition error"),
            DenseFactorizationError::SVD(_) => write!(f, "SVD error"),
            DenseFactorizationError::Cholesky(_) => write!(f, "Cholesky error"),
            DenseFactorizationError::LU(_) => write!(f, "LU error"),
        }
    }
}

impl std::error::Error for DenseFactorizationError {}
