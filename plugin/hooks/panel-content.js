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
    contracts:['Contratos','Contracts'],gap:['Pergunta','Question'],facts:['Fatos verificados','Verified facts'],
    kinds:['Tipos de trabalho','Work types'],waves:['Ondas','Waves']};
  const lines=[item.text || ''];
  for(const [field,names] of Object.entries(labels)) {
    const value=item[field];
    if(value===undefined || value===null || Array.isArray(value) && !value.length)continue;
    const body=Array.isArray(value) ? value.map(entry=>typeof entry==='object' ? `${entry.text || entry.path || JSON.stringify(entry)}${entry.source ? ` (${entry.source})` : ''}` : entry).join('\n- ') : String(value);
    lines.push(`**${names[english?1:0]}**\n${Array.isArray(value)?'- ':''}${body}`);
  }
  return lines.filter(Boolean).join('\n\n');
}

export function waveText(detail, view, english=false) {
  if (!detail) return english ? 'Select a wave to load its recorded content.' : 'Selecione uma onda para carregar seu conteúdo registrado.';
  if (view==='Pedido') return detail.request ?? (english ? 'No request has been sent to an agent for this wave yet.' : 'Esta onda ainda não tem um pedido enviado ao agente.');
  if (view==='Entrega') return detail.delivery?.text ?? (english ? 'No return received yet.' : 'Nenhum retorno recebido ainda.');
  if (view==='Testes') {
    const entries=[...(detail.steps || []),...(detail.tests || [])];
    return entries.length ? entries.map(item=>[item.item || item.criterion || '', item.text || JSON.stringify(item,null,2)].filter(Boolean).join('\n')).join('\n\n')
      : english ? 'No task validation recorded yet.' : 'Nenhuma validação de tarefa registrada ainda.';
  }
  const plan=detail.plan;
  if (!plan) return english ? 'No plan recorded.' : 'Nenhum plano registrado.';
  return [plan.text || '',plan.done_when ? `**${english?'Done when':'Concluída quando'}**\n${plan.done_when}` : '',
    detail.tasks?.length ? `**${english?'Tasks':'Tarefas'}**\n${detail.tasks.map(task=>`- ${task.code}: ${task.title || task.text || ''}`).join('\n')}` : '',
    detail.criteria?.length ? `**${english?'Acceptance criteria':'Critérios de aceite'}**\n${detail.criteria.map(item=>`- ${item.code}: ${item.when} → ${item.then}`).join('\n')}` : '',
    detail.user_input ? `${english?'Original request':'Solicitação de origem'}:\n${detail.user_input}` : '',
    plan.depends_on?.length ? `${english?'Depends on waves':'Depende das ondas'}: ${plan.depends_on.join(', ')}` : ''].filter(Boolean).join('\n\n');
}

// The official text surface limit is 10,000 characters. Page the complete
// original text; no silent truncation and no invented reconstruction.
export function contentPage(value, requested=0, size=4000) {
  const points=Array.from(String(value ?? ''));
  const pages=Math.max(1,Math.ceil(points.length/size));
  const page=Math.max(0,Math.min(requested,pages-1));
  return {text:points.slice(page*size,(page+1)*size).join(''),page,pages};
}
