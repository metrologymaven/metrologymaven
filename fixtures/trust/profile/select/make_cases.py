#!/usr/bin/env python3
"""Independent reference for TRUST.PROFILE.SELECT (crate fid-profile-cmd). CC-BY-4.0 project data.

Writes cases.json next to this script:

- "lean" and "regulated_xqa": the canonical documents of the Lean built-in and of Regulated
  instantiated with fixtures/profile/regulated-with-xqa.json, read from the TRUST.PROFILE.DEFS
  fixtures (fixtures/profile/builtin/lean.json, fixtures/profile/expected/regulated-with-xqa.json)
  and hashed here with hashlib; the digests are cross-checked against fixtures/profile/digests.json.
- "scenario": the calls of AC-1 to AC-4 in order, with the actor (the principal kind the bus
  derives from the channel), the parameters and the expected events, result or bus error.
- "forged": fold cases for the ProfileSet event: an entry origin (principal kind and identity,
  command id@major, parameters commitment), the entry's events, and whether the fold accepts
  them, with the active profile after. Every way to forge or splice the event is refused.

Rules typed in from spec/trust/profile/select/NODE.md and the module docs of
crates/fid-profile-cmd (the leaf's Design made concrete):

- sign.set_profile@1 runs only for a human principal (else policy/human_only); it
  instantiates the profile and, if the digest differs from the active one, returns one
  ProfileSet event {document, name, new_digest, old_digest, params_commitment} and the
  result {new_digest, old_digest, status: changed}; the same digest gives status unchanged
  and no event (REQ-001 to REQ-003).
- sign.show_profile@1 returns {digest, document, name, origin}; origin is "default" (Lean)
  until a ProfileSet event is folded, then "set" (REQ-004, REQ-005).
- profile.* refusals become validation/schema whose detail starts with the profile code.
- The parameters commitment is SHA-256 of the RFC 8785 bytes of the call's parameters
  (ASCII keys and strings only here, so sorted keys without whitespace are exactly RFC 8785).
- The fold accepts a ProfileSet event only in a sign.set_profile@1 entry of a human whose
  parameters commitment is the body's, with a canonical document of that name and digest,
  whose old_digest is the active digest and differs from new_digest; at most one per entry.
  Other event types are ignored.

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


def event(new, old, params, name):
    return {
        "type": EVENT,
        "body": {
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
    lean_with_xqa = {"name": "lean", "params": TENANT_FILE}
    changed = lambda new, old: {"new_digest": new["digest"], "old_digest": old["digest"], "status": "changed"}

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
            "expect": {"events": [event(xqa, lean, set_xqa, "regulated")], "result": changed(xqa, lean)},
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
            "expect": {"events": [event(lean, xqa, set_lean, "lean")], "result": changed(lean, xqa)},
        },
        # Lean set explicitly is origin "set", not "default".
        {"actor": "human", "op": "show", "params": {}, "expect": show(lean, "set")},
        {"actor": "human", "op": "set", "params": {"name": "strict"}, "expect": {"error": ["validation", "schema"]}},
    ]

    good = event(xqa, lean, set_xqa, "regulated")
    origin = {"kind": "human", "id": "alice", "command": SET, "params_commitment": commitment(set_xqa)}

    def with_origin(**kw):
        o = dict(origin)
        o.update(kw)
        return o

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

    accepted = {"expect": "accepted", "active": xqa["digest"], "origin_after": "set"}
    refused = {"expect": "refused", "active": lean["digest"], "origin_after": "default"}
    forged = [
        dict(name="valid", prior=[], origin=origin, events=[good], **accepted),
        dict(name="other_command", prior=[], origin=with_origin(command="settings.set_admin_value@1"), events=[good], **refused),
        dict(name="other_major", prior=[], origin=with_origin(command="sign.set_profile@2"), events=[good], **refused),
        dict(name="show_command", prior=[], origin=with_origin(command="sign.show_profile@1"), events=[good], **refused),
        dict(name="agent_origin", prior=[], origin=with_origin(kind="agent"), events=[good], **refused),
        dict(name="script_origin", prior=[], origin=with_origin(kind="script"), events=[good], **refused),
        dict(name="system_origin", prior=[], origin=with_origin(kind="system"), events=[good], **refused),
        dict(
            name="spliced_into_other_parameters",
            prior=[],
            origin=with_origin(params_commitment=commitment(set_lean)),
            events=[good],
            **refused,
        ),
        dict(name="digest_mismatch", prior=[], origin=origin, events=[with_body(new_digest=assistive["digest"])], **refused),
        dict(name="not_canonical", prior=[], origin=origin, events=[with_body(document=not_canonical)], **refused),
        dict(
            name="forbidden_parameter",
            prior=[],
            origin=origin,
            events=[with_body(document=forged_text, name="lean", new_digest=sha(forged_text.encode("ascii")))],
            **refused,
        ),
        dict(name="name_mismatch", prior=[], origin=origin, events=[with_body(name="lean")], **refused),
        dict(name="stale_old_digest", prior=[], origin=origin, events=[with_body(old_digest=regulated["digest"])], **refused),
        dict(
            name="unchanged_event",
            prior=[],
            origin=origin,
            events=[with_body(document=lean["document"], name="lean", new_digest=lean["digest"])],
            **refused,
        ),
        dict(name="two_events", prior=[], origin=origin, events=[good, good], **refused),
        dict(name="extra_field", prior=[], origin=origin, events=[with_body(set_by="bob")], **refused),
        dict(name="missing_field", prior=[], origin=origin, events=[with_body(params_commitment=None)], **refused),
        dict(name="non_string_field", prior=[], origin=origin, events=[with_body(name=1)], **refused),
        dict(
            name="other_events_ignored",
            prior=[],
            origin=with_origin(command="journal.create_branch@1"),
            events=[{"type": "BranchCreated", "body": {"name": "x"}}],
            expect="accepted",
            active=lean["digest"],
            origin_after="default",
        ),
        # The accepted event replayed in a later entry: its old digest is no longer active.
        dict(
            name="replayed_event",
            prior=[[origin, [good]]],
            origin=origin,
            events=[good],
            expect="refused",
            active=xqa["digest"],
            origin_after="set",
        ),
    ]
    return {
        "format": 1,
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
