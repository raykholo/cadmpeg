#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_version_layouts.py``. These never run cargo."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "test_preflight", Path(__file__).with_name("test_preflight.py"))
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = harness
SPEC.loader.exec_module(harness)
check = harness.load("preflight_version_layouts")

class VersionLayouts(harness.FixtureCase):
    def test_only_records_in_the_version_section_warn(self) -> None:
        spec = "docs/formats/example.md"
        table = "docs/layouts/example.toml"
        text = "# Format\n## 1. Header\nHeader bytes.\n## 2. Body\nBody bytes.\n"
        layout = ('spec = "docs/formats/example.md"\n'
                  '[[record]]\nname = "header"\nsection = "1"\nanchor = "Header bytes."\n'
                  '[[record]]\nname = "body"\nsection = "2"\nanchor = "Body bytes."\n')
        self.repo.commit({spec: text, table: layout})
        self.repo.branch()
        text = text.replace("Header bytes.", "From version 8, fields follow.\nHeader bytes.")
        self.repo.commit({spec: text})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertEqual(len(results[0].details), 1)
        self.assertIn("header", results[0].details[0])
        self.repo.write({table: layout.replace('anchor = "Header bytes."',
                                              'anchor = "From version 8, fields follow."')})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])

    def test_other_paragraphs_and_plural_versions_pass(self) -> None:
        spec = "docs/formats/example.md"
        table = "docs/layouts/example.toml"
        text = "# Format\n## 1. Header\nHeader bytes.\n\nTrailer bytes.\n"
        layout = ('spec = "docs/formats/example.md"\n'
                  '[[record]]\nname = "header"\nsection = "1"\nanchor = "Header bytes."\n'
                  '[[record]]\nname = "trailer"\nsection = "1"\n'
                  'anchor = "In versions 8 and 9 trailer bytes."\n')
        self.repo.commit({spec: text, table: layout})
        self.repo.branch()
        text = text.replace("Trailer bytes.", "From version 8, a flag follows.\n"
                            "In versions 8 and 9 trailer bytes.")
        self.repo.commit({spec: text})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["PASS"], results)

    def test_version_scope_phrases(self) -> None:
        for phrase in ("from version 8", "in version 8 and later", "segment major version",
                       "In segment major versions 15 through 22", "before version 28"):
            self.assertIsNotNone(check.SCOPE.search(phrase))


if __name__ == "__main__":
    unittest.main()
