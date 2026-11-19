"""Command line: ``python -m house_rules <subcommand> ...`` (run with ``tools`` on ``sys.path``).

Subcommands:
- ``check``: house-rules check (ECO.RULES.CHECK), see :mod:`house_rules.check`.
"""

import sys

from . import check

SUBCOMMANDS = {
    "check": check.main,
}


def main(argv):
    if not argv or argv[0] not in SUBCOMMANDS:
        sys.stdout.write("RULES-USAGE python -m house_rules {" + ",".join(sorted(SUBCOMMANDS)) + "} ...\n")
        return 1
    return SUBCOMMANDS[argv[0]](argv[1:])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
