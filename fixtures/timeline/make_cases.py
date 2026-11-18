"""Independent Python reference for spec node ECO.COMM.TIMELINE (crate fid-timeline).

Writes the event fixtures, the configs and the expected results consumed by
crates/fid-timeline/tests/acceptance.rs:

  *.json / *.toml   event timelines and configs (generated from day offsets below)
  expected.tsv      case, events, config, now, clock_seconds, clock_days, action
  timestamps.tsv    RFC 3339 string -> UTC epoch seconds, or "invalid"

The error fixtures (bad-*.json, bad-*.toml, malformed.json) are hand-written, checked in
as-is, and hand-labelled in ERROR_CASES; this script only checks that its own reader
also rejects them with the same code.

This is a separate implementation, written from the spec rules, not from the Rust code:
it parses times with the standard `datetime` module and computes the clock from a list
of blocked intervals. Standard library only. Run from anywhere:

    python fixtures/timeline/make_cases.py           # rewrite fixtures
    python fixtures/timeline/make_cases.py --check   # fail if anything would change
"""

import json
import re
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
EPOCH = datetime(1970, 1, 1, tzinfo=timezone.utc)
# Day 0 of the generated timelines.
DAY0 = datetime(2026, 3, 2, tzinfo=timezone.utc)
DAY = 86400

KINDS = {
    "opened", "review_changes_requested", "review_approved", "review_commented",
    "label_added", "label_removed", "commit_pushed", "comment", "review_requested",
    "bot_reminder_posted", "bot_close_proposed",
}
LABEL = "waiting-on-author"
BOT = "mm-ci[bot]"


class Failure(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


# ---------------------------------------------------------------- time

TS = re.compile(r"([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})([Zz]|([+-])([0-9]{2}):([0-9]{2}))")


def epoch_seconds(s):
    """RFC 3339 with mandatory offset, whole seconds; None if invalid."""
    m = TS.fullmatch(s)
    if not m:
        return None
    y, mo, d, h, mi, se = (int(m.group(i)) for i in range(1, 7))
    if m.group(7) in ("Z", "z"):
        off = timedelta(0)
    else:
        oh, om = int(m.group(9)), int(m.group(10))
        if oh > 23 or om > 59:
            return None
        off = timedelta(hours=oh, minutes=om)
        if m.group(8) == "-":
            off = -off
    try:
        dt = datetime(y, mo, d, h, mi, se, tzinfo=timezone(off))
    except ValueError:
        return None
    return (dt - EPOCH) // timedelta(seconds=1)


def iso(day, hours=0, minutes=0, seconds=0, tz_minutes=0):
    """Day-offset time as RFC 3339, written in the given UTC offset."""
    t = DAY0 + timedelta(days=day, hours=hours, minutes=minutes, seconds=seconds)
    t = t.astimezone(timezone(timedelta(minutes=tz_minutes)))
    s = t.strftime("%Y-%m-%dT%H:%M:%S")
    if tz_minutes == 0:
        return s + "Z"
    sign = "+" if tz_minutes > 0 else "-"
    a = abs(tz_minutes)
    return f"{s}{sign}{a // 60:02d}:{a % 60:02d}"


# ---------------------------------------------------------------- reference reader

def read_events(path):
    try:
        data = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_dups)
    except (ValueError, UnicodeDecodeError):
        raise Failure("TIMELINE-INPUT")
    if not isinstance(data, list):
        raise Failure("TIMELINE-INPUT")
    out = []
    for e in data:
        if not isinstance(e, dict) or set(e) - {"at", "actor", "kind", "label"}:
            raise Failure("TIMELINE-INPUT")
        for k in ("at", "actor", "kind"):
            if not isinstance(e.get(k), str):
                raise Failure("TIMELINE-INPUT")
        at = epoch_seconds(e["at"])
        if at is None or e["kind"] not in KINDS or not e["actor"].strip():
            raise Failure("TIMELINE-INPUT")
        is_label = e["kind"] in ("label_added", "label_removed")
        if is_label != isinstance(e.get("label"), str) or (not is_label and "label" in e):
            raise Failure("TIMELINE-INPUT")
        out.append({"at": at, "actor": e["actor"], "kind": e["kind"], "label": e.get("label")})
    return out


def no_dups(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ValueError("duplicate key")
    return dict(pairs)


def read_config(path):
    if path is None:
        return {"remind_days": 14, "close_days": 30, "author": None, "maintainers": None, "bots": []}
    text = path.read_text(encoding="utf-8")
    if text.lstrip().startswith("{"):
        try:
            raw = json.loads(text, object_pairs_hook=no_dups)
        except ValueError:
            raise Failure("TIMELINE-CONFIG")
    else:
        import tomllib  # Python 3.11+; only this generator needs it
        try:
            raw = tomllib.loads(text)
        except tomllib.TOMLDecodeError:
            raise Failure("TIMELINE-CONFIG")
    cfg = {"remind_days": 14, "close_days": 30, "author": None, "maintainers": None, "bots": []}
    for k, v in raw.items():
        if k in ("remind_days", "close_days"):
            if type(v) is not int or not 1 <= v <= 36500:
                raise Failure("TIMELINE-CONFIG")
        elif k == "author":
            if not isinstance(v, str) or not v.strip():
                raise Failure("TIMELINE-CONFIG")
        elif k in ("maintainers", "bots"):
            if not isinstance(v, list) or not all(isinstance(x, str) and x.strip() for x in v):
                raise Failure("TIMELINE-CONFIG")
        else:
            raise Failure("TIMELINE-CONFIG")
        cfg[k] = v
    return cfg


# ---------------------------------------------------------------- reference evaluator

def evaluate(events, cfg, now):
    """Returns (clock_seconds, action). Interval-list formulation of the spec rules."""
    ordered = [e for _, e in sorted(enumerate(events), key=lambda p: (p[1]["at"], p[0]))]
    low = str.lower
    author = cfg["author"]
    if author is None:
        opened = [e for e in ordered if e["kind"] == "opened"]
        if not opened:
            raise Failure("TIMELINE-CONFIG")
        author = opened[0]["actor"]
    bots = {low(b) for b in cfg["bots"]}
    maint = None if cfg["maintainers"] is None else {low(m) for m in cfg["maintainers"]}

    # Pass 1: blocker transitions as (position, time, "start"/"stop").
    seen = [e for e in ordered if e["at"] <= now]
    transitions = []
    blocked = False
    for pos, e in enumerate(seen):
        if e["kind"].startswith("bot_") or low(e["actor"]) in bots:
            continue
        who = low(e["actor"])
        is_author = who == low(author)
        is_maint = not is_author and (maint is None or who in maint)
        lab = e["label"] == LABEL
        if not blocked and is_maint and (e["kind"] == "review_changes_requested" or (e["kind"] == "label_added" and lab)):
            blocked = True
            transitions.append((pos, e["at"], "start"))
        elif blocked and ((is_author and e["kind"] in ("commit_pushed", "comment", "review_requested"))
                          or (e["kind"] == "label_removed" and lab)):
            blocked = False
            transitions.append((pos, e["at"], "stop"))

    # Pass 2: intervals.
    intervals = []
    for t in transitions:
        if t[2] == "start":
            intervals.append([t[1], None])
        else:
            intervals[-1][1] = t[1]
    clock = sum((end if end is not None else now) - start for start, end in intervals)

    if not blocked:
        return clock, "none"
    last_stop = max([p for p, _, k in transitions if k == "stop"], default=-1)
    reminded = any(e["kind"] == "bot_reminder_posted" for e in seen[last_stop + 1:])
    proposed = any(e["kind"] == "bot_close_proposed" for e in seen)
    if clock >= cfg["close_days"] * DAY and not proposed:
        return clock, "propose-close"
    if clock >= cfg["remind_days"] * DAY and not reminded:
        return clock, "remind"
    return clock, "none"


# ---------------------------------------------------------------- fixtures

def ev(at, actor, kind, label=None):
    e = {"at": at, "actor": actor, "kind": kind}
    if label is not None:
        e["label"] = label
    return e


A, B, C, D = "alice", "bob", "carol", "dave"
STALLED = [ev(iso(0), A, "opened"), ev(iso(0), B, "review_changes_requested")]
EXACT_E1 = iso(0, hours=4, minutes=30, tz_minutes=330)        # 10:00+05:30
EXACT_E2 = iso(6, hours=25, tz_minutes=-120)                  # day 7 01:00Z as -02:00
EXACT_E3 = iso(7, hours=1, minutes=30, seconds=7, tz_minutes=-480)

TIMELINES = {
    # AC-1: changes requested day 0, author push day 5, changes requested day 6.
    "active.json": [ev(iso(0), A, "opened"), ev(iso(0), B, "review_changes_requested"),
                    ev(iso(5), A, "commit_pushed"), ev(iso(6), B, "review_changes_requested")],
    # AC-2.
    "stalled.json": STALLED,
    "stalled-reminded.json": STALLED + [ev(iso(14), BOT, "bot_reminder_posted")],
    # AC-3: bot comments on days 3 and 10.
    "bots.json": STALLED + [ev(iso(3), BOT, "comment"), ev(iso(10), BOT, "comment")],
    # REQ-002: a bot removing the label or requesting changes changes nothing.
    "bots-noise.json": [ev(iso(0), A, "opened"), ev(iso(1), C, "label_added", LABEL),
                        ev(iso(4), BOT, "label_removed", LABEL), ev(iso(8), A, "comment"),
                        ev(iso(9), BOT, "review_changes_requested"), ev(iso(9), BOT, "label_added", LABEL)],
    # AC-4: review requested, no maintainer response.
    "maintainer-blocked.json": [ev(iso(0), A, "opened"), ev(iso(0), A, "review_requested")],
    # Reminder idempotence resets when the author stops being the blocker.
    "reblock.json": [ev(iso(0), A, "opened"), ev(iso(0), B, "review_changes_requested"),
                     ev(iso(14), BOT, "bot_reminder_posted"), ev(iso(16), A, "commit_pushed"),
                     ev(iso(17), C, "review_changes_requested")],
    "reblock-reminded.json": [ev(iso(0), A, "opened"), ev(iso(0), B, "review_changes_requested"),
                              ev(iso(14), BOT, "bot_reminder_posted"), ev(iso(16), A, "commit_pushed"),
                              ev(iso(17), C, "review_changes_requested"),
                              ev(iso(17, hours=1), BOT, "bot_reminder_posted")],
    "close-proposed.json": STALLED + [ev(iso(14), BOT, "bot_reminder_posted"),
                                      ev(iso(30), BOT, "bot_close_proposed")],
    # Label rules: only waiting-on-author; dave is not a maintainer in config.toml.
    "label.json": [ev(iso(0), A, "opened"), ev(iso(0, hours=6), B, "label_added", LABEL),
                   ev(iso(1), B, "label_added", "needs-ci"), ev(iso(3, hours=6), C, "label_removed", LABEL),
                   ev(iso(4), D, "label_added", LABEL)],
    "double.json": [ev(iso(0), A, "opened"), ev(iso(0), B, "review_changes_requested"),
                    ev(iso(1), B, "label_added", LABEL), ev(iso(2), A, "comment"),
                    ev(iso(3), B, "label_removed", LABEL), ev(iso(4), B, "review_approved"),
                    ev(iso(5), A, "review_commented")],
    "dave.json": [ev(iso(0), A, "opened"), ev(iso(0), D, "review_changes_requested")],
    # Mixed UTC offsets and second precision.
    "exact.json": [ev(iso(0), A, "opened"), ev(EXACT_E1, B, "review_changes_requested"),
                   ev(EXACT_E2, A, "review_requested"), ev(EXACT_E3, C, "review_changes_requested")],
    # Leap day inside the interval.
    "leap.json": [ev("2028-02-28T12:00:00Z", A, "opened"), ev("2028-02-28T12:00:00Z", B, "review_changes_requested")],
    # Equal timestamps keep file order; the file is not in time order.
    "ties.json": [ev(iso(1), B, "review_changes_requested"), ev(iso(1), A, "comment"),
                  ev(iso(0), A, "opened"), ev(iso(1), C, "review_changes_requested"),
                  ev(iso(0, hours=12), B, "review_changes_requested"), ev(iso(0, hours=18), A, "comment")],
    # Events after --now are ignored.
    "future.json": [ev(iso(0), A, "opened"), ev(iso(0), B, "review_changes_requested"),
                    ev(iso(6), BOT, "bot_reminder_posted"), ev(iso(10), A, "commit_pushed")],
    # No opened event: needs an author in the config.
    "noauthor.json": [ev(iso(0), B, "review_changes_requested")],
}

CONFIGS = {
    "config.toml": '# Fixture config for fid-timeline tests.\nauthor = "alice"\nmaintainers = ["bob", "Carol"]\n'
                   'bots = ["mm-ci[bot]"]\nremind_days = 14\nclose_days = 30\n',
    # AC-3 config: names the bot account.
    "bots.toml": '# Bot accounts whose events never move the clock (AC-3).\nbots = [\n  "mm-ci[bot]",  # CI\n'
                 '  "dependabot[bot]",\n]\nmaintainers = ["bob", "carol"]\nremind_days = 14\nclose_days = 30\n',
    # Only a bot list: author from the opened event, every other human is a maintainer.
    "bots-only.toml": "bots = ['MM-CI[bot]']\n",
    # REQ-005: thresholds come from the config.
    "short.json": '{"remind_days": 2, "close_days": 3, "author": "alice", "maintainers": ["bob"], '
                  '"bots": ["mm-ci[bot]"]}\n',
}


def utc(seconds):
    return datetime.fromtimestamp(seconds, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# exact.json: the second blocked interval starts at E3; the clock reaches 14 days when
# it has run 14 days minus the first interval (E1..E2).
EXACT_14D = epoch_seconds(EXACT_E3) + 14 * DAY - (epoch_seconds(EXACT_E2) - epoch_seconds(EXACT_E1))

# case, events, config (None = no --config), now (RFC 3339 string)
CASES = [
    ("exact-14d-minus-1s", "exact.json", "config.toml", utc(EXACT_14D - 1)),
    ("exact-14d", "exact.json", "config.toml", utc(EXACT_14D)),
    ("ac1", "active.json", None, iso(12)),
    ("ac1-config", "active.json", "config.toml", iso(12)),
    ("ac2-day15", "stalled.json", None, iso(15)),
    ("ac2-day15-config", "stalled.json", "config.toml", iso(15)),
    ("ac2-day13", "stalled.json", "config.toml", iso(13, hours=23, minutes=59, seconds=59)),
    ("ac2-day14", "stalled.json", "config.toml", iso(14)),
    ("ac2-reminded-day14", "stalled-reminded.json", "config.toml", iso(14)),
    ("ac2-reminded-day20", "stalled-reminded.json", "config.toml", iso(20)),
    ("ac2-reminded-day31", "stalled-reminded.json", "config.toml", iso(31)),
    ("ac2-day31-no-reminder", "stalled.json", "config.toml", iso(31)),
    ("ac3", "bots.json", "bots.toml", iso(15)),
    ("bots-noise", "bots-noise.json", "bots-only.toml", iso(30)),
    ("ac4", "maintainer-blocked.json", None, iso(40)),
    ("ac4-config", "maintainer-blocked.json", "config.toml", iso(40)),
    ("reblock-unblocked", "reblock.json", "config.toml", iso(16, hours=12)),
    ("reblock", "reblock.json", "config.toml", iso(18)),
    ("reblock-reminded", "reblock-reminded.json", "config.toml", iso(18)),
    ("close-proposed", "close-proposed.json", "config.toml", iso(45)),
    ("label-config", "label.json", "config.toml", iso(20)),
    ("label-default", "label.json", None, iso(20)),
    ("double", "double.json", "config.toml", iso(10)),
    ("dave-config", "dave.json", "config.toml", iso(20)),
    ("dave-default", "dave.json", None, iso(20)),
    ("ties", "ties.json", "config.toml", iso(15, hours=1)),
    ("future-day5", "future.json", "config.toml", iso(5)),
    ("future-day15", "future.json", "config.toml", iso(15)),
    ("short-day1", "stalled.json", "short.json", iso(1)),
    ("short-day2", "stalled.json", "short.json", iso(2)),
    ("short-day3", "stalled.json", "short.json", iso(3)),
    ("leap-14d", "leap.json", "config.toml", "2028-03-13T12:00:00Z"),
    ("leap-14d-minus-1s", "leap.json", "config.toml", "2028-03-13T11:59:59Z"),
    ("leap-offset", "leap.json", "config.toml", "2028-03-13T13:00:00+01:00"),
    ("noauthor-config", "noauthor.json", "config.toml", iso(20)),
]

# Hand-labelled error cases: case, events, config, now, code.
ERROR_CASES = [
    ("ac5-malformed", "malformed.json", None, iso(1), "TIMELINE-INPUT"),
    ("not-array", "bad-not-array.json", None, iso(1), "TIMELINE-INPUT"),
    ("unknown-kind", "bad-kind.json", None, iso(1), "TIMELINE-INPUT"),
    ("naive-time", "bad-naive-time.json", None, iso(1), "TIMELINE-INPUT"),
    ("fraction", "bad-fraction.json", None, iso(1), "TIMELINE-INPUT"),
    ("feb29", "bad-feb29.json", None, iso(1), "TIMELINE-INPUT"),
    ("missing-label", "bad-missing-label.json", None, iso(1), "TIMELINE-INPUT"),
    ("dup-key", "bad-dup-key.json", None, iso(1), "TIMELINE-INPUT"),
    ("unknown-field", "bad-unknown-field.json", None, iso(1), "TIMELINE-INPUT"),
    ("bad-now", "stalled.json", None, "2026-03-17", "TIMELINE-INPUT"),
    ("now-fraction", "stalled.json", None, "2026-03-17T00:00:00.5Z", "TIMELINE-INPUT"),
    ("noauthor", "noauthor.json", None, iso(20), "TIMELINE-CONFIG"),
    ("config-string-days", "stalled.json", "bad-days.toml", iso(1), "TIMELINE-CONFIG"),
    ("config-zero-days", "stalled.json", "bad-zero.toml", iso(1), "TIMELINE-CONFIG"),
    ("config-table", "stalled.json", "bad-table.toml", iso(1), "TIMELINE-CONFIG"),
    ("config-unknown", "stalled.json", "bad-unknown.json", iso(1), "TIMELINE-CONFIG"),
    ("config-syntax", "stalled.json", "bad-syntax.json", iso(1), "TIMELINE-CONFIG"),
]

TIMESTAMPS = [
    "1970-01-01T00:00:00Z", "1970-01-01T00:00:01+00:00", "1969-12-31T23:59:59Z",
    "2000-02-29T00:00:00Z", "2000-03-01T00:00:00Z", "1900-03-01T00:00:00Z",
    "2024-02-29T12:34:56+02:00", "2026-03-02T10:00:00+05:30", "2026-03-08T23:00:00-02:00",
    "2026-12-31T23:59:59-23:59", "0001-01-01T12:00:00Z", "9999-12-31T23:59:59Z",
    "2038-01-19T03:14:08Z", "2026-11-17T08:00:00-08:00",
    # RFC 3339 5.6: "T" and "Z" may be lowercase.
    "2026-01-01t00:00:00z", "2026-03-02t10:00:00+05:30",
    # invalid
    "2023-02-29T00:00:00Z", "1900-02-29T00:00:00Z", "2026-04-31T00:00:00Z", "2026-13-01T00:00:00Z",
    "2026-00-10T00:00:00Z", "2026-01-00T00:00:00Z", "0000-01-01T00:00:00Z", "2026-01-01T24:00:00Z",
    "2026-01-01T23:60:00Z", "2026-12-31T23:59:60Z", "2026-01-01T00:00:00", "2026-01-01T00:00:00.000Z",
    "2026-01-01 00:00:00Z", "2026-01-01x00:00:00Z", "2026-01-01T00:00:00y", "2026-01-01T00:00:00+24:00", "2026-01-01T00:00:00+05:60",
    "2026-01-01T00:00:00+0530", "26-01-01T00:00:00Z", "2026-1-01T00:00:00Z", "2026-01-01T00:00:00Z ",
    "", "２０２６-01-01T00:00:00Z",
]


def build():
    files = {}
    for name, events in TIMELINES.items():
        files[name] = json.dumps(events, indent=1) + "\n"
    files.update(CONFIGS)
    # The reference reads the timelines back from disk, so write them before evaluating.
    if "--check" not in sys.argv:
        for name, text in files.items():
            (HERE / name).write_text(text, encoding="utf-8", newline="\n")

    rows = ["# case\tevents\tconfig\tnow\tclock_seconds\tclock_days\taction  (generated by make_cases.py)"]
    for case, events, config, now in CASES:
        try:
            cfg = read_config(None if config is None else HERE / config)
            clock, action = evaluate(read_events(HERE / events), cfg, epoch_seconds(now))
            rows.append(f"{case}\t{events}\t{config or '-'}\t{now}\t{clock}\t{clock // DAY}\t{action}")
        except Failure as f:
            rows.append(f"{case}\t{events}\t{config or '-'}\t{now}\t-\t-\terror:{f.code}")
    for case, events, config, now, code in ERROR_CASES:
        try:
            if epoch_seconds(now) is None:
                raise Failure("TIMELINE-INPUT")
            got = None
            events_l = read_events(HERE / events)
            got_cfg = read_config(None if config is None else HERE / config)
            evaluate(events_l, got_cfg, epoch_seconds(now))
        except Failure as f:
            got = f.code
        assert got == code, f"{case}: reference says {got}, hand label says {code}"
        rows.append(f"{case}\t{events}\t{config or '-'}\t{now}\t-\t-\terror:{code}")
    files["expected.tsv"] = "\n".join(rows) + "\n"

    ts_rows = ["# rfc3339\tepoch_seconds_or_invalid  (generated by make_cases.py)"]
    for s in TIMESTAMPS:
        v = epoch_seconds(s)
        ts_rows.append(f"{s}\t{'invalid' if v is None else v}")
    files["timestamps.tsv"] = "\n".join(ts_rows) + "\n"
    return files


def main():
    files = build()
    stale = []
    for name, text in files.items():
        p = HERE / name
        if "--check" in sys.argv:
            if not p.exists() or p.read_text(encoding="utf-8") != text:
                stale.append(name)
        else:
            p.write_text(text, encoding="utf-8", newline="\n")
    if stale:
        print("stale:", ", ".join(stale))
        return 1
    print("ok" if "--check" in sys.argv else f"wrote {len(files)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
