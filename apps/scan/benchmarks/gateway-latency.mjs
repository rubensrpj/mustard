// Read-only, warmed, alternating shell-output timing. Use disposable, already
// scanned roots per arm and identical requests. No labels or paid inference.
import fs from 'node:fs';
import path from 'node:path';
import cp from 'node:child_process';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const flags=new Map();for(let at=2;at<process.argv.length;at+=2)flags.set(process.argv[at],process.argv[at+1]);
const required=name=>{assert.ok(flags.get(name),`Missing ${name}`);return path.resolve(flags.get(name));};
const manifestPath=required('--manifest'),out=required('--out'),raw=fs.readFileSync(manifestPath),manifest=JSON.parse(raw);
assert.equal(manifest.isolated,true,'Timing must use disposable checkouts');
const repeat=Number(flags.get('--repeat')||3);assert.ok(Number.isInteger(repeat)&&repeat>=3);
const sha=value=>crypto.createHash('sha256').update(value).digest('hex');
const env={...process.env,MUSTARD_RT_DELEGATED:'1'};
for(const key of ['TYPESAFE_API_KEY','MUSTARD_JEV_URL','MUSTARD_SEARCH_TRACE','CLAUDE_PLUGIN_ROOT','MUSTARD_ACTIVE_SPEC'])delete env[key];
fs.mkdirSync(path.dirname(out),{recursive:true});
env.CLAUDE_CONFIG_DIR=out+'.host';env.MUSTARD_SPEND_DIR=out+'.usage';
const arms=Object.keys(manifest.arms);assert.ok(arms.length>=2);
for(const item of manifest.cases)assert.equal((item.request.request||item.request).choose,false);
function invoke(arm,item){
 const start=performance.now(),root=item.roots[arm],binary=path.join(manifest.arms[arm],'mustard-rt');
 const result=cp.spawnSync(binary,['run','search','--root',root,'--request',JSON.stringify(item.request),'--shell-output'],{cwd:root,env,timeout:180000,maxBuffer:32*1024*1024});
 if(result.error)throw result.error;assert.ok([0,1].includes(result.status),result.stderr.toString());
 return {arm,id:item.id,ms:performance.now()-start,bytes:result.stdout.length,stdout_sha256:sha(result.stdout)};
}
const warmups=[];for(const item of manifest.cases)for(const arm of arms)warmups.push(invoke(arm,item));
const rows=[];
for(let round=0;round<repeat;round++)for(const [at,item] of manifest.cases.entries()){
 const order=(round+at)%2?[...arms].reverse():arms;
 for(const arm of order)rows.push({round,...invoke(arm,item)});
}
const median=values=>{const sorted=[...values].sort((a,b)=>a-b),middle=Math.floor(sorted.length/2);return sorted.length%2?sorted[middle]:(sorted[middle-1]+sorted[middle])/2;};
const summary=Object.fromEntries(arms.map(arm=>{
 const picked=rows.filter(row=>row.arm===arm);
 return [arm,{samples:picked.length,median_ms:median(picked.map(row=>row.ms)),
  per_case:Object.fromEntries(manifest.cases.map(item=>{const samples=picked.filter(row=>row.id===item.id);return [item.id,{median_ms:median(samples.map(row=>row.ms)),deterministic_output:new Set(samples.map(row=>row.stdout_sha256)).size===1,bytes:samples[0].bytes}];}))}];
}));
const result={manifest_sha256:sha(raw),binary_sha256:Object.fromEntries(arms.map(arm=>[arm,sha(fs.readFileSync(path.join(manifest.arms[arm],'mustard-rt')))])),
 method:'One unmeasured warm-up per case/arm, then alternating arms; actual --shell-output, diagnostics disabled, no parallel work within this runner.',
 repeat,rows,warmups,summary,limits:['Small known query sample, warm filesystem cache, one machine.','No autonomous host model, billing, application execution or end-to-end task time measured.','Models/configurations must be recorded by the caller; each root must already be scanned by its own binary.']};
fs.writeFileSync(out,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(summary,null,2));
