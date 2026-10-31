#!/usr/bin/env python3
"""Build REPORT.md from reports/<id>.{json,err,meta} collected under DIR."""
import collections, glob, json, os, sys

d = sys.argv[1] if len(sys.argv) > 1 else "reports"
manifest = {}
with open("ci/public-corpus.tsv", encoding="utf-8") as f:
    for line in f:
        if line.startswith("#") or not line.strip():
            continue
        r = line.rstrip("\n").split("\t")
        manifest[r[0]] = r


def find_findings(doc):
    if isinstance(doc, dict):
        cr = doc.get("check_report")
        if isinstance(cr, dict) and isinstance(cr.get("findings"), list):
            return cr["findings"]
        if isinstance(doc.get("findings"), list):
            return doc["findings"]
    return None


rows, tot = [], collections.Counter()
msgs_all = collections.Counter()
for meta_path in sorted(glob.glob(os.path.join(d, "**", "*.meta"), recursive=True)):
    base = meta_path[:-5]
    id_ = os.path.basename(base)
    meta = dict(l.strip().split("=", 1) for l in open(meta_path) if "=" in l)
    code = meta.get("exit", "?")
    findings, status, note = None, "", ""
    try:
        findings = find_findings(json.load(open(base + ".json")))
    except Exception:
        pass
    try:
        err = [l for l in open(base + ".err", errors="replace").read().splitlines() if l.strip()]
        note = err[-1][:160] if err else ""
    except OSError:
        pass
    errs = warns = 0
    top = []
    if findings is not None:
        c = collections.Counter()
        for fd in findings:
            sev = fd.get("severity")
            if sev in ("error", "blocking"):
                errs += 1
                c[fd.get("message", "")[:140]] += 1
            elif sev == "warning":
                warns += 1
        top = c.most_common(3)
        msgs_all.update(c)
    if code == "missing":
        status = "MISSING"
    elif code == "124":
        status = "TIMEOUT"
    elif findings is None:
        status = "DECODE-FAIL" if code not in ("0",) else "NO-REPORT"
        if note:
            msgs_all[note] += 1
            top = [(note, 1)]
    elif errs:
        status = "FAIL"
    else:
        status = "PASS"
    tot[status] += 1
    tot["errors"] += errs
    tot["warnings"] += warns
    m = manifest.get(id_, [id_, "", "", "", "", ""])
    rows.append((id_, m[3] or meta.get("file", ""), status, errs, warns, top, meta.get("wall", "0"), int(meta.get("rss_kb", "0") or 0) // 1024))

out = ["# Public corpus check report", ""]
out.append(f"Files: {len(rows)}  " + "  ".join(f"{k}: {v}" for k, v in sorted(tot.items()) if k.isupper() or k in ()))
out.append(f"Total error findings: {tot['errors']}  warnings: {tot['warnings']}")
if rows:
    out.append(f"Max wall: {max(float(r[6]) for r in rows):.1f} s  Max RSS: {max(r[7] for r in rows)} MiB")
out += ["", "## Top error messages (all files)", ""]
for m, n in msgs_all.most_common(10):
    out.append(f"- {n} x {m.replace('|', '/')}")
out += ["", "## Per file", "", "| id | path | status | errors | warnings | top errors | wall s | RSS MiB |", "|---|---|---|---|---|---|---|---|"]
for r in rows:
    top = "<br>".join(f"{n} x {m}".replace("|", "/") for m, n in r[5])
    out.append(f"| {r[0]} | {r[1].replace('|','/')} | {r[2]} | {r[3]} | {r[4]} | {top} | {r[6]} | {r[7]} |")
text = "\n".join(out) + "\n"
open("REPORT.md", "w").write(text)
print(text)
