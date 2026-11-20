"""Tests of tools/design_review_gate.py (spec node UX.SYSTEM.REVIEW, approver identity).

Offline: GitHub responses are hand-written dicts; no network and no `gh` call. Expected results
are hand-labelled from the wrapper's rules. Run from the repository root:
python -m unittest discover -s tools/tests -v
"""

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import design_review_gate as g  # noqa: E402

HEAD = "a" * 40
OLD = "b" * 40


def review(login, state, commit=HEAD, kind="User"):
    return {"user": {"login": login, "type": kind}, "state": state, "commit_id": commit}


class ApprovedLogins(unittest.TestCase):
    def test_only_latest_non_bot_approval_of_head_counts(self):
        reviews = [
            review("Alice", "APPROVED"),
            review("bob", "APPROVED", commit=OLD),  # approved an older commit
            review("carol", "APPROVED"),
            review("carol", "CHANGES_REQUESTED"),  # later review withdraws it
            review("dave", "CHANGES_REQUESTED"),
            review("dave", "APPROVED"),
            review("dave", "COMMENTED"),  # a comment keeps the approval
            review("helper[bot]", "APPROVED"),
            review("ci-app", "APPROVED", kind="Bot"),
            review("erin", "APPROVED"),
            review("erin", "DISMISSED"),
        ]
        self.assertEqual(g.approved_logins(reviews, HEAD), {"alice", "dave"})

    def test_empty_reviews_approve_nobody(self):
        self.assertEqual(g.approved_logins([], HEAD), set())


class Verify(unittest.TestCase):
    def test_approver_must_match_a_verified_login_case_insensitively(self):
        report = {
            "approvals": [
                {"record": "review/records/a.json", "approver": "ALICE"},
                {"record": "review/records/b.json", "approver": "mallory"},
            ]
        }
        out = g.verify(report, {"alice"})
        self.assertEqual(len(out), 1)
        self.assertTrue(out[0].startswith("REVIEW-APPROVER-UNVERIFIED review/records/b.json"))

    def test_no_records_no_findings(self):
        self.assertEqual(g.verify({"approvals": []}, set()), [])


class ChangedPaths(unittest.TestCase):
    FILES = [
        {"filename": "design/icons/probe.svg", "status": "modified"},
        {"filename": "design/review/records/pr-9.json", "status": "added"},
        {"filename": "ui/src/panel.tsx", "status": "modified"},
        {"filename": "design/icons/new.svg", "previous_filename": "design/icons/old.svg", "status": "renamed"},
        {"filename": "designs-other/x.txt", "status": "added"},
    ]

    def test_relative_to_the_design_directory(self):
        self.assertEqual(
            g.changed_paths(self.FILES, "design", False),
            ["icons/new.svg", "icons/old.svg", "icons/probe.svg", "review/records/pr-9.json"],
        )

    def test_relative_to_the_repository_with_base(self):
        self.assertEqual(
            g.changed_paths(self.FILES, "design", True),
            [
                "design/icons/new.svg",
                "design/icons/old.svg",
                "design/icons/probe.svg",
                "design/review/records/pr-9.json",
                "designs-other/x.txt",
                "ui/src/panel.tsx",
            ],
        )


class Run(unittest.TestCase):
    def test_no_design_change_passes_without_running_the_check(self):
        calls = []

        def fake(path):
            calls.append(path)
            if path.endswith("/files"):
                return [{"filename": "README.md"}]
            return {"head": {"sha": HEAD}}

        args = type("A", (), {"repo": "o/r", "pr": 7, "root": "design", "base": False, "fid_design": "none"})()
        self.assertEqual(g.run(args, gh=fake), 0)
        self.assertEqual(calls, ["repos/o/r/pulls/7", "repos/o/r/pulls/7/files"])


if __name__ == "__main__":
    unittest.main()
