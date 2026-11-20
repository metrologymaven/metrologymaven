use super::{auto::*, qdldl::*};

use crate::{
    algebra::{CscMatrix, MatrixTriangle},
    solver::{
        core::kktsolvers::direct::{BoxedDirectLDLSolver, DirectLDLSolverReqs},
        CoreSettings,
    },
};

// The Julia version implements the mapping from user setting
// to LDL solver implementation using dynamic dispatch on
// ::Val{:qdldl} types of arguments.   There is no equivalent
// in Rust, so we get these big match statements.

type LDLConstructor =
    fn(&CscMatrix, &[i8], &CoreSettings, Option<Vec<usize>>) -> BoxedDirectLDLSolver;

/// The KKT matrix shape and LDL solver constructor for the configured method.
pub fn get_ldlsolver_config(settings: &CoreSettings) -> (MatrixTriangle, LDLConstructor) {
    let ldlptr: LDLConstructor;
    let kktshape: MatrixTriangle;
    let case = settings.direct_solve_method.as_str();

    match case {
        "qdldl" => {
            kktshape = QDLDLDirectLDLSolver::required_matrix_shape();
            ldlptr = |M, D, S, P| Box::new(QDLDLDirectLDLSolver::new(M, D, S, P));
        }
        // "auto"; settings validation rejects every other value (upstream panicked here)
        _ => {
            kktshape = AutoDirectLDLSolver::required_matrix_shape();
            ldlptr = |M, D, S, P| AutoDirectLDLSolver::new(M, D, S, P);
        }
    }
    (kktshape, ldlptr)
}
