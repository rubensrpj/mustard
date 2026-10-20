// The settings screen: the safe projection of `run settings --full`, edited
// locally and written only by an explicit save. The Cloudflare token is
// write-only: typed here, sent in that one save, never drawn back.
const errors={
  'settings-unreadable':['Não foi possível ler o mustard.json. Corrija o arquivo antes de editar.','Cannot read mustard.json. Repair the file before editing.'],
  'settings-conflict':['A configuração mudou fora do painel. Descarte e refaça as escolhas.','Settings changed outside the panel. Discard and redo your choices.'],
  'settings-local-invalid':['As configurações locais têm formato inválido. Corrija-as antes de mudar o RTK.','Local settings are invalid. Repair them before changing RTK.'],
  'settings-apply-incomplete':['As escolhas foram gravadas, mas alguns arquivos não foram atualizados. Rode mustard-rt run upsert.','Choices were saved but some files were not updated. Run mustard-rt run upsert.'],
  'settings-busy':['Outra gravação está em andamento. Salve de novo.','Another write is in progress. Save again.'],
  'settings-token-exposed':['O mustard.json está no git: o token não foi guardado.','mustard.json is tracked by git: the token was not saved.'],
  'settings-invalid-fields':['Algum valor não foi aceito. Confira a conta e o projeto do Cloudflare.','A value was refused. Check the Cloudflare account and project.'],
};
const purposes=[
  ['jev_wave_planning',['Montar as ondas','Build the waves']],
  ['jev_context',['Escolher o contexto do pedido','Pick the request context']],
  ['jev_search',['Busca de código com dúvida','Ambiguous code search']],
  ['jev_spec_search',['Busca na spec','Spec search']],
];

export function settingsScreen(state,actions,{t,text,label,block,press,join,row,column,Select,Input}) {
  const head=[text(t('Configurações','Settings'),{bold:true}),label(t('salvo no mustard.json','saved in mustard.json'))];
  if(state?.message)head.push(text(t(...state.message),{color:'success'}));
  if(state?.error)head.push(text(t(...(errors[state.error] || ['A operação não terminou. Suas edições ficaram.','The operation did not finish. Your edits were kept.'])),{color:'error'}));
  if(!state?.loaded) {
    head.push(label(state?.busy?t('Carregando…','Loading…'):t('Configurações indisponíveis.','Settings unavailable.')));
    if(!state?.busy)head.push(press('settings-reload',t('Recarregar','Reload'),actions.reloadSettings));
    return block(null,head);
  }
  const values={...state.values,...state.changes},dirty=Object.keys(state.changes).length>0,busy=state.busy;
  const edit=(field,value)=>{if(!busy)actions.editSetting(field,value);};
  const toggle=(field,caption)=>press(`setting-${field}`,join(caption,values[field]?t('ligado','on'):t('desligado','off')),()=>edit(field,!values[field]));
  const choice=(field,caption,options)=>Select?Select({key:`setting-${field}`,label:caption,value:values[field],
    options:[...new Set([values[field],...options])].filter(Boolean).map(value=>({value,label:value})),onSelect:value=>edit(field,value)})
    :text(join(caption,values[field]));
  const models=['opus','sonnet','haiku'],efforts=['low','medium','high','xhigh','max'];
  const waves=Number(values.max_compiling_waves) || 1;
  const blocks=[block(null,head)];
  blocks.push(block(t('Agentes','Agents'),[
    label(t('Agente de onda','Wave agent')),choice('agent_model',t('Modelo','Model'),models),choice('agent_effort',t('Esforço','Effort'),efforts),
    label(t('Revisor final','Final reviewer')),choice('review_model',t('Modelo','Model'),models),choice('review_effort',t('Esforço','Effort'),efforts),
    label(t('Ondas ao mesmo tempo · quantas compilam juntas','Waves at once · how many compile together')),
    row([press('setting-waves-less','−',()=>edit('max_compiling_waves',Math.max(1,waves-1))),text(String(waves),{bold:true}),
      press('setting-waves-more','+',()=>edit('max_compiling_waves',Math.min(16,waves+1)))]),
    label(t('Vale para os próximos agentes; os que já trabalham seguem como estão.','Applies to the next agents; running ones keep going as they are.'))]));
  const jev=[label(t('cada pergunta é paga','each question is paid'))];
  if(!values.jev)jev.push(text(t('O Jev está desligado no projeto: as finalidades ficam guardadas e não chamam.','Jev is off in this project: purposes are kept and make no calls.'),{color:'warning'}),
    toggle('jev',t('Usar o Jev neste projeto','Use Jev in this project')));
  if(state.jev && state.jev.usable_key===false)jev.push(text(t('Sem chave: defina TYPESAFE_API_KEY no ambiente privado.','No key: set TYPESAFE_API_KEY in the private environment.'),{color:'warning'}));
  jev.push(...purposes.map(([field,caption])=>toggle(field,t(...caption))));
  // Sem escolha gravada, a busca na spec fica desligada até ser ligada aqui.
  if(state.jev?.purposes?.['spec-search']?.chosen===false && !('jev_spec_search' in state.changes))
    jev.push(label(t('Busca na spec desligada: nenhuma escolha gravada. Ligue acima para usar.','Spec search is off: no choice saved. Turn it on above to use it.')));
  blocks.push(block('Jev',jev));
  blocks.push(block(t('Ferramentas','Tools'),[toggle('vectors',t('Modelo local da busca','Local search model')),
    toggle('rtk',t('RTK · enxuga a saída dos comandos','RTK · trims command output'))]));
  const publication=state.publication || {};
  const field=(name,caption,placeholder)=>Input?Input({key:`setting-${name}`,label:caption,placeholder,value:values[name] ?? '',
    onInput:value=>edit(name,value.trim()?value.trim():null),onSubmit:value=>edit(name,value.trim()?value.trim():null)}):text(join(caption,values[name] ?? '—'));
  const cloudflare=[field('cloudflare_account',t('Conta','Account'),t('ID da conta','Account ID')),field('cloudflare_project',t('Projeto no Cloudflare','Cloudflare project'),t('nome do projeto','project name'))];
  if(publication.mustard_json_out_of_git===false)cloudflare.push(text(t('O mustard.json não está fora do git: o token não pode ser guardado.','mustard.json is not kept out of git: the token cannot be saved.'),{color:'warning'}));
  else {
    // The field never holds the token: Enter moves what was typed to the
    // pending change, and the field is drawn empty again.
    if(Input)cloudflare.push(Input({key:'setting-cloudflare_token',label:t('Token','Token'),value:'',submitLabel:t('usar','use'),
      placeholder:publication.token_saved?t('•••••••• guardado · digite para trocar','•••••••• saved · type to replace'):t('cole o token e tecle Enter','paste the token and press Enter'),
      onSubmit:value=>edit('cloudflare_token',value.trim()?value.trim():null)}));
    cloudflare.push(label(state.changes.cloudflare_token?t('token novo •••••••• pronto para salvar','new token •••••••• ready to save')
      :publication.token_saved?t('token guardado no mustard.json, que está fora do git','token saved in mustard.json, which is kept out of git')
      :t('guardado no mustard.json, que está fora do git','saved in mustard.json, which is kept out of git')));
  }
  blocks.push(block(t('Publicação · Cloudflare Pages','Publication · Cloudflare Pages'),cloudflare));
  const count=Object.keys(state.changes).length;
  blocks.push(block(null,[...(dirty?[label(t(`${count} ${count===1?'alteração pendente':'alterações pendentes'}`,`${count} pending ${count===1?'change':'changes'}`))]:[]),
    row([busy || !dirty?label(busy?t('Gravando…','Saving…'):t('Nada a salvar','Nothing to save')):press('settings-save',t('Salvar','Save'),actions.saveSettings,{variant:'primary'}),
      ...(busy?[]:[press('settings-reload',dirty?t('Descartar','Discard'):t('Recarregar','Reload'),actions.reloadSettings)])])]));
  return column(blocks);
}
