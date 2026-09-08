import copy
import importlib.util
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("review_corpus", Path(__file__).with_name("review-corpus.py"))
REVIEW = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REVIEW)


class ReviewTest(unittest.TestCase):
    def setUp(self):
        self.diagnostic = {"file": "Makefile", "rule": "MK006", "line": 3, "column": 1}
        self.audit = {"schema_version": 1, "binary_sha256": "binary", "extra_rules": [], "external_variables": [],
                      "projects": {"example": {"revision": "revision", "inputs": {"Makefile": "content"}, "failures": [],
                          "files": [{"file": "Makefile", "input_sha256": "content", "check": {"stdout": json.dumps([self.diagnostic])}}]}}}
        self.labels = {"schema_version": 1, "profile": {"extra_rules": [], "external_variables": []}, "cases": [
            {"id": "case", "project": "example", "revision": "revision", "root": "Makefile", "root_sha256": "content",
             "file": "Makefile", "file_sha256": "content", "rule": "MK006", "line": 3, "column": 1,
             "classification": "false_positive", "reason": "Controlled GNU Make case accepts the inactive branch."}]}

    def test_review_keeps_unreviewed_and_absent_cases_explicit(self):
        root = self.audit["projects"]["example"]["files"][0]
        root["check"]["stdout"] = json.dumps([self.diagnostic, dict(self.diagnostic, line=9)])
        result = REVIEW.review(self.audit, self.labels)
        self.assertEqual(result["unreviewed_findings"], 1)
        self.assertEqual(result["present_classifications"], {"false_positive": 1})
        root["check"]["stdout"] = "[]"
        result = REVIEW.review(self.audit, self.labels)
        self.assertEqual(result["reviewed_findings_absent"], 1)
        self.assertEqual(result["present_classifications"], {})

    def test_stale_sources_profiles_and_failed_audits_cannot_be_scored(self):
        for key in ["revision", "root_sha256", "file_sha256"]:
            labels = copy.deepcopy(self.labels)
            labels["cases"][0][key] = "changed"
            with self.assertRaises(ValueError):
                REVIEW.review(self.audit, labels)
        for key in ["extra_rules", "external_variables"]:
            audit = copy.deepcopy(self.audit)
            audit[key] = ["different"]
            with self.assertRaises(ValueError):
                REVIEW.review(audit, self.labels)
        self.audit["projects"]["example"]["failures"] = ["mutation"]
        with self.assertRaises(ValueError):
            REVIEW.review(self.audit, self.labels)

    def test_duplicate_reviews_and_ambiguous_diagnostics_fail(self):
        labels = copy.deepcopy(self.labels)
        labels["cases"].append(copy.deepcopy(labels["cases"][0]))
        with self.assertRaises(ValueError):
            REVIEW.review(self.audit, labels)
        self.audit["projects"]["example"]["files"][0]["check"]["stdout"] = json.dumps([self.diagnostic] * 2)
        with self.assertRaises(ValueError):
            REVIEW.review(self.audit, self.labels)
