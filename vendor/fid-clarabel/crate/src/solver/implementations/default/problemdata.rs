#![allow(non_snake_case)]

use super::*;
use crate::algebra::*;
use crate::solver::core::{
    cones::{CompositeCone, Cone},
    traits::ProblemData,
};
use crate::solver::SupportedConeT;

// ---------------
// Data type for default problem format
// ---------------

/// Standard-form solver type implementing the [`ProblemData`](crate::solver::core::traits::ProblemData) trait
pub struct DefaultProblemData {
    /// The matrix P in the quadratic objective term
    pub P: CscMatrix,
    /// The vector q in the quadratic objective term
    pub q: Vec<T>,
    /// The matrix A in the constraints
    pub A: CscMatrix,
    /// The vector b in the constraints
    pub b: Vec<T>,
    /// Vector of cones in the problem
    pub cones: Vec<SupportedConeT>,
    /// Number of variables
    pub n: usize,
    /// Number of constraints
    pub m: usize,
    /// Equilibration data for the problem
    pub equilibration: DefaultEquilibrationData,

    // unscaled inf norms of linear terms.  Set to "None"
    // during data updating to allow for multiple updates, and
    // then recalculated during solve if needed
    normq: Option<T>,
    normb: Option<T>,

    pub(crate) presolver: Option<Presolver>,
    dropped_zeros: usize, // number of eliminated structural zeros
}

impl DefaultProblemData {
    /// Create a new `DefaultProblemData` object
    pub fn new(
        P: &CscMatrix,
        q: &[T],
        A: &CscMatrix,
        b: &[T],
        cones: &[SupportedConeT],
        settings: &DefaultSettings,
    ) -> Self {
        // clean up the cones by consolidating repeated NNs,
        // eliminate empty cones, transform singletons etc
        // this makes a locally owned copy of the cones
        let cones = SupportedConeT::new_collapsed(cones);

        // some caution is required to ensure we take a minimal,
        // but nonzero, number of data copies during presolve steps

        let mut P_new: Option<CscMatrix> = None;
        let q_new: Option<Vec<T>> = None;
        let mut A_new: Option<CscMatrix> = None;
        let mut b_new: Option<Vec<T>> = None;
        let mut cones_new: Option<Vec<SupportedConeT>> = None;

        if !P.is_triu() {
            P_new = Some(P.to_triu());
        }

        // presolve : return nothing if disabled or no reduction
        // --------------------------------------
        let presolver = try_presolver(A, b, &cones, settings);

        if let Some(ref presolver) = presolver {
            let (_A_new, _b_new, _cones_new) = presolver.presolve(A, b, &cones);
            (A_new, b_new, cones_new) = (Some(_A_new), Some(_b_new), Some(_cones_new));
        }

        // now make sure we have a clean copy of everything if we
        // haven't made one already.   Necessary since we will scale
        // the internal copy and don't want to step on the user

        let mut P_new = P_new.unwrap_or_else(|| P.clone());
        let q_new = q_new.unwrap_or_else(|| q.to_vec());
        let mut A_new = A_new.unwrap_or_else(|| A.clone());
        let mut b_new = b_new.unwrap_or_else(|| b.to_vec());

        // cones was already copied, so can just pass through without cloning
        let cones_new = cones_new.unwrap_or(cones);

        //cap entries in b at INFINITY.  This is important
        //for inf values that were not in a reduced cone
        //this is not considered part of the "presolve", so
        //can always happen regardless of user settings
        let infbound = crate::get_infinity().as_T();
        b_new.scalarop(|x| tmin(x, infbound));

        // this ensures m is the *reduced* size m
        let (m, n) = A_new.size();

        // explicitly dropzeros on the copied data, since dropzeros
        // operates in place.  PJG: revisit this order of operations
        // once a proper presolver is implemented, since it might
        // be preferable to dropzeros then presolve
        let mut dropped_zeros = 0;
        if settings.input_sparse_dropzeros {
            dropped_zeros += P_new.dropzeros() + A_new.dropzeros();
        }

        let equilibration = DefaultEquilibrationData::new(n, m);

        let normq = Some(q_new.norm_inf());
        let normb = Some(b_new.norm_inf());

        Self {
            P: P_new,
            q: q_new,
            A: A_new,
            b: b_new,
            cones: cones_new,
            n,
            m,
            equilibration,
            normq,
            normb,
            dropped_zeros,
            presolver,
        }
    }

    pub(crate) fn get_normq(&mut self) -> T {
        if let Some(norm) = self.normq {
            norm
        } else {
            let dinv = &self.equilibration.dinv;
            let cinv = T::recip(self.equilibration.c);
            let norm = self.q.norm_inf_scaled(dinv) * cinv;
            self.normq = Some(norm);
            norm
        }
    }

    pub(crate) fn get_normb(&mut self) -> T {
        if let Some(norm) = self.normb {
            norm
        } else {
            let einv = &self.equilibration.einv;
            let norm = self.b.norm_inf_scaled(einv);
            self.normb = Some(norm);
            norm
        }
    }

    pub(crate) fn clear_normq(&mut self) {
        self.normq = None;
    }

    pub(crate) fn clear_normb(&mut self) {
        self.normb = None;
    }

    // data updating not supported following presolve reduction
    pub(crate) fn is_presolved(&self) -> bool {
        self.presolver.is_some()
    }

    // data updating not supported if structural zeros
    // have been eliminated
    pub(crate) fn is_dropped_zeros(&self) -> bool {
        self.dropped_zeros != 0
    }
}

impl ProblemData for DefaultProblemData {
    type V = DefaultVariables;
    type C = CompositeCone;
    type SE = DefaultSettings;

    fn equilibrate(&mut self, cones: &CompositeCone, settings: &DefaultSettings) {
        let data = self;
        let equil = &mut data.equilibration;

        // if equilibration is disabled, just return.  Note that
        // the default equilibration structure initializes with
        // identity scaling already.
        if !settings.equilibrate_enable {
            return;
        }

        // references to scaling matrices from workspace
        let (d, e) = (&mut equil.d, &mut equil.e);

        // use the inverse scalings as work vectors
        let dwork = &mut equil.dinv;
        let ework = &mut equil.einv;

        // references to problem data
        // note that P may be triu, but it shouldn't matter
        let (P, A, q, b) = (&mut data.P, &mut data.A, &mut data.q, &mut data.b);

        let scale_min = settings.equilibrate_min_scaling;
        let scale_max = settings.equilibrate_max_scaling;

        // perform scaling operations for a fixed number of steps
        for _ in 0..settings.equilibrate_max_iter {
            kkt_col_norms(P, A, dwork, ework);

            //zero rows or columns should not get scaled
            dwork.scalarop(|x| if x == T::zero() { T::one() } else { x });
            ework.scalarop(|x| if x == T::zero() { T::one() } else { x });

            dwork.rsqrt();
            ework.rsqrt();

            // bound the cumulative scaling
            for (dwork, &d) in std::iter::zip(dwork.iter_mut(), d.iter()) {
                *dwork = T::clip(dwork, scale_min / d, scale_max / d);
            }
            for (ework, &e) in std::iter::zip(ework.iter_mut(), e.iter()) {
                *ework = T::clip(ework, scale_min / e, scale_max / e);
            }

            // Scale the problem data and update the
            // equilibration matrices
            scale_data(P, A, q, b, Some(dwork), ework);
            d.hadamard(dwork);
            e.hadamard(ework);

            // now use the Dwork array to hold the
            // column norms of the newly scaled P
            // so that we can compute the mean
            P.col_norms(dwork);
            let mean_col_norm_P = dwork.mean();
            let inf_norm_q = q.norm_inf();

            if mean_col_norm_P != T::zero() && inf_norm_q != T::zero() {
                let scale_cost = tmax(inf_norm_q, mean_col_norm_P);
                let ctmp = T::recip(scale_cost);
                let ctmp = T::clip(&ctmp, scale_min / equil.c, scale_max / equil.c);

                // scale the penalty terms and overall scaling
                P.scale(ctmp);
                q.scale(ctmp);
                equil.c *= ctmp;
            }
        } //end Ruiz scaling loop

        // fix scalings in cones for which elementwise
        // scaling can't be applied. Rectification should
        //either do nothing or take a convex combination of
        //scalings over a cone, so shouldn't need to check
        //bounds on the scalings here
        if cones.rectify_equilibration(ework, e) {
            // only rescale again if some cones were rectified
            scale_data(P, A, q, b, None, ework);
            e.hadamard(ework);
        }

        // update the inverse scaling data
        equil.dinv.scalarop_from(T::recip, d);
        equil.einv.scalarop_from(T::recip, e);
    }
}

// ---------------
// utilities
// ---------------

fn kkt_col_norms(P: &CscMatrix, A: &CscMatrix, norm_LHS: &mut [T], norm_RHS: &mut [T]) {
    P.col_norms_sym(norm_LHS); // P can be triu
    A.col_norms_no_reset(norm_LHS); // incrementally from P norms
    A.row_norms(norm_RHS); // same as column norms of A'
}

fn scale_data(
    P: &mut CscMatrix,
    A: &mut CscMatrix,
    q: &mut [T],
    b: &mut [T],
    d: Option<&[T]>,
    e: &[T],
) {
    match d {
        Some(d) => {
            P.lrscale(d, d); // P[:,:] = Ds*P*Ds
            A.lrscale(e, d);
            q.hadamard(d);
        }
        None => {
            A.lscale(e); // A[:,:] = Es*A
        }
    }
    b.hadamard(e);
}

fn try_presolver(
    A: &CscMatrix,
    b: &[T],
    cones: &[SupportedConeT],
    settings: &DefaultSettings,
) -> Option<Presolver> {
    if !settings.presolve_enable {
        return None;
    }

    let presolver = Presolver::new(A, b, cones, settings);

    if !presolver.is_reduced() {
        return None;
    }

    Some(presolver)
}
