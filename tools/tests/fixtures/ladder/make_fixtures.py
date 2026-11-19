#!/usr/bin/env python3
"""Build the recorded history fixture for tools/ladder.py (spec node ECO.COMM.LADDER).

Writes git-log.txt: what `git log --format=%H%x00%an%x00%ae%x00%cI%x00%s` prints for a small
fictional history (newest first, fixed dates and authors, fake but well-formed hashes). The
expected results in tools/tests/test_ladder.py were labelled by hand from the spec's rules and
the comments below, not by running the tool.

    python tools/tests/fixtures/ladder/make_fixtures.py          # (re)write git-log.txt
    python tools/tests/fixtures/ladder/make_fixtures.py --check  # exit 1 if it is out of date

All names are fictional. Hand labels, with --now 2026-12-01T00:00:00Z and the default config:
  alice: 5 GDT expansions                                  -> eligible in GDT
  bob:   3 ALN expansions (+5 substantive ALN reviews)     -> eligible in ALN; 1 GDT expansion only
  carol: 6 KRN expansion commits; her KRN.SOLVE split reverted
         by dave on 2026-11-21 (10 days before --now), so
         it never counts and 5 remain                     -> not eligible (eligible at 2027-01-30)
  erin:  3 IO expansions as erin@example.net and 2 as the noreply login erin-dev
                                                           -> eligible in IO only when aliased
  dave:  noise (merge, lower-case ID, malformed ID, a revert of a split not in the log)
"""
import hashlib
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "git-log.txt"

ALICE = ("Alice Example", "1001+alice@users.noreply.github.com")
BOB = ("Bob Example", "1002+bob@users.noreply.github.com")
CAROL = ("Carol Example", "1003+carol@users.noreply.github.com")
DAVE = ("Dave Example", "1004+dave@users.noreply.github.com")
ERIN_MAIL = ("Erin Example", "Erin@Example.net")
ERIN_GH = ("Erin Example", "1005+erin-dev@users.noreply.github.com")

# (committer date as git %cI prints it, author, subject)
COMMITS = [
    ("2026-09-01T10:00:00+00:00", ALICE, "spec(GDT.TOL): split into 3 children"),
    ("2026-09-08T10:00:00+00:00", ALICE, "spec(GDT.TOL.FORM): leaf"),
    ("2026-09-15T10:00:00+00:00", ALICE, "spec(GDT.DAT): split into 2 children"),
    ("2026-09-22T12:00:00+02:00", ALICE, "spec(GDT.DAT.REF): blocked on D-012"),
    ("2026-10-01T10:00:00+00:00", ALICE, "spec(GDT.SYM): leaf"),
    ("2026-10-02T10:00:00+00:00", ALICE, "spec(GDT.SYM): reword purpose"),
    ("2026-10-03T10:00:00+00:00", ALICE, "docs: fix typo in README"),
    ("2026-09-03T09:00:00+00:00", BOB, "spec(ALN.RPS): split into 4 children"),
    ("2026-09-10T09:00:00+00:00", BOB, "spec(ALN.RPS.ITER): leaf"),
    ("2026-10-05T09:00:00+00:00", BOB, "spec(ALN.BEST): blocked on D-020"),
    ("2026-10-06T09:00:00+00:00", BOB, "spec(GDT.SYM.FLAT): leaf"),
    ("2026-09-02T08:00:00+00:00", CAROL, "spec(KRN.MESH): split into 2 children"),
    ("2026-09-09T08:00:00+00:00", CAROL, "spec(KRN.MESH.TRI): leaf"),
    ("2026-09-16T08:00:00+00:00", CAROL, "spec(KRN.MESH.QUAD): leaf"),
    ("2026-09-23T08:00:00+00:00", CAROL, "spec(KRN.CURVE): leaf"),
    ("2026-09-30T08:00:00+00:00", CAROL, "spec(KRN.SURF): blocked on D-031"),
    ("2026-10-10T08:00:00+00:00", CAROL, "spec(KRN.SOLVE): split into 4 children"),
    ("2026-11-21T00:00:00+00:00", DAVE, 'Revert "spec(KRN.SOLVE): split into 4 children"'),
    ("2026-11-22T00:00:00+00:00", DAVE, "Merge pull request #250 from example/branch"),
    ("2026-11-23T00:00:00+00:00", DAVE, "spec(gdt.lower): leaf"),
    ("2026-11-24T00:00:00+00:00", DAVE, "spec(.GDT): leaf"),
    ("2026-11-25T00:00:00+00:00", DAVE, 'Revert "spec(ALN.GONE): split into 2 children"'),
    ("2026-09-05T07:00:00+00:00", ERIN_MAIL, "spec(IO.STEP): split into 3 children"),
    ("2026-09-12T07:00:00+00:00", ERIN_MAIL, "spec(IO.STEP.P21): leaf"),
    ("2026-09-19T07:00:00+00:00", ERIN_MAIL, "spec(IO.QIF): leaf"),
    ("2026-09-26T07:00:00+00:00", ERIN_GH, "spec(IO.DMIS): leaf"),
    ("2026-10-03T07:00:00+00:00", ERIN_GH, "spec(IO.LAS): leaf"),
]


def fake_sha(i: int) -> str:
    return hashlib.sha1(f"ladder-fixture-{i}".encode("ascii")).hexdigest()


def render() -> bytes:
    rows = [(when, fake_sha(i), who, subject) for i, (when, who, subject) in enumerate(COMMITS)]
    # git log prints newest first; the fixed dates make this order total.
    rows.sort(key=lambda r: r[0][:19], reverse=True)
    lines = [f"{sha}\x00{name}\x00{email}\x00{when}\x00{subject}\n" for when, sha, (name, email), subject in rows]
    return "".join(lines).encode("utf-8")


def main(argv) -> int:
    data = render()
    if "--check" in argv:
        if not OUT.is_file() or OUT.read_bytes() != data:
            print(f"{OUT} is out of date; run {Path(__file__).name}", file=sys.stderr)
            return 1
        return 0
    OUT.write_bytes(data)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
