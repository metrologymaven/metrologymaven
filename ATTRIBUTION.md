# Third-party assets (CONSTITUTION §8)

Every third-party asset in this repository: code ported from, or test material taken
from, a source the project does not own. Each entry gives the files, the licence (SPDX ID
where one exists) and the notice the licence asks us to keep. Add an entry in the same PR
that adds the asset.

## Code

### Sun fdlibm (via FreeBSD msun and musl)

- **Files:** `crates/fid-num/src/f/vendor/` (`trig.rs`, `rem_pio2.rs`, `inv_trig.rs`,
  `exp_log.rs`, `hypot_cbrt.rs`, `mod.rs`); provenance and changes in
  `crates/fid-num/src/f/vendor/VENDOR.md`.
- **What:** algorithms and coefficient tables of sin, cos, tan, argument reduction, asin,
  acos, atan, atan2, exp, log and cbrt (cbrt via FreeBSD `s_cbrt.c`, optimized by Bruce D.
  Evans), ported by hand to Rust.
- **Licence:** SPDX `SunPro`. Status: pending maintainer decision **D-040**.
- **Notice (kept in every file header):**

  > Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
  > Developed at SunPro, a Sun Microsystems, Inc. business.
  > Permission to use, copy, modify, and distribute this software is freely granted,
  > provided that this notice is preserved.

### musl libc

- **Files:** the same `crates/fid-num/src/f/vendor/` files (musl's versions of the
  functions above; `hypot` is musl's own algorithm).
- **Upstream:** <https://git.musl-libc.org/cgit/musl/tree/src/math>
- **Licence:** SPDX `MIT`. Status: pending maintainer decision **D-040**.
- **Notice:** "Copyright © 2005-2020 Rich Felker, et al." with the full MIT permission
  text, kept in every file header and in `VENDOR.md`.

## Test material

- **Random123 Philox4x32-10 known-answer vectors** (D. E. Shaw Research): the first three
  rows of `fixtures/num/rng-philox-kat-v1.txt`, also in `fixtures/num/make_rng_fixtures.py`.
  Licence of the `kat_vectors` file: [unverified], pending D-038.
- **RFC 8785 (JSON Canonicalization Scheme) sample** in
  `fixtures/core/jrn/canon/vectors.json`. Terms of IETF document text and code
  components: [unverified], pending D-038.
