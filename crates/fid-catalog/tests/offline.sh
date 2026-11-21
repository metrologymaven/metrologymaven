#!/usr/bin/env bash
# RPT.CATALOG.STORE.SCHEMA AC-5: the schema tests (AC-1 to AC-4) pass with networking disabled.
# Linux CI only (listed in ci/fixture-checks.txt): the test binary is built first, then run as the
# same user inside a new network namespace. Before that, a positive control shows the probe reaches
# the network outside the namespace; inside it the probe must fail and `lo` must be the only link.
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
probe=(curl -sS -m 10 -o /dev/null https://github.com)
# Positive control: the same probe succeeds where the network is not cut.
if ! "${probe[@]}"; then echo "positive control failed: the probe cannot reach the network"; exit 1; fi
if cut "${probe[@]}"; then echo "network still reachable inside the namespace"; exit 1; fi
links=$(cut ip -o link | awk -F': ' '{print $2}')
if [ "$links" != "lo" ]; then echo "unexpected links in the namespace: $links"; exit 1; fi
cut "$bin" --test-threads 2
