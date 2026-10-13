import {profileNames,usageFigures,countdown,limitLabel} from './panel-runtime.js';

export function profileControls(data,view,actions,h) {
  const {t,text,note,row,section,button,picker,Input}=h,preferences=data.preferences;
  if(!preferences?.ok)return [];
  const profile=preferences.profiles.find(item=>item.id===preferences.selected);
  const rows=[row([picker('panel-profile',t('Visão','View'),preferences.profiles.map(item=>({value:item.id,
    label:profileNames[item.id]?t(...profileNames[item.id]):item.name})),preferences.selected,value=>actions.profile({selected:value})),
    button('profile-customize',t('Personalizar','Customize'),()=>actions.change({showProfileEditor:!view.showProfileEditor}))])];
  if(view.showProfileEditor && Input && profile)rows.push(section(t('Perfil de visualização','Presentation profile'),[
    note(t('Estas escolhas organizam o painel; não alteram agentes, modelos ou permissões.','These choices organize the pane; they do not change agents, models or permissions.')),
    Input({key:'profile-name',label:t('Nome','Name'),value:view.profileName ?? profile.name,onInput:value=>actions.change({profileName:value}),onSubmit:value=>actions.change({profileName:value})}),
    row(Object.entries(h.tabs).map(([tab,label])=>button('profile-toggle-'+tab,label,()=>{
      const tabs=view.profileTabs || profile.tabs;
      const next=tabs.includes(tab)?tabs.filter(item=>item!==tab):[...tabs,tab];
      if(!next.length)return;
      const home=view.profileTab || profile.tab;
      actions.change({profileTabs:next,profileTab:next.includes(home)?home:next[0]});
    },{active:(view.profileTabs || profile.tabs).includes(tab),disabled:(view.profileTabs || profile.tabs).length===1 && (view.profileTabs || profile.tabs).includes(tab)}))),
    picker('profile-home',t('Abrir na seção','Open section'),(view.profileTabs || profile.tabs).map(value=>({value,label:h.tabs[value]})),view.profileTab || profile.tab,value=>actions.change({profileTab:value})),
    row([button('profile-save',t('Salvar perfil','Save profile'),()=>actions.profile({profile:{...profile,name:view.profileName ?? profile.name,
      tab:view.profileTab || profile.tab,tabs:view.profileTabs || profile.tabs}}),{disabled:!(view.profileName ?? profile.name).trim()}),
      button('profile-copy',t('Salvar como novo','Save as new'),()=>actions.profile({profile:{id:'custom-'+Date.now().toString(36),
        name:view.profileName ?? profile.name,tab:view.profileTab || profile.tab,tabs:view.profileTabs || profile.tabs}}),{disabled:!(view.profileName ?? profile.name).trim()}),
      button('profile-delete',t('Remover perfil','Remove profile'),()=>actions.profile({delete:profile.id,selected:preferences.profiles.find(item=>item.id!==profile.id)?.id}),{disabled:preferences.profiles.length<2})]),
  ]));
  return rows;
}

export function ideasPanel(data,view,actions,h,busy) {
  const {t,text,note,row,section,button,picker,Input}=h,ledger=data.ideas;
  if(!ledger?.ok)return section(t('Ideias','Ideas'),[note(t('Não foi possível ler as ideias. Atualize para tentar novamente.','Could not read ideas. Refresh to retry.'))]);
  const query=(view.ideaQuery || '').toLocaleLowerCase();
  const items=ledger.items.filter(item=>(view.showArchivedIdeas || item.status==='open') &&
    (!query || `${item.id} ${item.title} ${item.detail}`.toLocaleLowerCase().includes(query)));
  const editor=Input?[
    Input({key:'idea-title',label:t('Título da ideia','Idea title'),value:view.ideaTitle || '',onInput:value=>actions.change({ideaTitle:value}),onSubmit:value=>actions.change({ideaTitle:value})}),
    Input({key:'idea-detail',label:t('Descrição','Description'),value:view.ideaDetail || '',onInput:value=>actions.change({ideaDetail:value}),onSubmit:value=>actions.change({ideaDetail:value})}),
    button('idea-save',t(view.selectedIdea?'Salvar alteração':'Guardar ideia',view.selectedIdea?'Save changes':'Save idea'),()=>actions.idea({
      action:view.selectedIdea?'edit':'add',id:view.selectedIdea,title:view.ideaTitle,detail:view.ideaDetail}),{disabled:busy || !view.ideaTitle?.trim() || !view.ideaDetail?.trim()}),
  ]:[note(t('Use mustard-rt run ideas para registrar uma ideia neste host.','Use mustard-rt run ideas to record an idea on this host.'))];
  const rows=[note(t('Sugestões ficam fora da execução até você decidir iniciar um levantamento. Pendências existentes aparecem na mesma lista.','Suggestions stay outside execution until you choose to start a survey. Existing pending work appears in the same list.')),
    ...editor,row([button('ideas-archived',t('Mostrar arquivadas e concluídas','Show archived and completed'),()=>actions.change({showArchivedIdeas:!view.showArchivedIdeas}),{active:view.showArchivedIdeas})])];
  if(Input)rows.push(Input({key:'idea-search',label:t('Buscar ideia ou pendência','Search idea or pending work'),value:view.ideaQuery || '',onInput:value=>actions.change({ideaQuery:value}),onSubmit:value=>actions.change({ideaQuery:value})}));
  for(const item of items.slice(0,50)) {
    const children=[text(`${item.title} · ${item.id}`,{bold:true}),text(item.detail),note(item.idea?t('Ideia · ainda não aprovada','Idea · not approved yet'):t('Pendência registrada','Recorded pending work'))];
    if(item.became)children.push(note(`${t('Vinculada à spec','Linked to spec')}: ${item.became}`));
    if(item.status==='open') {
      children.push(row([
        ...(item.idea?[button('idea-edit-'+item.id,t('Editar','Edit'),()=>actions.change({selectedIdea:item.id,ideaTitle:item.title,ideaDetail:item.detail}))]:[]),
        button('idea-start-'+item.id,t('Preparar levantamento','Prepare survey'),()=>actions.startIdea(item)),
        button('idea-complete-'+item.id,t('Marcar como tratada','Mark as addressed'),()=>actions.idea({action:'complete',id:item.id,reason:t('Marcada como tratada no painel.','Marked as addressed in the pane.')}),{disabled:busy}),
        button('idea-archive-'+item.id,t('Arquivar','Archive'),()=>actions.idea({action:'archive',id:item.id,reason:t('Arquivada pelo usuário no painel.','Archived by the user in the pane.')}),{disabled:busy}),
      ]));
      if(data.specs?.length)children.push(row([
        picker('idea-spec-'+item.id,t('Associar à spec','Associate with spec'),data.specs.map(spec=>({value:spec.name,label:spec.name})),view.ideaSpec || data.selected_spec,value=>actions.change({ideaSpec:value})),
        button('idea-link-'+item.id,t('Já atendida por esta spec','Already covered by this spec'),()=>actions.idea({action:'link',id:item.id,spec:view.ideaSpec || data.selected_spec}),{disabled:busy || !(view.ideaSpec || data.selected_spec)}),
      ]));
      const terms=item.title.toLocaleLowerCase().split(/\W+/).filter(term=>term.length>3);
      const matching=data.specs.filter(spec=>terms.some(term=>`${spec.name} ${spec.goal || ''}`.toLocaleLowerCase().includes(term)));
      if(matching.length)children.push(note(`${t('Specs com texto relacionado · confira antes de vincular','Specs with related text · verify before linking')}: ${matching.map(spec=>spec.name).join(', ')}`));
    }else children.push(row([note(`${item.status}: ${item.reason || ''}`),button('idea-reopen-'+item.id,t('Reabrir','Reopen'),()=>actions.idea({action:'reopen',id:item.id}),{disabled:busy})]));
    rows.push(section(item.idea?t('Ideia','Idea'):t('Pendência','Pending work'),children));
  }
  if(items.length>50)rows.push(note(t('Refine a busca para ver os demais registros.','Refine the search to see remaining records.')));
  if(!items.length)rows.push(note(t('Nenhuma ideia ou pendência nesta seleção.','No ideas or pending work in this selection.')));
  return section(t('Ideias do projeto','Project ideas'),rows);
}

export function liveAgents(data,spec,h) {
  const {t,text,note,section,fmt,date}=h,all=Object.values(data.live_usage?.agents || {});
  const agents=all.filter(agent=>!agent.spec || agent.spec===spec?.name);
  if(!agents.length)return [];
  return [section(t('Agentes observados nesta sessão','Agents observed in this session'),agents.slice(-24).map(agent=>{
    const labels={running:t('Em execução','Running'),returned:t('Retornou · consulte entrega e revisão','Returned · check delivery and review'),interrupted:t('Interrompido','Interrupted')};
    const figures=usageFigures(agent),duration=Date.parse(agent.ended_at || data.at)-Date.parse(agent.started_at);
    return section(`${agent.wave?`${t('Onda','Wave')} ${agent.wave} · `:''}${agent.agent_type || agent.id}`,[
      text(labels[agent.status] || t('Estado não informado','State unavailable'),{bold:true}),
      note(`${agent.model || t('Modelo não observado','Model not observed')}${agent.effort?' · '+agent.effort:''}${Number.isFinite(duration)?' · '+Math.max(0,Math.round(duration/60000))+' min':''}`),
      ...(!agent.wave?[note(t('Sem vínculo comprovado com uma onda; consumo não atribuído à spec.','No verified wave binding; usage is not attributed to this spec.'))]:[]),
      ...(agent.requests?[text(`${t('Entrada / saída','Input / output')}: ${fmt(figures.input)} / ${fmt(figures.output)} · ${t('cache lido / gravado','cache read / written')}: ${fmt(figures.read)} / ${fmt(figures.write)}`),
        note(`${t('Contexto da última solicitação','Last request context')}: ${fmt(agent.context_tokens)} tokens · ${fmt(agent.requests)} ${t('solicitações','requests')}`)]:[]),
      note(`${t('Última observação','Last observation')}: ${date(agent.last_step_at || agent.ended_at || agent.started_at)}`),
    ]);
  }))];
}

export function sessionFigures(data,usage,h) {
  const {t,text,note,section,fmt,date}=h,rows=[],limits=usage?.rateLimits || data.live_usage?.rateLimits || [];
  for(const limit of limits)rows.push(text(`${limitLabel(limit.kind,t('pt','en')==='en')}: ${fmt(limit.percentUsed)}%${limit.resetsAt?' · '+t('renova em','resets in')+' '+countdown(limit.resetsAt)+' · '+date(limit.resetsAt):''}`));
  if(!limits.length)rows.push(note(t('O host ainda não informou as cotas da assinatura.','The host has not reported subscription limits yet.')));
  const figures=usageFigures(data.live_usage?.main);
  if(figures)rows.push(text(`${t('Entrada / saída observadas','Observed input / output')}: ${fmt(figures.input)} / ${fmt(figures.output)}`),
    text(`${t('Cache lido / gravado','Cache read / written')}: ${fmt(figures.read)} / ${fmt(figures.write)}`),
    ...(figures.cachePercent!=null?[text(`${t('Entrada atendida por cache','Input served from cache')}: ${fmt(Math.round(figures.cachePercent))}%`)]:[]),
    note(t('Medições nativas desta sessão; não somar novamente aos totais dos históricos. Não estimamos a validade futura do cache.','Native measurements from this session; do not add them again to historical totals. Future cache validity is not estimated.')));
  return section(t('Assinatura e cache','Subscription and cache'),rows);
}

export function diagnosticsPanel(data,view,actions,h) {
  const {t,text,note,row,section,button,picker,Input,date}=h,diagnostics=data.diagnostics,health=data.integration_health || diagnostics?.health;
  const events=diagnostics?.events || [];
  const components=[...new Set(events.map(event=>event.component))];
  const filter=(view.diagnosticQuery || '').toLocaleLowerCase();
  const shown=events.filter(event=>(!view.diagnosticComponent || event.component===view.diagnosticComponent) && (!filter || JSON.stringify(event).toLocaleLowerCase().includes(filter)));
  const rows=[note(t('Configuração válida não comprova uso. Eventos indicam o que foi observado.','Valid configuration does not prove usage. Events show what was observed.'))];
  if(health)rows.push(text(`Mustard ${health.running_version || data.project.version} · ${t('projeto','project')} ${health.project_version || '—'}`),
    ...((health.issues || []).map(issue=>note(issue))),
    text(`RTK: ${health.rtk?.enabled?t('habilitado','enabled'):t('desligado','off')} · ${health.rtk?.executable_available?t('executável disponível','executable available'):t('executável não confirmado','executable unconfirmed')}`),
    text(`Jev: ${health.jev?.credential_available?t('credencial disponível','credential available'):t('credencial indisponível','credential unavailable')}`),
    text(`${t('Vetores indexados','Indexed vectors')}: ${health.vectors?.indexed_chunks ?? t('não medido','not measured')}`),
    ...((health.hosts?.observations || []).map(host=>note(`${host.host} · ${host.last_event} · ${date(host.at)} · ${host.tool_failures || 0} ${t('falhas','failures')}`))));
  if(Input)rows.push(Input({key:'diagnostic-search',label:t('Filtrar por spec, onda, sessão ou motivo','Filter by spec, wave, session or reason'),value:view.diagnosticQuery || '',onInput:value=>actions.change({diagnosticQuery:value}),onSubmit:value=>actions.change({diagnosticQuery:value})}));
  rows.push(picker('diagnostic-component',t('Componente','Component'),[{value:'',label:t('Todos','All')},...components.map(value=>({value,label:value}))],view.diagnosticComponent || '',value=>actions.change({diagnosticComponent:value})),
    ...shown.slice(-40).reverse().map(event=>section(`${event.component} · ${event.operation || ''}`,[text(event.reason),note(`${date(event.at)}${event.count?' · '+event.count+'×':''}`),
      ...(event.spec?[note(`${event.spec}${event.wave?' · '+event.wave:''}`)]:[])])),
    ...(!shown.length?[note(t('Nenhum erro registrado nesta seleção. Isso não comprova cobertura de todos os componentes.','No recorded error in this selection. This does not prove coverage of all components.'))]:[]),
    row([button('diagnostic-export',t('Exportar diagnóstico local','Export local diagnostic'),actions.exportDiagnostics),
      button('impact-branches',t('Conferir branches mescladas','Inspect merged branches'),()=>actions.preview({action:'branches'}))]));
  return section(t('Diagnóstico','Diagnostics'),rows);
}

export function impactPanel(impact,actions,h) {
  if(!impact)return null;
  const {t,text,note,row,section,button}=h;
  const rows=[note(t('A prévia não executa a operação. As proteções existentes continuam valendo.','The preview does not execute the operation. Existing protections still apply.'))];
  if(impact.changed)rows.push(text(t('O estado mudou. Confira esta prévia atualizada antes de publicar.','State changed. Review this updated preview before publishing.')));
  if(!impact.ok)rows.push(text(t('Não foi possível calcular a prévia.','Could not calculate the preview.')));
  for(const effect of impact.effects || []) {
    if(effect.kind==='publication')rows.push(text(t('Conteúdo a publicar','Content to publish'),{bold:true}),
      text(effect.scope==='spec'?`${t('Spec','Spec')}: ${effect.spec}`:effect.scope==='report'?`${t('Relatório','Report')}: ${effect.document || ''}`:t('Visão geral do projeto','Project overview')),
      note(t('Será gerada uma página estática deste momento, com os dados permitidos para publicação.','A static page of this moment will be generated using the data allowed for publication.')),
      note(`${t('Destino','Destination')}: ${effect.destination || t('provedor configurado','configured provider')}`),
      note(effect.consumption_included?t('Inclui consumo.','Includes consumption.'):t('Consumo não incluído.','Consumption excluded.')));
    else if(effect.kind==='merged-branches')rows.push(text(t('Branches mescladas encontradas','Merged branches found'),{bold:true}),text(effect.names?.join(' · ') || t('Nenhuma','None')),
      note(t('Esta consulta não exclui branches. Confira trabalho local e uso por outras sessões antes de limpar.','This query does not delete branches. Check local work and other sessions before cleanup.')));
    else if(effect.kind==='wave-attempt')rows.push(text(`${t('Onda','Wave')} ${effect.wave} · ${effect.spec}`,{bold:true}),
      note(effect.return_received?t('Existe retorno registrado; confira antes de descartar.','A return is recorded; review before discarding.'):t('Sem retorno registrado.','No recorded return.')));
    else rows.push(text(effect.path || effect.operation || effect.kind));
  }
  if(impact.active_waves?.length)rows.push(note(`${t('Ondas em andamento','Waves in progress')}: ${impact.active_waves.map(item=>`${item.spec} / ${item.wave}`).join(' · ')}`));
  if(impact.working_changes?.length)rows.push(text(t('Alterações locais presentes','Local changes present'),{bold:true}),...impact.working_changes.slice(0,20).map(change=>note(change)));
  if(impact.unknown?.length)rows.push(note(t('A prévia é parcial. Acesso ao provedor, condições de descarte ou efeitos do shell podem exigir conferência adicional.','The preview is partial. Provider access, discard conditions or shell effects may need additional verification.')));
  rows.push(row([...(impact.canExecute?[button('impact-confirm',t('Confirmar publicação','Confirm publication'),actions.confirmImpact)]:[]),
    button('impact-cancel',t('Fechar prévia','Close preview'),actions.cancelImpact)]));
  return section(t('Prévia de impacto','Impact preview'),rows);
}

export function shellImpactPane(elements,impact,close) {
  const {Box,Text,Button,Code}=elements;
  return Box({flexDirection:'column',padding:1,rowGap:1,backgroundColor:'black',children:[
    Text({children:'MUSTARD · Prévia de impacto',bold:true,color:'yellow'}),
    Text({children:'Informações calculadas sem executar o comando. Autorizações e bloqueios existentes continuam valendo.',wrap:'wrap',color:'white'}),
    ...(impact?.toolState?[Text({children:`Estado: ${impact.toolState}`,color:'yellow'})]:[]),
    ...(impact?.effects || []).slice(0,20).map(effect=>Text({children:effect.kind==='remove-path'?`Remoção: ${effect.path} · ${effect.directory?'diretório':'arquivo'} · recuperação não garantida`:
      effect.kind==='git-operation'?`Git: ${effect.operation} · confira alterações e revisões afetadas`:JSON.stringify(effect),wrap:'wrap',color:'white'})),
    Text({children:`Alterações locais: ${impact?.working_changes?.length || 0} · ondas ativas: ${impact?.active_waves?.length || 0}`,color:'white'}),
    ...(impact?.unknown?.length?[Text({children:'A prévia é parcial. Expansões do shell, revisões e estado remoto podem exigir conferência adicional.',color:'yellow',wrap:'wrap'})]:[]),
    ...(impact?.working_changes?.length && Code?[Code({source:impact.working_changes.slice(0,40).join('\n'),language:'text'})]:[]),
    Button({key:'close-impact-pane',label:'Fechar prévia',onPress:close}),
  ]});
}
