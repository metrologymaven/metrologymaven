"""Repository template self-test runner (spec node ECO.REPO.TPL.SELFTEST).

Standard library only. Usage::

    python -m repo_template selftest --checks <required-checks.toml> \\
        [--templates <dir>] [--rules-source <dir>] [--repo-id <id>] \\
        [--rules-rev <40-hex>] [--ci-rev <40-hex>] [--year <YYYY>]

For each variant in ``<templates>/variants.toml``:
1. Instantiates the variant into a fresh temporary directory with fixed revisions.
2. Initializes a local git repository and creates one commit with fixed author, committer
   and timestamp (no network, hardened git environment).
3. Runs every check listed in ``--checks`` using its ``local`` command.
4. Prints a deterministic summary listing each ``<variant>/<check>`` with ``PASS`` or ``FAIL``.

Errors (exit 1):
- ``SELFTEST-NO-LOCAL-COMMAND <check>``: a required check omits ``local`` or leaves it empty.
- ``SELFTEST-CHECK-FAILED <variant>/<check>``: a required check exited non-zero.
"""

from __future__ import annotations

import argparse
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
ROOT = TOOLS.parent
DEFAULT_RULES_SOURCE = ROOT / "house-rules"
DEFAULT_REPO_ID = "metrologymaven"
DEFAULT_RULES_REV = "a" * 40
DEFAULT_CI_REV = "b" * 40
DEFAULT_YEAR = "2026"
FIXED_AUTHOR_NAME = "Metrology Maven Selftest"
FIXED_AUTHOR_EMAIL = "selftest@metrologymaven.invalid"
FIXED_COMMIT_DATE = "2026-01-01T00:00:00Z"
CHECK_TIMEOUT_S = 120
GIT_TIMEOUT_S = 60

if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from house_rules import find_blocks  # noqa: E402
from house_rules.check import git_env  # noqa: E402
from repo_template.new import (  # noqa: E402
    DEFAULT_TEMPLATES,
    TemplateError,
    instantiate,
    load_variants,
)

try:  # Python 3.11+
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - CI runs 3.12
    tomllib = None


class SelftestError(Exception):
    """Typed selftest error whose message begins with ``SELFTEST-*``."""

    def __init__(self, code: str, message: str):
        super().__init__(f"{code} {message}".strip())
        self.code = code
        self.message = message


@dataclass(frozen=True)
class CheckSpec:
    """One required check loaded from ``required-checks.toml``."""

    id: str
    local: str | list[str]


@dataclass(frozen=True)
class CheckOutcome:
    """Outcome of running one check against one template variant."""

    variant: str
    check_id: str
    passed: bool
    returncode: int
    detail: str = ""


def load_required_checks(path: Path) -> list[CheckSpec]:
    """Parses ``required-checks.toml`` and enforces REQ-ECO.REPO.TPL.SELFTEST-004."""
    if tomllib is None:
        raise SelftestError("SELFTEST-PARSE", "Python 3.11+ tomllib required")
    try:
        raw = path.read_bytes()
    except OSError as e:
        raise SelftestError("SELFTEST-PARSE", f"{path}: {e}") from None
    try:
        data = tomllib.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as e:
        raise SelftestError("SELFTEST-PARSE", f"{path}: {e}") from None

    raw_checks = data.get("check")
    if not isinstance(raw_checks, list) or not raw_checks:
        raise SelftestError("SELFTEST-PARSE", f"{path}: expected non-empty [[check]] list")

    checks: list[CheckSpec] = []
    seen: set[str] = set()
    for idx, item in enumerate(raw_checks):
        if not isinstance(item, dict):
            raise SelftestError("SELFTEST-PARSE", f"{path}: check[{idx}] must be a table")
        cid = item.get("id") if "id" in item else item.get("name")
        if not isinstance(cid, str) or not cid.strip():
            raise SelftestError("SELFTEST-PARSE", f"{path}: check[{idx}] missing non-empty id")
        if cid in seen:
            raise SelftestError("SELFTEST-PARSE", f"{path}: duplicate check id {cid!r}")
        seen.add(cid)

        local = item.get("local")
        if local is None:
            raise SelftestError("SELFTEST-NO-LOCAL-COMMAND", cid)
        if isinstance(local, str):
            if not local.strip():
                raise SelftestError("SELFTEST-NO-LOCAL-COMMAND", cid)
        elif isinstance(local, list):
            if not local or not all(isinstance(part, str) and part != "" for part in local):
                raise SelftestError("SELFTEST-NO-LOCAL-COMMAND", cid)
        else:
            raise SelftestError("SELFTEST-NO-LOCAL-COMMAND", cid)
        checks.append(CheckSpec(id=cid, local=local))

    return checks


def _apply_placeholders(text: str, mapping: dict[str, str]) -> str:
    for k, v in mapping.items():
        text = text.replace("{{" + k + "}}", v)
    return text


def _copy_preserving_blocks(
    templates: Path,
    variant: str,
    repo_id: str,
    ci_rev: str,
    year: str,
    out: Path,
) -> None:
    """Fallback instantiation when a fixture template's AGENTS.md already embeds a managed block
    (e.g. a hand-edited block for AC-2) so downstream checks execute against it."""
    variants = load_variants(templates)
    folders = variants[variant]
    mapping = {"repo_id": repo_id, "ci_rev": ci_rev, "year": year}
    out.mkdir(parents=True, exist_ok=True)
    for folder in folders:
        src_root = templates / folder
        for src in sorted(src_root.rglob("*")):
            if src.is_dir():
                continue
            rel = src.relative_to(src_root).as_posix()
            dst = out / _apply_placeholders(rel, mapping)
            dst.parent.mkdir(parents=True, exist_ok=True)
            data = src.read_bytes()
            try:
                text = data.decode("utf-8")
                dst.write_bytes(_apply_placeholders(text, mapping).encode("utf-8"))
            except UnicodeDecodeError:
                dst.write_bytes(data)


def _template_has_preexisting_blocks(templates: Path, variant: str) -> bool:
    variants = load_variants(templates)
    for folder in variants.get(variant, ()):
        agents = templates / folder / "AGENTS.md"
        if agents.is_file():
            try:
                if find_blocks(agents.read_text(encoding="utf-8")):
                    return True
            except Exception:
                return True
    return False


def _git_hardened(repo_dir: Path, args: list[str]) -> None:
    env = git_env()
    env.update(
        {
            "GIT_AUTHOR_NAME": FIXED_AUTHOR_NAME,
            "GIT_AUTHOR_EMAIL": FIXED_AUTHOR_EMAIL,
            "GIT_AUTHOR_DATE": FIXED_COMMIT_DATE,
            "GIT_COMMITTER_NAME": FIXED_AUTHOR_NAME,
            "GIT_COMMITTER_EMAIL": FIXED_AUTHOR_EMAIL,
            "GIT_COMMITTER_DATE": FIXED_COMMIT_DATE,
        }
    )
    cmd = [
        "git",
        "-c",
        "protocol.allow=never",
        "-c",
        "core.fsmonitor=false",
        "-c",
        f"core.hooksPath={os.devnull}",
        *args,
    ]
    r = subprocess.run(
        cmd,
        cwd=repo_dir,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=GIT_TIMEOUT_S,
    )
    if r.returncode != 0:
        detail = (r.stderr.strip() or r.stdout.strip() or f"exit {r.returncode}")
        raise SelftestError("SELFTEST-GIT", f"git {' '.join(args)} failed: {detail}")


def _init_and_commit(repo_dir: Path) -> None:
    _git_hardened(repo_dir, ["init", "-q", "-b", "main"])
    _git_hardened(repo_dir, ["add", "-A"])
    _git_hardened(repo_dir, ["commit", "-q", "-m", "Initial commit from repo template"])


def _manifest_path(rules_source: Path) -> Path:
    toml_path = rules_source / "manifest.toml"
    if toml_path.is_file():
        return toml_path
    return rules_source / "manifest.json"


def _expand_command(
    spec: CheckSpec,
    repo_dir: Path,
    variant: str,
    repo_id: str,
    rules_source: Path,
    rules_rev: str,
    ci_rev: str,
) -> list[str]:
    repl = {
        "{python}": sys.executable,
        "{repo_dir}": str(repo_dir),
        "{variant}": variant,
        "{repo_id}": repo_id,
        "{rules_source}": str(rules_source),
        "{rules_rev}": rules_rev,
        "{ci_rev}": ci_rev,
        "{manifest}": str(_manifest_path(rules_source)),
    }
    raw_parts = spec.local if isinstance(spec.local, list) else shlex.split(spec.local)
    out: list[str] = []
    for part in raw_parts:
        s = part
        for k, v in repl.items():
            s = s.replace(k, v)
        out.append(s)
    return out


def run_selftest(
    checks_path: Path,
    templates: Path = DEFAULT_TEMPLATES,
    rules_source: Path = DEFAULT_RULES_SOURCE,
    repo_id: str = DEFAULT_REPO_ID,
    rules_rev: str = DEFAULT_RULES_REV,
    ci_rev: str = DEFAULT_CI_REV,
    year: str = DEFAULT_YEAR,
) -> list[CheckOutcome]:
    """Runs all required checks against every variant in ``templates/variants.toml``."""
    checks = load_required_checks(checks_path)
    try:
        variants = load_variants(templates)
    except TemplateError as e:
        raise SelftestError("SELFTEST-TEMPLATE", str(e)) from None

    if not variants:
        raise SelftestError("SELFTEST-TEMPLATE", f"{templates / 'variants.toml'}: no variants defined")

    outcomes: list[CheckOutcome] = []
    for variant in variants:
        with tempfile.TemporaryDirectory(prefix=f"repo-tpl-selftest-{variant}-") as tmp:
            scratch = Path(tmp) / "repo"
            if _template_has_preexisting_blocks(templates, variant):
                _copy_preserving_blocks(templates, variant, repo_id, ci_rev, year, scratch)
            else:
                try:
                    instantiate(
                        repo_id=repo_id,
                        variant=variant,
                        rules_source=rules_source,
                        rules_rev=rules_rev,
                        ci_rev=ci_rev,
                        out=scratch,
                        templates=templates,
                        year=year,
                    )
                except TemplateError as e:
                    raise SelftestError("SELFTEST-INSTANTIATE", f"{variant}: {e}") from None

            _init_and_commit(scratch)

            env = dict(os.environ)
            existing_pp = env.get("PYTHONPATH", "")
            env["PYTHONPATH"] = os.pathsep.join([str(TOOLS)] + ([existing_pp] if existing_pp else []))
            env.update(
                {
                    "SELFTEST_REPO_DIR": str(scratch),
                    "SELFTEST_VARIANT": variant,
                    "SELFTEST_REPO_ID": repo_id,
                    "SELFTEST_RULES_SOURCE": str(rules_source),
                    "SELFTEST_RULES_REV": rules_rev,
                    "SELFTEST_CI_REV": ci_rev,
                    "SELFTEST_MANIFEST": str(_manifest_path(rules_source)),
                    "SELFTEST_PYTHON": sys.executable,
                }
            )

            for spec in checks:
                argv = _expand_command(spec, scratch, variant, repo_id, rules_source, rules_rev, ci_rev)
                try:
                    res = subprocess.run(
                        argv,
                        cwd=scratch,
                        env=env,
                        stdin=subprocess.DEVNULL,
                        capture_output=True,
                        text=True,
                        timeout=CHECK_TIMEOUT_S,
                    )
                    detail = (res.stdout.strip() + "\n" + res.stderr.strip()).strip()
                    outcomes.append(
                        CheckOutcome(
                            variant=variant,
                            check_id=spec.id,
                            passed=(res.returncode == 0),
                            returncode=res.returncode,
                            detail=detail,
                        )
                    )
                except (OSError, subprocess.SubprocessError) as e:
                    outcomes.append(
                        CheckOutcome(
                            variant=variant,
                            check_id=spec.id,
                            passed=False,
                            returncode=1,
                            detail=str(e),
                        )
                    )
    return outcomes


def add_selftest_subparser(sub: argparse._SubParsersAction) -> None:
    p = sub.add_parser("selftest", help="instantiate every template variant and run required checks")
    p.add_argument("--checks", required=True, type=Path, help="path to required-checks.toml")
    p.add_argument("--templates", type=Path, default=DEFAULT_TEMPLATES, help="template folder containing variants.toml")
    p.add_argument("--rules-source", type=Path, default=DEFAULT_RULES_SOURCE, help="house-rules source folder")
    p.add_argument("--repo-id", default=DEFAULT_REPO_ID, help="repository ID for instantiation")
    p.add_argument("--rules-rev", default=DEFAULT_RULES_REV, help="40-hex house-rules revision")
    p.add_argument("--ci-rev", default=DEFAULT_CI_REV, help="40-hex shared CI workflows revision")
    p.add_argument("--year", default=DEFAULT_YEAR, help="four-digit licence year")


def run_selftest_args(args: argparse.Namespace) -> int:
    try:
        outcomes = run_selftest(
            checks_path=args.checks,
            templates=args.templates,
            rules_source=args.rules_source,
            repo_id=args.repo_id,
            rules_rev=args.rules_rev,
            ci_rev=args.ci_rev,
            year=args.year,
        )
    except SelftestError as e:
        print(str(e), file=sys.stderr)
        return 1

    failed: list[CheckOutcome] = []
    for o in outcomes:
        status = "PASS" if o.passed else "FAIL"
        print(f"{status} {o.variant}/{o.check_id}")
        if not o.passed:
            failed.append(o)
            if o.detail:
                for line in o.detail.splitlines():
                    print(f"  {line}", file=sys.stderr)

    if failed:
        for o in failed:
            print(f"SELFTEST-CHECK-FAILED {o.variant}/{o.check_id}", file=sys.stderr)
        return 1
    return 0
