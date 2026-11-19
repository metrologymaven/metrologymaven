#!/usr/bin/env python3
"""rfc.py: RFC comment-period tooling for decision records (spec node ECO.COMM.RFC).

Standard library only. Fully offline and deterministic: the current time is always given with
--now, approvals come from a recorded JSON export, and output is sorted.

    python tools/rfc.py start D-NNN --now 2026-12-01T09:00:00Z [--kind app] [--dir decisions]
    python tools/rfc.py status --now 2026-12-05T00:00:00Z [--dir decisions]
    python tools/rfc.py approve-check D-NNN --approvals reviews.json --now ... [--dir decisions]

All commands take --config (default tools/rfc_config.json), a JSON object with the keys
short_days, long_days, long_kinds, kinds, maintainers and aliases (see GOVERNANCE.md, D-003).

`start` inserts comment_period_start, comment_period_days and (when the record has none) rfc_kind
into the record's frontmatter, just before its closing `---`, and changes no other byte. It
edits the working tree only; it never commits.

Exit codes: 0 success; 1 a rule or input error, printed to stderr as `RFC-<CODE>: <message>`
(RFC-ALREADY-STARTED, RFC-NO-RECORD, RFC-SELF-APPROVAL, RFC-PERIOD-OPEN, RFC-CONFIG, RFC-INPUT);
2 a command-line usage error.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
import tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(Path(__file__).resolve().parent))

from claims import fmt_time  # noqa: E402  (ECO.COMM.CLAIM's UTC formatter)
from scope import parse_front  # noqa: E402  (reuse the spec-tree frontmatter parser)

DEFAULT_DIR = ROOT / "decisions"
DEFAULT_CONFIG = ROOT / "tools" / "rfc_config.json"

MAX_RECORD_BYTES = 1 << 20  # 1 MiB
MAX_APPROVALS_BYTES = 4 << 20  # 4 MiB
MAX_CONFIG_BYTES = 64 << 10  # 64 KiB
MAX_DAYS = 366

ID_RE = re.compile(r"^D-[0-9]{3,6}$")
TIME_RE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
KIND_RE = re.compile(r"^[a-z][a-z0-9-]{0,31}$")
PERIOD_FIELDS = ("comment_period_start", "comment_period_days")
CONFIG_KEYS = {"short_days", "long_days", "long_kinds", "kinds", "maintainers", "aliases"}
REQUIRED_CONFIG_KEYS = {"short_days", "long_days", "long_kinds", "maintainers"}
DEFAULT_KINDS = ["domain", "app", "process", "other"]


class RfcError(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message


# ---------------------------------------------------------------- time


def parse_time(value: str, what: str) -> datetime:
    """Parse a strict `YYYY-MM-DDTHH:MM:SSZ` UTC timestamp."""
    if not isinstance(value, str) or not TIME_RE.match(value):
        raise RfcError("RFC-INPUT", f"{what} must be a UTC time like 2026-12-01T09:00:00Z, got {value!r}")
    try:
        return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    except ValueError:
        raise RfcError("RFC-INPUT", f"{what} is not a valid date and time: {value!r}") from None


def add_days(start: datetime, days: int, what: str) -> datetime:
    """`start` plus whole days in UTC; a result past year 9999 is RFC-INPUT."""
    try:
        return start + timedelta(days=days)
    except OverflowError:
        raise RfcError("RFC-INPUT", f"{what}: {fmt_time(start)} plus {days} days is past year 9999") from None


# ---------------------------------------------------------------- files


def read_bounded(path: Path, limit: int, code: str, what: str) -> bytes:
    try:
        if path.is_symlink():
            raise RfcError("RFC-INPUT", f"{what} {path} is a symbolic link; refusing to follow it")
        if not path.is_file():
            raise RfcError(code, f"{what} {path} does not exist or is not a file")
        size = path.stat().st_size
        if size > limit:
            raise RfcError(code, f"{what} {path} is {size} bytes, over the {limit}-byte limit")
        with open(path, "rb") as fh:
            data = fh.read(limit + 1)
    except OSError as e:
        raise RfcError(code, f"cannot read {what} {path}: {e.strerror or e}") from None
    if len(data) > limit:
        raise RfcError(code, f"{what} {path} is over the {limit}-byte limit")
    return data


def decode(data: bytes, code: str, what: str) -> str:
    try:
        return data.decode("utf-8-sig")
    except UnicodeDecodeError:
        raise RfcError(code, f"{what} is not valid UTF-8") from None


# ---------------------------------------------------------------- config


def _pos_int(cfg: dict, key: str) -> int:
    v = cfg[key]
    if isinstance(v, bool) or not isinstance(v, int) or not 1 <= v <= MAX_DAYS:
        raise RfcError("RFC-CONFIG", f"config key {key} must be an integer from 1 to {MAX_DAYS}")
    return v


def _str_list(cfg: dict, key: str) -> list:
    v = cfg[key]
    if not isinstance(v, list) or not all(isinstance(x, str) and x.strip() for x in v):
        raise RfcError("RFC-CONFIG", f"config key {key} must be a list of non-empty strings")
    return v


def load_config(path: Path) -> dict:
    raw = read_bounded(path, MAX_CONFIG_BYTES, "RFC-CONFIG", "config")
    try:
        cfg = json.loads(decode(raw, "RFC-CONFIG", f"config {path}"))
    except json.JSONDecodeError as e:
        raise RfcError("RFC-CONFIG", f"config {path} is not valid JSON: {e.msg} at line {e.lineno}") from None
    except (ValueError, RecursionError):
        raise RfcError("RFC-CONFIG", f"config {path} is not valid JSON or is nested too deeply") from None
    if not isinstance(cfg, dict):
        raise RfcError("RFC-CONFIG", f"config {path} must be a JSON object")
    unknown = sorted(set(cfg) - CONFIG_KEYS)
    if unknown:
        raise RfcError("RFC-CONFIG", f"config {path} has unknown keys: {', '.join(unknown)}")
    missing = sorted(REQUIRED_CONFIG_KEYS - set(cfg))
    if missing:
        raise RfcError("RFC-CONFIG", f"config {path} lacks keys: {', '.join(missing)}")
    out = {
        "short_days": _pos_int(cfg, "short_days"),
        "long_days": _pos_int(cfg, "long_days"),
        "long_kinds": _str_list(cfg, "long_kinds"),
        "kinds": _str_list(cfg, "kinds") if "kinds" in cfg else list(DEFAULT_KINDS),
        "maintainers": {m.strip().casefold() for m in _str_list(cfg, "maintainers")},
        "aliases": {},
    }
    for k in out["kinds"]:
        if not KIND_RE.match(k):
            raise RfcError("RFC-CONFIG", f"config kind {k!r} must be lowercase letters, digits or '-'")
    stray = sorted(set(out["long_kinds"]) - set(out["kinds"]))
    if stray:
        raise RfcError("RFC-CONFIG", f"long_kinds names kinds not in kinds: {', '.join(stray)}")
    aliases = cfg.get("aliases", {})
    if not isinstance(aliases, dict):
        raise RfcError("RFC-CONFIG", "config key aliases must map a forge login to a list of raised_by values")
    for login, names in aliases.items():
        if not login.strip() or not isinstance(names, list) or not all(isinstance(n, str) and n.strip() for n in names):
            raise RfcError("RFC-CONFIG", f"aliases entry {login!r} must be a list of non-empty strings")
        out["aliases"][login.strip().casefold()] = {n.strip().casefold() for n in names}
    return out


# ---------------------------------------------------------------- records


def check_id(record_id: str) -> str:
    if not ID_RE.match(record_id):
        raise RfcError("RFC-INPUT", f"record ID must look like D-123, got {record_id!r}")
    return record_id


def find_record(directory: Path, record_id: str) -> Path:
    """The single file `<ID>.md` or `<ID>-*.md` in `directory` (symbolic links included, so that
    read_record refuses them rather than the ID looking absent). read_record_for checks its id."""
    if not directory.is_dir():
        raise RfcError("RFC-NO-RECORD", f"decision directory {directory} does not exist")
    try:
        names = sorted(p.name for p in directory.iterdir())
    except OSError as e:
        raise RfcError("RFC-NO-RECORD", f"cannot list {directory}: {e.strerror or e}") from None
    hits = [directory / n for n in names
            if (n == f"{record_id}.md" or (n.startswith(f"{record_id}-") and n.endswith(".md")))
            and ((directory / n).is_symlink() or (directory / n).is_file())]
    if not hits:
        raise RfcError("RFC-NO-RECORD", f"no decision record {record_id} in {directory}")
    if len(hits) > 1:
        raise RfcError("RFC-INPUT", f"more than one file for {record_id}: {', '.join(p.name for p in hits)}")
    return hits[0]


# Characters str.splitlines (used by scope.parse_front) treats as line breaks but bytes.splitlines
# (used by insert_fields) does not. A record containing one is refused, so both always agree on
# where frontmatter lines start and end.
ODD_LINE_BREAKS = "\x0b\x0c\x1c\x1d\x1e\x85\u2028\u2029"


def read_record(path: Path) -> tuple[bytes, dict]:
    raw = read_bounded(path, MAX_RECORD_BYTES, "RFC-INPUT", "record")
    text = decode(raw, "RFC-INPUT", f"record {path.name}")
    odd = sorted({f"U+{ord(c):04X}" for c in text if c in ODD_LINE_BREAKS})
    if odd:
        raise RfcError("RFC-INPUT", f"record {path.name} contains unusual line-break characters "
                                    f"({', '.join(odd)}); use LF or CRLF only")
    meta, err = parse_front(text)
    if meta is None:
        raise RfcError("RFC-INPUT", f"record {path.name}: {err}")
    rid = meta.get("id", "")
    if not isinstance(rid, str) or not ID_RE.match(rid):
        raise RfcError("RFC-INPUT", f"record {path.name} has no valid id (like D-123) in its frontmatter")
    return raw, meta


def read_record_for(path: Path, record_id: str) -> tuple[bytes, dict]:
    raw, meta = read_record(path)
    if meta["id"] != record_id:
        raise RfcError("RFC-INPUT", f"record {path.name} has id {meta['id']}, not {record_id}")
    return raw, meta


def scalar(meta: dict, key: str, where: str) -> str:
    v = meta.get(key, "")
    if isinstance(v, list):
        raise RfcError("RFC-INPUT", f"{where}: {key} must be a single value, not a list")
    return v


def recorded_period(meta: dict, where: str):
    """(start, days, close) from a record's frontmatter, or None when no period is recorded."""
    start_s = scalar(meta, "comment_period_start", where)
    days_s = scalar(meta, "comment_period_days", where)
    if not start_s and not days_s:
        return None
    if not start_s or not days_s:
        raise RfcError("RFC-INPUT", f"{where}: comment_period_start and comment_period_days must be set together")
    start = parse_time(start_s, f"{where}: comment_period_start")
    if not re.fullmatch(r"[0-9]{1,3}", days_s) or not 1 <= int(days_s) <= MAX_DAYS:
        raise RfcError("RFC-INPUT", f"{where}: comment_period_days must be an integer from 1 to {MAX_DAYS}")
    days = int(days_s)
    return start, days, add_days(start, days, f"{where}: comment period close")


def insert_fields(raw: bytes, fields: list) -> bytes:
    """Insert `key: value` lines before the frontmatter's closing `---`, keeping all other bytes.

    The new lines use the line ending of the opening `---` line.
    """
    lines = raw.splitlines(keepends=True)
    bom = b"\xef\xbb\xbf"
    first = lines[0][len(bom):] if lines and lines[0].startswith(bom) else (lines[0] if lines else b"")
    if first.rstrip(b"\r\n").strip() != b"---":
        raise RfcError("RFC-INPUT", "record has no frontmatter")
    eol = b"\r\n" if first.endswith(b"\r\n") else b"\n"
    for i in range(1, len(lines)):
        if lines[i].rstrip(b"\r\n").strip() == b"---":
            new = b"".join(f"{k}: {v}".encode("utf-8") + eol for k, v in fields)
            return b"".join(lines[:i]) + new + b"".join(lines[i:])
    raise RfcError("RFC-INPUT", "record frontmatter is not terminated")


def write_atomic(path: Path, data: bytes) -> None:
    fd, tmp = tempfile.mkstemp(prefix=".rfc-", dir=str(path.parent))
    try:
        with os.fdopen(fd, "wb") as fh:
            fh.write(data)
        os.replace(tmp, str(path))
    except OSError as e:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise RfcError("RFC-INPUT", f"cannot write {path}: {e.strerror or e}") from None


# ---------------------------------------------------------------- commands


def cmd_start(args, out) -> int:
    record_id = check_id(args.record)
    now = parse_time(args.now, "--now")
    cfg = load_config(Path(args.config))
    path = find_record(Path(args.dir), record_id)
    raw, meta = read_record_for(path, record_id)
    where = path.name
    if any(k in meta for k in PERIOD_FIELDS):
        raise RfcError("RFC-ALREADY-STARTED", f"{record_id} already has a comment period recorded")
    kind = scalar(meta, "rfc_kind", where)
    fields = []
    if kind:
        if args.kind and args.kind != kind:
            raise RfcError("RFC-INPUT", f"{record_id} has rfc_kind {kind}; --kind {args.kind} disagrees")
    else:
        kind = args.kind or "other"
        fields.append(("rfc_kind", kind))
    if kind not in cfg["kinds"]:
        raise RfcError("RFC-INPUT", f"rfc_kind {kind!r} is not one of: {', '.join(cfg['kinds'])}")
    days = cfg["long_days"] if kind in cfg["long_kinds"] else cfg["short_days"]
    close = add_days(now, days, "--now")  # before writing, so an overflow leaves the record untouched
    fields += [("comment_period_start", fmt_time(now)), ("comment_period_days", str(days))]
    write_atomic(path, insert_fields(raw, fields))
    out.write(f"{record_id} started {fmt_time(now)} kind={kind} days={days} closes {fmt_time(close)}\n")
    return 0


def cmd_status(args, out) -> int:
    now = parse_time(args.now, "--now")
    directory = Path(args.dir)
    if not directory.is_dir():
        raise RfcError("RFC-NO-RECORD", f"decision directory {directory} does not exist")
    try:
        paths = sorted(p for p in directory.iterdir() if p.name.startswith("D-") and p.name.endswith(".md")
                       and (p.is_symlink() or p.is_file()))
    except OSError as e:
        raise RfcError("RFC-NO-RECORD", f"cannot list {directory}: {e.strerror or e}") from None
    rows, bad = [], []
    for p in paths:
        try:
            _, meta = read_record(p)
            rid = meta["id"]
            period = recorded_period(meta, p.name)
        except RfcError as e:
            bad.append(f"{e.code}: {e.message}")
            continue
        if period is None:
            continue
        _, _, close = period
        rows.append((rid, "closed" if now >= close else "open", fmt_time(close)))
    for rid, state, close in sorted(rows):
        out.write(f"{rid} {state} closes {close}\n")
    for line in sorted(bad):
        sys.stderr.write(line + "\n")
    return 1 if bad else 0


def load_approvals(path: Path) -> list:
    raw = read_bounded(path, MAX_APPROVALS_BYTES, "RFC-INPUT", "approvals file")
    try:
        data = json.loads(decode(raw, "RFC-INPUT", f"approvals file {path}"))
    except json.JSONDecodeError as e:
        raise RfcError("RFC-INPUT", f"approvals file {path} is not valid JSON: {e.msg} at line {e.lineno}") from None
    except (ValueError, RecursionError):
        raise RfcError("RFC-INPUT", f"approvals file {path} is not valid JSON or is nested too deeply") from None
    if not isinstance(data, list):
        raise RfcError("RFC-INPUT", "approvals file must be a JSON list of {actor, state, at} objects")
    out = []
    for i, item in enumerate(data):
        if not isinstance(item, dict) or not all(isinstance(item.get(k), str) for k in ("actor", "state", "at")):
            raise RfcError("RFC-INPUT", f"approvals entry {i} must be an object with string actor, state and at")
        actor = item["actor"].strip().casefold()
        if not actor:
            raise RfcError("RFC-INPUT", f"approvals entry {i} has an empty actor")
        out.append((parse_time(item["at"], f"approvals entry {i} at"), i, actor, item["state"].strip().upper()))
    return out


def cmd_approve_check(args, out) -> int:
    record_id = check_id(args.record)
    now = parse_time(args.now, "--now")
    if not args.approvals:
        raise RfcError("RFC-INPUT", "approve-check needs --approvals <exported reviews JSON>")
    cfg = load_config(Path(args.config))
    path = find_record(Path(args.dir), record_id)
    _, meta = read_record_for(path, record_id)
    period = recorded_period(meta, path.name)
    if period is None:
        raise RfcError("RFC-PERIOD-OPEN", f"{record_id} has no comment period recorded; run start first")
    _, _, close = period
    if now < close:
        raise RfcError("RFC-PERIOD-OPEN", f"{record_id} comment period closes {fmt_time(close)}")
    author = scalar(meta, "raised_by", path.name).strip().casefold()
    if not author:
        raise RfcError("RFC-INPUT", f"{record_id} has no raised_by author")
    reviews = load_approvals(Path(args.approvals))
    # Each actor's latest review (by time, then file order) up to --now decides that actor's vote;
    # a review submitted before the period closed does not count as approval.
    latest: dict = {}
    for at, i, actor, state in sorted(reviews):
        if at <= now:
            latest[actor] = (at, state)
    approvers = sorted(a for a, (at, state) in latest.items() if state == "APPROVED" and at >= close)

    def is_author(actor: str) -> bool:
        return actor == author or author in cfg["aliases"].get(actor, set())

    valid = [a for a in approvers if a in cfg["maintainers"] and not is_author(a)]
    if not valid:
        raise RfcError("RFC-SELF-APPROVAL",
                       f"{record_id}: no maintainer other than the author ({author}) approved after "
                       f"{fmt_time(close)}")
    out.write(f"{record_id} approvable: approved by {', '.join(valid)}\n")
    return 0


# ---------------------------------------------------------------- main


class _Parser(argparse.ArgumentParser):
    def error(self, message: str):  # usage errors: exit 2 with a one-line message, no traceback
        self.print_usage(sys.stderr)
        sys.stderr.write(f"rfc.py: error: {message}\n")
        raise SystemExit(2)


def build_parser() -> argparse.ArgumentParser:
    ap = _Parser(prog="rfc.py", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True, parser_class=_Parser)

    def common(p):
        p.add_argument("--dir", default=str(DEFAULT_DIR), help="decision record directory (default decisions/)")
        p.add_argument("--now", required=True, help="current UTC time, e.g. 2026-12-01T09:00:00Z")
        p.add_argument("--config", default=str(DEFAULT_CONFIG), help="RFC config JSON")

    s = sub.add_parser("start", help="record the start of a record's comment period")
    s.add_argument("record")
    s.add_argument("--kind", help="rfc_kind to record when the record has none (default other)")
    common(s)
    s.set_defaults(fn=cmd_start)
    st = sub.add_parser("status", help="list records with an open or closed comment period")
    common(st)
    st.set_defaults(fn=cmd_status)
    a = sub.add_parser("approve-check", help="validate an exported approval set")
    a.add_argument("record")
    a.add_argument("--approvals", help="JSON list of {actor, state, at} exported from the forge")
    common(a)
    a.set_defaults(fn=cmd_approve_check)
    return ap


def main(argv=None, out=None) -> int:
    out = out if out is not None else sys.stdout
    args = build_parser().parse_args(argv)
    try:
        return args.fn(args, out)
    except RfcError as e:
        sys.stderr.write(f"{e.code}: {e.message}\n")
        return 1


if __name__ == "__main__":
    sys.exit(main())
