#!/usr/bin/env python3
"""Deterministic fixture generator for IO.QIF.GEN.FLAT (AC-1..AC-6).

Generates pinned XSD bundles and golden JSON outputs under `fixtures/io/qif/gen/flat/`:
  - `ext_chain/` + `ext_chain.ir.json`: A extends B extends C (AC-1)
  - `anon/` + `anon.names.json`: two nested anonymous types (AC-2)
  - `collide/`: two anonymous paths colliding on `A_B_C_Anon` (AC-3)
  - `subst/`: abstract head `Shape` + 3 members in source order (AC-4)
  - `counts/` + `counts_bad/`: linked `n` count rule & unlinked `count` error (AC-5)
  - `cycle/`: base-type inheritance cycle `X -> Y -> X` (`ir_base_cycle`)
  - `identity/`: `xs:key`, `xs:keyref`, `xs:unique` compiled into identity tables

Usage: python3 fixtures/io/qif/gen/flat/make_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
NS = "http://example.org/qif-flat"
XS = "http://www.w3.org/2001/XMLSchema"
LICENCE = b"CC-BY-4.0 synthetic fixture for IO.QIF.GEN.FLAT.\n"


def sha256_hex(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def canon_json(obj: object) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def manifest_for(xsd_name: str, xsd_bytes: bytes) -> bytes:
    text = (
        'manifest_version = 1\n'
        'licence_path = "LICENSE.txt"\n'
        f'licence_sha256 = "{sha256_hex(LICENCE)}"\n\n'
        '[source]\n'
        'url = "https://example.org/qif-flat-fixtures.git"\n'
        'rev = "3333333333333333333333333333333333333333"\n\n'
        '[[file]]\n'
        f'path = "{xsd_name}"\n'
        f'bytes = {len(xsd_bytes)}\n'
        f'sha256 = "{sha256_hex(xsd_bytes)}"\n'
        'role = "xsd"\n'
    )
    return text.encode("utf-8")


def qn(local: str, ns: str | None = NS) -> dict[str, str]:
    if ns is None:
        return {"local": local}
    return {"local": local, "ns": ns}


def occ(mn: int, mx: int | str) -> dict[str, str]:
    return {"max": str(mx), "min": str(mn)}


EXT_CHAIN_XSD = f"""<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="{XS}" targetNamespace="{NS}" xmlns:t="{NS}" elementFormDefault="qualified">
  <xs:complexType name="C">
    <xs:sequence>
      <xs:element name="c1" type="xs:string"/>
    </xs:sequence>
    <xs:attribute name="id" type="xs:string" use="required"/>
  </xs:complexType>
  <xs:complexType name="B">
    <xs:complexContent>
      <xs:extension base="t:C">
        <xs:sequence>
          <xs:element name="b1" type="xs:int"/>
        </xs:sequence>
        <xs:attribute name="label" type="xs:string" use="optional"/>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
  <xs:complexType name="A">
    <xs:complexContent>
      <xs:extension base="t:B">
        <xs:sequence>
          <xs:element name="a1" type="xs:double"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
</xs:schema>
""".encode("utf-8")

ANON_XSD = f"""<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="{XS}" targetNamespace="{NS}" xmlns:t="{NS}" elementFormDefault="qualified">
  <xs:element name="Root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Outer">
          <xs:complexType>
            <xs:sequence>
              <xs:element name="Leaf" type="xs:string"/>
            </xs:sequence>
          </xs:complexType>
        </xs:element>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
""".encode("utf-8")

COLLIDE_XSD = f"""<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="{XS}" targetNamespace="{NS}" xmlns:t="{NS}" elementFormDefault="qualified">
  <xs:complexType name="A_B">
    <xs:sequence>
      <xs:element name="C">
        <xs:complexType>
          <xs:sequence>
            <xs:element name="x" type="xs:string"/>
          </xs:sequence>
        </xs:complexType>
      </xs:element>
    </xs:sequence>
  </xs:complexType>
  <xs:complexType name="A">
    <xs:sequence>
      <xs:element name="B">
        <xs:complexType>
          <xs:sequence>
            <xs:element name="C">
              <xs:complexType>
                <xs:sequence>
                  <xs:element name="y" type="xs:int"/>
                </xs:sequence>
              </xs:complexType>
            </xs:element>
          </xs:sequence>
        </xs:complexType>
      </xs:element>
    </xs:sequence>
  </xs:complexType>
</xs:schema>
""".encode("utf-8")

SUBST_XSD = f"""<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="{XS}" targetNamespace="{NS}" xmlns:t="{NS}" elementFormDefault="qualified">
  <xs:complexType name="ShapeBase" abstract="true"/>
  <xs:element name="Shape" type="t:ShapeBase" abstract="true"/>
  <xs:element name="Circle" type="t:ShapeBase" substitutionGroup="t:Shape"/>
  <xs:element name="Plane" type="t:ShapeBase" substitutionGroup="t:Shape"/>
  <xs:element name="Cylinder" type="t:ShapeBase" substitutionGroup="t:Shape"/>
</xs:schema>
""".encode("utf-8")

COUNTS_XSD = f"""<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="{XS}" targetNamespace="{NS}" xmlns:t="{NS}" elementFormDefault="qualified">
  <xs:complexType name="PointListType">
    <xs:sequence>
      <xs:element name="Point" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
    </xs:sequence>
    <xs:attribute name="n" type="xs:unsignedInt" use="required"/>
  </xs:complexType>
</xs:schema>
""".encode("utf-8")

COUNTS_BAD_XSD = f"""<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="{XS}" targetNamespace="{NS}" xmlns:t="{NS}" elementFormDefault="qualified">
  <xs:complexType name="BrokenCountType">
    <xs:sequence>
      <xs:element name="Single" type="xs:string" minOccurs="1" maxOccurs="1"/>
    </xs:sequence>
    <xs:attribute name="count" type="xs:unsignedInt" use="required"/>
  </xs:complexType>
</xs:schema>
""".encode("utf-8")

CYCLE_XSD = f"""<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="{XS}" targetNamespace="{NS}" xmlns:t="{NS}" elementFormDefault="qualified">
  <xs:complexType name="X">
    <xs:complexContent>
      <xs:extension base="t:Y"/>
    </xs:complexContent>
  </xs:complexType>
  <xs:complexType name="Y">
    <xs:complexContent>
      <xs:extension base="t:X"/>
    </xs:complexContent>
  </xs:complexType>
</xs:schema>
""".encode("utf-8")

IDENTITY_XSD = f"""<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="{XS}" targetNamespace="{NS}" xmlns:t="{NS}" elementFormDefault="qualified">
  <xs:complexType name="DocType">
    <xs:sequence>
      <xs:element name="Item" type="xs:string" maxOccurs="unbounded"/>
      <xs:element name="Ref" type="xs:string" maxOccurs="unbounded"/>
    </xs:sequence>
  </xs:complexType>
  <xs:element name="Doc" type="t:DocType">
    <xs:key name="ItemKey">
      <xs:selector xpath="t:Item"/>
      <xs:field xpath="@id"/>
    </xs:key>
    <xs:keyref name="ItemRef" refer="t:ItemKey">
      <xs:selector xpath="t:Ref"/>
      <xs:field xpath="@target"/>
    </xs:keyref>
    <xs:unique name="ItemNameUnique">
      <xs:selector xpath="t:Item"/>
      <xs:field xpath="@name"/>
    </xs:unique>
  </xs:element>
</xs:schema>
""".encode("utf-8")


def build_ext_chain_ir() -> dict:
    c_elem = {
        "kind": "element",
        "name": qn("c1"),
        "nillable": False,
        "occurs": occ(1, 1),
        "type": qn("string", XS),
    }
    b_elem = {
        "kind": "element",
        "name": qn("b1"),
        "nillable": False,
        "occurs": occ(1, 1),
        "type": qn("int", XS),
    }
    a_elem = {
        "kind": "element",
        "name": qn("a1"),
        "nillable": False,
        "occurs": occ(1, 1),
        "type": qn("double", XS),
    }
    id_attr = {
        "name": qn("id", None),
        "type": qn("string", XS),
        "use": "required",
    }
    label_attr = {
        "name": qn("label", None),
        "type": qn("string", XS),
        "use": "optional",
    }
    key_a = f"{{{NS}}}A"
    key_b = f"{{{NS}}}B"
    key_c = f"{{{NS}}}C"
    return {
        "count_rules": [],
        "elements": {},
        "identity": [],
        "names": {
            key_a: "A",
            key_b: "B",
            key_c: "C",
        },
        "schema_version": "qif-ir/v1",
        "substitution": {},
        "types": {
            key_a: {
                "abstract": False,
                "attributes": [id_attr, label_attr],
                "base": qn("B"),
                "facets": [],
                "kind": "complex",
                "mixed": False,
                "name": qn("A"),
                "particles": [c_elem, b_elem, a_elem],
                "source": {"col": 3, "file": "ext_chain.xsd", "line": 19},
            },
            key_b: {
                "abstract": False,
                "attributes": [id_attr, label_attr],
                "base": qn("C"),
                "facets": [],
                "kind": "complex",
                "mixed": False,
                "name": qn("B"),
                "particles": [c_elem, b_elem],
                "source": {"col": 3, "file": "ext_chain.xsd", "line": 9},
            },
            key_c: {
                "abstract": False,
                "attributes": [id_attr],
                "facets": [],
                "kind": "complex",
                "mixed": False,
                "name": qn("C"),
                "particles": [c_elem],
                "source": {"col": 3, "file": "ext_chain.xsd", "line": 3},
            },
        },
    }


def build_anon_names() -> dict[str, str]:
    return {
        f"{{{NS}}}Root_Anon": "RootAnon",
        f"{{{NS}}}Root_Outer_Anon": "RootOuterAnon",
    }


def build_all() -> dict[str, bytes]:
    bundles = [
        ("ext_chain", "ext_chain.xsd", EXT_CHAIN_XSD),
        ("anon", "anon.xsd", ANON_XSD),
        ("collide", "collide.xsd", COLLIDE_XSD),
        ("subst", "subst.xsd", SUBST_XSD),
        ("counts", "counts.xsd", COUNTS_XSD),
        ("counts_bad", "counts_bad.xsd", COUNTS_BAD_XSD),
        ("cycle", "cycle.xsd", CYCLE_XSD),
        ("identity", "identity.xsd", IDENTITY_XSD),
    ]
    out: dict[str, bytes] = {}
    for folder, xsd_name, xsd_bytes in bundles:
        out[f"{folder}/LICENSE.txt"] = LICENCE
        out[f"{folder}/{xsd_name}"] = xsd_bytes
        out[f"{folder}/qif-xsd.manifest.toml"] = manifest_for(xsd_name, xsd_bytes)

    out["ext_chain.ir.json"] = canon_json(build_ext_chain_ir())
    out["anon.names.json"] = canon_json(build_anon_names())
    return out


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    files = build_all()
    if check:
        drift = []
        for rel, expected in files.items():
            p = HERE / rel
            if not p.exists() or p.read_bytes() != expected:
                drift.append(rel)
        if drift:
            print(f"qif flat fixtures out of date: {', '.join(drift)}", file=sys.stderr)
            return 1
        print(f"qif flat fixtures: ok ({len(files)} files)")
        return 0

    for rel, content in files.items():
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(content)
    print(f"wrote {len(files)} qif flat fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
