// Local content views. No prompt submission, model call or publication.
export const definitionNames = {
  context:['Objetivo e contexto','Goal and context'], decision:['Decisões','Decisions'], rule:['Regras','Rules'],
  criterion:['Critérios de aceite','Acceptance criteria'], task:['Tarefas','Tasks'], contract:['Entradas e saídas','Inputs and outputs'],
  limit:['Limites','Limits'], error:['Erros previstos','Expected errors'], edge_case:['Casos de borda','Edge cases'],
  out_of_scope:['Fora do escopo','Out of scope'], concern:['Pontos de atenção','Concerns'],
  point:['Perguntas do levantamento','Survey questions'], work_type:['Tipo de trabalho','Work type'],
};

export function definitionText(item, english=false) {
  const labels={when:['Quando','When'],then:['Resultado esperado','Expected result'],example:['Exemplo','Example'],
    value:['Valor','Value'],why:['Motivo','Reason'],reason:['Motivo','Reason'],proof:['Como verificar','How to verify'],
    message:['Mensagem prevista','Expected message'],expected:['Comportamento esperado','Expected behavior'],
    files:['Arquivos','Files'],depends_on:['Dependências','Dependencies'],covers:['Atende aos itens','Covers items'],
    contracts:['Contratos','Contracts'],gap:['Pergunta','Question'],facts:['Fatos relatados e fontes','Reported facts and sources'],
    kinds:['Tipos de trabalho','Work types'],waves:['Ondas','Waves']};
  const lines=[item.text || ''];
  for(const [field,names] of Object.entries(labels)) {
    const value=item[field];
    if(value===undefined || value===null || Array.isArray(value) && !value.length)continue;
    const printable=entry=>entry!==null && typeof entry==='object'?JSON.stringify(entry,null,2):String(entry);
    const body=Array.isArray(value) ? value.map(entry=>entry!==null && typeof entry==='object'
      ? `${entry.text || entry.path || printable(entry)}${entry.source ? ` (${printable(entry.source)})` : ''}` : printable(entry)).join('\n- ') : printable(value);
    lines.push(`**${names[english?1:0]}**\n${Array.isArray(value)?'- ':''}${body}`);
  }
  return lines.filter(Boolean).join('\n\n');
}

export function waveText(detail, view, english=false) {
  if(view==='Perfil') {
    if(!detail?.request_recorded)return english?'No request has been sent to an agent for this wave yet.':'Esta onda ainda não tem um pedido enviado ao agente.';
    const skills=detail.skills || [];
    return [
      `**${english?'Agent':'Agente'}**\n${detail.executor_requested || 'wave'}`,
      `**${english?'Skills delivered':'Skills enviadas'}**\n${skills.map(s=>`${s.name}\n${s.path}\n${s.read_confirmed===true?(english?'Read confirmed':'Leitura confirmada'):(english?'Reading not independently confirmed':'Leitura sem confirmação independente')}`).join('\n\n') || (english?'None recorded':'Nenhuma registrada')}`,
    ].join('\n\n');
  }
  if (!detail) return english ? 'Select a wave to load its recorded content.' : 'Selecione uma onda para carregar seu conteúdo registrado.';
  if (view==='Pedido') return detail.request ?? (english ? 'No request has been sent to an agent for this wave yet.' : 'Esta onda ainda não tem um pedido enviado ao agente.');
  if (view==='Entrega') {
    if(!detail.delivery)return english?'No return received yet.':'Nenhum retorno recebido ainda.';
    const observations=detail.delivery.observations || [];
    const statuses={failed:['Falha da ferramenta','Tool failure'],interrupted:['Interrompido','Interrupted'],returned:['Retorno capturado','Result captured']};
    return [english?'**Agent report · not independently verified facts**':'**Relato do agente · não comprova os fatos por si só**',detail.delivery.text,
      ...observations.map(item=>`${item.mode==='current'?(english?'Tool result in this dispatch':'Resultado de ferramenta neste envio'):(english?'Historical observation':'Observação histórica')} · ${item.observed_at}\n${statuses[item.result_status]?.[english?1:0] || ''}\n${item.source || item.tool}\n${item.text}\n${english?'Receipt':'Recibo'}: ${item.receipt}\n${english?'The receipt proves the tool result, not this interpretation.':'O recibo comprova o resultado da ferramenta, não esta interpretação.'}`)].filter(Boolean).join('\n\n');
  }
  if (view==='Testes') {
    const sections=[];
    if(detail.steps?.length)sections.push(`**${english?'Agent records · not independent execution evidence':'Registros do agente · não comprovam execução independente'}**\n`+
      detail.steps.map(item=>[item.item || '',item.text || JSON.stringify(item,null,2)].filter(Boolean).join('\n')).join('\n\n'));
    if(detail.tests?.length)sections.push(`**${english?'Criterion proofs executed by Mustard':'Provas de critério executadas pelo Mustard'}**\n`+
      detail.tests.map(item=>[item.criterion || '',item.result==='pass'?(english?'Passed':'Aprovada'):item.result==='fail'?(english?'Failed':'Falhou'):item.result || '',
        item.command || '',item.output || ''].filter(Boolean).join('\n')).join('\n\n'));
    else sections.push(english?'No criterion proof executed by Mustard for this wave yet.':'Nenhuma prova de critério executada pelo Mustard nesta onda.');
    return sections.join('\n\n');
  }
  const plan=detail.plan;
  if (!plan) return english ? 'No plan recorded.' : 'Nenhum plano registrado.';
  return [plan.text || '',plan.done_when ? `**${english?'Done when':'Concluída quando'}**\n${plan.done_when}` : '',
    detail.tasks?.length ? `**${english?'Tasks':'Tarefas'}**\n${detail.tasks.map(task=>`- ${task.code}: ${task.title || task.text || ''}`).join('\n')}` : '',
    detail.criteria?.length ? `**${english?'Acceptance criteria':'Critérios de aceite'}**\n${detail.criteria.map(item=>`- ${item.code}: ${item.when} → ${item.then}`).join('\n')}` : '',
    detail.user_input ? `${english?'Original request':'Solicitação de origem'}:\n${detail.user_input}` : '',
    plan.depends_on?.length ? `${english?'Depends on waves':'Depende das ondas'}: ${plan.depends_on.join(', ')}` : ''].filter(Boolean).join('\n\n');
}

// Keep each view small on both hosts while preserving the complete original.
// This local paging budget is not a claim about a host's maximum tree size.
export function contentPage(value, requested=0, size=4000) {
  const points=Array.from(String(value ?? ''));
  size=Math.max(1,Math.min(4000,Math.floor(size)||4000));
  const chunks=[];
  for(let start=0;start<points.length;) {
    let end=Math.min(points.length,start+size);
    if(end<points.length) {
      // Prefer complete paragraphs/lines, then words. Oversized uninterrupted
      // content still pages losslessly at the surface limit.
      const minimum=start+Math.floor(size/2);
      const boundary=separator=>{
        for(let at=end-separator.length;at>=minimum;at--)
          if(separator.every((point,n)=>points[at+n]===point))return at+separator.length;
      };
      end=boundary(['\n','\n']) ?? boundary(['\n']) ?? boundary([' ']) ?? end;
    }
    chunks.push(points.slice(start,end).join(''));start=end;
  }
  if(!chunks.length)chunks.push('');
  const pages=chunks.length;
  const page=Math.max(0,Math.min(requested,pages-1));
  return {text:chunks[page],page,pages};
}
