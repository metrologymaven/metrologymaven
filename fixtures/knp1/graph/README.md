# fixtures/knp1/graph

Hand-written fixtures for spec node KRN.NUM.LINT.GRAPH (crate `fid-knp1-lint`, module `graph`).
Path crates under `vendor/` stand in for third-party crates. The lockfiles are written by hand in
Cargo's v4 format; nothing here was produced by running the Rust code. Expected results
(`expected-closure.json`, `expected-findings.txt`) are hand-labelled from the spec's rules.

- `ok/`: root `fid-num` -> vendored `ext-a` (build-dep `ext-b`, proc-macro `ext-m`); `fid-num` also
  dev-depends on `ext-dev` (GPL, unlisted), which must stay out of the closure. AC-1, AC-3.
- `unlisted/`: the `ok/` workspace with `ext-b` missing from the allowlist. AC-2.
- `version/`: the `ok/` workspace with `ext-a` allowed only as `^2`. REQ-003.
- `unresolved/`: `fid-num` depends on registry crate `ext-r`, absent from `vendor/`. AC-4.
- `exclude-src/`: a config that excludes `src/solver.rs`. AC-5.

CC-BY-4.0 project data.
