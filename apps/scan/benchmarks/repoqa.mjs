// Development-only retrieval evaluation. Reads a frozen public corpus;
// never builds/runs its code, invokes a model, or changes host configuration.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const flags = new Map();
for (let at = 2; at < process.argv.length; at += 2) {
  assert.ok(process.argv[at]?.startsWith('--') && process.argv[at + 1], 'Expected --name value pairs');
  flags.set(process.argv[at].slice(2), process.argv[at + 1]);
}
function required(name) {
  assert.ok(flags.has(name), `Missing --${name}`);
  return path.resolve(flags.get(name));
}
const datasetPath = required('dataset');
const selectionPath = required('selection');
const binaries = { baseline: required('baseline'), current: required('current') };
const output = required('out');
const responsibility = flags.get('responsibility') === 'true';
assert.ok(!flags.has('responsibility') || ['true','false'].includes(flags.get('responsibility')), '--responsibility must be true or false');
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const raw = fs.readFileSync(datasetPath);
const dataset = JSON.parse(raw);
const selection = JSON.parse(fs.readFileSync(selectionPath));
assert.equal(sha(raw), selection.dataset_json_sha256, 'Dataset changed');
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-repoqa-'));
const env = { ...process.env, MUSTARD_RT_DELEGATED: '1', CLAUDE_CONFIG_DIR: path.join(temporary, 'host-state'), MUSTARD_SPEND_DIR: path.join(temporary, 'usage') };
for (const name of ['TYPESAFE_API_KEY', 'MUSTARD_JEV_URL', 'CLOUDFLARE_API_TOKEN', 'CLAUDE_PLUGIN_ROOT', 'MUSTARD_ACTIVE_SPEC']) delete env[name];
fs.mkdirSync(output, { recursive: true });
const report = {
  responsibility_experiment: responsibility,
  dataset_url: selection.dataset_url, dataset_json_sha256: sha(raw), selection_sha256: sha(fs.readFileSync(selectionPath)),
  method: 'Adapted retrieval test, not official RepoQA scoring. All supplied source files and ten unchanged descriptions for each selected repository. Top eight source cards; file/name/location separately. Original comments retained. No code execution or inference. Fixed order, exploratory latency.',
  binaries: Object.fromEntries(Object.entries(binaries).map(([version, directory]) => [version,
    Object.fromEntries(['scan', 'mustard-rt'].map(name => [name, sha(fs.readFileSync(path.join(directory, name)))]))])),
  results: [], repositories: [], local_model_calls: 0, remote_model_calls: 0,
};
function run(version, program, args, cwd) {
  const start = performance.now();
  const result = spawnSync(path.join(binaries[version], program), args, { cwd, env, encoding: 'utf8', timeout: 180000, maxBuffer: 128 * 1024 * 1024 });
  assert.equal(result.status, 0, `${program} failed: ${result.stderr || result.error?.message || result.stdout.slice(0, 500)}`);
  return { value: JSON.parse(result.stdout), ms: Math.round(performance.now() - start), bytes: Buffer.byteLength(result.stdout) };
}
function writeSource(root, relative, content) {
  assert.equal(typeof content, 'string');
  assert.ok(!path.isAbsolute(relative) && !relative.includes('\\') && !relative.includes('\0'));
  assert.ok(relative.split('/').every(part => part !== '..' && part !== '.git' && part !== '.claude' && part !== '.codex'));
  const destination = path.resolve(root, relative);
  assert.ok(destination.startsWith(root + path.sep));
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.writeFileSync(destination, content);
}
const nameOf = name => name.split(/::|[.#]/).at(-1);
function isSymbol(card, needle) {
  return card.source.file === needle.path && nameOf(card.name) === nameOf(needle.name)
    && card.source.line <= needle.end_line + 1 && needle.start_line + 1 <= card.source.end_line;
}
function excerpts(root, cards) {
  for (const card of cards) {
    const excerpt = card.source_excerpt;
    if (!excerpt) continue;
    const source = fs.readFileSync(path.join(root, card.source.file), 'utf8');
    assert.equal(sha(source), card.source.sha256);
    const lines = source.split(/\r?\n/);
    for (const line of excerpt.text.split('\n')) {
      const matched = /^(\d+) \| (.*)$/.exec(line); assert.ok(matched, 'Invalid numbered source excerpt');
      const at = Number(matched[1]);
      assert.ok(at >= card.source.line && at <= card.source.end_line);
      assert.ok(lines[at - 1].startsWith(matched[2]), 'Excerpt differs from actual source');
    }
  }
}
function complete(card) {
  const excerpt = card.source_excerpt;
  return !!excerpt && !excerpt.truncated && excerpt.line <= card.source.line && excerpt.end_line >= card.source.end_line;
}
try {
  for (let repoAt = 0; repoAt < selection.repositories.length; repoAt++) {
    const picked = selection.repositories[repoAt];
    const repo = dataset[picked.language].find(repo => repo.repo === picked.repo && repo.commit_sha === picked.commit);
    assert.ok(repo); assert.equal(repo.needles.length, picked.needles);
    assert.equal(Object.keys(repo.content).length, picked.files);
    assert.equal(Object.values(repo.content).reduce((n, text) => n + Buffer.byteLength(text), 0), picked.bytes);
    for (const version of repoAt % 2 ? ['current', 'baseline'] : ['baseline', 'current']) {
      const root = path.join(temporary, `${repoAt}-${version}`); fs.mkdirSync(root);
      for (const [relative, content] of Object.entries(repo.content)) writeSource(root, relative, content);
      assert.ok(!Object.hasOwn(repo.content, 'mustard.json'));
      fs.writeFileSync(path.join(root, 'mustard.json'), JSON.stringify({ language: { text: 'en-US', code: 'en-US' }, ai: { fallback: false, vectors: false }, search: { filter: 'none' } }));
      const index = run(version, 'scan', ['scan', root, '--out', path.join(root, '.claude/grain.db'), '--json'], root);
      assert.equal(index.value.remote_model_calls, 0); assert.equal(index.value.vectors_enabled, false);
      const audit = run(version, 'mustard-rt', ['run', 'map', 'audit', '--root', root], root).value;
      assert.equal(audit.ok, true, JSON.stringify(audit));
      report.repositories.push({ ...picked, version, scan_ms: index.ms, scan: index.value, audit, database_bytes: fs.statSync(path.join(root, '.claude/grain.db')).size });
      const files = new Map();
      for (let at = 0; at < repo.needles.length; at++) {
        const needle = repo.needles[at];
        assert.equal(typeof needle.description, 'string');
        if (!files.has(needle.path)) files.set(needle.path, run(version, 'mustard-rt', ['run', 'knowledge', '--root', root, '--file', needle.path, '--all', '--detail'], root).value.cards);
        const expected = files.get(needle.path).some(card => isSymbol(card, needle));
        const query = run(version, 'mustard-rt', ['run', 'knowledge', '--root', root, '--query', needle.description,
          ...(version === 'current' && responsibility ? ['--responsibility'] : [])], root);
        assert.equal(query.value.remote_model_calls, 0); assert.equal(query.value.local_model_calls, 0);
        for (const card of query.value.cards) assert.equal(sha(fs.readFileSync(path.join(root, card.source.file))), card.source.sha256);
        const alternatives = query.value.cards.flatMap(card => card.alternatives || []);
        excerpts(root, [...query.value.cards, ...alternatives]);
        const target = query.value.cards.filter(card => isSymbol(card, needle));
        const alternativeHit = alternatives.some(card => isSymbol(card, needle));
        let editing;
        if (version === 'current' && !responsibility) {
          editing = run(version, 'mustard-rt', ['run','knowledge','--root',root,'--query',needle.description,'--purpose','implement'], root);
          assert.equal(editing.value.local_model_calls,0); assert.equal(editing.value.remote_model_calls,0);
          excerpts(root, editing.value.cards);
          fs.writeFileSync(path.join(output, `${repoAt}-${at}-implement.json`), JSON.stringify(editing.value));
        }
        const row = { version, language: picked.language, repo: picked.repo, commit: picked.commit, id: `${picked.repo}:${at}`, query_sha256: sha(needle.description),
          expected: { file: needle.path, name: needle.name, start_line: needle.start_line, end_line: needle.end_line, lines_zero_based: true },
          index_coverage: expected, file_hit: query.value.cards.some(card => card.source.file === needle.path), symbol_hit: query.value.cards.some(card => isSymbol(card, needle)),
          alternative_symbol_hit: alternativeHit, target_current_excerpt: target.some(card => !!card.source_excerpt),
          complete_primary_declaration: target.some(complete),
          implement: editing && {bytes:editing.bytes,ms:editing.ms,complete_primary_declaration:editing.value.cards.filter(card=>isSymbol(card,needle)).some(complete)},
          failure_stage: !expected ? 'target-not-indexed' : !query.value.cards.some(card=>card.source.file===needle.path) ? 'file-not-retrieved'
            : !target.length ? alternativeHit ? 'target-only-in-alternatives' : 'symbol-not-selected' : target.some(card=>!!card.source_excerpt) ? 'current-excerpt-returned' : 'target-without-source-excerpt',
          implementation_sufficiency:'unverified; full indexed declaration does not prove sufficient task context',
          ms: query.ms, bytes: query.bytes, selected: query.value.cards.map(card => ({ name: card.name, source: card.source })), catalog: query.value.catalog };
        report.results.push(row);
        fs.writeFileSync(path.join(output, `${repoAt}-${at}-${version}.json`), JSON.stringify(query.value));
        console.log(JSON.stringify({ stage: 'query', version, language: picked.language, id: at, file: row.file_hit, symbol: row.symbol_hit, coverage: expected }));
      }
      fs.rmSync(root, { recursive: true, force: true });
      fs.writeFileSync(path.join(output, 'benchmark-partial.json'), JSON.stringify(report, null, 2));
    }
  }
  report.summary = Object.fromEntries(Object.keys(binaries).map(version => {
    const rows = report.results.filter(row => row.version === version);
    return [version, { total: rows.length, indexed_symbols: rows.filter(row => row.index_coverage).length,
      file_hits: rows.filter(row => row.file_hit).length, symbol_hits: rows.filter(row => row.symbol_hit).length,
      alternative_rescues:rows.filter(row=>!row.symbol_hit && row.alternative_symbol_hit).length,
      target_current_excerpts:rows.filter(row=>row.target_current_excerpt).length,
      complete_primary_declarations:rows.filter(row=>row.complete_primary_declaration).length,
      implement_complete_primary_declarations:rows.filter(row=>row.implement?.complete_primary_declaration).length,
      failure_stages:Object.fromEntries([...new Set(rows.map(row=>row.failure_stage))].map(stage=>[stage,rows.filter(row=>row.failure_stage===stage).length])),
      bytes: rows.reduce((n, row) => n + row.bytes, 0), median_ms:rows.map(row=>row.ms).sort((a,b)=>a-b)[Math.floor(rows.length/2)] }];
  }));
  fs.writeFileSync(path.join(output, 'benchmark.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ stage: 'complete', summary: report.summary }));
} finally {
  fs.rmSync(temporary, { recursive: true, force: true });
}
