# SPDX-License-Identifier: Apache-2.0
"""Warn on added spec invariants with no matching parser or validation call.

A code identifier beside must, is 0, is -1 or, or is one of should occur in
an argument to a require/refuse/ensure/check call. Names alone cannot prove
that the call enforces the condition, so this check emits WARN only.
"""

from __future__ import annotations

import re

from preflight import is_test_path
from preflight_ir_version import STRING, Tree, strip_comments

NAME = "spec-invariants"
ORDER = 85
SPEC = re.compile(r"^docs/formats/(?!.*open-items)[^/]+\.md$")
INVARIANT = re.compile(r"\bmust\b|\bis\s+`?0\b|\bis\s+`?[−-]1`?\s+or\b|"
                       r"\bis\s+one\s+of\b", re.IGNORECASE)
IDENTIFIER = re.compile(r"`([A-Za-z_]\w*)`")
CALL = re.compile(r"\b\w*(?:require|refuse|ensure|check)\w*!?\s*\(")


def checked_names(text: str) -> set[str]:
    text = STRING.sub('""', strip_comments(text))
    found = set()
    for match in CALL.finditer(text):
        if re.search(r"\bfn\s+$", text[max(0, match.start() - 10):match.start()]):
            continue
        start = match.end()
        depth = 1
        end = start
        while end < len(text) and depth:
            depth += {"(": 1, ")": -1}.get(text[end], 0)
            end += 1
        if depth == 0:
            found.update(re.findall(r"\b[A-Za-z_]\w*\b", text[start:end - 1]))
    return found


def parser_path(path: str) -> bool:
    if not path.endswith(".rs") or is_test_path(path) or "/src/" not in path:
        return False
    if any("validate" in part for part in path.split("/")):
        return True
    return (path.startswith("crates/cadmpeg-codec-")
            and not re.search(r"/(?:encod\w*|export\w*|writ\w*)(?:[/.])", path))


def run(context):
    candidates = []
    for path in context.changed_paths():
        if SPEC.match(path):
            for number, text in context.added_lines(path):
                if INVARIANT.search(text):
                    candidates.extend((path, number, name) for name in IDENTIFIER.findall(text))
    if not candidates:
        return context.passed("no added spec invariants with code identifiers")
    arguments = (["ls-tree", "-r", "--name-only", context.rev] if context.rev else
                 ["ls-files", "-co", "--exclude-standard"])
    paths = sorted({path for path in context.git(*arguments, "--", "crates/").stdout.splitlines()
                    if parser_path(path)})
    texts = (Tree(context).blobs(context.rev, paths) if context.rev else
             {path: context.read(path) or "" for path in paths})
    names = set()
    for text in texts.values():
        names.update(checked_names(text))
    details = [f"{path}:{number}: `{name}` has no require/refuse/ensure/check call argument"
               for path, number, name in candidates if name not in names]
    if details:
        return context.warned("spec invariants may need parser or validation checks", *details)
    return context.passed("added spec invariant identifiers occur in check calls")
