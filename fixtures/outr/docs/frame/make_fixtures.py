"""Writes the synthetic fixtures of spec node OUTR.DOCS.FRAME.NAV (CC-BY-4.0).

Every word and asset here is project-authored and synthetic.

Layout:
  src/
    nav.toml                      navigation tree for all six pages (with nested child)
    book/index.md                 user guide entry page (links, image, cap tags, raw HTML test)
    book/units.md                 user guide chapter (table, fenced code, inline code)
    book/replay.md                user guide chapter (headings, lists, blockquote)
    contributor/guide.md          contributor guide page (front matter, sh run fence)
    assets/mark.svg               tiny project-authored SVG brand mark
  ref/
    cli/demo.read_item@1.html     CLI reference HTML fragment (OUTR.DOCS.REF layout)
    mcp/demo.read_item@1.html     MCP reference HTML fragment (OUTR.DOCS.REF layout)
  nav-missing/
    nav.toml                      names book/ghost.md (AC-4 error fixture)
    book/index.md                 one existing page

Run from the repository root:
  python fixtures/outr/docs/frame/make_fixtures.py          (write)
  python fixtures/outr/docs/frame/make_fixtures.py --check  (exit 1 if any file differs)
"""

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
GENERATED_DIRS = ["src", "ref", "nav-missing"]

NAV_OK = """\
# Navigation tree for OUTR.DOCS.FRAME.NAV fixture (synthetic).
version = 1

[[section]]
title = "User guide"

  [[section.items]]
  title = "Introduction"
  source = "book/index.md"

  [[section.items]]
  title = "Units"
  source = "book/units.md"
  children = [
    { title = "Replay", source = "book/replay.md" },
  ]

[[section]]
title = "Contributor guide"

  [[section.items]]
  title = "Contributor guide"
  source = "contributor/guide.md"

[[section]]
title = "Reference"

  [[section.items]]
  title = "demo.read_item@1 (CLI)"
  source = "ref/cli/demo.read_item@1.html"

  [[section.items]]
  title = "demo.read_item@1 (MCP)"
  source = "ref/mcp/demo.read_item@1.html"
"""

BOOK_INDEX_MD = """\
---
title: User guide introduction
---
# User guide introduction

![Project mark](../assets/mark.svg)

Welcome to the synthetic docs fixture. Read [Units](units.md),
[Replay exact mode](replay.md#exact-mode), the
[Contributor guide](../contributor/guide.md), or the
[CLI reference](../ref/cli/demo.read_item@1.html).

<!-- cap: DEMO.CAP.01 -->

Capability note <!-- cap-planned: DEMO.CAP.02 --> Not yet available in this release.

Raw tags like <script>alert(1)</script> and <!-- not-a-cap --> must appear escaped.
"""

BOOK_UNITS_MD = """\
# Units and quantities

The kernel computes in SI metres (`m`) and keeps authored decimals with their unit.
Command names such as demo.read_item_two keep literal underscores.

| Quantity | Canonical unit | Example |
| --- | :---: | ---: |
| Length | `m` | 0.025 |
| Angle | `rad` | 1.5708 |

## Command example

```sh
fid demo read-item --unit mm
```

See also [Introduction](index.md).
"""

BOOK_REPLAY_MD = """\
# Replay

Every committed command is written to the semantic journal.

## Exact mode

> Re-running the same inputs under exact mode yields bit-identical outputs.

- First check: compare journal hashes.
- Second check: compare output digests.

1. Open the project archive.
2. Run the verifier offline.
"""

CONTRIB_GUIDE_MD = """\
---
title: Contributor guide
revision: 0000000000000000000000000000000000000000
---
# Contributor guide

Run the tree checker before opening a pull request:

```sh run
python tools/scope.py check
```

Return to the [User guide](../book/index.md).
"""

MARK_SVG = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" role="img" aria-label="Project mark">
<rect x="2" y="2" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2"/>
<circle cx="12" cy="12" r="4" fill="currentColor"/>
</svg>
"""

REF_CLI_HTML = """\
<article class="ref-page ref-cli">
<h1><code>demo.read_item@1</code></h1>
<p class="summary">Synthetic fixture command: read item.</p>
<dl class="ref-facts">
<dt>Command</dt><dd><code>demo.read_item@1</code></dd>
<dt>Side-effect class</dt><dd>R0</dd>
<dt>Determinism</dt><dd>D0</dd>
<dt>Human only</dt><dd>false</dd>
<dt>MCP</dt><dd>exposed</dd>
</dl>
<pre class="help">Synthetic fixture command: read item.&#10;Compare a &lt; b &amp; c &gt; d.</pre>
<h2>CLI</h2>
<p><code>fid demo read-item</code></p>
</article>
"""

REF_MCP_HTML = """\
<article class="ref-page ref-mcp">
<h1><code>demo.read_item@1</code></h1>
<p class="summary">Synthetic fixture command: read item.</p>
<dl class="ref-facts">
<dt>Command</dt><dd><code>demo.read_item@1</code></dd>
<dt>Side-effect class</dt><dd>R0</dd>
<dt>Determinism</dt><dd>D0</dd>
<dt>Human only</dt><dd>false</dd>
<dt>MCP</dt><dd>exposed</dd>
</dl>
<pre class="help">Synthetic fixture command: read item.</pre>
<h2>MCP tool</h2>
<p><code>demo_read_item</code></p>
</article>
"""

NAV_MISSING = """\
version = 1

[[section]]
title = "User guide"

  [[section.items]]
  title = "Introduction"
  source = "book/index.md"

  [[section.items]]
  title = "Ghost chapter"
  source = "book/ghost.md"
"""

NAV_MISSING_INDEX_MD = """\
# Introduction

Synthetic page for the missing-nav-source fixture.
"""


def expected_files():
    return {
        "src/nav.toml": NAV_OK,
        "src/book/index.md": BOOK_INDEX_MD,
        "src/book/units.md": BOOK_UNITS_MD,
        "src/book/replay.md": BOOK_REPLAY_MD,
        "src/contributor/guide.md": CONTRIB_GUIDE_MD,
        "src/assets/mark.svg": MARK_SVG,
        "ref/cli/demo.read_item@1.html": REF_CLI_HTML,
        "ref/mcp/demo.read_item@1.html": REF_MCP_HTML,
        "nav-missing/nav.toml": NAV_MISSING,
        "nav-missing/book/index.md": NAV_MISSING_INDEX_MD,
    }


def existing_files():
    found = {}
    for top in GENERATED_DIRS:
        root = os.path.join(HERE, top)
        if not os.path.isdir(root):
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames.sort()
            for name in sorted(filenames):
                full = os.path.join(dirpath, name)
                rel = os.path.relpath(full, HERE).replace("\\", "/")
                with open(full, "rb") as fh:
                    found[rel] = fh.read()
    return found


def main():
    check = "--check" in sys.argv[1:]
    if any(a != "--check" for a in sys.argv[1:]):
        sys.stderr.write("usage: make_fixtures.py [--check]\n")
        return 2

    want = {rel: text.encode("utf-8") for rel, text in expected_files().items()}
    if check:
        got = existing_files()
        bad = []
        for rel in sorted(set(want) | set(got)):
            if rel not in want:
                bad.append(f"unexpected file: {rel}")
            elif rel not in got:
                bad.append(f"missing file: {rel}")
            elif got[rel] != want[rel]:
                bad.append(f"differs: {rel}")
        if bad:
            for line in bad:
                sys.stderr.write(line + "\n")
            return 1
        return 0

    for rel, data in sorted(want.items()):
        full = os.path.join(HERE, rel)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "wb") as fh:
            fh.write(data)
    return 0


if __name__ == "__main__":
    sys.exit(main())
