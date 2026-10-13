// Present observed scope and coverage beside each figure. These counters have
// different denominators and must never become a fabricated net-saving score.
export function economyOverview(data,spec,actions,h) {
  const {t,text,note,row,section,card,button,fmt,money,date}=h;
  const rtk=data.rtk || {},jev=data.jev || {},usage=spec?.usage || {},operations=spec?.operations || {},search=operations.search;
  const measuredWaves=(spec?.waves || []).filter(wave=>wave.tokens!=null).length;
  const totalWaves=spec?.waves?.length || 0;
  const detail=usage.known_wave_breakdown;
  const open=value=>()=>actions.change({usageView:value});
  const percent=operations.calls>0?100*operations.refusals/operations.calls:null;
  const headings={search:t('Busca','Search'),'wave-planning':t('Planejamento de ondas','Wave planning'),context:t('Seleção de contexto','Context selection')};
  return section(t('O Mustard está economizando?','Is Mustard saving tokens?'),[
    text(t('Economia total: não medida','Total savings: not measured'),{bold:true}),
    note(t('Já há medidas de redução de saída e de consumo. Falta comparar a mesma tarefa com e sem Mustard para calcular o ganho total.','Output reduction and usage can be measured. Total savings require the same task measured with and without Mustard.')),
    section(t('1. Redução de saída · projeto','1. Output reduction · project'),rtk.status==='measured'?[
      row([card(t('RTK · redução estimada','RTK · estimated reduction'),fmt(rtk.percent)+'%'),card(t('Tokens de saída evitados · estimativa','Output tokens avoided · estimate'),fmt(rtk.saved_tokens))]),
      text(`${fmt(rtk.commands)} ${t('comandos','commands')} · ${fmt(rtk.input_tokens)} → ${fmt(rtk.output_tokens)} tokens`),
      note(`${t('Histórico deste diretório','History for this directory')}: ${rtk.period_start || '—'} → ${rtk.period_end || '—'}`),
      ...(rtk.stale?[note(t('Última atualização falhou; estes números são da medição anterior.','Latest refresh failed; these are the previous measurement’s figures.'))]:[]),
      note(t('Estimativa por tamanho da saída, não redução da fatura. Não atribuir esse histórico inteiro à spec selecionada.','Output-size estimate, not a bill reduction. Do not attribute the whole history to the selected spec.')),
    ]:[text(t('RTK ainda sem medição deste projeto.','RTK has no project measurement yet.')),
      note(t('Use Atualizar medições para consultar o histórico local.','Use Refresh measurements to query local history.'))]),
    section(t('2. Jev · gasto adicional no projeto','2. Jev · additional project spend'),[
      row([card(t('Chamadas registradas','Recorded requests'),fmt(jev.physical_requests)),card(t('Custo estimado','Estimated cost'),money(jev.cost_micro_usd))]),
      ...Object.entries(jev.by_purpose || {}).map(([purpose,count])=>text(`${headings[purpose] || purpose}: ${fmt(count)}`)),
      ...(jev.last_attempt?[note(`${t('Última chamada','Latest request')}: ${date(jev.last_attempt.at)}`)]:[]),
      note(t('Uso do Jev não comprova ganho. A comparação deve incluir esse custo.','Jev usage does not prove savings. A comparison must include this cost.')),
      button('economy-jev',t('Ver chamadas e configuração','View requests and configuration'),open('Jev')),
    ]),
    section(t('3. Ondas · spec selecionada','3. Waves · selected spec'),spec?[
      text(spec.name,{bold:true}),
      row([card(t('Tokens das ondas medidas','Measured wave tokens'),usage.known_wave_tokens!=null?fmt(usage.known_wave_tokens):t('Não medido','Not measured')),
        card(t('Cobertura da medição','Measurement coverage'),`${measuredWaves}/${totalWaves}`)]),
      text(`${t('Orquestrador · acumulado observado','Orchestrator · observed accumulated usage')}: ${fmt(usage.conductor_tokens)} tokens`),
      note(t('O acumulado do orquestrador pode incluir trabalho anterior na mesma sessão.','The orchestrator’s cumulative usage may include earlier work in the same session.')),
      ...(detail?[text(`${t('Ondas · cache relido / saída','Waves · cache reads / output')}: ${fmt(detail.cache_read_input_tokens)} / ${fmt(detail.output_tokens)} tokens`)]:[]),
      ...(usage.claude_cost?.known_micro_usd!=null?[text(`${t('Spec · custo equivalente de API observado','Spec · observed API-equivalent cost')}: ${money(usage.claude_cost.known_micro_usd)}`),
        note(t('Estimativa do host; não é cobrança da sua assinatura.','Host estimate; not a charge on your subscription.'))]:[]),
      ...(usage.waves_with_unknown_usage>0?[note(`${fmt(usage.waves_with_unknown_usage)} ${t('ondas sem medição. O subtotal não representa o consumo completo.','unmeasured waves. The subtotal is not complete usage.')}`)]:[]),
      note(t('Tokens acumulam as respostas e releituras do cache; não são o tamanho da janela de contexto.','Tokens accumulate responses and cache reads; they are not the context-window size.')),
      button('economy-waves',t('Comparar consumo por onda','Compare usage per wave'),open('Ondas')),
    ]:[note(t('Selecione uma spec para acompanhar as ondas.','Select a spec to inspect waves.'))]),
    section(t('4. Scan e atrito · spec selecionada','4. Scan and friction · selected spec'),[
      text(search?.calls?`${t('Buscas registradas','Recorded searches')}: ${fmt(search.calls)} · ${t('com índice','with index')}: ${fmt(search.enriched)} · ${t('retorno nativo','native fallback')}: ${fmt(search.fallback)}`:
        t('Nenhuma busca medida nesta spec. Utilidade do scan ainda não demonstrada.','No measured search for this spec. Scan usefulness has not been demonstrated yet.')),
      ...(search?.calls?[text(`${t('Modelo local / Jev nas buscas','Local model / Jev in searches')}: ${fmt(search.local_model_calls)} / ${fmt(search.remote_model_calls)}`),
        ...(search.paired_calls>0?[text(`${t('Saída nativa → entregue nas buscas medidas','Native → delivered output in measured searches')}: ${fmt(search.native_bytes)} → ${fmt(search.delivered_bytes)} bytes`),
          note(`${fmt(search.paired_calls)}/${fmt(search.calls)} ${t('consultas comparáveis. Mede volume da mesma consulta, não acerto nem economia total.','comparable queries. Measures same-query volume, not accuracy or total savings.')}`)]:
          [note(t('Histórico sem comparação de tamanho da mesma consulta; novas buscas passam a registrar original e entregue.','History lacks a same-query size comparison; new searches record original and delivered sizes.'))]),
        note(t('Usar o índice não prova que o resultado resolveu a pergunta. Ainda falta medir acerto e leituras posteriores.','Using the index does not prove that a result answered the question. Accuracy and subsequent reads still need measurement.'))]:[]),
      text(`${t('Recusas de comandos do Mustard','Mustard command refusals')}: ${fmt(operations.refusals)} / ${fmt(operations.calls)}${percent!=null?' ('+fmt(Math.round(percent*10)/10)+'%)':''}`),
      note(t('Recusas incluem validações e proteções. Repetições geram atrito, mas essa contagem não mede tokens desperdiçados.','Refusals include validations and protections. Retries create friction, but this count does not measure wasted tokens.')),
      button('economy-search',t('Ver medições de busca','View search measurements'),open('Busca')),
    ]),
  ]);
}
