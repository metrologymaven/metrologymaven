//! Registry command names and IDs (CONSTITUTION §3): `name@major`, e.g. `align.run_bestfit@1`.

use crate::error::{kept, TypesError};
use std::fmt;
use std::str::FromStr;

/// Longest major version, in decimal digits.
const MAJOR_DIGITS: usize = 5;

/// A command name without the major: `[a-z0-9_.]`, starting and ending with a lowercase
/// letter or digit, e.g. `align.run_bestfit`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandName(String);

impl CommandName {
    /// JSON Schema pattern of a command name.
    pub const PATTERN: &'static str = "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?$";

    /// Whether `s` is a valid command name.
    pub fn is_valid(s: &str) -> bool {
        let b = s.as_bytes();
        let edge = |c: Option<&u8>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        edge(b.first())
            && edge(b.last())
            && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'.')
    }

    /// Parses a command name.
    pub fn parse(s: &str) -> Result<CommandName, TypesError> {
        if CommandName::is_valid(s) {
            Ok(CommandName(s.to_string()))
        } else {
            Err(TypesError::BadCommandName(kept(s)))
        }
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A registry command ID, `name@major`: a [`CommandName`], `@`, and 1 to 5 decimal digits.
///
/// The text is kept exactly as parsed (so `x@01` stays `x@01`); [`CommandId::major`]
/// gives the number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandId {
    text: String,
    at: usize,
    major: u32,
}

impl CommandId {
    /// JSON Schema pattern of a command ID.
    pub const PATTERN: &'static str = "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@[0-9]{1,5}$";

    /// Parses `name@major`.
    pub fn parse(s: &str) -> Result<CommandId, TypesError> {
        let bad = || TypesError::BadCommandId(kept(s));
        let (name, major) = s.rsplit_once('@').ok_or_else(bad)?;
        if !CommandName::is_valid(name)
            || major.is_empty()
            || major.len() > MAJOR_DIGITS
            || !major.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(bad());
        }
        let n = major.parse::<u32>().map_err(|_| bad())?;
        Ok(CommandId { text: s.to_string(), at: name.len(), major: n })
    }

    /// Whether `s` is a valid command ID.
    pub fn is_valid(s: &str) -> bool {
        CommandId::parse(s).is_ok()
    }

    /// Builds `name@major`. Fails when `major` has more than 5 digits.
    pub fn new(name: &CommandName, major: u32) -> Result<CommandId, TypesError> {
        CommandId::parse(&format!("{}@{major}", name.as_str()))
    }

    /// The whole ID.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The name part, without `@major`.
    pub fn name(&self) -> &str {
        self.text.get(..self.at).unwrap_or_default()
    }

    /// The major version.
    pub fn major(&self) -> u32 {
        self.major
    }
}

macro_rules! text_impls {
    ($name:ident) => {
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = TypesError;
            fn from_str(s: &str) -> Result<Self, TypesError> {
                $name::parse(s)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
    };
}

text_impls!(CommandName);
text_impls!(CommandId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in ["a", "0", "align.run_321", "a_b.c9"] {
            assert!(CommandName::is_valid(ok), "{ok}");
            assert_eq!(CommandName::parse(ok).map(|n| n.to_string()), Ok(ok.to_string()));
        }
        for bad in ["", "_a", "a_", ".a", "a.", "A", "a-b", "a b", "a@1", "é"] {
            assert_eq!(CommandName::parse(bad).map_err(|e| e.code()), Err("bad_command_name"), "{bad:?}");
        }
    }

    #[test]
    fn ids() {
        let id = CommandId::parse("align.run_321@12").map(|i| (i.name().to_string(), i.major(), i.to_string()));
        assert_eq!(id, Ok(("align.run_321".to_string(), 12, "align.run_321@12".to_string())));
        assert_eq!(CommandId::parse("x@99999").map(|i| i.major()), Ok(99_999));
        assert_eq!(CommandId::parse("x@007").map(|i| (i.major(), i.to_string())), Ok((7, "x@007".to_string())));
        for bad in ["", "@1", "x", "x@", "x@-1", "x@+1", "x@1a", "x@123456", "X@1", "x@@1", "a@b@1", ".x@1"] {
            assert_eq!(CommandId::parse(bad).map_err(|e| e.code()), Err("bad_command_id"), "{bad:?}");
        }
    }

    #[test]
    fn new_formats_major() {
        let name = CommandName::parse("canon.digest");
        assert!(name.is_ok());
        if let Ok(name) = name {
            assert_eq!(CommandId::new(&name, 1).map(|i| i.to_string()), Ok("canon.digest@1".to_string()));
            assert!(CommandId::new(&name, 100_000).is_err());
        }
    }
}
