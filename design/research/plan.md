# {{FAMILY_NAME}} User Research Plan

Spec node: `UX.RESEARCH.PROTO` (REQ-UX.RESEARCH.PROTO-001 .. -005; satisfies REQ-UX.RESEARCH-007 .. -010).
Licence: CC-BY-4.0.

This plan governs all user research and usability testing for {{FAMILY_NAME}}, from early
discovery interviews in P0 through per-release usability evaluations from P1a onwards.

## 1. Goals and cadence

The research programme answers two questions with practising metrologists and shop-floor
operators:

1. **Discovery inquiry (P0, `UX.RESEARCH.INQUIRY`):** before the P1a workflow rail is frozen,
   complete at least 5 interviews or contextual inquiries with practising metrologists or
   operators who use current commercial inspection tools, and publish anonymised findings.
2. **Per-release usability testing (P1a+, `UX.RESEARCH.USAB` & `UX.RESEARCH.SUS`):** test
   every release that changes a GUI app with at least 5 practising participants on the
   acceptance jobs of the changed apps, publish the System Usability Scale (SUS) mean,
   participant count, 95 % confidence interval and ranked top issues within 30 days, and file
   tracked issues for every top-two-severity finding.

## 2. Governing documents

Every research activity follows four published documents under `design/research/`:

- **`design/research/plan.md`** (this file): programme scope, telemetry ban and publication rules.
- **`design/research/protocol.md`** (`ux-research-protocol@1`): recruitment criteria per FLOW
  persona (`JOB.PER.01`..`JOB.PER.09`), session scripts, tasks by acceptance-job ID, metrics
  and severity-scale reference.
- **`design/research/consent.md`**: informed-consent form covering purpose, recording,
  retention, access, withdrawal and separate opt-in to publish recordings.
- **`design/research/data-handling.md`**: participant code scheme (`P-<study>-<NN>`), private
  storage outside the public repository, deletion schedule and `fid-design research-check`.

## 3. No research via product telemetry without opt-in (CONSTITUTION C7)

**Research data is never collected through product telemetry unless the participant explicitly
opted in to that telemetry (REQ-UX.RESEARCH.PROTO-005, CONSTITUTION C7).**

- The default configuration of {{FAMILY_NAME}} sends no telemetry and no usage analytics.
- Usability timings, error counts and task outcomes are observed only in consented research
  sessions run under `design/research/protocol.md`, or exported deliberately by a participant
  who has given recorded informed consent.
- Any future opt-in telemetry programme belongs to `OUTR.COMM` and remains strictly separate
  from user research sessions.

## 4. Clean-room rule for incumbent workflows (CONSTITUTION C9)

Research reports describe incumbent metrology workflows strictly in the project's **own words**
(REQ-UX.RESEARCH-010, CONSTITUTION C9). Published research artefacts must never contain:

- screenshots, screen recordings or photographs of another vendor's software;
- copied text, menus, dialog strings or documentation from another vendor;
- icons, layouts or trade dress from another product.

Every raster image published in a research report must come from {{FAMILY_NAME}} itself and
carry a `research-capture@1` sidecar (`<image>.capture.json`) naming the project's own capture
command.

## 5. Publication gate

Before any research note, summary, table or image is committed to the public repository, the
study lead runs `fid-design research-check --root <dir> --key <private key.csv>` locally
against the private participant code key (held outside git). No output may be published until
`research-check` exits 0.
