// Label-aware grading is separate from answer-blind search/provider requests.
// Audit actual agent text, regressions and opt-in retrieval-stage metadata.
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
const args=process.argv.slice(2),option=n=>{assert.ok(args.includes(n),`Missing ${n}`);return path.resolve(args[args.indexOf(n)+1]);};
const root=option('--results'),selection=JSON.parse(fs.readFileSync(option('--selection'))),dataset=JSON.parse(fs.readFileSync(option('--dataset')));
const comparison=JSON.parse(fs.readFileSync(path.join(root,'comparison.json'))),rows=[];
for(const [ri,picked] of selection.repositories.entries()) {
 const repo=dataset[picked.language].find(r=>r.repo===picked.repo&&r.commit_sha===picked.commit);assert.ok(repo);
 for(const [id,needle] of repo.needles.entries()) for(const arm of ['baseline','current']) {
  const saved=JSON.parse(fs.readFileSync(path.join(root,`${ri}-${id}-${arm}.json`))),report=saved.report,ctx=report.task_context||{};
  const matches=c=>c?.source?.file===needle.path&&c.name===needle.name&&c.source.line<=needle.start_line+1&&c.source.end_line>=needle.end_line;
  const matchesId=value=>typeof value==='string'&&value.startsWith(needle.path+':')&&value.endsWith(':'+needle.name);
  const cards=ctx.cards||[],target=cards.find(matches);let file=null,ordinal=0,rank=null;
  for(const line of saved.view.split('\n')) {
   if(line.startsWith('@ '))file=line.slice(2);
   if(/^# .+ \d+-\d+ \[/.test(line)){ordinal++;if(target&&file===target.source.file&&line.startsWith(`# ${target.name} ${target.source.line}-${target.source.end_line} [`))rank=ordinal;}
   if(line.startsWith('# References (expand source/responsibility): '))for(const reference of line.split(': ').slice(1).join(': ').split('; ')){
    ordinal++;if(target&&file===target.source.file&&reference===`${target.name} ${target.source.line}-${target.source.end_line}`)rank=ordinal;
   }
  }
  const trace=(ctx.retrieval_trace?.candidates||[]).find(c=>c.source?.file===needle.path&&matchesId(c.id));
  rows.push({arm,repo:picked.repo,id,expected:{file:needle.path,name:needle.name},
   original_native_owner:(report.evidence?.current_owner_ids||[]).some(matchesId),
   complementary_native_owner:(ctx.complementary_discovery?.added_owner_ids||[]).some(matchesId),
   hydrated:ctx.retrieval_trace?Boolean(trace):null,rank_after_expansion:trace?.rank??null,decision_retained:trace?.decision_retained??null,
   returned:Boolean(target),visible_rank:rank,complete_body:Boolean(target?.initial_source_excerpt&&target.source_excerpt?.truncated===false&&saved.view.includes(target.source_excerpt.text)),
   source_hash_verified:target?.source.sha256??null});
 }
}
const summary=Object.fromEntries(['baseline','current'].map(arm=>{const a=rows.filter(r=>r.arm===arm);return [arm,{cases:a.length,returned:a.filter(r=>r.returned).length,
 visible:a.filter(r=>r.visible_rank!==null).length,complete_bodies:a.filter(r=>r.complete_body).length,
 recall_at_5:a.filter(r=>r.visible_rank!==null&&r.visible_rank<=5).length/a.length,
 recall_at_10:a.filter(r=>r.visible_rank!==null&&r.visible_rank<=10).length/a.length,
 mean_reciprocal_visible_rank:a.reduce((sum,r)=>sum+(r.visible_rank?1/r.visible_rank:0),0)/a.length}];}));
const regressions=[],gains=[];
for(const row of rows.filter(r=>r.arm==='current')) {
 const before=rows.find(r=>r.arm==='baseline'&&r.repo===row.repo&&r.id===row.id);
 if(before.returned&&!row.returned)regressions.push(row);
 if(!before.returned&&row.returned)gains.push(row);
}
const result={dataset_sha256:comparison.dataset_sha256,selection_sha256:comparison.selection_sha256,summary,gains,regressions,rows,
 limits:['Known function retrieval, not precision of every emitted reference or implementation correctness.','Native owner diagnostics use file/name; returned cards use declared source interval.','An absent trace rank cannot distinguish source admission from no written-clue score.','No host LLM ran; output bytes and reciprocal ranks are not billed tokens.']};
fs.writeFileSync(path.join(root,'audit.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({summary,gains:gains.length,regressions:regressions.length},null,2));
