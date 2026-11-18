"""Environment probe for the doc-run tests (OUTR.DOCS.CONTRIB).

Exit 3 when DOC_RUN_SENTINEL is visible (the checker must remove it), 4 when PATH is missing
(the checker must pass it through), else 0. Standard library only.
"""
import os
import sys

if "DOC_RUN_SENTINEL" in os.environ:
    sys.exit(3)
if "PATH" not in os.environ:
    sys.exit(4)
sys.exit(0)
