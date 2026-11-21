#!/usr/bin/env python3
"""Generate the fixtures of spec node AUTO.RUN.BIND.

Independent reference (Python standard library only): nothing here runs the Rust code.

Writes:

- `fixtures/scans/bracket-120.ply`: a synthetic ASCII PLY scan of the B-120 bracket (a flat
  plate with two mounting holes 120 mm apart), from a seeded linear congruential generator.
  Project-authored; its only role is to be hashed.
- `fixtures/auto/run/bind/expected.json`: the program digest (hashlib over the bytes of
  `fixtures/program/family/bracket.json`, which is canonical), the scan's SHA-256, the run IDs
  the test's run-ID source mints, and the canonical run records of AC-1, AC-2 and AC-3,
  hand-labelled from spec/auto/run/bind/NODE.md (REQ-001..005) and the record layout pinned in
  crates/fid-run/src/record.rs.
- `fixtures/auto/run/bind/forged.json`: the forged-event oracle cases of
  REQ-CORE.RPL.RECON.FOLD-008 (g) for the run ledger, items (a) to (e) plus the record policy,
  each with its verdict hand-labelled from FOLD-008 (a)-(g) and REQ-AUTO.RUN.BIND-001/002; a
  plain reading of those rules (`cross_check`) only guards against a mislabelled case. Each
  case's full entry record, parameters commitment included (SHA-256 of the RFC 8785 bytes of
  the program.run@1 parameters), is written out.

Canonical JSON: every string here is ASCII and every number a small integer, so json.dumps with
sorted keys and no whitespace is the RFC 8785 form.

CC-BY-4.0 project data. Run:  python fixtures/auto/run/bind/make_cases.py [--check]
"""

import copy
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent.parent
PROGRAM_FILE = ROOT / "fixtures/program/family/bracket.json"
SCAN_FILE = ROOT / "fixtures/scans/bracket-120.ply"
SCAN_HANDLE = "input:scans/bracket-120.ply"

PROGRAM_ID = "018f0000-0000-7000-8000-000000000100"
AGENT = {"id": "agent-1", "kind": "agent"}
HUMAN = {"id": "alice", "kind": "human"}
RUN_STARTED = "RunStarted"
RUN_COMMAND = "program.run"
PARAMS_COMMITMENT = "sha256:" + "ab" * 32
OTHER_COMMITMENT = "sha256:" + "cd" * 32
PROMOTED_FROM = "sha256:" + "ef" * 32
OTHER_DIGEST = "sha256:" + "12" * 32

RECORD_KEYS = sorted([
    "binding", "branch", "inputs", "lifecycle_state", "outcome", "override_deprecated", "principal",
    "program_digest", "program_id", "program_version", "run_id", "trial", "variant",
])
B120 = {"hole_spacing": {"unit": "mm", "value": "120"}, "scan": SCAN_HANDLE}


def canon(v):
    out = json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    assert out.isascii(), "fixture strings must be ASCII"
    return out


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


# ------------------------------------------------------------------ scan


def scan_bytes():
    """A 40 x 12 grid over a 160 mm x 48 mm plate, minus two holes of radius 6 mm centred
    120 mm apart, with LCG noise of at most +-0.02 mm on z. Coordinates in mm, 4 decimals."""
    state = 120  # seed

    def lcg():
        nonlocal state
        state = (state * 6364136223846793005 + 1442695040888963407) % (1 << 64)
        return state >> 11  # 53 bits

    pts = []
    for j in range(12):
        for i in range(40):
            x = -80 + i * 4 + 2
            y = -24 + j * 4 + 2
            if (x + 60) ** 2 + y ** 2 < 36 or (x - 60) ** 2 + y ** 2 < 36:
                continue
            z_um = (lcg() % 41) - 20  # micrometres, -20..20
            pts.append((x, y, z_um))
    lines = [
        "ply",
        "format ascii 1.0",
        "comment Metrology Maven synthetic fixture: bracket B-120 (AUTO.RUN.BIND), seed 120",
        "element vertex %d" % len(pts),
        "property float x",
        "property float y",
        "property float z",
        "end_header",
    ]
    for x, y, z in pts:
        sign = "-" if z < 0 else ""
        lines.append("%d.0000 %d.0000 %s0.%04d" % (x, y, sign, abs(z) * 10))
    return ("\n".join(lines) + "\n").encode("ascii")


# ------------------------------------------------------------------ run IDs


def uuid7(unix_ms, rand):
    """Uuid7::from_parts: 48-bit big-endian ms, then the ten random bytes with the version
    nibble set to 7 and the variant bits to 10 (RFC 9562 section 5.7)."""
    b = bytearray(unix_ms.to_bytes(8, "big")[2:] + bytes(rand))
    b[6] = 0x70 | (b[6] & 0x0F)
    b[8] = 0x80 | (b[8] & 0x3F)
    h = b.hex()
    return "%s-%s-%s-%s-%s" % (h[0:8], h[8:12], h[12:16], h[16:20], h[20:32])


def run_id(n):
    """The n-th ID (from 1) of the tests' counting run-ID source."""
    return uuid7(1_763_000_000_000 + n, [n] * 10)


# ------------------------------------------------------------------ records


def record(program_digest, scan_digest, rid, principal, state, trial, override, branch=None):
    return {
        "binding": copy.deepcopy(B120),
        "branch": branch,
        "inputs": {"scan": {"handle": SCAN_HANDLE, "sha256": scan_digest}},
        "lifecycle_state": state,
        "outcome": "pending",
        "override_deprecated": override,
        "principal": dict(principal),
        "program_digest": program_digest,
        "program_id": PROGRAM_ID,
        "program_version": 1,
        "run_id": rid,
        "trial": trial,
        "variant": "B-120",
    }


# ------------------------------------------------------------------ program.run@1 parameters


def run_params(r):
    """The program.run@1 parameters of a record (pinned in crates/fid-run/src/record.rs docs):
    the program, its digest, the binding, every input handle, the variant and both flags."""
    return {
        "binding": copy.deepcopy(r["binding"]),
        "inputs": {k: v["handle"] for k, v in r["inputs"].items()},
        "override_deprecated": r["override_deprecated"],
        "program": r["program_id"],
        "program_digest": r["program_digest"],
        "trial": r["trial"],
        "variant": r["variant"],
    }


def commitment(params):
    """CORE.BUS.POL.PROJ params_commitment: SHA-256 of the RFC 8785 bytes."""
    return sha(canon(params).encode("ascii"))


# ------------------------------------------------------------------ forged cases
#
# Every case changes one thing against a legitimate run start and carries its verdict,
# HAND-LABELLED from REQ-CORE.RPL.RECON.FOLD-008 (a)-(g) and REQ-AUTO.RUN.BIND-001/002, not
# derived from any implementation:
#   (a) origin: principal, command id@major, class, adding stage, parameters and branch are the
#       entry's own -> any mismatch is refused (fatal, rule (f));
#   (b) RunStarted under any other command is refused;
#   (c) RunStarted beside any Request* event, or twice, is refused;
#   (d) a run whose decided-against (version, state) no longer holds, or a replay of a run
#       already admitted, is refused by the fold and `conflict` at the commit stage;
#   (e) a promoted copy is skipped (leaf's stated rule); a run starts only on run/<run_id>;
#   policy: draft/registered need a person's trial, deprecated a person's override.
# Verdicts: apply in {admitted, refused, skipped, ignored}; precommit in {ok, conflict,
# refused, skipped, ignored}. A refused-by-(a)/(b)/(c)/(e)/policy case is refused in both.
#
# By default the forged entry commits the parameters of the (possibly forged) record, so a
# case tests exactly its one fault; `committed` overrides what the entry committed.

R = ("refused", "refused")
OK = ("admitted", "ok")
STALE = ("refused", "conflict")
SKIP = ("skipped", "skipped")
IGNORE = ("ignored", "ignored")

OTHER_RUN = "018f0000-0000-7000-8000-0000000009ff"

# (name, item, base, entry overrides, record overrides, committed-params overrides, events,
#  lifecycle at commit, replay, hand-labelled (apply, precommit))
CASES = [
    ("base_agent_released", "base", "agent_released", {}, {}, {}, None, "as_prepared", False, OK),
    ("base_human_trial_registered", "base", "human_trial", {}, {}, {}, None, "as_prepared", False, OK),
    ("base_human_override_deprecated", "base", "human_override", {}, {}, {}, None, "as_prepared", False, OK),
    ("a_other_principal_id", "a", "agent_released", {"origin_principal": {"id": "mallory", "kind": "agent"}}, {}, {}, None, "as_prepared", False, R),
    ("a_record_claims_human", "a", "agent_released", {}, {"principal": HUMAN}, {}, None, "as_prepared", False, R),
    ("a_origin_human_record_agent", "a", "agent_released", {"origin_principal": HUMAN, "journal_kind": "human"}, {}, {}, None, "as_prepared", False, R),
    ("a_journal_kind_differs", "a", "agent_released", {"journal_kind": "script"}, {}, {}, None, "as_prepared", False, R),
    ("a_agent_forges_trial_of_human", "a", "human_trial", {"origin_principal": AGENT, "journal_kind": "agent"}, {}, {}, None, "as_prepared", False, R),
    ("a_other_command", "a", "agent_released", {"origin_command": "program.run_batch@1", "journal_command": "program.run_batch"}, {}, {}, None, "as_prepared", False, R),
    ("a_other_major", "a", "agent_released", {"origin_command": "program.run@2", "journal_major": 2}, {}, {}, None, "as_prepared", False, R),
    ("a_journal_command_differs", "a", "agent_released", {"journal_command": "program.register"}, {}, {}, None, "as_prepared", False, R),
    ("a_other_class", "a", "agent_released", {"class": "W3"}, {}, {}, None, "as_prepared", False, R),
    ("a_added_by_bus", "a", "agent_released", {"source": "bus_route"}, {}, {}, None, "as_prepared", False, R),
    ("a_other_params_commitment", "a", "agent_released", {"params_commitment": OTHER_COMMITMENT}, {}, {}, None, "as_prepared", False, R),
    ("a_committed_without_trial", "a", "human_trial", {}, {}, {"trial": False}, None, "as_prepared", False, R),
    ("a_committed_without_override", "a", "human_override", {}, {}, {"override_deprecated": False}, None, "as_prepared", False, R),
    ("a_record_forges_trial_flag", "a", "human_trial", {}, {"trial": True}, {"trial": False}, None, "as_prepared", False, R),
    ("a_record_forges_override_flag", "a", "human_override", {}, {"override_deprecated": True}, {"override_deprecated": False}, None, "as_prepared", False, R),
    ("a_committed_other_binding", "a", "agent_released", {}, {}, {"binding": {"hole_spacing": {"unit": "mm", "value": "130"}, "scan": SCAN_HANDLE}}, None, "as_prepared", False, R),
    ("a_record_forges_binding", "a", "agent_released", {}, {"binding": {"hole_spacing": {"unit": "mm", "value": "130"}, "scan": SCAN_HANDLE}}, {"binding": copy.deepcopy(B120)}, None, "as_prepared", False, R),
    ("a_committed_other_input", "a", "agent_released", {}, {}, {"inputs": {"scan": "input:scans/other.ply"}}, None, "as_prepared", False, R),
    ("a_committed_other_variant", "a", "agent_released", {}, {}, {"variant": "B-4IN"}, None, "as_prepared", False, R),
    ("a_committed_other_program_digest", "a", "agent_released", {}, {}, {"program_digest": OTHER_DIGEST}, None, "as_prepared", False, R),
    ("a_entry_on_other_branch", "a", "agent_released", {"branch": "run/" + OTHER_RUN}, {}, {}, None, "as_prepared", False, R),
    ("a_record_branch_null", "a", "agent_released", {}, {"branch": None}, {}, None, "as_prepared", False, R),
    ("b_reserved_effect_other_command", "b", "agent_released", {"origin_command": "project.create_run@1", "journal_command": "project.create_run"}, {}, {}, None, "as_prepared", False, R),
    ("c_request_approved", "c", "human_trial", {}, {}, {}, [RUN_STARTED, "RequestApproved"], "as_prepared", False, R),
    ("c_request_filed", "c", "agent_released", {}, {}, {}, ["RequestFiled", RUN_STARTED], "as_prepared", False, R),
    ("c_two_run_started", "c", "agent_released", {}, {}, {}, [RUN_STARTED, RUN_STARTED], "as_prepared", False, R),
    ("c_no_run_started", "c", "agent_released", {}, {}, {}, ["StageAdded"], "as_prepared", False, IGNORE),
    ("d_deprecated_since_prepared", "d", "agent_released", {}, {}, {}, None, "deprecated", False, STALE),
    ("d_other_version", "d", "agent_released", {}, {"program_version": 2}, {}, None, "as_prepared", False, STALE),
    ("d_other_digest", "d", "agent_released", {}, {"program_digest": OTHER_DIGEST}, {}, None, "as_prepared", False, STALE),
    ("d_claims_released_while_registered", "d", "human_trial", {}, {"lifecycle_state": "released"}, {}, None, "as_prepared", False, STALE),
    ("d_replayed_entry", "d", "agent_released", {}, {}, {}, None, "as_prepared", True, STALE),
    ("e_promoted_copy", "e", "agent_released", {"promoted_from": PROMOTED_FROM, "branch": "main"}, {}, {}, None, "as_prepared", False, SKIP),
    ("e_run_on_main", "e", "agent_released", {"branch": "main"}, {"branch": "main"}, {}, None, "as_prepared", False, R),
    ("e_run_on_agent_branch", "e", "agent_released", {"branch": "agent/claude-client/1"}, {"branch": "agent/claude-client/1"}, {}, None, "as_prepared", False, R),
    ("e_run_on_another_runs_branch", "e", "agent_released", {"branch": "run/" + OTHER_RUN}, {"branch": "run/" + OTHER_RUN}, {}, None, "as_prepared", False, R),
    ("e_run_branch_prefix_only", "e", "agent_released", {"branch": "run/"}, {"branch": "run/"}, {}, None, "as_prepared", False, R),
    ("p_agent_trial_registered", "policy", "human_trial", {"origin_principal": AGENT, "journal_kind": "agent"}, {"principal": AGENT}, {}, None, "as_prepared", False, R),
    ("p_human_registered_without_trial", "policy", "human_trial", {}, {"trial": False}, {}, None, "as_prepared", False, R),
    ("p_agent_override_deprecated", "policy", "human_override", {"origin_principal": AGENT, "journal_kind": "agent"}, {"principal": AGENT}, {}, None, "as_prepared", False, R),
    ("p_human_deprecated_without_override", "policy", "human_override", {}, {"override_deprecated": False}, {}, None, "as_prepared", False, R),
    ("p_override_on_released", "policy", "agent_released", {}, {"override_deprecated": True}, {}, None, "as_prepared", False, OK),
    ("p_agent_trial_released", "policy", "agent_released", {}, {"trial": True}, {}, None, "as_prepared", False, OK),
    ("p_outcome_not_pending", "policy", "agent_released", {}, {"outcome": "succeeded"}, {}, None, "as_prepared", False, R),
    ("p_unknown_record_field", "policy", "agent_released", {}, {"approved_by": "alice"}, {}, None, "as_prepared", False, R),
]

# The lifecycle state each base was prepared against (version 1).
BASE_STATE = {
    "agent_released": "released",
    "human_trial": "registered",
    "human_override": "deprecated",
}


# ------------------------------------------------------------------ cross-check of the labels
#
# A plain reading of the rules above, used only to catch a mislabelled case: the script fails if
# it disagrees with a hand label. The labels, not this function, are what the Rust test asserts.


def policy_ok(r):
    human = r["principal"]["kind"] == "human"
    state = r["lifecycle_state"]
    if state == "released":
        return True
    if state in ("draft", "registered"):
        return human and r["trial"] is True
    if state == "deprecated":
        return human and r["override_deprecated"] is True
    return False


def well_formed(r):
    if not isinstance(r, dict) or sorted(r) != RECORD_KEYS:
        return False
    p = r["principal"]
    if not isinstance(p, dict) or sorted(p) != ["id", "kind"]:
        return False
    if r["lifecycle_state"] not in ("draft", "registered", "released", "deprecated"):
        return False
    if r["outcome"] not in ("pending", "succeeded", "failed", "cancelled"):
        return False
    return isinstance(r["trial"], bool) and isinstance(r["override_deprecated"], bool)


def cross_check(e, r, events, store, admitted, mode):
    mine = [x for x in events if x == RUN_STARTED]
    if not mine:
        return "ignored"
    if len(mine) > 1 or any(x.startswith("Request") for x in events):
        return "refused"
    if e["promoted_from"] is not None:
        return "skipped"
    if (e["origin_command"], e["journal_command"], e["journal_major"]) != (RUN_COMMAND + "@1", RUN_COMMAND, 1):
        return "refused"
    if e["class"] != "W2" or e["source"] != "handler":
        return "refused"
    if not well_formed(r):
        return "refused"
    if r["principal"] != e["origin_principal"] or e["journal_kind"] != r["principal"]["kind"]:
        return "refused"
    if commitment(run_params(r)) != e["params_commitment"]:
        return "refused"
    own = "run/" + r["run_id"]
    if r["branch"] != own or e["branch"] != own:
        return "refused"
    if r["outcome"] != "pending" or not policy_ok(r):
        return "refused"
    stale = "conflict" if mode == "precommit" else "refused"
    if store.get((r["program_id"], r["program_digest"]), (None, "draft")) != (r["program_version"], r["lifecycle_state"]):
        return stale
    if r["run_id"] in admitted:
        return stale
    return "ok" if mode == "precommit" else "admitted"


def forged(program_digest, scan_digest):
    branch = "run/" + run_id(1)
    bases = {
        "agent_released": record(program_digest, scan_digest, run_id(1), AGENT, "released", False, False, branch),
        "human_trial": record(program_digest, scan_digest, run_id(1), HUMAN, "registered", True, False, branch),
        "human_override": record(program_digest, scan_digest, run_id(1), HUMAN, "deprecated", False, True, branch),
    }
    out = []
    for name, item, base, entry_o, record_o, committed_o, events, lifecycle, replay, label in CASES:
        r = copy.deepcopy(bases[base])
        r.update(copy.deepcopy(record_o))
        committed = run_params(r) if well_formed(r) else run_params(bases[base])
        committed.update(copy.deepcopy(committed_o))
        e = {
            "branch": branch,
            "class": "W2",
            "journal_command": RUN_COMMAND,
            "journal_kind": bases[base]["principal"]["kind"],
            "journal_major": 1,
            "origin_command": RUN_COMMAND + "@1",
            "origin_principal": dict(bases[base]["principal"]),
            "params_commitment": commitment(committed),
            "promoted_from": None,
            "source": "handler",
        }
        e.update(copy.deepcopy(entry_o))
        events = events if events is not None else [RUN_STARTED]
        state = BASE_STATE[base] if lifecycle == "as_prepared" else lifecycle
        store = {(PROGRAM_ID, program_digest): (1, state)}
        admitted = {r["run_id"]} if replay else set()
        got = (cross_check(e, r, events, store, admitted, "apply"), cross_check(e, r, events, store, admitted, "precommit"))
        assert got == label, "%s: hand label %s, rules say %s" % (name, label, got)
        out.append({
            "apply": label[0],
            "base": base,
            "entry": e,
            "events": events,
            "item": item,
            "lifecycle": lifecycle,
            "name": name,
            "precommit": label[1],
            "record": record_o,
            "replay": replay,
        })
    items = {c["item"] for c in out}
    assert {"a", "b", "c", "d", "e"} <= items
    return out


# ------------------------------------------------------------------ main


def build():
    program_bytes = PROGRAM_FILE.read_bytes()
    program_digest = sha(program_bytes)
    scan = scan_bytes()
    scan_digest = sha(scan)
    ac1 = record(program_digest, scan_digest, run_id(1), AGENT, "released", False, False)
    ac2 = record(program_digest, scan_digest, run_id(1), HUMAN, "registered", True, False)
    ac3 = record(program_digest, scan_digest, run_id(1), HUMAN, "deprecated", False, True)
    expected = {
        "ac1_record": canon(ac1),
        "ac2_record": canon(ac2),
        "ac3_record": canon(ac3),
        "ac4_errors": [{"code": "missing_parameter", "param": "hole_spacing"}],
        "program_digest": program_digest,
        "program_id": PROGRAM_ID,
        "run_ids": [run_id(n) for n in (1, 2, 3)],
        "scan_handle": SCAN_HANDLE,
        "scan_sha256": scan_digest,
    }
    files = {
        SCAN_FILE: scan,
        HERE / "expected.json": (json.dumps(expected, indent=1, sort_keys=True) + "\n").encode("ascii"),
        HERE / "forged.json": (json.dumps(forged(program_digest, scan_digest), indent=1, sort_keys=True) + "\n").encode("ascii"),
    }
    return files


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for path, data in build().items():
        if check:
            if not path.exists() or path.read_bytes() != data:
                print("stale: %s" % path.relative_to(ROOT))
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if bad:
        print("run: python fixtures/auto/run/bind/make_cases.py")
        sys.exit(1)


if __name__ == "__main__":
    main()
