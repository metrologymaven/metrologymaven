"""Acceptance tests of spec node FLOW.PERSONA.MAPS (AC-1 .. AC-3) for the nine persona journey maps
under acceptance/journeys/persona/, checked by tools/check_persona_maps.py.

Each error-path case copies the shipped maps into a temporary folder and makes one change. The
expected finding codes are hand-labelled from the rules of FLOW.PERSONA.MAPS, FLOW.PERSONA.SCHEMA
and FLOW.PERSONA.CHECK; they were not produced by running the checker.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import copy
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import check_persona_maps as cpm  # noqa: E402

MAPS = ROOT / "acceptance" / "journeys" / "persona"
PERSONAS = ROOT / "acceptance" / "personas" / "personas.json"
SPEC = ROOT / "spec"

# From the build prompt of FLOW.PERSONA.MAPS.
EXERCISED = {
    "JOB.PER.01": ["JOB.ACC.02", "JOB.ACC.03"],
    "JOB.PER.02": ["JOB.ACC.01", "JOB.ACC.07"],
    "JOB.PER.03": ["JOB.ACC.05"],
    "JOB.PER.04": ["JOB.ACC.04", "JOB.ACC.10"],
    "JOB.PER.05": ["JOB.ACC.02", "JOB.ACC.06"],
    "JOB.PER.06": [],
    "JOB.PER.07": ["JOB.ACC.08"],
    "JOB.PER.08": [],
    "JOB.PER.09": ["JOB.ACC.09"],
}


def load(name):
    return json.loads((MAPS / name).read_text(encoding="utf-8"))


class Variant:
    """A temporary copy of the shipped maps with one change."""

    def __init__(self):
        self.dir = Path(tempfile.mkdtemp(prefix="persona-maps-"))
        for f in MAPS.glob("*.json"):
            shutil.copy(f, self.dir / f.name)

    def write(self, name, data):
        (self.dir / name).write_text(json.dumps(data, indent=2), encoding="utf-8")

    def edit(self, name, fn):
        m = json.loads((self.dir / name).read_text(encoding="utf-8"))
        fn(m)
        self.write(name, m)

    def check(self):
        return cpm.check(self.dir, PERSONAS, SPEC)

    def close(self):
        shutil.rmtree(self.dir, ignore_errors=True)


def lines(findings):
    return [f.line() for f in findings]


class ShippedMaps(unittest.TestCase):
    def test_ac1_every_persona_map_validates(self):
        count, findings = cpm.check(MAPS, PERSONAS, SPEC)
        self.assertEqual(lines(findings), [])
        self.assertEqual(count, 9)

    def test_ac1_cli_exit_zero(self):
        r = subprocess.run([sys.executable, str(ROOT / "tools" / "check_persona_maps.py"), "--json"],
                           capture_output=True, text=True, cwd=ROOT)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        out = json.loads(r.stdout)
        self.assertEqual(out, {"maps": 9, "findings": []})

    def test_one_map_per_persona_file_names(self):
        ids = [p["id"] for p in json.loads(PERSONAS.read_text(encoding="utf-8"))["personas"]]
        self.assertEqual(len(ids), 9)
        for pid in ids:
            name = "per-" + pid[-2:] + ".json"
            m = load(name)
            self.assertEqual(m["subject"], pid)
            self.assertEqual(m["kind"], "persona")
            self.assertEqual(m["format"], "journey-map@1")
            self.assertEqual(m["map_id"], name[:-5])
        self.assertEqual(sorted(p.name for p in MAPS.glob("*.json")),
                         ["per-0%d.json" % i for i in range(1, 10)])

    def test_exercised_by_matches_build_prompt(self):
        for pid, jobs in EXERCISED.items():
            self.assertEqual(load("per-" + pid[-2:] + ".json")["exercised_by"], jobs, pid)

    def test_app_matches_persona_primary_app(self):
        for p in json.loads(PERSONAS.read_text(encoding="utf-8"))["personas"]:
            self.assertEqual(load("per-" + p["id"][-2:] + ".json")["app"], p["primary_app"], p["id"])

    def test_ac2_auditor_uses_only_verify_commands(self):
        m = load("per-06.json")
        for s in m["steps"]:
            self.assertIn("command", s, s["n"])
            self.assertTrue(s["command"].startswith("verify."), s["command"])
            self.assertNotEqual(s["command"], "verify.fetch_kernel@1")  # X1, network
            self.assertNotEqual(s["actor"], "agent")

    def test_handoffs_to_other_personas_name_them(self):
        # REQ-003: hand-offs between personas named in the map text carry the receiving persona ID.
        known = {"JOB.PER.0%d" % i for i in range(1, 10)}
        named = 0
        for f in MAPS.glob("*.json"):
            m = json.loads(f.read_text(encoding="utf-8"))
            for s in m["steps"]:
                if isinstance(s["handoff"], str):
                    for p in cpm.PER_IN_TEXT_RE.findall(s["handoff"]):
                        self.assertIn(p, known)
                        self.assertIn(p, m["personas"])
                        named += 1
        self.assertGreaterEqual(named, 8)
        # The example of the spec: the quality engineer hands the form to the quality manager.
        self.assertIn("JOB.PER.05", load("per-01.json")["steps"][9]["handoff"])

    def test_human_only_steps_are_never_agents(self):
        for f in MAPS.glob("*.json"):
            for s in json.loads(f.read_text(encoding="utf-8"))["steps"]:
                if s["human_only"]:
                    self.assertNotEqual(s["actor"], "agent", (f.name, s["n"]))
                cmd = s.get("command", "")
                if ".adopt_" in cmd or cmd in {"report.finalize_fai@1", "program.release@1",
                                               "program.register@1", "sign.sign_package@1",
                                               "sign.set_profile@1", "view.enter_kiosk@1",
                                               "cmm.approve_ticket@1"}:
                    self.assertTrue(s["human_only"], (f.name, s["n"], cmd))


class ErrorPaths(unittest.TestCase):
    def setUp(self):
        self.v = Variant()

    def tearDown(self):
        self.v.close()

    def assert_finding(self, expected):
        _, findings = self.v.check()
        self.assertIn(expected, lines(findings))
        return findings

    def test_ac3_unexercised_per_02(self):
        self.v.edit("per-02.json", lambda m: m.update(exercised_by=[]))
        _, findings = self.v.check()
        self.assertEqual(lines(findings), ["MAP-UNEXERCISED JOB.PER.02"])

    def test_ac3_cli_exit_one(self):
        self.v.edit("per-02.json", lambda m: m.update(exercised_by=[]))
        r = subprocess.run([sys.executable, str(ROOT / "tools" / "check_persona_maps.py"),
                            "--maps", str(self.v.dir)], capture_output=True, text=True, cwd=ROOT)
        self.assertEqual(r.returncode, 1)
        self.assertIn("MAP-UNEXERCISED JOB.PER.02", r.stdout.splitlines())

    def test_unexercised_allowed_for_06_and_08(self):
        _, findings = self.v.check()
        self.assertEqual(lines(findings), [])

    def test_missing_exercised_by_field(self):
        self.v.edit("per-04.json", lambda m: m.pop("exercised_by"))
        self.assert_finding("MAP-SCHEMA JOB.PER.04 exercised_by must be a list of JOB.ACC.* IDs")

    def test_missing_map(self):
        (self.v.dir / "per-07.json").unlink()
        _, findings = self.v.check()
        self.assertEqual(lines(findings), ["MAP-MISSING JOB.PER.07"])

    def test_duplicate_map(self):
        shutil.copy(self.v.dir / "per-03.json", self.v.dir / "per-03-copy.json")
        self.assert_finding("MAP-DUPLICATE JOB.PER.03 per-03-copy.json and per-03.json")

    def test_unknown_persona_map(self):
        def f(m):
            m["subject"] = "JOB.PER.10"
            m["personas"] = ["JOB.PER.10"]
        self.v.edit("per-09.json", f)
        _, findings = self.v.check()
        self.assertIn("MAP-EXTRA JOB.PER.10 per-09.json: no such persona", lines(findings))
        self.assertIn("MAP-MISSING JOB.PER.09", lines(findings))

    def test_wrong_kind(self):
        self.v.edit("per-01.json", lambda m: m.update(kind="job"))
        self.assert_finding("MAP-SCHEMA JOB.PER.01 kind must be persona")

    def test_handoff_to_unknown_persona(self):
        self.v.edit("per-01.json", lambda m: m["steps"][9].update(handoff="JOB.PER.11 releases it"))
        self.assert_finding("MAP-HANDOFF JOB.PER.01 step 10: unknown persona JOB.PER.11")

    def test_handoff_to_persona_not_listed(self):
        self.v.edit("per-09.json", lambda m: m["steps"][14].update(handoff="Report goes to JOB.PER.05"))
        self.assert_finding("MAP-HANDOFF JOB.PER.09 step 15: JOB.PER.05 is not in personas")

    def test_auditor_non_verify_command(self):
        def f(m):
            m["steps"][0].update(command="sign.list_meanings@1", owner="TRUST.ESIG.MEANING")
        self.v.edit("per-06.json", f)
        self.assert_finding("MAP-AUDITOR JOB.PER.06 step 1: sign.list_meanings@1")

    def test_auditor_network_command(self):
        # verify.fetch_kernel@1 starts with verify. but is X1 (network), which REQ-004 forbids.
        def f(m):
            m["steps"][3].update(command="verify.fetch_kernel@1", owner="TRUST.REPLAY.KART.FETCH")
        self.v.edit("per-06.json", f)
        self.assert_finding("MAP-AUDITOR JOB.PER.06 step 4: verify.fetch_kernel@1")

    def test_auditor_screen(self):
        def f(m):
            s = m["steps"][0]
            s.pop("command")
            s.update(screen="SCR.RPT.INTAKE", owner="UX.SCREENS.RPTAPP")
        self.v.edit("per-06.json", f)
        self.assert_finding("MAP-AUDITOR JOB.PER.06 step 1: screen SCR.RPT.INTAKE")

    def test_release_step_not_human_only(self):
        self.v.edit("per-05.json", lambda m: m["steps"][4].update(human_only=False))
        _, findings = self.v.check()
        self.assertEqual(lines(findings),
                         ["MAP-HUMANONLY-MISSING JOB.PER.05 step 5: report.finalize_fai@1 (W3)"])

    def test_admin_step_not_human_only(self):
        self.v.edit("per-03.json", lambda m: m["steps"][0].update(human_only=False))
        self.assert_finding("MAP-HUMANONLY-MISSING JOB.PER.03 step 1: view.enter_kiosk@1 (A0)")

    def test_adoption_by_agent(self):
        self.v.edit("per-02.json", lambda m: m["steps"][7].update(actor="agent"))
        _, findings = self.v.check()
        self.assertEqual(lines(findings), ["MAP-AGENT-HUMANONLY JOB.PER.02 step 8"])

    def test_adoption_not_human_only(self):
        self.v.edit("per-01.json", lambda m: m["steps"][9].update(human_only=False))
        self.assert_finding("MAP-HUMANONLY-MISSING JOB.PER.01 step 10: report.adopt_fai@1 (W2)")

    def test_step_gap(self):
        self.v.edit("per-07.json", lambda m: m["steps"][2].update(n=4))
        _, findings = self.v.check()
        self.assertEqual(lines(findings), ["MAP-STEP-ORDER JOB.PER.07 steps must be numbered 1..N in order"])

    def test_command_and_screen(self):
        self.v.edit("per-07.json", lambda m: m["steps"][2].update(command="verify.package@1"))
        self.assert_finding("MAP-SCHEMA JOB.PER.07 step 3: needs exactly one of command or screen")

    def test_unknown_owner(self):
        self.v.edit("per-08.json", lambda m: m["steps"][0].update(owner="HAL.NOPE"))
        self.assert_finding("MAP-OWNER-UNKNOWN JOB.PER.08 step 1: HAL.NOPE")

    def test_command_not_owned_by_owner(self):
        self.v.edit("per-08.json", lambda m: m["steps"][0].update(command="device.list_widgets@1"))
        self.assert_finding("MAP-COMMAND-UNKNOWN JOB.PER.08 step 1: device.list_widgets@1 "
                            "is not a command of HAL.IDL.GEN.CMDS")

    def test_unknown_screen(self):
        self.v.edit("per-03.json", lambda m: m["steps"][1].update(screen="SCR.KIOSK.NOPE"))
        self.assert_finding("MAP-SCREEN-UNKNOWN JOB.PER.03 step 2: SCR.KIOSK.NOPE is not named by "
                            "UX.SCREENS.FLOOR")

    def test_phase_before_owner(self):
        self.v.edit("per-05.json", lambda m: m["steps"][8].pop("phase"))
        self.assert_finding("MAP-PHASE JOB.PER.05 step 9: RPT.CATALOG.IMPORT.BATCH starts in P1b, step is P1a")

    def test_duplicate_json_key(self):
        raw = (self.v.dir / "per-01.json").read_text(encoding="utf-8")
        raw = raw.replace('"revision": 1,', '"revision": 1,\n  "revision": 2,', 1)
        (self.v.dir / "per-01.json").write_text(raw, encoding="utf-8")
        _, findings = self.v.check()
        self.assertIn("MAP-SCHEMA per-01.json duplicate key revision", lines(findings))
        self.assertIn("MAP-MISSING JOB.PER.01", lines(findings))

    def test_too_many_steps(self):
        def f(m):
            base = m["steps"][0]
            m["steps"] = []
            for i in range(1, 502):
                s = copy.deepcopy(base)
                s["n"] = i
                s["handoff"] = "next"
                m["steps"].append(s)
        self.v.edit("per-06.json", f)
        self.assert_finding("MAP-SIZE JOB.PER.06 501 steps > 500")


if __name__ == "__main__":
    unittest.main()
