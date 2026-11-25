#!/usr/bin/env python3
"""Independent fixture generator for AGENT.LOCAL.GUIDE (stdlib only; CC-BY-4.0).

Builds a project-authored board bundle, catalog index, host/model cards, and the
independently rendered golden Markdown and HTML guidance pages under `fixtures/agent/local/`.

Usage:
  python3 fixtures/agent/local/make_fixtures.py          # write files
  python3 fixtures/agent/local/make_fixtures.py --check  # verify files match byte-for-byte
"""

import hashlib
import json
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
LICENSE = "CC-BY-4.0"
# Fixed line required on every generated page (written here independently of the Rust code).
SCOPE_NOTE = (
    "Success rates are board results for the release named above and the tasks it lists; "
    "they are not general accuracy claims about any model."
)


def sha256_str(tag: str) -> str:
    return "sha256:" + hashlib.sha256(tag.encode("utf-8")).hexdigest()


def html_escape(s: str) -> str:
    return (
        s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
        .replace("'", "&#39;")
    )


def format_rate(num: int, den: int) -> str:
    tenths = (num * 1000 + den // 2) // den
    whole, frac = divmod(tenths, 10)
    return f"{num}/{den} ({whole}.{frac}%)"


def build_all():
    workflows = [
        "inspect.align_scan",
        "inspect.check_flatness",
        "inspect.export_report",
        "inspect.measure_bore",
    ]
    catalog = {
        "manifest_digest": sha256_str("agent:local:guide:manifest:v1"),
        "rows": [
            {"catalog_version": sha256_str(f"agent:local:guide:wf:{wf}"), "workflow_id": wf}
            for wf in workflows
        ],
    }

    board = {
        "models": [
            {
                "by_task": {
                    "t-align-01": {"runs": 5, "successes": 5, "violations": 0},
                    "t-align-02": {"runs": 5, "successes": 4, "violations": 0},
                    "t-bore-01": {"runs": 4, "successes": 3, "violations": 0},
                    "t-report-01": {"runs": 5, "successes": 5, "violations": 1},
                },
                "model": "model-a",
                "open_weight": True,
            },
            {
                "by_task": {
                    "t-align-01": {"runs": 5, "successes": 4, "violations": 0},
                    "t-bore-01": {"runs": 4, "successes": 4, "violations": 0},
                    "t-flat-01": {"runs": 5, "successes": 2, "violations": 0},
                },
                "model": "model-b",
                "open_weight": True,
            },
            {
                "by_task": {
                    "t-align-01": {"runs": 5, "successes": 5, "violations": 0},
                },
                "model": "hosted-frontier-closed",
                "open_weight": False,
            },
        ],
        "release": "r2026.1",
        "tasks": {
            "t-align-01": {"workflows": ["inspect.align_scan"]},
            "t-align-02": {"workflows": ["inspect.align_scan"]},
            "t-bore-01": {"workflows": ["inspect.measure_bore"]},
            "t-flat-01": {"workflows": ["inspect.check_flatness"]},
            "t-report-01": {"workflows": ["inspect.export_report"]},
        },
        "verdict": {"outcome": "pass"},
    }

    model_a_host = {
        "cpu_model": "AMD EPYC 7543P 32-Core",
        "gpu_memory_gib": 48,
        "gpu_model": "NVIDIA RTX 6000 Ada",
        "os": "Ubuntu 24.04",
        "ram_gib": 128,
    }
    model_a_model_card = {
        "licence_declared": "Example-Permissive-1.0",
        "name": "model-a",
        "publisher": "Example Open Weights Lab",
        "quantisation": "Q4_K_M",
        "version": "2026.1",
        "weights_sha256": sha256_str("weights:model-a"),
    }
    model_b_model_card = {
        "licence_declared": "Example-Community-1.0",
        "name": "model-b",
        "publisher": "Example Open Weights Lab",
        "quantisation": "Q4_K_M",
        "version": "2026.1",
        "weights_sha256": sha256_str("weights:model-b"),
    }

    # Independent golden Markdown and HTML rendering
    hosts_map = {
        "model-a": "CPU: AMD EPYC 7543P 32-Core; RAM: 128 GiB; GPU: NVIDIA RTX 6000 Ada (48 GiB)",
        "model-b": "not reported",
    }
    licence_map = {
        "model-a": "Example-Permissive-1.0",
        "model-b": "Example-Community-1.0",
    }

    open_models = sorted(
        [m for m in board["models"] if m["open_weight"]],
        key=lambda m: m["model"],
    )
    threshold = Fraction(4, 5)

    md_lines = [
        "# Open-weight Local Model Guidance",
        "",
        "- **Board release:** `r2026.1`",
        "- **Red-team verdict:** `pass`",
        "- **Delegation threshold:** >= 4/5 (80.0%) exact workflow success rate and 0 committed safety violations",
        "",
        SCOPE_NOTE,
        "",
    ]
    html_lines = [
        "<!DOCTYPE html>",
        '<html lang="en">',
        "<head>",
        '<meta charset="utf-8">',
        "<title>Open-weight Local Model Guidance</title>",
        "</head>",
        "<body>",
        "<h1>Open-weight Local Model Guidance</h1>",
        "<ul>",
        "<li><strong>Board release:</strong> <code>r2026.1</code></li>",
        "<li><strong>Red-team verdict:</strong> <code>pass</code></li>",
        "<li><strong>Delegation threshold:</strong> &gt;= 4/5 (80.0%) exact workflow success rate and 0 committed safety violations</li>",
        "</ul>",
        f"<p>{html_escape(SCOPE_NOTE)}</p>",
    ]

    for m in open_models:
        mid = m["model"]
        hw = hosts_map[mid]
        lic = licence_map[mid]
        per_wf = {wf: [0, 0, 0] for wf in workflows}
        for tid, counts in m["by_task"].items():
            for wf in board["tasks"][tid]["workflows"]:
                per_wf[wf][0] += counts["runs"]
                per_wf[wf][1] += counts["successes"]
                per_wf[wf][2] += counts["violations"]

        md_lines.extend(
            [
                f"## {mid}",
                "",
                f"- **Publisher-declared licence:** {lic}",
                f"- **Hardware:** {hw}",
                "",
                "| Workflow | Success rate | Committed violations | Delegation |",
                "| --- | --- | --- | --- |",
            ]
        )
        html_lines.extend(
            [
                f'<section data-model="{html_escape(mid)}">',
                f"<h2>{html_escape(mid)}</h2>",
                "<ul>",
                f"<li><strong>Publisher-declared licence:</strong> {html_escape(lic)}</li>",
                f"<li><strong>Hardware:</strong> {html_escape(hw)}</li>",
                "</ul>",
                "<table>",
                "<thead><tr><th>Workflow</th><th>Success rate</th><th>Committed violations</th><th>Delegation</th></tr></thead>",
                "<tbody>",
            ]
        )

        not_rec = []
        for wf in sorted(workflows):
            runs, succ, viol = per_wf[wf]
            if runs == 0:
                rate_cell = "not evaluated"
                viol_cell = "-"
                status = "not evaluated"
            else:
                rate_cell = format_rate(succ, runs)
                viol_cell = str(viol)
                rec = Fraction(succ, runs) >= threshold and viol == 0
                status = "meets delegation threshold" if rec else "not recommended for delegation"
                if not rec:
                    not_rec.append(wf)
            md_lines.append(f"| `{wf}` | {rate_cell} | {viol_cell} | {status} |")
            html_lines.append(
                f"<tr><td><code>{html_escape(wf)}</code></td><td>{html_escape(rate_cell)}</td>"
                f"<td>{html_escape(viol_cell)}</td><td>{html_escape(status)}</td></tr>"
            )

        md_lines.append("")
        if not_rec:
            md_lines.append(
                "**Not recommended for delegation:** " + ", ".join(f"`{w}`" for w in not_rec)
            )
            html_not_rec = ", ".join(f"<code>{html_escape(w)}</code>" for w in not_rec)
            html_lines.append("</tbody>")
            html_lines.append("</table>")
            html_lines.append(f"<p><strong>Not recommended for delegation:</strong> {html_not_rec}</p>")
        else:
            md_lines.append("**Not recommended for delegation:** none")
            html_lines.append("</tbody>")
            html_lines.append("</table>")
            html_lines.append("<p><strong>Not recommended for delegation:</strong> none</p>")
        md_lines.append("")
        html_lines.append("</section>")

    html_lines.extend(["</body>", "</html>"])

    readme = (
        "# AGENT.LOCAL.GUIDE test fixtures\n"
        "\n"
        "- **Spec node:** `AGENT.LOCAL.GUIDE` (`fid-eval local guide`)\n"
        "- **License:** CC-BY-4.0 (project-authored synthetic scoreboard bundle and catalog index)\n"
        "- **Generator:** `python3 fixtures/agent/local/make_fixtures.py` (verify with `--check`)\n"
        "\n"
        "## Scenarios covered\n"
        "\n"
        "- `model-a` (open-weight, with `host.json` and `model.json`): `inspect.align_scan` 9/10\n"
        "  (meets delegation threshold), `inspect.measure_bore` 3/4 = 0.75 (AC-2: not recommended), `inspect.export_report`\n"
        "  5/5 with 1 committed violation (REQ-002: not recommended), `inspect.check_flatness` 0 runs\n"
        "  (AC-3: `not evaluated`).\n"
        "- `model-b` (open-weight, intentionally missing `host.json`): hardware shown as\n"
        "  `not reported` (REQ-006); `inspect.export_report` 0 runs (`not evaluated`).\n"
        "- `hosted-frontier-closed` (`open_weight: false`): excluded from the open-weight guide.\n"
        "\n"
        "Model, publisher and licence names are synthetic.\n"
    )

    return {
        HERE / "board" / "board.json": json.dumps(board, indent=2, sort_keys=True) + "\n",
        HERE / "catalog-index.json": json.dumps(catalog, indent=2, sort_keys=True) + "\n",
        HERE / "hosts" / "model-a" / "host.json": json.dumps(model_a_host, indent=2, sort_keys=True) + "\n",
        HERE / "hosts" / "model-a" / "model.json": json.dumps(model_a_model_card, indent=2, sort_keys=True) + "\n",
        HERE / "hosts" / "model-b" / "model.json": json.dumps(model_b_model_card, indent=2, sort_keys=True) + "\n",
        HERE / "expected" / "local-model-guide.md": "\n".join(md_lines) + "\n",
        HERE / "expected" / "local-model-guide.html": "\n".join(html_lines) + "\n",
        HERE / "README.md": readme,
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = build_all()
    if check:
        ok = True
        for path, expected in files.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_text(encoding="utf-8") != expected:
                print(f"MISMATCH: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    for path, content in files.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
