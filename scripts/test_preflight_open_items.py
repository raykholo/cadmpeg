#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_open_items.py``. These never run cargo."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "test_preflight", Path(__file__).with_name("test_preflight.py"))
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = harness
SPEC.loader.exec_module(harness)
check = harness.load("preflight_open_items")
open_prs = harness.load("preflight_open_prs")

PATH = "docs/formats/f3d-open-items.md"


def document(*entries: str) -> str:
    text = "# Open items\n\n## 1. Records\n\n"
    for entry in entries:
        text += f"### {entry}\n\n**Question.** Why?\n\n"
    return text


class OpenItemsCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item")})
        self.repo.branch()

    def results(self):
        return harness.run_check(check, self.repo.context())

    def test_new_free_id_passes(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                         "DR-90. Current revision")})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_id_taken_upstream_fails_with_next_free(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                         "DR-90. Current revision")})
        self.repo.checkout("upstream")
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                         "DR-90. Sketch member component", "DR-91. Sense")})
        self.repo.checkout("topic")
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn("next free is DR-92", results[0].details[0])

    def test_duplicate_within_file_fails(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                         "DR-89. Again")})
        self.assertEqual(harness.statuses(self.results()), ["FAIL"])

    def test_same_item_already_upstream_passes(self) -> None:
        entries = ("DR-88. Old item", "DR-89. Another item", "DR-90. Shared")
        self.repo.commit({PATH: document(*entries)})
        self.repo.checkout("upstream")
        self.repo.commit({PATH: document(*entries)})
        self.repo.checkout("topic")
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_wrong_section_and_skipped_number_warn(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item")
                          + "## 2. Decode\n\n### DE-01. Decode item\n"})
        self.repo.checkout("upstream")
        self.repo.git("merge", "-q", "topic")
        self.repo.checkout("topic")
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                        "DE-03. Misplaced decode")})
        with patch.object(open_prs, "claimed_ids", return_value={}):
            results = self.results()
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertTrue(any("expected section: decode" in d for d in results[0].details))
        self.assertTrue(any("skips DE-02" in d for d in results[0].details))

    def test_number_claimed_by_another_open_pull_is_not_a_gap(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                        "DR-91. After the claimed one")})
        with patch.object(open_prs, "claimed_ids", return_value={(PATH, "DR-90"): {7}}):
            self.assertEqual(harness.statuses(self.results()), ["PASS"])
        with patch.object(open_prs, "claimed_ids", return_value={}):
            self.assertEqual(harness.statuses(self.results()), ["WARN"])

    def test_gap_is_not_judged_without_the_open_pulls(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                        "DR-91. Skips one")})
        results = harness.run_check(check, self.repo.context(offline=True))
        self.assertEqual(harness.statuses(results), ["PASS"])

    def test_placement_checks_only_added_items(self) -> None:
        # Renaming a section leaves the existing items alone; only the new one is placed.
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item")
                          .replace("## 1. Records", "## 1. Record layouts")
                          + "## 2. Records\n\n### DR-90. New item\n"})
        with patch.object(open_prs, "claimed_ids", return_value={}):
            self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_contiguous_ids_in_their_section_pass(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                        "DR-90. New item", "DR-91. Next item")})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_next_free_keeps_zero_padding(self) -> None:
        self.assertEqual(check.next_free("GC", [("GC-01", "a", 1), ("GC-09A", "b", 2)]), "GC-10")


if __name__ == "__main__":
    unittest.main()
