# SPDX-License-Identifier: Apache-2.0
"""Warn when version-scoped specification sections have unqualified layout anchors.

An added version condition in a format specification applies to a record
whose anchor sits in the same paragraph of a section the record names, such as
the sentence that introduces its table. Each such record needs a version in
its anchor. This is a text heuristic; every finding is a WARN.
"""

from __future__ import annotations

import re
import tomllib

NAME = "version-layouts"
ORDER = 45
SPEC = re.compile(r"^docs/formats/(?!.*open-items)[^/]+\.md$")
SCOPE = re.compile(r"\b(?:from version\s+`?\d+|before version\s+`?\d+|"
                   r"in versions?\s+`?\d+`?\s+(?:and later|through|to)|"
                   r"segment major versions?)\b", re.IGNORECASE)
QUALIFIED = re.compile(r"\b(?:versions?\s+`?\d+|v\d+)\b", re.IGNORECASE)
HEADING = re.compile(r"^(#{1,6})\s+(.*)")


def section_spans(lines: list[str]) -> list[tuple[int, int, str]]:
    headings = [(index, len(match.group(1)), match.group(2))
                for index, line in enumerate(lines) if (match := HEADING.match(line))]
    spans = [(0, len(lines), "")]
    for index, (start, level, title) in enumerate(headings):
        end = next((at for at, depth, _ in headings[index + 1:] if depth <= level), len(lines))
        spans.append((start, end, title))
    return spans


def paragraph(lines: list[str], index: int) -> tuple[int, int]:
    """The lines of the paragraph around `index`, ended by blank lines and headings."""
    def breaks(at: int) -> bool:
        return not lines[at].strip() or bool(HEADING.match(lines[at]))
    start = index
    while start > 0 and not breaks(start - 1) and not breaks(start):
        start -= 1
    end = index + 1
    while end < len(lines) and not breaks(end):
        end += 1
    return start, end


def findings(text: str, added: list[tuple[int, str]], table: dict) -> list[str]:
    lines = text.splitlines()
    spans = section_spans(lines)
    scopes = [paragraph(lines, number - 1) for number, line in added if SCOPE.search(line)]
    found = []
    for record in table.get("record", []):
        anchor = record.get("anchor", "")
        section = record.get("section", "")
        if not anchor or QUALIFIED.search(anchor):
            continue
        candidates = [span for span in spans if section and re.match(
            re.escape(section) + r"(?:[. )]|$)", span[2])]
        for start, end, _ in candidates:
            positions = [at for at in range(start, end) if anchor in lines[at]]
            if any(low <= at < high for at in positions for low, high in scopes):
                found.append(f"{record.get('name', '?')}: anchor {anchor!r} has no version qualifier")
                break
    return found


def run(context):
    specs = {path for path in context.changed_paths() if SPEC.match(path)
             and any(SCOPE.search(line) for _, line in context.added_lines(path))}
    if not specs:
        return context.passed("no added version-scoped specification text")
    arguments = (["ls-tree", "-r", "--name-only", context.rev] if context.rev else
                 ["ls-files", "-co", "--exclude-standard"])
    paths = set(context.git(*arguments, "--", "docs/layouts/").stdout.splitlines())
    details = []
    for path in sorted(paths):
        if not path.endswith(".toml"):
            continue
        try:
            table = tomllib.loads(context.read(path) or "")
        except tomllib.TOMLDecodeError as error:
            details.append(f"{path}: cannot check version anchors: {error}")
            continue
        spec = table.get("spec")
        if spec in specs:
            details.extend(f"{path}: {detail}" for detail in findings(
                context.read(spec) or "", context.added_lines(spec), table))
    if details:
        return context.warned("version-scoped layout anchors need review", *details)
    return context.passed("version-scoped layout anchors name a version")
