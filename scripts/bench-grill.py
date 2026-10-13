#!/usr/bin/env python3
"""Compare grill output on an isolated, clean checkout. No model calls.

The source project and its spec are read only. Reports remain in a temporary
fixture. Bytes measure transport, not net LLM token savings.
"""
import argparse
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--root', required=True, type=Path)
parser.add_argument('--spec', required=True)
parser.add_argument('--before-event', required=True, type=int)
parser.add_argument('--baseline', required=True, type=Path)
parser.add_argument('--candidate', required=True, type=Path)
parser.add_argument('--out', type=Path)
args = parser.parse_args()
assert re.fullmatch(r'[A-Za-z0-9_-]+', args.spec), 'invalid spec name'
source = args.root.resolve()
assert not subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']), 'commit tracked source changes before comparing'
fixture = Path(tempfile.mkdtemp(prefix='mustard-grill-benchmark-'))
subprocess.run(['git', 'clone', '--quiet', '--shared', '--no-hardlinks', str(source), str(fixture)], check=True)
config = json.loads((source / 'mustard.json').read_text(encoding='utf-8'))
config.pop('jev', None)
config.update(ai={'fallback': False, 'vectors': False}, judgement={}, localFiles=[])
(fixture / 'mustard.json').write_text(json.dumps(config), encoding='utf-8')
spec = Path('.claude/spec') / args.spec / 'spec.ndjson'
rows = [json.loads(line) for line in (source / spec).read_text(encoding='utf-8').splitlines()]
prefix = ''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in rows if row['id'] < args.before_event)
(fixture / spec).parent.mkdir(parents=True, exist_ok=True)
with sqlite3.connect(f'file:{source / ".claude/grain.db"}?mode=ro', uri=True) as original, sqlite3.connect(fixture / '.claude/grain.db') as copied:
    original.backup(copied)
env = os.environ.copy()
env["MUSTARD_RT_DELEGATED"] = "1"
for key in ['TYPESAFE_API_KEY', 'JEV_API_KEY', 'CLAUDE_PLUGIN_ROOT', 'CLAUDE_PROJECT_DIR', 'MUSTARD_ACTIVE_SPEC', 'CODEX_THREAD_ID']:
    env.pop(key, None)
reports = []
for label, binary in [('baseline', args.baseline.resolve()), ('candidate', args.candidate.resolve())]:
    (fixture / spec).write_text(prefix, encoding='utf-8')
    result = subprocess.run([str(binary), 'run', 'grill', '--root', str(fixture), '--spec', args.spec], cwd=fixture, env=env, capture_output=True, text=True, timeout=120)
    report = json.loads(result.stdout)
    assert result.returncode == 0 and report['ok'], f'{label} grill failed: {report.get("reason")}'
    reports.append((result.stdout, report))
    (fixture / f'{label}.json').write_text(result.stdout, encoding='utf-8')
before, after = reports
summary = {'baseline_bytes': len(before[0].encode()), 'candidate_bytes': len(after[0].encode()),
           'reduction_percent': round((1 - len(after[0].encode()) / len(before[0].encode())) * 100, 2),
           'baseline_point_count': len(before[1]['points']), 'candidate_point_count': len(after[1]['points']),
           'next_point_identical': before[1]['next'] == after[1]['next'], 'model_calls': 0,
           'net_llm_token_savings': 'not measured', 'fixture': str(fixture)}
assert summary['next_point_identical'] and summary['baseline_point_count'] == summary['candidate_point_count']
if args.out:
    args.out.write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
print(json.dumps(summary, indent=2))
