//! Determinism classes (CONSTITUTION §4), side-effect classes (CONSTITUTION §3) and principal
//! kinds (CONSTITUTION §3, CORE.BUS.PRIN).

use crate::error::{kept, TypesError};
use std::fmt;
use std::str::FromStr;

macro_rules! class_enum {
    ($(#[$m:meta])* $name:ident, $err:ident { $($(#[$vm:meta])* $variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vm])* $variant,)+
        }

        impl $name {
            /// Every value, in declaration order (which is also the `Ord` order).
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $s,)+
                }
            }

            /// Parses the wire string (exact, case-sensitive).
            pub fn parse(s: &str) -> Result<Self, TypesError> {
                match s {
                    $($s => Ok($name::$variant),)+
                    other => Err(TypesError::$err(kept(other))),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = TypesError;
            fn from_str(s: &str) -> Result<Self, TypesError> {
                Self::parse(s)
            }
        }
    };
}

class_enum!(
    /// Determinism class (CONSTITUTION §4).
    Determinism, BadDeterminism {
        /// Portable bit-exact.
        D0 => "D0",
        /// Per-platform bit-exact.
        D1 => "D1",
        /// Seeded.
        D2 => "D2",
        /// Tolerance-convergent.
        D3 => "D3",
        /// Observed.
        N => "N",
    }
);

class_enum!(
    /// Side-effect class of a command (CONSTITUTION §3).
    SideEffect, BadSideEffect {
        /// Read.
        R0 => "R0",
        /// Compute (pure, scratch result).
        R1 => "R1",
        /// Mutate draft or agent branch.
        W1 => "W1",
        /// Mutate working document (journaled).
        W2 => "W2",
        /// Mutate controlled record (release, publish).
        W3 => "W3",
        /// Egress (data leaves the machine).
        X1 => "X1",
        /// Simulator I/O.
        I1 => "I1",
        /// Device I/O, energized but no motion.
        D1 => "D1",
        /// Physical motion.
        M1 => "M1",
        /// Signing.
        S1 => "S1",
        /// Admin and safety configuration.
        A0 => "A0",
    }
);

class_enum!(
    /// The kind of principal a bus session runs as (CONSTITUTION §3; REQ-CORE.BUS.PRIN-001).
    /// Only the bus derives it; holding a value of this type grants nothing (CORE.BUS.API).
    PrincipalKind, BadPrincipalKind {
        /// A person whose presence the bus has confirmed.
        Human => "human",
        /// An AI agent.
        Agent => "agent",
        /// A headless local caller with no confirmed person (REQ-CORE.BUS.PRIN-003).
        Script => "script",
        /// A service.
        Service => "service",
        /// A device.
        Device => "device",
        /// The application itself.
        System => "system",
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determinism_round_trips() {
        let wire: Vec<&str> = Determinism::ALL.iter().map(|d| d.as_str()).collect();
        assert_eq!(wire, ["D0", "D1", "D2", "D3", "N"]);
        for d in Determinism::ALL {
            assert_eq!(Determinism::parse(d.as_str()), Ok(*d));
            assert_eq!(d.to_string().parse::<Determinism>(), Ok(*d));
        }
        assert!(Determinism::D0 < Determinism::N);
    }

    #[test]
    fn side_effect_round_trips() {
        let wire: Vec<&str> = SideEffect::ALL.iter().map(|d| d.as_str()).collect();
        assert_eq!(wire, ["R0", "R1", "W1", "W2", "W3", "X1", "I1", "D1", "M1", "S1", "A0"]);
        for c in SideEffect::ALL {
            assert_eq!(SideEffect::parse(c.as_str()), Ok(*c));
        }
    }

    #[test]
    fn principal_kind_round_trips() {
        let wire: Vec<&str> = PrincipalKind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(wire, ["human", "agent", "script", "service", "device", "system"]);
        for k in PrincipalKind::ALL {
            assert_eq!(PrincipalKind::parse(k.as_str()), Ok(*k));
            assert_eq!(k.to_string().parse::<PrincipalKind>(), Ok(*k));
        }
        for bad in ["", "Human", "HUMAN", "user", " human", "human "] {
            assert_eq!(PrincipalKind::parse(bad).map_err(|e| e.code()), Err("bad_principal_kind"), "{bad:?}");
        }
    }

    #[test]
    fn unknown_values_are_refused() {
        for bad in ["", "d0", "D4", " D0", "D0 ", "n"] {
            assert_eq!(Determinism::parse(bad).map_err(|e| e.code()), Err("bad_determinism"), "{bad:?}");
        }
        for bad in ["", "r0", "W4", "D0", "X2"] {
            assert_eq!(SideEffect::parse(bad).map_err(|e| e.code()), Err("bad_side_effect"), "{bad:?}");
        }
    }
}
