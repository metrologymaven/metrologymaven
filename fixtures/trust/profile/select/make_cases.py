#!/usr/bin/env python3
"""Independent reference for TRUST.PROFILE.SELECT (crate fid-profile-cmd). CC-BY-4.0 project data.

Writes cases.json next to this script:

- "lean" and "regulated_xqa": the canonical documents of the Lean built-in and of Regulated
  instantiated with fixtures/profile/regulated-with-xqa.json, read from the TRUST.PROFILE.DEFS
  fixtures (fixtures/profile/builtin/lean.json, fixtures/profile/expected/regulated-with-xqa.json)
  and hashed here with hashlib; the digests are cross-checked against fixtures/profile/digests.json.
- "scenario": the calls of AC-1 to AC-4 in order, with the actor (the principal kind the bus
  derives from the channel), the parameters and the expected events, result or bus error.
- "forged": fold cases for the ProfileSet event: an entry record (bus origin: principal kind and
  identity, command id@major, parameters commitment; journal origin: kind, command, major,
  class, branch, adding stage; promoted_from; the entry hash), the entry's events, and whether
  the fold accepts them, with the active profile after; for decision mismatches also what the
  commit stage's precommit check returns. Every way to forge, splice, replay or race the event
  is refused; entries off main are ignored.

Rules typed in from spec/trust/profile/select/NODE.md and the module docs of
crates/fid-profile-cmd (the leaf's Design made concrete):

- sign.set_profile@1 runs only for a human principal (else policy/human_only); it
  instantiates the profile and, if the digest differs from the active one, returns one
  ProfileSet event {decided_at, document, name, new_digest, old_digest, params_commitment}
  (decided_at: the hash of the last applied ProfileSet entry, or "genesis"; the test journal
  hashes its n-th entry as SHA-256 of b"entry-n") and the
  result {new_digest, old_digest, status: changed}; the same digest gives status unchanged
  and no event (REQ-001 to REQ-003).
- sign.show_profile@1 returns {digest, document, name, origin}; origin is "default" (Lean)
  until a ProfileSet event is folded, then "set" (REQ-004, REQ-005).
- profile.* refusals become validation/schema whose detail starts with the profile code.
- The parameters commitment is SHA-256 of the RFC 8785 bytes of the call's parameters
  (ASCII keys and strings only here, so sorted keys without whitespace are exactly RFC 8785).
- The fold counts only entries on main. It accepts a ProfileSet event only in a direct (not
  promoted) A0 sign.set_profile@1 handler entry of a human, with no Request* event beside it,
  whose parameters commitment is the body's, whose decided_at is the last applied decision,
  with a canonical document of that name and digest, whose old_digest is the active digest and
  differs from new_digest; at most one per entry. Other event types are ignored.

Stdlib only; nothing here runs the Rust code. Run:
    python fixtures/trust/profile/select/make_cases.py [--check]
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
PROFILE = ROOT / "fixtures" / "profile"
TENANT_FILE = "fixtures/profile/regulated-with-xqa.json"
SET = "sign.set_profile@1"
EVENT = "ProfileSet"
GENESIS = "genesis"


def jcs(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def doc(path):
    text = path.read_bytes()
    obj = json.loads(text)
    if jcs(obj) != text:
        raise SystemExit("%s is not canonical" % path)
    return {"digest": sha(text), "document": text.decode("ascii"), "object": obj}


def commitment(params):
    return sha(jcs(params))


def entry_hash(n):
    """The hash the test journal gives its n-th committed entry (1-based): synthetic."""
    return sha(b"entry-%d" % n)


def event(new, old, params, name, decided_at):
    return {
        "type": EVENT,
        "body": {
            "decided_at": decided_at,
            "document": new["document"],
            "name": name,
            "new_digest": new["digest"],
            "old_digest": old["digest"],
            "params_commitment": commitment(params),
        },
    }


def show(p, origin):
    return {"result": {"digest": p["digest"], "document": p["object"], "name": p["object"]["name"], "origin": origin}}


def build():
    lean = doc(PROFILE / "builtin" / "lean.json")
    regulated = doc(PROFILE / "builtin" / "regulated.json")
    assistive = doc(PROFILE / "builtin" / "assistive_only.json")
    xqa = doc(PROFILE / "expected" / "regulated-with-xqa.json")
    pinned = json.loads((PROFILE / "digests.json").read_text(encoding="utf-8"))
    for key, d in (("lean", lean), ("regulated", regulated), ("assistive_only", assistive), ("regulated-with-xqa", xqa)):
        if pinned[key] != d["digest"]:
            raise SystemExit("digest of %s differs from fixtures/profile/digests.json" % key)

    set_xqa = {"name": "regulated", "params": TENANT_FILE}
    set_lean = {"name": "lean"}
    set_assistive = {"name": "assistive_only"}
    lean_with_xqa = {"name": "lean", "params": TENANT_FILE}
    changed = lambda new, old: {"new_digest": new["digest"], "old_digest": old["digest"], "status": "changed"}

    # The scenario's journal commits two entries: entry-1 (Lean -> xqa) and entry-2 (xqa -> Lean).
    scenario = [
        {"actor": "human", "op": "show", "params": {}, "expect": show(lean, "default")},  # AC-1
        {"actor": "agent", "op": "show", "params": {}, "expect": show(lean, "default")},
        # AC-3: any non-human principal, whatever the surface.
        {"actor": "agent", "op": "set", "params": set_xqa, "expect": {"error": ["policy", "human_only"]}},
        {"actor": "script", "op": "set", "params": set_xqa, "expect": {"error": ["policy", "human_only"]}},
        # AC-2.
        {
            "actor": "human",
            "op": "set",
            "params": set_xqa,
            "expect": {"events": [event(xqa, lean, set_xqa, "regulated", GENESIS)], "result": changed(xqa, lean)},
        },
        {"actor": "human", "op": "show", "params": {}, "expect": show(xqa, "set")},
        # AC-4.
        {
            "actor": "human",
            "op": "set",
            "params": set_xqa,
            "expect": {
                "events": [],
                "result": {"new_digest": xqa["digest"], "old_digest": xqa["digest"], "status": "unchanged"},
            },
        },
        # profile.* passthrough: a tenant key Lean does not allow.
        {
            "actor": "human",
            "op": "set",
            "params": lean_with_xqa,
            "expect": {"error": ["validation", "schema"], "detail_prefix": "profile.param_forbidden"},
        },
        {
            "actor": "human",
            "op": "set",
            "params": set_lean,
            "expect": {
                "events": [event(lean, xqa, set_lean, "lean", entry_hash(1))],
                "result": changed(lean, xqa),
            },
        },
        # Lean set explicitly is origin "set", not "default".
        {"actor": "human", "op": "show", "params": {}, "expect": show(lean, "set")},
        {"actor": "human", "op": "set", "params": {"name": "strict"}, "expect": {"error": ["validation", "schema"]}},
    ]

    good = event(xqa, lean, set_xqa, "regulated", GENESIS)
    back = event(lean, xqa, set_lean, "lean", entry_hash(1))
    to_assistive = event(assistive, lean, set_assistive, "assistive_only", GENESIS)

    def entry(n, params, **kw):
        e = {
            "kind": "human",
            "id": "alice",
            "command": SET,
            "params_commitment": commitment(params),
            "j_kind": "human",
            "j_command": "sign.set_profile",
            "j_major": 1,
            "class": "A0",
            "branch": "main",
            "source": "handler",
            "promoted_from": None,
            "hash": entry_hash(n),
        }
        e.update(kw)
        return e

    first = entry(1, set_xqa)

    def with_body(**kw):
        e = json.loads(json.dumps(good))
        for k, v in kw.items():
            if v is None:
                del e["body"][k]
            else:
                e["body"][k] = v
        return e

    # Lean with agent_writes changed: canonical, correctly hashed, but not a document
    # TRUST.PROFILE.DEFS could produce (param_forbidden on read).
    forged_lean = json.loads(lean["document"])
    forged_lean["params"]["agent_writes"] = "proposals_only"
    forged_text = jcs(forged_lean).decode("ascii")
    not_canonical = xqa["document"].replace('{"name"', '{ "name"', 1)
    if not_canonical == xqa["document"]:
        raise SystemExit("could not build the non-canonical document")
    approved = {
        "type": "RequestApproved",
        "body": {
            "approved_by": {"id": "alice", "kind": "human"},
            "command": SET,
            "decided_at_head": entry_hash(0),
            "params_commitment": commitment(set_xqa),
            "record_digest": sha(b"record"),
            "request": "0190f5c2-0000-7000-8000-000000000001",
            "requested_by": {"id": "bot", "kind": "agent"},
        },
    }

    accepted = {"expect": "accepted", "active": xqa["digest"], "origin_after": "set"}
    refused = {"expect": "refused", "active": lean["digest"], "origin_after": "default"}
    forged = [
        dict(name="valid", prior=[], entry=first, events=[good], **accepted),
        dict(name="other_command", prior=[], entry=entry(1, set_xqa, command="settings.set_admin_value@1"), events=[good], **refused),
        dict(name="other_major", prior=[], entry=entry(1, set_xqa, command="sign.set_profile@2"), events=[good], **refused),
        dict(name="show_command", prior=[], entry=entry(1, set_xqa, command="sign.show_profile@1"), events=[good], **refused),
        dict(name="journal_other_command", prior=[], entry=entry(1, set_xqa, j_command="settings.set_admin_value"), events=[good], **refused),
        dict(name="agent_origin", prior=[], entry=entry(1, set_xqa, kind="agent", j_kind="agent"), events=[good], **refused),
        dict(name="journal_kind_agent", prior=[], entry=entry(1, set_xqa, j_kind="agent"), events=[good], **refused),
        dict(name="script_origin", prior=[], entry=entry(1, set_xqa, kind="script", j_kind="script"), events=[good], **refused),
        dict(name="system_origin", prior=[], entry=entry(1, set_xqa, kind="system", j_kind="system"), events=[good], **refused),
        dict(name="not_a0", prior=[], entry=entry(1, set_xqa, **{"class": "W2"}), events=[good], **refused),
        dict(name="bus_added", prior=[], entry=entry(1, set_xqa, source="bus_route"), events=[good], **refused),
        # Maintainer decision on #378: a profile change is a direct human action, never an
        # approved agent request.
        dict(name="approved_agent_request", prior=[], entry=first, events=[good, approved], detail="request", **refused),
        # Only main counts; promoted copies are refused.
        dict(
            name="side_branch_ignored",
            prior=[],
            entry=entry(1, set_xqa, branch="feature/qa"),
            events=[good],
            expect="accepted",
            active=lean["digest"],
            origin_after="default",
        ),
        dict(name="promoted_copy", prior=[], entry=entry(1, set_xqa, promoted_from=entry_hash(9)), events=[good], detail="promoted", **refused),
        dict(
            name="spliced_into_other_parameters",
            prior=[],
            entry=entry(1, set_lean),
            events=[good],
            **refused,
        ),
        dict(name="digest_mismatch", prior=[], entry=first, events=[with_body(new_digest=assistive["digest"])], **refused),
        dict(name="not_canonical", prior=[], entry=first, events=[with_body(document=not_canonical)], **refused),
        dict(
            name="forbidden_parameter",
            prior=[],
            entry=first,
            events=[with_body(document=forged_text, name="lean", new_digest=sha(forged_text.encode("ascii")))],
            **refused,
        ),
        dict(name="name_mismatch", prior=[], entry=first, events=[with_body(name="lean")], **refused),
        dict(name="stale_old_digest", prior=[], entry=first, events=[with_body(old_digest=regulated["digest"])], **refused),
        dict(name="decided_at_unknown", prior=[], entry=first, events=[with_body(decided_at=entry_hash(7))], detail="decided_at", **refused),
        dict(
            name="unchanged_event",
            prior=[],
            entry=first,
            events=[with_body(document=lean["document"], name="lean", new_digest=lean["digest"])],
            **refused,
        ),
        dict(name="two_events", prior=[], entry=first, events=[good, good], **refused),
        dict(name="extra_field", prior=[], entry=first, events=[with_body(set_by="bob")], **refused),
        dict(name="missing_field", prior=[], entry=first, events=[with_body(decided_at=None)], **refused),
        dict(name="non_string_field", prior=[], entry=first, events=[with_body(name=1)], **refused),
        dict(
            name="other_events_ignored",
            prior=[],
            entry=entry(1, set_xqa, command="journal.create_branch@1", j_command="journal.create_branch", **{"class": "W2"}),
            events=[{"type": "BranchCreated", "body": {"name": "x"}}],
            expect="accepted",
            active=lean["digest"],
            origin_after="default",
        ),
        # The accepted event replayed in a later entry: its decision is no longer the last.
        dict(
            name="replayed_event",
            prior=[[first, [good]]],
            entry=entry(2, set_xqa),
            events=[good],
            expect="refused",
            active=xqa["digest"],
            origin_after="set",
        ),
        # Two sets decided on the same state: the second cannot apply (the commit stage's
        # compare-and-swap refuses it with conflict before commit; precommit_conflict).
        dict(
            name="concurrent_pair",
            prior=[[first, [good]]],
            entry=entry(2, set_assistive),
            events=[to_assistive],
            detail="decided_at",
            precommit=["conflict", "head_moved"],
            expect="refused",
            active=xqa["digest"],
            origin_after="set",
        ),
        # Lean -> xqa -> Lean, then the first payload again: its digests match the state again
        # (old Lean, new xqa), only the decision binding refuses it.
        dict(
            name="replay_after_return",
            prior=[[first, [good]], [entry(2, set_lean), [back]]],
            entry=entry(3, set_xqa),
            events=[good],
            detail="decided_at",
            precommit=["conflict", "head_moved"],
            expect="refused",
            active=lean["digest"],
            origin_after="set",
        ),
    ]
    return {
        "format": 2,
        "genesis": GENESIS,
        "tenant_file": TENANT_FILE,
        "lean": {"digest": lean["digest"], "document": lean["document"]},
        "regulated_xqa": {"digest": xqa["digest"], "document": xqa["document"]},
        "scenario": scenario,
        "forged": forged,
    }


def text():
    return json.dumps(build(), indent=2, sort_keys=True, ensure_ascii=True) + "\n"


def main():
    path = HERE / "cases.json"
    t = text()
    if "--check" in sys.argv[1:]:
        if not path.exists() or path.read_text(encoding="utf-8") != t:
            print("STALE cases.json")
            return 1
        return 0
    path.write_text(t, encoding="utf-8", newline="\n")
    print("wrote cases.json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
