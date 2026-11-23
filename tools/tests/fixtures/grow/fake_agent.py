#!/usr/bin/env python3
"""Fake agent fixture for ECO.COMM.GROW AC-2/AC-4 tests.

Writes 'ok', sleeps 0.2 s, and records concurrency and canary-ordering events
into the directory pointed to by GROW_TEST_STATE_DIR (if set).
"""

from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path


def main() -> int:
    state_dir_env = os.environ.get("GROW_TEST_STATE_DIR")
    pid = os.getpid()
    start_ns = time.time_ns()

    active_file = None
    active_at_start = 0
    canary_done_at_start = False

    if state_dir_env:
        state_dir = Path(state_dir_env)
        active_dir = state_dir / "active"
        events_dir = state_dir / "events"
        active_dir.mkdir(parents=True, exist_ok=True)
        events_dir.mkdir(parents=True, exist_ok=True)

        canary_marker = state_dir / "canary_done"
        canary_done_at_start = canary_marker.exists()

        active_file = active_dir / f"{pid}-{start_ns}.run"
        active_file.write_text(str(pid), encoding="utf-8")
        active_at_start = len(list(active_dir.iterdir()))

    print("ok", flush=True)
    time.sleep(0.2)

    end_ns = time.time_ns()
    if state_dir_env:
        state_dir = Path(state_dir_env)
        if active_file is not None:
            try:
                active_file.unlink()
            except OSError:
                pass
        canary_marker = state_dir / "canary_done"
        if not canary_marker.exists():
            canary_marker.write_text(str(end_ns), encoding="utf-8")
        event = {
            "pid": pid,
            "argv": sys.argv[1:],
            "start_ns": start_ns,
            "end_ns": end_ns,
            "active_at_start": active_at_start,
            "canary_done_at_start": canary_done_at_start,
        }
        event_file = state_dir / "events" / f"{start_ns}-{pid}.json"
        event_file.write_text(json.dumps(event), encoding="utf-8")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
