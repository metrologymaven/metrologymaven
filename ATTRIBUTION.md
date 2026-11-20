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
- **Licence:** SPDX `SunPro`. Status: accepted by maintainer decision **D-040** (2026-11-18), a recorded clean-room exception limited to this directory.
- **Notice (kept in every file header):**

  > Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
  > Developed at SunPro, a Sun Microsystems, Inc. business.
  > Permission to use, copy, modify, and distribute this software is freely granted,
  > provided that this notice is preserved.

### musl libc

- **Files:** the same `crates/fid-num/src/f/vendor/` files (musl's versions of the
  functions above; `hypot` is musl's own algorithm).
- **Upstream:** <https://git.musl-libc.org/cgit/musl/tree/src/math>
- **Licence:** SPDX `MIT`. Status: accepted by maintainer decision **D-040** (2026-11-18), a recorded clean-room exception limited to this directory.
- **Notice:** "Copyright © 2005-2020 Rich Felker, et al." with the full MIT permission
  text, kept in every file header and in `VENDOR.md`.

### Clarabel.rs (interior-point conic solver)

- **Files:** `vendor/fid-clarabel/upstream/` (the pristine crates.io package clarabel 0.11.1,
  commit `25540f559592068d0c8a80e46ded1b21760212a1`), `vendor/fid-clarabel/patches/` and the
  patched crate `vendor/fid-clarabel/crate/` (fid-clarabel); audit record
  `vendor/fid-clarabel/AUDIT.toml` (KRN.SOLV.CONIC.VEND, KRN.SOLV.CONIC.PATCH).
- **Upstream:** <https://github.com/oxfordcontrol/Clarabel.rs>, by Paul Goulart and Yuwen Chen
  (University of Oxford).
- **Licence:** SPDX `Apache-2.0` (crates.io metadata, and the licence text in
  `vendor/fid-clarabel/upstream/LICENSE.md`, kept unchanged as `crate/LICENSE.md`). The patch
  files record every change file by file, and (Apache-2.0 section 4(b)) every modified or
  new file of `crate/` starts with the notice "Modified from Clarabel 0.11.1 by Metrology
  Maven; see vendor/fid-clarabel/patches/" and `crate/README.md` starts with a fork notice
  (patch 0009).
- **Notice:** upstream ships no NOTICE file; its `LICENSE.md` and `CITATION.bib` are kept in
  both trees.

## Test material

- **Random123 Philox4x32-10 known-answer vectors** (D. E. Shaw Research): the first three
  rows of `fixtures/num/rng-philox-kat-v1.txt`, also in `fixtures/num/make_rng_fixtures.py`.
  Licence of the `kat_vectors` file: [unverified], pending D-038.
- **RFC 8785 (JSON Canonicalization Scheme) sample** in
  `fixtures/core/jrn/canon/vectors.json`. Terms of IETF document text and code
  components: [unverified], pending D-038.

## Fonts

The report fonts bundled in `crates/fid-report-pdf/assets/fonts/` (spec node
`RPT.OUTPUT.PDF.TEXT`, REQ-005) are **not** third-party: their outlines are original work
of the project, generated from the stroke skeletons in
`crates/fid-report-pdf/fontgen/make_fonts.py` with the Python standard library. They are
listed here because the spec requires every bundled font to appear in this file with its
licence. No third-party font, font tool or outline was used.

| File | Family | SPDX licence |
|---|---|---|
| `MavenReportSans-Regular.ttf` | Maven Report Sans, weight 400 | `MIT OR Apache-2.0` |
| `MavenReportSans-Bold.ttf` | Maven Report Sans, weight 700 | `MIT OR Apache-2.0` |
| `MavenReportMono-Regular.ttf` | Maven Report Mono, weight 400 | `MIT OR Apache-2.0` |
| `MavenReportMono-Bold.ttf` | Maven Report Mono, weight 700 | `MIT OR Apache-2.0` |

The test fonts `fixtures/rpt/output/text/script-test.ttf` and `composite-test.ttf` come
from the same script and carry the same licence.
