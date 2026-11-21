#!/usr/bin/env bash
# RPT.CATALOG.STORE.SCHEMA AC-5: the schema tests (AC-1 to AC-4) pass with networking disabled.
# Linux CI only (listed in ci/fixture-checks.txt): the test binary is built first, then run as the
# same user inside a new network namespace, after checking that the namespace really has no network.
set -euo pipefail
bin=$(cargo test -q -j 2 -p fid-catalog --test schema --no-run --message-format=json \
  | python3 -c 'import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "schema":
        print(m["executable"])')
test -n "$bin"
user=$(id -un)
cut() { sudo -E unshare --net -- sudo -E -u "$user" env "PATH=$PATH" "$@"; }
if cut curl -sS -m 10 -o /dev/null https://github.com; then echo "network still reachable"; exit 1; fi
cut "$bin" --test-threads 2
