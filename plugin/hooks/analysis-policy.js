// Native-session analysis policy. These decisions never call a model.
export const isAnalysis = type => typeof type==='string' && type.split(':').at(-1)==='mustard-analysis';
export function analysisSpawn(event) {
  if(!isAnalysis(event.subagentType))return event;
  if(event.fork)return {deny:'Mustard analysis requires an isolated native subagent. A fork inherits the parent transcript/model; use native evidence instead.'};
  if(typeof event.prompt!=='string' || event.prompt.length>32000)return {deny:'Mustard analysis expects a bounded scan package; prepare a smaller source-bound question first.'};
  // The agent definition already pins the session's small model. Rewriting the
  // model here sends the bare alias down a path the host rejects as unknown, so
  // a different family is refused instead of rewritten.
  if(event.model!=null && !/haiku|luna/.test(event.model))return {deny:'Mustard analysis runs on the session small model: launch it without `model`, or with `model: haiku`.'};
  return event;
}
export function fullAnalysisInput(usage) {
  const parts=[usage?.input_tokens,usage?.cache_read_input_tokens,usage?.cache_creation_input_tokens];
  return parts.every(n=>Number.isSafeInteger(n) && n>=0)?parts.reduce((sum,n)=>sum+n,0):null;
}
export function analysisTool(event,state) {
  if(!state)return null;
  // Reserve headroom for the final answer and native tool/schema overhead.
  if(state.input!=null && state.input>=90000)return {decision:'deny',reason:'Mustard analysis is near its fixed 100000-token input budget. Return the current result now; do not read more or start another analysis.'};
  const tool=event.tool;
  const request=event.input?.request || event.input;
  const safe=['Read','Grep','Glob','ToolSearch'].includes(tool) || tool==='mcp__mustard__search'
    && ['Read','Grep','Glob','Symbol','Trace','References','Structure'].includes(request?.tool);
  return safe?null:{decision:'deny',reason:'Mustard analysis is read-only: supplied evidence or code discovery only. Return the requested receipt to the conductor; no editing, shell execution or further delegation.'};
}
