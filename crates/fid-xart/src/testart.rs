//! Test-only native artifacts (AC-2), compiled only into the binaries `xart-test-artifact` and
//! `xart-test-artifact-reversed` (not part of the library API). They speak the `fid-kernel-wasm` protocol and register the
//! KRN.MAN.WASM stand-in algorithms `test.echo.identity@1.0.0` and `test.stack.recurse@1.0.0`
//! through `fid_kci::evaluate`.
//!
//! The faithful build behaves like the stand-ins. The reversed build is the "deliberately
//! different summation order" of AC-2: the stand-in set has no summing algorithm, so its echo
//! concatenates the inputs in reverse order instead. Its answers therefore differ from the
//! WASM artifact exactly on echo calls with two or more different inputs, and the report must
//! count those as mismatches without failing the gate. Neither binary is a kernel artifact.

use fid_canon::CValue;
use fid_kci::{
    evaluate, parse_card, AlgorithmFailure, AlgorithmId, AlgorithmTable, CardRegistry, Digest, KernelAlgorithm,
    KernelCall, KernelError,
};

use std::collections::BTreeMap;

type Obj = BTreeMap<String, CValue>;

/// Parses canonical JSON (duplicate keys refused); `None` if not canonical.
fn parse_canonical(input: &[u8]) -> Option<CValue> {
    let v = fid_canon::parse_with_limits(input, MAX_CALL_BYTES, 8).ok()?;
    (fid_canon::to_canonical_bytes(&v).ok()? == input).then_some(v)
}

/// The object if it has exactly the `required` keys plus any of the `optional` ones.
fn object<'a>(v: &'a CValue, required: &[&str], optional: &[&str]) -> Option<&'a Obj> {
    let CValue::Obj(m) = v else {
        return None;
    };
    let known = |k: &String| required.contains(&k.as_str()) || optional.contains(&k.as_str());
    (required.iter().all(|k| m.contains_key(*k)) && m.keys().all(known)).then_some(m)
}

fn string<'a>(m: &'a Obj, key: &str) -> Option<&'a str> {
    match m.get(key) {
        Some(CValue::Str(s)) => Some(s),
        _ => None,
    }
}

/// The KRN.MAN.KCI echo: `params | seed-or-"-" | input0 | input1 ...`.
pub const ECHO_ID: &str = "test.echo.identity@1.0.0";
/// Outputs the depth in its parameters (the stand-in recurses to it).
pub const RECURSE_ID: &str = "test.stack.recurse@1.0.0";
/// Largest call read on stdin (the `fid-kernel-wasm` limit).
pub const MAX_CALL_BYTES: usize = 64 * 1024 * 1024;
/// Most inputs in one call (the `fid-kernel-wasm` limit).
pub const MAX_INPUTS: usize = 4096;

/// Input order of the echo algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// As the stand-in: inputs in call order.
    Faithful,
    /// Deliberately divergent: inputs in reverse order.
    Reversed,
}

struct Echo(AlgorithmId, Order);

impl KernelAlgorithm for Echo {
    fn id(&self) -> AlgorithmId {
        self.0.clone()
    }
    fn validate(&self, params: &[u8]) -> Result<(), KernelError> {
        if params.iter().all(|b| (0x20..=0x7e).contains(b)) {
            Ok(())
        } else {
            Err(KernelError::InvalidParams { reason: "params_not_printable_ascii" })
        }
    }
    fn run(&self, params: &[u8], seed: Option<u64>, inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        if params == b"fail" {
            return Err(KernelError::Algorithm(AlgorithmFailure { code: "asked_to_fail" }));
        }
        let mut out = params.to_vec();
        out.push(b'|');
        match seed {
            Some(s) => out.extend_from_slice(s.to_string().as_bytes()),
            None => out.push(b'-'),
        }
        let mut ordered: Vec<&[u8]> = inputs.to_vec();
        if self.1 == Order::Reversed {
            ordered.reverse();
        }
        for x in ordered {
            out.push(b'|');
            out.extend_from_slice(x);
        }
        Ok(out)
    }
}

/// A decimal `u64` without leading zeros.
fn decimal_u64(b: &[u8]) -> Option<u64> {
    let ok = !b.is_empty() && b.len() <= 20 && b.iter().all(u8::is_ascii_digit) && (b == b"0" || b[0] != b'0');
    if ok {
        std::str::from_utf8(b).ok()?.parse().ok()
    } else {
        None
    }
}

struct Recurse(AlgorithmId);

impl KernelAlgorithm for Recurse {
    fn id(&self) -> AlgorithmId {
        self.0.clone()
    }
    fn validate(&self, params: &[u8]) -> Result<(), KernelError> {
        decimal_u64(params).map(|_| ()).ok_or(KernelError::InvalidParams { reason: "params_not_decimal_u64" })
    }
    fn run(&self, params: &[u8], _seed: Option<u64>, _inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        let n = decimal_u64(params).ok_or(KernelError::InvalidParams { reason: "params_not_decimal_u64" })?;
        Ok(n.to_string().into_bytes())
    }
}

fn card(id: &str, title: &str) -> String {
    [
        format!("id = \"{id}\""),
        format!("title = \"{title}\""),
        "method = \"Test algorithm of the cross-artifact gate.\"".to_string(),
        "applicability = \"Tests of fid-xart only.\"".to_string(),
        "references = [\"Spec node KRN.MAN.XART, acceptance criterion AC-2.\"]".to_string(),
        "determinism = \"D0\"".to_string(),
        "failure_modes = [\"Malformed parameters are refused.\"]".to_string(),
        "validation = \"fixtures/man/xart-calls-v1/calls.txt\"".to_string(),
    ]
    .join("\n")
}

/// The test table, or `None` if a card or ID fails to parse (unit-tested not to).
pub fn table(order: Order) -> Option<AlgorithmTable> {
    let echo = AlgorithmId::parse(ECHO_ID).ok()?;
    let recurse = AlgorithmId::parse(RECURSE_ID).ok()?;
    let cards = CardRegistry::new(vec![
        parse_card(&card(ECHO_ID, "Echo test algorithm")).ok()?,
        parse_card(&card(RECURSE_ID, "Depth test algorithm")).ok()?,
    ])
    .ok()?;
    let algorithms: Vec<Box<dyn KernelAlgorithm>> = vec![Box::new(Echo(echo, order)), Box::new(Recurse(recurse))];
    AlgorithmTable::new(algorithms, cards).ok()
}

fn hex(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    b.chunks_exact(2).map(|p| Some((nib(p[0])? << 4) | nib(p[1])?)).collect()
}

/// Parses one canonical call as `fid-kernel-wasm` pins it; `None` if malformed.
pub fn parse_call(input: &[u8]) -> Option<KernelCall> {
    let v = parse_canonical(input)?;
    let m = object(&v, &["algorithm", "inputs", "params"], &["seed"])?;
    let id = string(m, "algorithm")?;
    let algorithm = AlgorithmId::parse(id).ok().filter(|a| a.to_string() == id)?;
    let params = hex(string(m, "params")?)?;
    let seed = match m.get("seed") {
        None => None,
        Some(CValue::Str(s)) => Some(decimal_u64(s.as_bytes())?),
        Some(_) => return None,
    };
    let Some(CValue::Arr(list)) = m.get("inputs") else {
        return None;
    };
    if list.len() > MAX_INPUTS {
        return None;
    }
    let mut inputs = Vec::with_capacity(list.len());
    for x in list {
        let x = object(x, &["bytes", "sha256"], &[])?;
        let bytes = hex(string(x, "bytes")?)?;
        let claimed = Digest::parse(string(x, "sha256")?).ok()?;
        inputs.push((bytes, claimed));
    }
    Some(KernelCall { algorithm, params, seed, inputs })
}

fn error(code: &str) -> Vec<u8> {
    format!("{{\"error\":\"{code}\"}}").into_bytes()
}

/// Exit code and stdout for one call, as the `fid-kernel-wasm` shim answers.
pub fn respond(order: Order, input: &[u8]) -> (u8, Vec<u8>) {
    if input.len() > MAX_CALL_BYTES {
        return (2, error("oversized_call"));
    }
    let Some(call) = parse_call(input) else {
        return (2, error("malformed_call"));
    };
    let Some(table) = table(order) else {
        return (1, error("table_unavailable"));
    };
    match evaluate(&table, &call) {
        Ok(r) => (0, r.canonical_json()),
        Err(e) => (1, error(e.code())),
    }
}

/// `main` of the test artifacts: one call on stdin, the answer on stdout.
pub fn main_with(order: Order) -> std::process::ExitCode {
    use std::io::{Read, Write};
    let mut input = Vec::new();
    let read = std::io::stdin().lock().take(MAX_CALL_BYTES as u64 + 1).read_to_end(&mut input);
    let (code, out) = match read {
        Ok(_) => respond(order, &input),
        Err(_) => (2, error("malformed_call")),
    };
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&out).and_then(|()| stdout.flush()).is_err() {
        return std::process::ExitCode::from(3);
    }
    std::process::ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_build() {
        assert!(table(Order::Faithful).is_some());
        assert!(table(Order::Reversed).is_some());
    }
}
