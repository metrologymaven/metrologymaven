<!-- Generated from crates/fid-cli/src/exit.rs (spec node AUTO.CLI.OUT). Do not edit by hand. -->
<!-- Regenerate: FID_BLESS=1 cargo test -p fid-cli --test out -->

# `fid` exit codes

Every `fid` command exits with exactly one of these codes, so scripts and CI can branch on the
outcome without parsing text. With `--json`, stdout holds exactly one result envelope
(`schema/envelope-v1.json`) and everything else goes to stderr.

| Code | Name | Meaning |
|---:|---|---|
| 0 | `success` | The command committed, computed or completed a dry run (outcome `committed`, `computed` or `dry_run`). |
| 1 | `internal_error` | Internal error: outcome `failed` with `internal_error`, an envelope that could not be encoded or written, or an outcome the CLI cannot end on (`accepted` or unknown). |
| 2 | `usage_error` | Usage error: the invocation was malformed (`usage_error`, `dry_run_unsupported`). Nothing ran. |
| 3 | `policy_refused` | Refused by side-effect policy (`policy_refused`, `raw_path_refused`), including M1, S1 and A0 commands refused at the CLI. Nothing ran. |
| 4 | `failed` | The command failed with any other error code. |
| 5 | `cancelled` | The command was cancelled (outcome `cancelled`, or `failed` with `cancelled`). |
| 6 | `not_run` | Timeout with outcome `not_run`: the command did not start. |
| 7 | `no_reply` | Timeout with outcome `no_reply`: the command may or may not have run; check the journal. |
| 8 | `request_filed` | A request was filed for a human (outcome `request_filed`); the command has not run yet. |
