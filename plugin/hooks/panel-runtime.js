// Small host adapter. Native Rust owns business data; this module only moves
// host observations to the binary and formats what the host measures.
export function hostReceipt(e,result) {
  const kind=result?'step':e.agentId?'complete':'measure';
  return {kind,agent_id:e.agentId,turn_id:e.turnId,index:e.index,model:result?.usage?.model || e.model,effort:typeof e.effort==='string'?e.effort:undefined,
    usage:result?.usage,aborted:e.isAborted===true,...(e.context?{context:e.context}:{}),...(e.rateLimits?{rateLimits:e.rateLimits}:{}),...(e.cost?{cost:e.cost}:{})};
}
export function countdown(value,now=Date.now()) {
  const ms=Date.parse(value)-now;if(!Number.isFinite(ms))return '';
  if(ms<=0)return '0m';const minutes=Math.ceil(ms/60000),hours=Math.floor(minutes/60);
  return hours>=24?`${Math.floor(hours/24)}d ${hours%24}h`:hours?`${hours}h ${minutes%60}m`:`${minutes}m`;
}
