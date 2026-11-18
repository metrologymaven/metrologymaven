#!/usr/bin/env python3
r"""Independent reference generator for the IO.QIF.XML.PARSE fixtures (crate fid-xml).

Every XML document below is a small snippet written for this test suite (decision D-038:
no QIF XSDs or third-party files). Expected outcomes come from two independent sources,
neither of which is the Rust code:

* Event streams (`*.events`) of the well-formed documents are produced by the Python
  standard library's expat binding (xml.parsers.expat) in namespace mode, with DTD
  processing switched off: parameter entities are never parsed and a DOCTYPE or entity
  declaration raises before anything is resolved. The script asserts that the UTF-16
  documents give exactly the events of their UTF-8 twin (AC-4).
* Error outcomes (`expected.txt`) are hand-labelled from the leaf's rules
  (spec/io/qif/xml/parse/NODE.md): the error code, the forbidden construct and its
  position, where the position is found by searching this script's own bytes for the
  construct (byte offset, 1-based line, 1-based column counted in bytes). For the
  forbidden constructs the script also asserts that expat, with DTDs disabled, stops on
  the same line. Every malformed case is asserted to be rejected by expat as well.

Event line format (tab separated; text escaped as `\\`, `\t`, `\n`, `\r` and `\u{hex}` for
the other control characters):

  N <prefix> <uri>                       namespace declaration (before its element)
  S <prefix> {<uri>}<local> <attr>...    start tag; attr = <prefix>:{<uri>}<local>=<value>
  E {<uri>}<local>                       end tag (also after an empty-element tag)
  T <text>                               character data, adjacent pieces joined
  C <text>                               CDATA section content
  M <text>                               comment
  P <target> <data>                      processing instruction

Run (stdlib only):
  python fixtures/io/qif/xml/parse/make_fixtures.py           # rewrite the fixtures
  python fixtures/io/qif/xml/parse/make_fixtures.py --check   # fail if any fixture is stale
"""

import sys
from pathlib import Path
from xml.parsers import expat

HERE = Path(__file__).resolve().parent
XINCLUDE = "http://www.w3.org/2001/XInclude"

# ------------------------------------------------------------------ documents

BASIC = (
    b'<?xml version="1.0" encoding="UTF-8"?>\r\n'
    b"<!-- leading comment -->\r\n"
    b'<?app-hint mode="fast" ?>\r\n'
    b'<root id="r1" note="tab\there&#9;kept" multi="line one\r\nline two">\r\n'
    b"  <item n='1'>alpha &amp; beta &lt;gamma&gt; &quot;delta&quot; &apos;eps&apos;</item>\r\n"
    + "  <item n=\"2\">&#65;&#x42;&#x1F600; café µm</item>\r\n".encode("utf-8")
    + b"  <empty/>\r\n"
    b'  <code><![CDATA[if (a < b && c > d) { x = "]]"; }]]></code>\r\n'
    b"  <!-- inner comment -->\r\n"
    b"  <mixed>one<b>two</b>three\rfour<![CDATA[]]></mixed>\r\n"
    b"  <?proc   data with  spaces ?>\r\n"
    b"  <e2 ></e2  >\r\n"
    b"</root>\r\n"
    b"<!-- trailing comment -->\r\n"
)

NS = (
    b'<?xml version="1.0"?>\n'
    b'<q:doc xmlns:q="urn:example:q" xmlns="urn:example:default" xml:lang="en">\n'
    b'  <q:head q:kind="main" plain="1">\n'
    b"    <title>Probe</title>\n"
    b"  </q:head>\n"
    b'  <body xmlns:ext="urn:example:ext">\n'
    b'    <ext:block ext:id="b1" q:ref="h">\n'
    b'      <ext:part xmlns="">no default here</ext:part>\n'
    b'      <q:leaf xmlns:q="urn:example:q2" q:x="1"/>\n'
    b"      <!-- kept -->\n"
    b"    </ext:block>\n"
    b"  </body>\n"
    b"</q:doc>\n"
)


def twin_text(encoding_name: str) -> str:
    return (
        f'<?xml version="1.0" encoding="{encoding_name}"?>\n'
        '<messung einheit="mm">\n'
        '  <wert typ="Länge">12.5 µm ± 0.1</wert>\n'
        "  <notiz>\U0001D11E Größe ≤ 3</notiz>\n"
        "</messung>\n"
    )


UTF16 = b"\xff\xfe" + twin_text("UTF-16").encode("utf-16-le")
UTF16BE = b"\xfe\xff" + twin_text("UTF-16").encode("utf-16-be")
UTF16_TWIN = twin_text("UTF-8").encode("utf-8")
BASIC_BOM = b"\xef\xbb\xbf" + BASIC


def nested(depth: int) -> bytes:
    return b"<e>\n" * depth + b"</e>\n" * depth


def many_attrs(count: int) -> bytes:
    attrs = b"".join(b' a%d="%d"' % (i, i) for i in range(1, count + 1))
    return b"<r" + attrs + b"/>\n"


DEEP64 = nested(64)
DEEP65 = nested(65)
ATTRS256 = many_attrs(256)
ATTRS257 = many_attrs(257)

XXE = (
    b'<?xml version="1.0" encoding="UTF-8"?>\n'
    b"<!DOCTYPE probe [\n"
    b'  <!ENTITY secret SYSTEM "file:///etc/hostname">\n'
    b"]>\n"
    b"<probe>&secret;</probe>\n"
)
DOCTYPE = (
    b'<?xml version="1.0"?>\n'
    b"<!-- a document type declaration with an external subset -->\n"
    b'<!DOCTYPE probe SYSTEM "probe.dtd">\n'
    b"<probe/>\n"
)
LAUGHS = [b'<?xml version="1.0"?>\n', b"<!DOCTYPE bomb [\n", b'  <!ENTITY a "ha">\n']
for _prev, _name in zip("abcdefgh", "bcdefghi"):
    LAUGHS.append(b'  <!ENTITY %s "%s">\n' % (_name.encode(), (b"&" + _prev.encode() + b";") * 10))
LAUGHS += [b"]>\n", b"<bomb>&i;</bomb>\n"]
BILLION_LAUGHS = b"".join(LAUGHS)
XINCLUDE_DOC = (
    b'<?xml version="1.0"?>\n'
    b'<plan xmlns:xi="http://www.w3.org/2001/XInclude">\n'
    b"  <step/>\n"
    b'  <xi:include href="secret.xml" parse="text"/>\n'
    b"</plan>\n"
)
XINCLUDE_DEFAULT = b'<plan>\n  <include xmlns="http://www.w3.org/2001/XInclude" href="x.xml"/>\n</plan>\n'
ENTITY_REF = b"<probe>\n  <a>&custom;</a>\n</probe>\n"
ENTITY_REF_ATTR = b'<probe a="x&custom;"/>\n'
ENTITY_DECL = b'<!ENTITY x "y">\n<probe/>\n'

LATIN1 = b'<?xml version="1.0" encoding="ISO-8859-1"?>\n<r>caf\xe9</r>\n'
UTF16_NOBOM = '<?xml version="1.0" encoding="UTF-16"?>\n<r/>\n'.encode("utf-16-le")
UTF16_DECL_UTF8 = b"\xff\xfe" + '<?xml version="1.0" encoding="UTF-8"?>\n<r/>\n'.encode("utf-16-le")
UTF8_DECL_UTF16 = b'<?xml version="1.0" encoding="UTF-16"?>\n<r/>\n'
UTF32 = b"\xff\xfe\x00\x00" + "<r/>\n".encode("utf-32-le")

# Hand-labelled malformed cases: name -> (bytes, expected code). Each must also be
# rejected by expat.
MALFORMED = {
    "mismatched_end": (b"<a><b></a></b>\n", "malformed"),
    "unclosed": (b"<a><b></b>\n", "truncated_file"),
    "two_roots": (b"<a/>\n<b/>\n", "malformed"),
    "cdata_end_in_text": (b"<a>x ]]> y</a>\n", "malformed"),
    "double_dash_comment": (b"<a><!-- x -- y --></a>\n", "malformed"),
    "lt_in_attr": (b'<a b="x<y"/>\n', "malformed"),
    "duplicate_attr": (b'<a b="1" b="2"/>\n', "malformed"),
    "duplicate_expanded_attr": (b'<a xmlns:p="urn:u" xmlns:q="urn:u" p:x="1" q:x="2"/>\n', "malformed"),
    "unbound_prefix": (b"<p:a/>\n", "malformed"),
    "unbound_attr_prefix": (b'<a p:b="1"/>\n', "malformed"),
    "empty_prefix_decl": (b'<a xmlns:p=""/>\n', "malformed"),
    "xmlns_prefix_decl": (b'<a xmlns:xmlns="urn:u"/>\n', "malformed"),
    "xml_prefix_rebound": (b'<a xmlns:xml="urn:u"/>\n', "malformed"),
    "bad_charref": (b"<a>&#0;</a>\n", "malformed"),
    "huge_charref": (b"<a>&#x110000;</a>\n", "malformed"),
    "control_char": (b"<a>\x01</a>\n", "malformed"),
    "bad_utf8": (b"<a>\xff</a>\n", "malformed"),
    "text_before_root": (b"x<a/>\n", "malformed"),
    "empty": (b"", "truncated_file"),
    "truncated_tag": (b'<a b="1"', "truncated_file"),
    "late_xml_decl": (b'<a/>\n<?xml version="1.0"?>\n', "malformed"),
    "bad_name_start": (b"<1a/>\n", "malformed"),
    "two_colons": (b'<a:b:c xmlns:a="urn:u"/>\n', "malformed"),
    "attrs_without_space": (b'<a b="1"c="2"/>\n', "malformed"),
    "bare_ampersand": (b"<a>a & b</a>\n", "malformed"),
    "unterminated_ref": (b"<a>&amp</a>\n", "malformed"),
    "lone_surrogate": (b"\xff\xfe" + "<a>".encode("utf-16-le") + b"\x00\xd8" + "</a>".encode("utf-16-le"), "malformed"),
    "cdata_outside_root": (b"<![CDATA[x]]><a/>\n", "malformed"),
    "end_tag_at_top": (b"</a>\n", "malformed"),
    "pi_target_xml_upper": (b"<a><?XML x?></a>\n", "malformed"),
    "attr_without_value": (b"<a b/>\n", "malformed"),
}

# ------------------------------------------------------------------ expat reference


class Refused(Exception):
    """Raised from a handler when expat reports a construct the reader must refuse."""

    def __init__(self, line):
        super().__init__(line)
        self.line = line


def esc(s: str) -> str:
    out = []
    for ch in s:
        if ch == "\\":
            out.append("\\\\")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ord(ch) < 0x20 or ord(ch) == 0x7F:
            out.append("\\u{%x}" % ord(ch))
        else:
            out.append(ch)
    return "".join(out)


def split_name(name: str):
    parts = name.split(" ")
    if len(parts) == 1:
        return "", "", parts[0]
    if len(parts) == 2:
        return "", parts[0], parts[1]
    return parts[2], parts[0], parts[1]


def new_parser():
    p = expat.ParserCreate(namespace_separator=" ")
    p.namespace_prefixes = True
    p.ordered_attributes = True
    p.buffer_text = False
    p.SetParamEntityParsing(expat.XML_PARAM_ENTITY_PARSING_NEVER)

    def refuse(*_args):
        raise Refused(p.CurrentLineNumber)

    p.StartDoctypeDeclHandler = refuse
    p.EntityDeclHandler = refuse
    p.ExternalEntityRefHandler = refuse
    return p


def events(data: bytes) -> str:
    p = new_parser()
    out = []
    text = []
    cdata = []
    state = {"cdata": False}

    def flush():
        if text:
            out.append("T\t" + esc("".join(text)))
            text.clear()

    def start_ns(prefix, uri):
        flush()
        out.append("N\t%s\t%s" % (esc(prefix or ""), esc(uri or "")))

    def start(name, attrs):
        flush()
        prefix, uri, local = split_name(name)
        if uri == XINCLUDE:
            raise Refused(p.CurrentLineNumber)
        fields = ["S", esc(prefix), "{%s}%s" % (esc(uri), esc(local))]
        for i in range(0, len(attrs), 2):
            ap, au, al = split_name(attrs[i])
            fields.append("%s:{%s}%s=%s" % (esc(ap), esc(au), esc(al), esc(attrs[i + 1])))
        out.append("\t".join(fields))

    def end(name):
        flush()
        _prefix, uri, local = split_name(name)
        out.append("E\t{%s}%s" % (esc(uri), esc(local)))

    def chars(data):
        (cdata if state["cdata"] else text).append(data)

    def start_cdata():
        flush()
        state["cdata"] = True
        cdata.clear()

    def end_cdata():
        state["cdata"] = False
        out.append("C\t" + esc("".join(cdata)))

    def comment(data):
        flush()
        out.append("M\t" + esc(data))

    def pi(target, data):
        flush()
        out.append("P\t%s\t%s" % (esc(target), esc(data)))

    p.StartNamespaceDeclHandler = start_ns
    p.StartElementHandler = start
    p.EndElementHandler = end
    p.CharacterDataHandler = chars
    p.StartCdataSectionHandler = start_cdata
    p.EndCdataSectionHandler = end_cdata
    p.CommentHandler = comment
    p.ProcessingInstructionHandler = pi
    p.Parse(data, True)
    flush()
    return "".join(line + "\n" for line in out)


def expat_failure_line(data: bytes) -> int:
    """The line on which expat (DTDs disabled) refuses or rejects `data`."""
    try:
        events(data)
    except Refused as r:
        return r.line
    except expat.ExpatError as e:
        return e.lineno
    raise AssertionError("expat accepted a document that must fail")


def locate(data: bytes, needle: bytes, nth: int = 1):
    """Byte offset, 1-based line and 1-based byte column of the nth occurrence."""
    at = -1
    for _ in range(nth):
        at = data.index(needle, at + 1)
    line = data.count(b"\n", 0, at) + 1
    col = at - (data.rfind(b"\n", 0, at) + 1) + 1
    return at, line, col


# ------------------------------------------------------------------ generation


def generate():
    files = {}
    expected = ["# Generated by make_fixtures.py. key value"]

    def put(name, data):
        files[name] = data

    def want(key, value):
        expected.append("%s %s" % (key, value))

    # Well-formed documents and their expat event streams.
    ok = {
        "basic.xml": BASIC,
        "basic_bom.xml": BASIC_BOM,
        "ns.xml": NS,
        "utf16.xml": UTF16,
        "utf16be.xml": UTF16BE,
        "utf16_twin.xml": UTF16_TWIN,
        "deep64.xml": DEEP64,
        "attrs256.xml": ATTRS256,
    }
    for name, data in ok.items():
        put(name, data)
        put(name[: -len(".xml")] + ".events", events(data).encode("utf-8"))
    # AC-4: the UTF-16 documents give the events of their UTF-8 twin; a BOM changes nothing.
    assert events(UTF16) == events(UTF16_TWIN) == events(UTF16BE)
    assert events(BASIC_BOM) == events(BASIC)

    # AC-1: forbidden constructs, at the construct's position.
    forbidden = [
        ("xxe", XXE, "doctype", b"<!DOCTYPE"),
        ("doctype", DOCTYPE, "doctype", b"<!DOCTYPE"),
        ("billion_laughs", BILLION_LAUGHS, "doctype", b"<!DOCTYPE"),
        ("xinclude", XINCLUDE_DOC, "xinclude", b"<xi:include"),
        ("xinclude_default", XINCLUDE_DEFAULT, "xinclude", b"<include"),
        ("entity_ref", ENTITY_REF, "entity_reference", b"&custom;"),
        ("entity_ref_attr", ENTITY_REF_ATTR, "entity_reference", b"&custom;"),
        ("entity_decl", ENTITY_DECL, "entity_declaration", b"<!ENTITY"),
    ]
    for name, data, construct, needle in forbidden:
        put(name + ".xml", data)
        at, line, col = locate(data, needle)
        assert expat_failure_line(data) == line, name
        want(name + ".code", "xml_forbidden_construct")
        want(name + ".construct", construct)
        want(name + ".byte", at)
        want(name + ".line", line)
        want(name + ".col", col)

    # AC-2: depth 65 fails at the 65th start tag; depth 64 parses (events above).
    put("deep65.xml", DEEP65)
    assert events(DEEP65).count("S\t") == 65  # well-formed apart from the limit
    at, line, col = locate(DEEP65, b"<e>", 65)
    for key, value in [("code", "limit_exceeded"), ("limit", "depth"), ("value", 64), ("byte", at)]:
        want("deep65." + key, value)
    want("deep65.line", line)
    want("deep65.col", col)

    # AC-3: 257 attributes fail at the 257th attribute name; 256 parse (events above).
    put("attrs257.xml", ATTRS257)
    assert events(ATTRS257).startswith("S\t\t{}r\t")
    at, line, col = locate(ATTRS257, b"a257=")
    for key, value in [("code", "limit_exceeded"), ("limit", "attributes"), ("value", 256), ("byte", at)]:
        want("attrs257." + key, value)
    want("attrs257.line", line)
    want("attrs257.col", col)

    # AC-4: encodings other than UTF-8 and BOM-marked UTF-16 are refused.
    encodings = [
        ("latin1", LATIN1),
        ("utf16_nobom", UTF16_NOBOM),
        ("utf16_decl_utf8", UTF16_DECL_UTF8),
        ("utf8_decl_utf16", UTF8_DECL_UTF16),
        ("utf32", UTF32),
    ]
    events(LATIN1)  # well-formed for expat; only the encoding is refused
    for name, data in encodings:
        put(name + ".xml", data)
        want(name + ".code", "xml_encoding_unsupported")

    # Malformed input: hand-labelled code; expat rejects every one.
    for name, (data, code) in sorted(MALFORMED.items()):
        try:
            events(data)
        except (expat.ExpatError, Refused):
            pass
        else:
            raise AssertionError("expat accepted malformed case " + name)
        put("malformed/" + name + ".xml", data)
        want("malformed." + name + ".code", code)

    put("expected.txt", ("\n".join(expected) + "\n").encode("utf-8"))
    return files


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, data in sorted(generate().items()):
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if stale:
        print("stale fixtures (rerun make_fixtures.py):", ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
