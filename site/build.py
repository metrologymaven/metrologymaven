#!/usr/bin/env python3
"""Build the Metrology Maven website (OUTR.SITE) into site/dist/.

Single command, no dependencies beyond Python 3.10+:   python site/build.py

Every number, status and roadmap entry on the site is generated from the spec
tree at the current revision (REQ-OUTR.SITE-001/002/004), never typed by hand.
The output is plain static files with no third-party requests (REQ-OUTR.SITE-007).
"""

from __future__ import annotations

import datetime as dt
import html
import shutil
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

SITE = Path(__file__).resolve().parent
ROOT = SITE.parent
sys.path.insert(0, str(ROOT / "tools"))
import scope  # noqa: E402

DIST = SITE / "dist"
ORIGIN = "https://metrologymaven.io"
REPO = "https://github.com/metrologymaven/metrologymaven"
CONTACT = "hello@metrologymaven.io"
e = html.escape

PHASES = [
    ("P0", "Foundations", "Numerics, least-squares and minimum-zone fits, the command registry, CLI and MCP, the journal, a QIF subset and the free verifier."),
    ("P1a", "First usable product", "Transcription reports, a prismatic GD&T subset (ASME), a minimal GUI and 3D viewer."),
    ("P1b", "Scan-to-CAD inspection", "Mesh and point-cloud inspection, ISO GPS dialect, CAD + PMI, SPC, Python scripting, report designer."),
    ("P2", "CMMs and live devices", "CMM programming and execution: I++ DME client, simulator, approval station, DMIS import."),
    ("P3", "Large volume and form", "Laser-tracker networks, live portable arms, form and texture, automatic path planning and process-aware sampling research."),
    ("P4", "Ecosystem", "OPC UA, MTConnect, a plugin marketplace, and open-format workflow migration."),
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

NAV = [("/", "Home"), ("/apps/", "Apps"), ("/how-it-works/", "How it works"), ("/status/", "Status"),
       ("/roadmap/", "Roadmap"), ("/contribute/", "Contribute"), ("/company/", "Company")]

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
            if n.meta.get("needs_expert") and n.status == "open":
                per[dom(n)]["expert"] += 1
            if n.lst("built_in"):
                per[dom(n)]["built"] += 1
    phase_leaves = Counter(n.meta.get("phase") for n in nodes if n.status == "leaf")
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
    queue = scope.expert_queue(nodes)
    return {"nodes": nodes, "by_id": by_id, "domains": domains, "per": per, "phase_leaves": phase_leaves,
            "seeds": seeds, "seed_rows": seed_rows, "decisions": decisions, "queue": queue,
            "leaves": sum(1 for n in nodes if n.status == "leaf"),
            "built": sum(1 for n in nodes if n.lst("built_in")),
            "open": sum(1 for n in nodes if n.status == "open"),
            "rev": git_rev(), "date": dt.date.today().isoformat()}


# ---------------------------------------------------------------- layout

def page(path: str, title: str, desc: str, body: str, d) -> tuple[str, str]:
    nav = "".join(f'<a href="{h}"{" aria-current=page" if h == path else ""}>{t}</a>' for h, t in NAV)
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
</head>
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


NOTICE = ('<p class="notice"><b>Honest status:</b> nothing here is usable yet. The project is in its '
          'specification stage: every feature is written down as a testable work order, and the first code is being '
          'built in the open. Numbers on this site are generated from the repository.</p>')


def app_card(a) -> str:
    return (f'<a class="card" href="/apps/{a["slug"]}/"><span class="tag">App {a["order"]} · starts {a["phase"]}</span>'
            f'<h3>Metrology Maven {e(a["name"])}</h3><p>{e(a["workflow"])}</p>'
            f'<span class="pill spec">specification</span></a>')


# ---------------------------------------------------------------- pages

def home(d):
    s = d
    body = f"""
<section class="hero">
{fcf("⌖", "OPEN SOURCE", "DIMENSIONAL METROLOGY")}
<h1>Open-source inspection software for scanners, CMMs and laser trackers</h1>
<p class="lead">Metrology Maven is an open-source, QIF-native dimensional metrology platform for independent machine shops, quality labs and engineers. It is free to use, local-first, and every result can be re-checked by anyone. People and AI agents drive it through the same typed commands.</p>
<div class="btns"><a class="btn primary" href="{REPO}">See it on GitHub</a><a class="btn" href="/contribute/">Help build it</a><a class="btn" href="/how-it-works/">How it works</a></div>
{NOTICE}
</section>
<section class="sheet" aria-labelledby="apps-h">
{fcf("1", "FOUR APPS", "ONE CORE")}
<h2 id="apps-h">Four focused apps on one shared core</h2>
<p class="muted">Each app does one job people already understand. Under the hood they share one math kernel, one file format (QIF-native), one command list and one 3D viewer, so results move between them without conversion.</p>
<div class="grid">{"".join(app_card(a) for a in APPS)}</div>
</section>
<section class="sheet" aria-labelledby="why-h">
{fcf("2", "WHY", "DIFFERENT")}
<h2 id="why-h">Designed around trust, not licences</h2>
<div class="grid">
<div class="card"><h3>Free and open</h3><p>Code under MIT OR Apache-2.0. No locked features, no dongles, no per-seat fees.</p></div>
<div class="card"><h3>Every number can be checked</h3><p>A free, independent verifier re-checks results from the recorded journal, bit for bit.</p></div>
<div class="card"><h3>Agents first</h3><p>Every action is a typed command available from the CLI and MCP, designed so automation handles routine steps while people approve at a few fixed gates.</p></div>
<div class="card"><h3>Records everything</h3><p>Every step is journaled, screenshotted and replayable, which makes audits a matter of reading the record.</p></div>
</div>
</section>
<section class="sheet" aria-labelledby="num-h">
{fcf("3", "THE PLAN", "IN NUMBERS")}
<h2 id="num-h">The whole product, written down before it is built</h2>
<p class="muted">The plan is a public tree of small specification files. Each piece is split until it is one pull request of work, with its requirements, tests and a build prompt that a coding agent or a person can follow.</p>
<div class="stats">
<div class="stat"><b>{len(s['domains'])}</b><small>capability domains</small></div>
<div class="stat"><b>{len(s['nodes']):,}</b><small>spec nodes</small></div>
<div class="stat"><b>{s['leaves']:,}</b><small>ready-to-build work orders</small></div>
<div class="stat"><b>{s['built']:,}</b><small>work orders built in code</small></div>
<div class="stat"><b>{len(s['seeds'])}</b><small>industry workflow features tracked</small></div>
<div class="stat"><b>{sum(len(v) for v in s['queue'].values())}</b><small>tasks waiting for experts</small></div>
</div>
<p><a href="/status/">See the full status, generated from the repository →</a></p>
</section>
"""
    return page("/", "Home", "Open-source dimensional metrology software: inspection, CMM programming, laser-tracker networks and reporting, built in the open.", body, d)


def apps_index(d):
    body = f"""
<section class="hero">{fcf("A", "APPS")}
<h1>The app family</h1>
<p class="lead">Report and Inspect come first, because they help shops that run several brands of equipment straight away. Probe and Network follow.</p>{NOTICE}</section>
<section class="sheet"><div class="grid">{"".join(app_card(a) for a in APPS)}</div></section>"""
    return page("/apps/", "Apps", "The Metrology Maven app family: Report, Inspect, Probe and Network.", body, d)


def app_page(a, d):
    per = d["per"]
    rows = []
    gaps = []
    for dom in a["domains"]:
        n = d["by_id"].get(dom)
        c = per[dom]
        rows.append(f'<tr><td><span class="mono">{dom}</span> {e(n.meta.get("title", "") if n else "")}</td>'
                    f'<td class="n">{c["leaf"]}</td><td class="n">{c["built"]}</td><td class="n">{c["open"]}</td><td class="n">{c["expert"]}</td></tr>')
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
    gap_list = "".join(f'<li><span class="mono">{e(x.id)}</span> {e(x.meta.get("title", ""))}'
                       f'{" <span class=\"pill red\">needs expert</span>" if x.meta.get("needs_expert") else ""}</li>'
                       for x in sorted(gaps, key=lambda x: x.id)[:60])
    more = f"<li>…and {len(gaps) - 60} more in the repository</li>" if len(gaps) > 60 else ""
    body = f"""
<section class="hero">{fcf(str(a["order"]), "APP", a["name"].upper(), "STARTS " + a["phase"])}
<h1>Metrology Maven {e(a["name"])}</h1>
<p class="lead">{e(a["pitch"])}</p>
<p>Workflow focus: <b>{e(a["workflow"])}</b></p>
<p><span class="pill spec">Status: specification (pre-alpha)</span> <span class="pill p">Work starts in {a["phase"]}</span></p>
</section>
<section class="sheet">{fcf("S", "STATUS", "GENERATED")}
<h2>Where it stands</h2>
<p class="muted">Generated from the spec tree at <span class="mono">{d['rev']}</span>. "Ready" means a work order is written and can be built; it does not mean the feature exists yet.</p>
<div class="tbl"><table><thead><tr><th>Domain</th><th>Ready to build</th><th>Built</th><th>Still open</th><th>Needs expert</th></tr></thead><tbody>{"".join(rows)}</tbody></table></div>
{seeds}
<h3>Known gaps</h3>
<details><summary>{len(gaps)} open spec nodes in this app's domains</summary><ul class="clean">{gap_list}{more}</ul></details>
</section>"""
    return page(f"/apps/{a['slug']}/", f"{a['name']}", f"Metrology Maven {a['name']}: {a['pitch']}", body, d)


def how(d):
    body = f"""
<section class="hero">{fcf("H", "HOW IT WORKS")}
<h1>How the project is built</h1>
<p class="lead">Most of the work is done by AI coding agents following a public, very detailed plan. People act at a few fixed points where judgement and accountability matter.</p></section>
<section class="sheet">{fcf("1", "THE SPEC TREE")}
<h2>A plan you can run</h2>
<p>The product is described as a tree: 1 product, {len(d['domains'])} domains, then subsystems, modules and features, down to <b>{d['leaves']:,} leaves</b>. A leaf is one pull request of work. It contains testable requirements, acceptance tests with named data, a design, and a build prompt that a coding agent can follow on its own.</p>
<ol class="steps"><li><b>Pick</b>an open node</li><li><b>Expand</b>split it, or finish it as a leaf</li><li><b>Validate</b><code>tools/scope.py check</code> enforces every rule</li><li><b>Review</b>humans for big splits, an independent agent for small ones</li><li><b>Build</b>leaves are built in dependency order, in parallel waves</li></ol>
<p class="muted">The validator fails a change that drops a requirement or a tracked workflow feature, creates a dependency loop, or adds a feature without its CLI command, MCP tool and journal entry.</p>
</section>
<section class="sheet">{fcf("2", "ARCHITECTURE")}
<h2>Five layers, each using only the ones below</h2>
<div class="tbl"><table><thead><tr><th>Layer</th><th>What lives there</th></tr></thead><tbody>
<tr><td><b>L3 Surfaces</b></td><td>CLI, MCP servers, Python, the GPU viewer, the four apps, agent skills</td></tr>
<tr><td><b>L2 Services</b></td><td>File formats, alignment, GD&T, uncertainty, scanning, reports, signing, instruments, CMM, trackers</td></tr>
<tr><td><b>L1 Core</b></td><td>Command registry, command bus, tamper-evident journal, replay, project storage</td></tr>
<tr><td><b>L0 Foundation</b></td><td>Deterministic math kernel: fits, geometry, GD&T math, units. Never touched by the GPU.</td></tr>
</tbody></table></div>
<p class="muted">Rust throughout (including the web build via WebAssembly), wgpu for graphics, Python for user scripting.</p>
</section>
<section class="sheet">{fcf("3", "HUMAN GATES")}
<h2>Where people stay in charge</h2>
<div class="grid">
<div class="card"><span class="tag">H1</span><h3>Machine motion</h3><p>A person approves each CMM program once, after simulation. A physical start remains on every cycle.</p></div>
<div class="card"><span class="tag">H2</span><h3>Record release</h3><p>Controlled records such as first-article reports are released by a person.</p></div>
<div class="card"><span class="tag">H3</span><h3>Conformance</h3><p>Accept/reject decisions where a customer's quality system requires a named person.</p></div>
<div class="card"><span class="tag">H4</span><h3>Interpretation</h3><p>GD&T interpretation rules are signed off by credentialed experts.</p></div>
</div>
<p class="muted">An agent never acts as a person: every action is recorded under its own principal identity.</p>
</section>"""
    return page("/how-it-works/", "How it works", "How Metrology Maven is specified, built by agents and governed by human gates.", body, d)


def status(d):
    rows = []
    mx = max(d["per"][x.id]["nodes"] for x in d["domains"]) or 1
    for x in d["domains"]:
        c = d["per"][x.id]
        rows.append(f'<tr><td><span class="mono">{x.id}</span></td><td>{e(x.meta.get("title", ""))}</td><td class="n">{x.meta.get("phase", "")}</td>'
                    f'<td class="n">{c["nodes"]}</td><td class="n">{c["leaf"]}</td><td class="n">{c["built"]}</td><td class="n">{c["open"]}</td><td class="n">{c["expert"]}</td>'
                    f'<td><div class="bar"><i style="width:{100 * c["leaf"] / mx:.0f}%"></i></div></td></tr>')
    srows = "".join(f'<tr><td>{e(f.removesuffix(".md"))}</td><td class="n">{c["covered"]}</td><td class="n">{c["out of scope"]}</td><td class="n">{c["uncovered"]}</td></tr>'
                    for f, c in sorted(d["seed_rows"].items()))
    acc = sum(1 for x in d["decisions"] if x.get("status") == "accepted")
    open_dec = [x for x in d["decisions"] if x.get("status") not in ("accepted", "superseded")]
    dec = "".join(f'<li><span class="mono">{e(x.get("id", ""))}</span> {e(x.get("title", ""))}</li>' for x in open_dec) or "<li>None</li>"
    body = f"""
<section class="hero">{fcf("S", "STATUS", d['rev'], d['date'])}
<h1>Project status</h1>
<p class="lead">Generated from the repository at commit <a class="mono" href="{REPO}/commit/{d['rev']}">{d['rev']}</a> on {d['date']}. Nothing on this page is written by hand.</p>{NOTICE}</section>
<section class="sheet">{fcf("1", "SPEC TREE BY DOMAIN")}
<div class="stats"><div class="stat"><b>{len(d['nodes']):,}</b><small>nodes</small></div><div class="stat"><b>{d['leaves']:,}</b><small>leaves (ready to build)</small></div><div class="stat"><b>{d['built']:,}</b><small>leaves built in code</small></div><div class="stat"><b>{d['open']}</b><small>open nodes</small></div><div class="stat"><b>{acc}/{len(d['decisions'])}</b><small>decisions accepted</small></div></div>
<div class="tbl"><table><thead><tr><th>ID</th><th>Domain</th><th>Starts</th><th>Nodes</th><th>Leaves</th><th>Built</th><th>Open</th><th>Expert</th><th>Leaves</th></tr></thead><tbody>{"".join(rows)}</tbody></table></div>
<p class="muted">Full tree: <a href="{REPO}/blob/main/generated/INDEX.md">generated/INDEX.md</a>.</p>
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
                     f'<td class="n">{d["phase_leaves"].get(p, 0)}</td><td>{apps or "–"}</td></tr>')
    body = f"""
<section class="hero">{fcf("R", "ROADMAP", "BY PHASE")}
<h1>Roadmap</h1>
<p class="lead">Phases describe the order in which work starts. There are no dates here: dates are published only when a launch milestone is committed.</p></section>
<section class="sheet"><div class="tbl"><table><thead><tr><th>Phase</th><th>Theme</th><th>Leaves</th><th>App work starts</th></tr></thead><tbody>{"".join(items)}</tbody></table></div>
<p class="muted">An app reaches 1.0 only when every phase holding its core workflow is finished: Report and Inspect after P1b, Probe after P2, Network after P3. The first milestone, <b>launch-1</b>, targets Inspect and Report: <a href="{REPO}/blob/main/milestones/launch-1.md">milestones/launch-1.md</a>.</p></section>"""
    return page("/roadmap/", "Roadmap", "The Metrology Maven roadmap by phase, generated from the spec tree.", body, d)


def contribute(d):
    blocks = []
    for area, items in d["queue"].items():
        li = "".join(f'<li><a href="{REPO}/blob/main/{scope.rel(n.path)}"><span class="mono">{e(n.id)}</span></a> {e(n.meta.get("title", ""))}</li>' for n in items)
        blocks.append(f'<details><summary>{e(scope.EXPERT_AREAS[area])} · {len(items)} tasks</summary><ul class="clean">{li}</ul>'
                      f'<p><a href="{REPO}/issues?q=is%3Aissue+is%3Aopen+label%3Aexpert%3A{area}">Open issues labelled expert:{area} →</a></p></details>')
    body = f"""
<section class="hero">{fcf("C", "CONTRIBUTE")}
<h1>Help build open metrology software</h1>
<p class="lead">The work is already broken into small pieces. Pick one that fits what you know. Claiming is automatic: comment <code>/claim</code> on an issue.</p>
<div class="btns"><a class="btn primary" href="{REPO}/issues?q=is%3Aissue+is%3Aopen+label%3Aexpert-queue">Expert tasks</a><a class="btn" href="{REPO}/issues/new?template=expert-offer.yml">Offer your expertise</a><a class="btn" href="{REPO}/blob/main/CONTRIBUTING.md">Contributing guide</a></div></section>
<section class="sheet">{fcf("1", "METROLOGY EXPERTS")}
<h2>We need people who know GD&T, uncertainty and CMMs</h2>
<p>Automated workflows draft most of the specification, but some parts must not be guessed: how a standard is interpreted, how uncertainty is budgeted, how a controller moves. Those tasks are marked for experts and listed below. You can write the piece yourself, or review a draft in plain words and approve the result.</p>
{"".join(blocks)}
<p class="muted">You'll be credited by name in the spec, in CONTRIBUTORS.md and in release notes. Never paste text from a standard: cite the clause and explain it in your own words.</p>
</section>
<section class="sheet">{fcf("2", "OTHER WAYS")}
<div class="grid">
<div class="card"><h3>Work in a machine shop or quality lab?</h3><p>Review workflow checklists in <code>seeds/</code> or partner with us on non-proprietary test parts for real-world CMM and scan benchmarks. No coding needed.</p></div>
<div class="card"><h3>Run a coding agent?</h3><p>Pick an open node with <code>python tools/scope.py open</code> and follow <code>prompts/expand-node.md</code>. Name your agent tool in the PR.</p></div>
<div class="card"><h3>Write Rust?</h3><p>Build leaves from the first waves in <code>generated/milestones/launch-1.md</code>. Each leaf's build prompt is self-contained.</p></div>
<div class="card"><h3>Make instruments or run a university lab?</h3><p>Talk to us about open-protocol driver integration (I++ DME, QIF) or joint research benchmarks: <a href="mailto:{CONTACT}">{CONTACT}</a>.</p></div>
</div></section>"""
    return page("/contribute/", "Contribute", "Contribute to Metrology Maven: expert tasks in GD&T, uncertainty and CMM control, workflow verification and Rust engineering.", body, d)


def company(d):
    body = f"""
<section class="hero">{fcf("M", "COMPANY")}
<h1>Metrology Maven, the company</h1>
<p class="lead">Metrology Maven stewards the open-source project and funds its infrastructure. The open-source software is free. The company earns from enterprise services and regulated evidence packages.</p></section>
<section class="sheet">{fcf("1", "OPEN BOUNDARY")}<div class="two">
<div class="col free"><h3>Free forever</h3><ul class="clean"><li>All four apps, the deterministic math kernel and QIF-native formats</li><li>The CLI and MCP servers</li><li>The independent verifier (<code>fid-verify</code>)</li><li>Evidence templates and open test runners</li></ul></div>
<div class="col"><h3>Commercial services (planned)</h3><ul class="clean"><li>Pre-built regulated release dossiers for aerospace, medical and accredited labs</li><li>Support agreements and long-term-support releases</li><li>Hosted verification and evidence archive</li><li>On-premises private machine-adapter fine-tuning and fleet drift monitoring</li><li>Instrument-maker integration testing</li></ul></div>
</div>
<p>No feature of the open apps will ever be moved behind a paid service (D-036). Interested in early access, a shop-floor pilot or a research partnership? <a href="mailto:{CONTACT}">{CONTACT}</a></p></section>
<section class="sheet">{fcf("2", "FUTURE ROLES", "NOT ACTIVELY HIRING")}
<h2>Future roles &amp; talent network</h2>
<p class="notice"><b>Transparent status:</b> Metrology Maven is pre-alpha and is <b>not actively hiring full-time yet</b>. We share our planned team structure early so interested engineers, metrologists and technicians can follow the project and be notified when roles open.</p>
<div class="grid">
<div class="card"><h3>Core &amp; Geometry Engineering</h3><p>Deterministic Rust kernel, computational geometry (B-rep/NURBS), <code>wgpu</code>/WASM graphics, QIF/STEP/DMIS interoperability, and process-aware ML research.</p></div>
<div class="card"><h3>Metrology &amp; Verification</h3><p>Dimensional metrologists (ASME Y14.5 &amp; ISO GPS), measurement uncertainty &amp; SPC engineers, and independent verification engineers.</p></div>
<div class="card"><h3>CMM Lab &amp; Shop-Floor Technicians</h3><p>CMM programmers, CMM operators, 3D scanning technicians, portable/LVM technicians, calibration technicians, and field deployment specialists.</p></div>
<div class="card"><h3>Product, Design &amp; Community</h3><p>Technical CAD/inspection UX designers, metrology documentation writers, and open-source developer relations.</p></div>
</div>
<p class="muted">Want to be notified when full-time or paid part-time benchmark roles open? Email <a href="mailto:{CONTACT}?subject=Talent%20Network">{CONTACT}</a> with your background and role area, or start by collaborating on an open <a href="/contribute/">spec or benchmark task</a>.</p></section>"""
    return page("/company/", "Company", "Metrology Maven stewards the open-source metrology platform, planned commercial services, and future team roadmap.", body, d)


def not_found(d):
    body = f"""<section class="hero">{fcf("404", "NOT FOUND")}<h1>That page isn't here</h1>
<p class="lead">This site was rebuilt for the open-source project. Try the <a href="/">home page</a> or the <a href="/status/">status page</a>.</p></section>"""
    return page("/404", "Not found", "Page not found.", body, d)


# ---------------------------------------------------------------- build

def main() -> int:
    d = load()
    pages = [home(d), apps_index(d), how(d), status(d), roadmap(d), contribute(d), company(d), not_found(d)]
    pages += [app_page(a, d) for a in APPS]
    if DIST.exists():
        shutil.rmtree(DIST)
    DIST.mkdir(parents=True)
    for f in (SITE / "src").iterdir():
        shutil.copy2(f, DIST / f.name)
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
    print(f"built {len(pages)} pages into {DIST.relative_to(ROOT)} (largest {worst[1].relative_to(DIST)}: {worst[0]:,} bytes)")
    if worst[0] > budget:
        print(f"ERROR page over the {budget:,}-byte budget (REQ-OUTR.SITE-010)")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
