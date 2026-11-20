#!/usr/bin/env python3
"""Reference minimum-degree orderings for KRN.SOLV.CONIC.PATCH AC-4.

An independent Python implementation of the ordering rule that patch 0006 puts into the
vendored solver's LDL path (`fid_clarabel::qdldl::min_degree_ordering`):

  The elimination graph has an edge {i, j} for every stored off-diagonal entry (i, j) of
  the upper-triangular CSC pattern (values and the diagonal are ignored). At each step the
  uneliminated node with the smallest current degree is eliminated, ties going to the
  lowest index; its remaining neighbours then form a clique. perm[k] is the k-th pivot.

The patterns are hand-chosen (arrow, path, grid) plus pseudo-random ones from a fixed
linear congruential generator, so the fixture does not depend on Python's `random`.
Writes orders.txt next to this script; `--check` verifies the committed file instead.
Standard library only.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, 'orders.txt')


def min_degree(n, colptr, rowval):
    adj = [set() for _ in range(n)]
    for col in range(n):
        for k in range(colptr[col], colptr[col + 1]):
            row = rowval[k]
            if row != col:
                adj[row].add(col)
                adj[col].add(row)
    alive = [True] * n
    perm = []
    for _ in range(n):
        p = min((i for i in range(n) if alive[i]), key=lambda i: (len(adj[i]), i))
        perm.append(p)
        alive[p] = False
        nbrs = adj[p]
        adj[p] = set()
        for u in nbrs:
            adj[u].discard(p)
            adj[u] |= (nbrs - {u})
    return perm


def triu_csc(n, edges):
    """Upper-triangular CSC pattern with a full diagonal and the given edges (i < j)."""
    cols = [[c] for c in range(n)]
    for (i, j) in edges:
        a, b = min(i, j), max(i, j)
        if a != b and a not in cols[b]:
            cols[b].append(a)
    colptr, rowval = [0], []
    for c in range(n):
        rowval.extend(sorted(cols[c]))
        colptr.append(len(rowval))
    return colptr, rowval


def lcg(seed):
    state = seed
    while True:
        state = (6364136223846793005 * state + 1442695040888963407) % (1 << 64)
        yield state >> 33


def cases():
    out = []
    # arrow: node 0 linked to every other node
    n = 7
    out.append(('arrow7', n, [(0, j) for j in range(1, n)]))
    # path 0-1-...-9
    n = 10
    out.append(('path10', n, [(i, i + 1) for i in range(n - 1)]))
    # 4x4 grid (5-point stencil)
    g = 4
    edges = []
    for r in range(g):
        for c in range(g):
            v = r * g + c
            if c + 1 < g:
                edges.append((v, v + 1))
            if r + 1 < g:
                edges.append((v, v + g))
    out.append(('grid4x4', g * g, edges))
    # KKT-like pattern [P A'; A -I]: 3 variables, 4 constraints (hs35 structure)
    out.append(('kkt_hs35', 7, [(0, 1), (0, 2), (0, 3), (1, 3), (2, 3), (0, 4), (1, 5), (2, 6)]))
    # pseudo-random symmetric patterns
    for seed, n, m in [(1, 12, 20), (2, 25, 40), (3, 40, 90)]:
        rng = lcg(seed)
        edges = []
        for _ in range(m):
            i = next(rng) % n
            j = next(rng) % n
            if i != j:
                edges.append((i, j))
        out.append(('random%d_n%d' % (seed, n), n, edges))
    return out


def render():
    lines = ['# KRN.SOLV.CONIC.PATCH AC-4: minimum-degree orderings of upper-triangular CSC patterns,',
             '# computed by make_orders.py (independent reference of the patch 0006 rule).',
             '# Each case: name n / colptr / rowval / perm (space-separated integers).']
    for name, n, edges in cases():
        colptr, rowval = triu_csc(n, edges)
        perm = min_degree(n, colptr, rowval)
        lines.append('case %s %d' % (name, n))
        lines.append('colptr ' + ' '.join(map(str, colptr)))
        lines.append('rowval ' + ' '.join(map(str, rowval)))
        lines.append('perm ' + ' '.join(map(str, perm)))
    return '\n'.join(lines) + '\n'


def main():
    text = render()
    if '--check' in sys.argv[1:]:
        with open(OUT, encoding='utf-8', newline='') as f:
            if f.read() != text:
                print('orders.txt is stale: rerun make_orders.py', file=sys.stderr)
                return 1
        print('orders.txt: ok')
        return 0
    with open(OUT, 'w', encoding='utf-8', newline='\n') as f:
        f.write(text)
    return 0


if __name__ == '__main__':
    sys.exit(main())
