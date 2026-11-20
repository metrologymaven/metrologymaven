// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
#![allow(non_snake_case)]
use super::*;
use crate::algebra::*;
use crate::solver::core::traits::Residuals;

// ---------------
// Residuals type for default problem format
// ---------------

/// Standard-form solver type implementing the [`Residuals`](crate::solver::core::traits::Residuals) trait
pub struct DefaultResiduals {
    // the main KKT residuals
    pub(crate) rx: Vec<T>,
    pub(crate) rz: Vec<T>,
    pub(crate) rtau: T,

    // partial residuals for infeasibility checks
    pub(crate) rx_inf: Vec<T>,
    pub(crate) rz_inf: Vec<T>,

    // various inner products.
    // NB: these are invariant w.r.t equilibration
    pub(crate) dot_qx: T,
    pub(crate) dot_bz: T,
    pub(crate) dot_sz: T,
    pub(crate) dot_xPx: T,

    // the product Px by itself. Required for infeasibilty checks
    pub(crate) Px: Vec<T>,
}

impl DefaultResiduals {
    /// Create a new `DefaultResiduals` object
    pub fn new(n: usize, m: usize) -> Self {
        let rx = vec![T::zero(); n];
        let rz = vec![T::zero(); m];
        let rtau = T::one();

        let rx_inf = vec![T::zero(); n];
        let rz_inf = vec![T::zero(); m];

        let Px = vec![T::zero(); n];

        Self {
            rx,
            rz,
            rtau,
            rx_inf,
            rz_inf,
            Px,
            dot_qx: T::zero(),
            dot_bz: T::zero(),
            dot_sz: T::zero(),
            dot_xPx: T::zero(),
        }
    }
}

impl Residuals for DefaultResiduals {
    type D = DefaultProblemData;
    type V = DefaultVariables;

    fn update(&mut self, variables: &DefaultVariables, data: &DefaultProblemData) {
        // various products used multiple times
        let qx = data.q.dot(&variables.x);
        let bz = data.b.dot(&variables.z);
        let sz = variables.s.dot(&variables.z);

        //Px = P*x, P treated as symmetric
        let symP = data.P.sym_up();
        symP.symv(&mut self.Px, &variables.x, T::one(), T::zero());

        let xPx = variables.x.dot(&self.Px);

        //partial residual calc so we can check primal/dual
        //infeasibility conditions

        //Same as:
        //rx_inf .= -data.A'* variables.z
        let At = data.A.t();
        At.gemv(&mut self.rx_inf, &variables.z, -T::one(), T::zero());

        //Same as:  residuals.rz_inf .=  data.A * variables.x + variables.s
        self.rz_inf.copy_from(&variables.s);
        let A = &data.A;
        A.gemv(&mut self.rz_inf, &variables.x, T::one(), T::one());

        //complete the residuals
        //rx = rx_inf - Px - qτ
        self.rx.waxpby(-T::one(), &self.Px, -variables.tau, &data.q);
        self.rx.axpby(T::one(), &self.rx_inf, T::one());

        // rz = rz_inf - bτ
        self.rz
            .waxpby(T::one(), &self.rz_inf, -variables.tau, &data.b);

        // τ = qz + bz + κ + xPx/τ;
        self.rtau = qx + bz + variables.kappa + xPx / variables.tau;

        //save local versions
        self.dot_qx = qx;
        self.dot_bz = bz;
        self.dot_sz = sz;
        self.dot_xPx = xPx;
    }
}
