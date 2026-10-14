// Keep the first screen useful. Comparisons retain their own scopes; details
// and methodological limits remain in the individual measurement views.
export function economyOverview(data,spec,actions,h) {
  const {t,text,note,row,section,card,button,fmt,money,date}=h;
  const rtk=data.rtk || {},jev=data.jev || {},usage=spec?.usage || {},operations=spec?.operations || {},search=operations.search;
  const measured=(spec?.waves || []).filter(wave=>wave.tokens!=null).length;
  const delivered=spec?.delivery_metrics;
  const open=value=>()=>actions.change({usageView:value});
  const refusals=operations.calls>0?Math.round(1000*operations.refusals/operations.calls)/10:null;
  const paired=search?.paired_calls>0;
  const change=paired && search.native_bytes>0?Math.round(1000*(search.delivered_bytes/search.native_bytes-1))/10:null;
  const period=value=>typeof value==='string' && /^\d{4}-\d{2}-\d{2}$/.test(value)
    ? t(value.split('-').reverse().join('/'),value) : date(value);
  return section(t('O Mustard está economizando?','Is Mustard saving tokens?'),[
    text(t('Economia líquida: ainda não comprovada','Net savings: not yet demonstrated'),{bold:true}),
    note(t('Acompanhe primeiro entrega, retrabalho e consumo. Percentual de economia exige comparar a mesma tarefa com e sem Mustard.','Start with delivery, rework and usage. A savings percentage requires comparing the same task with and without Mustard.')),
    section(t('Spec selecionada · consumo e atrito','Selected spec · usage and friction'),spec?[
      ...(delivered?[
        row([card(t('Ondas aceitas','Accepted waves'),`${fmt(delivered.accepted_waves)}/${fmt(delivered.planned_waves)}`),
          card(t('Critérios com prova registrada','Criteria with recorded proof'),fmt(delivered.verified_criteria))]),
        text(`${t('Tentativas de despacho','Dispatch attempts')}: ${fmt(delivered.dispatch_attempts)} · ${t('ondas com repetição','waves with retries')}: ${fmt(delivered.waves_with_retries)}`),
        ...(delivered.zero_change_deliveries>0?[note(`${fmt(delivered.zero_change_deliveries)} ${t('retornos sem alteração: confira bloqueios ou tarefas repetidas.','returns without changes: check blockers or repeated tasks.')}`)]:[]),
        text(`${t('Tokens por onda aceita, incluindo tentativas','Tokens per accepted wave, including attempts')}: ${fmt(delivered.tokens_per_accepted_wave)}`),
        ...(delivered.accepted_attempts_without_usage>0?[note(`${fmt(delivered.accepted_attempts_without_usage)} ${t('tentativas de ondas aceitas sem consumo conhecido; o custo por entrega fica não medido.','accepted wave attempts without known usage; cost per delivery remains unmeasured.')}`)]:[]),
      ]:[]),
      row([card(t('Tokens das ondas medidas','Measured wave tokens'),fmt(usage.known_wave_tokens)),
        card(t('Cobertura da medição','Measurement coverage'),`${measured}/${spec.waves?.length || 0}`)]),
      ...(usage.waves_with_unknown_usage>0?[note(`${fmt(usage.waves_with_unknown_usage)} ${t('ondas sem medição; subtotal incompleto.','unmeasured waves; incomplete subtotal.')}`)]:[]),
      text(`${t('Recusas de comandos do Mustard','Mustard command refusals')}: ${fmt(operations.refusals)} / ${fmt(operations.calls)}${refusals!=null?' ('+fmt(refusals)+'%)':''}`),
      button('economy-waves',t('Ver consumo e entregas por onda','View usage and deliveries per wave'),open('Ondas')),
    ]:[note(t('Selecione uma spec para acompanhar as ondas.','Select a spec to inspect waves.'))]),
    section(t('Scan · buscas desta spec','Scan · searches in this spec'),[
      text(search?.calls?`${t('Buscas','Searches')}: ${fmt(search.calls)} · ${t('com índice','with index')}: ${fmt(search.enriched)} · ${t('retorno nativo','native fallback')}: ${fmt(search.fallback)}`:
        t('Nenhuma busca medida nesta spec.','No measured search for this spec.')),
      ...(search?.calls>0?[text(`${t('Tempo médio observado','Observed average latency')}: ${fmt(Math.round(search.ms/search.calls))} ms`),
        note(t('Com índice não significa acerto. Retorno nativo mantém a busca funcionando quando o índice não ajuda.','Indexed does not mean accurate. Native fallback keeps retrieval working when the index adds no value.'))]:[]),
      ...(data.knowledge?.analysis?[text(`${t('Análises de código reutilizadas / concluídas','Reused / completed code analyses')}: ${fmt(data.knowledge.analysis.reused_operations)} / ${fmt(data.knowledge.analysis.completed_analyses)}`),
        note(t('O consumo de Haiku/Luna já está incluído no histórico do host; não somar novamente.','Haiku/Luna usage is already included in host history; do not add it again.'))]:[]),
      ...(paired?[
        text(`${t('Saída nativa → entregue nas buscas medidas','Native → delivered output in measured searches')}: ${fmt(search.native_bytes)} → ${fmt(search.delivered_bytes)} bytes`),
        ...(change!=null?[note(change<0?t(`${fmt(-change)}% menos texto; acerto ainda não medido.`,`${fmt(-change)}% less text; accuracy not yet measured.`):
          change>0?t(`${fmt(change)}% mais texto; verificar se o contexto adicional ajudou.`,`${fmt(change)}% more text; check whether the extra context helped.`):t('Mesmo volume de texto; acerto ainda não medido.','Same text volume; accuracy not yet measured.'))]:[]),
      ]:[note(t('Utilidade ainda não demonstrada por comparação de consultas.','Usefulness has not been demonstrated by comparable queries.'))]),
      button('economy-search',t('Ver comparação das buscas','View search comparison'),open('Busca')),
    ]),
    section(t('Projeto · histórico separado da spec','Project · history separate from this spec'),[
      row([card(t('RTK · redução estimada de saída','RTK · estimated output reduction'),rtk.status==='measured'?fmt(rtk.percent)+'%':t('Não medido','Not measured')),
        card(t('Jev · custo adicional estimado','Jev · estimated additional cost'),money(jev.cost_micro_usd))]),
      text(`${t('Jev · chamadas registradas','Jev · recorded requests')}: ${fmt(jev.physical_requests)}`),
      ...(rtk.stale?[note(t('RTK: última atualização falhou; histórico anterior.','RTK: latest refresh failed; showing previous history.'))]:[]),
      ...(rtk.period_start?[note(`${t('RTK · período','RTK · period')}: ${period(rtk.period_start)} → ${period(rtk.period_end)}`)]:[]),
      note(t('RTK estima volume de saída; Jev registra gasto. Nenhum deles comprova redução da fatura.','RTK estimates output volume; Jev records spend. Neither proves a bill reduction.')),
      row([button('economy-rtk',t('Detalhes do RTK','RTK details'),open('RTK')),
        button('economy-jev',t('Chamadas Jev','Jev requests'),open('Jev')),
        button('economy-session',t('Sessão do orquestrador','Orchestrator session'),open('Sessão'))]),
    ]),
  ]);
}
