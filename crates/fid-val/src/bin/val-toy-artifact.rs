//! Deterministic toy kernel artifact speaking the `fid-kernel-wasm` / `fid-xart` stdin/stdout
//! protocol for `fixtures/man/val-toy-v1` (`KRN.MAN.VAL.RUN` AC-1..AC-4).
//!
//! Uses only `fid-num::f` transcendentals so outputs are bit-identical across Windows, Linux
//! and macOS.

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_kci::kci::Sha256;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::ExitCode;

const MAX_CALL_BYTES: usize = 8 * 1024 * 1024;

fn emit_error(code: &str) -> ExitCode {
    let mut o = BTreeMap::new();
    o.insert("error".into(), CValue::Str(code.into()));
    if let Ok(bytes) = to_canonical_bytes(&CValue::Obj(o)) {
        let _ = std::io::stdout().write_all(&bytes);
    }
    ExitCode::from(1)
}

fn emit_ok(algorithm: &str, output: BTreeMap<String, CValue>) -> ExitCode {
    let output_val = CValue::Obj(output);
    let Ok(out_bytes) = to_canonical_bytes(&output_val) else {
        return emit_error("encode_failed");
    };
    let mut h = Sha256::new();
    h.update(&out_bytes);
    let digest = Digest::from_bytes(h.finish());

    let mut env = BTreeMap::new();
    env.insert("algorithm".into(), CValue::Str(algorithm.into()));
    env.insert("output".into(), output_val);
    env.insert("output_digest".into(), CValue::Str(digest.as_str().into()));
    if let Ok(bytes) = to_canonical_bytes(&CValue::Obj(env)) {
        let _ = std::io::stdout().write_all(&bytes);
        ExitCode::SUCCESS
    } else {
        emit_error("encode_failed")
    }
}

fn parse_f64_bits(v: &CValue) -> Option<f64> {
    match v {
        CValue::Num(x) if x.is_finite() => Some(*x),
        CValue::Str(s) => {
            let hex = s.strip_prefix("0x")?;
            if hex.len() != 16 {
                return None;
            }
            let bits = u64::from_str_radix(hex, 16).ok()?;
            let f = f64::from_bits(bits);
            f.is_finite().then_some(f)
        }
        _ => None,
    }
}

fn abs_f64(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & !(1u64 << 63))
}

fn plus_zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

/// Deterministic 3x3 symmetric Jacobi eigen-decomposition returning the unit eigenvector of
/// the smallest eigenvalue, sign-canonicalised (first component with `|c| > 1e-12` positive).
fn smallest_eigenvector_3x3(mut a: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        for &(p, q) in &[(0usize, 1usize), (0, 2), (1, 2)] {
            let apq = a[p][q];
            if abs_f64(apq) <= 1e-30 {
                continue;
            }
            let tau = (a[q][q] - a[p][p]) / (2.0 * apq);
            let root = fid_num::f::sqrt(1.0 + tau * tau).ok()?;
            let t = if tau >= 0.0 { 1.0 / (tau + root) } else { -1.0 / (-tau + root) };
            let c = 1.0 / fid_num::f::sqrt(1.0 + t * t).ok()?;
            let s = t * c;

            let app = a[p][p];
            let aqq = a[q][q];
            a[p][p] = app - t * apq;
            a[q][q] = aqq + t * apq;
            a[p][q] = 0.0;
            a[q][p] = 0.0;
            for r in 0..3 {
                if r != p && r != q {
                    let arp = a[r][p];
                    let arq = a[r][q];
                    a[r][p] = c * arp - s * arq;
                    a[p][r] = a[r][p];
                    a[r][q] = s * arp + c * arq;
                    a[q][r] = a[r][q];
                }
            }
            for row in &mut v {
                let vp = row[p];
                let vq = row[q];
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    let mut best = 0usize;
    if a[1][1] < a[best][best] {
        best = 1;
    }
    if a[2][2] < a[best][best] {
        best = 2;
    }
    let mut e = [v[0][best], v[1][best], v[2][best]];
    let n = fid_num::f::sqrt(e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).ok()?;
    if n == 0.0 || !n.is_finite() {
        return None;
    }
    e = [e[0] / n, e[1] / n, e[2] / n];
    // Canonical sign.
    for c in e {
        if abs_f64(c) > 1e-12 {
            if c < 0.0 {
                e = [-e[0], -e[1], -e[2]];
            }
            break;
        }
    }
    Some([plus_zero(e[0]), plus_zero(e[1]), plus_zero(e[2])])
}

fn fit_plane(pts: &[[f64; 3]]) -> Result<([f64; 3], [f64; 3], f64), &'static str> {
    if pts.len() < 3 {
        return Err("insufficient_points");
    }
    let n = pts.len() as f64;
    let mut c = [0.0; 3];
    for p in pts {
        c[0] += p[0];
        c[1] += p[1];
        c[2] += p[2];
    }
    c = [plus_zero(c[0] / n), plus_zero(c[1] / n), plus_zero(c[2] / n)];
    let mut cov = [[0.0; 3]; 3];
    for p in pts {
        let d = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j];
            }
        }
    }
    let normal = smallest_eigenvector_3x3(cov).ok_or("degenerate_plane")?;
    let mut ss = 0.0;
    for p in pts {
        let r = (p[0] - c[0]) * normal[0] + (p[1] - c[1]) * normal[1] + (p[2] - c[2]) * normal[2];
        ss += r * r;
    }
    let rms = fid_num::f::sqrt(ss / n).map_err(|_| "numeric_error")?;
    Ok((c, normal, plus_zero(rms)))
}

fn solve_3x3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for col in 0..3 {
        let mut piv = col;
        for row in (col + 1)..3 {
            if abs_f64(a[row][col]) > abs_f64(a[piv][col]) {
                piv = row;
            }
        }
        if abs_f64(a[piv][col]) <= 1e-18 {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        let diag = a[col][col];
        for row in (col + 1)..3 {
            let f = a[row][col] / diag;
            a[row][col] = 0.0;
            for k in (col + 1)..3 {
                a[row][k] -= f * a[col][k];
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = [0.0; 3];
    for i in (0..3).rev() {
        let mut s = b[i];
        for k in (i + 1)..3 {
            s -= a[i][k] * x[k];
        }
        x[i] = s / a[i][i];
        if !x[i].is_finite() {
            return None;
        }
    }
    Some(x)
}

struct CircleFit {
    centre: [f64; 3],
    normal: [f64; 3],
    radius: f64,
    rms: f64,
}

fn fit_circle(pts: &[[f64; 3]]) -> Result<CircleFit, &'static str> {
    if pts.len() < 3 {
        return Err("insufficient_points");
    }
    let (centroid, normal, _) = fit_plane(pts)?;
    // Orthonormal 2D frame (u, v) in plane orthogonal to `normal`.
    let helper = if abs_f64(normal[0]) < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u_raw = [
        normal[1] * helper[2] - normal[2] * helper[1],
        normal[2] * helper[0] - normal[0] * helper[2],
        normal[0] * helper[1] - normal[1] * helper[0],
    ];
    let un = fid_num::f::sqrt(u_raw[0] * u_raw[0] + u_raw[1] * u_raw[1] + u_raw[2] * u_raw[2])
        .map_err(|_| "degenerate_circle")?;
    if un == 0.0 {
        return Err("degenerate_circle");
    }
    let u = [u_raw[0] / un, u_raw[1] / un, u_raw[2] / un];
    let v =
        [normal[1] * u[2] - normal[2] * u[1], normal[2] * u[0] - normal[0] * u[2], normal[0] * u[1] - normal[1] * u[0]];

    // Kasa algebraic fit in 2D local coordinates: 2*cx*x + 2*cy*y + d = x^2 + y^2.
    let mut ata = [[0.0; 3]; 3];
    let mut atb = [0.0; 3];
    for p in pts {
        let dp = [p[0] - centroid[0], p[1] - centroid[1], p[2] - centroid[2]];
        let x = dp[0] * u[0] + dp[1] * u[1] + dp[2] * u[2];
        let y = dp[0] * v[0] + dp[1] * v[1] + dp[2] * v[2];
        let row = [2.0 * x, 2.0 * y, 1.0];
        let rhs = x * x + y * y;
        for i in 0..3 {
            atb[i] += row[i] * rhs;
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
        }
    }
    let sol = solve_3x3(ata, atb).ok_or("degenerate_circle")?;
    let (cx, cy, d) = (sol[0], sol[1], sol[2]);
    let r_sq = cx * cx + cy * cy + d;
    if r_sq <= 0.0 || !r_sq.is_finite() {
        return Err("degenerate_circle");
    }
    let radius = fid_num::f::sqrt(r_sq).map_err(|_| "numeric_error")?;
    let centre = [
        plus_zero(centroid[0] + cx * u[0] + cy * v[0]),
        plus_zero(centroid[1] + cx * u[1] + cy * v[1]),
        plus_zero(centroid[2] + cx * u[2] + cy * v[2]),
    ];
    let mut ss = 0.0;
    for p in pts {
        let dp = [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]];
        let x = dp[0] * u[0] + dp[1] * u[1] + dp[2] * u[2];
        let y = dp[0] * v[0] + dp[1] * v[1] + dp[2] * v[2];
        let rad_i = fid_num::f::sqrt(x * x + y * y).map_err(|_| "numeric_error")?;
        let dr = rad_i - radius;
        ss += dr * dr;
    }
    let rms = fid_num::f::sqrt(ss / (pts.len() as f64)).map_err(|_| "numeric_error")?;
    Ok(CircleFit { centre, normal, radius: plus_zero(radius), rms: plus_zero(rms) })
}

fn vec3_val(v: [f64; 3]) -> CValue {
    CValue::Arr(vec![CValue::F64Bits(v[0]), CValue::F64Bits(v[1]), CValue::F64Bits(v[2])])
}

fn main() -> ExitCode {
    let mut raw = Vec::new();
    if std::io::stdin().take((MAX_CALL_BYTES as u64) + 1).read_to_end(&mut raw).is_err() {
        return emit_error("io_error");
    }
    if raw.len() > MAX_CALL_BYTES {
        return emit_error("call_too_large");
    }
    let Ok(CValue::Obj(o)) = parse_with_limits(&raw, MAX_CALL_BYTES, 16) else {
        return emit_error("malformed_call");
    };
    let Some(CValue::Str(algo)) = o.get("algorithm") else {
        return emit_error("missing_algorithm");
    };
    let Some(CValue::Arr(pts_val)) = o.get("points") else {
        return emit_error("missing_points");
    };
    let mut pts = Vec::with_capacity(pts_val.len());
    for pv in pts_val {
        let CValue::Arr(coords) = pv else {
            return emit_error("bad_point");
        };
        if coords.len() != 3 {
            return emit_error("bad_point");
        }
        let (Some(x), Some(y), Some(z)) =
            (parse_f64_bits(&coords[0]), parse_f64_bits(&coords[1]), parse_f64_bits(&coords[2]))
        else {
            return emit_error("bad_point");
        };
        pts.push([x, y, z]);
    }

    match algo.as_str() {
        "ls.plane@1.0.0" => match fit_plane(&pts) {
            Ok((point, normal, rms)) => {
                let mut out = BTreeMap::new();
                out.insert("normal".into(), vec3_val(normal));
                out.insert("point".into(), vec3_val(point));
                out.insert("rms".into(), CValue::F64Bits(rms));
                emit_ok(algo, out)
            }
            Err(code) => emit_error(code),
        },
        "ls.circle@1.0.0" => match fit_circle(&pts) {
            Ok(fit) => {
                let mut out = BTreeMap::new();
                out.insert("centre".into(), vec3_val(fit.centre));
                out.insert("normal".into(), vec3_val(fit.normal));
                out.insert("radius".into(), CValue::F64Bits(fit.radius));
                out.insert("rms".into(), CValue::F64Bits(fit.rms));
                emit_ok(algo, out)
            }
            Err(code) => emit_error(code),
        },
        _ => emit_error("unknown_algorithm"),
    }
}
