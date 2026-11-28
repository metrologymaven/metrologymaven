//! Multivariate propagation of fitted-feature covariances. Spec node `UNC.PROP.MVAR`.
//!
//! Implemented leaves: `UNC.PROP.MVAR.PARAM` (the [`FeatureCovariance`] type and the
//! `fcov-param@1` parameterisations, see [`param`]) and `UNC.PROP.MVAR.VMCM` (joint Monte
//! Carlo sample mean, covariance and Mahalanobis coverage region of a characteristic group,
//! see [`vmcm`]).

pub mod param;
pub mod vmcm;

pub use param::{
    check_covariance, from_param, tangent_basis, to_param, ConeReference, CovarianceCheck, CovarianceDefect,
    FeatureCovariance, FeatureType, FromParam, ParamError, TangentBasis, ToParam, ASYM_REL_TOL, MAX_DIM, PSD_REL_TOL,
    UNIT_TOL, VERSION as FCOV_PARAM_VERSION,
};
pub use vmcm::{
    propagate_vector_mcm, CovarianceRefusal, SampleRegion, VectorMcmResult, VmcmError,
    ALGORITHM_ID as VMCM_ALGORITHM_ID,
};
