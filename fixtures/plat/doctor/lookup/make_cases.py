"""Independent reference oracle for PLAT.DOCTOR.LOOKUP (tier and hardware-profile verdicts).

Licensed with the code (MIT OR Apache-2.0, D-001). Python standard library only.

Implements the leaf's rules straight from spec/plat/doctor/lookup/NODE.md (not from the Rust
code) and writes `cases.json`: a canonical JSON array of cases, each with an input snapshot
(the EnvSnapshot fixtures of PLAT.DOCTOR.PROBE, some with hand-made mutations), the profile
file used (or null for "no profile file"), the phase, and the expected assessment as the
canonical JSON object `fid_doctor::lookup::Assessment::to_json` must produce byte for byte.
The matrix is always fixtures/plat/matrix/valid.toml.

Rules implemented (REQ-PLAT.DOCTOR.LOOKUP-001..006):
  tier      matrix row for (os family, version, arch): tier if since_phase <= phase, else
            planned; no row -> not_listed; os or arch unknown -> unknown (reason copied).
            Versions are 1 to 4 dot-separated runs of 1-10 ASCII digits, each a u32,
            compared with missing parts as 0; a probed version that does not parse -> unknown
            (reason error), since it was never compared with the matrix.
  fields    cpu_level, cores, ram_gib (total RAM), gpu_class, vram_gib, storage_free_gib
            (install volume), display (both dimensions), in that order.
  gpu class fallback or device_type cpu -> software; integrated -> integrated; discrete with
            VRAM < 6 GiB -> discrete_entry, >= 6 -> discrete_mid; discrete with no VRAM ->
            "at least discrete_entry" (meets up to discrete_entry, unknown above); other ->
            unknown.
  cpu level ranks within an arch: x86-64-v1 < v2 < v3 < v4; armv8.0 < armv8.2. A level or
            arch the profile cannot rank is unknown.
  verdict   below if any shortfall, else indeterminate if any unknown, else meets.
  outcome   unsupported_platform (planned or not listed) 3 > below_minimum 2 >
            indeterminate 4 (minimum indeterminate, or tier unknown) > meets_minimum 1 >
            meets_recommended 0.
  no file   minimum and recommended both indeterminate with reason no_profiles.

Run from the repository root:
  python fixtures/plat/doctor/lookup/make_cases.py          (write)
  python fixtures/plat/doctor/lookup/make_cases.py --check  (exit 1 if cases.json differs)
"""

import copy
import json
import os
import re
import sys
import tomllib
import unicodedata

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", "..", ".."))
OUT = os.path.join(HERE, "cases.json")

PHASES = ["P0", "P1a", "P1b", "P2", "P3", "P4", "later"]
GPU_RANK = ["software", "integrated", "discrete_entry", "discrete_mid"]
LEVEL_RANK = {
    ("x86_64", "x86-64-v1"): 1,
    ("x86_64", "x86-64-v2"): 2,
    ("x86_64", "x86-64-v3"): 3,
    ("x86_64", "x86-64-v4"): 4,
    ("aarch64", "armv8.0"): 0,
    ("aarch64", "armv8.2"): 2,
}
FIELDS = ["cpu_level", "cores", "ram_gib", "gpu_class", "vram_gib", "storage_free_gib", "display"]
EXIT = {
    "unsupported_platform": 3,
    "below_minimum": 2,
    "indeterminate": 4,
    "meets_minimum": 1,
    "meets_recommended": 0,
}
MID_VRAM = 6


def num(x):
    """Integral floats print as integers in RFC 8785 (ECMAScript number formatting)."""
    if isinstance(x, float) and x.is_integer():
        return int(x)
    return x


def canon(value):
    def norm(v):
        if isinstance(v, dict):
            return {k: norm(x) for k, x in v.items()}
        if isinstance(v, list):
            return [norm(x) for x in v]
        if isinstance(v, float):
            return num(v)
        return v

    return json.dumps(norm(value), sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def load_toml(rel):
    with open(os.path.join(ROOT, rel), "rb") as f:
        return tomllib.load(f)


def load_snapshot(name):
    with open(os.path.join(ROOT, "fixtures", "plat", "doctor", name), "rb") as f:
        return json.load(f)


U32_MAX = 4294967295
VERSION_RE = re.compile(r"[0-9]{1,10}(\.[0-9]{1,10}){0,3}")


def version(s):
    """Dotted version of 1 to 4 ASCII-digit parts, each a u32, missing parts 0; else None."""
    if VERSION_RE.fullmatch(s) is None:
        return None
    parts = [int(p) for p in s.split(".")]
    if any(p > U32_MAX for p in parts):
        return None
    return tuple(parts + [0] * (4 - len(parts)))


def bound_detail(s):
    """Control characters to spaces, whitespace runs collapsed, at most 200 characters."""
    cleaned = "".join(" " if unicodedata.category(c) == "Cc" else c for c in s)
    return " ".join(cleaned.split())[:200]


def tier_of(snap, matrix, phase):
    for item in ("os", "arch"):
        p = snap[item]
        if p["status"] != "known":
            return {"status": "unknown", "reason": p["reason"], "detail": item + ": " + p["detail"]}
    family = snap["os"]["value"]["family"]
    ver = snap["os"]["value"]["version"]
    arch = snap["arch"]["value"]
    v = version(ver)
    if v is None:
        # Never compared with any row: unknown, not a checked not_listed.
        return {"status": "unknown", "reason": "error", "detail": bound_detail("os: unparsable version " + ver)}
    for row in matrix["row"]:
        lo = version(row["version_min"])
        hi = version(row["version_max"]) if "version_max" in row else None
        if row["os"] == family and row["arch"] == arch and v >= lo and (hi is None or v < hi):
            if PHASES.index(phase) >= PHASES.index(row["since_phase"]):
                return {"status": "known", "value": {"kind": "tier", "tier": row["tier"]}}
            return {
                "status": "known",
                "value": {"kind": "planned", "tier": row["tier"], "since_phase": row["since_phase"]},
            }
    return {"status": "known", "value": {"kind": "not_listed"}}


def known(snap, item):
    p = snap[item]
    return p["value"] if p["status"] == "known" else None


def gpu_class(gpu):
    """Returns ("exact", class), ("at_least", class) or None."""
    if gpu["fallback"] or gpu["device_type"] == "cpu":
        return ("exact", "software")
    if gpu["device_type"] == "integrated":
        return ("exact", "integrated")
    if gpu["device_type"] == "discrete":
        if gpu["vram_gib"] is None:
            return ("at_least", "discrete_entry")
        return ("exact", "discrete_entry" if gpu["vram_gib"] < MID_VRAM else "discrete_mid")
    return None


def check(field, prof, snap):
    """Returns "meets", "unknown" or (required, found)."""
    cpu, ram, gpu = known(snap, "cpu"), known(snap, "ram"), known(snap, "gpu")
    if field == "cpu_level":
        arch = known(snap, "arch")
        if cpu is None or arch is None or arch not in prof["cpu_level"]:
            return "unknown"
        req = prof["cpu_level"][arch]
        r, f = LEVEL_RANK.get((arch, req)), LEVEL_RANK.get((arch, cpu["instruction_set_level"]))
        if r is None or f is None:
            return "unknown"
        return "meets" if f >= r else (req, cpu["instruction_set_level"])
    if field == "cores":
        if cpu is None:
            return "unknown"
        return "meets" if cpu["physical_cores"] >= prof["cores"] else (prof["cores"], cpu["physical_cores"])
    if field in ("ram_gib", "vram_gib", "storage_free_gib"):
        if field == "ram_gib":
            found = None if ram is None else ram["total_gib"]
        elif field == "vram_gib":
            found = None if gpu is None else gpu["vram_gib"]
        else:
            vols = known(snap, "storage") or []
            inst = [v for v in vols if v["role"] == "install"]
            found = inst[0]["free_gib"] if inst else None
        if found is None:
            return "unknown"
        return "meets" if found >= prof[field] else (prof[field], found)
    if field == "gpu_class":
        c = None if gpu is None else gpu_class(gpu)
        req = prof["gpu_class"]
        if c is None:
            return "unknown"
        kind, cls = c
        if GPU_RANK.index(cls) >= GPU_RANK.index(req):
            return "meets"
        return "unknown" if kind == "at_least" else (req, cls)
    if field == "display":
        d = known(snap, "display")
        if d is None:
            return "unknown"
        req = prof["display"]
        return "meets" if d[0] >= req[0] and d[1] >= req[1] else (list(req), list(d))
    raise AssertionError(field)


def verdict(name, prof, snap):
    shortfalls, unknowns = [], []
    for field in FIELDS:
        r = check(field, prof, snap)
        if r == "unknown":
            unknowns.append(field)
        elif r != "meets":
            shortfalls.append({"field": field, "required": r[0], "found": r[1]})
    v = "below" if shortfalls else ("indeterminate" if unknowns else "meets")
    return {"name": name, "verdict": v, "reason": None, "shortfalls": shortfalls, "unknowns": unknowns}


def assess(snap, matrix, profiles, phase):
    tier = tier_of(snap, matrix, phase)
    if profiles is None:
        verdicts = {
            n: {"name": n, "verdict": "indeterminate", "reason": "no_profiles", "shortfalls": [], "unknowns": []}
            for n in ("minimum", "recommended")
        }
    else:
        verdicts = {n: verdict(n, p, snap) for n, p in profiles["profiles"].items()}
    vmin = verdicts["minimum"]["verdict"]
    vrec = verdicts["recommended"]["verdict"]
    if tier["status"] == "known" and tier["value"]["kind"] != "tier":
        outcome = "unsupported_platform"
    elif vmin == "below":
        outcome = "below_minimum"
    elif vmin == "indeterminate" or tier["status"] == "unknown":
        outcome = "indeterminate"
    elif vrec != "meets":
        outcome = "meets_minimum"
    else:
        outcome = "meets_recommended"
    return {
        "schema_version": 1,
        "tier": tier,
        "profiles": [verdicts[n] for n in sorted(verdicts)],
        "outcome": outcome,
        "exit_code": EXIT[outcome],
    }


def mutate(base, **changes):
    """Copies snapshot `base` and applies `path=value` changes (path segments joined by __)."""
    s = copy.deepcopy(load_snapshot(base))
    for path, value in changes.items():
        keys = path.split("__")
        target = s
        for k in keys[:-1]:
            target = target[k]
        target[keys[-1]] = value
    return s


def unknown(reason, detail):
    return {"status": "unknown", "reason": reason, "detail": detail}


PROFILES = "fixtures/plat/hw/profiles-valid.toml"

# (name, snapshot, profile file or None, phase)
CASES = [
    ("ac1_win11_discrete", load_snapshot("env-win11-discrete.json"), PROFILES, "P1a"),
    ("ac2_ubuntu_vm_software", load_snapshot("env-ubuntu-vm-software.json"), PROFILES, "P1a"),
    ("ac3_freebsd", load_snapshot("env-freebsd.json"), PROFILES, "P1a"),
    ("ac4_unknown_ram", load_snapshot("env-unknown-ram.json"), PROFILES, "P1a"),
    ("ac5_no_profiles", load_snapshot("env-win11-discrete.json"), None, "P1a"),
    ("macos_arm_integrated", load_snapshot("env-macos-arm.json"), PROFILES, "P1a"),
    ("all_unknown_but_os", load_snapshot("env-unknowns.json"), PROFILES, "P1a"),
    ("win11_at_p0_planned", load_snapshot("env-win11-discrete.json"), PROFILES, "P0"),
    ("win11_at_later", load_snapshot("env-win11-discrete.json"), PROFILES, "later"),
    ("ubuntu_aarch64_planned_tier2", mutate("env-ubuntu-vm-software.json", arch__value="aarch64"), PROFILES, "P1a"),
    ("ubuntu_aarch64_tier2_at_p1b", mutate("env-ubuntu-vm-software.json", arch__value="aarch64"), PROFILES, "P1b"),
    ("ubuntu_version_past_range", mutate("env-ubuntu-vm-software.json", os__value__version="24.10"), PROFILES, "P1a"),
    ("macos_x86_64_tier3_later", mutate("env-macos-arm.json", arch__value="x86_64"), PROFILES, "later"),
    ("win10_lower_row", mutate("env-win11-discrete.json", os__value__version="10.0.19045"), PROFILES, "P1a"),
    ("win_below_matrix", mutate("env-win11-discrete.json", os__value__version="6.1"), PROFILES, "P1a"),
    ("unparsable_version", mutate("env-win11-discrete.json", os__value__version="11 Pro"), PROFILES, "P1a"),
    ("unparsable_version_plus", mutate("env-win11-discrete.json", os__value__version="+1"), PROFILES, "P1a"),
    ("unparsable_version_underscore", mutate("env-win11-discrete.json", os__value__version="1_0"), PROFILES, "P1a"),
    ("unparsable_version_five_parts", mutate("env-win11-discrete.json", os__value__version="10.0.22631.1.2"),
     PROFILES, "P1a"),
    ("unparsable_version_u32_overflow", mutate("env-win11-discrete.json", os__value__version="10.0.4294967296"),
     PROFILES, "P1a"),
    ("version_u32_max_parses", mutate("env-win11-discrete.json", os__value__version="10.0.4294967295"), PROFILES,
     "P1a"),
    ("unparsable_version_and_below", mutate("env-ubuntu-vm-software.json", os__value__version="24.04 LTS"),
     PROFILES, "P1a"),
    ("riscv_not_listed", mutate("env-win11-discrete.json", arch__value="riscv64"), PROFILES, "P1a"),
    ("os_unknown_tier_unknown", mutate("env-win11-discrete.json", os=unknown("error", "os-release unreadable")),
     PROFILES, "P1a"),
    ("arch_unknown_tier_unknown", mutate("env-win11-discrete.json", arch=unknown("unsupported", "no arch probe")),
     PROFILES, "P1a"),
    ("os_unknown_and_below", mutate("env-ubuntu-vm-software.json", os=unknown("timeout", "probe exceeded deadline")),
     PROFILES, "P1a"),
    ("discrete_vram_4_entry", mutate("env-win11-discrete.json", gpu__value__vram_gib=4), PROFILES, "P1a"),
    ("discrete_vram_5_99_entry", mutate("env-win11-discrete.json", gpu__value__vram_gib=5.99), PROFILES, "P1a"),
    ("discrete_vram_6_mid", mutate("env-win11-discrete.json", gpu__value__vram_gib=6), PROFILES, "P1a"),
    ("discrete_vram_unknown", mutate("env-win11-discrete.json", gpu__value__vram_gib=None), PROFILES, "P1a"),
    ("virtual_gpu_unknown_class", mutate("env-win11-discrete.json", gpu__value__device_type="virtual"), PROFILES,
     "P1a"),
    ("no_device_type_unknown_class", mutate("env-win11-discrete.json", gpu__value__device_type=None), PROFILES,
     "P1a"),
    ("cpu_adapter_software", mutate("env-win11-discrete.json", gpu__value__device_type="cpu"), PROFILES, "P1a"),
    ("fallback_flag_software", mutate("env-win11-discrete.json", gpu__value__fallback=True), PROFILES, "P1a"),
    ("gpu_not_built", mutate("env-win11-discrete.json", gpu=unknown("not_built", "built without the gpu feature")),
     PROFILES, "P1a"),
    ("cpu_v1_below", mutate("env-win11-discrete.json", cpu__value__instruction_set_level="x86-64-v1"), PROFILES,
     "P1a"),
    ("cpu_v4_meets", mutate("env-win11-discrete.json", cpu__value__instruction_set_level="x86-64-v4"), PROFILES,
     "P1a"),
    ("cpu_v2_meets_min_only", mutate("env-win11-discrete.json", cpu__value__instruction_set_level="x86-64-v2"),
     PROFILES, "P1a"),
    ("cpu_level_unrecognised", mutate("env-win11-discrete.json", cpu__value__instruction_set_level="unknown"),
     PROFILES, "P1a"),
    ("cpu_level_wrong_arch", mutate("env-win11-discrete.json", cpu__value__instruction_set_level="armv8.2"),
     PROFILES, "P1a"),
    ("arm_v8_0_below_recommended", mutate("env-macos-arm.json", cpu__value__instruction_set_level="armv8.0"),
     PROFILES, "P1a"),
    ("cores_3_below", mutate("env-win11-discrete.json", cpu__value__physical_cores=3), PROFILES, "P1a"),
    ("cores_4_min_only", mutate("env-win11-discrete.json", cpu__value__physical_cores=4), PROFILES, "P1a"),
    ("ram_31_84_below_recommended", mutate("env-win11-discrete.json", ram__value__total_gib=31.84), PROFILES, "P1a"),
    ("ram_16_exact_min", mutate("env-win11-discrete.json", ram__value__total_gib=16), PROFILES, "P1a"),
    ("ram_15_5_below", mutate("env-win11-discrete.json", ram__value__total_gib=15.5), PROFILES, "P1a"),
    ("storage_data_only", mutate("env-win11-discrete.json",
                                 storage__value=[{"free_gib": 450, "role": "data", "total_gib": 1024}]), PROFILES,
     "P1a"),
    ("storage_install_19_5", mutate("env-win11-discrete.json",
                                    storage__value=[{"free_gib": 19.5, "role": "install", "total_gib": 512}]),
     PROFILES, "P1a"),
    ("storage_unknown", mutate("env-win11-discrete.json", storage=unknown("unsupported", "storage probe unsupported")),
     PROFILES, "P1a"),
    ("display_1920x1200_below_rec", mutate("env-win11-discrete.json", display__value=[1920, 1200]), PROFILES, "P1a"),
    ("display_portrait_below_min", mutate("env-win11-discrete.json", display__value=[1080, 1920]), PROFILES, "P1a"),
    ("display_unknown", mutate("env-win11-discrete.json", display=unknown("no_display", "no DISPLAY set")), PROFILES,
     "P1a"),
    ("freebsd_no_profiles", load_snapshot("env-freebsd.json"), None, "P1a"),
    ("os_unknown_no_profiles", mutate("env-win11-discrete.json", os=unknown("error", "x")), None, "P1a"),
]


def build():
    matrix = load_toml("fixtures/plat/matrix/valid.toml")
    cases = []
    names = set()
    for name, snap, prof_path, phase in CASES:
        assert name not in names, name
        names.add(name)
        profiles = None if prof_path is None else load_toml(prof_path)
        cases.append(
            {
                "name": name,
                "phase": phase,
                "profiles": prof_path,
                "snapshot": snap,
                "expected": assess(snap, matrix, profiles, phase),
            }
        )
    # One case per line keeps diffs reviewable; each line is canonical JSON.
    lines = [canon(c) for c in cases]
    return b"[\n" + b",\n".join(lines) + b"\n]\n"


def main():
    data = build()
    if "--check" in sys.argv[1:]:
        try:
            with open(OUT, "rb") as f:
                current = f.read()
        except FileNotFoundError:
            current = b""
        if current != data:
            print(f"{os.path.relpath(OUT, ROOT)} is stale; run without --check", file=sys.stderr)
            return 1
        return 0
    with open(OUT, "wb") as f:
        f.write(data)
    return 0


if __name__ == "__main__":
    sys.exit(main())
