// Licence notices of the upstream sources this file is ported from (kept as required):
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunPro, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================
//
// musl libc, src/math (MIT licence):
//
// Copyright © 2005-2020 Rich Felker, et al.
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! sin, cos, tan: musl `sin.c`, `cos.c`, `tan.c` with kernels `__sin.c`, `__cos.c`,
//! `__tan.c` (fdlibm). Inputs are finite.

use super::rem_pio2::rem_pio2;
use super::{hi_word, with_lo_zero};
use core::f64::consts::FRAC_PI_4;

const S1: f64 = -0.16666666666666632; // 0xBFC5555555555549
const S2: f64 = 0.00833333333332249; // 0x3F8111111110F8A6
const S3: f64 = -0.0001984126982985795; // 0xBF2A01A019C161D5
const S4: f64 = 2.7557313707070068e-06; // 0x3EC71DE357B1FE7D
const S5: f64 = -2.5050760253406863e-08; // 0xBE5AE5E68A2B9CEB
const S6: f64 = 1.58969099521155e-10; // 0x3DE5D93A5ACFD57C

const C1: f64 = 0.0416666666666666; // 0x3FA555555555554C
const C2: f64 = -0.001388888888887411; // 0xBF56C16C16C15177
const C3: f64 = 2.480158728947673e-05; // 0x3EFA01A019CB1590
const C4: f64 = -2.7557314351390663e-07; // 0xBE927E4F809C52AD
const C5: f64 = 2.087572321298175e-09; // 0x3E21EE9EBDB4B1C4
const C6: f64 = -1.1359647557788195e-11; // 0xBDA8FAE9BE8838D4

const T: [f64; 13] = [
    0.3333333333333341,      // 0x3FD5555555555563
    0.13333333333320124,     // 0x3FC111111110FE7A
    0.05396825397622605,     // 0x3FABA1BA1BB341FE
    0.021869488294859542,    // 0x3F9664F48406D637
    0.0088632398235993,      // 0x3F8226E3E96E8493
    0.0035920791075913124,   // 0x3F6D6D22C9560328
    0.0014562094543252903,   // 0x3F57DBC8FEE08315
    0.0005880412408202641,   // 0x3F4344D8F2F26501
    0.0002464631348184699,   // 0x3F3026F71A8D1068
    7.817944429395571e-05,   // 0x3F147E88A03792A6
    7.140724913826082e-05,   // 0x3F12B80F32F0A7E9
    -1.8558637485527546e-05, // 0xBEF375CBDB605373
    2.590730518636337e-05,   // 0x3EFB2A7074BF7AD4
];
const PIO4LO: f64 = 3.061616997868383e-17; // 0x3C81A62633145C07

/// sin(x + y) on [-pi/4, pi/4]; `iy == 0` means y is zero.
fn k_sin(x: f64, y: f64, iy: bool) -> f64 {
    let z = x * x;
    let w = z * z;
    let r = S2 + z * (S3 + z * S4) + z * w * (S5 + z * S6);
    let v = z * x;
    if iy {
        x - ((z * (0.5 * y - v * r) - y) - v * S1)
    } else {
        x + v * (S1 + z * r)
    }
}

/// cos(x + y) on [-pi/4, pi/4].
fn k_cos(x: f64, y: f64) -> f64 {
    let z = x * x;
    let w = z * z;
    let r = z * (C1 + z * (C2 + z * C3)) + w * w * (C4 + z * (C5 + z * C6));
    let hz = 0.5 * z;
    let w = 1.0 - hz;
    w + (((1.0 - w) - hz) + (z * r - x * y))
}

/// tan(x + y) on [-pi/4, pi/4], or -1/tan(x + y) when `odd`.
fn k_tan(x: f64, y: f64, odd: bool) -> f64 {
    let hx = hi_word(x);
    let big = (hx & 0x7FFF_FFFF) >= 0x3FE5_9428; // |x| >= 0.6744
    let sign = hx >> 31 == 1;
    let (mut x, mut y) = (x, y);
    if big {
        if sign {
            x = -x;
            y = -y;
        }
        x = (FRAC_PI_4 - x) + (PIO4LO - y);
        y = 0.0;
    }
    let z = x * x;
    let w = z * z;
    let r = T[1] + w * (T[3] + w * (T[5] + w * (T[7] + w * (T[9] + w * T[11]))));
    let v = z * (T[2] + w * (T[4] + w * (T[6] + w * (T[8] + w * (T[10] + w * T[12])))));
    let s = z * x;
    let r = y + z * (s * (r + v) + y) + s * T[0];
    let w = x + r;
    if big {
        let s = if odd { -1.0 } else { 1.0 };
        let v = s - 2.0 * (x + (r - w * w / (w + s)));
        return if sign { -v } else { v };
    }
    if !odd {
        return w;
    }
    // -1/(x + r) computed accurately.
    let w0 = with_lo_zero(w);
    let v = r - (w0 - x);
    let a = -1.0 / w;
    let a0 = with_lo_zero(a);
    a0 + a * (1.0 + a0 * w0 + a0 * v)
}

pub(crate) fn sin(x: f64) -> f64 {
    let ix = hi_word(x) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        if ix < 0x3E50_0000 {
            return x;
        }
        return k_sin(x, 0.0, false);
    }
    let (n, y0, y1) = rem_pio2(x);
    match n {
        0 => k_sin(y0, y1, true),
        1 => k_cos(y0, y1),
        2 => -k_sin(y0, y1, true),
        _ => -k_cos(y0, y1),
    }
}

pub(crate) fn cos(x: f64) -> f64 {
    let ix = hi_word(x) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        if ix < 0x3E46_A09E {
            return 1.0;
        }
        return k_cos(x, 0.0);
    }
    let (n, y0, y1) = rem_pio2(x);
    match n {
        0 => k_cos(y0, y1),
        1 => -k_sin(y0, y1, true),
        2 => -k_cos(y0, y1),
        _ => k_sin(y0, y1, true),
    }
}

pub(crate) fn tan(x: f64) -> f64 {
    let ix = hi_word(x) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        if ix < 0x3E40_0000 {
            return x;
        }
        return k_tan(x, 0.0, false);
    }
    let (n, y0, y1) = rem_pio2(x);
    k_tan(y0, y1, n & 1 == 1)
}
