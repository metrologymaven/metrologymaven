# Replay

Every committed command is written to the semantic journal.

## Exact mode

> Re-running the same inputs under exact mode yields bit-identical outputs.

- First check: compare journal hashes.
- Second check: compare output digests.

1. Open the project archive.
2. Run the verifier offline.
