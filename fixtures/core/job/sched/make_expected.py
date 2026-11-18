#!/usr/bin/env python3
"""Generate fixtures/core/job/sched/mixed_queue.expected.json (CORE.JOB.SCHED AC-1).

Independent reference for the start order of the jobs in mixed_queue.json. It is a
discrete-event simulation of the leaf's rules, written from the spec text alone
(REQ-CORE.JOB.SCHED-003, -004), not from the Rust code:

- at most `limit` jobs run at once; the rest wait;
- a free slot goes to the oldest queued interactive job, else the oldest queued batch job;
- all jobs are submitted before any handler may proceed (the Rust test holds every
  handler at a gate until the last submission), so submission itself takes no time.
  A job that finds a free slot at submission starts at once;
- a running job ends `sleep_ms` after it started; jobs ending at the same time free
  their slots in start order (the start order does not depend on this tie rule here).

CC-BY-4.0 project data.
Run:  python fixtures/core/job/sched/make_expected.py
"""

import json
from collections import deque
from pathlib import Path

here = Path(__file__).parent
spec = json.loads((here / "mixed_queue.json").read_text(encoding="utf-8"))
limit = spec["limit"]

queues = {"interactive": deque(), "batch": deque()}
running = []  # (end_time, start_index, name)
start_order = []
now = 0


def start_free_slots():
    while len(running) < limit:
        if queues["interactive"]:
            job = queues["interactive"].popleft()
        elif queues["batch"]:
            job = queues["batch"].popleft()
        else:
            return
        running.append((now + job["sleep_ms"], len(start_order), job["name"]))
        start_order.append(job["name"])


for job in spec["jobs"]:
    assert job["class"] in queues
    queues[job["class"]].append(job)
    start_free_slots()

while running:
    running.sort()
    end, _, _ = running.pop(0)
    now = end
    start_free_slots()

assert len(start_order) == len(spec["jobs"])
doc = {
    "description": "Start order for mixed_queue.json, from make_expected.py. CC-BY-4.0.",
    "start_order": start_order,
}
out = here / "mixed_queue.expected.json"
out.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")
print(out, start_order)
