// Safe projection only: credentials are neither editable nor received here.
export function settingsPanel(state,actions,{t,text,note,row,section,button,picker},group='Busca') {
  const errors={
    'settings-unreadable':['Não foi possível ler mustard.json. Corrija o arquivo antes de editar.','Cannot read mustard.json. Repair the file before editing.'],
    'settings-conflict':['A configuração mudou fora do painel. Recarregue e reaplique suas escolhas.','Settings changed outside the panel. Reload and reapply your choices.'],
    'settings-local-invalid':['As configurações locais têm formato inválido. Corrija-as antes de alterar o RTK.','Local settings are invalid. Repair them before changing RTK.'],
    'settings-apply-incomplete':['As escolhas foram gravadas, mas alguns arquivos não foram atualizados. Execute mustard-rt run upsert.','Choices were saved but some files were not updated. Run mustard-rt run upsert.'],
    'settings-busy':['Outra gravação está em andamento. Tente salvar novamente.','Another write is in progress. Try saving again.'],
  };
  const result=[note(t('Configurações deste projeto · alterações somente ao salvar.','Project settings · changes apply only when saved.'))];
  const health=state?.availability;
  if(health && group==='Geral')result.push(note(t(`Versão em execução: ${health.running_version} · configuração: ${health.project_version || 'não informada'}`,`Running version: ${health.running_version} · configured: ${health.project_version || 'not provided'}`)),
    note(health.installation_receipt_current?t('Instalação conferida para esta versão.','Installation verified for this version.'):
      t('A instalação precisa ser conferida. Consulte Diagnóstico antes de atualizar uma execução ativa.','Installation needs verification. Check Diagnostics before updating an active run.')));
  if(health && group==='Busca')result.push(note(t(`Vetores indexados: ${health.vectors?.indexed_chunks ?? 'não medido'}. Habilitar o modelo não comprova que ele foi utilizado.`,`Indexed vectors: ${health.vectors?.indexed_chunks ?? 'not measured'}. Enabling the model does not prove usage.`)));
  if(state?.message)result.push(text(t(...state.message)));
  if(state?.error)result.push(text(t(...(errors[state.error] || ['Não foi possível concluir a operação. Suas edições foram preservadas.','The operation could not complete. Your edits were preserved.']))));
  if(!state?.loaded) {
    result.push(note(t(state?.busy?'Carregando configurações…':'Configurações indisponíveis.',state?.busy?'Loading settings…':'Settings unavailable.')));
    result.push(button('settings-reload',t('Recarregar configurações','Reload settings'),actions.reloadSettings,{disabled:state?.busy}));
    return section(t('Configurações','Settings'),result);
  }
  const values={...state.values,...state.changes},dirty=Object.keys(state.changes).length>0;
  result.push(picker('settings-group',t('Grupo','Group'),[['Busca','Busca','Search'],['Jev','Jev','Jev'],['Agentes','Agentes','Agents'],['Geral','Geral','General']]
    .map(([value,pt,en])=>({value,label:t(pt,en)})),group,value=>actions.change({settingsGroup:value})));
  const toggle=(field,label,description)=>section(t(...label),[
    note(t(...description)),row([
      button(`setting-${field}-on`,t('Habilitado','Enabled'),()=>actions.editSetting(field,true),{active:values[field]===true,disabled:state.busy}),
      button(`setting-${field}-off`,t('Desabilitado','Disabled'),()=>actions.editSetting(field,false),{active:values[field]===false,disabled:state.busy}),
    ])]);
  if(group==='Geral')result.push(toggle('enabled',['Mustard no projeto','Mustard in this project'],['Ativa os recursos do Mustard neste projeto.','Enables Mustard features in this project.']),
    toggle('rtk',['RTK','RTK'],['Compacta saídas de comandos compatíveis. Salvar também atualiza o gancho local.','Compacts supported command outputs. Saving also updates the local hook.']));
  if(group==='Geral' && state.rtk?.own_hook_left===true)result.push(note(t('O gancho do próprio RTK continua nas configurações locais e disputa a troca com o Mustard. Execute mustard-rt run upsert para tirá-lo.','RTK\'s own hook is still in the local settings and races Mustard\'s swap. Run mustard-rt run upsert to take it out.')));
  if(group==='Busca')result.push(toggle('vectors',['Modelo local da busca','Local search model'],['Modelo embarcado para busca vetorial. Roda na máquina, sem Ollama ou cobrança de API.','Embedded model for vector search. Runs locally, without Ollama or API charges.']),
    toggle('search_answer',['Resposta compacta da busca','Compact search response'],['Usa evidências do índice quando válidas e mantém o retorno nativo como reserva.','Uses valid index evidence and keeps native results as fallback.']));
  if(group==='Jev') {
  result.push(toggle('jev',['Jev · decisões criteriosas','Jev · selective decisions'],['Permite classificar e escolher quando a análise nativa não basta. Habilitar não força chamadas.','Allows classification and choice when native analysis is insufficient. Enabling does not force requests.']));
  result.push(note(state.jev?.usable_key?t('Chave disponível · crédito e conexão não foram verificados.','Key available · credit and connection were not verified.'):
    t('Sem chave disponível. Configure TYPESAFE_API_KEY no ambiente privado; habilitar sozinho não ativa chamadas.','No key available. Configure TYPESAFE_API_KEY in the private environment; enabling alone cannot activate requests.')));
  for(const [field,label,description] of [
    ['jev_search',['Jev na busca','Jev in search'],['Desempata responsabilidades ambíguas após a busca nativa.','Resolves ambiguous responsibilities after native search.']],
    ['jev_wave_planning',['Jev no planejamento','Jev in planning'],['Classifica opções no planejamento das ondas quando necessário.','Classifies wave planning options when needed.']],
    ['jev_context',['Jev no contexto','Jev in context'],['Escolhe contexto relevante quando os critérios nativos são insuficientes.','Selects relevant context when native criteria are insufficient.']],
  ])result.push(toggle(field,label,description));
  if(!values.jev)result.push(note(t('As finalidades ficam guardadas, mas não chamam Jev enquanto a opção geral estiver desligada.','Purpose choices are retained, but cannot call Jev while the master switch is off.')));
  }
  const models=[...new Set([state.values.agent_model,'sonnet','opus','haiku'])].filter(Boolean);
  if(group==='Agentes')result.push(section(t('Agentes das ondas','Wave agents'),[
    picker('setting-agent-model',t('Modelo','Model'),models.map(value=>({value,label:value})),values.agent_model,value=>{if(!state.busy)actions.editSetting('agent_model',value);}),
    picker('setting-agent-effort',t('Esforço','Effort'),['low','medium','high','xhigh','max'].map(value=>({value,label:value})),values.agent_effort,value=>{if(!state.busy)actions.editSetting('agent_effort',value);}),
    note(t('Aplica-se aos próximos agentes. Os agentes em execução mantêm o modelo atual.','Applies to new agents. Running agents keep their current model.')),
    picker('setting-session-claude',t('Análise pontual · Claude','Targeted analysis · Claude'),[...new Set([values.session_claude_model||'haiku','haiku','inherit'])].map(value=>({value,label:value})),values.session_claude_model||'haiku',value=>{if(!state.busy)actions.editSetting('session_claude_model',value);}),
    picker('setting-session-codex',t('Análise pontual · Codex','Targeted analysis · Codex'),[...new Set([values.session_codex_model||'gpt-6-luna','gpt-6-luna','inherit'])].map(value=>({value,label:value})),values.session_codex_model||'gpt-6-luna',value=>{if(!state.busy)actions.editSetting('session_codex_model',value);}),
    note(t('Haiku/Luna: versão mais recente disponível na sessão, esforço médio e até 100 mil tokens de entrada total por análise isolada. Só quando faltar interpretação; reutilize enquanto as fontes e regras estiverem iguais. Preferência não comprova a execução.','Haiku/Luna: latest version available in the session, medium effort and up to 100K full input tokens per isolated analysis. Only for missing interpretation; reuse while sources and rules match. Preference is not proof of execution.')),

  ]));
  if(group==='Busca')result.push(section(t('Índice do projeto','Project index'),[
    note(state.index_present?t('Índice disponível. Atualize após habilitar o modelo para preparar os vetores.','Index available. Update after enabling the model to prepare vectors.'):
      t('Índice ainda não gerado. Salve suas escolhas e atualize o índice.','No index yet. Save your choices and update the index.')),
    button('settings-scan',t('Atualizar índice','Update index'),actions.scanSettings,{disabled:state.busy || dirty}),
    note(t('A atualização usa o binário local; não consulta Jev.','The update uses the local binary; it does not call Jev.')),
  ]));
  if(dirty)result.push(note(t(`${Object.keys(state.changes).length} alterações pendentes em todos os grupos.`,`${Object.keys(state.changes).length} pending changes across groups.`)));
  result.push(row([
    button('settings-save',t(state.busy?'Aplicando…':'Salvar alterações',state.busy?'Applying…':'Save changes'),actions.saveSettings,{disabled:state.busy || !dirty}),
    button('settings-reload',t(dirty?'Descartar e recarregar':'Recarregar configurações',dirty?'Discard and reload':'Reload settings'),actions.reloadSettings,{disabled:state.busy}),
  ]));
  result.push(note(t('Nenhuma chave é exibida. O Claude pode pedir confirmação para recarregar os hooks alterados.','No key is displayed. Claude may ask for confirmation to reload modified hooks.')));
  return section(t('Configurações','Settings'),result);
}
