#!/usr/bin/env python3
"""Independent check of objectives.txt (KRN.SOLV.CONIC.PATCH AC-3).

objectives.txt records the objectives that UNPATCHED Clarabel 0.11.1 reaches on its own
in-scope example problems. This script recomputes the exact optimum of each problem in
rational arithmetic (closed form or the KKT system of the known active set, with the
active set's optimality conditions checked), and checks that every recorded objective is
within the solver tolerance (1e-6 relative) of it, that the hex bits and the decimal agree,
and that every case is present. Standard library only.
"""
import os
import struct
import sys
from fractions import Fraction as F

HERE = os.path.dirname(os.path.abspath(__file__))


def fl(x):
    return F(x)  # exact value of the binary64 literal


def solve_linear(A, b):
    """Gaussian elimination over the rationals."""
    n = len(A)
    M = [row[:] + [b[i]] for i, row in enumerate(A)]
    for c in range(n):
        p = next(r for r in range(c, n) if M[r][c] != 0)
        M[c], M[p] = M[p], M[c]
        for r in range(n):
            if r != c and M[r][c] != 0:
                f = M[r][c] / M[c][c]
                M[r] = [a - f * b for a, b in zip(M[r], M[c])]
    return [M[i][n] / M[i][i] for i in range(n)]


def exact():
    out = {}
    # example_lp: min x1 - x2, |x_i| <= 1  ->  x = (-1, 1)
    out['lp'] = F(-2)
    # example_qp: min 3 x1^2 + 2 x2^2 - x1 - 4 x2, x1 = 2 x2, |x_i| <= 1:
    # substituting x1 = 2 x2 gives 14 x2^2 - 6 x2, minimised at x2 = 3/14 (bounds inactive)
    x2 = F(3, 14)
    x1 = 2 * x2
    assert abs(x1) <= 1 and abs(x2) <= 1
    out['qp'] = 3 * x1 * x1 + 2 * x2 * x2 - x1 - 4 * x2
    # example_socp: min x2^2 s.t. (1, 2 x1 - 2, x2 - 2) in SOC, i.e. ||(2x1-2, x2-2)|| <= 1:
    # smallest |x2| is x2 = 1 (with x1 = 1), objective 0.5 * 2 * x2^2 = 1
    out['socp'] = F(1)
    # example_box: min 0.5 ||x||^2 + 1'x, |x_i| <= 1, n = 200: x = -1 (interior of the
    # unconstrained minimiser's box face), objective 200 * (1/2 - 1)
    out['box'] = 200 * (F(1, 2) - 1)
    # hs35.json: P upper triangle as stored (binary64 values taken exactly)
    p00, p01, p11, p02, p22 = fl(4.000000000000001), fl(2.0000000000000004), fl(4.000000000000001), fl(2.0), fl(2.0)
    P = [[p00, p01, p02], [p01, p11, F(0)], [p02, F(0), p22]]
    q = [F(-8), F(-6), F(-4)]
    a = [F(1), F(1), F(2)]  # active row: x1 + x2 + 2 x3 <= 3 ; x >= 0 inactive
    K = [P[i] + [a[i]] for i in range(3)] + [a + [F(0)]]
    rhs = [-qi for qi in q] + [F(3)]
    *x, lam = solve_linear(K, rhs)
    assert lam >= 0 and all(xi > 0 for xi in x), 'hs35 active set is not optimal'
    out['hs35'] = sum(x[i] * P[i][j] * x[j] for i in range(3) for j in range(3)) / 2 + sum(q[i] * x[i] for i in range(3))
    return out


def main():
    ex = exact()
    seen = []
    ok = True
    with open(os.path.join(HERE, 'objectives.txt'), encoding='utf-8') as f:
        for line in f:
            if line.startswith('#') or not line.strip():
                continue
            name, status, hexbits, dec = line.split()
            value = struct.unpack('>d', bytes.fromhex(hexbits))[0]
            if value != float(dec):
                print('%s: hex %s and decimal %s disagree' % (name, hexbits, dec))
                ok = False
            if status != 'Solved':
                print('%s: status %s' % (name, status))
                ok = False
            rel = abs(F(value) - ex[name]) / abs(ex[name])
            if rel > F(1, 10**6):
                print('%s: recorded %r is %.3g relative from the exact optimum %s' % (name, value, float(rel), ex[name]))
                ok = False
            seen.append(name)
    if seen != ['lp', 'qp', 'socp', 'box', 'hs35']:
        print('cases missing or out of order: %s' % seen)
        ok = False
    print('objectives.txt: ' + ('ok' if ok else 'FAILED'))
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
