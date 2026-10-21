// The settings screen: the safe projection of `run settings --full`, edited
// locally and written only by an explicit save. The Cloudflare token is
// write-only: typed here, sent in that one save, never drawn back.
// As duas superfícies seguem a tela Configurações do layout: cada grupo num
// cartão, o nome à esquerda e o controle à direita; o título fica no cabeçalho
// do painel. No terminal salvar e descartar levam as teclas s e d.
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

export function settingsScreen(state,actions,{t,terminal,paint,space,apart,text,soft,label,join,row,column,fill,section,rows,entry,block,action,Box,Select,Input,Button,Text,hot}) {
  const notes=[];
  if(state?.message)notes.push(text(t(...state.message),{color:paint.success}));
  if(state?.error)notes.push(text(t(...(errors[state.error] || ['A operação não terminou. Suas edições ficaram.','The operation did not finish. Your edits were kept.'])),{color:paint.warning}));
  const top=notes.length?[column(notes)]:[];
  if(!state?.loaded) {
    const waiting=[label(state?.busy?t('Carregando…','Loading…'):t('Configurações indisponíveis.','Settings unavailable.')),
      ...(state?.busy?[]:[action('settings-reload',t('Recarregar','Reload'),actions.reloadSettings,'outline')])];
    return column([...top,...waiting],{rowGap:space(10)});
  }
  const values={...state.values,...state.changes},dirty=Object.keys(state.changes).length>0,busy=state.busy;
  const edit=(field,value)=>{if(!busy)actions.editSetting(field,value);};
  // O interruptor do layout: ligado, o trilho mostarda com o botão escuro à
  // direita; desligado, o trilho apagado com o botão à esquerda. O botão do
  // aplicativo só aceita texto dentro dele, e o interruptor é esse texto.
  const gap=terminal?'  ':'  ';
  const knob=on=>Text({children:on?`${gap}●`:`●${gap}`,color:on?paint.page:paint.muted,backgroundColor:on?paint.mustard:paint.line});
  // Sem escolha gravada (`unset`), a chave está desligada, diga o valor o que disser.
  const toggle=(field,caption,hint,note,unset=false)=>{const on=!unset && Boolean(values[field]),now=on?t('ligado','on'):t('desligado','off'),key=`setting-${field}`,flip=()=>edit(field,!on);
    return [entry([row([column([text(caption),...(hint?[label(hint)]:[]),...(note?[label(note)]:[])],{flexGrow:1,flexShrink:1}),
      Button({key,label:join(hint?`${caption} · ${hint}`:caption,now),plain:true,onPress:flip,children:[knob(on)]})],
      {flexWrap:'nowrap',alignItems:'center',columnGap:space(12)})],{paddingY:space(12)})];};
  // As opções na ordem delas, do menor ao maior; um valor gravado fora da lista
  // vem por último. No terminal o seletor fica no quadro escuro do layout.
  const choice=(field,caption,options,shown=value=>value)=>{const picker=Select?Select({key:`setting-${field}`,value:values[field],
    options:[...new Set([...options,values[field]])].filter(Boolean).map(value=>({value,label:shown(value)})),onSelect:value=>edit(field,value)})
    :text(join(caption,values[field]));
    return terminal?Box({backgroundColor:paint.box,paddingX:space(12),children:[picker]}):picker;};
  const models=['opus','sonnet','haiku'],efforts=['low','medium','high','xhigh','max'];
  const named=value=>value.charAt(0).toUpperCase()+value.slice(1);
  const waves=Number(values.max_compiling_waves) || 1;
  // O contador: menos e mais no quadro escuro, o número no texto de destaque.
  const stepper=row([action('setting-waves-less','−',()=>edit('max_compiling_waves',Math.max(1,waves-1)),'box'),
    text(String(waves),{size:'body',weight:'bold',tabular:true,color:paint.accent}),
    action('setting-waves-more','+',()=>edit('max_compiling_waves',Math.min(16,waves+1)),'box')],{flexWrap:'nowrap',alignItems:'center',columnGap:space(10)});
  const blocks=[...top];
  const agent=(caption,model,effort)=>entry([row([fill(text(caption)),choice(model,t('Modelo','Model'),models,named),choice(effort,t('Esforço','Effort'),efforts)],
    {flexWrap:'nowrap',alignItems:'center',columnGap:space(10)})],{paddingY:space(12)});
  blocks.push(section(t('Agentes','Agents'),[rows([agent(t('Agente de onda','Wave agent'),'agent_model','agent_effort'),agent(t('Revisor final','Final reviewer'),'review_model','review_effort'),
    entry([row([column([text(t('Ondas ao mesmo tempo','Waves at once')),label(t('quantas compilam juntas','how many compile together'))],{flexGrow:1,flexShrink:1}),stepper],
      {flexWrap:'nowrap',alignItems:'center',columnGap:space(12)})],{paddingY:space(12)})])]));
  const jev=[];
  if(!values.jev)jev.push(entry([text(t('O Jev está desligado no projeto: as finalidades ficam guardadas e não chamam.','Jev is off in this project: purposes are kept and make no calls.'),{size:'footnote',color:paint.warning})]),
    ...toggle('jev',t('Usar o Jev neste projeto','Use Jev in this project')));
  if(state.jev && state.jev.usable_key===false)jev.push(entry([text(t('Sem chave: defina TYPESAFE_API_KEY no ambiente privado.','No key: set TYPESAFE_API_KEY in the private environment.'),{size:'footnote',color:paint.warning})]));
  // Sem escolha gravada, a busca na spec fica desligada até ser ligada aqui: a
  // chave diz desligado e a nota diz só por quê, sem repetir o estado.
  const unchosen=state.jev?.purposes?.['spec-search']?.chosen===false && !('jev_spec_search' in state.changes);
  jev.push(...purposes.flatMap(([field,caption])=>toggle(field,t(...caption),undefined,field==='jev_spec_search' && unchosen
    ?t('nenhuma escolha gravada','no choice saved'):undefined,field==='jev_spec_search' && unchosen)));
  blocks.push(section('Jev',[rows(jev)],label(t('cada pergunta é paga','each question is paid'))));
  blocks.push(section(t('Ferramentas','Tools'),[rows([...toggle('vectors',t('Modelo local da busca','Local search model')),...toggle('rtk','RTK',t('enxuga a saída dos comandos','trims command output'))])]));
  const publication=state.publication || {};
  // Um campo do layout: o rótulo pequeno em cima e o campo embaixo; no
  // terminal o campo fica no fundo escuro da página, dentro do cartão.
  const framed=field=>terminal?Box({backgroundColor:paint.page,paddingX:space(12),children:[field]}):field;
  const field=(name,caption,placeholder)=>column([soft(caption),Input?framed(Input({key:`setting-${name}`,placeholder,value:values[name] ?? '',
    onInput:value=>edit(name,value.trim()?value.trim():null),onSubmit:value=>edit(name,value.trim()?value.trim():null)})):text(values[name] ?? '—')],{rowGap:space(5)});
  const cloudflare=[field('cloudflare_account',t('Conta','Account'),t('ID da conta','Account ID')),field('cloudflare_project',t('Projeto no Cloudflare','Cloudflare project'),t('nome do projeto','project name'))];
  if(publication.mustard_json_out_of_git===false)cloudflare.push(text(t('O mustard.json não está fora do git: o token não pode ser guardado.','mustard.json is not kept out of git: the token cannot be saved.'),{color:paint.warning}));
  else {
    // The field never holds the token: Enter moves what was typed to the
    // pending change, and the field is drawn empty again. Guardado, ele se
    // lê como o campo de senha do layout, só os pontos.
    const token=Input?[framed(Input({key:'setting-cloudflare_token',value:'',submitLabel:t('usar','use'),
      placeholder:publication.token_saved?'••••••••••••••••':t('cole o token e tecle Enter','paste the token and press Enter'),
      onSubmit:value=>edit('cloudflare_token',value.trim()?value.trim():null)}))]:[];
    const kept=state.changes.cloudflare_token?t('token novo •••••••• pronto para salvar','new token •••••••• ready to save')
      :t('guardado no mustard.json, que está fora do git','saved in mustard.json, which is kept out of git');
    cloudflare.push(column([soft(t('Token','Token')),...token,label(kept)],{rowGap:space(5)}));
  }
  blocks.push(block(t('Publicação · Cloudflare Pages','Publication · Cloudflare Pages'),cloudflare,{paddingY:space(16),rowGap:apart(14)}));
  // Salvar e descartar só aparecem com uma escolha pendente; enquanto grava,
  // só o aviso; sem nada a salvar, nada.
  const count=Object.keys(state.changes).length;
  const pending=dirty?[label(t(`${count} ${count===1?'alteração pendente':'alterações pendentes'}`,`${count} pending ${count===1?'change':'changes'}`))]:[];
  const save=action('settings-save',t('Salvar','Save'),actions.saveSettings,'solid',hot('s'));
  const discard=action('settings-reload',t('Descartar','Discard'),actions.reloadSettings,'outline',hot('d'));
  if(busy || dirty)blocks.push(column([...pending,row([busy?label(t('Gravando…','Saving…')):save,...(busy?[]:[discard])],{alignItems:'center',columnGap:space(10)})],{rowGap:space(8)}));
  return column(blocks,{rowGap:space(22)});
}
