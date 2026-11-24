#!/usr/bin/env python3
"""Generate the ECO.DATA.CAT fixtures (MIT OR Apache-2.0, like the spec node says).

Writes hand-written catalogues fixtures/datasets/*.toml, the level-1 domain list
domains.txt, and expected.txt: for every catalogue, the findings `<CODE> <subject>`
in the order the check must print them (sorted by subject, then code), or `ok`.

The expected findings are labelled by hand from the rules of spec/eco/data/cat/NODE.md
(the EXPECT table below). Before anything is written, an independent checker in this
script (Python tomllib plus its own SPDX evaluator, sharing no code with the Rust
crate) must reach exactly the same findings for every fixture.

All entries are synthetic: digests are made up and URLs point at example.org. They are
not catalogue entries and say nothing about the real terms of any organisation.
Run:  python fixtures/datasets/make_fixtures.py   (Python 3.11 or later)
"""

import re
import tomllib
from pathlib import Path

HERE = Path(__file__).parent

DOMAINS = ["AGENT", "ALN", "AUTO", "CAL", "CMM", "CORE", "ECO", "ENT", "FLOW", "GDT", "GEO", "HAL", "I18N", "IND",
           "IO", "KRN", "LVM", "MIG", "OUTR", "PLAT", "QUAL", "RPT", "SCAN", "SURF", "TRUST", "UNC", "UX", "VIZ"]

POLICY = ('[policy]\n'
          'redistributable = ["CC0-1.0", "CC-BY-4.0", "CC-BY-SA-4.0", "MIT", "Apache-2.0", "BSD-3-Clause",\n'
          '                   "ODbL-1.0", "PDDL-1.0"]\n')

D1 = "0123456789abcdef" * 4
D2 = "fedcba9876543210" * 4
D3 = "00112233445566778899aabbccddeeff" * 2


def entry(id_, licence="CC-BY-4.0", redistribution="mirror", owner="KRN", sha=D1, extra="", urls=None,
          terms=None, size="1024"):
    lines = ["", "[[dataset]]"]
    if id_ is not None:
        lines.append(f'id = "{id_}"')
    lines += [f'title = "Synthetic dataset {id_}"', f'source = "https://example.org/datasets/{id_}"',
              f'licence = "{licence}"']
    if terms:
        lines.append(f'terms_url = "{terms}"')
    lines += [f"size_bytes = {size}", f'sha256 = "{sha}"', f'owner = "{owner}"',
              f'redistribution = "{redistribution}"']
    if urls is None:
        urls = f'["https://example.org/files/{id_}.zip"]'
    if urls != "omit":
        lines.append(f"urls = {urls}")
    if extra:
        lines.append(extra)
    return "\n".join(lines) + "\n"


HEAD = "# ECO.DATA.CAT fixture ({}). Synthetic entries only. MIT OR Apache-2.0.\n"
TERMS = "https://example.org/terms"

FIXTURES = {
    # AC-1: one CC-BY-4.0 mirror, one LicenseRef-NIST-Terms fetch-only, one CC0-1.0 mirror.
    "valid.toml": POLICY
    + entry("probe-sphere-scans", "CC-BY-4.0", owner="SCAN",
            urls='[\n  "https://example.org/files/a.zip",  # primary\n  "https://mirror.example.org/a.zip",\n]',
            extra='notes = "Hand-written \\"valid\\" entry. Tab:\\t end"')
    + entry("nist-like-fit-reference", "LicenseRef-NIST-Terms", "fetch-only", owner="KRN", sha=D2, terms=TERMS,
            extra="contains_standards_text = false\ncontains_customer_data = false")
    + entry("cc0-point-clouds.v1", "CC0-1.0", owner="IO", sha=D3, size="1_000_000",
            extra="contains_customer_data = true\nrelease_ref = 'decisions/D-999-example-release.md'"),
    # AC-2
    "mirror-noredist.toml": POLICY + entry("vendor-eula-scan", "LicenseRef-Vendor-EULA", terms=TERMS),
    # AC-3
    "standards.toml": POLICY + entry("quoted-standard", extra="contains_standards_text = true"),
    # AC-4
    "bad-digest.toml": POLICY + entry("short-digest", sha=D1[:63]),
    "dup.toml": POLICY + entry("same-id") + entry("other-id", sha=D2) + entry("same-id", sha=D3),
    # AC-5
    "bad-owner.toml": POLICY + entry("unknown-owner", owner="XYZ"),
    # REQ-003, second half: customer data needs a release_ref.
    "customer.toml": POLICY + entry("customer-no-release", extra="contains_customer_data = true")
    + entry("customer-released", extra='contains_customer_data = true\nrelease_ref = "D-998"'),
    # REQ-002: OR any, AND all, LicenseRef never unless listed, WITH is one term.
    "expressions.toml": POLICY
    + entry("or-licenseref", "MIT OR LicenseRef-Custom", terms=TERMS)
    + entry("and-licenseref", "MIT AND LicenseRef-Custom", terms=TERMS)
    + entry("nested-ok", "(CC-BY-4.0 OR GPL-3.0-only) AND CC0-1.0")
    + entry("nested-bad", "(CC-BY-4.0 AND GPL-3.0-only) OR LicenseRef-Custom", terms=TERMS)
    + entry("gpl-fetch-only", "GPL-3.0-only", "fetch-only")
    + entry("gpl-mirror", "GPL-3.0-only")
    + entry("with-exception", "Apache-2.0 WITH LLVM-exception"),
    # REQ-001: schema errors, one entry each.
    "schema.toml": POLICY
    + entry("Bad_ID")
    + entry(None)
    + entry("no-urls", urls="omit")
    + entry("empty-urls", urls="[]")
    + entry("ref-no-terms", "LicenseRef-Custom", "fetch-only")
    + entry("bad-redist", redistribution="copy")
    + entry("unknown-field", extra='colour = "blue"')
    + entry("negative-size", size="-1")
    + entry("bad-expression", "MIT OR")
    + entry("upper-digest", sha=D1.upper())
    + entry("bad-url", urls='["ftp://example.org/x"]')
    + entry("with-ref-no-terms", "MIT WITH LicenseRef-Custom-exception", "fetch-only")
    + entry("too-many-terms", " AND ".join(["MIT"] * 257))
    + entry("max-terms-ok", " OR ".join(["MIT"] * 256)),
    # REQ-005: several findings, entries out of order; printed by subject, then code.
    "multi.toml": POLICY
    + entry("zz-last", "LicenseRef-Vendor-EULA", owner="NOPE", terms=TERMS,
            extra="contains_standards_text = true\ncontains_customer_data = true")
    + entry("aa-first", "GPL-2.0-only", owner="KRN.NUM"),
    # Not TOML at all (unterminated string): one file-level finding.
    "syntax.toml": POLICY + '\n[[dataset]]\nid = "broken\n',
    # No policy table.
    "no-policy.toml": entry("lonely"),
}

EXPECT = {
    "valid.toml": [],
    "mirror-noredist.toml": ["DATA-REDIST vendor-eula-scan"],
    "standards.toml": ["DATA-STANDARDS quoted-standard"],
    "bad-digest.toml": ["DATA-SCHEMA short-digest"],
    "dup.toml": ["DATA-DUP same-id"],
    "bad-owner.toml": ["DATA-OWNER unknown-owner"],
    "customer.toml": ["DATA-CUSTOMER customer-no-release"],
    "expressions.toml": ["DATA-REDIST and-licenseref", "DATA-REDIST gpl-mirror", "DATA-REDIST nested-bad",
                         "DATA-REDIST with-exception"],
    "schema.toml": ["DATA-SCHEMA Bad_ID", "DATA-SCHEMA bad-expression", "DATA-SCHEMA bad-redist",
                    "DATA-SCHEMA bad-url", "DATA-SCHEMA dataset#2", "DATA-SCHEMA empty-urls",
                    "DATA-SCHEMA negative-size", "DATA-SCHEMA no-urls", "DATA-SCHEMA ref-no-terms",
                    "DATA-SCHEMA too-many-terms", "DATA-SCHEMA unknown-field", "DATA-SCHEMA upper-digest",
                    "DATA-SCHEMA with-ref-no-terms"],
    "multi.toml": ["DATA-OWNER aa-first", "DATA-REDIST aa-first", "DATA-CUSTOMER zz-last", "DATA-OWNER zz-last",
                   "DATA-REDIST zz-last", "DATA-STANDARDS zz-last"],
    "syntax.toml": ["DATA-SCHEMA -"],
    "no-policy.toml": ["DATA-SCHEMA -"],
}

# ---------------------------------------------------------------- independent checker

ID_RE = re.compile(r"[a-z0-9][a-z0-9._-]{2,63}")
HEX_RE = re.compile(r"[0-9a-f]{64}")
LIC_RE = re.compile(r"[A-Za-z0-9.+-]+")
URL_RE = re.compile(r"https?://\S+")
FIELDS = {"id", "title", "source", "licence", "terms_url", "size_bytes", "sha256", "owner", "redistribution",
          "urls", "notes", "contains_standards_text", "contains_customer_data", "release_ref"}


MAX_TERMS = 256  # spec choice for this leaf: longer expressions are malformed (DATA-SCHEMA)


def spdx(text):
    """Parse to nested tuples ('or'|'and', a, b) or ('lic', name); None if malformed
    or if it has more than MAX_TERMS terms (`A WITH B` counts once)."""
    toks = re.findall(r"\(|\)|[^\s()]+", text)
    if sum(1 for i, t in enumerate(toks) if LIC_RE.fullmatch(t) and t not in ("AND", "OR", "WITH")
           and (i == 0 or toks[i - 1] != "WITH")) > MAX_TERMS:
        return None
    pos = 0

    def peek():
        return toks[pos] if pos < len(toks) else None

    def take():
        nonlocal pos
        pos += 1
        return toks[pos - 1]

    def atom():
        t = peek()
        if t == "(":
            take()
            e = disj()
            if e is None or peek() != ")":
                return None
            take()
            return e
        if t is None or t in (")", "AND", "OR", "WITH") or not LIC_RE.fullmatch(t):
            return None
        name = take()
        if peek() == "WITH":
            take()
            ex = peek()
            if ex is None or ex in ("(", ")", "AND", "OR", "WITH") or not LIC_RE.fullmatch(ex):
                return None
            name += " WITH " + take()
        return ("lic", name)

    def conj():
        e = atom()
        while e is not None and peek() == "AND":
            take()
            r = atom()
            e = None if r is None else ("and", e, r)
        return e

    def disj():
        e = conj()
        while e is not None and peek() == "OR":
            take()
            r = conj()
            e = None if r is None else ("or", e, r)
        return e

    e = disj()
    return e if e is not None and pos == len(toks) else None


def ok(e, allowed):
    if e[0] == "lic":
        return e[1] in allowed
    a, b = ok(e[1], allowed), ok(e[2], allowed)
    return (a or b) if e[0] == "or" else (a and b)


def names(e):
    return [e[1]] if e[0] == "lic" else names(e[1]) + names(e[2])


def check(text, domains):
    try:
        doc = tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return ["DATA-SCHEMA -"]
    pol = doc.get("policy")
    if not isinstance(pol, dict) or set(pol) != {"redistributable"} or not all(
            isinstance(x, str) for x in pol["redistributable"]):
        return ["DATA-SCHEMA -"]
    allowed = set(pol["redistributable"])
    out = set()
    seen = {}
    for n, d in enumerate(doc.get("dataset", []), 1):
        sid = d["id"] if isinstance(d.get("id"), str) else f"dataset#{n}"
        seen[sid] = seen.get(sid, 0) + 1
        bad = set(d) - FIELDS
        s = lambda k: isinstance(d.get(k), str) and d[k] != ""  # noqa: E731
        bad |= {k for k in ("id", "title", "source", "licence", "sha256", "owner", "redistribution") if not s(k)}
        if s("id") and not ID_RE.fullmatch(d["id"]):
            bad.add("id")
        if s("source") and not URL_RE.fullmatch(d["source"]):
            bad.add("source")
        if not (isinstance(d.get("size_bytes"), int) and not isinstance(d["size_bytes"], bool)
                and d["size_bytes"] >= 0):
            bad.add("size_bytes")
        if s("sha256") and not HEX_RE.fullmatch(d["sha256"]):
            bad.add("sha256")
        if s("redistribution") and d["redistribution"] not in ("mirror", "fetch-only"):
            bad.add("redistribution")
        urls = d.get("urls")
        if not (isinstance(urls, list) and urls and all(isinstance(u, str) and URL_RE.fullmatch(u) for u in urls)):
            bad.add("urls")
        bad |= {k for k in ("notes", "release_ref", "terms_url") if k in d and not s(k)}
        bad |= {k for k in ("contains_standards_text", "contains_customer_data")
                if k in d and not isinstance(d[k], bool)}
        expr = spdx(d["licence"]) if s("licence") else None
        if s("licence") and expr is None:
            bad.add("licence")
        # LicenseRef- as the licence or as the exception after WITH needs terms_url.
        if expr is not None and any(p.startswith("LicenseRef-") for x in names(expr) for p in x.split(" WITH ")):
            if not (s("terms_url") and URL_RE.fullmatch(d["terms_url"])):
                bad.add("terms_url")
        if bad:
            out.add(("DATA-SCHEMA", sid))
        if s("owner") and d["owner"] not in domains:
            out.add(("DATA-OWNER", sid))
        if expr is not None and d.get("redistribution") == "mirror" and not ok(expr, allowed):
            out.add(("DATA-REDIST", sid))
        if d.get("contains_standards_text") is True:
            out.add(("DATA-STANDARDS", sid))
        if d.get("contains_customer_data") is True and not s("release_ref"):
            out.add(("DATA-CUSTOMER", sid))
    out |= {("DATA-DUP", k) for k, v in seen.items() if v > 1}
    return [f"{c} {sid}" for sid, c in sorted((sid, c) for c, sid in out)]


def main():
    domains_txt = "# Level-1 domain IDs of the spec tree (ECO.DATA.CAT fixture). MIT OR Apache-2.0.\n" + "\n".join(
        DOMAINS) + "\n"
    (HERE / "domains.txt").write_text(domains_txt, encoding="utf-8", newline="\n")
    lines = ["# ECO.DATA.CAT expected findings, labelled by hand from the spec rules. <file>\\t<CODE subject>|ok"]
    for name, body in FIXTURES.items():
        text = HEAD.format(name) + body
        got = check(text, set(DOMAINS))
        assert got == EXPECT[name], f"{name}: checker {got} != hand label {EXPECT[name]}"
        (HERE / name).write_text(text, encoding="utf-8", newline="\n")
        lines += [f"{name}\t{f}" for f in EXPECT[name]] or [f"{name}\tok"]
    (HERE / "expected.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(FIXTURES)} catalogues, domains.txt and expected.txt")


if __name__ == "__main__":
    main()
