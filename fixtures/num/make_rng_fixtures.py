#!/usr/bin/env python3
"""Independent Philox4x32-10 reference for KRN.NUM.RNG (CC-BY-4.0 project data).

Writes fixtures/num/rng-philox-kat-v1.txt (block known-answer vectors) and
fixtures/num/rng-stream-v1.txt (golden SHA-256 of 10^6 uniform_f64 draws).
The implementation follows Salmon, Moraes, Dror, Shaw, "Parallel random numbers:
as easy as 1, 2, 3" (SC 2011) and is checked against the three published
Random123 kat_vectors for philox4x32_10 before anything is written.
Run:  python fixtures/num/make_rng_fixtures.py
"""

import hashlib
import struct
from pathlib import Path

M0, M1 = 0xD2511F53, 0xCD9E8D57
W0, W1 = 0x9E3779B9, 0xBB67AE85
MASK = 0xFFFFFFFF


def philox(ctr, key):
    c0, c1, c2, c3 = ctr
    k0, k1 = key
    for r in range(10):
        if r:
            k0, k1 = (k0 + W0) & MASK, (k1 + W1) & MASK
        p0, p1 = M0 * c0, M1 * c2
        hi0, lo0, hi1, lo1 = p0 >> 32, p0 & MASK, p1 >> 32, p1 & MASK
        c0, c1, c2, c3 = hi1 ^ c1 ^ k0, lo1, hi0 ^ c3 ^ k1, lo0
    return (c0, c1, c2, c3)


# Published Random123 known answers for philox4x32_10 (kat_vectors).
PUBLISHED = [
    ((0, 0, 0, 0), (0, 0), (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)),
    ((MASK,) * 4, (MASK, MASK), (0x408F276D, 0x41C83B0E, 0xA20BC7C6, 0x6D5451FD)),
    ((0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344), (0xA4093822, 0x299F31D0),
     (0xD16CFE09, 0x94FDCCEB, 0x5001E420, 0x24126EA1)),
]
for ctr, key, want in PUBLISHED:
    got = philox(ctr, key)
    assert got == want, (ctr, key, [hex(x) for x in got])


def block(seed, stream, index):
    """Project addressing: key = seed (lo, hi); counter = (index lo, index hi, stream lo, stream hi)."""
    return philox((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32))


def next_u64(seed, stream, index):
    w = block(seed, stream, index)
    return (w[1] << 32) | w[0]


def uniform_f64(u):
    return (u >> 11) * 2.0 ** -53


HERE = Path(__file__).parent
lines = ["# KRN.NUM.RNG AC-1: Philox4x32-10 block known answers.",
         "# First 3 rows: published Random123 kat_vectors. Others: this reference, project addressing.",
         "# Format: c0 c1 c2 c3 k0 k1 -> o0 o1 o2 o3 (hex u32)"]
rows = [(c, k) for c, k, _ in PUBLISHED]
for seed, stream, index in [(0, 0, 0), (1, 0, 0), (0, 1, 0), (0, 0, 1), (0x0123456789ABCDEF, 7, 0),
                            (0x0123456789ABCDEF, 7, 999_999), (2**64 - 1, 2**64 - 1, 2**64 - 1),
                            (42, 3, 2**32), (42, 2**32, 5), (0xDEADBEEFCAFEF00D, 12345, 67890)]:
    rows.append(((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32)))
for c, k in rows:
    o = philox(c, k)
    lines.append(" ".join(f"{x:08x}" for x in (*c, *k)) + " -> " + " ".join(f"{x:08x}" for x in o))
(HERE / "rng-philox-kat-v1.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

seed, stream, n = 0x0123456789ABCDEF, 7, 1_000_000
h = hashlib.sha256()
for i in range(n):
    h.update(struct.pack("<d", uniform_f64(next_u64(seed, stream, i))))
first = [uniform_f64(next_u64(seed, stream, i)) for i in range(3)]
(HERE / "rng-stream-v1.txt").write_text(
    "# KRN.NUM.RNG AC-2: SHA-256 of the little-endian f64 bit patterns of uniform_f64 for\n"
    "# seed 0x0123456789ABCDEF, stream 7, indices 0..999999 (one u64 per index: word1<<32 | word0).\n"
    f"digest sha256:{h.hexdigest()}\n"
    + "".join(f"first{i} {struct.unpack('<Q', struct.pack('<d', x))[0]:016x}\n" for i, x in enumerate(first)),
    encoding="utf-8", newline="\n")
print("published KATs match; wrote fixtures")
