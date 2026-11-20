//! Trial-block sampling and per-trial kernel evaluation. Spec node `UNC.PROP.MCM.BLOCK`,
//! algorithm [`ALGORITHM_ID`] = `unc.prop.mcm_block@1.0.0`.
//!
//! [`run_block`] evaluates one fixed-size block of Monte Carlo trials (JCGM 101:2008,
//! clauses 7.2 to 7.4 [unverified: clause numbering]): every trial draws each input from
//! its distribution at a counter-based RNG address and re-runs every operator of the
//! model through the kernel call interface ([`fid_kci::evaluate`]). The result depends
//! only on (model digest, inputs digest, seed, block index, block size), never on the
//! order in which blocks are computed or on the thread that computed them
//! (REQ-UNC.PROP.MCM.BLOCK-002): this module is single-threaded and I/O-free; thread
//! scheduling belongs to the command (`UNC.PROP.MCM.CMD`).
//!
//! # Provenance (fail closed, before any trial)
//!
//! A model reaches this module only as a [`ValidModel`] (`UNC.MODEL.GRAPH.CHECK`), which the
//! engine resolves from the model store (`UNC.MODEL.GRAPH.STORE`). [`BlockModel::prepare`]
//! binds it to the running kernel in this order:
//!
//! 1. the release manifest's digest must equal the manifest the model was validated
//!    against (`provenance{model_manifest}`);
//! 2. the running kernel answers the reserved KCI query `kernel.manifest.core_digest@1.0.0`
//!    with the core digest compiled into it. A kernel without one answers
//!    `manifest_core_digest_unset` (`provenance{kernel_manifest_unset}`); any other error or
//!    a malformed answer is `provenance{kernel_unavailable}`;
//! 3. that digest must equal the release manifest's core digest
//!    (`provenance{kernel_manifest_mismatch}`).
//!
//! The host wires the kernel's own compiled-in digest (the `fid-kernel-wasm` constant or the
//! artifact itself) and must never derive it from the manifest file, which would compare the
//! file with itself. Then every operator's algorithm must be in the kernel's table with a
//! spec card whose `input_arity` accepts the operator's input-port count
//! (`unsupported_model{operator_not_in_kernel}`, `unsupported_model{operator_arity}`).
//!
//! # Inputs and sampling address (pinned)
//!
//! [`SamplingPlan::new`] fixes how the model's inputs are drawn. Input *ordinals* are the
//! positions of the inputs in the model's canonical (ID-sorted) order. An input outside any
//! correlation group is drawn with [`Distribution::sample_at`] at address
//! `(seed, ordinal, t)` for global trial index `t`. A correlation group is drawn as one
//! multivariate normal ([`MvNormal`], components in the group's sorted input order) at
//! address `(seed, ordinal of the group's first input, t)`; its draws use the multivariate
//! sub-stream of `UNC.MODEL.DIST`, so the members' consecutive standard normals are addressed
//! by component and the other members' ordinals are left unused. A group with a member that
//! is not normal is `unsupported_model{correlated_non_normal}` (no copula is specified). The
//! plan's canonical JSON is
//! `{"algorithm","model","samplers":[{"inputs":[<id>...],"kind","stream"}...]}` (samplers in
//! ascending stream order, `stream` as a decimal string, `kind` the distribution kind or
//! `mv_normal`); [`SamplingPlan::digest`] is SHA-256 over it, the block's *inputs digest*.
//!
//! # Trials (pinned order)
//!
//! A [`BlockSpec`] is checked first: `block_size >= 1` (`invalid_block{block_size}`), the
//! end index `(block_index + 1) * block_size` must fit in `u64` (`invalid_block{index}`)
//! and the last index must be addressable by every sampler (`invalid_block{index}`); its
//! model digest and inputs digest must equal the prepared model's and plan's
//! (`block_spec_mismatch{model_digest}`, `block_spec_mismatch{inputs_digest}`). Then for
//! `t` in `block_index * block_size .. (block_index + 1) * block_size`, ascending:
//!
//! 1. Sample every sampler in plan order. A draw that is NaN or infinite fails the trial
//!    with `non_finite` (detail `sample`, REQ-UNC.PROP.MCM.BLOCK-004); an exhausted gamma
//!    sub-stream with `sampling_exhausted`.
//! 2. Evaluate every operator in the model's Kahn order (`ValidModel::order`) with one
//!    [`evaluate`] call: parameters are the RFC 8785 bytes of the operator's `params`, no
//!    seed, and one input blob per input port in declared order, holding the port's values
//!    as consecutive little-endian IEEE-754 doubles in canonical edge order (source node,
//!    then source port). The output must be exactly one little-endian double per output
//!    port in declared order (otherwise `bad_output`); a NaN or infinite output fails the
//!    trial with `non_finite` (detail `output`). An algorithm failure fails the trial with
//!    `algorithm_failed` and the algorithm's own code as detail (REQ-UNC.PROP.MCM.BLOCK-003).
//!    Any other kernel error (floating-point environment, lookup, card, arity, digest or
//!    parameter validation) is a property of the model or the kernel, not of the trial, and
//!    ends the block with `kernel{code, trial}`.
//! 3. A trial that did not fail appends each output node's value (outputs in ID order) to
//!    its column of [`BlockOut::values`]; a failed trial appends `(t, code, detail)` to
//!    [`BlockOut::failed`] and is excluded from every column. The first failure of a trial
//!    is the one recorded; the trial stops there.
//!
//! # Canonical encoding of [`BlockOut`]
//!
//! [`BlockOut::canonical_json`]: RFC 8785 bytes of
//! `{"block_index","failed":[{"code","detail"?,"index"}...],"values":[[<f64>...]...]}`
//! with integers as decimal strings and every double as its 16 lowercase hex digits of
//! IEEE-754 bits (the kernel convention).
//!
//! KNP-1: this module does no floating-point arithmetic of its own (sampling is
//! `UNC.MODEL.DIST`, operators are kernel algorithms); no hash containers, no panics, a NaN
//! is always an error.

use std::collections::BTreeMap;

use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::sha256_digest;
use fid_kci::{
    evaluate, AlgorithmId, AlgorithmTable, KernelCall, KernelError, KernelManifest, MANIFEST_CORE_DIGEST_QUERY,
};
use fid_num::NumError;
use fid_types::Digest;

use super::McmError;
use crate::dist::{Correlation, DistError, Distribution, Kind, MvNormal, Params, Quantity};
use crate::dist::{MAX_GAMMA_INDEX, MAX_MV_INDEX};
use crate::model::{ValidModel, VALUE_PORT};

/// Algorithm ID of this rule set.
pub const ALGORITHM_ID: &str = "unc.prop.mcm_block@1.0.0";

/// Bytes of one value on an edge: a little-endian IEEE-754 double.
pub const VALUE_BYTES: usize = 8;

/// Trial failure code: a sampled value or an operator output is NaN or infinite.
pub const FAIL_NON_FINITE: &str = "non_finite";
/// Trial failure code: the operator's algorithm failed (detail: its own code).
pub const FAIL_ALGORITHM: &str = "algorithm_failed";
/// Trial failure code: the scaled-t gamma sub-stream was exhausted.
pub const FAIL_SAMPLING_EXHAUSTED: &str = "sampling_exhausted";
/// Trial failure code: an operator's output is not one double per output port.
pub const FAIL_BAD_OUTPUT: &str = "bad_output";

/// The identity of one block (REQ-UNC.PROP.MCM.BLOCK-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSpec {
    /// Digest of the model ([`crate::model::UncertaintyModel::digest`]).
    pub model_digest: Digest,
    /// Digest of the sampling plan ([`SamplingPlan::digest`]).
    pub inputs_digest: Digest,
    /// RNG key.
    pub seed: u64,
    /// 0-based block index.
    pub block_index: u64,
    /// Trials per block, at least 1.
    pub block_size: u64,
}

/// One failed trial (REQ-UNC.PROP.MCM.BLOCK-003, -004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrialFailure {
    /// Global trial index.
    pub index: u64,
    /// One of [`FAIL_NON_FINITE`], [`FAIL_ALGORITHM`], [`FAIL_SAMPLING_EXHAUSTED`],
    /// [`FAIL_BAD_OUTPUT`].
    pub code: &'static str,
    /// `sample` or `output` for `non_finite`, the algorithm's code for `algorithm_failed`.
    pub detail: Option<&'static str>,
}

/// The result of one block.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockOut {
    pub block_index: u64,
    /// One column per output node (in ID order), each in trial order, failed trials excluded.
    pub values: Vec<Vec<f64>>,
    /// Failed trials in ascending index order.
    pub failed: Vec<TrialFailure>,
}

impl BlockOut {
    /// RFC 8785 bytes (see the module documentation).
    pub fn canonical_json(&self) -> Result<Vec<u8>, McmError> {
        let mut failed = Vec::new();
        failed.try_reserve_exact(self.failed.len()).map_err(|_| McmError::Allocation)?;
        for f in &self.failed {
            let mut m = vec![("code".to_owned(), Json::Str(f.code.to_owned())), ("index".to_owned(), num(f.index))];
            if let Some(d) = f.detail {
                m.push(("detail".to_owned(), Json::Str(d.to_owned())));
            }
            failed.push(Json::Obj(m));
        }
        let mut values = Vec::new();
        values.try_reserve_exact(self.values.len()).map_err(|_| McmError::Allocation)?;
        for col in &self.values {
            let mut c = Vec::new();
            c.try_reserve_exact(col.len()).map_err(|_| McmError::Allocation)?;
            c.extend(col.iter().map(|x| Json::Str(format!("{:016x}", x.to_bits()))));
            values.push(Json::Arr(c));
        }
        let doc = Json::Obj(vec![
            ("block_index".to_owned(), num(self.block_index)),
            ("failed".to_owned(), Json::Arr(failed)),
            ("values".to_owned(), Json::Arr(values)),
        ]);
        to_canonical(&doc).map_err(|_| McmError::Internal { detail: "block_out_encoding" })
    }
}

fn num(x: u64) -> Json {
    Json::Str(x.to_string())
}

// ------------------------------------------------------------------ sampling plan

#[derive(Debug, Clone)]
enum Sampler {
    Uni { dist: Distribution, stream: u64 },
    Mv { dist: MvNormal, stream: u64, slots: Vec<usize> },
}

impl Sampler {
    fn stream(&self) -> u64 {
        match self {
            Sampler::Uni { stream, .. } | Sampler::Mv { stream, .. } => *stream,
        }
    }

    /// Largest addressable trial index.
    fn max_index(&self) -> u64 {
        match self {
            Sampler::Uni { dist, .. } if dist.kind() == Kind::ScaledT => MAX_GAMMA_INDEX,
            Sampler::Uni { .. } => u64::MAX,
            Sampler::Mv { .. } => MAX_MV_INDEX,
        }
    }
}

/// How the model's inputs are drawn (see the module documentation); its digest is the
/// block's inputs digest.
#[derive(Debug, Clone)]
pub struct SamplingPlan {
    model_digest: Digest,
    samplers: Vec<Sampler>,
    inputs: usize,
    canonical: Vec<u8>,
    digest: Digest,
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Normal => "normal",
        Kind::Rect => "rect",
        Kind::Tri => "tri",
        Kind::Arcsine => "arcsine",
        Kind::ScaledT => "scaled_t",
        Kind::Trap => "trap",
        Kind::MvNormal => "mv_normal",
    }
}

fn unsupported(detail: &'static str) -> McmError {
    McmError::UnsupportedModel { detail }
}

impl SamplingPlan {
    /// The plan of `model`'s inputs.
    pub fn new(model: &ValidModel) -> Result<SamplingPlan, McmError> {
        let m = model.model();
        let inputs = m.inputs();
        let ordinal = |id: &str| inputs.binary_search_by(|n| n.id.as_bytes().cmp(id.as_bytes())).ok();
        let mut grouped = vec![false; inputs.len()];
        let mut samplers = Vec::new();
        let mut ids: Vec<Vec<&str>> = Vec::new();
        for g in m.correlations() {
            let mut slots = Vec::new();
            let mut mu = Vec::new();
            let mut sigma = Vec::new();
            for id in &g.inputs {
                let k = ordinal(id).ok_or(McmError::Internal { detail: "correlation_member" })?;
                let x = &inputs[k];
                let Params::Normal { mu: mk, sigma: sk } = x.distribution.params() else {
                    return Err(unsupported("correlated_non_normal"));
                };
                grouped[k] = true;
                slots.push(k);
                mu.push(Quantity::new(mk, x.unit));
                sigma.push(Quantity::new(sk, x.unit));
            }
            let n = g.inputs.len();
            let data: Vec<f64> = g.matrix.iter().flatten().copied().collect();
            let corr = Correlation::new(n, data).map_err(McmError::Dist)?;
            let dist = MvNormal::new(&mu, &sigma, corr).map_err(McmError::Dist)?;
            let stream = slots.first().copied().ok_or(McmError::Internal { detail: "empty_group" })? as u64;
            samplers.push(Sampler::Mv { dist, stream, slots });
            ids.push(g.inputs.iter().map(String::as_str).collect());
        }
        for (k, x) in inputs.iter().enumerate() {
            if !grouped[k] {
                samplers.push(Sampler::Uni { dist: x.distribution, stream: k as u64 });
                ids.push(vec![x.id.as_str()]);
            }
        }
        let mut order: Vec<usize> = (0..samplers.len()).collect();
        order.sort_by_key(|&i| samplers[i].stream());
        let mut docs = Vec::with_capacity(order.len());
        for &i in &order {
            let kind = match &samplers[i] {
                Sampler::Uni { dist, .. } => dist.kind(),
                Sampler::Mv { .. } => Kind::MvNormal,
            };
            docs.push(Json::Obj(vec![
                ("inputs".to_owned(), Json::Arr(ids[i].iter().map(|s| Json::Str((*s).to_owned())).collect())),
                ("kind".to_owned(), Json::Str(kind_name(kind).to_owned())),
                ("stream".to_owned(), num(samplers[i].stream())),
            ]));
        }
        let samplers: Vec<Sampler> = order.iter().map(|&i| samplers[i].clone()).collect();
        let doc = Json::Obj(vec![
            ("algorithm".to_owned(), Json::Str(ALGORITHM_ID.to_owned())),
            ("model".to_owned(), Json::Str(m.digest().as_str().to_owned())),
            ("samplers".to_owned(), Json::Arr(docs)),
        ]);
        let canonical = to_canonical(&doc).map_err(|_| McmError::Internal { detail: "plan_encoding" })?;
        let digest = sha256_digest(&canonical);
        Ok(SamplingPlan { model_digest: m.digest(), samplers, inputs: inputs.len(), canonical, digest })
    }

    /// The inputs digest: SHA-256 of [`Self::canonical_json`].
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The canonical JSON of the plan.
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }

    /// Digest of the model the plan was made for.
    pub fn model_digest(&self) -> Digest {
        self.model_digest
    }
}

// ------------------------------------------------------------------ prepared model

#[derive(Debug, Clone)]
struct OpPlan {
    algorithm: AlgorithmId,
    params: Vec<u8>,
    /// Per input port, the value slots of its edges in canonical edge order.
    ports_in: Vec<Vec<usize>>,
    /// First value slot of the output ports (one slot per port).
    out_slot: usize,
    outs: usize,
}

/// A validated model bound to the running kernel (see the module documentation).
#[derive(Debug)]
pub struct BlockModel<'k> {
    kernel: &'k AlgorithmTable,
    model_digest: Digest,
    inputs: usize,
    slots: usize,
    /// In Kahn order.
    ops: Vec<OpPlan>,
    /// Value slot of each output node, in ID order.
    outputs: Vec<usize>,
}

fn provenance(detail: &'static str) -> McmError {
    McmError::Provenance { detail }
}

/// The core digest the running kernel reports through the reserved KCI query.
fn running_core_digest(kernel: &AlgorithmTable) -> Result<Digest, McmError> {
    let query = AlgorithmId::parse(MANIFEST_CORE_DIGEST_QUERY).map_err(|_| provenance("kernel_unavailable"))?;
    let call = KernelCall { algorithm: query, params: Vec::new(), seed: None, inputs: Vec::new() };
    let result = match evaluate(kernel, &call) {
        Ok(r) => r,
        Err(KernelError::ManifestCoreDigestUnset) => return Err(provenance("kernel_manifest_unset")),
        Err(_) => return Err(provenance("kernel_unavailable")),
    };
    let text = std::str::from_utf8(result.output()).map_err(|_| provenance("kernel_unavailable"))?;
    Digest::parse(text).map_err(|_| provenance("kernel_unavailable"))
}

impl<'k> BlockModel<'k> {
    /// Binds `model` to `kernel` through `manifest` and plans the per-trial evaluation.
    pub fn prepare(
        model: &ValidModel,
        manifest: &KernelManifest,
        kernel: &'k AlgorithmTable,
    ) -> Result<BlockModel<'k>, McmError> {
        if manifest.manifest_digest() != model.manifest_digest() {
            return Err(provenance("model_manifest"));
        }
        if running_core_digest(kernel)? != *manifest.core_digest() {
            return Err(provenance("kernel_manifest_mismatch"));
        }
        let m = model.model();
        let ids = kernel.ids();

        // Value slots: inputs by ordinal, then each operator's output ports.
        let mut sources: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for (k, x) in m.inputs().iter().enumerate() {
            sources.insert((x.id.as_str(), VALUE_PORT), k);
        }
        let mut slots = m.inputs().len();
        let mut out_slot: BTreeMap<&str, usize> = BTreeMap::new();
        for op in m.operators() {
            out_slot.insert(op.id.as_str(), slots);
            for p in &op.ports_out {
                sources.insert((op.id.as_str(), p.name.as_str()), slots);
                slots += 1;
            }
        }
        let resolve = |node: &str, port: &str| {
            sources.get(&(node, port)).copied().ok_or(McmError::Internal { detail: "unresolved_edge" })
        };

        let mut ops = Vec::with_capacity(m.operators().len());
        for id in model.order() {
            let Ok(i) = m.operators().binary_search_by(|o| o.id.as_bytes().cmp(id.as_bytes())) else {
                continue;
            };
            let op = &m.operators()[i];
            let algorithm = op.algorithm().ok_or(McmError::Internal { detail: "operator_algorithm" })?;
            if ids.binary_search(&algorithm).is_err() {
                return Err(unsupported("operator_not_in_kernel"));
            }
            let card = kernel.cards().get(&algorithm).ok_or(unsupported("operator_not_in_kernel"))?;
            if !card.input_arity.accepts(op.ports_in.len()) {
                return Err(unsupported("operator_arity"));
            }
            let params = to_canonical(&op.params).map_err(|_| McmError::Internal { detail: "operator_params" })?;
            let mut ports_in = Vec::with_capacity(op.ports_in.len());
            for p in &op.ports_in {
                let mut srcs = Vec::new();
                for e in m.edges().iter().filter(|e| e.to.node == op.id && e.to.port == p.name) {
                    srcs.push(resolve(&e.from.node, &e.from.port)?);
                }
                ports_in.push(srcs);
            }
            let first = out_slot.get(op.id.as_str()).copied().ok_or(McmError::Internal { detail: "operator_slot" })?;
            ops.push(OpPlan { algorithm, params, ports_in, out_slot: first, outs: op.ports_out.len() });
        }
        if ops.len() != m.operators().len() {
            return Err(McmError::Internal { detail: "operator_order" });
        }

        let mut outputs = Vec::with_capacity(m.outputs().len());
        for o in m.outputs() {
            let e = m
                .edges()
                .iter()
                .find(|e| e.to.node == o.id && e.to.port == VALUE_PORT)
                .ok_or(McmError::Internal { detail: "output_edge" })?;
            outputs.push(resolve(&e.from.node, &e.from.port)?);
        }
        Ok(BlockModel { kernel, model_digest: m.digest(), inputs: m.inputs().len(), slots, ops, outputs })
    }

    /// Digest of the bound model.
    pub fn model_digest(&self) -> Digest {
        self.model_digest
    }
}

// ------------------------------------------------------------------ trials

enum TrialError {
    Fail(&'static str, Option<&'static str>),
    Abort(McmError),
}

fn sample_error(e: DistError) -> TrialError {
    match e {
        DistError::Num(NumError::NotANumber { .. } | NumError::Infinite { .. }) => {
            TrialError::Fail(FAIL_NON_FINITE, Some("sample"))
        }
        DistError::SamplingExhausted { .. } => TrialError::Fail(FAIL_SAMPLING_EXHAUSTED, None),
        other => TrialError::Abort(McmError::Dist(other)),
    }
}

fn trial(model: &BlockModel<'_>, plan: &SamplingPlan, seed: u64, t: u64, v: &mut [f64]) -> Result<(), TrialError> {
    for s in &plan.samplers {
        match s {
            Sampler::Uni { dist, stream } => {
                let slot = usize::try_from(*stream).map_err(|_| TrialError::Abort(McmError::Allocation))?;
                v[slot] = dist.sample_at(seed, *stream, t).map_err(sample_error)?;
            }
            Sampler::Mv { dist, stream, slots } => {
                let x = dist.sample(seed, *stream, t..t + 1).map_err(sample_error)?;
                for (&slot, &xi) in slots.iter().zip(&x) {
                    v[slot] = xi;
                }
            }
        }
    }
    // REQ-UNC.PROP.MCM.BLOCK-004 for samples (the samplers already refuse non-finite draws).
    if v[..model.inputs].iter().any(|x| !x.is_finite()) {
        return Err(TrialError::Fail(FAIL_NON_FINITE, Some("sample")));
    }
    for op in &model.ops {
        let mut inputs = Vec::with_capacity(op.ports_in.len());
        for srcs in &op.ports_in {
            let mut blob = Vec::with_capacity(srcs.len() * VALUE_BYTES);
            for &s in srcs {
                blob.extend_from_slice(&v[s].to_le_bytes());
            }
            let d = sha256_digest(&blob);
            inputs.push((blob, d));
        }
        let call = KernelCall { algorithm: op.algorithm.clone(), params: op.params.clone(), seed: None, inputs };
        let result = match evaluate(model.kernel, &call) {
            Ok(r) => r,
            Err(KernelError::Algorithm(f)) => return Err(TrialError::Fail(FAIL_ALGORITHM, Some(f.code))),
            Err(e) => return Err(TrialError::Abort(McmError::Kernel { code: e.code(), trial: t })),
        };
        let out = result.output();
        if out.len() != op.outs * VALUE_BYTES {
            return Err(TrialError::Fail(FAIL_BAD_OUTPUT, None));
        }
        for (k, chunk) in out.chunks_exact(VALUE_BYTES).enumerate() {
            let mut b = [0u8; VALUE_BYTES];
            b.copy_from_slice(chunk);
            let x = f64::from_le_bytes(b);
            if !x.is_finite() {
                return Err(TrialError::Fail(FAIL_NON_FINITE, Some("output")));
            }
            v[op.out_slot + k] = x;
        }
    }
    Ok(())
}

fn invalid_block(detail: &'static str) -> McmError {
    McmError::InvalidBlock { detail }
}

/// Evaluates one block of trials (REQ-UNC.PROP.MCM.BLOCK-001 .. -004; see the module
/// documentation for the checks and the per-trial order).
pub fn run_block(model: &BlockModel<'_>, inputs: &SamplingPlan, spec: &BlockSpec) -> Result<BlockOut, McmError> {
    if spec.block_size == 0 {
        return Err(invalid_block("block_size"));
    }
    let start = spec.block_index.checked_mul(spec.block_size).ok_or(invalid_block("index"))?;
    let end = start.checked_add(spec.block_size).ok_or(invalid_block("index"))?;
    if inputs.samplers.iter().any(|s| end - 1 > s.max_index()) {
        return Err(invalid_block("index"));
    }
    if spec.model_digest != model.model_digest || inputs.model_digest != model.model_digest {
        return Err(McmError::BlockSpecMismatch { field: "model_digest" });
    }
    if spec.inputs_digest != inputs.digest || inputs.inputs != model.inputs {
        return Err(McmError::BlockSpecMismatch { field: "inputs_digest" });
    }
    let size = usize::try_from(spec.block_size).map_err(|_| McmError::Allocation)?;
    let mut values: Vec<Vec<f64>> = Vec::new();
    values.try_reserve_exact(model.outputs.len()).map_err(|_| McmError::Allocation)?;
    for _ in &model.outputs {
        let mut col = Vec::new();
        col.try_reserve_exact(size).map_err(|_| McmError::Allocation)?;
        values.push(col);
    }
    let mut failed = Vec::new();
    let mut v = Vec::new();
    v.try_reserve_exact(model.slots).map_err(|_| McmError::Allocation)?;
    v.resize(model.slots, 0.0);
    for t in start..end {
        match trial(model, inputs, spec.seed, t, &mut v) {
            Ok(()) => {
                for (col, &slot) in values.iter_mut().zip(&model.outputs) {
                    col.push(v[slot]);
                }
            }
            Err(TrialError::Fail(code, detail)) => {
                failed.try_reserve(1).map_err(|_| McmError::Allocation)?;
                failed.push(TrialFailure { index: t, code, detail });
            }
            Err(TrialError::Abort(e)) => return Err(e),
        }
    }
    Ok(BlockOut { block_index: spec.block_index, values, failed })
}
