// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
use super::*;
use crate::algebra::T;
use crate::algebra::{sq_root, tmin, triangular_number};
use std::collections::BTreeMap;
use std::iter::zip;
use std::ops::Range;

// -------------------------------------
// default composite cone type
// -------------------------------------

pub struct CompositeCone {
    cones: Vec<SupportedCone>,

    //Type count for each cone type (upstream printed it; kept for the wrapper)
    #[allow(dead_code)]
    // (KNP-1, patch 0004: BTreeMap instead of HashMap, so iteration order is fixed)
    pub(crate) type_counts: BTreeMap<SupportedConeTag, usize>,

    //overall size of the composite cone
    pub(crate) numel: usize,
    pub(crate) degree: usize,

    //ranges for the indices of the constituent cones
    pub(crate) rng_cones: Vec<Range<usize>>,

    //ranges for the indices of the constituent Hs blocks
    //associated with each cone
    pub(crate) rng_blocks: Vec<Range<usize>>,

    // the flag for symmetric cone check
    _is_symmetric: bool,
}

impl CompositeCone {
    /// The composite cone of `types`; `None` if a size, degree or block-range sum
    /// overflows `usize` (patch 0003: checked instead of panicking).
    pub fn new(types: &[SupportedConeT]) -> Option<Self> {
        // make an internal copy to protect from user modification
        let types = types.to_vec();
        let ncones = types.len();
        let mut cones: Vec<SupportedCone> = Vec::with_capacity(ncones);

        // Count for the number of each cone type, indexed by SupportedConeTag
        // NB: ideally we could fix max capacity here,  but Enum::variant_count is not
        // yet a stable feature.  Capacity should be number of SupportedCone variants.
        // See: https://github.com/rust-lang/rust/issues/73662
        let mut type_counts = BTreeMap::new();

        // assumed symmetric to start
        let mut _is_symmetric = true;

        // create cones with the given dims
        for t in types.iter() {
            //make a new cone
            let cone = make_cone(t);

            //update global problem symmetry
            _is_symmetric = _is_symmetric && cone.is_symmetric();

            //increment type counts
            *type_counts.entry(cone.as_tag()).or_insert(0) += 1;

            cones.push(cone);
        }

        // count up elements and degree
        let numel = cones
            .iter()
            .try_fold(0usize, |a, c| a.checked_add(c.numel()))?;
        let degree = cones
            .iter()
            .try_fold(0usize, |a, c| a.checked_add(c.degree()))?;

        //ranges for the subvectors associated with each cone,
        //and the ranges for the corresponding entries
        //in the Hs sparse block

        let rng_cones = make_rng_cones(&cones)?;
        let rng_blocks = make_rng_blocks(&cones)?;

        Some(Self {
            cones,
            //types,
            type_counts,
            numel,
            degree,
            rng_cones,
            rng_blocks,
            _is_symmetric,
        })
    }
}

fn make_rng_cones(cones: &[SupportedCone]) -> Option<Vec<Range<usize>>> {
    let mut rngs = Vec::with_capacity(cones.len());

    if !cones.is_empty() {
        let mut start: usize = 0;
        for cone in cones {
            let stop = start.checked_add(cone.numel())?;
            rngs.push(start..stop);
            start = stop;
        }
    }
    Some(rngs)
}

fn make_rng_blocks(cones: &[SupportedCone]) -> Option<Vec<Range<usize>>> {
    let mut rngs = Vec::with_capacity(cones.len());

    if !cones.is_empty() {
        let mut start: usize = 0;
        for cone in cones {
            let nvars = cone.numel();
            let block = if cone.Hs_is_diagonal() {
                nvars
            } else {
                triangular_number(nvars)?
            };
            let stop = start.checked_add(block)?;
            rngs.push(start..stop);
            start = stop;
        }
    }
    Some(rngs)
}

impl CompositeCone {
    pub fn len(&self) -> usize {
        self.cones.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cones.is_empty()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, SupportedCone> {
        self.cones.iter()
    }
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, SupportedCone> {
        self.cones.iter_mut()
    }
    #[allow(dead_code)]
    pub(crate) fn get_type_count(&self, tag: SupportedConeTag) -> usize {
        if self.type_counts.contains_key(&tag) {
            self.type_counts[&tag]
        } else {
            0
        }
    }
}

impl Cone for CompositeCone {
    fn degree(&self) -> usize {
        self.degree
    }

    fn numel(&self) -> usize {
        self.numel
    }

    fn is_symmetric(&self) -> bool {
        self._is_symmetric
    }

    fn is_sparse_expandable(&self) -> bool {
        //This should probably never be called (upstream: unreachable!(); no panics in the kernel)
        self.cones.iter().any(|cone| cone.is_sparse_expandable())
    }

    fn allows_primal_dual_scaling(&self) -> bool {
        self.cones
            .iter()
            .all(|cone| cone.allows_primal_dual_scaling())
    }

    fn rectify_equilibration(&self, delta: &mut [T], e: &[T]) -> bool {
        let mut any_changed = false;

        // we will update e <- δ .* e using return values
        // from this function.  default is to do nothing at all
        delta.fill(T::one());
        for (cone, rng) in zip(&self.cones, &self.rng_cones) {
            let deltai = &mut delta[rng.clone()];
            let ei = &e[rng.clone()];
            any_changed |= cone.rectify_equilibration(deltai, ei);
        }
        any_changed
    }

    fn margins(&mut self, z: &mut [T], pd: PrimalOrDualCone) -> (T, T) {
        let mut alpha = T::MAX;
        let mut beta = T::zero();
        for (cone, rng) in zip(&mut self.cones, &self.rng_cones) {
            let (alphai, betai) = cone.margins(&mut z[rng.clone()], pd);
            alpha = tmin(alpha, alphai);
            beta += betai;
        }
        (alpha, beta)
    }

    fn scaled_unit_shift(&self, z: &mut [T], alpha: T, pd: PrimalOrDualCone) {
        for (cone, rng) in zip(&self.cones, &self.rng_cones) {
            cone.scaled_unit_shift(&mut z[rng.clone()], alpha, pd);
        }
    }

    fn unit_initialization(&self, z: &mut [T], s: &mut [T]) {
        for (cone, rng) in zip(&self.cones, &self.rng_cones) {
            cone.unit_initialization(&mut z[rng.clone()], &mut s[rng.clone()]);
        }
    }

    fn set_identity_scaling(&mut self) {
        for cone in self.iter_mut() {
            cone.set_identity_scaling();
        }
    }

    fn update_scaling(
        &mut self,
        s: &[T],
        z: &[T],
        mu: T,
        scaling_strategy: ScalingStrategy,
    ) -> bool {
        let mut is_scaling_success;
        for (cone, rng) in zip(&mut self.cones, &self.rng_cones) {
            let si = &s[rng.clone()];
            let zi = &z[rng.clone()];
            is_scaling_success = cone.update_scaling(si, zi, mu, scaling_strategy);
            if !is_scaling_success {
                return false;
            }
        }
        true
    }

    fn Hs_is_diagonal(&self) -> bool {
        //This function should probably never be called since
        //we only us it to interrogate the blocks, but we can
        //implement something reasonable anyway
        self.cones.iter().all(|cone| cone.Hs_is_diagonal())
    }

    #[allow(non_snake_case)]
    fn get_Hs(&self, Hsblock: &mut [T]) {
        for (cone, rng) in zip(&self.cones, &self.rng_blocks) {
            cone.get_Hs(&mut Hsblock[rng.clone()]);
        }
    }

    fn mul_Hs(&mut self, y: &mut [T], x: &[T], work: &mut [T]) {
        for (cone, rng) in zip(&mut self.cones, &self.rng_cones) {
            cone.mul_Hs(&mut y[rng.clone()], &x[rng.clone()], &mut work[rng.clone()]);
        }
    }

    fn affine_ds(&self, ds: &mut [T], s: &[T]) {
        for (cone, rng) in zip(&self.cones, &self.rng_cones) {
            let dsi = &mut ds[rng.clone()];
            let si = &s[rng.clone()];
            cone.affine_ds(dsi, si);
        }
    }

    fn combined_ds_shift(
        &mut self,
        shift: &mut [T],
        step_z: &mut [T],
        step_s: &mut [T],
        sigmamu: T,
    ) {
        // Here we must first explicitly borrow the subvector
        // of cones, since trying to access it using self.iter_mut
        // causes a borrow conflict with ranges.

        // It is necessary for the function to mutate self since
        // nonsymmetric cones modify their internal state when
        // computing the ds_shift

        for (cone, rng) in zip(&mut self.cones, &self.rng_cones) {
            let shifti = &mut shift[rng.clone()];
            let step_zi = &mut step_z[rng.clone()];
            let step_si = &mut step_s[rng.clone()];
            cone.combined_ds_shift(shifti, step_zi, step_si, sigmamu);
        }
    }

    fn Ds_from_Dz_offset(&mut self, out: &mut [T], ds: &[T], work: &mut [T], z: &[T]) {
        for (cone, rng) in zip(&mut self.cones, &self.rng_cones) {
            let outi = &mut out[rng.clone()];
            let dsi = &ds[rng.clone()];
            let worki = &mut work[rng.clone()];
            let zi = &z[rng.clone()];
            cone.Ds_from_Dz_offset(outi, dsi, worki, zi);
        }
    }

    fn step_length(
        &mut self,
        dz: &[T],
        ds: &[T],
        z: &[T],
        s: &[T],
        settings: &CoreSettings,
        alphamax: T,
    ) -> (T, T) {
        let mut alpha = alphamax;
        let all_symmetric = self.is_symmetric();

        let mut innerfcn = |alpha: T, symcond: bool| -> T {
            let mut alpha = alpha;
            for (cone, rng) in zip(&mut self.cones, &self.rng_cones) {
                if cone.is_symmetric() != symcond {
                    continue;
                }
                let (dzi, dsi) = (&dz[rng.clone()], &ds[rng.clone()]);
                let (zi, si) = (&z[rng.clone()], &s[rng.clone()]);
                let (nextalphaz, nextalphas) = cone.step_length(dzi, dsi, zi, si, settings, alpha);
                alpha = tmin(alpha, tmin(nextalphaz, nextalphas));
            }
            alpha
        };

        // Force symmetric cones first.
        alpha = innerfcn(alpha, true);

        // if we have any nonsymmetric cones, then back off from full steps slightly
        // so that centrality checks and logarithms don't fail right at the boundaries
        if !all_symmetric {
            let ceil = T::one() - sq_root(T::epsilon());
            alpha = tmin(alpha, ceil);
        }

        // Force asymmetric cones last.
        alpha = innerfcn(alpha, false);

        (alpha, alpha)
    }

    fn compute_barrier(&mut self, z: &[T], s: &[T], dz: &[T], ds: &[T], alpha: T) -> T {
        let mut barrier = T::zero();
        for (cone, rng) in zip(&mut self.cones, &self.rng_cones) {
            let zi = &z[rng.clone()];
            let si = &s[rng.clone()];
            let dzi = &dz[rng.clone()];
            let dsi = &ds[rng.clone()];
            barrier += cone.compute_barrier(zi, si, dzi, dsi, alpha);
        }
        barrier
    }
}
