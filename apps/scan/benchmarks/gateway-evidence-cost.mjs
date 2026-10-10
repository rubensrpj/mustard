// Compare delivery + actual missing-range Reads for the SAME known evidence.
// This is answer-informed expansion after retrieval, not an autonomous agent
// or a billed-token benchmark. Labels never enter the initial search requests.
import fs from 'node:fs';import path from 'node:path';import cp from 'node:child_process';
import assert from 'node:assert/strict';import crypto from 'node:crypto';
import {deliveredLines,visibleReference as visible} from './source-evidence.mjs';
const flags=new Map();for(let at=2;at<process.argv.length;at+=2)flags.set(process.argv[at],process.argv[at+1]);
const required=name=>{assert.ok(flags.get(name),`Missing ${name}`);return path.resolve(flags.get(name));};
const root=required('--root'),labelsFile=required('--labels'),out=required('--out');
const labels=JSON.parse(fs.readFileSync(labelsFile)).checks;
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
const sources=new Map();for(const label of labels){const bytes=fs.readFileSync(path.join(root,label.file));assert.equal(sha(bytes),label.sha256);sources.set(label.file,bytes.toString().split(/\r?\n/));}
const arms={};

for(const arm of ['baseline','current']){
 const directory=required(`--${arm}`),comparison=JSON.parse(fs.readFileSync(path.join(directory,'comparison.json')));
 const cases=comparison.rows.map(row=>JSON.parse(fs.readFileSync(path.join(directory,row.id+'.json'))));
 const located=labels.filter(label=>cases.some(item=>[...(item.report.task_context?.cards||[]),...(item.report.task_context?.static_references||[])].some(card=>
  (card.initial_source_excerpt||card.initial_reference)&&card.name===label.name&&card.source.file===label.file&&card.source.sha256===label.sha256
   &&card.source.line<=label.line&&card.source.end_line>=label.end_line&&visible(card,item.view))));
 const delivered=new Map();let repeated=0;
 for(const item of cases)for(const [file,lines] of deliveredLines(item.view)){
  if(!sources.has(file))continue;const known=delivered.get(file)||new Set();
  for(const [line,text] of lines)if(sources.get(file)[line-1]===text){if(known.has(line))repeated++;known.add(line);}
  delivered.set(file,known);
 }
 arms[arm]={directory,cases,located,delivered,requests_sha256:comparison.requests_sha256,
  bootstrap_bytes:cases.reduce((n,item)=>n+Buffer.byteLength(item.view),0),repeated_required_file_source_lines:repeated};
}
assert.equal(arms.baseline.requests_sha256,arms.current.requests_sha256);
const common=labels.filter(label=>['baseline','current'].every(arm=>arms[arm].located.some(found=>found.id===label.id)));
assert.ok(common.length,'No common located evidence to compare');
const env={...process.env,MUSTARD_RT_DELEGATED:'1'};for(const key of ['TYPESAFE_API_KEY','MUSTARD_JEV_URL','MUSTARD_SEARCH_TRACE','CLAUDE_PLUGIN_ROOT'])delete env[key];
const measured={};
for(const arm of ['baseline','current']){
 const state=arms[arm],missing=new Map(),reads=[];let readBytes=0,readLines=0;
 for(const label of common){const lines=missing.get(label.file)||new Set();
  for(let line=label.line;line<=label.end_line;line++)if(!state.delivered.get(label.file)?.has(line))lines.add(line);
  missing.set(label.file,lines);
 }
 for(const [file,numbers] of missing){
  const intervals=[];for(const line of [...numbers].sort((a,b)=>a-b)){const last=intervals.at(-1);if(last&&last[1]+1===line)last[1]=line;else intervals.push([line,line]);}
  for(const [from,to] of intervals){
   const request={schema_version:1,request:{tool:'Read',input:{file_path:file,offset:from,limit:to-from+1},intent:'Read the missing current source range already located in the task evidence',purpose:'locate',choose:false}};
   const result=cp.spawnSync(path.join(required(`--${arm}-bin`),'mustard-rt'),['run','search','--root',root,'--request',JSON.stringify(request),'--shell-output'],{cwd:root,env,maxBuffer:32*1024*1024,timeout:60000});
   assert.equal(result.status,0,result.stderr.toString()||result.stdout.toString());
   const delivered=JSON.parse(result.stdout);assert.equal(delivered.offset,from);assert.equal(delivered.numLines,to-from+1);
   assert.equal(delivered.content,sources.get(file).slice(from-1,to).join('\n'));
   readBytes+=result.stdout.length;readLines+=to-from+1;reads.push({file,offset:from,limit:to-from+1,bytes:result.stdout.length});
  }
 }
 measured[arm]={located_targets:state.located.map(label=>label.id),unlocated_targets:labels.filter(label=>!state.located.some(found=>found.id===label.id)).map(label=>label.id),
  bootstrap_bytes:state.bootstrap_bytes,required_file_repeated_source_lines:state.repeated_required_file_source_lines,
  missing_range_reads:reads.length,missing_source_lines:readLines,additional_read_bytes:readBytes,
  equivalent_evidence_bytes:state.bootstrap_bytes+readBytes,reads};
}
const result={labels_sha256:sha(fs.readFileSync(labelsFile)),requests_sha256:arms.baseline.requests_sha256,required_targets:labels.length,comparable_targets:common.map(label=>label.id),...measured,
 equivalent_evidence_byte_reduction_percent:100*(1-measured.current.equivalent_evidence_bytes/measured.baseline.equivalent_evidence_bytes),
 whole_session_tokens:null,whole_session_cost:null,host_model_ran:false,
 limits:['Known source-evidence targets; not a blind evaluation.','Expansions are answer-informed and identical required ranges are graded in both arms; this is a delivery-cost lower bound, not an autonomous tool trajectory.','Unlocated targets are explicit and excluded from the common-evidence cost; they are not declared solved.','Bytes include actual Read responses, not billed tokens or correctness of a complete implementation.']};
fs.mkdirSync(path.dirname(out),{recursive:true});fs.writeFileSync(out,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({comparable_targets:common.length,baseline:measured.baseline,current:measured.current,equivalent_evidence_byte_reduction_percent:result.equivalent_evidence_byte_reduction_percent},null,2));
