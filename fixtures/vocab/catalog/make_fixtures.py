#!/usr/bin/env python3
"""Generate and verify fixtures for ECO.RULES.VOCAB.CATALOG.

Writes fixtures/vocab/catalog/:
- messages.en.json (4 messages, one with plural variants `one` and `other`)
- none.toml (empty suppressions)
- suppress-valid.toml (valid suppressions for single and plural messages)
- suppress-unknown-id.toml (AC-3: unknown message ID `ui.ghost.message`)
- suppress-empty-reason.toml (AC-4: empty reason `reason = ""`)
- product-rules-fixture.toml (AC-2: VC001 claim rule fixture)
- expected-units.jsonl (independent Python canonical JSON Lines expectation for AC-1)

Run with --check in CI to verify committed fixtures match byte for byte.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

MESSAGES = {
    "locale": "en",
    "messages": [
        {
            "id": "ui.part.summary",
            "variants": {
                "one": "Loaded {certified_count} feature from {file_name}.",
                "other": "Loaded {certified_count} features from {file_name}.",
            },
            "source": {"file": "ui/part.ftl", "line": 12},
        },
        {
            "id": "report.footer.note",
            "text": "Certified results",
            "source": {"file": "report/footer.ftl", "line": 4},
        },
        {
            "id": "cli.align.done",
            "text": "Alignment completed for {part_id} with {{literal}} braces.",
            "source": {"file": "cli/align.ftl", "line": 27},
        },
        {
            "id": "dialog.confirm.title",
            "text": "Confirm release batch {batch_id}",
            "source": {"file": "ui/dialog.ftl", "line": 8},
        },
    ],
}

NONE_TOML = "# No suppressions.\n"

SUPPRESS_VALID_TOML = """\
[[suppress]]
id = "report.footer.note"
rule = "VC001"
reason = "Fixture-reviewed suppression for AC test."

[[suppress]]
id = "ui.part.summary"
rule = "VC001"
reason = "Plural suppression applies to all variants."
"""

SUPPRESS_UNKNOWN_ID_TOML = """\
[[suppress]]
id = "ui.ghost.message"
rule = "VC001"
reason = "Names a message ID absent from messages.en.json."
"""

SUPPRESS_EMPTY_REASON_TOML = """\
[[suppress]]
id = "report.footer.note"
rule = "VC001"
reason = ""
"""

PRODUCT_RULES_FIXTURE_TOML = """\
# Fixture product rule file for ECO.RULES.VOCAB.CATALOG AC-2 (CC-BY-4.0).
format = "fid-vocab-rules/1"
version = "catalog-fixture-1"

[[rule]]
id = "VC001"
kind = "claim"
severity = "error"
message = "Certification or conformance wording needs a standard designator in the same sentence."
words = ["certified", "compliant", "conformant", "validated", "traceable"]
qualifiers = ["designator"]
scope = "sentence"
examples.pass = [
    "Evaluated per ISO 1101:2017 and validated against the public reference set.",
]
examples.fail = [
    "Certified results",
]
"""

_PLACEHOLDER_RE = re.compile(r"\{\{|\}\}|\{[^\{\}\s]+\}")


def replace_placeholders(text: str) -> str:
    def _sub(m: re.Match[str]) -> str:
        tok = m.group(0)
        if tok in ("{{", "}}"):
            return tok
        return "{arg}"

    return _PLACEHOLDER_RE.sub(_sub, text)


def build_expected_units_jsonl() -> str:
    units: list[dict[str, object]] = []
    for msg in MESSAGES["messages"]:
        src = msg["source"]
        if "text" in msg:
            units.append(
                {
                    "col": 0,
                    "file": src["file"],
                    "id": msg["id"],
                    "line": src["line"],
                    "links": [],
                    "suppress": [],
                    "text": replace_placeholders(msg["text"]),
                }
            )
        else:
            for vk, vt in sorted(msg["variants"].items()):
                units.append(
                    {
                        "col": 0,
                        "file": src["file"],
                        "id": f"{msg['id']}#{vk}",
                        "line": src["line"],
                        "links": [],
                        "suppress": [],
                        "text": replace_placeholders(vt),
                    }
                )
    units.sort(key=lambda u: str(u["id"]))
    lines = [
        json.dumps(u, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        for u in units
    ]
    return "\n".join(lines) + "\n"


def desired_files() -> dict[Path, bytes]:
    return {
        HERE / "messages.en.json": (
            json.dumps(MESSAGES, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
        ).encode("utf-8"),
        HERE / "none.toml": NONE_TOML.encode("utf-8"),
        HERE / "suppress-valid.toml": SUPPRESS_VALID_TOML.encode("utf-8"),
        HERE / "suppress-unknown-id.toml": SUPPRESS_UNKNOWN_ID_TOML.encode("utf-8"),
        HERE / "suppress-empty-reason.toml": SUPPRESS_EMPTY_REASON_TOML.encode("utf-8"),
        HERE / "product-rules-fixture.toml": PRODUCT_RULES_FIXTURE_TOML.encode("utf-8"),
        HERE / "expected-units.jsonl": build_expected_units_jsonl().encode("utf-8"),
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv
    desired = desired_files()
    if check:
        ok = True
        for path, expected in sorted(desired.items()):
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_bytes() != expected:
                print(f"DRIFT: {path}", file=sys.stderr)
                ok = False
        if not ok:
            return 1
        print(f"ok: {len(desired)} ECO.RULES.VOCAB.CATALOG fixture files match")
        return 0

    for path, content in sorted(desired.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    print(f"wrote {len(desired)} ECO.RULES.VOCAB.CATALOG fixture files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
