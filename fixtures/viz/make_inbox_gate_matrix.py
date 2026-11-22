#!/usr/bin/env python3
"""Generate and verify fixtures/viz/inbox-gate-matrix.csv for VIZ.INBOX.GATES.PLAN.

License: CC-BY-4.0 (synthetic matrix derived directly from REQ-VIZ.INBOX.GATES.PLAN-001..004:
3 profiles x protected or not x batch 1 or 5 x human or agent x ceremony wired or not = 48 rows).

Usage:
    python3 fixtures/viz/make_inbox_gate_matrix.py
    python3 fixtures/viz/make_inbox_gate_matrix.py --check
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CSV_PATH = ROOT / "inbox-gate-matrix.csv"

PROFILES = ("lean", "regulated", "assistive_only")
PROTECTED = (False, True)
COUNTS = (1, 5)
PRINCIPALS = ("human", "agent")
PROVIDERS = (False, True)


def expected(
    profile: str,
    target_protected: bool,
    command_count: int,
    principal: str,
    provider_present: bool,
) -> tuple[str, str]:
    # REQ-004: non-human principal is unavailable in every profile.
    if principal != "human":
        return ("unavailable", "human_required")
    # REQ-001: protected branch without a CeremonyProvider is unavailable.
    if target_protected and not provider_present:
        return ("unavailable", "ceremony_unavailable")

    steps: list[str] = []
    if command_count > 1:
        steps.append(f"ConfirmCount({command_count})")
    if profile == "regulated":
        steps.append("Reason")
    if target_protected:
        meanings = "AUTHORED+APPROVED" if profile == "regulated" else ""
        steps.append(f"Signature({meanings})")
    if profile == "regulated":
        # Regulated built-in sets program_release_by_other = always (TRUST.PROFILE.DEFS).
        steps.append("SecondPerson")
    return ("steps", ";".join(steps))


def render_csv() -> str:
    lines = [
        "# Synthetic gate-plan matrix for VIZ.INBOX.GATES.PLAN AC-1 (CC-BY-4.0).",
        "# 3 profiles x protected(false,true) x count(1,5) x principal(human,agent) x provider(false,true) = 48 rows.",
        "profile,target_protected,command_count,principal,provider_present,expected_kind,expected_value",
    ]
    for profile in PROFILES:
        for protected in PROTECTED:
            for count in COUNTS:
                for principal in PRINCIPALS:
                    for provider in PROVIDERS:
                        kind, val = expected(profile, protected, count, principal, provider)
                        lines.append(
                            f"{profile},{str(protected).lower()},{count},{principal},{str(provider).lower()},{kind},{val}"
                        )
    return "\n".join(lines) + "\n"


def main() -> int:
    want = render_csv()
    if "--check" in sys.argv[1:]:
        if not CSV_PATH.is_file():
            print(f"missing fixture: {CSV_PATH.name}", file=sys.stderr)
            return 1
        got = CSV_PATH.read_text(encoding="utf-8")
        if got != want:
            print(f"fixture drift: {CSV_PATH.name}", file=sys.stderr)
            return 1
        return 0

    CSV_PATH.write_text(want, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
