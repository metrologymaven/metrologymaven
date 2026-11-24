"""Environment normalization, clean child env construction, and host capability probes."""

from __future__ import annotations

import os
import shutil
import socket
import subprocess
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Mapping, Sequence

WIN_RUNTIME_VARS: tuple[str, ...] = (
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "LOCALAPPDATA",
    "APPDATA",
    "PROGRAMDATA",
)

COMMON_RUNTIME_VARS: tuple[str, ...] = (
    "PATH",
    "HOME",
    "TMPDIR",
    "TEMP",
    "TMP",
    "LANG",
    "LC_ALL",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "CARGO_TARGET_DIR",
    "CI",
)


@dataclass(frozen=True)
class Capabilities:
    platform: str
    is_linux: bool
    is_windows: bool
    is_macos: bool
    is_ci: bool
    python_exe: str
    python_version: str
    cargo_path: str | None
    rustc_version: str | None
    node_path: str | None
    node_version: str | None
    bash_path: str | None
    wasm32_wasip1: bool
    c_toolchain_available: bool
    c_toolchain_probe: str
    c_toolchain_detail: str
    loopback_tcp_allowed: bool
    loopback_tcp_detail: str

    def to_dict(self) -> dict[str, object]:
        return asdict(self)


def normalize_path(raw_path: str | None = None) -> str:
    """Ensure ~/.cargo/bin and (on Windows) Git bin are present on PATH when needed."""
    sep = os.pathsep
    current = raw_path if raw_path is not None else os.environ.get("PATH", "")
    parts = [p for p in current.split(sep) if p]

    def _has_exe(name: str) -> bool:
        return shutil.which(name, path=sep.join(parts)) is not None

    cargo_bin = Path.home() / ".cargo" / "bin"
    if not _has_exe("cargo") and cargo_bin.is_dir():
        c_str = str(cargo_bin)
        if c_str not in parts:
            parts.insert(0, c_str)

    if sys.platform == "win32":
        py_dir = str(Path(sys.executable).resolve().parent)
        if py_dir not in parts:
            parts.insert(0, py_dir)
        git_bin = Path(r"C:\Program Files\Git\bin")
        if not _has_exe("bash") and git_bin.is_dir():
            g_str = str(git_bin)
            if g_str not in parts:
                parts.insert(0, g_str)

    return sep.join(parts)


def normalized_env(base: Mapping[str, str] | None = None) -> dict[str, str]:
    """Return a copy of `base` (or os.environ) with normalized PATH."""
    env = dict(os.environ if base is None else base)
    env["PATH"] = normalize_path(env.get("PATH", ""))
    return env


def clean_env(
    extra: Mapping[str, str] | None = None,
    *,
    strip: Sequence[str] = (),
    base: Mapping[str, str] | None = None,
) -> dict[str, str]:
    """Construct a clean child environment preserving OS/Windows runtime variables."""
    src = normalized_env(base)
    out: dict[str, str] = {}

    keep_keys = list(COMMON_RUNTIME_VARS)
    if sys.platform == "win32":
        keep_keys.extend(WIN_RUNTIME_VARS)

    if sys.platform == "win32":
        upper_map = {k.upper(): v for k, v in src.items()}
        for k in keep_keys:
            if k.upper() in upper_map and k not in out:
                out[k] = upper_map[k.upper()]
    else:
        for k in keep_keys:
            if k in src:
                out[k] = src[k]

    for k in strip:
        if sys.platform == "win32":
            for existing in list(out):
                if existing.upper() == k.upper():
                    del out[existing]
        else:
            out.pop(k, None)

    if extra:
        for k, v in extra.items():
            if sys.platform == "win32":
                for existing in list(out):
                    if existing.upper() == k.upper():
                        del out[existing]
            out[k] = v
    return out


def _run_probe(cmd: Sequence[str], *, cwd: Path | None = None, timeout: int = 30) -> tuple[int, str, str]:
    try:
        proc = subprocess.run(
            list(cmd),
            cwd=cwd,
            env=normalized_env(),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
        )
        return proc.returncode, proc.stdout.strip(), proc.stderr.strip()
    except (OSError, subprocess.TimeoutExpired) as exc:
        return -1, "", str(exc)


def probe_loopback_tcp() -> tuple[bool, str]:
    """Diagnostic probe for whether 127.0.0.1 bind/connect is permitted.

    NOTE: Never used to skip loopback tests under `--strict`!
    """
    srv = None
    cli = None
    conn = None
    try:
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.settimeout(2.0)
        srv.bind(("127.0.0.1", 0))
        srv.listen(1)
        addr = srv.getsockname()
        cli = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        cli.settimeout(2.0)
        cli.connect(addr)
        conn, _ = srv.accept()
        return True, f"bind/connect succeeded on {addr[0]}:{addr[1]}"
    except OSError as exc:
        return False, f"{type(exc).__name__}: {exc}"
    finally:
        for s in (conn, cli, srv):
            if s is not None:
                try:
                    s.close()
                except OSError:
                    pass


def probe_c_toolchain(repo_root: Path, cargo_path: str | None) -> tuple[bool, str]:
    """Probe C/SQLite compilation capability via `cargo check --locked -p fid-store`."""
    if not cargo_path:
        return False, "cargo executable not found"
    rc, out, err = _run_probe(
        [cargo_path, "check", "--locked", "-p", "fid-store"],
        cwd=repo_root,
        timeout=300,
    )
    if rc == 0:
        return True, "cargo check --locked -p fid-store succeeded"
    combined = (err or out or "unknown failure").splitlines()
    tail = "\n".join(combined[-8:])
    return False, f"cargo check --locked -p fid-store failed (exit {rc}): {tail}"


def probe_capabilities(repo_root: Path) -> Capabilities:
    """Probe host OS, toolchain binaries, C/SQLite capability, and diagnostic loopback TCP."""
    norm_path = normalize_path()
    cargo_path = shutil.which("cargo", path=norm_path)
    rustc_path = shutil.which("rustc", path=norm_path)
    rustup_path = shutil.which("rustup", path=norm_path)
    node_path = shutil.which("node", path=norm_path)
    bash_path = shutil.which("bash", path=norm_path)

    rustc_version = None
    if rustc_path:
        rc, out, _ = _run_probe([rustc_path, "-vV"], cwd=repo_root, timeout=15)
        if rc == 0:
            rustc_version = " | ".join(line.strip() for line in out.splitlines()[:3])

    node_version = None
    if node_path:
        rc, out, _ = _run_probe([node_path, "--version"], cwd=repo_root, timeout=15)
        if rc == 0:
            node_version = out

    wasm32_wasip1 = False
    if rustup_path:
        rc, out, _ = _run_probe([rustup_path, "target", "list", "--installed"], cwd=repo_root, timeout=15)
        if rc == 0:
            wasm32_wasip1 = "wasm32-wasip1" in out.splitlines()

    c_ok, c_detail = probe_c_toolchain(repo_root, cargo_path)
    loop_ok, loop_detail = probe_loopback_tcp()

    return Capabilities(
        platform=sys.platform,
        is_linux=sys.platform.startswith("linux"),
        is_windows=sys.platform == "win32",
        is_macos=sys.platform == "darwin",
        is_ci=os.environ.get("CI", "").strip().lower() in ("1", "true", "yes"),
        python_exe=sys.executable,
        python_version=sys.version.split()[0],
        cargo_path=cargo_path,
        rustc_version=rustc_version,
        node_path=node_path,
        node_version=node_version,
        bash_path=bash_path,
        wasm32_wasip1=wasm32_wasip1,
        c_toolchain_available=c_ok,
        c_toolchain_probe="cargo check --locked -p fid-store",
        c_toolchain_detail=c_detail,
        loopback_tcp_allowed=loop_ok,
        loopback_tcp_detail=loop_detail,
    )
