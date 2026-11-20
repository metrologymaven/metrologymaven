"""Independent reference for the export fixtures (IO.REG.ENV.EXPORT, AC-1, AC-2, AC-6).

Stdlib only; shares no code with the Rust crate. It writes:

- `points.json`: the input point set of the test format `fid-test-points` (integer
  nanometre coordinates; some points carry a colour the format cannot hold);
- `expected.json`: for the AC-1 export (stride 1) and a stride-3 export, the file's byte
  count and SHA-256, the canonical export report (real and dry run) and the canonical
  `export.file@1` journal record of a `script` principal, with the report digest (SHA-256
  of the canonical report without its `dry_run` key, so a dry run's would-be record is the
  same).

The test format is defined here and in the Rust test writer from the same written rule
(the leaf's "test-only format"):

    b"FTP 1\\n" + b"<n>\\n" + for every stride-th point (index 0, s, 2s, ...): b"<x> <y> <z>\\n"

with n the number of points written and integers in decimal without leading zeros.
A point set holding any coloured point reports the lossy name `point_set.color`.

Canonical JSON follows RFC 8785 with the fid-canon project rule (64-bit integers as
decimal strings, small integers such as `stride` as JSON numbers).

Run from the repository root:
    python fixtures/io/reg/env/export/make_fixtures.py          # write
    python fixtures/io/reg/env/export/make_fixtures.py --check  # compare only
"""

import hashlib
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
N_POINTS = 1500
SOURCE = ["0190f3a2-7c41-7d2e-9b10-3c5e8f6a4d21"]
FORMAT = "fid-test-points"
WRITER_VERSION = "1.0.0"
TARGET = "root://out/a.ftp"


class U64(int):
    """A 64-bit integer, written as a decimal string."""


def jcs(v) -> str:
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, U64):
        return '"' + str(int(v)) + '"'
    if isinstance(v, int):
        return str(v)
    if isinstance(v, str):
        out = ['"']
        short = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\t": "\\t", "\n": "\\n", "\f": "\\f", "\r": "\\r"}
        for ch in v:
            if ch in short:
                out.append(short[ch])
            elif ord(ch) < 0x20:
                out.append("\\u%04x" % ord(ch))
            else:
                out.append(ch)
        out.append('"')
        return "".join(out)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


def points():
    pts = []
    for i in range(N_POINTS):
        x = (i * 7919) % 100003 - 50000
        y = (i * 104729) % 99991 - 40000
        z = i * 13 - 9000
        pts.append([x, y, z])
    colored = [i for i in range(N_POINTS) if i % 97 == 5]
    return {"colored": colored, "points": pts}


def ftp_bytes(data, stride):
    chosen = data["points"][::stride]
    lines = ["FTP 1", str(len(chosen))] + ["%d %d %d" % tuple(p) for p in chosen]
    return ("\n".join(lines) + "\n").encode("ascii")


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def case(data, stride):
    body = ftp_bytes(data, stride)
    lossy = ["point_set.color"] if any(i % stride == 0 for i in data["colored"]) else []

    def report(dry):
        return {
            "bytes": U64(len(body)),
            "dry_run": dry,
            "format": FORMAT,
            "lossy": lossy,
            "sha256": sha(body),
            "target": TARGET,
            "warnings": [],
            "warnings_total": U64(0),
        }

    real = jcs(report(False))
    event = {
        "bytes": U64(len(body)),
        "command": "export.file@1",
        "force_extension": False,
        "format": FORMAT,
        "lossy": lossy,
        "options": {"stride": stride},
        "overwrite": False,
        "principal": "script",
        "report_sha256": sha(jcs({k: v for k, v in report(False).items() if k != "dry_run"}).encode("utf-8")),
        "sha256": sha(body),
        "source": SOURCE,
        "target": TARGET,
        "writer_version": WRITER_VERSION,
    }
    return {
        "bytes": str(len(body)),
        "dry_report": jcs(report(True)),
        "event": jcs(event),
        "report": real,
        "sha256": sha(body),
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    data = points()
    expected = {"source": SOURCE, "stride1": case(data, 1), "stride3": case(data, 3)}
    outputs = [("points.json", jcs(data)), ("expected.json", jcs(expected))]
    bad = 0
    for name, text in outputs:
        raw = text.encode("utf-8")
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != raw:
                print(f"stale: {path}")
                bad = 1
        else:
            path.write_bytes(raw)
    return bad


if __name__ == "__main__":
    sys.exit(main())
