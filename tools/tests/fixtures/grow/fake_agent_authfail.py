#!/usr/bin/env python3
"""Fake agent fixture simulating authentication failure for ECO.COMM.GROW AC-3."""

from __future__ import annotations

import os
import sys
import time
from pathlib import Path


def main() -> int:
    state_dir_env = os.environ.get("GROW_TEST_STATE_DIR")
    if state_dir_env:
        events_dir = Path(state_dir_env) / "events"
        events_dir.mkdir(parents=True, exist_ok=True)
        marker = events_dir / f"{time.time_ns()}-{os.getpid()}.txt"
        marker.write_text(" ".join(sys.argv[1:]), encoding="utf-8")
    print("Invalid API key", flush=True)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
