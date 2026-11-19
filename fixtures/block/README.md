# Fixtures for ECO.CI.BOTS.BLOCK (`cargo xtask bot-blocking`)

Hand-written thread exports, register and maintainer list (MIT OR Apache-2.0). Every name is
fictional. The `expected-*.txt` files are the exact stdout, hand-labelled from the leaf's
rules (REQ-001..005), not produced by running the tool:

| Export | Exit | Why |
|---|---|---|
| `threads-advisory.json` | 0 | AC-1: two bot threads, no marks |
| `threads-blocking.json` | 1 | AC-2: maintainer `ada-quill` replied `/blocking`, unresolved |
| `threads-unmarked.json` | 0 | AC-3: the same maintainer later replied `/advisory` |
| `threads-resolved.json` | 0 | AC-3: blocking but resolved |
| `threads-non-maintainer.json` | 0 | AC-4: `cora-vale` is not a maintainer; one `ignored-mark` note |
| `threads-truncated.json` | 1 | AC-5: cut off mid-string, `BLOCK-INPUT` |
| `threads-mixed.json` | 1 | Sorting by ID; latest maintainer mark wins; a human-opened thread never blocks (REQ-004); marks by a non-maintainer and by a bot are notes; `line: null` prints `-`; a mark only counts as the first token |
