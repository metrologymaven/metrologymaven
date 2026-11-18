# Platform support tiers

Spec node `PLAT.MATRIX.POLICY.DEFS` (requirement `REQ-PLAT.MATRIX.POLICY-001`).

Every operating system version and architecture that Metrology Maven lists is a row in
[`platform/support-matrix.toml`](../../platform/support-matrix.toml), and every row has one
of three tiers. This page says what each tier means in practice: what CI runs, whether a
failure blocks a release, and what support response to expect. Which rows exist and which
tier each one gets is set elsewhere (see [Where the rows come from](#where-the-rows-come-from)).

Check this page's table with `cargo xtask plat-matrix lint-tiers`.

## Tiers

| Tier | CI obligation | Release-blocking | Support response | Promotion/demotion |
|---|---|---|---|---|
| Tier 1 | Builds and runs the full test suite on every release. | Yes: a failed, cancelled, skipped or missing Tier 1 job blocks the release. | Maintainers respond to regressions before the release ships. | Promoted only after its Tier 1 jobs pass on two consecutive release candidates; demoted at the next release after the vendor's end of support. |
| Tier 2 | Builds on every release; the test suite is not required. | No: a failure is noted in the release notes and does not block. | Best-effort fixes; no commitment that a release waits for one. | Raised to Tier 1 only under the promotion rule; its vendor end-of-support date is tracked and CI warns ahead of it. |
| Tier 3 | None: no CI job is required. | No. | Community-maintained: fixes come from contributors who use the platform. | Moves up only when maintainers add its CI jobs by pull request; Tier 1 also needs the promotion rule. |

## What the terms mean

- **Release** means a tagged release candidate and the release made from it. The release
  gate (spec node `PLAT.MATRIX.CI`) reads the job results of a candidate and returns
  `PASS` or `BLOCK` by the rules in the table.
- **Full test suite** means `cargo test --workspace` and the other checks the release
  workflow runs for that combination.
- **Support response** is stated qualitatively. Response times are a maintainer
  commitment, and no numbers are set yet; this page will state them when the maintainers
  do.
- A row's tier applies only from its `since_phase`. Before that phase, the matrix lookup
  reports the row as *planned*, not supported.

## Promotion and demotion

- **Promotion to Tier 1.** A combination becomes Tier 1 only after its Tier 1 jobs have
  passed on at least two consecutive release candidates (`REQ-PLAT.MATRIX.POLICY-007`;
  the count is [unverified] and may be tuned). The promotion check belongs to spec node
  `PLAT.MATRIX.POLICY.ROWS`.
- **Demotion at end of support.** When the vendor's end-of-support date of a Tier 1 OS
  version has passed (the extended end date when the vendor sells extended updates), the
  next release demotes it out of Tier 1 and records the date and reason in the row's
  notes (`REQ-PLAT.MATRIX.POLICY-005`). CI warns ahead of each date. The end-of-support
  list and these checks belong to spec node `PLAT.MATRIX.POLICY.EOL`.
- **Other changes** to a row's tier are made by pull request to
  `platform/support-matrix.toml`, reviewed like any other change.

## Where the rows come from

Decision D-010 sets the Tier 1 window from phase P1a: Windows 10 and Windows 11 on x86-64,
the latest two Ubuntu LTS releases on x86-64 and the latest two macOS major releases on
ARM64. Linux ARM64 is Tier 2 and Windows ARM64 Tier 3. Spec node
`PLAT.MATRIX.POLICY.ROWS` applies this to the matrix file.

## Status today

No platform is supported at any tier yet. The project has made no release, and every row
in `platform/support-matrix.toml` is a *planned* Tier 1 row from phase P1a: it records what
the repository's CI runs on each pull request today, not a support promise. The tiers stay
[unverified] until the rows above are applied and the promotion rule has been met.
