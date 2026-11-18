# fixtures/knp1/graph

Hand-written fixtures for spec node KRN.NUM.LINT.GRAPH (crate `fid-knp1-lint`, module `graph`).
Path crates under `vendor/` stand in for third-party crates. The lockfiles are written by hand in
Cargo's v4 format; nothing here was produced by running the Rust code. Expected results
(`expected-closure.json`, `expected-findings.txt`) are hand-labelled from the spec's rules.

- `ok/`: root `fid-num` -> vendored `ext-a` (build-dep `ext-b`, proc-macro `ext-m`); `fid-num` also
  dev-depends on `ext-dev` (GPL, unlisted), which must stay out of the closure; `ext-a` pulls
  `src/nested/helper.rs` in with `#[path]` from a nested package that is not in the closure, so
  that file must be listed. AC-1, AC-3.
- `unlisted/`: the `ok/` workspace with `ext-b` missing from the allowlist. AC-2.
- `version/`: the `ok/` workspace with `ext-a` allowed only as `^2`. REQ-003.
- `unresolved/`: `fid-num` depends on registry crate `ext-r`, absent from `vendor/`. AC-4.
- `wide/`: renamed dependency (`package =`), workspace-inherited dependency, `[target.*]`
  dependency nested inside the root crate, git source, registry `name-version` layout and two
  versions of `ext-dup`; `ext-win` inherits `license.workspace = true` without being a member,
  so its licence counts as missing.
- `inherit-version/`: a vendored crate with `version.workspace = true` that is not a member:
  `KNP1-GRAPH-UNRESOLVED`.
- `exclude-src/`: a config that excludes `src/solver.rs`. AC-5.

CC-BY-4.0 project data.
