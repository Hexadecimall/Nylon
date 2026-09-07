"""Callback hazard detection checks."""
import unittest
from rt_lint import hazards


class CallbackChecks(unittest.TestCase):
    def test_rejects_allocator_and_lock_operations(self):
        for source in ["Vec::new()", "std::fs::read(name)", "Mutex::new(0)",
                       "value.clone()", "println!(value)", 'extern "C" {}']:
            self.assertEqual(hazards(source), [1])

    def test_accepts_borrowed_processing(self):
        self.assertEqual(hazards("output[0] = input[0] * gain;"), [])


if __name__ == "__main__":
    unittest.main()
