//! RFC 6901 JSON pointers into a parameter document, ordered in document order.
//!
//! Document order is the order of the RFC 8785 canonical form, the form the journal
//! stores: a value comes before everything inside it; array elements by index; object
//! members by their names compared as UTF-16 code units (RFC 8785 §3.2.3).

use std::cmp::Ordering;
use std::fmt;

/// One step of a pointer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Token {
    /// An array index.
    Index(usize),
    /// An object member name.
    Key(String),
}

/// A JSON pointer into the parameter document (`""` is the whole document).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct JsonPointer(Vec<Token>);

/// Compares two member names as UTF-16 code units (RFC 8785 member order).
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

impl JsonPointer {
    /// The pointer to the whole document.
    pub fn root() -> Self {
        JsonPointer(Vec::new())
    }

    /// This pointer extended by an object member.
    pub fn key(&self, name: &str) -> Self {
        let mut t = self.0.clone();
        t.push(Token::Key(name.to_string()));
        JsonPointer(t)
    }

    /// This pointer extended by an array index.
    pub fn index(&self, i: usize) -> Self {
        let mut t = self.0.clone();
        t.push(Token::Index(i));
        JsonPointer(t)
    }

    /// The tokens, root first.
    pub fn tokens(&self) -> &[Token] {
        &self.0
    }

    /// Number of tokens.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether this is the root pointer.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Document order (see the module docs). A pointer comes before every pointer below it.
    pub fn doc_cmp(&self, other: &JsonPointer) -> Ordering {
        for (a, b) in self.0.iter().zip(other.0.iter()) {
            let o = match (a, b) {
                (Token::Index(x), Token::Index(y)) => x.cmp(y),
                (Token::Key(x), Token::Key(y)) => cmp_utf16(x, y),
                // Never both under one parent; a fixed order keeps the comparison total.
                (Token::Index(_), Token::Key(_)) => Ordering::Less,
                (Token::Key(_), Token::Index(_)) => Ordering::Greater,
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        self.0.len().cmp(&other.0.len())
    }
}

impl fmt::Display for JsonPointer {
    /// The RFC 6901 string: `~` as `~0`, `/` as `~1`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for t in &self.0 {
            f.write_str("/")?;
            match t {
                Token::Index(i) => write!(f, "{i}")?,
                Token::Key(k) => f.write_str(&k.replace('~', "~0").replace('/', "~1"))?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn display_escapes() {
        let p = JsonPointer::root().key("a/b").key("c~d").index(3);
        assert_eq!(p.to_string(), "/a~1b/c~0d/3");
        assert_eq!(JsonPointer::root().to_string(), "");
    }

    #[test]
    fn document_order() {
        let r = JsonPointer::root();
        assert_eq!(r.doc_cmp(&r.key("a")), Ordering::Less);
        assert_eq!(r.key("b").doc_cmp(&r.key("a").key("z")), Ordering::Greater);
        assert_eq!(r.index(2).doc_cmp(&r.index(10)), Ordering::Less);
        // U+FF61 sorts after U+1F600 in UTF-8 byte order but before it in UTF-16.
        assert_eq!(cmp_utf16("\u{ff61}", "\u{1f600}"), Ordering::Greater);
        assert_eq!("\u{ff61}".cmp("\u{1f600}"), Ordering::Less);
    }
}
