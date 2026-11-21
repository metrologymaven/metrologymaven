//! The platform identity types shared by the support matrix (`PLAT.MATRIX.FILE`, in
//! `fid-platform`) and the GPU policy: OS family, CPU architecture and dotted numeric
//! versions. Moved here unchanged from `fid-platform::matrix`, which re-exports them.

use std::cmp::Ordering;
use std::fmt;

macro_rules! named_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($var),+ }
        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// The name used in the matrix file.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }
            /// Parses the name used in the matrix file.
            pub fn from_name(s: &str) -> Option<Self> {
                match s { $($s => Some($name::$var),)+ _ => None }
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

named_enum!(
    /// OS family (extensible by schema bump).
    OsFamily { Windows = "windows", Ubuntu = "ubuntu", Debian = "debian", Fedora = "fedora", Rhel = "rhel", Macos = "macos" }
);
named_enum!(
    /// CPU architecture.
    Arch { X86_64 = "x86_64", Aarch64 = "aarch64" }
);
/// A dotted numeric version of 1 to 4 `u32` components. Missing trailing components
/// compare as 0, so `26` == `26.0` and `22.04` == `22.4`.
#[derive(Debug, Clone, Copy)]
pub struct Version {
    parts: [u32; 4],
    len: usize,
}

impl Version {
    /// Parses `"10.0.19041"`-style text: 1 to 4 non-empty runs of ASCII digits separated by
    /// single dots, each fitting in a `u32`.
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = [0u32; 4];
        let mut len = 0;
        for piece in s.split('.') {
            if len == 4 || piece.is_empty() || piece.len() > 10 || !piece.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            parts[len] = piece.parse().ok()?;
            len += 1;
        }
        Some(Version { parts, len })
    }

    /// The numeric components as written.
    pub fn components(&self) -> &[u32] {
        &self.parts[..self.len]
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}
impl Eq for Version {}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.components().iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{p}")?;
        }
        Ok(())
    }
}
