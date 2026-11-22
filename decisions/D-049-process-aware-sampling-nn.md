---
id: D-049
title: Process-conditioned geometric neural network for CMM sampling strategy proposals
status: accepted
raised_by: founder
blocks: [CMM.STRAT.PROC]
amends: [D-036]
decide_by: before expanding CMM.STRAT.PROC
updated: 2026-11-22
---

# D-049: Process-conditioned geometric neural network for CMM sampling strategy proposals

## Question

May Metrology Maven include an optional, offline, Process-Conditioned Geometric Neural Network (PC-GNN) that proposes CMM sampling strategies from CAD geometry, tolerances and manufacturing process metadata—and what boundaries govern its safety, uncertainty handling, open/commercial split and research-risk validation?

## Context

CMM programming software normally generates uniform or curvature-only hit layouts that ignore how a part was manufactured. In practice, geometric form errors follow process-dependent signatures (for example chucking lobing and taper in turning, cantilever tool and thin-wall deflection in milling, shrinkage warpage and sink marks in molding, bend springback in stamping, and overhang sag or thermal curl in metal additive manufacturing).

Adding a compact geometric neural network (~15M-50M parameters, exported as ONNX for local CPU inference) raises four architectural questions across `CMM.STRAT`, `AGENT`, `UNC` and `ENT`:

1. **Kernel boundary (CONSTITUTION C1):** No neural network may compute a value of record, a fit, a conformance decision or an official measurement uncertainty.
2. **Embedded-AI boundary (`AGENT` Out-of-Scope):** `spec/agent/NODE.md` excludes an embedded LLM in P0-P2. A small, frozen-weight geometric prior model is distinct from an LLM, runs offline in milliseconds on CPU, and proposes only draft `MeasurementStrategy` objects.
3. **Two-layer process vs. exact-machine scope (D-036 rule 3):** Base process-family physics generalizes across shops on day one, while exact-machine / mold-cavity fingerprints are private to each factory and must never require uploading proprietary CAD to a cloud service.
4. **Research risk (D-036 rule 4):** Sim-to-real transfer from synthetic physics-error generators to real machined, molded, stamped and printed parts is unproven research risk and should carry measurable hypotheses against real scanned benchmark parts.

Full technical architecture, surrogate physics equations and real-part validation protocol live in `research/process-sampling-nn/`.

## Options

| Option | For | Against |
|---|---|---|
| A. Deterministic rule libraries only (no ML model in `CMM.STRAT`) | Simplest; zero ML runtime dependency | Cannot generalize spatial error risk across arbitrary freeform or complex multi-feature B-rep geometry; misses major research/SBIR differentiator |
| B. End-to-end neural net emitting CMM motion/DMIS directly | Single model | Unsafe (hallucination risks crashing physical CMMs); violates C1, C5 and H1; impossible to verify deterministically across arbitrary machine kinematics |
| C. Hybrid neuro-symbolic proposer: local CPU PC-GNN proposes draft `MeasurementStrategy` + deterministic verification + H1 gate + two-layer open/ENT adapter split | Preserves C1/C5/C7 and H1 safety; runs offline on shop-floor CPU; clean open-source base + private on-prem enterprise adapter path; directly supports D-036 grant/research framing | Adds an optional ONNX inference dependency in a P3 module and requires a real-part scan benchmark to close the sim-to-real gap |

## Recommendation

**Option C**, with five binding rules:

1. **Proposer only, never kernel or controller (C1, C5):** The PC-GNN outputs only a spatial deviation-risk field (`mu`, `sigma^2`), harmonic-risk spectrum and draft `MeasurementStrategy` proposal tagged `agent_authored: true` with model ID and weight digest. Every proposed strategy must pass deterministic `CMM.STRAT.CHECK`, accessibility/collision verification (`CMM.COLL`, `CMM.PATH`, `CMM.SIM`), and human `H1` motion-envelope approval before physical execution.
2. **Uncertainty and accuracy separation:** The PC-GNN accepts target CMM accuracy (`MPE_E`) and tolerance band (`T`) as planning inputs and outputs calibrated predictive variance (`sigma^2`). Whenever `sigma^2` exceeds a declared threshold, the planner deterministically falls back to conservative uniform/dense sampling. Official reported measurement uncertainty (`U`) remains strictly the deterministic `UNC` kernel's responsibility after physical probing.
3. **Offline local-first runtime (C7):** Inference runs locally on standard shop-floor CPUs via pinned ONNX weights with zero network calls and no embedded LLM.
4. **Two-layer open vs. commercial boundary (D-036 rule 3):**
   - **Open (MIT OR Apache-2.0):** the process taxonomy, deterministic strategy-compilation layer, surrogate physics-error generators, open benchmark artifacts, and base process-family weights.
   - **Commercial `service` / `ENT` add-on:** on-premises private fine-tuning of exact-machine / mold-cavity LoRA adapters and multi-CMM fleet drift monitoring inside self-hosted `fid server` (zero CAD egress).
5. **Research-risk designation (amends D-036 rule 4):** Add `CMM.STRAT.PROC` to the D-036 rule 4 list of research-risk nodes whose acceptance criteria are stated as measurable hypotheses (`H-1` through `H-4`) against real scanned benchmark parts.

## Decision

**Accepted (option C), 2026-11-22, founder (via D-052).** `CMM.STRAT.PROC` moves from `status: decision` to `status: open` and is added to the `AGENTS.md` rule 10 research-risk list.
