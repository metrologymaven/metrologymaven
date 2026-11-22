#!/usr/bin/env python3
"""Generate and verify the IO.QIF.GEN.PARSE fixtures (AC-1..AC-6). CC-BY-4.0 project data.

All schemas below are hand-written synthetic fixtures (NOT upstream DMSC QIF files).
SHA-256 digests, manifests, `constructs.ir.json` (via Python's stdlib `xml.parsers.expat`),
and `qif3-counts.json` (via `count_decls.py`) are computed independently of `crates/fid-qif-gen`.

  constructs/        one instance of every supported XSD construct + constructs.ir.json  (AC-1)
  redefine/          redefine.xsd using xs:redefine                                      (AC-2)
  dangling/          dangling.xsd referencing type="q:Missing"                           (AC-3)
  cycle/             a.xsd <-> b.xsd mutual xs:include                                   (AC-4)
  doctype/           doctype.xsd with <!DOCTYPE>                                         (AC-5)
  qif3_bundle/       multi-file QIF-style schema bundle + qif3-counts.json               (AC-6)
  unpinned_include/  root.xsd including an unpinned file                                 (extra)

Run:
  python fixtures/io/qif/gen/parse/make_fixtures.py
  python fixtures/io/qif/gen/parse/make_fixtures.py --check
"""

import argparse
import hashlib
import json
import shutil
import sys
import xml.parsers.expat as expat
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import count_decls  # noqa: E402

XS_NS = "http://www.w3.org/2001/XMLSchema"
XML_NS = "http://www.w3.org/XML/1998/namespace"
LICENCE_PATH = "LICENSE.txt"
LICENCE = "Synthetic licence text for the IO.QIF.GEN.PARSE test fixtures (CC-BY-4.0).\n"
SOURCE_URL = "https://example.org/fid-test/parse-fixtures.git"
SOURCE_REV = "1111111111111111111111111111111111111111"

CONSTRUCTS_XSD = """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:t="urn:fid:test:constructs"
           targetNamespace="urn:fid:test:constructs"
           elementFormDefault="qualified"
           attributeFormDefault="unqualified"
           version="1.0"
           xml:lang="en">
  <xs:annotation>
    <xs:documentation>Synthetic fixture covering every supported XSD construct.</xs:documentation>
    <xs:appinfo>generator-test</xs:appinfo>
  </xs:annotation>

  <xs:simpleType name="ColorEnum">
    <xs:annotation>
      <xs:documentation>Enumerated colour token.</xs:documentation>
    </xs:annotation>
    <xs:restriction base="xs:token">
      <xs:enumeration value="red"/>
      <xs:enumeration value="green"/>
      <xs:enumeration value="blue"/>
      <xs:whiteSpace value="collapse" fixed="true"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:simpleType name="CodeString">
    <xs:restriction base="xs:string">
      <xs:pattern value="[A-Z]{2}-[0-9]{4}"/>
      <xs:minLength value="7"/>
      <xs:maxLength value="7"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:simpleType name="FixedTag">
    <xs:restriction base="xs:string">
      <xs:length value="4"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:simpleType name="BoundedDecimal">
    <xs:restriction base="xs:decimal">
      <xs:minInclusive value="-100.0"/>
      <xs:maxInclusive value="100.0"/>
      <xs:totalDigits value="8"/>
      <xs:fractionDigits value="3"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:simpleType name="StrictPositiveDouble">
    <xs:restriction base="xs:double">
      <xs:minExclusive value="0.0"/>
      <xs:maxExclusive value="1000.0"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:simpleType name="DoubleList">
    <xs:list itemType="xs:double"/>
  </xs:simpleType>

  <xs:simpleType name="InlineList">
    <xs:list>
      <xs:simpleType>
        <xs:restriction base="xs:integer">
          <xs:minInclusive value="0"/>
        </xs:restriction>
      </xs:simpleType>
    </xs:list>
  </xs:simpleType>

  <xs:simpleType name="CodeOrInt">
    <xs:union memberTypes="t:CodeString xs:integer">
      <xs:simpleType>
        <xs:restriction base="xs:string">
          <xs:enumeration value="AUTO"/>
        </xs:restriction>
      </xs:simpleType>
    </xs:union>
  </xs:simpleType>

  <xs:simpleType name="InlineBaseRestricted">
    <xs:restriction>
      <xs:simpleType>
        <xs:restriction base="xs:string">
          <xs:minLength value="1"/>
        </xs:restriction>
      </xs:simpleType>
      <xs:maxLength value="16"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:attribute name="GlobalTag" type="t:FixedTag" default="NONE"/>

  <xs:attributeGroup name="AuditAttrs">
    <xs:annotation>
      <xs:documentation>Shared audit attributes.</xs:documentation>
    </xs:annotation>
    <xs:attribute name="version" type="xs:string" fixed="1"/>
    <xs:attribute ref="t:GlobalTag" use="optional"/>
  </xs:attributeGroup>

  <xs:group name="MetaGroup">
    <xs:annotation>
      <xs:documentation>Shared metadata sequence.</xs:documentation>
    </xs:annotation>
    <xs:sequence>
      <xs:element name="Label" type="xs:string" minOccurs="0" maxOccurs="1" default="untitled"/>
      <xs:element name="Note" type="xs:string" minOccurs="0" maxOccurs="unbounded" nillable="true"/>
    </xs:sequence>
  </xs:group>

  <xs:complexType name="MeasuredValue">
    <xs:simpleContent>
      <xs:extension base="t:BoundedDecimal">
        <xs:attribute name="unit" type="xs:string" use="required"/>
      </xs:extension>
    </xs:simpleContent>
  </xs:complexType>

  <xs:complexType name="TightMeasuredValue">
    <xs:simpleContent>
      <xs:restriction base="t:MeasuredValue">
        <xs:maxInclusive value="50.0"/>
        <xs:attribute name="unit" type="xs:string" use="required" fixed="mm"/>
      </xs:restriction>
    </xs:simpleContent>
  </xs:complexType>

  <xs:complexType name="BaseItem" abstract="true">
    <xs:sequence>
      <xs:group ref="t:MetaGroup" minOccurs="0" maxOccurs="1"/>
      <xs:choice minOccurs="1" maxOccurs="1">
        <xs:element name="Scalar" type="t:MeasuredValue"/>
        <xs:element name="Vector" type="t:DoubleList"/>
      </xs:choice>
    </xs:sequence>
    <xs:attribute name="id" type="xs:ID" use="required"/>
    <xs:attributeGroup ref="t:AuditAttrs"/>
  </xs:complexType>

  <xs:complexType name="ExtendedItem">
    <xs:complexContent mixed="true">
      <xs:extension base="t:BaseItem">
        <xs:sequence>
          <xs:element ref="t:StatusFlag" minOccurs="0"/>
          <xs:element name="TagRef" type="xs:IDREF" minOccurs="0"/>
        </xs:sequence>
        <xs:attribute name="inlineAttr" use="optional">
          <xs:simpleType>
            <xs:restriction base="xs:string">
              <xs:maxLength value="8"/>
            </xs:restriction>
          </xs:simpleType>
        </xs:attribute>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>

  <xs:complexType name="RestrictedItem">
    <xs:complexContent>
      <xs:restriction base="t:BaseItem">
        <xs:sequence>
          <xs:choice>
            <xs:element name="Scalar" type="t:TightMeasuredValue"/>
          </xs:choice>
        </xs:sequence>
        <xs:attribute name="id" type="xs:ID" use="required"/>
      </xs:restriction>
    </xs:complexContent>
  </xs:complexType>

  <xs:complexType name="UnorderedHeader">
    <xs:all minOccurs="0" maxOccurs="1">
      <xs:element name="Author" type="xs:string" minOccurs="0"/>
      <xs:element name="SchemaRev" type="xs:string" fixed="3.0"/>
    </xs:all>
  </xs:complexType>

  <xs:element name="StatusFlag">
    <xs:simpleType>
      <xs:restriction base="xs:boolean"/>
    </xs:simpleType>
  </xs:element>

  <xs:element name="ItemHead" type="t:BaseItem" abstract="true"/>
  <xs:element name="ConcreteItem" type="t:ExtendedItem" substitutionGroup="t:ItemHead"/>

  <xs:element name="Document">
    <xs:annotation>
      <xs:documentation>Root document with identity constraints.</xs:documentation>
    </xs:annotation>
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Header" type="t:UnorderedHeader" minOccurs="0"/>
        <xs:element ref="t:ItemHead" minOccurs="0" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
    <xs:key name="ItemIdKey">
      <xs:selector xpath="t:ConcreteItem"/>
      <xs:field xpath="@id"/>
    </xs:key>
    <xs:keyref name="ItemIdRef" refer="t:ItemIdKey">
      <xs:selector xpath="t:ConcreteItem"/>
      <xs:field xpath="t:TagRef"/>
    </xs:keyref>
    <xs:unique name="UniqueLabel">
      <xs:selector xpath="t:ConcreteItem"/>
      <xs:field xpath="t:Label"/>
    </xs:unique>
  </xs:element>
</xs:schema>
"""

REDEFINE_XSD = """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="urn:fid:test:redefine"
           elementFormDefault="qualified">
  <xs:redefine schemaLocation="other.xsd"/>
</xs:schema>
"""

DANGLING_XSD = """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:q="urn:fid:test:dangling"
           targetNamespace="urn:fid:test:dangling"
           elementFormDefault="qualified">
  <xs:element name="Broken" type="q:Missing"/>
</xs:schema>
"""

CYCLE_A_XSD = """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:c="urn:fid:test:cycle"
           targetNamespace="urn:fid:test:cycle"
           elementFormDefault="qualified">
  <xs:include schemaLocation="b.xsd"/>
  <xs:complexType name="TypeA">
    <xs:sequence>
      <xs:element name="B" type="c:TypeB" minOccurs="0"/>
    </xs:sequence>
  </xs:complexType>
  <xs:element name="RootA" type="c:TypeA"/>
</xs:schema>
"""

CYCLE_B_XSD = """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:c="urn:fid:test:cycle"
           targetNamespace="urn:fid:test:cycle"
           elementFormDefault="qualified">
  <xs:include schemaLocation="a.xsd"/>
  <xs:complexType name="TypeB">
    <xs:sequence>
      <xs:element name="A" type="c:TypeA" minOccurs="0"/>
    </xs:sequence>
  </xs:complexType>
</xs:schema>
"""

DOCTYPE_XSD = """\
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xs:schema [
  <!ENTITY evil "boom">
]>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:fid:test:doctype">
  <xs:element name="X" type="xs:string"/>
</xs:schema>
"""

UNPINNED_INCLUDE_XSD = """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:fid:test:unpinned">
  <xs:include schemaLocation="not_in_manifest.xsd"/>
  <xs:element name="Root" type="xs:string"/>
</xs:schema>
"""

# Synthetic multi-file QIF 3.0-style bundle exercising includes, imports, substitution groups,
# units, primitives, features, characteristics, and identity constraints across 5 files.
QIF3_BUNDLE = {
    "QIFLibrary/Primitives.xsd": """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:q="urn:fid:test:qif3"
           targetNamespace="urn:fid:test:qif3"
           elementFormDefault="qualified"
           attributeFormDefault="unqualified">
  <xs:simpleType name="QIFIdType">
    <xs:restriction base="xs:unsignedInt">
      <xs:minInclusive value="1"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:simpleType name="QPIdType">
    <xs:restriction base="xs:string">
      <xs:pattern value="[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:simpleType name="ListDoubleType">
    <xs:list itemType="xs:double"/>
  </xs:simpleType>

  <xs:simpleType name="PointSimpleType">
    <xs:restriction base="q:ListDoubleType">
      <xs:length value="3"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:complexType name="PointType">
    <xs:simpleContent>
      <xs:extension base="q:PointSimpleType">
        <xs:attribute name="linearUnit" type="xs:token"/>
      </xs:extension>
    </xs:simpleContent>
  </xs:complexType>

  <xs:complexType name="UnitVectorType">
    <xs:simpleContent>
      <xs:extension base="q:PointSimpleType"/>
    </xs:simpleContent>
  </xs:complexType>

  <xs:attributeGroup name="IdentifiableAttrs">
    <xs:attribute name="id" type="q:QIFIdType" use="required"/>
  </xs:attributeGroup>
</xs:schema>
""",
    "QIFLibrary/Units.xsd": """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:q="urn:fid:test:qif3"
           targetNamespace="urn:fid:test:qif3"
           elementFormDefault="qualified"
           attributeFormDefault="unqualified">
  <xs:include schemaLocation="Primitives.xsd"/>

  <xs:simpleType name="LinearUnitNameType">
    <xs:restriction base="xs:token">
      <xs:enumeration value="mm"/>
      <xs:enumeration value="m"/>
      <xs:enumeration value="in"/>
    </xs:restriction>
  </xs:simpleType>

  <xs:complexType name="LinearValueType">
    <xs:simpleContent>
      <xs:extension base="xs:double">
        <xs:attribute name="linearUnit" type="q:LinearUnitNameType"/>
      </xs:extension>
    </xs:simpleContent>
  </xs:complexType>

  <xs:complexType name="PrimaryUnitsType">
    <xs:sequence>
      <xs:element name="LinearUnit" type="q:LinearUnitNameType"/>
    </xs:sequence>
  </xs:complexType>
</xs:schema>
""",
    "QIFLibrary/Features.xsd": """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:q="urn:fid:test:qif3"
           targetNamespace="urn:fid:test:qif3"
           elementFormDefault="qualified"
           attributeFormDefault="unqualified">
  <xs:include schemaLocation="Primitives.xsd"/>
  <xs:include schemaLocation="Units.xsd"/>

  <xs:complexType name="FeatureBaseType" abstract="true">
    <xs:sequence>
      <xs:element name="Name" type="xs:string" minOccurs="0"/>
    </xs:sequence>
    <xs:attributeGroup ref="q:IdentifiableAttrs"/>
  </xs:complexType>

  <xs:complexType name="CylinderFeatureType">
    <xs:complexContent>
      <xs:extension base="q:FeatureBaseType">
        <xs:sequence>
          <xs:element name="Location" type="q:PointType"/>
          <xs:element name="Axis" type="q:UnitVectorType"/>
          <xs:element name="Diameter" type="q:LinearValueType"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>

  <xs:complexType name="PlaneFeatureType">
    <xs:complexContent>
      <xs:extension base="q:FeatureBaseType">
        <xs:sequence>
          <xs:element name="Location" type="q:PointType"/>
          <xs:element name="Normal" type="q:UnitVectorType"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>

  <xs:element name="FeatureNominal" type="q:FeatureBaseType" abstract="true"/>
  <xs:element name="CylinderFeatureNominal" type="q:CylinderFeatureType" substitutionGroup="q:FeatureNominal"/>
  <xs:element name="PlaneFeatureNominal" type="q:PlaneFeatureType" substitutionGroup="q:FeatureNominal"/>
</xs:schema>
""",
    "QIFLibrary/Characteristics.xsd": """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:q="urn:fid:test:qif3"
           targetNamespace="urn:fid:test:qif3"
           elementFormDefault="qualified"
           attributeFormDefault="unqualified">
  <xs:include schemaLocation="Primitives.xsd"/>
  <xs:include schemaLocation="Units.xsd"/>

  <xs:complexType name="CharacteristicBaseType" abstract="true">
    <xs:sequence>
      <xs:element name="Name" type="xs:string" minOccurs="0"/>
      <xs:element name="FeatureNominalId" type="q:QIFIdType" minOccurs="0"/>
    </xs:sequence>
    <xs:attributeGroup ref="q:IdentifiableAttrs"/>
  </xs:complexType>

  <xs:complexType name="DiameterCharacteristicType">
    <xs:complexContent>
      <xs:extension base="q:CharacteristicBaseType">
        <xs:sequence>
          <xs:element name="TargetValue" type="q:LinearValueType"/>
          <xs:element name="Tolerance" type="q:LinearValueType"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>

  <xs:element name="CharacteristicNominal" type="q:CharacteristicBaseType" abstract="true"/>
  <xs:element name="DiameterCharacteristicNominal" type="q:DiameterCharacteristicType" substitutionGroup="q:CharacteristicNominal"/>
</xs:schema>
""",
    "QIFDocument.xsd": """\
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:q="urn:fid:test:qif3"
           targetNamespace="urn:fid:test:qif3"
           elementFormDefault="qualified"
           attributeFormDefault="unqualified"
           version="3.0.0">
  <xs:include schemaLocation="QIFLibrary/Primitives.xsd"/>
  <xs:include schemaLocation="QIFLibrary/Units.xsd"/>
  <xs:include schemaLocation="QIFLibrary/Features.xsd"/>
  <xs:include schemaLocation="QIFLibrary/Characteristics.xsd"/>

  <xs:group name="DocumentHeaderGroup">
    <xs:sequence>
      <xs:element name="QPId" type="q:QPIdType"/>
      <xs:element name="PrimaryUnits" type="q:PrimaryUnitsType" minOccurs="0"/>
    </xs:sequence>
  </xs:group>

  <xs:complexType name="QIFDocumentType">
    <xs:sequence>
      <xs:group ref="q:DocumentHeaderGroup"/>
      <xs:element ref="q:FeatureNominal" minOccurs="0" maxOccurs="unbounded"/>
      <xs:element ref="q:CharacteristicNominal" minOccurs="0" maxOccurs="unbounded"/>
    </xs:sequence>
    <xs:attribute name="versionQIF" type="xs:string" fixed="3.0.0"/>
  </xs:complexType>

  <xs:element name="QIFDocument" type="q:QIFDocumentType">
    <xs:key name="FeatureNominalKey">
      <xs:selector xpath="q:CylinderFeatureNominal|q:PlaneFeatureNominal"/>
      <xs:field xpath="@id"/>
    </xs:key>
    <xs:keyref name="CharacteristicFeatureRef" refer="q:FeatureNominalKey">
      <xs:selector xpath="q:DiameterCharacteristicNominal"/>
      <xs:field xpath="q:FeatureNominalId"/>
    </xs:keyref>
  </xs:element>
</xs:schema>
""",
}


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def manifest(files: dict[str, str]) -> bytes:
    lic = LICENCE.encode("utf-8")
    out = [
        "# Synthetic test manifest (IO.QIF.GEN.PARSE fixtures); written by make_fixtures.py.",
        "manifest_version = 1",
        f'licence_path = "{LICENCE_PATH}"',
        f'licence_sha256 = "{sha(lic)}"',
        "",
        "[source]",
        f'url = "{SOURCE_URL}"',
        f'rev = "{SOURCE_REV}"',
    ]
    for path in sorted(files):
        data = files[path].encode("utf-8")
        out += ["", "[[file]]", f'path = "{path}"', f"bytes = {len(data)}", f'sha256 = "{sha(data)}"', 'role = "xsd"']
    return ("\n".join(out) + "\n").encode("utf-8")


# ------------------------------------------------------------------ independent Python XSD -> IR builder

class Node:
    def __init__(self, ns: str | None, local: str, attrs: dict[tuple[str | None, str], str], scope: dict[str | None, str], pos: dict):
        self.ns = ns
        self.local = local
        self.attrs = attrs
        self.scope = scope
        self.pos = pos
        self.children: list[Node] = []
        self.text_parts: list[str] = []


def parse_xml_expat(file_id: str, xml_text: str) -> Node:
    """Parse XML with stdlib expat, tracking 1-based line/column and active namespace scope."""
    p = expat.ParserCreate(namespace_separator="\x01")
    ns_stack: list[dict[str | None, str]] = [{}]
    pending_ns: dict[str | None, str] = {}
    node_stack: list[Node] = []
    root: Node | None = None

    def start_ns(prefix, uri):
        pending_ns[prefix] = uri

    def split_name(raw: str) -> tuple[str | None, str]:
        if "\x01" in raw:
            ns, local = raw.split("\x01", 1)
            return ns, local
        return None, raw

    def start_el(name, raw_attrs):
        nonlocal root, pending_ns
        scope = dict(ns_stack[-1])
        for k, v in pending_ns.items():
            if v == "":
                scope.pop(k, None)
            else:
                scope[k] = v
        pending_ns = {}
        ns_stack.append(scope)
        ns, local = split_name(name)
        attrs = {split_name(k): v for k, v in raw_attrs.items()}
        pos = {"col": p.CurrentColumnNumber + 1, "file": file_id, "line": p.CurrentLineNumber}
        node = Node(ns, local, attrs, scope, pos)
        if node_stack:
            node_stack[-1].children.append(node)
        else:
            root = node
        node_stack.append(node)

    def end_el(_name):
        node_stack.pop()
        ns_stack.pop()

    def char_data(data):
        if node_stack:
            node_stack[-1].text_parts.append(data)

    p.StartNamespaceDeclHandler = start_ns
    p.StartElementHandler = start_el
    p.EndElementHandler = end_el
    p.CharacterDataHandler = char_data
    p.Parse(xml_text.encode("utf-8"), True)
    assert root is not None
    return root


def resolve_qname(raw: str, scope: dict[str | None, str]) -> dict:
    s = raw.strip()
    if ":" in s:
        prefix, local = s.split(":", 1)
        uri = XML_NS if prefix == "xml" else scope[prefix]
        return {"local": local, "ns": uri}
    return {"local": s, "ns": scope.get(None)}


def parse_bool(raw: str | None) -> bool:
    return raw is not None and raw.strip() in ("true", "1")


def parse_occurs(node: Node) -> dict:
    mn = int(node.attrs.get((None, "minOccurs"), "1").strip())
    mx_raw = node.attrs.get((None, "maxOccurs"), "1").strip()
    mx: int | str = "unbounded" if mx_raw == "unbounded" else int(mx_raw)
    return {"max": mx, "min": mn}


def collect_text(node: Node) -> str:
    parts = list(node.text_parts)
    for c in node.children:
        parts.append(collect_text(c))
    return "".join(parts)


def build_annotation(node: Node) -> dict | None:
    docs = []
    first_pos = None
    for c in node.children:
        if c.ns == XS_NS and c.local == "documentation":
            if first_pos is None:
                first_pos = c.pos
            docs.append(collect_text(c).strip())
    if not docs:
        return None
    return {"pos": first_pos or node.pos, "text": "\n".join(docs)}


def child_annotation(node: Node) -> dict | None:
    for c in node.children:
        if c.ns == XS_NS and c.local == "annotation":
            return build_annotation(c)
    return None


def build_facet(node: Node) -> dict:
    k = node.local
    raw = node.attrs[(None, "value")]
    if k in ("length", "minLength", "maxLength", "totalDigits", "fractionDigits"):
        val: int | str = int(raw.strip())
    elif k == "whiteSpace":
        val = raw.strip()
    else:
        val = raw
    fixed = False if k in ("enumeration", "pattern") else parse_bool(node.attrs.get((None, "fixed")))
    return {"annotation": child_annotation(node), "fixed": fixed, "kind": k, "pos": node.pos, "value": val}


def build_simple_type(node: Node, tns: str | None, top_level: bool) -> dict:
    name = {"local": node.attrs[(None, "name")].strip(), "ns": tns} if top_level else None
    variety = None
    for c in node.children:
        if c.ns != XS_NS or c.local == "annotation":
            continue
        if c.local == "restriction":
            base = resolve_qname(c.attrs[(None, "base")], c.scope) if (None, "base") in c.attrs else None
            inline_base = None
            facets = []
            for gc in c.children:
                if gc.ns == XS_NS and gc.local == "simpleType":
                    inline_base = build_simple_type(gc, tns, False)
                elif gc.ns == XS_NS and gc.local != "annotation":
                    facets.append(build_facet(gc))
            variety = {"base": base, "facets": facets, "inline_base": inline_base, "kind": "restriction", "pos": c.pos}
        elif c.local == "list":
            item_type = resolve_qname(c.attrs[(None, "itemType")], c.scope) if (None, "itemType") in c.attrs else None
            inline_item = None
            for gc in c.children:
                if gc.ns == XS_NS and gc.local == "simpleType":
                    inline_item = build_simple_type(gc, tns, False)
            variety = {"inline_item": inline_item, "item_type": item_type, "kind": "list", "pos": c.pos}
        elif c.local == "union":
            mtypes = [resolve_qname(t, c.scope) for t in c.attrs.get((None, "memberTypes"), "").split()]
            inline_members = [build_simple_type(gc, tns, False) for gc in c.children if gc.ns == XS_NS and gc.local == "simpleType"]
            variety = {"inline_members": inline_members, "kind": "union", "member_types": mtypes, "pos": c.pos}
    assert variety is not None
    return {"annotation": child_annotation(node), "name": name, "pos": node.pos, "variety": variety}


def build_attr_decl(node: Node, name: dict, use_kind: str, tns: str | None) -> dict:
    if (None, "type") in node.attrs:
        ty = {"kind": "named", "name": resolve_qname(node.attrs[(None, "type")], node.scope)}
    else:
        inline = next((build_simple_type(c, tns, False) for c in node.children if c.ns == XS_NS and c.local == "simpleType"), None)
        ty = {"kind": "inline_simple", "simple_type": inline} if inline is not None else {"kind": "any_simple_type"}
    return {
        "annotation": child_annotation(node),
        "default": node.attrs.get((None, "default")),
        "fixed": node.attrs.get((None, "fixed")),
        "name": name,
        "pos": node.pos,
        "type": ty,
        "use": use_kind,
    }


def build_attr_item(node: Node, tns: str | None, afd: str) -> dict:
    if node.local == "attributeGroup":
        return {
            "annotation": child_annotation(node),
            "kind": "group_ref",
            "pos": node.pos,
            "target": resolve_qname(node.attrs[(None, "ref")], node.scope),
        }
    use_kind = node.attrs.get((None, "use"), "optional").strip()
    if (None, "ref") in node.attrs:
        return {
            "annotation": child_annotation(node),
            "default": node.attrs.get((None, "default")),
            "fixed": node.attrs.get((None, "fixed")),
            "kind": "ref",
            "pos": node.pos,
            "target": resolve_qname(node.attrs[(None, "ref")], node.scope),
            "use": use_kind,
        }
    form = node.attrs.get((None, "form"), afd).strip()
    ns = tns if form == "qualified" else None
    name = {"local": node.attrs[(None, "name")].strip(), "ns": ns}
    return {"decl": build_attr_decl(node, name, use_kind, tns), "kind": "decl"}


def build_constraint(node: Node, tns: str | None) -> dict:
    refer = resolve_qname(node.attrs[(None, "refer")], node.scope) if (None, "refer") in node.attrs else None
    selector = next(c.attrs[(None, "xpath")] for c in node.children if c.ns == XS_NS and c.local == "selector")
    fields = [c.attrs[(None, "xpath")] for c in node.children if c.ns == XS_NS and c.local == "field"]
    return {
        "annotation": child_annotation(node),
        "fields": fields,
        "kind": node.local,
        "name": {"local": node.attrs[(None, "name")].strip(), "ns": tns},
        "pos": node.pos,
        "refer": refer,
        "selector": selector,
    }


def build_element_decl(node: Node, name: dict, tns: str | None, efd: str, afd: str, top_level: bool) -> dict:
    sg = resolve_qname(node.attrs[(None, "substitutionGroup")], node.scope) if top_level and (None, "substitutionGroup") in node.attrs else None
    is_abs = parse_bool(node.attrs.get((None, "abstract"))) if top_level else False
    if (None, "type") in node.attrs:
        ty = {"kind": "named", "name": resolve_qname(node.attrs[(None, "type")], node.scope)}
    else:
        ic = next((build_complex_type(c, tns, efd, afd, False) for c in node.children if c.ns == XS_NS and c.local == "complexType"), None)
        ist = next((build_simple_type(c, tns, False) for c in node.children if c.ns == XS_NS and c.local == "simpleType"), None)
        if ic is not None:
            ty = {"complex_type": ic, "kind": "inline_complex"}
        elif ist is not None:
            ty = {"kind": "inline_simple", "simple_type": ist}
        else:
            ty = {"kind": "any_type"}
    constraints = [build_constraint(c, tns) for c in node.children if c.ns == XS_NS and c.local in ("key", "keyref", "unique")]
    return {
        "abstract": is_abs,
        "annotation": child_annotation(node),
        "constraints": constraints,
        "default": node.attrs.get((None, "default")),
        "fixed": node.attrs.get((None, "fixed")),
        "name": name,
        "nillable": parse_bool(node.attrs.get((None, "nillable"))),
        "pos": node.pos,
        "substitution_group": sg,
        "type": ty,
    }


def build_particle(node: Node, tns: str | None, efd: str, afd: str, in_group_def: bool) -> dict:
    occurs = {"max": 1, "min": 1} if in_group_def else parse_occurs(node)
    if node.local in ("sequence", "choice", "all"):
        items = [
            build_particle(c, tns, efd, afd, False)
            for c in node.children
            if c.ns == XS_NS and c.local in ("sequence", "choice", "all", "element", "group")
        ]
        return {"annotation": child_annotation(node), "items": items, "kind": node.local, "occurs": occurs, "pos": node.pos}
    if node.local == "group":
        return {
            "annotation": child_annotation(node),
            "kind": "group_ref",
            "occurs": occurs,
            "pos": node.pos,
            "target": resolve_qname(node.attrs[(None, "ref")], node.scope),
        }
    assert node.local == "element"
    if (None, "ref") in node.attrs:
        return {
            "annotation": child_annotation(node),
            "kind": "element_ref",
            "occurs": occurs,
            "pos": node.pos,
            "target": resolve_qname(node.attrs[(None, "ref")], node.scope),
        }
    form = node.attrs.get((None, "form"), efd).strip()
    ns = tns if form == "qualified" else None
    name = {"local": node.attrs[(None, "name")].strip(), "ns": ns}
    return {"decl": build_element_decl(node, name, tns, efd, afd, False), "kind": "element", "occurs": occurs}


def build_complex_type(node: Node, tns: str | None, efd: str, afd: str, top_level: bool) -> dict:
    name = {"local": node.attrs[(None, "name")].strip(), "ns": tns} if top_level else None
    is_abs = parse_bool(node.attrs.get((None, "abstract"))) if top_level else False
    mixed = parse_bool(node.attrs.get((None, "mixed")))
    content = None
    direct_particle = None
    direct_attrs = []
    for c in node.children:
        if c.ns != XS_NS or c.local == "annotation":
            continue
        if c.local == "simpleContent":
            deriv = next(gc for gc in c.children if gc.ns == XS_NS and gc.local in ("extension", "restriction"))
            facets = [build_facet(gg) for gg in deriv.children if gg.ns == XS_NS and gg.local not in ("annotation", "attribute", "attributeGroup")]
            attrs = [build_attr_item(gg, tns, afd) for gg in deriv.children if gg.ns == XS_NS and gg.local in ("attribute", "attributeGroup")]
            content = {
                "attributes": attrs,
                "base": resolve_qname(deriv.attrs[(None, "base")], deriv.scope),
                "derivation": deriv.local,
                "facets": facets,
                "kind": "simple_content",
                "pos": deriv.pos,
            }
        elif c.local == "complexContent":
            cmixed = parse_bool(c.attrs[(None, "mixed")]) if (None, "mixed") in c.attrs else None
            deriv = next(gc for gc in c.children if gc.ns == XS_NS and gc.local in ("extension", "restriction"))
            part = next((build_particle(gg, tns, efd, afd, False) for gg in deriv.children if gg.ns == XS_NS and gg.local in ("sequence", "choice", "all", "group")), None)
            attrs = [build_attr_item(gg, tns, afd) for gg in deriv.children if gg.ns == XS_NS and gg.local in ("attribute", "attributeGroup")]
            content = {
                "attributes": attrs,
                "base": resolve_qname(deriv.attrs[(None, "base")], deriv.scope),
                "derivation": deriv.local,
                "kind": "complex_content",
                "mixed": cmixed,
                "particle": part,
                "pos": deriv.pos,
            }
        elif c.local in ("sequence", "choice", "all", "group"):
            direct_particle = build_particle(c, tns, efd, afd, False)
        elif c.local in ("attribute", "attributeGroup"):
            direct_attrs.append(build_attr_item(c, tns, afd))
    if content is None:
        content = {"attributes": direct_attrs, "kind": "direct", "particle": direct_particle}
    return {
        "abstract": is_abs,
        "annotation": child_annotation(node),
        "content": content,
        "mixed": mixed,
        "name": name,
        "pos": node.pos,
    }


def build_raw_ir_json(file_id: str, xml_text: str) -> bytes:
    root = parse_xml_expat(file_id, xml_text)
    tns = root.attrs.get((None, "targetNamespace"))
    efd = root.attrs.get((None, "elementFormDefault"), "unqualified").strip()
    afd = root.attrs.get((None, "attributeFormDefault"), "unqualified").strip()
    decls = []
    for c in root.children:
        if c.ns != XS_NS or c.local in ("annotation", "include", "import"):
            continue
        if c.local == "complexType":
            decls.append({"decl": build_complex_type(c, tns, efd, afd, True), "kind": "complex_type"})
        elif c.local == "simpleType":
            decls.append({"decl": build_simple_type(c, tns, True), "kind": "simple_type"})
        elif c.local == "element":
            name = {"local": c.attrs[(None, "name")].strip(), "ns": tns}
            decls.append({"decl": build_element_decl(c, name, tns, efd, afd, True), "kind": "element"})
        elif c.local == "attribute":
            name = {"local": c.attrs[(None, "name")].strip(), "ns": tns}
            decls.append({"decl": build_attr_decl(c, name, "optional", tns), "kind": "attribute"})
        elif c.local == "group":
            part = next(build_particle(gc, tns, efd, afd, True) for gc in c.children if gc.ns == XS_NS and gc.local in ("sequence", "choice", "all"))
            decls.append({
                "decl": {
                    "annotation": child_annotation(c),
                    "name": {"local": c.attrs[(None, "name")].strip(), "ns": tns},
                    "particle": part,
                    "pos": c.pos,
                },
                "kind": "group",
            })
        elif c.local == "attributeGroup":
            attrs = [build_attr_item(gc, tns, afd) for gc in c.children if gc.ns == XS_NS and gc.local in ("attribute", "attributeGroup")]
            decls.append({
                "decl": {
                    "annotation": child_annotation(c),
                    "attributes": attrs,
                    "name": {"local": c.attrs[(None, "name")].strip(), "ns": tns},
                    "pos": c.pos,
                },
                "kind": "attribute_group",
            })
    ir = {
        "decls": decls,
        "files": [file_id],
        "schema": "qif-raw-ir@1",
        "target_namespaces": {file_id: tns},
    }
    return json.dumps(ir, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def expected_outputs() -> dict[str, bytes]:
    cases: dict[str, dict[str, str]] = {
        "constructs": {"constructs.xsd": CONSTRUCTS_XSD},
        "redefine": {"redefine.xsd": REDEFINE_XSD},
        "dangling": {"dangling.xsd": DANGLING_XSD},
        "cycle": {"a.xsd": CYCLE_A_XSD, "b.xsd": CYCLE_B_XSD},
        "doctype": {"doctype.xsd": DOCTYPE_XSD},
        "unpinned_include": {"root.xsd": UNPINNED_INCLUDE_XSD},
        "qif3_bundle": QIF3_BUNDLE,
    }
    out: dict[str, bytes] = {}
    for name, files in cases.items():
        for rel, text in files.items():
            out[f"{name}/{rel}"] = text.encode("utf-8")
        out[f"{name}/{LICENCE_PATH}"] = LICENCE.encode("utf-8")
        out[f"{name}/qif-xsd.manifest.toml"] = manifest(files)

    out["constructs.ir.json"] = build_raw_ir_json("constructs.xsd", CONSTRUCTS_XSD)
    return out


def compute_qif3_counts(bundle_dir: Path) -> bytes:
    return count_decls.render_json(count_decls.count_schema_dir(bundle_dir))


def write_all() -> None:
    expected = expected_outputs()
    managed_dirs = {"constructs", "redefine", "dangling", "cycle", "doctype", "unpinned_include", "qif3_bundle"}
    for d in managed_dirs:
        target = HERE / d
        if target.exists():
            shutil.rmtree(target)
    for rel, data in sorted(expected.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    (HERE / "qif3-counts.json").write_bytes(compute_qif3_counts(HERE / "qif3_bundle"))


def check_all() -> int:
    expected = expected_outputs()
    managed_dirs = {"constructs", "redefine", "dangling", "cycle", "doctype", "unpinned_include", "qif3_bundle"}
    ok = True

    for rel, data in sorted(expected.items()):
        p = HERE / rel
        if not p.exists():
            print(f"MISSING {p}", file=sys.stderr)
            ok = False
        elif p.read_bytes() != data:
            print(f"DRIFT {p}", file=sys.stderr)
            ok = False

    # Check no extra files in managed directories.
    for d in sorted(managed_dirs):
        root = HERE / d
        if not root.exists():
            continue
        for f in sorted(root.rglob("*")):
            if f.is_file():
                rel = f.relative_to(HERE).as_posix()
                if rel not in expected:
                    print(f"EXTRA {f}", file=sys.stderr)
                    ok = False

    counts_path = HERE / "qif3-counts.json"
    if not counts_path.exists():
        print(f"MISSING {counts_path}", file=sys.stderr)
        ok = False
    elif (HERE / "qif3_bundle").exists():
        expected_counts = compute_qif3_counts(HERE / "qif3_bundle")
        if counts_path.read_bytes() != expected_counts:
            print(f"DRIFT {counts_path}", file=sys.stderr)
            ok = False

    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="Verify generated fixtures are up to date")
    args = ap.parse_args()
    if args.check:
        return check_all()
    write_all()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
