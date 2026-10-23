# SPDX-License-Identifier: Apache-2.0
"""Warn on new public IR struct fields with no name in the IR validation code.

The validation code is every `validate*.rs` file of `cadmpeg-ir` and every Rust
file under a `validate*/` module directory, tests excluded. Field names are
compared per struct against the merge base. A name in a validation file is only
a review hint; this check cannot prove validation.
"""

from __future__ import annotations

import re
from pathlib import PurePosixPath

from preflight import is_test_path
from preflight_ir_version import Tree, item_end, strip_comments

NAME = "ir-validation"
ORDER = 25
PREFIX = "crates/cadmpeg-ir/"
STRUCT = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+(\w+)\b")
FIELD = re.compile(r"\bpub\s+(\w+)\s*:")


def fields(text: str) -> set[tuple[str, str]]:
    lines = strip_comments(text).splitlines()
    found = set()
    for start, line in enumerate(lines):
        match = STRUCT.match(line)
        if match:
            body = "\n".join(lines[start:item_end(lines, start) + 1])
            found.update((match.group(1), field) for field in FIELD.findall(body))
    return found


def validation_file(path: str) -> bool:
    """A `validate*.rs` file of the IR crate, or any Rust file under a `validate*/` module."""
    parts = PurePosixPath(path).parts
    return path.endswith(".rs") and any(part.startswith("validate") for part in parts[3:])


def run(context):
    paths = [path for path in context.changed_paths()
             if path.startswith(PREFIX) and path.endswith(".rs") and not is_test_path(path)]
    if not paths:
        return context.passed("no IR Rust source changed")
    tree = Tree(context)
    before = set()
    after = {}
    for path in paths:
        before.update(fields(context.read_base(path) or ""))
        after[path] = fields(context.read(path) or "")
    added = {path: values - before for path, values in after.items()}
    if not any(added.values()):
        return context.passed("no new public IR struct fields")
    arguments = (["ls-tree", "-r", "--name-only", context.rev] if context.rev else
                 ["ls-files", "-co", "--exclude-standard"])
    validators = sorted({path for path in context.git(*arguments, "--", PREFIX).stdout.splitlines()
                         if validation_file(path) and not is_test_path(path)})
    texts = (tree.blobs(context.rev, validators) if context.rev else
             {path: context.read(path) or "" for path in validators})
    validation = "\n".join(texts.values())
    details = [f"{path}: {struct}.{field}: no field name in the validation code"
               for path, values in added.items() for struct, field in sorted(values)
               if not re.search(r"\b" + re.escape(field) + r"\b", validation)]
    if details:
        return context.warned("new public IR fields may need validation", *details)
    return context.passed("new public IR field names occur in the validation code")
