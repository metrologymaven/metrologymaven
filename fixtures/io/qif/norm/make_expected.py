#!/usr/bin/env python3
"""Independent reference normaliser for qif-norm@1 (spec node IO.QIF.NORM).

Writes expected/<name>.norm for every *.qif fixture in this folder and the corpus
manifest corpus/p0.toml (with SHA-256 digests). It reimplements the rules from the prose
in crates/fid-qif/src/norm/mod.rs on a different design: Python's expat parser (DTDs
are not used by any fixture; the script refuses DOCTYPE), a full in-memory tree, and a
recursive serialiser. It never runs the Rust code.

    python fixtures/io/qif/norm/make_expected.py          # (re)write expected files
    python fixtures/io/qif/norm/make_expected.py --check  # fail if any file is stale

stdlib only. The simple-content table and the prefix table are read from
crates/fid-qif/qif-norm.toml with tomllib.
"""

import hashlib
import sys
import tomllib
import xml.parsers.expat
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
RULES = tomllib.loads((ROOT / "crates" / "fid-qif" / "qif-norm.toml").read_text(encoding="utf-8"))
XML_NS = "http://www.w3.org/XML/1998/namespace"
XSI_NS = "http://www.w3.org/2001/XMLSchema-instance"
WS = " \t\n\r"


def clark(name):
    if name.startswith("{"):
        uri, local = name[1:].split("}", 1)
        return (uri, local)
    return ("", name)


FIXED = dict(RULES["prefixes"])
SIMPLE_EL = {clark(n) for n in RULES["simple_content"]["elements"]}
SIMPLE_AT = {clark(n) for n in RULES["simple_content"]["attributes"]}


def collapse(s):
    return " ".join(p for p in s.replace("\t", " ").replace("\n", " ").replace("\r", " ").split(" ") if p)


def split_name(n):
    # expat with namespace_separator=" " reports "uri local" or "local".
    if " " in n:
        uri, local = n.split(" ", 1)
        return uri, local
    return "", n


class El:
    def __init__(self, uri, local, attrs, scope):
        self.uri, self.local, self.attrs, self.scope = uri, local, attrs, scope
        self.children = []  # ("text", s) | ("pi", target, data) | ("el", El)


def parse(data):
    p = xml.parsers.expat.ParserCreate(namespace_separator=" ")
    p.ordered_attributes = True
    root = []
    stack = []
    scopes = [{"xml": XML_NS}]
    pending_ns = {}
    top_pis = []  # (position, target, data): position 0 before root, 1 after

    def start_ns(prefix, uri):
        pending_ns[prefix] = uri or ""

    def start(name, attrs):
        scope = dict(scopes[-1])
        scope.update(pending_ns)
        pending_ns.clear()
        scopes.append(scope)
        uri, local = split_name(name)
        pairs = [(split_name(attrs[i]), attrs[i + 1]) for i in range(0, len(attrs), 2)]
        el = El(uri, local, pairs, scope)
        if stack:
            stack[-1].children.append(("el", el))
        else:
            root.append(el)
        stack.append(el)

    def end(name):
        stack.pop()
        scopes.pop()

    def chars(s):
        if stack:
            kids = stack[-1].children
            if kids and kids[-1][0] == "text":
                kids[-1] = ("text", kids[-1][1] + s)
            else:
                kids.append(("text", s))

    def pi(target, d):
        if stack:
            stack[-1].children.append(("pi", target, d))
        else:
            top_pis.append((1 if root else 0, target, d))

    def doctype(*a):
        raise ValueError("DOCTYPE refused")

    p.StartNamespaceDeclHandler = start_ns
    p.StartElementHandler = start
    p.EndElementHandler = end
    p.CharacterDataHandler = chars
    p.ProcessingInstructionHandler = pi
    p.StartDoctypeDeclHandler = doctype
    p.Parse(data, True)
    return root[0], top_pis


def esc_text(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\r", "&#xD;")


def esc_attr(s):
    return (s.replace("&", "&amp;").replace("<", "&lt;").replace('"', "&quot;")
            .replace("\t", "&#x9;").replace("\n", "&#xA;").replace("\r", "&#xD;"))


def resolve_type(el, value):
    v = collapse(value)
    if " " in v:
        return None
    if ":" in v:
        prefix, local = v.split(":", 1)
        if not prefix or not local or ":" in local:
            return None
    else:
        prefix, local = None, v
        if not local:
            return None
    key = prefix if prefix is not None else None
    if key is None:
        uri = el.scope.get(None, "")
        return (uri or "", local)
    if key in el.scope and el.scope[key]:
        return (el.scope[key], local)
    return None


class Norm:
    def __init__(self):
        self.generated = {}

    def prefix(self, uri):
        if uri == XML_NS:
            return "xml"
        if uri in FIXED:
            return FIXED[uri]
        if uri not in self.generated:
            self.generated[uri] = "n%d" % (len(self.generated) + 1)
        return self.generated[uri]

    def q(self, uri, local):
        return "%s:%s" % (self.prefix(uri), local) if uri else local

    def element(self, el, declared):
        types = {}
        used = set()
        if el.uri:
            used.add(el.uri)
        for (auri, alocal), value in el.attrs:
            if auri:
                used.add(auri)
            if auri == XSI_NS and alocal == "type":
                r = resolve_type(el, value)
                types[(auri, alocal)] = r
                if r and r[0]:
                    used.add(r[0])
        for u in sorted(used):
            self.prefix(u)
        attrs = []
        for (auri, alocal), value in el.attrs:
            if auri == XSI_NS and alocal == "type":
                r = types[(auri, alocal)]
                v = self.q(r[0], r[1]) if r else collapse(value)
            elif (auri, alocal) in SIMPLE_AT:
                v = collapse(value)
            else:
                v = value
            attrs.append(((auri, alocal), self.q(auri, alocal), v))
        attrs.sort(key=lambda a: a[0])
        new = sorted(u for u in used if u != XML_NS and u not in declared)
        decls = sorted((self.prefix(u), u) for u in new)
        name = self.q(el.uri, el.local)
        out = ["<", name]
        for p, u in decls:
            out.append(' xmlns:%s="%s"' % (p, esc_attr(u)))
        for _, n, v in attrs:
            out.append(' %s="%s"' % (n, esc_attr(v)))
        out.append(">")
        inner = declared | set(new)
        has_child = any(c[0] == "el" for c in el.children)
        simple = (el.uri, el.local) in SIMPLE_EL
        for c in el.children:
            if c[0] == "text":
                t = c[1]
                if has_child and t.strip(WS) == "":
                    continue
                if simple:
                    t = collapse(t)
                out.append(esc_text(t))
            elif c[0] == "pi":
                out.append("<?%s%s?>" % (c[1], " " + c[2] if c[2] else ""))
            else:
                out.append(self.element(c[1], inner))
        out.append("</%s>" % name)
        return "".join(out)


def normalise(data):
    root, pis = parse(data)
    n = Norm()
    before = "".join("<?%s%s?>\n" % (t, " " + d if d else "") for pos, t, d in pis if pos == 0)
    after = "".join("\n<?%s%s?>" % (t, " " + d if d else "") for pos, t, d in pis if pos == 1)
    return (before + n.element(root, frozenset()) + after).encode("utf-8")


CORPUS_FILES = ["ws_a.qif", "list_ws.qif"]


def corpus_manifest():
    lines = [
        "# QIF P0-subset corpus manifest for the round-trip harness tests (IO.QIF.NORM AC-6),",
        "# in the IO.CORP.MANIF format. Generated by fixtures/io/qif/norm/make_expected.py.",
        'format = "qif.xml"',
    ]
    for name in CORPUS_FILES:
        digest = hashlib.sha256((HERE / name).read_bytes()).hexdigest()
        lines += [
            "",
            "[[entry]]",
            'path = "files/%s"' % name,
            'source = "self-written, fixtures/io/qif/norm/%s"' % name,
            'licence = "CC-BY-4.0"',
            'sha256 = "sha256:%s"' % digest,
            "expect = { accept = {} }",
            'tags = ["p0-subset"]',
        ]
    digest = hashlib.sha256((HERE / "dec_a.qif").read_bytes()).hexdigest()
    lines += [
        "",
        "# Not in the P0 subset: the harness skips it.",
        "[[entry]]",
        'path = "files/dec_a.qif"',
        'source = "self-written, fixtures/io/qif/norm/dec_a.qif"',
        'licence = "CC-BY-4.0"',
        'sha256 = "sha256:%s"' % digest,
        "expect = { accept = {} }",
        'tags = ["extended"]',
    ]
    return ("\n".join(lines) + "\n").encode("utf-8")


def outputs():
    out = {}
    for f in sorted(HERE.glob("*.qif")):
        if f.name.startswith("forbidden"):
            continue
        out[HERE / "expected" / (f.stem + ".norm")] = normalise(f.read_bytes())
    out[HERE / "corpus" / "p0.toml"] = corpus_manifest()
    for name in CORPUS_FILES + ["dec_a.qif"]:
        out[HERE / "corpus" / "files" / name] = (HERE / name).read_bytes()
    return out


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for path, data in outputs().items():
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(path)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    # Idempotence of the reference itself.
    for f in sorted(HERE.glob("*.qif")):
        if f.name.startswith("forbidden"):
            continue
        once = normalise(f.read_bytes())
        if normalise(once) != once:
            print("reference normaliser is not idempotent on", f.name)
            return 1
    if stale:
        for p in stale:
            print("stale:", p.relative_to(ROOT))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
