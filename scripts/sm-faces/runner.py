#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Compare public SolidWorks sheet-metal topology on fork runners."""

import argparse
import collections
import csv
import difflib
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import urllib.request


MANIFEST = "https://progress.dutchmancad.xyz/scoreboard/public-files.json"
RECIPES = {f"S02{x}" for x in "abcdefghi"} | {f"S03{x}" for x in "abcdef"}
RECIPES |= {f"S04{x}" for x in "abcdefg"} | {"S05a"} | {f"S06{x}" for x in "abcd"}
KINDS = ("plane", "cylinder", "cone", "sphere", "torus", "nurbs", "unknown")
FIELDS = ["sha", "file", "recipe", "app", "stage", "status", "body_faces", "face_rows"]
FIELDS += list(KINDS) + ["other_kinds", "volumes_mm3", "boxes_mm"]


def write_tsv(path, fields, rows):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fields, delimiter="\t", extrasaction="ignore")
        writer.writeheader()
        writer.writerows(rows)


def read_tsv(path):
    with path.open(newline="") as stream:
        return list(csv.DictReader(stream, delimiter="\t"))


def select_files(manifest):
    rows = manifest["files"]
    selected = []
    names = set()
    for row in rows:
        app, recipe = row["app"].lower(), row["recipe"]
        if not app.startswith("solidworks"):
            continue
        if not (recipe.startswith(("S02", "S03", "S04", "S06")) or recipe == "S05a"):
            continue
        name = row["file"]
        if not name.lower().endswith(".sldprt"):
            continue
        if row.get("consent", "").lower() != "cc0":
            raise ValueError(f"CC0 consent required: {name}")
        if Path(name).name != name or "\\" in name or name.startswith(".") or name in names:
            raise ValueError(f"invalid or duplicate filename: {name}")
        if not re.fullmatch(r"[0-9a-f]{64}", row["sha256"]):
            raise ValueError(f"invalid sha256: {name}")
        if not row["url"].startswith("https://"):
            raise ValueError(f"HTTPS required: {name}")
        names.add(name)
        selected.append(row)
    if not selected:
        raise ValueError("public manifest selected zero SolidWorks sheet-metal files")
    return sorted(selected, key=lambda row: row["file"])


def verify_file(path, row):
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != row["sha256"]:
        raise ValueError(f"sha256 mismatch: {path.name}")
    if len(data) != int(row["bytes"]):
        raise ValueError(f"byte count mismatch: {path.name}")


def read_shas(path):
    shas = path.read_text().split()
    if not 1 <= len(shas) <= 256 or len(set(shas)) != len(shas):
        raise ValueError("supply 1..256 distinct full commit SHAs in chronological order")
    if any(not re.fullmatch(r"[0-9a-f]{40}", sha) for sha in shas):
        raise ValueError("each revision must be a full commit SHA")
    return shas


def prepare(args):
    shas = read_shas(args.shas)
    args.out.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(urllib.request.Request(MANIFEST, headers={"User-Agent": "cadmpeg-sm-faces"}), timeout=90) as response:
        data = response.read()
    (args.out / "public-files.json").write_bytes(data)
    selected = select_files(json.loads(data))
    (args.out / "selected.json").write_text(json.dumps(selected, indent=2) + "\n")
    (args.out / "shas.txt").write_text("\n".join(shas) + "\n")
    files = args.out / "files"
    files.mkdir()
    for row in selected:
        with urllib.request.urlopen(urllib.request.Request(row["url"], headers={"User-Agent": "cadmpeg-sm-faces"}), timeout=90) as response:
            (files / row["file"]).write_bytes(response.read())
        verify_file(files / row["file"], row)
    missing = []
    for version in ("2023", "2024"):
        present = {r["recipe"] for r in selected if version in r["app"] or version in r["file"]}
        missing.extend({"version": version, "recipe": r} for r in sorted(RECIPES - present))
    write_tsv(args.out / "missing-public-recipes.tsv", ["version", "recipe"], missing)
    (args.out / "manifest.sha256").write_text(hashlib.sha256(data).hexdigest() + "\n")
    matrix = {"include": [{"sha": sha, "trace": sha in (shas[0], shas[-1])} for sha in shas]}
    with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
        stream.write("matrix=" + json.dumps(matrix) + "\n")
    print(f"Verified {len(selected)} public inputs; {len(missing)} requested release/recipe pairs absent")


def command(binary, args, output, *, env=None):
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("wb") as stdout, output.with_suffix(output.suffix + ".stderr").open("wb") as stderr:
        try:
            result = subprocess.run([str(binary), *map(str, args)], stdout=stdout, stderr=stderr,
                                    timeout=180, env=env, check=False)
            code = result.returncode
        except subprocess.TimeoutExpired:
            code = 124
    with (output.parent / "commands.jsonl").open("a") as stream:
        stream.write(json.dumps({"argv": list(map(str, args)), "exit": code, "stdout": output.name}) + "\n")
    return code


def measurement_row(sha, source, stage, body_path, face_path, codes):
    result = {key: source[key] for key in ("file", "recipe", "app")}
    result.update(sha=sha, stage=stage, status="ok" if all(c == 0 for c in codes) else "failed")
    if result["status"] == "failed":
        return result
    bodies, faces = read_tsv(body_path), read_tsv(face_path)
    if not bodies or not faces or "faces" not in bodies[0] or "kind" not in faces[0]:
        result["status"] = "empty-or-unsupported-table"
        return result
    kinds = collections.Counter(face["kind"] for face in faces)
    result.update(body_faces=sum(int(body["faces"]) for body in bodies), face_rows=len(faces))
    result.update({kind: kinds[kind] for kind in KINDS})
    result["other_kinds"] = json.dumps({k: v for k, v in kinds.items() if k not in KINDS}, sort_keys=True)
    result["volumes_mm3"] = json.dumps([b["volume_mm3"] for b in bodies])
    axes = [f"bbox_{end}_{axis}_mm" for end in ("min", "max") for axis in "xyz"]
    result["boxes_mm"] = json.dumps([[b[key] for key in axes] for b in bodies])
    return result


def inspect_document(binary, artifact, directory, prefix):
    command(binary, ["query", "summary", artifact], directory / f"{prefix}.summary.tsv")
    counts = directory / f"{prefix}.counts.tsv"
    if command(binary, ["query", "counts", artifact], counts):
        return
    for row in read_tsv(counts):
        arena = f"{row.get('namespace', '')}.{row.get('arena', '')}"
        if arena in ("geometry.faces", "geometry.surfaces", "topology.faces", "topology.surfaces"):
            command(binary, ["query", "schema", "file", artifact, arena], directory / f"{prefix}.{arena}.schema.tsv")
            command(binary, ["query", "item", artifact, arena, "--head", "100000"],
                    directory / f"{prefix}.{arena}.json")


def run(args):
    args.out.mkdir(parents=True, exist_ok=True)
    binary = args.bin.resolve()
    selected = json.loads((args.inputs / "selected.json").read_text())
    rows = []
    for source in selected:
        path = (args.inputs / "files" / source["file"]).resolve()
        verify_file(path, source)
        directory = (args.out / source["file"]).resolve()
        directory.mkdir(parents=True)
        step = directory / "out.step"
        convert = command(binary, ["convert", path, "-o", step, "--report", directory / "convert.json"], directory / "convert.stdout")
        if args.details and (directory / "convert.json").exists():
            command(binary, ["query", "summary", directory / "convert.json"], directory / "convert.summary.tsv")
            command(binary, ["query", "losses", directory / "convert.json"], directory / "losses.tsv")
        for stage, input_path in (("native", path), ("step", step)):
            body = directory / f"{stage}.bodies.tsv"
            face = directory / f"{stage}.faces.tsv"
            codes = [convert] if stage == "step" else []
            if input_path.exists():
                codes.append(command(binary, ["measure", input_path, "--tsv"], body))
                codes.append(command(binary, ["measure", input_path, "--faces", "--tsv"], face))
                if args.details:
                    report = directory / f"{stage}.measure.json"
                    command(binary, ["measure", input_path, "--json"], report)
                    command(binary, ["query", "summary", report], directory / f"{stage}.measure.summary.tsv")
                    artifact = directory / f"{stage}.cadir.json"
                    if command(binary, ["dump", input_path, "-o", artifact], directory / f"{stage}.dump.stdout") == 0:
                        inspect_document(binary, artifact, directory, stage)
                        sidecar = artifact.with_suffix(".fidelity.json")
                        if sidecar.exists():
                            command(binary, ["query", "fidelity", sidecar], directory / f"{stage}.fidelity.tsv")
                row = measurement_row(args.sha, source, stage, body, face, codes)
            else:
                row = {key: source[key] for key in ("file", "recipe", "app")}
                row.update(sha=args.sha, stage=stage, status="conversion-failed")
            rows.append(row)
        write_tsv(args.out / "results.tsv", FIELDS, rows)
    with open(os.environ.get("GITHUB_STEP_SUMMARY", os.devnull), "a") as stream:
        stream.write(f"### {args.sha}\n\n```tsv\n" + (args.out / "results.tsv").read_text() + "```\n")
    if any(row["status"] != "ok" for row in rows):
        raise SystemExit("measurement failures recorded in results.tsv and command logs")


def instrument_text(text):
    replacements = [
        ("    let mut site = scan_site(ctx, bodies, &mut |_| {})?;", '''    if std::env::var_os("CADMPEG_SM_FACES_TRACE").is_some() {
        eprintln!("SMF\\tSITE\\tname\\t0\\t{}", stream.as_str());
    }
    let mut site = scan_site(ctx, bodies, &mut |_| {})?;'''),
        ("        observe(StreamTopology {", '''        if std::env::var_os("CADMPEG_SM_FACES_TRACE").is_some() {
            eprintln!("SMF\\tSTREAM\\theader\\t{}\\tdeltas={} from_nodes={} {:?}", index, is_deltas, from_nodes, header);
            smf_trace_tables("STREAM", &scanned_tables);
            eprintln!("SMF\\tSTREAM\\ttyped\\t{}\\t{:?}", index, stream_typed_facts);
        }
        observe(StreamTopology {'''),
        ("    let typed_records = typed_body_records(typed_facts, t);", '''    smf_trace_tables("MERGED", t);
    if std::env::var_os("CADMPEG_SM_FACES_TRACE").is_some() {
        eprintln!("SMF\\tMERGED\\tsite\\t0\\t{}", stream.as_str());
        eprintln!("SMF\\tMERGED\\ttyped\\t0\\t{:?}", typed_facts);
        for bridge in t.bridges().values() {
            eprintln!("SMF\\tMERGED\\tcarrier\\t{}\\t{:?}", bridge.attr, carriers.surface(bridge.refs[4]));
        }
    }
    let typed_records = typed_body_records(typed_facts, t);'''),
        ("        out.faces.push(Face {", '''        if std::env::var_os("CADMPEG_SM_FACES_TRACE").is_some() {
            eprintln!("SMF\\tEMIT\\tface\\t{}\\tid={} surface_attr={} loops={:?}", f.bridge_attr, id_face(f.bridge_attr).as_str(), f.surface_attr, loops);
        }
        out.faces.push(Face {'''),
    ]
    for before, after in replacements:
        if text.count(before) != 1:
            raise ValueError(f"trace anchor changed: {before.strip()}")
        text = text.replace(before, after)
    text += '''
fn smf_trace_tables(phase: &str, tables: &topology::Tables) {
    if std::env::var_os("CADMPEG_SM_FACES_TRACE").is_none() {
        return;
    }
    for (attr, record) in tables.bridges() {
        eprintln!("SMF\\t{}\\tface\\t{}\\t{:?}", phase, attr, record);
    }
    for (attr, record) in tables.loops() {
        eprintln!("SMF\\t{}\\tloop\\t{}\\t{:?}", phase, attr, record);
    }
    for (attr, record) in tables.coedges() {
        eprintln!("SMF\\t{}\\tfin\\t{}\\t{:?}", phase, attr, record);
    }
}
'''
    return text


def instrument(args):
    path = args.source / "crates/cadmpeg-codec-sldprt/src/brep/graph.rs"
    before = path.read_text()
    after = instrument_text(before)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("".join(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
                                                  fromfile=str(path), tofile=str(path))))
    path.write_text(after)


def trace(args):
    binary = args.bin.resolve()
    env = dict(os.environ, CADMPEG_SM_FACES_TRACE="1")
    failed = False
    for row in json.loads((args.inputs / "selected.json").read_text()):
        source = (args.inputs / "files" / row["file"]).resolve()
        verify_file(source, row)
        directory = (args.out / row["file"]).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        log = directory / "trace.stdout"
        code = command(binary, ["convert", source, "-o", directory / "trace.step"], log, env=env)
        lines = log.with_suffix(".stdout.stderr").read_text().splitlines()
        records = [line for line in lines if line.startswith("SMF\t")]
        (directory / "source-topology.tsv").write_text("marker\tphase\tfamily\tattr\trecord\n" + "\n".join(records) + "\n")
        diff = command(binary, ["diff", directory / "out.step", directory / "trace.step", "--geometry", "--json"],
                       directory / "trace-vs-production.json")
        failed |= code != 0 or diff != 0 or not records
    if failed:
        raise SystemExit("source trace or comparison failed; inspect per-file logs")


def signature(row):
    return tuple(row[key] for key in ("body_faces", "face_rows", *KINDS, "other_kinds"))


def collect(args):
    shas = read_shas(args.inputs / "shas.txt")
    selected = json.loads((args.inputs / "selected.json").read_text())
    data = {}
    all_rows = []
    for sha in shas:
        path = args.results / f"sm-faces-{sha}" / "results.tsv"
        rows = read_tsv(path) if path.exists() else []
        all_rows.extend(rows)
        for row in rows:
            data[sha, row["file"], row["stage"]] = row
    write_tsv(args.out / "all-results.tsv", FIELDS, all_rows)
    changes, gaps, endpoints = [], [], []
    for row in selected:
        for stage in ("native", "step"):
            previous = None
            for sha in shas:
                current = data.get((sha, row["file"], stage))
                if current is None or current["status"] != "ok":
                    gaps.append(dict(sha=sha, file=row["file"], stage=stage, status=current["status"] if current else "missing"))
                    previous = None
                    continue
                if previous and signature(previous) != signature(current):
                    changes.append(dict(file=row["file"], stage=stage, before=previous["sha"], after=sha,
                                        old_faces=previous["body_faces"], new_faces=current["body_faces"],
                                        old_planes=previous["plane"], new_planes=current["plane"],
                                        old_cylinders=previous["cylinder"], new_cylinders=current["cylinder"]))
                previous = current
            old = data.get((shas[0], row["file"], stage))
            new = data.get((shas[-1], row["file"], stage))
            available = old and new and old["status"] == new["status"] == "ok"
            endpoints.append(dict(file=row["file"], stage=stage,
                                  result=("same-counts" if signature(old) == signature(new) else "changed-counts") if available else "unavailable"))
    write_tsv(args.out / "transitions.tsv", ["file", "stage", "before", "after", "old_faces", "new_faces", "old_planes", "new_planes", "old_cylinders", "new_cylinders"], changes)
    write_tsv(args.out / "gaps.tsv", ["sha", "file", "stage", "status"], gaps)
    write_tsv(args.out / "endpoints.tsv", ["file", "stage", "result"], endpoints)
    geometry = []
    identities = []
    binary = (args.results / f"sm-faces-{shas[-1]}" / "cadmpeg").resolve()
    if binary.exists():
        binary.chmod(binary.stat().st_mode | 0o100)
    for source in selected:
        directories = [args.results / f"sm-faces-{sha}" / source["file"] for sha in (shas[0], shas[-1])]
        steps = [(directory / "out.step").resolve() for directory in directories]
        if binary.exists() and all(path.exists() for path in steps):
            code = command(binary, ["diff", *steps, "--geometry", "--json"],
                           args.out / "geometry" / f"{source['file']}.json")
            geometry.append(dict(file=source["file"], exit=code))
        else:
            geometry.append(dict(file=source["file"], exit="missing-endpoint-artifact"))
        for stage in ("native", "step"):
            tables = [directory / f"{stage}.faces.tsv" for directory in directories]
            if not all(path.exists() for path in tables):
                continue
            old_faces, new_faces = ({row["face"]: row for row in read_tsv(path)} for path in tables)
            for face in sorted(old_faces.keys() | new_faces.keys()):
                old, new = old_faces.get(face), new_faces.get(face)
                status = "removed" if new is None else "added" if old is None else "retained"
                identities.append(dict(file=source["file"], stage=stage, face=face, status=status,
                                       old_kind=old["kind"] if old else "", new_kind=new["kind"] if new else "",
                                       old_surface=old["surface"] if old else "", new_surface=new["surface"] if new else ""))
    write_tsv(args.out / "geometry-status.tsv", ["file", "exit"], geometry)
    write_tsv(args.out / "face-identities.tsv", ["file", "stage", "face", "status", "old_kind", "new_kind", "old_surface", "new_surface"], identities)
    note = f"Compared {len(shas)} revisions and {len(selected)} public files; {len(changes)} adjacent count transitions; {len(gaps)} gaps.\n"
    note += "Equal endpoint counts establish equality for this range's endpoints; the SolidWorks count difference still requires source review.\n"
    note += "Each transition is bounded by the two listed SHAs. A full first-parent list gives adjacent commits.\n"
    note += "Source pairs and surface continuity require review of source-topology.tsv, native face tables, CADIR and losses.\n"
    (args.out / "README.txt").write_text(note)
    with open(os.environ.get("GITHUB_STEP_SUMMARY", os.devnull), "a") as stream:
        stream.write(note)
        for row in changes:
            stream.write(f"\n{row['file']} {row['stage']}: {row['before'][:10]} → {row['after'][:10]}, {row['old_faces']} → {row['new_faces']} faces\n")
    if gaps:
        raise SystemExit("comparison has gaps; inspect gaps.tsv and failed matrix jobs")
    if any(row["exit"] not in (0, 1) for row in geometry):
        raise SystemExit("geometry comparison incomplete; inspect geometry-status.tsv")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("prepare", "run", "instrument", "trace", "collect"):
        sub = commands.add_parser(name)
        sub.add_argument("--out", type=Path, required=True)
        if name == "prepare":
            sub.add_argument("--shas", type=Path, required=True)
        elif name == "instrument":
            sub.add_argument("--source", type=Path, required=True)
        else:
            sub.add_argument("--inputs", type=Path, required=True)
            if name == "collect":
                sub.add_argument("--results", type=Path, required=True)
            else:
                sub.add_argument("--bin", type=Path, required=True)
            if name == "run":
                sub.add_argument("--sha", required=True)
                sub.add_argument("--details", action="store_true")
    args = parser.parse_args()
    globals()[args.command](args)


if __name__ == "__main__":
    main()
