import unittest

from textkit import slugify


class SlugifySmokeTest(unittest.TestCase):
    def test_lowercases_and_separates_words(self):
        self.assertEqual(slugify("Hello World"), "hello-world")


if __name__ == "__main__":
    unittest.main()
