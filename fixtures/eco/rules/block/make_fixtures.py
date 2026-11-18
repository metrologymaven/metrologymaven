#!/usr/bin/env python3
"""Independent reference for spec node ECO.RULES.BLOCK (generated house-rules blocks).

Writes, next to this script:
- one.md, unbalanced.md, stray-end.md, nested.md, id-mismatch.md, bad-marker.md: hand-written Markdown
- expected.txt: what parsing each file must give (blocks with their fields, or the error kind and line)
- render-cases-v1.txt: 200 bodies from random.Random(42) with the exact rendered block text (AC-5)

This is a separate implementation of the format (regular expressions, str.split), written from the
spec, so the Rust crate fid-rules-block is checked against it rather than against itself.
Run:  python fixtures/eco/rules/block/make_fixtures.py
"""

import hashlib
import random
import re
from pathlib import Path

HERE = Path(__file__).parent

ID = r"[a-z][a-z0-9-]{1,31}"
REPO = r"[A-Za-z0-9][A-Za-z0-9._/-]{0,99}"
BEGIN = re.compile(rf"<!-- house-rules:begin block=({ID}) source=({REPO})@([0-9a-f]{{40}}) sha256=([0-9a-f]{{64}}) -->")
END = re.compile(rf"<!-- house-rules:end block=({ID}) -->")
CANDIDATE = "<!-- house-rules:"


class BlockError(Exception):
    def __init__(self, kind, line):
        super().__init__(f"{kind} at line {line}")
        self.kind, self.line = kind, line


def digest(body):
    return hashlib.sha256(body.replace("\r\n", "\n").encode("utf-8")).hexdigest()


def render(block_id, repo, rev, body):
    sha = digest(body)
    return (
        f"<!-- house-rules:begin block={block_id} source={repo}@{rev} sha256={sha} -->\n"
        f"{body}\n<!-- house-rules:end block={block_id} -->\n"
    )


def find_blocks(text):
    lines = text.split("\n")
    out, open_ = [], None
    for i, raw in enumerate(lines):
        n = i + 1
        line = raw[:-1] if raw.endswith("\r") else raw
        if not line.lstrip(" \t").startswith(CANDIDATE):
            continue
        b, e = BEGIN.fullmatch(line), END.fullmatch(line)
        if b:
            if open_ is not None:
                raise BlockError("nested", n)
            open_ = (n, b.groups())
        elif e:
            if open_ is None:
                raise BlockError("unbalanced", n)
            (bn, (bid, repo, rev, sha)) = open_
            if e.group(1) != bid:
                raise BlockError("id-mismatch", n)
            body_lines = lines[bn : n - 1]
            if body_lines and body_lines[-1].endswith("\r"):
                body_lines[-1] = body_lines[-1][:-1]
            out.append(dict(id=bid, repo=repo, rev=rev, sha=sha, body="\n".join(body_lines), begin=bn, end=n))
            open_ = None
        else:
            raise BlockError("bad-marker", n)
    if open_ is not None:
        raise BlockError("unbalanced", open_[0])
    return out


REV = "0123456789abcdef0123456789abcdef01234567"
REV2 = "fedcba9876543210fedcba9876543210fedcba98"
REPO_ID = "metrologymaven/fid-contrib"
CORE_BODY = (
    "## House rules (core)\n"
    "\n"
    "1. A person is accountable for every change an agent opens.\n"
    "2. Check, then claim. Report only what ran.\n"
    "<!-- an ordinary comment stays in the body -->\n"
    "Units: µm, ±0.5 °C, Ø12 — non-ASCII text is hashed as UTF-8.\n"
    "   trailing spaces are kept   "
)
RUST_BODY = "## Rust\n\nNo unsafe, no unwrap in library code."


def doc_one():
    return "# AGENTS.md\n\nRepo-local text before the block.\n\n" + render(
        "core", REPO_ID, REV, CORE_BODY
    ) + "\nRepo-local text after the block.\n"


def doc_two():
    return (
        "Intro.\n"
        + render("core", REPO_ID, REV, CORE_BODY)
        + render("rust", "metrologymaven/metrologymaven", REV2, RUST_BODY)
        + render("empty-body", "a", REV, "")
        + "Outro, no final newline."
    )


def tampered():
    # A valid block whose body was edited after rendering: parsing succeeds, the digest differs.
    return render("core", REPO_ID, REV, CORE_BODY).replace("accountable", "responsible")


ERR_DOCS = {
    "unbalanced.md": "Text.\n" + render("core", REPO_ID, REV, "x").split("<!-- house-rules:end")[0] + "more text\n",
    "stray-end.md": "Text.\n\n<!-- house-rules:end block=core -->\n",
    "nested.md": "Text.\n"
    + "<!-- house-rules:begin block=outer source=r@" + REV + " sha256=" + digest("") + " -->\n"
    + "inner follows\n"
    + render("inner", REPO_ID, REV, "body")
    + "<!-- house-rules:end block=outer -->\n",
    "id-mismatch.md": render("core", REPO_ID, REV, "a\nb").replace("end block=core", "end block=rust"),
    "bad-marker.md": "Text.\n" + render("core", REPO_ID, REV, "a").replace("sha256=", "sha256=sha256:", 1),
}


def write(name, text):
    (HERE / name).write_bytes(text.encode("utf-8"))


def expected_line(name, text):
    try:
        blocks = find_blocks(text)
    except BlockError as e:
        return [f"{name}\terror\t{e.kind}\t{e.line}"]
    rows = [f"{name}\tok\t{len(blocks)}"]
    for b in blocks:
        rows.append(
            "\t".join(
                ["block", b["id"], b["repo"], b["rev"], b["sha"], digest(b["body"]), str(b["begin"]), str(b["end"]),
                 b["body"].encode("utf-8").hex()]
            )
        )
    return rows


def gen_body(rng):
    words = ["rule", "agent", "µm", "Ø", "—", "check", "<!-- note -->", "`code`", "|a|b|", "#", "  ", "\t", "<!--"]
    n = rng.randrange(0, 8)
    lines = []
    for _ in range(n):
        k = rng.randrange(0, 6)
        line = " ".join(rng.choice(words) for _ in range(k))
        if rng.random() < 0.1:
            line += "\rmid"
        lines.append(line)
    sep = "\r\n" if rng.random() < 0.3 else "\n"
    body = sep.join(lines)
    if rng.random() < 0.2:
        body += sep
    return body


def main():
    docs = {"one.md": doc_one(), "two.md": doc_two(), "tampered.md": tampered(), **ERR_DOCS}
    rows = [
        "# ECO.RULES.BLOCK expected parse results, from make_fixtures.py (independent reference). CC-BY-4.0.",
        "# <file> ok <n>, then n rows: block <id> <repo> <rev> <recorded sha> <sha of body> <begin> <end> <body utf-8 hex>",
        "# <file> error <kind> <1-based line>",
    ]
    for name, text in docs.items():
        write(name, text)
        rows += expected_line(name, text)
        crlf = text.replace("\n", "\r\n")
        rows += expected_line(name + "+crlf", crlf)
    (HERE / "expected.txt").write_text("\n".join(rows) + "\n", encoding="utf-8", newline="\n")

    rng = random.Random(42)
    cases = ["# ECO.RULES.BLOCK AC-5: <id> <repo> <rev> <sha256 of body> <body hex> <rendered hex>. CC-BY-4.0."]
    for i in range(200):
        bid = "b" + "".join(rng.choice("abcdefghijklmnopqrstuvwxyz0123456789-") for _ in range(rng.randrange(1, 32)))
        repo = rng.choice(["a", "metrologymaven/fid-contrib", "Org.Name/repo_1", "x" * 100])
        rev = "".join(rng.choice("0123456789abcdef") for _ in range(40))
        body = gen_body(rng)
        text = render(bid, repo, rev, body)
        got = find_blocks(text)
        assert len(got) == 1 and got[0]["body"] == body and got[0]["sha"] == digest(body), (i, body)
        assert got[0]["id"] == bid and got[0]["repo"] == repo and got[0]["rev"] == rev
        cases.append("\t".join([bid, repo, rev, digest(body), body.encode("utf-8").hex(), text.encode("utf-8").hex()]))
    (HERE / "render-cases-v1.txt").write_text("\n".join(cases) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
