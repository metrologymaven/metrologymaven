#!/usr/bin/env python3
"""Generate the CORE.RPL.BISECT fixtures (AC-1, AC-2, AC-3, AC-4).

Independent oracle: stdlib Python only, nothing here runs or imports the Rust code.
Expected values are hand-labelled from the leaf's rules (injected divergence
position, injected cause, the stated precedence) and cross-checked by a plain
linear scan. CC-BY-4.0 project data, generated from a fixed seed.

Run:  python fixtures/core/rpl/bisect/make_fixtures.py
"""

import hashlib
import json
import math
import random
from pathlib import Path

HERE = Path(__file__).resolve().parent
SEED = 20261118


def sha(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def write(name: str, doc: dict) -> None:
    text = json.dumps(doc, indent=1, sort_keys=True, ensure_ascii=True) + "\n"
    (HERE / name).write_text(text, encoding="utf-8", newline="\n")


# ---------------------------------------------------------------------------
# AC-1: span_1000.json. 1,000 synthetic journal entries. Each entry has a payload;
# the state after entry k is a hash chain over payloads 0..=k. The replayed run
# is identical except that the payload of entry 637 is altered, so every state
# from 637 onward differs (a monotone divergence predicate).
# ---------------------------------------------------------------------------
N = 1000
INJECT = 637
rng = random.Random(SEED)
entries = []
rec_state = sha(b"genesis")
rep_state = rec_state
for seq in range(N):
    payload = bytes(rng.getrandbits(8) for _ in range(16))
    rep_payload = payload if seq != INJECT else bytes([payload[0] ^ 0x01]) + payload[1:]
    rec_state = sha(rec_state.encode() + payload)
    rep_state = sha(rep_state.encode() + rep_payload)
    entries.append({
        "seq": seq,
        "recorded_state": rec_state,
        "replayed_state": rep_state,
        "outcome": "IDENTICAL" if rec_state == rep_state else "DIVERGENT",
    })

# Oracle cross-check by linear scan.
linear_first = next(e["seq"] for e in entries if e["recorded_state"] != e["replayed_state"])
assert linear_first == INJECT
assert all(e["recorded_state"] != e["replayed_state"] for e in entries[INJECT:])
write("span_1000.json", {
    "license": "CC-BY-4.0",
    "seed": SEED,
    "n": N,
    "expect": {"first_divergent": INJECT, "max_evaluations": math.ceil(math.log2(N)) + 1},
    "entries": entries,
})


# ---------------------------------------------------------------------------
# AC-4: non_monotone.json. Predicates given as their set of true positions
# (half-open intervals). Expected entry = lowest true position (linear scan):
# that value is spec-derived and independent of any search.
#
# The `non_monotone` flags are DESIGN-DERIVED, not independent: whether a
# violation is visible depends on which points a search evaluates, so they come
# from reference_search() below, a Python transcription of the search stated in
# the Rust doc comment (lower bound over [0, n-1) assuming the last entry
# diverges, then a spot check of entry n-1). Each fixture case records this in
# `non_monotone_source`. The flagged cases all share the hand-checkable property
# that the true island ends before n-1, so pred(n-1) is false.
# ---------------------------------------------------------------------------
def truth(intervals, k):
    return any(a <= k < b for a, b in intervals)


def reference_search(n, intervals):
    seen = {}
    lo, hi = 0, n - 1
    while lo < hi:
        mid = lo + (hi - lo) // 2
        seen[mid] = truth(intervals, mid)
        if seen[mid]:
            hi = mid
        else:
            lo = mid + 1
    seen[n - 1] = truth(intervals, n - 1)
    if not any(seen.values()):
        return None, False  # every evaluation false: no divergence in the span
    k = lo
    violated = any((j < k and v) or (j >= k and not v) for j, v in seen.items())
    return k, violated


nm_cases = []
for name, n, intervals in [
    ("island 637..800 in 1000", 1000, [(637, 800)]),
    ("island 300..900 in 1000", 1000, [(300, 900)]),
    ("single true at 7 in 16", 16, [(7, 8)]),
    ("island 1..3 in 4", 4, [(1, 3)]),
]:
    first = min(a for a, _ in intervals)
    k, violated = reference_search(n, intervals)
    assert violated, name  # every case here must be one the spot check detects
    assert not truth(intervals, n - 1), name
    nm_cases.append({"name": name, "n": n, "true_intervals": [list(iv) for iv in intervals],
                     "expect": {"first_divergent": first, "non_monotone": True},
                     "non_monotone_source": "design-derived"})

# Undetectable by design: agrees with a monotone predicate on every evaluated point.
intervals = [(2, 3), (500, 1000)]
k, violated = reference_search(1000, intervals)
assert k == 500 and not violated
nm_cases.append({"name": "undetected: true at 2 and 500..1000 in 1000", "n": 1000,
                 "true_intervals": [list(iv) for iv in intervals],
                 "expect": {"first_divergent": 500, "non_monotone": False, "max_evaluations": 11,
                            "lowest_true": 2},
                 "non_monotone_source": "design-derived"})

# Monotone controls: no flag, plain bisection answer, budget respected.
for name, n, d in [("monotone 637 in 1000", 1000, 637), ("monotone 0 in 7", 7, 0),
                   ("monotone last in 9", 9, 8), ("no divergence in 12", 12, None), ("single entry diverges", 1, 0)]:
    intervals = [] if d is None else [(d, n)]
    k, violated = reference_search(n, intervals)
    assert not violated and k == d, name
    nm_cases.append({"name": name, "n": n, "true_intervals": [list(iv) for iv in intervals],
                     "expect": {"first_divergent": d, "non_monotone": False,
                                "max_evaluations": math.ceil(math.log2(n)) + 1},
                     "non_monotone_source": "monotone by construction"})
write("non_monotone.json", {"license": "CC-BY-4.0", "cases": nm_cases})


# ---------------------------------------------------------------------------
# AC-2 / AC-3 / REQ-005: attribution.json. A base entry's facts; each case
# perturbs named fields of the replayed copy. Expected primary/secondary are
# written by hand from the leaf's precedence:
#   input > parameter > defaults_profile > command_migration > kernel.
# ---------------------------------------------------------------------------
arng = random.Random(SEED + 1)


def d():
    return sha(arng.getrandbits(64).to_bytes(8, "big"))


base = {
    "seq": 637,
    "inputs": [{"tag": "entity", "digest": d()}, {"tag": "param", "digest": d()}],
    "arguments": [d(), d(), d()],
    "defaults_profile": d(),
    "command": "geo.fit.circle@2",
    "migrator": None,
    "kernel_manifest": d(),
}


def perturb(fields):
    r = json.loads(json.dumps(base))
    for f in fields:
        if f == "input":
            r["inputs"][1]["digest"] = d()
        elif f == "parameter":
            r["arguments"][0] = d()
        elif f == "defaults_profile":
            r["defaults_profile"] = d()
        elif f == "command_major":
            r["command"] = "geo.fit.circle@3"
        elif f == "migrator":
            r["migrator"] = "geo.fit.circle@2->3"
        elif f == "kernel":
            r["kernel_manifest"] = d()
        elif f == "input_count":
            r["inputs"].append({"tag": "r1_result", "digest": d()})
        elif f == "input_tag":
            r["inputs"][0]["tag"] = "r1_result"
        else:
            raise ValueError(f)
    return r


att_cases = []


def acase(name, ac, fields, primary, secondary, notes=()):
    att_cases.append({"name": name, "ac": ac, "recorded": base, "replayed": perturb(fields),
                      "expect": {"entry_seq": 637, "primary": primary, "secondary": list(secondary),
                                 "notes": list(notes)}})


# AC-2: one case per cause.
acase("input digest differs", "AC-2", ["input"], "input", [])
acase("argument commitment differs", "AC-2", ["parameter"], "parameter", [])
acase("defaults profile differs", "AC-2", ["defaults_profile"], "defaults_profile", [])
acase("command major differs", "AC-2", ["command_major"], "command_migration", [])
acase("kernel manifest differs", "AC-2", ["kernel"], "kernel", [])
# AC-3
acase("input and kernel differ", "AC-3", ["input", "kernel"], "input", ["kernel"])
# REQ-004 / REQ-005 extras
acase("migrator differs only", "REQ-004", ["migrator"], "command_migration", [])
acase("input tag changed only", "REQ-004", ["input_tag"], "input", [])
acase("extra input entry", "REQ-004", ["input_count"], "input", [])
acase("everything but input differs", "REQ-004",
      ["kernel", "migrator", "defaults_profile", "parameter"], "parameter",
      ["defaults_profile", "command_migration", "kernel"])
acase("everything differs", "REQ-004",
      ["kernel", "command_major", "defaults_profile", "parameter", "input"], "input",
      ["parameter", "defaults_profile", "command_migration", "kernel"])
acase("nothing differs", "REQ-005", [], "kernel", [], ["no_input_difference"])
write("attribution.json", {"license": "CC-BY-4.0", "seed": SEED + 1, "cases": att_cases})
