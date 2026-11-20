# Design review checklist

Spec node: `UX.SYSTEM.REVIEW` (REQ-UX.SYSTEM.REVIEW-001 .. -006). Licence: CC-BY-4.0.

Every PR that changes a path listed in `review/paths.toml` passes this review before merge. The
reviewer writes a `design-review@1` record in `review/records/` in the same PR, and
`fid-design review-check` checks it. Only a person approves: a record whose approver is an
agent or a bot account never passes.

**The CLI cannot verify who approved.** `approver.name` and `approver.principal` are text that
anyone, including an agent, can write. Two things outside the CLI back them up:
`.github/CODEOWNERS` assigns `design/review/records/` to the human maintainer, and the CI
wrapper `tools/design_review_gate.py` requires each record's `approver.name` to be the GitHub
login of a non-bot reviewer who approved the PR's head commit. Neither can tell a person from an
agent that holds that person's credentials. Write the approver's GitHub login as `name`.

The product is named only `{{FAMILY_NAME}}` in every design artefact until decision D-002 names
it. Candidate names are never written in this repository.

## Checklist items

Each item has an ID. The record gives one result per item: `pass`, `fail` or `n/a` with a
reason. A record with any `fail` does not approve; fix the artefact and review again.

- **C-PRINCIPLES** The artefacts follow the design principles in `principles.md`. The record
  lists, in `principles`, the ID of every principle checked (for example `P-DENSE`, `P-NOCOLOR`).
- **C-ORIGINALITY** Every artefact in the record has an `originality@1` declaration (format
  below), with the author, the date, the references consulted described in words, and the
  originality statement.
- **C-DRESS** Every trade-dress distinctness rule in `review/dress-rules.json` was checked. Each
  rule is its own item: the record's `dress` list holds one result per rule ID, and a missing
  rule fails the check.
- **C-NOCOLOR** No status is shown by colour alone: every coloured status cue also has a text
  label, an icon shape or a pattern.
- **C-KEYBOARD** Every action in the changed design has a keyboard path (focus order, shortcut or
  menu), written down in the artefact or the PR.
- **C-CONTRAST** Contrast evidence from the token check is attached: the contrast ratios that the
  token tooling reports for every foreground and background pair the change uses.
- **C-NAME** The artefacts name the product only as `{{FAMILY_NAME}}` and contain no candidate
  product name. Maintainers run `review-check --deny-names <private file>` locally; the deny list
  is never committed.

## Originality statement (`originality-statement@1`)

> I made this artefact for this project. I did not copy any code, text, icon, screenshot or
> layout of another product into it. The references I list are things I looked at, described in
> my own words; none of them is an image or a file of another product.

## Formats

`originality@1`, stored beside the artefact as `<file>.origin.json` or under
`review/origin/<file>.origin.json` (one of the two, not both):

```json
{
  "artefact": "icons/probe.svg",
  "author": "A. Contributor",
  "date": "2026-11-19",
  "references": ["A hand sketch of a touch probe made for this project"],
  "statement_version": "originality-statement@1"
}
```

References are words only: no links, no image files, no images of another product.

`design-review@1`, stored as `review/records/<name>.json`:

```json
{
  "pr": 123,
  "artefacts": ["icons/probe.svg"],
  "digests": { "icons/probe.svg": "sha256:<64 lowercase hex of the file as approved>" },
  "principles": ["P-DATA", "P-NOCOLOR"],
  "dress": [{ "rule": "<rule id>", "result": "pass" }],
  "checklist": [
    { "item": "C-CONTRAST", "result": "n/a", "reason": "No colour changed." },
    { "item": "C-DRESS", "result": "pass" },
    { "item": "C-KEYBOARD", "result": "pass" },
    { "item": "C-NAME", "result": "pass" },
    { "item": "C-NOCOLOR", "result": "pass" },
    { "item": "C-ORIGINALITY", "result": "pass" },
    { "item": "C-PRINCIPLES", "result": "pass" }
  ],
  "approver": { "name": "<GitHub login>", "principal": "human" },
  "date": "2026-11-19"
}
```

All paths are relative to the design directory (or to the repository with `--base`) and use `/`.
A record counts only in the PR that adds or changes it, only when its `pr` is the PR given with
`--pr`, and only while every `digests` entry matches the file on disk (`"deleted"` for an
artefact the PR deletes). Any edit to an artefact after approval makes the record stale
(`REVIEW-RECORD-STALE`); review it again. Governance files under `review/` need no
originality declaration. `review-check --all` lists every file under the map that lacks one. Text in records and declarations must be in Unicode NFC and must not hold
control, bidirectional, invisible or compatibility characters or words that mix Latin with
Cyrillic or Greek letters; such text is refused, not cleaned.

## Trade-dress rules

`review/dress-rules.json` holds `[{ "id": .., "text_ref": .. }]`. A maintainer copies the rule
IDs from the brand guidelines (`OUTR.BRAND.GUIDE.DRESS`) in a reviewed PR. Until that PR lands
the slot is empty, and `review-check` refuses every record: no design can be approved without
the trade-dress rules.
