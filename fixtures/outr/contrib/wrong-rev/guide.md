---
title: Wrong revision (fixture for OUTR.DOCS.CONTRIB AC-4)
revision: 0000000000000000000000000000000000000000
---

# Wrong revision

This guide states a revision that is never the checked-out commit, so the checker must stop
with `revision_mismatch` before it runs the block below.

```sh run
python tools/scope.py no-such-verb
```
