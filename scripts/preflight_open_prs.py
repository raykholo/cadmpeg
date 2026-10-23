# SPDX-License-Identifier: Apache-2.0
"""No other open pull request changes the same files, or the overlap is known.

Two open PRs that change the same files either duplicate work or must keep
each other's changes when the second one rebases. The check asks GitHub,
through the `gh` CLI, for the upstream repository's open PRs and warns on
each one that shares files with this branch. It needs network access and
an authenticated `gh`; `--offline` skips it. Added open-item IDs are also
compared with added headings in PR diffs; a retitled heading keeps its ID
and is not new. Exact collisions fail; unreadable diffs produce a skip note. Diff requests run in parallel with bounded workers.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
from concurrent.futures import ThreadPoolExecutor

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


def id_collisions(context, repo: str, pulls: list[dict], branch: str | None):
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
    others = [pull for pull in pulls if pull.get("headRefName") != branch]
    with ThreadPoolExecutor(max_workers=8) as pool:
        diffs = list(pool.map(lambda pull: pull_diff(repo, pull["number"]), others))
    for pull, diff in zip(others, diffs):
        if diff is None:
            results.append(context.skipped(f"could not read diff of PR #{pull['number']}"))
            continue
        shared = sorted(ours & added_ids(diff))
        if shared:
            results.append(context.failed(
                f"open-item IDs also added by PR #{pull['number']}",
                *(f"{path}: {identifier}" for path, identifier in shared),
                "fix: renumber the new item and every reference to it",
            ))
    return results


def run(context):
    if context.options.offline:
        return context.skipped("--offline")
    if shutil.which("gh") is None:
        return context.skipped("the gh CLI is not installed")
    repo = repository(context)
    if repo is None:
        return context.skipped(f"cannot tell the GitHub repository of {context.upstream}")
    try:
        pulls = open_pulls(repo)
    except (subprocess.TimeoutExpired, json.JSONDecodeError):
        pulls = None
    if pulls is None:
        return context.skipped(f"gh could not list the open PRs of {repo}")
    branch = branch_name(context)
    found = overlaps(pulls, set(context.changed_paths()), branch)
    results = id_collisions(context, repo, pulls, branch)
    if not found:
        return results or context.passed(f"no open PR in {repo} changes the same files")
    results.append(context.warned(
        f"{len(found)} open PR(s) change the same files",
        *found[:15],
        "check: read each one; drop duplicate work, and agree which lands first and what the "
        "second must keep",
    ))
    return results
