# SPDX-License-Identifier: Apache-2.0
"""Run with python3 -B -m unittest discover -s ci -p test_paired_measure.py."""
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('paired', Path(__file__).with_name('paired-measure.py'))
PAIRED = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PAIRED)


@contextlib.contextmanager
def working_directory(path):
    previous = Path.cwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(previous)


class PairedMeasurementTests(unittest.TestCase):
    def test_manifest_has_four_archives_and_exact_public_index(self):
        rows = PAIRED.inputs()
        self.assertEqual(len(rows), 178)
        self.assertEqual(sum(name.startswith('RailCore_') for name in rows), 4)
        self.assertTrue(all(PAIRED.source(row['url'])[3].lower().endswith(('.f3d', '.f3z'))
                            for row in rows.values()))

    def test_source_rejects_unpinned_and_private_service_urls(self):
        for url in ('https://example.org/a/b/' + 'a' * 40 + '/x.f3d',
                    'https://raw.githubusercontent.com/a/b/main/x.f3d',
                    'https://raw.githubusercontent.com/a/b/' + 'a' * 40 + '/%2E%2E/x.f3d'):
            with self.subTest(url=url), self.assertRaises(ValueError):
                PAIRED.source(url)

    def test_plan_covers_each_input_once(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / 'output'
            with patch.dict(os.environ, BASE_REF='origin/main', HEAD_REF='feature', GITHUB_OUTPUT=str(output)), \
                    patch.object(PAIRED, 'resolve', side_effect=['a' * 40, 'b' * 40]), \
                    patch.object(PAIRED.Path, 'write_text') as write:
                PAIRED.plan()
            plan = json.loads(write.call_args.args[0])
            shards = plan['matrix']['include']
            names = [name for shard in shards for name in shard['ids'].split()]
            self.assertEqual(names, list(PAIRED.inputs()))
            self.assertEqual(len(shards), 60)
            self.assertEqual(plan['base'], 'a' * 40)
            self.assertEqual(plan['head'], 'b' * 40)
            self.assertIn('matrix=', output.read_text())

    def test_features_include_native_unresolved_and_postprocess_extrudes(self):
        definitions = [
            {'definition': 'extrude', 'profile': {'kind': 'sketch_regions'}},
            {'definition': 'extrude', 'profile': {'kind': 'sketch_selection'}},
            {'definition': 'native', 'kind': 'Extrude'},
            {'definition': 'unresolved', 'family': 'extrude'},
            {'definition': 'post_process', 'operation': {'definition': 'extrude', 'profile': {'kind': 'native'}}},
            {'definition': 'fillet'},
        ]
        rows = [{'id': str(i), 'name': 'feature', 'definition': definition}
                for i, definition in enumerate(definitions)]
        result = PAIRED.features(rows)
        self.assertEqual([row['profile'] for row in result.values()],
                         ['sketch_regions', 'sketch_selection', 'native', 'unresolved', 'native', None])
        with self.assertRaises(ValueError):
            PAIRED.features([rows[0], rows[0]])

    def test_measure_captures_queries_failure_exits_and_cadir_hash(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp) / 'result'
            def command(binary, args, prefix, seconds=900):
                if args[0] == 'dump':
                    (folder / 'cadir.json').write_text('synthetic CADIR')
                    (folder / 'dump.json').write_text('{}')
                    return 0
                (folder / 'check.json').write_text('{}')
                return 1
            def query(binary, view, artifact, prefix, *args):
                if view == 'summary' and artifact.name == 'dump.json':
                    return {'decode_losses': '2'}
                return {'losses': [{'code': 'f3d.unsupported-profile'}, {'code': 'f3d.unsupported-profile'}],
                        'summary': {'findings': '3 (2 error, 1 warning)'},
                        'findings': [{'severity': 'error'}, {'severity': 'blocking'}, {'severity': 'warning'}],
                        'counts': {'model.features': 1},
                        'item': [{'id': 'ext', 'name': 'Extrude1', 'definition': {
                            'definition': 'extrude', 'profile': {'kind': 'sketch_regions'}}}]}[view]
            with patch.object(PAIRED, 'command', side_effect=command), patch.object(PAIRED, 'query', side_effect=query) as queries:
                result = PAIRED.measure(Path('binary'), Path('input.f3d'), folder)
            self.assertEqual(result['dump_exit'], 0)
            self.assertEqual(result['check_exit'], 1)
            self.assertEqual(result['check_errors'], 2)
            self.assertEqual(result['losses'], {'f3d.unsupported-profile': 2})
            self.assertEqual(result['cadir_sha256'], hashlib.sha256(b'synthetic CADIR').hexdigest())
            self.assertEqual(result['features']['ext']['profile'], 'sketch_regions')
            self.assertEqual(result['metric_errors'], [])
            self.assertFalse((folder / 'cadir.json').exists())
            self.assertEqual(queries.call_args.args[-2:], ('--head', '1'))

    def test_query_failure_is_recorded(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp) / 'result'
            def command(*args):
                (folder / 'dump.json').write_text('{}')
                return 124
            with patch.object(PAIRED, 'command', side_effect=command), \
                    patch.object(PAIRED, 'query', side_effect=ValueError('query losses exit 1')):
                result = PAIRED.measure(Path('binary'), Path('input.f3d'), folder)
            self.assertEqual(result['dump_exit'], 124)
            self.assertEqual(result['check_exit'], 124)
            self.assertIsNone(result['check_errors'])
            self.assertEqual(result['metric_errors'], ['query losses exit 1'])

    def test_query_timeout_and_envelope(self):
        with tempfile.TemporaryDirectory() as temp:
            prefix = Path(temp) / 'query'
            prefix.with_suffix('.out').write_text(json.dumps({'status': 'ok', 'view': 'losses', 'payload': []}))
            with patch.object(PAIRED, 'command', return_value=0) as command:
                self.assertEqual(PAIRED.query(Path('binary'), 'losses', Path('report'), prefix), [])
                self.assertEqual(command.call_args.kwargs, {'seconds': 60})
            with patch.object(PAIRED, 'command', return_value=124), self.assertRaises(ValueError):
                PAIRED.query(Path('binary'), 'losses', Path('report'), prefix)

    def test_failed_decode_keeps_loss_counts_unavailable(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp) / 'result'
            def command(binary, args, prefix, seconds=900):
                if args[0] == 'dump':
                    (folder / 'dump.json').write_text('{}')
                    return 1
                (folder / 'check.json').write_text('{}')
                return 1
            with patch.object(PAIRED, 'command', side_effect=command), \
                    patch.object(PAIRED, 'query', return_value={'decode_report': None}):
                result = PAIRED.measure(Path('binary'), Path('input.f3d'), folder)
            self.assertIsNone(result['losses'])
            self.assertIsNone(result['check_errors'])
            self.assertEqual(result['metric_errors'], [])

    def test_diff_exposes_loss_and_check_changes_and_feature_details(self):
        base = {'dump_exit': 0, 'check_exit': 1, 'check_errors': 2, 'losses': {'f3d.profile': 3},
                'cadir_sha256': 'old', 'features': {'ext': {'name': 'Extrude1', 'profile': 'native', 'sha256': 'old'}}}
        head = copy.deepcopy(base)
        head.update(check_exit=0, check_errors=0, losses={'f3d.profile': 1, 'f3d.other': 1}, cadir_sha256='new')
        head['features']['ext'].update(profile='sketch_regions', sha256='new')
        gains, regressions, details = PAIRED.changes(base, head)
        self.assertIn('f3d.profile -2', gains)
        self.assertIn('check errors -2', gains)
        self.assertEqual(regressions, ['f3d.other +1'])
        self.assertTrue(any('native → sketch_regions' in line for line in details))
        self.assertTrue(any('cadir_sha256: old → new' in line for line in details))
        head['features'] = None
        self.assertIn('feature comparison unavailable for one commit', PAIRED.changes(base, head)[2])

    def test_report_fails_for_missing_shards(self):
        with tempfile.TemporaryDirectory() as temp, contextlib.redirect_stdout(io.StringIO()) as output:
            status = PAIRED.report(Path(temp))
        self.assertEqual(status, 1)
        self.assertIn('Pairs received: 0/178', output.getvalue())
        self.assertIn('RailCore_Mini: missing result', output.getvalue())

    def test_fetch_verifies_hash_and_preserves_literal_source_paths(self):
        row = {'name': 'sample', 'url': 'https://raw.githubusercontent.com/owner/repo/' + 'a' * 40 + '/[sample].f3d',
               'sha256': hashlib.sha256(b'public file').hexdigest()}
        for content, expected_error in ((b'public file', False), (b'wrong bytes', True)):
            with self.subTest(content=content), tempfile.TemporaryDirectory() as temp, working_directory(temp):
                def git(args, **kwargs):
                    if 'checkout' in args:
                        self.assertIn('--literal-pathspecs', args)
                        work = Path(args[args.index('-C') + 1])
                        self.assertEqual((work / 'paths.nul').read_bytes(), b'[sample].f3d\0')
                        (work / '[sample].f3d').write_bytes(content)
                with patch.object(PAIRED, 'selected', return_value=[row]), patch.object(PAIRED.subprocess, 'run', side_effect=git):
                    PAIRED.fetch()
                errors = json.loads(Path('fetch-errors.json').read_text())
                self.assertEqual('sample' in errors, expected_error)
                self.assertFalse(Path('paired-work/0').exists())

    def test_complete_report_and_metric_failure(self):
        row = {'name': 'sample', 'url': 'https://raw.githubusercontent.com/owner/repo/' + 'a' * 40 + '/sample.f3d',
               'sha256': 'c' * 64}
        data = {'commit': 'a' * 40, 'dump_exit': 0, 'check_exit': 0, 'check_errors': 0,
                'losses': {}, 'features': {}, 'metric_errors': [], 'cadir_sha256': 'd' * 64}
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest = root / 'manifest.tsv'
            manifest.write_text('\t'.join(row.values()) + '\n')
            (root / 'paired-plan').mkdir()
            (root / 'paired-plan/paired-plan.json').write_text(json.dumps({
                'base': 'a' * 40, 'head': 'b' * 40, 'manifest_sha256': PAIRED.digest(manifest)}))
            (root / 'paired-shard-s00').mkdir()
            result = dict(row, sides={'base': data, 'head': dict(data, commit='b' * 40)})
            target = root / 'paired-shard-s00/sample.json'
            for error, expected_status in (([], 0), (['query failure'], 1)):
                result['sides']['head']['metric_errors'] = error
                target.write_text(json.dumps(result))
                with patch.object(PAIRED, 'MANIFEST', manifest), contextlib.redirect_stdout(io.StringIO()) as output:
                    self.assertEqual(PAIRED.report(root), expected_status)
                self.assertIn('Pairs received: 1/1', output.getvalue())

    def test_command_sets_per_file_timeout_and_captures_logs(self):
        with tempfile.TemporaryDirectory() as temp:
            prefix = Path(temp) / 'dump'
            with patch.object(PAIRED.subprocess, 'run') as run:
                run.return_value.returncode = 124
                self.assertEqual(PAIRED.command(Path('cadmpeg'), ['dump', 'sample.f3d'], prefix), 124)
                self.assertEqual(run.call_args.args[0],
                                 ['timeout', '--kill-after=30', '900', 'cadmpeg', 'dump', 'sample.f3d'])
            self.assertTrue(prefix.with_suffix('.out').exists())
            self.assertTrue(prefix.with_suffix('.err').exists())


if __name__ == '__main__':
    unittest.main()
