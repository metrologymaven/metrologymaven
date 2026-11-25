//! Test support: a small JSON reader for `fixtures/rpt/stats/cap_cases.json` (the kernel crate
//! has no dependencies). Copied from `crates/fid-geo-dist/tests/common/mod.rs`.
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

static NULL: Json = Json::Null;

impl Json {
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(v) => v.iter().find(|(k, _)| k == key).map_or(&NULL, |(_, x)| x),
            _ => &NULL,
        }
    }
    pub fn num(&self) -> f64 {
        match self {
            Json::Num(x) => *x,
            other => panic!("expected a number, got {other:?}"),
        }
    }
    pub fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("expected a string, got {other:?}"),
        }
    }
    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            other => panic!("expected an array, got {other:?}"),
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
    pub fn truthy(&self) -> bool {
        matches!(self, Json::Bool(true))
    }
}

pub fn parse_json(text: &str) -> Json {
    let mut p = Parser { b: text.as_bytes(), i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, p.b.len(), "trailing bytes after JSON value");
    v
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn peek(&self) -> u8 {
        self.b[self.i]
    }
    fn expect(&mut self, c: u8) {
        assert_eq!(self.peek(), c, "JSON: expected '{}' at byte {}", c as char, self.i);
        self.i += 1;
    }
    fn literal(&mut self, word: &str, v: Json) -> Json {
        assert!(self.b[self.i..].starts_with(word.as_bytes()), "JSON: bad literal at byte {}", self.i);
        self.i += word.len();
        v
    }
    fn value(&mut self) -> Json {
        self.ws();
        match self.peek() {
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.peek() == b'}' {
                    self.i += 1;
                    return Json::Obj(v);
                }
                loop {
                    self.ws();
                    let k = self.string();
                    self.ws();
                    self.expect(b':');
                    let x = self.value();
                    v.push((k, x));
                    self.ws();
                    if self.peek() == b',' {
                        self.i += 1;
                    } else {
                        self.expect(b'}');
                        return Json::Obj(v);
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.peek() == b']' {
                    self.i += 1;
                    return Json::Arr(v);
                }
                loop {
                    v.push(self.value());
                    self.ws();
                    if self.peek() == b',' {
                        self.i += 1;
                    } else {
                        self.expect(b']');
                        return Json::Arr(v);
                    }
                }
            }
            b'"' => Json::Str(self.string()),
            b'n' => self.literal("null", Json::Null),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            _ => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.i += 1;
                }
                let s = std::str::from_utf8(&self.b[start..self.i]).unwrap();
                Json::Num(s.parse::<f64>().unwrap_or_else(|_| panic!("JSON: bad number {s:?}")))
            }
        }
    }
    fn string(&mut self) -> String {
        self.expect(b'"');
        let mut out = Vec::new();
        loop {
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).unwrap(),
                b'\\' => {
                    let e = self.b[self.i];
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let h = std::str::from_utf8(&self.b[self.i..self.i + 4]).unwrap();
                            self.i += 4;
                            let ch = char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap();
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => panic!("JSON: bad escape"),
                    }
                }
                _ => out.push(c),
            }
        }
    }
}
