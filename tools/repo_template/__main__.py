"""CLI entry point: ``python -m repo_template {new,promote,repin,selftest} ...``."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .new import DEFAULT_TEMPLATES, TemplateError, instantiate
from .promote import add_subparsers, run_promote_args, run_repin_args
from .selftest import add_selftest_subparser, run_selftest_args


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="python -m repo_template", description="Repository template and promotion tools.")
    sub = ap.add_subparsers(dest="command", required=True)

    new = sub.add_parser("new", help="create a repository from a template variant")
    new.add_argument("--id", required=True, help="repository ID of the new repository")
    new.add_argument("--variant", required=True, help="template variant (variants.toml), e.g. code or spec")
    new.add_argument("--rules-source", required=True, type=Path, help="house-rules source folder, checked out at --rules-rev")
    new.add_argument("--rules-rev", required=True, help="40-hex revision of the house-rules source")
    new.add_argument("--ci-rev", required=True, help="40-hex revision of the shared CI workflows")
    new.add_argument("--out", required=True, type=Path, help="new folder to create (must not exist, or be empty)")
    new.add_argument("--year", help="licence year (YYYY); needed when the rules source has no git commit for --rules-rev")
    new.add_argument("--templates", type=Path, default=DEFAULT_TEMPLATES, help=argparse.SUPPRESS)

    add_subparsers(sub)
    add_selftest_subparser(sub)

    args = ap.parse_args(argv)
    if args.command == "new":
        try:
            written = instantiate(
                args.id,
                args.variant,
                args.rules_source,
                args.rules_rev,
                args.ci_rev,
                args.out,
                args.templates,
                args.year,
            )
        except TemplateError as e:
            print(f"error: {e}", file=sys.stderr)
            return 1
        print(f"created {args.out} from variant {args.variant}: {len(written)} files")
        return 0
    if args.command == "promote":
        return run_promote_args(args)
    if args.command == "repin":
        return run_repin_args(args)
    if args.command == "selftest":
        return run_selftest_args(args)
    return 2


if __name__ == "__main__":
    sys.exit(main())
