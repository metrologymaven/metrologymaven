"""Writes the synthetic fixtures of spec node OUTR.DOCS.QA (CC-BY-4.0).

Every word here is project-authored and synthetic; third-party URLs use reserved example domains.

Layout:
  ok/
    frame.css
    book/index.html               valid relative links and fragments
    book/units.html               target page with id="si"
  broken/
    book/index.html               links to ghost.html (line 10) and units.html#nope (line 11)
    book/units.html               exists, but has no id="nope"
  external/
    book/links.html               three external http:// URLs (ok, missing, hang) for AC-3
  tracker/
    book/index.html               page loading a third-party script (AC-4)
  lint/
    book/intro.md                 hand-written book source with CL001 wording (AC-5)
    ref/cli/demo.read_item@1.html generated reference page with the same word (excluded from lint)

Run from the repository root:
  python fixtures/outr/docs/qa/make_fixtures.py          (write)
  python fixtures/outr/docs/qa/make_fixtures.py --check  (exit 1 if any file differs)
"""

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
GENERATED_DIRS = ["ok", "broken", "external", "tracker", "lint"]

FRAME_CSS = "body{margin:0}\n"

OK_INDEX = """\
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Introduction</title>
<link rel="stylesheet" href="../frame.css">
</head>
<body>
<a href="#main-content">Skip to content</a>
<main id="main-content">
<h1>Introduction</h1>
<p>Read <a href="units.html#si">SI units</a>.</p>
</main>
</body>
</html>
"""

OK_UNITS = """\
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Units</title>
<link rel="stylesheet" href="../frame.css">
</head>
<body>
<main id="main-content">
<h1>Units</h1>
<h2 id="si">SI metres</h2>
<p>Return to <a href="index.html">Introduction</a>.</p>
</main>
</body>
</html>
"""

BROKEN_INDEX = """\
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Broken links fixture</title>
</head>
<body>
<main id="main-content">
<h1>Broken links</h1>
<p>Missing file: <a href="ghost.html">Ghost</a>.</p>
<p>Missing anchor: <a href="units.html#nope">Missing section</a>.</p>
</main>
</body>
</html>
"""

BROKEN_UNITS = """\
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Units</title>
</head>
<body>
<main id="main-content">
<h1>Units</h1>
<h2 id="si">SI metres</h2>
</main>
</body>
</html>
"""

EXTERNAL_LINKS = """\
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>External links fixture</title>
</head>
<body>
<main id="main-content">
<h1>External references</h1>
<ul>
<li><a href="http://127.0.0.1:1/ok">Good link</a></li>
<li><a href="http://127.0.0.1:1/missing">Broken 404 link</a></li>
<li><a href="http://127.0.0.1:1/hang">Hanging link</a></li>
</ul>
</main>
</body>
</html>
"""

TRACKER_INDEX = """\
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Tracker fixture</title>
<script src="https://tracker.example/analytics.js"></script>
</head>
<body>
<main id="main-content">
<h1>Tracker page</h1>
<p>Synthetic fixture with a third-party script.</p>
</main>
</body>
</html>
"""

LINT_BOOK_MD = """\
# Synthetic book chapter

The software is certified for dimensional inspection.
"""

LINT_REF_HTML = """\
<article class="ref-page ref-cli">
<h1><code>demo.read_item@1</code></h1>
<p class="summary">Generated reference page mentioning certified fixtures.</p>
<pre class="help">Synthetic help text: certified test output.</pre>
</article>
"""


def expected_files():
    return {
        "ok/frame.css": FRAME_CSS,
        "ok/book/index.html": OK_INDEX,
        "ok/book/units.html": OK_UNITS,
        "broken/book/index.html": BROKEN_INDEX,
        "broken/book/units.html": BROKEN_UNITS,
        "external/book/links.html": EXTERNAL_LINKS,
        "tracker/book/index.html": TRACKER_INDEX,
        "lint/book/intro.md": LINT_BOOK_MD,
        "lint/ref/cli/demo.read_item@1.html": LINT_REF_HTML,
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
