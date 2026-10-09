// Paired whole-task evaluation on trusted, authored fixtures. --dry-run is free.
// --execute requires an explicit API key because --bare never reads OAuth.
// Official CLI contract: https://code.claude.com/docs/en/headless
import fs from 'node:fs';import path from 'node:path';import os from 'node:os';
import cp from 'node:child_process';import crypto from 'node:crypto';import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
const checkout=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..');
const args=process.argv.slice(2),option=n=>args[args.indexOf(n)+1];
assert.ok(args.includes('--out'),'Supply --out');
assert.ok(args.includes('--dry-run')!==args.includes('--execute'),'Choose --dry-run or --execute');
const out=path.resolve(option('--out'));fs.mkdirSync(out,{recursive:true});
const execute=args.includes('--execute'),model=args.includes('--model')?option('--model'):null;
if(execute){assert.ok(model,'Pin the same full --model ID for both arms');assert.ok(process.env.ANTHROPIC_API_KEY,'Bare paired runs require ANTHROPIC_API_KEY; subscription OAuth is not loaded');}
const plugin=path.resolve(args.includes('--plugin')?option('--plugin'):path.join(checkout,'target/review-plugin'));
const bin=path.join(checkout,'target/debug');
const fixture={
  'package.json':JSON.stringify({type:'module',scripts:{test:'node --test'}}),
  'src/storage.js':`export function findPlan(store,id) { const plan=store.plans.find(p=>p.id===id); if(!plan) throw new Error('Unknown plan'); return plan; }\n`,
  'src/csv.js':`export function csvCell(value) { const text=String(value??''); return /[",\\r\\n]/.test(text)?'"'+text.replaceAll('"','""')+'"':text; }\n`,
  'src/input-format.js':`export const inputHeaders=['Site','Volume','Note'];\nexport const inputFields=['site','volume','note'];\n`,
  'src/export.js':`import {findPlan} from './storage.js';\nimport {inputHeaders,inputFields} from './input-format.js';\nimport {csvCell} from './csv.js';\nexport function downloadInput(store,id) { findPlan(store,id); throw new Error('Download not implemented'); }\n`,
  'src/summary.js':`export function downloadSummary(plan) { return 'Site;Total\\n'+plan.rows.length; }\n`,
  'src/cancel.js':`import {findPlan} from './storage.js';\nexport function cancelPlan(store,id) { const plan=findPlan(store,id); plan.status='cancelled'; store.audit.push({id,event:'cancel'}); return plan; }\n`,
};
const tasks=[
  {id:'copy-input-export',prompt:'Implement downloadInput(store,id). Export current stored plan rows as CSV in the original input format, preserving row order and the columns defined by the importer. A copied plan must export its own edited rows. Escape commas, quotes and line breaks, retain zero, render missing cells empty. Use CRLF and include a final CRLF. Reject unknown IDs and never mutate the store. Reuse existing format and serialization helpers. Make the code change and validate it.',
    grade:`const {downloadInput}=await import(pathToFileURL(path.join(root,'src/export.js')));\nconst store={plans:[{id:'original',rows:[{site:'old',volume:99,note:'original'}]},{id:'copy',rows:[{site:'A,B',volume:0,note:'say "yes"'},{site:'new',volume:12,note:'two\\nlines'},{site:'last',volume:null}]}]};\nconst before=structuredClone(store);\nassert.equal(downloadInput(store,'copy'),'Site,Volume,Note\\r\\n"A,B",0,"say ""yes"""\\r\\nnew,12,"two\\nlines"\\r\\nlast,,\\r\\n');\nassert.deepEqual(store,before);\nassert.equal(downloadInput({plans:[{id:'empty',rows:[]}]},'empty'),'Site,Volume,Note\\r\\n');\nassert.throws(()=>downloadInput(store,'absent'));`,
    reference:`import {findPlan} from './storage.js';import {inputHeaders,inputFields} from './input-format.js';import {csvCell} from './csv.js';\nexport function downloadInput(store,id) {const p=findPlan(store,id);return [inputHeaders,...p.rows.map(r=>inputFields.map(f=>r[f]))].map(r=>r.map(csvCell).join(',')).join('\\r\\n')+'\\r\\n';}\n`,file:'src/export.js'},
  {id:'idempotent-cancellation',prompt:'Fix cancelPlan(store,id). Pending and ready plans may be cancelled. Completed plans must reject cancellation without changing any data. Cancelling an already cancelled plan must be idempotent and must not append a second audit event. The event is {id,event:"cancel"}. Reject unknown IDs. Return a detached copy so callers cannot mutate storage through the result. Preserve all unrelated plans and fields. Make the code change and validate it.',
    grade:`const {cancelPlan}=await import(pathToFileURL(path.join(root,'src/cancel.js')));\nfor(const status of ['pending','ready']) {const store={plans:[{id:'one',status,rows:[{value:7}]},{id:'other',status:'ready'}],audit:[]};const other=structuredClone(store.plans[1]);const result=cancelPlan(store,'one');assert.equal(result.status,'cancelled');assert.deepEqual(store.audit,[{id:'one',event:'cancel'}]);assert.deepEqual(store.plans[1],other);result.rows[0].value=8;assert.equal(store.plans[0].rows[0].value,7);cancelPlan(store,'one');assert.equal(store.audit.length,1);}\nconst done={plans:[{id:'one',status:'completed',rows:[]}],audit:[]},before=structuredClone(done);assert.throws(()=>cancelPlan(done,'one'));assert.deepEqual(done,before);assert.throws(()=>cancelPlan(done,'absent'));`,
    reference:`import {findPlan} from './storage.js';export function cancelPlan(store,id) {const p=findPlan(store,id);if(p.status==='completed')throw new Error('Completed');if(p.status!=='cancelled'){p.status='cancelled';store.audit.push({id,event:'cancel'});}return structuredClone(p);}\n`,file:'src/cancel.js'},
];
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'mustard-session-pair-'));
const env={...process.env,MUSTARD_RT_DELEGATED:'1',PATH:bin+path.delimiter+process.env.PATH};
for(const key of ['TYPESAFE_API_KEY','MUSTARD_JEV_URL','CLAUDE_PLUGIN_ROOT','CLAUDE_PROJECT_DIR','MUSTARD_WORKSPACE_ROOT','MUSTARD_ACTIVE_SPEC'])delete env[key];
const rows=[];
function create(root){fs.mkdirSync(root,{recursive:true});for(const [file,text] of Object.entries(fixture)){fs.mkdirSync(path.dirname(path.join(root,file)),{recursive:true});fs.writeFileSync(path.join(root,file),text);}}
function grade(root,task){const r=cp.spawnSync(process.execPath,['--input-type=module','-e',`import assert from 'node:assert/strict';import path from 'node:path';import {pathToFileURL} from 'node:url';const root=process.env.FIXTURE_ROOT;${task.grade}`],{cwd:temp,env:{...env,FIXTURE_ROOT:root},timeout:10000});return {passed:r.status===0,error:r.status===0?null:r.stderr.toString().slice(0,3000)};}
function metrics(events){
  const result=events.filter(e=>e.type==='result').at(-1);
  const tools=events.filter(e=>e.type==='assistant').flatMap(e=>e.message?.content||[]).filter(b=>b.type==='tool_use');
  const read=tools.filter(t=>t.name==='Read'||t.name==='mcp__mustard__search'&&t.input?.request?.tool==='Read');
  const names=new Map();let repeated=0;
  for(const t of read){const key=JSON.stringify(t.input?.request?.input||t.input);if(names.has(key))repeated++;names.set(key,true);}
  return {result_received:!!result,host_success:result?.is_error===false,usage:result?.usage??null,
    estimated_cost_usd:result?.total_cost_usd??null,model_usage:result?.modelUsage??null,
    tool_calls:tools.length,read_calls:read.length,repeated_identical_reads:repeated,
    gateway_calls:tools.filter(t=>t.name==='mcp__mustard__search').length,
    limitations:'Tool counts describe model-issued calls; internal native calls and overlapping read ranges are not inferred. Client cost is an estimate.'};
}
try{
  for(const [at,task] of tasks.entries()){
    const dry=path.join(temp,task.id+'-grader');create(dry);assert.equal(grade(dry,task).passed,false,'Original must fail the independent behavior checks');
    fs.writeFileSync(path.join(dry,task.file),task.reference);assert.equal(grade(dry,task).passed,true,'Reference must satisfy behavior checks');
    if(!execute)continue;
    for(const arm of at%2?['mustard','baseline']:['baseline','mustard']){
      const root=path.join(temp,task.id+'-'+arm),armOut=path.join(out,task.id+'-'+arm);create(root);fs.mkdirSync(armOut);
      const host=path.join(temp,task.id+'-'+arm+'-host');fs.mkdirSync(host);
      const childEnv={...env,CLAUDE_CONFIG_DIR:host,MUSTARD_SPEND_DIR:path.join(armOut,'provider-usage')};
      if(arm==='mustard'){
        const init=cp.spawnSync(path.join(bin,'mustard'),['init','--yes'],{cwd:root,env:childEnv,maxBuffer:16*1024*1024});assert.equal(init.status,0,init.stderr.toString());
        const configFile=path.join(root,'mustard.json'),config=JSON.parse(fs.readFileSync(configFile));config.ai={fallback:false,vectors:false};fs.writeFileSync(configFile,JSON.stringify(config));
      }
      const argv=['--bare','-p',task.prompt,'--model',model,'--output-format','stream-json','--verbose','--permission-mode','acceptEdits',
        '--allowedTools','Read,Grep,Glob,Edit,Write,Bash,mcp__mustard__search'];
      if(arm==='mustard')argv.push('--plugin-dir',plugin,'--append-system-prompt','Use the registered Mustard search gateway for all project code searches and reads. Preserve original tool arguments and include the specific question as intent. Reuse complete bodies already delivered. Implement the task and validate it.');
      const stream=fs.openSync(path.join(armOut,'events.jsonl'),'w'),errors=fs.openSync(path.join(armOut,'stderr.txt'),'w');const start=performance.now();
      const r=cp.spawnSync('claude',argv,{cwd:root,env:childEnv,stdio:['ignore',stream,errors],timeout:20*60*1000});fs.closeSync(stream);fs.closeSync(errors);
      const events=fs.readFileSync(path.join(armOut,'events.jsonl'),'utf8').split('\n').filter(Boolean).flatMap(line=>{try{return [JSON.parse(line)];}catch{return [];}});
      const measured=metrics(events),grading=grade(root,task);
      const valid=r.status===0&&measured.result_received&&measured.host_success&&(arm!=='mustard'||measured.gateway_calls>0);
      rows.push({task:task.id,arm,model,prompt_sha256:crypto.createHash('sha256').update(task.prompt).digest('hex'),exit_code:r.status,ms:performance.now()-start,
        valid_host_run:valid,grading,...measured});
      fs.cpSync(path.join(root,'src'),path.join(armOut,'source'),{recursive:true});
      fs.writeFileSync(path.join(out,'partial.json'),JSON.stringify(rows,null,2));
    }
  }
  const tokenTotal=usage=>['input_tokens','output_tokens','cache_creation_input_tokens','cache_read_input_tokens'].every(k=>Number.isFinite(usage?.[k]))
    ? ['input_tokens','output_tokens','cache_creation_input_tokens','cache_read_input_tokens'].reduce((n,k)=>n+usage[k],0) : null;
  const reduction=(before,after)=>Number.isFinite(before)&&Number.isFinite(after)&&before>0?100*(before-after)/before:null;
  const pairs=tasks.map(task=>{
    const baseline=rows.find(r=>r.task===task.id&&r.arm==='baseline'),mustard=rows.find(r=>r.task===task.id&&r.arm==='mustard');
    const valid=!!baseline&&!!mustard&&baseline.valid_host_run&&mustard.valid_host_run&&baseline.grading.passed&&mustard.grading.passed;
    return {task:task.id,both_completed_correctly:valid,
      whole_session_token_reduction_percent:valid?reduction(tokenTotal(baseline.usage),tokenTotal(mustard.usage)):null,
      estimated_cost_reduction_percent:valid?reduction(baseline.estimated_cost_usd,mustard.estimated_cost_usd):null,
      tool_call_reduction_percent:valid?reduction(baseline.tool_calls,mustard.tool_calls):null};
  });
  const result={executed:execute,grader_self_checks:true,rows,pairs,whole_session_savings:null,
    interpretation:execute?'Compare tokens including cache categories and behavior success per paired task. No generic savings percentage is inferred.':'Free preflight passed. Real paired sessions are pending; no model tokens, costs or success measured.',
    limits:['Two authored JavaScript tasks; not a broad coding benchmark.','Bare API runs differ from subscription interactive sessions.','Mustard arm is invalid unless the registered gateway was actually called.','Jev disabled in both arms to isolate deterministic gateway overhead; its paid responsibility pilot is separate.']};
  fs.writeFileSync(path.join(out,'comparison.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
}finally{fs.rmSync(temp,{recursive:true,force:true});}
