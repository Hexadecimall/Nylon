"""Checks that the regression rule separates a slowdown from runner noise."""
import unittest

from benchmark_compare import quantile, regression


class SeparationRule(unittest.TestCase):
    def test_noise_around_the_threshold_is_not_a_regression(self):
        # Overlapping samples whose medians differ by more than five per
        # cent: the shape of a noisy shared runner.
        baseline = [40.0, 41.0, 42.0, 43.0, 44.0, 45.0, 46.0, 47.0, 48.0]
        candidate = [42.0, 43.0, 44.0, 45.0, 46.0, 47.0, 48.0, 49.0, 50.0]
        self.assertFalse(regression(baseline, candidate))

    def test_a_clear_slowdown_is_a_regression(self):
        baseline = [40.0, 40.5, 41.0, 41.5, 42.0]
        candidate = [60.0, 60.5, 61.0, 61.5, 62.0]
        self.assertTrue(regression(baseline, candidate))

    def test_a_separated_shift_below_the_threshold_is_allowed(self):
        baseline = [100.0, 100.1, 100.2]
        candidate = [102.0, 102.1, 102.2]
        self.assertFalse(regression(baseline, candidate))

    def test_identical_samples_are_not_a_regression(self):
        sample = [40.0] * 21
        self.assertFalse(regression(sample, sample))


class Quantiles(unittest.TestCase):
    def test_endpoints_and_interpolation(self):
        values = [1.0, 2.0, 3.0, 4.0]
        self.assertEqual(quantile(values, 0.0), 1.0)
        self.assertEqual(quantile(values, 1.0), 4.0)
        self.assertAlmostEqual(quantile(values, 0.5), 2.5)

    def test_a_single_measurement(self):
        self.assertEqual(quantile([7.0], 0.5), 7.0)


if __name__ == "__main__":
    unittest.main()
