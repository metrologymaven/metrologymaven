//! The single-pass parser. Parameter nesting uses an explicit frame stack, so the call
//! depth is constant whatever the input.

use crate::error::{Bound, StepError};
use crate::text::{self, TextError};
use crate::{BitVec, DanglingRef, Ident, Instance, Lexeme, P21File, Param, ParseOptions, ParseReport, StepText};
use fid_io_reg::{Budget, Position};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const PARAM_SIZE: u64 = std::mem::size_of::<Param>() as u64;

fn pos(at: usize) -> Position {
    Position::byte(u64::try_from(at).unwrap_or(u64::MAX))
}

enum FrameKind {
    Record,
    List,
    Typed(Ident),
}

struct Frame {
    kind: FrameKind,
    items: Vec<Param>,
    at: usize,
}

pub(crate) struct Parser<'s, 'b> {
    src: &'s [u8],
    pos: usize,
    budget: &'b mut Budget,
    opts: &'b ParseOptions,
    names: BTreeMap<Box<str>, Ident>,
    max_depth: u32,
    deepest: u32,
    instances: u64,
}

impl<'s, 'b> Parser<'s, 'b> {
    pub(crate) fn new(src: &'s [u8], budget: &'b mut Budget, opts: &'b ParseOptions) -> Self {
        let max_depth = opts.max_depth.min(budget.limits().depth());
        Parser { src, pos: 0, budget, opts, names: BTreeMap::new(), max_depth, deepest: 0, instances: 0 }
    }

    // ----- small helpers -------------------------------------------------------------

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn malformed_at(at: usize, reason: &'static str) -> StepError {
        StepError::Malformed { at: pos(at), reason, entity: None }
    }

    fn truncated(&self) -> StepError {
        StepError::TruncatedFile { at: pos(self.src.len()) }
    }

    /// `truncated_file` at the end of input, `malformed{reason}` otherwise.
    fn unexpected(&self, reason: &'static str) -> StepError {
        if self.pos >= self.src.len() {
            self.truncated()
        } else {
            Self::malformed_at(self.pos, reason)
        }
    }

    fn unsupported(at: usize, feature: &'static str) -> StepError {
        StepError::FeatureUnsupported { at: pos(at), feature }
    }

    fn limit(bound: Bound, value: u64, at: usize) -> StepError {
        StepError::LimitExceeded { bound, value, at: pos(at) }
    }

    fn token_limit(&self, start: usize) -> Result<(), StepError> {
        if self.pos.saturating_sub(start) > self.opts.max_token_bytes {
            return Err(Self::limit(Bound::Token, self.opts.max_token_bytes as u64, start));
        }
        Ok(())
    }

    /// Skips whitespace and `/* */` comments.
    fn skip_ws(&mut self) -> Result<(), StepError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.pos += 1,
                Some(b'/') if self.src.get(self.pos + 1) == Some(&b'*') => {
                    let body = self.src.get(self.pos + 2..).unwrap_or(&[]);
                    match body.windows(2).position(|w| w == b"*/") {
                        Some(i) => self.pos += 2 + i + 2,
                        None => return Err(self.truncated()),
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn expect(&mut self, b: u8, reason: &'static str) -> Result<(), StepError> {
        self.skip_ws()?;
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.unexpected(reason))
        }
    }

    /// A keyword `!?[A-Za-z_][A-Za-z0-9_]*` (with `-` too if `dash`), at the current
    /// position (no whitespace skipped).
    fn word(&mut self, dash: bool) -> Result<&'s [u8], StepError> {
        let src = self.src;
        let start = self.pos;
        if self.peek() == Some(b'!') {
            self.pos += 1;
        }
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {}
            _ => {
                self.pos = start;
                return Err(self.unexpected("expected_keyword"));
            }
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || (dash && c == b'-') {
                self.pos += 1;
                self.token_limit(start)?;
            } else {
                break;
            }
        }
        Ok(src.get(start..self.pos).unwrap_or(&[]))
    }

    /// Upper-cases and interns a name read by [`Parser::word`].
    fn intern(&mut self, w: &[u8], at: usize) -> Result<Ident, StepError> {
        let up: String = w.iter().map(|&c| char::from(c.to_ascii_uppercase())).collect();
        if let Some(id) = self.names.get(up.as_str()) {
            return Ok(id.clone());
        }
        if !crate::valid_ident(&up) {
            return Err(Self::malformed_at(at, "bad_name"));
        }
        self.budget.charge_alloc(2 * up.len() as u64, pos(at))?;
        let id = Ident(Arc::from(up.as_str()));
        self.names.insert(up.into_boxed_str(), id.clone());
        Ok(id)
    }

    fn name(&mut self) -> Result<Ident, StepError> {
        self.skip_ws()?;
        let at = self.pos;
        let w = self.word(false)?;
        self.intern(w, at)
    }

    fn entity_number(&mut self) -> Result<u64, StepError> {
        let start = self.pos;
        let mut n: u64 = 0;
        while let Some(c) = self.peek() {
            if !c.is_ascii_digit() {
                break;
            }
            n = n
                .checked_mul(10)
                .and_then(|v| v.checked_add(u64::from(c - b'0')))
                .ok_or_else(|| Self::malformed_at(start, "entity_number_out_of_range"))?;
            self.pos += 1;
            self.token_limit(start)?;
        }
        if self.pos == start {
            return Err(self.unexpected("expected_entity_number"));
        }
        Ok(n)
    }

    fn count_instance(&mut self, at: usize) -> Result<(), StepError> {
        if self.instances >= self.opts.max_instances {
            return Err(Self::limit(Bound::Instances, self.opts.max_instances, at));
        }
        self.instances += 1;
        self.budget.charge_elements(1, pos(at))?;
        Ok(())
    }

    // ----- file structure ------------------------------------------------------------

    pub(crate) fn file(mut self) -> Result<P21File, StepError> {
        self.skip_ws()?;
        let start = self.pos;
        if self.word(true)? != b"ISO-10303-21" {
            return Err(Self::malformed_at(start, "not_part21"));
        }
        self.expect(b';', "expected_semicolon")?;
        self.skip_ws()?;
        let start = self.pos;
        if self.word(true)? != b"HEADER" {
            return Err(Self::malformed_at(start, "expected_header"));
        }
        self.expect(b';', "expected_semicolon")?;

        let mut header = Vec::new();
        loop {
            self.skip_ws()?;
            let start = self.pos;
            let w = self.word(false)?;
            if w == b"ENDSEC" {
                self.expect(b';', "expected_semicolon")?;
                break;
            }
            let name = self.intern(w, start)?;
            let params = self.record_params()?;
            self.expect(b';', "expected_semicolon")?;
            self.count_instance(start)?;
            header.push(Instance { number: 0, parts: vec![(name, params)], offset: start as u64 });
        }

        let mut data = BTreeMap::new();
        let mut seen_data = false;
        loop {
            self.skip_ws()?;
            let start = self.pos;
            match self.word(true)? {
                b"DATA" => {
                    self.skip_ws()?;
                    if self.peek() == Some(b'(') {
                        return Err(Self::unsupported(start, "data_section_parameters"));
                    }
                    self.expect(b';', "expected_semicolon")?;
                    self.data_section(&mut data)?;
                    seen_data = true;
                }
                b"END-ISO-10303-21" => {
                    self.expect(b';', "expected_semicolon")?;
                    break;
                }
                b"ANCHOR" => return Err(Self::unsupported(start, "anchor_section")),
                b"REFERENCE" => return Err(Self::unsupported(start, "reference_section")),
                b"SIGNATURE" => return Err(Self::unsupported(start, "signature_section")),
                _ => return Err(Self::malformed_at(start, "unknown_section")),
            }
        }
        if !seen_data {
            return Err(Self::malformed_at(self.pos, "missing_data_section"));
        }
        self.skip_ws()?;
        if self.pos < self.src.len() {
            let start = self.pos;
            if let Ok(b"SIGNATURE") = self.word(true) {
                return Err(Self::unsupported(start, "signature_section"));
            }
            return Err(Self::malformed_at(start, "trailing_data"));
        }

        let dangling = dangling(&data);
        let report = ParseReport { dangling, instances: self.instances, max_depth: self.deepest };
        let names = self.names.into_values().collect();
        Ok(P21File { header, data, report, names })
    }

    fn data_section(&mut self, data: &mut BTreeMap<u64, Instance>) -> Result<(), StepError> {
        loop {
            self.skip_ws()?;
            let start = self.pos;
            match self.peek() {
                None => return Err(self.truncated()),
                Some(b'#') => {
                    let inst = self.instance()?;
                    if data.contains_key(&inst.number) {
                        return Err(StepError::Malformed {
                            at: pos(start),
                            reason: "duplicate_entity",
                            entity: Some(inst.number),
                        });
                    }
                    data.insert(inst.number, inst);
                }
                Some(b'@') => return Err(Self::unsupported(start, "value_instance")),
                Some(_) => {
                    if self.word(true)? == b"ENDSEC" {
                        self.expect(b';', "expected_semicolon")?;
                        return Ok(());
                    }
                    return Err(Self::malformed_at(start, "expected_instance"));
                }
            }
        }
    }

    /// `#n=NAME(...);` or `#n=(A(...)B(...));`, at the `#`.
    fn instance(&mut self) -> Result<Instance, StepError> {
        let start = self.pos;
        self.pos += 1;
        let number = self.entity_number()?;
        self.expect(b'=', "expected_equals")?;
        self.skip_ws()?;
        let mut parts = Vec::new();
        if self.peek() == Some(b'(') {
            self.pos += 1;
            loop {
                self.skip_ws()?;
                match self.peek() {
                    Some(b')') => {
                        self.pos += 1;
                        break;
                    }
                    None => return Err(self.truncated()),
                    Some(_) => {
                        let at = self.pos;
                        let name = self.name()?;
                        let params = self.record_params()?;
                        self.budget.charge_elements(1, pos(at))?;
                        parts.push((name, params));
                    }
                }
            }
            if parts.is_empty() {
                return Err(Self::malformed_at(start, "empty_complex_instance"));
            }
        } else {
            let name = self.name()?;
            let params = self.record_params()?;
            parts.push((name, params));
        }
        self.expect(b';', "expected_semicolon")?;
        self.count_instance(start)?;
        Ok(Instance { number, parts, offset: start as u64 })
    }

    // ----- parameters ----------------------------------------------------------------

    fn push_item(&mut self, stack: &mut [Frame], p: Param, at: usize) -> Result<(), StepError> {
        self.budget.charge_elements(1, pos(at))?;
        self.budget.charge_alloc(PARAM_SIZE, pos(at))?;
        match stack.last_mut() {
            Some(top) => {
                top.items.push(p);
                Ok(())
            }
            None => Err(Self::malformed_at(at, "internal_state")),
        }
    }

    fn open(&mut self, stack: &mut Vec<Frame>, kind: FrameKind, at: usize) -> Result<(), StepError> {
        let nesting = u32::try_from(stack.len().saturating_sub(1)).unwrap_or(u32::MAX);
        if nesting >= self.max_depth {
            return Err(Self::limit(Bound::Depth, u64::from(self.max_depth), at));
        }
        self.deepest = self.deepest.max(nesting + 1);
        stack.push(Frame { kind, items: Vec::new(), at });
        Ok(())
    }

    /// `( params )` of a record, at optional whitespace before the `(`.
    fn record_params(&mut self) -> Result<Vec<Param>, StepError> {
        self.expect(b'(', "expected_open_paren")?;
        let mut stack: Vec<Frame> = vec![Frame { kind: FrameKind::Record, items: Vec::new(), at: self.pos }];
        // True right after `(` or `,`; `after_comma` forbids `)` there.
        let mut want_value = true;
        let mut after_comma = false;
        loop {
            self.skip_ws()?;
            let at = self.pos;
            let c = match self.peek() {
                Some(c) => c,
                None => return Err(self.truncated()),
            };
            let closing = if want_value {
                let empty_ok = !after_comma
                    && stack.last().is_some_and(|f| f.items.is_empty() && !matches!(f.kind, FrameKind::Typed(_)));
                c == b')' && empty_ok
            } else {
                match c {
                    b',' => {
                        if matches!(stack.last().map(|f| &f.kind), Some(FrameKind::Typed(_))) {
                            return Err(Self::malformed_at(at, "typed_arity"));
                        }
                        self.pos += 1;
                        want_value = true;
                        after_comma = true;
                        continue;
                    }
                    b')' => true,
                    _ => return Err(Self::malformed_at(at, "expected_comma_or_close")),
                }
            };

            if closing {
                self.pos += 1;
                let frame = match stack.pop() {
                    Some(f) => f,
                    None => return Err(Self::malformed_at(at, "internal_state")),
                };
                let param = match frame.kind {
                    FrameKind::Record => return Ok(frame.items),
                    FrameKind::List => Param::List(frame.items),
                    FrameKind::Typed(name) => {
                        let mut it = frame.items.into_iter();
                        match (it.next(), it.next()) {
                            (Some(p), None) => Param::Typed(name, Box::new(p)),
                            _ => return Err(Self::malformed_at(frame.at, "typed_arity")),
                        }
                    }
                };
                self.push_item(&mut stack, param, at)?;
                want_value = false;
                after_comma = false;
                continue;
            }

            // A value starts here.
            let value = match c {
                b'(' => {
                    self.open(&mut stack, FrameKind::List, at)?;
                    self.pos += 1;
                    want_value = true;
                    after_comma = false;
                    continue;
                }
                b'$' => {
                    self.pos += 1;
                    Param::Unset
                }
                b'*' => {
                    self.pos += 1;
                    Param::Derived
                }
                b'#' => {
                    self.pos += 1;
                    Param::Ref(self.entity_number()?)
                }
                b'\'' => Param::Str(self.string()?),
                b'"' => Param::Bin(self.binary()?),
                b'.' => Param::Enum(self.enumeration()?),
                b'+' | b'-' | b'0'..=b'9' => self.number()?,
                b'@' => return Err(Self::unsupported(at, "value_instance_reference")),
                c if c.is_ascii_alphabetic() || c == b'_' || c == b'!' => {
                    let w = self.word(false)?;
                    let name = self.intern(w, at)?;
                    self.expect(b'(', "expected_open_paren")?;
                    self.open(&mut stack, FrameKind::Typed(name), at)?;
                    want_value = true;
                    after_comma = false;
                    continue;
                }
                _ => return Err(Self::malformed_at(at, "expected_value")),
            };
            self.push_item(&mut stack, value, at)?;
            want_value = false;
            after_comma = false;
        }
    }

    fn string(&mut self) -> Result<StepText, StepError> {
        let start = self.pos;
        let max = self.opts.max_string_bytes;
        let (s, end) = text::decode(self.src, start + 1, max).map_err(|e| match e {
            TextError::Malformed(reason, at) => Self::malformed_at(at, reason),
            TextError::Truncated => self.truncated(),
            TextError::TooLong(_) => Self::limit(Bound::String, max as u64, start),
            TextError::Unsupported(feature, at) => Self::unsupported(at, feature),
        })?;
        self.budget.charge_alloc(s.len() as u64, pos(start))?;
        self.pos = end;
        Ok(StepText(s.into_boxed_str()))
    }

    fn binary(&mut self) -> Result<BitVec, StepError> {
        let start = self.pos;
        self.pos += 1;
        let mut digits: Vec<u8> = Vec::new();
        loop {
            let b = match self.peek() {
                Some(b) => b,
                None => return Err(self.truncated()),
            };
            if b == b'"' {
                self.pos += 1;
                break;
            }
            let v = match b {
                b'0'..=b'9' => b - b'0',
                b'A'..=b'F' => b - b'A' + 10,
                b'a'..=b'f' => b - b'a' + 10,
                _ => return Err(Self::malformed_at(self.pos, "bad_binary")),
            };
            if digits.len() >= self.opts.max_binary_digits {
                return Err(Self::limit(Bound::Binary, self.opts.max_binary_digits as u64, start));
            }
            digits.push(v);
            self.pos += 1;
        }
        self.budget.charge_alloc(digits.len() as u64, pos(start))?;
        let (unused, hex) = match digits.split_first() {
            Some((&u, rest)) => (u, rest.to_vec()),
            None => return Err(Self::malformed_at(start, "empty_binary")),
        };
        BitVec::new(unused, hex).ok_or_else(|| Self::malformed_at(start, "bad_binary"))
    }

    fn enumeration(&mut self) -> Result<Ident, StepError> {
        self.pos += 1;
        let at = self.pos;
        let w = self.word(false)?;
        if self.peek() != Some(b'.') {
            return Err(self.unexpected("expected_enum_end"));
        }
        self.pos += 1;
        self.intern(w, at)
    }

    fn digits(&mut self, start: usize) -> Result<(), StepError> {
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
            self.token_limit(start)?;
        }
        Ok(())
    }

    fn number(&mut self) -> Result<Param, StepError> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let d0 = self.pos;
        self.digits(start)?;
        if self.pos == d0 {
            return Err(self.unexpected("bad_number"));
        }
        let mut real = false;
        if self.peek() == Some(b'.') {
            real = true;
            self.pos += 1;
            self.digits(start)?;
            if matches!(self.peek(), Some(b'E' | b'e')) {
                self.pos += 1;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                let e0 = self.pos;
                self.digits(start)?;
                if self.pos == e0 {
                    return Err(self.unexpected("bad_exponent"));
                }
            }
        }
        self.token_limit(start)?;
        let lex = self
            .src
            .get(start..self.pos)
            .and_then(|b| std::str::from_utf8(b).ok())
            .ok_or_else(|| Self::malformed_at(start, "bad_number"))?;
        if real {
            let v: f64 = lex.parse().map_err(|_| Self::malformed_at(start, "bad_real"))?;
            if !v.is_finite() {
                return Err(Self::malformed_at(start, "real_out_of_range"));
            }
            self.budget.charge_alloc(lex.len() as u64, pos(start))?;
            Ok(Param::Real(v, Some(Lexeme(Box::from(lex)))))
        } else {
            let v: i64 = lex.parse().map_err(|_| Self::malformed_at(start, "integer_out_of_range"))?;
            Ok(Param::Int(v))
        }
    }
}

/// References to undefined entity numbers, sorted, without repeats. Iterative.
fn dangling(data: &BTreeMap<u64, Instance>) -> Vec<DanglingRef> {
    let mut found = BTreeSet::new();
    let mut stack: Vec<&Param> = Vec::new();
    for (&from, inst) in data {
        for (_, params) in &inst.parts {
            stack.extend(params.iter());
            while let Some(p) = stack.pop() {
                match p {
                    Param::Ref(to) => {
                        if !data.contains_key(to) {
                            found.insert(DanglingRef { from, to: *to });
                        }
                    }
                    Param::List(items) => stack.extend(items.iter()),
                    Param::Typed(_, inner) => stack.push(inner),
                    _ => {}
                }
            }
        }
    }
    found.into_iter().collect()
}
