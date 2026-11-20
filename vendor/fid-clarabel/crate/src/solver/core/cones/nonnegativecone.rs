use super::*;
use crate::algebra::*;
use std::iter::zip;

// -------------------------------------
// Nonnegative Cone
// -------------------------------------

pub struct NonnegativeCone {
    dim: usize,
    w: Vec<T>,
    lambda: Vec<T>,
}

impl NonnegativeCone {
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            w: vec![T::zero(); dim],
            lambda: vec![T::zero(); dim],
        }
    }
}

impl Cone for NonnegativeCone {
    fn degree(&self) -> usize {
        self.dim
    }

    fn numel(&self) -> usize {
        self.dim
    }

    fn is_symmetric(&self) -> bool {
        true
    }

    fn is_sparse_expandable(&self) -> bool {
        false
    }

    fn allows_primal_dual_scaling(&self) -> bool {
        true
    }

    fn rectify_equilibration(&self, delta: &mut [T], _e: &[T]) -> bool {
        delta.set(T::one());
        false
    }

    fn margins(&mut self, z: &mut [T], _pd: PrimalOrDualCone) -> (T, T) {
        let alpha = z.minimum();
        let beta = z
            .iter()
            .fold(T::zero(), |beta, &zi| beta + tmax(zi, T::zero()));
        (alpha, beta)
    }

    fn scaled_unit_shift(&self, z: &mut [T], alpha: T, _pd: PrimalOrDualCone) {
        z.translate(alpha);
    }

    fn unit_initialization(&self, z: &mut [T], s: &mut [T]) {
        z.fill(T::one());
        s.fill(T::one());
    }

    fn set_identity_scaling(&mut self) {
        self.w.fill(T::one());
    }

    fn update_scaling(
        &mut self,
        s: &[T],
        z: &[T],
        _mu: T,
        _scaling_strategy: ScalingStrategy,
    ) -> bool {
        for ((lambda, w), (s, z)) in zip(zip(&mut self.lambda, &mut self.w), zip(s, z)) {
            *lambda = sq_root((*s) * (*z));
            *w = sq_root((*s) / (*z));
        }

        true
    }

    fn Hs_is_diagonal(&self) -> bool {
        true
    }

    fn get_Hs(&self, Hsblock: &mut [T]) {
        assert_eq!(self.w.len(), Hsblock.len());
        for (blki, &wi) in zip(Hsblock, &self.w) {
            *blki = wi * wi;
        }
    }

    fn mul_Hs(&mut self, y: &mut [T], x: &[T], _work: &mut [T]) {
        //NB : seemingly sensitive to order of multiplication
        for (yi, (&wi, &xi)) in y.iter_mut().zip(self.w.iter().zip(x)) {
            *yi = wi * (wi * xi)
        }
    }

    fn affine_ds(&self, ds: &mut [T], _s: &[T]) {
        assert_eq!(self.lambda.len(), ds.len());
        for (dsi, &lambdai) in zip(ds, &self.lambda) {
            *dsi = lambdai * lambdai;
        }
    }

    fn combined_ds_shift(&mut self, dz: &mut [T], step_z: &mut [T], step_s: &mut [T], sigmamu: T) {
        //PJG: could be done faster for nonnegatives?
        self._combined_ds_shift_symmetric(dz, step_z, step_s, sigmamu);
    }

    fn Ds_from_Dz_offset(&mut self, out: &mut [T], ds: &[T], _work: &mut [T], z: &[T]) {
        for (outi, (&dsi, &zi)) in zip(out, zip(ds, z)) {
            *outi = dsi / zi;
        }
    }

    fn step_length(
        &mut self,
        dz: &[T],
        ds: &[T],
        z: &[T],
        s: &[T],
        _settings: &CoreSettings,
        alphamax: T,
    ) -> (T, T) {
        assert_eq!(z.len(), s.len());
        assert_eq!(dz.len(), z.len());
        assert_eq!(ds.len(), s.len());

        let mut alphaz = alphamax;
        let mut alphas = alphamax;

        for i in 0..z.len() {
            if dz[i] < T::zero() {
                alphaz = tmin(alphaz, -z[i] / dz[i]);
            }
            if ds[i] < T::zero() {
                alphas = tmin(alphas, -s[i] / ds[i]);
            }
        }
        (alphaz, alphas)
    }

    fn compute_barrier(&mut self, z: &[T], s: &[T], dz: &[T], ds: &[T], alpha: T) -> T {
        assert_eq!(z.len(), s.len());
        assert_eq!(dz.len(), z.len());
        assert_eq!(ds.len(), s.len());
        let mut barrier = T::zero();
        for ((&s, &ds), (&z, &dz)) in zip(zip(s, ds), zip(z, dz)) {
            let si = s + alpha * ds;
            let zi = z + alpha * dz;
            barrier -= (si * zi).logsafe();
        }
        barrier
    }
}

// ---------------------------------------------
// operations supported by symmetric cones only
// ---------------------------------------------

impl SymmetricCone for NonnegativeCone {
    fn lambda_inv_circ_op(&mut self, x: &mut [T], z: &[T]) {
        _inv_circ_op(x, &self.lambda, z);
    }

    fn mul_W(&mut self, _is_transpose: MatrixShape, y: &mut [T], x: &[T], alpha: T, beta: T) {
        assert_eq!(y.len(), x.len());
        assert_eq!(y.len(), self.w.len());
        for i in 0..y.len() {
            y[i] = alpha * (x[i] * self.w[i]) + beta * y[i];
        }
    }

    fn mul_Winv(&mut self, _is_transpose: MatrixShape, y: &mut [T], x: &[T], alpha: T, beta: T) {
        assert_eq!(y.len(), x.len());
        assert_eq!(y.len(), self.w.len());
        for i in 0..y.len() {
            y[i] = alpha * (x[i] / self.w[i]) + beta * y[i];
        }
    }
}

// ---------------------------------------------
// Jordan algebra operations for symmetric cones
// ---------------------------------------------

impl JordanAlgebra for NonnegativeCone {
    fn circ_op(&mut self, x: &mut [T], y: &[T], z: &[T]) {
        _circ_op(x, y, z);
    }

    fn inv_circ_op(&mut self, x: &mut [T], y: &[T], z: &[T]) {
        _inv_circ_op(x, y, z);
    }
}

// circ ops don't use self for this cone, so put the actual
// implementations outside so that they can be called by
// other functions with entering borrow check hell
fn _circ_op(x: &mut [T], y: &[T], z: &[T]) {
    for (x, (y, z)) in zip(x, zip(y, z)) {
        *x = (*y) * (*z);
    }
}

fn _inv_circ_op(x: &mut [T], y: &[T], z: &[T]) {
    for (x, (y, z)) in zip(x, zip(y, z)) {
        *x = (*z) / (*y);
    }
}
