# SPDX-License-Identifier: Apache-2.0
"""A new open item takes an ID that upstream has not used for another item.

Open-items documents number their items per prefix (`### DR-90.`,
`### GC-41.`). Two branches that each add the next number merge without a
text conflict and leave two items with one ID. The check compares the items
a branch adds with the upstream tip and with the rest of the file, and names
the next free number for each collision. It also warns when an added item
sits outside its prefix's section or skips a number that neither upstream nor
another open PR (from the open-prs check's survey) uses.
"""

from __future__ import annotations

import re
from collections import Counter

NAME = "open-items"
ORDER = 30

OPEN_ITEMS = re.compile(r"^docs/formats/[^/]+-open-items\.md$")
ITEM = re.compile(r"^#{2,4} ([A-Z]{1,4})-(\d+)([A-Z]?)\.\s*(.*?)\s*$")


def items(text: str | None) -> list[tuple[str, str, int]]:
    """(ID, title, line) for each item heading."""
    found = []
    for number, line in enumerate((text or "").splitlines(), 1):
        match = ITEM.match(line)
        if match:
            prefix, digits, suffix, title = match.groups()
            found.append((f"{prefix}-{digits}{suffix}", title, number))
    return found


def next_free(prefix: str, *documents: list[tuple[str, str, int]]) -> str:
    numbers = [int(re.match(rf"{prefix}-(\d+)", identifier).group(1))
               for document in documents for identifier, _, _ in document
               if identifier.startswith(f"{prefix}-")]
    width = max((len(identifier.split("-")[1].rstrip("ABCDEFGHIJKLMNOPQRSTUVWXYZ"))
                 for document in documents for identifier, _, _ in document
                 if identifier.startswith(f"{prefix}-")), default=2)
    return f"{prefix}-{max(numbers, default=0) + 1:0{width}d}"


def sections(text: str | None) -> dict[str, set[str]]:
    """Section headings used by each prefix."""
    found = {}
    section = ""
    for line in (text or "").splitlines():
        if line.startswith("## "):
            section = re.sub(r"^## (?:\d+\.\s*)?", "", line).strip().lower()
        match = ITEM.match(line)
        if match:
            found.setdefault(match.group(1), set()).add(section)
    return found


def number(identifier: str) -> int:
    return int(identifier.split("-")[1].rstrip("ABCDEFGHIJKLMNOPQRSTUVWXYZ"))


def hygiene(context, path: str) -> list[str]:
    """Placement and numbering of the items this branch adds."""
    text = context.read(path) or ""
    base = context.read_base(path)
    upstream = context.read_upstream(path)
    known = sections(base)
    for prefix, titles in sections(upstream).items():
        known.setdefault(prefix, set()).update(titles)
    previous = items(base) + items(upstream)
    old_ids = {identifier for identifier, _, _ in previous}
    warnings = []
    section = ""
    for line_number, line in enumerate(text.splitlines(), 1):
        if line.startswith("## "):
            section = re.sub(r"^## (?:\d+\.\s*)?", "", line).strip().lower()
        match = ITEM.match(line)
        if not match:
            continue
        prefix, digits, suffix, _ = match.groups()
        if f"{prefix}-{digits}{suffix}" in old_ids:
            continue
        expected = known.get(prefix, set())
        semantic = {"DE": "decode", "EN": "encode"}.get(prefix)
        if ((expected and section not in expected)
                or (not expected and semantic and semantic not in section)):
            warnings.append(f"{path}:{line_number}: {prefix}-{digits}{suffix} under '{section}'; "
                            f"expected section: {', '.join(sorted(expected)) or semantic}")
    added = [entry for entry in items(text) if entry[0] not in old_ids]
    if not added:
        return warnings
    # Numbers that other open PRs add are taken, so skipping them is right. Without
    # the open PRs (offline, no gh) a gap cannot be judged and is not reported.
    from preflight_open_prs import claimed_ids
    claims = claimed_ids(context)
    if claims is None:
        return warnings
    maximum = {}
    for identifier, _, _ in previous:
        prefix = identifier.split("-")[0]
        maximum[prefix] = max(maximum.get(prefix, 0), number(identifier))
    claimed = {}
    for document, identifier in claims:
        if document == path:
            claimed.setdefault(identifier.split("-")[0], set()).add(number(identifier))
    for identifier, _, line_number in sorted(added, key=lambda entry: (
            entry[0].split("-")[0], number(entry[0]))):
        prefix, digits = identifier.split("-")
        value = number(identifier)
        high = maximum.get(prefix, 0)
        free = [missing for missing in range(high + 1, value)
                if missing not in claimed.get(prefix, set())]
        if free:
            width = len(digits.rstrip("ABCDEFGHIJKLMNOPQRSTUVWXYZ"))
            warnings.append(f"{path}:{line_number}: {identifier} skips {prefix}-{free[0]:0{width}d}")
        maximum[prefix] = max(high, value)
    return warnings


def run(context):
    paths = [path for path in context.changed_paths() if OPEN_ITEMS.match(path)]
    if not paths:
        return context.passed("no open-items document changed")
    results = []
    for path in paths:
        new = items(context.read(path))
        base_ids = {identifier for identifier, _, _ in items(context.read_base(path))}
        upstream = items(context.read_upstream(path))
        upstream_titles = {identifier: title for identifier, title, _ in upstream}
        problems = []
        counts = Counter(identifier for identifier, _, _ in new)
        for identifier, title, line in new:
            prefix = identifier.split("-")[0]
            free = next_free(prefix, new, upstream)
            if counts[identifier] > 1:
                problems.append(f"{path}:{line}: {identifier} appears {counts[identifier]} times; "
                                f"next free is {free}")
            elif (identifier not in base_ids and identifier in upstream_titles
                  and upstream_titles[identifier] != title):
                problems.append(f"{path}:{line}: {identifier} \"{title}\" collides with upstream's "
                                f"{identifier} \"{upstream_titles[identifier]}\"; next free is {free}")
        warnings = hygiene(context, path)
        if warnings:
            results.append(context.warned(f"open-item hygiene in {path}", *warnings))
        if problems:
            results.append(context.failed(
                f"open-item ID collision in {path}", *problems,
                "fix: renumber the new item and every reference to it (spec text, loss "
                "messages, code and test comments: git grep -n '<old ID>')",
            ))
    return results or context.passed(f"{len(paths)} open-items document(s): no ID collision")
