// Frozen backend cases. Offline by default; --jev explicitly enables paid choices.
import fs from 'node:fs';
import path from 'node:path';
import cp from 'node:child_process';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';

const checkout = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const args = process.argv.slice(2);
const option = name => args[args.indexOf(name) + 1];
assert.ok(args.includes('--root'), 'Supply --root with the isolated backend snapshot');
const root = path.resolve(option('--root'));
const paid = args.includes('--jev');
const env = { ...process.env, MUSTARD_RT_DELEGATED: '1' };
if (paid) assert.ok(env.TYPESAFE_API_KEY, '--jev requires TYPESAFE_API_KEY in the environment');
const binary = path.join(checkout, 'target/debug/mustard-rt');
const fixture = JSON.parse(fs.readFileSync(path.join(checkout, 'apps/scan/tests/fixtures/gateway-responsibility-20261009.json')));
const revision = cp.spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' });
assert.equal(revision.status, 0, revision.stderr);
assert.equal(revision.stdout.trim(), fixture.source_commit, 'Use the same source commit for this comparison');

function run(request, flag) {
  const result = cp.spawnSync(binary, ['run', 'search', '--root', root, '--request', JSON.stringify(request), ...(flag ? [flag] : [])],
    { cwd: root, env, maxBuffer: 16 * 1024 * 1024 });
  if (result.error) throw result.error;
  assert.ok([0, 1].includes(result.status), result.stderr.toString());
  return result;
}

function restore(bytes) {
  const text = bytes.toString();
  if (!text.startsWith('@ ') && !text.startsWith('# ')) return text;
  let file = '';
  const lines = [];
  for (const line of text.split('\n')) {
    if (line.startsWith('@ ')) file = line.slice(2);
    else if (line && !line.startsWith('# ')) {
      assert.ok(file);
      lines.push(`${file}:${line}`);
    }
  }
  return lines.join('\n') + (text.endsWith('\n') ? '\n' : '');
}

const cases = [];
for (const item of fixture.cases) {
  // Keep this frozen responsibility/parity suite on the original-occurrence
  // representation. gateway-task.mjs exercises task evidence separately.
  const request = { ...item.request, purpose:'locate', choose: paid && item.request.choose };
  const report = JSON.parse(run(request).stdout);
  const native = run({ ...request, choose: false }, '--raw');
  assert.equal(report.result.stdout, native.stdout.toString());
  assert.equal(report.result.stderr, native.stderr.toString());
  assert.equal(report.exit_code, native.status);
  const view = run(request, '--shell-output').stdout;
  assert.equal(restore(view), native.stdout.toString(), `${item.id}: original occurrences changed`);
  const evidence = report.evidence;
  if (paid) {
    const selected = evidence.symbols.filter(card => evidence.recommended_symbols.includes(card.id));
    if (item.expected.name) {
      assert.ok(selected.some(card => card.name === item.expected.name && card.source.file === item.expected.file), item.id);
    } else assert.deepEqual(selected, [], item.id);
    if (item.expected.outcome) assert.equal(evidence.selection.outcomes.responsibility, item.expected.outcome, item.id);
    if (item.expected.native_only) assert.equal(report.remote_model_calls, 0, item.id);
    if (item.expected.outcome === 'no-match') assert.ok(view.toString().includes('# selection: no-match'));
    const repeat = JSON.parse(run(request).stdout);
    assert.equal(repeat.remote_model_calls, 0, `${item.id}: repeated choice should use cache`);
  } else assert.equal(report.remote_model_calls, 0, item.id);
  cases.push({ id: item.id, native_bytes: native.stdout.length, agent_bytes: view.length,
    native_result_preserved: true, selection: evidence.selection, recommended: evidence.recommended_symbols });
}
const result = { source_commit: fixture.source_commit, paid_opt_in: paid, cases,
  limitations: 'Known development cases; bytes are not billed tokens. This does not execute backend code or measure an implementation session.' };
if (args.includes('--out')) fs.writeFileSync(option('--out'), JSON.stringify(result, null, 2) + '\n');
process.stdout.write(JSON.stringify(result, null, 2) + '\n');
