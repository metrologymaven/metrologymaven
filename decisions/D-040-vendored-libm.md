---
id: D-040
title: Vendored SunPro and MIT libm ports in fid-num
status: accepted
raised_by: review of PR #72 (KRN.NUM.FN)
blocks: [KRN.NUM.FN]
decide_by: before PR #72 merges
updated: 2026-11-18
---

# D-040: Vendored SunPro and MIT libm ports in fid-num

**Status: ACCEPTED 2026-11-18, maintainer.**

## Question

May `fid-num` contain a hand port of the fdlibm (SPDX `SunPro`) and musl (SPDX `MIT`) math
library sources, with their notices kept? If so, how is that reconciled with the
clean-room rule, and how is vendored code licence-checked?

## Context

- KRN.NUM.FN (REQ-KRN.NUM.FN-002) requires every transcendental except sqrt to come from
  "one vendored, version-pinned pure-Rust libm source tree" and names a musl-derived crate
  as the candidate. PR #72 implements this as `crates/fid-num/src/f/vendor/`: a hand port of
  the fdlibm/musl algorithms and coefficient tables, with project changes (integer
  large-argument reduction, atan2 accuracy fix). Provenance: `VENDOR.md`; notices: every
  file header and the root `ATTRIBUTION.md`.
- CONSTITUTION §8 says "Clean room. Public specs and observed behaviour only. No copied
  code". A port of published, permissively licensed source with notices kept is not clean
  room in that sense. The constitution and the leaf spec conflict.
- §8 also says the dependency licence allowlist is set by D-001. `SunPro` is not a common
  allowlist entry; `MIT` is. Whether the allowlist covers vendored source (not only crate
  dependencies) is not stated.
- `cargo deny` (deny.toml) only sees crate dependencies and their declared licences. It
  cannot see code vendored into a crate's own source tree, so licence checking of vendored
  code needs another mechanism (ATTRIBUTION.md review, a vendor manifest check, or the
  KRN.NUM.LINT node).
- The alternative, a clean-room implementation from published mathematics only (minimax
  polynomials fitted by the project, its own reduction), is feasible but larger, and loses
  the decades of scrutiny the fdlibm coefficients have had.

## Options

| Option | For | Against |
|---|---|---|
| A. Allow permissively licensed (SunPro, MIT) ports in `fid-num` with notices kept, listed in ATTRIBUTION.md and VENDOR.md; amend §8 with a named exception | Matches the spec; well-tested algorithms; already built and verified (≤ 0.82 ulp, bit-identical on all CI targets) | Exception to the clean-room rule; needs `SunPro` on the allowlist; vendored code invisible to cargo-deny |
| B. Allow only MIT/Apache sources (port from musl's MIT tree and treat fdlibm's notice as satisfied by keeping it) | Single common licence family | fdlibm notice still applies to the derived code; legally the same as A |
| C. Require a clean-room implementation (own polynomials and reduction) | Satisfies §8 as written | More work; new numerical risk; PR #72's port would be replaced |

## Recommendation

A, with a §8 amendment naming "vendored numeric ports with notices kept, listed in
ATTRIBUTION.md" as an exception, `SunPro` added to the D-001 allowlist for vendored source,
and a CI check (KRN.NUM.LINT or ECO) that every directory named `vendor/` has a `VENDOR.md`
and an ATTRIBUTION.md entry. [unverified: legal reading of the SunPro notice for derived
works; counsel may want to confirm.]

## Decision

**Accepted 2026-11-18, maintainer.**

1. The code in `crates/fid-num/src/f/vendor/` is a port of permissively licensed fdlibm
   (SunPro notice) and musl (MIT) algorithms. Their notices are preserved in
   `crates/fid-num/src/f/vendor/VENDOR.md` (and in the file headers and `ATTRIBUTION.md`).
2. This is an explicit, recorded exception to the clean-room rule of CONSTITUTION §8,
   limited to that directory. It does not extend to any other directory or crate.
3. The port (not a verbatim rust-lang/libm copy) is accepted, including the documented
   atan2 and Payne-Hanek deviations recorded in `VENDOR.md`.
