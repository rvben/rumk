"""Regression coverage for competitor output nondeterminism."""
import importlib.util
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("compare_linters", Path(__file__).with_name("compare-linters.py"))
COMPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARE)


class ComparisonTest(unittest.TestCase):
    def test_checkmake_order_does_not_hide_changed_or_duplicate_diagnostics(self):
        one = {"rule": "minphony", "line_number": 2, "violation": "missing test"}
        two = {"rule": "phonydeclared", "line_number": 2, "violation": "all is not phony"}

        def result(items, tool="checkmake"):
            return {"case": "unknown-include", "tool": tool, "exit_code": 1,
                    "stdout": json.dumps(items), "stderr": "", "inputs": {"Makefile": "hash"}}

        original = result([one, two])
        raw = original["stdout"]
        self.assertEqual(COMPARE.observation(original), COMPARE.observation(result([two, one])))
        self.assertEqual(original["stdout"], raw)
        for changed in [[one], [one, two, two], [one, dict(two, line_number=3)],
                        [one, dict(two, violation="different")]]:
            self.assertNotEqual(COMPARE.observation(original), COMPARE.observation(result(changed)))
        self.assertNotEqual(COMPARE.observation(result([one, two], "rumk")),
                            COMPARE.observation(result([two, one], "rumk")))


if __name__ == "__main__":
    unittest.main()
