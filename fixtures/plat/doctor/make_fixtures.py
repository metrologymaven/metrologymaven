"""Writes canonical EnvSnapshot fixtures for PLAT.DOCTOR.PROBE and PLAT.DOCTOR.LOOKUP.

Licensed with the code (MIT OR Apache-2.0, D-001). Every snapshot is hand-written and
synthetic, formatted as exact RFC 8785 canonical JSON (matching fid_canon::to_canonical_bytes)
so FakeEnv round-trips byte-identically (PLAT.DOCTOR.PROBE AC-3).

Layout:
  env-win11-discrete.json     Windows 11 x86_64 workstation with discrete 12 GiB GPU
  env-ubuntu-vm-software.json Ubuntu 24.04 x86_64 VM with 8 GiB RAM and software fallback
  env-macos-arm.json          macOS 15.1 aarch64 laptop with integrated Metal GPU
  env-unknowns.json           Snapshot with error, timeout, no_display and not_built unknowns
  env-freebsd.json            FreeBSD 14.1 x86_64 (unsupported platform for LOOKUP AC-3)
  env-unknown-ram.json        Windows 11 meeting everything except RAM unknown (LOOKUP AC-4)

Run from the repository root:
  python fixtures/plat/doctor/make_fixtures.py          (write)
  python fixtures/plat/doctor/make_fixtures.py --check  (exit 1 if any file differs)
"""

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def canon(value):
    """RFC 8785 canonical JSON for ASCII strings, integers, finite floats, bools and null."""
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def known(value):
    return {"status": "known", "value": value}


def unknown(reason, detail):
    return {"status": "unknown", "reason": reason, "detail": detail}


def snapshot(*, os_val, arch, cpu, ram, storage, display, gpu):
    return {
        "schema_version": 1,
        "os": os_val,
        "arch": arch,
        "cpu": cpu,
        "ram": ram,
        "storage": storage,
        "display": display,
        "gpu": gpu,
    }


FIXTURES = {
    "env-win11-discrete.json": snapshot(
        os_val=known({"family": "windows", "version": "10.0.22631"}),
        arch=known("x86_64"),
        cpu=known(
            {
                "model": "Synthetic Workstation CPU 8C",
                "physical_cores": 8,
                "instruction_set_level": "x86-64-v3",
            }
        ),
        ram=known({"total_gib": 32, "available_gib": 24}),
        storage=known(
            [
                {"role": "install", "free_gib": 200, "total_gib": 512},
                {"role": "data", "free_gib": 450, "total_gib": 1024},
            ]
        ),
        display=known([2560, 1440]),
        gpu=known(
            {
                "backend": "dx12",
                "adapter": "Synthetic Discrete GPU 12G",
                "driver_version": "560.94",
                "device_type": "discrete",
                "vram_gib": 12,
                "fallback": False,
                "fallback_cause": "none",
                "reason": "",
            }
        ),
    ),
    "env-ubuntu-vm-software.json": snapshot(
        os_val=known({"family": "ubuntu", "version": "24.04"}),
        arch=known("x86_64"),
        cpu=known(
            {
                "model": "Synthetic Virtual CPU 4C",
                "physical_cores": 4,
                "instruction_set_level": "x86-64-v2",
            }
        ),
        ram=known({"total_gib": 8, "available_gib": 5}),
        storage=known(
            [
                {"role": "install", "free_gib": 40, "total_gib": 80},
                {"role": "data", "free_gib": 40, "total_gib": 80},
            ]
        ),
        display=known([1920, 1080]),
        gpu=known(
            {
                "backend": "software",
                "adapter": None,
                "driver_version": None,
                "device_type": None,
                "vram_gib": None,
                "fallback": True,
                "fallback_cause": "no_qualifying_adapter",
                "reason": "no hardware adapter qualifies; software",
            }
        ),
    ),
    "env-macos-arm.json": snapshot(
        os_val=known({"family": "macos", "version": "15.1"}),
        arch=known("aarch64"),
        cpu=known(
            {
                "model": "Synthetic ARM Laptop CPU 8C",
                "physical_cores": 8,
                "instruction_set_level": "armv8.2",
            }
        ),
        ram=known({"total_gib": 16, "available_gib": 11}),
        storage=known(
            [
                {"role": "install", "free_gib": 120, "total_gib": 512},
                {"role": "data", "free_gib": 120, "total_gib": 512},
            ]
        ),
        display=known([2560, 1600]),
        gpu=known(
            {
                "backend": "metal",
                "adapter": "Synthetic Integrated Metal GPU",
                "driver_version": None,
                "device_type": "integrated",
                "vram_gib": 4,
                "fallback": False,
                "fallback_cause": "none",
                "reason": "",
            }
        ),
    ),
    "env-unknowns.json": snapshot(
        os_val=known({"family": "ubuntu", "version": "24.04"}),
        arch=known("x86_64"),
        cpu=unknown("timeout", "probe exceeded deadline"),
        ram=unknown("error", "cannot read meminfo"),
        storage=unknown("unsupported", "storage probe unsupported"),
        display=unknown("no_display", "no DISPLAY or WAYLAND_DISPLAY set"),
        gpu=unknown("not_built", "built without the gpu feature"),
    ),
    "env-freebsd.json": snapshot(
        os_val=known({"family": "freebsd", "version": "14.1"}),
        arch=known("x86_64"),
        cpu=known(
            {
                "model": "Synthetic Workstation CPU 8C",
                "physical_cores": 8,
                "instruction_set_level": "x86-64-v3",
            }
        ),
        ram=known({"total_gib": 32, "available_gib": 24}),
        storage=known(
            [
                {"role": "install", "free_gib": 150, "total_gib": 512},
                {"role": "data", "free_gib": 150, "total_gib": 512},
            ]
        ),
        display=known([2560, 1440]),
        gpu=known(
            {
                "backend": "vulkan",
                "adapter": "Synthetic Discrete GPU 8G",
                "driver_version": "24.2.0",
                "device_type": "discrete",
                "vram_gib": 8,
                "fallback": False,
                "fallback_cause": "none",
                "reason": "",
            }
        ),
    ),
    "env-unknown-ram.json": snapshot(
        os_val=known({"family": "windows", "version": "10.0.22631"}),
        arch=known("x86_64"),
        cpu=known(
            {
                "model": "Synthetic Workstation CPU 8C",
                "physical_cores": 8,
                "instruction_set_level": "x86-64-v3",
            }
        ),
        ram=unknown("error", "memory probe failed"),
        storage=known(
            [
                {"role": "install", "free_gib": 200, "total_gib": 512},
                {"role": "data", "free_gib": 450, "total_gib": 1024},
            ]
        ),
        display=known([2560, 1440]),
        gpu=known(
            {
                "backend": "dx12",
                "adapter": "Synthetic Discrete GPU 12G",
                "driver_version": "560.94",
                "device_type": "discrete",
                "vram_gib": 12,
                "fallback": False,
                "fallback_cause": "none",
                "reason": "",
            }
        ),
    ),
}


def main():
    check = "--check" in sys.argv[1:]
    if any(a != "--check" for a in sys.argv[1:]):
        sys.stderr.write("usage: make_fixtures.py [--check]\n")
        return 2

    want = {name: canon(val) for name, val in FIXTURES.items()}
    if check:
        bad = []
        existing = {
            f for f in os.listdir(HERE) if f.startswith("env-") and f.endswith(".json")
        } if os.path.isdir(HERE) else set()
        for name in sorted(set(want) | existing):
            full = os.path.join(HERE, name)
            if name not in want:
                bad.append(f"unexpected fixture: {name}")
            elif not os.path.isfile(full):
                bad.append(f"missing fixture: {name}")
            else:
                with open(full, "rb") as fh:
                    if fh.read() != want[name]:
                        bad.append(f"differs: {name}")
        if bad:
            for line in bad:
                sys.stderr.write(line + "\n")
            return 1
        return 0

    os.makedirs(HERE, exist_ok=True)
    for name, data in sorted(want.items()):
        with open(os.path.join(HERE, name), "wb") as fh:
            fh.write(data)
    return 0


if __name__ == "__main__":
    sys.exit(main())
