// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
use super::*;
use crate::algebra::*;
use crate::solver::core::{
    cones::{CompositeCone, Cone, PrimalOrDualCone},
    traits::{Settings, Variables},
    ScalingStrategy, StepDirection,
};

// ---------------
// Variables type for default problem format
// ---------------

/// Standard-form solver type implementing the [`Variables`](crate::solver::core::traits::Variables) trait
pub struct DefaultVariables {
    /// scaled primal variables
    pub x: Vec<T>,
    /// slack variables
    pub s: Vec<T>,
    /// scaled dual variables
    pub z: Vec<T>,
    /// homogenization scalar τ
    pub tau: T,
    /// homogenization scalar κ
    pub kappa: T,
}

impl std::fmt::Debug for DefaultVariables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "x: {:?}\ns: {:?}\nz: {:?}\nτ: {:?}\nκ: {:?}\n",
            self.x, self.s, self.z, self.tau, self.kappa
        )
    }
}

impl DefaultVariables {
    /// Create a new `DefaultVariables` object
    pub fn new(n: usize, m: usize) -> Self {
        let x = vec![T::zero(); n];
        let s = vec![T::zero(); m];
        let z = vec![T::zero(); m];
        let tau = T::one();
        let kappa = T::one();

        Self {
            x,
            s,
            z,
            tau,
            kappa,
        }
    }
}

impl Variables for DefaultVariables {
    type D = DefaultProblemData;
    type R = DefaultResiduals;
    type C = CompositeCone;
    type SE = DefaultSettings;

    fn calc_mu(&mut self, residuals: &DefaultResiduals, cones: &CompositeCone) -> T {
        let denom = (cones.degree() + 1).as_T();
        (residuals.dot_sz + self.tau * self.kappa) / denom
    }

    fn affine_step_rhs(
        &mut self,
        residuals: &DefaultResiduals,
        variables: &Self,
        cones: &CompositeCone,
    ) {
        self.x.copy_from(&residuals.rx);
        self.z.copy_from(&residuals.rz);
        cones.affine_ds(&mut self.s, &variables.s);
        self.tau = residuals.rtau;
        self.kappa = variables.tau * variables.kappa;
    }

    fn combined_step_rhs(
        &mut self,
        residuals: &DefaultResiduals,
        variables: &Self,
        cones: &mut CompositeCone,
        step: &mut Self,
        sigma: T,
        mu: T,
        m: T,
    ) {
        let dotsigmamu = sigma * mu;

        self.x.axpby(T::one() - sigma, &residuals.rx, T::zero()); //self.x  = (1 - σ)*rx
        self.tau = (T::one() - sigma) * residuals.rtau;
        self.kappa = -dotsigmamu + m * step.tau * step.kappa + variables.tau * variables.kappa;

        // ds is different for symmetric and asymmetric cones:
        // Symmetric cones: d.s = λ ◦ λ + W⁻¹Δs ∘ WΔz − σμe
        // Asymmetric cones: d.s = s + σμ*g(z)

        // we want to scale the Mehotra correction in the symmetric
        // case by M, so just scale step_z by M.  This is an unnecessary
        // vector operation (since it amounts to M*z'*s), but it
        // doesn't happen very often
        if m != T::one() {
            step.z.scale(m);
        }

        cones.combined_ds_shift(&mut self.z, &mut step.z, &mut step.s, dotsigmamu);

        //We are relying on d.s = affine_ds already here
        self.s.axpby(T::one(), &self.z, T::one());

        // now we copy the scaled res for rz and d.z is no longer work
        self.z.axpby(T::one() - sigma, &residuals.rz, T::zero());
    }

    fn calc_step_length(
        &self,
        step: &Self,
        cones: &mut CompositeCone,
        settings: &DefaultSettings,
        step_direction: StepDirection,
    ) -> T {
        let alphatau = {
            if step.tau < T::zero() {
                -self.tau / step.tau
            } else {
                T::MAX
            }
        };

        let alphakappa = {
            if step.kappa < T::zero() {
                -self.kappa / step.kappa
            } else {
                T::MAX
            }
        };

        let alpha = [alphatau, alphakappa, T::one()].minimum();
        let (alphaz, alphas) =
            cones.step_length(&step.z, &step.s, &self.z, &self.s, settings.core(), alpha);

        // itself only allows for a single maximum value.
        // To enable split lengths, we need to also pass a
        // tuple of limits to the step_length function of
        // every cone
        let mut alpha = tmin(alphaz, alphas);

        if step_direction == StepDirection::Combined {
            alpha *= settings.core().max_step_fraction;
        }

        alpha
    }

    fn add_step(&mut self, step: &Self, alpha: T) {
        self.x.axpby(alpha, &step.x, T::one());
        self.s.axpby(alpha, &step.s, T::one());
        self.z.axpby(alpha, &step.z, T::one());
        self.tau += alpha * step.tau;
        self.kappa += alpha * step.kappa;
    }

    fn symmetric_initialization(&mut self, cones: &mut CompositeCone) {
        _shift_to_cone_interior(&mut self.s, cones, PrimalOrDualCone::PrimalCone);
        _shift_to_cone_interior(&mut self.z, cones, PrimalOrDualCone::DualCone);

        self.tau = T::one();
        self.kappa = T::one();
    }

    fn unit_initialization(&mut self, cones: &CompositeCone) {
        cones.unit_initialization(&mut self.z, &mut self.s);

        self.x.set(T::zero());
        self.tau = T::one();
        self.kappa = T::one();
    }

    fn copy_from(&mut self, src: &Self) {
        self.x.copy_from(&src.x);
        self.s.copy_from(&src.s);
        self.z.copy_from(&src.z);
        self.tau = src.tau;
        self.kappa = src.kappa;
    }

    fn scale_cones(
        &self,
        cones: &mut CompositeCone,
        mu: T,
        scaling_strategy: ScalingStrategy,
    ) -> bool {
        cones.update_scaling(&self.s, &self.z, mu, scaling_strategy)
    }

    fn barrier(&self, step: &Self, alpha: T, cones: &mut CompositeCone) -> T {
        let central_coef = (cones.degree() + 1).as_T();

        let cur_tau = self.tau + alpha * step.tau;
        let cur_kappa = self.kappa + alpha * step.kappa;

        // compute current μ
        let sz = <[T] as VectorMath>::dot_shifted(&self.z, &self.s, &step.z, &step.s, alpha);
        let mu = (sz + cur_tau * cur_kappa) / central_coef;

        // barrier terms from gap and scalars
        let mut barrier = central_coef * mu.logsafe() - cur_tau.logsafe() - cur_kappa.logsafe();

        // barriers from the cones
        let (z, s) = (&self.z, &self.s);
        let (dz, ds) = (&step.z, &step.s);

        barrier += cones.compute_barrier(z, s, dz, ds, alpha);

        barrier
    }

    fn rescale(&mut self) {
        let scale = tmax(self.tau, self.kappa);
        let invscale = scale.recip();

        self.x.scale(invscale);
        self.z.scale(invscale);
        self.s.scale(invscale);
        self.tau *= invscale;
        self.kappa *= invscale;
    }
}

fn _shift_to_cone_interior(z: &mut [T], cones: &mut CompositeCone, pd: PrimalOrDualCone) {
    let (min_margin, pos_margin) = cones.margins(z, pd);
    let target = tmax(
        T::one(),
        (pos_margin * (0.1).as_T()) / cones.degree().as_T(),
    );

    if min_margin <= T::zero() {
        // at least some component is outside its cone
        // done in two stages since otherwise (1-α) = -α for
        // large α, which makes z exactly 0. (or worse, -0.0 )
        cones.scaled_unit_shift(z, -min_margin, pd);
        cones.scaled_unit_shift(z, target, pd);
    } else if min_margin < target {
        // margin is positive but small.
        cones.scaled_unit_shift(z, target - min_margin, pd);
    } else {
        // good margin, but still shift explicitly by
        // zero to catch any elements in the zero cone
        // that need to be forced to zero
        cones.scaled_unit_shift(z, T::zero(), pd);
    }
}

impl DefaultVariables {
    pub(crate) fn unscale(&mut self, data: &DefaultProblemData, is_infeasible: bool) {
        // if we have an infeasible problem, normalize
        // using κ to get an infeasibility certificate.
        // Otherwise use τ to get an unscaled solution.
        let scaleinv = {
            if is_infeasible {
                T::recip(self.kappa)
            } else {
                T::recip(self.tau)
            }
        };

        // also undo the equilibration
        let d = &data.equilibration.d;
        let (e, einv) = (&data.equilibration.e, &data.equilibration.einv);
        let cinv = T::recip(data.equilibration.c);

        self.x.hadamard(d).scale(scaleinv);
        self.z.hadamard(e).scale(scaleinv * cinv);
        self.s.hadamard(einv).scale(scaleinv);

        self.tau *= scaleinv;
        self.kappa *= scaleinv;
    }

    #[allow(dead_code)]
    pub(crate) fn dims(&self) -> (usize, usize) {
        (self.x.len(), self.s.len())
    }
}
