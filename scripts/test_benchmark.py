"""Performance threshold checks."""
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

import benchmark_compare
from benchmark_compare import Unbuildable, regression


class BenchmarkChecks(unittest.TestCase):
    def test_five_percent_boundary(self):
        self.assertFalse(regression([100] * 9, [105] * 9))
        self.assertTrue(regression([100] * 9, [105.01] * 9))
        self.assertFalse(regression([100] * 9, [90] * 9))

    def test_median_rejects_an_isolated_spike(self):
        self.assertFalse(regression([100] * 9, [100] * 8 + [1000]))

    def test_invalid_measurements_fail(self):
        for sample in [0, -1, float("nan"), float("inf")]:
            with self.assertRaises(ValueError):
                regression([sample], [100])



class BaselineChecks(unittest.TestCase):
    def test_a_baseline_that_does_not_build_is_skipped(self):
        # The check exists to catch a slowdown in the change under test.
        # A revision behind it that does not compile gives nothing to
        # measure, so it is reported rather than failing the run.
        def refuse(root, name):
            if pathlib.Path(root) != pathlib.Path.cwd():
                raise Unbuildable("baseline")
            raise AssertionError("the candidate should not be built")

        with mock.patch.object(benchmark_compare, "build", refuse):
            self.assertFalse(benchmark_compare.compare(pathlib.Path("/nowhere"), "render", "ns"))

    def test_a_candidate_that_does_not_build_still_fails(self):
        def refuse(root, name):
            if pathlib.Path(root) == pathlib.Path.cwd():
                raise Unbuildable("candidate")
            return "/nonexistent/baseline"

        with mock.patch.object(benchmark_compare, "build", refuse):
            with self.assertRaises(Unbuildable):
                benchmark_compare.compare(pathlib.Path("/nowhere"), "render", "ns")

    def test_a_failed_build_command_becomes_an_unbuildable_revision(self):
        with tempfile.TemporaryDirectory() as directory:
            def fail(*args, **kwargs):
                raise subprocess.CalledProcessError(101, args[0] if args else "cargo")

            with mock.patch.object(benchmark_compare.subprocess, "run", lambda *a, **k: None):
                with mock.patch.object(benchmark_compare.subprocess, "check_output", fail):
                    with self.assertRaises(Unbuildable):
                        benchmark_compare.build(pathlib.Path(directory), "render")


if __name__ == "__main__":
    unittest.main()
