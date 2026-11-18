# Fixtures for ECO.REPO.TPL.FILES

Licence: CC-BY-4.0. Hand-written; the expected results in
`tools/repo_template/tests/test_repo_template.py` are hand-labelled from the spec rules.

- `rules/`: a small house-rules source (same manifest format as ECO.RULES.SRC). `fid-demo`
  is assigned `core` and `rust`; `python` belongs to another repository.
- `bad-placeholder/`: a template root whose only variant contains the unknown placeholder
  `{{owner}}` (AC-2).
- `nonempty-out/`: an output folder that already has a file (AC-3).
