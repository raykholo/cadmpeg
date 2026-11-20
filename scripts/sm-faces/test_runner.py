# SPDX-License-Identifier: Apache-2.0
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import runner


def public_row(recipe="S02a", app="solidworks-2023"):
    return dict(file=f"{app}_{recipe}.sldprt", app=app, recipe=recipe, consent="cc0",
                sha256=hashlib.sha256(b"public test bytes").hexdigest(), bytes=17,
                url=f"https://example.test/{app}_{recipe}.sldprt")


class PublicSelectionTests(unittest.TestCase):
    def test_requested_families_and_releases(self):
        wanted = [public_row(recipe, f"solidworks-{version}")
                  for version in (2023, 2024) for recipe in sorted(runner.RECIPES)]
        excluded = [public_row("S01a"), public_row("S05b"), public_row("S07a"),
                    public_row("S02a", "inventor-2024")]
        self.assertEqual(runner.select_files(dict(files=wanted + excluded)),
                         sorted(wanted, key=lambda row: row["file"]))
        self.assertEqual(len(wanted), 54)

    def test_unsafe_download_metadata_is_rejected(self):
        for change in (dict(consent="private"), dict(file="../secret.sldprt"),
                       dict(file="dir\\secret.sldprt"), dict(sha256="bad"),
                       dict(url="file:///secret")):
            with self.subTest(change=change), self.assertRaises(ValueError):
                runner.select_files(dict(files=[public_row() | change]))

    def test_duplicates_and_empty_selection_are_rejected(self):
        for rows in ([public_row(), public_row()], [public_row("S05b")]):
            with self.assertRaises(ValueError):
                runner.select_files(dict(files=rows))

    def test_download_digest_and_size_are_checked(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "part.sldprt"
            path.write_bytes(b"public test bytes")
            runner.verify_file(path, public_row())
            with self.assertRaisesRegex(ValueError, "byte count"):
                runner.verify_file(path, public_row() | dict(bytes=18))
            path.write_bytes(b"modified")
            with self.assertRaisesRegex(ValueError, "sha256"):
                runner.verify_file(path, public_row())

    def test_prepare_freezes_manifest_and_reports_missing_recipes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            shas = root / "shas.txt"
            shas.write_text("a" * 40 + "\n" + "b" * 40 + "\n" + "c" * 40 + "\n")
            replies = [io.BytesIO(json.dumps(dict(files=[public_row()])).encode()),
                       io.BytesIO(b"public test bytes")]
            output = root / "outputs"
            with patch.object(runner.urllib.request, "urlopen", side_effect=replies), \
                    patch.dict(os.environ, GITHUB_OUTPUT=str(output)):
                runner.prepare(argparse.Namespace(shas=shas, out=root / "inputs"))
            missing = runner.read_tsv(root / "inputs/missing-public-recipes.tsv")
            self.assertEqual(len(missing), 53)
            matrix = json.loads(output.read_text().split("=", 1)[1])["include"]
            self.assertEqual([row["trace"] for row in matrix], [True, False, True])
            self.assertTrue((root / "inputs/public-files.json").exists())

    def test_bad_revision_lists_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "shas"
            for content in ("", "main", "a" * 40 + "\n" + "a" * 40,
                            "\n".join(f"{n:040x}" for n in range(257))):
                path.write_text(content)
                with self.assertRaises(ValueError):
                    runner.read_shas(path)


class MeasurementTests(unittest.TestCase):
    def test_conversion_failure_keeps_native_measurement_and_attempts_written_step(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            inputs = root / "inputs"
            (inputs / "files").mkdir(parents=True)
            source = public_row()
            (inputs / "selected.json").write_text(json.dumps([source]))
            (inputs / "files" / source["file"]).write_bytes(b"public test bytes")
            calls = []

            def invoke(binary, args, output):
                calls.append(args)
                if args[0] == "convert":
                    Path(args[3]).write_text("written artifact")
                    return 1
                if "--faces" in args:
                    runner.write_tsv(output, ["face", "surface", "kind"],
                                     [dict(face=f"face#{n}", surface=f"surface#{n}", kind="plane") for n in range(6)])
                else:
                    axes = [f"bbox_{end}_{axis}_mm" for end in ("min", "max") for axis in "xyz"]
                    row = dict(faces=6, volume_mm3="1000") | dict.fromkeys(axes, "0")
                    runner.write_tsv(output, list(row), [row])
                return 0

            args = argparse.Namespace(inputs=inputs, out=root / "out", bin=Path("unused"), sha="a" * 40, details=False)
            with patch.object(runner, "command", side_effect=invoke), self.assertRaises(SystemExit):
                runner.run(args)
            rows = runner.read_tsv(args.out / "results.tsv")
            self.assertEqual([(row["stage"], row["status"]) for row in rows], [("native", "ok"), ("step", "failed")])
            self.assertTrue(any(args[0] == "measure" and str(args[1]).endswith("out.step") for args in calls))

    def test_body_face_count_and_surface_face_count_remain_separate(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            axes = [f"bbox_{end}_{axis}_mm" for end in ("min", "max") for axis in "xyz"]
            fields = ["body", "faces", "volume_mm3", *axes]
            body = dict(body="body#1", faces=14, volume_mm3="1000.00000001")
            body.update({key: "0" for key in axes})
            runner.write_tsv(root / "body.tsv", fields, [body])
            runner.write_tsv(root / "faces.tsv", ["face", "kind"],
                             [dict(face=f"face#{n}", kind="plane" if n < 8 else "cylinder") for n in range(10)])
            result = runner.measurement_row("a" * 40, public_row(), "native",
                                            root / "body.tsv", root / "faces.tsv", [0, 0])
            self.assertEqual((result["body_faces"], result["face_rows"], result["plane"], result["cylinder"]),
                             (14, 10, 8, 2))
            self.assertIn("1000.00000001", result["volumes_mm3"])

    def test_failed_measurement_keeps_counts_unavailable(self):
        result = runner.measurement_row("a" * 40, public_row(), "step", Path("absent"), Path("absent"), [1])
        self.assertEqual(result["status"], "failed")
        self.assertNotIn("body_faces", result)

    def test_each_command_has_a_timeout_and_records_it(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "body.tsv"
            with patch.object(runner.subprocess, "run", side_effect=subprocess.TimeoutExpired("cadmpeg", 180)) as invoke:
                self.assertEqual(runner.command(Path("cadmpeg"), ["measure", "part"], output), 124)
            self.assertEqual(invoke.call_args.kwargs["timeout"], 180)
            self.assertEqual(json.loads((output.parent / "commands.jsonl").read_text())["exit"], 124)

    def test_query_counts_namespace_selects_geometry_arenas(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            calls = []

            def invoke(binary, args, output):
                calls.append(args)
                if args[1] == "counts":
                    runner.write_tsv(output, ["namespace", "arena", "entries"],
                                     [dict(namespace="geometry", arena="faces", entries=14),
                                      dict(namespace="geometry", arena="surfaces", entries=14)])
                return 0

            with patch.object(runner, "command", side_effect=invoke):
                runner.inspect_document(Path("cadmpeg"), Path("part.json"), root, "native")
            self.assertEqual({args[3] for args in calls if args[:2] == ["query", "item"]},
                             {"geometry.faces", "geometry.surfaces"})


class ComparisonTests(unittest.TestCase):
    def test_adjacent_transition_names_both_commits_and_surface_counts(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            inputs, results = root / "inputs", root / "results"
            inputs.mkdir()
            shas = [c * 40 for c in "ab"]
            (inputs / "shas.txt").write_text("\n".join(shas) + "\n")
            source = public_row()
            (inputs / "selected.json").write_text(json.dumps([source]))
            for sha, faces in ((shas[0], 14), (shas[1], 10)):
                directory = results / f"sm-faces-{sha}"
                directory.mkdir(parents=True)
                (directory / "cadmpeg").touch()
                (directory / source["file"]).mkdir()
                (directory / source["file"] / "out.step").touch()
                rows = []
                for stage in ("native", "step"):
                    row = dict.fromkeys(runner.FIELDS, "0")
                    row.update(sha=sha, file=source["file"], stage=stage, status="ok",
                               body_faces=str(faces), face_rows=str(faces), plane="8", cylinder=str(faces - 8))
                    rows.append(row)
                runner.write_tsv(directory / "results.tsv", runner.FIELDS, rows)
            args = argparse.Namespace(inputs=inputs, results=results, out=root / "comparison")
            with patch.object(runner, "command", return_value=0):
                runner.collect(args)
            changes = runner.read_tsv(args.out / "transitions.tsv")
            self.assertEqual(len(changes), 2)
            self.assertTrue(all(row["before"] == shas[0] and row["after"] == shas[1] for row in changes))
            self.assertTrue(all((row["old_cylinders"], row["new_cylinders"]) == ("6", "2") for row in changes))

    def test_incomplete_revision_breaks_the_transition_interval(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            inputs, results = root / "inputs", root / "results"
            inputs.mkdir()
            shas = [c * 40 for c in "abc"]
            (inputs / "shas.txt").write_text("\n".join(shas) + "\n")
            source = public_row()
            (inputs / "selected.json").write_text(json.dumps([source]))
            for sha, count in ((shas[0], 14), (shas[-1], 10)):
                rows = []
                for stage in ("native", "step"):
                    row = dict.fromkeys(runner.FIELDS, "0")
                    row.update(sha=sha, file=source["file"], stage=stage, status="ok",
                               body_faces=str(count), face_rows=str(count))
                    rows.append(row)
                runner.write_tsv(results / f"sm-faces-{sha}" / "results.tsv", runner.FIELDS, rows)
            args = argparse.Namespace(inputs=inputs, results=results, out=root / "comparison")
            with self.assertRaises(SystemExit):
                runner.collect(args)
            self.assertEqual(runner.read_tsv(args.out / "transitions.tsv"), [])
            self.assertEqual(len(runner.read_tsv(args.out / "gaps.tsv")), 2)
            self.assertTrue(all(row["result"] == "changed-counts" for row in runner.read_tsv(args.out / "endpoints.tsv")))

    def test_tracing_requires_exact_source_anchors(self):
        with self.assertRaisesRegex(ValueError, "trace anchor"):
            runner.instrument_text("unrelated source")


if __name__ == "__main__":
    unittest.main()
