import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("benchmark_linters", Path(__file__).with_name("benchmark-linters.py"))
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)


class BenchmarkTest(unittest.TestCase):
    def test_measures_one_child_and_keeps_its_status_and_output(self):
        with tempfile.TemporaryDirectory() as directory:
            result = BENCH.measured([sys.executable, "-c", "print('probe'); raise SystemExit(7)"], Path(directory), 5)
        self.assertEqual(result["stdout"], "probe\n")
        self.assertEqual(result["exit_code"], 7)
        self.assertGreater(result["seconds"], 0)
        if result["peak_rss_bytes"] is not None:
            self.assertGreater(result["peak_rss_bytes"], 0)

    def test_missing_memory_is_not_reported_as_zero(self):
        report = BENCH.summary([{"seconds": 1, "peak_rss_bytes": 100, "exit_code": 0},
                                {"seconds": 3, "peak_rss_bytes": None, "exit_code": 1}])
        self.assertEqual(report["median_seconds"], 2)
        self.assertIsNone(report["median_peak_rss_bytes"])
        self.assertEqual(report["exit_codes"], [0, 1])

    def test_checkmake_order_is_irrelevant_but_duplicates_are_not(self):
        self.assertEqual(BENCH.stable_stdout("checkmake", '[{"a":1},{"a":2}]'), BENCH.stable_stdout("checkmake", '[{"a":2},{"a":1}]'))
        self.assertNotEqual(BENCH.stable_stdout("checkmake", '[{"a":1},{"a":1}]'), BENCH.stable_stdout("checkmake", '[{"a":1}]'))

    def test_timeout_is_retained_and_never_scored_as_a_completed_run(self):
        with tempfile.TemporaryDirectory() as directory:
            result = BENCH.measured([sys.executable, "-c", "import time; time.sleep(10)"], Path(directory), 0.05)
        self.assertTrue(result["timed_out"])
        self.assertIsNone(result["exit_code"])
        report = BENCH.summary([result])
        self.assertIsNone(report["median_seconds"])
        self.assertIsNone(report["median_peak_rss_bytes"])
        self.assertEqual(report["timed_out_samples"], 1)

    def test_failed_executable_launch_has_no_performance_score(self):
        report = BENCH.summary([{"seconds": 0.001, "peak_rss_bytes": 100, "exit_code": 127}])
        self.assertIsNone(report["median_seconds"])
        self.assertIsNone(report["median_peak_rss_bytes"])
        self.assertEqual(report["launch_failure_samples"], 1)
