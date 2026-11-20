use super::*;
use crate::solver::core::{
    cones::{CompositeCone, SupportedConeT},
    kktsolvers::HasLinearSolverInfo,
    traits::ProblemData,
    SettingsError, Solver,
};
use crate::solver::traits::Settings;

use crate::algebra::*;

/// Solver for problems in standard conic program form
pub type DefaultSolver = Solver<
    DefaultProblemData,
    DefaultVariables,
    DefaultResiduals,
    DefaultKKTSystem,
    CompositeCone,
    DefaultInfo,
    DefaultSolution,
    DefaultSettings,
>;

/// Error types returned by the DefaultSolver

#[derive(Debug)]
/// Error type returned by settings validation
pub enum SolverError {
    /// An error attributable to one of the fields
    BadInputData(&'static str),

    /// Error from settings validation with details
    SettingsError(SettingsError),
}

impl std::fmt::Display for SolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolverError::BadInputData(x) => write!(f, "Bad input data: {x}"),
            SolverError::SettingsError(x) => write!(f, "Bad settings: {x}"),
        }
    }
}

impl std::error::Error for SolverError {}

impl From<SettingsError> for SolverError {
    fn from(e: SettingsError) -> Self {
        SolverError::SettingsError(e)
    }
}

impl DefaultSolver {
    pub fn new(
        P: &CscMatrix,
        q: &[T],
        A: &CscMatrix,
        b: &[T],
        cones: &[SupportedConeT],
        settings: DefaultSettings,
    ) -> Result<Self, SolverError> {
        //sanity check problem dimensions and data (KNP-1: no NaN, CSC format checked
        //here so that the internal code cannot panic on malformed input)
        check_dimensions(P, q, A, b, cones)?;
        check_data(P, q, A, b)?;
        //sanity check settings
        settings.validate()?;

        let mut info = DefaultInfo::new();

        // user facing results go here.
        let solution = DefaultSolution::new(A.n, A.m);

        // presolve if needed, then take an internal copy of the problem data
        let mut data = DefaultProblemData::new(P, q, A, b, cones, &settings);

        let cones = CompositeCone::new(&data.cones);
        if cones.numel != data.m {
            return Err(SolverError::BadInputData(
                "Constraint dimensions inconsistent with size of cones",
            ));
        }
        let variables = DefaultVariables::new(data.n, data.m);
        let residuals = DefaultResiduals::new(data.n, data.m);

        // equilibrate problem data immediately on setup.
        // this prevents multiple equlibrations if solve!
        // is called more than once.
        data.equilibrate(&cones, &settings);

        let kktsystem = DefaultKKTSystem::new(&data, &cones, &settings);
        info.linsolver = kktsystem.linear_solver_info();

        // work variables for assembling step direction LHS/RHS
        let step_rhs = DefaultVariables::new(data.n, data.m);
        let step_lhs = DefaultVariables::new(data.n, data.m);
        let prev_vars = DefaultVariables::new(data.n, data.m);

        let output = Self {
            data,
            variables,
            residuals,
            kktsystem,
            step_lhs,
            step_rhs,
            prev_vars,
            info,
            solution,
            cones,
            settings,
            phantom: std::marker::PhantomData,
        };

        Ok(output)
    }
}

fn check_dimensions(
    P: &CscMatrix,
    q: &[T],
    A: &CscMatrix,
    b: &[T],
    cone_types: &[SupportedConeT],
) -> Result<(), SolverError> {
    let m = b.len();
    let n = q.len();
    let p = cone_types.iter().fold(0, |acc, cone| acc + cone.nvars());

    if m != A.nrows() {
        return Err(SolverError::BadInputData("A and b incompatible dimensions"));
    }
    if p != m {
        return Err(SolverError::BadInputData(
            "Constraint dimensions inconsistent with size of cones",
        ));
    }
    if n != A.ncols() {
        return Err(SolverError::BadInputData("A and q incompatible dimensions"));
    }
    if n != P.ncols() {
        return Err(SolverError::BadInputData("P and q incompatible dimensions"));
    }
    if !P.is_square() {
        return Err(SolverError::BadInputData("P not square"));
    }

    Ok(())
}

// KNP-1 input check (patch 0003): matrices in valid CSC form, no NaN anywhere, finite
// P, A and q. b may hold infinite bounds (removed by the presolver or capped at the
// infinity bound), but not NaN.
fn check_data(P: &CscMatrix, q: &[T], A: &CscMatrix, b: &[T]) -> Result<(), SolverError> {
    if P.check_format().is_err() || A.check_format().is_err() {
        return Err(SolverError::BadInputData(
            "P or A is not a valid CSC matrix",
        ));
    }
    if !P
        .nzval
        .iter()
        .chain(A.nzval.iter())
        .chain(q.iter())
        .all(|x| x.is_finite())
    {
        return Err(SolverError::BadInputData(
            "P, A or q has a NaN or infinite entry",
        ));
    }
    if b.iter().any(|x| x.is_nan()) {
        return Err(SolverError::BadInputData("b has a NaN entry"));
    }
    Ok(())
}
