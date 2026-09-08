"""Check rejection coverage without storing prohibited fixtures."""
import contextlib
import io
import unittest

from lint_rules import TOKENS, check


class RuleTests(unittest.TestCase):
    def rejected(self, text):
        with contextlib.redirect_stdout(io.StringIO()):
            return check("fixture", text)

    def test_each_token(self):
        for token in TOKENS:
            with self.subTest(token=token):
                self.assertTrue(self.rejected(token))
                self.assertTrue(self.rejected(token.upper()))

    def test_host_locations(self):
        for text in ["/" + "Users/name/file", "/" + "home/name/file",
                     "/" + "Volumes/disk", "C:" + chr(92) + "work", "~" + "/file"]:
            self.assertTrue(self.rejected(text))

    def test_bare_home_prefix_is_allowed(self):
        # Code that refuses home-relative paths has to name the prefix.
        self.assertFalse(self.rejected('path.starts_with("~" + "/")'))

    def test_pictograph(self):
        self.assertTrue(self.rejected(chr(0x1F916)))

    def test_reference_text(self):
        self.assertFalse(self.rejected("The sequencer uses a bounded queue."))
        self.assertFalse(self.rejected("Copyright (c) Nylon Contributors"))


if __name__ == "__main__":
    unittest.main()
