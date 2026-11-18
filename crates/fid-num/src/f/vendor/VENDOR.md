# Vendored libm: provenance and changes

Pin: `fid-num-libm 1` (`VENDOR_VERSION` in `mod.rs`). Any change that can alter an output
bit bumps this version and the golden digests in `fixtures/num/fn-selftest-v1.txt`.

## Upstream

| Function | Upstream algorithm (musl `src/math/`, itself from FreeBSD msun / Sun fdlibm 5.3) |
|---|---|
| sin, cos, tan | `sin.c`, `cos.c`, `tan.c`, kernels `__sin.c`, `__cos.c`, `__tan.c` |
| argument reduction | `__rem_pio2.c` medium path (large path replaced, see below) |
| asin, acos | `asin.c`, `acos.c` |
| atan, atan2 | `atan.c`, `atan2.c` (atan2 changed, see below) |
| exp | `exp.c` in its fdlibm-derived form (musl up to 1.1.x) and `scalbn.c` |
| ln | `log.c` in its fdlibm-derived form (musl up to 1.1.x) |
| hypot | `hypot.c` (double-length squares by splitting) |
| cbrt | `cbrt.c` |

- musl: <https://git.musl-libc.org/cgit/musl/tree/src/math>, MIT licence.
- The `libm` crate of the rust-lang organisation (<https://github.com/rust-lang/libm>,
  MIT OR Apache-2.0) is a Rust port of the same musl sources and was the spec's candidate.

**How this tree was made.** It is a hand port of the algorithms and coefficient tables
listed above, written for this repository; it is not a byte-for-byte copy of any upstream
commit, so no upstream commit hash applies [deviation from the KRN.NUM.FN design, stated in
the PR]. Every coefficient is written as the shortest decimal that round-trips to the
upstream bit pattern (given in a comment), and `fixtures/num/make_fn_fixtures.py` checks
each bit pattern against the decimal value published in the fdlibm sources. The table of
2/pi and floor(pi/2 * 2^127) are computed by that script from Machin's formula.

## Licence notices

The fdlibm-derived algorithms carry this notice, preserved as it requires:

> Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
> Developed at SunPro, a Sun Microsystems, Inc. business.
> Permission to use, copy, modify, and distribute this software is freely granted,
> provided that this notice is preserved.

musl is MIT-licensed (Copyright © 2005-2020 Rich Felker, et al.). The `hypot` algorithm is
musl's own. This port is distributed under the workspace licence (MIT OR Apache-2.0).

## Project changes (all outputs are pinned by the golden digests)

1. **Generic path only.** No `mul_add`/FMA, no inline assembly, no intrinsics, no
   `cfg(target_arch)` paths, no `long double`/`double_t` excess precision: every operation
   is a binary64 `+ - * /` or `f64::sqrt` (correctly rounded), evaluated in source order.
2. **Special values removed.** NaN, infinity and out-of-domain branches (and the
   floating-point exception side effects, `FORCE_EVAL`) are gone; the facade rejects those
   arguments with typed errors before calling in.
3. **Argument reduction.** The small-|x| shortcuts of `__rem_pio2` (|x| < 9pi/4) are not
   used; the medium Cody-Waite path, valid for every |x| < 2^20 pi/2, handles them. For
   larger |x|, `__rem_pio2_large` is replaced by a Payne-Hanek reduction in exact integer
   arithmetic (`u128` limbs, 1280 bits of 2/pi, 254 fraction bits), rounded once to the
   double-double y0 + y1. It is easier to review and to model bit-exactly in Python.
4. **atan2.** musl's atan2 is up to about 1.2 ulp, above the spec's 1 ulp, because it
   rounds y/x and, for x < 0, rounds atan(|y/x|) before computing pi - atan. Two changes:
   for t = |y/x| in [2^-27, 2^27] the division residual (exact by Dekker's product) adds
   (y/x - t)/(1 + t^2) inside atan before its final rounding, and for x < 0 the result
   pi - (hi + lo) is formed with one final rounding. With x > 0 and no correction term the
   output is bit-identical to musl's.
