"""Writes the fixtures of spec node OUTR.COMM.MOD and their expected results (CC-BY-4.0).

Every case, handle and channel here is invented for the tests; there is no real moderation entry.
Handles (mod-ash, mod-birch, mod-cedar, mod-dune) are made up. Expected results are computed
here, independently of the Rust code: the finding lines in EXPECTED are hand-labelled from the
spec's rules, and the counts in `expected-*.json` are computed by `counts()` below with Python's
datetime. Nothing here was produced by running the Rust code.

Tests (crates/fid-outreach/tests/moderation.rs) copy `log.toml` to a temporary folder outside the
repository and run, with the fixture's roster:
  fid-outreach mod-check --log <tmp>/log.toml --roster roster.toml --today 2027-04-02 --json
  fid-outreach mod-stats --log <tmp>/log.toml --quarter 2027Q1 --target-hours 72 --out counts.json

Run from the repository root: python fixtures/outr/comm/mod/make_fixtures.py [--check]
"""

import datetime as dt
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
HEADER = "# Synthetic moderation log for tests (CC-BY-4.0). Every case and handle is invented.\n"

ROSTER = (
    "# Synthetic moderator roster for tests (CC-BY-4.0). Handles are invented.\n"
    'moderators = ["mod-ash", "mod-birch", "mod-cedar"]\n'
)

# (id, filed_at, acknowledged_at, channel, outcome, moderator, parties_conflicted, handover)
OK_CASES = [
    ("MOD-2027-001", "2027-01-04T09:00:00Z", "2027-01-04T15:30:00Z", "forum", "no_action", "mod-ash", [], None),
    # Acknowledged exactly 72 hours after filing: within the target.
    ("MOD-2027-002", "2027-01-20T22:15:00Z", "2027-01-23T22:15:00Z", "chat", "warning", "mod-birch", [], None),
    # Acknowledged 72 hours and 1 minute after filing: late.
    ("MOD-2027-003", "2027-02-11T08:00:00Z", "2027-02-14T08:01:00Z", "issues", "removal", "mod-cedar", [], None),
    # mod-ash had a conflict and handed the case to mod-birch.
    (
        "MOD-2027-004",
        "2027-03-02T12:00:00Z",
        "2027-03-03T10:00:00Z",
        "forum",
        "ban",
        "mod-birch",
        ["mod-ash"],
        ("mod-ash", "mod-birch"),
    ),
    ("MOD-2027-005", "2027-03-30T23:30:00Z", "2027-03-31T08:00:00Z", "private", "no_action", "mod-ash", [], None),
]

OLD_CASE = ("MOD-2024-031", "2024-12-01T10:00:00Z", "2024-12-02T09:00:00Z", "chat", "warning", "mod-cedar", [], None)

# (quarter, outcome, ack_within_target)
OLD_ANON = [("2024Q3", "removal", True), ("2024Q3", "no_action", False)]


def replace(cases, case_id, **changes):
    keys = ["id", "filed_at", "acknowledged_at", "channel", "outcome", "moderator", "parties_conflicted", "handover"]
    out = []
    for c in cases:
        if c[0] == case_id:
            d = dict(zip(keys, c))
            d.update(changes)
            c = tuple(d[k] for k in keys)
        out.append(c)
    return out


def q(s):
    return '"' + s + '"'


def render(cases, anon=(), extra=None):
    """`extra` maps a case ID to additional raw TOML lines for that case."""
    lines = [HEADER.rstrip("\n")]
    for c in cases:
        cid, filed, ack, chan, outcome, mod, conflicted, handover = c
        lines += [
            "",
            "[[case]]",
            f"id = {q(cid)}",
            f"filed_at = {q(filed)}",
            f"acknowledged_at = {q(ack)}",
            f"channel = {q(chan)}",
            f"outcome = {q(outcome)}",
            f"moderator = {q(mod)}",
            "parties_conflicted = [" + ", ".join(q(h) for h in conflicted) + "]",
        ]
        if handover:
            lines.append(f"handover = {{ from = {q(handover[0])}, to = {q(handover[1])} }}")
        lines += (extra or {}).get(cid, [])
    for quarter, outcome, ack in anon:
        lines += ["", "[[anon]]", f"quarter = {q(quarter)}", f"outcome = {q(outcome)}"]
        lines.append(f"ack_within_target = {'true' if ack else 'false'}")
    return "\n".join(lines) + "\n"


def parse_utc(s):
    return dt.datetime.strptime(s, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc)


def quarter_of(stamp):
    d = parse_utc(stamp)
    return f"{d.year:04d}Q{(d.month - 1) // 3 + 1}"


def counts(cases, anon, quarter, target_hours):
    """Reference implementation of REQ-OUTR.COMM.MOD-005 counts."""
    by = {"no_action": 0, "warning": 0, "removal": 0, "ban": 0}
    total = within = 0
    for c in cases:
        if quarter_of(c[1]) != quarter:
            continue
        total += 1
        by[c[4]] += 1
        if parse_utc(c[2]) - parse_utc(c[1]) <= dt.timedelta(hours=target_hours):
            within += 1
    for aq, outcome, ack in anon:
        if aq != quarter:
            continue
        total += 1
        by[outcome] += 1
        within += 1 if ack else 0
    obj = {
        "quarter": quarter,
        "total": total,
        "by_outcome": by,
        "ack_within_target": {"count": within, "total": total, "target_hours": target_hours},
    }
    # RFC 8785 for this shape: sorted keys, no spaces, integers only.
    return json.dumps(obj, sort_keys=True, separators=(",", ":"))


FIXTURES = {
    "ok": {"log.toml": render(OK_CASES)},
    "conflict": {
        "log.toml": render(replace(OK_CASES, "MOD-2027-004", moderator="mod-ash", handover=None)),
    },
    "old": {"log.toml": render([OLD_CASE] + OK_CASES[:1], OLD_ANON)},
    "bad-category": {"log.toml": render(replace(OK_CASES, "MOD-2027-003", outcome="suspension"))},
    "unknown-moderator": {"log.toml": render(replace(OK_CASES, "MOD-2027-005", moderator="mod-dune"))},
    # Personal data the format does not allow: an e-mail address as the handle, and an extra key.
    "personal-data": {
        "log.toml": render(
            replace(OK_CASES, "MOD-2027-001", moderator="reporter@example.invalid"),
            extra={"MOD-2027-001": ['reporter_name = "Invented Person"']},
        )
    },
}

# Hand-labelled from the spec's rules: mod-check with --today 2027-04-02, text-mode finding lines.
EXPECTED = {
    "ok": (0, []),
    "conflict": (1, ["conflict_not_handed_over MOD-2027-004"]),
    "old": (1, ["retention_overdue MOD-2024-031"]),
    "bad-category": (1, ["field_invalid outcome MOD-2027-003"]),
    "unknown-moderator": (1, ["moderator_unknown MOD-2027-005"]),
    "personal-data": (1, ["field_invalid moderator MOD-2027-001", "field_invalid reporter_name MOD-2027-001"]),
}

COUNTS = {
    ("ok", "expected-2027Q1.json"): (OK_CASES, [], "2027Q1", 72),
    ("old", "expected-2024Q3.json"): ([OLD_CASE] + OK_CASES[:1], OLD_ANON, "2024Q3", 72),
    ("old", "expected-2024Q4.json"): ([OLD_CASE] + OK_CASES[:1], OLD_ANON, "2024Q4", 72),
}


def outputs():
    files = {}
    for name, fs in FIXTURES.items():
        for fname, text in fs.items():
            files[os.path.join(name, fname)] = text
        files[os.path.join(name, "roster.toml")] = ROSTER
    for (name, fname), args in COUNTS.items():
        files[os.path.join(name, fname)] = counts(*args)
    return files


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, text in outputs().items():
        path = os.path.join(HERE, rel)
        if check:
            try:
                with open(path, encoding="utf-8", newline="") as f:
                    if f.read() != text:
                        stale.append(rel)
            except FileNotFoundError:
                stale.append(rel)
        else:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8", newline="\n") as f:
                f.write(text)
    if stale:
        print("stale fixtures: " + ", ".join(sorted(stale)))
        sys.exit(1)


if __name__ == "__main__":
    main()
