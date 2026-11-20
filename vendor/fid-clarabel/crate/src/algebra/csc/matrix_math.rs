use crate::algebra::*;
use std::iter::zip;

impl MatrixVectorMultiply for CscMatrix {
    fn gemv(&self, y: &mut [T], x: &[T], a: T, b: T) {
        _csc_axpby_N(self, y, x, a, b);
    }
}

impl MatrixVectorMultiply for Adjoint<'_, CscMatrix> {
    fn gemv(&self, y: &mut [T], x: &[T], a: T, b: T) {
        _csc_axpby_T(self.src, y, x, a, b);
    }
}

impl SymMatrixMath for Symmetric<'_, CscMatrix> {
    fn symv(&self, y: &mut [T], x: &[T], a: T, b: T) {
        // KNP-1 (patch 0005): the bounds-checked version; upstream used an unsafe copy
        _csc_symv_safe(self.src, self.uplo, y, x, a, b)
    }

    fn quad_form(&self, y: &[T], x: &[T]) -> T {
        _csc_quad_form(self.src, self.uplo, y, x)
    }
}

impl MatrixMath for CscMatrix {
    fn col_sums(&self, sums: &mut [T]) {
        assert_eq!(self.n, sums.len());
        for (col, sum) in sums.iter_mut().enumerate() {
            let rng = self.colptr[col]..self.colptr[col + 1];
            *sum = self.nzval[rng].sum();
        }
    }

    fn row_sums(&self, sums: &mut [T]) {
        assert_eq!(self.m, sums.len());
        sums.fill(T::zero());
        for (&row, &val) in zip(&self.rowval, &self.nzval) {
            sums[row] += val;
        }
    }

    fn col_norms(&self, norms: &mut [T]) {
        norms.fill(T::zero());
        self.col_norms_no_reset(norms);
    }

    fn col_norms_no_reset(&self, norms: &mut [T]) {
        assert_eq!(norms.len(), self.colptr.len() - 1);

        for (i, v) in norms.iter_mut().enumerate() {
            *v = self
                .nzval
                .iter()
                .take(self.colptr[i + 1])
                .skip(self.colptr[i])
                .fold(*v, |m, &nzval| tmax(m, T::abs(nzval)));
        }
    }

    fn col_norms_sym(&self, norms: &mut [T]) {
        norms.fill(T::zero());
        self.col_norms_sym_no_reset(norms);
    }

    fn col_norms_sym_no_reset(&self, norms: &mut [T]) {
        assert_eq!(norms.len(), self.colptr.len() - 1);

        for i in 0..norms.len() {
            for j in self.colptr[i]..self.colptr[i + 1] {
                let tmp = T::abs(self.nzval[j]);
                let r = self.rowval[j];
                norms[i] = tmax(norms[i], tmp);
                norms[r] = tmax(norms[r], tmp);
            }
        }
    }

    fn row_norms(&self, norms: &mut [T]) {
        norms.fill(T::zero());
        self.row_norms_no_reset(norms);
    }

    fn row_norms_no_reset(&self, norms: &mut [T]) {
        assert_eq!(self.rowval.len(), self.colptr.last().copied().unwrap_or(0));

        for (row, val) in zip(&self.rowval, &self.nzval) {
            norms[*row] = tmax(norms[*row], T::abs(*val));
        }
    }
}

impl MatrixMathMut for CscMatrix {
    //scalar mut operations
    fn scale(&mut self, c: T) {
        self.nzval.scale(c);
    }

    fn negate(&mut self) {
        self.nzval.negate();
    }

    fn lscale(&mut self, l: &[T]) {
        for (val, row) in zip(&mut self.nzval, &self.rowval) {
            *val *= l[*row];
        }
    }

    fn rscale(&mut self, r: &[T]) {
        let colptr = &self.colptr;
        let vals = &mut self.nzval;

        assert_eq!(vals.len(), colptr.last().copied().unwrap_or(0));
        for i in 0..self.n {
            vals[colptr[i]..colptr[i + 1]].scale(r[i]);
        }
    }

    fn lrscale(&mut self, l: &[T], r: &[T]) {
        assert_eq!(self.nzval.len(), self.colptr.last().copied().unwrap_or(0));

        for (col, &ri) in r.iter().enumerate() {
            let (first, last) = (self.colptr[col], self.colptr[col + 1]);
            let vals = &mut self.nzval[first..last];
            let rows = &self.rowval[first..last];

            for (val, row) in zip(vals, rows) {
                *val *= l[*row] * ri;
            }
        }
    }
}

#[allow(non_snake_case)]
fn _csc_symv_safe(A: &CscMatrix, _uplo: MatrixTriangle, y: &mut [T], x: &[T], a: T, b: T) {
    // NB: this function works identically regardless of uplo.  Implemented
    // this for for consistency with _csc_quad_form interface

    y.scale(b);

    assert!(x.len() == A.n);
    assert!(y.len() == A.n);
    assert!(A.n == A.m);

    for (col, &xcol) in x.iter().enumerate() {
        let first = A.colptr[col];
        let last = A.colptr[col + 1];
        let rows = &A.rowval[first..last];
        let nzvals = &A.nzval[first..last];

        for (&row, &Aij) in zip(rows, nzvals) {
            y[row] += a * Aij * xcol;

            if row != col {
                //don't double up on the diagonal
                y[col] += a * Aij * x[row];
            }
        }
    }
}

#[allow(non_snake_case)]
#[allow(clippy::comparison_chain)]
fn _csc_quad_form(M: &CscMatrix, uplo: MatrixTriangle, y: &[T], x: &[T]) -> T {
    assert_eq!(M.n, M.m);
    assert_eq!(x.len(), M.n);
    assert_eq!(y.len(), M.n);
    assert!(M.colptr.len() == M.n + 1);
    assert!(M.nzval.len() == M.rowval.len());

    if M.n == 0 {
        return T::zero();
    }

    let mut out = T::zero();

    let cmp = match uplo {
        MatrixTriangle::Triu => usize::lt,
        MatrixTriangle::Tril => usize::gt,
    };

    for col in 0..M.n {
        //column number

        let mut tmp1 = T::zero();
        let mut tmp2 = T::zero();

        //start / stop indices for the current column
        let first = M.colptr[col];
        let last = M.colptr[col + 1];

        let values = &M.nzval[first..last];
        let rows = &M.rowval[first..last];

        for (&Mv, &row) in zip(values, rows) {
            if cmp(&row, &col) {
                //triangular terms only
                tmp1 += Mv * x[row];
                tmp2 += Mv * y[row];
            } else if row == col {
                out += Mv * x[col] * y[col];
            } else {
                // not triangular: upstream panicked; P is made upper triangular at
                // solver setup, so this is unreachable, and a NaN fails the solve
                return T::NAN;
            }
        }
        out += tmp1 * y[col] + tmp2 * x[col];
    }
    out
}

// sparse matrix-vector multiply, no transpose
#[allow(non_snake_case)]
fn _csc_axpby_N(A: &CscMatrix, y: &mut [T], x: &[T], a: T, b: T) {
    //first do the b*y part
    if b.is_zero() {
        y.fill(T::zero());
    } else if b == T::one() {
    } else if b == -T::one() {
        y.negate();
    } else {
        y.scale(b);
    }

    // if a is zero, we're done
    if a.is_zero() {
        return;
    }

    assert_eq!(A.nzval.len(), A.colptr.last().copied().unwrap_or(0));
    assert_eq!(x.len(), A.n);

    //y += A*x
    if a == T::one() {
        for (j, xj) in x.iter().enumerate().take(A.n) {
            for i in A.colptr[j]..A.colptr[j + 1] {
                y[A.rowval[i]] += A.nzval[i] * *xj;
            }
        }
    } else if a == -T::one() {
        for (j, xj) in x.iter().enumerate().take(A.n) {
            for i in A.colptr[j]..A.colptr[j + 1] {
                y[A.rowval[i]] -= A.nzval[i] * *xj;
            }
        }
    } else {
        for (j, xj) in x.iter().enumerate().take(A.n) {
            for i in A.colptr[j]..A.colptr[j + 1] {
                y[A.rowval[i]] += a * A.nzval[i] * *xj;
            }
        }
    }
}

// sparse matrix-vector multiply, transposed
#[allow(non_snake_case)]
fn _csc_axpby_T(A: &CscMatrix, y: &mut [T], x: &[T], a: T, b: T) {
    //first do the b*y part
    if b.is_zero() {
        y.fill(T::zero());
    } else if b == T::one() {
    } else if b == -T::one() {
        y.negate();
    } else {
        y.scale(b);
    }

    // if a is zero, we're done
    if a.is_zero() {
        return;
    }

    assert_eq!(A.nzval.len(), A.colptr.last().copied().unwrap_or(0));
    assert_eq!(x.len(), A.m);

    //y += A*x
    if a == T::one() {
        for (j, yj) in y.iter_mut().enumerate().take(A.n) {
            for k in A.colptr[j]..A.colptr[j + 1] {
                *yj += A.nzval[k] * x[A.rowval[k]];
            }
        }
    } else if a == -T::one() {
        for (j, yj) in y.iter_mut().enumerate().take(A.n) {
            for k in A.colptr[j]..A.colptr[j + 1] {
                *yj -= A.nzval[k] * x[A.rowval[k]];
            }
        }
    } else {
        for (j, yj) in y.iter_mut().enumerate().take(A.n) {
            for k in A.colptr[j]..A.colptr[j + 1] {
                *yj += a * A.nzval[k] * x[A.rowval[k]];
            }
        }
    }
}
