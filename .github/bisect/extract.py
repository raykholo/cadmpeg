# Reduce a cadmpeg dump to the parts the re-pin regressions need.
import json, sys
dump, name, out = sys.argv[1], sys.argv[2], sys.argv[3]
d = json.load(open(dump))
m = d.get("model") or {}
r = {"name": name}
if name.startswith("railcore"):
    r["occurrences"] = m.get("occurrences")
    r["definitions"] = [
        {k: p.get(k) for k in ("id", "source_name", "label", "occurrences", "storage", "kind")}
        for p in m.get("product_definitions") or []
    ]
else:
    r["features"] = m.get("features")
    r["design"] = d.get("design")
    g = d.get("geometry") or {}
    r["counts"] = {k: len(v) for k, v in g.items() if isinstance(v, list)}
json.dump(r, open(out, "w"))
