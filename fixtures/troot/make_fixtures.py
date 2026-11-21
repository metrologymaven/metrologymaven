#!/usr/bin/env python3
"""Independent reference for the TRUST.SIGN.TROOT fixtures (fid-sign-core, module troot).

Stdlib only. Nothing here runs or imports the Rust code under test. It reuses two other
independent stdlib generators of this repository:

- fixtures/dsse/make_fixtures.py: Ed25519 (RFC 8032) and ECDSA P-256 (RFC 6979) signing,
  DSSE PAE, SubjectPublicKeyInfo DER, PEM and fingerprints (published RFC test keys only);
- fixtures/pkg/make_fixtures.py: the deterministic Result Package ZIP writer and its
  `minimal` contents.

It writes:

- packages/*.zip: the `minimal` Result Package plus DSSE envelopes under `signatures/`;
- roots/*.json (trust roots), revoked/*.json (revocation files), keyring/*.json (known but
  untrusted keys), malformed/*.json (files the checker must refuse);
- cases.json: every check case (package, trust root, optional revocation and keyring
  files) with the expected outcome: an expected report file under expected/, or an error
  code and reason;
- expected/*.json: the expected `verify.check_signature@1` report, RFC 8785 canonical JSON.

The expected reports are computed here from the rules of spec/trust/sign/troot/NODE.md
(see classify()), from what this script knows it signed, never from the Rust code.

Signing times in the signature statements are signer-asserted claims (D-045: claimed values
never look measured), so the report names them `claimed_signed_at`.

TEST KEYS ONLY: anyone can sign with these keys; nothing signed with them proves anything.

Run:  python fixtures/troot/make_fixtures.py          (write)
      python fixtures/troot/make_fixtures.py --check  (fail if any file would change)
"""

import base64
import hashlib
import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent


def load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


dsse = load("troot_dsse_ref", "dsse/make_fixtures.py")
pkg = load("troot_pkg_ref", "pkg/make_fixtures.py")

PAYLOAD_TYPE = "application/vnd.in-toto+json"
STATEMENT_TYPE = "https://in-toto.io/Statement/v1"
# The signature predicate type is owned by TRUST.ESIG.BIND (not built yet), which may rename
# it; the checker pins this working name and refuses any other type.
SIG_PREDICATE_TYPE = "https://metrologymaven.io/signature/predicate/v1"
MAX_SIGNATURE_STATEMENT_BYTES = 1024 * 1024
MAX_SIGNATURES_PER_PACKAGE = 64

# ------------------------------------------------------------------ canonical JSON


def canon(v):
    """RFC 8785 for the data used here: strings, non-negative integers, lists, objects."""
    if isinstance(v, bool):
        raise TypeError("no booleans here")
    if isinstance(v, int):
        assert 0 <= v < 2**53
        return str(v)
    if isinstance(v, str):
        return pkg.jstr(v)
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(pkg.jstr(k) + ":" + canon(v[k]) for k in keys) + "}"
    raise TypeError(repr(v))


def cb(v):
    return canon(v).encode("utf-8")


# ------------------------------------------------------------------ RFC 3339 UTC times


def parse_time(t):
    """YYYY-MM-DDTHH:MM:SS[.fraction]Z -> comparable tuple. Mirrors the spec's rule that
    times are RFC 3339 UTC; only the forms used in these fixtures need to parse."""
    assert t.endswith("Z"), t
    body = t[:-1]
    frac = ""
    if "." in body:
        body, frac = body.split(".")
    date, clock = body.split("T")
    y, mo, d = (int(x) for x in date.split("-"))
    h, mi, s = (int(x) for x in clock.split(":"))
    nanos = int((frac + "000000000")[:9])
    return (y, mo, d, h, mi, s, nanos)


# ------------------------------------------------------------------ keys

ED_SEED = bytes.fromhex(dsse.RFC8032[0]["secret"])
ED_SPKI = dsse.spki_ed25519(dsse.ed_public(ED_SEED))
ED_FP = dsse.fingerprint(ED_SPKI)
ED_PEM = dsse.pem("PUBLIC KEY", ED_SPKI)
P_X = dsse.RFC6979_X
P_SPKI = dsse.spki_p256(dsse.p_public(P_X))
P_FP = dsse.fingerprint(P_SPKI)
P_PEM = dsse.pem("PUBLIC KEY", P_SPKI)
RSA_PEM = (ROOT / "dsse/keys/rsa-test.pub.pem").read_text()
RSA_FP = (ROOT / "dsse/keys/rsa-test.fingerprint").read_text().strip()

P_LABEL = "Test signer A (P-256, published RFC 6979 key)"
ED_LABEL = "Test signer B (Ed25519, published RFC 8032 key)"


def sign(which, message):
    if which == "p256":
        r, s = dsse.p_sign(P_X, message)
        return dsse.ecdsa_der(r, dsse.low_s(s))
    return dsse.ed_sign(ED_SEED, message)


FP = {"p256": P_FP, "ed25519": ED_FP}

# ------------------------------------------------------------------ the package

MBYTES, SBYTES, _ = pkg.build_package(pkg.MINIMAL)
STATEMENT_HEX = hashlib.sha256(SBYTES).hexdigest()


def package_with(envelopes, statement=True):
    """The `minimal` package with `signatures/<name>` entries added after statement.json
    (or without statement.json when `statement` is false)."""
    members = sorted(
        ((m["path"], m["media_type"], pkg.body_of(m)) for m in pkg.MINIMAL["members"]),
        key=lambda t: t[0].encode("utf-8"),
    )
    entries = [("manifest.json", "application/json", MBYTES)] + members
    if statement:
        entries.append(("statement.json", "application/json", SBYTES))
    for name, data in envelopes:
        entries.append(("signatures/" + name, "application/json", data))
    return pkg.build_zip(entries)


def sig_statement(subject_hex, predicate, predicate_type=SIG_PREDICATE_TYPE):
    return cb(
        {
            "_type": STATEMENT_TYPE,
            "subject": [{"name": "statement.json", "digest": {"sha256": subject_hex}}],
            "predicateType": predicate_type,
            "predicate": predicate,
        }
    )


def predicate(signed_at, fp=None, meaning="reviewed"):
    p = {"meaning_code": meaning, "signed_at": signed_at}
    if fp is not None:
        p["key_fingerprint"] = fp
    return p


def envelope(payload, sigs, payload_type=PAYLOAD_TYPE):
    """sigs: list of (keyid, signature bytes)."""
    return dsse.envelope(payload, payload_type, sigs)


# Each signature this script makes is described by a dict the oracle classifies:
#   keyid: the keyid written; signer: which key really signed ("p256"/"ed25519");
#   valid: whether the signature bytes are a correct signature by `signer`;
#   payload: the payload bytes; payload_type.
# Times are claims made by the (test) signer.
P_TIME = "2026-03-01T10:00:00Z"
ED_TIME = "2026-03-02T09:30:00.250Z"


def make_sig(signer, payload, keyid=None, corrupt=False, payload_type=PAYLOAD_TYPE):
    m = dsse.pae(payload_type.encode(), payload)
    s = sign(signer, m)
    if corrupt:
        s = s[:-1] + bytes([s[-1] ^ 0x01])
    kid = FP[signer] if keyid is None else keyid
    info = {"keyid": kid, "signer": signer, "valid": not corrupt, "payload": payload, "payload_type": payload_type}
    return (kid, s), info


def build_packages():
    pkgs = {}

    # two-signatures: one envelope per signer (the multi-signer form of TRUST.ESIG.MULTI),
    # each over a signature statement whose subject is statement.json.
    e = []
    for name, signer, t in (("0.dsse.json", "p256", P_TIME), ("1.dsse.json", "ed25519", ED_TIME)):
        payload = sig_statement(STATEMENT_HEX, predicate(t, FP[signer]))
        sig, info = make_sig(signer, payload)
        e.append((name, envelope(payload, [sig]), [info]))
    pkgs["two-signatures"] = e

    # faults: one envelope per rule the checker applies.
    f = []

    def add(name, payload, sigs_infos, payload_type=PAYLOAD_TYPE):
        f.append((name, envelope(payload, [s for s, _ in sigs_infos], payload_type), [i for _, i in sigs_infos]))

    # 0: payload is statement.json itself: valid, but no signing time.
    add("00.dsse.json", SBYTES, [make_sig("ed25519", SBYTES)])
    # 1: signature bytes corrupted.
    p = sig_statement(STATEMENT_HEX, predicate(P_TIME, P_FP))
    add("01.dsse.json", p, [make_sig("p256", p, corrupt=True)])
    # 2: a valid signature over a statement for some other package.
    p = sig_statement(hashlib.sha256(b"another package statement").hexdigest(), predicate(P_TIME, P_FP))
    add("02.dsse.json", p, [make_sig("p256", p)])
    # 3: the predicate names the other key's fingerprint.
    p = sig_statement(STATEMENT_HEX, predicate(P_TIME, ED_FP))
    add("03.dsse.json", p, [make_sig("p256", p)])
    # 4: no signed_at in the predicate.
    p = sig_statement(STATEMENT_HEX, {"meaning_code": "reviewed", "key_fingerprint": P_FP})
    add("04.dsse.json", p, [make_sig("p256", p)])
    # 5: empty keyid: the checker finds the key by trying the listed keys.
    p = sig_statement(STATEMENT_HEX, predicate(ED_TIME, ED_FP))
    add("05.dsse.json", p, [make_sig("ed25519", p, keyid="")])
    # 6: one envelope carrying two signatures over the same statement (no key_fingerprint).
    p = sig_statement(STATEMENT_HEX, predicate(P_TIME, None, "approved"))
    add("06.dsse.json", p, [make_sig("p256", p), make_sig("ed25519", p)])
    # 7: a keyid no file lists.
    p = sig_statement(STATEMENT_HEX, predicate(P_TIME))
    add("07.dsse.json", p, [make_sig("p256", p, keyid="0" * 64)])
    # 8: a payload type other than in-toto.
    p = sig_statement(STATEMENT_HEX, predicate(P_TIME, P_FP))
    add("08.dsse.json", p, [make_sig("p256", p, payload_type="text/plain")], "text/plain")
    # 9: signed_at that is not RFC 3339 UTC.
    p = sig_statement(STATEMENT_HEX, predicate("2026-03-01T10:00:00+01:00", P_FP))
    add("09.dsse.json", p, [make_sig("p256", p)])
    # 10: a signature statement with another predicate type.
    p = sig_statement(STATEMENT_HEX, predicate(P_TIME, P_FP), "https://example.invalid/other/v1")
    add("10.dsse.json", p, [make_sig("p256", p)])
    # 11: an unbound payload whose signature is also corrupt: refused on the binding, unverified.
    p = sig_statement(hashlib.sha256(b"another package statement").hexdigest(), predicate(P_TIME, P_FP))
    add("11.dsse.json", p, [make_sig("p256", p, corrupt=True)])
    pkgs["faults"] = f

    # junk-64: 64 envelopes (the per-package maximum), each an unbound non-JSON payload
    # with a garbage signature under a listed keyid: all INVALID payload_not_bound, unverified.
    j = []
    for i in range(MAX_SIGNATURES_PER_PACKAGE):
        payload = dsse.chain(b"junk-%d" % i, 2048)
        info = {"keyid": P_FP, "signer": "p256", "valid": False, "payload": payload, "payload_type": PAYLOAD_TYPE}
        j.append(("%02d.dsse.json" % i, envelope(payload, [(P_FP, dsse.chain(b"junk-sig-%d" % i, 72))]), [info]))
    pkgs["junk-64"] = j

    # too-many: two envelopes holding 65 signatures in all (one more than the maximum).
    junk = sig_statement(STATEMENT_HEX, predicate(P_TIME, P_FP))
    many = []
    for name, n in (("0.dsse.json", 40), ("1.dsse.json", 25)):
        sigs = [(P_FP, dsse.chain(b"%s-%d" % (name.encode(), k), 72)) for k in range(n)]
        many.append((name, envelope(junk, sigs), []))
    pkgs["too-many"] = many

    # no-statement: a valid signature statement, but the package has no statement.json.
    p = sig_statement(STATEMENT_HEX, predicate(P_TIME, P_FP))
    sig, info = make_sig("p256", p)
    pkgs["no-statement"] = [("0.dsse.json", envelope(p, [sig]), [info])]
    return pkgs


NO_STATEMENT = {"no-statement"}


# ------------------------------------------------------------------ trust files


def key(label, pem, fp, not_before, not_after=None):
    k = {"label": label, "spki_pem": pem, "fingerprint": fp, "not_before": not_before}
    if not_after is not None:
        k["not_after"] = not_after
    return k


P_KEY = key(P_LABEL, P_PEM, P_FP, "2026-01-01T00:00:00Z", "2026-12-31T23:59:59Z")
ED_KEY = key(ED_LABEL, ED_PEM, ED_FP, "2026-01-01T00:00:00Z")

ROOTS = {
    "both": {"keys": [P_KEY, ED_KEY]},
    "one": {"keys": [P_KEY]},
    # p256 expired before it signed; ed25519 valid only from 0.25 s after it signed.
    "expired": {
        "keys": [
            key(P_LABEL, P_PEM, P_FP, "2026-01-01T00:00:00Z", "2026-02-28T23:59:59Z"),
            key(ED_LABEL, ED_PEM, ED_FP, "2026-03-02T09:30:00.5Z"),
        ]
    },
    # Window edges are inclusive: p256 not_before and ed25519 not_after equal the claimed times.
    "edges": {
        "keys": [
            key(P_LABEL, P_PEM, P_FP, P_TIME),
            key(ED_LABEL, ED_PEM, ED_FP, "2026-01-01T00:00:00Z", "2026-03-02T09:30:00.25Z"),
        ]
    },
    "empty": {"keys": []},
}

REVOKED = {
    "revoked-before": {"entries": [{"fingerprint": P_FP, "revoked_at": "2026-02-15T00:00:00Z",
                                    "reason": "key compromise (test)"}]},
    "revoked-after": {"entries": [{"fingerprint": P_FP, "revoked_at": "2026-06-01T00:00:00Z",
                                   "reason": "rotated to a new key (test)"}]},
    "revoked-at-signing": {"entries": [{"fingerprint": P_FP, "revoked_at": P_TIME, "reason": "boundary (test)"}]},
    "revoked-ed": {"entries": [{"fingerprint": ED_FP, "revoked_at": "2026-01-15T00:00:00Z", "reason": "test"}]},
}

KEYRINGS = {
    "keyring": {"keys": [{"label": P_LABEL, "spki_pem": P_PEM, "fingerprint": P_FP},
                         {"label": ED_LABEL, "spki_pem": ED_PEM, "fingerprint": ED_FP}]},
}


def malformed_files():
    both = cb(ROOTS["both"])
    m = {}
    m["truncated.json"] = (both[: len(both) // 2], "not_json")
    m["fingerprint-mismatch.json"] = (cb({"keys": [key(P_LABEL, P_PEM, ED_FP, "2026-01-01T00:00:00Z")]}),
                                      "fingerprint_mismatch")
    m["duplicate-fingerprint.json"] = (cb({"keys": [P_KEY, P_KEY]}), "duplicate_fingerprint")
    m["duplicate-member.json"] = (b'{"keys":[],"keys":[]}', "not_json")
    m["unknown-field.json"] = (cb({"keys": [dict(P_KEY, trusted="yes")]}), "schema")
    m["missing-field.json"] = (cb({"keys": [{"label": P_LABEL, "spki_pem": P_PEM, "fingerprint": P_FP}]}),
                               "schema")
    m["bad-time.json"] = (cb({"keys": [key(P_LABEL, P_PEM, P_FP, "2026-02-30T00:00:00Z")]}), "time")
    m["local-time.json"] = (cb({"keys": [key(P_LABEL, P_PEM, P_FP, "2026-01-01T00:00:00+00:00")]}), "time")
    m["window-inverted.json"] = (cb({"keys": [key(P_LABEL, P_PEM, P_FP, "2026-06-01T00:00:00Z",
                                                 "2026-01-01T00:00:00Z")]}), "window")
    m["rsa-key.json"] = (cb({"keys": [key("RSA (unsupported)", RSA_PEM, RSA_FP, "2026-01-01T00:00:00Z")]}), "key")
    m["empty-label.json"] = (cb({"keys": [key("", P_PEM, P_FP, "2026-01-01T00:00:00Z")]}), "schema")
    m["not-object.json"] = (b"[]", "schema")
    m["revoked-duplicate.json"] = (cb({"entries": REVOKED["revoked-before"]["entries"] * 2}), "duplicate_fingerprint")
    m["revoked-bad-fingerprint.json"] = (cb({"entries": [{"fingerprint": P_FP.upper(), "revoked_at": P_TIME,
                                                          "reason": "x"}]}), "schema")
    m["keyring-fingerprint-mismatch.json"] = (cb({"keys": [{"label": P_LABEL, "spki_pem": P_PEM,
                                                            "fingerprint": ED_FP}]}), "fingerprint_mismatch")
    m["keyring-with-window.json"] = (cb({"keys": [P_KEY]}), "schema")
    return m


# ------------------------------------------------------------------ the oracle


def classify(info, envelope_bindings, root, revoked, keyring, as_of=None):
    """One signature -> its report entry (without index and envelope)."""
    out = {"claimed_keyid": info["keyid"], "notes": []}
    keys = [(k, True) for k in root["keys"]] + [(k, False) for k in (keyring or {"keys": []})["keys"]]
    binding = envelope_bindings
    if binding.get("unbound"):
        # Refused on the binding alone: no verification; the key is named only by keyid lookup.
        match = [k for k, _ in keys if info["keyid"] != "" and k["fingerprint"] == info["keyid"]]
        if match:
            out["fingerprint"] = match[0]["fingerprint"]
            out["key_label"] = match[0]["label"]
        out.update(status="INVALID", detail=binding["unbound"])
        return out
    if info["keyid"] == "":
        if len(keys) > 16:
            out.update(status="UNKNOWN_KEY", detail="keyid_missing")
            return out
        # Try every listed key in order; the first that verifies is the signer.
        found = [(k, t) for k, t in keys if k["fingerprint"] == FP[info["signer"]] and info["valid"]]
        if not found:
            out.update(status="UNKNOWN_KEY", detail="no_listed_key_verifies")
            return out
        k, trusted = found[0]
    else:
        match = [(k, t) for k, t in keys if k["fingerprint"] == info["keyid"]]
        if not match:
            out.update(status="UNKNOWN_KEY", detail="key_not_listed")
            return out
        k, trusted = match[0]
    out["fingerprint"] = k["fingerprint"]
    out["key_label"] = k["label"]
    # The signature verifies only if this key really made it and the bytes are intact.
    if not (info["valid"] and k["fingerprint"] == FP[info["signer"]]):
        out.update(status="INVALID", detail="signature_invalid")
        return out
    if binding.get("key_fingerprint") not in (None, k["fingerprint"]):
        out.update(status="INVALID", detail="key_fingerprint_mismatch")
        return out
    if "meaning_code" in binding:
        out["claimed_meaning_code"] = binding["meaning_code"]
    if "signed_at" in binding:
        out["claimed_signed_at"] = binding["signed_at"]
    out.update(time_rules(k, trusted, binding, revoked))
    if out["status"] != "REVOKED" and any(
            e["fingerprint"] == k["fingerprint"] for e in (revoked or {"entries": []})["entries"]):
        out["notes"].append("revoked_after_claimed_signing")
    if trusted and "not_after" in k and as_of is not None and parse_time(k["not_after"]) < parse_time(as_of):
        out["notes"].append("key_validity_ended_as_of")
    return out


def time_rules(k, trusted, binding, revoked):
    out = {}
    t = parse_time(binding["signed_at"]) if "signed_at" in binding else None
    rv = [e for e in (revoked or {"entries": []})["entries"] if e["fingerprint"] == k["fingerprint"]]
    if rv:
        if t is None:
            out.update(status="REVOKED", detail="revoked_no_signing_time")
            return out
        if t >= parse_time(rv[0]["revoked_at"]):
            out.update(status="REVOKED", detail="revoked_before_signing")
            return out
    if not trusted:
        out.update(status="VALID_UNTRUSTED", detail="key_not_trusted")
        return out
    if t is None:
        out.update(status="VALID_UNTRUSTED", detail="no_signing_time")
        return out
    if t < parse_time(k["not_before"]):
        out.update(status="EXPIRED", detail="before_not_before")
        return out
    if "not_after" in k and t > parse_time(k["not_after"]):
        out.update(status="EXPIRED", detail="after_not_after")
        return out
    out["status"] = "VALID_TRUSTED"
    return out


def binding_of(payload, payload_type, has_statement=True):
    """How an envelope's payload is bound to statement.json (computed from what was built)."""
    if not has_statement:
        return {"unbound": "statement_missing"}
    if payload_type != PAYLOAD_TYPE:
        return {"unbound": "payload_type"}
    if hashlib.sha256(payload).hexdigest() == STATEMENT_HEX:
        return {}
    if len(payload) > MAX_SIGNATURE_STATEMENT_BYTES:
        return {"unbound": "payload_too_large"}
    import json

    try:
        st = json.loads(payload)
    except ValueError:
        return {"unbound": "payload_not_bound"}
    if st["_type"] != STATEMENT_TYPE:
        return {"unbound": "payload_not_bound"}
    if st["predicateType"] != SIG_PREDICATE_TYPE:
        return {"unbound": "predicate_type"}
    subj = st["subject"]
    if not (len(subj) == 1 and subj[0]["name"] == "statement.json" and subj[0]["digest"]["sha256"] == STATEMENT_HEX):
        return {"unbound": "payload_not_bound"}
    pred = st["predicate"]
    b = {}
    if "signed_at" not in pred:
        return {"unbound": "signed_at_missing"}
    if not pred["signed_at"].endswith("Z"):
        return {"unbound": "signed_at_invalid"}
    b["signed_at"] = pred["signed_at"]
    if "key_fingerprint" in pred:
        b["key_fingerprint"] = pred["key_fingerprint"]
    if "meaning_code" in pred:
        b["meaning_code"] = pred["meaning_code"]
    return b


def report(envelopes, root, revoked, keyring, has_statement=True, as_of=None):
    if not envelopes:
        return {"package_status": "UNSIGNED", "signatures": [], "statement_sha256": STATEMENT_HEX}
    sigs = []
    for name, _, infos in envelopes:
        b = binding_of(infos[0]["payload"], infos[0]["payload_type"], has_statement)
        for info in infos:
            entry = classify(info, b, root, revoked, keyring, as_of)
            entry["index"] = len(sigs)
            entry["envelope"] = "signatures/" + name
            sigs.append(entry)
    out = {"package_status": "SIGNED", "signatures": sigs}
    if has_statement:
        out["statement_sha256"] = STATEMENT_HEX
    return out


# (case name, package, root, revoked, keyring, spec reference)
CASES = [
    ("ac1-both-trusted", "two-signatures", "both", None, None, "AC-1"),
    ("ac2-one-trusted", "two-signatures", "one", None, "keyring", "AC-2"),
    ("ac2-one-no-keyring", "two-signatures", "one", None, None, "AC-2, no keyring"),
    ("ac3-revoked-before", "two-signatures", "both", "revoked-before", None, "AC-3"),
    ("revoked-after", "two-signatures", "both", "revoked-after", None, "REQ-002 rotation"),
    ("revoked-at-signing", "two-signatures", "both", "revoked-at-signing", None, "REQ-002 boundary"),
    ("expired", "two-signatures", "expired", None, None, "REQ-003"),
    ("edges", "two-signatures", "edges", None, None, "REQ-002 inclusive window"),
    ("empty-root-keyring", "two-signatures", "empty", None, "keyring", "REQ-003"),
    ("faults-both", "faults", "both", None, None, "REQ-001..003"),
    ("faults-revoked-ed", "faults", "both", "revoked-ed", None, "REQ-003"),
    ("faults-keyring", "faults", "empty", None, "keyring", "REQ-003"),
    ("faults-one", "faults", "one", None, None, "REQ-003, key search"),
    ("no-statement", "no-statement", "both", None, None, "REQ-001"),
    ("unsigned", "unsigned", "both", None, None, "REQ-006"),
    ("junk-64", "junk-64", "both", None, None, "bounded work: unbound payloads are never verified"),
]

# Cases with a caller-supplied reference time (as_of): (case, package, root, revoked, as_of).
AS_OF_CASES = [
    ("as-of-after-p256-window", "two-signatures", "both", "revoked-after", "2027-06-01T00:00:00Z"),
    ("as-of-inside-window", "two-signatures", "both", None, "2026-06-01T00:00:00Z"),
]


def build():
    files = {}
    pkgs = build_packages()
    for name, envs in sorted(pkgs.items()):
        files["packages/%s.zip" % name] = package_with([(n, data) for n, data, _ in envs], name not in NO_STATEMENT)
    pkgs["unsigned"] = []
    files["packages/unsigned.zip"] = package_with([])
    for name, v in ROOTS.items():
        files["roots/%s.json" % name] = cb(v)
    for name, v in REVOKED.items():
        files["revoked/%s.json" % name] = cb(v)
    for name, v in KEYRINGS.items():
        files["keyring/%s.json" % name] = cb(v)
    mal = malformed_files()
    for name, (data, _) in mal.items():
        files["malformed/" + name] = data

    cases = []
    for name, p, root, rev, ring, ref in CASES:
        rep = report(pkgs[p], ROOTS[root], REVOKED.get(rev), KEYRINGS.get(ring), p not in NO_STATEMENT)
        files["expected/%s.json" % name] = cb(rep)
        c = {"name": name, "package": "packages/%s.zip" % p, "trust_root": "roots/%s.json" % root,
             "expect": "expected/%s.json" % name, "spec": ref}
        if rev:
            c["revoked"] = "revoked/%s.json" % rev
        if ring:
            c["keys"] = "keyring/%s.json" % ring
        cases.append(c)
    for name, p, root, rev, as_of in AS_OF_CASES:
        rep = report(pkgs[p], ROOTS[root], REVOKED.get(rev), None, True, as_of)
        files["expected/%s.json" % name] = cb(rep)
        c = {"name": name, "package": "packages/%s.zip" % p, "trust_root": "roots/%s.json" % root,
             "as_of": as_of, "expect": "expected/%s.json" % name, "spec": "review of #408: notes"}
        if rev:
            c["revoked"] = "revoked/%s.json" % rev
        cases.append(c)
    cases.append({"name": "too-many-signatures", "package": "packages/too-many.zip", "trust_root": "roots/both.json",
                  "error": {"code": "sig.malformed"}, "spec": "at most 64 signatures per package"})
    # Error cases: a malformed trust root, revocation file or keyring fails the command.
    for name, (_, reason) in sorted(mal.items()):
        stem = name[: -len(".json")]
        c = {"name": "malformed-" + stem, "package": "packages/two-signatures.zip",
             "error": {"code": "troot.malformed", "reason": reason}, "spec": "REQ-005 / AC-4"}
        if stem.startswith("revoked-"):
            c["trust_root"] = "roots/both.json"
            c["revoked"] = "malformed/" + name
        elif stem.startswith("keyring-"):
            c["trust_root"] = "roots/both.json"
            c["keys"] = "malformed/" + name
        else:
            c["trust_root"] = "malformed/" + name
        cases.append(c)
    files["cases.json"] = cb(cases)
    return files


def self_check():
    dsse.self_check()
    assert parse_time("2026-03-02T09:30:00.25Z") == parse_time(ED_TIME)
    assert parse_time("2026-03-02T09:30:00.5Z") > parse_time(ED_TIME)
    assert P_FP == (ROOT / "dsse/keys/p256-test.fingerprint").read_text().strip()
    assert ED_FP == (ROOT / "dsse/keys/ed25519-test.fingerprint").read_text().strip()
    assert base64.b64decode("".join(P_PEM.splitlines()[1:-1])) == P_SPKI


def main():
    self_check()
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, data in sorted(build().items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    if stale:
        print("stale fixtures: " + ", ".join(stale))
        return 1
    print("fixtures/troot: %s" % ("up to date" if check else "written"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
