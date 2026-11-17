---
id: OUTR.COMM.SURVEY.TOOL
title: Survey tool criteria and tracking check
level: 4
parent: OUTR.COMM.SURVEY
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM.SURVEY-002]
covers: []
depends_on: []
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.SURVEY.TOOL: Survey tool criteria and tracking check

## Purpose

The criteria a survey tool must meet before the project uses it (self-hostable, no IP logging, no fingerprinting, no third-party cookies or scripts, raw export and deletion), the assessment record a maintainer fills for a candidate tool, and a static check of the survey form page that catches third-party scripts, cookies and hidden tracking before a survey opens.

## Scope

**In:** `community/surveys/TOOL-CRITERIA.md`; the assessment record format; the `survey-tool check` command over an assessment record and a saved copy of the survey form page.
**Out:** question sets and survey definitions (`OUTR.COMM.SURVEY.QSET`); aggregation and deletion (`OUTR.COMM.SURVEY.AGG`); hosting the tool (maintainers' infrastructure).

## Requirements

- **REQ-OUTR.COMM.SURVEY.TOOL-001** The criteria shall require that the tool can be self-hosted, can be configured to store no IP address and no device fingerprint, sets no third-party cookie, loads no third-party script, exports raw responses, and deletes responses on request.
- **REQ-OUTR.COMM.SURVEY.TOOL-002** An assessment record shall answer each criterion with `pass` or `fail` and a note, name the tool version and the assessor, and `survey-tool check` shall fail if any criterion is missing or `fail`.
- **REQ-OUTR.COMM.SURVEY.TOOL-003** If the saved survey form page loads a script, stylesheet, font, image or frame from an origin other than the survey's own origin, then the check shall fail with `third_party_resource` naming the URL.
- **REQ-OUTR.COMM.SURVEY.TOOL-004** If the saved form page contains a hidden input or script that reads browser fingerprinting properties (canvas, WebGL, audio context, font enumeration or user-agent client hints) [unverified completeness of this list], then the check shall fail with `fingerprinting_suspected`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/comm/survey/tool/ok/` (an assessment with all criteria pass and a form page using only same-origin resources), when `fid-outreach survey-tool check --assessment assessment.toml --form form.html --origin https://survey.example.org --json` runs, then it exits 0.
- **AC-2** (error path) Given `cdn-script/` (a `<script src>` on another origin), when the check runs, then it exits 1 with `third_party_resource` and the URL.
- **AC-3** (error path) Given `canvas/` (an inline script calling `toDataURL` on a canvas), when the check runs, then it exits 1 with `fingerprinting_suspected`.
- **AC-4** (error path) Given `ip-fail/` (the IP-storage criterion answered fail), when the check runs, then it exits 1 naming that criterion.

## Design

- **Command:** `fid-outreach survey-tool check --assessment <file> --form <file> --origin <url> [--json]`. Project tooling; nearest class R0.
- **Assessment record:** `tool`, `version`, `assessor`, `date`, `[criteria] self_hosted, no_ip_storage, no_fingerprint, no_third_party_cookies, no_third_party_scripts, raw_export, deletion = { result, note }`.
- **Form scan:** bounded HTML tokenizer; collects `src`, `href`, `action`, `srcset` and CSS `url()` values; inline scripts scanned for a fixed list of fingerprinting API names.
- **Errors:** `criterion_missing`, `criterion_failed`, `third_party_resource`, `fingerprinting_suspected`, `parse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `survey::tool`; criteria document in the outreach repository.

## Test fixtures

Hand-written assessments and HTML pages under `fixtures/outr/comm/survey/tool/` (`ok`, `cdn-script`, `canvas`, `ip-fail`), CC-BY-4.0.

## Build prompt

> In the outreach repository, write `community/surveys/TOOL-CRITERIA.md` in your own words: a survey tool must be self-hostable, configurable to store no IP address and no device fingerprint, set no third-party cookie, load no third-party script, export raw responses and delete them on request.
>
> In the Rust crate `fid-outreach`, add module `survey::tool` with `fid-outreach survey-tool check --assessment <file> --form <file> --origin <url> [--json]`. The assessment TOML has `tool`, `version`, `assessor`, `date` and `[criteria]` with `self_hosted`, `no_ip_storage`, `no_fingerprint`, `no_third_party_cookies`, `no_third_party_scripts`, `raw_export`, `deletion`, each `{ result = "pass" | "fail", note }`: report `criterion_missing` or `criterion_failed`. Scan the saved form HTML (bounded tokenizer) for `src`, `href`, `action`, `srcset` and CSS `url()` values on another origin (`third_party_resource <url>`), and inline scripts using canvas `toDataURL` or `getImageData`, WebGL context, `AudioContext`, font enumeration or user-agent client hints (`fingerprinting_suspected`). Exit 0, 1 findings, 2 unreadable input.
>
> Tests with fixtures under `fixtures/outr/comm/survey/tool/`: ok, third-party script, canvas fingerprinting, failed criterion.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; BTreeMap rather than HashMap.

## Open questions

- Which self-hostable survey tool meets the criteria is not yet researched [unverified]; the first assessment record decides it.

## Children

_Leaf: none._
