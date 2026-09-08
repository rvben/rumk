"""Ground truth, scorer integrity, and safe-fix contracts for authored defects."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("semantic", Path(__file__).with_name("semantic-benchmark.py"))
SEMANTIC = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SEMANTIC)


class ScoringTest(unittest.TestCase):
    def test_baseline_ignores_timings_but_not_findings_or_fixes(self):
        one = {"tools": {"checkmake": {"stdout": '[{"rule":"a"},{"rule":"b"}]', "seconds": [1]}}}
        two = {"tools": {"checkmake": {"stdout": '[{"rule":"b"},{"rule":"a"}]', "seconds": [2]}}}
        self.assertEqual(SEMANTIC.stable_report(one), SEMANTIC.stable_report(two))
        two["tools"]["checkmake"]["stdout"] = '[{"rule":"a"}]'
        self.assertNotEqual(SEMANTIC.stable_report(one), SEMANTIC.stable_report(two))
        self.assertNotEqual(SEMANTIC.stable_report({"inputs_after": {"Makefile": "old"}}),
                            SEMANTIC.stable_report({"inputs_after": {"Makefile": "new"}}))

    def test_unrelated_warnings_and_exit_codes_do_not_count(self):
        case = {"rule": "MK205", "matchers": {"checkmake": "uniquetargets", "unmake": r": WD_NOP:"}}
        result = {"exit_code": 1, "stdout": '[{"rule":"MK201","file":"Makefile"}]', "stderr": ""}
        self.assertFalse(SEMANTIC.diagnostic_match("rumk", result, case))
        result["stdout"] = '[{"rule":"MK205","file":"Makefile"}]'
        self.assertTrue(SEMANTIC.diagnostic_match("rumk", result, case))
        result["stdout"] = '[{"rule":"minphony","file_name":"Makefile"}]'
        self.assertFalse(SEMANTIC.diagnostic_match("checkmake", result, case))
        result["stdout"] = 'warning: Makefile: MAKEFILE_PRECEDENCE: lowercase Makefile'
        self.assertFalse(SEMANTIC.diagnostic_match("unmake", result, case))
        result["stdout"] = 'warning: Makefile:2: WD_NOP: directory lost'
        self.assertTrue(SEMANTIC.diagnostic_match("unmake", result, case))

    def test_source_validation_errors_are_distinct_from_crashes(self):
        value = {"exit_code": 2, "stdout": "Makefile:4: Error: Duplicate target 'probe'", "stderr": ""}
        self.assertTrue(SEMANTIC.diagnostic_exit("mbake", value))
        self.assertFalse(SEMANTIC.diagnostic_exit("rumk", value))
        self.assertFalse(SEMANTIC.diagnostic_exit("mbake", dict(value, stdout="Traceback")))

    def test_oracle_checks_behavior_not_only_status(self):
        result = {"exit_code": 0, "stdout": "wrong\n", "stderr": "Circular dependency"}
        self.assertFalse(SEMANTIC.contract_matches(result, {"exit_code": 0, "stdout": "expected\n"}))
        self.assertFalse(SEMANTIC.contract_matches(result, {"stderr_absent": "Circular"}))
        self.assertTrue(SEMANTIC.contract_matches(result, {"stderr_contains": "Circular"}))


class SemanticIntegrationTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.make = os.environ.get("GNU_MAKE") or shutil.which("make")
        if not cls.make or "GNU Make" not in subprocess.check_output([cls.make, "--version"], text=True):
            raise unittest.SkipTest("GNU Make is required for semantic probes")
        cls.binary = Path(os.environ.get("RUMK_TEST_BINARY", SEMANTIC.ROOT / "target/debug/rumk")).resolve()
        if not cls.binary.exists():
            raise unittest.SkipTest(f"Build the selected Rumk binary first: {cls.binary}")

    def test_pairs_and_safe_fixes_with_one_fixed_lint_profile(self):
        report = SEMANTIC.benchmark({"rumk": str(self.binary)}, self.make, 1)
        self.assertEqual(report["failures"], [])
        cases = {case["name"]: case for case in json.loads(SEMANTIC.MANIFEST.read_text())["cases"]}
        for row in report["results"]:
            case = cases[row["name"]]
            with self.subTest(case=row["name"]):
                broken = row["tools"]["rumk"]["broken"]
                working = row["tools"]["rumk"]["working"]
                self.assertEqual(broken["detected"], case["expected_rumk_detection"])
                self.assertEqual(working["detected"], case.get("expected_rumk_control_flag", False))
                fix = broken["safe_fix"]
                if case["safe_fix"] == "repair":
                    self.assertTrue(fix["changed"])
                    self.assertTrue(fix["working_contract_met"])
                else:
                    self.assertTrue(fix["original_contract_preserved"])
                    self.assertFalse(fix["changed"])

    def test_external_names_clear_optional_controls_without_hiding_defects(self):
        report = SEMANTIC.benchmark({"rumk": str(self.binary)}, self.make, 1, external_variables=["TESTS"])
        self.assertEqual(report["failures"], [])
        self.assertEqual(report["external_variables"], ["TESTS"])
        cases = {case["name"]: case for case in json.loads(SEMANTIC.MANIFEST.read_text())["cases"]}
        for row in report["results"]:
            with self.subTest(case=row["name"]):
                self.assertEqual(row["tools"]["rumk"]["broken"]["detected"], cases[row["name"]]["expected_rumk_detection"])
                self.assertFalse(row["tools"]["rumk"]["working"]["detected"])
        optional = next(row for row in report["results"] if row["name"] == "recipe-only-compiler-settings")
        self.assertEqual(json.loads(optional["tools"]["rumk"]["working"]["stdout"]), [])

    def test_broad_pattern_can_feed_a_builtin_after_direct_match_fails(self):
        case = {
            "rule": "MK216", "matchers": {}, "builtin_rules": True,
            "args": ["-n", "probe"],
            "files": {"templates/output.c.src": "input"},
            "working": "probe: generated/output.o\n\t@echo built\ngenerated/%: templates/%.src\n\t@echo generated\n",
        }
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            SEMANTIC.populate(directory, case, "working")
            result = SEMANTIC.oracle(self.make, directory, case)
            self.assertTrue(SEMANTIC.contract_matches(result, {
                "exit_code": 0, "stdout_contains": "generated/output.c",
            }), result)
            result = SEMANTIC.invoke(SEMANTIC.commands("rumk", str(self.binary)), directory)
            self.assertFalse(SEMANTIC.diagnostic_match("rumk", result, case))

    def test_explicit_command_intent_repairs_collision_only_with_unsafe_opt_in(self):
        case = next(c for c in json.loads(SEMANTIC.MANIFEST.read_text())["cases"]
                    if c["name"] == "custom-phony-collision")
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            SEMANTIC.populate(directory, case, "broken", ["verify"])
            command = SEMANTIC.commands("rumk", str(self.binary))
            result = SEMANTIC.invoke(command, directory)
            self.assertTrue(SEMANTIC.diagnostic_match("rumk", result, case))
            before = (directory / "Makefile").read_bytes()
            SEMANTIC.invoke(command + ["--fix"], directory)
            self.assertEqual(before, (directory / "Makefile").read_bytes())
            SEMANTIC.invoke(command + ["--fix", "--unsafe-fixes"], directory)
            self.assertTrue(SEMANTIC.contract_matches(SEMANTIC.oracle(self.make, directory, case), case["oracle"]["working"]))
            self.assertFalse(SEMANTIC.diagnostic_match("rumk", SEMANTIC.invoke(command, directory), case))


if __name__ == "__main__":
    unittest.main()
