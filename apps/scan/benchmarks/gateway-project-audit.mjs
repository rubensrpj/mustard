// Label-aware grading stays separate from source-first requests and providers.
// A JSON candidate counts only if its current source range reached the agent.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {completeBody,visibleReference as visible} from './source-evidence.mjs';
const flags=new Map();for(let at=2;at<process.argv.length;at+=2)flags.set(process.argv[at],process.argv[at+1]);
const required=name=>{assert.ok(flags.get(name),`Missing ${name}`);return path.resolve(flags.get(name));};
const labelsPath=required('--labels'),source=required('--source'),out=required('--out');
const labelsRaw=fs.readFileSync(labelsPath),labels=JSON.parse(labelsRaw).checks;
const sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
for(const label of labels)assert.equal(sha(fs.readFileSync(path.join(source,label.file))),label.sha256,'The labelled source changed');

const arms={};
for(const arm of ['baseline','current']){
 const directory=required(`--${arm}`),comparison=JSON.parse(fs.readFileSync(path.join(directory,'comparison.json')));
 const cases=comparison.rows.map(row=>({id:row.id,...JSON.parse(fs.readFileSync(path.join(directory,row.id+'.json')))}));
 const rows=labels.map(label=>{
  const matches=card=>card.name===label.name&&card.source.file===label.file&&card.source.sha256===label.sha256
   &&card.source.line<=label.line&&card.source.end_line>=label.end_line;
  const delivered=[],complete=[];let bestTrace=null;
  for(const item of cases){
   for(const card of [...(item.report.task_context?.cards||[]),...(item.report.task_context?.static_references||[])])if(matches(card)&&(card.initial_source_excerpt||card.initial_reference)&&visible(card,item.view)){
    delivered.push(item.id);
    if(completeBody(card,item.view))complete.push(item.id);
   }
   for(const trace of item.report.task_context?.retrieval_trace?.candidates||[])if(trace.source?.file===label.file&&trace.id.endsWith(':'+label.name)){
    if(!bestTrace||(trace.rank??Infinity)<(bestTrace.rank??Infinity))bestTrace={case:item.id,...trace};
   }
  }
  return {id:label.id,visible:delivered.length>0,complete_body:complete.length>0,cases:delivered,complete_cases:complete,
   best_trace:bestTrace&&{case:bestTrace.case,rank:bestTrace.rank,source_admission:bestTrace.source_admission,decision_status:bestTrace.decision_status,presentation_status:bestTrace.presentation_status}};
 });
 arms[arm]={requests_sha256:comparison.requests_sha256,binary_sha256:comparison.binary_sha256,rows,
  summary:{targets:labels.length,visible:rows.filter(row=>row.visible).length,complete_bodies:rows.filter(row=>row.complete_body).length,
   view_bytes:comparison.summary.view_bytes,native_parity:comparison.summary.native_parity,requests:comparison.summary.cases}};
}
assert.equal(arms.baseline.requests_sha256,arms.current.requests_sha256,'Compare the same requests');
const gains=arms.current.rows.filter(row=>row.visible&&!arms.baseline.rows.find(old=>old.id===row.id).visible).map(row=>row.id);
const regressions=arms.current.rows.filter(row=>!row.visible&&arms.baseline.rows.find(old=>old.id===row.id).visible).map(row=>row.id);
const report={labels_sha256:sha(labelsRaw),...arms,gains,regressions,
 limits:['Task-guided known targets, not an independent accuracy evaluation.','Visible source ranges are not proof of behavior or complete implementation context.','Output bytes are not billed tokens; no host model ran.']};
fs.mkdirSync(path.dirname(out),{recursive:true});fs.writeFileSync(out,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({baseline:arms.baseline.summary,current:arms.current.summary,gains,regressions},null,2));
