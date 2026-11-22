#!/usr/bin/env python3
"""Generate, freshness-check, and capability-audit the format-plugin ABI artifacts
from the single IDL source `crates/fid-io-plugin/abi/format-plugin.idl.json`
(spec node IO.REG.PLUGIN.ABI, REQ-001, REQ-006, AC-1, AC-5).

Usage:
    python3 crates/fid-io-plugin/abi/gen_abi.py
    python3 crates/fid-io-plugin/abi/gen_abi.py --check
    python3 crates/fid-io-plugin/abi/gen_abi.py --audit
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
IDL_PATH = HERE / "format-plugin.idl.json"
GEN_DIR = HERE / "generated"
WIT_PATH = GEN_DIR / "format-plugin.wit"
MSG_PATH = GEN_DIR / "message-protocol.json"

FORBIDDEN_CAP_RE = re.compile(
    r"\b(wasi:filesystem|wasi:sockets|wasi:clocks|wasi:http|filesystem|socket|sockets|network|clock|clocks|document|journal|store|open_file|read_file|write_file)\b",
    re.IGNORECASE,
)


def canon_json_bytes(obj: object) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def idl_digest(idl: dict) -> str:
    return "sha256:" + hashlib.sha256(canon_json_bytes(idl)).hexdigest()


def render_wit(idl: dict) -> str:
    ver = idl["version"]
    pkg = idl["package"]
    dig = idl_digest(idl)
    lines = [
        f"// Generated from format-plugin.idl.json ({dig}). Do not edit by hand.",
        f"// Spec node IO.REG.PLUGIN.ABI (contract {ver['major']}.{ver['minor']}).",
        f"package {pkg}@{ver['major']}.{ver['minor']}.0;",
        "",
        "interface types {",
    ]
    for td in idl["types"]:
        kind = td["kind"]
        name = td["name"]
        if kind == "record":
            lines.append(f"    record {name} {{")
            for f in td["fields"]:
                lines.append(f"        {f['name']}: {f['type']},")
            lines.append("    }")
        elif kind == "variant":
            lines.append(f"    variant {name} {{")
            for c in td["cases"]:
                lines.append(f"        {c['name']}({c['payload']}),")
            lines.append("    }")
        else:
            raise ValueError(f"unsupported IDL type kind: {kind}")
        lines.append("")
    if lines[-1] == "":
        lines.pop()
    lines.append("}")
    lines.append("")
    lines.append("interface plugin {")
    type_names = [td["name"] for td in idl["types"]]
    lines.append(f"    use types.{{{', '.join(type_names)}}};")
    lines.append("")
    for ex in idl["exports"]:
        params = ", ".join(f"{p['name']}: {p['type']}" for p in ex["params"])
        lines.append(f"    {ex['name']}: func({params}) -> {ex['result']};")
    lines.append("}")
    lines.append("")
    lines.append("world format-plugin {")
    for imp in idl.get("imports", []):
        lines.append(f"    import {imp};")
    lines.append("    export types;")
    lines.append("    export plugin;")
    lines.append("}")
    return "\n".join(lines) + "\n"


def render_message_protocol(idl: dict) -> str:
    ver = idl["version"]
    dig = idl_digest(idl)
    messages = [
        {"tag": 1, "name": "DescriptorRequest", "direction": "host_to_plugin", "payload": None},
        {"tag": 2, "name": "DescriptorResponse", "direction": "plugin_to_host", "payload": "plugin-descriptor"},
        {"tag": 3, "name": "ReadRequest", "direction": "host_to_plugin", "payload": "read-request"},
        {"tag": 4, "name": "ReadRecord", "direction": "plugin_to_host", "payload": "plugin-record"},
        {"tag": 5, "name": "ReadEnd", "direction": "plugin_to_host", "payload": None},
        {"tag": 6, "name": "WriteRequest", "direction": "host_to_plugin", "payload": "write-request"},
        {"tag": 7, "name": "WriteResponse", "direction": "plugin_to_host", "payload": "list<u8>"},
        {"tag": 8, "name": "ErrorResponse", "direction": "plugin_to_host", "payload": "plugin-error"},
    ]
    doc = {
        "contract_version": {"major": ver["major"], "minor": ver["minor"]},
        "exports": [ex["name"] for ex in idl["exports"]],
        "format": "format-plugin-messages@1",
        "framing": idl["framing"],
        "imports": idl.get("imports", []),
        "messages": messages,
        "package": idl["package"],
        "source_idl_sha256": dig,
        "types": idl["types"],
    }
    return json.dumps(doc, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def audit_capabilities(idl: dict, wit_text: str, msg_text: str) -> list[str]:
    violations: list[str] = []
    if idl.get("imports"):
        violations.append(f"IDL declares non-empty imports: {idl['imports']!r}")
    for i, line in enumerate(wit_text.splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        if re.match(r"^import\b", stripped):
            violations.append(f"WIT line {i} declares forbidden import: {stripped}")
        if FORBIDDEN_CAP_RE.search(stripped):
            violations.append(f"WIT line {i} mentions forbidden capability token: {stripped}")
    msg = json.loads(msg_text)
    if msg.get("imports"):
        violations.append(f"message schema declares non-empty imports: {msg['imports']!r}")
    for ex in idl.get("exports", []):
        if FORBIDDEN_CAP_RE.search(ex["name"]):
            violations.append(f"export {ex['name']} mentions forbidden capability")
    return violations


def main() -> int:
    check_mode = "--check" in sys.argv[1:]
    audit_only = "--audit" in sys.argv[1:]

    idl = json.loads(IDL_PATH.read_text(encoding="utf-8"))
    wit_want = render_wit(idl)
    msg_want = render_message_protocol(idl)

    if audit_only:
        violations = audit_capabilities(idl, wit_want, msg_want)
        if violations:
            for v in violations:
                print(f"capability_violation: {v}", file=sys.stderr)
            return 1
        return 0

    if check_mode:
        errors: list[str] = []
        for path, want in ((WIT_PATH, wit_want), (MSG_PATH, msg_want)):
            if not path.is_file():
                errors.append(f"abi_generated_out_of_date: missing {path.relative_to(HERE)}")
                continue
            got = path.read_text(encoding="utf-8")
            if got != want:
                errors.append(f"abi_generated_out_of_date: {path.relative_to(HERE)} differs from IDL source")
        violations = audit_capabilities(idl, wit_want, msg_want)
        for v in violations:
            errors.append(f"capability_violation: {v}")
        if errors:
            for e in errors:
                print(e, file=sys.stderr)
            return 1
        return 0

    GEN_DIR.mkdir(parents=True, exist_ok=True)
    WIT_PATH.write_text(wit_want, encoding="utf-8")
    MSG_PATH.write_text(msg_want, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
