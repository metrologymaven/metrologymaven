#!/usr/bin/env python3
"""Generate the KRN.MAN.REPRO toolchain-digest fixtures (CC-BY-4.0 project data) independently
of the Rust code.

Every expected value comes from Python's standard library (tomllib, json, hashlib) applied to
the rules pinned in spec/krn/man/repro/NODE.md and the module docs of
crates/fid-manifest-gen/src/toolchain.rs, never from the Rust implementation.

Each case directory under fixtures/man/repro/cases/ holds the inputs of one run:

  rust-toolchain.toml     -> kernel-build/rust-toolchain.toml
  cargo-config.toml       -> kernel-build/.cargo/config.toml
  kernel-build.toml       -> kernel-build/kernel-build.toml
  Cargo.toml              -> the workspace manifest
  workspace-config.toml   -> the workspace .cargo/config.toml (absent when the case has none)
  Cargo.lock              -> the lockfile (only its bytes are hashed)
  rustc-vV.txt            -> output of `rustc -vV`
  rustc-cfg.txt           -> output of `rustc --print cfg --target <target> <static rustflags>`
  expected.txt            -> `record <canonical JSON>` and `digest sha256:<hex>` lines, or
                             `error <message>` (the error's Display form)

The base case is a plausible copy of the real settings; every other case changes one thing.
--check also checks the repository's real kernel-build/ settings with this independent checker
(AC-4), so a second implementation agrees that they are complete.

Run:          python fixtures/man/repro/make_fixtures.py
Check (CI):   python fixtures/man/repro/make_fixtures.py --check
"""

import hashlib
import json
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).parent
ROOT = HERE.parent.parent.parent
CASES = HERE / "cases"

RUST_TOOLCHAIN = "kernel-build/rust-toolchain.toml"
CARGO_CONFIG = "kernel-build/.cargo/config.toml"
KERNEL_BUILD = "kernel-build/kernel-build.toml"
WORKSPACE_MANIFEST = "Cargo.toml"
WORKSPACE_CONFIG = ".cargo/config.toml"
LOCKFILE = "Cargo.lock"
PROFILE = "kernel-artifact"
PROFILE_KEYS = sorted(["codegen-units", "debug", "debug-assertions", "incremental", "inherits", "lto",
                       "opt-level", "overflow-checks", "panic", "strip"])
REQUIRED = {"codegen-units": "int:1", "incremental": "bool:false", "inherits": "str:release", "panic": "str:abort"}
FMA_FEATURES = ["fma", "relaxed-simd"]
REMAP_KEYS = ["cargo_home", "target_dir", "vendor", "workspace"]
MAX_SETTINGS = 64 * 1024
MAX_LOCK = 16 * 1024 * 1024


class Refused(Exception):
    pass


def err(code, file, key):
    raise Refused(f"{code}: {file}: {key}")


def load(data, file):
    if len(data) > MAX_SETTINGS:
        raise Refused(f"bad_file: {file}")
    try:
        return tomllib.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError):
        raise Refused(f"bad_file: {file}")


def only(t, allowed, file, prefix):
    for k in sorted(t):
        if k not in allowed:
            err("unexpected_key", file, prefix + k)


def get_table(t, key, file, prefix):
    if key not in t:
        err("missing", file, prefix + key)
    if not isinstance(t[key], dict):
        err("bad_value", file, prefix + key)
    return t[key]


def get_str(t, key, file, prefix):
    if key not in t:
        err("missing", file, prefix + key)
    v = t[key]
    if not isinstance(v, str) or v == "":
        err("bad_value", file, prefix + key)
    return v


def get_strs(t, key, file, prefix):
    if key not in t:
        err("missing", file, prefix + key)
    v = t[key]
    if not isinstance(v, list) or not all(isinstance(x, str) and x for x in v):
        err("bad_value", file, prefix + key)
    return v


def exact_version(s):
    parts = s.split(".")
    return len(parts) == 3 and all(p.isascii() and p.isdigit() and (len(p) == 1 or p[0] != "0") for p in parts)


def plain(s, dot):
    ok = set("abcdefghijklmnopqrstuvwxyz0123456789_-" + ("." if dot else ""))
    return s != "" and all(c in ok for c in s)


def fixed_prefix(s):
    ok = set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._/-")
    return (len(s) > 1 and s.startswith("/") and not s.endswith("/") and all(c in ok for c in s)
            and all(c not in ("", ".", "..") for c in s.split("/")[1:]))


def typed(v):
    if isinstance(v, bool):
        return "bool:" + ("true" if v else "false")
    if isinstance(v, int):
        return f"int:{v}"
    if isinstance(v, str):
        return "str:" + v
    return None


def settings(files):
    # rust-toolchain.toml
    f = RUST_TOOLCHAIN
    doc = load(files["rust-toolchain.toml"], f)
    only(doc, ["toolchain"], f, "")
    tc = get_table(doc, "toolchain", f, "")
    only(tc, ["channel", "components", "profile", "targets"], f, "toolchain.")
    channel = get_str(tc, "channel", f, "toolchain.")
    if not exact_version(channel):
        err("bad_value", f, "toolchain.channel")
    get_strs(tc, "components", f, "toolchain.")
    if get_str(tc, "profile", f, "toolchain.") != "minimal":
        err("bad_value", f, "toolchain.profile")
    targets = get_strs(tc, "targets", f, "toolchain.")
    # kernel-build.toml
    f = KERNEL_BUILD
    doc = load(files["kernel-build.toml"], f)
    only(doc, ["artifact", "remap", "schema", "toolchain"], f, "")
    if get_str(doc, "schema", f, "") != "1":
        err("bad_value", f, "schema")
    t = get_table(doc, "toolchain", f, "")
    only(t, ["commit_hash", "llvm_version"], f, "toolchain.")
    commit = get_str(t, "commit_hash", f, "toolchain.")
    if len(commit) != 40 or not all(c in "0123456789abcdef" for c in commit):
        err("bad_value", f, "toolchain.commit_hash")
    llvm = get_str(t, "llvm_version", f, "toolchain.")
    if not all(p and p.isascii() and p.isdigit() for p in llvm.split(".")):
        err("bad_value", f, "toolchain.llvm_version")
    a = get_table(doc, "artifact", f, "")
    only(a, ["bin", "package"], f, "artifact.")
    package = get_str(a, "package", f, "artifact.")
    binary = get_str(a, "bin", f, "artifact.")
    for k, v in (("bin", binary), ("package", package)):
        if not plain(v, False):
            err("bad_value", f, "artifact." + k)
    r = get_table(doc, "remap", f, "")
    only(r, REMAP_KEYS, f, "remap.")
    remap, seen = {}, set()
    for k in REMAP_KEYS:
        v = get_str(r, k, f, "remap.")
        if not fixed_prefix(v) or v in seen:
            err("bad_value", f, "remap." + k)
        seen.add(v)
        remap[k] = v
    # kernel-build/.cargo/config.toml
    f = CARGO_CONFIG
    doc = load(files["cargo-config.toml"], f)
    only(doc, ["build", "profile", "target"], f, "")
    b = get_table(doc, "build", f, "")
    only(b, ["target"], f, "build.")
    target = get_str(b, "target", f, "build.")
    if not plain(target, True):
        err("bad_value", f, "build.target")
    tt = get_table(doc, "target", f, "")
    only(tt, [target], f, "target.")
    prefix = f"target.{target}."
    t = get_table(tt, target, f, "target.")
    only(t, ["rustflags"], f, prefix)
    flags = get_strs(t, "rustflags", f, prefix)
    disabled = set()
    key = prefix + "rustflags"
    if len(flags) % 2:
        err("bad_value", f, key)
    for i in range(0, len(flags), 2):
        if flags[i] != "-C" or "=" not in flags[i + 1]:
            err("bad_value", f, key)
        k, v = flags[i + 1].split("=", 1)
        if k == "target-cpu":
            # Allowlist: only the generic baseline CPUs.
            if v not in ("generic", "mvp"):
                err("bad_value", f, key)
        elif k == "target-feature":
            # Features may only be disabled; any `+name` is refused.
            for feat in v.split(","):
                if feat[:1] == "-" and plain(feat[1:], False):
                    disabled.add(feat[1:])
                else:
                    err("bad_value", f, key)
        else:
            err("bad_value", f, key)
    profiles = get_table(doc, "profile", f, "")
    only(profiles, [PROFILE], f, "profile.")
    pp = f"profile.{PROFILE}."
    p = get_table(profiles, PROFILE, f, "profile.")
    only(p, PROFILE_KEYS, f, pp)
    profile = {}
    for k in PROFILE_KEYS:
        if k not in p:
            err("missing", f, pp + k)
        tv = typed(p[k])
        if tv is None or (k in REQUIRED and REQUIRED[k] != tv):
            err("bad_value", f, pp + k)
        profile[k] = tv
    if target not in targets:
        err("bad_value", RUST_TOOLCHAIN, "toolchain.targets")
    # the workspace's own files
    m = load(files["Cargo.toml"], WORKSPACE_MANIFEST)
    prof = m.get("profile")
    if isinstance(prof, dict):
        if PROFILE in prof:
            err("workspace_override", WORKSPACE_MANIFEST, f"profile.{PROFILE}")
        rel = prof.get("release")
        if isinstance(rel, dict):
            for k in ["build-override", "package", "rustflags"]:
                if k in rel:
                    err("workspace_override", WORKSPACE_MANIFEST, "profile.release." + k)
    if files.get("workspace-config.toml") is not None:
        c = load(files["workspace-config.toml"], WORKSPACE_CONFIG)
        for k in sorted(c):
            if k not in ("alias", "resolver"):
                err("workspace_override", WORKSPACE_CONFIG, k)
    lock = files["Cargo.lock"]
    if len(lock) == 0 or len(lock) > MAX_LOCK:
        raise Refused(f"bad_file: {LOCKFILE}")
    return dict(channel=channel, commit=commit, llvm=llvm, package=package, bin=binary, target=target,
                flags=flags, profile=profile, remap=remap, disabled=disabled,
                lock="sha256:" + hashlib.sha256(lock).hexdigest())


def rustc_version(text):
    lines = text.splitlines()
    if not lines or not lines[0].startswith("rustc "):
        raise Refused("bad_rustc_output: version_header")
    fields = {}
    for line in lines[1:]:
        if line == "":
            continue
        if ": " not in line:
            raise Refused("bad_rustc_output: version_line")
        k, v = line.split(": ", 1)
        if k in fields:
            raise Refused("bad_rustc_output: version_duplicate")
        fields[k] = v
    if not all(k in fields for k in ("release", "commit-hash", "LLVM version")):
        raise Refused("bad_rustc_output: version_fields")
    return fields["release"], fields["commit-hash"], fields["LLVM version"]


def cfg_features(text):
    feats, any_line = set(), False
    for line in text.splitlines():
        if line == "":
            continue
        any_line = True
        name, eq, value = line.partition("=")
        if name == "" or not all(c.isascii() and (c.isalnum() or c == "_") for c in name):
            raise Refused("bad_rustc_output: cfg_line")
        if eq:
            if len(value) < 2 or value[0] != '"' or value[-1] != '"' or '"' in value[1:-1] or "\\" in value:
                raise Refused("bad_rustc_output: cfg_line")
            if name == "target_feature":
                feats.add(value[1:-1])
    if not any_line:
        raise Refused("bad_rustc_output: cfg_empty")
    return sorted(feats)


def quoted(s):
    # Rust's {:?} of a plain ASCII string without quotes or backslashes.
    assert all(32 <= ord(c) < 127 and c not in '"\\' for c in s)
    return '"' + s + '"'


def record(files):
    st = settings(files)
    release, commit, llvm = rustc_version(files["rustc-vV.txt"].decode("utf-8"))
    for field, pinned, actual in (("release", st["channel"], release), ("commit-hash", st["commit"], commit),
                                  ("LLVM version", st["llvm"], llvm)):
        if pinned != actual:
            raise Refused(f"toolchain_drift: {field}: pinned {quoted(pinned)}, actual {quoted(actual)}")
    feats = cfg_features(files["rustc-cfg.txt"].decode("utf-8"))
    for f in FMA_FEATURES:
        if f in feats:
            raise Refused(f"fma_feature_enabled: {f}")
    for f in sorted(st["disabled"]):
        if f in feats:
            raise Refused(f"feature_not_disabled: {f}")
    doc = {
        "artifact": {"bin": st["bin"], "package": st["package"], "profile": PROFILE, "target": st["target"]},
        "lockfile": st["lock"],
        "profile": st["profile"],
        "remap": st["remap"],
        "rustflags": st["flags"],
        "schema": "fid-kernel-build/1",
        "target_features": feats,
        "toolchain": {"channel": st["channel"], "commit_hash": st["commit"], "llvm_version": st["llvm"]},
    }
    canonical = json.dumps(doc, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return canonical, "sha256:" + hashlib.sha256(canonical).hexdigest()


# --- base inputs ---------------------------------------------------------------------------------

BASE = {
    "rust-toolchain.toml": b'[toolchain]\nchannel = "1.89.0"\ncomponents = []\nprofile = "minimal"\n'
                           b'targets = ["wasm32-wasip1"]\n',
    "cargo-config.toml": b'[build]\ntarget = "wasm32-wasip1"\n\n[target.wasm32-wasip1]\n'
                         b'rustflags = ["-C", "target-cpu=generic", "-C", "target-feature=-relaxed-simd,-simd128"]\n\n'
                         b'[profile.kernel-artifact]\ninherits = "release"\nopt-level = 3\ndebug = false\n'
                         b'strip = "symbols"\ndebug-assertions = false\noverflow-checks = false\nlto = "fat"\n'
                         b'panic = "abort"\nincremental = false\ncodegen-units = 1\n',
    "kernel-build.toml": b'schema = "1"\n\n[toolchain]\ncommit_hash = "29483883eed69d5fb4db01964cdf2af4d86e9cb2"\n'
                         b'llvm_version = "20.1.7"\n\n[artifact]\npackage = "fid-kernel-wasm"\n'
                         b'bin = "fid-kernel-wasm"\n\n[remap]\nworkspace = "/fid/src"\n'
                         b'cargo_home = "/fid/cargo-home"\nvendor = "/fid/vendor"\ntarget_dir = "/fid/target"\n',
    "Cargo.toml": b'[workspace]\nresolver = "2"\nmembers = ["crates/*"]\n\n[workspace.package]\nversion = "0.0.1"\n',
    "workspace-config.toml": b'[alias]\nxtask = "run -q -p fid-xtask --"\n\n[resolver]\n'
                             b'incompatible-rust-versions = "fallback"\n',
    "Cargo.lock": b'# This file is automatically @generated by Cargo.\nversion = 4\n\n[[package]]\n'
                  b'name = "fid-kernel-wasm"\nversion = "0.0.1"\n',
    "rustc-vV.txt": b"rustc 1.89.0 (29483883e 2025-08-04)\nbinary: rustc\n"
                    b"commit-hash: 29483883eed69d5fb4db01964cdf2af4d86e9cb2\ncommit-date: 2025-08-04\n"
                    b"host: x86_64-unknown-linux-gnu\nrelease: 1.89.0\nLLVM version: 20.1.7\n",
    "rustc-cfg.txt": b'debug_assertions\npanic="abort"\ntarget_abi=""\ntarget_arch="wasm32"\n'
                     b'target_endian="little"\ntarget_env="p1"\ntarget_family="wasm"\n'
                     b'target_feature="bulk-memory"\ntarget_feature="multivalue"\n'
                     b'target_feature="mutable-globals"\ntarget_feature="nontrapping-fptoint"\n'
                     b'target_feature="reference-types"\ntarget_feature="sign-ext"\n'
                     b'target_has_atomic="16"\ntarget_has_atomic="32"\ntarget_has_atomic="64"\n'
                     b'target_has_atomic="8"\ntarget_has_atomic="ptr"\ntarget_os="wasi"\n'
                     b'target_pointer_width="32"\ntarget_vendor="unknown"\n',
}


def edit(name, old, new):
    def f(files):
        assert files[name].count(old) == 1, (name, old)
        files[name] = files[name].replace(old, new)
    return f


def drop(name):
    def f(files):
        files[name] = None
    return f


# name -> list of edits applied to a copy of BASE
CASES_SPEC = {
    "ok": [],
    # A macOS ARM64 host gives the same record: the host line is not part of it.
    "ok-other-host": [edit("rustc-vV.txt", b"host: x86_64-unknown-linux-gnu", b"host: aarch64-apple-darwin")],
    "ok-no-workspace-config": [drop("workspace-config.toml")],
    "ok-thin-lto": [edit("cargo-config.toml", b'lto = "fat"', b'lto = "thin"')],
    "ok-lock-changed": [edit("Cargo.lock", b'version = "0.0.1"', b'version = "0.0.2"')],
    "missing-codegen-units": [edit("cargo-config.toml", b"codegen-units = 1\n", b"")],
    "codegen-units-16": [edit("cargo-config.toml", b"codegen-units = 1", b"codegen-units = 16")],
    "panic-unwind": [edit("cargo-config.toml", b'panic = "abort"', b'panic = "unwind"')],
    "incremental-on": [edit("cargo-config.toml", b"incremental = false", b"incremental = true")],
    "unknown-profile-key": [edit("cargo-config.toml", b"codegen-units = 1\n", b"codegen-units = 1\nrpath = false\n")],
    "profile-package-override": [edit("cargo-config.toml", b"codegen-units = 1\n",
                                      b"codegen-units = 1\n\n[profile.kernel-artifact.package.fid-kci]\nopt-level = 1\n")],
    "target-cpu-native": [edit("cargo-config.toml", b"target-cpu=generic", b"target-cpu=native")],
    "target-cpu-other": [edit("cargo-config.toml", b"target-cpu=generic", b"target-cpu=bleeding-edge")],
    "plus-feature": [edit("cargo-config.toml", b"-relaxed-simd,-simd128", b"-relaxed-simd,-simd128,+bulk-memory")],
    "fma-flag": [edit("cargo-config.toml", b"-relaxed-simd,-simd128", b"+relaxed-simd")],
    "build-rustflags": [edit("cargo-config.toml", b'target = "wasm32-wasip1"\n',
                             b'target = "wasm32-wasip1"\nrustflags = ["-C", "opt-level=1"]\n')],
    "channel-stable": [edit("rust-toolchain.toml", b'"1.89.0"', b'"stable"')],
    "target-not-installed": [edit("rust-toolchain.toml", b'["wasm32-wasip1"]', b'["wasm32-unknown-unknown"]')],
    "remap-relative": [edit("kernel-build.toml", b'vendor = "/fid/vendor"', b'vendor = "fid/vendor"')],
    "duplicate-key": [edit("kernel-build.toml", b'bin = "fid-kernel-wasm"\n',
                           b'bin = "fid-kernel-wasm"\nbin = "other"\n')],
    "workspace-release-package": [edit("Cargo.toml", b'version = "0.0.1"\n',
                                       b'version = "0.0.1"\n\n[profile.release.package.fid-kci]\nopt-level = 1\n')],
    "workspace-config-build": [edit("workspace-config.toml", b"[resolver]",
                                    b'[build]\nrustflags = ["-C", "target-cpu=native"]\n\n[resolver]')],
    "workspace-config-source": [edit("workspace-config.toml", b"[resolver]",
                                     b'[source.crates-io]\nreplace-with = "mirror"\n\n[resolver]')],
    "drift-release": [edit("rustc-vV.txt", b"release: 1.89.0", b"release: 1.90.0")],
    "drift-commit": [edit("rustc-vV.txt", b"commit-hash: 2948", b"commit-hash: 0948")],
    "drift-llvm": [edit("rustc-vV.txt", b"LLVM version: 20.1.7", b"LLVM version: 20.1.8")],
    "fma-enabled": [edit("rustc-cfg.txt", b'target_feature="reference-types"\n',
                         b'target_feature="reference-types"\ntarget_feature="relaxed-simd"\n')],
    "simd-not-disabled": [edit("rustc-cfg.txt", b'target_feature="sign-ext"\n',
                               b'target_feature="sign-ext"\ntarget_feature="simd128"\n')],
    "cfg-malformed": [edit("rustc-cfg.txt", b'target_os="wasi"', b"target_os=wasi")],
    "version-no-llvm": [edit("rustc-vV.txt", b"LLVM version: 20.1.7\n", b"")],
}


def case_files(name):
    files = dict(BASE)
    for e in CASES_SPEC[name]:
        e(files)
    return files


def expected(files):
    try:
        canonical, digest = record(files)
    except Refused as e:
        return f"error {e}\n".encode("utf-8")
    return b"record " + canonical + b"\n" + f"digest {digest}\n".encode("ascii")


# --- golden calls for the reproducible reference artifact ------------------------------------------
# The reference module registers no algorithm, so of the KRN.MAN.WASM calls only those whose
# outcome does not depend on a registered algorithm apply, relabelled by hand from the shim's
# table (crates/fid-kernel-wasm/src/shim.rs): a malformed call is `malformed_call` (exit 2)
# whatever is registered; `unknown-algorithm` stays `unknown_algorithm` (exit 1); and every
# well-formed call of the KCI vectors (kci-call-*, all digests correct) names an algorithm the
# reference module does not have, so it is `unknown_algorithm` (exit 1). Calls that rely on
# a stand-in algorithm's behaviour (digest checks, failures, resource limits) are left out.
def reference_calls():
    rows = ["# KRN.MAN.REPRO golden calls for the reproducible reference artifact (no algorithms registered).",
            "# Written by fixtures/man/repro/make_fixtures.py from fixtures/man/wasm/calls-v1.txt. CC-BY-4.0.",
            "# Format: <name><TAB><any | wasm><TAB><canonical call><TAB><runner exit><TAB><expected stdout>."]
    src = (ROOT / "fixtures" / "man" / "wasm" / "calls-v1.txt").read_text(encoding="utf-8")
    for line in src.splitlines():
        if not line or line.startswith("#"):
            continue
        name, mode, call, exitc, out = line.split("\t")
        if name.startswith("malformed-"):
            assert (exitc, out) == ("2", '{"error":"malformed_call"}'), name
            rows.append("\t".join([name, mode, call, exitc, out]))
        elif name == "unknown-algorithm" or name.startswith("kci-call-"):
            assert mode == "any", name
            rows.append("\t".join([name, mode, call, "1", '{"error":"unknown_algorithm"}']))
    assert len(rows) - 3 >= 30, len(rows)
    return ("\n".join(rows) + "\n").encode("ascii")


def build():
    out = {HERE / "calls-reference-v1.txt": reference_calls()}
    for name in sorted(CASES_SPEC):
        files = case_files(name)
        d = CASES / name
        for fname, data in files.items():
            if data is not None:
                out[d / fname] = data
        out[d / "expected.txt"] = expected(files)
    return out


def check_real():
    """AC-4 by a second implementation: the repository's real settings pass."""
    files = {
        "rust-toolchain.toml": (ROOT / RUST_TOOLCHAIN).read_bytes(),
        "cargo-config.toml": (ROOT / CARGO_CONFIG).read_bytes(),
        "kernel-build.toml": (ROOT / KERNEL_BUILD).read_bytes(),
        "Cargo.toml": (ROOT / WORKSPACE_MANIFEST).read_bytes(),
        "workspace-config.toml": (ROOT / WORKSPACE_CONFIG).read_bytes() if (ROOT / WORKSPACE_CONFIG).exists() else None,
        "Cargo.lock": (ROOT / LOCKFILE).read_bytes(),
    }
    st = settings(files)
    # The rebuild script reads these with a line-based reader; they must be what tomllib reads.
    script_keys = {
        (RUST_TOOLCHAIN, "toolchain", "channel"): st["channel"],
        (CARGO_CONFIG, "build", "target"): st["target"],
        (KERNEL_BUILD, "toolchain", "commit_hash"): st["commit"],
        (KERNEL_BUILD, "toolchain", "llvm_version"): st["llvm"],
        (KERNEL_BUILD, "artifact", "package"): st["package"],
        (KERNEL_BUILD, "artifact", "bin"): st["bin"],
    }
    for k in REMAP_KEYS:
        script_keys[(KERNEL_BUILD, "remap", k)] = st["remap"][k]
    for (file, section, key), want in script_keys.items():
        hits, cur = [], None
        for line in (ROOT / file).read_text(encoding="utf-8").splitlines():
            s = line.strip()
            if s.startswith("["):
                cur = s.strip("[]").strip()
            elif cur == section and s.split("=", 1)[0].strip() == key:
                hits.append(s)
        if hits != [f'{key} = "{want}"']:
            raise SystemExit(f"{file}: [{section}] {key} must be one line `{key} = \"{want}\"`, got {hits}")
    print(f"real kernel-build settings ok: {st['channel']} {st['target']} {st['package']}")


def main():
    check = "--check" in sys.argv[1:]
    out = build()
    bad = []
    for path, data in sorted(out.items()):
        if check:
            if not path.exists() or path.read_bytes() != data:
                bad.append(path)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        known = set(out)
        for p in CASES.rglob("*"):
            if p.is_file() and p not in known:
                bad.append(p)
        if bad:
            for p in bad:
                print(f"stale: {p.relative_to(ROOT).as_posix()}")
            raise SystemExit(1)
    check_real()
    print(f"{len(CASES_SPEC)} cases {'up to date' if check else 'written'}")


if __name__ == "__main__":
    main()
