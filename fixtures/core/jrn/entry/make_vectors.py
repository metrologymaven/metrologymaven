#!/usr/bin/env python3
"""Generate fixtures/core/jrn/entry/vectors.json and invalid_headers.json
(CORE.JRN.ENTRY AC-1, AC-3, REQ-011).

An independent reference for the entry model: it builds each header and payload
from a draft with Python's standard library only (hashlib, json), so the Rust
crate `fid-journal` is checked against a second implementation, not itself.

Rules implemented here (from spec/core/jrn/entry/NODE.md):
- commitment = "sha256:" + SHA-256(16 salt bytes || RFC 8785 bytes of the value);
- entry hash = SHA-256 of the RFC 8785 header with `hash` omitted;
- 64-bit integers (seq, mono_ns, seed) are decimal strings;
- each stream is its own chain (CORE.JRN.CHAIN.APPEND REQ-002): sequences start at 1 and
  the first entry's `prev` is `sha256:` + 64 zeros; `prev` is never null;
- `logical_parent` is null when there is none, never the zero digest;
- signatures are detached and not part of the header;
- the human-assertion pre-image is the entry hash with `human_assertion` also omitted;
- consumed R1 result digests are appended to the inputs tagged `r1_result`;
- a deleted entity's output digest is the tombstone `sha256:` + 64 zeros;
- schema version 2 (REQ-013..REQ-015, maintainer decision): a header records
  `agent_coedited` (boolean) and, on a command, `command.event_sources` (one of `handler`,
  `bus_route`, `bus_release`, `bus_approval` per event, in event order) exactly when it holds
  something version 1 cannot: `agent_coedited: true` or a source other than `handler`. A
  draft's `event_sources` is either absent (every event from the handler) or one per event;
  when the header is version 2 and the draft gave none, every event is `handler`. Anything
  else is written as version 1 without either field, byte for byte as before.

Values hold no non-integer floats and no object keys outside the BMP, so
json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=False) equals RFC 8785.
Salts are fixed, derived from the case name, so the fixture is reproducible.
CC-BY-4.0 project data. Run:  python fixtures/core/jrn/entry/make_vectors.py
"""

import copy
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
TOMBSTONE = "sha256:" + "0" * 64
ZERO = TOMBSTONE  # also the `prev` of sequence 1


def canon(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def d(label):
    """A stand-in digest of some named content."""
    return sha(label.encode("utf-8"))


def salt_for(case, field):
    return hashlib.sha256(f"salt/{case}/{field}".encode("utf-8")).hexdigest()[:32]


def commit(salt_hex, value):
    return sha(bytes.fromhex(salt_hex) + canon(value))


def build(name, draft, seq, prev):
    salts = {}
    payload = {}

    def c(field, value):
        s = salt_for(name, field)
        salts[field] = s
        payload[field] = value
        return commit(s, value)

    p = draft["principal"]
    principal = {"kind": p["kind"], "commit": c("identity", p["identity"])}
    if "on_behalf_of" in p:
        principal["on_behalf_of_commit"] = c("on_behalf_of", p["on_behalf_of"])
    k = draft["kind"]
    coedited = draft.get("agent_coedited", False)
    sources = draft["command"].get("event_sources", []) if k == "command" else []
    if sources and len(sources) != len(draft["command"]["events"]):
        raise ValueError(f"{name}: one event source per event")
    v2 = coedited is True or any(x != "handler" for x in sources)
    h = {
        "schema_version": 2 if v2 else 1,
        "kind": draft["kind"],
        "stream": draft["stream"],
        "seq": str(seq),
        "prev": prev,
        "branch": draft["branch"],
        "logical_parent": draft["logical_parent"],
        "time": dict(draft["time"]),
        "principal": principal,
        "ai_assistance": draft["ai_assistance"],
        "manifests": dict(draft["manifests"]),
    }
    if "reason" in draft:
        h["reason_commit"] = c("reason", draft["reason"])
    if "human_assertion" in draft:
        h["human_assertion"] = draft["human_assertion"]
    if v2:
        h["agent_coedited"] = coedited
    if k == "command":
        cd = draft["command"]
        inputs = [dict(i) for i in cd["inputs"]] + [{"tag": "r1_result", "digest": x} for x in cd.get("consumed_r1", [])]
        outputs = []
        for o in cd["outputs"]:
            out = {}
            if "entity" in o:
                out["entity"] = o["entity"]
            out["digest"] = TOMBSTONE if o.get("deleted") else o["digest"]
            outputs.append(out)
        h["command"] = {
            "id": cd["id"], "major": cd["major"], "class": cd["class"], "determinism": cd["determinism"],
            "seed": cd["seed"], "args_commit": c("args", cd["args"]), "events_commit": c("events", cd["events"]),
            "inputs": inputs, "outputs": outputs, "algorithms": list(cd["algorithms"]),
        }
        if v2:
            h["command"]["event_sources"] = list(sources) or ["handler"] * len(cd["events"])
    elif k == "session_header":
        h["session"] = dict(draft["session"])
    elif k == "principal_upgrade":
        u = draft["upgrade"]
        h["upgrade"] = {"session_id": u["session_id"], "method": u["method"],
                        "identity_commit": c("upgrade_identity", u["identity"]), "confirmed_at": u["confirmed_at"]}
    elif k == "head_move":
        h["head_move"] = dict(draft["head_move"])
    else:
        raise ValueError(k)
    h["hash"] = sha(canon(h))
    pre = {key: val for key, val in h.items() if key not in ("hash", "human_assertion")}
    payload["salts"] = salts
    commitments = {}
    for f in salts:
        commitments[f] = commit(salts[f], payload[f])
    return h, payload, commitments, sha(canon(pre))


# ------------------------------------------------------------------ drafts

REG = [d(f"registry-manifest-{i}") for i in range(3)]
KER = [d(f"kernel-manifest-{i}") for i in range(3)]
PRINCIPALS = [
    ("human", {"user": "op-17", "name": "Ana Müller"}),
    ("agent", {"client": "claude-code", "session": "a7"}),
    ("script", {"user": "op-17", "argv0": "fid", "pid": 4412}),
    ("service", {"service": "nightly-report"}),
    ("device", {"device": "cmm-02", "serial": "SN-0042"}),
    ("system", {"component": "journal"}),
]
AI = ["none", "suggested", "authored", "executed"]
SYNC = ["synced", "unsynced", "unknown"]


def base(i, kind):
    pk, ident = PRINCIPALS[i % len(PRINCIPALS)]
    dr = {
        "kind": kind,
        "stream": "activity" if i % 7 == 6 else "record",
        "branch": ["main", "agent/claude-code/3", "feature/bore-check"][i % 3],
        "logical_parent": None if i % 5 == 0 else d(f"state-{i}"),
        "time": {"utc": f"2026-11-17T08:{i % 60:02d}:{(i * 7) % 60:02d}" + (".123456789Z" if i % 2 else "Z"),
                 "mono_ns": str(1_000_000_000 * i + 17 + (2**64 - 1 - 10**12 if i % 9 == 8 else 0)),
                 "sync": SYNC[i % 3]},
        "principal": {"kind": pk, "identity": ident},
        "ai_assistance": AI[i % 4],
        "manifests": {"registry": REG[i % 3], "kernel": KER[(i + 1) % 3]},
    }
    if pk in ("agent", "script") and i % 2 == 0:
        dr["principal"]["on_behalf_of"] = {"user": "op-17"}
    if i % 4 == 3:
        dr["reason"] = ["undo misprobe", "re-run after recalibration", "Ø 12 H7 bore recheck — µm drift"][i % 3]
    if i % 6 == 5:
        dr["human_assertion"] = "ed25519:" + hashlib.sha256(f"assert-{i}".encode()).hexdigest()
    return dr


COMMANDS = [
    ("align.run_321", "W2", "D0", ["align.fit.rps_321@1.0.0"]),
    ("feature.fit_circle", "W1", "D1", ["feature.fit.circle_ls@1.2.0", "feature.fit.circle_mz@0.3.1"]),
    ("report.release", "W3", "D0", []),
    ("probe.calibrate", "M1", "D3", ["probe.cal.sphere_ls@2.0.0"]),
    ("data.import_points", "D1", "N", []),
    ("results.view", "R1", "D0", []),
    ("trust.sign_checkpoint", "S1", "D0", []),
    ("plan.delete_feature", "W2", "D2", []),
]


def command_draft(i):
    cid, cls, det, algs = COMMANDS[i % len(COMMANDS)]
    dr = base(i, "command")
    outputs = [{"entity": f"F-{i:04d}", "digest": d(f"out-{i}-0")}, {"digest": d(f"blob-{i}")}]
    if cid == "plan.delete_feature" or i % 5 == 2:
        outputs.append({"entity": f"F-{i + 100:04d}", "deleted": True})
    dr["command"] = {
        "id": cid, "major": 1 + i % 3, "class": cls, "determinism": det,
        "seed": str([0, 42, 2**64 - 1, 9007199254740993][i % 4]),
        "args": {"feature": f"F-{i:04d}", "points": [1, -2, 300], "nested": {"b": True, "a": None}, "note": "Ø6 µm"},
        "events": [{"type": f"{cid}@1", "n": i}] if i % 3 else [],
        "inputs": [{"tag": "param", "digest": d(f"in-{i}")}, {"tag": "entity", "digest": d(f"ent-{i}")}][: 1 + i % 2],
        "outputs": outputs,
        "algorithms": algs,
    }
    if cls in ("W1", "W2", "W3") and i % 2 == 0:
        dr["command"]["consumed_r1"] = [d(f"r1-{i}-{j}") for j in range(1 + i % 3)]
    return dr


def session_draft(i):
    dr = base(i, "session_header")
    dr["session"] = {"session_id": f"sess-{i:03d}", "app_version": f"0.{i}.0"}
    return dr


def upgrade_draft(i):
    dr = base(i, "principal_upgrade")
    dr["upgrade"] = {"session_id": f"sess-{i:03d}", "method": ["gui_prompt", "os_presence"][i % 2],
                     "identity": {"user": "op-17", "name": "Ana Müller"}, "confirmed_at": f"2026-11-17T09:{i:02d}:00Z"}
    return dr


def head_move_draft(i):
    dr = base(i, "head_move")
    dr["head_move"] = {"from": d(f"head-{i}"), "to": d(f"head-{i - 1}")}
    return dr


def v2_draft(i):
    """Schema version 2 cases (REQ-013..REQ-015), appended after the version 1 vectors."""
    j = i - 36
    agent_branch = {"type": "BranchCreated", "body": {"name": "agent/claude-code/9", "from": d(f"head-{i}")}}
    if j == 6:
        dr = session_draft(i)
        dr["agent_coedited"] = True
        return dr
    if j == 7:
        dr = head_move_draft(i)
        dr["agent_coedited"] = True
        return dr
    dr = command_draft(i)
    c = dr["command"]
    if j == 0:  # the route stage adds an agent branch after the handler's event
        c["events"] = [{"type": "feature.nominal_set@1", "n": i}, agent_branch]
        c["event_sources"] = ["handler", "bus_route"]
    elif j == 1:  # an agent commit co-edited onto a human branch; no sources given: all handler
        dr["agent_coedited"] = True
        c["events"] = [{"type": "feature.nominal_set@1", "n": i}, {"type": "feature.tol_set@1", "n": i}]
    elif j == 2:  # release protection added by the commit stage on a W3 entry on main
        dr["branch"] = "main"
        c.update({"id": "report.release", "class": "W3"})
        c["events"] = [{"type": "report.released@1", "n": i},
                       {"type": "BranchProtected", "body": {"name": "main", "cause": "release"}}]
        c["event_sources"] = ["handler", "bus_release"]
    elif j == 3:  # the RequestApproved of an approval
        c["events"] = [{"type": "feature.nominal_set@1", "n": i}, {"type": "RequestApproved", "n": i}]
        c["event_sources"] = ["handler", "bus_approval"]
    elif j == 4:  # co-edited and routed, every source given
        dr["agent_coedited"] = True
        c["events"] = [agent_branch]
        c["event_sources"] = ["bus_route"]
    elif j == 5:  # co-edited with no events at all
        dr["agent_coedited"] = True
        c["events"] = []
    elif j == 8:  # every source given and all handler: still version 1, byte for byte
        c["events"] = [{"type": "feature.nominal_set@1", "n": i}]
        c["event_sources"] = ["handler"]
        dr["agent_coedited"] = False
    else:
        raise ValueError(j)
    return dr


V2_CASES = 9


def main():
    vectors = []
    # One chain per stream: (next seq, prev hash).
    chains = {}
    plan = [("session_header", session_draft)] + [("command", command_draft)] * 16 + \
        [("session_header", session_draft)] * 5 + [("principal_upgrade", upgrade_draft)] * 7 + \
        [("head_move", head_move_draft)] * 7 + [("v2", v2_draft)] * V2_CASES
    for i, (kind, mk) in enumerate(plan):
        draft = mk(i)
        name = f"{i:02d}_{kind}" if kind != "v2" else f"{i:02d}_v2_{draft['kind']}"

        seq, prev = chains.get(draft["stream"], (1, ZERO))
        h, payload, commitments, pre = build(name, draft, seq, prev)
        vectors.append({"name": name, "seq": str(seq), "prev": prev, "draft": draft, "salts": payload["salts"],
                        "header": h, "payload": payload, "hash": h["hash"], "commitments": commitments,
                        "assertion_preimage": pre})
        chains[draft["stream"]] = (seq + 1, h["hash"])
    (HERE / "vectors.json").write_text(json.dumps(vectors, indent=1, ensure_ascii=False) + "\n", encoding="utf-8",
                                       newline="\n")

    # Invalid headers: each a single hand-labelled mutation of a valid header.
    by_kind = {}
    for v in vectors:
        by_kind.setdefault(v["header"]["kind"], v["header"])
    cmd, ses, upg, hm = (by_kind[k] for k in ("command", "session_header", "principal_upgrade", "head_move"))
    v2s = [v["header"] for v in vectors if v["header"]["schema_version"] == 2]
    cmd2 = next(h for h in v2s if h["kind"] == "command" and h["agent_coedited"] is False)
    ses2 = next(h for h in v2s if h["kind"] == "session_header")
    bad = []

    def mut(name, h, fn):
        h = copy.deepcopy(h)
        fn(h)
        bad.append({"name": name, "header": h})

    mut("missing_ai_assistance", cmd, lambda h: h.pop("ai_assistance"))
    mut("ai_assistance_maybe", cmd, lambda h: h.__setitem__("ai_assistance", "maybe"))
    mut("command_body_missing", cmd, lambda h: h.pop("command"))
    mut("session_with_command_body", ses, lambda h: h.__setitem__("command", cmd["command"]))
    mut("seq_as_number", cmd, lambda h: h.__setitem__("seq", 5))
    mut("seq_leading_zero", cmd, lambda h: h.__setitem__("seq", "01"))
    mut("prev_uppercase_digest", cmd, lambda h: h.__setitem__("prev", "sha256:" + "A" * 64))
    mut("unknown_top_level_field", cmd, lambda h: h.__setitem__("trace", "x"))
    mut("schema_version_2", cmd, lambda h: h.__setitem__("schema_version", 2))
    mut("principal_kind_robot", cmd, lambda h: h["principal"].__setitem__("kind", "robot"))
    mut("principal_identity_in_clear", cmd, lambda h: h["principal"].__setitem__("identity", {"user": "op-17"}))
    mut("upgrade_method_password", upg, lambda h: h["upgrade"].__setitem__("method", "password"))
    mut("mono_ns_missing", hm, lambda h: h["time"].pop("mono_ns"))
    mut("utc_without_z", hm, lambda h: h["time"].__setitem__("utc", "2026-11-17T08:00:00+01:00"))
    mut("output_digest_missing", cmd, lambda h: h["command"]["outputs"][0].pop("digest"))
    mut("algorithm_id_malformed", cmd, lambda h: h["command"].__setitem__("algorithms", ["align.rps@1"]))
    mut("major_negative", cmd, lambda h: h["command"].__setitem__("major", -1))
    mut("class_w9", cmd, lambda h: h["command"].__setitem__("class", "W9"))
    mut("hash_missing", ses, lambda h: h.pop("hash"))
    mut("args_commit_missing", cmd, lambda h: h["command"].pop("args_commit"))
    mut("head_move_kind_mismatch", hm, lambda h: h.__setitem__("kind", "session_header"))
    mut("empty_human_assertion", ses, lambda h: h.__setitem__("human_assertion", ""))
    mut("input_tag_uppercase", cmd, lambda h: h["command"]["inputs"][0].__setitem__("tag", "Param"))
    mut("session_app_version_missing", ses, lambda h: h["session"].pop("app_version"))
    mut("seq_overflow_u64", cmd, lambda h: h.__setitem__("seq", "99999999999999999999"))
    mut("seq_u64_max_plus_one", cmd, lambda h: h.__setitem__("seq", "18446744073709551616"))
    mut("seq_zero", ses, lambda h: h.__setitem__("seq", "0"))
    mut("mono_ns_overflow_u64", hm, lambda h: h["time"].__setitem__("mono_ns", "18446744073709551616"))
    mut("algorithm_version_over_u32", cmd, lambda h: h["command"].__setitem__("algorithms", ["a.b.c@4294967296.0.0"]))
    mut("prev_null", ses, lambda h: h.__setitem__("prev", None))
    mut("logical_parent_zero", cmd, lambda h: h.__setitem__("logical_parent", ZERO))
    mut("tombstone_without_entity", cmd, lambda h: h["command"]["outputs"][1].__setitem__("digest", ZERO))
    mut("signatures_in_header", ses, lambda h: h.__setitem__("signatures", []))
    # One case per nested rule: unknown key and missing key in every object, plus each
    # typed property that is not otherwise probed.
    mut("seq_trailing_newline", cmd, lambda h: h.__setitem__("seq", "1\n"))
    mut("prev_trailing_newline", cmd, lambda h: h.__setitem__("prev", cmd["prev"] + "\n"))
    mut("time_extra_field", hm, lambda h: h["time"].__setitem__("tz", "UTC"))
    mut("time_sync_missing", hm, lambda h: h["time"].pop("sync"))
    mut("time_sync_empty", hm, lambda h: h["time"].__setitem__("sync", ""))
    mut("principal_commit_missing", cmd, lambda h: h["principal"].pop("commit"))
    mut("on_behalf_of_commit_not_digest", cmd, lambda h: h["principal"].__setitem__("on_behalf_of_commit", "op-17"))
    mut("manifests_extra_field", cmd, lambda h: h["manifests"].__setitem__("plugins", ZERO))
    mut("manifests_kernel_missing", cmd, lambda h: h["manifests"].pop("kernel"))
    mut("command_extra_field", cmd, lambda h: h["command"].__setitem__("trace_id", "t-1"))
    mut("seed_not_decimal", cmd, lambda h: h["command"].__setitem__("seed", "seed-42"))
    mut("major_over_u32", cmd, lambda h: h["command"].__setitem__("major", 4294967296))
    mut("determinism_d9", cmd, lambda h: h["command"].__setitem__("determinism", "D9"))
    mut("events_commit_not_digest", cmd, lambda h: h["command"].__setitem__("events_commit", "events"))
    mut("command_id_uppercase", cmd, lambda h: h["command"].__setitem__("id", "Align.run"))
    mut("input_extra_field", cmd, lambda h: h["command"]["inputs"][0].__setitem__("note", "x"))
    mut("input_digest_missing", cmd, lambda h: h["command"]["inputs"][0].pop("digest"))
    mut("output_extra_field", cmd, lambda h: h["command"]["outputs"][0].__setitem__("deleted", True))
    mut("output_entity_empty", cmd, lambda h: h["command"]["outputs"][0].__setitem__("entity", ""))
    mut("session_extra_field", ses, lambda h: h["session"].__setitem__("host", "pc-1"))
    mut("session_id_empty", ses, lambda h: h["session"].__setitem__("session_id", ""))
    mut("upgrade_extra_field", upg, lambda h: h["upgrade"].__setitem__("identity", {"user": "op-17"}))
    mut("upgrade_confirmed_at_missing", upg, lambda h: h["upgrade"].pop("confirmed_at"))
    mut("upgrade_identity_commit_not_digest", upg, lambda h: h["upgrade"].__setitem__("identity_commit", "x"))
    mut("head_move_extra_field", hm, lambda h: h["head_move"].__setitem__("reason", "undo"))
    mut("head_move_to_missing", hm, lambda h: h["head_move"].pop("to"))
    mut("head_move_to_not_digest", hm, lambda h: h["head_move"].__setitem__("to", "state-3"))
    mut("stream_uppercase", cmd, lambda h: h.__setitem__("stream", "Record"))
    mut("branch_leading_slash", cmd, lambda h: h.__setitem__("branch", "/main"))
    mut("reason_commit_not_digest", hm, lambda h: h.__setitem__("reason_commit", "because"))
    # Schema version 2 (REQ-013..REQ-015).
    mut("schema_version_3", cmd, lambda h: h.__setitem__("schema_version", 3))
    mut("v1_with_agent_coedited", cmd, lambda h: h.__setitem__("agent_coedited", False))
    mut("v1_with_event_sources", cmd, lambda h: h["command"].__setitem__("event_sources", ["handler"]))
    mut("v2_missing_agent_coedited", cmd2, lambda h: h.pop("agent_coedited"))
    mut("v2_command_missing_event_sources", cmd2, lambda h: (h.__setitem__("agent_coedited", True),
                                                             h["command"].pop("event_sources")))
    mut("v2_only_defaults", cmd2, lambda h: h["command"].__setitem__(
        "event_sources", ["handler"] * len(h["command"]["event_sources"])))
    mut("v2_session_only_defaults", ses2, lambda h: h.__setitem__("agent_coedited", False))
    mut("event_source_unknown", cmd2, lambda h: h["command"]["event_sources"].__setitem__(0, "plugin"))
    mut("event_sources_not_array", cmd2, lambda h: h["command"].__setitem__("event_sources", "bus_route"))
    mut("agent_coedited_not_boolean", cmd2, lambda h: h.__setitem__("agent_coedited", "yes"))
    (HERE / "invalid_headers.json").write_text(json.dumps(bad, indent=1, ensure_ascii=False) + "\n", encoding="utf-8",
                                               newline="\n")
    kinds = sorted({v["header"]["kind"] for v in vectors})
    print(f"wrote {len(vectors)} vectors ({', '.join(kinds)}) and {len(bad)} invalid headers")


if __name__ == "__main__":
    main()
