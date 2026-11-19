//! Bounded reader for statement documents: the JSON subset the canonical form uses
//! (objects, arrays and strings of printable ASCII without escapes). Anything else fails
//! closed with `malformed{detail}`; insignificant whitespace is read and then refused by
//! the canonical byte comparison as `non_canonical`.

use fid_kci::kci::json::Json;

use super::StmtError;

/// Deepest nesting of arrays and objects; the statement schema needs 3.
pub(crate) const MAX_DEPTH: usize = 8;

fn bad(detail: &'static str) -> StmtError {
    StmtError::Malformed { detail }
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

/// Reads one value spanning the whole input.
pub(crate) fn read(input: &[u8]) -> Result<Json, StmtError> {
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

    fn expect(&mut self, c: u8) -> Result<(), StmtError> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(bad("syntax"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, StmtError> {
        match self.peek() {
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'[') | Some(b'{') if depth >= MAX_DEPTH => Err(bad("too_deep")),
            Some(b'[') => self.array(depth + 1),
            Some(b'{') => self.object(depth + 1),
            Some(_) => Err(bad("unsupported_value")),
            None => Err(bad("syntax")),
        }
    }

    fn string(&mut self) -> Result<String, StmtError> {
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
        let s = self.b.get(start..self.i).unwrap_or_default();
        self.i += 1;
        // Only bytes 0x20..=0x7e were accepted, so this is always valid UTF-8.
        std::str::from_utf8(s).map(str::to_owned).map_err(|_| bad("not_ascii_text"))
    }

    fn array(&mut self, depth: usize) -> Result<Json, StmtError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
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

    fn object(&mut self, depth: usize) -> Result<Json, StmtError> {
        self.expect(b'{')?;
        let mut members: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Obj(members));
        }
        loop {
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
        if keys.windows(2).any(|w| w[0] == w[1]) {
            return Err(bad("duplicate_key"));
        }
        Ok(Json::Obj(members))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn detail(input: &str) -> Option<&'static str> {
        read(input.as_bytes()).err().and_then(StmtError::field)
    }

    #[test]
    fn refuses_what_the_canonical_form_never_holds() {
        assert_eq!(detail(r#"{"a":1}"#), Some("unsupported_value"));
        assert_eq!(detail(r#"{"a":true}"#), Some("unsupported_value"));
        assert_eq!(detail(r#"{"a":"\n"}"#), Some("escape"));
        assert_eq!(detail("{\"a\":\"\u{e9}\"}"), Some("not_ascii_text"));
        assert_eq!(detail(r#"{"a":"x","a":"y"}"#), Some("duplicate_key"));
        assert_eq!(detail(r#"{"a":"x"} x"#), Some("trailing_data"));
        assert_eq!(detail(r#"{"a":"x",}"#), Some("syntax"));
        assert_eq!(detail(r#"["x""#), Some("syntax"));
        assert_eq!(detail(""), Some("syntax"));
    }

    #[test]
    fn nesting_is_bounded() {
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(read(ok.as_bytes()).is_ok());
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert_eq!(detail(&deep), Some("too_deep"));
        // Far deeper than the stack could take if the bound were missing.
        assert_eq!(detail(&"[".repeat(1_000_000)), Some("too_deep"));
    }

    #[test]
    fn reads_whitespace_and_nesting() {
        let v = read(b" { \"a\" : [ \"x\" , { } ] } ").unwrap();
        assert_eq!(v, Json::Obj(vec![("a".into(), Json::Arr(vec![Json::Str("x".into()), Json::Obj(vec![])]))]));
    }
}
