# Design-partner timing and usability study: protocol

- **Protocol version:** `study-protocol@1` (version 1, 2026-11-18). Any change to a rule below gets a new version; each study round states the version it ran under.
- **Spec node:** FLOW.STUDY.PROTO (parent FLOW.STUDY).
- **Task list:** [`acceptance/study/tasks.json`](../../acceptance/study/tasks.json), format `study-tasks@1`, checked by `cargo xtask study-tasks-check`.
- **Record format:** every session produces records in the format `study-record@1` defined by FLOW.STUDY.RECORD. Nothing else leaves a partner site.

The targets in this protocol (80 % first-time success, SUS 70) are low-confidence targets from concept doc 10 [unverified]. Maintainers confirm or change them after the first round of sessions; a change is a new protocol version.

## 1. Why this study exists

The nightly acceptance suite measures machine time and correctness on public data. It cannot measure how long a person takes to do a real job, whether a newcomer succeeds on their own, or how usable the product feels. Only design partners, at their own sites and on their own parts, can measure that. This protocol says how those sessions run so that the results can be compared from release to release and repeated by others.

## 2. What is out of scope here

- **Finding and signing up partners.** Recruitment and every contact with a prospective partner are done by a named maintainer, never by an agent or a script. This document does not describe outreach.
- **The record format and its import** (FLOW.STUDY.RECORD), **public claims** (FLOW.STUDY.CLAIM) and **wider usability research** such as interviews and think-aloud tests (UX.RESEARCH). Where UX.RESEARCH.PROTO sets rules for consent and data handling, this protocol follows them (section 9).

## 3. Roles

| Role | Who | Does |
|---|---|---|
| Study lead | A named maintainer | Plans the round, holds the consent records and the token key outside the public repository, imports records with `fid accept import-timing` (a human-only command). |
| Observer | The study lead or a second maintainer, present on site or by video call that the partner controls | Runs the tutorial, keeps the stopwatch, fills in the observer sheet (section 8). Never touches the keyboard during a timed task. |
| Participant | A person at the partner who does the work the task describes | Does the tasks, answers the SUS questionnaire. |
| Partner contact | A person at the partner who agreed to the study | Confirms which licensed tool, parts and data are used; keeps all part data on site. |

## 4. Partners and sessions

1. A study round for a release needs **at least 3 partners** (3 to 5 is the plan).
2. Each partner runs **at least 2 sessions per studied release**. Sessions are on different days so that the second session is not just a repeat a few minutes later.
3. A partner is an organisation; the records identify it only by a random partner token (section 9). Several participants at one partner share its token; the record's `session` field tells sessions apart.
4. A release counts as studied for a job only when at least 3 distinct partner tokens have records for that job and release.

## 5. Tasks, start and end

The studied tasks are the entries of `acceptance/study/tasks.json`. Each entry names an acceptance job, the journaled command that starts the clock, the journaled command that stops it, the report package variant the task must produce, and the persona journey maps and personas it is drawn from.

| Job | Short name | Start event (file open) | End event (export of the released report) | Report variant | Journey map | Persona |
|---|---|---|---|---|---|---|
| JOB.ACC.01 | J1, scan first article from CAD and PMI | `import.cad_step@1` | `report.export_pdf@1` | `scan-gdt-report` | `per-02` | JOB.PER.02 |
| JOB.ACC.02 | J2, AS9102 transcription from an existing CMM report | `import.results_dmis@1` | `report.export_pdf@1` | `fai-as9102-report` | `per-01` | JOB.PER.01 |
| JOB.ACC.03 | J3, drawing-only first article from CMM, arm and gauge data | `import.results_csv@1` | `report.export_pdf@1` | `fai-drawing-only-report` | `per-01` | JOB.PER.01 |

Rules:

1. **Start** is the journal timestamp of the first committed start command in the session (the participant opens the task's first source file).
2. **End** is the journal timestamp of the first committed end command that exports the report after it was released (adopted). An export of an unreleased draft does not stop the clock.
3. Task time is end minus start. The observer also times the same span with a stopwatch, starting when the participant confirms the file-open dialog and stopping when the export completes. Both go into the record; FLOW.STUDY.RECORD flags records where they differ by more than 10 %.
4. The journal is the project's own local journal on the partner's machine. Only the two timestamps are copied to the record; the journal itself stays on site.
5. When a job's file-open is done with a different import command (for example a partner whose CMM writes CSV, not DMIS), the observer notes the command used and the session still counts; the task list names the usual case.
6. A task is abandoned when the participant gives up or a time cap set by the study lead before the round is reached. An abandoned task is recorded as not successful and has no end timestamp in the median.

## 6. Incumbent baseline

1. The partner times **its own licensed tool** on **the same task and the same data** it uses with Metrology Maven, in the same session or one close to it.
2. The partner runs the incumbent tool. The observer only reads the stopwatch; the project never operates, installs or receives the incumbent tool, its files or its output.
3. Start and end follow the same meaning as section 5: opening the first source file to exporting the finished report.
4. The baseline time goes into the record's optional `incumbent_s` field and nowhere else. Records are imported into the project's acceptance store, which is **not public**: it is not part of the public repository, not published, and readable only by maintainers running the study. Under D-021, incumbent baselines are **internal study records only**: they are not published, not compared in public, and not named against a product, until a later decision allows it.
5. Order: half of the partners do the incumbent run first and half do it second, so learning the part does not favour one tool.

## 7. Usability measures

### 7.1 Unassisted first-time success (JOB.USE.02)

1. Measured on **J1, J2 and J3** (JOB.ACC.01, JOB.ACC.02, JOB.ACC.03).
2. Before the first attempt the participant does **one tutorial**: the guided tutorial for that app shipped with the release, replayed once in step mode. Nothing else: no demo by the observer, no written help beyond what the product shows.
3. **First-time success** means the participant reaches the end event (section 5) on the first attempt, **unassisted**: the observer gave no help of any kind (no hint, no answer to a question about the product, no pointing). The product's own help, "Why?" panel and suggested fixes are allowed.
4. If the observer helps, the record's `assisted` flag is set and the attempt does not count as a success. Safety or data-loss interventions are help too.
5. The first-time success rate is successes divided by first attempts, per job and release. **Bar: at least 80 %** [unverified].

### 7.2 System Usability Scale (JOB.USE.03)

1. After the last task of the session the participant fills in the **standard ten-item System Usability Scale** (Brooke, 1996), unchanged except for using the product name. The project does not reword, drop or add items.
2. The score is computed with the standard SUS scoring, giving 0 to 100, and goes into the record's `sus` field. Answers to the single items are not recorded; only the score leaves the site.
3. The mean SUS per job and release is compared with the **bar of at least 70** [unverified].

## 8. Observer sheet

The observer sheet has exactly the fields of a `study-record@1` record, one sheet per task attempt, and nothing that could identify a person, a part or a company.

| Sheet field | Record field | What the observer writes |
|---|---|---|
| Job | `job_id` | The job ID from the task list, e.g. `JOB.ACC.02`. |
| Release | `release` | The product release under study. |
| Partner token | `partner_token` | The partner's random token (section 9), never the partner's name. |
| Session | `session` | Session number for this partner and release. |
| Governance profile | `profile` | The profile the project ran under. |
| Journal start / end | `journal_start`, `journal_end` | The two journal timestamps from section 5. |
| Stopwatch | `stopwatch_s` | Stopwatch seconds. |
| Success | `success` | Yes only for an unassisted first-time success (7.1). |
| SUS | `sus` | The 0 to 100 score (7.2). |
| Assisted | `assisted` | Yes if the observer helped at all. |
| Incumbent time | `incumbent_s` (optional) | The partner's own tool on the same task and data (section 6). |

Free-text notes, if any, are written on the observer's own copy of the sheet and kept in the **study's private store**: storage held by the study lead outside the public repository and outside the project's acceptance store (the same place as the consent records and the token key, section 9). They are never imported and never committed.

Guidance for notes: write no names of people or companies, no part numbers, drawing numbers or job numbers, and no customer or programme details; refer to people only by participant code and to the partner only by token. If any of these is written by mistake, the observer redacts it (strikes it out unreadably, or deletes it from a digital note) before the notes leave the session.

## 9. Consent, data minimisation and anonymisation

These rules follow the research rules of UX.RESEARCH.PROTO (consent form, participant codes, raw data outside the public repository, deletion schedule, no research through telemetry without opt-in). Where that node's documents exist under `design/research/`, they apply in full; this section adds what is specific to timing sessions.

### 9.1 Informed consent

1. No session starts until each participant has given informed consent, signed or recorded, and the partner organisation has agreed to the study in writing.
2. The consent covers: the purpose (measuring task time and usability of the product), what is recorded (the fields of section 8 and nothing else; no audio, video or screen recording unless the participant separately opts in), how long raw notes, recordings, consent records and imported records are kept (section 9.4), who sees raw data (the study lead and observer only), and the right to stop or withdraw at any time without giving a reason.
3. Withdrawal means the participant's records are not imported, or are deleted from the project's acceptance store if already imported, and their raw notes and any recordings are destroyed. To make this possible, the study lead's key (9.3) maps each participant code to the (partner token, session, job) of every record that participant produced.
4. Participants are told before they consent that aggregates already published (FLOW.STUDY.CLAIM) cannot be withdrawn: a withdrawal removes their records from every later aggregate, but a published summary is not recalled. Published aggregates never identify a participant.
5. **Recordings.** Audio, video or screen recording happens only if the participant opted in to it separately. Recordings are kept in the study's private store (section 8), never in the public repository or the acceptance store, and are deleted with the raw notes (9.4). A recording, or any part of it, is published only if the participant also gave the separate opt-in for publishing recordings that UX.RESEARCH.PROTO-002 requires, and never if it shows part data, drawings or the partner's identity.
6. Consent records are referenced only by participant code `P-<study>-<NN>` and are held by the study lead outside the public repository.

### 9.2 What leaves the partner site

1. **Part data, drawings, CAD, scans, CMM output, reports, screenshots, journals and the partner's identity stay at the partner site.** Only the record fields of `study-record@1` (section 8) leave it.
2. FLOW.STUDY.RECORD enforces this at import: a file with any other field, any attachment or any long string is rejected whole.
3. The project collects no study data through product telemetry. Telemetry stays off unless the user opted in (CONSTITUTION C7), and even then it is not used as study data.

### 9.3 Anonymisation

1. Each partner gets a **partner token**: 128 random bits written as hex, generated by the study lead for this study. It carries no information about the partner.
2. The key that links tokens and participant codes to real organisations and people, and each participant code to the (partner token, session, job) of that participant's records, is kept only by the study lead in the study's private store, outside the public repository and outside the project's acceptance store.
3. Nothing in this repository names a partner, a participant, a part or a customer. Published summaries (FLOW.STUDY.CLAIM) show aggregates only, never single records or tokens.

### 9.4 Retention

1. **No session may start until the retention periods below have been decided by the maintainers and written into the consent form** the participant signs. None is decided yet [unverified: to be set before the first session].
2. Raw notes, recordings and the token key are deleted at the end of the period stated in the consent.
3. Imported records in the project's acceptance store are kept for the period stated in the consent and then deleted; aggregates already published stay published (9.1).
4. Consent records are kept for the period stated in the consent (long enough to show that each imported record had consent), then deleted.

## 10. Agents

Agents may help prepare a round (checking the task list, drafting observer sheets) but do not run sessions, contact partners or participants, or import records. The import, `accept.import_timing@1`, is a human-only command.

## 11. Changing this protocol

The task list and this document change only through a pull request. `cargo xtask study-tasks-check` must pass: every task's job has a journey map, every command ID has the form `domain.verb_object@N`, every named map and persona exists, and this document cites `study-record@1`, `study-tasks@1` and `acceptance/study/tasks.json`.
