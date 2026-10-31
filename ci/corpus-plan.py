#!/usr/bin/env python3
"""Emit the GitHub Actions shard matrix for ci/public-corpus.tsv.

Each very large assembly gets its own shard; every other file is grouped by
repository in chunks of CHUNK files.
"""
import json, os, sys

CHUNK = 40
SOLO_SUFFIX = ("_Assembly.f3d",)


def main():
    rows = []
    with open("ci/public-corpus.tsv", encoding="utf-8") as f:
        for line in f:
            if line.startswith("#") or not line.strip():
                continue
            rows.append(line.rstrip("\n").split("\t"))
    shards = []
    by_repo = {}
    for r in rows:
        if r[3].endswith(SOLO_SUFFIX):
            shards.append([r[0]])
        else:
            by_repo.setdefault(r[1], []).append(r[0])
    for ids in by_repo.values():
        for i in range(0, len(ids), CHUNK):
            shards.append(ids[i : i + CHUNK])
    matrix = {"include": [{"shard": f"s{n:02d}", "ids": " ".join(ids)} for n, ids in enumerate(shards, 1)]}
    out = json.dumps(matrix)
    print(out)
    gh = os.environ.get("GITHUB_OUTPUT")
    if gh:
        with open(gh, "a") as f:
            f.write(f"matrix={out}\n")


if __name__ == "__main__":
    sys.exit(main())
