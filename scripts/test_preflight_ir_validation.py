#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_ir_validation.py``. These never run cargo."""

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
check = harness.load("preflight_ir_validation")

class IrValidation(harness.FixtureCase):
    def test_new_struct_field_without_validation_warns(self) -> None:
        path = "crates/cadmpeg-ir/src/body.rs"
        validate = "crates/cadmpeg-ir/src/validate_model.rs"
        self.repo.commit({path: "pub struct Body { pub old: u32 }\n",
                          validate: "fn check() { body.old; }\n"})
        self.repo.branch()
        self.repo.commit({path: "pub struct Body { pub old: u32, pub count: u32 }\n"})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn("Body.count", results[0].details[0])
        self.repo.write({validate: "fn check() { body.count; }\n"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context(rev="HEAD"))),
                         ["WARN"])

    def test_enum_fields_and_private_fields_are_excluded(self) -> None:
        text = "pub enum Kind { Value { pub ignored: u32 } }\nstruct A { private: u32 }"
        self.assertEqual(check.fields(text), set())

    def test_field_move_does_not_add_a_field(self) -> None:
        old = "crates/cadmpeg-ir/src/old.rs"
        new = "crates/cadmpeg-ir/src/new.rs"
        text = "pub struct Body { pub count: u32 }\n"
        self.repo.commit({old: text})
        self.repo.branch()
        self.repo.commit({old: None, new: text})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])


if __name__ == "__main__":
    unittest.main()
