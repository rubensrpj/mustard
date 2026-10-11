import design from '../ui/theme.js';
import {definitionNames,definitionText,waveText,contentPage} from './panel-content.js';

export const initialPanelView = () => ({tab:'Projeto',definitionGroup:'context',definitionQuery:'',definitionPage:0,
  selectedDefinition:undefined,selectedWave:undefined,waveView:'Plano',contentOffset:0,technicalOffset:0,
  showTechnical:false,usageView:'Resumo',showPublications:false});

// The terminal's ANSI palette can quantize dark RGB surfaces into medium gray.
// Use real black there; desktop keeps the shared web palette. Borders and labels
// carry selection as well as color, including on terminals without truecolor.
export function panelPalette(surface) {
  return surface==='terminal' ? {...design.dark,paper:'black',surface:'black',sidebar:'black',inset:'black',
    ink:'whiteBright',muted:'white',line:'gray',accentText:'yellow',accentSoft:'black'} : design.dark;
}

export function renderPanel({elements,surface,columns,bodyRows,data,usage,problem,selected,view,publicationMessage,publishing,actions}) {
  const {Box,Text,Button,Select,Input,Link}=elements;
  const theme=panelPalette(surface), english=data?.project?.language==='en-US';
  const compact=bodyRows>0 && bodyRows<24;
  const t=(pt,en)=>english?en:pt;
  const clean=value=>String(value ?? '').replace(/[\u0000-\u0008\u000b-\u001f\u007f]/g,'');
  const text=(value,props={})=>Text({children:clean(value),color:theme.ink,wrap:'wrap',...props});
  const note=value=>text(value,{color:theme.muted});
  const fmt=value=>value==null?t('Não medido','Not measured'):Number(value).toLocaleString(english?'en-US':'pt-BR');
  const short=value=>value==null?t('Não medido','Not measured'):new Intl.NumberFormat(english?'en-US':'pt-BR',
    {notation:'compact',maximumFractionDigits:1}).format(value);
  const money=value=>value==null?t('Não medido','Not measured'):'$'+(value/1000000).toFixed(4);
  const date=value=>{
    const parsed=value ? new Date(value) : undefined;
    return !parsed || Number.isNaN(parsed.valueOf())?t('Sem medição','No measurement'):parsed.toLocaleString(english?'en-US':'pt-BR',{dateStyle:'short',timeStyle:'short'});
  };
  const states={survey:['Levantamento','Survey'],plan:['Planejamento','Planning'],planned:['Planejada','Planned'],
    approved:['Aprovada','Approved'],running:['Em execução','Running'],received:['Retorno recebido','Return received'],
    integrated:['Integrada','Integrated'],committed:['Comitada','Committed'],'repair-required':['Conserto necessário','Repair required'],
    qa:['Validação','Validation'],reviewing:['Revisão','Review'],review:['Revisão','Review'],pr:['PR aberto','PR open'],
    pr_open:['PR aberto','PR open'],closed:['Fechada','Closed'],delivered:['Entregue','Delivered'],
    discarded:['Descartada','Discarded'],discarded_auto:['Descartada','Discarded']};
  const state=value=>states[value]?t(...states[value]):clean(value || t('Não informado','Not provided'));
  const button=(key,label,onPress,{active=false,disabled=false,hotkey}={})=>Box({key:`control-${key}`,...(compact?{}:{borderStyle:'round'}),
    borderColor:active?theme.accentText:theme.line,paddingX:1,children:[disabled?note(label):Button({key,
      label:clean(active?`${label} ✓`:label),plain:true,onPress,...(hotkey?{hotkey}:{}),hover:{color:theme.accentText,bold:true}})]});
  const row=children=>Box({flexDirection:'row',flexWrap:'wrap',columnGap:1,children});
  const section=(title,children)=>Box({flexDirection:'column',...(compact?{}:{borderStyle:'round',borderColor:theme.line,paddingX:1}),
    children:[Box({marginBottom:compact?0:1,children:[text(title,{bold:true,color:theme.accentText})]}),...children]});
  const card=(label,value)=>Box({flexDirection:'column',borderStyle:'round',borderColor:theme.line,paddingX:1,flexGrow:1,
    children:[note(label),text(value,{bold:true})]});
  const content=(value,raw=false)=>raw ? text(value) : Box({flexDirection:'column',rowGap:1,children:clean(value).split('\n\n').map(paragraph=>{
    const label=paragraph.match(/^\*\*([^\n]+)\*\*\n([\s\S]*)$/);
    return label?Box({flexDirection:'column',children:[text(label[1],{bold:true,color:theme.accentText}),text(label[2])]}):text(paragraph);
  })});
  const pagination=(prefix,page,patch)=>page.pages>1?[row([
    button(`${prefix}-prev`,t('Anterior','Previous'),()=>actions.change(patch(page.page-1)),{disabled:page.page===0}),
    note(t(`Parte ${page.page+1} de ${page.pages}`,`Part ${page.page+1} of ${page.pages}`)),
    button(`${prefix}-next`,t('Próxima','Next'),()=>actions.change(patch(page.page+1)),{disabled:page.page+1>=page.pages}),
  ])]:[];
  const picker=(key,label,options,value,onSelect)=>surface!=='mobile' && Select
    ? Box({...(compact?{}:{borderStyle:'round',borderColor:theme.line,paddingX:1}),children:[Select({key,label,options,value,onSelect})]})
    : row(options.map(option=>button(`${key}-${option.value}`,option.label,()=>onSelect(option.value),{active:option.value===value})));
  const specs=data?.specs || [], spec=specs.find(item=>item.name===selected);
  const counts=item=>item?.counted_waves || [item?.waves?.filter(w=>['integrated','committed'].includes(w.status)).length || 0,item?.waves?.length || 0];
  const progress=item=>{const [done,total]=counts(item);return total?`${done}/${total} ${t('ondas concluídas','waves completed')}`:t('Sem ondas planejadas','No planned waves');};
  const go=(tab)=> {
    const wave=tab==='Ondas' && view.selectedWave===undefined
      ? (spec?.waves?.find(w=>['running','repair-required','received'].includes(w.status)) || spec?.waves?.[0])?.wave : view.selectedWave;
    actions.change({tab,contentOffset:0,...(tab==='Ondas'?{selectedWave:wave}:{})},tab==='Ondas' && wave!==undefined);
  };
  const tabs=[['Projeto','Projeto','Project'],['Definições','Definições','Definitions'],['Ondas','Ondas','Waves'],['Execução','Execução','Execution'],['Consumo','Consumo','Usage']];
  const rows=[row([text('MUSTARD',{bold:true,color:theme.accentText}),...(data?[text(data.project.name || 'Mustard',{bold:true})]:[])])];
  if(data && !compact)rows.push(note(data.project.branch || t('Sem branch','No branch')));
  if(surface!=='mobile' && (compact || columns && columns<88))rows.push(picker('panel-section',t('Seção','Section'),
    tabs.map(([value,pt,en])=>({value,label:t(pt,en)})),view.tab,go));
  else rows.push(row(tabs.map(([key,pt,en],index)=>button(`tab-${key}`,t(pt,en),()=>go(key),{active:view.tab===key,hotkey:String(index+1)}))));
  if(problem)rows.push(section(t('Atualização indisponível','Update unavailable'),[text(problem),note(`${t('Último estado','Last state')}: ${date(data?.at)}`)]));
  if(!data)return Box({flexDirection:'column',padding:1,backgroundColor:theme.paper,children:[...rows,text(t('Carregando projeto…','Loading project…'))]});
  if(specs.length)rows.push(picker('select-spec','Spec',specs.map(item=>({value:item.name,label:`${item.name} · ${state(item.phase)}`})),selected,actions.selectSpec));
  if(view.tab!=='Projeto' && spec)rows.push(text(spec.goal || spec.name,{bold:true}));

  if(view.tab==='Projeto') {
    if(spec)rows.push(section(t('Trabalho atual','Current work'),[
      text(`${spec.name} · ${state(spec.phase)}`,{bold:true}),text(spec.goal || ''),text(progress(spec)),
      row([button('open-current-definitions',t('Ver definições','Read definitions'),()=>go('Definições')),
        button('open-current-waves',t('Acompanhar ondas','Follow waves'),()=>go('Ondas'))]),
    ]));
    else rows.push(note(t('Selecione uma spec para consultar definições e ondas.','Select a spec to read definitions and waves.')));
    const total=specs.reduce((sum,item)=>{const [done,count]=counts(item);return [sum[0]+done,sum[1]+count];},[0,0]);
    rows.push(row([card(t('Specs no projeto','Project specs'),fmt(specs.length)),
      card(t('Ondas de todas as specs','Waves across all specs'),`${total[0]}/${total[1]} ${t('concluídas','completed')}`)]));
    if(specs.some(item=>item!==spec))rows.push(section(t('Outras especificações','Other specifications'),specs.filter(item=>item!==spec).map(item=>
      row([text(`${item.name} · ${state(item.phase)}`),button(`project-${item.name}`,t('Abrir','Open'),()=>actions.selectSpec(item.name))]))
      .concat(specs.length===1?[note(t('Nenhuma outra spec registrada.','No other recorded specs.'))]:[])));
  }

  if(view.tab==='Definições' && spec) {
    const definitions=spec.definitions || {}, groups=Object.keys(definitionNames).filter(key=>definitions[key]?.length);
    const group=groups.includes(view.definitionGroup)?view.definitionGroup:groups[0];
    if(groups.length)rows.push(picker('definition-group',t('Categoria','Category'),groups.map(key=>({value:key,
      label:`${t(...definitionNames[key])} (${definitions[key].length})`})),group,value=>actions.change({definitionGroup:value,definitionPage:0,
      definitionQuery:'',selectedDefinition:undefined,contentOffset:0,technicalOffset:0,showTechnical:false})));
    if(surface!=='mobile' && Input && groups.length)rows.push(Input({key:'definition-search',label:t('Buscar nesta categoria','Search this category'),
      placeholder:t('Título, código ou texto','Title, code or text'),value:view.definitionQuery,
      onInput:value=>actions.change({definitionQuery:value,definitionPage:0,selectedDefinition:undefined,contentOffset:0,technicalOffset:0,showTechnical:false}),
      onSubmit:value=>actions.change({definitionQuery:value,definitionPage:0,selectedDefinition:undefined,contentOffset:0,technicalOffset:0,showTechnical:false})}));
    const query=view.definitionQuery.trim().toLocaleLowerCase(),items=(definitions[group] || []).filter(item=>
      !query || [item.code,item.title,item.gap,definitionText(item,english)].join(' ').toLocaleLowerCase().includes(query));
    const pageCount=Math.max(1,Math.ceil(items.length/6)),page=Math.min(view.definitionPage,pageCount-1),visible=items.slice(page*6,page*6+6);
    const item=visible.find(item=>String(item.code || item.id)===String(view.selectedDefinition)) || visible[0];
    const list=section(t(`Itens encontrados (${items.length})`,`Matching items (${items.length})`),[
      ...(surface!=='mobile' && Select && (columns || 0)<112 && visible.length
        ? [picker('definition-item',t('Item','Item'),visible.map(entry=>({value:String(entry.code || entry.id),label:`${entry.code || entry.id} · ${entry.title || entry.gap || t('Sem título','Untitled')}`})),
          String(item.code || item.id),value=>actions.change({selectedDefinition:value,contentOffset:0,technicalOffset:0,showTechnical:false}))]
        : visible.map(entry=>Box({flexDirection:'column',children:[button(`item-${entry.code || entry.id}`,entry.code || String(entry.id),
          ()=>actions.change({selectedDefinition:entry.code || entry.id,contentOffset:0,technicalOffset:0,showTechnical:false}),{active:item===entry}),
          text(entry.title || entry.gap || t('Sem título','Untitled'))]}))),
      ...(!items.length?[note(t(groups.length?'Nenhum item corresponde à busca.':'Nenhuma definição registrada.',groups.length?'No matching items.':'No recorded definitions.'))]:[]),
      ...pagination('definitions',{page,pages:pageCount},value=>({definitionPage:value,contentOffset:0,showTechnical:false})),
    ]);
    const reader=[];
    if(item) {
      const body=contentPage(definitionText(item,english),view.contentOffset);
      reader.push(text(item.title || item.gap || t(...definitionNames[group]),{bold:true}),note(item.code || item.id),
        content(body.text),...pagination('definition-body',body,value=>({contentOffset:value})));
      if(item.agent) {
        reader.push(button(`technical-${item.code}`,t(view.showTechnical?'Ocultar instruções técnicas':'Ver instruções técnicas',view.showTechnical?'Hide technical instructions':'Read technical instructions'),
          ()=>actions.change({showTechnical:!view.showTechnical,technicalOffset:0})));
        if(view.showTechnical) {const technical=contentPage(item.agent,view.technicalOffset);
          reader.push(content(technical.text,true),...pagination('definition-technical',technical,value=>({technicalOffset:value})));}
      }
    }
    // Column counts are character cells on every Mods surface. Stack on narrow panes.
    rows.push(Box({flexDirection:(columns || 0)>=112?'row':'column',columnGap:1,rowGap:1,children:[
      Box({flexDirection:'column',...(columns>=112?{width:'38%'}:{}),children:[list]}),
      ...(reader.length?[Box({flexDirection:'column',flexGrow:1,flexShrink:1,children:[section(t('Conteúdo definido','Recorded definition'),reader)]})]:[]),
    ]}));
  }

  if(view.tab==='Ondas' && spec) {
    rows.push(text(`${t('Esta spec','This spec')}: ${progress(spec)}`,{bold:true}));
    if(!spec.waves.length)rows.push(section(t('Planejamento das ondas','Wave planning'),[
      text(t('As ondas ainda não foram agrupadas.','Waves have not been grouped yet.')),
      button('wave-definitions',t('Ver tarefas e critérios','Read tasks and criteria'),()=>go('Definições'))]));
    else {
      rows.push(spec.waves.length>3 && surface!=='mobile' && Select
        ? picker('select-wave',t('Onda','Wave'),spec.waves.map(w=>({value:String(w.wave),label:`${w.wave} · ${state(w.status)}`})),String(view.selectedWave),
          value=>actions.change({selectedWave:Number(value),waveView:'Plano',contentOffset:0},true))
        : row(spec.waves.map(w=>button(`wave-${w.wave}`,`${t('Onda','Wave')} ${w.wave} · ${state(w.status)}`,
          ()=>actions.change({selectedWave:w.wave,waveView:'Plano',contentOffset:0},true),{active:view.selectedWave===w.wave}))));
      if(view.selectedWave===undefined)rows.push(note(t('Selecione uma onda para ver plano, input, entrega e testes.','Select a wave to read its plan, input, return and tests.')));
      else {
        const names={Plano:['Plano e tarefas','Plan and tasks'],Pedido:['Input exato enviado','Exact sent input'],Entrega:['Entrega','Return'],Testes:['Testes','Tests']};
        rows.push(row(Object.entries(names).map(([value,label])=>button(`wave-view-${value}`,t(...label),
          ()=>actions.change({waveView:value,contentOffset:0}),{active:view.waveView===value}))));
        const detail=data.wave_detail?.spec===selected && data.wave_detail.wave===view.selectedWave?data.wave_detail:undefined;
        if(!detail)rows.push(note(t('Carregando conteúdo registrado…','Loading recorded content…')));
        else {
          const body=contentPage(waveText(detail,view.waveView,english),view.contentOffset);
          const children=[];
          if(view.waveView==='Pedido')children.push(note(t('Cópia integral do pedido enviado ao agente. Nenhum conteúdo foi resumido.','Complete copy of the request sent to the agent. No content has been summarized.')));
          if(view.waveView==='Entrega' && detail.delivery && !detail.delivery_integrated)children.push(text(
            t('Retorno recebido · aguardando validação do orquestrador','Return received · awaiting orchestrator validation'),{color:theme.accentText}));
          children.push(content(body.text,view.waveView!=='Plano'),...pagination('content',body,value=>({contentOffset:value})));
          rows.push(section(`${t('Onda','Wave')} ${view.selectedWave} · ${t(...names[view.waveView])}`,children));
        }
      }
    }
  }

  if(view.tab==='Execução' && spec) {
    rows.push(section(t('Andamento desta spec','This spec progress'),[
      text(progress(spec),{bold:true}),
      text(`${t('Validação final','Final validation')}: ${spec.final_validation_valid==null?t('Não verificada','Not checked'):spec.final_validation_valid?t('Válida','Valid'):t('Pendente','Pending')}`),
      text(`${t('Revisão','Review')}: ${spec.review_approved?t('Aprovada','Approved'):t('Pendente','Pending')}`),
    ]));
    const queue=spec.scheduling;
    if(queue) {
      const reasons={ready:['Pronta para agrupamento','Ready for grouping'],dependencies:['Aguarda dependências','Waiting for dependencies'],
        'file-reservation':['Arquivo reservado','File reserved'],capacity:['Aguarda vaga','Waiting for capacity'],
        'missing-criteria':['Falta vínculo com critério','Missing acceptance criterion'],'cleanup-last':['Limpeza aguarda implementação','Cleanup waits for implementation'],
        'spec-not-running':['Spec fora de execução','Spec is not running']};
      rows.push(section(t('Fila de trabalho','Work queue'),[
        text(`${fmt(queue.backlog)} ${t('tarefas no backlog','backlog tasks')} · ${fmt(queue.ready)} ${t('prontas','ready')}`),
        note(`${t('Ondas em execução','Running waves')}: ${fmt(queue.occupied_wave_slots)}/${fmt(queue.capacity)}`),
        ...queue.tasks.map(item=>Box({flexDirection:'column',children:[text(`${item.task} · ${item.title || ''}`,{bold:true}),
          note(`${reasons[item.reason]?t(...reasons[item.reason]):state(item.reason)}${item.dependencies?.length?': '+item.dependencies.join(', '):''}${item.holding_waves?.length?': '+item.holding_waves.join(', '):''}`)]})),
        ...(queue.omitted_tasks?[note(`${fmt(queue.omitted_tasks)} ${t('outras tarefas','other tasks')}`)]:[]),
      ]));
    }
    if(spec.stages?.length)rows.push(section(t('Validações registradas','Recorded validations'),spec.stages.map(stage=>
      text(`${stage.phase} · ${stage.stage}: ${stage.result} · ${fmt(stage.ms)} ms`))));
    if(spec.undeclared_commands?.length)rows.push(note(`${t('Comandos ainda não configurados','Commands not configured yet')}: ${spec.undeclared_commands.join(', ')}`));
  }

  if(view.tab==='Consumo') {
    const usageView=view.usageView || 'Resumo', rtk=data.rtk || {}, jev=data.jev || {};
    rows.push(picker('usage-section',t('Medição','Measurement'),[
      ['Resumo','Resumo','Overview'],['Sessão','Sessão','Session'],['Ondas','Ondas','Waves'],
      ['RTK','RTK','RTK'],['Jev','Jev','Jev'],['Histórico','Histórico','History'],['Detalhes','Detalhes','Details'],
    ].map(([value,pt,en])=>({value,label:t(pt,en)})),usageView,value=>actions.change({usageView:value})));
    if(usageView==='Resumo')rows.push(section(t('O Mustard está economizando?','Is Mustard saving tokens?'),[
      text(t('Economia total do modelo: ainda não comprovada.','Total model savings: not proven yet.'),{bold:true}),
      note(t('Falta uma comparação de trabalho equivalente com e sem Mustard.','A comparison of equivalent work with and without Mustard is missing.')),
      text(rtk.status==='measured'?`RTK: ${fmt(rtk.percent)}% ${t('de redução estimada na saída deste projeto','estimated reduction in this project’s output')}`:
        t('RTK: medição deste projeto indisponível.','RTK: project measurement unavailable.')),
      text(`Jev: ${fmt(jev.physical_requests)} ${t('chamadas no projeto','project requests')}${jev.cost_micro_usd!=null?' · '+money(jev.cost_micro_usd)+' '+t('estimados','estimated'):''}`),
      ...(spec?.usage?.claude_cost?.known_micro_usd!=null?[text(`${t('Claude nesta spec · equivalente API','Claude for this spec · API equivalent')}: ${money(spec.usage.claude_cost.known_micro_usd)}`)]:[]),
      note(t('RTK mede compressão de saída. Custos e totais de tokens não provam economia.','RTK measures output compression. Costs and token totals do not prove savings.')),
    ]));
    if(usageView==='RTK')rows.push(section(t('RTK neste projeto','RTK for this project'),[
      text(rtk.configured===false?t('Filtro desligado na configuração.','Filter disabled in configuration.'):
        rtk.hook_configured===true?t('Hook configurado para comandos compatíveis.','Hook configured for supported commands.'):
          t('Hook não confirmado; confira o diagnóstico da instalação.','Hook not confirmed; check installation diagnostics.')),
      ...(rtk.status==='measured'?[
        text(`${t('Comandos registrados','Recorded commands')}: ${fmt(rtk.commands)}`),
        ...(rtk.period_start && rtk.period_end?[note(`${t('Período do histórico RTK','RTK history period')}: ${rtk.period_start} → ${rtk.period_end}`)]:[]),
        text(`${t('Saída original → filtrada','Original → filtered output')}: ${fmt(rtk.input_tokens)} → ${fmt(rtk.output_tokens)} tokens`),
        text(`${t('Redução estimada da saída','Estimated output reduction')}: ${fmt(rtk.percent)}% · ${fmt(rtk.saved_tokens)} tokens`),
        note(`${t('Medição','Measurement')}: ${date(rtk.measured_at)}${rtk.stale?' · '+t('atualização falhou; mostrando última medição','refresh failed; showing last measurement'):''}`),
      ]:[text(rtk.status==='no-recorded-commands'?t('Nenhum comando RTK registrado neste diretório; ainda não há amostra para calcular economia.','No RTK commands recorded in this directory; no sample to calculate savings yet.'):
        t('Sem medição válida. Isso não significa economia zero.','No valid measurement. This does not mean zero savings.'))]),
      note(t('Histórico do diretório deste projeto; cópias externas de ondas podem ficar fora. Estimativa do RTK por bytes/4; não é redução da fatura nem do contexto inteiro.','History for this project directory; external wave checkouts may be excluded. RTK bytes/4 estimate; not a reduction in billing or the whole context.')),
    ]));
    const session=[],context=usage?.context;
    if(context) {
      const percent=context.percent ?? context.percentUsed;
      if(percent!=null)session.push(text(`${t('Contexto da sessão','Session context')}: ${fmt(percent)}%${context.tokens!=null && context.window!=null?' · '+fmt(context.tokens)+'/'+fmt(context.window)+' tokens':''}`));
    }
    const observed=data.statusline;
    if(observed?.model)session.push(text(`${t('Modelo','Model')}: ${observed.model}`));
    if(observed?.duration_ms!=null)session.push(note(`${t('Duração','Duration')}: ${Math.round(observed.duration_ms/60000)} min`));
    if(usage?.cost?.usd!=null)session.push(text(`${t('Custo equivalente de API nesta sessão','API-equivalent cost this session')}: $${usage.cost.usd}`));
    if(usageView==='Sessão' && session.length)rows.push(section(t('Sessão atual','Current session'),session));
    if(usageView==='Ondas' && spec)rows.push(section(t('Spec selecionada','Selected spec'),[
      text(spec.name,{bold:true}),
      ...(spec.usage?.wave_tokens!=null?[text(`${t('Ondas','Waves')}: ${fmt(spec.usage.wave_tokens)} tokens`)]:[note(t(spec.usage?.known_wave_tokens>0?'Total das ondas ainda incompleto.':'Consumo das ondas ainda não medido.',spec.usage?.known_wave_tokens>0?'Wave total is still incomplete.':'Wave usage has not been measured yet.'))]),
      ...(spec.usage?.wave_tokens==null && spec.usage?.known_wave_tokens>0?[text(`${t('Subtotal das ondas medidas','Measured waves subtotal')}: ${fmt(spec.usage.known_wave_tokens)} tokens`),
        note(`${fmt(spec.usage.waves_with_unknown_usage)} ${t('ondas ainda sem medição; subtotal incompleto.','waves still unmeasured; subtotal is incomplete.')}`)]:[]),
      ...(spec.usage?.conductor_tokens!=null?[text(`${t('Orquestrador','Orchestrator')}: ${fmt(spec.usage.conductor_tokens)} tokens`)]:[]),
      note(t('Totais acumulados das respostas, incluindo cache relido; não são o tamanho do contexto.','Accumulated response totals, including cache reads; not the context size.')),
      ...(spec.usage?.known_wave_breakdown?[note(`${t('Ondas · entrada/saída','Waves · input/output')}: ${fmt(spec.usage.known_wave_breakdown.input_tokens)}/${fmt(spec.usage.known_wave_breakdown.output_tokens)} · ${t('cache criado/lido','cache written/read')}: ${fmt(spec.usage.known_wave_breakdown.cache_creation_input_tokens)}/${fmt(spec.usage.known_wave_breakdown.cache_read_input_tokens)}`)]:[]),
      ...(spec.usage?.measured_at?[note(`${t('Medição das ondas','Wave measurement')}: ${date(spec.usage.measured_at)}`)]:[]),
      ...(spec.usage?.claude_cost?.known_micro_usd!=null?[text(`${t('Custo equivalente de API observado','Observed API-equivalent cost')}: ${money(spec.usage.claude_cost.known_micro_usd)}`)]:[]),
    ]));
    if(usageView==='Ondas' && spec?.waves.length)rows.push(section(t('Medição por onda','Usage per wave'),spec.waves.map(wave=>text(
      `${t('Onda','Wave')} ${wave.wave} · ${state(wave.status)} · ${fmt(wave.tokens)}${wave.tokens!=null?' tokens':''}`))));
    const history=data.consumption;
    if(usageView==='Histórico')rows.push(section(t('Histórico observado','Observed history'),[
      note(t('Acumulado do histórico disponível; inclui hoje parcialmente.','Accumulated available history; includes partial data for today.')),
      ...(history?.available?[row([card(t('Este projeto · tokens','This project · tokens'),short(history.project_tokens)),
        card(t('Toda a máquina · tokens','Whole machine · tokens'),short(history.machine_tokens))])]:[note(t('Histórico ainda não disponível.','History is not available yet.'))]),
      note(`${t('Última medição do projeto','Latest project measurement')}: ${date(history?.measured_at)}`),
      ...(data.claude_cost?.known_micro_usd!=null?[text(`${t('Claude estimado nas sessões observadas','Estimated Claude cost in observed sessions')}: ${money(data.claude_cost.known_micro_usd)}`)]:[]),
      note(t('Custos Claude são estimativas equivalentes de API; não são cobranças da assinatura nem medidas de economia.','Claude costs are API-equivalent estimates, not subscription charges or measured savings.')),
    ]));
    const jevConfig=jev.configuration, reasons={
      'ai-fallback-disabled':['Desligado: ai.fallback não está habilitado.','Disabled: ai.fallback is not enabled.'],
      'purpose-not-configured':['Sem filtro Jev configurado para esta finalidade.','No Jev filter configured for this purpose.'],
      missing_key:['Chave não configurada.','Key not configured.'],
      key_in_git:['Chave do arquivo recusada: mustard.json está versionado.','File key refused: mustard.json is tracked by git.'],
      configured:['Habilitado sob critério; não é acionado em toda busca.','Enabled selectively; not called for every search.']};
    const jevStatus=[];
    if(jevConfig)for(const [purpose,label] of [['search',t('Busca','Search')],['wave-planning',t('Ondas','Waves')],['context',t('Contexto','Context')]]) {
      const entry=jevConfig.purposes?.[purpose];
      if(entry)jevStatus.push(text(`${label}: ${reasons[entry.reason]?t(...reasons[entry.reason]):entry.reason}`));
    }
    const searchUsage=spec?.operations?.search;
    const gateLabels={'exact-read':['Leitura exata','Exact read'],locate:['Busca literal','Literal search'],
      'not-requested':['Escolha não solicitada','Choice not requested'],'missing-intent':['Sem intenção','Missing intent'],
      eligible:['Elegível para escolha','Eligible for choice'],'ai-fallback-disabled':['IA remota desligada','Remote AI disabled'],
      'purpose-not-configured':['Finalidade não configurada','Purpose not configured'],'provider-unavailable':['Provedor indisponível','Provider unavailable'],
      'native-operation':['Operação nativa','Native operation']};
    if(usageView==='Jev')rows.push(section('Jev',[...jevStatus,...(jev.physical_requests===0?[text(t('Nenhuma chamada registrada neste projeto.','No recorded requests in this project.'))]:[
      text(`${t('Chamadas registradas no projeto','Recorded project requests')}: ${fmt(jev.physical_requests)}`),
      ...(jev.cost_micro_usd!=null?[text(`${t('Custo estimado','Estimated cost')}: ${money(jev.cost_micro_usd)}`)]:[note(t('Custo ainda não medido.','Cost has not been measured yet.'))]),
    ]),
      ...(jev.last_attempt?[text(`${t('Última chamada','Latest request')}: ${jev.last_attempt.purpose || t('finalidade não registrada','purpose not recorded')} · ${jev.last_attempt.status>=200 && jev.last_attempt.status<300?t('sucesso','success'):t('falha ou resultado não confirmado','failed or unconfirmed')} · ${date(jev.last_attempt.at)}`)]:[]),
      ...(searchUsage?.decision_coverage?[
        text(`${t('Buscas com diagnóstico nesta spec','Searches with diagnostics in this spec')}: ${fmt(searchUsage.decision_coverage)}/${fmt(searchUsage.calls)}`),
        ...Object.entries(searchUsage.choice_gates || {}).map(([gate,count])=>text(`${gateLabels[gate]?t(...gateLabels[gate]):gate}: ${fmt(count)}`)),
        text(`${t('Cache reaproveitado','Cache reused')}: ${fmt(searchUsage.cache_hits)} · ${t('escolhas aceitas','accepted choices')}: ${fmt(searchUsage.accepted_choices)}`),
        note(`${t('Inferências locais registradas nas buscas','Recorded local inferences in searches')}: ${fmt(searchUsage.local_model_calls)}`),
        note(t('Cobertura parcial: o histórico anterior não explica as decisões e não foi preenchido com suposições.','Partial coverage: older history does not explain decisions and was not filled with assumptions.')),
      ]:[note(t('Diagnóstico por busca ainda não disponível; as novas consultas passam a registrar o motivo.','Per-search diagnostics not available yet; new queries record the reason.'))]),
      note(t('Leituras exatas e buscas literais seguem nativas. Jev só escolhe entre alternativas quando a finalidade está habilitada e a busca precisa dessa decisão.','Exact reads and literal searches stay native. Jev chooses between alternatives only when the purpose is enabled and the search needs that decision.')),
      note(t('Custo Jev estimado pela tabela; abrir este painel não faz chamada paga.','Jev cost is estimated from the pricing table; opening this panel makes no paid request.')),
    ]));
    if(usageView==='Detalhes') {
      rows.push(section(t('Origem e cobertura','Sources and coverage'),[
        text(`Mustard ${data.project.version || t('versão não informada','version not provided')}`),
        ...(data.retrieval?[
          text(data.retrieval.index_present?t('Scan: índice presente; buscas conferem o código atual.','Scan: index present; searches verify current source.'):
            t('Scan: índice ausente neste projeto. Execute /mustard:upsert e atualize o scan.','Scan: index missing in this project. Run /mustard:upsert and refresh the scan.')),
          text(data.retrieval.vectors_enabled?t('Modelo semântico local: habilitado para investigações.','Local semantic model: enabled for investigations.'):
            t('Modelo semântico local: desligado na configuração.','Local semantic model: disabled in configuration.')),
        ]:[]),
        ...(history?.available?[text(`${t('Projeto','Project')}: ${fmt(history.project_tokens)} tokens · ${t('máquina','machine')}: ${fmt(history.machine_tokens)} tokens`)]:[]),
        ...(observed?.measured_at?[note(`${t('Observação da sessão','Session observation')}: ${date(observed.measured_at)}`)]:[]),
        ...(data.claude_cost?.unattributed_micro_usd!=null?[text(`${t('Custo sem atribuição segura a spec','Cost without safe spec attribution')}: ${money(data.claude_cost.unattributed_micro_usd)}`)]:[]),
        ...(history?.trend?[text(`${t('Máquina · último dia de trabalho fechado vs média dos 7 anteriores','Machine · last closed workday vs average of previous 7')}: ${history.trend.change_percent>0?'+':''}${fmt(history.trend.change_percent)}%`)]:[]),
        ...(jev.requests_with_unknown_usage?[note(`${fmt(jev.requests_with_unknown_usage)} ${t('chamadas Jev sem medição de uso','Jev requests without usage measurements')}`)]:[]),
      ]));
      const operations=spec?.operations;
      if(operations)rows.push(section(t('Operações do Mustard nesta spec','Mustard operations in this spec'),[
        text(`${fmt(operations.calls)} ${t('chamadas','calls')} · ${fmt(operations.refusals)} ${t('recusas','refusals')}`),
        text(`${t('Entrada/saída estimadas das chamadas medidas','Estimated input/output in measured calls')}: ${fmt(operations.estimated_input_tokens)}/${fmt(operations.estimated_output_tokens)} tokens`),
        note(t('Estimativa pelo tamanho do texto. Não mede cobrança nem economia do modelo.','Text-size estimate. Does not measure model billing or savings.')),
        ...(operations.search?.calls?[text(`${t('Buscas','Searches')}: ${fmt(operations.search.calls)} · ${fmt(operations.search.enriched)} ${t('cruzadas com o índice','crossed with the index')} · ${fmt(operations.search.fallback)} ${t('com retorno nativo de reserva','with native fallback')}`)]:[]),
      ]));
      for(const limit of usage?.rateLimits || [])if(limit.percentUsed!=null)rows.push(text(`${limit.kind || limit.label || limit.name || t('Janela','Window')}: ${fmt(limit.percentUsed)}% · ${t('renova','resets')} ${date(limit.resetsAt)}`));
      const breakdown=spec?.usage?.known_wave_breakdown;
      if(breakdown)rows.push(section(t('Tokens das ondas com medição','Wave tokens with measurements'),[
        text(`${t('Entrada','Input')}: ${fmt(breakdown.input_tokens)} · ${t('saída','output')}: ${fmt(breakdown.output_tokens)}`),
        text(`${t('Cache criado/lido','Cache written/read')}: ${fmt(breakdown.cache_creation_input_tokens)}/${fmt(breakdown.cache_read_input_tokens)}`),
        note(`${fmt(spec.usage.waves_with_unknown_breakdown)} ${t('ondas sem detalhamento','waves without usage details')}`),
      ]));
    }
  }
  if(!spec && !['Projeto','Consumo'].includes(view.tab))rows.push(note(t('Nenhuma spec selecionada.','No spec selected.')));
  if(publicationMessage && (publicationMessage.name===selected || ['project','report'].includes(publicationMessage.name)))rows.push(text(publicationMessage.text));
  rows.push(row([button('refresh',t('Atualizar','Refresh'),actions.refresh,{hotkey:'r'}),
    button('publications',t('Publicação externa','External publication'),()=>actions.change({showPublications:!view.showPublications}),{active:view.showPublications}),
    button('close',t('Fechar','Close'),actions.close,{hotkey:'q'})]));
  if(view.showPublications) {
    const children=[note(t('Publica um retrato do estado atual somente ao acionar um dos botões abaixo.','Publishes a snapshot of the current state only when you activate a button below.')),
      row([button('publish',t('Publicar esta spec','Publish this spec'),()=>actions.publish('spec '+selected),{disabled:publishing || !selected}),
        button('publish-project',t('Publicar projeto','Publish project'),()=>actions.publish('project'),{disabled:publishing})])];
    for(const [label,url] of [[t('Última página do projeto','Latest project page'),data.project.publication_url],[t('Última página desta spec','Latest page for this spec'),spec?.publication?.url]])
      if(url)children.push(Link?Link({href:url,label}):note(`${label}: ${url}`));
    rows.push(section(t('Publicação sob pedido','Publication on request'),children));
  }
  rows.push(note(`${t('Ao vivo','Live')} · ${date(data.at)}`));
  return Box({flexDirection:'column',rowGap:compact?0:1,padding:compact?0:1,backgroundColor:theme.paper,children:rows});
}
