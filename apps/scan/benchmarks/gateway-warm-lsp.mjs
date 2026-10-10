// Real installed TypeScript server, isolated backend snapshot only. No model.
import fs from 'node:fs';import path from 'node:path';import cp from 'node:child_process';import assert from 'node:assert/strict';import crypto from 'node:crypto';
const opts=new Map();for(let i=2;i<process.argv.length;i+=2)opts.set(process.argv[i],process.argv[i+1]);
const required=n=>{assert.ok(opts.get(n),`Missing ${n}`);return path.resolve(opts.get(n));};
const root=required('--root'),baseline=required('--baseline'),current=required('--current'),out=required('--out');
assert.ok(root.includes(`${path.sep}target${path.sep}`),'Use an isolated target snapshot, never the original backend');
fs.mkdirSync(out,{recursive:true});
const env={...process.env,MUSTARD_RT_DELEGATED:'1',GATEWAY_BENCHMARK_RUN:crypto.randomUUID(),CLAUDE_CONFIG_DIR:path.join(out,'host'),MUSTARD_SPEND_DIR:path.join(out,'usage')};for(const key of ['TYPESAFE_API_KEY','MUSTARD_JEV_URL','CLAUDE_PLUGIN_ROOT'])delete env[key];
const cases=[['src/puzzle/controller/puzzle.controller.ts',112,'src/common/services/excel.service.ts'],
  ['src/cem/controller/budget.controller.ts',226,'src/common/services/excel.service.ts'],
  ['src/mlplan/controllers/planning/planning.controller.ts',46,'src/mlplan/services/micro-planning/download.service.ts']];
const rows=[];
function prepareIndex(binary){
  const database=path.join(root,'.claude/grain.db');
  const r=cp.spawnSync(path.join(path.dirname(binary),'scan'),['scan',root,'--native','--out',database,'--json'],
    {cwd:root,env,timeout:120000,maxBuffer:16*1024*1024});
  assert.equal(r.status,0,r.stderr.toString());
  fs.writeFileSync(path.join(out,path.basename(path.dirname(binary))+'-index.json'),r.stdout);
}
function run(version,file,line,expected,phase){
  const text=fs.readFileSync(path.join(root,file),'utf8').split('\n')[line-1];
  const column=Buffer.byteLength(text.slice(0,text.indexOf('createXlsxStream')));
  const request={schema_version:1,request:{tool:'References',input:{file_path:file,line,column,relation:'definitions'},intent:'Which declaration owns this call in its current type/import scope?',purpose:'understand',choose:false}};
  const start=performance.now();const r=cp.spawnSync(version==='baseline'?baseline:current,['run','search','--root',root,'--request',JSON.stringify(request)],{cwd:root,env,timeout:45000,maxBuffer:16*1024*1024});
  assert.equal(r.status,0,r.stderr.toString());const answer=JSON.parse(r.stdout),refs=answer.result.references;
  const row={version,phase,file,line,column,expected,actual:refs.map(r=>r.source.file),correct:refs.length===1&&refs[0].source.file===expected,
    ms:performance.now()-start,status:answer.result.status,remote_model_calls:answer.remote_model_calls};
  assert.equal(row.remote_model_calls,0);rows.push(row);
}
// Each binary has its own index schema. Migration must not be counted as
// compiler startup, nor may one arm silently inspect the other's empty index.
prepareIndex(baseline);
for(const item of cases)run('baseline',...item,'cold');
prepareIndex(current);
for(const item of cases)run('current',...item,'first-pass');
for(const item of cases)run('current',...item,'warm-pass');
const changed=path.join(root,cases[0][0]),original=fs.readFileSync(changed);
try{fs.appendFileSync(changed,'\n');run('current',...cases[0],'after-source-change');}finally{fs.writeFileSync(changed,original);}
const average=r=>r.reduce((n,r)=>n+r.ms,0)/r.length;
const cold=average(rows.filter(r=>r.version==='baseline')),warm=average(rows.filter(r=>r.phase==='warm-pass'));
const result={rows,cold_mean_ms:cold,warm_mean_ms:warm,warm_latency_reduction_percent:100*(cold-warm)/cold,
  each_arm_native_index_prepared_before_timing:true,
  correct:rows.filter(r=>r.correct).length,total:rows.length,
  source_edit_restarted_session:rows.at(-1).status.includes('new session'),
  caveat:'Installed TypeScript server and this backend only; sessions are reused, answers are queried again. Dependency freshness uses metadata, not exhaustive compiler-input attestation.'};
fs.writeFileSync(path.join(out,'comparison.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
