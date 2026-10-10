// Development evaluation on two isolated copies of a clean, authorized Git
// repository. Never builds/runs its code, calls a model or changes the original.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';

const args = new Map();
for(let at=2;at<process.argv.length;at+=2){assert.ok(process.argv[at]?.startsWith('--')&&process.argv[at+1]);args.set(process.argv[at].slice(2),process.argv[at+1]);}
const required = key => {assert.ok(args.has(key),`Missing --${key}`);return path.resolve(args.get(key));};
const source=required('source'),questionsPath=required('questions'),out=required('out');
const binaries={baseline:required('baseline'),current:required('current')};
const plan=JSON.parse(fs.readFileSync(questionsPath));
assert.ok(plan.questions.length>0&&plan.questions.length<=128);
const temporary=fs.mkdtempSync(path.join(os.tmpdir(),'mustard-investigation-'));
const env={...process.env,MUSTARD_RT_DELEGATED:'1',CLAUDE_CONFIG_DIR:path.join(temporary,'host-state'),MUSTARD_SPEND_DIR:path.join(temporary,'usage')};
for(const key of ['TYPESAFE_API_KEY','MUSTARD_JEV_URL','CLOUDFLARE_API_TOKEN','CLAUDE_PLUGIN_ROOT','MUSTARD_ACTIVE_SPEC'])delete env[key];
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
function run(program,values,cwd){
  const start=performance.now();const r=spawnSync(program,values,{cwd,env,encoding:'utf8',timeout:180000,maxBuffer:128*1024*1024});
  assert.equal(r.status,0,`${program}: ${r.stderr||r.error?.message||r.stdout.slice(0,600)}`);
  return {text:r.stdout,ms:Math.round(performance.now()-start),bytes:Buffer.byteLength(r.stdout)};
}
const state=()=>({head:run('git',['rev-parse','HEAD'],source).text.trim(),branch:run('git',['branch','--show-current'],source).text.trim(),status:run('git',['status','--porcelain=v1','-z'],source).text});
const before=state();assert.equal(before.status,'');assert.equal(before.head,plan.source_commit);
fs.mkdirSync(out,{recursive:true});
const report={source:before,questions_sha256:sha(fs.readFileSync(questionsPath)),models:0,results:[],indexes:[],binaries:{},
  limitations:'Known development questions. Native retrieval only; bytes are not billed tokens. Candidate alternatives and complete indexed spans do not certify sufficient implementation context. Debug latency, alternating query order, one machine.'};
try{
  const roots={};
  for(const [version,directory] of Object.entries(binaries)){
    const root=path.join(temporary,version);roots[version]=root;
    run('git',['clone','--no-hardlinks','--no-checkout','--quiet',source,root],temporary);
    run('git',['-c','core.hooksPath=/dev/null','checkout','--quiet','--detach',before.head],root);
    for(const file of run('git',['ls-files','-z'],root).text.split('\0').filter(Boolean)){
      if(/\.md$/i.test(file)||/(^|\/)\.env($|\.)/.test(file)||/(^|\/)(\.claude|\.codex)(\/|$)/.test(file)||file==='mustard.json')fs.rmSync(path.join(root,file),{force:true});
    }
    fs.writeFileSync(path.join(root,'mustard.json'),JSON.stringify({language:{text:'pt-BR',code:'en-US'},ai:{fallback:false,vectors:false},search:{filter:'none'}}));
    const index=run(path.join(directory,'scan'),['scan',root,'--out',path.join(root,'.claude/grain.db'),'--json'],root);
    const indexed=JSON.parse(index.text);assert.equal(indexed.remote_model_calls,0);
    const audit=JSON.parse(run(path.join(directory,'mustard-rt'),['run','map','audit','--root',root],root).text);assert.equal(audit.ok,true);
    report.indexes.push({version,ms:index.ms,scan:indexed,audit,bytes:fs.statSync(path.join(root,'.claude/grain.db')).size});
    report.binaries[version]=Object.fromEntries(['scan','mustard-rt'].map(name=>[name,sha(fs.readFileSync(path.join(directory,name)))]));
  }
  for(let at=0;at<plan.questions.length;at++)for(const version of at%2?['current','baseline']:['baseline','current']){
    const question=plan.questions[at],root=roots[version];
    const flags=version==='current'?[...(question.intent?['--intent',question.intent]:[]),...(question.purpose?['--purpose',question.purpose]:[])]:[];
    const read=run(path.join(binaries[version],'mustard-rt'),['run','knowledge','--root',root,'--query',question.query,...flags],root);
    const result=JSON.parse(read.text);assert.equal(result.local_model_calls,0);assert.equal(result.remote_model_calls,0);
    const cards=result.cards,alternatives=cards.flatMap(card=>card.alternatives||[]);
    for(const card of [...cards,...alternatives]){
      assert.equal(sha(fs.readFileSync(path.join(root,card.source.file))),card.source.sha256);
      if(card.source_excerpt){
        const lines=fs.readFileSync(path.join(root,card.source.file),'utf8').split(/\r?\n/);
        for(const line of card.source_excerpt.text.split('\n')){const match=/^(\d+) \| (.*)$/.exec(line);assert.ok(match);assert.ok(lines[Number(match[1])-1].startsWith(match[2]));}
      }
    }
    const target=card=>question.expected.includes(card.source.file)&&(question.symbols||[]).some(name=>name===card.name||name===card.id);
    const targetCards=cards.filter(target);
    const row={version,id:question.id,query:question.query,file_hit:cards.some(card=>question.expected.includes(card.source.file)),
      symbol_hit:question.symbols?.length?targetCards.length>0:null,alternative_symbol_hit:question.symbols?.length?alternatives.some(target):null,
      target_current_excerpt:targetCards.some(card=>!!card.source_excerpt),ms:read.ms,bytes:read.bytes,
      selected:cards.map(card=>({name:card.name,source:card.source})),partial:result.investigation?.partial};
    report.results.push(row);fs.writeFileSync(path.join(out,`${version}-${question.id}.json`),JSON.stringify(result));
    console.log(JSON.stringify({version,id:row.id,file:row.file_hit,symbol:row.symbol_hit,alternative:row.alternative_symbol_hit}));
  }
  report.summary=Object.fromEntries(Object.keys(binaries).map(version=>{
    const rows=report.results.filter(row=>row.version===version);
    return [version,{questions:rows.length,file_hits:rows.filter(row=>row.file_hit).length,symbol_hits:rows.filter(row=>row.symbol_hit).length,
      symbol_questions:rows.filter(row=>row.symbol_hit!==null).length,alternative_rescues:rows.filter(row=>!row.symbol_hit&&row.alternative_symbol_hit).length,
      target_current_excerpts:rows.filter(row=>row.target_current_excerpt).length,bytes:rows.reduce((n,row)=>n+row.bytes,0),median_ms:rows.map(row=>row.ms).sort((a,b)=>a-b)[Math.floor(rows.length/2)]}];
  }));
  assert.deepEqual(state(),before);report.original_unchanged=true;
  fs.writeFileSync(path.join(out,'benchmark.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report.summary));
}finally{fs.rmSync(temporary,{recursive:true,force:true});assert.deepEqual(state(),before);}
