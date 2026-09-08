"""Callback hazard detection checks."""
import unittest
from rt_lint import hazards, strip_control


class CallbackChecks(unittest.TestCase):
    def test_rejects_allocator_and_lock_operations(self):
        for source in ["Vec::new()", "std::fs::read(name)", "Mutex::new(0)",
                       "value.clone()", "println!(value)", 'extern "C" {}']:
            self.assertEqual(hazards(source), [1])

    def test_accepts_borrowed_processing(self):
        self.assertEqual(hazards("output[0] = input[0] * gain;"), [])

    def test_marked_regions_are_exempt_but_keep_line_numbers(self):
        source = "\n".join([
            "Vec::new()",
            "// off the audio thread",
            "Box::new(value)",
            "// back on the audio thread",
            "Mutex::new(0)",
        ])
        self.assertEqual(hazards(strip_control("f.rs", source)), [1, 5])

    def test_an_unclosed_region_is_refused(self):
        with self.assertRaises(SystemExit):
            strip_control("f.rs", "// off the audio thread\nBox::new(1)")

    def test_a_region_closed_twice_is_refused(self):
        source = "\n".join([
            "// off the audio thread",
            "// back on the audio thread",
            "// back on the audio thread",
        ])
        with self.assertRaises(SystemExit):
            strip_control("f.rs", source)


if __name__ == "__main__":
    unittest.main()
