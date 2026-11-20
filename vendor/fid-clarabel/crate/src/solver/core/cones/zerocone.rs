use super::*;
use crate::algebra::*;
use core::marker::PhantomData;

// -------------------------------------
// Zero Cone
// -------------------------------------

pub struct ZeroCone {
    dim: usize,
    phantom: PhantomData<T>,
}

impl ZeroCone {
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            phantom: PhantomData,
        }
    }
}

impl Cone for ZeroCone {
    fn degree(&self) -> usize {
        0
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

    fn margins(&mut self, _z: &mut [T], _pd: PrimalOrDualCone) -> (T, T) {
        // for either primal or dual case we specify infinite
        // minimum margin and zero total margin.
        // if we later shift a vector into the zero cone
        // using scaled_unit_shift!, we just zero it
        // out regardless of the applied shift anway
        (T::MAX, T::zero())
    }
    fn scaled_unit_shift(&self, z: &mut [T], _alpha: T, pd: PrimalOrDualCone) {
        if pd == PrimalOrDualCone::PrimalCone {
            z.fill(T::zero());
        } else {
            // do nothing
        }
    }

    fn unit_initialization(&self, z: &mut [T], s: &mut [T]) {
        s.fill(T::zero());
        z.fill(T::zero());
    }

    fn set_identity_scaling(&mut self) {
        //nothing to do
    }

    fn update_scaling(
        &mut self,
        _s: &[T],
        _z: &[T],
        _mu: T,
        _scaling_strategy: ScalingStrategy,
    ) -> bool {
        true
    }

    fn Hs_is_diagonal(&self) -> bool {
        true
    }

    fn get_Hs(&self, Hsblock: &mut [T]) {
        Hsblock.fill(T::zero());
    }

    fn mul_Hs(&mut self, y: &mut [T], _x: &[T], _work: &mut [T]) {
        y.fill(T::zero());
    }

    fn affine_ds(&self, ds: &mut [T], _s: &[T]) {
        ds.fill(T::zero());
    }

    fn combined_ds_shift(
        &mut self,
        shift: &mut [T],
        _step_z: &mut [T],
        _step_s: &mut [T],
        _sigmamu: T,
    ) {
        shift.fill(T::zero());
    }

    fn Ds_from_Dz_offset(&mut self, out: &mut [T], _ds: &[T], _work: &mut [T], _z: &[T]) {
        out.fill(T::zero());
    }

    fn step_length(
        &mut self,
        _dz: &[T],
        _ds: &[T],
        _z: &[T],
        _s: &[T],
        _settings: &CoreSettings,
        alphamax: T,
    ) -> (T, T) {
        //equality constraints allow arbitrary step length
        (alphamax, alphamax)
    }

    fn compute_barrier(&mut self, _z: &[T], _s: &[T], _dz: &[T], _ds: &[T], _alpha: T) -> T {
        T::zero()
    }
}
