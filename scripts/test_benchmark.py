"""Performance threshold checks."""
import unittest
from benchmark_compare import regression


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


if __name__ == "__main__":
    unittest.main()
