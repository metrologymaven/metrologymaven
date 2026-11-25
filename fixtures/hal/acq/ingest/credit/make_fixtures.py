#!/usr/bin/env python3
"""Generate the fixtures of spec leaf HAL.ACQ.INGEST.CREDIT (credit ledger for data-plane flow control).

Independent stdlib-only reference. Nothing here runs the Rust code.

The reference ledger
--------------------
A plain re-statement of the leaf's rules (REQ-HAL.ACQ.INGEST.CREDIT-001 .. -005) with unbounded
Python integers, so an underflow shows up as a negative number and stops the script instead of
wrapping. Credit is counted in two units at once, bytes and batches:

    window      the configured cap (bytes, batches)
    out         credit the source holds and may spend
    fly         credit spent on admitted batches that downstream has not acknowledged yet,
                kept as a list of (first, last, bytes) in admission order; fly is its sum

    grant         Active only: out := window - fly; the grant is the increase. Stopping or
                  Stopped: nothing is granted (REQ-004).
    admit(b, r)   accepted iff out.bytes >= b and out.batches >= 1; then out -= (b, 1) and
                  (r.first, r.last, b) is appended to the list (REQ-001). Otherwise refused with `credit_exceeded` and the gap
                  {r.first, r.last, CREDIT_EXHAUSTED}; nothing changes (REQ-002). Admission
                  still spends credit already held while Stopping (draining, no drop).
    ack(b, r)     keyed: (r.first, r.last, b) must equal an entry of the list (the first such
                  entry is removed). Otherwise refused with `credit_unknown_ack` and nothing
                  changes: a duplicate, a batch never admitted, or a different byte size.
                  Active only: out := min(out + (b, 1), window - fly), per unit; the return is
                  the increase. Stopping or Stopped: the batch is released, nothing returned.
                  So out never exceeds the window, and neither does out + fly nor what is held
                  downstream (REQ-003).
    begin_stop    Active -> Stopping (idempotent; Stopped stays Stopped).
    finish_stop   -> Stopped and out := 0.

Trace line per event (shared with the Rust test, byte for byte):

    "<op> <result> <state> out=<bytes>/<batches> fly=<bytes>/<batches>"

with <result> one of `granted=<b>/<n>`, `accepted`,
`refused credit_exceeded gap=<from>-<to>:CREDIT_EXHAUSTED`, `returned=<b>/<n>`,
`refused credit_unknown_ack range=<first>-<last> bytes=<b>` or `ok`, and
<state> one of `active`, `stopping`, `stopped` (after the event).

Fixtures (AC-1 .. AC-4): hand-written event scripts below, each with the reference trace as
`expected`. AC-5: the seeded generator below (seed 11, KRN.NUM.RNG addressing, re-implemented
here and checked against the published Random123 answers) builds 10 000 scripts; the SHA-256 of
all their trace lines is written to `property-v1.txt` and the Rust test must reproduce it, run
twice.

Generator, per script s in 0 .. 9 999 (stream s of seed 11, draw indices from 0, `below` as in
KRN.NUM.RNG: rejection sampling, reject x < (2^64 - n) mod n, else x mod n):

    wb = 1 + below(2^20); wn = 1 + below(8); n = 1 + below(48); next = 0
    n times: k = below(100)
      k < 40: admit, bytes = 1 + below(wb), len = 1 + below(16),
              range = [next, next + len - 1], next += len
      k < 70: ack. Let A be the list of every admit event generated so far in this script
              (accepted or not). j = below(len(A) + 1).
              j < len(A): the j-th admit's range; bytes = its bytes + (1 if below(4) == 0
              else 0), so about a quarter are oversized.
              j = len(A): a never-admitted batch: range [next, next], bytes = 1 + below(wb)
              (next is not advanced).
      k < 90: grant
      k < 96: begin_stop
      else:   finish_stop

Digest input: for each script and event, "<s> <i> <trace line>\n", in order.

CC-BY-4.0 project data. Run:  python fixtures/hal/acq/ingest/credit/make_fixtures.py [--check]
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
MB = 1 << 20


class CheckFailed(Exception):
    pass


def check(cond, what):
    """Explicit check that also runs under `python -O` (unlike assert)."""
    if not cond:
        raise CheckFailed(repr(what))


# ---------------------------------------------------------------- reference ledger


class Ledger:
    def __init__(self, wb, wn):
        check(wb > 0 and wn > 0, "window must be positive in both units")
        self.wb, self.wn = wb, wn
        self.ob = self.on = 0
        self.held = []
        self.state = "active"

    @property
    def fb(self):
        return sum(h[2] for h in self.held)

    @property
    def fn(self):
        return len(self.held)

    def _invariants(self):
        for v in (self.ob, self.on, self.fb, self.fn):
            check(v >= 0, "credit went negative")
        check(self.ob + self.fb <= self.wb, "bytes above window")
        check(self.on + self.fn <= self.wn, "batches above window")

    def line(self, op, result):
        self._invariants()
        return f"{op} {result} {self.state} out={self.ob}/{self.on} fly={self.fb}/{self.fn}"

    def grant(self):
        gb = gn = 0
        if self.state == "active":
            tb, tn = self.wb - self.fb, self.wn - self.fn
            gb, gn = max(tb - self.ob, 0), max(tn - self.on, 0)
            self.ob, self.on = max(self.ob, tb), max(self.on, tn)
        return self.line("grant", f"granted={gb}/{gn}")

    def admit(self, b, first, last):
        check(first <= last, "inverted range in a fixture")
        if self.ob >= b and self.on >= 1:
            self.ob -= b
            self.on -= 1
            self.held.append((first, last, b))
            return self.line("admit", "accepted")
        return self.line("admit", f"refused credit_exceeded gap={first}-{last}:CREDIT_EXHAUSTED")

    def ack(self, b, first, last):
        key = (first, last, b)
        if key not in self.held:
            return self.line("ack", f"refused credit_unknown_ack range={first}-{last} bytes={b}")
        self.held.remove(key)
        rb = rn = 0
        if self.state == "active":
            nb = min(self.ob + b, self.wb - self.fb)
            nn = min(self.on + 1, self.wn - self.fn)
            rb, rn = max(nb - self.ob, 0), max(nn - self.on, 0)
            self.ob, self.on = max(nb, self.ob), max(nn, self.on)
        return self.line("ack", f"returned={rb}/{rn}")

    def begin_stop(self):
        if self.state == "active":
            self.state = "stopping"
        return self.line("begin_stop", "ok")

    def finish_stop(self):
        self.state = "stopped"
        self.ob = self.on = 0
        return self.line("finish_stop", "ok")


def run(window, events):
    led = Ledger(window["bytes"], window["batches"])
    out = []
    for ev in events:
        op, args = ev[0], ev[1:]
        if op == "grant":
            out.append(led.grant())
        elif op == "admit":
            out.append(led.admit(*args))
        elif op == "ack":
            out.append(led.ack(*args))
        elif op == "begin_stop":
            out.append(led.begin_stop())
        elif op == "finish_stop":
            out.append(led.finish_stop())
        else:
            raise CheckFailed(f"unknown op {op}")
    return out


# ---------------------------------------------------------------- hand-written scripts (AC-1 .. AC-4)


def steady():
    # AC-1: 8 MB / 4 batches; the source keeps up to four 1.5 MB batches in flight and each
    # acknowledgement returns credit for the next one. 24 batches, all accepted.
    ev = [["grant"]]
    seq = 0
    size = 3 * MB // 2
    for i in range(24):
        ev.append(["admit", size, seq, seq + 9])
        seq += 10
        if i >= 3:
            a = seq - 40
            ev.append(["ack", size, a, a + 9])
    for k in range(3, 0, -1):
        a = seq - 10 * k
        ev.append(["ack", size, a, a + 9])
    return {"comment": "HAL.ACQ.INGEST.CREDIT AC-1: steady flow within an 8 MB / 4-batch window",
            "window": {"bytes": 8 * MB, "batches": 4}, "events": ev}


def overrun():
    # AC-2: four 1 MB batches use up all four batch credits (bytes remain); the fifth is refused.
    ev = [["grant"]]
    for i in range(5):
        ev.append(["admit", MB, 100 * i, 100 * i + 99])
    return {"comment": "HAL.ACQ.INGEST.CREDIT AC-2: a fifth batch with no batch credit left",
            "window": {"bytes": 8 * MB, "batches": 4}, "events": ev}


def bytes_overrun():
    # REQ-001 byte unit: batch credit remains, byte credit does not; also a single batch larger
    # than the whole window.
    ev = [["grant"], ["admit", 6 * MB, 0, 5], ["admit", 3 * MB, 6, 8], ["admit", 2 * MB, 9, 10],
          ["ack", 6 * MB, 0, 5], ["admit", 9 * MB, 11, 19], ["admit", 0, 20, 20]]
    return {"comment": "HAL.ACQ.INGEST.CREDIT REQ-001: byte credit exhausted while batch credit remains",
            "window": {"bytes": 8 * MB, "batches": 4}, "events": ev}


def ack_cap():
    # AC-3: acknowledgements are keyed by the admitted range and size. The two that would push
    # credit (or what downstream holds) past the window, a duplicate and an oversized one, are
    # refused and change nothing; an acknowledgement before any admission is refused too. After
    # the real ones, outstanding is exactly the window.
    ev = [["ack", MB, 0, 1], ["grant"], ["admit", 2 * MB, 0, 1], ["admit", 2 * MB, 2, 3],
          ["ack", 2 * MB, 0, 1], ["ack", 2 * MB, 0, 1], ["ack", 5 * MB, 2, 3],
          ["ack", 9007199254740992, 2, 3], ["ack", 2 * MB, 2, 3], ["grant"], ["ack", 2 * MB, 2, 3]]
    return {"comment": "HAL.ACQ.INGEST.CREDIT AC-3: duplicate and oversized acknowledgements are "
                       "refused; outstanding ends exactly at the window",
            "window": {"bytes": 8 * MB, "batches": 4}, "events": ev}


def stopping():
    # AC-4: once stopping, neither a grant nor an acknowledgement adds credit; batches the
    # source already holds credit for still drain; stopped refuses everything.
    ev = [["grant"], ["admit", MB, 0, 0], ["admit", MB, 1, 1], ["begin_stop"], ["grant"],
          ["ack", MB, 0, 0], ["admit", MB, 2, 2], ["admit", MB, 3, 3], ["admit", MB, 4, 4],
          ["begin_stop"], ["grant"], ["finish_stop"], ["grant"], ["ack", MB, 1, 1],
          ["admit", 0, 5, 5]]
    return {"comment": "HAL.ACQ.INGEST.CREDIT AC-4: no credit while stopping or stopped",
            "window": {"bytes": 8 * MB, "batches": 4}, "events": ev}


def dup_ack():
    # Review of #608: a duplicate acknowledgement must not release a second batch. With batch A
    # acked twice, the old floored model wiped B from in flight and let the source send past
    # the window while B was still held downstream. Keyed acks refuse the duplicate.
    ev = [["grant"], ["admit", 4 * MB, 0, 9], ["admit", 4 * MB, 10, 19], ["ack", 4 * MB, 0, 9],
          ["ack", 4 * MB, 0, 9], ["grant"], ["admit", 4 * MB, 20, 29], ["admit", 4 * MB, 30, 39]]
    return {"comment": "HAL.ACQ.INGEST.CREDIT review case: a duplicate acknowledgement is refused "
                       "with credit_unknown_ack",
            "window": {"bytes": 8 * MB, "batches": 4}, "events": ev}


def oversized_ack():
    # Review of #608: an acknowledgement larger than the admitted batch must not release more
    # than that batch. Keyed acks refuse it; the exact one then succeeds.
    ev = [["grant"], ["admit", MB, 0, 0], ["admit", 6 * MB, 1, 1], ["ack", 7 * MB, 0, 0],
          ["grant"], ["admit", 2 * MB, 2, 2], ["ack", MB, 0, 0], ["admit", 2 * MB, 3, 3]]
    return {"comment": "HAL.ACQ.INGEST.CREDIT review case: an oversized acknowledgement is refused "
                       "with credit_unknown_ack",
            "window": {"bytes": 8 * MB, "batches": 4}, "events": ev}


SCRIPTS = {
    "dup_ack.json": dup_ack,
    "oversized_ack.json": oversized_ack,
    "steady.json": steady,
    "overrun.json": overrun,
    "bytes_overrun.json": bytes_overrun,
    "ack_cap.json": ack_cap,
    "stopping.json": stopping,
}


def self_check(name, doc):
    """Spot checks of the expected traces against the ACs, independent of the model."""
    exp = doc["expected"]
    if name == "steady.json":
        check(all("refused" not in l for l in exp), "AC-1 all accepted")
        check(sum(l.startswith("admit accepted") for l in exp) == 24, "AC-1 24 admits")
    if name == "overrun.json":
        check(exp[5] == "admit refused credit_exceeded gap=400-499:CREDIT_EXHAUSTED active "
              "out=4194304/0 fly=4194304/4", "AC-2 fifth refused")
    if name == "ack_cap.json":
        check(exp[0].startswith("ack refused credit_unknown_ack"), "AC-3 ack before admit refused")
        check(exp[5] == "ack refused credit_unknown_ack range=0-1 bytes=2097152 active "
              "out=6291456/3 fly=2097152/1", "AC-3 duplicate refused")
        check(exp[6].startswith("ack refused credit_unknown_ack range=2-3 bytes=5242880"),
              "AC-3 oversized refused")
        check(exp[7].startswith("ack refused credit_unknown_ack"), "AC-3 huge refused")
        check(exp[8].endswith("out=8388608/4 fly=0/0"), "AC-3 at window exactly")
        check(exp[-1].startswith("ack refused") and exp[-1].endswith("out=8388608/4 fly=0/0"),
              "AC-3 late duplicate refused")
    if name == "dup_ack.json":
        check(exp[4].startswith("ack refused credit_unknown_ack range=0-9 bytes=4194304"),
              "duplicate refused")
        check(exp[5].startswith("grant granted=0/0 "), "the duplicate added no credit")
        check(exp[7].startswith("admit refused credit_exceeded"), "no send past the window")
    if name == "oversized_ack.json":
        check(exp[3].startswith("ack refused credit_unknown_ack range=0-0 bytes=7340032"),
              "oversized refused")
        check(exp[5].startswith("admit refused credit_exceeded"), "no send past the window")
        check(exp[7] == "admit accepted active out=0/2 fly=8388608/2", "exact ack frees 1 MB")
    if name == "stopping.json":
        check(exp[4].startswith("grant granted=0/0 stopping"), "AC-4 no grant while stopping")
        check(exp[5].startswith("ack returned=0/0 stopping"), "AC-4 no return while stopping")
        check(exp[12].startswith("grant granted=0/0 stopped"), "AC-4 no grant when stopped")


def render(doc):
    return json.dumps(doc, indent=1) + "\n"


# ---------------------------------------------------------------- Philox4x32-10 (KRN.NUM.RNG)
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


for _ctr, _key, _want in [
    ((0, 0, 0, 0), (0, 0), (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)),
    ((MASK,) * 4, (MASK, MASK), (0x408F276D, 0x41C83B0E, 0xA20BC7C6, 0x6D5451FD)),
]:
    check(philox(_ctr, _key) == _want, "philox known answer")


class Cursor:
    def __init__(self, seed, stream):
        self.key = (seed & MASK, seed >> 32)
        self.stream = stream
        self.index = 0

    def next_u64(self):
        i, s = self.index, self.stream
        w = philox((i & MASK, i >> 32, s & MASK, s >> 32), self.key)
        self.index += 1
        return (w[1] << 32) | w[0]

    def below(self, n):
        check(n > 0, "below(0)")
        threshold = ((1 << 64) - n) % n
        while True:
            x = self.next_u64()
            if x >= threshold:
                return x % n


SEED = 11
N_SCRIPTS = 10_000


def gen_script(s):
    c = Cursor(SEED, s)
    wb = 1 + c.below(1 << 20)
    wn = 1 + c.below(8)
    n = 1 + c.below(48)
    nxt = 0
    ev = []
    admits = []
    for _ in range(n):
        k = c.below(100)
        if k < 40:
            b = 1 + c.below(wb)
            ln = 1 + c.below(16)
            ev.append(["admit", b, nxt, nxt + ln - 1])
            admits.append(ev[-1])
            nxt += ln
        elif k < 70:
            j = c.below(len(admits) + 1)
            if j < len(admits):
                _, b, first, last = admits[j]
                over = 1 if c.below(4) == 0 else 0
                ev.append(["ack", b + over, first, last])
            else:
                ev.append(["ack", 1 + c.below(wb), nxt, nxt])
        elif k < 90:
            ev.append(["grant"])
        elif k < 96:
            ev.append(["begin_stop"])
        else:
            ev.append(["finish_stop"])
    return {"bytes": wb, "batches": wn}, ev


def property_digest():
    h = hashlib.sha256()
    refused = stopped = unknown = acked = 0
    for s in range(N_SCRIPTS):
        window, ev = gen_script(s)
        for i, line in enumerate(run(window, ev)):
            refused += " refused " in line
            stopped += line.startswith("finish_stop")
            unknown += "credit_unknown_ack" in line
            acked += line.startswith("ack returned")
            h.update(f"{s} {i} {line}\n".encode())
    check(refused > 1000 and stopped > 1000, "generator exercises refusals and stops")
    check(unknown > 1000 and acked > 1000, "generator exercises matched and unknown acks")
    return h.hexdigest()


def build():
    files = {}
    for name, fn in SCRIPTS.items():
        doc = fn()
        doc["expected"] = run(doc["window"], doc["events"])
        self_check(name, doc)
        files[name] = render(doc)
    files["property-v1.txt"] = (
        "# HAL.ACQ.INGEST.CREDIT AC-5: SHA-256 of the trace lines of the seeded scripts.\n"
        "# Written by make_fixtures.py; see its docstring for the generator and line format.\n"
        f"seed {SEED}\nscripts {N_SCRIPTS}\nsha256 sha256:{property_digest()}\n"
    )
    return files


def main(argv):
    files = build()
    if "--check" in argv:
        bad = [n for n, t in files.items() if not (HERE / n).exists()
               or (HERE / n).read_text(encoding="utf-8") != t]
        if bad:
            print("stale fixtures: " + ", ".join(bad), file=sys.stderr)
            return 1
        print(f"ok: {len(files)} fixtures up to date")
        return 0
    for n, t in files.items():
        (HERE / n).write_text(t, encoding="utf-8", newline="\n")
    print(f"wrote {len(files)} fixtures")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
