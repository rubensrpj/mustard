// Explicit paid pilot on a frozen, external selection. No expected symbol is
// supplied to the search or provider. The grader alone reads public labels.
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import cp from 'node:child_process';import crypto from 'node:crypto';import assert from 'node:assert/strict';
const args=process.argv.slice(2),get=n=>{assert.ok(args.includes(n),`Missing ${n}`);return args[args.indexOf(n)+1];};
assert.ok(args.includes('--jev'),'Paid pilot requires --jev');
const datasetPath=path.resolve(get('--dataset')),selectionPath=path.resolve(get('--selection')),bin=path.resolve(get('--bin')),out=path.resolve(get('--out'));
for(const name of ['mustard-rt','scan'])fs.accessSync(path.join(bin,name),fs.constants.X_OK);
const raw=fs.readFileSync(datasetPath),selection=JSON.parse(fs.readFileSync(selectionPath)),dataset=JSON.parse(raw);
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');assert.equal(sha(raw),selection.dataset_json_sha256);
let key=process.env.TYPESAFE_API_KEY;if(!key&&args.includes('--config'))key=JSON.parse(fs.readFileSync(get('--config'))).jev?.key;assert.ok(key,'Missing Jev key');
fs.mkdirSync(out,{recursive:true});const temp=fs.mkdtempSync(path.join(os.tmpdir(),'mustard-gateway-choice-'));
const env={...process.env,MUSTARD_RT_DELEGATED:'1',TYPESAFE_API_KEY:key,CLAUDE_CONFIG_DIR:path.join(temp,'host'),MUSTARD_SPEND_DIR:path.join(temp,'usage')};
for(const k of ['MUSTARD_JEV_URL','CLAUDE_PLUGIN_ROOT','MUSTARD_ACTIVE_SPEC'])delete env[k];
const stop=new Set('about after also between being could does every following function implementation implements into other return returns should some specific their there these this those through using values when where which while with within would'.split(' '));
function request(description){const words=[...new Set(description.match(/[A-Za-z_][A-Za-z_0-9]{3,}/g)||[])].filter(w=>!stop.has(w.toLowerCase())).slice(0,12);return {schema_version:1,request:{tool:'rg',input:{args:['--sort=path','-n','--with-filename',words.join('|')||'__no_written_clue__','.']},intent:description.slice(0,4000),purpose:'implement',choose:false}};}
function run(program,argv,root){const r=cp.spawnSync(path.join(bin,program),argv,{cwd:root,env,timeout:120000,maxBuffer:32*1024*1024});if(r.error)throw r.error;assert.ok([0,1].includes(r.status),r.stderr.toString());return r.stdout;}
const match=(c,n)=>Boolean(c?.source.file===n.path&&c.name===n.name&&c.source.line<=n.start_line+1&&c.source.end_line>=n.end_line);
const rows=[];
try {
 for(const [at,picked] of selection.repositories.entries()) {
  const repo=dataset[picked.language].find(r=>r.repo===picked.repo&&r.commit_sha===picked.commit);assert.ok(repo);
  const root=path.join(temp,String(at));fs.mkdirSync(root);
  for(const [file,text] of Object.entries(repo.content)){const p=path.resolve(root,file);assert.ok(p.startsWith(root+path.sep));fs.mkdirSync(path.dirname(p),{recursive:true});fs.writeFileSync(p,text);}
  const vectors=args.includes('--vectors');
  fs.writeFileSync(path.join(root,'mustard.json'),JSON.stringify({language:{text:'en-US',code:'en-US'},ai:{fallback:true,vectors},judgement:{search:{filter:'jev'}}}));
  run('scan',['scan',root,...(vectors?[]:['--native']),'--out',path.join(root,'.claude/grain.db'),'--json'],root);
  // Fixed before results: first two descriptions in every selected language.
  for(const [id,needle] of repo.needles.slice(0,2).entries()) {
   const q=request(needle.description),invoke=envelope=>['run','search','--root',root,'--request',JSON.stringify(envelope)];
   const native=JSON.parse(run('mustard-rt',invoke(q),root));assert.equal(native.remote_model_calls,0);
   q.request.choose=true;const chosen=JSON.parse(run('mustard-rt',invoke(q),root)),ctx=chosen.task_context||{};
   const selected=(ctx.cards||[]).filter(c=>(ctx.recommended_symbols||[]).includes(c.id));
   // A failed transport is not a cache miss to retry with more paid calls.
   const repeated=chosen.remote_model_calls===null?null:JSON.parse(run('mustard-rt',invoke(q),root));
   const nativeView=run('mustard-rt',[...invoke({...q,request:{...q.request,choose:false}}),'--shell-output'],root);
   // Do not execute a paid selection again just to measure its presentation.
   // Report bytes are diagnostic transport size, not the agent-facing view.
   const chosenView=Buffer.from(JSON.stringify(chosen));
   const row={repo:picked.repo,id,query_sha256:sha(needle.description),expected_in_native_pool:(native.task_context?.cards||[]).some(c=>match(c,needle)),
    expected_in_choice_pool:(ctx.cards||[]).some(c=>match(c,needle)),selected:selected.map(c=>({file:c.source.file,name:c.name})),
    native_first_correct:match(native.task_context?.cards?.[0],needle),
    choice_first_correct:selected.length===1?match(selected[0],needle):match(ctx.cards?.[0],needle),
    correct_selection:selected.some(c=>match(c,needle)),wrong_accepted:selected.length>0&&!selected.some(c=>match(c,needle)),abstained:selected.length===0,
    native_status:native.task_context?.status,choice_status:ctx.status,choice_reason:ctx.reason,
    physical_calls:chosen.remote_model_calls,usage:ctx.selection,repeat_calls:repeated?.remote_model_calls??null,
    cache_verified:repeated?.remote_model_calls===0,repeat_usage:repeated?.task_context?.selection,
    native_view_bytes:nativeView.length,chosen_report_bytes:chosenView.length};
   rows.push(row);fs.writeFileSync(path.join(out,`${at}-${id}.json`),JSON.stringify({row,request:q,native,chosen,repeated}));
   fs.writeFileSync(path.join(out,'partial.json'),JSON.stringify(rows,null,2));
  }
 }
 const sum=field=>rows.reduce((n,r)=>n+r[field],0);
 const report={dataset_sha256:sha(raw),selection_sha256:sha(fs.readFileSync(selectionPath)),binary_sha256:sha(fs.readFileSync(path.join(bin,'mustard-rt'))),vectors:args.includes('--vectors'),rows,
  summary:{cases:rows.length,valid_native:rows.filter(r=>r.native_status==='current-task-evidence').length,valid_choice:rows.filter(r=>r.choice_status==='current-task-evidence').length,
   expected_in_native_pool:rows.filter(r=>r.expected_in_native_pool).length,correct_selections:rows.filter(r=>r.correct_selection).length,wrong_accepted:rows.filter(r=>r.wrong_accepted).length,abstained:rows.filter(r=>r.abstained).length,
   native_first_correct:rows.filter(r=>r.native_first_correct).length,choice_first_correct:rows.filter(r=>r.choice_first_correct).length,
   physical_calls:rows.every(r=>r.physical_calls!==null&&r.repeat_calls!==null)?sum('physical_calls')+sum('repeat_calls'):null,
   known_input_tokens:rows.reduce((n,r)=>n+(r.usage?.known_input_tokens||0)+(r.repeat_usage?.known_input_tokens||0),0),
   estimated_micro_usd:rows.every(r=>(r.usage?.usage_complete||r.physical_calls===0)&&(r.repeat_usage?.usage_complete||r.repeat_calls===0))?rows.reduce((n,r)=>n+(r.usage?.cost_micro_usd||0)+(r.repeat_usage?.cost_micro_usd||0),0):null,
   cache_verified:rows.filter(r=>r.cache_verified).length},
  limits:['Eight public descriptions, first two per language, no label hints in search/provider input.','Choice quality measured separately from retrieval; source candidates and abstentions do not prove business behavior.','Configured-price cost estimate, not provider invoice; bytes are not billed host tokens.','No autonomous Claude implementation; whole-task quality and savings remain unmeasured.']};
 fs.writeFileSync(path.join(out,'comparison.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report.summary,null,2));
} finally {
 // Preserve the physical attempt ledger even when a case or assertion fails.
 const usage=path.join(temp,'usage');if(fs.existsSync(usage))fs.cpSync(usage,path.join(out,'physical-usage'),{recursive:true});
 for(const entry of fs.readdirSync(temp,{withFileTypes:true}).filter(e=>e.isDirectory())) {
  const ledger=path.join(temp,entry.name,'.claude/judgements');
  if(fs.existsSync(ledger))fs.cpSync(ledger,path.join(out,'judgements',entry.name),{recursive:true});
 }
 fs.rmSync(temp,{recursive:true,force:true});
}
