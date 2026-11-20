//! Bounded JSON reader for rule and catalog documents: the subset the kernel writer emits
//! (objects, arrays and strings of printable ASCII without escapes), plus insignificant
//! whitespace. Anything else fails closed with `malformed{detail}`: numbers, literals,
//! escapes, non-ASCII text, duplicate keys, trailing data, more than [`MAX_DEPTH`] levels,
//! more than [`MAX_ITEMS`] members or items in one container, or more than
//! [`MAX_INPUT_BYTES`] bytes. Nothing recurses deeper than [`MAX_DEPTH`].

pub use fid_kci::kci::json::Json;

use super::DecError;

/// Largest document read, in bytes.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Deepest nesting of arrays and objects (catalog documents need 4).
pub const MAX_DEPTH: usize = 8;
/// Most members of one object or items of one array.
pub const MAX_ITEMS: usize = 1024;

fn bad(detail: &'static str) -> DecError {
    DecError::Malformed { detail }
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

/// Reads one value spanning the whole input.
pub fn read(input: &[u8]) -> Result<Json, DecError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(bad("too_large"));
    }
    let mut r = Reader { b: input, i: 0 };
    r.ws();
    let v = r.value(0)?;
    r.ws();
    if r.i != r.b.len() {
        return Err(bad("trailing_data"));
    }
    Ok(v)
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), DecError> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(bad("syntax"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, DecError> {
        match self.peek() {
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'[') | Some(b'{') if depth >= MAX_DEPTH => Err(bad("too_deep")),
            Some(b'[') => self.array(depth + 1),
            Some(b'{') => self.object(depth + 1),
            Some(_) => Err(bad("unsupported_value")),
            None => Err(bad("syntax")),
        }
    }

    fn string(&mut self) -> Result<String, DecError> {
        self.expect(b'"')?;
        let start = self.i;
        loop {
            match self.peek() {
                Some(b'"') => break,
                Some(b'\\') => return Err(bad("escape")),
                Some(0x20..=0x7e) => self.i += 1,
                Some(_) => return Err(bad("not_ascii_text")),
                None => return Err(bad("syntax")),
            }
        }
        let s = self.b.get(start..self.i).ok_or_else(|| bad("syntax"))?;
        self.i += 1;
        // Only bytes 0x20..=0x7e were accepted, so this is always valid UTF-8.
        std::str::from_utf8(s).map(str::to_owned).map_err(|_| bad("not_ascii_text"))
    }

    fn array(&mut self, depth: usize) -> Result<Json, DecError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            if items.len() >= MAX_ITEMS {
                return Err(bad("too_many_items"));
            }
            self.ws();
            items.push(self.value(depth)?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(bad("syntax")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, DecError> {
        self.expect(b'{')?;
        let mut members: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Obj(members));
        }
        loop {
            if members.len() >= MAX_ITEMS {
                return Err(bad("too_many_items"));
            }
            self.ws();
            let k = self.string()?;
            self.ws();
            self.expect(b':')?;
            self.ws();
            let v = self.value(depth)?;
            members.push((k, v));
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    break;
                }
                _ => return Err(bad("syntax")),
            }
        }
        let mut keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        if keys.windows(2).any(|w| w.first() == w.get(1)) {
            return Err(bad("duplicate_key"));
        }
        Ok(Json::Obj(members))
    }
}

/// The member `key` of an object's members, if present.
pub(crate) fn member<'a>(members: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    members.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Checks that an object has exactly the members `keys`: an unknown member first
/// (`malformed{unknown_member}`), then a missing one (`malformed{missing_member}`).
pub(crate) fn exact_members(members: &[(String, Json)], keys: &[&str]) -> Result<(), DecError> {
    if members.iter().any(|(k, _)| !keys.contains(&k.as_str())) {
        return Err(bad("unknown_member"));
    }
    if keys.iter().any(|k| member(members, k).is_none()) {
        return Err(bad("missing_member"));
    }
    // Every member is known and every key present, so a longer list repeats a key (only
    // possible in a hand-built value; the reader refuses duplicates).
    if members.len() != keys.len() {
        return Err(bad("duplicate_key"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(input: &str) -> Option<&'static str> {
        read(input.as_bytes()).err().and_then(DecError::detail)
    }

    #[test]
    fn refuses_what_the_kernel_writer_never_emits() {
        assert_eq!(detail(r#"{"a":1}"#), Some("unsupported_value"));
        assert_eq!(detail(r#"{"a":null}"#), Some("unsupported_value"));
        assert_eq!(detail(r#"{"a":"\n"}"#), Some("escape"));
        assert_eq!(detail("{\"a\":\"\u{e9}\"}"), Some("not_ascii_text"));
        assert_eq!(detail(r#"{"a":"x","a":"y"}"#), Some("duplicate_key"));
        assert_eq!(detail(r#"{"a":"x"} x"#), Some("trailing_data"));
        assert_eq!(detail(r#"{"a":"x",}"#), Some("syntax"));
        assert_eq!(detail(r#"["x""#), Some("syntax"));
        assert_eq!(detail(""), Some("syntax"));
    }

    #[test]
    fn sizes_are_bounded() {
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(read(ok.as_bytes()).is_ok());
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert_eq!(detail(&deep), Some("too_deep"));
        assert_eq!(detail(&"[".repeat(MAX_INPUT_BYTES + 1)), Some("too_large"));
        let many = format!("[{}\"x\"]", "\"x\",".repeat(MAX_ITEMS));
        assert_eq!(detail(&many), Some("too_many_items"));
        let fits = format!("[{}\"x\"]", "\"x\",".repeat(MAX_ITEMS - 1));
        assert!(read(fits.as_bytes()).is_ok());
    }
}
