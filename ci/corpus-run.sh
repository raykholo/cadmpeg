#!/usr/bin/env bash
# Run `cadmpeg check` over corpus/<id>/* for each id in $IDS.
# Writes reports/<id>.json (stdout), reports/<id>.err (stderr) and
# reports/<id>.meta (exit code, wall seconds, peak RSS in KiB, input size).
# A check failure never fails this script.
set -uo pipefail
BIN=${BIN:-./cadmpeg}
mkdir -p reports
for id in $IDS; do
  f=$(find "corpus/$id" -type f 2>/dev/null | head -n1)
  if [ -z "$f" ]; then
    printf 'exit=missing\nwall=0\nrss_kb=0\nsize=0\nfile=\n' > "reports/$id.meta"
    continue
  fi
  size=$(stat -c %s "$f")
  /usr/bin/time -v -o "reports/$id.time" timeout 1800 "$BIN" check "$f" --json > "reports/$id.json" 2> "reports/$id.err"
  code=$?
  wall=$(awk -F': ' '/Elapsed \(wall clock\)/{n=split($NF,a,":"); s=0; for(i=1;i<=n;i++) s=s*60+a[i]; print s}' "reports/$id.time")
  rss=$(awk -F': ' '/Maximum resident set size/{print $2}' "reports/$id.time")
  printf 'exit=%s\nwall=%s\nrss_kb=%s\nsize=%s\nfile=%s\n' "$code" "${wall:-0}" "${rss:-0}" "$size" "$(basename "$f")" > "reports/$id.meta"
  rm -f "reports/$id.time"
done
exit 0
