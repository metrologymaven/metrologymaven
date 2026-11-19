"""Instantiate a repository from ``templates/repo/`` (spec node ECO.REPO.TPL.FILES).

Steps, in order:

1. Refuse an output folder that exists and is not empty (``TPL-OUT-NOT-EMPTY``), and an output
   folder whose parent does not exist (``TPL-OUT``). Nothing is written in either case.
2. Build the result in a staging folder *inside* the output folder
   (``<out>/.repo-template.partial``), so the tool never writes outside the folder it was given.
3. Copy the variant's folders (``variants.toml``) into the staging folder, in sorted order, and
   substitute ``{{repo_id}}``, ``{{ci_rev}}`` and ``{{year}}``.
4. Write the house-rules blocks and ``CLAUDE.md`` by running
   ``python -m house_rules sync --source <rules-source> --source-rev <rules-rev> --repo-id <id>
   --target <staging>`` (ECO.RULES.SYNC). Until that tool exists in ``tools/house_rules/``, an
   equivalent built-in step that only handles a block-free ``AGENTS.md`` (always the case for a
   fresh template) renders the blocks with :func:`house_rules.render`. Any failure is
   ``TPL-SYNC-FAILED``.
5. Scan every output path name and file for a leftover ``{{name}}`` token (``TPL-PLACEHOLDER``).
6. Move the staged files into the output folder.

On any failure after step 1 the partial output is deleted: the output folder is removed if this
run created it, or left empty if it existed empty before (entries already moved out of staging
are removed again).

``{{year}}`` is the year of the rules revision's commit date (committer time zone), read with
``git`` from the rules source folder; never the clock. If the rules source is not a git checkout
that contains the revision, instantiation fails with ``TPL-YEAR`` unless ``--year YYYY`` is given
(which also overrides the git date). Output is therefore D0: the same inputs give the same bytes.

Exit codes: 0 created, 1 refused or failed (``TPL-*`` on stderr), 2 bad command line.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
ROOT = TOOLS.parent
DEFAULT_TEMPLATES = ROOT / "templates" / "repo"
STAGING = ".repo-template.partial"
DEFAULT_SOURCE_REPO = "metrologymaven"
PLACEHOLDER = re.compile(r"\{\{[A-Za-z_][A-Za-z0-9_]*\}\}")
SYNC_TIMEOUT_S = 60

if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from house_rules import (  # noqa: E402
    BlockError,
    RenderError,
    find_blocks,
    is_valid_repo_id,
    is_valid_rev,
    render,
)

try:  # Python 3.11+
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - CI runs 3.12
    tomllib = None


class TemplateError(Exception):
    """A refused or failed instantiation; ``code`` is one of the ``TPL-*`` codes."""

    def __init__(self, code: str, message: str):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


# ---------------------------------------------------------------------------------------------
# Inputs


def load_variants(templates: Path) -> dict[str, list[str]]:
    """``variants.toml`` as ``{variant id: [folder, ...]}``."""
    path = templates / "variants.toml"
    if tomllib is None:
        raise TemplateError("TPL-TEMPLATE", "reading variants.toml needs Python 3.11 or newer")
    try:
        with open(path, "rb") as f:
            data = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise TemplateError("TPL-TEMPLATE", f"{path}: {e}") from None
    variants: dict[str, list[str]] = {}
    for v in data.get("variant", []):
        vid, include = v.get("id"), v.get("include")
        if not isinstance(vid, str) or not isinstance(include, list) or not all(isinstance(i, str) for i in include):
            raise TemplateError("TPL-TEMPLATE", f"{path}: every [[variant]] needs id and include = [...]")
        for folder in include:
            if folder in ("", ".", "..") or "/" in folder or "\\" in folder:
                raise TemplateError("TPL-TEMPLATE", f"{path}: include entry {folder!r} is not a plain folder name")
        variants[vid] = include
    return variants


def rules_year(rules_source: Path, rev: str) -> str | None:
    """Year of ``rev``'s commit date from git in ``rules_source``, or None if unavailable."""
    try:
        r = subprocess.run(
            ["git", "-C", str(rules_source), "show", "-s", "--no-show-signature", "--date=format:%Y", "--format=%cd", rev, "--"],
            capture_output=True,
            text=True,
            timeout=SYNC_TIMEOUT_S,
            env={**os.environ, "GIT_TERMINAL_PROMPT": "0", "GIT_NO_LAZY_FETCH": "1"},
        )
    except (OSError, subprocess.SubprocessError):
        return None
    year = r.stdout.strip()
    if r.returncode != 0 or not re.fullmatch(r"[0-9]{4}", year):
        return None
    return year


# ---------------------------------------------------------------------------------------------
# Steps


def _template_files(templates: Path, include: list[str]) -> list[tuple[str, Path]]:
    """Every file of the included folders as (relative POSIX path, source), sorted per folder."""
    seen: dict[str, str] = {}
    files: list[tuple[str, Path]] = []
    for folder in include:
        base = templates / folder
        if not base.is_dir():
            raise TemplateError("TPL-TEMPLATE", f"template folder {folder!r} does not exist")
        for src in sorted(p for p in base.rglob("*") if p.is_file()):
            if src.is_symlink():
                raise TemplateError("TPL-TEMPLATE", f"{folder}/{src.relative_to(base).as_posix()} is a symbolic link")
            rel = src.relative_to(base).as_posix()
            if rel in seen:
                raise TemplateError("TPL-TEMPLATE", f"{rel} is in both {seen[rel]!r} and {folder!r}")
            seen[rel] = folder
            files.append((rel, src))
    return files


def _substitute(text: str, values: dict[str, str]) -> str:
    for name, value in values.items():
        token = "{{" + name + "}}"
        text = text.replace(token, value)
    return text


def copy_variant(templates: Path, include: list[str], dest: Path, values: dict[str, str]) -> None:
    for rel, src in _template_files(templates, include):
        try:
            text = src.read_bytes().decode("utf-8")
        except UnicodeDecodeError:
            raise TemplateError("TPL-TEMPLATE", f"{rel} is not UTF-8 text") from None
        out = dest / rel
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(_substitute(text, values).encode("utf-8"))


def _load_manifest(source: Path) -> dict:
    toml_path, json_path = source / "manifest.toml", source / "manifest.json"
    try:
        if tomllib is not None and toml_path.is_file():
            with open(toml_path, "rb") as f:
                return tomllib.load(f)
        with open(json_path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError) as e:
        raise TemplateError("TPL-SYNC-FAILED", f"SYNC-SOURCE: cannot read the manifest in {source}: {e}") from None


def builtin_sync(source: Path, rev: str, repo_id: str, target: Path) -> None:
    """Stand-in for ``python -m house_rules sync`` on a fresh tree (no blocks in AGENTS.md yet).

    Renders every block the manifest assigns to ``repo_id`` (manifest order) and inserts them
    after the first heading line of ``AGENTS.md``; writes ``CLAUDE.md`` as ``@AGENTS.md\\n``.
    """
    manifest = _load_manifest(source)
    source_repo = manifest.get("source_repo", DEFAULT_SOURCE_REPO)
    blocks = manifest.get("block", [])
    if not isinstance(blocks, list) or not all(isinstance(b, dict) for b in blocks):
        raise TemplateError("TPL-SYNC-FAILED", "SYNC-SOURCE: the manifest's block entries must be a list of tables")
    for b in blocks:
        if not isinstance(b.get("repos"), list) or not all(isinstance(r, str) for r in b["repos"]):
            raise TemplateError("TPL-SYNC-FAILED", f"SYNC-SOURCE: block {b.get('id')!r}: repos must be a list of repository IDs")
    assigned = [b for b in blocks if repo_id in b["repos"]]
    if not assigned:
        raise TemplateError("TPL-SYNC-FAILED", f"SYNC-REPO: repository {repo_id!r} has no blocks in the manifest")
    rendered = []
    for b in assigned:
        block_id, file = b.get("id"), b.get("file")
        if not isinstance(block_id, str) or not isinstance(file, str):
            raise TemplateError("TPL-SYNC-FAILED", "SYNC-SOURCE: a [[block]] lacks id or file")
        try:
            with open(source / file, encoding="utf-8", newline="") as f:
                body = f.read().replace("\r\n", "\n")
        except OSError as e:
            raise TemplateError("TPL-SYNC-FAILED", f"SYNC-SOURCE: block {block_id}: {e}") from None
        if body.endswith("\n"):
            body = body[:-1]
        try:
            rendered.append(render(block_id, source_repo, rev, body))
        except RenderError as e:
            raise TemplateError("TPL-SYNC-FAILED", f"SYNC-SOURCE: block {block_id}: {e}") from None

    agents = target / "AGENTS.md"
    try:
        with open(agents, encoding="utf-8", newline="") as f:
            text = f.read()
    except OSError as e:
        raise TemplateError("TPL-SYNC-FAILED", f"SYNC-TARGET: {e}") from None
    try:
        if find_blocks(text):
            raise TemplateError("TPL-SYNC-FAILED", "SYNC-TARGET: the template AGENTS.md already contains blocks")
    except BlockError as e:
        raise TemplateError("TPL-SYNC-FAILED", f"SYNC-TARGET: AGENTS.md: {e}") from None
    lines = text.split("\n")
    heading = next((i for i, line in enumerate(lines) if line.startswith("#")), None)
    if heading is None:
        raise TemplateError("TPL-SYNC-FAILED", "SYNC-TARGET: AGENTS.md has no heading line")
    head = "\n".join(lines[: heading + 1]) + "\n"
    rest = "\n".join(lines[heading + 1 :])
    agents.write_bytes((head + "\n" + "\n".join(rendered) + rest).encode("utf-8"))
    (target / "CLAUDE.md").write_bytes(b"@AGENTS.md\n")


def run_sync(source: Path, rev: str, repo_id: str, target: Path) -> None:
    # house_rules/__main__.py also serves `check` (ECO.RULES.CHECK); `sync` exists once sync.py does.
    if not (TOOLS / "house_rules" / "sync.py").is_file():
        builtin_sync(source, rev, repo_id, target)
        return
    env = dict(os.environ)
    env["PYTHONPATH"] = os.pathsep.join([str(TOOLS)] + ([env["PYTHONPATH"]] if env.get("PYTHONPATH") else []))
    cmd = [sys.executable, "-m", "house_rules", "sync", "--source", str(source), "--source-rev", rev,
           "--repo-id", repo_id, "--target", str(target)]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=SYNC_TIMEOUT_S, env=env)
    except (OSError, subprocess.SubprocessError) as e:
        raise TemplateError("TPL-SYNC-FAILED", str(e)) from None
    if r.returncode != 0:
        detail = (r.stderr.strip() or r.stdout.strip() or f"exit {r.returncode}").splitlines()[-1]
        raise TemplateError("TPL-SYNC-FAILED", f"house_rules sync exited {r.returncode}: {detail}")


def leftover_placeholders(root: Path) -> list[tuple[str, str]]:
    """Every (relative POSIX path, token) with ``{{name}}`` left in its name or contents, sorted."""
    found = []
    for p in sorted(root.rglob("*")):
        rel = p.relative_to(root).as_posix()
        tokens = set(PLACEHOLDER.findall(p.name))
        if p.is_file():
            tokens |= set(PLACEHOLDER.findall(p.read_bytes().decode("utf-8", errors="replace")))
        for token in sorted(tokens):
            found.append((rel, token))
    return found


# ---------------------------------------------------------------------------------------------
# Driver


def instantiate(repo_id: str, variant: str, rules_source: Path, rules_rev: str, ci_rev: str, out: Path,
                templates: Path = DEFAULT_TEMPLATES, year: str | None = None) -> list[str]:
    """Create the repository in ``out``; return the relative paths written. Raises TemplateError."""
    if not is_valid_repo_id(repo_id):
        raise TemplateError("TPL-ARGS", f"--id {repo_id!r} is not a valid repository ID")
    for flag, rev in (("--rules-rev", rules_rev), ("--ci-rev", ci_rev)):
        if not is_valid_rev(rev):
            raise TemplateError("TPL-ARGS", f"{flag} must be a full 40-character lowercase hex commit hash")
    if year is not None and not re.fullmatch(r"[0-9]{4}", year):
        raise TemplateError("TPL-ARGS", "--year must be four digits")
    if out.exists() or out.is_symlink():
        if not out.is_dir() or out.is_symlink() or any(out.iterdir()):
            raise TemplateError("TPL-OUT-NOT-EMPTY", f"{out} exists and is not an empty folder")
        created = False
    else:
        if not out.parent.is_dir():
            raise TemplateError("TPL-OUT", f"the parent folder of {out} does not exist")
        created = True
    variants = load_variants(templates)
    if variant not in variants:
        raise TemplateError("TPL-VARIANT", f"unknown variant {variant!r} (known: {', '.join(sorted(variants))})")

    if year is None:
        year = rules_year(rules_source, rules_rev)
    if year is None:
        raise TemplateError(
            "TPL-YEAR",
            f"no commit {rules_rev} found with git in {rules_source}, so the licence year is unknown; "
            "check out the rules source at that revision or pass --year YYYY",
        )
    values = {"repo_id": repo_id, "ci_rev": ci_rev, "year": year}

    if created:
        out.mkdir()
    staging = out / STAGING
    moved: list[str] = []
    try:
        staging.mkdir()
        copy_variant(templates, variants[variant], staging, values)
        run_sync(rules_source, rules_rev, repo_id, staging)
        leftovers = leftover_placeholders(staging)
        if leftovers:
            raise TemplateError("TPL-PLACEHOLDER", "; ".join(f"{f}: {t}" for f, t in leftovers))
        written = [p.relative_to(staging).as_posix() for p in sorted(staging.rglob("*")) if p.is_file()]
        for entry in sorted(staging.iterdir()):
            os.replace(entry, out / entry.name)
            moved.append(entry.name)
        staging.rmdir()
        return written
    except BaseException:
        if created:
            shutil.rmtree(out, ignore_errors=True)
        else:
            # Leave the pre-existing folder empty again: undo the moves, then drop staging.
            for name in moved:
                _remove(out / name)
            _remove(staging)
        raise


def _remove(path: Path) -> None:
    try:
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        else:
            path.unlink()
    except FileNotFoundError:
        pass
    except OSError:
        shutil.rmtree(path, ignore_errors=True)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="python -m repo_template", description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="command", required=True)
    new = sub.add_parser("new", help="create a repository from a template variant")
    new.add_argument("--id", required=True, help="repository ID of the new repository")
    new.add_argument("--variant", required=True, help="template variant (variants.toml), e.g. code or spec")
    new.add_argument("--rules-source", required=True, type=Path, help="house-rules source folder, checked out at --rules-rev")
    new.add_argument("--rules-rev", required=True, help="40-hex revision of the house-rules source")
    new.add_argument("--ci-rev", required=True, help="40-hex revision of the shared CI workflows")
    new.add_argument("--out", required=True, type=Path, help="new folder to create (must not exist, or be empty)")
    new.add_argument("--year", help="licence year (YYYY); needed when the rules source has no git commit for --rules-rev")
    new.add_argument("--templates", type=Path, default=DEFAULT_TEMPLATES, help=argparse.SUPPRESS)
    args = ap.parse_args(argv)
    try:
        written = instantiate(args.id, args.variant, args.rules_source, args.rules_rev, args.ci_rev, args.out,
                              args.templates, args.year)
    except TemplateError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    print(f"created {args.out} from variant {args.variant}: {len(written)} files")
    return 0
