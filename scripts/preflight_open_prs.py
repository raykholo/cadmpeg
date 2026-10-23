# SPDX-License-Identifier: Apache-2.0
"""No other open pull request changes the same files, or the overlap is known.

Two open PRs that change the same files either duplicate work or must keep
each other's changes when the second one rebases. The check asks GitHub,
through the `gh` CLI, for the upstream repository's open PRs and warns on
each one that shares files with this branch. It needs network access and
an authenticated `gh`; `--offline` skips it. Added open-item IDs are also
compared with the headings that other open PRs add; a retitled heading keeps
its ID and is not new. Exact collisions fail. Only the PRs whose files, as
`gh pr list` reports them, include an open-items document this branch changes
have their diffs read, in parallel; an unreadable diff produces a skip note.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass

from preflight_open_items import OPEN_ITEMS, ITEM, items

NAME = "open-prs"
ORDER = 90

REPOSITORY = re.compile(r"github\.com[:/]([\w.-]+/[\w.-]+?)(?:\.git)?/?$")


def repository(context) -> str | None:
    remote = context.upstream.split("/", 1)[0]
    url = context.git("remote", "get-url", remote).stdout.strip()
    match = REPOSITORY.search(url)
    return match.group(1) if match else None


def branch_name(context) -> str | None:
    """The branch under test: `HEAD`'s, or the branch that `--rev` names or points at."""
    if context.rev is None:
        name = context.git("rev-parse", "--abbrev-ref", "HEAD").stdout.strip()
        return name if name and name != "HEAD" else None
    full = context.git("rev-parse", "--symbolic-full-name", context.rev).stdout.strip()
    if full.startswith("refs/heads/"):
        return full.removeprefix("refs/heads/")
    if full.startswith("refs/remotes/"):
        return full.removeprefix("refs/remotes/").split("/", 1)[-1]
    pointing = context.git("branch", "--points-at", context.rev,
                           "--format=%(refname:short)").stdout.split()
    return pointing[0] if len(pointing) == 1 else None


def open_pulls(repo: str) -> list[dict] | None:
    process = subprocess.run(
        ["gh", "pr", "list", "-R", repo, "--state", "open", "--limit", "1000",
         "--json", "number,title,author,headRefName,files"],
        capture_output=True, text=True, timeout=60)
    return json.loads(process.stdout) if process.returncode == 0 else None


def overlaps(pulls: list[dict], paths: set[str], branch: str | None) -> list[str]:
    found = []
    for pull in pulls:
        if pull.get("headRefName") == branch:
            continue
        shared = sorted(paths & {file["path"] for file in pull.get("files") or []})
        if shared:
            author = (pull.get("author") or {}).get("login", "?")
            found.append(f"#{pull['number']} {pull['title']} ({author}): "
                         + ", ".join(shared[:6]) + (" ..." if len(shared) > 6 else ""))
    return found


def added_ids(diff: str) -> set[tuple[str, str]]:
    """New open-item headings in a unified PR diff, keyed by document and ID.

    A heading that the same diff also removes from the same document is a
    retitled item, not a new ID.
    """
    path = ""
    added = set()
    removed = set()
    for line in diff.splitlines():
        if line.startswith(("+++ ", "--- ")):
            if line.startswith("+++ "):
                path = line[4:].removeprefix("b/")
            continue
        if line[:1] in "+-" and line[1:5] == "### " and OPEN_ITEMS.match(path):
            match = ITEM.match(line[1:])
            if match:
                prefix, digits, suffix, _ = match.groups()
                (added if line[0] == "+" else removed).add((path, f"{prefix}-{digits}{suffix}"))
    return added - removed


def pull_diff(repo: str, number: int) -> str | None:
    try:
        process = subprocess.run(
            ["gh", "pr", "diff", str(number), "-R", repo, "--color=never"],
            capture_output=True, text=True, timeout=10)
    except subprocess.TimeoutExpired:
        return None
    return process.stdout if process.returncode == 0 else None


@dataclass
class Survey:
    """The open PRs, asked once per run and shared with the open-items check."""

    skip: str | None = None
    repo: str = ""
    pulls: list[dict] | None = None
    branch: str | None = None
    diffs: dict[int, str | None] | None = None


def survey(context) -> Survey:
    cached = context.cache.get(NAME)
    if cached is not None:
        return cached
    result = Survey()
    repo = None
    if context.options.offline:
        result.skip = "--offline"
    elif shutil.which("gh") is None:
        result.skip = "the gh CLI is not installed"
    elif (repo := repository(context)) is None:
        result.skip = f"cannot tell the GitHub repository of {context.upstream}"
    if repo is not None:
        result.repo = repo
        try:
            result.pulls = open_pulls(repo)
        except (subprocess.TimeoutExpired, json.JSONDecodeError):
            result.pulls = None
        if result.pulls is None:
            result.skip = f"gh could not list the open PRs of {repo}"
        else:
            result.branch = branch_name(context)
    context.cache[NAME] = result
    return result


def pull_diffs(context, found: Survey) -> dict[int, str | None]:
    """Diffs of the other open PRs whose files include an open-items document this branch changes."""
    if found.diffs is None:
        documents = {path for path in context.changed_paths() if OPEN_ITEMS.match(path)}
        touching = [pull["number"] for pull in found.pulls or []
                    if pull.get("headRefName") != found.branch
                    and documents & {file["path"] for file in pull.get("files") or []}]
        with ThreadPoolExecutor(max_workers=8) as pool:
            diffs = list(pool.map(lambda number: pull_diff(found.repo, number), touching))
        found.diffs = dict(zip(touching, diffs))
    return found.diffs


def claimed_ids(context) -> dict[tuple[str, str], set[int]] | None:
    """(document, ID) -> the other open PRs that add that item; None when GitHub was not asked."""
    found = survey(context)
    if found.pulls is None:
        return None
    claims: dict[tuple[str, str], set[int]] = {}
    for number, diff in pull_diffs(context, found).items():
        for key in added_ids(diff or ""):
            claims.setdefault(key, set()).add(number)
    return claims


def id_collisions(context, found: Survey):
    ours = set()
    for path in context.changed_paths():
        if OPEN_ITEMS.match(path):
            existing = {identifier for identifier, _, _ in items(context.read_base(path))}
            for _, line in context.added_lines(path):
                match = ITEM.match(line)
                if match and line.startswith("### "):
                    prefix, digits, suffix, _ = match.groups()
                    identifier = f"{prefix}-{digits}{suffix}"
                    if identifier not in existing:
                        ours.add((path, identifier))
    results = []
    if not ours:
        return results
    for number, diff in sorted(pull_diffs(context, found).items()):
        if diff is None:
            results.append(context.skipped(f"could not read diff of PR #{number}"))
            continue
        shared = sorted(ours & added_ids(diff))
        if shared:
            results.append(context.failed(
                f"open-item IDs also added by PR #{number}",
                *(f"{path}: {identifier}" for path, identifier in shared),
                "fix: renumber the new item and every reference to it",
            ))
    return results


def run(context):
    found = survey(context)
    if found.pulls is None:
        return context.skipped(found.skip or "the open PRs are unknown")
    overlap = overlaps(found.pulls, set(context.changed_paths()), found.branch)
    results = id_collisions(context, found)
    if not overlap:
        return results or context.passed(f"no open PR in {found.repo} changes the same files")
    results.append(context.warned(
        f"{len(overlap)} open PR(s) change the same files",
        *overlap[:15],
        "check: read each one; drop duplicate work, and agree which lands first and what the "
        "second must keep",
    ))
    return results
