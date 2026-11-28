#!/usr/bin/env python3
"""Build the Metrology Maven website (OUTR.SITE) into site/dist/.

Single command, no dependencies beyond Python 3.10+:   python site/build.py

Every number, status and roadmap entry on the site is generated from the spec
tree at the current revision (REQ-OUTR.SITE-001/002/004), never typed by hand.
The output is plain static files with no third-party requests (REQ-OUTR.SITE-007).
"""

from __future__ import annotations

import datetime as dt
import hashlib
import html
import json
import math
import shutil
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

SITE = Path(__file__).resolve().parent
ROOT = SITE.parent
GUIDE_DIR = ROOT / "guide"
WASM_SRC = ROOT / "target" / "wasm32-wasip1" / "release" / "fid-guide-wasm.wasm"
sys.path.insert(0, str(ROOT / "tools"))
import scope  # noqa: E402

DIST = SITE / "dist"
ORIGIN = "https://metrologymaven.io"
REPO = "https://github.com/metrologymaven/metrologymaven"
CONTACT = "hello@metrologymaven.io"
e = html.escape

PHASES = [
    ("P0", "Foundations", "Numerics, least-squares and minimum-zone fits, the command registry, CLI and MCP, the journal, a QIF subset, the free verifier and the Executable Field Guide."),
    ("P1a", "First usable product", "Transcription reports, a prismatic GD&T subset (ASME), a minimal GUI and 3D viewer."),
    ("P1b", "Scan-to-CAD inspection", "Mesh and point-cloud inspection, ISO GPS dialect, CAD + PMI, SPC, Python scripting, report designer."),
    ("P2", "CMMs and live devices", "CMM programming and execution: I++ DME client, simulator, approval station, DMIS import."),
    ("P3", "Large volume, form & MSA", "Laser-tracker networks, live portable arms, surface texture and precision form (SURF), MSA and non-normal capability (RPT.STATS.MSA, D-054), automatic path planning."),
    ("P4", "Calibration labs & ecosystem", "Calibration-lab verification (CAL: ISO 10360, ISO 230, ISO/IEC 17025, PTB DCC, D-054), OPC UA, MTConnect, plugin marketplace, open-format migration."),
]

APPS = [
    {"slug": "report", "name": "Report", "phase": "P1a", "order": 1,
     "workflow": "Signed FAI, PPAP and SPC reporting from any instrument",
     "pitch": "Turn measurement results from any CMM, scanner or hand gauge into signed, verifiable first-article, PPAP and SPC reports.",
     "domains": ["RPT", "TRUST", "QUAL", "MIG"], "seeds": []},
    {"slug": "inspect", "name": "Inspect", "phase": "P1b", "order": 2,
     "workflow": "Scan-to-CAD comparison, GD&T evaluation and replayable colour maps",
     "pitch": "Compare scans and point clouds to CAD, evaluate GD&T and produce colour maps and reports, with every step recorded and replayable.",
     "domains": ["SCAN", "ALN", "GDT", "IO", "VIZ", "UNC"], "seeds": ["gom-inspect.md", "polyworks.md"]},
    {"slug": "probe", "name": "Probe", "phase": "P2", "order": 3,
     "workflow": "Open-standard CMM programming, simulation and execution (I++ DME & DMIS)",
     "pitch": "Program, simulate and run coordinate measuring machines through open protocols, with deterministic collision checks and a human approval gate before any motion.",
     "domains": ["CMM", "HAL"], "seeds": ["pc-dmis.md"]},
    {"slug": "network", "name": "Network", "phase": "P3", "order": 4,
     "workflow": "Multi-instrument laser-tracker and portable metrology networks with uncertainty",
     "pitch": "Bundle laser trackers and other large-volume instruments into one measurement network with uncertainty you can defend.",
     "domains": ["LVM"], "seeds": ["spatialanalyzer.md"]},
]

SHARED_CORE_DOMAINS = ["CORE", "KRN", "GEO", "AUTO", "FLOW", "AGENT", "PLAT", "ECO", "UX", "OUTR"]

NAV = [("/", "Home"), ("/apps/", "Apps"), ("/guide/", "Field Guide"), ("/how-it-works/", "How it works"),
       ("/status/", "Status"), ("/roadmap/", "Roadmap"), ("/contribute/", "Contribute"), ("/company/", "Company")]

LOGO = ('<svg viewBox="0 0 32 32" aria-hidden="true"><rect width="32" height="32" rx="4" fill="currentColor"/>'
        '<circle cx="16" cy="16" r="9" fill="none" stroke="var(--sheet)" stroke-width="2"/>'
        '<path d="M16 3v7M16 22v7M3 16h7M22 16h7" stroke="var(--sheet)" stroke-width="2"/>'
        '<circle cx="16" cy="16" r="3.2" fill="var(--ruby)"/></svg>')


# ---------------------------------------------------------------- data

def git_rev() -> str:
    try:
        return subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, capture_output=True,
                              text=True, check=True).stdout.strip()
    except Exception:  # noqa: BLE001 - building outside git is fine
        return "local"


def load():
    rep = scope.Report()
    nodes = scope.load_nodes(rep)
    seeds, oos = scope.load_seeds(rep)
    by_id = {n.id: n for n in nodes}
    domains = sorted((n for n in nodes if n.level == 1), key=lambda n: n.id)

    def dom(n):
        return n.id.split(".")[0]

    per = defaultdict(Counter)
    for n in nodes:
        if n.level >= 1:
            per[dom(n)]["nodes"] += 1
            per[dom(n)][n.status] += 1
            if n.lst("built_in"):
                per[dom(n)]["built"] += 1
    phase_leaves = Counter(n.meta.get("phase") for n in nodes if n.status == "leaf")
    phase_built = Counter(n.meta.get("phase") for n in nodes if n.lst("built_in"))
    oos_patterns = [p for p, _ in oos]
    seed_rows = defaultdict(lambda: Counter())
    for sid, (_feat, fname) in seeds.items():
        if scope.matches(oos_patterns, sid):
            seed_rows[fname]["out of scope"] += 1
        elif scope.deepest_cover(nodes, sid):
            seed_rows[fname]["covered"] += 1
        else:
            seed_rows[fname]["uncovered"] += 1
    decisions = []
    for f in sorted(scope.DECISIONS.glob("D-*.md")):
        meta, _ = scope.parse_front(f.read_text(encoding="utf-8"))
        if meta:
            decisions.append(meta)
    field_map = None
    fm_path = GUIDE_DIR / "map" / "field-map.json"
    if fm_path.exists():
        field_map = json.loads(fm_path.read_text(encoding="utf-8"))
    topics = []
    topics_dir = GUIDE_DIR / "topics"
    if topics_dir.exists():
        for tf in sorted(topics_dir.glob("*.json")):
            topics.append(json.loads(tf.read_text(encoding="utf-8")))
        topics.sort(key=lambda t: (t.get("order", 999), t.get("slug", "")))
    wasm_sha256 = None
    wasm_size = 0
    if WASM_SRC.exists():
        raw = WASM_SRC.read_bytes()
        wasm_sha256 = "sha256:" + hashlib.sha256(raw).hexdigest()
        wasm_size = len(raw)
    return {"nodes": nodes, "by_id": by_id, "domains": domains, "per": per,
            "phase_leaves": phase_leaves, "phase_built": phase_built,
            "seeds": seeds, "seed_rows": seed_rows, "decisions": decisions,
            "field_map": field_map, "topics": topics,
            "wasm_sha256": wasm_sha256, "wasm_size": wasm_size,
            "leaves": sum(1 for n in nodes if n.status == "leaf"),
            "built": sum(1 for n in nodes if n.lst("built_in")),
            "open": sum(1 for n in nodes if n.status == "open"),
            "rev": git_rev(), "date": dt.date.today().isoformat()}


# ---------------------------------------------------------------- layout

def page(path: str, title: str, desc: str, body: str, d, extra_head: str = "") -> tuple[str, str]:
    nav = "".join(
        f'<a href="{h}"{" aria-current=page" if (h == path or (h != "/" and path.startswith(h))) else ""}>{t}</a>'
        for h, t in NAV
    )
    full = f"{title} · Metrology Maven" if path != "/" else "Metrology Maven: open-source dimensional metrology"
    out = f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{e(full)}</title>
<meta name="description" content="{e(desc)}">
<link rel="canonical" href="{ORIGIN}{path}">
<meta property="og:title" content="{e(full)}">
<meta property="og:description" content="{e(desc)}">
<meta property="og:url" content="{ORIGIN}{path}">
<meta property="og:type" content="website">
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="/style.css">
{extra_head}</head>
<body>
<a class="skip" href="#main">Skip to content</a>
<header class="top"><div class="wrap">
<a class="brand" href="/">{LOGO}<span>Metrology Maven</span></a>
<nav class="main" aria-label="Main">{nav}</nav>
</div></header>
<main id="main" class="wrap">
{body}
</main>
<footer class="foot"><div class="wrap">
<span>Metrology Maven · open-source dimensional metrology · code MIT OR Apache-2.0, spec CC-BY-4.0</span>
<span><a href="{REPO}">GitHub</a> · <a href="mailto:{CONTACT}">{CONTACT}</a> · built from <a href="{REPO}/commit/{d['rev']}"><span class="mono">{d['rev']}</span></a> on {d['date']}</span>
</div></footer>
</body>
</html>
"""
    return path, out


def fcf(*parts: str) -> str:
    sym, *rest = parts
    return '<div class="fcf"><span class="sym">' + e(sym) + "</span>" + "".join(f"<span>{e(p)}</span>" for p in rest) + "</div>"


def notice(d) -> str:
    return (f'<p class="notice"><b>Honest status (pre-alpha):</b> nothing here is usable for measurement yet. '
            f'The product is specified as testable work orders, and <b>{d["built"]:,} of {d["leaves"]:,}</b> '
            f'leaves are built in code (<code>crates/</code>). Every number on this site is generated from the repository.</p>')


def app_counts(a, d) -> tuple[int, int]:
    built = sum(d["per"][dom]["built"] for dom in a["domains"])
    leaves = sum(d["per"][dom]["leaf"] for dom in a["domains"])
    return built, leaves


def app_card(a, d) -> str:
    built, leaves = app_counts(a, d)
    if built > 0:
        pill = f'<span class="pill p">in build · {built}/{leaves} leaves</span>'
    else:
        pill = '<span class="pill spec">specification</span>'
    return (f'<a class="card" href="/apps/{a["slug"]}/"><span class="tag">App {a["order"]} · starts {a["phase"]}</span>'
            f'<h3>Metrology Maven {e(a["name"])}</h3><p>{e(a["workflow"])}</p>'
            f'{pill}</a>')


# ---------------------------------------------------------------- pages

def home(d):
    s = d
    acc = sum(1 for x in s["decisions"] if x.get("status") == "accepted")
    body = f"""
<section class="hero">
{fcf("⌖", "OPEN SOURCE", "DIMENSIONAL METROLOGY")}
<h1>Open-source inspection software for scanners, CMMs and laser trackers</h1>
<p class="lead">Metrology Maven is an open-source, QIF-native dimensional metrology platform for independent machine shops, quality labs and engineers. It is free to use, local-first, and every result can be re-checked by anyone. People and AI agents drive it through the same typed commands.</p>
<div class="btns"><a class="btn primary" href="/guide/">Interactive Field Guide</a><a class="btn" href="{REPO}">See it on GitHub</a><a class="btn" href="/status/">Live status</a><a class="btn" href="/how-it-works/">How it works</a></div>
{notice(d)}
</section>
<section class="sheet" aria-labelledby="apps-h">
{fcf("1", "FOUR APPS", "ONE CORE")}
<h2 id="apps-h">Four focused apps on one shared core</h2>
<p class="muted">Each app does one job people already understand. Under the hood they share one math kernel, one file format (QIF-native), one command list and one 3D viewer, so results move between them without conversion.</p>
<div class="grid">{"".join(app_card(a, d) for a in APPS)}</div>
</section>
<section class="sheet" aria-labelledby="why-h">
{fcf("2", "WHY", "DIFFERENT")}
<h2 id="why-h">Designed around trust, not licences</h2>
<div class="grid">
<div class="card"><h3>Free and open</h3><p>Code under MIT OR Apache-2.0. No locked features, no dongles, no per-seat fees.</p></div>
<div class="card"><h3>Every number can be checked</h3><p>A free, independent verifier re-checks results from the recorded journal, bit for bit.</p></div>
<div class="card"><h3>Agents first</h3><p>Every action is a typed command available from the CLI and MCP, designed so automation handles routine steps while shop-floor safety interlocks stay enforced.</p></div>
<div class="card"><h3>Records everything</h3><p>Every step is journaled, screenshotted and replayable, which makes audits a matter of reading the record.</p></div>
</div>
</section>
<section class="sheet" aria-labelledby="num-h">
{fcf("3", "THE PLAN", "IN NUMBERS")}
<h2 id="num-h">The whole product, written down and built in the open</h2>
<p class="muted">The plan is a public tree of small specification files. Each piece is split until it is one pull request of work, with its requirements, tests and a self-contained build prompt verified by CI.</p>
<div class="stats">
<div class="stat"><b>{len(s['domains'])}</b><small>capability domains</small></div>
<div class="stat"><b>{len(s['nodes']):,}</b><small>spec nodes</small></div>
<div class="stat"><b>{s['leaves']:,}</b><small>ready-to-build work orders</small></div>
<div class="stat"><b>{s['built']:,}</b><small>work orders built in code</small></div>
<div class="stat"><b>{len(s['seeds'])}</b><small>industry workflow features tracked</small></div>
<div class="stat"><b>{acc}/{len(s['decisions'])}</b><small>decisions accepted</small></div>
</div>
<p><a href="/status/">See the full status, generated from the repository →</a></p>
</section>
"""
    return page("/", "Home", "Open-source dimensional metrology software: inspection, CMM programming, laser-tracker networks and reporting, built in the open.", body, d)


def apps_index(d):
    core_rows = []
    for dom in SHARED_CORE_DOMAINS:
        n = d["by_id"].get(dom)
        c = d["per"][dom]
        core_rows.append(
            f'<tr><td><span class="mono">{dom}</span> {e(n.meta.get("title", "") if n else "")}</td>'
            f'<td class="n">{c["leaf"]}</td><td class="n">{c["built"]}</td><td class="n">{c["open"]}</td></tr>'
        )
    body = f"""
<section class="hero">{fcf("A", "APPS")}
<h1>The app family</h1>
<p class="lead">Report and Inspect come first, because they help shops that run several brands of equipment straight away. Probe and Network follow.</p>{notice(d)}</section>
<section class="sheet"><div class="grid">{"".join(app_card(a, d) for a in APPS)}</div></section>
<section class="sheet">{fcf("C", "SHARED CORE", "FOUNDATION")}
<h2>Shared core &amp; platform domains</h2>
<p class="muted">All four apps sit on the same deterministic kernel, geometry engine, command registry, journal, CLI and MCP surfaces.</p>
<div class="tbl"><table><thead><tr><th>Domain</th><th>Ready to build</th><th>Built</th><th>Still open</th></tr></thead><tbody>{"".join(core_rows)}</tbody></table></div>
<p class="muted">See all {len(d['domains'])} domains on the <a href="/status/">status page</a>.</p>
</section>"""
    return page("/apps/", "Apps", "The Metrology Maven app family: Report, Inspect, Probe and Network.", body, d)


def app_page(a, d):
    per = d["per"]
    built, leaves = app_counts(a, d)
    rows = []
    gaps = []
    for dom in a["domains"]:
        n = d["by_id"].get(dom)
        c = per[dom]
        rows.append(f'<tr><td><span class="mono">{dom}</span> {e(n.meta.get("title", "") if n else "")}</td>'
                    f'<td class="n">{c["leaf"]}</td><td class="n">{c["built"]}</td><td class="n">{c["open"]}</td></tr>')
        gaps += [x for x in d["nodes"] if x.id.split(".")[0] == dom and x.status == "open"]
    seeds = ""
    if a["seeds"]:
        srows = []
        for f in a["seeds"]:
            c = d["seed_rows"][f]
            tot = sum(c.values()) or 1
            srows.append(f'<tr><td>{e(f.removesuffix(".md"))}</td><td class="n">{c["covered"]}</td><td class="n">{c["out of scope"]}</td>'
                         f'<td class="n">{c["uncovered"]}</td><td><div class="bar"><i style="width:{100 * c["covered"] / tot:.0f}%"></i></div></td></tr>')
        seeds = f"""<h3>Industry workflow coverage mapped into the spec</h3>
<p class="muted">Reference workflow inventories in <code>seeds/</code> are traced to the spec nodes that will cover them. "Specified" means written down as a work order, not built.</p>
<div class="tbl"><table><thead><tr><th>Workflow inventory</th><th>Specified</th><th>Out of scope</th><th>Not yet</th><th>Share specified</th></tr></thead><tbody>{"".join(srows)}</tbody></table></div>"""
    gap_list = "".join(f'<li><span class="mono">{e(x.id)}</span> {e(x.meta.get("title", ""))}</li>'
                       for x in sorted(gaps, key=lambda x: x.id)[:60])
    more = f"<li>…and {len(gaps) - 60} more in the repository</li>" if len(gaps) > 60 else ""
    if built > 0:
        status_pill = f'<span class="pill p">Status: in build (pre-alpha · {built}/{leaves} app-domain leaves)</span>'
    else:
        status_pill = '<span class="pill spec">Status: specification (pre-alpha)</span>'
    body = f"""
<section class="hero">{fcf(str(a["order"]), "APP", a["name"].upper(), "STARTS " + a["phase"])}
<h1>Metrology Maven {e(a["name"])}</h1>
<p class="lead">{e(a["pitch"])}</p>
<p>Workflow focus: <b>{e(a["workflow"])}</b></p>
<p>{status_pill} <span class="pill p">Work starts in {a["phase"]}</span></p>
</section>
<section class="sheet">{fcf("S", "STATUS", "GENERATED")}
<h2>Where it stands</h2>
<p class="muted">Generated from the spec tree at <span class="mono">{d['rev']}</span>. "Ready" means a work order is written and can be built; it does not mean the feature exists yet. This app also builds on the <a href="/apps/">shared core domains</a>.</p>
<div class="tbl"><table><thead><tr><th>Domain</th><th>Ready to build</th><th>Built</th><th>Still open</th></tr></thead><tbody>{"".join(rows)}</tbody></table></div>
{seeds}
<h3>Known gaps</h3>
<details><summary>{len(gaps)} open spec nodes in this app's domains</summary><ul class="clean">{gap_list}{more}</ul></details>
</section>"""
    return page(f"/apps/{a['slug']}/", f"{a['name']}", f"Metrology Maven {a['name']}: {a['pitch']}", body, d)


def how(d):
    body = f"""
<section class="hero">{fcf("H", "HOW IT WORKS")}
<h1>How the project is built</h1>
<p class="lead">Metrology Maven is specified, reviewed, and built by AI coding agents following a public, strictly checked specification tree and mathematical oracles, with zero human bottlenecks in the build loop.</p></section>
<section class="sheet">{fcf("1", "THE SPEC TREE")}
<h2>A plan you can run</h2>
<p>The product is described as a tree: 1 product, {len(d['domains'])} domains, then subsystems, modules and features, down to <b>{d['leaves']:,} leaves</b>. A leaf is one pull request of work. It contains testable requirements, acceptance tests with named data, a design, and a build prompt that a coding agent can follow on its own.</p>
<ol class="steps"><li><b>Pick</b>an open node</li><li><b>Expand</b>split it, or finish it as a leaf</li><li><b>Check</b><code>tools/scope.py check</code> enforces every rule</li><li><b>Review</b>independent agent review plus required CI gates</li><li><b>Build</b>leaves are built in dependency order, in parallel waves</li></ol>
<p class="muted">The checker fails a change that drops a requirement or a tracked workflow feature, creates a dependency loop, or adds a feature without its CLI command, MCP tool and journal entry.</p>
</section>
<section class="sheet">{fcf("2", "ARCHITECTURE")}
<h2>Five layers, each using only the ones below</h2>
<div class="tbl"><table><thead><tr><th>Layer</th><th>What lives there</th></tr></thead><tbody>
<tr><td><b>L3 Surfaces</b></td><td>CLI, MCP servers, Python, the GPU viewer, the four apps, agent skills</td></tr>
<tr><td><b>L2 Services</b></td><td>File formats, alignment, GD&T, uncertainty, scanning, reports, signing, instruments, CMM, trackers</td></tr>
<tr><td><b>L1 Core</b></td><td>Command registry, command bus, tamper-evident journal, replay, project storage</td></tr>
<tr><td><b>L0 Foundation</b></td><td>Deterministic math kernel: fits, geometry, GD&T math, units. Never touched by the GPU.</td></tr>
<tr><td><b>Tooling</b></td><td>Spec checker, CI and licence gates, verification suites, outreach and evidence checks (reads artifacts and pure L0 libraries only)</td></tr>
</tbody></table></div>
<p class="muted">Rust throughout (including the web build via WebAssembly), wgpu + Vello/Parley for graphics and 2D surfaces, Python for user scripting.</p>
</section>
<section class="sheet">{fcf("3", "RUNTIME SAFETY &amp; PROVENANCE")}
<h2>Zero-bottleneck build, honest runtime controls</h2>
<div class="grid">
<div class="card"><span class="tag">H1</span><h3>Machine motion (runtime)</h3><p>On the shop floor, each CMM program and envelope is approved after simulation and collision checks, with physical cycle-start interlocks.</p></div>
<div class="card"><span class="tag">H2</span><h3>Record release (runtime)</h3><p>Controlled records such as first-article reports are released in batches with full provenance of every step.</p></div>
<div class="card"><span class="tag">H3</span><h3>Conformance (runtime)</h3><p>Typed decision rules run everywhere; customer quality systems can optionally require named-person accept/reject.</p></div>
<div class="card"><span class="tag">D-045 / D-052</span><h3>Provisional interpretation</h3><p>GD&T rules are built and tested in CI against clean-room math and oracles; any unreviewed rule stays visibly labelled <code>interpretation: provisional</code> everywhere it goes.</p></div>
</div>
<p class="muted">An agent never acts as a person: every action is recorded under its own principal identity, and every verification suite runs against open mathematical generators and arbitrary-precision oracles (<a href="{REPO}/blob/main/decisions/D-052-autonomous-zero-human-bottlenecks.md">D-052</a>).</p>
</section>"""
    return page("/how-it-works/", "How it works", "How Metrology Maven is specified, built by coding agents in automated waves, and checked against deterministic oracles.", body, d)


def status(d):
    rows = []
    mx = max(d["per"][x.id]["nodes"] for x in d["domains"]) or 1
    for x in d["domains"]:
        c = d["per"][x.id]
        rows.append(f'<tr><td><span class="mono">{x.id}</span></td><td>{e(x.meta.get("title", ""))}</td><td class="n">{x.meta.get("phase", "")}</td>'
                    f'<td class="n">{c["nodes"]}</td><td class="n">{c["leaf"]}</td><td class="n">{c["built"]}</td><td class="n">{c["open"]}</td>'
                    f'<td><div class="bar"><i style="width:{100 * c["leaf"] / mx:.0f}%"></i></div></td></tr>')
    srows = "".join(f'<tr><td>{e(f.removesuffix(".md"))}</td><td class="n">{c["covered"]}</td><td class="n">{c["out of scope"]}</td><td class="n">{c["uncovered"]}</td></tr>'
                    for f, c in sorted(d["seed_rows"].items()))
    acc = sum(1 for x in d["decisions"] if x.get("status") == "accepted")
    open_dec = [x for x in d["decisions"] if x.get("status") not in ("accepted", "superseded")]
    dec = "".join(f'<li><span class="mono">{e(x.get("id", ""))}</span> {e(x.get("title", ""))}</li>' for x in open_dec) or "<li>None</li>"
    body = f"""
<section class="hero">{fcf("S", "STATUS", d['rev'], d['date'])}
<h1>Project status</h1>
<p class="lead">Generated from the repository at commit <a class="mono" href="{REPO}/commit/{d['rev']}">{d['rev']}</a> on {d['date']}. Nothing on this page is written by hand.</p>{notice(d)}</section>
<section class="sheet">{fcf("1", "SPEC TREE BY DOMAIN")}
<div class="stats"><div class="stat"><b>{len(d['nodes']):,}</b><small>nodes</small></div><div class="stat"><b>{d['leaves']:,}</b><small>leaves (ready to build)</small></div><div class="stat"><b>{d['built']:,}</b><small>leaves built in code</small></div><div class="stat"><b>{d['open']}</b><small>open nodes</small></div><div class="stat"><b>{acc}/{len(d['decisions'])}</b><small>decisions accepted</small></div></div>
<div class="tbl"><table><thead><tr><th>ID</th><th>Domain</th><th>Starts</th><th>Nodes</th><th>Leaves</th><th>Built</th><th>Open</th><th>Leaves</th></tr></thead><tbody>{"".join(rows)}</tbody></table></div>
<p class="muted">Full tree: <a href="{REPO}/blob/main/generated/INDEX.md">generated/INDEX.md</a> · Built leaves: <a href="{REPO}/blob/main/generated/BUILT.md">generated/BUILT.md</a>.</p>
</section>
<section class="sheet">{fcf("2", "WORKFLOW COVERAGE")}
<p class="muted">Reference feature and device inventories tracked in <code>seeds/</code>, and how many rows the spec covers. Specified is not built. Full mapping: <a href="{REPO}/blob/main/generated/COVERAGE.md">generated/COVERAGE.md</a>.</p>
<div class="tbl"><table><thead><tr><th>Inventory</th><th>Specified</th><th>Out of scope</th><th>Not yet</th></tr></thead><tbody>{srows}</tbody></table></div>
</section>
<section class="sheet">{fcf("3", "OPEN DECISIONS")}<ul class="clean">{dec}</ul>
<p class="muted">All decisions: <a href="{REPO}/blob/main/generated/DECISIONS.md">generated/DECISIONS.md</a>.</p></section>"""
    return page("/status/", "Status", "Live project status generated from the Metrology Maven spec tree.", body, d)


def roadmap(d):
    items = []
    for p, name, text in PHASES:
        apps = ", ".join(a["name"] for a in APPS if a["phase"] == p)
        items.append(f'<tr><td class="n"><b>{p}</b></td><td><b>{name}</b><br><span class="muted">{e(text)}</span></td>'
                     f'<td class="n">{d["phase_leaves"].get(p, 0)}</td><td class="n">{d["phase_built"].get(p, 0)}</td><td>{apps or "–"}</td></tr>')
    body = f"""
<section class="hero">{fcf("R", "ROADMAP", "BY PHASE")}
<h1>Roadmap</h1>
<p class="lead">Phases describe the order in which work starts. There are no dates here: dates are published only when a launch milestone is committed.</p></section>
<section class="sheet"><div class="tbl"><table><thead><tr><th>Phase</th><th>Theme</th><th>Leaves</th><th>Built</th><th>App work starts</th></tr></thead><tbody>{"".join(items)}</tbody></table></div>
<p class="muted">An app reaches 1.0 only when every phase holding its core workflow is finished: Report and Inspect after P1b, Probe after P2, Network after P3. The first milestone, <b>launch-1</b>, targets Inspect and Report: <a href="{REPO}/blob/main/milestones/launch-1.md">milestones/launch-1.md</a>.</p></section>"""
    return page("/roadmap/", "Roadmap", "The Metrology Maven roadmap by phase, generated from the spec tree.", body, d)


def contribute(d):
    body = f"""
<section class="hero">{fcf("C", "CONTRIBUTE")}
<h1>Build from the open spec tree</h1>
<p class="lead">Every piece of work is a self-contained node in the repository. No waiting, no outreach, and no gatekeepers: pick an open node or buildable leaf and open a pull request.</p>
<div class="btns"><a class="btn primary" href="{REPO}">Open repository</a><a class="btn" href="{REPO}/blob/main/generated/milestones/launch-1.md">Launch-1 build waves</a><a class="btn" href="{REPO}/blob/main/CONTRIBUTING.md">Contributing guide</a></div></section>
<section class="sheet">{fcf("1", "SELF-SERVICE WORKFLOWS")}
<div class="grid">
<div class="card"><h3>Build Rust leaves</h3><p>Pick an unbuilt leaf from the earliest waves in <code>generated/milestones/launch-1.md</code>. Each leaf's build prompt is self-contained and checked by CI.</p></div>
<div class="card"><h3>Expand open spec nodes</h3><p>Run <code>python tools/scope.py open</code> and follow <code>prompts/expand-node.md</code> with a coding agent or by hand. Name your agent tool in the PR.</p></div>
<div class="card"><h3>Improve fixtures &amp; workflow rows</h3><p>Add clean-room mathematical fixtures under <code>fixtures/</code> or refine workflow coverage rows in <code>seeds/</code>.</p></div>
<div class="card"><h3>Optional GD&amp;T rule sign-off</h3><p>Unreviewed GD&amp;T rules carry <code>interpretation: provisional</code> (<a href="{REPO}/blob/main/decisions/D-045-provisional-gdt-interpretation.md">D-045</a>). Reviewers may optionally sign off rules via PR, without ever blocking development.</p></div>
</div>
<p class="muted">All commits are DCO signed-off (<code>git commit -s</code>). Never paste text from ISO, ASME, or VDA standards: cite the designator and clause in your own words.</p>
</section>"""
    return page("/contribute/", "Contribute", "Self-service contribution guide for Metrology Maven: build Rust leaves, expand spec nodes, and add mathematical test fixtures.", body, d)


def company(d):
    body = f"""
<section class="hero">{fcf("M", "COMPANY")}
<h1>Metrology Maven, the company</h1>
<p class="lead">Metrology Maven stewards the open-source core platform and funds its infrastructure. The four base apps, deterministic kernel and verifier are free and open source. The company builds commercial enterprise, industry-pack, AI-fleet and regulated-assurance products on top.</p></section>
<section class="sheet">{fcf("1", "OPEN-CORE BOUNDARY")}<div class="two">
<div class="col free"><h3>Open core (MIT OR Apache-2.0)</h3><ul class="clean"><li>All four base apps (Report, Inspect, Probe, Network), the deterministic math kernel and QIF/STEP/DMIS formats</li><li>The CLI, MCP servers, Python scripting and vendor-neutral HAL</li><li>The independent verifier (<code>fid-verify</code>)</li><li>Open validation templates (<code>QUAL.VAP</code>), IQ/OQ runner and the <code>IND.CONTRACT</code> plugin SDK</li></ul></div>
<div class="col"><h3>Commercial products &amp; services (D-053)</h3><ul class="clean"><li>Multi-site Enterprise server: SSO/SAML, RBAC, sync, cross-site dashboards and PLM/MES/ERP connectors (<code>ENT</code>)</li><li>Commercial industry packs built on <code>IND.CONTRACT</code> (airfoil, gear, sheet metal, gap &amp; flush, tube, pattern, portable)</li><li>Hosted verification, evidence archive and OEM supplier quality verification portal</li><li>On-premises private machine-adapter LoRA fine-tuning, fleet drift monitoring and autonomous AI auto-programming (D-049)</li><li>Pre-built certified regulated LTS builds, release dossiers, support SLAs and OEM hardware integration packs</li></ul></div>
</div>
<p>No feature of the four open base apps will ever be moved behind a licence check (<a href="{REPO}/blob/main/decisions/D-036-funding-alignment.md">D-036</a>, <a href="{REPO}/blob/main/decisions/D-053-open-core-commercial-boundary.md">D-053</a>).</p></section>"""
    return page("/company/", "Company", "Metrology Maven stewards the open-source metrology platform and defines a clear open-core and commercial boundary (D-053).", body, d)


def _subtree_counts(node_id: str, d) -> tuple[int, int, int]:
    """Return (leaves, built, open) under a domain or subtree node ID."""
    if "." not in node_id and node_id in d["per"]:
        c = d["per"][node_id]
        return c["leaf"], c["built"], c["open"]
    prefix = node_id + "."
    leaves = built = open_n = 0
    for n in d["nodes"]:
        if n.id == node_id or n.id.startswith(prefix):
            if n.status == "leaf":
                leaves += 1
            elif n.status == "open":
                open_n += 1
            if n.lst("built_in"):
                built += 1
    return leaves, built, open_n


def _scale_bar(lo_m: float, hi_m: float, tier: str) -> str:
    """Logarithmic bar from 1e-10 m (0.1 nm) to 1e2 m (100 m) using CSP-safe SVG attributes."""
    log_min, log_max = -10.0, 2.0
    a = max(0.0, min(100.0, (math.log10(max(lo_m, 1e-10)) - log_min) / (log_max - log_min) * 100.0))
    b = max(a + 3.0, min(100.0, (math.log10(min(hi_m, 1e2)) - log_min) / (log_max - log_min) * 100.0))
    cls = "scale-span" + (" post" if tier == "core-post-1.0" else " plug" if tier == "plugin-companion" else "")
    return (
        f'<svg class="scale-svg" viewBox="0 0 100 10" preserveAspectRatio="none" aria-hidden="true">'
        f'<rect class="scale-bg" x="0" y="0" width="100" height="10"/>'
        f'<rect class="{cls}" x="{a:.1f}" y="0" width="{b - a:.1f}" height="10"/>'
        f'</svg>'
    )


def render_field_map_section(fmap: dict, d) -> str:
    tier_meta = {t["id"]: t for t in fmap["tiers"]}
    topic_titles = {t["slug"]: t["title"] for t in d["topics"]}
    tier_pills = {"core-1.0": ("ok", "Core 1.0"), "core-post-1.0": ("spec", "Post-1.0 Core (D-054)"),
                  "plugin-companion": ("p", "ECO.SDK Plugin / Companion")}
    buttons = [f'<button type="button" class="lab-btn" data-tier="all" aria-pressed="true">All {len(fmap["pillars"])} branches</button>']
    for t in fmap["tiers"]:
        buttons.append(f'<button type="button" class="lab-btn" data-tier="{e(t["id"])}" aria-pressed="false">{e(t["label"])}</button>')

    cards = []
    for p in fmap["pillars"]:
        leaves = built = open_n = 0
        for dom in p["spec_domains"]:
            dl, db, do = _subtree_counts(dom, d)
            leaves += dl
            built += db
            open_n += do
        pcls, plabel = tier_pills.get(p["tier"], ("p", p["tier"]))
        stds = ", ".join(p["standards"])
        doms = ", ".join(p["spec_domains"])
        # Only a core-1.0 branch has its own lab; a post-1.0 or plugin branch may point at a
        # related chapter, but must not read as having a lab of its own.
        link = ""
        if p.get("guide_slug"):
            slug = p["guide_slug"]
            label = ("Open interactive lab" if p["tier"] == "core-1.0"
                     else f"Related chapter: {topic_titles.get(slug, slug)} (no lab for this branch yet)")
            link = f' · <a href="/guide/{e(slug)}/">{e(label)} →</a>'
        cards.append(
            f'<article class="card" data-pillar-tier="{e(p["tier"])}">'
            f'<span class="tag">{p["order"]:02d} · {e(p["scale_label"])} · {e(p["phase"])}</span>'
            f'<h3>{e(p["title"])}</h3>'
            f'<p>{e(p["why"])}</p>'
            f'<p><small><b>Instruments:</b> {e(", ".join(p["instruments"]))}</small></p>'
            f'<p><small><b>Standards (designators only):</b> {e(stds)}</small></p>'
            f'{_scale_bar(p["scale_min_m"], p["scale_max_m"], p["tier"])}'
            f'<p><span class="pill {pcls}">{e(plabel)}</span> '
            f'<span class="pill p">{built}/{leaves} leaves built ({open_n} open)</span></p>'
            f'<p><small class="muted">Spec nodes: <code>{e(doms)}</code>{link}</small></p>'
            f'</article>'
        )
    t_rows = "".join(f'<tr><td><b>{e(t["label"])}</b></td><td>{e(t["description"])}</td></tr>' for t in fmap["tiers"])
    return (
        f'<div class="tbl"><table><thead><tr><th>D-054 Scope Tier</th><th>Architectural boundary</th></tr></thead>'
        f'<tbody>{t_rows}</tbody></table></div>'
        f'<div class="tier-filter" data-field-map-filter role="group" aria-label="Filter metrology pillars by scope tier">{"".join(buttons)}</div>'
        f'<div class="grid">{"".join(cards)}</div>'
        f'<p class="muted"><small>Scale bar spans logarithmic length scale 0.1 nm (left) to 100 m (right). '
        f'Counts reflect live repository revision <span class="mono">{d["rev"]}</span> under '
        f'<a href="{REPO}/blob/main/decisions/D-054-dimensional-metrology-scope-and-field-guide.md">Decision D-054</a>.</small></p>'
    )


def _static_preset_rows(presets: list[dict]) -> str:
    rows = []
    for pr in presets:
        res = pr.get("expected_result", {})
        highlights = []
        for k in ("outcome", "diameter", "usage_size", "lpu_check_passed", "mc_validation", "replay_verdict", "tilt_mrad",
                  "diameter_envelope_excess"):
            if k in res:
                highlights.append(f"{k}={res[k]}")
        summary = ", ".join(highlights) if highlights else "verified"
        rows.append(
            f'<tr><td><b>{e(pr["label"])}</b><br><small class="muted">{e(pr["description"])}</small></td>'
            f'<td class="mono">{e(pr["op"])}</td>'
            f'<td class="mono">{e(summary)}</td>'
            f'<td class="mono"><small>{e(pr.get("expected_digest", "")[:24])}…</small></td></tr>'
        )
    return (
        '<div class="tbl"><table><thead><tr><th>Verified preset</th><th>Op</th><th>Kernel output</th><th>RFC 8785 digest</th></tr></thead>'
        f'<tbody>{"".join(rows)}</tbody></table></div>'
    )


def guide_index(d):
    topics = d["topics"]
    cards = []
    for t in topics:
        status_pill = ('<span class="pill ok">Live WASM kernel</span>' if t["status"] == "live-kernel"
                       else '<span class="pill p">Interactive map</span>')
        prov_pill = ' <span class="pill spec">D-045 provisional</span>' if t.get("provisional") else ""
        crates = ", ".join(t.get("crates", [])) or "spec tree"
        cards.append(
            f'<a class="card" href="/guide/{e(t["slug"])}/">'
            f'<span class="tag">Chapter {t["order"]:02d} · {e(t["phase"])}</span>'
            f'<h3>{e(t["title"])}</h3>'
            f'<p>{e(t["subtitle"])}</p>'
            f'<p>{status_pill}{prov_pill}</p>'
            f'<p><small class="muted">Backed by: <code>{e(crates)}</code></small></p></a>'
        )
    wasm_note = (f'<span class="pill ok">WASM kernel attached ({d["wasm_size"] // 1024} KB · {d["wasm_sha256"][:19]}…)</span>'
                 if d["wasm_sha256"] else '<span class="pill spec">Static verified presets (compile fid-guide-wasm for live WASM)</span>')
    fmap_html = render_field_map_section(d["field_map"], d) if d["field_map"] else ""
    body = f"""
<section class="hero">
{fcf("G", "FIELD GUIDE", "EXECUTABLE METROLOGY", "D-054")}
<h1>Executable Dimensional Metrology Field Guide</h1>
<p class="lead">Piece-by-piece dimensional metrology labs backed directly by Metrology Maven's implemented pure-Rust kernel crates compiled to WebAssembly—never a detached prose wiki, never hand-invented numbers.</p>
<p>{wasm_note} <span class="pill p">Zero third-party requests</span></p>
{notice(d)}
</section>
<section class="sheet" aria-labelledby="chapters-h">
{fcf("1", "LIVE KERNEL LABS", "P0–P1a")}
<h2 id="chapters-h">Interactive chapters locked to implemented crates</h2>
<p class="muted">Each chapter executes real functions from <code>fid-fit</code>, <code>fid-gps</code>, <code>fid-kunc</code>, <code>fid-kdec</code> and <code>fid-canon</code>. Every preset output and SHA-256 digest is checked against the Rust kernel in CI.</p>
<div class="grid">{"".join(cards)}</div>
</section>
<section class="sheet" aria-labelledby="map-h">
{fcf("2", "FIELD MAP", "0.1 NM TO 100 M")}
<h2 id="map-h">How all of dimensional metrology fits together (D-054)</h2>
<p class="muted">Rather than claiming every branch of metrology in 1.0, Metrology Maven publishes an honest three-tier map across twelve orders of magnitude, with live spec-tree counts generated at build time.</p>
{fmap_html}
</section>"""
    return page("/guide/", "Field Guide", "Executable, WASM-interactive Dimensional Metrology Field Guide and complete 0.1 nm to 100 m scope map.", body, d,
                extra_head='<script src="/guide-lab.js" defer></script>\n')


def guide_topic_page(topic: dict, d):
    slug = topic["slug"]
    prov_banner = ""
    if topic.get("provisional"):
        prov_banner = (
            '<p class="notice"><b>Provisional interpretation notice (D-045 / D-052):</b> '
            'GD&amp;T and decision-rule defaults in this chapter are clean-room mathematical implementations '
            'checked against independent oracles and stay visibly labelled <code>interpretation: provisional</code> '
            'until independent credentialed sign-off.</p>'
        )
    sections_html = []
    for sec in topic.get("sections", []):
        ps = "".join(f"<p>{e(p)}</p>" for p in sec.get("paragraphs", []))
        sections_html.append(f'<h3>{e(sec["heading"])}</h3>{ps}')

    std_rows = "".join(
        f'<tr><td class="mono">{e(s["designator"])}</td><td>{e(s["topic"])}</td></tr>'
        for s in topic.get("standards", [])
    )
    lab = topic.get("lab", {})
    presets = lab.get("presets", [])
    wasm_attrs = (f' data-wasm-url="/guide/fid-guide-wasm.wasm" data-wasm-sha256="{e(d["wasm_sha256"])}"'
                  if d["wasm_sha256"] else "")
    topic_json = json.dumps(topic, separators=(",", ":")).replace("</", "<\\/")

    if lab.get("widget_kind") == "field-map-explorer" and d["field_map"]:
        lab_block = render_field_map_section(d["field_map"], d)
    else:
        lab_block = (
            f'<p><span id="guide-wasm-status" class="pill p">Kernel-verified static presets ready</span></p>'
            f'<p class="muted">{e(lab.get("instructions", ""))}</p>'
            f'<div id="guide-lab-root" class="lab" data-widget-kind="{e(lab.get("widget_kind", ""))}"{wasm_attrs}>'
            f'{_static_preset_rows(presets)}</div>'
            f'<script type="application/json" id="guide-topic-data">{topic_json}</script>'
        )

    crates_str = ", ".join(topic.get("crates", [])) or "spec tree only"
    nodes_str = ", ".join(topic.get("spec_nodes", []))
    all_links = " · ".join(
        f'<a href="/guide/{e(t["slug"])}/"{" aria-current=page" if t["slug"] == slug else ""}>{t["order"]:02d}. {e(t["slug"])}</a>'
        for t in d["topics"]
    )
    body = f"""
<section class="hero">
{fcf(f"{topic['order']:02d}", "FIELD GUIDE", topic["phase"], "PROVISIONAL" if topic.get("provisional") else "KNP-1")}
<h1>{e(topic["title"])}</h1>
<p class="lead">{e(topic["subtitle"])}</p>
<p class="muted"><a href="/guide/">← All Field Guide chapters</a> · {all_links}</p>
{prov_banner}
</section>
<section class="sheet" aria-labelledby="lab-h">
{fcf("LAB", "EXECUTABLE", "WASM KERNEL")}
<h2 id="lab-h">Interactive kernel sandbox</h2>
{lab_block}
</section>
<section class="sheet" aria-labelledby="concept-h">
{fcf("NOTES", "CLEAN ROOM", "CONCEPTS")}
<h2 id="concept-h">Core principles</h2>
{"".join(sections_html)}
<h3>Standards cited (designators &amp; clauses only)</h3>
<p class="muted">Metrology Maven never embeds copyrighted standards text; every rule is stated as clean-room mathematics with designator citations.</p>
<div class="tbl"><table><thead><tr><th>Designator</th><th>Topic</th></tr></thead><tbody>{std_rows}</tbody></table></div>
</section>
<section class="sheet" aria-labelledby="cli-h">
{fcf("CLI", "REPRODUCE", "LOCAL")}
<h2 id="cli-h">Run the exact same computation locally</h2>
<pre><code>{e(topic.get("cli_example", ""))}</code></pre>
<p class="muted">Backed by crates: <code>{e(crates_str)}</code> · Spec nodes: <code>{e(nodes_str)}</code></p>
</section>"""
    return page(f"/guide/{slug}/", topic["title"], topic["subtitle"], body, d,
                extra_head='<script src="/guide-lab.js" defer></script>\n')


def not_found(d):
    body = f"""<section class="hero">{fcf("404", "NOT FOUND")}<h1>That page isn't here</h1>
<p class="lead">This site was rebuilt for the open-source project. Try the <a href="/">home page</a>, the <a href="/guide/">Field Guide</a> or the <a href="/status/">status page</a>.</p></section>"""
    return page("/404", "Not found", "Page not found.", body, d)


# ---------------------------------------------------------------- build

def main() -> int:
    d = load()
    pages = [home(d), apps_index(d), guide_index(d), how(d), status(d), roadmap(d), contribute(d), company(d), not_found(d)]
    pages += [app_page(a, d) for a in APPS]
    pages += [guide_topic_page(t, d) for t in d["topics"]]
    if DIST.exists():
        shutil.rmtree(DIST)
    DIST.mkdir(parents=True)
    for f in (SITE / "src").iterdir():
        shutil.copy2(f, DIST / f.name)
    if WASM_SRC.exists():
        wasm_out = DIST / "guide" / "fid-guide-wasm.wasm"
        wasm_out.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(WASM_SRC, wasm_out)
    urls = []
    for path, text in pages:
        if path == "/404":
            out = DIST / "404.html"
        else:
            out = DIST / path.strip("/") / "index.html" if path != "/" else DIST / "index.html"
            urls.append(path)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(text, encoding="utf-8", newline="\n")
    (DIST / "robots.txt").write_text(f"User-agent: *\nAllow: /\nSitemap: {ORIGIN}/sitemap.xml\n", encoding="utf-8", newline="\n")
    sm = "".join(f"<url><loc>{ORIGIN}{u}</loc><lastmod>{d['date']}</lastmod></url>" for u in urls)
    (DIST / "sitemap.xml").write_text('<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
                                      + sm + "</urlset>\n", encoding="utf-8", newline="\n")
    budget = 500_000
    worst = max((p.stat().st_size, p) for p in DIST.rglob("*.html"))
    wasm_msg = f", WASM kernel {d['wasm_size']:,} bytes" if d["wasm_sha256"] else ", no WASM binary staged"
    print(f"built {len(pages)} pages into {DIST.relative_to(ROOT)} (largest {worst[1].relative_to(DIST)}: {worst[0]:,} bytes{wasm_msg})")
    if worst[0] > budget:
        print(f"ERROR page over the {budget:,}-byte budget (REQ-OUTR.SITE-010)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
