//! Element identifiers, roles and element references.

use core::fmt;

use super::error::ConstructError;

/// Longest role or parameter name, in bytes.
pub const MAX_NAME_LEN: usize = 32;

/// An opaque 128-bit element ID: the bytes of the element's stable entity UUID
/// (CONSTITUTION §5). The nil ID (all zero) is refused. Its text form is the canonical
/// 36-character lowercase hyphenated UUID, the only spelling `CORE.DEP` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementId([u8; 16]);

impl ElementId {
    /// Wraps the 16 bytes. [`ConstructError::InvalidDecl`] (`element_id`) for the nil ID.
    pub fn from_bytes(bytes: [u8; 16]) -> Result<ElementId, ConstructError> {
        if bytes == [0; 16] {
            return Err(ConstructError::InvalidDecl { field: "element_id" });
        }
        Ok(ElementId(bytes))
    }

    /// Parses the canonical text form (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`, lowercase hex).
    /// Any other spelling (uppercase, braces, no hyphens) and the nil ID are refused with
    /// [`ConstructError::InvalidDecl`] (`element_id`).
    pub fn parse(text: &str) -> Result<ElementId, ConstructError> {
        let bad = ConstructError::InvalidDecl { field: "element_id" };
        let b = text.as_bytes();
        if b.len() != 36 {
            return Err(bad);
        }
        let mut out = [0u8; 16];
        let mut k = 0;
        let mut i = 0;
        while i < 36 {
            if matches!(i, 8 | 13 | 18 | 23) {
                if b[i] != b'-' {
                    return Err(bad);
                }
                i += 1;
                continue;
            }
            let hi = nibble(b[i]).ok_or(bad)?;
            let lo = nibble(b[i + 1]).ok_or(bad)?;
            out[k] = (hi << 4) | lo;
            k += 1;
            i += 2;
        }
        ElementId::from_bytes(out)
    }

    /// The 16 bytes.
    pub fn to_bytes(self) -> [u8; 16] {
        self.0
    }

    /// The canonical text form.
    pub fn to_text(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(36);
        for (i, byte) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                s.push('-');
            }
            s.push(char::from(HEX[usize::from(byte >> 4)]));
            s.push(char::from(HEX[usize::from(byte & 15)]));
        }
        s
    }
}

impl fmt::Display for ElementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Whether `s` is a name: `[a-z][a-z0-9_]*`, at most [`MAX_NAME_LEN`] bytes.
pub(crate) fn name_ok(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_NAME_LEN
        && matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// The role an input plays in a construction (for example `a`, `b`, `line`, `plane`):
/// `[a-z][a-z0-9_]*`, at most [`MAX_NAME_LEN`] bytes. Roles are fixed by each construction's
/// code, so they are `'static`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Role(&'static str);

impl Role {
    /// Checks the grammar. [`ConstructError::InvalidDecl`] (`role`) otherwise.
    pub fn new(name: &'static str) -> Result<Role, ConstructError> {
        if name_ok(name) {
            Ok(Role(name))
        } else {
            Err(ConstructError::InvalidDecl { field: "role" })
        }
    }

    /// The role name.
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

/// A declared input: which element, in which role. This is the handle a construction keeps
/// and uses to read its input from [`super::Inputs`]; an input can be read only through a
/// reference its declaration lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementRef {
    id: ElementId,
    role: Role,
}

impl ElementRef {
    /// Pairs an element with its role.
    pub fn new(id: ElementId, role: Role) -> ElementRef {
        ElementRef { id, role }
    }

    /// The element.
    pub fn id(&self) -> ElementId {
        self.id
    }

    /// The role.
    pub fn role(&self) -> Role {
        self.role
    }
}
