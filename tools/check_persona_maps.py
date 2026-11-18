#!/usr/bin/env python3
"""Check the persona journey maps of spec node FLOW.PERSONA.MAPS.

Reads every `*.json` file under `acceptance/journeys/persona/` (one `journey-map@1` map per
persona, `kind: persona`), the persona file `acceptance/personas/personas.json` (FLOW.PERSONA.PERS)
and the spec tree, and reports findings one per line as `CODE SUBJECT detail`, sorted.

This is an interim, stdlib-only check. The real one is `fid accept check-journeys`
(FLOW.PERSONA.CHECK, with the structural validator of FLOW.PERSONA.SCHEMA), which is not built yet.
It applies the same rules where those nodes define them and uses their finding codes:

  structural (FLOW.PERSONA.SCHEMA)  MAP-SIZE, MAP-SCHEMA, MAP-STEP-ORDER, MAP-AGENT-HUMANONLY
  persona maps (FLOW.PERSONA.MAPS)  MAP-MISSING, MAP-DUPLICATE, MAP-EXTRA, MAP-UNEXERCISED,
                                    MAP-HANDOFF, MAP-AUDITOR, MAP-HUMANONLY-MISSING
  spec tree (FLOW.PERSONA.CHECK)    MAP-OWNER-UNKNOWN, MAP-COMMAND-UNKNOWN, MAP-SCREEN-UNKNOWN,
                                    MAP-PHASE

Usage: python tools/check_persona_maps.py [--maps DIR] [--personas FILE] [--spec DIR] [--json]
Exit 0 and `persona-maps-check: N maps` when there is no finding; 1 with findings; 2 on usage errors.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

MAX_BYTES = 1024 * 1024
MAX_DEPTH = 32
MAX_STEPS = 500

FORMAT = "journey-map@1"
KINDS = {"persona", "job", "setup"}
ACTORS = {"human", "operator", "admin", "agent", "system"}
PHASES = ["P0", "P1a", "P1b", "P2", "P3", "P4", "later"]
# Personas that have no acceptance job of their own (REQ-FLOW.PERSONA.MAPS-002).
UNEXERCISED_OK = {"JOB.PER.06", "JOB.PER.08"}
AUDITOR = "JOB.PER.06"
# Classes whose commands are human-only (CONSTITUTION §3: W3 release, S1 signing, A0 admin, M1 motion).
HUMAN_CLASSES = {"W3", "S1", "A0", "M1"}

MAP_KEYS = {"format", "map_id", "subject", "kind", "revision", "app", "phase", "personas", "steps",
            "exercised_by"}
MAP_REQUIRED = MAP_KEYS - {"exercised_by"}
STEP_KEYS = {"n", "actor", "command", "screen", "owner", "handoff", "decision", "human_only", "phase"}
STEP_REQUIRED = {"n", "actor", "owner", "handoff", "human_only"}

COMMAND_RE = re.compile(r"^[a-z][a-z0-9]*\.[a-z][a-z0-9_]*@[1-9][0-9]*$")
SCREEN_RE = re.compile(r"^SCR\.[A-Z][A-Z0-9]*\.[A-Z][A-Z0-9_]*$")
NODE_RE = re.compile(r"^[A-Z][A-Z0-9]*(\.[A-Z][A-Z0-9]{1,7})*$")
PER_RE = re.compile(r"^JOB\.PER\.[0-9]{2}$")
ACC_RE = re.compile(r"^JOB\.ACC\.[0-9]{2}$")
SEED_RE = re.compile(r"^JOB\.[A-Z]+\.[0-9]{2,3}$")
PER_IN_TEXT_RE = re.compile(r"JOB\.PER\.[0-9]{2}")
SCREEN_IN_TEXT_RE = re.compile(r"SCR\.[A-Z][A-Z0-9]*\.[A-Z][A-Z0-9_]*")


class Finding:
    def __init__(self, code: str, subject: str, detail: str) -> None:
        self.code, self.subject, self.detail = code, subject, detail

    def key(self):
        return (self.code, self.subject, self.detail)

    def line(self) -> str:
        return f"{self.code} {self.subject} {self.detail}".rstrip()


# ---------------------------------------------------------------------------------------------
# Spec tree index
# ---------------------------------------------------------------------------------------------

class SpecNode:
    def __init__(self, nid: str, phase: str, body: str, commands: dict[str, str | None],
                 names: dict[str, str | None]) -> None:
        self.id = nid
        self.phase = phase
        self.body = body
        self.commands = commands  # full command ID (from journal:) -> class or None
        self.names = names  # command name without @major (from cli:) -> class or None


def _front(text: str):
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        return None, ""
    meta: dict = {}
    for i, line in enumerate(lines[1:], start=1):
        if line.strip() == "---":
            return meta, "\n".join(lines[i + 1:])
        if ":" not in line or line.lstrip().startswith("#"):
            continue
        key, val = line.split(":", 1)
        key, val = key.strip(), val.strip()
        if val.startswith("[") and val.endswith("]"):
            inner = val[1:-1].strip()
            meta[key] = [x.strip() for x in inner.split(",") if x.strip()] if inner else []
        else:
            meta[key] = val
    return None, ""


def _cli_name(cli: str) -> str | None:
    parts = cli.split()
    if len(parts) >= 3 and parts[0] == "fid" and "*" not in cli:
        return parts[1] + "." + parts[2].replace("-", "_")
    return None


def load_spec(spec_dir: Path) -> dict[str, SpecNode]:
    key = str(spec_dir.resolve())
    if key not in _SPEC_CACHE:
        _SPEC_CACHE[key] = _load_spec(spec_dir)
    return _SPEC_CACHE[key]


_SPEC_CACHE: dict[str, dict[str, SpecNode]] = {}


def _load_spec(spec_dir: Path) -> dict[str, SpecNode]:
    nodes: dict[str, SpecNode] = {}
    for path in sorted(spec_dir.rglob("NODE.md")):
        meta, body = _front(path.read_text(encoding="utf-8"))
        if not meta or not meta.get("id"):
            continue
        cli = meta.get("cli") if isinstance(meta.get("cli"), list) else []
        journal = meta.get("journal") if isinstance(meta.get("journal"), list) else []
        se = meta.get("side_effect", [])
        classes = se if isinstance(se, list) else ([se] if se else [])
        names: dict[str, str | None] = {}
        for i, c in enumerate(cli):
            name = _cli_name(c)
            if name is None:
                continue
            if len(classes) == len(cli):
                names[name] = classes[i]
            elif len(classes) == 1:
                names[name] = classes[0]
            else:
                names[name] = None
        commands: dict[str, str | None] = {}
        for j in journal:
            if "*" in j or not COMMAND_RE.match(j):
                continue
            base = j.split("@", 1)[0]
            if base in names:
                commands[j] = names[base]
            elif len(classes) == 1:
                commands[j] = classes[0]
            else:
                commands[j] = None
        nodes[meta["id"]] = SpecNode(meta["id"], str(meta.get("phase", "")), body, commands, names)
    return nodes


def command_in_node(node: SpecNode, command: str) -> tuple[bool, str | None]:
    if command in node.commands:
        return True, node.commands[command]
    base, major = command.split("@", 1)
    # A command named only in `cli:` (no journal entry, e.g. an R0 read) is taken as major 1.
    if base in node.names and major == "1" and not any(c.startswith(base + "@") for c in node.commands):
        return True, node.names[base]
    return False, None


# ---------------------------------------------------------------------------------------------
# Bounded JSON parse
# ---------------------------------------------------------------------------------------------

class DuplicateKey(ValueError):
    pass


def _no_dupes(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise DuplicateKey(k)
        out[k] = v
    return out


def _depth(v, d=1) -> int:
    if isinstance(v, dict):
        return max([d] + [_depth(x, d + 1) for x in v.values()])
    if isinstance(v, list):
        return max([d] + [_depth(x, d + 1) for x in v])
    return d


def _parse(raw: bytes):
    """Returns (value, error code, detail)."""
    if len(raw) > MAX_BYTES:
        return None, "MAP-SIZE", f"{len(raw)} bytes > {MAX_BYTES}"
    # Reject very deep nesting before json recursion.
    depth = level = 0
    in_str = esc = False
    for ch in raw.decode("utf-8", errors="replace"):
        if in_str:
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == '"':
                in_str = False
        elif ch == '"':
            in_str = True
        elif ch in "[{":
            level += 1
            depth = max(depth, level)
        elif ch in "]}":
            level -= 1
    if depth > MAX_DEPTH:
        return None, "MAP-SIZE", f"nesting {depth} > {MAX_DEPTH}"
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_no_dupes)
    except DuplicateKey as e:
        return None, "MAP-SCHEMA", f"duplicate key {e}"
    except (UnicodeDecodeError, ValueError) as e:
        return None, "MAP-SCHEMA", f"not JSON: {e}"
    if _depth(value) > MAX_DEPTH:
        return None, "MAP-SIZE", f"nesting > {MAX_DEPTH}"
    return value, None, ""


# ---------------------------------------------------------------------------------------------
# Structural validation (journey-map@1, persona kind)
# ---------------------------------------------------------------------------------------------

def _is_int(v) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def validate_structure(m, name: str) -> list[Finding]:
    """Schema, size and numbering rules. `name` is the file name, used as subject when the map
    has no usable subject. Returns findings; the map is usable only when none is returned."""
    f: list[Finding] = []
    subj = m.get("subject") if isinstance(m, dict) and isinstance(m.get("subject"), str) else name

    def bad(detail: str) -> None:
        f.append(Finding("MAP-SCHEMA", subj, detail))

    if not isinstance(m, dict):
        bad("map is not an object")
        return f
    for k in sorted(set(m) - MAP_KEYS):
        bad(f"unknown field {k}")
    for k in sorted(MAP_REQUIRED - set(m)):
        bad(f"missing field {k}")
    if f:
        return f
    if m["format"] != FORMAT:
        bad(f"format must be {FORMAT}")
    if not isinstance(m["map_id"], str) or not re.match(r"^[a-z0-9][a-z0-9._-]{0,63}$", m["map_id"]):
        bad("map_id must be a short lowercase ID")
    if not isinstance(m["subject"], str) or not SEED_RE.match(m["subject"]):
        bad("subject must be a JOB.* seed ID")
    if m["kind"] not in KINDS:
        bad("kind must be persona, job or setup")
    if not _is_int(m["revision"]) or m["revision"] < 1:
        bad("revision must be an integer >= 1")
    if not isinstance(m["app"], str) or not m["app"]:
        bad("app must be a non-empty string")
    if m["phase"] not in PHASES:
        bad("phase must be one of " + " ".join(PHASES))
    pers = m["personas"]
    if not isinstance(pers, list) or not pers or not all(isinstance(p, str) and PER_RE.match(p) for p in pers):
        bad("personas must be a non-empty list of JOB.PER.* IDs")
    elif len(set(pers)) != len(pers):
        bad("personas lists an ID twice")
    if m["kind"] == "persona":
        ex = m.get("exercised_by")
        if not isinstance(ex, list) or not all(isinstance(x, str) and ACC_RE.match(x) for x in ex):
            bad("exercised_by must be a list of JOB.ACC.* IDs")
        elif len(set(ex)) != len(ex):
            bad("exercised_by lists an ID twice")
    steps = m["steps"]
    if not isinstance(steps, list) or not steps:
        bad("steps must be a non-empty list")
        return f
    if len(steps) > MAX_STEPS:
        f.append(Finding("MAP-SIZE", subj, f"{len(steps)} steps > {MAX_STEPS}"))
        return f
    for i, s in enumerate(steps, start=1):
        at = f"step {s.get('n', '?') if isinstance(s, dict) else '?'}"
        if not isinstance(s, dict):
            bad(f"step at index {i} is not an object")
            continue
        for k in sorted(set(s) - STEP_KEYS):
            bad(f"{at}: unknown field {k}")
        for k in sorted(STEP_REQUIRED - set(s)):
            bad(f"{at}: missing field {k}")
        if ("command" in s) == ("screen" in s):
            bad(f"{at}: needs exactly one of command or screen")
        if "command" in s and not (isinstance(s["command"], str) and COMMAND_RE.match(s["command"])):
            bad(f"{at}: command must be domain.verb_object@major")
        if "screen" in s and not (isinstance(s["screen"], str) and SCREEN_RE.match(s["screen"])):
            bad(f"{at}: screen must be SCR.<APP>.<NAME>")
        if "n" in s and not _is_int(s["n"]):
            bad(f"{at}: n must be an integer")
        if "actor" in s and s["actor"] not in ACTORS:
            bad(f"{at}: actor must be one of " + " ".join(sorted(ACTORS)))
        if "owner" in s and not (isinstance(s["owner"], str) and NODE_RE.match(s["owner"])):
            bad(f"{at}: owner must be a spec node ID")
        if "handoff" in s:
            h = s["handoff"]
            if not ((isinstance(h, str) and h.strip()) or _is_int(h)):
                bad(f"{at}: handoff must be text or a step number")
        if "decision" in s and not (isinstance(s["decision"], str) and s["decision"].strip()):
            bad(f"{at}: decision must be non-empty text")
        if "human_only" in s and not isinstance(s["human_only"], bool):
            bad(f"{at}: human_only must be a boolean")
        if "phase" in s and s["phase"] not in PHASES:
            bad(f"{at}: phase must be one of " + " ".join(PHASES))
    if f:
        return f
    ns = [s["n"] for s in steps]
    if ns != list(range(1, len(steps) + 1)):
        f.append(Finding("MAP-STEP-ORDER", subj, "steps must be numbered 1..N in order"))
        return f
    for s in steps:
        h = s["handoff"]
        if _is_int(h) and not (s["n"] < h <= len(steps)):
            bad(f"step {s['n']}: handoff {h} is not a later step")
    for s in steps:
        if s["human_only"] and s["actor"] == "agent":
            f.append(Finding("MAP-AGENT-HUMANONLY", subj, f"step {s['n']}"))
    return f


# ---------------------------------------------------------------------------------------------
# Whole check
# ---------------------------------------------------------------------------------------------

def load_personas(path: Path) -> list[str]:
    data = json.loads(path.read_text(encoding="utf-8"))
    return [p["id"] for p in data["personas"]]


def check(maps_dir: Path, personas_file: Path, spec_dir: Path):
    """Returns (number of maps read, sorted findings)."""
    findings: list[Finding] = []
    try:
        persona_ids = load_personas(personas_file)
    except (OSError, ValueError, KeyError, TypeError) as e:
        return 0, [Finding("MAP-SCHEMA", "-", f"cannot read persona file: {e}")]
    known = set(persona_ids)
    spec = load_spec(spec_dir)
    if not spec:
        return 0, [Finding("MAP-SCHEMA", "-", f"no spec nodes under {spec_dir}")]

    files = sorted(maps_dir.glob("*.json")) if maps_dir.is_dir() else []
    by_subject: dict[str, str] = {}
    count = 0
    for path in files:
        raw = path.read_bytes()
        m, code, detail = _parse(raw)
        if code:
            findings.append(Finding(code, path.name, detail))
            continue
        count += 1
        structural = validate_structure(m, path.name)
        findings.extend(structural)
        if any(x.code != "MAP-AGENT-HUMANONLY" for x in structural):
            # An invalid map still counts as present for its persona, so it is not also MAP-MISSING.
            subj = m.get("subject") if isinstance(m, dict) else None
            if isinstance(subj, str) and subj in known and subj not in by_subject:
                by_subject[subj] = path.name
            continue
        subj = m["subject"]
        if m["kind"] != "persona":
            findings.append(Finding("MAP-SCHEMA", subj, "kind must be persona"))
            continue
        if subj not in known:
            findings.append(Finding("MAP-EXTRA", subj, f"{path.name}: no such persona"))
            continue
        if subj in by_subject:
            findings.append(Finding("MAP-DUPLICATE", subj, f"{by_subject[subj]} and {path.name}"))
            continue
        by_subject[subj] = path.name
        if subj not in m["personas"]:
            findings.append(Finding("MAP-SCHEMA", subj, "personas must include the subject"))
        for p in m["personas"]:
            if p not in known:
                findings.append(Finding("MAP-HANDOFF", subj, f"personas names unknown {p}"))
        if not m["exercised_by"] and subj not in UNEXERCISED_OK:
            findings.append(Finding("MAP-UNEXERCISED", subj, ""))
        findings.extend(_check_steps(m, spec, known))

    for pid in persona_ids:
        if pid not in by_subject:
            findings.append(Finding("MAP-MISSING", pid, ""))
    findings = sorted({x.key(): x for x in findings}.values(), key=lambda x: x.key())
    return count, findings


def _check_steps(m, spec: dict[str, SpecNode], known: set[str]) -> list[Finding]:
    f: list[Finding] = []
    subj = m["subject"]
    order = {p: i for i, p in enumerate(PHASES)}
    for s in m["steps"]:
        at = f"step {s['n']}"
        h = s["handoff"]
        if isinstance(h, str):
            for p in PER_IN_TEXT_RE.findall(h):
                if p not in known:
                    f.append(Finding("MAP-HANDOFF", subj, f"{at}: unknown persona {p}"))
                elif p not in m["personas"]:
                    f.append(Finding("MAP-HANDOFF", subj, f"{at}: {p} is not in personas"))
        owner = spec.get(s["owner"])
        if owner is None:
            f.append(Finding("MAP-OWNER-UNKNOWN", subj, f"{at}: {s['owner']}"))
            continue
        cls = None
        if "command" in s:
            cmd = s["command"]
            found, cls = command_in_node(owner, cmd)
            if not found:
                f.append(Finding("MAP-COMMAND-UNKNOWN", subj, f"{at}: {cmd} is not a command of {owner.id}"))
            human = cls in HUMAN_CLASSES or ".adopt_" in cmd
            if human and not s["human_only"]:
                f.append(Finding("MAP-HUMANONLY-MISSING", subj, f"{at}: {cmd} ({cls or 'adoption'})"))
            if human and s["actor"] == "agent":
                f.append(Finding("MAP-AGENT-HUMANONLY", subj, f"{at}"))
            if subj == AUDITOR and (not cmd.startswith("verify.") or cls == "X1"):
                f.append(Finding("MAP-AUDITOR", subj, f"{at}: {cmd}"))
        else:
            scr = s["screen"]
            if scr not in SCREEN_IN_TEXT_RE.findall(owner.body):
                f.append(Finding("MAP-SCREEN-UNKNOWN", subj, f"{at}: {scr} is not named by {owner.id}"))
            if subj == AUDITOR:
                f.append(Finding("MAP-AUDITOR", subj, f"{at}: screen {scr}"))
        eff = s.get("phase", m["phase"])
        if owner.phase in order and order[owner.phase] > order[eff]:
            f.append(Finding("MAP-PHASE", subj, f"{at}: {owner.id} starts in {owner.phase}, step is {eff}"))
    return f


def main(argv: list[str]) -> int:
    maps_dir = ROOT / "acceptance" / "journeys" / "persona"
    personas = ROOT / "acceptance" / "personas" / "personas.json"
    spec_dir = ROOT / "spec"
    as_json = False
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--json":
            as_json = True
            i += 1
        elif a in ("--maps", "--personas", "--spec") and i + 1 < len(argv):
            p = Path(argv[i + 1])
            if a == "--maps":
                maps_dir = p
            elif a == "--personas":
                personas = p
            else:
                spec_dir = p
            i += 2
        else:
            print("usage: check_persona_maps.py [--maps DIR] [--personas FILE] [--spec DIR] [--json]",
                  file=sys.stderr)
            return 2
    count, findings = check(maps_dir, personas, spec_dir)
    if as_json:
        print(json.dumps({"maps": count, "findings": [
            {"code": x.code, "subject": x.subject, "detail": x.detail} for x in findings]}, indent=2))
    else:
        for x in findings:
            print(x.line())
        if findings:
            print(f"persona-maps-check: {len(findings)} findings")
        else:
            print(f"persona-maps-check: {count} maps")
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
