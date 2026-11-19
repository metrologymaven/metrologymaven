"""Independent reference for the CAD worker protocol encodings (IO.CAD.WORKER.PROTO, AC-1).

Stdlib only; shares no code with the Rust crate. It re-implements the seeded generator of
samples.rs (SplitMix64, seed 7, 1,000 messages, fields drawn in wire order) and encodes each
message from the wire format written in the leaf and in crates/fid-cad-proto/src/lib.rs:
frame = u32 LE payload length, u8 kind, payload; integers LE; bool one byte; f64 as LE
IEEE 754 bits; Option = tag byte 0/1 then value; bytes/text = u32 length then bytes (UTF-8);
Vec = u32 count then items. It writes the SHA-256 of every frame to digests.txt, plus the
SHA-256 of all frames concatenated.

Run from the repository root:
    python fixtures/io/cad/worker/proto/make_digests.py          # write
    python fixtures/io/cad/worker/proto/make_digests.py --check  # compare only
"""

import hashlib
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SEED = 7
COUNT = 1000
MASK = (1 << 64) - 1
KIND_NAMES = ["hello", "load_step", "load_iges", "shutdown", "hello_ack", "progress", "records", "report", "failed"]
KIND_CODES = [0x01, 0x02, 0x03, 0x04, 0x81, 0x82, 0x83, 0x84, 0x85]
ALPHABET = ["a", "Z", "0", " ", "\\", '"', "é", "中", "\U0001f600", "‮", "\n", "\u0000"]


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, n):
        return self.next() % n


class Out:
    def __init__(self):
        self.b = bytearray()

    def u8(self, v):
        self.b += struct.pack("<B", v)

    def u16(self, v):
        self.b += struct.pack("<H", v)

    def u32(self, v):
        self.b += struct.pack("<I", v)

    def u64(self, v):
        self.b += struct.pack("<Q", v)

    def f64(self, v):
        self.b += struct.pack("<d", v)

    def raw(self, data):
        self.u32(len(data))
        self.b += data


def gen_f(r, o):
    x = (r.next() >> 11) - (1 << 52)
    o.f64(x / 1024)  # exact: |x| < 2^53 and 1024 is a power of two


def gen_idx(r, o):
    o.u32(r.below(1 << 32))


def gen_text(r, o):
    n = r.below(9)
    o.raw("".join(ALPHABET[r.below(len(ALPHABET))] for _ in range(n)).encode("utf-8"))


def gen_bytes(r, o):
    n = r.below(64)
    o.raw(bytes(r.below(256) for _ in range(n)))


def gen_version(r, o):
    o.u16(r.below(4))
    o.u16(r.below(65536))


def gen_vec(r, o, maximum, item):
    n = r.below(maximum + 1)
    o.u32(n)
    for _ in range(n):
        item(r, o)


def gen_bool(r, o):
    o.u8(r.below(2))


def gen_record(r, o):
    tag = r.below(15)
    o.u8(tag)
    if r.below(2) == 0:
        o.u8(0)
        o.u64(r.next())
    else:
        o.u8(1)
        gen_idx(r, o)
    if r.below(2) == 0:
        o.u8(0)
    else:
        o.u8(1)
        gen_text(r, o)
    if tag == 0:  # Surface: kind (7 values), params
        o.u8(r.below(7))
        gen_vec(r, o, 6, gen_f)
    elif tag == 1:  # Curve: kind (5 values), params
        o.u8(r.below(5))
        gen_vec(r, o, 6, gen_f)
    elif tag == 2:  # Pcurve: surface, params
        gen_idx(r, o)
        gen_vec(r, o, 6, gen_f)
    elif tag == 3:  # Vertex
        for _ in range(3):
            gen_f(r, o)
    elif tag == 4:  # Edge: curve, start, end
        for _ in range(3):
            gen_idx(r, o)
    elif tag == 5:  # Coedge: edge, reversed, pcurve option
        gen_idx(r, o)
        gen_bool(r, o)
        if r.below(2) == 0:
            o.u8(0)
        else:
            o.u8(1)
            gen_idx(r, o)
    elif tag == 6:  # Loop
        gen_vec(r, o, 5, gen_idx)
    elif tag == 7:  # Face: surface, same_sense, loops
        gen_idx(r, o)
        gen_bool(r, o)
        gen_vec(r, o, 3, gen_idx)
    elif tag == 8:  # Shell: closed, faces
        gen_bool(r, o)
        gen_vec(r, o, 5, gen_idx)
    elif tag in (9, 10):  # Body shells / Product bodies
        gen_vec(r, o, 3, gen_idx)
    elif tag == 11:  # Instance: parent, child, 12 floats
        gen_idx(r, o)
        gen_idx(r, o)
        for _ in range(12):
            gen_f(r, o)
    elif tag == 12:  # PmiItem: kind (4 values), value option, targets
        o.u8(r.below(4))
        if r.below(2) == 0:
            o.u8(0)
        else:
            o.u8(1)
            gen_f(r, o)
        gen_vec(r, o, 4, gen_idx)
    elif tag == 13:  # IgesEntity: entity_type, form, params
        o.u16(r.below(65536))
        o.u16(r.below(65536))
        gen_vec(r, o, 6, gen_f)
    else:  # Skipped: reason (3 values)
        o.u8(r.below(3))


def payload(r, k):
    o = Out()
    if k == 0:
        gen_version(r, o)
    elif k == 1:
        gen_bool(r, o)
        gen_bytes(r, o)
    elif k == 2:
        gen_bytes(r, o)
    elif k == 3:
        pass
    elif k == 4:
        gen_version(r, o)
        gen_text(r, o)
        gen_text(r, o)
    elif k == 5:
        total = r.next() >> 1
        done = r.next() % (total + 1)
        o.u64(done)
        o.u64(total)
    elif k == 6:
        gen_idx(r, o)
        gen_vec(r, o, 7, gen_record)
    elif k == 7:
        o.u64(r.next())
        o.u64(r.next())
        gen_vec(r, o, 3, gen_text)
    else:
        o.u8(r.below(4))
        gen_text(r, o)
    return bytes(o.b)


def render():
    r = Rng(SEED)
    lines = [
        "# IO.CAD.WORKER.PROTO AC-1: SHA-256 of each encoded frame (header and payload),",
        "# seed 7, 1000 messages; last line: SHA-256 of all frames concatenated.",
        "# Written by make_digests.py (independent of the Rust crate); do not edit.",
    ]
    whole = hashlib.sha256()
    for i in range(COUNT):
        k = i % 9
        p = payload(r, k)
        frame = struct.pack("<IB", len(p), KIND_CODES[k]) + p
        whole.update(frame)
        lines.append("%d %s %s" % (i, KIND_NAMES[k], hashlib.sha256(frame).hexdigest()))
    lines.append("all %s" % whole.hexdigest())
    return ("\n".join(lines) + "\n").encode("ascii")


def main():
    out = HERE / "digests.txt"
    data = render()
    if "--check" in sys.argv[1:]:
        if not out.exists() or out.read_bytes() != data:
            print("stale: %s (run make_digests.py)" % out, file=sys.stderr)
            return 1
        print("digests.txt up to date")
        return 0
    out.write_bytes(data)
    print("wrote %s" % out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
