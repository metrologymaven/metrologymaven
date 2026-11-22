"""Fixture generator for VIZ.RENDER.FRAME.TIER (fixtures/viz/adapters.json).

Independent stdlib-only reference generator. Produces synthetic, project-authored recorded
adapter feature/limit sets covering all four capability tiers (T0, T1, T2, T3) and computes
each case's expected tier, point-raster path and transparency path directly from the spec
rules (REQ-VIZ.RENDER.FRAME.TIER-001, -002), never calling the Rust crate.

    python fixtures/viz/make_adapters.py          # write fixtures/viz/adapters.json
    python fixtures/viz/make_adapters.py --check  # fail if fixtures/viz/adapters.json differs
"""

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_PATH = os.path.join(HERE, "adapters.json")


def classify(features, limits, headless_server):
    supports_compute = (
        features["compute_shaders"]
        and limits["max_compute_workgroups_per_dimension"] > 0
        and limits["max_storage_buffers_per_shader_stage"] > 0
    )
    supports_atomic64 = (
        features["shader_int64_atomic_min_max"] or features["shader_int64_atomic_all_ops"]
    )
    if headless_server:
        tier = "T3"
    elif supports_compute and supports_atomic64:
        tier = "T0"
    elif supports_compute:
        tier = "T1"
    else:
        tier = "T2"
    point_raster = {"T0": "compute64", "T1": "twopass32", "T2": "sprites", "T3": "sprites"}[tier]
    transparency = {"T0": "wboit", "T1": "wboit", "T2": "sorted", "T3": "peel"}[tier]
    return tier, point_raster, transparency


def case(
    case_id,
    backend,
    adapter,
    driver,
    *,
    atomic_min_max=False,
    atomic_all=False,
    compute=True,
    workgroups=65535,
    storage_buffers=8,
    max_buffer_size=268435456,
    headless_server=False,
):
    features = {
        "compute_shaders": compute,
        "shader_int64_atomic_all_ops": atomic_all,
        "shader_int64_atomic_min_max": atomic_min_max,
    }
    limits = {
        "max_buffer_size": max_buffer_size,
        "max_compute_workgroups_per_dimension": workgroups,
        "max_storage_buffers_per_shader_stage": storage_buffers,
    }
    tier, point_raster, transparency = classify(features, limits, headless_server)
    return {
        "adapter": adapter,
        "backend": backend,
        "driver": driver,
        "expected_point_raster": point_raster,
        "expected_tier": tier,
        "expected_transparency": transparency,
        "features": features,
        "headless_server": headless_server,
        "id": case_id,
        "limits": limits,
    }


def build() -> bytes:
    doc = {
        "schema_version": 1,
        "adapters": [
            case(
                "vulkan-discrete-atomic64-minmax",
                "vulkan",
                "Reference Vulkan Discrete GPU",
                "Mesa RADV 25.0.0",
                atomic_min_max=True,
                compute=True,
                max_buffer_size=1073741824,
            ),
            case(
                "dx12-discrete-atomic64-all",
                "dx12",
                "Reference DX12 Discrete GPU",
                "WDDM 3.2 (32.0.15.6000)",
                atomic_all=True,
                compute=True,
                max_buffer_size=1073741824,
            ),
            case(
                "metal-integrated-compute-no-atomic64",
                "metal",
                "Reference Apple Integrated GPU",
                "Metal 3.2",
                atomic_min_max=False,
                compute=True,
                max_buffer_size=536870912,
            ),
            case(
                "webgpu-browser-compute-only",
                "webgpu",
                "Reference WebGPU Adapter",
                "WebGPU 1.0",
                atomic_min_max=False,
                compute=True,
                max_buffer_size=268435456,
            ),
            case(
                "software-warp-compute-no-atomic64",
                "software",
                "Microsoft Basic Render Driver",
                "WARP 10.0",
                atomic_min_max=False,
                compute=True,
                max_buffer_size=268435456,
            ),
            case(
                "software-lavapipe-atomic64",
                "software",
                "llvmpipe (LLVM 19.1, 256 bits)",
                "Mesa 25.0.0 (LLVM 19.1)",
                atomic_min_max=True,
                compute=True,
                max_buffer_size=268435456,
            ),
            case(
                "gl-webgl2-no-compute",
                "gl",
                "Reference WebGL2 / GLES 3.0 Adapter",
                "OpenGL ES 3.0",
                atomic_min_max=False,
                compute=False,
                workgroups=0,
                storage_buffers=0,
                max_buffer_size=134217728,
            ),
            case(
                "gl-zero-workgroups-demoted-to-t2",
                "gl",
                "Reference GL With Zero Compute Workgroups",
                "OpenGL 4.1",
                atomic_min_max=False,
                compute=True,
                workgroups=0,
                storage_buffers=4,
                max_buffer_size=134217728,
            ),
            case(
                "atomic64-without-compute-demoted-to-t2",
                "vulkan",
                "Reference Adapter Without Compute Shaders",
                "Synthetic 1.0",
                atomic_min_max=True,
                compute=False,
                workgroups=0,
                storage_buffers=0,
                max_buffer_size=134217728,
            ),
            case(
                "headless-server-stills-vulkan",
                "vulkan",
                "Reference Server GPU (Headless Stills)",
                "Mesa RADV 25.0.0",
                atomic_min_max=True,
                compute=True,
                max_buffer_size=1073741824,
                headless_server=True,
            ),
            case(
                "headless-server-stills-software",
                "software",
                "llvmpipe Headless Stills",
                "Mesa 25.0.0",
                atomic_min_max=False,
                compute=True,
                max_buffer_size=268435456,
                headless_server=True,
            ),
        ],
    }
    return (json.dumps(doc, indent=2, sort_keys=True) + "\n").encode("utf-8")


def main():
    check = "--check" in sys.argv[1:]
    data = build()
    if check:
        try:
            with open(OUT_PATH, "rb") as f:
                if f.read() == data:
                    print("1 fixture file matches (fixtures/viz/adapters.json)")
                    return 0
        except OSError:
            pass
        print("stale or missing: fixtures/viz/adapters.json", file=sys.stderr)
        print("run: python fixtures/viz/make_adapters.py", file=sys.stderr)
        return 1
    with open(OUT_PATH, "wb") as f:
        f.write(data)
    print("1 fixture file written (fixtures/viz/adapters.json)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
