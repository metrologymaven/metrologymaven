#!/usr/bin/env python3
"""Converts NIST StRD nonlinear-regression `.dat` files for a LOCAL run of KRN.SOLV.NLLS AC-1.

The NIST files are third-party reference data. Their redistribution terms are open in decision
D-038, so neither the files nor their contents are committed here. Download them yourself from
the NIST StRD site, then:

    python fixtures/solv/nist-strd-nls-v1/convert.py <dir with *.dat> <output dir>
    FID_NIST_STRD_NLS=<output dir> cargo test -p fid-solv --test nlls -- --ignored

Each output `<Name>.txt` uses the block format of fixtures/solv/strd-models-synth-v1.txt:
`problem <model> name <Name> n <n> m <m>`, `x`/`y` as binary64 hex, `ref` (certified values),
`ref_rss`, and `start1`/`start2` as the published decimal strings. The `.dat` layout is read
from the file's own header ("Starting Values (lines a to b)", "Data (lines c to d)"); the data
columns are y then x. [unverified against every file: written from the published layout]

Stdlib only; reads nothing but the given directory and writes only the output directory.
"""

import struct
import sys
from pathlib import Path

# NIST dataset name -> model form implemented in crates/fid-solv/tests/nlls.rs.
MODEL = {
    "Misra1a": "misra1a",
    "Chwirut1": "chwirut",
    "Chwirut2": "chwirut",
    "DanWood": "danwood",
    "Lanczos1": "lanczos3",
    "Lanczos2": "lanczos3",
    "Lanczos3": "lanczos3",
    "Kirby2": "kirby2",
    "Misra1d": "misra1d",
    "Roszman1": "roszman1",
    "MGH17": "mgh17",
}


def hx(x):
    return f"{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def line_range(lines, label):
    for ln in lines:
        if label in ln and "(lines" in ln:
            t = ln.split("(lines", 1)[1].replace(")", " ").split()
            return int(t[0]), int(t[2])
    raise ValueError(f"no '{label}' line range")


def convert(path):
    lines = path.read_text(encoding="latin-1").splitlines()
    name = path.stem
    s0, s1 = line_range(lines, "Starting Values")
    d0, d1 = line_range(lines, "Data")
    starts1, starts2, cert = [], [], []
    for ln in lines[s0 - 1:s1]:
        t = ln.split("=", 1)[1].split()
        starts1.append(t[0])
        starts2.append(t[1])
        cert.append(t[2])
    rss = next(ln.split(":", 1)[1].split()[0] for ln in lines if ln.strip().startswith("Residual Sum of Squares"))
    ys, xs = [], []
    for ln in lines[d0 - 1:d1]:
        t = ln.split()
        ys.append(float(t[0]))
        xs.append(float(t[1]))
    model = MODEL.get(name, name.lower())
    return "\n".join([
        f"# NIST StRD {name}, converted locally by convert.py; not for commit (D-038).",
        f"problem {model} name {name} n {len(cert)} m {len(xs)}",
        "x " + " ".join(hx(v) for v in xs),
        "y " + " ".join(hx(v) for v in ys),
        "ref " + " ".join(cert),
        "ref_rss " + rss,
        "start1 " + " ".join(starts1),
        "start2 " + " ".join(starts2),
    ]) + "\n"


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    for path in sorted(src.glob("*.dat")):
        (out / f"{path.stem}.txt").write_text(convert(path), encoding="utf-8")
        print(f"{path.name} -> {path.stem}.txt ({MODEL.get(path.stem, 'model not implemented')})")


if __name__ == "__main__":
    main()
