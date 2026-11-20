// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
use crate::algebra::T;
use crate::algebra::{sq_root, tmax, tmin, AsFloatT, FloatT, ScalarMath, VectorMath};
use fid_num::red;
use fid_num::NumError;
use std::iter::zip;

// KNP-1 (patch 0005): every sum, dot product and 2-norm of the solver goes through
// fid_num::red (KRN.NUM.RED): a fixed block-and-tree compensated summation, plain products
// (no FMA), bit-identical on every target. fid-num reports a NaN, an infinity or an
// overflow as an error; it becomes a NaN here, which the solver turns into a
// NumericalError status (see algebra/knp.rs).
fn red_value(r: Result<T, NumError>) -> T {
    r.unwrap_or(T::NAN)
}

impl VectorMath for [T] {
    fn copy_from(&mut self, src: &[T]) -> &mut Self {
        self.copy_from_slice(src);
        self
    }

    fn select(&self, index: &[bool]) -> Vec<T> {
        // invariant (patch 0003): presolve keep flags have one entry per element.
        assert_eq!(self.len(), index.len());
        zip(self, index)
            .filter(|(_x, &b)| b)
            .map(|(&x, _b)| x)
            .collect()
    }

    fn scalarop(&mut self, op: impl Fn(T) -> T) -> &mut Self {
        for x in &mut *self {
            *x = op(*x);
        }
        self
    }

    fn scalarop_from(&mut self, op: impl Fn(T) -> T, v: &[T]) -> &mut Self {
        for (x, v) in zip(&mut *self, v) {
            *x = op(*v);
        }
        self
    }

    fn translate(&mut self, c: T) -> &mut Self {
        //NB: translate is a scalar shift of all variables and is
        //used only in the NN cone to force vectors into R^n_+
        self.scalarop(|x| x + c)
    }

    fn set(&mut self, c: T) -> &mut Self {
        self.fill(c);
        self
    }

    fn scale(&mut self, c: T) -> &mut Self {
        self.scalarop(|x| x * c)
    }

    fn recip(&mut self) -> &mut Self {
        self.scalarop(T::recip)
    }

    fn elem_root(&mut self) -> &mut Self {
        self.scalarop(sq_root)
    }

    fn rsqrt(&mut self) -> &mut Self {
        self.scalarop(|x| T::recip(sq_root(x)))
    }

    fn negate(&mut self) -> &mut Self {
        self.scalarop(|x| -x)
    }

    fn hadamard(&mut self, y: &[T]) -> &mut Self {
        zip(&mut *self, y).for_each(|(x, y)| *x *= *y);
        self
    }

    fn clip(&mut self, min_thresh: T, max_thresh: T) -> &mut Self {
        self.scalarop(|x| x.clip(min_thresh, max_thresh))
    }

    fn normalize(&mut self) -> T {
        let norm = self.norm();
        if norm.is_zero() {
            return T::zero();
        }
        self.scale(norm.recip());
        norm
    }

    fn dot(&self, y: &[T]) -> T {
        red_value(red::dot(self, y))
    }

    fn dot_shifted(z: &[T], s: &[T], dz: &[T], ds: &[T], alpha: T) -> T {
        // invariant (patch 0003): z, s, dz, ds are the same cone range of solver vectors.
        assert_eq!(z.len(), s.len());
        assert_eq!(z.len(), dz.len());
        assert_eq!(s.len(), ds.len());

        let mut si = Vec::with_capacity(s.len());
        let mut zi = Vec::with_capacity(z.len());
        for ((&s, &ds), (&z, &dz)) in zip(zip(s, ds), zip(z, dz)) {
            si.push(s + alpha * ds);
            zi.push(z + alpha * dz);
        }
        red_value(red::dot(&si, &zi))
    }

    fn dist(&self, y: &Self) -> T {
        let d: Vec<T> = zip(self, y).map(|(&x, &y)| x - y).collect();
        red_value(red::norm2(&d))
    }

    fn sum(&self) -> T {
        red_value(red::sum(self))
    }

    fn sumsq(&self) -> T {
        red_value(red::sum_sq(self))
    }

    // 2-norm (overflow-safe; replaces upstream's stable_norm)
    fn norm(&self) -> T {
        red_value(red::norm2(self))
    }

    //2-norm of elementwise product self.*v
    fn norm_scaled(&self, v: &[T]) -> T {
        // invariant (patch 0003): both are solver vectors of the same dimension (n or m).
        assert_eq!(self.len(), v.len());
        let p: Vec<T> = zip(self, v).map(|(&yi, &vi)| yi * vi).collect();
        red_value(red::norm2(&p))
    }

    // 2-norm of (self + α.dz)
    fn norm_shifted(&self, dz: &[T], alpha: T) -> T {
        let p: Vec<T> = zip(self, dz).map(|(&zi, &dzi)| zi + alpha * dzi).collect();
        red_value(red::norm2(&p))
    }

    // Returns infinity norm
    fn norm_inf(&self) -> T {
        let mut out = T::zero();
        for &v in self {
            if v.is_nan() {
                return T::nan();
            }
            out = tmax(out, v.abs());
        }
        out
    }

    // Returns one norm
    fn norm_one(&self) -> T {
        let a: Vec<T> = self.iter().map(|v| v.abs()).collect();
        red_value(red::sum(&a))
    }

    //inf-norm of elementwise product self.*v
    fn norm_inf_scaled(&self, v: &Self) -> T {
        // invariant (patch 0003): both are solver vectors of the same dimension (n or m).
        assert_eq!(self.len(), v.len());
        zip(self, v).fold(T::zero(), |acc, (&x, &y)| tmax(acc, T::abs(x * y)))
    }

    //
    fn norm_one_scaled(&self, v: &Self) -> T {
        let a: Vec<T> = zip(self, v).map(|(&x, &y)| T::abs(x * y)).collect();
        red_value(red::sum(&a))
    }

    // max absolute difference (used for unit testing)
    fn norm_inf_diff(&self, b: &[T]) -> T {
        zip(self, b).fold(T::zero(), |acc, (x, y)| tmax(acc, T::abs(*x - *y)))
    }

    fn minimum(&self) -> T {
        self.iter().fold(T::infinity(), |r, &s| tmin(r, s))
    }

    fn maximum(&self) -> T {
        self.iter().fold(-T::infinity(), |r, &s| tmax(r, s))
    }

    fn mean(&self) -> T {
        let mean = if self.is_empty() {
            T::zero()
        } else {
            let num = self.sum();
            let den = self.len().as_T();
            num / den
        };
        mean
    }

    fn is_finite(&self) -> bool {
        self.iter().all(|&x| T::is_finite(x))
    }

    fn axpby(&mut self, a: T, x: &[T], b: T) -> &mut Self {
        // invariant (patch 0003): both are solver vectors of the same dimension (n or m).
        assert_eq!(self.len(), x.len());

        zip(&mut *self, x).for_each(|(y, x)| *y = a * (*x) + b * (*y));
        self
    }

    fn waxpby(&mut self, a: T, x: &[T], b: T, y: &[T]) -> &mut Self {
        // invariant (patch 0003): both are solver vectors of the same dimension (n or m).
        assert_eq!(self.len(), x.len());
        assert_eq!(self.len(), y.len());

        for (w, (x, y)) in zip(&mut *self, zip(x, y)) {
            *w = a * (*x) + b * (*y);
        }
        self
    }
}
