// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
#![allow(non_snake_case)]
use crate::algebra::*;
use core::cmp::{max, min};
use std::collections::BTreeSet;
use std::iter::zip;

/// Error codes returnable from [`QDLDLFactorisation`](QDLDLFactorisation) factor operations
#[derive(Debug)]
pub enum QDLDLError {
    /// Matrix dimension fields are incompatible
    IncompatibleDimension,
    /// Matrix has a zero column
    EmptyColumn,
    /// Matrix is not upper triangular
    NotUpperTriangular,
    /// Matrix factorization produced a zero pivot
    ZeroPivot,
    /// Invalid permutation vector supplied
    InvalidPermutation,
    /// Matrix exceeds [`MAX_ORDERING_DIM`], [`MAX_ORDERING_NNZ`] or [`MAX_ORDERING_FILL`]
    OrderingTooLarge,
}

impl std::fmt::Display for QDLDLError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QDLDLError::IncompatibleDimension => {
                write!(f, "Matrix dimension fields are incompatible")
            }
            QDLDLError::EmptyColumn => write!(f, "Matrix has a zero column"),
            QDLDLError::NotUpperTriangular => write!(f, "Matrix is not upper triangular"),
            QDLDLError::ZeroPivot => write!(f, "Matrix factorization produced a zero pivot"),
            QDLDLError::InvalidPermutation => write!(f, "Invalid permutation vector"),
            QDLDLError::OrderingTooLarge => {
                write!(
                    f,
                    "Matrix exceeds the size budget of the minimum-degree ordering"
                )
            }
        }
    }
}

impl std::error::Error for QDLDLError {}

#[derive(Debug, Clone)]
#[allow(missing_docs)]
/// Required settings for [`QDLDLFactorisation`](QDLDLFactorisation)
pub struct QDLDLSettings {
    /// optional user-supplied custom permutation vector for the matrix
    pub perm: Option<Vec<usize>>,

    /// Logical factorisation only, no numerical factorisation
    pub logical: bool,

    /// optional user-supplied signs of the diagonal elements of D in LDL^T
    pub Dsigns: Option<Vec<i8>>,

    /// Enable regularization during factorisation
    pub regularize_enable: bool,

    /// Regularization epsilon parameter
    pub regularize_eps: T,

    /// Regularization delta parameter
    pub regularize_delta: T,
}

impl Default for QDLDLSettings {
    fn default() -> QDLDLSettings {
        // (KNP-1, patch 0007: hand-written defaults; upstream used derive_builder)
        QDLDLSettings {
            perm: None,
            logical: false,
            Dsigns: None,
            regularize_enable: true,
            regularize_eps: (1e-12).as_T(),
            regularize_delta: (1e-7).as_T(),
        }
    }
}

/// Performs $LDL^T$ factorization of a symmetric quasidefinite matrix
#[derive(Debug)]
pub struct QDLDLFactorisation {
    /// permutation vector
    pub perm: Vec<usize>,
    /// inverse permutation
    #[allow(dead_code)] //Unused because we call ipermute in solve instead.  Keep anyway.
    iperm: Vec<usize>,
    /// lower triangular factor L in LDL^T
    pub L: CscMatrix,
    /// vector of diagonal elements of D in LDL^T
    pub D: Vec<T>,
    /// vector of reciprocal diagonal elements of D in LDL^T
    pub Dinv: Vec<T>,
    /// internal workspace data
    workspace: QDLDLWorkspace,
    /// true if factorisation is symbolic only
    is_symbolic: bool,
}

impl QDLDLFactorisation {
    /// create a new LDL^T factorisation
    pub fn new(
        Ain: &CscMatrix,
        opts: Option<QDLDLSettings>,
    ) -> Result<QDLDLFactorisation, QDLDLError> {
        //sanity check on structure
        check_structure(Ain)?;
        _qdldl_new(Ain, opts)
    }

    /// returns the number of positive eigenvalues
    pub fn positive_inertia(&self) -> usize {
        self.workspace.positive_inertia
    }
    /// returns the number of regularisation shifts
    /// that were applied during factorisation
    pub fn regularize_count(&self) -> usize {
        self.workspace.regularize_count
    }

    /// Solves Ax = b using LDL factors for A.
    /// Solves in place (x replaces b). If the factorisation is logical only or `b` has
    /// the wrong length, `b` is filled with NaN (upstream panicked; patch 0003).
    pub fn solve(&mut self, b: &mut [T]) {
        if self.is_symbolic || b.len() != self.D.len() {
            b.fill(T::nan());
            return;
        }

        // permute b
        let tmp = &mut self.workspace.fwork;
        permute(tmp, b, &self.perm);

        //solve in place with tmp as permuted RHS
        _solve(
            &self.L.colptr,
            &self.L.rowval,
            &self.L.nzval,
            &self.Dinv,
            tmp,
        );

        // inverse permutation to put unpermuted soln in b
        ipermute(b, tmp, &self.perm);
    }

    /// Update a subset of the values of the matrix to be (re)factored.  See [`refactor`](crate::qdldl::QDLDLFactorisation::refactor)
    ///
    pub fn update_values(&mut self, indices: &[usize], values: &[T]) {
        let nzval = &mut self.workspace.triuA.nzval; // post perm internal data
        let AtoPAPt = &self.workspace.AtoPAPt; //mapping from input matrix entries to triuA

        for (i, &idx) in indices.iter().enumerate() {
            nzval[AtoPAPt[idx]] = values[i];
        }
    }

    /// Update a subset of the values of the matrix to be (re)factored.  See [`refactor`](crate::qdldl::QDLDLFactorisation::refactor)
    ///
    pub fn scale_values(&mut self, indices: &[usize], scale: T) {
        let nzval = &mut self.workspace.triuA.nzval; // post perm internal data
        let AtoPAPt = &self.workspace.AtoPAPt; //mapping from input matrix entries to triuA

        for &idx in indices.iter() {
            nzval[AtoPAPt[idx]] *= scale;
        }
    }

    /// Shifts a subset of the values of the matrix to be (re)factored.   The
    /// values are offset by `offset`, with `signs` a vector of +/- 1 values
    /// indicating the direction of shifts. See [`refactor`](crate::qdldl::QDLDLFactorisation::refactor)
    ///
    /// Pairs `indices` and `signs` up to the shorter of the two (upstream asserted
    /// equal lengths; patch 0003).
    pub fn offset_values(&mut self, indices: &[usize], offset: T, signs: &[i8]) {
        let nzval = &mut self.workspace.triuA.nzval; // post perm internal data
        let AtoPAPt = &self.workspace.AtoPAPt; //mapping from input matrix entries to triuA

        for (&idx, &sign) in zip(indices, signs) {
            match sign.signum() {
                1 => {
                    nzval[AtoPAPt[idx]] += offset;
                }
                -1 => {
                    nzval[AtoPAPt[idx]] -= offset;
                }
                _ => {}
            }
        }
    }

    /// Refactor a matrix after its data has been modified.   See [`update_values`](crate::qdldl::QDLDLFactorisation::update_values),
    /// [`scale_values`](crate::qdldl::QDLDLFactorisation::scale_values) and [`offset_values`](crate::qdldl::QDLDLFactorisation::offset_values)
    ///
    pub fn refactor(&mut self) -> Result<(), QDLDLError> {
        // It never makes sense to call refactor for a logical
        // factorization since it will always be the same.  Calling
        // this function implies that we want a numerical factorization
        self.is_symbolic = false;
        _factor(
            &mut self.L,
            &mut self.D,
            &mut self.Dinv,
            &mut self.workspace,
            self.is_symbolic,
        )
    }

    /// Returns the number of nonzeros in A for A = LDL^T
    pub fn nnzA(&self) -> usize {
        self.workspace.triuA.nnz()
    }

    /// Returns the number of nonzeros in L for A = LDL^T
    pub fn nnzL(&self) -> usize {
        self.L.nnz()
    }
}

fn check_structure(A: &CscMatrix) -> Result<(), QDLDLError> {
    if !A.is_square() {
        return Err(QDLDLError::IncompatibleDimension);
    }

    if !A.is_triu() {
        return Err(QDLDLError::NotUpperTriangular);
    }

    //Error if A doesn't have at least one entry in every column
    if !A.colptr.windows(2).all(|c| c[0] < c[1]) {
        return Err(QDLDLError::EmptyColumn);
    }

    Ok(())
}

fn _qdldl_new(
    Ain: &CscMatrix,
    opts: Option<QDLDLSettings>,
) -> Result<QDLDLFactorisation, QDLDLError> {
    let n = Ain.nrows();

    //get default values if no options passed at all
    let opts = opts.unwrap_or_default();

    //Use the minimum-degree ordering of the sparsity pattern (KNP-1, patch 0006;
    //upstream used AMD from the amd crate) if a user-provided ordering
    //is not supplied.   For no ordering at all, the
    //user would need to pass (0..n).collect() explicitly
    let perm = match opts.perm {
        Some(p) => p,
        None => min_degree_ordering(Ain)?,
    };
    let iperm = _invperm(&perm)?;

    //permute to (another) upper triangular matrix and store the
    //index mapping the input's entries to the permutation's entries
    let (A, AtoPAPt) = permute_symmetric(Ain, &iperm);

    // handle the (possibly permuted) vector of
    // diagonal D signs if one was specified.  Otherwise
    // otherwise all signs are positive
    let mut Dsigns = vec![1_i8; n];
    if let Some(ds) = opts.Dsigns {
        Dsigns = vec![1_i8; n];
        permute(&mut Dsigns, &ds, &perm);
    }

    // allocate workspace
    let mut workspace = QDLDLWorkspace::new(
        A,
        AtoPAPt,
        Dsigns,
        opts.regularize_enable,
        opts.regularize_eps,
        opts.regularize_delta,
    )?;

    //total nonzeros in factorization
    let sumLnz = workspace.Lnz.iter().sum();

    // allocate space for the L matrix row indices and data
    let mut L = CscMatrix::spalloc((n, n), sumLnz);

    // allocate for D and D inverse in LDL^T
    let mut D = vec![T::zero(); n];
    let mut Dinv = vec![T::zero(); n];

    // factor the matrix into A = LDL^T
    _factor(&mut L, &mut D, &mut Dinv, &mut workspace, opts.logical)?;

    Ok(QDLDLFactorisation {
        perm,
        iperm,
        L,
        D,
        Dinv,
        workspace,
        is_symbolic: opts.logical,
    })
}

#[derive(Debug)]
struct QDLDLWorkspace {
    // internal workspace data
    etree: Vec<usize>,
    Lnz: Vec<usize>,
    iwork: Vec<usize>,
    bwork: Vec<bool>,
    fwork: Vec<T>,

    // number of positive values in D
    positive_inertia: usize,

    // The upper triangular matrix factorisation target
    // This is the post ordering PAPt of the original data
    triuA: CscMatrix,

    // mapping from entries in the triu form
    // of the original input to the post ordering
    // triu form used for the factorization
    // this can be used when modifying entries
    // of the data matrix for refactoring
    AtoPAPt: Vec<usize>,

    //regularization signs and parameters
    Dsigns: Vec<i8>,
    regularize_enable: bool,
    regularize_eps: T,
    regularize_delta: T,

    // number of regularized entries in D
    regularize_count: usize,
}

impl QDLDLWorkspace {
    pub fn new(
        triuA: CscMatrix,
        AtoPAPt: Vec<usize>,
        Dsigns: Vec<i8>,
        regularize_enable: bool,
        regularize_eps: T,
        regularize_delta: T,
    ) -> Result<Self, QDLDLError> {
        let mut etree = vec![0; triuA.ncols()];
        let mut Lnz = vec![0; triuA.ncols()]; //nonzeros in each L column
        let mut iwork = vec![0; triuA.ncols() * 3];
        let bwork = vec![false; triuA.ncols()];
        let fwork = vec![T::zero(); triuA.ncols()];

        // compute elimination tree using QDLDL converted code
        _etree(
            triuA.nrows(),
            &triuA.colptr,
            &triuA.rowval,
            &mut iwork,
            &mut Lnz,
            &mut etree,
        )?;

        // positive inertia count.
        let positive_inertia = 0;

        // number of regularized entries in D. None to start
        let regularize_count = 0;

        Ok(Self {
            etree,
            Lnz,
            iwork,
            bwork,
            fwork,
            positive_inertia,
            triuA,
            AtoPAPt,
            Dsigns,
            regularize_enable,
            regularize_eps,
            regularize_delta,
            regularize_count,
        })
    }
}

fn _factor(
    L: &mut CscMatrix,
    D: &mut [T],
    Dinv: &mut [T],
    workspace: &mut QDLDLWorkspace,
    logical: bool,
) -> Result<(), QDLDLError> {
    if logical {
        L.nzval.fill(T::one());
        D.fill(T::one());
        Dinv.fill(T::one());
    }

    // factor using QDLDL C style converted code
    let A = &workspace.triuA;

    let pos_d_count = _factor_inner(
        A.n,
        &A.colptr,
        &A.rowval,
        &A.nzval,
        &mut L.colptr,
        &mut L.rowval,
        &mut L.nzval,
        D,
        Dinv,
        &workspace.Lnz,
        &workspace.etree,
        &mut workspace.bwork,
        &mut workspace.iwork,
        &mut workspace.fwork,
        logical,
        &workspace.Dsigns,
        workspace.regularize_enable,
        workspace.regularize_eps,
        workspace.regularize_delta,
        &mut workspace.regularize_count,
    )?;

    workspace.positive_inertia = pos_d_count;

    Ok(())
}

const QDLDL_UNKNOWN: usize = usize::MAX;
const QDLDL_USED: bool = true;
const QDLDL_UNUSED: bool = false;

// Compute the elimination tree for a quasidefinite matrix
// in compressed sparse column form.

fn _etree(
    n: usize,
    Ap: &[usize],
    Ai: &[usize],
    work: &mut [usize],
    Lnz: &mut [usize],
    etree: &mut [usize],
) -> Result<usize, QDLDLError> {
    // zero out Lnz and work.  Set all etree values to unknown
    work.fill(0);
    Lnz.fill(0);
    etree.fill(QDLDL_UNKNOWN);

    // compute the elimination tree
    for j in 0..n {
        work[j] = j;
        for istart in Ai.iter().take(Ap[j + 1]).skip(Ap[j]) {
            let mut i = *istart;

            while work[i] != j {
                if etree[i] == QDLDL_UNKNOWN {
                    etree[i] = j;
                }
                Lnz[i] += 1; // nonzeros in this column
                work[i] = j;
                i = etree[i];
            }
        }
    }

    Ok(0)
}

//allow too_many_arguments since this follows the implementation
//of the C version of QDLDL.
#[allow(clippy::too_many_arguments)]
fn _factor_inner(
    n: usize,
    Ap: &[usize],
    Ai: &[usize],
    Ax: &[T],
    Lp: &mut [usize],
    Li: &mut [usize],
    Lx: &mut [T],
    D: &mut [T],
    Dinv: &mut [T],
    Lnz: &[usize],
    etree: &[usize],
    bwork: &mut [bool],
    iwork: &mut [usize],
    fwork: &mut [T],
    logical_factor: bool,
    Dsigns: &[i8],
    regularize_enable: bool,
    regularize_eps: T,
    regularize_delta: T,
    regularize_count: &mut usize,
) -> Result<usize, QDLDLError> {
    *regularize_count = 0;
    let mut positiveValuesInD = 0;

    // partition working memory into pieces
    let y_markers = bwork;
    let (y_idx, iwork) = iwork.split_at_mut(n);
    let (elim_buffer, next_colspace) = iwork.split_at_mut(n);
    let y_vals = fwork;

    //set Lp to cumsum(Lnz), starting from zero
    Lp[0] = 0;
    let mut acc = 0;
    for (Lp, Lnz) in zip(&mut Lp[1..], Lnz) {
        acc += Lnz;
        *Lp = acc;
    }

    //  set all y_idx to be 'unused' initially
    // in each column of L, the next available space
    // to start is just the first space in the column
    y_markers.fill(QDLDL_UNUSED);
    y_vals.fill(T::zero());
    D.fill(T::zero());
    next_colspace.copy_from_slice(&Lp[0..Lp.len() - 1]);

    if !logical_factor {
        // First element of the diagonal D.
        D[0] = Ax[0];
        if regularize_enable {
            let sign = T::from(Dsigns[0]);
            if D[0] * sign < regularize_eps {
                D[0] = regularize_delta * sign;
                *regularize_count += 1;
            }
        }

        if D[0].is_zero() {
            return Err(QDLDLError::ZeroPivot);
        }
        if D[0] > T::zero() {
            positiveValuesInD += 1;
        }
        Dinv[0] = T::recip(D[0]);
    }

    // Start from second row (k=1) here. The upper LH corner is trivially 0
    // in L b/c we are only computing the subdiagonal elements
    for k in 1..n {
        // NB : For each k, we compute a solution to
        // y = L(0:(k-1),0:k-1))\b, where b is the kth
        // column of A that sits above the diagonal.
        // The solution y is then the kth row of L,
        // with an implied '1' at the diagonal entry.

        // number of nonzeros in this row of L
        let mut nnz_y = 0; // number of elements in this row

        // This loop determines where nonzeros
        // will go in the kth row of L, but doesn't
        // compute the actual values

        for i in Ap[k]..Ap[k + 1] {
            let bidx = Ai[i]; //we are working on this element of b

            // Initialize D[k] as the element of this column
            // corresponding to the diagonal place.  Don't use
            // this element as part of the elimination step
            // that computes the k^th row of L
            if bidx == k {
                D[k] = Ax[i];
                continue;
            }

            y_vals[bidx] = Ax[i]; // initialise y(bidx) = b(bidx)

            // use the forward elimination tree to figure
            // out which elements must be eliminated after
            // this element of b
            let next_idx = bidx;

            if y_markers[next_idx] == QDLDL_UNUSED {
                //this y term not already visited

                y_markers[next_idx] = QDLDL_USED; //I touched this one
                elim_buffer[0] = next_idx; // It goes at the start of the current list
                let mut nnz_e = 1; //length of unvisited elimination path from here

                let mut next_idx = etree[bidx];

                while next_idx != QDLDL_UNKNOWN && next_idx < k {
                    if y_markers[next_idx] == QDLDL_USED {
                        break;
                    }

                    y_markers[next_idx] = QDLDL_USED; // I touched this one
                    elim_buffer[nnz_e] = next_idx; // It goes in the current list
                    next_idx = etree[next_idx]; // one step further along tree
                    nnz_e += 1; // the list is one longer than before
                }

                // now put the buffered elimination list into
                // my current ordering in reverse order
                while nnz_e != 0 {
                    nnz_e -= 1;
                    y_idx[nnz_y] = elim_buffer[nnz_e];
                    nnz_y += 1;
                }
            }
        }

        // This for loop places nonzeros values in the k^th row
        for i in (0..nnz_y).rev() {
            // which column are we working on?
            let cidx = y_idx[i];

            // loop along the elements in this
            // column of L and subtract to solve to y
            let tmp_idx = next_colspace[cidx];

            // don't compute Lx for logical factorisation
            // this logic is not implemented in the C version
            if !logical_factor {
                let y_vals_cidx = y_vals[cidx];

                let (f, l) = (Lp[cidx], tmp_idx);
                // (KNP-1, patch 0006: bounds-checked indexing; upstream used unsafe
                // get_unchecked here)
                for (&Lxj, &Lij) in zip(&Lx[f..l], &Li[f..l]) {
                    y_vals[Lij] -= Lxj * y_vals_cidx;
                }

                // Now I have the cidx^th element of y = L\b.
                // so compute the corresponding element of
                // this row of L and put it into the right place
                let Lx_tmp_idx = y_vals_cidx * Dinv[cidx];
                Lx[tmp_idx] = Lx_tmp_idx;
                D[k] -= y_vals_cidx * Lx_tmp_idx;
            }

            // record which row it went into
            Li[tmp_idx] = k;
            next_colspace[cidx] += 1;

            // reset the y_vals and indices back to zero and QDLDL_UNUSED
            // once I'm done with them
            y_vals[cidx] = T::zero();
            y_markers[cidx] = QDLDL_UNUSED;
        }

        if !logical_factor {
            // apply dynamic regularization
            if regularize_enable {
                let sign = T::from(Dsigns[k]);
                if D[k] * sign < regularize_eps {
                    D[k] = regularize_delta * sign;
                    *regularize_count += 1;
                }
            }

            // Maintain a count of the positive entries
            // in D.  If we hit a zero, we can't factor
            // this matrix, so abort
            if D[k].is_zero() {
                return Err(QDLDLError::ZeroPivot);
            }
            if D[k] > T::zero() {
                positiveValuesInD += 1;
            }

            // compute the inverse of the diagonal
            Dinv[k] = T::recip(D[k]);
        }
    } //end for k

    Ok(positiveValuesInD)
}

// Solves (L+I)x = b, with x replacing b (with standard bounds checks)
fn _lsolve_safe(Lp: &[usize], Li: &[usize], Lx: &[T], x: &mut [T]) {
    for i in 0..x.len() {
        let xi = x[i];
        let (f, l) = (Lp[i], Lp[i + 1]);
        let Lx = &Lx[f..l];
        let Li = &Li[f..l];
        for (&Lij, &Lxj) in zip(Li, Lx) {
            x[Lij] -= Lxj * xi;
        }
    }
}

// Solves (L+I)'x = b, with x replacing b (with standard bounds checks)
fn _ltsolve_safe(Lp: &[usize], Li: &[usize], Lx: &[T], x: &mut [T]) {
    for i in (0..x.len()).rev() {
        let mut s = T::zero();
        let (f, l) = (Lp[i], Lp[i + 1]);
        let Lx = &Lx[f..l];
        let Li = &Li[f..l];
        for (&Lij, &Lxj) in zip(Li, Lx) {
            s += Lxj * x[Lij];
        }
        x[i] -= s;
    }
}

// Solves D(L+I)'x = b, with x replacing b (with standard bounds checks)
fn _dltsolve_safe(Lp: &[usize], Li: &[usize], Lx: &[T], Dinv: &[T], x: &mut [T]) {
    for i in (0..x.len()).rev() {
        let mut s = T::zero();
        let (f, l) = (Lp[i], Lp[i + 1]);
        let Lx = &Lx[f..l];
        let Li = &Li[f..l];
        for (&Lij, &Lxj) in zip(Li, Lx) {
            s += Lxj * x[Lij];
        }
        x[i] *= Dinv[i];
        x[i] -= s;
    }
}

// Solves Ax = b where A has given LDL factors, with x replacing b
fn _solve(Lp: &[usize], Li: &[usize], Lx: &[T], Dinv: &[T], b: &mut [T]) {
    // KNP-1 (patch 0006): the bounds-checked substitutions; upstream called unsafe
    // copies with unchecked indexing here.
    _lsolve_safe(Lp, Li, Lx, b);

    // combined D and L^T solve in one pass
    _dltsolve_safe(Lp, Li, Lx, Dinv, b);
}

// Construct an inverse permutation from a permutation
fn _invperm(p: &[usize]) -> Result<Vec<usize>, QDLDLError> {
    let mut b = vec![0; p.len()];

    for (i, j) in p.iter().enumerate() {
        if *j < p.len() && b[*j] == 0 {
            b[*j] = i;
        } else {
            return Err(QDLDLError::InvalidPermutation);
        }
    }
    Ok(b)
}

// permutation and inverse permutation
// functions that require no allocation
// p must be a valid permutation vector
// in both cases for safety

pub(crate) fn permute<T: Copy>(x: &mut [T], b: &[T], p: &[usize]) {
    zip(p, x).for_each(|(p, x)| *x = b[*p]);
}

pub(crate) fn ipermute<T: Copy>(x: &mut [T], b: &[T], p: &[usize]) {
    zip(p, b).for_each(|(p, b)| x[*p] = *b);
}

// Given a sparse symmetric matrix `A` (with only upper triangular entries), return
// permuted sparse symmetric matrix `P` (also only upper triangular) given the
// inverse permutation vector `iperm`."
pub(crate) fn permute_symmetric(A: &CscMatrix, iperm: &[usize]) -> (CscMatrix, Vec<usize>) {
    // perform a number of argument checks
    let (_m, n) = A.size();
    let mut P = CscMatrix::spalloc((n, n), A.nnz());

    // we will record a mapping of entries from A to PAPt
    let mut AtoPAPt = vec![0; A.nnz()];

    _permute_symmetric_inner(
        A,
        &mut AtoPAPt,
        iperm,
        &mut P.rowval,
        &mut P.colptr,
        &mut P.nzval,
    );
    (P, AtoPAPt)
}

// the main function without extra argument checks
// following the book: Timothy Davis - Direct Methods for Sparse Linear Systems

fn _permute_symmetric_inner(
    A: &CscMatrix,
    AtoPAPt: &mut [usize],
    iperm: &[usize],
    Pr: &mut [usize],
    Pc: &mut [usize],
    Pv: &mut [T],
) {
    // 1. count number of entries that each column of P will have
    let n = A.nrows();
    let mut num_entries = vec![0; n];
    let Ar = &A.rowval;
    let Ac = &A.colptr;
    let Av = &A.nzval;

    // count the number of upper-triangle entries in columns of P,
    // keeping in mind the row permutation
    for colA in 0..n {
        let colP = iperm[colA];
        // loop over entries of A in column A...
        for rowA in Ar.iter().take(Ac[colA + 1]).skip(Ac[colA]) {
            let rowP = iperm[*rowA];
            // ...and check if entry is upper triangular
            if *rowA <= colA {
                // determine to which column the entry belongs after permutation
                let col_idx = max(rowP, colP);
                num_entries[col_idx] += 1;
            }
        }
    }

    // 2. calculate permuted Pc = P.colptr from number of entries
    // Pc is one longer than num_entries here.
    Pc[0] = 0;
    let mut acc = 0;
    for (Pckp1, ne) in zip(&mut Pc[1..], &num_entries) {
        *Pckp1 = acc + ne;
        acc = *Pckp1;
    }
    // reuse this memory to keep track of free entries in rowval
    num_entries.copy_from_slice(&Pc[0..n]);

    // use alias
    let mut row_starts = num_entries;

    // 3. permute the row entries and position of corresponding nzval
    for colA in 0..n {
        let colP = iperm[colA];
        // loop over rows of A and determine where each row entry of A should be stored
        for rowA_idx in Ac[colA]..Ac[colA + 1] {
            let rowA = Ar[rowA_idx];
            // check if upper triangular
            if rowA <= colA {
                let rowP = iperm[rowA];
                // determine column to store the entry
                let col_idx = max(colP, rowP);

                // find next free location in rowval (this results in unordered columns in the rowval)
                let rowP_idx = row_starts[col_idx];

                // store rowval and nzval
                Pr[rowP_idx] = min(colP, rowP);
                Pv[rowP_idx] = Av[rowA_idx];

                //record this into the mapping vector
                AtoPAPt[rowA_idx] = rowP_idx;

                // increment next free location
                row_starts[col_idx] += 1;
            }
        }
    }
}

/// Fill-reducing ordering of a symmetric matrix given by its upper triangle `A`, computed
/// from the sparsity pattern only (KNP-1, patch 0006; replaces AMD from the `amd` crate).
///
/// Exact minimum degree on the elimination graph: the graph has an edge `{i, j}` for every
/// stored off-diagonal entry `(i, j)` (values and the diagonal are ignored); at each step the
/// uneliminated node of smallest current degree is eliminated, ties going to the lowest
/// index, and its remaining neighbours become a clique. Every set is a `BTreeSet` and every
/// choice is a total order on `(degree, index)`, so the result is a function of the pattern
/// alone and identical on every target. Returns `perm` with `perm[k]` the `k`-th pivot.
///
/// Size budget: the selection step scans every node, so the cost grows as `n^2` plus the
/// work on the fill. A matrix with more than [`MAX_ORDERING_DIM`] columns or
/// [`MAX_ORDERING_NNZ`] stored entries, or whose elimination graph grows past
/// [`MAX_ORDERING_FILL`] edges, is refused with [`QDLDLError::OrderingTooLarge`]; so is a
/// malformed pattern. The budget is the same on every target, so the outcome is too.
pub fn min_degree_ordering(A: &CscMatrix) -> Result<Vec<usize>, QDLDLError> {
    let n = A.n;
    if n > MAX_ORDERING_DIM || A.rowval.len() > MAX_ORDERING_NNZ || A.colptr.len() != n + 1 {
        return Err(QDLDLError::OrderingTooLarge);
    }
    let mut adj: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    let mut edges = 0usize;
    for col in 0..n {
        let Some(rows) = A.rowval.get(A.colptr[col]..A.colptr[col + 1]) else {
            return Err(QDLDLError::OrderingTooLarge);
        };
        for &row in rows {
            if row != col && row < n && adj[row].insert(col) {
                adj[col].insert(row);
                edges += 1;
            }
        }
    }
    let mut eliminated = vec![false; n];
    let mut perm = Vec::with_capacity(n);
    for _ in 0..n {
        // smallest (degree, index) among the uneliminated nodes
        let mut best: Option<(usize, usize)> = None;
        for (i, nbrs) in adj.iter().enumerate() {
            if !eliminated[i] && best.map_or(true, |(d, _)| nbrs.len() < d) {
                best = Some((nbrs.len(), i));
            }
        }
        let Some((_, p)) = best else { break };
        perm.push(p);
        eliminated[p] = true;
        let nbrs = std::mem::take(&mut adj[p]);
        edges -= nbrs.len();
        for &u in &nbrs {
            adj[u].remove(&p);
            for &v in &nbrs {
                if v != u && adj[u].insert(v) && u < v {
                    edges += 1;
                }
            }
        }
        if edges > MAX_ORDERING_FILL {
            return Err(QDLDLError::OrderingTooLarge);
        }
    }
    Ok(perm)
}

/// Most columns (KKT dimension) [`min_degree_ordering`] accepts.
pub const MAX_ORDERING_DIM: usize = 20_000;
/// Most stored entries of the upper-triangular pattern [`min_degree_ordering`] accepts.
pub const MAX_ORDERING_NNZ: usize = 1_000_000;
/// Most edges the elimination graph may reach during [`min_degree_ordering`] (bounds the
/// fill, and so the memory and time spent on it).
pub const MAX_ORDERING_FILL: usize = 4_000_000;
