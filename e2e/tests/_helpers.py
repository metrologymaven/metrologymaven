"""Shared opaque-box E2E test helpers (control server mock, MCP stdio client, site builder)."""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import secrets
import shutil
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Mapping, Sequence

from e2e.lib.env import clean_env
from e2e.lib.harness import TestContext, TestFailure


def set_owner_only_acl(path: Path, *, is_dir: bool) -> None:
    """Restrict permissions on `path` to the current user (POSIX chmod or Windows icacls)."""
    if sys.platform != "win32":
        os.chmod(path, 0o700 if is_dir else 0o600)
        return
    username = os.environ.get("USERNAME") or os.getlogin()
    grant = f"{username}:(OI)(CI)F" if is_dir else f"{username}:F"
    proc = subprocess.run(
        ["icacls", str(path), "/inheritance:r", "/grant:r", grant],
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    if proc.returncode != 0:
        raise TestFailure(f"icacls failed on {path}: {proc.stderr or proc.stdout}")


class FidCtrlMockServer:
    """Minimal pure-Python implementation of `fid-ctrl` loopback server (AUTO.CTRL.WIRE/AUTH)."""

    def __init__(self, root: Path, *, allow_auth: bool = True) -> None:
        self.root = root
        self.runtime_dir = root / "run"
        self.token_dir = root / "profile"
        self.token_file = self.token_dir / "ctrl.token"
        self.allow_auth = allow_auth
        self.instance_id = secrets.token_hex(16)
        self.token = secrets.token_bytes(32)
        self._sock: socket.socket | None = None
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._conn_lock = threading.Lock()
        self._active_conns: set[socket.socket] = set()
        self._conn_threads: list[threading.Thread] = []
        self.port: int = 0
        self.discovery_file: Path | None = None

    def start(self) -> None:
        self.runtime_dir.mkdir(parents=True, exist_ok=True)
        self.token_dir.mkdir(parents=True, exist_ok=True)
        set_owner_only_acl(self.runtime_dir, is_dir=True)
        set_owner_only_acl(self.token_dir, is_dir=True)
        self.token_file.write_bytes(self.token)
        set_owner_only_acl(self.token_file, is_dir=False)

        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(("127.0.0.1", 0))
        sock.listen(4)
        sock.settimeout(0.1)
        self._sock = sock
        self.port = int(sock.getsockname()[1])

        disc = {
            "address": "127.0.0.1",
            "instance_id": self.instance_id,
            "pid": os.getpid(),
            "port": self.port,
            "versions": [1],
        }
        disc_path = self.runtime_dir / f"fid-ctrl-{self.instance_id}.json"
        disc_path.write_text(json.dumps(disc, separators=(",", ":"), sort_keys=True), encoding="utf-8")
        set_owner_only_acl(disc_path, is_dir=False)
        self.discovery_file = disc_path

        self._thread = threading.Thread(target=self._serve_loop, daemon=True)
        self._thread.start()

    def _serve_loop(self) -> None:
        assert self._sock is not None
        while not self._stop.is_set():
            try:
                conn, _ = self._sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            conn.settimeout(2.0)
            with self._conn_lock:
                self._active_conns.add(conn)
                t = threading.Thread(target=self._handle_conn, args=(conn,), daemon=True)
                self._conn_threads.append(t)
            t.start()

    def _handle_conn(self, conn: socket.socket) -> None:
        try:
            conn.settimeout(2.0)
            f = conn.makefile("r", encoding="utf-8", newline="\n")
            server_nonce: bytes | None = None
            authenticated = False
            for raw_line in f:
                line = raw_line.rstrip("\r\n")
                if not line:
                    continue
                req = json.loads(line)
                rid = req.get("id")
                op = req.get("op")
                if op == "hello":
                    server_nonce = secrets.token_bytes(32)
                    reply = {
                        "v": 1,
                        "id": rid,
                        "event": {
                            "kind": "hello",
                            "version": 1,
                            "versions": [1],
                            "instance_id": self.instance_id,
                            "nonce": server_nonce.hex(),
                        },
                    }
                    conn.sendall((json.dumps(reply, separators=(",", ":"), sort_keys=True) + "\n").encode("utf-8"))
                elif op == "auth":
                    params = req.get("params", {})
                    client_nonce_hex = params.get("nonce", "")
                    client_proof_hex = params.get("proof", "")
                    if server_nonce is None or not self.allow_auth:
                        err = {
                            "v": 1,
                            "id": rid,
                            "event": {"kind": "error", "code": "auth_failed", "message": "auth failed"},
                        }
                        conn.sendall((json.dumps(err, separators=(",", ":")) + "\n").encode("utf-8"))
                        break
                    client_nonce = bytes.fromhex(client_nonce_hex)
                    expected = hmac.new(
                        self.token,
                        b"fid-ctrl-client-v1" + server_nonce + client_nonce + self.instance_id.encode("utf-8"),
                        hashlib.sha256,
                    ).digest()
                    if not hmac.compare_digest(expected.hex(), client_proof_hex):
                        err = {
                            "v": 1,
                            "id": rid,
                            "event": {"kind": "error", "code": "auth_failed", "message": "wrong proof"},
                        }
                        conn.sendall((json.dumps(err, separators=(",", ":")) + "\n").encode("utf-8"))
                        break
                    srv_proof = hmac.new(
                        self.token,
                        b"fid-ctrl-server-v1" + client_nonce + server_nonce + self.instance_id.encode("utf-8"),
                        hashlib.sha256,
                    ).digest()
                    authenticated = True
                    reply = {
                        "v": 1,
                        "id": rid,
                        "event": {"kind": "auth", "proof": srv_proof.hex()},
                    }
                    conn.sendall((json.dumps(reply, separators=(",", ":"), sort_keys=True) + "\n").encode("utf-8"))
                else:
                    if not authenticated:
                        break
                    err = {
                        "v": 1,
                        "id": rid,
                        "event": {"kind": "error", "code": "not_found", "message": "no operations"},
                    }
                    conn.sendall((json.dumps(err, separators=(",", ":")) + "\n").encode("utf-8"))
        except Exception:  # noqa: BLE001
            pass
        finally:
            with self._conn_lock:
                self._active_conns.discard(conn)
            try:
                conn.close()
            except OSError:
                pass

    def stop(self) -> None:
        self._stop.set()
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
        with self._conn_lock:
            conns = list(self._active_conns)
            threads = list(self._conn_threads)
        for conn in conns:
            try:
                conn.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            try:
                conn.close()
            except OSError:
                pass
        if self._thread is not None:
            self._thread.join(timeout=2.0)
        for t in threads:
            t.join(timeout=2.0)
        if self.discovery_file is not None and self.discovery_file.exists():
            try:
                self.discovery_file.unlink()
            except OSError:
                pass


def write_disclosure_profiles_index(workdir: Path, filename: str = "disclosure-profiles.json") -> Path:
    """Write a canonical disclosure-enabled MCP profiles index without mutating fixtures/mcp/profiles/expected.json."""
    index = {
        "format": 1,
        "manifest_digest": "sha256:9f3731a2d45012c5e6ae7f17a95acb19c4e2023298f1fafb4f4ef9ed0f708ee4",
        "profiles": [
            {
                "front_door": [
                    "tools.search_tools@1",
                    "tools.describe_tool@1",
                    "tools.invoke_readonly@1",
                    "tools.enable_tools@1",
                    "tools.list_domains@1",
                    "tools.get_workflow@1",
                    "project.list_features@1",
                    "program.save_draft@1",
                ],
                "name": "core",
                "served": [
                    "egress.send@1",
                    "program.save_draft@1",
                    "project.list_features@1",
                    "rpt.release_report@1",
                    "tools.describe_tool@1",
                    "tools.enable_tools@1",
                    "tools.get_workflow@1",
                    "tools.invoke_readonly@1",
                    "tools.list_domains@1",
                    "tools.search_tools@1",
                ],
                "title": "Metrology Maven Core",
            },
            {
                "front_door": [
                    "tools.search_tools@1",
                    "tools.describe_tool@1",
                    "tools.invoke_readonly@1",
                    "tools.enable_tools@1",
                    "tools.list_domains@1",
                    "tools.get_workflow@1",
                    "align.run_bestfit@1",
                    "align.set_datum@1",
                    "sim.run_program@1",
                ],
                "name": "inspect",
                "served": [
                    "align.adopt_alignment@1",
                    "align.run_bestfit@1",
                    "align.run_bestfit@2",
                    "align.set_datum@1",
                    "probe.read_temperature@1",
                    "project.list_features@1",
                    "sim.run_program@1",
                    "tools.describe_tool@1",
                    "tools.enable_tools@1",
                    "tools.get_workflow@1",
                    "tools.invoke_readonly@1",
                    "tools.list_domains@1",
                    "tools.search_tools@1",
                ],
                "title": "Metrology Maven Inspect",
            },
        ],
        "surface": "mcp",
    }
    out = workdir / filename
    out.write_text(json.dumps(index, separators=(",", ":"), sort_keys=True), encoding="utf-8")
    return out


def write_site_a11y_tokens(workdir: Path, filename: str = "site-tokens.json") -> Path:
    """Write a token file matching site/src/style.css light/dark custom properties for `fid-outreach qa a11y`."""
    tokens = {
        "themes": {
            "light": {
                "paper": "#f4f6f8",
                "sheet": "#ffffff",
                "ink": "#15202b",
                "ink.2": "#4a5866",
                "rule": "#cfd7df",
                "grid": "#e8edf2",
                "ruby": "#b5132e",
                "ruby.soft": "#f6e3e7",
                "steel": "#2f5f8a",
                "steel.soft": "#e2ecf5",
                "ok": "#256a41",
                "ok.soft": "#e1f1e7",
                "warn": "#9a5a08",
                "warn.soft": "#f8ecd9",
            },
            "dark": {
                "paper": "#0f151b",
                "sheet": "#151d25",
                "ink": "#e7edf2",
                "ink.2": "#a3b3c1",
                "rule": "#2c3946",
                "grid": "#1b252e",
                "ruby": "#f06a80",
                "ruby.soft": "#3a1720",
                "steel": "#86b6e0",
                "steel.soft": "#16283a",
                "ok": "#6cc08f",
                "ok.soft": "#14301f",
                "warn": "#e3a650",
                "warn.soft": "#33240f",
            },
        }
    }
    out = workdir / filename
    out.write_text(json.dumps(tokens, indent=2, sort_keys=True), encoding="utf-8")
    return out


def run_mcp_session(
    mcp_bin: Path,
    args: Sequence[str],
    messages: Sequence[Mapping[str, Any] | str],
    *,
    cwd: Path,
    timeout: float = 20.0,
) -> tuple[int, list[dict[str, Any]], str, str]:
    """Send line-delimited JSON-RPC messages over stdio to `fid-mcp` and close stdin cleanly."""
    payload_lines: list[bytes] = []
    for msg in messages:
        if isinstance(msg, str):
            payload_lines.append((msg.rstrip("\n") + "\n").encode("utf-8"))
        else:
            payload_lines.append((json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8"))
    stdin_bytes = b"".join(payload_lines)

    proc = subprocess.Popen(
        [str(mcp_bin), *args],
        cwd=cwd,
        env=clean_env(),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        out_b, err_b = proc.communicate(input=stdin_bytes, timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        out_b, err_b = proc.communicate(timeout=5)
        raise TestFailure(f"fid-mcp timed out after {timeout}s; stderr={err_b.decode('utf-8', 'replace')!r}")

    stdout = out_b.decode("utf-8", errors="replace")
    stderr = err_b.decode("utf-8", errors="replace")
    parsed: list[dict[str, Any]] = []
    for line in stdout.splitlines():
        if not line.strip():
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError as exc:
            raise TestFailure(f"fid-mcp emitted non-JSON line on stdout: {line!r} ({exc}); stderr={stderr!r}") from exc
        if not isinstance(obj, dict):
            raise TestFailure(f"fid-mcp stdout line is not JSON object: {line!r}")
        parsed.append(obj)
    return proc.returncode, parsed, stdout, stderr


def mcp_init_messages() -> list[dict[str, Any]]:
    return [
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "e2e-harness", "version": "1.0.0"},
            },
        },
        {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}},
    ]


def build_isolated_site(ctx: TestContext, dest_name: str = "site_work") -> Path:
    """Copy site inputs into `ctx.workdir` and run `python site/build.py` without touching the repo."""
    copy_root = ctx.workdir / dest_name
    rel_dirs = ["site", "tools", "spec", "seeds", "decisions"]
    if (ctx.repo_root / "guide").is_dir():
        rel_dirs.append("guide")
    ctx.copy_repo_subset(copy_root, rel_dirs)
    # Remove any pre-existing dist/ inside the copy so build is fresh.
    dist = copy_root / "site" / "dist"
    if dist.exists():
        shutil.rmtree(dist)
    # Also copy built wasm artifact if present so site/build.py can pick it up.
    wasm_rel = Path("target") / "wasm32-wasip1" / "release" / "fid-guide-wasm.wasm"
    if (ctx.repo_root / wasm_rel).is_file():
        dst_wasm = copy_root / wasm_rel
        dst_wasm.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ctx.repo_root / wasm_rel, dst_wasm)

    res = ctx.run([sys.executable, str(copy_root / "site" / "build.py")], cwd=copy_root, timeout=120)
    if res.returncode != 0:
        raise TestFailure(
            f"site/build.py failed (exit {res.returncode}):\nstdout={res.stdout}\nstderr={res.stderr}"
        )
    if not dist.is_dir():
        raise TestFailure("site/build.py exited 0 but site/dist/ was not created")
    return copy_root


def prepare_scope_root(ctx: TestContext, dest_name: str = "scope_work") -> Path:
    """Create an isolated repo copy for `scope.py check` / `milestone` without dirtying tracked files."""
    copy_root = ctx.workdir / dest_name
    rel_dirs = ["spec", "seeds", "decisions", "tools"]
    if (ctx.repo_root / "milestones").is_dir():
        rel_dirs.append("milestones")
    ctx.copy_repo_subset(copy_root, rel_dirs)
    (copy_root / "generated").mkdir(parents=True, exist_ok=True)

    # Mirror existing crates/*/Cargo.toml and existing non-crate repo subdirs checked by `built_in`.
    for cargo_toml in (ctx.repo_root / "crates").glob("*/Cargo.toml"):
        dst = copy_root / "crates" / cargo_toml.parent.name / "Cargo.toml"
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(cargo_toml, dst)

    for top in (
        "suite",
        "fixtures",
        "house-rules",
        "acceptance",
        "kernel-build",
        "scripts",
        "vendor",
        "docs",
        "ci",
        "guide",
        "release",
        "site",
    ):
        src_top = ctx.repo_root / top
        if not src_top.is_dir():
            continue
        # Create directory skeleton (up to 4 levels) for any existing directories so built_in path checks match repo.
        (copy_root / top).mkdir(parents=True, exist_ok=True)
        for sub in src_top.rglob("*"):
            if sub.is_dir():
                rel = sub.relative_to(ctx.repo_root)
                if len(rel.parts) <= 5:
                    (copy_root / rel).mkdir(parents=True, exist_ok=True)
    return copy_root
