#!/usr/bin/env python3
"""Independent reference for SCAN.MESHOP.PROV; writes prov_cases.txt next to this file.

Stdlib only (hashlib, struct). Shares no code with the Rust crate fid-scan-mesh: it reads
fixtures/scan/mesh_open_cube.txt itself, simulates the four test operations of
crates/fid-scan-mesh/tests/meshop_prov.rs from the builder rules in
spec/scan/meshop/prov/NODE.md, and hashes every layout with hashlib.

Rules applied (from the spec and the crate's documented layouts):

  * GEO.TYPES element digest (layout v1, mesh kind 3, no channels):
        "FIDGEO" | 0x01 | 0x03 | frame [16] | n u64 | x[n] f64 | y[n] f64 | z[n] f64
        | n_tris u64 | (a, b, c) u64 each | n_channels u64 = 0     (little-endian)
  * Derived-mesh digest (a mesh with the synthesized attribute):
        "FIDMOP" | 0x01 | element digest [32] | n_tris u64 | flags [ceil(n_tris / 8)]
    flags LSB-first: triangle t is bit t % 8 of byte t / 8.
  * Version digest of an input: element digest without the attribute, derived digest
    with it. Every output has the attribute.
  * Flags: copy keeps the input flag, derive is the OR of its sources, create is true; an
    input without the attribute is all-false.
  * copy_vertex maps each input vertex once, in first-copy order.
  * Record (kernel canonical JSON, keys in byte order, numbers as strings):
        {"determinism":..,"inputs":[..],"kind":"scan.mesh_op_record@1","op":..,
         "output":..,"params":{..},"quality_after":Q,"quality_before":[Q..],
         "seed":"<n>"|null,"synthesized":"<count>","triangles":"<count>"}
    The quality reports Q are the `canonical` lines of the SCAN.POLY.QUAL reference
    fixtures (fixtures/scan/*.quality.txt, written by make_mesh_quality.py); the
    report does not depend on triangle or vertex order, so a filled open cube has the
    closed cube's report and a reordered open cube the open cube's.

Test operations (all copy input vertices 0..n-1 first, in order):

  fill      D0, params {"face":"z0"}: copy triangles 0..9, then create (0,2,1) and
            (0,3,2) (the closed cube's missing z = 0 face). AC-1.
  split     D0, params {"triangles":"0 1"}, on the open cube flagged with triangle 0
            synthesized: triangles 0 and 1 are split at edge midpoints m_ab, m_bc, m_ca
            (new vertices, in that order, coordinates 0.5 * (p + q)) into the derived
            children (a, m_ab, m_ca), (m_ab, b, m_bc), (m_ca, m_bc, c), (m_ab, m_bc, m_ca);
            other triangles are copied. AC-2.
  shuffle   D2, no params, seed s: copy triangles in the order of a Fisher-Yates shuffle
            of 0..9 driven by splitmix64(s): for i = 9 down to 1, j = next() % (i + 1),
            swap(i, j). AC-4 (seeds 42 and 7).
  identity  D0, no params: copy every triangle in order. AC-5, and on the fill output
            (chain).

Run:  python fixtures/scan/meshop/make_prov_cases.py [--check]
With --check nothing is written; exits non-zero if prov_cases.txt differs.
CC-BY-4.0.
"""

import hashlib
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SCAN = os.path.dirname(HERE)
OUT = os.path.join(HERE, "prov_cases.txt")
M64 = (1 << 64) - 1


def load_mesh(name):
    frame, verts, tris = None, [], []
    with open(os.path.join(SCAN, name), encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            k, *rest = line.split(" ")
            if k == "frame":
                frame = bytes.fromhex(rest[0])
            elif k == "v":
                verts.append(tuple(float(s) for s in rest))
            elif k == "t":
                tris.append(tuple(int(s) for s in rest))
            else:
                raise AssertionError(k)
    assert frame is not None and len(frame) == 16
    return frame, verts, tris


def quality_canonical(name):
    with open(os.path.join(SCAN, name + ".quality.txt"), encoding="utf-8") as f:
        for line in f:
            if line.startswith("canonical "):
                return line[len("canonical "):].strip()
    raise AssertionError(name)


def f64le(x):
    if x == 0.0:
        x = 0.0  # -0.0 is written as +0.0
    return struct.pack("<d", x)


def u64le(v):
    return struct.pack("<Q", v)


def element_digest(frame, verts, tris):
    out = [b"FIDGEO", bytes([1, 3]), frame, u64le(len(verts))]
    for k in range(3):
        out.append(b"".join(f64le(p[k]) for p in verts))
    out.append(u64le(len(tris)))
    out.append(b"".join(u64le(a) + u64le(b) + u64le(c) for (a, b, c) in tris))
    out.append(u64le(0))
    return hashlib.sha256(b"".join(out)).digest()


def flag_bytes(flags):
    b = bytearray((len(flags) + 7) // 8)
    for t, f in enumerate(flags):
        if f:
            b[t // 8] |= 1 << (t % 8)
    return bytes(b)


def derived_digest(frame, verts, tris, flags):
    assert len(flags) == len(tris)
    e = element_digest(frame, verts, tris)
    data = b"FIDMOP" + bytes([1]) + e + u64le(len(tris)) + flag_bytes(flags)
    return hashlib.sha256(data).digest()


def wire(d):
    return "sha256:" + d.hex()


class Builder:
    """The builder rules, re-implemented."""

    def __init__(self, inp_verts, inp_tris, inp_flags):
        self.iv, self.it, self.ifl = inp_verts, inp_tris, inp_flags
        self.verts, self.tris, self.flags, self.vmap = [], [], [], {}

    def copy_vertex(self, v):
        if v not in self.vmap:
            self.vmap[v] = len(self.verts)
            self.verts.append(self.iv[v])
        return self.vmap[v]

    def add_vertex(self, p):
        self.verts.append(p)
        return len(self.verts) - 1

    def copy_triangle(self, t):
        self.tris.append(tuple(self.copy_vertex(v) for v in self.it[t]))
        self.flags.append(self.ifl[t])

    def derive_triangle(self, sources, verts):
        assert sources
        self.tris.append(tuple(verts))
        self.flags.append(any(self.ifl[s] for s in sources))

    def create_triangle(self, verts):
        self.tris.append(tuple(verts))
        self.flags.append(True)


def op_fill(b):
    for v in range(len(b.iv)):
        b.copy_vertex(v)
    for t in range(len(b.it)):
        b.copy_triangle(t)
    b.create_triangle((0, 2, 1))
    b.create_triangle((0, 3, 2))


def mid(p, q):
    return tuple(0.5 * (p[k] + q[k]) for k in range(3))


def op_split(b, which):
    for v in range(len(b.iv)):
        b.copy_vertex(v)
    for t in range(len(b.it)):
        if t not in which:
            b.copy_triangle(t)
            continue
        a, bb, c = (b.vmap[v] for v in b.it[t])
        pa, pb, pc = (b.verts[i] for i in (a, bb, c))
        mab = b.add_vertex(mid(pa, pb))
        mbc = b.add_vertex(mid(pb, pc))
        mca = b.add_vertex(mid(pc, pa))
        for kid in ((a, mab, mca), (mab, bb, mbc), (mca, mbc, c), (mab, mbc, mca)):
            b.derive_triangle([t], kid)


def splitmix64(seed):
    state = seed & M64
    while True:
        state = (state + 0x9E3779B97F4A7C15) & M64
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
        yield z ^ (z >> 31)


def op_shuffle(b, seed):
    for v in range(len(b.iv)):
        b.copy_vertex(v)
    order = list(range(len(b.it)))
    rng = splitmix64(seed)
    for i in range(len(order) - 1, 0, -1):
        j = next(rng) % (i + 1)
        order[i], order[j] = order[j], order[i]
    for t in order:
        b.copy_triangle(t)
    return order


def op_identity(b):
    for v in range(len(b.iv)):
        b.copy_vertex(v)
    for t in range(len(b.it)):
        b.copy_triangle(t)


def record(det, inputs, op, output, params, q_after, q_before, seed, synth, tris):
    seed_s = "null" if seed is None else '"%d"' % seed
    return (
        '{"determinism":"%s","inputs":[%s],"kind":"scan.mesh_op_record@1","op":"%s","output":"%s",'
        '"params":%s,"quality_after":%s,"quality_before":[%s],"seed":%s,"synthesized":"%d","triangles":"%d"}'
        % (det, ",".join('"%s"' % i for i in inputs), op, output, params, q_after, ",".join(q_before), seed_s,
           synth, tris)
    )


def case_lines(name, frame, in_digest, b, extra, rec):
    out_d = derived_digest(frame, b.verts, b.tris, b.flags)
    lines = [
        "case %s" % name,
        "input_digest %s" % in_digest,
        "output_digest %s" % wire(out_d),
        "output_element_digest %s" % wire(element_digest(frame, b.verts, b.tris)),
        "vertices %d" % len(b.verts),
        "triangles %d" % len(b.tris),
        "synthesized %d" % sum(1 for f in b.flags if f),
        "flags %s" % "".join("1" if f else "0" for f in b.flags),
        "flag_bytes %s" % flag_bytes(b.flags).hex(),
    ]
    lines += extra
    if rec is not None:
        r = rec(wire(out_d))
        lines.append("record %s" % r)
        lines.append("record_digest %s" % wire(hashlib.sha256(r.encode("ascii")).digest()))
    lines.append("end")
    return lines, b


def render():
    frame, verts, tris = load_mesh("mesh_open_cube.txt")
    assert len(verts) == 8 and len(tris) == 10
    q_open = quality_canonical("mesh_open_cube")
    q_closed = quality_canonical("mesh_closed_cube")
    raw = wire(element_digest(frame, verts, tris))
    none = [False] * len(tris)
    lines = [
        "# SCAN.MESHOP.PROV expected results. CC-BY-4.0.",
        "# Written by make_prov_cases.py (independent of the Rust crate). Input mesh_open_cube.txt.",
        "open_cube_digest %s" % raw,
    ]

    b = Builder(verts, tris, none)
    op_fill(b)
    fill_lines, fill_b = case_lines(
        "fill", frame, raw, b, [],
        lambda out: record("D0", [raw], "mesh.test_fill@1", out, '{"face":"z0"}', q_closed, [q_open], None,
                           2, 12))
    lines += fill_lines

    flagged = [t == 0 for t in range(len(tris))]
    in_split = wire(derived_digest(frame, verts, tris, flagged))
    b = Builder(verts, tris, flagged)
    op_split(b, {0, 1})
    lines += case_lines("split", frame, in_split, b, ["input_flags %s" % "".join("1" if f else "0" for f in flagged)],
                        None)[0]

    for seed in (42, 7):
        b = Builder(verts, tris, none)
        order = op_shuffle(b, seed)
        lines += case_lines(
            "shuffle_%d" % seed, frame, raw, b, ["order %s" % " ".join(str(t) for t in order)],
            lambda out, s=seed: record("D2", [raw], "mesh.test_shuffle@1", out, "{}", q_open, [q_open], s, 0, 10))[0]

    b = Builder(verts, tris, none)
    op_identity(b)
    lines += case_lines(
        "identity", frame, raw, b, [],
        lambda out: record("D0", [raw], "mesh.test_identity@1", out, "{}", q_open, [q_open], None, 0, 10))[0]

    fill_out = wire(derived_digest(frame, fill_b.verts, fill_b.tris, fill_b.flags))
    b = Builder(fill_b.verts, fill_b.tris, fill_b.flags)
    op_identity(b)
    lines += case_lines(
        "chain", frame, fill_out, b, [],
        lambda out: record("D0", [fill_out], "mesh.test_identity@1", out, "{}", q_closed, [q_closed], None, 2, 12))[0]
    return "\n".join(lines) + "\n"


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        with open(OUT, encoding="utf-8", newline="") as f:
            if f.read() != text:
                print("prov_cases.txt is stale; rerun make_prov_cases.py", file=sys.stderr)
                return 1
        return 0
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
