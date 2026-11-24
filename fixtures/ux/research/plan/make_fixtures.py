#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.RESEARCH.PROTO and checks their hand labels.

Synthetic data with invented participant names, CC-BY-4.0. Python stdlib only. Run from any
directory:

    python fixtures/ux/research/plan/make_fixtures.py          # (re)write the fixtures
    python fixtures/ux/research/plan/make_fixtures.py --check  # fail if files differ or labels disagree

Produces:
- `key.csv` (invented synthetic participant names)
- `clean/` (AC-1: clean notes, session record with consent_ref, PNG with valid capture sidecar)
- `has-email/` (AC-2: RESEARCH-PII email)
- `has-name/` (AC-3: RESEARCH-PII name, matching a name in key.csv)
- `has-phone/` (RESEARCH-PII phone)
- `phone-dots/`, `phone-local/`, `phone-uk/`, `phone-de/` (RESEARCH-PII phone: `555.019.2834`,
  `555-0192`, `07700 900123`, `030 12345678`)
- `has-name-part/` (RESEARCH-PII name: a surname alone from key.csv)
- `screenshot/` (AC-4: RESEARCH-IMAGE-PROVENANCE, PNG without capture sidecar)
- `incumbent/` (RESEARCH-INCUMBENT)
- `missing-consent/` (RESEARCH-CONSENT-REF)
- `expected.json` (hand-labelled expectations, cross-checked by an independent Python oracle)
"""

import json
import pathlib
import re
import struct
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent


def tiny_png() -> bytes:
    """Minimal valid 1x1 opaque grey PNG (synthetic, CC-BY-4.0)."""
    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(tag + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
    idat = chunk(b"IDAT", zlib.compress(b"\x00\x80\x80\x80"))
    iend = chunk(b"IEND", b"")
    return sig + ihdr + idat + iend


PNG_BYTES = tiny_png()

KEY_CSV = """\
# Synthetic invented participant key for UX.RESEARCH.PROTO fixtures (CC-BY-4.0).
code,name
P-inq01-01,Jordan Kestrel
P-inq01-02,Nadia Oakhurst
"""

CLEAN_NOTES = """\
# Discovery inquiry notes (synthetic, CC-BY-4.0)

Protocol version: `ux-research-protocol@1`

- Participant `P-inq01-01` (persona `JOB.PER.01`) completed the review workflow in 420 s.
- Participant `P-inq01-02` (persona `JOB.PER.02`) described scan-to-CAD alignment in own words.
"""

HAS_EMAIL_NOTES = """\
# Inquiry notes with leaked email (synthetic, CC-BY-4.0)

Follow up with P-inq01-01 at synthetic.participant@example.invalid about fixture timing.
"""

HAS_NAME_NOTES = """\
# Inquiry notes with leaked name from key.csv (synthetic, CC-BY-4.0)

During the debrief, Jordan Kestrel asked for clearer tolerance-band labels.
"""

HAS_PHONE_NOTES = """\
# Inquiry notes with leaked phone number (synthetic, CC-BY-4.0)

Callback number recorded by mistake: +1-555-019-2834.
"""

PHONE_NOTES = {
    "phone-dots": "Callback recorded by mistake: 555.019.2834.",
    "phone-local": "Desk extension noted by mistake: 555-0192.",
    "phone-uk": "Mobile noted by mistake: 07700 900123.",
    "phone-de": "Office line noted by mistake: 030 12345678.",
}


def phone_notes(text: str) -> str:
    return f"# Inquiry notes with leaked phone number (synthetic, CC-BY-4.0)\n\n{text}\n"


HAS_NAME_PART_NOTES = """\
# Inquiry notes with a name part from key.csv (synthetic, CC-BY-4.0)

Kestrel preferred the compact tolerance view.
"""

INCUMBENT_NOTES = """\
---
incumbent-artefact: true
---
# Flagged incumbent artefact (synthetic, CC-BY-4.0)

This note is explicitly flagged as an incumbent artefact and must not be published.
"""


def js(v) -> bytes:
    return (json.dumps(v, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode("utf-8")


CLEAN_SESSION = {
    "consent_ref": "consent-inq01-01",
    "participant": "P-inq01-01",
    "persona": "JOB.PER.01",
    "protocol_version": "ux-research-protocol@1",
}

CLEAN_CAPTURE = {
    "command": "fid viz capture",
    "date": "2026-11-22",
    "format": "research-capture@1",
    "image": "screens/view-01.png",
}

MISSING_CONSENT_SESSION = {
    "participant": "P-inq01-02",
    "persona": "JOB.PER.02",
    "protocol_version": "ux-research-protocol@1",
}

# Hand-labelled expectations derived directly from UX.RESEARCH.PROTO Requirements, Design and AC-1..4.
LABELS = {
    "clean": {
        "root": "clean",
        "key": "key.csv",
        "exit": 0,
        "findings": [],
    },
    "has-email": {
        "root": "has-email",
        "key": "key.csv",
        "exit": 1,
        "findings": ["RESEARCH-PII email"],
    },
    "has-name": {
        "root": "has-name",
        "key": "key.csv",
        "exit": 1,
        "findings": ["RESEARCH-PII name"],
    },
    "has-phone": {
        "root": "has-phone",
        "key": "key.csv",
        "exit": 1,
        "findings": ["RESEARCH-PII phone"],
    },
    "phone-dots": {"root": "phone-dots", "key": "key.csv", "exit": 1, "findings": ["RESEARCH-PII phone"]},
    "phone-local": {"root": "phone-local", "key": "key.csv", "exit": 1, "findings": ["RESEARCH-PII phone"]},
    "phone-uk": {"root": "phone-uk", "key": "key.csv", "exit": 1, "findings": ["RESEARCH-PII phone"]},
    "phone-de": {"root": "phone-de", "key": "key.csv", "exit": 1, "findings": ["RESEARCH-PII phone"]},
    "has-name-part": {"root": "has-name-part", "key": "key.csv", "exit": 1, "findings": ["RESEARCH-PII name"]},
    "screenshot": {
        "root": "screenshot",
        "key": "key.csv",
        "exit": 1,
        "findings": ["RESEARCH-IMAGE-PROVENANCE shot.png"],
    },
    "incumbent": {
        "root": "incumbent",
        "key": "key.csv",
        "exit": 1,
        "findings": ["RESEARCH-INCUMBENT flagged.md"],
    },
    "missing-consent": {
        "root": "missing-consent",
        "key": "key.csv",
        "exit": 1,
        "findings": ["RESEARCH-CONSENT-REF session.json"],
    },
}


def expected_files():
    return {
        "key.csv": KEY_CSV.encode("utf-8"),
        "clean/notes.md": CLEAN_NOTES.encode("utf-8"),
        "clean/session.json": js(CLEAN_SESSION),
        "clean/screens/view-01.png": PNG_BYTES,
        "clean/screens/view-01.png.capture.json": js(CLEAN_CAPTURE),
        "has-email/notes.md": HAS_EMAIL_NOTES.encode("utf-8"),
        "has-name/notes.md": HAS_NAME_NOTES.encode("utf-8"),
        "has-phone/notes.md": HAS_PHONE_NOTES.encode("utf-8"),
        **{f"{case}/notes.md": phone_notes(text).encode("utf-8") for case, text in PHONE_NOTES.items()},
        "has-name-part/notes.md": HAS_NAME_PART_NOTES.encode("utf-8"),
        "screenshot/shot.png": PNG_BYTES,
        "incumbent/flagged.md": INCUMBENT_NOTES.encode("utf-8"),
        "missing-consent/session.json": js(MISSING_CONSENT_SESSION),
        "expected.json": js(LABELS),
    }


# Independent phone patterns (hand-written from the review of #451, not from the Rust detector):
# `+<cc>` international, 3-3-4 with `-`, space or `.`, a 3-4 local number with `-` or space,
# and a trunk-prefixed national number (`0` + 1..4 digits, then 1..3 more groups, 9+ digits).
PHONE_PATTERNS = [
    r"\+\d{1,3}(?:[-\s.]\d{2,5}){2,4}",
    r"(?<![\w.])\d{3}[-.\s]\d{3}[-.\s]\d{4}(?!\w)",
    r"(?<![\w.])\d{3}[-\s]\d{4}(?![\w]|\.\d)",
    r"(?<![\w.])0\d{1,4}(?:[-\s]\d{2,8}){1,3}(?![\w]|\.\d)(?<=\d)",
]


def oracle_check(root_dir: pathlib.Path, key_csv: pathlib.Path):
    names = []
    for raw in key_csv.read_text(encoding="utf-8").splitlines():
        ln = raw.strip()
        if not ln or ln.startswith("#") or ln.lower().startswith("code,"):
            continue
        _, name = ln.split(",", 1)
        names.append(name.strip().lower())
    findings = []
    for p in sorted(root_dir.rglob("*")):
        if p.is_dir():
            continue
        rel = p.relative_to(root_dir).as_posix()
        if p.suffix.lower() in (".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"):
            sc = p.parent / (p.name + ".capture.json")
            if not sc.is_file():
                findings.append(f"RESEARCH-IMAGE-PROVENANCE {rel}")
            else:
                doc = json.loads(sc.read_text(encoding="utf-8"))
                if doc.get("format") != "research-capture@1" or not str(doc.get("command", "")).startswith("fid "):
                    findings.append(f"RESEARCH-IMAGE-PROVENANCE {rel}")
            continue
        text = p.read_text(encoding="utf-8")
        if re.search(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", text):
            findings.append("RESEARCH-PII email")
        if any(re.search(pat, text) for pat in PHONE_PATTERNS):
            findings.append("RESEARCH-PII phone")
        low = text.lower()
        for nm in names:
            # The full name, or any part of it with 3+ characters, as whole words.
            words = [nm] + [w for w in nm.split() if len(w) >= 3]
            if any(re.search(r"(?<!\w)" + re.escape(w) + r"(?!\w)", low) for w in words):
                findings.append("RESEARCH-PII name")
        if "incumbent-artefact: true" in low:
            findings.append(f"RESEARCH-INCUMBENT {rel}")
        if rel.endswith(".json") and not rel.endswith(".capture.json"):
            doc = json.loads(text)
            if "participant" in doc and not doc.get("consent_ref"):
                findings.append(f"RESEARCH-CONSENT-REF {rel}")
    findings = sorted(set(findings))
    return {"exit": 1 if findings else 0, "findings": findings}


def main(argv):
    check = "--check" in argv
    if any(a not in ("--check",) for a in argv):
        sys.exit("usage: make_fixtures.py [--check]")

    files = expected_files()
    if check:
        errors = []
        for rel, want in sorted(files.items()):
            p = HERE / rel
            if not p.is_file():
                errors.append(f"missing {rel}")
            elif p.read_bytes() != want:
                errors.append(f"stale {rel}")
        for p in sorted(HERE.rglob("*")):
            if p.is_dir():
                continue
            rel = p.relative_to(HERE).as_posix()
            if rel != "make_fixtures.py" and rel not in files:
                errors.append(f"unexpected {rel}")
        for key, label in sorted(LABELS.items()):
            got = oracle_check(HERE / label["root"], HERE / label["key"])
            for field in ("exit", "findings"):
                if got[field] != label[field]:
                    errors.append(f"oracle disagrees on {key}.{field}: got {got[field]!r}, label {label[field]!r}")
        if errors:
            for e in errors:
                print(f"error: {e}", file=sys.stderr)
            sys.exit(1)
        print(f"fixtures/ux/research/plan: {len(LABELS)} cases, {len(files)} files ok")
        return

    for rel, data in sorted(files.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    print(f"wrote {len(files)} files across {len(LABELS)} cases")


if __name__ == "__main__":
    main(sys.argv[1:])
