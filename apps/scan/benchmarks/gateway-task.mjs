// Replays known searches as task evidence. Offline unless --jev is explicit.
import fs from 'node:fs';
import path from 'node:path';
import cp from 'node:child_process';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
const checkout=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..');
const args=process.argv.slice(2);
const option=name=>args[args.indexOf(name)+1];
assert.ok(args.includes('--root'),'Supply --root with the isolated frozen backend snapshot');
const root=path.resolve(option('--root'));
const paid=args.includes('--jev');
const env={...process.env,MUSTARD_RT_DELEGATED:'1'};
if(paid) assert.ok(env.TYPESAFE_API_KEY,'--jev requires TYPESAFE_API_KEY');
else {delete env.TYPESAFE_API_KEY;delete env.MUSTARD_JEV_URL;}
const binary=path.join(args.includes('--bin')?path.resolve(option('--bin')):path.join(checkout,'target/debug'),'mustard-rt');
const fixture=JSON.parse(fs.readFileSync(path.join(checkout,'apps/scan/tests/fixtures/gateway-task-20261009.json')));
const revision=()=>cp.execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim();
assert.equal(revision(),fixture.source_commit,'Use the frozen source commit');
const state=cp.execFileSync('git',['status','--porcelain'],{cwd:root,encoding:'utf8'});
function run(request,flag) {
  const start=performance.now();
  const out=cp.spawnSync(binary,['run','search','--root',root,'--request',JSON.stringify(request),...(flag?[flag]:[])],{cwd:root,env,maxBuffer:32*1024*1024});
  if(out.error)throw out.error;
  assert.ok([0,1].includes(out.status),out.stderr.toString());
  return {...out,ms:Math.round(performance.now()-start)};
}
const lines=new Set();
function sourceLines(view) {
  let file='';let count=0;
  for(const row of view.toString().split('\n')) {
    if(row.startsWith('@ '))file=row.slice(2);
    const match=/^(\d+) \| (.*)$/.exec(row);
    if(!match || !file || !fs.existsSync(path.join(root,file)))continue;
    const original=fs.readFileSync(path.join(root,file),'utf8').split(/\r?\n/)[Number(match[1])-1];
    assert.ok(original?.startsWith(match[2]),`Stale or invented source at ${file}:${match[1]}`);
    if(original===match[2]){lines.add(`${file}:${match[1]}`);count++;}
  }
  return count;
}
const cases=[];
const bodies=new Set();
const declarations=new Set();
for(const item of fixture.cases) {
  const result=run(item.request);const report=JSON.parse(result.stdout);
  assert.equal(report.remote_model_calls,0,item.id);
  const locate={...item.request,purpose:'locate',choose:false};
  const original=JSON.parse(run(locate).stdout);
  if(item.request.tool==='Grep') {
    assert.deepEqual([...report.result.filenames].sort(),[...original.result.filenames].sort());
  } else {
    assert.deepEqual(report.result,original.result);assert.equal(report.exit_code,original.exit_code);
  }
  const view=run(item.request,'--shell-output').stdout;
  const native=run(locate,'--shell-output').stdout;
  const context=report.task_context;
  assert.equal(context.status,'current-task-evidence',item.id);
  assert.ok(view.toString().startsWith('# task evidence'),item.id);
  assert.ok(view.toString().includes('purpose=locate'));
  let complete=0;
  for(const card of context.cards) {
    const source=fs.readFileSync(path.join(root,card.source.file));
    assert.equal(crypto.createHash('sha256').update(source).digest('hex'),card.source.sha256);
    declarations.add(`${card.source.file}:${card.name}`);
    if(card.initial_source_excerpt && card.source_excerpt.truncated===false) {bodies.add(`${card.source.file}:${card.name}`);complete++;}
  }
  for(const reference of context.static_references||[])if(reference.initial_reference)declarations.add(`${reference.source.file}:${reference.name}`);
  cases.push({id:item.id,request:item.request,native_agent_bytes:native.length,task_agent_bytes:view.length,ms:result.ms,
    complete_bodies:complete,current_source_lines:sourceLines(view),native_result_preserved:true,
    primary:context.cards.filter(card=>card.retrieval!=='alternative').map(card=>({name:card.name,source:card.source,complete:!card.source_excerpt.truncated})),
    clues:context.written_clues,scope:context.scope,remote_model_calls:report.remote_model_calls});
}
const required=[
  ['src/puzzle/pi/services/pi-auxiliar.service.ts','processPlanBackground'],
  ['src/puzzle/services/plantio-download-orchestrator.service.ts','listData'],
  ['src/puzzle/pi/services/plan/format-plan.service.ts','duplicateLineage'],
  ['src/puzzle/pi/services/plan/duplicate-plan-files.service.ts','copyPlanFiles'],
  ['src/common/services/excel.service.ts','createXlsxStream'],
  ['src/puzzle/pi/services/plan-input/optimizer-output-source.service.ts','toMenuRow'],
  ['src/puzzle/pi/services/menu/search-menu.service.ts','listMenu'],
  ['src/puzzle/pi/services/curve/plantio-curve.service.ts','plantioCurveToDownload'],
].map(([file,name])=>({file,name,located:declarations.has(`${file}:${name}`),complete_body_delivered:bodies.has(`${file}:${name}`)}));
const choices=[];
if(paid) {
  const choiceFixture=JSON.parse(fs.readFileSync(path.join(checkout,'apps/scan/tests/fixtures/gateway-responsibility-20261009.json')));
  for(const item of choiceFixture.cases) {
    const report=JSON.parse(run(item.request).stdout);
    const context=item.request.purpose==='locate'?report.evidence:report.task_context;
    const symbols=context.cards||context.symbols;
    const selected=symbols.filter(card=>context.recommended_symbols.includes(card.id));
    const outcome=context.selection.outcomes?.responsibility;
    const ok=item.expected.name?selected.some(card=>card.name===item.expected.name && card.source.file===item.expected.file):outcome===item.expected.outcome;
    const repeat=JSON.parse(run(item.request).stdout);
    assert.equal(repeat.remote_model_calls,0,'Repeated decisions must reuse the cache');
    choices.push({id:item.id,ok,recommended:selected.map(card=>card.name),outcome,usage:context.selection,stages:context.selection_stages});
  }
}
assert.equal(revision(),fixture.source_commit);
assert.equal(cp.execFileSync('git',['status','--porcelain'],{cwd:root,encoding:'utf8'}),state,'Source checkout changed');
const result={source_commit:fixture.source_commit,binary_sha256:crypto.createHash('sha256').update(fs.readFileSync(binary)).digest('hex'),paid_opt_in:paid,cases,required_evidence:required,
  totals:{search_requests:cases.length,native_agent_bytes:cases.reduce((n,c)=>n+c.native_agent_bytes,0),task_agent_bytes:cases.reduce((n,c)=>n+c.task_agent_bytes,0),unique_current_source_lines:lines.size,located_required:required.filter(item=>item.located).length,complete_required:required.filter(item=>item.complete_body_delivered).length},choices,
  limitations:['Known development case, not independent accuracy.','Task packets include source that locate does not deliver; byte totals are not an equivalent-work or billed-token comparison.','Complete function bodies and located references do not prove a correct spec or implementation.','No reduction in whole-session tool calls, tokens, cost or time is claimed.']};
if(args.includes('--out'))fs.writeFileSync(option('--out'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({totals:result.totals,required_evidence:required,choices},null,2));
assert.ok(choices.every(choice=>choice.ok),'A known responsibility expectation failed; inspect the saved report');
