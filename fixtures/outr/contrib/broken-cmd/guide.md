---
title: Broken command (fixture for OUTR.DOCS.CONTRIB AC-2)
revision: @HEAD@
---

# Broken command

A documented error example. It exits non-zero, as its block declares, so it passes.

```sh run expect-fail
python tools/scope.py no-such-verb
```

The same command in an ordinary runnable block fails the check at the block on line 16.

```sh run
python tools/scope.py no-such-verb
```
