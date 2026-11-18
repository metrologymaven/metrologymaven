"""Blocks until killed; the doc-run timeout test relies on the checker killing it.

It never finishes on its own within any block time limit a test uses, so the timeout outcome
does not depend on how fast the machine is. The one-hour cap only bounds a stray process if
the checker failed to kill it. Standard library only.
"""
import time

for _ in range(3600):
    time.sleep(1)
