#!/usr/bin/env python3
"""Expected SHA-256 digests and sizes for the IO.REG.WRITE tests (crates/fid-io-reg/tests/write.rs).

Independent of the Rust code: payloads are rebuilt here from their written rules and hashed
with Python's hashlib. Run from the repository root:

    python fixtures/io/reg/write/make_expected.py

and commit the regenerated expected.txt. Line format: `<name> <bytes> <sha256 hex>`.

Payload rules (the Rust tests build the same bytes from the same rules):
- mib:   1 MiB, byte i = (31*i + 7*(i >> 8)) mod 256
- half:  the first 512 KiB of mib
- old:   the ASCII text "solid old\\nendsolid old\\n"
- abc:   the ASCII text "abc" (FIPS 180-2 example; digest hand-checked against the standard)
- empty: no bytes (digest hand-checked against the standard's empty-string value)
"""

import hashlib
import os

HERE = os.path.dirname(os.path.abspath(__file__))


def mib():
    return bytes((31 * i + 7 * (i >> 8)) & 0xFF for i in range(1 << 20))


def main():
    m = mib()
    cases = [
        ("mib", m),
        ("half", m[: 512 * 1024]),
        ("old", b"solid old\nendsolid old\n"),
        ("abc", b"abc"),
        ("empty", b""),
    ]
    lines = ["%s %d %s" % (n, len(d), hashlib.sha256(d).hexdigest()) for n, d in cases]
    # Hand-labelled known answers (FIPS 180-2 / NIST CSRC examples).
    assert lines[3].endswith("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    assert lines[4].endswith("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    with open(os.path.join(HERE, "expected.txt"), "w", encoding="ascii", newline="\n") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
