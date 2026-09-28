"""Coverage accounting and read-only tooling integration."""
import importlib.util
import os
from pathlib import Path
import tempfile
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("coverage_audit", ROOT / "scripts/audit-prerequisite-coverage.py")
COVERAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COVERAGE)


class CoverageTest(unittest.TestCase):
    def test_overlapping_blockers_count_roots_separately_from_occurrences(self):
        report = COVERAGE.summarize([
            {"coverage": {"root_blockers": {"unknown_activity": 7, "unresolved_rule": 2},
                          "outcomes": {"root_excluded": 1}, "edges": [{}]}},
            {"coverage": {"root_blockers": {}, "outcomes": {"missing": 1}, "edges": [{}],
                          "local_exclusions": [{"source": "Makefile", "line": 1,
                                                "reason": "unresolved_prerequisites"}]}},
        ])
        self.assertEqual(report["eligible_roots"], 1)
        self.assertEqual(report["roots_with_local_exclusions"], 1)
        self.assertEqual(report["local_exclusions"], 1)
        self.assertEqual(report["roots_by_blocker"]["unknown_activity"], 1)
        self.assertEqual(report["blocker_occurrences"]["unknown_activity"], 7)
        self.assertEqual(report["visible_edges"], 2)

    def test_ranking_distinguishes_sole_blockers_from_overlapping_exclusions(self):
        def root(blockers, edges):
            return {"coverage": {"root_blockers": blockers,
                                 "outcomes": {"root_excluded": edges}, "edges": [{}] * edges}}
        report = COVERAGE.summarize([
            root({"shell_function": 20, "unknown_activity": 3}, 10),
            root({"unknown_activity": 1, "shell_function": 4}, 5),
            root({"suffix_rule": 1}, 2),
            root({}, 0),
        ])
        self.assertEqual(report["sole_blocker_roots"], {"suffix_rule": 1})
        self.assertEqual(report["blocker_sets"], [
            {"blockers": ["shell_function", "unknown_activity"], "roots": 2, "visible_edges": 15},
            {"blockers": ["suffix_rule"], "roots": 1, "visible_edges": 2},
        ])
        self.assertEqual(report["eligible_roots"], 1)

    def test_example_is_repeatable_and_never_executes_make(self):
        binary = Path(os.environ.get("RUMK_COVERAGE_BINARY", ROOT / "target/debug/examples/prerequisite-coverage")).resolve()
        if not binary.exists():
            self.skipTest("Build --example prerequisite-coverage first")
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            subprocess.run(["git", "init", "--template=", str(root)], check=True, capture_output=True)
            (root / "Makefile").write_text("VALUE := $(shell touch side-effect)\nprobe: missing\n")
            subprocess.run(["git", "-C", temp, "add", "Makefile"], check=True)
            subprocess.run(["git", "-C", temp, "-c", "user.name=Corpus Test", "-c", "user.email=corpus@example.invalid",
                            "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null",
                            "commit", "-m", "test: fixture"], check=True, capture_output=True)
            result = COVERAGE.audit(binary, root, 10)
            self.assertEqual(result["summary"]["eligible_roots"], 0)
            self.assertFalse((root / "side-effect").exists())
