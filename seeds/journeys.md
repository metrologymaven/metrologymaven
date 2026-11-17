# User journeys and acceptance jobs

Prefix `JOB`. These rows are the end-to-end jobs and personas the product must serve. FLOW nodes must cover every row. Source: concept doc 10 §3–5 (`../docs/concept/10-core-workflows-and-feature-scope.md`), paraphrased. Targets there are marked [L] (low confidence) until measured with design partners.

## Personas

| ID | Persona | Notes |
|---|---|---|
| JOB.PER.01 | Quality engineer at a multi-brand job shop: transcription reports from existing CMM output | First paid wedge (Track A) |
| JOB.PER.02 | Scan inspector: scan-file inspection against CAD | Inspect app |
| JOB.PER.03 | Shop-floor operator: gauge entry and kiosk runs | Kiosk mode |
| JOB.PER.04 | CMM programmer: re-evaluation, then I++ runs | Probe app |
| JOB.PER.05 | Quality manager: release, SPC, governance | Report app |
| JOB.PER.06 | Auditor or calibration-lab verifier | fid-verify |
| JOB.PER.07 | Supplier-network buyer / OEM supplier quality | Package intake |
| JOB.PER.08 | Instrument-maker engineer | Driver SDK |
| JOB.PER.09 | Large-volume metrologist: tracker networks, build and fixture guidance | Network app; not in doc 10 |

## Acceptance jobs (the daily-driver suite)

| ID | Job | Notes |
|---|---|---|
| JOB.ACC.01 | Scan first-article inspection from CAD + PMI to a colour map and GD&T report | Doc 10 J1; gating |
| JOB.ACC.02 | Transcription AS9102 from an existing CMM report against a ballooned drawing | J2; gating |
| JOB.ACC.03 | Drawing-only first-article inspection from CMM, arm and hand-gauge data | J3; gating |
| JOB.ACC.04 | Re-evaluation from CMM points with uncertainty and a decision rule | J4; gating |
| JOB.ACC.05 | Kiosk production run of a released scan program by an operator | J5; gating |
| JOB.ACC.06 | SPC continuity from imported history plus new runs | J6; tracked |
| JOB.ACC.07 | Sections, gap and flush, and thickness on a sheet-metal or moulded part | J7; gating |
| JOB.ACC.08 | Supplier package intake with tamper detection | J8; tracked |
| JOB.ACC.09 | Tracker network: multi-station adjustment and build guidance | Network app; added |
| JOB.ACC.10 | CMM program from PMI, simulated, then run on an I++ machine after approval | Probe app; added |

## Cross-cutting usability bars

| ID | Bar | Notes |
|---|---|---|
| JOB.USE.01 | Shop Setup to first project in under 10 minutes (Lean profile) | [L] |
| JOB.USE.02 | Unassisted first-time success on the first three jobs after one tutorial | ≥ 80 % [L] |
| JOB.USE.03 | System Usability Scale score | ≥ 70 [L] |
| JOB.USE.04 | Crash-free sessions, and zero committed-command loss | > 99.5 % |
| JOB.USE.05 | No dead ends: every unresolved item offers a suggested fix | |
| JOB.USE.06 | Keyboard-complete data entry and mapping | |
| JOB.USE.07 | First-run Shop Setup wizard (units, profile, signing identity, decision rules, instrument library, templates, agent access) | Doc 10 §5 |
| JOB.USE.08 | Guided tutorials as recorded programs replayed in step mode, with "your turn" checks | Doc 10 §5 |
| JOB.USE.09 | A "Why?" panel on every number and every not-decidable result | Doc 10 §5 |
