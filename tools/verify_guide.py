#!/usr/bin/env python3
"""Guide-follow check for VERIFY.md (spec node ECO.REL.SIGN.GUIDE, AC-3). Stdlib only.

Extracts the fenced command blocks of VERIFY.md whose info string carries a tag
(`verify-windows`, `verify-linux`, `verify-macos`), and runs them verbatim against a copy of a
fixture release:

1. on an untouched copy: the commands must exit 0;
2. on a copy where one byte of one artefact listed in SHA256SUMS is changed: they must fail.

Shells: verify-windows runs in Windows PowerShell 5.1 (`powershell -File`, the PowerShell that
ships with Windows), verify-linux in `bash -e`, verify-macos in `zsh -e` (the macOS default
shell). Every block with the tag runs, in document order, as one script. The script is written
outside the release copy so it is never one of the files being checked. This tool neither
needs nor uses the network; CI runs it with the network cut where the OS allows.

Usage:
  python tools/verify_guide.py --tag verify-linux [--doc VERIFY.md] [--release DIR]
  python tools/verify_guide.py --tag verify-linux --print    (print the extracted script)
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TAGS = ("verify-windows", "verify-linux", "verify-macos")
FENCE = re.compile(r"^(?P<indent> {0,3})(?P<fence>`{3,}|~{3,})(?P<info>[^`]*)$")


class GuideError(Exception):
    pass


def extract(text, tag):
    """Returns the bodies of every fenced block whose info string has `tag` as a word."""
    blocks = []
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        m = FENCE.match(lines[i])
        if not m:
            i += 1
            continue
        fence = m.group("fence")
        words = m.group("info").split()
        body = []
        i += 1
        closed = False
        while i < len(lines):
            c = FENCE.match(lines[i])
            if c and c.group("fence")[0] == fence[0] and len(c.group("fence")) >= len(fence) and not c.group("info").strip():
                closed = True
                i += 1
                break
            body.append(lines[i])
            i += 1
        if not closed:
            raise GuideError(f"unclosed fenced block (info {' '.join(words)!r})")
        if tag in words:
            blocks.append("\n".join(body) + "\n")
    return blocks


def script_for(text, tag):
    blocks = extract(text, tag)
    if not blocks:
        raise GuideError(f"no fenced block tagged {tag}")
    return "".join(blocks)


def command(tag, script_path):
    if tag == "verify-windows":
        return ["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(script_path)]
    if tag == "verify-linux":
        return ["bash", "--noprofile", "--norc", "-e", str(script_path)]
    if tag == "verify-macos":
        return ["zsh", "-f", "-e", str(script_path)]
    raise GuideError(f"unknown tag {tag}")


def listed_names(sums_text):
    names = []
    for line in sums_text.splitlines():
        m = re.fullmatch(r"[0-9a-f]{64}  (\S+)", line)
        if not m:
            raise GuideError(f"bad SHA256SUMS line {line!r}")
        names.append(m.group(1))
    if not names:
        raise GuideError("SHA256SUMS lists no files")
    return names


def tamper(release_copy):
    """Flips the low bit of the last byte of the first artefact listed in SHA256SUMS."""
    name = listed_names((release_copy / "SHA256SUMS").read_text(encoding="ascii"))[0]
    p = release_copy / name
    data = bytearray(p.read_bytes())
    if not data:
        raise GuideError(f"{name} is empty")
    data[-1] ^= 0x01
    p.write_bytes(bytes(data))
    return name


def run(tag, script, release, work):
    """Runs `script` in a fresh copy of `release`; returns the exit code."""
    suffix = ".ps1" if tag == "verify-windows" else ".sh"
    script_path = work / ("guide" + suffix)
    script_path.write_text(script, encoding="utf-8", newline="\r\n" if suffix == ".ps1" else "\n")
    env = dict(os.environ)
    if tag == "verify-windows":
        # A PSModulePath inherited from PowerShell 7 (as on CI runners) stops Windows
        # PowerShell 5.1 from autoloading Get-FileHash.
        env.pop("PSModulePath", None)
    proc = subprocess.run(command(tag, script_path), cwd=release, env=env)
    return proc.returncode


def follow(doc, tag, release):
    script = script_for(doc.read_text(encoding="utf-8"), tag)
    with tempfile.TemporaryDirectory(prefix="verify-guide-") as tmp:
        work = Path(tmp)
        ok_copy = work / "ok"
        shutil.copytree(release, ok_copy)
        print(f"== {tag}: untouched fixture release", flush=True)
        code = run(tag, script, ok_copy, work)
        if code != 0:
            raise GuideError(f"{tag}: the guide's commands failed on the untouched release (exit {code})")
        bad_copy = work / "tampered"
        shutil.copytree(release, bad_copy)
        name = tamper(bad_copy)
        print(f"== {tag}: {name} with one byte changed (must fail)", flush=True)
        code = run(tag, script, bad_copy, work)
        if code == 0:
            raise GuideError(f"{tag}: the guide's commands accepted {name} with one byte changed")
    print(f"verify-guide: ok {tag}")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--tag", required=True, choices=TAGS)
    ap.add_argument("--doc", default=str(ROOT / "VERIFY.md"))
    ap.add_argument("--release", default=str(ROOT / "fixtures/rel/guide/release"))
    ap.add_argument("--print", action="store_true", help="print the extracted script and exit")
    a = ap.parse_args(argv)
    try:
        if a.print:
            sys.stdout.write(script_for(Path(a.doc).read_text(encoding="utf-8"), a.tag))
            return 0
        follow(Path(a.doc), a.tag, Path(a.release))
    except (GuideError, OSError) as e:
        print(f"verify-guide: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
