// Executable transport/freshness acceptance; authored fixture, no paid API.
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import cp from 'node:child_process';import assert from 'node:assert/strict';
import crypto from 'node:crypto';import {DatabaseSync} from 'node:sqlite';
import {fileURLToPath} from 'node:url';
const checkout=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..');
const at=process.argv.indexOf('--bin'),bin=at<0?path.join(checkout,'target/debug'):path.resolve(process.argv[at+1]);
const root=fs.mkdtempSync(path.join(os.tmpdir(),'mustard-local-model-'));
const env={...process.env,MUSTARD_RT_DELEGATED:'1',CLAUDE_CONFIG_DIR:path.join(root,'host'),MUSTARD_SPEND_DIR:path.join(root,'usage')};
for(const key of ['TYPESAFE_API_KEY','MUSTARD_JEV_URL','CLAUDE_PLUGIN_ROOT','MUSTARD_SEARCH_TRACE','MUSTARD_ACTIVE_SPEC'])delete env[key];
function run(program,args){const r=cp.spawnSync(program,args,{cwd:root,env,timeout:60000,maxBuffer:16*1024*1024});assert.ok([0,1].includes(r.status),r.stderr.toString());return r;}
const request={schema_version:1,request:{tool:'rg',input:{args:['-n','--with-filename','entry_sentinel','source.rs']},intent:'read and parse JSON data from a file',purpose:'implement',choose:false}};
function search(){const r=run(path.join(bin,'mustard-rt'),['run','search','--root',root,'--request',JSON.stringify(request)]);assert.equal(r.status,0);const report=JSON.parse(r.stdout);assert.equal(report.remote_model_calls,0);return report;}
function index(){const db=new DatabaseSync(path.join(root,'.claude/grain.db'));try{return db.prepare('SELECT DISTINCT sha256 FROM code_vectors WHERE path=?').all('source.rs').map(r=>r.sha256);}finally{db.close();}}
const sha=text=>crypto.createHash('sha256').update(text).digest('hex');
try {
  fs.writeFileSync(path.join(root,'mustard.json'),'{}');
  let source='pub fn read_json_file() { let entry_sentinel=1; parse_json(read_file()); }\npub fn sort_numbers() { sort(); }\n';
  fs.writeFileSync(path.join(root,'source.rs'),source);
  assert.equal(run(path.join(bin,'scan'),['scan',root,'--native','--out',path.join(root,'.claude/grain.db'),'--json']).status,0);
  const first=search();assert.ok(first.local_index_refresh.computed_vectors>0);
  assert.ok(first.task_context.investigation.local_model_calls>0);assert.deepEqual(index(),[sha(source)]);
  const repeat=search();assert.equal(repeat.local_index_refresh.computed_vectors,0);
  source=source.replace('read_json_file','load_json_document').replace('entry_sentinel=1','entry_sentinel=2');
  fs.writeFileSync(path.join(root,'source.rs'),source);
  const native=run('rg',request.request.input.args);
  const raw=run(path.join(bin,'mustard-rt'),['run','search','--root',root,'--raw','--intent',request.request.intent,'--purpose','implement','--','rg',...request.request.input.args]);
  assert.deepEqual(raw.stdout,native.stdout);assert.deepEqual(raw.stderr,native.stderr);assert.equal(raw.status,native.status);
  assert.ok(!index().includes(sha(source)),'Raw literal operation must not rebuild local vectors');
  const revised=search();assert.ok(revised.local_index_refresh.computed_vectors>0);
  assert.deepEqual(index(),[sha(source)]);
  assert.ok(revised.task_context.cards.some(c=>c.name==='load_json_document'&&c.source.sha256===sha(source)));
  fs.writeFileSync(path.join(root,'mustard.json'),' {"ai":{"vectors":false,"fallback":false}}');
  const disabled=search();assert.equal(disabled.local_index_refresh,undefined);
  assert.equal(disabled.task_context.investigation.local_model_calls,0);
  console.log(JSON.stringify({ok:true,default_local_vectors:true,first_semantic_request_builds_index:true,unchanged_documents_not_reembedded:true,
    raw_native_parity_without_vector_refresh:true,changed_source_receipts_refreshed:true,explicit_opt_out:true,remote_model_calls:0,
    binaries:Object.fromEntries(['mustard-rt','scan'].map(name=>[name,sha(fs.readFileSync(path.join(bin,name)))]))}));
}finally{fs.rmSync(root,{recursive:true,force:true});}
