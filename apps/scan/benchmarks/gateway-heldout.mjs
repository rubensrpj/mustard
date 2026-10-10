// New public RepoQA repositories. Same deterministic, answer-blind requests in
// both arms. No code execution, remote inference, filename hints or host installation.
// Local embeddings require explicit per-arm --vectors/--baseline-vectors flags.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import cp from 'node:child_process';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {completeBody} from './source-evidence.mjs';
const options=new Map();for(let at=2;at<process.argv.length;at+=2)options.set(process.argv[at],process.argv[at+1]);
const required=name=>{assert.ok(options.get(name),`Missing ${name}`);return path.resolve(options.get(name));};
const datasetPath=required('--dataset'),selectionPath=required('--selection'),out=required('--out');
const bins={baseline:required('--baseline'),current:required('--current')};
for(const key of ['--vectors','--baseline-vectors'])assert.ok(!options.has(key)||['true','false'].includes(options.get(key)),`${key} must be true or false`);
console.log(JSON.stringify({configuration:{baseline_vectors:options.get('--baseline-vectors')==='true',current_vectors:options.get('--vectors')==='true',paid_inference:false}}));
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
const bytes=fs.readFileSync(datasetPath),dataset=JSON.parse(bytes),selection=JSON.parse(fs.readFileSync(selectionPath));
assert.equal(sha(bytes),selection.dataset_json_sha256);
const previous=options.has('--reuse-baseline')?JSON.parse(fs.readFileSync(path.join(required('--reuse-baseline'),'comparison.json'))):null;
if(previous){
 assert.equal(previous.dataset_sha256,sha(bytes));assert.equal(previous.selection_sha256,sha(fs.readFileSync(selectionPath)));
 assert.equal(previous.binary_sha256.baseline,sha(fs.readFileSync(path.join(bins.baseline,'mustard-rt'))));
 assert.equal(previous.baseline_vectors,options.get('--baseline-vectors')==='true','Reused baseline must have the same recorded local-model configuration');
 assert.equal(previous.rows.filter(r=>r.version==='baseline').length,selection.repositories.length*10);
}
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'mustard-heldout-'));fs.mkdirSync(out,{recursive:true});
const env={...process.env,MUSTARD_RT_DELEGATED:'1',MUSTARD_SEARCH_TRACE:options.get('--trace')==='full'?'1':'projection',CLAUDE_CONFIG_DIR:path.join(temp,'host'),MUSTARD_SPEND_DIR:path.join(temp,'usage')};
for(const key of ['TYPESAFE_API_KEY','MUSTARD_JEV_URL','CLAUDE_PLUGIN_ROOT','MUSTARD_ACTIVE_SPEC'])delete env[key];
function run(program,args,root){const start=performance.now();const r=cp.spawnSync(program,args,{cwd:root,env,maxBuffer:32*1024*1024,timeout:60000});assert.ok([0,1].includes(r.status),r.stderr.toString());return {bytes:r.stdout,ms:performance.now()-start};}
const rows=previous?previous.rows.filter(r=>r.version==='baseline'):[];
if(previous)for(const file of fs.readdirSync(required('--reuse-baseline')).filter(f=>f.endsWith('-baseline.json')))fs.copyFileSync(path.join(required('--reuse-baseline'),file),path.join(out,file));
const stop=new Set('about after also between being could does every following function implementation implements into other return returns should some specific their there these this those through using values when where which while with within would'.split(' '));
function request(description){
  const tokens=[...new Set(description.match(/[A-Za-z_][A-Za-z_0-9]{3,}/g)||[])].filter(t=>!stop.has(t.toLowerCase())).slice(0,12);
  return {schema_version:1,request:{tool:'rg',input:{args:['--sort=path','-n','--with-filename',tokens.join('|')||'__no_written_clue__','.']},intent:description.slice(0,4000),purpose:'implement',choose:false}};
}
const matches=(card,needle)=>card.source?.file===needle.path && card.name===needle.name && card.source.line<=needle.start_line+1 && card.source.end_line>=needle.end_line;
function visible(card,text){
 let file=null;
 for(const line of text.split('\n')){
  if(line.startsWith('@ '))file=line.slice(2);
  if(file===card.source.file&&line.includes(`${card.name} ${card.source.line}-${card.source.end_line}`))return true;
 }
 return false;
}
try {
  for(const [repoAt,picked] of selection.repositories.entries()){
    const repo=dataset[picked.language].find(r=>r.repo===picked.repo&&r.commit_sha===picked.commit);assert.ok(repo);
    for(const version of repoAt%2?['current','baseline']:['baseline','current']){
      if(options.has('--only')&&options.get('--only')!==version)continue;
      if(previous&&version==='baseline')continue;
      const root=path.join(temp,`${repoAt}-${version}`);fs.mkdirSync(root);
      for(const [file,text] of Object.entries(repo.content)){
        const destination=path.resolve(root,file);assert.ok(destination.startsWith(root+path.sep));
        fs.mkdirSync(path.dirname(destination),{recursive:true});fs.writeFileSync(destination,text);
      }
      const vectors=options.get(version==='current'?'--vectors':'--baseline-vectors')==='true';
      fs.writeFileSync(path.join(root,'mustard.json'),JSON.stringify({language:{text:'en-US',code:'en-US'},ai:{fallback:false,vectors},search:{filter:'none'}}));
      run(path.join(bins[version],'scan'),['scan',root,...(vectors?[]:['--native']),'--out',path.join(root,'.claude/grain.db'),'--json'],root);
      for(const [at,needle] of repo.needles.entries()){
        const invocation=request(needle.description),args=['run','search','--root',root,'--request',JSON.stringify(invocation)];
        const query=run(path.join(bins[version],'mustard-rt'),args,root),report=JSON.parse(query.bytes);
        const view=typeof report._trace_agent_view==='string'?{bytes:Buffer.from(report._trace_agent_view)}:run(path.join(bins[version],'mustard-rt'),[...args,'--shell-output'],root);
        if(at===0&&typeof report._trace_agent_view==='string')assert.deepEqual(view.bytes,run(path.join(bins[version],'mustard-rt'),[...args,'--shell-output'],root).bytes,'Diagnostic projection differs from actual host output');
        const native=run(path.join(bins[version],'mustard-rt'),['run','search','--root',root,'--request',JSON.stringify({...invocation,request:{...invocation.request,purpose:'locate'}})],root);
        const original=JSON.parse(native.bytes).result;
        // Separate physical ripgrep executions may reorder files without --sort.
        const records=value=>({...value,content:value.content?.split('\n').sort().join('\n'),stdout:value.stdout?.split('\n').sort().join('\n')});
        assert.deepEqual(records(report.result),records(original));assert.equal(report.remote_model_calls,0);
        const cards=report.task_context?.cards||[];
        for(const card of cards)assert.equal(sha(fs.readFileSync(path.join(root,card.source.file))),card.source.sha256);
        const found=cards.filter(c=>matches(c,needle));
        const delivered=view.bytes.toString();
        rows.push({version,language:picked.language,repo:picked.repo,id:at,query_sha256:sha(needle.description),
          expected:{file:needle.path,name:needle.name},symbol_found:found.length>0,
          visible_symbol_found:found.some(c=>(c.initial_source_excerpt||c.initial_reference)&&visible(c,delivered)),
          native_owner_found:(report.evidence?.current_owner_ids||[]).some(id=>id.startsWith(needle.path+':')&&id.endsWith(':'+needle.name)),
          complete_body:found.some(c=>completeBody(c,delivered)),
          output_bytes:view.bytes.length,ms:query.ms,chain_steps:report.task_context?.chain?.steps?.length||0,
          remote_model_calls:report.remote_model_calls,native_parity:true});
        fs.writeFileSync(path.join(out,`${repoAt}-${at}-${version}.json`),JSON.stringify({invocation,report,view:view.bytes.toString()}));
      }
      fs.rmSync(root,{recursive:true,force:true});
    }
  }
  const summary=Object.fromEntries(Object.keys(bins).map(version=>{const arm=rows.filter(r=>r.version===version);return [version,{cases:arm.length,
    found:arm.filter(r=>r.symbol_found).length,visible_found:arm.filter(r=>r.visible_symbol_found).length,native_owner_found:arm.filter(r=>r.native_owner_found).length,complete_bodies:arm.filter(r=>r.complete_body).length,bytes:arm.reduce((n,r)=>n+r.output_bytes,0),
    total_ms:arm.reduce((n,r)=>n+r.ms,0),native_parity:arm.filter(r=>r.native_parity).length,remote_model_calls:0}];}));
  const result={dataset_sha256:sha(bytes),selection_sha256:sha(fs.readFileSync(selectionPath)),
    current_vectors:options.get('--vectors')==='true',
    baseline_vectors:options.get('--baseline-vectors')==='true',
    diagnostics:options.get('--trace')==='full'?'full-candidate-trace':'projection-only; no per-candidate trace',
    baseline_reused:previous!==null,
    binary_sha256:Object.fromEntries(Object.entries(bins).map(([name,dir])=>[name,sha(fs.readFileSync(path.join(dir,'mustard-rt')))])),rows,summary,
    method:'Adapted public-label retrieval via an answer-blind fixed lexical bootstrap and native rg --sort=path. New repositories, not autonomous Claude searches or official RepoQA scoring.',
    whole_session_tokens:null,whole_session_cost:null,limitations:['Output bytes are not billed tokens.','Complete expected declaration is not sufficient evidence of implementation correctness.','No host LLM ran; tool-call reduction and whole-task success remain unmeasured.']};
  fs.writeFileSync(path.join(out,'comparison.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(summary,null,2));
} finally {fs.rmSync(temp,{recursive:true,force:true});}
