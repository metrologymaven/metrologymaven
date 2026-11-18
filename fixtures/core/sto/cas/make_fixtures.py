#!/usr/bin/env python3
"""Generate the CORE.STO.CAS fixtures (AC-1, AC-4) with an independent SHA-256 tool.

Expected addresses come from Python's hashlib, never from the Rust code under test.

Content of blob `i` is the SplitMix64 byte stream for seed `seed`: each 64-bit output is
written little-endian and the stream is cut to `size` bytes. SplitMix64 (Steele, Lea and
Flood, 2014): state += 0x9E3779B97F4A7C15; z = state; z = (z ^ z>>30) * 0xBF58476D1CE4E5B9;
z = (z ^ z>>27) * 0x94D049BB133111EB; output z ^ z>>31 (all mod 2^64).

Objects larger than 8 MiB are cut into 8 MiB chunks; the object's name is the SHA-256 of
its canonical `fid-chunks@1` manifest (RFC 8785; the project rule writes 64-bit integers as
decimal strings), written here by hand:
  {"chunk_size":"8388608","chunks":["sha256:..",...],"format":"fid-chunks@1","size":"<n>"}

Outputs (CC-BY-4.0 project data):
  blobs.json          100 blobs from 0 B to 20 MiB: seed, size, address, chunk addresses
  manifest_20mib.json canonical manifest of the 20 MiB blob (no trailing newline)

Run:   python fixtures/core/sto/cas/make_fixtures.py
Check: python fixtures/core/sto/cas/make_fixtures.py --check
"""

import hashlib
import json
import math
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
MASK = (1 << 64) - 1
MIB = 1024 * 1024
CHUNK = 8 * MIB
SEED_BASE = 0x5EED_CA50_0000_0000


def splitmix_bytes(seed, size):
    """The first `size` bytes of the SplitMix64 stream for `seed`."""
    words = (size + 7) // 8
    out = []
    state = seed & MASK
    for _ in range(words):
        state = (state + 0x9E3779B97F4A7C15) & MASK
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        out.append(z ^ (z >> 31))
    return struct.pack("<%dQ" % words, *out)[:size]


def address(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def manifest_bytes(size, chunks):
    listed = ",".join('"%s"' % c for c in chunks)
    text = '{"chunk_size":"%d","chunks":[%s],"format":"fid-chunks@1","size":"%d"}' % (CHUNK, listed, size)
    return text.encode("ascii")


def sizes():
    """18 boundary sizes, then 82 log-uniform sizes in 1 B .. 2 MiB from a fixed LCG."""
    edges = [0, 1, 55, 56, 63, 64, 65, 1000, 65535, 65536, 65537, MIB,
             CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK, 2 * CHUNK + 1, 20 * MIB]
    out = list(edges)
    x = 12345
    while len(out) < 100:
        x = (x * 6364136223846793005 + 1442695040888963407) & MASK
        u = (x >> 11) / float(1 << 53)
        out.append(int(math.exp(u * math.log(2 * MIB))))
    return out


def build():
    blobs = []
    manifest_20 = None
    for i, size in enumerate(sizes()):
        seed = SEED_BASE + i
        data = splitmix_bytes(seed, size)
        entry = {"index": i, "seed": "%d" % seed, "size": size}
        if size <= CHUNK:
            entry["kind"] = "blob"
            entry["address"] = address(data)
            entry["chunks"] = []
        else:
            chunks = [address(data[o:o + CHUNK]) for o in range(0, size, CHUNK)]
            m = manifest_bytes(size, chunks)
            entry["kind"] = "object"
            entry["address"] = address(m)
            entry["chunks"] = chunks
            if size == 20 * MIB:
                manifest_20 = m
        blobs.append(entry)
    doc = {
        "about": "CORE.STO.CAS AC-1 and AC-4. Content: SplitMix64 stream of `seed` (decimal u64), "
                 "little-endian words, cut to `size`. Addresses from Python hashlib. CC-BY-4.0.",
        "chunk_size": CHUNK,
        "blobs": blobs,
    }
    text = json.dumps(doc, indent=1, sort_keys=True) + "\n"
    return {"blobs.json": text.encode("ascii"), "manifest_20mib.json": manifest_20}


def main():
    files = build()
    # Self-checks of the generator against known values.
    assert hashlib.sha256(b"").hexdigest() == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    # SplitMix64 seed 0, first output (reference value of the published algorithm).
    assert struct.unpack("<Q", splitmix_bytes(0, 8))[0] == 0xE220A8397B1DCDAF
    check = "--check" in sys.argv[1:]
    stale = []
    for name, data in files.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale fixtures: " + ", ".join(stale))
        return 1
    print("ok" if check else "wrote " + ", ".join(sorted(files)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
