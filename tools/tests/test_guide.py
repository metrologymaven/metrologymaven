"""End-to-end verification tests for the Executable Metrology Field Guide
(spec nodes OUTR.DOCS.GUIDE.MAP, OUTR.DOCS.GUIDE.TOPICS, OUTR.DOCS.GUIDE.SITE).

Checks:
1. Dimensional Metrology Field Map schema, live spec node existence, D-054 tiers, vocab hygiene.
2. All 6 Field Guide topic chapters: crate existence, spec node existence, D-045 provisional flags,
   no standards-text leakage or incumbent comparisons (including lab instructions & preset text).
3. Bit-exact execution of every preset through the native `fid-guide-wasm` binary.
4. Bit-exact execution of every preset through the browser WASI shim (`site/src/guide-lab.js`)
   running `site/dist/guide/fid-guide-wasm.wasm` under Node.js WebAssembly, plus single-doc canon.
5. `site/build.py` output: `/guide/` + all topic pages built, < 500 KB budget, zero third-party
   origins, zero inline executable `<script>` tags, zero inline `style=` attributes (strict
   `style-src 'self'`), and scoped CSP in `site/worker.js`.
6. The staged `site/dist/guide/fid-guide-wasm.wasm` embeds no user-home path.

The WASM parts (4 and 6) skip with a visible reason when the binary is not built, except under
`FID_GUIDE_REQUIRE_WASM=1` (set by `.github/workflows/site.yml`), where they fail.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
import scope  # noqa: E402

FIELD_MAP = ROOT / "guide" / "map" / "field-map.json"
TOPICS_DIR = ROOT / "guide" / "topics"
INCUMBENTS = ROOT / "house-rules" / "vocab" / "incumbents.txt"
NATIVE_BIN = ROOT / "target" / "debug" / "fid-guide-wasm"
WASM_BIN = ROOT / "target" / "wasm32-wasip1" / "release" / "fid-guide-wasm.wasm"
DIST = ROOT / "site" / "dist"
STAGED_WASM = DIST / "guide" / "fid-guide-wasm.wasm"
# The site workflow sets this: there a missing WASM binary is a failure, never a skip. Other
# runners (the fixture-checks step, a contributor's machine) skip the WASM parts visibly.
REQUIRE_WASM = os.environ.get("FID_GUIDE_REQUIRE_WASM") == "1"
# User-home paths a build must not leak into the staged binary (#567). `/home/runner` is the
# GitHub runner's own home and is allowed.
HOME_PATH_RES = (
    re.compile(rb"[A-Za-z]:[\\/]+Users[\\/]+", re.I),
    re.compile(rb"/home/(?!runner(?![A-Za-z0-9._-]))[A-Za-z0-9._-]+"),
    re.compile(rb"/Users/[A-Za-z0-9._-]+/"),
)


def _load_incumbent_terms() -> list[str]:
    terms = []
    for line in INCUMBENTS.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s and not s.startswith("#"):
            terms.append(s)
    return terms


def _check_vocab_hygiene(text: str, where: str, incumbents: list[str]) -> list[str]:
    errors: list[str] = []
    # VC002/VC003: never name incumbent products in Field Guide copy.
    for term in incumbents:
        if re.search(rf"\b{re.escape(term)}\b", text):
            errors.append(f"{where}: mentions external incumbent name `{term}`")
    # VC001 / CL001: regulated claim words require an explicit standard designator in the same sentence.
    claim_re = re.compile(
        r"\b(certif(?:y|ies|ied|ying|ication)|compliant|conformant|validat(?:e|es|ed|ing)|traceab(?:le|ility))\b",
        re.I,
    )
    desig_re = re.compile(r"\b(ISO|ASME|JCGM|VDA|AIAG|ASTM|RFC|IEEE|PTB|VDI)\b")
    for sent in re.split(r"(?<=[.!?])\s+", text):
        if claim_re.search(sent) and not desig_re.search(sent):
            errors.append(f"{where}: unqualified regulated claim wording in sentence: {sent!r}")
    return errors


class FieldGuideTest(unittest.TestCase):
    def _require_wasm_or_skip(self, what: str) -> None:
        """Fails under FID_GUIDE_REQUIRE_WASM=1 when the WASM binary is missing, else skips visibly."""
        if WASM_BIN.exists():
            return
        msg = f"{what}: {WASM_BIN.relative_to(ROOT)} is not built"
        if REQUIRE_WASM:
            self.fail(msg + " (FID_GUIDE_REQUIRE_WASM=1); run "
                      "cargo build --release --target wasm32-wasip1 -p fid-guide-wasm first")
        self.skipTest(msg + "; SKIPPED (set FID_GUIDE_REQUIRE_WASM=1 to make this a failure)")

    @classmethod
    def setUpClass(cls) -> None:
        rep = scope.Report()
        cls.nodes = scope.load_nodes(rep)
        cls.by_id = {n.id: n for n in cls.nodes}
        cls.incumbents = _load_incumbent_terms()
        cls.fmap = json.loads(FIELD_MAP.read_text(encoding="utf-8"))
        cls.topics = [
            json.loads(p.read_text(encoding="utf-8"))
            for p in sorted(TOPICS_DIR.glob("*.json"))
        ]
        cls.topics.sort(key=lambda t: t["order"])

    def test_ac1_field_map_schema_and_live_spec_nodes(self) -> None:
        self.assertEqual(self.fmap["schema"], "field-map/v1")
        self.assertEqual(self.fmap["decision_ref"], "D-054")
        tier_ids = {t["id"] for t in self.fmap["tiers"]}
        self.assertEqual(tier_ids, {"core-1.0", "core-post-1.0", "plugin-companion"})
        self.assertGreaterEqual(len(self.fmap["pillars"]), 10)

        by_pillar = {p["id"]: p for p in self.fmap["pillars"]}
        self.assertEqual(by_pillar["msa-quality-stats"]["tier"], "core-post-1.0")
        self.assertEqual(by_pillar["surface-texture-form"]["tier"], "core-post-1.0")
        self.assertEqual(by_pillar["calibration-lab-verification"]["tier"], "core-post-1.0")
        for pid in (
            "vision-2d-optical",
            "industrial-xray-ct",
            "on-machine-cnc-probing",
            "micro-nano-semi-optics",
            "adjacent-dim-engineering",
        ):
            self.assertEqual(by_pillar[pid]["tier"], "plugin-companion")

        slugs = {t["slug"] for t in self.topics}
        vocab_errors: list[str] = []
        for p in self.fmap["pillars"]:
            self.assertIn(p["tier"], tier_ids)
            self.assertGreater(p["scale_max_m"], p["scale_min_m"])
            for dom in p["spec_domains"]:
                self.assertIn(dom, self.by_id, f"field-map references unknown spec node `{dom}`")
            if p.get("guide_slug") is not None:
                self.assertIn(p["guide_slug"], slugs)
            vocab_errors += _check_vocab_hygiene(
                p["title"] + ". " + p["why"], f"field-map:{p['id']}", self.incumbents
            )
        self.assertEqual(vocab_errors, [], "\n".join(vocab_errors))

    def test_ac2_topics_structure_crates_nodes_and_provisional_labels(self) -> None:
        self.assertEqual(len(self.topics), 6)
        by_slug = {t["slug"]: t for t in self.topics}
        for req_slug in ("fits", "gdt", "uncertainty", "decisions", "numerics-replay", "field-map"):
            self.assertIn(req_slug, by_slug)

        self.assertTrue(by_slug["gdt"]["provisional"])
        self.assertTrue(by_slug["decisions"]["provisional"])

        vocab_errors: list[str] = []
        for t in self.topics:
            self.assertEqual(t["schema"], "guide-topic/v1")
            for crate in t["crates"]:
                self.assertTrue((ROOT / "crates" / crate / "Cargo.toml").exists(), f"missing crate {crate}")
            for nid in t["spec_nodes"]:
                self.assertIn(nid, self.by_id, f"topic {t['slug']} references unknown spec node {nid}")
            self.assertTrue(t.get("cli_example"), f"topic {t['slug']} missing cli_example")
            lab = t.get("lab", {})
            chunks = (
                [t["title"], t["subtitle"], lab.get("instructions", "")]
                + [p for s in t["sections"] for p in s["paragraphs"]]
                + [pr["label"] + ". " + pr["description"] for pr in lab.get("presets", [])]
            )
            vocab_errors += _check_vocab_hygiene(" ".join(chunks), f"topic:{t['slug']}", self.incumbents)

        # Also check user-visible string literals in site/src/guide-lab.js.
        js_src = (ROOT / "site" / "src" / "guide-lab.js").read_text(encoding="utf-8")
        js_strings = " ".join(re.findall(r'"([^"\\]{4,})"', js_src))
        vocab_errors += _check_vocab_hygiene(js_strings, "site/src/guide-lab.js", self.incumbents)
        self.assertEqual(vocab_errors, [], "\n".join(vocab_errors))

    def test_ac3_native_kernel_verifies_every_preset_bit_for_bit(self) -> None:
        if not NATIVE_BIN.exists():
            subprocess.run(["cargo", "build", "-q", "-p", "fid-guide-wasm"], cwd=ROOT, check=True)
        checked = 0
        for t in self.topics:
            for pr in t.get("lab", {}).get("presets", []):
                payload = json.dumps(pr["request"]).encode("utf-8")
                proc = subprocess.run([str(NATIVE_BIN)], input=payload, capture_output=True, check=False)
                self.assertEqual(proc.returncode, 0, f"{t['slug']}/{pr['id']}: {proc.stdout.decode()}")
                resp = json.loads(proc.stdout.decode("utf-8"))
                self.assertTrue(resp["ok"])
                self.assertEqual(
                    resp["result_digest"],
                    pr["expected_digest"],
                    f"digest mismatch for {t['slug']}/{pr['id']}",
                )
                checked += 1
        self.assertGreaterEqual(checked, 15)

    def test_ac4_site_build_csp_and_browser_wasi_bit_identity(self) -> None:
        proc = subprocess.run(
            [sys.executable, str(ROOT / "site" / "build.py")],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

        guide_pages = [DIST / "guide" / "index.html"] + [
            DIST / "guide" / t["slug"] / "index.html" for t in self.topics
        ]
        for gp in guide_pages:
            self.assertTrue(gp.exists(), f"missing built page {gp}")
            html_text = gp.read_text(encoding="utf-8")
            self.assertLess(len(html_text.encode("utf-8")), 500_000)
            # Zero inline style= attributes (strict style-src 'self' CSP).
            self.assertIsNone(re.search(r'\bstyle="', html_text), f"inline style attribute in {gp}")
            # No inline executable <script> tags (only type="application/json" or src="/guide-lab.js").
            for m in re.finditer(r"<script\b([^>]*)>", html_text, re.I):
                attrs = m.group(1)
                is_json = 'type="application/json"' in attrs
                is_same_origin_src = 'src="/guide-lab.js"' in attrs
                self.assertTrue(is_json or is_same_origin_src, f"disallowed script tag in {gp}: <script{attrs}>")
            # Zero third-party origins in src= links (REQ-OUTR.SITE-007).
            self.assertIsNone(re.search(r'src="https?://', html_text))
            # No-JS fallback preset tables must render real kernel values, never a generic placeholder.
            self.assertNotIn('<td class="mono">verified</td>', html_text, f"placeholder fallback in {gp}")

        worker = (ROOT / "site" / "worker.js").read_text(encoding="utf-8")
        self.assertIn('"/guide/"', worker)
        self.assertIn("'self' 'wasm-unsafe-eval'", worker)
        self.assertIn("'none'", worker)

        js_src = (ROOT / "site" / "src" / "guide-lab.js").read_text(encoding="utf-8")
        self.assertIsNone(re.search(r"\.style\.", js_src), "runtime .style. write violates strict style-src 'self'")

        # The non-WASM checks above have passed; the WASM bit-identity part follows.
        self._require_wasm_or_skip("AC-4 browser WASI bit-identity")
        staged_wasm = STAGED_WASM
        self.assertTrue(staged_wasm.exists())
        expected_sha = "sha256:" + hashlib.sha256(staged_wasm.read_bytes()).hexdigest()
        self.assertIn(expected_sha, (DIST / "guide" / "fits" / "index.html").read_text(encoding="utf-8"))

        js_check = r"""
const fs = require("fs");
const path = require("path");
const { runKernelWasm } = require(path.resolve("site/src/guide-lab.js"));
const wasmMod = new WebAssembly.Module(fs.readFileSync(path.resolve("site/dist/guide/fid-guide-wasm.wasm")));
const topicsDir = path.resolve("guide/topics");
let count = 0;
for (const file of fs.readdirSync(topicsDir).sort()) {
  if (!file.endsWith(".json")) continue;
  const topic = JSON.parse(fs.readFileSync(path.join(topicsDir, file), "utf8"));
  for (const pr of (topic.lab && topic.lab.presets) || []) {
    const out = runKernelWasm(wasmMod, pr.request);
    if (out.exitCode !== 0 || !out.response.ok) {
      throw new Error(`WASM failed on ${file}/${pr.id}: ${out.raw}`);
    }
    if (out.response.result_digest !== pr.expected_digest) {
      throw new Error(`WASM digest mismatch on ${file}/${pr.id}: got ${out.response.result_digest} != ${pr.expected_digest}`);
    }
    // Verify Uncertainty Lab UI um-to-m slider round-trip preserves exact preset digests.
    if (pr.op === "unc.propagate") {
      const req2 = JSON.parse(JSON.stringify(pr.request));
      const i0 = req2.params.inputs[0];
      const s1 = Number(((i0.sigma != null ? i0.sigma : i0.half_width) * 1e6).toFixed(6));
      const scaled = Number((s1 * 1e-6).toFixed(9));
      if (i0.sigma != null) i0.sigma = scaled;
      else i0.half_width = scaled;
      const out2 = runKernelWasm(wasmMod, req2);
      if (out2.response.result_digest !== pr.expected_digest) {
        throw new Error(`UI slider round-trip digest drift on ${pr.id}: ${out2.response.result_digest} != ${pr.expected_digest}`);
      }
    }
    count++;
  }
}
// Single-doc canon check (Document B omitted).
const single = runKernelWasm(wasmMod, { op: "canon.digest", params: { json_a: '{"b":1,"a":2}' } });
if (single.exitCode !== 0 || !single.response.ok || single.response.result.canonical_a !== '{"a":2,"b":1}') {
  throw new Error("Single-doc canon.digest failed: " + single.raw);
}
console.log(`WASM bit-exact verified ${count} presets + UI round-trip + single-doc canon`);
"""
        nproc = subprocess.run(["node", "-e", js_check], cwd=ROOT, capture_output=True, text=True, check=False)
        self.assertEqual(nproc.returncode, 0, nproc.stdout + nproc.stderr)
        self.assertIn("WASM bit-exact verified", nproc.stdout)

    def test_ac5_staged_wasm_has_no_user_home_path(self) -> None:
        self._require_wasm_or_skip("AC-5 staged WASM home-path check")
        if not STAGED_WASM.exists():
            proc = subprocess.run([sys.executable, str(ROOT / "site" / "build.py")], cwd=ROOT,
                                  capture_output=True, text=True, check=False)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        raw = STAGED_WASM.read_bytes()
        leaks = sorted({m.group(0).decode("latin-1") for rx in HOME_PATH_RES for m in rx.finditer(raw)})
        self.assertEqual(leaks, [], f"staged {STAGED_WASM.relative_to(ROOT)} embeds user-home paths: {leaks}")


if __name__ == "__main__":
    unittest.main()
