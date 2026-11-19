//! The canonical call encoding read on stdin (spec node KRN.MAN.WASM, Design).
//!
//! One `KernelCall` is the canonical JSON (RFC 8785 rules, members sorted by UTF-16 code
//! units, no whitespace) of
//!
//! ```text
//! {"algorithm":"<id>","inputs":[{"bytes":"<hex>","sha256":"sha256:<64 hex>"},...],"params":"<hex>","seed":"<u64>"}
//! ```
//!
//! where `<id>` is an algorithm ID in its exact form, byte strings are lowercase hex of even
//! length, the seed is a decimal `u64` without leading zeros and the `seed` member is absent
//! when there is no seed. Every value is a string or an array, so the canonical form has no
//! escapes and no numbers. Only these exact bytes are accepted: whitespace, another member
//! order, an unknown member, uppercase hex, a JSON number or trailing bytes are malformed.
//! The call is at most [`MAX_CALL_BYTES`] bytes and has at most [`MAX_INPUTS`] inputs.

use fid_kci::{AlgorithmId, Digest, KernelCall};

/// Largest accepted call, in bytes.
pub const MAX_CALL_BYTES: usize = 64 * 1024 * 1024;
/// Most inputs one call may carry.
pub const MAX_INPUTS: usize = 4096;
/// Longest algorithm ID, in bytes (as the manifest schema).
pub const MAX_ID_BYTES: usize = 128;

/// Why a call was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallError {
    /// Longer than [`MAX_CALL_BYTES`].
    Oversized,
    /// Not the canonical encoding of a call; `offset` is where reading stopped.
    Malformed { offset: usize },
}

impl CallError {
    /// Stable code, written to stdout as `{"error":"<code>"}`.
    pub fn code(&self) -> &'static str {
        match self {
            CallError::Oversized => "oversized_call",
            CallError::Malformed { .. } => "malformed_call",
        }
    }
}

struct Cursor<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn bad<T>(&self) -> Result<T, CallError> {
        Err(CallError::Malformed { offset: self.pos })
    }

    fn eat(&mut self, lit: &[u8]) -> bool {
        if self.b.get(self.pos..).is_some_and(|rest| rest.starts_with(lit)) {
            self.pos += lit.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, lit: &[u8]) -> Result<(), CallError> {
        if self.eat(lit) {
            Ok(())
        } else {
            self.bad()
        }
    }

    /// Bytes up to (not including) the next `"`, at most `max` of them, each `ok`.
    fn until_quote(&mut self, max: usize, ok: fn(u8) -> bool) -> Result<&'a [u8], CallError> {
        let start = self.pos;
        let rest = self.b.get(start..).unwrap_or(&[]);
        let Some(len) = rest.iter().position(|&c| c == b'"') else {
            return self.bad();
        };
        if len > max {
            return self.bad();
        }
        let s = rest.get(..len).unwrap_or(&[]);
        if let Some(i) = s.iter().position(|&c| !ok(c)) {
            self.pos = start + i;
            return self.bad();
        }
        self.pos = start + len;
        Ok(s)
    }

    fn hex(&mut self) -> Result<Vec<u8>, CallError> {
        let start = self.pos;
        let s = self.until_quote(usize::MAX, is_lower_hex)?;
        if s.len() % 2 != 0 {
            self.pos = start;
            return self.bad();
        }
        Ok(s.chunks_exact(2).map(|p| (nibble(p[0]) << 4) | nibble(p[1])).collect())
    }
}

fn is_lower_hex(c: u8) -> bool {
    c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
}

fn nibble(c: u8) -> u8 {
    if c.is_ascii_digit() {
        c - b'0'
    } else {
        c - b'a' + 10
    }
}

fn is_id_byte(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'@')
}

/// A decimal `u64` without leading zeros.
fn decimal_u64(s: &[u8]) -> Option<u64> {
    if s.is_empty() || (s.len() > 1 && s[0] == b'0') {
        return None;
    }
    let mut v: u64 = 0;
    for &c in s {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u64::from(c - b'0'))?;
    }
    Some(v)
}

/// Reads one call from its canonical bytes.
pub fn parse_call(bytes: &[u8]) -> Result<KernelCall, CallError> {
    if bytes.len() > MAX_CALL_BYTES {
        return Err(CallError::Oversized);
    }
    let mut c = Cursor { b: bytes, pos: 0 };
    c.expect(b"{\"algorithm\":\"")?;
    let id_at = c.pos;
    let id_bytes = c.until_quote(MAX_ID_BYTES, is_id_byte)?;
    let id_text = std::str::from_utf8(id_bytes).map_err(|_| CallError::Malformed { offset: id_at })?;
    let algorithm = AlgorithmId::parse(id_text).map_err(|_| CallError::Malformed { offset: id_at })?;
    if algorithm.to_string() != id_text {
        return Err(CallError::Malformed { offset: id_at });
    }
    c.expect(b"\",\"inputs\":[")?;
    let mut inputs = Vec::new();
    if !c.eat(b"]") {
        loop {
            if inputs.len() == MAX_INPUTS {
                return c.bad();
            }
            c.expect(b"{\"bytes\":\"")?;
            let data = c.hex()?;
            c.expect(b"\",\"sha256\":\"sha256:")?;
            let d_at = c.pos;
            let hex = c.until_quote(64, is_lower_hex)?;
            if hex.len() != 64 {
                return Err(CallError::Malformed { offset: d_at });
            }
            let mut wire = Vec::with_capacity(71);
            wire.extend_from_slice(b"sha256:");
            wire.extend_from_slice(hex);
            let text = std::str::from_utf8(&wire).map_err(|_| CallError::Malformed { offset: d_at })?;
            let digest = Digest::parse(text).map_err(|_| CallError::Malformed { offset: d_at })?;
            c.expect(b"\"}")?;
            inputs.push((data, digest));
            if c.eat(b",") {
                continue;
            }
            c.expect(b"]")?;
            break;
        }
    }
    c.expect(b",\"params\":\"")?;
    let params = c.hex()?;
    c.expect(b"\"")?;
    let seed = if c.eat(b",\"seed\":\"") {
        let at = c.pos;
        let s = c.until_quote(20, |b| b.is_ascii_digit())?;
        let v = decimal_u64(s).ok_or(CallError::Malformed { offset: at })?;
        c.expect(b"\"")?;
        Some(v)
    } else {
        None
    };
    c.expect(b"}")?;
    if c.pos != bytes.len() {
        return c.bad();
    }
    Ok(KernelCall { algorithm, params, seed, inputs })
}

fn push_hex(out: &mut Vec<u8>, data: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &b in data {
        out.push(HEX[usize::from(b >> 4)]);
        out.push(HEX[usize::from(b & 0x0f)]);
    }
}

/// Writes a call's canonical bytes (the inverse of [`parse_call`]).
pub fn encode_call(call: &KernelCall) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"{\"algorithm\":\"");
    out.extend_from_slice(call.algorithm.to_string().as_bytes());
    out.extend_from_slice(b"\",\"inputs\":[");
    for (i, (data, digest)) in call.inputs.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(b"{\"bytes\":\"");
        push_hex(&mut out, data);
        out.extend_from_slice(b"\",\"sha256\":\"");
        out.extend_from_slice(digest.as_str().as_bytes());
        out.extend_from_slice(b"\"}");
    }
    out.extend_from_slice(b"],\"params\":\"");
    push_hex(&mut out, &call.params);
    out.push(b'"');
    if let Some(s) = call.seed {
        out.extend_from_slice(b",\"seed\":\"");
        out.extend_from_slice(s.to_string().as_bytes());
        out.push(b'"');
    }
    out.push(b'}');
    out
}
