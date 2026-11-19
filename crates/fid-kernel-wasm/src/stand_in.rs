//! Stand-in test algorithms, registered only with the `test-algorithms` feature.
//!
//! The real algorithm table comes from the `fid-kernel` assembly crate, which does not exist
//! yet (out of scope for KRN.MAN.WASM). These three algorithms let CI run the fixtures through
//! the module: `test.echo.identity@1.0.0` is the KRN.MAN.KCI echo (AC-2),
//! `test.stack.recurse@1.0.0` recurses to a depth given in its parameters (AC-3) and
//! `test.memory.alloc@1.0.0` allocates a buffer of a given size (REQ-KRN.MAN.WASM-005).

use std::hint::black_box;

use fid_kci::{parse_card, AlgorithmFailure, AlgorithmId, AlgorithmTable, CardRegistry, KernelAlgorithm, KernelError};

/// The KRN.MAN.KCI echo: `params | seed-or-"-" | input0 | input1 ...`.
pub const ECHO_ID: &str = "test.echo.identity@1.0.0";
/// Recurses to the depth in its parameters; outputs the depth reached.
pub const RECURSE_ID: &str = "test.stack.recurse@1.0.0";
/// Allocates the number of bytes in its parameters; outputs the length allocated.
pub const ALLOC_ID: &str = "test.memory.alloc@1.0.0";

fn card(id: &str, title: &str, method: &str) -> String {
    [
        format!("id = \"{id}\""),
        format!("title = \"{title}\""),
        format!("method = \"{method}\""),
        "applicability = \"Tests of the kernel WASM artifact only.\"".to_string(),
        "references = [\"Spec node KRN.MAN.WASM, acceptance criteria AC-2 and AC-3.\"]".to_string(),
        "determinism = \"D0\"".to_string(),
        "failure_modes = [\"Malformed parameters are refused.\"]".to_string(),
        "validation = \"fixtures/man/wasm/calls-v1.txt\"".to_string(),
    ]
    .join("\n")
}

/// Parameters as a decimal `u64` without leading zeros.
fn decimal_param(params: &[u8]) -> Result<u64, KernelError> {
    let bad = || KernelError::InvalidParams { reason: "params_not_decimal_u64" };
    if params.is_empty() || params.len() > 20 || (params.len() > 1 && params.first() == Some(&b'0')) {
        return Err(bad());
    }
    let mut v: u64 = 0;
    for &c in params {
        if !c.is_ascii_digit() {
            return Err(bad());
        }
        v = v.checked_mul(10).and_then(|x| x.checked_add(u64::from(c - b'0'))).ok_or_else(bad)?;
    }
    Ok(v)
}

struct Echo(AlgorithmId);

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
        for x in inputs {
            out.push(b'|');
            out.extend_from_slice(x);
        }
        Ok(out)
    }
}

/// Non-tail recursion: `black_box` hides the result and the argument from the optimiser, so
/// every level keeps a frame.
fn depth(n: u64) -> u64 {
    if n == 0 {
        0
    } else {
        black_box(depth(black_box(n - 1))).wrapping_add(1)
    }
}

struct Recurse(AlgorithmId);

impl KernelAlgorithm for Recurse {
    fn id(&self) -> AlgorithmId {
        self.0.clone()
    }
    fn validate(&self, params: &[u8]) -> Result<(), KernelError> {
        decimal_param(params).map(|_| ())
    }
    fn run(&self, params: &[u8], _seed: Option<u64>, _inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        let n = decimal_param(params)?;
        Ok(depth(n).to_string().into_bytes())
    }
}

struct Alloc(AlgorithmId);

impl KernelAlgorithm for Alloc {
    fn id(&self) -> AlgorithmId {
        self.0.clone()
    }
    fn validate(&self, params: &[u8]) -> Result<(), KernelError> {
        decimal_param(params).map(|_| ())
    }
    fn run(&self, params: &[u8], _seed: Option<u64>, _inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        let n = usize::try_from(decimal_param(params)?)
            .map_err(|_| KernelError::InvalidParams { reason: "size_exceeds_address_space" })?;
        // An infallible allocation on purpose: past the configured memory limit, `memory.grow`
        // fails and the runtime reports the allocation failure (REQ-KRN.MAN.WASM-005).
        let buf = black_box(vec![0u8; n]);
        Ok(buf.len().to_string().into_bytes())
    }
}

/// The stand-in table, or `None` if a card or ID fails to parse (it never does: unit-tested).
pub fn table() -> Option<AlgorithmTable> {
    let echo = AlgorithmId::parse(ECHO_ID).ok()?;
    let recurse = AlgorithmId::parse(RECURSE_ID).ok()?;
    let alloc = AlgorithmId::parse(ALLOC_ID).ok()?;
    let cards = CardRegistry::new(vec![
        parse_card(&card(ECHO_ID, "Echo test algorithm", "Concatenates parameters, seed and inputs.")).ok()?,
        parse_card(&card(RECURSE_ID, "Stack recursion test algorithm", "Recurses to the requested depth.")).ok()?,
        parse_card(&card(ALLOC_ID, "Allocation test algorithm", "Allocates the requested number of bytes.")).ok()?,
    ])
    .ok()?;
    let algorithms: Vec<Box<dyn KernelAlgorithm>> =
        vec![Box::new(Echo(echo)), Box::new(Recurse(recurse)), Box::new(Alloc(alloc))];
    AlgorithmTable::new(algorithms, cards).ok()
}
