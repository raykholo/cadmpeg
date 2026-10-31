#!/usr/bin/env bash
# Fetch the manifest rows named by $IDS into corpus/<id>/<basename>.
# Usage: IDS="vc-001 vc-002" ci/corpus-fetch.sh
# One blobless depth-1 fetch per repository; only the listed paths are
# checked out, so only those blobs are downloaded.
set -euo pipefail
MANIFEST=ci/public-corpus.tsv
rm -rf corpus work
mkdir -p corpus work
repos=$(for id in $IDS; do awk -F'\t' -v id="$id" '$1==id{print $2"\t"$3}' "$MANIFEST"; done | sort -u)
n=0
while IFS=$'\t' read -r url ref; do
  [ -n "$url" ] || continue
  n=$((n + 1))
  d=work/r$n
  git init -q "$d"
  git -C "$d" remote add origin "$url"
  git -C "$d" fetch -q --depth 1 --filter=blob:none origin "$ref"
  : > "$d/paths.nul"
  for id in $IDS; do
    awk -F'\t' -v id="$id" -v u="$url" -v r="$ref" '$1==id && $2==u && $3==r{printf "%s%c",$4,0}' "$MANIFEST" >> "$d/paths.nul"
  done
  git -C "$d" checkout -q FETCH_HEAD --pathspec-from-file=paths.nul --pathspec-file-nul
  # Git LFS pointers: resolve when a repository uses LFS.
  if grep -rlq '^version https://git-lfs' "$d" --include='*.f3d' --include='*.f3z' --include='*.step' --include='*.stp' --include='*.STEP' 2>/dev/null; then
    git -C "$d" lfs install --local >/dev/null
    git -C "$d" lfs pull || echo "lfs pull failed for $url"
  fi
  for id in $IDS; do
    p=$(awk -F'\t' -v id="$id" -v u="$url" -v r="$ref" '$1==id && $2==u && $3==r{print $4}' "$MANIFEST")
    [ -n "$p" ] || continue
    mkdir -p "corpus/$id"
    if [ -f "$d/$p" ]; then cp "$d/$p" "corpus/$id/$(basename "$p")"; else echo "missing: $id $p" >&2; fi
  done
done <<< "$repos"
