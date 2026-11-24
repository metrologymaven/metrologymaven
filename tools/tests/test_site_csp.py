"""Tier-5 tests for site/build.py CSP safety, HTML escaping, and site/worker.js routing."""

from __future__ import annotations

import importlib.util
import json
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BUILD_PY = ROOT / "site" / "build.py"
WORKER_JS = ROOT / "site" / "worker.js"


def load_build_module():
    spec = importlib.util.spec_from_file_location("site_build", BUILD_PY)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class SiteCspTier5Test(unittest.TestCase):
    def test_all_generated_pages_have_zero_inline_style_and_escape_metadata(self) -> None:
        mod = load_build_module()
        with tempfile.TemporaryDirectory(prefix="mm-site-csp-") as tmp:
            dist = Path(tmp) / "dist"
            orig_dist = mod.DIST
            try:
                mod.DIST = dist
                rc = mod.main()
            finally:
                mod.DIST = orig_dist
            self.assertEqual(rc, 0)
            html_files = list(dist.rglob("*.html"))
            self.assertGreater(len(html_files), 10)
            for p in html_files:
                text = p.read_text(encoding="utf-8")
                self.assertIsNone(
                    re.search(r"\bstyle\s*=", text, re.I),
                    f"inline style= attribute found in {p.relative_to(dist)}",
                )

            # Verify HTML escaping of rev / phase / id.
            d = mod.load()
            d["rev"] = 'abc<script>1</script>'
            d["date"] = '2026-01-01" onclick="x()'
            for n in d["domains"][:1]:
                n.meta["phase"] = "<b id=xss>P0</b>"
            _, status_html = mod.status(d)
            self.assertNotIn("<script>1</script>", status_html)
            self.assertNotIn("<b id=xss>", status_html)
            self.assertIn("&lt;b id=xss&gt;P0&lt;/b&gt;", status_html)

            t = dict(d["topics"][0])
            t["phase"] = "<img src=x onerror=alert(1)>"
            _, topic_html = mod.guide_topic_page(t, d)
            hero_html = topic_html.split('<script type="application/json"', 1)[0]
            self.assertNotIn("<img src=x onerror=alert(1)>", hero_html)
            self.assertIn("&lt;img src=x onerror=alert(1)&gt;", hero_html)

    def test_worker_js_normalizes_percent_encoding_and_dot_segments(self) -> None:
        node = shutil.which("node")
        if not node:
            self.skipTest("node not installed")
        worker_url = WORKER_JS.as_uri()
        probe = (
            f"import worker from {json.dumps(worker_url)};\n"
            "const env = { ASSETS: { fetch: async (req) => new Response('ok', { headers: { 'content-type': 'text/html' } }) } };\n"
            "const paths = ['/%6eotes/foo', '/%67uide/fits/', '/guide/..%2fstatus/', '/guide/%2e%2e/status/', '/%ZZ', '/guide-fake/'];\n"
            "const out = {};\n"
            "for (const p of paths) {\n"
            "  const res = await worker.fetch(new Request('https://metrologymaven.io' + p), env);\n"
            "  out[p] = { status: res.status, csp: res.headers.get('Content-Security-Policy') || '' };\n"
            "}\n"
            "console.log(JSON.stringify(out));\n"
        )
        r = subprocess.run([node, "--input-type=module", "-e", probe], capture_output=True, text=True, check=False)
        self.assertEqual(r.returncode, 0, r.stderr)
        data = json.loads(r.stdout.strip())
        self.assertEqual(data["/%6eotes/foo"]["status"], 301)
        self.assertIn("script-src 'self' 'wasm-unsafe-eval'", data["/%67uide/fits/"]["csp"])
        for blocked in ("/guide/..%2fstatus/", "/guide/%2e%2e/status/", "/%ZZ", "/guide-fake/"):
            csp = data[blocked]["csp"]
            self.assertRegex(csp, r"(?:^|;\s*)script-src\s+'none'\s*(?:;|$)", f"{blocked}: {csp}")
            self.assertNotIn("wasm-unsafe-eval", csp)


if __name__ == "__main__":
    unittest.main()
