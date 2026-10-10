// Read-only investigation of application source in an explicitly isolated
// checkout. The gateway may update that checkout's local search database.
import fs from 'node:fs';import path from 'node:path';import cp from 'node:child_process';import assert from 'node:assert/strict';import crypto from 'node:crypto';
const args=process.argv.slice(2),option=n=>{assert.ok(args.includes(n),`Missing ${n}`);return path.resolve(args[args.indexOf(n)+1]);};
const root=option('--root'),bin=option('--bin'),requestsFile=option('--requests'),out=option('--out');
assert.ok(args.includes('--isolated'),'Use a disposable checkout, then pass --isolated');
const raw=fs.readFileSync(requestsFile),requests=JSON.parse(raw);fs.mkdirSync(out,{recursive:true});
const env={...process.env,MUSTARD_RT_DELEGATED:'1',MUSTARD_SEARCH_TRACE:'projection',CLAUDE_CONFIG_DIR:path.join(out,'host'),MUSTARD_SPEND_DIR:path.join(out,'usage')};
for(const k of ['TYPESAFE_API_KEY','MUSTARD_JEV_URL','CLAUDE_PLUGIN_ROOT','MUSTARD_ACTIVE_SPEC'])delete env[k];
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
function run(program,argv){const at=performance.now(),r=cp.spawnSync(program,argv,{cwd:root,env,timeout:180000,maxBuffer:64*1024*1024});if(r.error)throw r.error;assert.ok([0,1].includes(r.status),r.stderr.toString());return {bytes:r.stdout,ms:performance.now()-at};}
const rows=[];
for(const q of requests.requests){
 assert.match(q.id,/^[a-z0-9-]+$/);assert.equal(q.envelope.request.choose,false,'This runner never sends project code to a provider');
 const argv=['run','search','--root',root,'--request',JSON.stringify(q.envelope)],first=run(path.join(bin,'mustard-rt'),argv),report=JSON.parse(first.bytes);
 const view=typeof report._trace_agent_view==='string'?{bytes:Buffer.from(report._trace_agent_view)}:run(path.join(bin,'mustard-rt'),[...argv,'--shell-output']);
 if(rows.length===0&&typeof report._trace_agent_view==='string')assert.deepEqual(view.bytes,run(path.join(bin,'mustard-rt'),[...argv,'--shell-output']).bytes);
 const native=run(path.join(bin,'mustard-rt'),['run','search','--root',root,'--request',JSON.stringify({...q.envelope,request:{...q.envelope.request,purpose:'locate'}})]);
 const original=JSON.parse(native.bytes);
 const canonical=result=>result.mode==='files_with_matches'?{...result,filenames:[...result.filenames].sort()}:result;
 assert.deepEqual(canonical(report.result),canonical(original.result));assert.equal(report.exit_code,original.exit_code);assert.equal(report.remote_model_calls,0);
 for(const c of report.task_context?.cards||[])assert.equal(sha(fs.readFileSync(path.join(root,c.source.file))),c.source.sha256);
 const row={id:q.id,ms:first.ms,view_bytes:view.bytes.length,native_bytes:Buffer.byteLength(report.result.stdout||report.result.content||''),native_parity:true,remote_model_calls:0,status:report.task_context?.status,
  cards:(report.task_context?.cards||[]).map(c=>({name:c.name,source:c.source,body:!!c.initial_source_excerpt,reference:!!c.initial_reference})),flow:report.task_context?.candidate_flow};
 rows.push(row);fs.writeFileSync(path.join(out,q.id+'.json'),JSON.stringify({request:q.envelope,report,view:view.bytes.toString()}));fs.writeFileSync(path.join(out,'partial.json'),JSON.stringify(rows,null,2));
}
const result={requests_sha256:sha(raw),binary_sha256:sha(fs.readFileSync(path.join(bin,'mustard-rt'))),rows,
 summary:{cases:rows.length,native_parity:rows.filter(r=>r.native_parity).length,view_bytes:rows.reduce((n,r)=>n+r.view_bytes,0),remote_model_calls:0},
 limits:['Task-guided investigation, not a blind benchmark or actual Claude session.','No application code, payment API or database was executed.','Static retrieval does not certify business readiness.','Bytes are not billed tokens.']};
fs.writeFileSync(path.join(out,'comparison.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result.summary,null,2));
