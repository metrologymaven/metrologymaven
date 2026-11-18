#!/usr/bin/env python3
"""Generate fixtures/core/job/prog/expected.json (CORE.JOB.PROG AC-1, AC-2).

The test handler `slow_sum` sums the integers 0 .. n-1 (n = 10^7), reports
progress every 10^5 integers and checks for cancellation every 10^4. Its Rust
implementation is test code in crates/fid-jobs/tests/progress.rs; this script is
the independent reference for its output and output digest.

The output is the object {"n": n, "sum": sum} with both values as 64-bit
integers, which the project's canonical JSON (CORE.JRN.CANON) writes as decimal
strings. The digest is "sha256:" + hex SHA-256 of those canonical bytes.
CC-BY-4.0 project data.
Run:  python fixtures/core/job/prog/make_expected.py
"""

import hashlib
import json
from pathlib import Path

N = 10**7
total = 0
for i in range(N):
    total += i
assert total == N * (N - 1) // 2

# Canonical JSON: keys sorted, no whitespace, 64-bit integers as decimal strings.
canonical = json.dumps({"n": str(N), "sum": str(total)}, sort_keys=True, separators=(",", ":"))
digest = "sha256:" + hashlib.sha256(canonical.encode("utf-8")).hexdigest()

doc = {
    "handler": "slow_sum",
    "n": N,
    "report_every": 10**5,
    "checkpoint_every": 10**4,
    "sum": str(total),
    "canonical_output": canonical,
    "digest": digest,
}
out = Path(__file__).with_name("expected.json")
out.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(out, digest)
