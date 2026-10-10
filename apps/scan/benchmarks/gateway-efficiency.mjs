// Compare the SAME native searches against gateway presentations on an isolated
// snapshot. No model/code execution. Development cases, not task/token savings.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';

const options=new Map();
for(let at=2;at<process.argv.length;at+=2){assert.ok(process.argv[at]?.startsWith('--')&&process.argv[at+1]);options.set(process.argv[at].slice(2),path.resolve(process.argv[at+1]));}
for(const key of ['source','baseline','current','out'])assert.ok(options.has(key),`Missing --${key}`);
const source=options.get('source'),baseline=options.get('baseline'),current=options.get('current'),out=options.get('out');
// Frozen before the comparison: ordinary literal/regex/format requests, with
// both narrow and broad scopes. No natural-language ranking is measured here.
const cases=[
  {id:'edit-policy',args:['-n','--with-filename','assertCanEditPlan','src']},
  {id:'removal-policy',args:['-n','--with-filename','assertCanRemovePlan','src']},
  {id:'optimizer-dispatch',args:['-n','--with-filename','sendToHeuristic','src']},
  {id:'official-plan',args:['-n','--with-filename','makeThePlanOfficial','src']},
  {id:'http-routes',args:['-n','--with-filename','@(Get|Post|Patch|Delete)\\(','src/puzzle']},
  {id:'plan-flow',args:['-n','--with-filename','return|throw|await','src/puzzle/pi/services/plan/plantio-plan.service.ts']},
  {id:'removal-context',args:['-n','--with-filename','-C','2','assertCanRemovePlan','src/puzzle/shared/services/plan-removal.policy.ts']},
  {id:'match-counts',args:['-c','assertCanEditPlan','src']},
  {id:'file-list',args:['--files','src/puzzle/shared']},
  {id:'no-match',args:['-n','--with-filename','mustard_benchmark_absent_20261009','src']},
].map(item=>({...item,args:['--sort=path',...item.args]}));
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'mustard-gateway-efficiency-'));
const root=path.join(temp,'project');
const env={...process.env,MUSTARD_RT_DELEGATED:'1',CLAUDE_CONFIG_DIR:path.join(temp,'host'),MUSTARD_SPEND_DIR:path.join(temp,'usage'),PATH:current+path.delimiter+process.env.PATH};
for(const key of ['TYPESAFE_API_KEY','MUSTARD_JEV_URL','CLOUDFLARE_API_TOKEN','CLAUDE_PLUGIN_ROOT','CLAUDE_PROJECT_DIR','MUSTARD_WORKSPACE_ROOT','MUSTARD_ACTIVE_SPEC'])delete env[key];
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
function run(program,args,cwd=root){
  const start=performance.now();const r=spawnSync(program,args,{cwd,env,timeout:180000,maxBuffer:128*1024*1024});
  assert.ok([0,1].includes(r.status),r.stderr?.toString()||r.error?.message);
  return {...r,ms:performance.now()-start};
}
const state=()=>({head:run('git',['rev-parse','HEAD'],source).stdout.toString().trim(),status:run('git',['status','--porcelain=v1','-z'],source).stdout.toString()});
const before=state();assert.equal(before.status,'');
const median=values=>[...values].sort((a,b)=>a-b)[Math.floor(values.length/2)];
function restore(bytes){
  const text=bytes.toString();
  if(!text.startsWith('@ '))return bytes;
  let file='',result='';
  for(const row of text.split('\n')){
    if(row.startsWith('@ '))file=row.slice(2);
    else if(row.startsWith('# '))continue;
    else if(row){assert.ok(file);assert.match(row,/^\d+:/);result+=file+':'+row+'\n';}
  }
  if(!text.endsWith('\n'))result=result.slice(0,-1);
  return Buffer.from(result);
}
try{
  run('git',['clone','--no-hardlinks','--no-checkout','--quiet',source,root],temp);
  run('git',['-c','core.hooksPath=/dev/null','checkout','--quiet','--detach',before.head]);
  for(const file of run('git',['ls-files','-z']).stdout.toString().split('\0').filter(Boolean)){
    if(/\.md$/i.test(file)||/(^|\/)\.env($|\.)/.test(file)||/(^|\/)(\.claude|\.codex)(\/|$)/.test(file)||file==='mustard.json')fs.rmSync(path.join(root,file),{force:true});
  }
  fs.writeFileSync(path.join(root,'mustard.json'),JSON.stringify({ai:{fallback:false,vectors:false},search:{filter:'none'}}));
  const scan=JSON.parse(run(path.join(current,'scan'),['scan',root,'--native','--out',path.join(root,'.claude/grain.db'),'--json']).stdout);
  assert.equal(scan.remote_model_calls,0);
  const results=[];
  for(const item of cases){
    const command=(directory,flags=[])=>[path.join(directory,'mustard-rt'),['run','search','--root',root,...flags,'--','rg',...item.args]];
    const original=run('rg',item.args);
    // Warm persistent observations/index once, separately from timings.
    const report=JSON.parse(run(...command(current)).stdout);
    assert.equal(report.remote_model_calls,0);assert.equal(report.local_model_calls,0);
    const oldJson=run(...command(baseline));
    const times={native:[],before:[],after:[]};let oldView,newView;
    for(let round=0;round<3;round++){
      const order=round%2?['after','before','native']:['native','before','after'];
      for(const version of order){
        const r=version==='native'?run('rg',item.args):run(...command(version==='before'?baseline:current,['--shell-output']));
        times[version].push(r.ms);assert.equal(r.status,original.status);assert.deepEqual(r.stderr,original.stderr);
        if(version==='before')oldView=r.stdout;
        if(version==='after'){
          newView=r.stdout;
          assert.deepEqual(restore(newView),original.stdout,'All occurrences, text and order must survive');
          assert.ok(newView.length<=original.stdout.length,'Routine output must not grow');
        }
      }
    }
    const row={id:item.id,args:item.args,native_bytes:original.stdout.length,before_shell_bytes:oldView.length,before_mods_json_bytes:oldJson.stdout.length,
      after_agent_bytes:newView.length,files_with_owner_ranges:(newView.toString().match(/# static owners:/g)||[]).length,
      native_ms:median(times.native),before_ms:median(times.before),after_ms:median(times.after),lossless:true};
    results.push(row);console.log(JSON.stringify(row));
  }
  assert.deepEqual(state(),before);
  const bytes=key=>results.reduce((sum,row)=>sum+row[key],0);
  const total={native_bytes:bytes('native_bytes'),before_shell_bytes:bytes('before_shell_bytes'),before_mods_json_bytes:bytes('before_mods_json_bytes'),after_agent_bytes:bytes('after_agent_bytes')};
  const report={source:before,cases_sha256:sha(JSON.stringify(cases)),binaries:Object.fromEntries([['baseline',baseline],['current',current]].map(([name,dir])=>[name,sha(fs.readFileSync(path.join(dir,'mustard-rt')))])),
    summary:{...total,vs_native_percent:(total.after_agent_bytes/total.native_bytes-1)*100,vs_before_shell_percent:(total.after_agent_bytes/total.before_shell_bytes-1)*100,
      vs_before_mods_percent:(total.after_agent_bytes/total.before_mods_json_bytes-1)*100,native_median_ms:median(results.map(row=>row.native_ms)),
      before_median_ms:median(results.map(row=>row.before_ms)),after_median_ms:median(results.map(row=>row.after_ms)),lossless_cases:results.length,cases:results.length},results,
    local_model_calls:0,remote_model_calls:0,original_unchanged:true,
    limitations:'Known development snapshot/cases. Bytes, not model tokens. Warm debug CLI medians, three alternating rounds, one machine. No implementation/spec/session quality or total cost measured. Optional Choice not requested.'};
  fs.mkdirSync(out,{recursive:true});fs.writeFileSync(path.join(out,'benchmark.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report.summary));
}finally{fs.rmSync(temp,{recursive:true,force:true});assert.deepEqual(state(),before);}
