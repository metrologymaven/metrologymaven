//! Bounded reader for model documents: objects, arrays and strings of printable ASCII
//! without escapes, with insignificant whitespace. Anything else fails closed with
//! `schema_invalid{json.<detail>}`; nesting deeper than [`MAX_DEPTH`] fails `too_large`.
//! Iterative, so the call stack does not grow with the input.

use fid_kci::kci::json::Json;

use super::{invalid, SchemaError, MAX_DEPTH};

/// A container being filled.
enum Frame {
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>, Option<String>),
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

/// Reads one value spanning the whole input.
pub(crate) fn read(input: &[u8]) -> Result<Json, SchemaError> {
    let mut r = Reader { b: input, i: 0 };
    r.ws();
    let v = r.value()?;
    r.ws();
    if r.i != r.b.len() {
        return Err(invalid("json.trailing_data"));
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

    fn string(&mut self) -> Result<String, SchemaError> {
        if self.peek() != Some(b'"') {
            return Err(invalid("json.syntax"));
        }
        self.i += 1;
        let start = self.i;
        loop {
            match self.peek() {
                Some(b'"') => break,
                Some(b'\\') => return Err(invalid("json.escape")),
                Some(0x20..=0x7e) => self.i += 1,
                Some(_) => return Err(invalid("json.text")),
                None => return Err(invalid("json.syntax")),
            }
        }
        let s = self.b.get(start..self.i).unwrap_or_default();
        self.i += 1;
        // Only bytes 0x20..=0x7e were accepted, so this is always valid UTF-8.
        std::str::from_utf8(s).map(str::to_owned).map_err(|_| invalid("json.text"))
    }

    /// Reads the start of a value: a complete string, or an opened container.
    fn start(&mut self, depth: usize) -> Result<Result<Json, Frame>, SchemaError> {
        match self.peek() {
            Some(b'"') => Ok(Ok(Json::Str(self.string()?))),
            Some(b'[' | b'{') if depth >= MAX_DEPTH => Err(SchemaError::TooLarge),
            Some(b'[') => {
                self.i += 1;
                Ok(Err(Frame::Arr(Vec::new())))
            }
            Some(b'{') => {
                self.i += 1;
                Ok(Err(Frame::Obj(Vec::new(), None)))
            }
            Some(_) => Err(invalid("json.value")),
            None => Err(invalid("json.syntax")),
        }
    }

    /// Reads a key and its colon inside an object.
    fn key(&mut self) -> Result<String, SchemaError> {
        self.ws();
        let k = self.string()?;
        self.ws();
        if self.peek() != Some(b':') {
            return Err(invalid("json.syntax"));
        }
        self.i += 1;
        self.ws();
        Ok(k)
    }

    fn value(&mut self) -> Result<Json, SchemaError> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut done = match self.start(0)? {
            Ok(v) => return Ok(v),
            Err(f) => {
                stack.push(f);
                None
            }
        };
        // Each turn either finishes the innermost container or reads one more member.
        loop {
            if let Some(v) = done.take() {
                match stack.last_mut() {
                    Some(Frame::Arr(items)) => items.push(v),
                    Some(Frame::Obj(members, key)) => match key.take() {
                        Some(k) => members.push((k, v)),
                        None => return Err(invalid("json.syntax")),
                    },
                    None => return Ok(v),
                }
                self.ws();
                let close = match stack.last() {
                    Some(Frame::Arr(_)) => b']',
                    _ => b'}',
                };
                match self.peek() {
                    Some(b',') => self.i += 1,
                    Some(c) if c == close => {
                        self.i += 1;
                        done = Some(close_frame(stack.pop())?);
                        continue;
                    }
                    _ => return Err(invalid("json.syntax")),
                }
            } else {
                // Just opened: allow an empty container.
                self.ws();
                let close = match stack.last() {
                    Some(Frame::Arr(_)) => b']',
                    _ => b'}',
                };
                if self.peek() == Some(close) {
                    self.i += 1;
                    done = Some(close_frame(stack.pop())?);
                    continue;
                }
            }
            // Read the next member.
            self.ws();
            if let Some(Frame::Obj(_, key)) = stack.last_mut() {
                let k = self.key()?;
                *key = Some(k);
            }
            match self.start(stack.len())? {
                Ok(v) => done = Some(v),
                Err(f) => stack.push(f),
            }
        }
    }
}

fn close_frame(f: Option<Frame>) -> Result<Json, SchemaError> {
    match f {
        Some(Frame::Arr(items)) => Ok(Json::Arr(items)),
        Some(Frame::Obj(members, None)) => {
            let mut keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
            keys.sort_unstable();
            if keys.windows(2).any(|w| w[0] == w[1]) {
                return Err(invalid("json.duplicate_key"));
            }
            Ok(Json::Obj(members))
        }
        _ => Err(invalid("json.syntax")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn err(input: &str) -> SchemaError {
        read(input.as_bytes()).err().unwrap()
    }

    fn field(input: &str) -> Option<&'static str> {
        err(input).field()
    }

    #[test]
    fn refuses_everything_outside_the_subset() {
        assert_eq!(field(r#"{"a":1}"#), Some("json.value"));
        assert_eq!(field(r#"{"a":true}"#), Some("json.value"));
        assert_eq!(field(r#"{"a":null}"#), Some("json.value"));
        assert_eq!(field(r#"{"a":"\n"}"#), Some("json.escape"));
        assert_eq!(field(concat!("{\"a\":\"", "\\", "u00e9\"}")), Some("json.escape"));
        assert_eq!(field("{\"a\":\"\u{e9}\"}"), Some("json.text"));
        assert_eq!(field("{\"a\":\"x\u{202e}y\"}"), Some("json.text"));
        assert_eq!(field("{\"a\":\"x\ty\"}"), Some("json.text"));
        assert_eq!(field("{\"a\u{7f}\":\"x\"}"), Some("json.text"));
        assert_eq!(field(r#"{"a":"x","a":"y"}"#), Some("json.duplicate_key"));
        assert_eq!(field(r#"{"a":"x"} x"#), Some("json.trailing_data"));
        assert_eq!(field(r#"{"a":"x",}"#), Some("json.syntax"));
        assert_eq!(field(r#"["x",]"#), Some("json.value"));
        assert_eq!(field(r#"["x""#), Some("json.syntax"));
        assert_eq!(field(r#"{"a" "x"}"#), Some("json.syntax"));
        assert_eq!(field(r#"{"a":"x"]"#), Some("json.syntax"));
        assert_eq!(field(r#"["x"}"#), Some("json.syntax"));
        assert_eq!(field("\u{feff}{}"), Some("json.value"));
        assert_eq!(field(""), Some("json.syntax"));
        assert_eq!(field("{"), Some("json.syntax"));
    }

    #[test]
    fn depth_is_bounded_at_32() {
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(read(ok.as_bytes()).is_ok());
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert_eq!(err(&deep), SchemaError::TooLarge);
        let deep_obj = format!("{}\"x\"{}", "{\"a\":".repeat(MAX_DEPTH + 1), "}".repeat(MAX_DEPTH + 1));
        assert_eq!(err(&deep_obj), SchemaError::TooLarge);
        assert_eq!(err(&"[".repeat(1_000_000)), SchemaError::TooLarge);
    }

    #[test]
    fn reads_whitespace_nesting_and_member_order() {
        let v = read(b" { \"b\" : [ \"x\" , { } , [ ] ] , \"a\":\"y\" } ").unwrap();
        assert_eq!(
            v,
            Json::Obj(vec![
                ("b".into(), Json::Arr(vec![Json::Str("x".into()), Json::Obj(vec![]), Json::Arr(vec![])])),
                ("a".into(), Json::Str("y".into())),
            ])
        );
        assert_eq!(read(b"\"s\"").unwrap(), Json::Str("s".into()));
    }
}
