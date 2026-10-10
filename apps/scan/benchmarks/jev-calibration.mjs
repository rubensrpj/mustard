// Explicit paid source-backed pilot. Secrets are read only into the child
// environment; output contains decisions/usage, never credentials or headers.
import fs from 'node:fs';import path from 'node:path';import os from 'node:os';
import cp from 'node:child_process';import assert from 'node:assert/strict';import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
const checkout=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..');
const args=process.argv.slice(2),option=n=>args[args.indexOf(n)+1];
assert.ok(args.includes('--jev'),'Paid pilot requires --jev');assert.ok(args.includes('--out'),'Supply --out');
const out=path.resolve(option('--out'));fs.mkdirSync(out,{recursive:true});
const bin=path.resolve(args.includes('--bin')?option('--bin'):path.join(checkout,'target/debug'));
let key=process.env.TYPESAFE_API_KEY;
if(!key&&args.includes('--config'))key=JSON.parse(fs.readFileSync(option('--config'),'utf8')).jev?.key;
assert.ok(key,'No Jev key available');
const fixturePath=path.resolve(args.includes('--fixture')?option('--fixture'):path.join(checkout,'apps/scan/tests/fixtures/jev-responsibility-calibration-20261009.json'));
const raw=fs.readFileSync(fixturePath),fixture=JSON.parse(raw);
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'mustard-jev-calibration-'));
const env={...process.env,MUSTARD_RT_DELEGATED:'1',TYPESAFE_API_KEY:key,CLAUDE_CONFIG_DIR:path.join(temp,'host'),MUSTARD_SPEND_DIR:path.join(temp,'usage')};
delete env.MUSTARD_JEV_URL;delete env.CLAUDE_PLUGIN_ROOT;
const run=(program,argv,root)=>{const start=performance.now();const r=cp.spawnSync(program,argv,{cwd:root,env,maxBuffer:16*1024*1024,timeout:60000});assert.equal(r.status,0,r.stderr.toString());return {value:JSON.parse(r.stdout),ms:performance.now()-start};};
const rows=[];
try{
  for(const item of fixture.cases){
    const root=path.join(temp,item.id);fs.mkdirSync(root);
    fs.writeFileSync(path.join(root,'mustard.json'),JSON.stringify({language:{text:'en-US',code:'en-US'},ai:{fallback:true,vectors:false},judgement:{search:{filter:'jev'}}}));
    fs.writeFileSync(path.join(root,'source.ts'),item.source);
    run(path.join(bin,'scan'),['scan',root,'--native','--out',path.join(root,'.claude/grain.db'),'--json'],root);
    const request={schema_version:1,request:{tool:'rg',input:{args:['-n','--with-filename','--sort=path','function','source.ts']},intent:item.intent,purpose:'implement',choose:true}};
    const argv=['run','search','--root',root,'--request',JSON.stringify(request)];
    const first=run(path.join(bin,'mustard-rt'),argv,root),context=first.value.task_context;
    const selected=(context.cards||[]).filter(c=>(context.recommended_symbols||[]).includes(c.id)).map(c=>c.name);
    const outcome=context.selection?.outcomes?.responsibility;
    const correct=item.expected==='abstain'?selected.length===0:item.expected==='none'?outcome==='no-match':selected.includes(item.expected);
    const repeat=first.value.remote_model_calls===null?null:run(path.join(bin,'mustard-rt'),argv,root);
    if(repeat)assert.equal(repeat.value.remote_model_calls,0,'Repeat did not reuse provider cache');
    rows.push({id:item.id,split:item.split,expected:item.expected,selected,outcome,correct,ms:first.ms,usage:context.selection,repeat_physical_calls:repeat?.value.remote_model_calls??null,
      input_source_sha256:crypto.createHash('sha256').update(item.source).digest('hex')});
    fs.writeFileSync(path.join(out,'pilot-partial.json'),JSON.stringify(rows,null,2));
  }
  const summary=Object.fromEntries(['calibration','holdout'].map(split=>{const r=rows.filter(r=>r.split===split);return [split,{cases:r.length,correct:r.filter(r=>r.correct).length,
    wrong_accepted:r.filter(r=>!r.correct&&r.selected.length>0).length,abstained:r.filter(r=>r.selected.length===0).length}];}));
  const knownCalls=rows.reduce((n,r)=>n+(r.usage?.remote_model_calls||0),0),calls=rows.every(r=>Number.isInteger(r.usage?.remote_model_calls)&&r.repeat_physical_calls===0)?knownCalls:null,tokens=rows.reduce((n,r)=>n+(r.usage?.known_input_tokens||0),0);
  const result={fixture_sha256:crypto.createHash('sha256').update(raw).digest('hex'),binary_sha256:crypto.createHash('sha256').update(fs.readFileSync(path.join(bin,'mustard-rt'))).digest('hex'),rows,summary,physical_calls:calls,known_input_tokens:tokens,
    cost_micro_usd:rows.every(r=>r.usage?.usage_complete||r.usage?.remote_model_calls===0)?rows.reduce((n,r)=>n+(r.usage.cost_micro_usd||0),0):null,
    policy:{confidence:0.5,probability:0.7,margin:0.2},
    interpretation:'Policy evaluated without tuning on heldout. Small authored sample, not general calibration. Failures remain in the report.',
    whole_session_tokens:null,whole_session_savings:null};
  fs.writeFileSync(path.join(out,'pilot.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({summary,calls,tokens,cost_micro_usd:result.cost_micro_usd},null,2));
}finally{
 const usage=path.join(temp,'usage');if(fs.existsSync(usage))fs.cpSync(usage,path.join(out,'physical-usage'),{recursive:true});
 for(const item of fs.readdirSync(temp,{withFileTypes:true}).filter(e=>e.isDirectory())){
  const ledger=path.join(temp,item.name,'.claude/judgements');if(fs.existsSync(ledger))fs.cpSync(ledger,path.join(out,'judgements',item.name),{recursive:true});
 }
 fs.rmSync(temp,{recursive:true,force:true});
}
