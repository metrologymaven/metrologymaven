"""Library promotion and consumer repin tools (spec node ECO.REPO.TPL.PROMOTE).

Standard library only; invokes ``git`` with a hardened non-interactive environment.

Subcommands (invoked via ``python -m repo_template <cmd>``):

- ``promote``: extract every commit touching ``--path`` with original authorship, dates and
  messages into a fresh local repository with zero remotes, verify the library tree digest
  matches the source, and apply the ``code`` repository template as exactly one extra commit
  without modifying any file of the library.
- ``repin``: rewrite path dependencies on ``--crate`` across a workspace to git dependencies
  pinned to ``--url`` and 40-hex ``--rev``, preserving formatting of untouched lines.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
ROOT = TOOLS.parent

if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from house_rules import is_valid_repo_id, is_valid_rev  # noqa: E402
from house_rules.check import git_env  # noqa: E402
from repo_template.new import DEFAULT_TEMPLATES, TemplateError, instantiate  # noqa: E402

try:  # Python 3.11+
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None

__all__ = [
    "PromoteError",
    "compute_git_tree_digest",
    "promote",
    "repin",
    "add_subparsers",
    "run_promote_args",
    "run_repin_args",
]

GIT_TIMEOUT_S = 120
_CRATE_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_-]*$")
_YEAR_RE = re.compile(r"^([0-9]{4})")
_ZERO_REV = "0" * 40


class PromoteError(Exception):
    """Typed promotion/repin error whose string begins with an error code."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


def _git_cmd(repo: Path, args: list[str], *, allow_protocol_none: bool = True) -> list[str]:
    base = ["git", "-C", str(repo), "-c", "core.fsmonitor=false", "-c", f"core.hooksPath={os.devnull}"]
    if allow_protocol_none:
        base += ["-c", "protocol.allow=never"]
    return base + args


def _run_git(
    repo: Path,
    args: list[str],
    *,
    input_bytes: bytes | None = None,
    extra_env: dict[str, str] | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[bytes]:
    env = git_env()
    if extra_env:
        env.update(extra_env)
    try:
        r = subprocess.run(
            _git_cmd(repo, args),
            input=input_bytes,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=GIT_TIMEOUT_S,
            env=env,
        )
    except (OSError, subprocess.SubprocessError) as e:
        raise PromoteError("PROMOTE-GIT", f"git {' '.join(args)} failed: {e}") from None
    if check and r.returncode != 0:
        err = r.stderr.decode("utf-8", errors="replace").strip()
        raise PromoteError("PROMOTE-GIT", f"git {' '.join(args)} exited {r.returncode}: {err}")
    return r


def _parse_author(author: str) -> tuple[str, str]:
    """Parse ``Name <email>`` or ``Name email`` into (name, email)."""
    s = author.strip()
    m = re.fullmatch(r"(.+?)\s*<([^<>\s]+)>", s)
    if m:
        return m.group(1).strip(), m.group(2).strip()
    parts = s.rsplit(None, 1)
    if len(parts) == 2 and "@" in parts[1]:
        return parts[0].strip(), parts[1].strip()
    raise PromoteError("PROMOTE-ARGS", f"--author must be 'Name <email>' or 'Name email', got {author!r}")


def _normalize_rel_path(path_str: str) -> str:
    p = path_str.replace("\\", "/").strip("/")
    if not p or p.startswith("/") or ".." in p.split("/"):
        raise PromoteError("PROMOTE-ARGS", f"--path must be a clean relative directory path, got {path_str!r}")
    return p


def compute_git_tree_digest(repo: Path, tree_spec: str) -> str:
    """Canonical SHA-256 over sorted (POSIX path, git mode, blob bytes) of a git tree."""
    r = _run_git(repo, ["ls-tree", "-r", "-z", tree_spec])
    entries: list[tuple[str, str, str]] = []
    for rec in r.stdout.split(b"\0"):
        if not rec:
            continue
        meta_b, path_b = rec.split(b"\t", 1)
        mode, obj_type, oid = meta_b.decode("ascii").split(" ", 2)
        if obj_type != "blob":
            continue
        entries.append((path_b.decode("utf-8"), mode, oid))
    entries.sort(key=lambda t: t[0])
    h = hashlib.sha256()
    for rel_path, mode, oid in entries:
        blob = _run_git(repo, ["cat-file", "blob", oid]).stdout
        h.update(f"{mode} {rel_path}\0{len(blob)}\0".encode("utf-8"))
        h.update(blob)
    return h.hexdigest()


def _prepare_rules_source(
    rules_source: Path | None,
    repo_id: str,
    tmp_dir: Path,
) -> Path:
    """Return a rules source directory whose manifest assigns at least core/rust to repo_id."""
    if rules_source is not None:
        return rules_source
    default_src = ROOT / "house-rules"
    if not default_src.is_dir():
        raise PromoteError("PROMOTE-ARGS", f"default rules source {default_src} not found; pass --rules-source")
    # If repo_id is already in the default manifest, use it directly.
    toml_path = default_src / "manifest.toml"
    json_path = default_src / "manifest.json"
    if tomllib is not None and toml_path.is_file():
        with open(toml_path, "rb") as f:
            manifest = tomllib.load(f)
    else:
        manifest = json.loads(json_path.read_text(encoding="utf-8"))
    blocks = manifest.get("block", [])
    if any(repo_id in b.get("repos", []) for b in blocks):
        return default_src
    # Copy default rules into tmp_dir and add repo_id to core and rust blocks so instantiate works cleanly.
    staged_rules = tmp_dir / "house-rules"
    shutil.copytree(default_src, staged_rules)
    for b in blocks:
        if b.get("id") in ("core", "rust") and repo_id not in b.get("repos", []):
            b["repos"].append(repo_id)
    (staged_rules / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
    # Remove manifest.toml in staged copy so instantiate reads our updated manifest.json (also works on Python < 3.11).
    staged_toml = staged_rules / "manifest.toml"
    if staged_toml.exists():
        staged_toml.unlink()
    return staged_rules


def _clear_worktree_except_git(repo: Path) -> None:
    for entry in list(repo.iterdir()):
        if entry.name == ".git":
            continue
        if entry.is_dir() and not entry.is_symlink():
            shutil.rmtree(entry)
        else:
            entry.unlink()


def _write_tree_into_worktree(source: Path, tree_spec: str, dest: Path) -> None:
    """Materialize every blob of ``tree_spec`` from ``source`` into ``dest``."""
    _clear_worktree_except_git(dest)
    r = _run_git(source, ["ls-tree", "-r", "-z", tree_spec])
    for rec in r.stdout.split(b"\0"):
        if not rec:
            continue
        meta_b, path_b = rec.split(b"\t", 1)
        mode, obj_type, oid = meta_b.decode("ascii").split(" ", 2)
        if obj_type != "blob":
            continue
        rel = path_b.decode("utf-8")
        out_path = dest / rel
        out_path.parent.mkdir(parents=True, exist_ok=True)
        blob = _run_git(source, ["cat-file", "blob", oid]).stdout
        out_path.write_bytes(blob)
        if mode == "100755":
            os.chmod(out_path, 0o755)


def promote(
    source: Path,
    rel_path: str,
    repo_id: str,
    author: str,
    date: str,
    out: Path,
    *,
    rules_source: Path | None = None,
    rules_rev: str | None = None,
    ci_rev: str | None = None,
    year: str | None = None,
    templates: Path = DEFAULT_TEMPLATES,
) -> dict[str, object]:
    """Extract history for ``rel_path`` from ``source`` into ``out`` and add one template commit."""
    if not is_valid_repo_id(repo_id):
        raise PromoteError("PROMOTE-ARGS", f"--id {repo_id!r} is not a valid repository ID")
    author_name, author_email = _parse_author(author)
    norm_path = _normalize_rel_path(rel_path)

    m_year = _YEAR_RE.match(date.strip())
    if not m_year:
        raise PromoteError("PROMOTE-ARGS", f"--date must start with YYYY, got {date!r}")
    eff_year = year or m_year.group(1)

    if not source.is_dir():
        raise PromoteError("PROMOTE-ARGS", f"--source {source} is not a directory")
    if out.exists() or out.is_symlink():
        if not out.is_dir() or out.is_symlink() or any(out.iterdir()):
            raise PromoteError("PROMOTE-OUT", f"--out {out} exists and is not empty")
        created_out = False
    else:
        if not out.parent.is_dir():
            raise PromoteError("PROMOTE-OUT", f"parent directory of {out} does not exist")
        created_out = True

    # REQ-004: refuse dirty source working tree.
    status = _run_git(source, ["status", "--porcelain"]).stdout.decode("utf-8", errors="replace").strip()
    if status:
        raise PromoteError("PROMOTE-DIRTY", f"source repository {source} has uncommitted changes")

    # Verify target path exists at HEAD and capture its tree OID + canonical digest.
    check_tree = _run_git(source, ["rev-parse", "--verify", f"HEAD:{norm_path}"], check=False)
    if check_tree.returncode != 0:
        raise PromoteError("PROMOTE-PATH-NOT-FOUND", f"path {norm_path!r} does not exist at HEAD of {source}")
    source_tree_oid = check_tree.stdout.decode("ascii").strip()
    source_digest = compute_git_tree_digest(source, f"HEAD:{norm_path}")

    # Resolve revisions for template instantiation.
    if rules_rev is None:
        head_rev = _run_git(source, ["rev-parse", "HEAD"]).stdout.decode("ascii").strip()
        rules_rev = head_rev if is_valid_rev(head_rev) else _ZERO_REV
    if ci_rev is None:
        ci_rev = _ZERO_REV
    if not is_valid_rev(rules_rev) or not is_valid_rev(ci_rev):
        raise PromoteError("PROMOTE-ARGS", "--rules-rev and --ci-rev must be 40 lowercase hex digits")

    # Collect every commit touching norm_path in chronological order.
    rev_out = _run_git(source, ["rev-list", "--reverse", "HEAD", "--", norm_path]).stdout.decode("ascii").strip()
    commits = [line.strip() for line in rev_out.splitlines() if line.strip()]
    if not commits:
        raise PromoteError("PROMOTE-PATH-NOT-FOUND", f"no commits touch {norm_path!r} in {source}")

    if created_out:
        out.mkdir()

    try:
        # Initialize empty git repo with branch 'main' and zero remotes (REQ-005).
        _run_git(out, ["init", "-b", "main"])
        _run_git(out, ["config", "user.name", author_name])
        _run_git(out, ["config", "user.email", author_email])

        # Replay each commit touching norm_path, preserving author, committer, dates and message.
        for cid in commits:
            fmt = "%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI%x00%B"
            meta = _run_git(source, ["show", "-s", "--no-show-signature", f"--format={fmt}", cid]).stdout
            parts = meta.split(b"\0", 6)
            an, ae, ad, cn, ce, cd = [p.decode("utf-8") for p in parts[:6]]
            msg = parts[6]

            _write_tree_into_worktree(source, f"{cid}:{norm_path}", out)
            _run_git(out, ["add", "-A"])
            commit_env = {
                "GIT_AUTHOR_NAME": an,
                "GIT_AUTHOR_EMAIL": ae,
                "GIT_AUTHOR_DATE": ad,
                "GIT_COMMITTER_NAME": cn,
                "GIT_COMMITTER_EMAIL": ce,
                "GIT_COMMITTER_DATE": cd,
            }
            _run_git(out, ["commit", "--allow-empty", "-F", "-"], input_bytes=msg, extra_env=commit_env)

        # REQ-001: verify tree digest and git tree OID at last extracted commit match source.
        extracted_tree_oid = _run_git(out, ["rev-parse", "HEAD^{tree}"]).stdout.decode("ascii").strip()
        extracted_digest = compute_git_tree_digest(out, "HEAD^{tree}")
        if extracted_tree_oid != source_tree_oid or extracted_digest != source_digest:
            raise PromoteError(
                "PROMOTE-DIGEST-MISMATCH",
                f"extracted tree ({extracted_tree_oid}, {extracted_digest}) != source ({source_tree_oid}, {source_digest})",
            )

        # REQ-002: apply repository template ('code' variant) as exactly one commit without touching library files.
        with tempfile.TemporaryDirectory(prefix="promote-tpl-") as td:
            td_path = Path(td)
            eff_rules = _prepare_rules_source(rules_source, repo_id, td_path)
            tpl_out = td_path / "tpl"
            try:
                instantiate(
                    repo_id=repo_id,
                    variant="code",
                    rules_source=eff_rules,
                    rules_rev=rules_rev,
                    ci_rev=ci_rev,
                    out=tpl_out,
                    templates=templates,
                    year=eff_year,
                )
            except TemplateError as e:
                raise PromoteError("PROMOTE-TEMPLATE", str(e)) from None

            added_files: list[str] = []
            for src_file in sorted(p for p in tpl_out.rglob("*") if p.is_file()):
                rel = src_file.relative_to(tpl_out)
                dst_file = out / rel
                if dst_file.exists() or dst_file.is_symlink():
                    # Never overwrite any file of the extracted library (REQ-002).
                    continue
                dst_file.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src_file, dst_file)
                added_files.append(rel.as_posix())

        _run_git(out, ["add", "-A"])
        tpl_env = {
            "GIT_AUTHOR_NAME": author_name,
            "GIT_AUTHOR_EMAIL": author_email,
            "GIT_AUTHOR_DATE": date,
            "GIT_COMMITTER_NAME": author_name,
            "GIT_COMMITTER_EMAIL": author_email,
            "GIT_COMMITTER_DATE": date,
        }
        tpl_msg = f"chore({repo_id}): apply code repository template\n".encode("utf-8")
        _run_git(out, ["commit", "--allow-empty", "-F", "-"], input_bytes=tpl_msg, extra_env=tpl_env)

        head_rev = _run_git(out, ["rev-parse", "HEAD"]).stdout.decode("ascii").strip()
        count = int(_run_git(out, ["rev-list", "--count", "HEAD"]).stdout.decode("ascii").strip())
        remotes = _run_git(out, ["remote"]).stdout.decode("utf-8").strip()
        if remotes:
            raise PromoteError("PROMOTE-GIT", f"output repository unexpectedly has remotes: {remotes}")

        return {
            "extracted_commits": len(commits),
            "total_commits": count,
            "head": head_rev,
            "tree_digest": extracted_digest,
            "template_files_added": len(added_files),
        }
    except BaseException:
        if created_out:
            shutil.rmtree(out, ignore_errors=True)
        raise


# ---------------------------------------------------------------------------------------------
# Repin (bounded line-preserving Cargo.toml editor)

_SECTION_RE = re.compile(r"^\s*\[([^\]]+)\]\s*(?:#.*)?$")
_DEP_SECTIONS = {
    "dependencies",
    "dev-dependencies",
    "build-dependencies",
    "dev_dependencies",
    "build_dependencies",
    "workspace.dependencies",
}


def _is_dep_section(header: str) -> bool:
    h = header.strip()
    if h in _DEP_SECTIONS:
        return True
    # target.<cfg>.(dev-|build-)?dependencies
    if h.startswith("target."):
        last = h.rsplit(".", 1)[-1]
        if last in _DEP_SECTIONS:
            return True
    return False


def _is_crate_subtable(header: str, crate: str) -> bool:
    h = header.strip()
    for base in _DEP_SECTIONS:
        if h == f"{base}.{crate}":
            return True
    if h.startswith("target."):
        for base in ("dependencies", "dev-dependencies", "build-dependencies", "dev_dependencies", "build_dependencies"):
            if h.endswith(f".{base}.{crate}"):
                return True
    return False


def _rewrite_inline_dep_line(line: str, crate: str, url: str, rev: str) -> str | None:
    """If ``line`` defines a path dependency on ``crate`` via inline table, rewrite path -> git+rev."""
    # Match `<key> = { ... }` preserving indentation, comments, and line ending.
    eol = "\r\n" if line.endswith("\r\n") else ("\n" if line.endswith("\n") else "")
    body = line[: len(line) - len(eol)] if eol else line

    m = re.match(r"^(\s*)([A-Za-z0-9_-]+)(\s*=\s*\{)(.*?)(\}\s*(?:#.*)?)$", body)
    if not m:
        return None
    indent, key, eq_open, inner, close_tail = m.groups()
    pkg_match = re.search(r'\bpackage\s*=\s*"([^"]+)"', inner)
    target_crate = pkg_match.group(1) if pkg_match else key
    if target_crate != crate:
        return None
    path_pat = re.compile(r'\bpath\s*=\s*"[^"]*"')
    if not path_pat.search(inner):
        return None
    # Replace only the `path = "..."` assignment with `git = "...", rev = "..."`, leaving all other keys untouched.
    replacement = f'git = "{url}", rev = "{rev}"'
    new_inner = path_pat.sub(replacement, inner, count=1)
    return f"{indent}{key}{eq_open}{new_inner}{close_tail}{eol}"


def _rewrite_subtable_path_line(line: str, url: str, rev: str) -> str | None:
    eol = "\r\n" if line.endswith("\r\n") else ("\n" if line.endswith("\n") else "")
    body = line[: len(line) - len(eol)] if eol else line
    m = re.match(r'^(\s*)path\s*=\s*"[^"]*"(\s*(?:#.*)?)$', body)
    if not m:
        return None
    indent, tail = m.groups()
    return f'{indent}git = "{url}"{eol}{indent}rev = "{rev}"{tail}{eol}'


def _rewrite_manifest(text: str, crate: str, url: str, rev: str) -> tuple[str, bool]:
    lines = text.splitlines(keepends=True)
    in_dep = False
    in_crate_sub = False
    changed = False
    out: list[str] = []

    for line in lines:
        sm = _SECTION_RE.match(line)
        if sm:
            hdr = sm.group(1).strip()
            in_dep = _is_dep_section(hdr)
            in_crate_sub = _is_crate_subtable(hdr, crate)
            out.append(line)
            continue

        if in_dep:
            rewritten = _rewrite_inline_dep_line(line, crate, url, rev)
            if rewritten is not None:
                out.append(rewritten)
                changed = True
                continue
        elif in_crate_sub:
            rewritten = _rewrite_subtable_path_line(line, url, rev)
            if rewritten is not None:
                out.append(rewritten)
                changed = True
                continue
        out.append(line)

    return "".join(out), changed


def repin(workspace: Path, crate: str, url: str, rev: str) -> list[str]:
    """Rewrite every path dependency on ``crate`` in ``workspace`` to a git dep pinned at ``rev``."""
    if not workspace.is_dir():
        raise PromoteError("REPIN-ARGS", f"--workspace {workspace} is not a directory")
    if not _CRATE_RE.fullmatch(crate):
        raise PromoteError("REPIN-ARGS", f"--crate {crate!r} is not a valid crate name")
    if not url or any(ch.isspace() or ch == '"' for ch in url):
        raise PromoteError("REPIN-ARGS", f"--url {url!r} is not a valid git URL")
    if not is_valid_rev(rev):
        raise PromoteError("REPIN-ARGS", "--rev must be a 40-character lowercase hex commit hash")

    rewritten_files: list[str] = []
    for manifest in sorted(workspace.rglob("Cargo.toml"), key=lambda p: p.relative_to(workspace).as_posix()):
        rel_parts = manifest.relative_to(workspace).parts
        if any(part in (".git", "target") for part in rel_parts):
            continue
        if manifest.is_symlink():
            continue
        try:
            old_text = manifest.read_bytes().decode("utf-8")
        except (OSError, UnicodeDecodeError) as e:
            raise PromoteError("REPIN-IO", f"cannot read {manifest}: {e}") from None
        new_text, changed = _rewrite_manifest(old_text, crate, url, rev)
        if changed:
            manifest.write_bytes(new_text.encode("utf-8"))
            rewritten_files.append(manifest.relative_to(workspace).as_posix())

    if not rewritten_files:
        raise PromoteError("REPIN-NO-DEPENDENTS", f"no path dependency on {crate!r} found in {workspace}")
    return rewritten_files


# ---------------------------------------------------------------------------------------------
# CLI wiring


def add_subparsers(sub: argparse._SubParsersAction) -> None:  # type: ignore[type-arg]
    p = sub.add_parser("promote", help="extract a crate's history into a new repository with template")
    p.add_argument("--source", required=True, type=Path, help="source monorepo directory (must be clean)")
    p.add_argument("--path", required=True, help="relative crate directory path, e.g. crates/fid-b")
    p.add_argument("--id", required=True, help="repository ID for the new repository")
    p.add_argument("--author", required=True, help="template commit author ('Name <email>' or 'Name email')")
    p.add_argument("--date", required=True, help="template commit ISO-8601 date, e.g. 2026-11-22T00:00:00Z")
    p.add_argument("--out", required=True, type=Path, help="output directory for the new repository")
    p.add_argument("--rules-source", type=Path, default=None, help="optional house-rules source folder")
    p.add_argument("--rules-rev", default=None, help="optional 40-hex house-rules revision")
    p.add_argument("--ci-rev", default=None, help="optional 40-hex shared CI revision")
    p.add_argument("--year", default=None, help="optional 4-digit licence year (defaults to year of --date)")
    p.add_argument("--templates", type=Path, default=DEFAULT_TEMPLATES, help=argparse.SUPPRESS)

    r = sub.add_parser("repin", help="rewrite workspace path dependencies on a promoted crate to pinned git deps")
    r.add_argument("--workspace", required=True, type=Path, help="monorepo workspace directory")
    r.add_argument("--crate", required=True, help="promoted crate name")
    r.add_argument("--url", required=True, help="new repository git URL")
    r.add_argument("--rev", required=True, help="40-hex commit hash to pin")


def run_promote_args(args: argparse.Namespace) -> int:
    try:
        info = promote(
            source=args.source,
            rel_path=args.path,
            repo_id=args.id,
            author=args.author,
            date=args.date,
            out=args.out,
            rules_source=args.rules_source,
            rules_rev=args.rules_rev,
            ci_rev=args.ci_rev,
            year=args.year,
            templates=args.templates,
        )
    except PromoteError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    print(
        f"promoted {args.path} -> {args.out} ({info['extracted_commits']} extracted + 1 template = "
        f"{info['total_commits']} commits, head {info['head']}, tree_digest {info['tree_digest']})"
    )
    return 0


def run_repin_args(args: argparse.Namespace) -> int:
    try:
        files = repin(args.workspace, args.crate, args.url, args.rev)
    except PromoteError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    print(f"repinned {args.crate} -> {args.url}@{args.rev} in {len(files)} manifest(s): {', '.join(files)}")
    return 0
