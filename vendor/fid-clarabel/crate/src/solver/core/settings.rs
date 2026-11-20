use crate::solver::implementations::default::DefaultSettings;

/// Solver general core settings are the same as in the default solver.
///
/// Go [here](crate::solver::implementations::default::DefaultSettings)
/// to view the complete list.
///
pub type CoreSettings = DefaultSettings;

#[derive(Debug)]
/// Error type returned by settings validation
pub enum SettingsError {
    /// An error attributable to one of the fields
    BadFieldValue(&'static str),
    /// An error thrown when immutable settings are modified within the solver
    ImmutableSetting(&'static str),
    /// a subsolver error of some kind (e.g. not found, no license)
    LinearSolverProblem {
        solver: &'static str,
        problem: &'static str,
    },
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsError::BadFieldValue(x) => write!(f, "Bad value for field \"{x}\""),
            SettingsError::ImmutableSetting(x) => {
                write!(f, "Attempt to modify immutable setting \"{x}\"")
            }
            SettingsError::LinearSolverProblem { solver, problem } => {
                write!(f, "Problem with {solver} solver ({problem})")
            }
        }
    }
}

impl std::error::Error for SettingsError {}
