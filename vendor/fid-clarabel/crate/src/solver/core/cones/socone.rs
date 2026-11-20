use super::*;
use crate::algebra::*;

// -------------------------------------
// Second order Cone
// -------------------------------------

pub struct SecondOrderConeSparseData {
    //vectors for rank 2 update representation of W^2
    pub u: Vec<T>,
    pub v: Vec<T>,

    //additional scalar terms for rank-2 rep
    pub d: T,
}

impl SecondOrderConeSparseData {
    pub fn new(dim: usize) -> Self {
        Self {
            u: vec![T::zero(); dim],
            v: vec![T::zero(); dim],
            d: T::one(),
        }
    }
}

pub struct SecondOrderCone {
    pub dim: usize,
    //internal working variables for W and its products
    pub w: Vec<T>,
    //scaled version of (s,z)
    pub lambda: Vec<T>,
    pub eta: T,
    pub sparse_data: Option<SecondOrderConeSparseData>,
}

impl SecondOrderCone {
    pub fn new(dim: usize) -> Self {
        const SOC_NO_EXPANSION_MAX_SIZE: usize = 4;

        assert!(dim >= 2);

        let w = vec![T::zero(); dim];
        let lambda = vec![T::zero(); dim];
        let eta = T::zero();

        let sparse_data = {
            if dim > SOC_NO_EXPANSION_MAX_SIZE {
                Some(SecondOrderConeSparseData::new(dim))
            } else {
                None
            }
        };

        Self {
            dim,
            w,
            lambda,
            eta,
            sparse_data,
        }
    }
}

impl Cone for SecondOrderCone {
    fn degree(&self) -> usize {
        // degree = 1 for SOC, since e'*e = 1
        1
    }

    fn numel(&self) -> usize {
        self.dim
    }

    fn is_symmetric(&self) -> bool {
        true
    }

    fn is_sparse_expandable(&self) -> bool {
        self.sparse_data.is_some()
    }

    fn allows_primal_dual_scaling(&self) -> bool {
        true
    }

    fn rectify_equilibration(&self, delta: &mut [T], e: &[T]) -> bool {
        delta.copy_from(e).recip().scale(e.mean());

        true // scalar equilibration
    }

    // functions relating to unit vectors and cone initialization
    fn margins(&mut self, z: &mut [T], _pd: PrimalOrDualCone) -> (T, T) {
        let alpha = z[0] - z[1..].norm();
        let beta = tmax(T::zero(), alpha);
        (alpha, beta)
    }

    fn scaled_unit_shift(&self, z: &mut [T], alpha: T, _pd: PrimalOrDualCone) {
        z[0] += alpha;
    }

    fn unit_initialization(&self, z: &mut [T], s: &mut [T]) {
        s.fill(T::zero());
        z.fill(T::zero());
        self.scaled_unit_shift(s, T::one(), PrimalOrDualCone::PrimalCone);
        self.scaled_unit_shift(z, T::one(), PrimalOrDualCone::DualCone);
    }

    fn set_identity_scaling(&mut self) {
        self.w.fill(T::zero());
        self.w[0] = T::one();
        self.eta = T::one();

        if let Some(sparse_data) = &mut self.sparse_data {
            sparse_data.d = (0.5).as_T();
            sparse_data.u.fill(T::zero());
            sparse_data.u[0] = std::f64::consts::FRAC_1_SQRT_2;
            sparse_data.v.fill(T::zero());
        }
    }

    fn update_scaling(
        &mut self,
        s: &[T],
        z: &[T],
        _mu: T,
        _scaling_strategy: ScalingStrategy,
    ) -> bool {
        let two: T = (2.0).as_T();
        let half: T = (0.5).as_T();

        //first calculate the scaled vector w
        let zscale = _sqrt_soc_residual(z);
        let sscale = _sqrt_soc_residual(s);

        //Fail if either s or z is not an interior point
        if zscale.is_zero() || sscale.is_zero() {
            return false;
        }

        //the leading scalar term for W^TW
        self.eta = sq_root(sscale / zscale);

        // construct w and normalize
        let w = &mut self.w;
        w.copy_from(s);
        w.scale(sscale.recip());
        w[0] += z[0] / zscale;
        w[1..].axpby(-zscale.recip(), &z[1..], T::one());

        let wscale = _sqrt_soc_residual(w);
        // Fail if w is not an interior point
        if wscale.is_zero() {
            return false;
        }
        w.scale(wscale.recip());

        // try to force badly scaled w to come out normalized
        let w1sq = w[1..].sumsq();
        w[0] = sq_root(T::one() + w1sq);

        //Compute the scaling point λ.   Should satisfy λ = Wz = W^{-T}s
        let gamma = half * wscale;
        self.lambda[0] = gamma;
        self.lambda[1..].waxpby(
            (gamma + z[0] / zscale) / sscale,
            &s[1..],
            (gamma + s[0] / sscale) / zscale,
            &z[1..],
        );
        self.lambda[1..].scale(T::recip(s[0] / sscale + z[0] / zscale + two * gamma));
        self.lambda.scale(sq_root(sscale * zscale));

        // Populate sparse expansion terms if allocated
        if let Some(sparse_data) = &mut self.sparse_data {
            //various intermediate calcs for u,v,d,η
            let alpha = two * w[0];

            //Scalar d is the upper LH corner of the diagonal
            //term in the rank-2 update form of W^TW
            let wsq = w[0] * w[0] + w1sq;
            let wsqinv = wsq.recip();
            sparse_data.d = half * wsqinv;

            //the vectors for the rank two update
            //representation of W^TW
            let u0 = sq_root(wsq - sparse_data.d);
            let u1 = alpha / u0;
            let v0 = T::zero();
            let v1 = sq_root(two * (two + wsqinv) / (two * wsq - wsqinv));

            sparse_data.u[0] = u0;
            sparse_data.u[1..].axpby(u1, &self.w[1..], T::zero());
            sparse_data.v[0] = v0;
            sparse_data.v[1..].axpby(v1, &self.w[1..], T::zero());
        }

        true
    }

    fn Hs_is_diagonal(&self) -> bool {
        self.is_sparse_expandable()
    }

    fn get_Hs(&self, Hsblock: &mut [T]) {
        if let Some(sparse_data) = &self.sparse_data {
            // For sparse form, we are returning here the diagonal D block
            // from the sparse representation of W^TW, but not the
            // extra two entries at the bottom right of the block.
            Hsblock.fill(self.eta * self.eta);
            Hsblock[0] *= sparse_data.d;
        } else {
            // for dense form, we return H = \eta^2 (2*ww^T - J), where
            // J = diag(1,-I).  We are packing into dense triu form

            // 2 * w[0]^2 - 1., avoiding bad cancellations
            Hsblock[0] = (std::f64::consts::SQRT_2 * self.w[0] - T::one())
                * (std::f64::consts::SQRT_2 * self.w[0] + T::one());

            let mut hidx = 1;
            let two: T = (2.).as_T();

            for col in 1..self.dim {
                let wcol = self.w[col];
                for row in 0..=col {
                    Hsblock[hidx] = two * self.w[row] * wcol;
                    hidx += 1
                }
                //go back to add the offset term from J
                Hsblock[hidx - 1] += T::one()
            }
            Hsblock.scale(self.eta * self.eta);
        }
    }

    fn mul_Hs(&mut self, y: &mut [T], x: &[T], _work: &mut [T]) {
        //self.mul_W(MatrixShape::N, work, x, T::one(), T::zero()); // work = Wx
        //self.mul_W(MatrixShape::T, y, work, T::one(), T::zero()); // y = c Wᵀwork = W^TWx
        let c = self.w.dot(x) * (2.).as_T();
        y.copy_from(x);
        y[0] = -x[0];
        y.axpby(c, &self.w, T::one());
        y.scale(self.eta * self.eta);
    }

    fn affine_ds(&self, ds: &mut [T], _s: &[T]) {
        _circ_op(ds, &self.lambda, &self.lambda);
    }

    fn combined_ds_shift(
        &mut self,
        shift: &mut [T],
        step_z: &mut [T],
        step_s: &mut [T],
        sigmamu: T,
    ) {
        self._combined_ds_shift_symmetric(shift, step_z, step_s, sigmamu);
    }

    fn Ds_from_Dz_offset(&mut self, out: &mut [T], ds: &[T], _work: &mut [T], z: &[T]) {
        // out = Wᵀ(λ \ ds).  Below is equivalent,
        // but appears to be a little more stable

        let resz = _soc_residual(z);

        let lambda1ds1 = self.lambda[1..].dot(&ds[1..]);
        let w1ds1 = self.w[1..].dot(&ds[1..]);

        out.scalarop_from(|zi| -zi, z);
        out[0] = z[0];

        let c = self.lambda[0] * ds[0] - lambda1ds1;
        out.scale(c / resz);

        out[0] += self.eta * w1ds1;
        for (outi, (&dsi, &wi)) in
            std::iter::zip(out[1..].iter_mut(), std::iter::zip(&ds[1..], &self.w[1..]))
        {
            *outi += self.eta * (dsi + w1ds1 / (T::one() + self.w[0]) * wi);
        }

        out.scale(self.lambda[0].recip());
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
        let alphaz = _step_length_soc_component(z, dz, alphamax);
        let alphas = _step_length_soc_component(s, ds, alphamax);

        (alphaz, alphas)
    }

    fn compute_barrier(&mut self, z: &[T], s: &[T], dz: &[T], ds: &[T], alpha: T) -> T {
        let res_s = _soc_residual_shifted(s, ds, alpha);
        let res_z = _soc_residual_shifted(z, dz, alpha);

        // avoid numerical issue if res_s <= 0 or res_z <= 0
        if res_s > T::zero() && res_z > T::zero() {
            -(res_s * res_z).logsafe() * (0.5).as_T()
        } else {
            T::infinity()
        }
    }
}

// ---------------------------------------------
// operations supported by symmetric cones only
// ---------------------------------------------

impl SymmetricCone for SecondOrderCone {
    fn lambda_inv_circ_op(&mut self, x: &mut [T], z: &[T]) {
        _inv_circ_op(x, &self.lambda, z);
    }

    fn mul_W(&mut self, _is_transpose: MatrixShape, y: &mut [T], x: &[T], alpha: T, beta: T) {
        // symmetric, so ignore transpose
        _soc_mul_W_inner(y, x, alpha, beta, &self.w, self.eta);
    }

    fn mul_Winv(&mut self, _is_transpose: MatrixShape, y: &mut [T], x: &[T], alpha: T, beta: T) {
        _soc_mul_Winv_inner(y, x, alpha, beta, &self.w, self.eta);
    }
}

// ---------------------------------------------
// Jordan algebra operations for symmetric cones
// ---------------------------------------------

impl JordanAlgebra for SecondOrderCone {
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
    x[0] = y.dot(z);
    let (y0, z0) = (y[0], z[0]);
    x[1..].waxpby(y0, &z[1..], z0, &y[1..]);
}

fn _inv_circ_op(x: &mut [T], y: &[T], z: &[T]) {
    let p = _soc_residual(y);
    let pinv = T::recip(p);
    let v = y[1..].dot(&z[1..]);

    x[0] = (y[0] * z[0] - v) * pinv;

    let c1 = pinv * (v / y[0] - z[0]);
    let c2 = T::recip(y[0]);
    x[1..].waxpby(c1, &y[1..], c2, &z[1..]);
}

// ---------------------------------------------
// internal operations for second order cones
// ---------------------------------------------

fn _soc_residual(z: &[T]) -> T {
    let z1norm = z[1..].norm();
    (z[0] - z1norm) * (z[0] + z1norm)
}

fn _sqrt_soc_residual(z: &[T]) -> T {
    let res = _soc_residual(z);
    if res > T::zero() {
        sq_root(res)
    } else {
        T::zero()
    }
}

// compute the residual at z + \alpha dz
// without storing the intermediate vector
fn _soc_residual_shifted(z: &[T], dz: &[T], alpha: T) -> T {
    let x0 = z[0] + alpha * dz[0];
    let x1norm = z[1..].norm_shifted(&dz[1..], alpha);
    (x0 - x1norm) * (x0 + x1norm)
}

// find the maximum step length α≥0 so that
// x + αy stays in the SOC
fn _step_length_soc_component(x: &[T], y: &[T], alphamax: T) -> T {
    let mut alphamax = alphamax;

    // upper bound the step length by the maximum allowable
    // step length for the scalar part of the cone
    if x[0] >= T::zero() && y[0] < T::zero() {
        alphamax = tmin(alphamax, -x[0] / y[0]);
    }

    // assume that x is in the SOC, and find the minimum positive root
    // of the quadratic equation:  ||x₁+αy₁||^2 = (x₀ + αy₀)^2

    let two: T = (2.).as_T();
    let four: T = (4.).as_T();

    let a = _soc_residual(y); //NB: could be negative
    let b = two * (x[0] * y[0] - x[1..].dot(&y[1..]));
    let c = tmax(T::zero(), _soc_residual(x)); //should be ≥0
    let d = b * b - four * a * c;

    if !(c >= T::zero()) {
        // Not reachable for finite data since c ≥ 0 above (upstream panicked here). A NaN
        // gives a zero step, which ends the solve with a status instead of a panic.
        return T::zero();
    }

    #[allow(clippy::if_same_then_else)] // allows explanation of separate cases
    if (a > T::zero() && b > T::zero()) || d < T::zero() {
        //all negative roots / complex root pair
        //-> infinite step length
        return alphamax;
    } else if a.is_zero() {
        // edge case with only one root.  This corresponds to
        // the case where the search direction is exactly on the
        // cone boundary.   The root should be -c/b, but b can't
        // be negative since both (x,y) are in the cone and it is
        // self dual, so <x,y> \ge 0 necessarily.
        return alphamax;
    } else if c.is_zero() {
        // Edge case with one of the roots at 0.   This corresponds
        // to the case where the initial point is exactly on the
        // cone boundary.  The other root is -b/a.   If the search
        // direction is in the cone, then a >= 0 and b can't be
        // negative due to self-duality.  If a < 0, then the
        // direction is outside the cone and b can't be positive.
        // Either way, step length is determined by whether or not
        // the search direction is in the cone.
        return if a >= T::zero() { alphamax } else { T::zero() };
    }

    // if we got this far then we need to calculate a pair
    // of real roots and choose the smallest positive one.
    // We need to be cautious about cancellations though.
    // See §1.4: Goldberg, ACM Computing Surveys, 1991
    // https://dl.acm.org/doi/pdf/10.1145/103162.103163

    let t = {
        if b >= T::zero() {
            -b - sq_root(d)
        } else {
            -b + sq_root(d)
        }
    };

    let r1: T = (two * c) / t;
    let r2: T = t / (two * a);

    // return the minimum positive root, up to αmax
    let r1 = if r1 < T::zero() { T::infinity() } else { r1 };
    let r2 = if r2 < T::zero() { T::infinity() } else { r2 };

    tmin(alphamax, tmin(r1, r2))
}

// Must move the actual implementations of W*x to an outside
// fcn.  The operation λ = Wz produces a borrow conflict
// otherwise because λ is part of the cone's internal data
// and we can't borrow self and &mut λ at the same time.
// Could also have been done using std::mem::take

#[allow(non_snake_case)]
fn _soc_mul_W_inner(y: &mut [T], x: &[T], alpha: T, beta: T, w: &[T], eta: T) {
    // use the fast product method from ECOS ECC paper
    let zeta = w[1..].dot(&x[1..]);
    let c = x[0] + zeta / (T::one() + w[0]);

    y[0] = (alpha * eta) * (w[0] * x[0] + zeta) + beta * y[0];

    y[1..].axpby(alpha * eta * c, &w[1..], beta);
    y[1..].axpby(alpha * eta, &x[1..], T::one());
}

fn _soc_mul_Winv_inner(y: &mut [T], x: &[T], alpha: T, beta: T, w: &[T], eta: T) {
    // use the fast inverse product method from ECOS ECC paper
    let zeta = w[1..].dot(&x[1..]);
    let c = -x[0] + zeta / (T::one() + w[0]);

    y[0] = (alpha / eta) * (w[0] * x[0] - zeta) + beta * y[0];

    y[1..].axpby(alpha / eta * c, &w[1..], beta);
    y[1..].axpby(alpha / eta, &x[1..], T::one());
}
