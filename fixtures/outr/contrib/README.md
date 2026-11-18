# Fixtures for OUTR.DOCS.CONTRIB (`fid-outreach doc-run`)

Hand-written; the expected results are hand-labelled from the spec's rules in
`crates/fid-outreach/tests/doc_run.rs`, never produced by running the checker.

| Fixture | What it is | Expected |
|---|---|---|
| `broken-cmd/guide.md` | An `sh run expect-fail` block (line 10) and an `sh run` block (line 16), both calling `python tools/scope.py no-such-verb` | Exit 1; the block on line 10 passes, the block on line 16 fails at its command on line 17, and the output names `guide.md:16` |
| `wrong-rev/guide.md` | States revision `0000000000000000000000000000000000000000` | Exit 1 with `revision_mismatch`, no block run |

`broken-cmd/guide.md` states `revision: @HEAD@`, a placeholder: a guide can never contain
the ID of the commit that adds it, so the test copies the guide to a temporary file with the
placeholder replaced by `git rev-parse HEAD` before it runs the checker. Run directly, the
checker rejects the placeholder as an invalid revision (exit 2).

`scripts/env_probe.py` (exit 3 if `DOC_RUN_SENTINEL` is visible, 4 if `PATH` is missing, else 0)
and `scripts/sleep.py` (sleeps 30 s) are helper scripts for the environment and timeout tests.
