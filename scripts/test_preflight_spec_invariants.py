#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_spec_invariants.py``. These never run cargo."""

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
check = harness.load("preflight_spec_invariants")

class SpecInvariants(harness.FixtureCase):
    def test_missing_check_warns_and_multiline_call_passes(self) -> None:
        spec = "docs/formats/example.md"
        parser = "crates/cadmpeg-codec-example/src/decode.rs"
        self.repo.commit({spec: "# Format\n", parser: ""})
        self.repo.branch()
        self.repo.commit({spec: "# Format\nThe `count` must be positive.\n"})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn("`count`", results[0].details[0])
        self.repo.write({parser: "fn parse() { require_value(\n record.count > 0,\n); }"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context(rev="HEAD"))),
                         ["WARN"])

    def test_invariant_forms_and_ignored_literals(self) -> None:
        for text in ("`count` must be positive", "`tag` is 0", "`slot` is −1 or positive",
                     "`kind` is one of A, B", "`tag` is `0`", "`slot` is `-1` or positive"):
            self.assertIsNotNone(check.INVARIANT.search(text), text)
        code = ('// require_value(comment)\nfn check_value(definition: u8) {}\n'
                'ensure!(record.actual, "literal"); refuse(bad(nested));')
        self.assertEqual(check.checked_names(code), {"record", "actual", "bad", "nested"})
        self.assertFalse(check.parser_path("crates/cadmpeg-codec-x/src/export.rs"))
        self.assertFalse(check.parser_path("crates/cadmpeg-codec-x/src/integration_tests.rs"))


if __name__ == "__main__":
    unittest.main()
