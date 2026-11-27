//! Multivariate propagation of fitted-feature covariances. Spec node `UNC.PROP.MVAR`.
//!
//! Implemented leaves: `UNC.PROP.MVAR.PARAM` (the [`FeatureCovariance`] type and the
//! `fcov-param@1` parameterisations, see [`param`]).

pub mod param;

pub use param::{
    check_covariance, from_param, tangent_basis, to_param, ConeReference, CovarianceCheck, CovarianceDefect,
    FeatureCovariance, FeatureType, FromParam, ParamError, TangentBasis, ToParam, ASYM_REL_TOL, PSD_REL_TOL, UNIT_TOL,
    VERSION as FCOV_PARAM_VERSION,
};
