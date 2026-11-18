#!/usr/bin/env python3
"""Corpus checks of AGENT.EVAL.CORPUS that `fid-eval suite check` does not make (stdlib only).

Given a corpus suite directory (default `suite/`) with its `TRACE.json`:

- trace (REQ-002, AC-2): every golden journal cited in TRACE.json has the recorded digest and
  an intact entry chain (each entry hash recomputed from RFC 8785 canonical JSON); every task
  names the golden digest of its source job; every oracle item has exactly one trace row, the
  cited entry lies in the task's step range and holds a ResultRecord for that characteristic
  whose outcome, evaluation basis, value bits and unit equal the item's; and the oracle holds
  every ResultRecord at the range end (the last one per characteristic), no more and no less.
- formula (REQ-004): an item whose record is formula-derived must cite an
  `expr.evaluate_formula@1` entry, require the record's evaluation basis, and be flagged in the
  trace; a flagged row must cite such an entry.
- licences (REQ-003, AC-3): every licence is `project-authored` or on the project allowlist
  (`[policy] redistributable` of datasets.toml), and the licence entries list exactly the files
  in the start-state archive. A failure names the task and the file.
- coverage (REQ-001, AC-4): every app with golden journals has tasks in all three tiers, and
  every workflow of agent/catalog.json whose app has golden journals is exercised by a task.

Exit 0 when all checks pass, 1 otherwise. Usage: python tools/check_eval_corpus.py [SUITE_DIR]
"""

import hashlib
import json
import struct
import sys
import tarfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TIERS = ("single_command", "multi_step", "full_job")
FORMULA_COMMAND = "expr.evaluate_formula@1"


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def canonical(v) -> bytes:
    """RFC 8785 for JSON without non-integer numbers (golden entries carry amounts as bit
    strings). Keys are sorted by UTF-16 code units; strings use the JSON short escapes."""
    def enc(x):
        if x is None:
            return "null"
        if x is True:
            return "true"
        if x is False:
            return "false"
        if isinstance(x, int):
            return str(x)
        if isinstance(x, float):
            if x.is_integer() and abs(x) < 2 ** 53:
                return str(int(x))
            raise ValueError("non-integer number in a golden entry")
        if isinstance(x, str):
            return json.dumps(x, ensure_ascii=False)
        if isinstance(x, list):
            return "[" + ",".join(enc(i) for i in x) + "]"
        if isinstance(x, dict):
            keys = sorted(x, key=lambda k: k.encode("utf-16-be"))
            return "{" + ",".join(json.dumps(k, ensure_ascii=False) + ":" + enc(x[k]) for k in keys) + "}"
        raise ValueError(f"unsupported JSON value {x!r}")
    return enc(v).encode("utf-8")


def amount_bits(a):
    """The IEEE 754 bits of an oracle or record amount (number or '0x' + 16 hex digits)."""
    if isinstance(a, str):
        return a.lower()
    return "0x" + struct.pack(">d", float(a)).hex()


def allowlist(root: Path) -> set:
    with open(root / "datasets.toml", "rb") as f:
        policy = tomllib.load(f).get("policy", {})
    return set(policy.get("redistributable", [])) | {"project-authored"}


def tar_members(path: Path) -> list:
    with tarfile.open(path, mode="r:") as tf:
        return sorted(m.name for m in tf.getmembers() if m.isfile())


def load_json(p: Path):
    return json.loads(p.read_text(encoding="utf-8"))


def check(suite: Path, root: Path = ROOT) -> list:
    errs = []
    trace = load_json(suite / "TRACE.json")
    allowed = allowlist(root)

    # Golden journals: digest and entry chain.
    golden = {}
    for g in trace["golden"]:
        data = (root / g["path"]).read_bytes()
        if sha(data) != g["digest"]:
            errs.append(f"golden {g['job']}: {g['path']} has digest {sha(data)}, TRACE.json says {g['digest']}")
        doc = json.loads(data)
        prev = None
        entries = {}
        for e in doc["entries"]:
            body = {k: v for k, v in e.items() if k != "hash"}
            if sha(canonical(body)) != e["hash"]:
                errs.append(f"golden {g['job']}: entry {e['seq']} hash does not match its content")
            if e.get("prev") != prev:
                errs.append(f"golden {g['job']}: entry {e['seq']} breaks the hash chain")
            prev = e["hash"]
            entries[e["seq"]] = e
        golden[g["job"]] = {"digest": g["digest"], "app": doc.get("app", g.get("app")), "entries": entries}

    rows = {t["task"]: t for t in trace["tasks"]}
    task_dirs = sorted(p.name for p in (suite / "tasks").iterdir() if p.is_dir())
    for tid in sorted(set(rows) - set(task_dirs)):
        errs.append(f"TRACE.json names task {tid}, which is not in the suite")
    by_app = {}
    exercised = set()
    for tid in task_dirs:
        task = load_json(suite / "tasks" / tid / "task.json")
        oracle = load_json(suite / "oracles" / tid / "oracle.json")
        for a in task["apps"]:
            by_app.setdefault(a, set()).add(task["tier"])
        exercised.update(task["workflows"])

        # Licences (REQ-003, AC-3).
        lic_files = []
        for lic in task["licences"]:
            lic_files.append(lic["file"])
            if lic["spdx"] not in allowed:
                errs.append(f"task {tid}: input file {lic['file']} has licence {lic['spdx']!r}, which is not "
                            "project-authored or on the licence allowlist (datasets.toml [policy] redistributable)")
        members = tar_members(suite / "tasks" / tid / task["start_state"]["path"])
        for f in sorted(set(members) - set(lic_files)):
            errs.append(f"task {tid}: input file {f} has no licence entry")
        for f in sorted(set(lic_files) - set(members)):
            errs.append(f"task {tid}: licence entry for {f}, which is not in the start-state archive")

        # Trace (REQ-002, AC-2) and formula rule (REQ-004).
        row = rows.get(tid)
        if row is None:
            errs.append(f"task {tid}: no row in TRACE.json")
            continue
        g = golden.get(task["source_job"])
        if g is None or row["job"] != task["source_job"]:
            errs.append(f"task {tid}: source job {task['source_job']} has no golden journal in TRACE.json")
            continue
        if task["golden_digest"] != g["digest"]:
            errs.append(f"task {tid}: golden_digest differs from the digest of the {task['source_job']} journal")
        first, last = row["first_seq"], row["last_seq"]
        at_end = {}
        for seq in sorted(g["entries"]):
            if first <= seq <= last:
                for r in g["entries"][seq]["result_records"]:
                    at_end[r["characteristic"]] = (seq, r)
        traced = {}
        for t_item in row["items"]:
            if t_item["characteristic"] in traced:
                errs.append(f"task {tid}: {t_item['characteristic']} traced twice")
            traced[t_item["characteristic"]] = t_item
        items = {it["characteristic"]: it for it in oracle["items"]}
        for ch in sorted(set(at_end) - set(items)):
            errs.append(f"task {tid}: ResultRecord {ch} at the range end is missing from the oracle")
        for ch, it in sorted(items.items()):
            t_item = traced.get(ch)
            if t_item is None:
                errs.append(f"task {tid}: oracle item {ch} has no trace row")
                continue
            e = g["entries"].get(t_item["entry_seq"])
            if e is None or e["hash"] != t_item["entry_hash"]:
                errs.append(f"task {tid}: {ch} cites entry {t_item['entry_seq']} {t_item['entry_hash']}, "
                            "which is not in the golden journal")
                continue
            if not first <= e["seq"] <= last:
                errs.append(f"task {tid}: {ch} cites entry {e['seq']} outside steps {first}-{last}")
            recs = [r for r in e["result_records"] if r["characteristic"] == ch]
            if len(recs) != 1:
                errs.append(f"task {tid}: entry {e['seq']} holds {len(recs)} ResultRecords for {ch}")
                continue
            r = recs[0]
            if at_end.get(ch, (None,))[0] != e["seq"]:
                errs.append(f"task {tid}: {ch} cites entry {e['seq']}, not the last record in the range")
            if it["outcome"] != r["decision"]["outcome"]:
                errs.append(f"task {tid}: {ch} outcome {it['outcome']} != golden {r['decision']['outcome']}")
            if it["basis"] != r["evaluation_basis"]:
                errs.append(f"task {tid}: {ch} basis {it['basis']} != golden {r['evaluation_basis']}")
            rv, iv = r["value"], it.get("value")
            if (rv is None) != (iv is None):
                errs.append(f"task {tid}: {ch} value presence differs from the golden record")
            elif rv is not None:
                if amount_bits(iv["amount"]) != amount_bits(rv["amount"]):
                    errs.append(f"task {tid}: {ch} value {iv['amount']} != golden {rv['amount']}")
                if iv["unit"] != rv["unit"]:
                    errs.append(f"task {tid}: {ch} unit {iv['unit']} != golden {rv['unit']}")
            derived = r.get("derivation") == "formula"
            if derived and e["command"] != FORMULA_COMMAND:
                errs.append(f"task {tid}: formula-derived {ch} comes from {e['command']}, not {FORMULA_COMMAND}")
            if bool(t_item.get("formula")) != derived:
                errs.append(f"task {tid}: {ch} formula flag in TRACE.json does not match the golden record")
            if t_item.get("formula") and e["command"] != FORMULA_COMMAND:
                errs.append(f"task {tid}: {ch} is flagged formula-derived but cites {e['command']}")
        for ch in sorted(set(traced) - set(items)):
            errs.append(f"task {tid}: trace row {ch} has no oracle item")

    # Coverage (REQ-001, AC-4).
    apps = sorted({g["app"] for g in golden.values()})
    for a in apps:
        for tier in TIERS:
            if tier not in by_app.get(a, set()):
                errs.append(f"app {a} has golden journals but no {tier} task")
    cat_path = root / "agent" / "catalog.json"
    if cat_path.is_file():
        for w in load_json(cat_path).get("workflows", []):
            wid = w.get("id") if isinstance(w, dict) else w
            app = (w.get("app") if isinstance(w, dict) else None) or str(wid).split(".")[0]
            if app in apps and wid not in exercised:
                errs.append(f"curated workflow {wid} (app {app}) is exercised by no task")
    return errs


def main(argv) -> int:
    suite = Path(argv[1]) if len(argv) > 1 else ROOT / "suite"
    errs = check(suite)
    for e in errs:
        print("FAIL", e)
    print("eval corpus check:", "failed" if errs else "ok")
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
