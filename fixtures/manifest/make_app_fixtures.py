#!/usr/bin/env python3
"""Independent reference for ECO.APPS.MANIFEST.SCHEMA (app.toml v1 catalogues).

Writes the miniature catalogues the `fid-app-manifest` tests read from fixtures/manifest/:

- registry-manifest.json (and registry-majors.json / registry-unsorted.json, two majors of
  one command in manifest order and reversed): a CORE.REG.MAN manifest {format: 1, quantity_catalogue_digest, commands},
  each command a CORE.REG.DEF.FIELD definition plus definition_digest = SHA-256 of the RFC 8785
  form of the definition without that field, sorted by (id, major), canonical bytes;
- settings.json: a CORE.STO.SET `fid-settings-manifest/1` document (one admin setting, i.e. a
  setting only the A0 command `settings.set_admin_value@1` may change);
- panels.json / layouts.json: the VIZ.SHELL.DOCK.HOST / LAYOUT catalogue exports
  (`fid.panels` v1, `fid.layouts` v1), reduced to the fields the loader reads (`id`);
- templates.json: a provisional `fid.templates` v1 list of RPT.MODEL template IDs (RPT.MODEL
  publishes no catalogue format yet).

The app manifests (*.toml), apps.toml and the seed file are hand-written, and the expected
findings in crates/fid-app-manifest/tests are hand-labelled from the spec rules; nothing here is
produced by the Rust code.

Stdlib only. `--check` regenerates in memory and fails if any committed file differs.
"""

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAN = ROOT / "fixtures" / "manifest"
QCD = "sha256:" + "ab" * 32

# Declared MCP projection per class, as CORE.REG.DEF.FIELD writes it (`never` for M1, S1, A0).
DECLARED = {
    "R0": "exposed", "R1": "exposed", "I1": "exposed", "W1": "exposed", "W2": "exposed",
    "W3": "request_only", "X1": "request_only", "D1": "request_only",
    "M1": "never", "S1": "never", "A0": "never",
}


def canon(v):
    """RFC 8785 for the values used here: ASCII keys, small integers, strings, bools."""
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def definition(name, major, side_effect):
    d = {
        "id": name,
        "major": major,
        "title": f"Fixture {name}@{major}",
        "description": f"Fixture definition of class {side_effect}.",
        "params_schema": {"type": "object", "properties": {}},
        "result_schema": {"type": "object"},
        "side_effect": side_effect,
        "mcp_projection": DECLARED[side_effect],
        "determinism": "D0",
        "idempotency": "natural",
        "fs_effect": "none",
        "undo": "not_undoable",
        "preconditions": [],
        "effects": [],
        "algorithms": [],
        "exposure": ["cli", "mcp", "python"],
        "lifecycle": {"state": "active"},
        "human_only": side_effect in ("M1", "S1", "A0"),
        "web": "available",
    }
    d["definition_digest"] = sha(canon(d))
    return d


# (name, major, class). Namespaces: cmm (holds the M1 command of AC-3), project, report,
# settings, view.
COMMANDS = [
    ("cmm.run_program", 1, "M1"),
    ("cmm.show_status", 1, "R0"),
    ("project.open_project", 1, "W2"),
    ("report.generate_report", 1, "R1"),
    ("report.show_report", 1, "R0"),
    ("settings.set_value", 1, "W2"),
    ("view.apply_layout", 1, "W2"),
]


def registry_manifest():
    defs = [definition(n, m, c) for (n, m, c) in sorted(COMMANDS, key=lambda t: (t[0].encode(), t[1]))]
    return canon({"format": 1, "quantity_catalogue_digest": QCD, "commands": defs})


def registry_majors(order):
    """Two majors of one command, in manifest order (major 2 before 10) or reversed. The
    manifest order is numeric on the major; the wire strings would sort `@10` first."""
    defs = [definition("report.show_report", m, "R0") for m in order]
    return canon({"format": 1, "quantity_catalogue_digest": QCD, "commands": defs})


def settings_manifest():
    settings = [
        {
            "admin": True,
            "default": [],
            "key": "net.allowed_roots",
            "owner": "net",
            "schema": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 1024},
                       "maxItems": 64, "uniqueItems": True},
            "scopes": ["machine"],
        },
        {
            "admin": False,
            "default": 2,
            "key": "report.decimal_places",
            "owner": "report",
            "schema": {"type": "integer", "minimum": 0, "maximum": 9},
            "scopes": ["project", "user"],
        },
        {
            "admin": False,
            "default": True,
            "key": "ui.show_tips",
            "owner": "ui",
            "schema": {"type": "boolean"},
            "scopes": ["user"],
        },
    ]
    return canon({"format": "fid-settings-manifest/1", "settings": settings})


def panels():
    rows = [
        {"crate": "fid-report", "id": "report.outline", "title": "panel.report.outline.title"},
        {"crate": "fid-report", "id": "report.preview", "title": "panel.report.preview.title"},
        {"crate": "fid-shell", "id": "view.viewport", "title": "panel.view.viewport.title"},
    ]
    return canon({"format": "fid.panels", "version": 1, "panels": rows})


def layouts():
    rows = [
        {"id": "inspect.default", "name": "Inspector", "panels": ["view.viewport"]},
        {"id": "report.default", "name": "Reviewer", "panels": ["report.outline", "report.preview"]},
    ]
    return canon({"format": "fid.layouts", "version": 1, "layouts": rows})


def templates():
    rows = [
        {"id": "0193a5b0-7c1e-7d2a-9f00-3b5c6d7e8f90", "version": "1"},
        {"id": "0193a5b0-7c1e-7d2a-9f00-3b5c6d7e8f91", "version": "2"},
    ]
    return canon({"format": "fid.templates", "version": 1, "templates": rows})


def outputs():
    return {
        MAN / "registry-manifest.json": registry_manifest(),
        MAN / "registry-majors.json": registry_majors([2, 10]),
        MAN / "registry-unsorted.json": registry_majors([10, 2]),
        MAN / "settings.json": settings_manifest(),
        MAN / "panels.json": panels(),
        MAN / "layouts.json": layouts(),
        MAN / "templates.json": templates(),
    }


def main():
    check = "--check" in sys.argv[1:]
    bad = []
    for path, data in outputs().items():
        if check:
            if not path.exists() or path.read_bytes() != data:
                bad.append(path.relative_to(ROOT).as_posix())
        else:
            path.write_bytes(data)
    if bad:
        print("out of date (run fixtures/manifest/make_app_fixtures.py):", ", ".join(bad))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
