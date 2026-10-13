// Small host adapter. Native Rust owns business data; this module only keeps
// presentation state and moves versioned, allowlisted messages to the binary.
export const sections={Projeto:'project',Definições:'definitions',Ondas:'waves',Execução:'execution',Consumo:'usage',Ideias:'ideas',Diagnóstico:'diagnostics',Configurações:'settings'};
export const profileNames={overview:['Visão geral','Overview'],planning:['Planejamento','Planning'],execution:['Execução','Execution'],review:['Revisão','Review'],usage:['Consumo','Usage']};
export function hostReceipt(e,result) {
  const kind=result?'step':e.agentId?'complete':'measure';
  return {kind,agent_id:e.agentId,turn_id:e.turnId,index:e.index,model:result?.usage?.model || e.model,effort:typeof e.effort==='string'?e.effort:undefined,
    usage:result?.usage,aborted:e.isAborted===true,...(e.context?{context:e.context}:{}),...(e.rateLimits?{rateLimits:e.rateLimits}:{}),...(e.cost?{cost:e.cost}:{})};
}
export function usageFigures(row) {
  if(!row)return null;
  const input=row.input_tokens,read=row.cache_read_input_tokens,write=row.cache_creation_input_tokens;
  const complete=[input,read,write].every(value=>typeof value==='number' && Number.isFinite(value));
  return {input,read,write,output:row.output_tokens,cachePercent:complete && input+read+write>0?100*read/(input+read+write):null};
}
export function limitLabel(kind,english) {
  return ({five_hour:english?'5 hours':'5 horas',seven_day:english?'Week':'Semana'})[kind] || kind;
}
export function countdown(value,now=Date.now()) {
  const ms=Date.parse(value)-now;if(!Number.isFinite(ms))return '';
  if(ms<=0)return '0m';const minutes=Math.ceil(ms/60000),hours=Math.floor(minutes/60);
  return hours>=24?`${Math.floor(hours/24)}d ${hours%24}h`:hours?`${hours}h ${minutes%60}m`:`${minutes}m`;
}
