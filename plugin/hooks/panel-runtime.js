// Small host adapter. Native Rust owns business data; this module only moves
// host observations to the binary.
export function hostReceipt(e,result) {
  const kind=result?'step':e.agentId?'complete':'measure';
  return {kind,agent_id:e.agentId,turn_id:e.turnId,index:e.index,model:result?.usage?.model || e.model,effort:typeof e.effort==='string'?e.effort:undefined,
    usage:result?.usage,aborted:e.isAborted===true,...(e.context?{context:e.context}:{}),...(e.rateLimits?{rateLimits:e.rateLimits}:{}),...(e.cost?{cost:e.cost}:{})};
}
