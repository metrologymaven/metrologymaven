"""Individual SUT binary builder and lookup via `cargo build --locked --bin <name>`."""

from __future__ import annotations

import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

from e2e.lib.env import Capabilities, normalized_env

# Structured build outcomes
BUILT = "BUILT"
MISSING_CRATE_OR_BIN = "MISSING_CRATE_OR_BIN"
HOST_C_TOOLCHAIN_BLOCKED = "HOST_C_TOOLCHAIN_BLOCKED"
BUILD_ERROR = "BUILD_ERROR"

# Crates known to depend on C/SQLite compilation (cc / libsqlite3-sys).
C_BLOCKED_CRATES = frozenset({"fid-xtask", "fid-val", "fid-crash"})

# Ordered list of all SUT binaries probed/built by the E2E suite.
SUT_BINARIES: tuple[str, ...] = (
    "fid",
    "fid-slice",
    "fid-mcp",
    "fid-pkg-conformance",
    "fid-outreach",
    "fid-accept",
    "fid-eval-harness",
    "fid-verify",
    "fid-guide-wasm",
    "fid-xtask",
    "fid-val",
    "fid-crash",
)

_C_TOOLCHAIN_ERROR_HINTS = (
    "libsqlite3-sys",
    "failed to run custom build command for `libsqlite3-sys",
    "cannot execute 'cc1'",
    "kernel32.lib",
    "error occurred in cc-rs",
    "c compiler",
    "msvc",
    "sqlite3.c",
    "error calling dlltool",
    "dlltool.exe",
)


@dataclass(frozen=True)
class BuildResult:
    bin_name: str
    crate_name: str | None
    status: str
    path: Path | None
    detail: str


class BinaryManager:
    """Discovers workspace binary targets and builds requested binaries individually."""

    def __init__(self, repo_root: Path, caps: Capabilities) -> None:
        self.repo_root = repo_root
        self.caps = caps
        self._target_dir: Path = repo_root / "target"
        self._bin_to_pkg: dict[str, str] = {}
        self._pkg_names: set[str] = set()
        self._metadata_error: str | None = None
        self._cache: dict[str, BuildResult] = {}
        self._load_metadata()

    def _load_metadata(self) -> None:
        if not self.caps.cargo_path:
            self._metadata_error = "cargo executable not found"
            return
        try:
            proc = subprocess.run(
                [self.caps.cargo_path, "metadata", "--locked", "--no-deps", "--format-version", "1"],
                cwd=self.repo_root,
                env=normalized_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=120,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            self._metadata_error = f"cargo metadata failed: {exc}"
            return
        if proc.returncode != 0:
            self._metadata_error = f"cargo metadata --locked failed (exit {proc.returncode}): {proc.stderr.strip()}"
            return
        meta = json.loads(proc.stdout)
        if meta.get("target_directory"):
            self._target_dir = Path(meta["target_directory"])
        ws_ids = set(meta.get("workspace_members", []))
        for pkg in meta.get("packages", []):
            if pkg.get("id") not in ws_ids:
                continue
            pname = pkg["name"]
            self._pkg_names.add(pname)
            for tgt in pkg.get("targets", []):
                if "bin" in tgt.get("kind", []):
                    self._bin_to_pkg[tgt["name"]] = pname

    @property
    def target_dir(self) -> Path:
        return self._target_dir

    def has_crate(self, crate_name: str) -> bool:
        return crate_name in self._pkg_names

    def exe_name(self, bin_name: str) -> str:
        return f"{bin_name}.exe" if sys.platform == "win32" else bin_name

    def build_bin(self, bin_name: str) -> BuildResult:
        if bin_name in self._cache:
            return self._cache[bin_name]

        if self._metadata_error:
            res = BuildResult(bin_name, None, BUILD_ERROR, None, self._metadata_error)
            self._cache[bin_name] = res
            return res

        crate_name = self._bin_to_pkg.get(bin_name)
        if crate_name is None:
            res = BuildResult(
                bin_name,
                None,
                MISSING_CRATE_OR_BIN,
                None,
                f"binary target `{bin_name}` is not present in workspace cargo metadata",
            )
            self._cache[bin_name] = res
            return res

        if crate_name in C_BLOCKED_CRATES and not self.caps.c_toolchain_available:
            res = BuildResult(
                bin_name,
                crate_name,
                HOST_C_TOOLCHAIN_BLOCKED,
                None,
                f"crate `{crate_name}` requires C/SQLite compilation and host C probe failed: {self.caps.c_toolchain_detail}",
            )
            self._cache[bin_name] = res
            return res

        assert self.caps.cargo_path is not None
        try:
            proc = subprocess.run(
                [self.caps.cargo_path, "build", "--locked", "--bin", bin_name],
                cwd=self.repo_root,
                env=normalized_env(),
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=600,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            res = BuildResult(bin_name, crate_name, BUILD_ERROR, None, f"cargo build failed: {exc}")
            self._cache[bin_name] = res
            return res

        if proc.returncode != 0:
            err_lower = (proc.stderr + "\n" + proc.stdout).lower()
            if not self.caps.c_toolchain_available and any(h in err_lower for h in _C_TOOLCHAIN_ERROR_HINTS):
                res = BuildResult(
                    bin_name,
                    crate_name,
                    HOST_C_TOOLCHAIN_BLOCKED,
                    None,
                    f"build of `{bin_name}` ({crate_name}) blocked by missing host C/SQLite toolchain",
                )
            else:
                tail = "\n".join(proc.stderr.splitlines()[-15:])
                res = BuildResult(
                    bin_name,
                    crate_name,
                    BUILD_ERROR,
                    None,
                    f"cargo build --locked --bin {bin_name} exited {proc.returncode}:\n{tail}",
                )
            self._cache[bin_name] = res
            return res

        exe_path = self._target_dir / "debug" / self.exe_name(bin_name)
        if not exe_path.is_file():
            res = BuildResult(
                bin_name,
                crate_name,
                BUILD_ERROR,
                None,
                f"cargo build succeeded but binary was not found at {exe_path}",
            )
            self._cache[bin_name] = res
            return res

        res = BuildResult(bin_name, crate_name, BUILT, exe_path, f"built {exe_path}")
        self._cache[bin_name] = res
        return res

    def build_all(self, bins: tuple[str, ...] = SUT_BINARIES) -> dict[str, BuildResult]:
        return {name: self.build_bin(name) for name in bins}
