#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Fetch pinned public Fusion files and compare two GitHub runner builds."""
import collections
import csv
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
from urllib.parse import quote, unquote, urlsplit

MANIFEST = Path('ci/paired-inputs.tsv')
CHUNK = 3


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def source(url):
    parsed = urlsplit(url)
    if parsed.scheme != 'https' or parsed.netloc != 'raw.githubusercontent.com':
        raise ValueError(f'expected a public raw GitHub URL: {url}')
    owner, repo, commit, path = parsed.path.lstrip('/').split('/', 3)
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError(f'expected a pinned commit: {url}')
    path = unquote(path)
    if '..' in Path(path).parts or Path(path).is_absolute():
        raise ValueError(f'unsafe source path: {path}')
    return owner, repo, commit, path


def inputs():
    with MANIFEST.open() as stream:
        rows = list(csv.reader((line for line in stream if not line.startswith('#')), delimiter='\t'))
    result = {}
    for name, url, sha in rows:
        if not re.fullmatch(r'[A-Za-z0-9_-]+', name) or name in result:
            raise ValueError(f'invalid or duplicate input name: {name}')
        if not re.fullmatch(r'[0-9a-f]{64}', sha):
            raise ValueError(f'invalid SHA-256: {name}')
        source(url)
        result[name] = {'name': name, 'url': url, 'sha256': sha}
    if not result:
        raise ValueError('empty input manifest')
    return result


def selected():
    rows = inputs()
    return [rows[name] for name in os.environ['IDS'].split()]


def resolve(repository, ref):
    if ref == 'origin/main' and repository == 'cadmpeg/cadmpeg':
        ref = 'main'
    if re.fullmatch(r'[0-9a-f]{40}', ref):
        return ref
    subprocess.run(['git', 'check-ref-format', 'refs/heads/' + ref], check=True)
    output = subprocess.check_output(
        ['timeout', '120', 'git', '-c', 'credential.helper=', 'ls-remote',
         'https://github.com/' + repository + '.git', 'refs/heads/' + ref], text=True)
    lines = output.splitlines()
    if len(lines) != 1:
        raise ValueError(f'branch lookup failed: {repository}:{ref}')
    return lines[0].split()[0]


def plan():
    rows = list(inputs())
    matrix = {'include': [
        {'shard': f's{index // CHUNK:02d}', 'ids': ' '.join(rows[index:index + CHUNK])}
        for index in range(0, len(rows), CHUNK)]}
    values = {'matrix': matrix, 'manifest_sha256': digest(MANIFEST)}
    for side, repository in [('base', 'cadmpeg/cadmpeg'), ('head', 'raykholo/cadmpeg')]:
        ref = os.environ[side.upper() + '_REF']
        values[side] = resolve(repository, ref)
        values[side + '_ref'] = ref
        values[side + '_repository'] = repository
    Path('paired-plan.json').write_text(json.dumps(values, indent=2) + '\n')
    with open(os.environ['GITHUB_OUTPUT'], 'a') as stream:
        for key in ('matrix', 'base', 'head'):
            value = json.dumps(values[key]) if key == 'matrix' else values[key]
            stream.write(f'{key}={value}\n')


def input_path(row):
    return Path('corpus') / row['name'] / Path(source(row['url'])[3]).name


def fetch():
    groups = collections.defaultdict(list)
    for row in selected():
        groups[source(row['url'])[:3]].append(row)
    errors = {}
    for index, ((owner, repo, commit), rows) in enumerate(groups.items()):
        work = Path('paired-work') / str(index)
        work.mkdir(parents=True)
        env = dict(os.environ, GIT_LFS_SKIP_SMUDGE='1', GIT_TERMINAL_PROMPT='0')
        def git(*args):
            subprocess.run(['timeout', '600', 'git', '-C', str(work),
                            '-c', 'credential.helper=', *args], check=True, env=env)
        try:
            git('init', '-q')
            git('remote', 'add', 'origin', f'https://github.com/{owner}/{repo}.git')
            git('fetch', '-q', '--depth', '1', '--filter=blob:none', 'origin', commit)
            paths = [source(row['url'])[3] for row in rows]
            (work / 'paths.nul').write_bytes(b'\0'.join(path.encode() for path in paths) + b'\0')
            git('--literal-pathspecs', 'checkout', '-q', 'FETCH_HEAD',
                '--pathspec-from-file=paths.nul', '--pathspec-file-nul')
            for row in rows:
                try:
                    origin = work / source(row['url'])[3]
                    target = input_path(row)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with origin.open('rb') as stream:
                        lfs = stream.read(100).startswith(b'version https://git-lfs.github.com/spec/v1')
                    if lfs:
                        url = f'https://media.githubusercontent.com/media/{owner}/{repo}/{commit}/'
                        url += quote(source(row['url'])[3])
                        subprocess.run(['timeout', '600', 'curl', '--fail', '--location', '--retry', '3',
                                        '--max-time', '600', url, '-o', str(target)], check=True)
                    else:
                        shutil.copyfile(origin, target)
                    if digest(target) != row['sha256']:
                        raise ValueError('SHA-256 mismatch')
                except (OSError, ValueError, subprocess.CalledProcessError) as error:
                    errors[row['name']] = str(error)
        except (OSError, subprocess.CalledProcessError) as error:
            errors.update((row['name'], str(error)) for row in rows)
        finally:
            shutil.rmtree(work)
    Path('fetch-errors.json').write_text(json.dumps(errors, indent=2) + '\n')


QUERY_INPUT_LIMIT = 256 * 1024 * 1024


def command(binary, args, prefix, seconds=900):
    with prefix.with_suffix('.out').open('w') as out, prefix.with_suffix('.err').open('w') as err:
        return subprocess.run(['timeout', '--kill-after=30', str(seconds), str(binary), *map(str, args)],
                              stdout=out, stderr=err).returncode


def query(binary, view, artifact, prefix, *args):
    code = command(binary, ['query', view, artifact, *args, '--json'], prefix, seconds=60)
    if code:
        raise ValueError(f'query {view} exit {code}')
    envelope = json.loads(prefix.with_suffix('.out').read_text())
    if envelope.get('view') != view or envelope.get('status') != 'ok':
        raise ValueError(f'invalid query {view} envelope')
    return envelope['payload']


def features(records):
    result = {}
    for record in records:
        definition = record.get('definition') or {}
        if definition.get('kind') in ('operation', 'post_process') and 'value' in definition:
            # Tagged form: {kind: operation|post_process, value: ...}; an
            # operation is {kind: <family>, value: {...}}.
            wrapped = definition.get('value') or {}
            operation = wrapped.get('operation', {}) if definition.get('kind') == 'post_process' else wrapped
            family = operation.get('kind')
            body = operation.get('value') or {}
        else:
            operation = definition.get('operation', {}) if definition.get('definition') == 'post_process' else definition
            family = operation.get('definition')
            body = operation
        profile = None
        if family == 'extrude':
            profile = (body.get('profile') or {}).get('kind', 'missing')
        elif family in ('native', 'unresolved') and str(body.get('kind', body.get('family', ''))).lower() == 'extrude':
            profile = family
        key = record['id']
        if key in result:
            raise ValueError(f'duplicate feature id: {key}')
        result[key] = {'name': record.get('name'), 'family': family, 'profile': profile,
                       'sha256': hashlib.sha256(json.dumps(record, sort_keys=True,
                                                          separators=(',', ':')).encode()).hexdigest()}
    return result


def measure(binary, path, folder):
    folder.mkdir(parents=True)
    cadir, dump_report, check_report = [folder / name for name in ('cadir.json', 'dump.json', 'check.json')]
    result = {'dump_exit': command(binary, ['dump', path, '-o', cadir, '--report', dump_report], folder / 'dump-log'),
              'check_exit': command(binary, ['check', path, '-o', check_report], folder / 'check-log'),
              'check_errors': None, 'losses': None, 'features': None, 'cadir_sha256': None,
              'metric_errors': []}
    try:
        if dump_report.exists():
            summary = query(binary, 'summary', dump_report, folder / 'dump-summary')
            if 'decode_losses' in summary:
                losses = query(binary, 'losses', dump_report, folder / 'losses')
                result['losses'] = dict(collections.Counter(loss['code'] for loss in losses))
            elif result['dump_exit'] == 0:
                raise ValueError('successful dump has no decode section')
        elif result['dump_exit'] == 0:
            raise ValueError('successful dump has no report')
        if check_report.exists():
            summary = query(binary, 'summary', check_report, folder / 'check-summary')
            if 'findings' in summary:
                findings = query(binary, 'findings', check_report, folder / 'findings')
                result['check_errors'] = sum(row['severity'] in ('error', 'blocking') for row in findings)
            elif result['check_exit'] == 0:
                raise ValueError('check has no findings section')
        elif result['check_exit'] == 0:
            raise ValueError('check has no report')
        if result['dump_exit'] == 0:
            result['cadir_sha256'] = digest(cadir)
            if cadir.stat().st_size > QUERY_INPUT_LIMIT:
                # `cadmpeg query` refuses inputs above its 256 MiB limit; read the
                # feature arena directly for large assemblies.
                records = (json.loads(cadir.read_text()).get('model') or {}).get('features') or []
            else:
                counts = query(binary, 'counts', cadir, folder / 'counts')
                count = counts.get('model.features', 0)
                records = query(binary, 'item', cadir, folder / 'features', 'model.features', '--head', str(count)) if count else []
                if len(records) != count:
                    raise ValueError('feature query count mismatch')
            result['features'] = features(records)
    except (OSError, ValueError, KeyError, TypeError) as error:
        result['metric_errors'].append(str(error))
    for artifact in folder.glob('cadir*'):
        artifact.unlink()
    return result


def run():
    fetch_errors = json.loads(Path('fetch-errors.json').read_text())
    for row in selected():
        result = dict(row, sides={})
        path = input_path(row)
        if row['name'] in fetch_errors:
            result['fetch_error'] = fetch_errors[row['name']]
        elif not path.is_file() or digest(path) != row['sha256']:
            result['fetch_error'] = 'input missing or SHA-256 mismatch'
        else:
            for side in ('base', 'head'):
                binary = Path('binary') / ('cadmpeg-' + side)
                folder = Path('paired-results') / row['name'] / side
                result['sides'][side] = measure(binary.resolve(), path, folder)
                result['sides'][side]['commit'] = (Path('binary') / (side + '.sha')).read_text().strip()
        target = Path('paired-results') / (row['name'] + '.json')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
        if path.exists():
            path.unlink()


def cell(value):
    return str(value).replace('|', '\\|').replace('\n', ' ')


def changes(base, head):
    gains, regressions, details = [], [], []
    for command_name in ('dump', 'check'):
        field = command_name + '_exit'
        if (base[field] == 0) != (head[field] == 0):
            label = command_name + (' recovered' if head[field] == 0 else ' failed')
            (gains if head[field] == 0 else regressions).append(label)
    for field in ('dump_exit', 'check_exit', 'check_errors', 'cadir_sha256'):
        if base[field] != head[field]:
            details.append(f'{field}: {base[field]} → {head[field]}')
    if base['check_errors'] is not None and head['check_errors'] is not None:
        delta = head['check_errors'] - base['check_errors']
        if delta:
            (gains if delta < 0 else regressions).append(f'check errors {delta:+d}')
    if base['losses'] is not None and head['losses'] is not None:
        for code in sorted(base['losses'].keys() | head['losses'].keys()):
            delta = head['losses'].get(code, 0) - base['losses'].get(code, 0)
            if delta:
                (gains if delta < 0 else regressions).append(f'{code} {delta:+d}')
    if base['features'] is None or head['features'] is None:
        if base['features'] != head['features']:
            details.append('feature comparison unavailable for one commit')
        return gains, regressions, details
    for key in sorted(base['features'].keys() | head['features'].keys()):
        before = (base['features'] or {}).get(key)
        after = (head['features'] or {}).get(key)
        if before != after:
            details.append(f'feature {key} ({(after or before)["name"]}): '
                           f'{before["profile"] if before else "absent"} → '
                           f'{after["profile"] if after else "absent"}; record changed')
    return gains, regressions, details


def report(root):
    rows = inputs()
    files = list(root.glob('paired-shard-*/*.json'))
    found = {}
    for file in files:
        row = json.loads(file.read_text())
        if row['name'] in found:
            raise ValueError(f'duplicate result: {row["name"]}')
        found[row['name']] = row
    plan_file = root / 'paired-plan' / 'paired-plan.json'
    planned = json.loads(plan_file.read_text()) if plan_file.exists() else {}
    print('# Paired public Fusion measurement\n')
    print(f'Base: `{planned.get("base_repository")}@{planned.get("base")}`. '
          f'Head: `{planned.get("head_repository")}@{planned.get("head")}`.\n')
    print('CADIR hashes cover the output bytes. Feature hashes cover canonical query records. '
          'Gains and regressions count dump recovery/failure, check errors, and losses per code.\n')
    print('| File | Gains | Regressions | Dump exit (base → head) | Check exit / errors (base → head) | Extrude profiles (base → head) |\n|---|---|---|---|---|---|')
    incomplete = []
    details = []
    if planned.get('manifest_sha256') != digest(MANIFEST):
        incomplete.append('missing plan or manifest hash mismatch')
    for name in found.keys() - rows.keys():
        incomplete.append(f'{name}: unexpected input')
    for name, expected in rows.items():
        row = found.get(name)
        if not row or row.get('fetch_error') or set(row['sides']) != {'base', 'head'}:
            incomplete.append(f'{name}: {row.get("fetch_error", "missing pair") if row else "missing result"}')
            continue
        if any(row.get(key) != expected[key] for key in ('url', 'sha256')):
            incomplete.append(f'{name}: input identity mismatch')
        base, head = row['sides']['base'], row['sides']['head']
        for side, data in row['sides'].items():
            if data['commit'] != planned.get(side) or data['metric_errors']:
                incomplete.append(f'{name}/{side}: commit mismatch or metric errors {data["metric_errors"]}')
        gains, regressions, changed = changes(base, head)
        checks = f'{base["check_exit"]} / {base["check_errors"]} → {head["check_exit"]} / {head["check_errors"]}'
        profiles = []
        for data in (base, head):
            counts = collections.Counter(feature['profile'] for feature in (data['features'] or {}).values()
                                         if feature['profile'] is not None)
            profiles.append(', '.join(f'{kind}={count}' for kind, count in sorted(counts.items()))
                            if data['features'] is not None else 'unavailable')
        dumps = f'{base["dump_exit"]} → {head["dump_exit"]}'
        print('| ' + ' | '.join(map(cell, (name, '; '.join(gains), '; '.join(regressions), dumps, checks,
                                         ' → '.join(profiles)))) + ' |')
        if changed:
            details.extend([f'\n## {name}\n', *('- ' + cell(line) for line in changed)])
    print('\n' + '\n'.join(details))
    print(f'\nPairs received: {len(found)}/{len(rows)}. Incomplete: {len(incomplete)}.\n')
    for item in incomplete:
        print('- ' + cell(item))
    return int(bool(incomplete))


if __name__ == '__main__':
    action = sys.argv[1]
    if action == 'report':
        sys.exit(report(Path(sys.argv[2])))
    {'plan': plan, 'fetch': fetch, 'run': run}[action]()
