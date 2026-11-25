//! The error type of this crate.

use std::fmt;

/// Longest input kept in an error, in bytes; longer inputs are cut at a char boundary.
const KEEP: usize = 128;

/// A string that is not a valid value of one of this crate's types. The rejected input
/// is kept (cut to 128 bytes) for messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypesError {
    BadDeterminism(String),
    BadSideEffect(String),
    BadDigest(String),
    BadCommandName(String),
    BadCommandId(String),
    BadPrincipalKind(String),
}

impl TypesError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            TypesError::BadDeterminism(_) => "bad_determinism",
            TypesError::BadSideEffect(_) => "bad_side_effect",
            TypesError::BadDigest(_) => "bad_digest",
            TypesError::BadCommandName(_) => "bad_command_name",
            TypesError::BadCommandId(_) => "bad_command_id",
            TypesError::BadPrincipalKind(_) => "bad_principal_kind",
        }
    }

    /// The rejected input (possibly cut).
    pub fn input(&self) -> &str {
        match self {
            TypesError::BadDeterminism(s)
            | TypesError::BadSideEffect(s)
            | TypesError::BadDigest(s)
            | TypesError::BadCommandName(s)
            | TypesError::BadCommandId(s)
            | TypesError::BadPrincipalKind(s) => s,
        }
    }
}

/// Copies at most [`KEEP`] bytes of `s`, cut at a char boundary.
pub(crate) fn kept(s: &str) -> String {
    if s.len() <= KEEP {
        return s.to_string();
    }
    let mut end = KEEP;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

impl fmt::Display for TypesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.code(), self.input())
    }
}

impl std::error::Error for TypesError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable() {
        let all = [
            (TypesError::BadDeterminism("x".into()), "bad_determinism"),
            (TypesError::BadSideEffect("x".into()), "bad_side_effect"),
            (TypesError::BadDigest("x".into()), "bad_digest"),
            (TypesError::BadCommandName("x".into()), "bad_command_name"),
            (TypesError::BadCommandId("x".into()), "bad_command_id"),
            (TypesError::BadPrincipalKind("x".into()), "bad_principal_kind"),
        ];
        for (e, code) in all {
            assert_eq!(e.code(), code);
            assert_eq!(e.to_string(), format!("{code}: \"x\""));
        }
    }

    #[test]
    fn long_input_is_cut_at_a_char_boundary() {
        assert_eq!(kept("abc"), "abc");
        let long = "é".repeat(100); // 200 bytes
        let k = kept(&long);
        assert!(k.len() <= KEEP && !k.is_empty());
        assert!(long.starts_with(&k));
    }
}
