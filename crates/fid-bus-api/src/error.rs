//! The bus error as a surface sees it (REQ-CORE.BUS.API-006, REQ-CORE.BUS.API-007).

use std::fmt;

/// Longest code or reason, in bytes (each byte one of `[a-z0-9_]`).
pub const MAX_CODE_BYTES: usize = 64;
/// Longest detail, in characters (Unicode scalar values): the `Detail` limit of
/// `CORE.BUS.PIPE.ERR` (`fid_bus::MAX_DETAIL_CHARS`).
pub const MAX_DETAIL_CHARS: usize = 2000;
/// The code of [`InvalidApiError`].
pub const INVALID_API_ERROR: &str = "invalid_api_error";

/// The stage that raised an [`ApiError`] (REQ-CORE.BUS.API-006).
///
/// The eight pipeline stages are those of `fid_bus::pipeline::Stage`, in the same order.
/// [`ApiStage::Session`] covers errors raised before the pipeline runs: an unauthenticated
/// channel, or a refused or failed human upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApiStage {
    Session,
    Resolve,
    Validate,
    Authorise,
    Precondition,
    Route,
    Handler,
    Effects,
    Commit,
}

impl ApiStage {
    /// Every stage, `session` first and then the pipeline stages in their fixed order.
    pub const ALL: &'static [ApiStage] = &[
        ApiStage::Session,
        ApiStage::Resolve,
        ApiStage::Validate,
        ApiStage::Authorise,
        ApiStage::Precondition,
        ApiStage::Route,
        ApiStage::Handler,
        ApiStage::Effects,
        ApiStage::Commit,
    ];

    /// The wire name: `session`, `resolve`, `validate`, `authorise`, `precondition`, `route`,
    /// `handler`, `effects` or `commit`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ApiStage::Session => "session",
            ApiStage::Resolve => "resolve",
            ApiStage::Validate => "validate",
            ApiStage::Authorise => "authorise",
            ApiStage::Precondition => "precondition",
            ApiStage::Route => "route",
            ApiStage::Handler => "handler",
            ApiStage::Effects => "effects",
            ApiStage::Commit => "commit",
        }
    }

    /// Parses the wire name (exact, case-sensitive).
    pub fn parse(s: &str) -> Option<ApiStage> {
        ApiStage::ALL.iter().copied().find(|stage| stage.as_str() == s)
    }
}

impl fmt::Display for ApiStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A bus error as a port reports it: the bus error's code, reason and detail strings
/// unchanged, and the stage that raised it (REQ-CORE.BUS.API-006).
///
/// The fields are private and checked once, by [`ApiError::new`]. The detail is untrusted
/// text (CONSTITUTION C6): display it, never parse it or act on it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApiError {
    code: String,
    reason: String,
    detail: String,
    stage: ApiStage,
}

/// Why [`ApiError::new`] built nothing (REQ-CORE.BUS.API-007). Its code is always
/// [`INVALID_API_ERROR`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InvalidApiError {
    field: &'static str,
}

impl InvalidApiError {
    /// The stable code, `invalid_api_error`.
    pub fn code(&self) -> &'static str {
        INVALID_API_ERROR
    }

    /// The first field that failed its check: `code`, `reason` or `detail`.
    pub fn field(&self) -> &'static str {
        self.field
    }
}

impl fmt::Display for InvalidApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{INVALID_API_ERROR}: bad {}", self.field)
    }
}

impl std::error::Error for InvalidApiError {}

/// Whether `s` is 1 to [`MAX_CODE_BYTES`] bytes, each one of `[a-z0-9_]`.
fn is_code(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_CODE_BYTES
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl ApiError {
    /// Builds an error from a bus error's code, reason and detail and the stage that raised
    /// it, keeping the strings unchanged.
    ///
    /// Returns [`InvalidApiError`] and builds nothing if the code or the reason is not 1 to
    /// [`MAX_CODE_BYTES`] bytes of `[a-z0-9_]`, or if the detail is longer than
    /// [`MAX_DETAIL_CHARS`] characters (REQ-CORE.BUS.API-007). Nothing is cut or rewritten.
    pub fn new(code: &str, reason: &str, detail: &str, stage: ApiStage) -> Result<ApiError, InvalidApiError> {
        if !is_code(code) {
            return Err(InvalidApiError { field: "code" });
        }
        if !is_code(reason) {
            return Err(InvalidApiError { field: "reason" });
        }
        if detail.chars().nth(MAX_DETAIL_CHARS).is_some() {
            return Err(InvalidApiError { field: "detail" });
        }
        Ok(ApiError { code: code.to_string(), reason: reason.to_string(), detail: detail.to_string(), stage })
    }

    /// The bus error code, for example `policy`.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The bus error reason, for example `raw_path_refused`.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The untrusted detail text, for display only.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// The stage that raised the error.
    pub fn stage(&self) -> ApiStage {
        self.stage
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{} ({}): {:?}", self.code, self.reason, self.stage, self.detail)
    }
}

impl std::error::Error for ApiError {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn stage_names_round_trip() {
        let names: Vec<&str> = ApiStage::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            names,
            ["session", "resolve", "validate", "authorise", "precondition", "route", "handler", "effects", "commit"]
        );
        for s in ApiStage::ALL {
            assert_eq!(ApiStage::parse(s.as_str()), Some(*s));
        }
        for bad in ["", "Session", "authorize", "pipeline"] {
            assert_eq!(ApiStage::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_first_bad_field_is_named() {
        let field = |c: &str, r: &str, d: &str| ApiError::new(c, r, d, ApiStage::Handler).map_err(|e| e.field());
        assert_eq!(field("", "", ""), Err("code"));
        assert_eq!(field("policy", "Bad", ""), Err("reason"));
        assert_eq!(field("policy", "bad", &"x".repeat(MAX_DETAIL_CHARS + 1)), Err("detail"));
        let e = ApiError::new("policy", "bad", &"x".repeat(MAX_DETAIL_CHARS + 1), ApiStage::Session);
        assert_eq!(
            e.map_err(|e| (e.code(), e.to_string())),
            Err(("invalid_api_error", "invalid_api_error: bad detail".to_string()))
        );
    }

    #[test]
    fn display_quotes_the_untrusted_detail() {
        let e = ApiError::new("not_found", "unknown_command", "no \"x\"\n", ApiStage::Resolve).unwrap();
        assert_eq!(e.to_string(), r#"not_found/unknown_command (resolve): "no \"x\"\n""#);
    }
}
