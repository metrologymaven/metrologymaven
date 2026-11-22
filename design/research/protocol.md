# {{FAMILY_NAME}} User Research Protocol

- **Protocol version:** `ux-research-protocol@1` (version 1, 2026-11-22). Every study record
  cites this version ID. Any change to recruitment rules, task order, scoring or scripts bumps
  the version.
- **Spec node:** `UX.RESEARCH.PROTO` (REQ-UX.RESEARCH.PROTO-001). Licence: CC-BY-4.0.
- **Persona source:** `acceptance/personas/personas.json` (`FLOW.PERSONA.PERS`, `persona-set@1`).

## 1. Recruitment criteria per FLOW persona

Participants are practising metrologists, inspectors, engineers or operators whose daily work
matches one of the nine FLOW personas. Recruitment is carried out by a named human maintainer.

| Persona ID | Role summary | Environment | Primary app | Entry phase | Governance | Screening criteria |
|---|---|---|---|---|---|---|
| `JOB.PER.01` | Quality engineer at a contract machining job shop running multi-vendor CMMs | `office` | `report` | P1a | `lean` | Produces customer inspection reports (e.g. AS9102 / FAI) from CMM outputs at least monthly |
| `JOB.PER.02` | Scan inspector comparing optical/laser scan clouds to CAD models | `lab` | `inspect` | P1b | `lean` | Aligns 3D scan data to CAD and evaluates GD&T tolerances in regular production or FAI work |
| `JOB.PER.03` | Shop-floor operator entering hand-gauge readings and running released inspections | `shop_floor` | `inspect` | P1b | `lean` | Operates kiosk or shop-floor inspection stations and records go/no-go or hand-gauge checks |
| `JOB.PER.04` | CMM programmer writing, simulating and running measurement programs | `lab` | `probe` | P2 | `lean` | Authors or maintains CMM probing programs (DMIS or vendor equivalents) |
| `JOB.PER.05` | Quality manager releasing reports, tracking SPC trends and owning sign-off rules | `office` | `report` | P1a | `regulated` | Reviews, signs and releases inspection packages under a quality management system |
| `JOB.PER.06` | Auditor or calibration-lab reviewer verifying received results packages | `office` | `verifier` | P0 | `regulated` | Audits measurement records, traceability chains or calibration certificates |
| `JOB.PER.07` | Supplier quality engineer receiving and dispositioning supplier packages | `office` | `report` | P1a | `regulated` | Evaluates incoming supplier FAI or lot-inspection packages against drawing tolerances |
| `JOB.PER.08` | Instrument-maker engineer authoring a measuring-device driver | `lab` | `sdk` | P2 | `lean` | Develops or integrates metrology hardware drivers, controllers or SDK bridges |
| `JOB.PER.09` | Large-volume metrologist running laser-tracker networks for jigs and fixtures | `field` | `network` | P3 | `lean` | Operates multi-station laser-tracker networks for large-assembly alignment or build |

### Sample size per round

- **Discovery inquiry (P0):** at least 5 practising participants spanning at least 2 personas
  (prioritising `JOB.PER.01`, `JOB.PER.02`, `JOB.PER.03`, `JOB.PER.05`, `JOB.PER.06`).
- **Release usability test (P1a+):** at least 5 practising participants matching the primary
  personas of the apps changed in that release.

## 2. Tasks by acceptance-job ID

Usability test sessions evaluate the acceptance jobs defined by `FLOW` for each changed app:

| Acceptance Job ID | Task summary | Target persona(s) | Primary app |
|---|---|---|---|
| `JOB.ACC.01` | Scan first article inspection from CAD and PMI to released PDF report | `JOB.PER.02` | `inspect` / `report` |
| `JOB.ACC.02` | AS9102 first-article transcription from an existing CMM output file to released report | `JOB.PER.01`, `JOB.PER.05` | `report` |
| `JOB.ACC.03` | Drawing-only first-article inspection combining CMM, arm and hand-gauge readings | `JOB.PER.01`, `JOB.PER.03` | `report` / `inspect` |

Additional acceptance jobs added by `FLOW` for P2 (`probe`, `sdk`) and P3 (`network`) enter
subsequent protocol versions by their `JOB.ACC.*` IDs.

In P0 discovery inquiry sessions (before GUI apps ship), tasks are structured contextual
walkthroughs of the participant's current workflow steps (setup, alignment, evaluation,
reporting, verification), recorded strictly in the observer's own words with no vendor files
or screenshots captured.

## 3. Session scripts

### 3.1 Opening and consent verification (5 minutes)

1. Confirm the participant has read and signed or verbally recorded consent under
   `design/research/consent.md`, and note the assigned participant code `P-<study>-<NN>` and
   consent reference on the private key sheet. **Do not start any recording or task without
   recorded consent.**
2. State clearly: *"We are testing {{FAMILY_NAME}}, not you. You can pause, skip any question,
   or stop and withdraw at any time without giving a reason. Nothing with your name or company
   will be published."*
3. Confirm whether the separate recording-publication opt-in is checked or unchecked.

### 3.2 Warm-up and context (5 minutes)

1. Ask the participant to describe their typical parts, instruments and reporting deliverables
   in their own words (without sharing confidential customer drawings or vendor screenshots).
2. Confirm persona match (`JOB.PER.01`..`JOB.PER.09`).

### 3.3 Think-aloud tasks (35–45 minutes)

1. For each acceptance job in the session's task order, read the task prompt aloud and provide
   the synthetic public dataset (never proprietary customer data unless kept solely on the
   participant's own machine).
2. Ask the participant to narrate what they expect and what surprises them.
3. **Observer neutrality:** do not hint, point, or touch the keyboard while a task is timed.
   If the participant asks a question, reply *"What would you expect to happen here?"*
4. Provide an assist only if the participant is blocked for more than 2 minutes or explicitly
   asks to move on; increment `assists` and mark task completion `assisted` (or `fail` if
   abandoned).

### 3.4 Debrief and SUS questionnaire (10 minutes)

1. Administer the standard ten-item System Usability Scale (Brooke, 1996; 1 to 5 scale) with
   {{FAMILY_NAME}} as the system name, without rewording or dropping items.
2. Ask two open debrief questions: *"Which single moment caused the most hesitation?"* and
   *"What did you trust least about the numbers on screen?"*
3. Thank the participant and remind them how to request withdrawal or deletion under
   `design/research/consent.md`.

## 4. Metrics

For every usability test session (`UX.RESEARCH.USAB`, `usab-session@1`):

- **Per task (`job_id`):**
  - `completion`: `success` (completed unassisted), `assisted` (completed after observer hint),
    or `fail` (abandoned or wrong output).
  - `time_s`: elapsed task duration in seconds.
  - `errors`: count of user or workflow errors made during the task.
  - `assists`: count of observer interventions.
- **Per participant:**
  - `sus`: ten integer item responses (1..5), scored to 0..100 in `UX.RESEARCH.SUS`.
- **Per release (`UX.RESEARCH.SUS`):**
  - Participant count `n` (minimum 5), SUS mean, 95 % confidence interval, task completion
    rates, and usability regression check (flag if SUS mean drops more than 5 points below the
    previous release [unverified threshold]).

## 5. Severity scale reference

Issues observed during research sessions are rated on the four-level severity scale defined in
`design/research/severity.md` (`UX.RESEARCH.USAB`):

- **Level 1 (Blocks task):** prevents completing the task or causes an incorrect measurement
  or wrong conformance decision.
- **Level 2 (Major delay or error):** causes severe hesitation, repeated errors, or requires
  observer assistance.
- **Level 3 (Minor):** causes brief hesitation or workaround; participant recovers unassisted.
- **Level 4 (Cosmetic):** visual or wording polish issue with no task impact.

Every issue rated Level 1 or Level 2 must be filed as a tracked issue naming the affected spec
node ID before the research report is published.
