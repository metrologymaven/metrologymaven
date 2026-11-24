# {{FAMILY_NAME}} Research Data Handling and Publication Check

Spec node: `UX.RESEARCH.PROTO` (REQ-UX.RESEARCH.PROTO-003, -004, -005). Licence: CC-BY-4.0.

This document defines how participant data is coded, stored, anonymised, deleted and checked
before publication.

## 1. Participant code scheme

Every published research file identifies participants **only** by an anonymised participant code
of the form:

```text
P-<study>-<NN>
```

- `<study>` is a short lowercase study identifier (`[a-z0-9][a-z0-9-]*`, e.g. `inq01`, `r01`).
- `<NN>` is a zero-padded sequence number of two or more digits (`01`, `02`, ..., `15`).
- Examples: `P-inq01-01`, `P-r01-05`.

Every published session record that names a participant code must also carry a non-empty
`consent_ref` confirming recorded informed consent before the session began.

## 2. Private storage outside the public repository

The following artefacts must **never** be committed to git or uploaded to public issue trackers:

- raw session notes and transcripts;
- audio or video recordings (unless a specific anonymised screen clip has separate opt-in
  consent under `design/research/consent.md` section 6);
- signed consent forms;
- the **private code key** (`key.csv`), which maps `P-<study>-<NN>` to participant names.

The private code key uses a minimal CSV format stored in access-controlled maintainer storage:

```csv
code,name
P-inq01-01,Participant Full Name
```

## 3. Deletion schedule

- Raw notes, recordings, signed consents and `key.csv` are deleted no later than the maximum
  retention period stated in `design/research/consent.md` (12 months [unverified] from the
  session date).
- If a participant withdraws at any time, their raw notes, recordings and row in `key.csv` are
  deleted promptly and the deletion date is logged privately by the study lead.

## 4. Telemetry ban (CONSTITUTION C7)

Research data is never collected via product telemetry unless the participant has explicitly
opted in to that telemetry. Session observers never enable telemetry on a participant's machine
without separate explicit consent.

## 5. Incumbent workflows and image provenance (CONSTITUTION C9)

- Describe incumbent metrology workflows only in the project's own words. Never commit
  incumbent screenshots, icons, dialog text or vendor files.
- Every published raster image (`.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`, `.bmp`, `.tif`,
  `.tiff`) must carry a sidecar `<image>.capture.json` (`research-capture@1`) proving it was
  captured from {{FAMILY_NAME}} using the project's own capture command:

```json
{
  "format": "research-capture@1",
  "image": "screens/run-01.png",
  "command": "fid viz capture",
  "date": "2026-11-22"
}
```

## 6. Pre-publication check (`fid-design research-check`)

Before committing any research output under `design/research/`, run:

```sh
fid-design research-check --root <research output dir> --key <private key.csv> [--json]
```

What `research-check` enforces:

1. **`RESEARCH-PII email`:** fails if any text file contains an e-mail address.
2. **`RESEARCH-PII phone`:** fails if any text file contains a phone number.
3. **`RESEARCH-PII name`:** fails if any file or path contains a name from `--key` (matched
   case-insensitively after Unicode NFKC and hidden-character stripping). **The key is read
   only from `--key`, never written, and matched names are never printed or logged.**
4. **`RESEARCH-IMAGE-PROVENANCE`:** fails if any raster image lacks a valid
   `<image>.capture.json` sidecar naming a project capture command (`fid <app> capture`).
5. **`RESEARCH-INCUMBENT`:** fails if any file is flagged as an incumbent artefact.
6. **`RESEARCH-CONSENT-REF`:** fails if a session/study record uses an invalid participant
   code or omits `consent_ref`.
7. **`RESEARCH-SCHEMA`:** fails on malformed sidecars/JSON, symbolic links or an invalid key
   file.

Exit codes: `0` clean, `1` findings, `2` unreadable input or bad usage.
