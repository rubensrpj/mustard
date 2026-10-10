// Explicit paid pilot over supplied development questions. Requires an API
// key in the environment; no credentials, source checkout, or host install
// are changed. Never executes the benchmark project's code.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const flags=new Map();
for(let at=2;at<process.argv.length;at+=2) flags.set(process.argv[at].slice(2),process.argv[at+1]);
const required=name=>{assert.ok(flags.get(name),`Missing --${name}`);return path.resolve(flags.get(name));};
const source=required('source'),bins=required('bins'),questionsPath=required('questions'),output=required('out');
assert.ok(process.env.TYPESAFE_API_KEY,'Provide TYPESAFE_API_KEY explicitly for this paid pilot');
const questions=JSON.parse(fs.readFileSync(questionsPath));
assert.ok(Array.isArray(questions.questions) && questions.questions.length);
assert.ok(questions.questions.every(q=>/^[A-Za-z0-9_-]+$/.test(q.id)),'Question ids must be safe output names');
const temporary=fs.mkdtempSync(path.join(os.tmpdir(),'mustard-jev-choice-'));
const root=path.join(temporary,'source');
const env={...process.env,MUSTARD_RT_DELEGATED:'1',CLAUDE_CONFIG_DIR:path.join(temporary,'host'),MUSTARD_SPEND_DIR:path.join(temporary,'usage')};
for(const key of ['MUSTARD_JEV_URL','CLOUDFLARE_API_TOKEN','CLAUDE_PLUGIN_ROOT','MUSTARD_ACTIVE_SPEC'])delete env[key];
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
function run(program,args,cwd=root) {
 const start=performance.now();const result=spawnSync(program,args,{cwd,env,encoding:'utf8',timeout:180000,maxBuffer:100*1024*1024});
 assert.equal(result.status,0,`${path.basename(program)} failed: ${result.stderr || result.error?.message || result.stdout.slice(0,500)}`);
 return {text:result.stdout,ms:Math.round(performance.now()-start)};
}
const state=()=>({head:run('git',['rev-parse','HEAD'],source).text.trim(),status:run('git',['status','--porcelain=v1','-z'],source).text});
const before=state();assert.equal(before.status,'');
const config={language:{text:'pt-BR',code:'en-US'},ai:{fallback:false,vectors:false},search:{filter:'none'},judgement:{search:{filter:'jev'},'wave-planning':{filter:'none'},context:{filter:'none'}}};
const writeConfig=()=>fs.writeFileSync(path.join(root,'mustard.json'),JSON.stringify(config),{mode:0o600});
const rt=path.join(bins,'mustard-rt');
const retrieve=query=>{const reading=run(rt,['run','knowledge','--root',root,'--query',query,'--responsibility']);return {...reading,report:JSON.parse(reading.text)};};
const hit=(report,q)=>({file:report.cards.some(c=>q.expected.includes(c.source.file)),
 symbol:q.symbols?.length?report.cards.some(c=>q.expected.includes(c.source.file)&&q.symbols.includes(c.name)):null});
try {
 run('git',['clone','--quiet','--no-hardlinks','--no-checkout',source,root],temporary);
 run('git',['-c','core.hooksPath=/dev/null','checkout','--quiet','--detach',before.head]);
 const excluded=run('git',['ls-files','-z']).text.split('\0').filter(f=>/\.md$/i.test(f)||/(^|\/)\.env($|\.)/.test(f)||/(^|\/)(\.claude|\.codex)(\/|$)/.test(f)||f==='mustard.json');
 for(const file of excluded)fs.rmSync(path.join(root,file),{force:true});
 writeConfig();run(path.join(bins,'scan'),['scan',root,'--out',path.join(root,'.claude/grain.db'),'--json']);
 fs.mkdirSync(output,{recursive:true});
 const report={source_commit:before.head,question_sha256:sha(fs.readFileSync(questionsPath)),executable_sha256:sha(fs.readFileSync(rt)),
  method:'Development-only paired native/optional-choice retrieval; unchanged supplied queries; first and repeated query; no host model, not independent quality scoring or billed-token savings.',questions:[]};
 for(const q of questions.questions.filter(q=>q.id!=='N11')) {
  config.ai.fallback=false;writeConfig();const native=retrieve(q.query);assert.equal(native.report.remote_model_calls,0);
  const row={id:q.id,query:q.query,expected:q.expected,symbols:q.symbols,native:{...hit(native.report,q),ms:native.ms,bytes:Buffer.byteLength(native.text),ambiguities:native.report.responsibility_selection?.ambiguous_files}};
  if(row.native.ambiguities>0) {
   config.ai.fallback=true;writeConfig();
   const first=retrieve(q.query);
   row.choice={...hit(first.report,q),ms:first.ms,bytes:Buffer.byteLength(first.text),usage:first.report.responsibility_selection.usage,remote_model_calls:first.report.remote_model_calls};
   if(row.choice.usage.status==='jev-choice') {
    const repeat=retrieve(q.query);
    row.repeat={usage:repeat.report.responsibility_selection.usage,remote_model_calls:repeat.report.remote_model_calls};
    assert.equal(repeat.report.remote_model_calls,0,'Repeat must use semantic request cache');
    fs.writeFileSync(path.join(output,`${q.id}-repeat.json`),JSON.stringify(repeat.report,null,2));
   }
   for(const [label,value] of [['native',native.report],['choice',first.report]])fs.writeFileSync(path.join(output,`${q.id}-${label}.json`),JSON.stringify(value,null,2));
  }
  report.questions.push(row);console.log(JSON.stringify({id:q.id,ambiguities:row.native.ambiguities,native:row.native.symbol,choice:row.choice?.symbol,usage:row.choice?.usage.status}));
  if(row.choice?.usage.status==='native-fallback') {report.stopped_after_provider_failure=true;break;}
 }
 config.ai.fallback=true;writeConfig();
 for(const query of ['UnknownUnicornIdentifier'])assert.equal(retrieve(query).report.remote_model_calls,0);
 const existing=report.questions.find(q=>q.choice?.usage.status==='jev-choice');
 if(existing) {
  const read=retrieve(existing.query);const card=read.report.cards[0];
  const exact=JSON.parse(run(rt,['run','knowledge','--root',root,'--symbol',card.id,'--detail']).text);
  assert.equal(exact.remote_model_calls,0);report.exact_and_missing_zero_calls=true;
 }
 const rows=name=>fs.existsSync(path.join(root,'.claude/judgements',name))?fs.readFileSync(path.join(root,'.claude/judgements',name),'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];
 report.physical_attempts=rows('attempts.ndjson');report.logical_requests=rows('requests.ndjson');
 report.known_input_tokens=report.physical_attempts.reduce((sum,r)=>sum+(r.input_tokens??0),0);
 report.requests_with_unknown_usage=report.physical_attempts.filter(r=>!Number.isInteger(r.input_tokens)).length;
 report.estimated_cost_micro_usd=report.requests_with_unknown_usage?null:Math.round(report.known_input_tokens*0.042);
 report.original_unchanged=true;assert.deepEqual(state(),before);
 fs.writeFileSync(path.join(output,'pilot.json'),JSON.stringify(report,null,2));
 console.log(JSON.stringify({file:path.join(output,'pilot.json'),physical_attempts:report.physical_attempts.length,input_tokens:report.known_input_tokens,cost_micro_usd:report.estimated_cost_micro_usd}));
} finally {fs.rmSync(temporary,{recursive:true,force:true});assert.deepEqual(state(),before);}
