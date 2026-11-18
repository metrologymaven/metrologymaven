"""Independent stdlib reference geometry for the first evaluation corpus (AGENT.EVAL.CORPUS).

Least-squares (Gaussian) associations only, written from first principles for the synthetic
golden journals in make_corpus.py. Nothing here calls or mirrors the Rust kernel; it is the
"second implementation" the corpus oracles are computed by.

All functions take and return millimetres (the unit of the synthetic input files).

Bit-reproducible across platforms and Python versions: only +, -, *, / and math.sqrt (all
correctly rounded in IEEE 754) are used on the data, sums run left to right (not the builtin
sum, whose float algorithm changed in Python 3.12), and no libm function (sin, cos, atan2,
hypot) touches a value that reaches a golden journal.
"""

import math


def _sum(values):
    """Plain left-to-right float sum."""
    s = 0.0
    for v in values:
        s += v
    return s


def _solve(a, b):
    """Solve the square system a x = b by Gaussian elimination with partial pivoting."""
    n = len(b)
    m = [list(map(float, row)) + [float(b[i])] for i, row in enumerate(a)]
    for col in range(n):
        piv = max(range(col, n), key=lambda r: abs(m[r][col]))
        if m[piv][col] == 0.0:
            raise ValueError("singular system")
        m[col], m[piv] = m[piv], m[col]
        for r in range(col + 1, n):
            f = m[r][col] / m[col][col]
            for c in range(col, n + 1):
                m[r][c] -= f * m[col][c]
    x = [0.0] * n
    for r in range(n - 1, -1, -1):
        s = m[r][n] - _sum(m[r][c] * x[c] for c in range(r + 1, n))
        x[r] = s / m[r][r]
    return x


def circle_fit(points):
    """Least-squares (geometric, orthogonal-distance) circle through 2D points.

    Algebraic (Kasa) start, then Gauss-Newton on sum (|p - c| - r)^2.
    Returns (cx, cy, r). Needs at least 3 points.
    """
    n = len(points)
    if n < 3:
        raise ValueError("a circle needs at least 3 points")
    mx = _sum(p[0] for p in points) / n
    my = _sum(p[1] for p in points) / n
    # Kasa on centred coordinates: u^2 + v^2 = 2 a u + 2 b v + c.
    suu = svv = suv = 0.0
    suz = svz = sz = 0.0
    for x, y in points:
        u, v = x - mx, y - my
        z = u * u + v * v
        suu += u * u
        svv += v * v
        suv += u * v
        suz += u * z
        svz += v * z
        sz += z
    a, b = _solve([[2 * suu, 2 * suv], [2 * suv, 2 * svv]], [suz, svz])
    cx, cy = mx + a, my + b
    r = math.sqrt(a * a + b * b + sz / n)
    for _ in range(200):
        jtj = [[0.0] * 3 for _ in range(3)]
        jtr = [0.0] * 3
        for x, y in points:
            dx, dy = x - cx, y - cy
            d = math.sqrt(dx * dx + dy * dy)
            res = d - r
            j = [-dx / d, -dy / d, -1.0]
            for i in range(3):
                jtr[i] += j[i] * res
                for k in range(3):
                    jtj[i][k] += j[i] * j[k]
        step = _solve(jtj, [-v for v in jtr])
        cx, cy, r = cx + step[0], cy + step[1], r + step[2]
        if max(abs(s) for s in step) < 1e-13:
            break
    else:
        raise ValueError("circle fit did not converge")
    return cx, cy, r


def _jacobi_eigen(s):
    """Eigenvalues and eigenvectors (columns) of a symmetric 3x3 matrix (cyclic Jacobi)."""
    a = [row[:] for row in s]
    v = [[1.0 if i == j else 0.0 for j in range(3)] for i in range(3)]
    for _ in range(100):
        off = _sum(a[i][j] * a[i][j] for i in range(3) for j in range(3) if i != j)
        if off < 1e-30:
            break
        for p in range(3):
            for q in range(p + 1, 3):
                if a[p][q] == 0.0:
                    continue
                theta = (a[q][q] - a[p][p]) / (2 * a[p][q])
                t = (1.0 if theta >= 0 else -1.0) / (abs(theta) + math.sqrt(theta * theta + 1))
                c = 1 / math.sqrt(t * t + 1)
                sn = t * c
                for k in range(3):
                    akp, akq = a[k][p], a[k][q]
                    a[k][p], a[k][q] = c * akp - sn * akq, sn * akp + c * akq
                for k in range(3):
                    apk, aqk = a[p][k], a[q][k]
                    a[p][k], a[q][k] = c * apk - sn * aqk, sn * apk + c * aqk
                for k in range(3):
                    vkp, vkq = v[k][p], v[k][q]
                    v[k][p], v[k][q] = c * vkp - sn * vkq, sn * vkp + c * vkq
    return [a[i][i] for i in range(3)], v


def plane_fit(points):
    """Least-squares (orthogonal) plane through 3D points: (centroid, unit normal)."""
    n = len(points)
    if n < 3:
        raise ValueError("a plane needs at least 3 points")
    c = [_sum(p[i] for p in points) / n for i in range(3)]
    s = [[_sum((p[i] - c[i]) * (p[j] - c[j]) for p in points) for j in range(3)] for i in range(3)]
    vals, vecs = _jacobi_eigen(s)
    k = min(range(3), key=lambda i: vals[i])
    nrm = [vecs[i][k] for i in range(3)]
    if nrm[2] < 0:
        nrm = [-x for x in nrm]
    return c, nrm


def flatness_lsq(points):
    """Peak-to-valley of orthogonal residuals from the least-squares plane."""
    c, nrm = plane_fit(points)
    res = [_sum((p[i] - c[i]) * nrm[i] for i in range(3)) for p in points]
    return max(res) - min(res)


def line_fit(points):
    """Least-squares (orthogonal) 2D line: (centroid, unit normal)."""
    n = len(points)
    if n < 2:
        raise ValueError("a line needs at least 2 points")
    mx = _sum(p[0] for p in points) / n
    my = _sum(p[1] for p in points) / n
    sxx = _sum((p[0] - mx) * (p[0] - mx) for p in points)
    syy = _sum((p[1] - my) * (p[1] - my) for p in points)
    sxy = _sum((p[0] - mx) * (p[1] - my) for p in points)
    # Normal = eigenvector of the smallest eigenvalue of [[sxx, sxy], [sxy, syy]].
    lam = 0.5 * (sxx + syy) - math.sqrt(0.25 * (sxx - syy) * (sxx - syy) + sxy * sxy)
    u = (sxy, lam - sxx)
    w = (lam - syy, sxy)
    nx, ny = u if u[0] * u[0] + u[1] * u[1] >= w[0] * w[0] + w[1] * w[1] else w
    ln = math.sqrt(nx * nx + ny * ny)
    if ln == 0.0:
        raise ValueError("degenerate line")
    return (mx, my), (nx / ln, ny / ln)


def slot_width(side_a, side_b):
    """Width between two opposite slot flanks: distance of the flank centroids along the
    mean of the two least-squares flank normals."""
    ca, na = line_fit(side_a)
    cb, nb = line_fit(side_b)
    if na[0] * nb[0] + na[1] * nb[1] < 0:
        nb = (-nb[0], -nb[1])
    nx, ny = na[0] + nb[0], na[1] + nb[1]
    ln = math.sqrt(nx * nx + ny * ny)
    nx, ny = nx / ln, ny / ln
    return abs((cb[0] - ca[0]) * nx + (cb[1] - ca[1]) * ny)
