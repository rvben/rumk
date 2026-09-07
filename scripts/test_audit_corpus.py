"""Integration checks for the corpus auditor; build rumk before running."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("audit_corpus", ROOT / "scripts/audit-corpus.py")
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)
BINARY = Path(os.environ.get("RUMK_TEST_BINARY", ROOT / "target/debug/rumk")).resolve()


class CorpusAuditTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="rumk-audit-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        # An empty template and hooks directory isolate local Git configuration.
        template = self.root / "empty-template"
        template.mkdir()
        self.repo = self.root / "project"
        subprocess.run(["git", "init", "--template", str(template), str(self.repo)],
                       check=True, capture_output=True)
        self.git("config", "core.hooksPath", str(template))
        (self.repo / "Makefile").write_text(
            "NEVER := $(shell touch side-effect)\ninclude tasks.mk\n.PHONY: all\nall:;\n")
        (self.repo / "tasks.mk").write_text("CC=cc\n")
        self.git("add", "Makefile", "tasks.mk")
        self.git("-c", "user.name=Corpus Test", "-c", "user.email=corpus@example.invalid",
                 "-c", "commit.gpgsign=false", "commit", "-m", "test: prepare corpus fixture")

    def git(self, *args):
        return AUDIT.git(self.repo, *args)

    def test_real_binary_is_repeatable_and_never_executes_make(self):
        result = AUDIT.audit(BINARY, self.repo, 2, 10)
        self.assertEqual(result["failures"], [])
        self.assertEqual(len(result["files"]), 2)
        self.assertTrue(any(file["format_changed"] for file in result["files"]))
        self.assertEqual(len(result["files"][0]["seconds"]), 2)
        self.assertFalse((self.repo / "side-effect").exists())
        for file in result["files"]:
            self.assertIsInstance(json.loads(file["check"]["stdout"]), list)
        self.assertEqual(self.git("status", "--porcelain"), b"")

    def test_dirty_sources_are_rejected(self):
        (self.repo / "Makefile").write_text("changed\n")
        with self.assertRaisesRegex(RuntimeError, "must be clean"):
            AUDIT.audit(BINARY, self.repo, 1, 10)

    def test_opt_in_rule_is_checked_on_disk_and_stdin(self):
        (self.repo / "Makefile").write_text("probe: missing.txt\n")
        self.git("add", "Makefile")
        self.git("-c", "user.name=Corpus Test", "-c", "user.email=corpus@example.invalid",
                 "-c", "commit.gpgsign=false", "commit", "-m", "test: add absent prerequisite")
        result = AUDIT.audit(BINARY, self.repo, 1, 10, ["MK216"])
        self.assertEqual(result["failures"], [])
        findings = [d for file in result["files"] for d in json.loads(file["check"]["stdout"])
                    if d["rule"] == "MK216"]
        self.assertEqual(len(findings), 1)
        self.assertIn("missing.txt", findings[0]["message"])

    def test_audit_detects_new_side_effects_and_unstable_output(self):
        original = AUDIT.invoke
        checks = 0

        def faulty(*args, **kwargs):
            nonlocal checks
            result, elapsed = original(*args, **kwargs)
            (self.repo / "unexpected").write_text("mutation")
            if args[2][0] == "check":
                checks += 1
                result.stderr = str(checks).encode()
            return result, elapsed

        with patch.object(AUDIT, "invoke", side_effect=faulty):
            result = AUDIT.audit(BINARY, self.repo, 1, 10)
        self.assertIn("Project files changed during read-only audit", result["failures"])
        self.assertTrue(any("unstable diagnostics" in failure for failure in result["failures"]))
        self.assertTrue(any("disk/stdin" in failure for failure in result["failures"]))


if __name__ == "__main__":
    unittest.main()
