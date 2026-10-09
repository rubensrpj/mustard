// Native acceptance only: isolated fixture, no repository code execution or paid API.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import cp from 'node:child_process';
import http from 'node:http';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { fileURLToPath } from 'node:url';

const checkout = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const bin = path.join(checkout, 'target/debug');
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-search-acceptance-'));
const root = path.join(temporary, 'main');
fs.mkdirSync(root);
const env = { ...process.env, MUSTARD_RT_DELEGATED: '1', CLAUDE_CONFIG_DIR: path.join(temporary, 'host'),
  MUSTARD_SPEND_DIR: path.join(temporary, 'usage'), PATH: bin + path.delimiter + process.env.PATH };
for (const key of ['TYPESAFE_API_KEY', 'MUSTARD_JEV_URL', 'CLOUDFLARE_API_TOKEN', 'CLAUDE_PLUGIN_ROOT',
  'CLAUDE_PROJECT_DIR', 'MUSTARD_WORKSPACE_ROOT', 'MUSTARD_ACTIVE_SPEC']) delete env[key];
let httpRequests = 0;
const server = http.createServer((_, response) => { httpRequests++; response.writeHead(500); response.end('Unexpected inference'); });
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
function run(program, args, cwd = root, input) {
  const executable = ['mustard', 'mustard-rt', 'scan'].includes(program) ? path.join(bin, program) : program;
  const output = cp.spawnSync(executable, args, { cwd, env, input, maxBuffer: 16 * 1024 * 1024 });
  if (output.error) throw output.error;
  return output;
}
function successful(program, args, cwd = root) {
  const output = run(program, args, cwd);
  assert.equal(output.status, 0, output.stderr.toString() || output.stdout.toString());
  return output;
}
function search(args, cwd = root) {
  const output = run('mustard-rt', ['run', 'search', '--root', cwd, ...args], cwd);
  assert.ok([0, 1].includes(output.status), output.stderr.toString() || output.stdout.toString());
  return JSON.parse(output.stdout);
}
function hook(tool, input, cwd = root) {
  const output = run('mustard-rt', ['on', 'PreToolUse'], cwd, JSON.stringify({ hook_event_name: 'PreToolUse',
    tool_name: tool, cwd, session_id: 'gateway-native-fixture', tool_input: input }));
  assert.equal(output.status, 0, output.stderr.toString());
  return JSON.parse(output.stdout).hookSpecificOutput;
}
try {
  assert.deepEqual(fs.readdirSync(root), []);
  successful('mustard', ['init', '--yes']);
  const sessionMap=fs.readFileSync(path.join(root,'.claude/mustard/session-map.md'),'utf8');
  assert.ok(sessionMap.includes('{request:{tool,input,intent,purpose,choose?}}'));
  assert.ok(sessionMap.includes('mustard:spec:'));
  assert.ok(sessionMap.includes('corpos completos') || sessionMap.includes('complete bodies'));
  const configFile = path.join(root, 'mustard.json');
  const config = JSON.parse(fs.readFileSync(configFile));
  config.ai = { fallback: true, vectors: true };
  fs.writeFileSync(configFile, JSON.stringify(config));
  env.TYPESAFE_API_KEY = 'acceptance-only-invalid-key';
  env.MUSTARD_JEV_URL = `http://127.0.0.1:${server.address().port}/v1/systemone`;
  fs.mkdirSync(path.join(root, 'src'));
  fs.writeFileSync(path.join(root, 'src/lib.rs'), '/// Stores a quartz snapshot.\npub fn persist() { let quartz = 1; }\n');
  const argv = ['rg', '--sort=path', '-n', '--with-filename', 'let quartz', 'src'];
  const first = search(['--', ...argv]);
  assert.equal(first.learning.new_facts, 1,JSON.stringify({learning:first.learning,fallback:first.fallback}));
  assert.equal(first.learning.scan.status, 'refreshed-native');
  assert.equal(first.learning.needs_scan, false);
  assert.equal(first.evidence.symbols[0].name, 'persist');
  assert.equal(first.evidence.symbols[0].read.input.offset, 2);
  assert.equal(first.remote_model_calls, 0);
  const repeat = search(['--', ...argv]);
  assert.equal(repeat.learning.new_facts, 0);
  assert.equal(repeat.learning.reused_facts, 1);
  assert.equal(repeat.learning.needs_scan, false);
  assert.equal(repeat.learning.scan, undefined);
  const oldSchema=new DatabaseSync(path.join(root,'.claude/grain.db'));
  oldSchema.prepare("UPDATE blocks SET version=15 WHERE name='decls'").run();oldSchema.close();
  const migrated=search(['--','rg','-n','--with-filename','absent-upgrade-sentinel','src']);
  assert.equal(migrated.exit_code,1);
  assert.equal(migrated.learning.scan.status,'refreshed-native');
  assert.equal(migrated.learning.needs_scan,false);
  assert.equal(search(['--',...argv]).evidence.symbols[0].name,'persist');
  fs.writeFileSync(path.join(root, 'src/lib.rs'), '/// Stores the revised snapshot.\npub fn revised() { let quartz = 2; }\n');
  fs.writeFileSync(path.join(root, 'src/new.rs'), 'pub fn discovered() { let quartz = 3; }\n');
  const changed = search(['--', ...argv]);
  assert.equal(changed.learning.new_facts, 2);
  assert.equal(changed.learning.scan.status, 'refreshed-native');
  assert.deepEqual(changed.evidence.symbols.map(s => s.name).sort(), ['discovered', 'revised']);
  assert.equal(changed.evidence.intent_requested, true);
  assert.equal(changed.remote_model_calls, 0);

  const isolatedBin = path.join(temporary, 'runtime-without-scan');
  fs.mkdirSync(isolatedBin);
  fs.copyFileSync(path.join(bin, 'mustard-rt'), path.join(isolatedBin, 'mustard-rt'));
  const rgPath = successful('sh', ['-c', 'command -v rg']).stdout.toString().trim();
  fs.symlinkSync(rgPath, path.join(isolatedBin, 'rg'));
  fs.writeFileSync(path.join(root, 'src/pending.rs'), 'pub fn later_indexed() { let pending_quartz = 4; }\n');
  const pendingRun = cp.spawnSync(path.join(isolatedBin, 'mustard-rt'), ['run', 'search', '--root', root,
    '--', 'rg', '-n', '--with-filename', 'pending_quartz', 'src'], {cwd:root, env:{...env, PATH:isolatedBin}});
  assert.equal(pendingRun.status, 0, pendingRun.stderr.toString());
  const pending = JSON.parse(pendingRun.stdout);
  assert.equal(pending.learning.scan.status, 'pending');
  assert.equal(pending.learning.needs_scan, true);
  assert.ok(pending.result.stdout.includes('pending_quartz'));
  const recovered = search(['--', 'rg', '-n', '--with-filename', 'pending_quartz', 'src']);
  assert.equal(recovered.learning.scan.status, 'refreshed-native');
  assert.equal(recovered.evidence.symbols[0].name, 'later_indexed');

  for (const args of [argv.slice(1), ['-n', 'absent-sentinel', 'src'], ['-n', '[', 'src'], ['--sort=path', '-c', 'quartz', 'src']]) {
    const original = run('rg', args);
    const gateway = run('mustard-rt', ['run', 'search', '--root', root, '--raw', '--', 'rg', ...args]);
    assert.deepEqual(gateway.stdout, original.stdout);
    assert.deepEqual(gateway.stderr, original.stderr);
    assert.equal(gateway.status, original.status);
  }
  fs.writeFileSync(path.join(root, 'src/unsupported.cpp'), 'void unsupported_sentinel() {}\n');
  const raw = run('rg', ['-n', '--with-filename', 'unsupported_sentinel', 'src/unsupported.cpp']);
  const fallback = run('mustard-rt', ['run', 'search', '--root', root, '--shell-output', '--', 'rg', '-n', '--with-filename', 'unsupported_sentinel', 'src/unsupported.cpp']);
  assert.deepEqual(fallback.stdout, raw.stdout);
  assert.deepEqual(fallback.stderr, raw.stderr);
  assert.equal(fallback.status, raw.status);

  const typed = { tool: 'Grep', input: { pattern: 'quartz', path: 'src', output_mode: 'content', '-n': true, head_limit: 1, offset: 1 }, intent: 'inspect persistence', purpose: 'implement' };
  const typedResult = search(['--request', JSON.stringify(typed)]);
  assert.equal(typedResult.result.numLines, 1);
  assert.equal(typedResult.intent, typed.intent);
  const read = search(['--request', JSON.stringify({ tool: 'Read', input: { file_path: 'src/lib.rs', offset: 2, limit: 1 } })]);
  assert.equal(read.evidence.symbols[0].name, 'revised');
  const glob = search(['--request', JSON.stringify({ tool: 'Glob', input: { pattern: '**/*.rs', path: 'src' } })]);
  assert.equal(glob.result.numFiles, 3);
  const routed = hook('Grep', typed.input);
  assert.equal(routed.permissionDecision, 'deny');
  assert.ok(routed.permissionDecisionReason.includes('mcp__mustard__search'));
  assert.ok(routed.permissionDecisionReason.includes(JSON.stringify(typed.input)));
  const noTools=path.join(temporary,'no-native-search');
  fs.mkdirSync(noTools);
  const unavailable=cp.spawnSync(path.join(bin,'mustard-rt'),['on','PreToolUse'],{cwd:root,env:{...env,PATH:noTools},
    input:JSON.stringify({hook_event_name:'PreToolUse',tool_name:'Grep',cwd:root,tool_input:typed.input})});
  assert.equal(unavailable.status,0,unavailable.stderr.toString());
  const passed=unavailable.stdout.toString().trim()?JSON.parse(unavailable.stdout):{};
  assert.notEqual(passed.hookSpecificOutput?.permissionDecision,'deny','Without rg, the original host tool must stay usable');
  const original = run('rg', argv.slice(1));
  const rewritten = hook('Bash', { command: "rg --sort=path -n --with-filename 'let quartz' src", description: 'inspect persistence', timeout: 5000 });
  assert.ok(rewritten.updatedInput.command.includes('run search'));
  assert.equal(rewritten.updatedInput.timeout, 5000);
  const executed = run('sh', ['-c', rewritten.updatedInput.command]);
  assert.equal(executed.status, 0, executed.stderr.toString());
  assert.deepEqual(executed.stdout, original.stdout, 'Small searches keep native output; diagnostics must not be appended');
  // A multi-file rg can order files differently between two executions. Keep
  // this cross-execution size check on one file; pagination is checked above.
  const agentRequest={...typed,purpose:'locate',input:{...typed.input,path:'src/lib.rs',offset:0}};
  const agentExpected=search(['--request',JSON.stringify(agentRequest)]);
  const typedAgent = successful('mustard-rt', ['run','search','--root',root,'--shell-output','--request',JSON.stringify(agentRequest)]);
  assert.ok(typedAgent.stdout.length <= Buffer.byteLength(JSON.stringify(agentExpected.result))+1);
  assert.ok(!typedAgent.stdout.toString().includes('source_hashes'));
  assert.ok(!typedAgent.stdout.toString().includes('local_model_calls'));
  const taskRequest={...agentRequest,intent:'revised quartz snapshot',purpose:'spec'};
  const taskResult=search(['--request',JSON.stringify(taskRequest)]);
  assert.equal(taskResult.task_context.status,'current-task-evidence');
  const taskView=successful('mustard-rt',['run','search','--root',root,'--shell-output','--request',JSON.stringify(taskRequest)]);
  assert.ok(taskView.stdout.toString().includes('# task evidence'));
  assert.ok(taskView.stdout.toString().includes('purpose=locate'));
  const versionedTask=search(['--request',JSON.stringify({schema_version:1,request:taskRequest})]);
  assert.equal(versionedTask.task_context.status,'current-task-evidence');
  assert.equal(versionedTask.intent,taskRequest.intent);
  const annotated=hook('Bash',{command:"rg --sort=path -n --with-filename 'let quartz' src/lib.rs",description:'mustard:spec: revised quartz snapshot'});
  assert.ok(annotated.updatedInput.command.includes('"schema_version":1'));
  const annotatedResult=run('sh',['-c',annotated.updatedInput.command]);
  assert.equal(annotatedResult.status,0,annotatedResult.stderr.toString());
  assert.ok(annotatedResult.stdout.toString().includes('# task evidence'));
  for(const invalid of [
    {schema_version:1,request:{tool:'rg',input:{args:['quartz','src']},intent:'question'}},
    {schema_version:1,request:{...taskRequest,intent:''}},
    {schema_version:2,request:taskRequest},
    {schema_version:1,request:{...taskRequest,tool:'References',input:{file_path:'src/lib.rs',line:0,column:0}}},
  ]){
    const refused=run('mustard-rt',['run','search','--root',root,'--request',JSON.stringify(invalid)]);
    assert.equal(refused.status,2);
    const result=JSON.parse(refused.stdout);
    assert.equal(result.executed,false);
    assert.equal(result.remote_model_calls,0);
    assert.ok(result.fallback.startsWith('correct-search-request'));
  }
  fs.writeFileSync(path.join(root,'src/fresh.py'),"def restore():\n    return 'fresh_oracle_snapshot'\n");
  const discovered=search(['--request',JSON.stringify({...typed,input:{pattern:'let quartz',path:'src',output_mode:'content'},intent:'fresh_oracle_snapshot',purpose:'spec'})]);
  assert.equal(discovered.task_context.learning.scan.status,'refreshed-native');
  assert.ok(discovered.task_context.cards.some(card=>card.name==='restore'));
  assert.equal(discovered.remote_model_calls,0);
  for(const request of [{tool:'Read',input:{file_path:'src/lib.rs',offset:2,limit:1}},{tool:'Glob',input:{pattern:'**/*.rs',path:'src'}}]){
    const diagnostic=search(['--request',JSON.stringify(request)]);
    const agent=successful('mustard-rt',['run','search','--root',root,'--shell-output','--request',JSON.stringify(request)]);
    const result=JSON.parse(agent.stdout);
    if(request.tool==='Glob'){
      // Separate native enumerations need not order files alike. The unit test
      // checks exact field/order preservation for one executed Answer.
      result.filenames.sort();diagnostic.result.filenames.sort();
    }
    assert.deepEqual(result,diagnostic.result,'Typed Read/Glob retain their complete result contract');
  }
  fs.writeFileSync(path.join(root, 'private.pem'), 'secret-fixture-sentinel');
  const refused = run('mustard-rt', ['run', 'search', '--root', root, '--request', JSON.stringify({ tool: 'Read', input: { file_path: 'private.pem' } })]);
  assert.equal(refused.status, 2);
  assert.ok(!refused.stdout.toString().includes('secret-fixture-sentinel'));

  successful('git', ['init', '-q']);
  successful('git', ['add', 'src']);
  successful('git', ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-q', '-m', 'fixture']);
  const linked = path.join(temporary, 'linked');
  successful('git', ['worktree', 'add', '-q', '-b', 'linked-fixture', linked]);
  fs.writeFileSync(path.join(linked, 'src/lib.rs'), 'pub fn linked_only() { let quartz = 4; }\n');
  const linkedResult = search(['--', ...argv], linked);
  assert.ok(linkedResult.evidence.symbols.some(s => s.name === 'linked_only'));
  assert.ok(fs.existsSync(path.join(linked, '.claude/grain.db')));
  assert.notEqual(linkedResult.learning.tree, first.learning.tree);
  const linkedSymbol=linkedResult.evidence.symbols.find(s=>s.name==='linked_only');
  const expanded=JSON.parse(successful('mustard-rt',linkedSymbol.expand.args,linked).stdout);
  assert.ok(expanded.cards.some(s=>s.name==='linked_only'));
  const main = search(['--', ...argv]);
  assert.ok(main.evidence.symbols.some(s => s.name === 'revised'));
  assert.ok(!main.evidence.symbols.some(s => s.name === 'linked_only'));
  assert.equal(main.learning.needs_scan, false);
  fs.writeFileSync(path.join(root,'src/focused.rs'),'pub fn read_current() { fetch(); }\npub fn fetch() { let quartz_snapshot=3; }\npub fn archive() { let report_snapshot=4; }\n');
  const focusedRequest={tool:'rg',input:{args:['-n','--with-filename','read_current','src/focused.rs']},intent:'recover quartz snapshot report',purpose:'spec',choose:true};
  const focused=search(['--request',JSON.stringify({schema_version:1,request:focusedRequest})]);
  assert.equal(focused.task_context.selection_basis,'exact-symbol-identity');
  assert.equal(focused.remote_model_calls,0);
  const focusedView=successful('mustard-rt',['run','search','--root',root,'--request',JSON.stringify({schema_version:1,request:focusedRequest}),'--shell-output']).stdout.toString();
  assert.ok(focusedView.includes('pub fn read_current()'));
  assert.ok(focusedView.includes('let quartz_snapshot=3'));
  assert.equal(focused.task_context.chain.steps.length,1);
  assert.ok(!focusedView.includes('let report_snapshot=4'));
  assert.ok(focusedView.includes('Native follow-up:'));
  fs.writeFileSync(path.join(root,'src/reuse.rs'),'pub fn retain_context() {\n'+Array.from({length:30},(_,i)=>`    let snapshot_${i}=${i};\n`).join('')+'}\n');
  const reuseRequest={...focusedRequest,input:{args:['-n','retain_context','src/reuse.rs']},intent:'inspect retain_context',purpose:'implement',choose:false};
  const context={session:'native-test',agent:'main',epoch:'first',acknowledged:[]};
  const delivered=JSON.stringify({schema_version:1,request:reuseRequest,context});
  const firstBody=successful('mustard-rt',['run','search','--root',root,'--request',delivered,'--shell-output']).stdout.toString();
  const receipt=firstBody.match(/# mustard-delivery:([a-f0-9]{64})/)[1];
  assert.ok(firstBody.includes('let snapshot_29=29'));
  const reused=successful('mustard-rt',['run','search','--root',root,'--request',JSON.stringify({schema_version:1,request:reuseRequest,context:{...context,acknowledged:[receipt]}}),'--shell-output']).stdout.toString();
  assert.ok(reused.includes('already delivered'));
  assert.ok(!reused.includes('let snapshot_29=29'));
  assert.ok(Buffer.byteLength(reused)<Buffer.byteLength(firstBody));
  const firstVisible=firstBody.replace(/\n# mustard-delivery:[a-f0-9]{64}:\d+:[a-f0-9]{16}\n?$/,'');
  const freshContext=successful('mustard-rt',['run','search','--root',root,'--request',JSON.stringify({schema_version:1,request:reuseRequest,context:{...context,epoch:'after-compact',acknowledged:[receipt]}}),'--shell-output']).stdout.toString();
  assert.ok(freshContext.includes('let snapshot_29=29'));
  const expandedRanges=JSON.parse(successful('mustard-rt',['run','map','summary','--root',root,'--file','src/focused.rs']).stdout);
  assert.ok(expandedRanges.parts.some(part=>part.name==='fetch'));
  assert.ok(expandedRanges.parts.some(part=>part.name==='archive'));
  fs.writeFileSync(path.join(root, 'src/structural.rs'), 'pub fn discovered_by_structure() { revised(); }\n');
  const structure = search(['--request', JSON.stringify({schema_version:1,request:{tool:'Structure',
    input:{file_path:'src/structural.rs',query:'(call_expression) @call'},intent:'Find current call owners',purpose:'understand',choose:false}})]);
  assert.equal(structure.learning.scan.status, 'refreshed-native');
  assert.ok(structure.result.owners.symbols.some(symbol => symbol.name === 'discovered_by_structure'));
  assert.equal(structure.remote_model_calls, 0);

  await new Promise(resolve => setImmediate(resolve));
  assert.equal(httpRequests, 0, 'Neither ordinary gateway nor native learning may call inference');
  const report = { ok: true, commit: successful('git', ['rev-parse', 'HEAD'], checkout).stdout.toString().trim(),
    truly_empty_native_install: true, automatic_native_refresh: true, repeat_deduplicates_without_rescan: true,
    changed_and_new_sources: true, failed_scan_keeps_native_and_retries_later: true,
    raw_stdout_stderr_status_parity: true, native_fallback_parity: true,
    automatic_output_does_not_append_reports: true, typed_agent_pagination: true,
    typed_tools: true, migrated_index_refreshes_after_empty_native_search:true, native_search_unavailable_passes_host_tool: true,
    task_evidence_visible_to_agent:true, complementary_new_source_refreshes_natively:true,
    named_declaration_without_inference:true, typed_operation_contract:true, acknowledged_body_reuse_and_compaction_reset:true, native_dependency_body:true,
    delivery_bytes:{initial:Buffer.byteLength(firstVisible),repeated:Buffer.byteLength(reused),initial_transport:Buffer.byteLength(firstBody),reduction_percent:100*(Buffer.byteLength(firstVisible)-Buffer.byteLength(reused))/Buffer.byteLength(firstVisible)}, structural_result_recrossed_after_refresh:true, deferred_candidates_expand_via_binary:true,
    real_classic_hook_handoff_and_rewrite: true, original_read_guard: true,
    checkout_isolation: true, default_http_requests: httpRequests, local_model_calls: 0, remote_model_calls: 0,
    binaries: ['mustard', 'mustard-rt', 'scan'].map(name => ({ name,
      sha256: crypto.createHash('sha256').update(fs.readFileSync(path.join(bin, name))).digest('hex') })) };
  const output = path.join(checkout, 'target/scan-gateway-20261009');
  fs.mkdirSync(output, { recursive: true });
  fs.writeFileSync(path.join(output, 'native-acceptance.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report));
} finally {
  await new Promise(resolve => server.close(resolve));
  fs.rmSync(temporary, { recursive: true, force: true });
}
