#!/usr/bin/env python3
"""Generate and verify fixtures for UNC.UEC.QIF (QIF 3.0 uncertainty statement mapping).

Reads valid canonical statements from fixtures/unc/stmt/valid/ and writes:
- fixtures/unc/qif/docs/uec0.qif
- fixtures/unc/qif/docs/uec1.qif
- fixtures/unc/qif/docs/uec2.qif
- fixtures/unc/qif/docs/uec3.qif
- fixtures/unc/qif/native_only.qif (AC-4)
- fixtures/unc/qif/conflict.qif (AC-5: native U differs from extension U)
- fixtures/unc/qif/unsupported_version.qif (AC-6: version unc-qif-stmt@9)

In --check mode, verifies all files match byte for byte and checks XML well-formedness
and schema conformance against schemas/unc/fx-uncertainty-statement-v1.xsd.
"""

from __future__ import annotations

import json
import struct
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from xml.sax.saxutils import escape

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent.parent
STMT_DIR = REPO / "fixtures/unc/stmt/valid"
XSD_PATH = REPO / "schemas/unc/fx-uncertainty-statement-v1.xsd"

QIF_NS = "http://qifstandards.org/xsd/qif3"
FX_NS = "https://metrologymaven.org/ns/qif-ext/v1"
EXT_VERSION = "unc-qif-stmt@1"


def hex_to_f64(h: str) -> float:
    return struct.unpack(">d", bytes.fromhex(h))[0]


def f64_repr(v: float) -> str:
    return f"{v:.17g}"


def elem(tag: str, text: str) -> str:
    return f"<{tag}>{escape(text)}</{tag}>"


def stmt_to_fx_xml(stmt: dict, version: str = EXT_VERSION) -> str:
    parts = [f'<fx:UncertaintyStatement xmlns:fx="{FX_NS}" version="{escape(version)}">']
    parts.append(elem("fx:Class", stmt["class"]))
    if "uec0_reason" in stmt:
        parts.append(elem("fx:Uec0Reason", stmt["uec0_reason"]))
    if "method" in stmt:
        parts.append(elem("fx:Method", stmt["method"]))
    if "unit" in stmt:
        parts.append(elem("fx:Unit", stmt["unit"]))
    if "u_std" in stmt:
        parts.append(elem("fx:UStd", stmt["u_std"]))
    if "expanded" in stmt:
        e = stmt["expanded"]
        parts.append(f"<fx:Expanded>{elem('fx:U', e['U'])}{elem('fx:K', e['k'])}</fx:Expanded>")
    if "interval" in stmt:
        iv = stmt["interval"]
        parts.append(
            "<fx:Interval>"
            + elem("fx:Low", iv["low"])
            + elem("fx:High", iv["high"])
            + elem("fx:P", iv["p"])
            + elem("fx:Kind", iv["kind"])
            + "</fx:Interval>"
        )
    if "model" in stmt:
        m = stmt["model"]
        parts.append(
            "<fx:Model>"
            + elem("fx:Id", m["id"])
            + elem("fx:Semver", m["semver"])
            + elem("fx:Digest", m["digest"])
            + "</fx:Model>"
        )
    if stmt.get("sources"):
        s_items = []
        for s in stmt["sources"]:
            s_xml = elem("fx:Kind", s["kind"]) + elem("fx:Id", s["id"]) + elem("fx:Digest", s["digest"])
            if s.get("unverified") == "true":
                s_xml += elem("fx:Unverified", "true")
            s_items.append(f"<fx:Source>{s_xml}</fx:Source>")
        parts.append(f"<fx:Sources>{''.join(s_items)}</fx:Sources>")
    if stmt.get("contributors"):
        c_items = []
        for c in stmt["contributors"]:
            c_xml = elem("fx:InputId", c["input_id"])
            if "sensitivity" in c:
                c_xml += elem("fx:Sensitivity", c["sensitivity"])
            c_xml += elem("fx:UContrib", c["u_contrib"]) + elem("fx:VarianceShare", c["variance_share"])
            c_items.append(f"<fx:Contributor>{c_xml}</fx:Contributor>")
        parts.append(f"<fx:Contributors>{''.join(c_items)}</fx:Contributors>")
    if "lpu_validated_by_mc" in stmt:
        parts.append(elem("fx:LpuValidatedByMc", stmt["lpu_validated_by_mc"]))
    if "nonlinearity" in stmt:
        nl = stmt["nonlinearity"]
        nl_xml = (
            elem("fx:Version", nl["version"])
            + elem("fx:Outcome", nl["outcome"])
            + elem("fx:V1", nl["v1"])
            + elem("fx:V2", nl["v2"])
        )
        if "ratio" in nl:
            nl_xml += elem("fx:Ratio", nl["ratio"])
        nl_xml += (
            elem("fx:ProbedInputs", nl["probed_inputs"])
            + elem("fx:ProbedPairs", nl["probed_pairs"])
            + elem("fx:CrossTermsTruncated", nl["cross_terms_truncated"])
        )
        if "reason" in nl:
            nl_xml += elem("fx:Reason", nl["reason"])
        parts.append(f"<fx:Nonlinearity>{nl_xml}</fx:Nonlinearity>")
    if "selection_rule" in stmt:
        parts.append(elem("fx:SelectionRule", stmt["selection_rule"]))
    if "estimate_convention" in stmt:
        parts.append(elem("fx:EstimateConvention", stmt["estimate_convention"]))
    prov = stmt["provenance"]
    p_xml = ""
    if prov.get("rule_sets"):
        p_xml += "<fx:RuleSets>" + "".join(elem("fx:RuleSet", r) for r in prov["rule_sets"]) + "</fx:RuleSets>"
    if "principal" in prov:
        p_xml += elem("fx:Principal", prov["principal"])
    if "principal_kind" in prov:
        p_xml += elem("fx:PrincipalKind", prov["principal_kind"])
    p_xml += elem("fx:Proposal", prov["proposal"])
    if "adopted_by" in prov:
        p_xml += elem("fx:AdoptedBy", prov["adopted_by"])
    if "supersedes" in prov:
        p_xml += elem("fx:Supersedes", prov["supersedes"])
    parts.append(f"<fx:Provenance>{p_xml}</fx:Provenance>")
    parts.append("</fx:UncertaintyStatement>")
    return "".join(parts)


def wrap_qif_doc(comment: str, native_lines: list[str], fx_xml: str | None) -> str:
    native_block = ""
    if native_lines:
        inner = "\n".join(f"          {line}" for line in native_lines)
        native_block = f"        <MeasurementUncertainty>\n{inner}\n        </MeasurementUncertainty>\n"
    attr_block = ""
    if fx_xml is not None:
        attr_block = (
            '        <Attributes n="1">\n'
            '          <AttributeUserData name="fx:UncertaintyStatement">\n'
            "            <UserDataXML>\n"
            f"              {fx_xml}\n"
            "            </UserDataXML>\n"
            "          </AttributeUserData>\n"
            "        </Attributes>\n"
        )
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        f"<!-- {comment} -->\n"
        f'<QIFDocument xmlns="{QIF_NS}" versionQIF="3.0.0" idMax="1">\n'
        "  <Results>\n"
        '    <MeasurementResultsSet n="1">\n'
        '      <MeasurementResults id="1">\n'
        f"{attr_block}"
        f"{native_block}"
        "      </MeasurementResults>\n"
        "    </MeasurementResultsSet>\n"
        "  </Results>\n"
        "</QIFDocument>\n"
    )


def native_lines_from_stmt(stmt: dict, override_u: float | None = None) -> list[str]:
    if stmt["class"] == "UEC-0":
        return []
    lines: list[str] = []
    if "unit" in stmt:
        lines.append(elem("Unit", stmt["unit"]))
    if "u_std" in stmt:
        lines.append(elem("CombinedUncertainty", f64_repr(hex_to_f64(stmt["u_std"]))))
    if "expanded" in stmt:
        u_val = override_u if override_u is not None else hex_to_f64(stmt["expanded"]["U"])
        k_val = hex_to_f64(stmt["expanded"]["k"])
        lines.append(elem("ExpandedUncertainty", f64_repr(u_val)))
        lines.append(elem("CoverageFactor", f64_repr(k_val)))
    if "interval" in stmt:
        iv = stmt["interval"]
        lines.append(elem("LowerBound", f64_repr(hex_to_f64(iv["low"]))))
        lines.append(elem("UpperBound", f64_repr(hex_to_f64(iv["high"]))))
        lines.append(elem("CoverageProbability", f64_repr(hex_to_f64(iv["p"]))))
    return lines


def desired_files() -> dict[Path, bytes]:
    uec0 = json.loads((STMT_DIR / "uec0_model_missing.json").read_text("utf-8"))
    uec1 = json.loads((STMT_DIR / "uec1_declared.json").read_text("utf-8"))
    uec2 = json.loads((STMT_DIR / "uec2_lpu.json").read_text("utf-8"))
    uec3 = json.loads((STMT_DIR / "uec3_iso15530_3.json").read_text("utf-8"))

    out: dict[Path, bytes] = {}
    for name, stmt in [("uec0", uec0), ("uec1", uec1), ("uec2", uec2), ("uec3", uec3)]:
        doc = wrap_qif_doc(
            f"UNC.UEC.QIF AC-3 fixture ({name.upper()}), synthetic, CC-BY-4.0. Generated by make_fixtures.py.",
            native_lines_from_stmt(stmt),
            stmt_to_fx_xml(stmt),
        )
        out[HERE / "docs" / f"{name}.qif"] = doc.encode("utf-8")

    # AC-4: native uncertainty values only (no fx:UncertaintyStatement extension).
    out[HERE / "native_only.qif"] = wrap_qif_doc(
        "UNC.UEC.QIF AC-4 fixture (native only, no extension), synthetic, CC-BY-4.0.",
        [
            elem("Unit", "si:m"),
            elem("CombinedUncertainty", "0.00000125"),
            elem("ExpandedUncertainty", "0.0000025"),
            elem("CoverageFactor", "2"),
        ],
        None,
    ).encode("utf-8")

    # AC-5: native U deliberately differs from extension U (> 1 ulp).
    out[HERE / "conflict.qif"] = wrap_qif_doc(
        "UNC.UEC.QIF AC-5 fixture (native U conflicts with extension U), synthetic, CC-BY-4.0.",
        native_lines_from_stmt(uec1, override_u=0.005),
        stmt_to_fx_xml(uec1),
    ).encode("utf-8")

    # AC-6: unsupported extension version unc-qif-stmt@9.
    out[HERE / "unsupported_version.qif"] = wrap_qif_doc(
        "UNC.UEC.QIF AC-6 fixture (unsupported extension version unc-qif-stmt@9), synthetic, CC-BY-4.0.",
        native_lines_from_stmt(uec1),
        stmt_to_fx_xml(uec1, version="unc-qif-stmt@9"),
    ).encode("utf-8")

    return out


def validate_xsd_and_docs(files: dict[Path, bytes]) -> bool:
    # Verify the published XSD itself is well-formed XML Schema 1.0.
    xsd_root = ET.fromstring(XSD_PATH.read_bytes())
    expected_order = [
        "Class", "Uec0Reason", "Method", "Unit", "UStd", "Expanded", "Interval",
        "Model", "Sources", "Contributors", "LpuValidatedByMc", "Nonlinearity",
        "SelectionRule", "EstimateConvention", "Provenance",
    ]
    xs_ns = "{http://www.w3.org/2001/XMLSchema}"
    seq_elems = xsd_root.findall(
        f".//{xs_ns}complexType[@name='UncertaintyStatementType']/{xs_ns}sequence/{xs_ns}element"
    )
    xsd_order = [e.attrib["name"] for e in seq_elems]
    if xsd_order != expected_order:
        print(f"XSD sequence mismatch: {xsd_order} != {expected_order}", file=sys.stderr)
        return False

    # Verify all docs/*.qif parse as XML and obey the XSD element order and fixed version.
    for path, content in sorted(files.items()):
        root = ET.fromstring(content)
        if path.parent.name == "docs":
            fx_elem = root.find(f".//{{{FX_NS}}}UncertaintyStatement")
            if fx_elem is None:
                print(f"MISSING fx:UncertaintyStatement in {path}", file=sys.stderr)
                return False
            if fx_elem.attrib.get("version") != EXT_VERSION:
                print(f"BAD version in {path}", file=sys.stderr)
                return False
            child_names = [c.tag.split("}", 1)[1] for c in fx_elem]
            indices = [expected_order.index(n) for n in child_names]
            if indices != sorted(indices) or len(indices) != len(set(indices)):
                print(f"XSD order violation in {path}: {child_names}", file=sys.stderr)
                return False
    return True


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
        if not ok or not validate_xsd_and_docs(desired):
            return 1
        print(f"ok: {len(desired)} UNC.UEC.QIF fixture files match and conform to XSD")
        return 0

    for path, content in sorted(desired.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    if not validate_xsd_and_docs(desired):
        return 1
    print(f"wrote {len(desired)} UNC.UEC.QIF fixture files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
