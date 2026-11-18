## House rules (Python)

These rules hold in the spec repository.

### Standard library only for spec tooling

Spec-tree and house-rules tools use the Python standard library only, so they run in every repository's CI without installing packages or a Rust toolchain.

### Run the spec check

After changing the spec tree, run `python tools/scope.py check`, fix errors in the files you touched, and commit any regenerated files under `generated/`.
