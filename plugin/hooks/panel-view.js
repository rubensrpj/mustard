// The panel's screens as one tree for every surface. The binary owns every
// number; this module only lays out the answer of the visible screen. The
// terminal and the desktop differ in navigation (keys in the footer against
// tabs on top), in the chart (blocks against Svg) and in spacing.
import {contentPage} from './panel-content.js';
import {settingsScreen} from './panel-settings.js';
import {countdown} from './panel-runtime.js';

export const PAGE=8;
export const KINDS=['task','criterion','decision','rule','question'];
export const FILTERS=['all','attention','committed','uncommitted'];
export const initialPanelState=()=>({screen:'overview',spec:null,kind:'task',query:'',filter:'all',rows:PAGE,item:null,
  back:[],history:false,waveRequest:false,searching:false,pickSpec:false,delivered:false});

/** The `run panel` section and arguments of a screen; none for Settings. */
export function screenQuery(state) {
  if(state.screen==='items')return {section:'items',args:['--kind',state.kind,'--filter',state.filter,'--offset','0','--limit',String(Math.min(50,state.rows)),
    ...(state.query.trim()?['--query',state.query.trim()]:[])]};
  if(state.screen==='item')return {section:'item',args:state.item?['--item',state.item]:[]};
  return ['overview','spec','usage'].includes(state.screen)?{section:state.screen,args:[]}:undefined;
}

const phases={survey:['levantamento','survey'],plan:['planejamento','planning'],planned:['planejada','planned'],approved:['aprovada','approved'],
  running:['execução','running'],closed:['fechada','closed'],pr_open:['PR aberto','PR open'],delivered:['entregue','delivered'],discarded:['descartada','discarded']};
// How a merged pull request reads against the phase the spec is still in.
const still={running:['segue em execução','is still running'],approved:['segue aprovada, sem execução','is still approved, not running'],
  closed:['segue fechada, sem entrega','is still closed, not delivered'],pr_open:['segue com o PR aberto','still has the PR open']};
const waveStates={todo:['a fazer','to do'],running:['em execução','running'],'no-agent':['parada sem agente','stopped, no agent'],
  integrated:['integrada, sem commit','integrated, no commit'],committed:['comitada','committed'],rejected:['reprovada','rejected'],backlog:['no backlog','in the backlog']};
const kindNames={task:['Tarefas','Tasks'],criterion:['Critérios','Criteria'],decision:['Decisões','Decisions'],rule:['Regras','Rules'],question:['Perguntas','Questions']};
const filterNames={all:['Todas','All'],attention:['Pedem atenção','Need attention'],committed:['Comitadas','Committed'],uncommitted:['Sem commit','Not committed']};
const reasons={'wave-no-agent':['parada sem agente','stopped, no agent'],'wave-rejected':['reprovada','rejected'],
  'criterion-failing':['prova falhando','proof failing'],'criterion-without-proof':['sem prova','no proof'],'question-open':['aberta','open']};
const authors={assistant:['pelo condutor','by the conductor'],user:['pelo usuário','by the user'],round:['pela rodada','by the round']};
// How an item another one points to is named on screen: by its kind in words.
const itemKinds={criterion:['O critério','The criterion'],decision:['A decisão','The decision'],rule:['A regra','The rule'],request:['O pedido','The request'],
  limit:['O limite','The limit'],point:['A pergunta','The question'],task:['A tarefa','The task']};
const titles={overview:['Visão geral','Overview'],spec:['Spec','Spec'],items:['Itens','Items'],item:['Item','Item'],usage:['Consumo','Usage'],settings:['Configurações','Settings']};

/** The two chart colors for the person's theme, as the `/config` row names it. */
export function chartColors(theme) {
  return String(theme ?? '').includes('light')?{bar:'#9a7400',empty:'#8a877d'}:{bar:'#e1ad01',empty:'#6e6c66'};
}

/** One block per day: `▁` to `█` by the highest day, `·` for a day without measure. */
export function dayBlocks(days) {
  const max=Math.max(0,...days.map(day=>day.tokens ?? 0));
  return days.map(day=>day.tokens==null?'·':'▁▂▃▄▅▆▇█'[max?Math.min(7,Math.max(0,Math.ceil(day.tokens/max*8)-1)):0]).join('');
}

export function renderPanel({elements,surface,columns,state,screens={},project,usage,settings,measuring=false,publication,problem,theme,now=Date.now(),actions}) {
  const {Box,Text,Button,Select,Input,Svg}=elements;
  const terminal=surface==='terminal';
  const english=project?.language==='en-US', locale=english?'en-US':'pt-BR';
  const t=(pt,en)=>english?en:pt, name=pair=>pair?t(...pair):undefined;
  const clean=value=>String(value ?? '').replace(/[\u0000-\u0008\u000b-\u001f\u007f]/g,'');
  const text=(value,props={})=>Text({children:clean(value),wrap:'wrap',...props});
  const label=value=>text(value,{color:'subtle'});
  const join=(...parts)=>parts.filter(part=>part!=null && part!=='').join('  ·  ');
  const int=value=>value==null?'—':Number(value).toLocaleString(locale);
  // Numbers keep plain spaces: a terminal draws the no-break one as any other.
  const spaced=value=>value.replace(/[\u00a0\u202f]/g,' ');
  const compact=value=>value==null?'—':spaced(new Intl.NumberFormat(locale,{notation:'compact',maximumSignificantDigits:3}).format(value));
  // A rounded figure drops the cents only from a hundred dollars up.
  const money=(micro,round=false)=>{if(micro==null)return t('não medido','not measured');const digits=round && micro>=1e8?0:2;
    return spaced(new Intl.NumberFormat(locale,{style:'currency',currency:'USD',minimumFractionDigits:digits,maximumFractionDigits:digits}).format(micro/1e6));};
  // A date reads as it was recorded, never shifted into another zone.
  const day=value=>{const m=/^(\d{4})-(\d{2})-(\d{2})/.exec(String(value ?? ''));return m?(english?`${m[2]}/${m[3]}`:`${m[3]}/${m[2]}`):'—';};
  const hour=value=>/T(\d{2}:\d{2})/.exec(String(value ?? ''))?.[1] ?? '';
  const percent=value=>value==null?'—':`${Number(value).toLocaleString(locale,{maximumFractionDigits:1})}%`;
  const row=(children,props={})=>Box({flexDirection:'row',flexWrap:'wrap',columnGap:terminal?2:1,...props,children});
  const block=(title,lines,props={})=>Box({flexDirection:'column',...(terminal?{}:{borderStyle:'round',borderColor:'subtle',paddingX:1}),...props,
    children:[...(title?[label(title)]:[]),Box({flexDirection:'column',...(terminal && title?{paddingLeft:2}:{}),...(terminal?{}:{rowGap:0}),children:lines})]});
  const press=(key,caption,onPress,extra={})=>Button({key,label:clean(caption),plain:true,onPress,...extra});
  const warn=value=>text(`▲  ${value}`,{color:'warning'});
  // An item by its own text, never by its code: `A regra "…"`.
  const named=item=>[name(itemKinds[item.kind]),`"${item.title || t('sem título','untitled')}"`].filter(Boolean).join(' ');
  const state_=state;
  const screen=state_.screen;
  const answer=screens[screen];
  // A key is a Button with a hotkey: in the terminal's footer; on desktop in
  // place, where the screen draws it. Navigation keys live in the desktop's
  // top row instead, so `nav` draws them in the terminal alone.
  const keys=[];
  const keyButton=k=>k.disabled?label(k.disabled):press(k.id,k.caption,k.onPress,k.hotkey?{hotkey:k.hotkey}:{});
  const key=(id,hotkey,caption,onPress,disabled)=>{const k={id,hotkey,caption,onPress,disabled};if(terminal)keys.push(keyButton(k));else body.push(keyButton(k));};
  const nav=(id,hotkey,caption,onPress)=>{if(terminal)keys.push(keyButton({id,hotkey,caption,onPress}));};

  const body=[];
  if(problem)body.push(text(problem,{color:'error'}));
  if(screen==='settings') {
    body.push(settingsScreen(settings,actions,{t,text,label,block,press,join,row,column:children=>Box({flexDirection:'column',rowGap:1,children}),Select,Input}));
    nav('back','v',t('voltar','back'),actions.back);
  } else if(!answer) body.push(label(t('Carregando…','Loading…')));
  else if(answer.reason==='panel-no-spec' && screen!=='overview') body.push(text(t('Nenhuma spec neste projeto.','No spec in this project.')));
  else if(screen==='overview') overview(answer);
  else if(screen==='spec') specScreen(answer);
  else if(screen==='items') itemsScreen(answer);
  else if(screen==='item') itemScreen(answer);
  else if(screen==='usage') usageScreen(answer);

  function overview(d) {
    const s=d.spec;
    if(!s) body.push(block(null,[text(t('Nenhuma spec aberta neste projeto.','No open spec in this project.'))]));
    else {
      const phase=text(name(phases[s.phase]) ?? s.phase,{color:'claude'});
      const tasks=`${int(s.tasks.finished)} ${t('de','of')} ${int(s.tasks.total)}`;
      if(terminal) body.push(block(null,[text(s.name,{bold:true}),row([phase,text('·',{color:'subtle'}),text(`${tasks} ${t('tarefas feitas','tasks done')}`)]),
        text(join(`${int(s.criteria.passed)} ${t('de','of')} ${int(s.criteria.total)} ${t('critérios','criteria')}`,
          `${int(s.waves.finished)} ${t('de','of')} ${int(s.waves.planned)} ${t('ondas','waves')}`))]));
      else {
        const card=(figure,caption)=>Box({flexDirection:'column',children:[text(figure,{bold:true}),label(caption)]});
        body.push(block(t('Agora','Now'),[row([text(s.name,{bold:true}),phase],{justifyContent:'space-between'}),label(s.goal),
          row([press('go-items',`${s.tasks.finished}/${s.tasks.total} ${t('tarefas feitas','tasks done')}`,()=>actions.go('items',{kind:'task'}),{hotkey:'t'}),
            card(`${s.criteria.passed}/${s.criteria.total}`,t('critérios passando','criteria passing')),
            card(`${s.waves.finished}/${s.waves.planned}`,t('ondas concluídas','waves finished'))],{columnGap:3})]));
      }
      const warnings=d.warnings.map(warning).filter(Boolean);
      if(warnings.length)body.push(block(t('Precisa de atenção','Needs attention'),warnings.flat()));
      // The binary's sentence for the step; its command stays off the screen.
      if(s.next?.text)body.push(block(t('Próximo passo','Next step'),[terminal?text(s.next.text)
        :row([Box({flexGrow:1,children:[text(s.next.text)]}),press('next-see',t('Ver','See'),()=>actions.go('spec'))],{alignItems:'center'})]));
      const air=s.air;
      body.push(block(t('Ondas no ar','Waves in the air'),[text(join(air.running?`${air.running} ${t('no ar','running')}`:t('nenhuma no ar','none running'),
        `${air.free_slots} ${t(air.free_slots===1?'vaga livre':'vagas livres',air.free_slots===1?'free slot':'free slots')}`)),
        ...(air.backlog?[text(`${air.backlog} ${t('tarefas na fila','tasks queued')}`)]:[]),
        ...(d.usage?.session_agents!=null?[label(`${d.usage.session_agents} ${t('agentes trabalhando na sessão','agents working in the session')}`)]:[])]));
    }
    if(d.others?.shown?.length)body.push(block(t('Outras specs que pedem atenção','Other specs that need attention'),[
      ...d.others.shown.map(other=>Box({flexDirection:'column',children:[press(`spec-${other.name}`,other.name,()=>actions.selectSpec(other.name)),
        text(join(name(phases[other.phase]) ?? other.phase,other.warning?.kind==='pr-merged'?`PR #${other.warning.pr ?? '?'} ${t('na base desde','in the base since')} ${day(other.warning.at)}`:''),{color:'warning'})]})),
      label(`${d.others.attention} ${t('de','of')} ${d.others.total} specs`)]));
    // The project summary as the binary gave it: the newest deliveries first.
    if(d.deliveries?.shown?.length)body.push(block(t('Entregas do projeto','Project deliveries'),[
      ...d.deliveries.shown.map(item=>Box({flexDirection:'column',children:[text(item.title,{bold:true}),
        label(join(day(item.delivered_at),item.pr!=null?`PR #${item.pr}`:'',item.rules?`${int(item.rules)} ${t(item.rules===1?'regra':'regras',item.rules===1?'rule':'rules')}`:'')),
        ...(item.delivered?[text(item.delivered)]:[])]})),
      label(`${int(d.deliveries.shown.length)} ${t('de','of')} ${int(d.deliveries.total)} ${t('entregas','deliveries')}`)]));
    const limits=Object.fromEntries((usage?.rateLimits || []).map(limit=>[limit.kind,limit]));
    const host=[`${t('contexto','context')} ${percent(usage?.context?.percent)}`,`${t('cota 5 h','5 h quota')} ${percent(limits.five_hour?.percentUsed)}`,
      `${t('semana','week')} ${percent(limits.seven_day?.percentUsed)}`];
    body.push(block(t('Uso','Usage'),[...(terminal?[text(join(host[0],host[1])),text(host[2])]:[text(host.join('    '))]),
      text(join(`${money(d.usage?.cost_micro_usd,true)} ${t('no projeto','in the project')}`,`${int(d.usage?.sessions)} ${t('sessões','sessions')}`)),
      ...(terminal?[]:[label(`${compact(d.usage?.tokens)} tokens`)])]));
    nav('go-spec','s',t('spec','spec'),()=>actions.go('spec'));
    nav('go-items','t',t('tarefas','tasks'),()=>actions.go('items',{kind:'task'}));
    nav('go-usage','c',t('consumo','usage'),()=>actions.go('usage'));
    // The same publication as /mustard:pages project, only on its press, and
    // its answer as the command gives it.
    key('publish','p',terminal?t('publicar','publish'):t('Publicar no Cloudflare','Publish to Cloudflare'),actions.publish,
      publication?.busy && t('Publicando…','Publishing…'));
    if(publication?.text)body.push(text(publication.text));
    nav('go-settings',undefined,t('configurações','settings'),()=>actions.go('settings'));
  }

  function warning(item) {
    if(item.kind==='pr-merged') {
      const head=`PR #${item.pr ?? '?'} ${t('na base desde','in the base since')} ${day(item.at)}`;
      const tail=`${t('mas a spec','but the spec')} ${name(still[item.phase]) ?? name(phases[item.phase]) ?? item.phase}`;
      return terminal?[warn(`${head},`),text(`   ${tail}`,{color:'warning'})]:[warn(`${head}, ${tail}.`)];
    }
    if(item.kind==='review-reopened') {
      const again=day(item.reopened_at)===day(item.approved_at)?`${t('às','at')} ${hour(item.reopened_at)}`:`${t('em','on')} ${day(item.reopened_at)}`;
      return [warn(`${t('Revisão aprovada em','Review approved on')} ${day(item.approved_at)} ${t('e reaberta','and reopened')} ${again}`)];
    }
    if(item.kind==='wave-no-agent')return [warn(item.waves.length===1?`${t('A onda','Wave')} ${item.waves[0]} ${t('está parada sem agente','is stopped without an agent')}`
      :`${t('As ondas','Waves')} ${item.waves.join(', ')} ${t('estão paradas sem agente','are stopped without an agent')}`)];
    return undefined;
  }

  function specScreen(d) {
    const s=d.spec;
    body.push(block(null,[text(s.name,{bold:true}),label(s.goal),text(name(phases[s.phase]) ?? s.phase,{color:'claude'})]));
    body.push(block(null,[text(`${t('Branch','Branch')}  ${s.branch ?? '—'}${s.base?` ← ${s.base}`:''}`),
      text(join(`${t('Aberta','Opened')} ${day(s.opened_at)}`,s.last_commit?`${t('último commit','last commit')} ${day(s.last_commit.at)}`:t('sem commit','no commit'))),
      ...(terminal?[]:[label(`${int(s.commits)} commits`)])]));
    const c=s.counts;
    const tasks=`${int(c.tasks.total)} ${t('tarefas','tasks')}  ·  ${c.tasks.finished===c.tasks.total?t('todas feitas','all done'):`${int(c.tasks.finished)} ${t('feitas','done')}`}`;
    const open=kind=>()=>actions.go('items',{kind});
    body.push(block(t('Combinado','Agreed'),[
      terminal?text(tasks):press('go-items',tasks,open('task'),{hotkey:'t'}),
      press('items-criterion',join(`${int(c.criteria.total)} ${t('critérios','criteria')}`,c.criteria.failed?`${c.criteria.failed} ${t('falhando','failing')}`
        :c.criteria.without_proof?`${c.criteria.without_proof} ${t('sem prova','without proof')}`:t('todos com prova','all proven')),open('criterion')),
      press('items-question',join(`${int(c.questions.total)} ${t('perguntas','questions')}`,`${int(c.questions.open)} ${t('abertas','open')}`),open('question')),
      press('items-decision',`${int(c.decisions)} ${t('decisões','decisions')}`,open('decision')),
      press('items-rule',`${int(c.rules)} ${t('regras','rules')}`,open('rule'))]));
    const w=s.waves;
    body.push(block(t('Ondas','Waves'),[text(join(`${int(w.planned)} ${t('planejadas','planned')}`,`${int(w.finished)} ${t('concluídas','finished')}`)),
      text(join(`${int(w.committed)} ${t('comitadas','committed')}`,`${int(w.integrated)} ${t('integradas','integrated')}`)),
      ...(w.running||w.todo?[text(join(w.running?`${w.running} ${t('em execução','running')}`:'',w.todo?`${w.todo} ${t('a fazer','to do')}`:''))]:[]),
      ...(w.no_agent.length?[warn(`${t('parada sem agente','stopped, no agent')}: ${t('onda','wave')} ${w.no_agent.join(', ')}`)]:[]),
      ...(w.rejected.length?[warn(`${t('reprovada','rejected')}: ${t('onda','wave')} ${w.rejected.join(', ')}`)]:[])]));
    const r=s.review;
    const review=r.valid?text(`${t('revisão final aprovada em','final review approved on')} ${day(r.approved_at)}`,{color:'success'})
      :r.reopened_at?warn(`${t('revisão aprovada em','review approved on')} ${day(r.approved_at)} ${t('e reaberta','and reopened')} ${day(r.reopened_at)===day(r.approved_at)?`${t('às','at')} ${hour(r.reopened_at)}`:`${t('em','on')} ${day(r.reopened_at)}`}`)
      :label(t('revisão final ainda não aprovada','final review not approved yet'));
    const pr=s.pr?.merged_at?text(`PR #${s.pr.number ?? '?'} ${t('na base desde','in the base since')} ${day(s.pr.merged_at)}`,{color:'success'})
      :s.pr?.number?text(`PR #${s.pr.number} ${t('aberto','open')}`):label(t('sem PR','no PR'));
    body.push(block(t('Fechamento','Closing'),[review,pr]));
    if(!terminal || state_.pickSpec)body.push(specList(d));
    nav('go-items','t',t('tarefas','tasks'),open('task'));
    nav('other-spec','o',state_.pickSpec?t('esconder specs','hide specs'):t('outra spec','other spec'),actions.toggleSpecs);
    nav('back','v',t('voltar','back'),actions.back);
  }

  function specList(d) {
    const groups=[[t('Em andamento','In progress'),['approved','running','closed','pr_open']],[t('Levantamento','Survey'),['survey','plan','planned',null,undefined]]];
    const done=d.specs.filter(item=>item.phase==='delivered'),dropped=Math.max(d.discarded ?? 0,d.specs.filter(item=>item.phase==='discarded').length);
    const entry=item=>press(`spec-${item.name}`,join(`${item.name}${item.name===d.selected_spec?' ✓':''}`,
      ['survey','plan','planned'].includes(item.phase)?`${t('desde','since')} ${day(item.created)}`:name(phases[item.phase]) ?? item.phase),()=>actions.selectSpec(item.name));
    const lines=[];
    for(const [title,members] of groups) {
      const rows=d.specs.filter(item=>members.includes(item.phase));
      if(rows.length)lines.push(label(title),...rows.map(entry));
    }
    if(done.length || dropped)lines.push(press('delivered',join(`${done.length} ${t('entregues','delivered')}`,dropped?`${dropped} ${t(dropped===1?'descartada':'descartadas','discarded')}`:''),actions.toggleDelivered));
    if(state_.delivered)lines.push(...d.specs.filter(item=>['delivered','discarded'].includes(item.phase)).map(entry));
    return block(`${t('Trocar de spec','Switch spec')} · ${d.specs.length}`,lines);
  }

  function rowDetail(item,kind) {
    const reason=name(reasons[item.attention]);
    if(kind==='task') {
      if(item.wave==null)return t('no backlog','in the backlog');
      const stateText=reason ?? `${name(waveStates[item.state]) ?? item.state}${item.state==='committed' && item.at?` ${t('em','on')} ${day(item.at)}`:''}`;
      const head=join(`${t('onda','wave')} ${item.wave}`,stateText);
      if(terminal)return head;
      return join(head,item.resends?`${t('reenviada','resent')} ${item.resends} ${t(item.resends===1?'vez':'vezes',item.resends===1?'time':'times')}`:'',
        item.unmet?t('combinado não atendido','agreement not met'):'',item.files!=null?`${item.files} ${t('arquivos','files')}`:'',item.covers?`${t('cobre','covers')} ${item.covers}`:'');
    }
    return reason ?? (item.wave!=null?`${t('onda','wave')} ${item.wave}`:'');
  }

  function itemsScreen(d) {
    const kind=d.kind;
    const heading=`${name(kindNames[kind])} · ${int(d.counts[kind])}`;
    if(terminal)body.push(block(null,[text(heading,{bold:true}),
      ...(Select?[Select({key:'kind',label:t('Tipo','Kind'),value:kind,options:KINDS.map(value=>({value,label:`${name(kindNames[value])} ${int(d.counts[value])}`})),onSelect:actions.setKind})]:[])]));
    else body.push(row(KINDS.map(value=>press(`kind-${value}`,`${name(kindNames[value])} ${int(d.counts[value])}`,()=>actions.setKind(value),value===kind?{variant:'primary'}:{}))));
    if(Input && (!terminal || state_.searching || state_.query))body.push(Input({key:'search',label:t('Buscar','Search'),placeholder:t('Título ou arquivo','Title or file'),
      value:state_.query,...(terminal && state_.searching?{autoFocus:true}:{}),onSubmit:actions.search}));
    if(state_.query)body.push(row([label(`${t('Busca','Search')}: ${state_.query}`),press('search-clear',t('limpar','clear'),()=>actions.search(''))]));
    const shown=kind==='task'?FILTERS:['all','attention'];
    if(terminal) {if(d.filter!=='all')body.push(label(`${t('Filtro','Filter')}: ${name(filterNames[d.filter])} (${int(d.filters[d.filter])})`));}
    else body.push(row(shown.map(value=>press(`filter-${value}`,`${name(filterNames[value])} ${int(d.filters[value])}`,()=>actions.setFilter(value),value===d.filter?{variant:'primary'}:{}))));
    const rows=d.rows.map(item=>{const detail=rowDetail(item,kind);
      return Box({flexDirection:'column',children:[press(`item-${item.code}`,item.title || t('Sem título','Untitled'),()=>actions.openItem(item.code)),
        ...(detail?[text(detail,{color:item.attention?'warning':'subtle'})]:[])]});});
    body.push(Box({flexDirection:'column',rowGap:terminal?1:0,children:rows.length?rows:[label(t('Nenhum item encontrado.','No item found.'))]}));
    if(d.rows.length)body.push(label(join(`1 ${t('a','to')} ${d.rows.length} ${t('de','of')} ${int(d.total)}`,
      !state_.query && d.filter==='all'?t('as que pedem atenção vêm antes','the ones needing attention come first'):'')));
    if(d.rows.length<d.total)body.push(press('more',t('Mostrar mais','Show more'),actions.more));
    if(terminal)body.push(label(t('tab escolhe  ·  enter abre','tab picks  ·  enter opens')));
    nav('search-open','b',t('buscar','search'),actions.openSearch);
    nav('filter','f',`${t('filtrar','filter')} (${name(filterNames[d.filter])})`,actions.cycleFilter);
    nav('back','v',t('voltar','back'),actions.back);
  }

  function itemScreen(d) {
    const item=d.item;
    if(!item){body.push(text(t('Item não encontrado.','Item not found.')));nav('back','v',t('voltar','back'),actions.back);return;}
    body.push(block(null,[text(item.title || t('Sem título','Untitled'),{bold:true})]));
    // The wave's date reads with its state: the commit, the return, the send.
    const when=item.at?item.state==='committed'?` ${t('em','on')} ${day(item.at)}`:'':'';
    const since=item.at && item.state!=='committed'?[label(`${item.state==='integrated' || item.state==='rejected'?t('entregue em','delivered on'):t('desde','since')} ${day(item.at)}`)]:[];
    if(item.kind==='task')body.push(block(null,[text(item.wave==null?t('no backlog','in the backlog')
      :join(`${t('onda','wave')} ${item.wave}`,`${name(waveStates[item.state]) ?? item.state}${when}`)),...since]));
    const long=value=>value?[text(value.text),...(value.cut?[label(t(`texto cortado: ${int(value.chars)} caracteres no total`,`text cut: ${int(value.chars)} characters in all`))]:[])]:[];
    if(item.request)body.push(block(t('Pedido','Request'),long(item.request)));
    if(item.done)body.push(block(t('O que o agente fez','What the agent did'),long(item.done)));
    if(item.result?.agreed?.length)body.push(block(t('Resultado','Result'),[
      text(item.result.unmet?t('combinado não atendido','agreement not met'):t('combinado atendido','agreement met'),{color:item.result.unmet?'warning':'success'}),
      ...item.result.agreed.flatMap(answer=>[text(join(named(answer),answer.met?t('atendido','met'):t('não atendido','not met'))),...(answer.note?[label(answer.note)]:[])])]));
    if(item.done_when){const page=contentPage(item.done_when,0,600);body.push(block(t('Término','Done when'),[text(page.text+(page.pages>1?' …':''))]));}
    if(item.files?.length || item.delivered_files!=null)body.push(block(t('Arquivos','Files'),[...(item.files || []).map(file=>text(file)),
      ...(item.delivered_files!=null?[label(`${int(item.delivered_files)} ${t('arquivos na entrega da onda','files in the wave delivery')}`)]:[])]));
    if(item.covers?.length)body.push(block(t('Cobre','Covers'),[...item.covers.map(cover=>text(named(cover))),
      ...(item.covers.some(cover=>cover.kind==='criterion')?[]:[label(t('Nenhum critério ligado.','No criterion linked.'))])]));
    // The tests run on what the task covers: the proof and its newest run.
    const results={pass:['passou','passed'],fail:['falhou','failed']};
    if(item.tests?.length)body.push(block(t('Testes','Tests'),item.tests.flatMap(run=>[
      text(join(named({kind:'criterion',title:run.title}),run.result?name(results[run.result]) ?? run.result:t('não rodado','not run'),run.at?day(run.at):''),
        {color:run.result==='fail'?'warning':run.result==='pass'?'success':'subtle'}),...(run.proof?[label(run.proof)]:[])])));
    // The wave reviewer's newest verdict: approved, or rejected with its reason and the files it points at.
    if(item.review){const approved=item.review.result==='approved';
      body.push(block(null,[text(approved?t('Revisão: aprovada','Review: approved')
        :`${t('Revisão: reprovada','Review: rejected')}${item.review.reason?` — ${item.review.reason}`:''}`,{color:approved?'success':'warning'}),
        ...(approved?[]:(item.review.files || []).map(file=>label(file)))]));}
    // The wave's request goes whole, as the agent got it, only on its key.
    if(item.wave_request!=null) {
      key('wave-request','p',`${state_.waveRequest?t('esconder pedido da onda','hide wave request'):t('pedido da onda','wave request')} (${int(item.wave_request.length)} ${t('caracteres','characters')})`,actions.toggleWaveRequest);
      if(state_.waveRequest)body.push(block(t('Pedido da onda','Wave request'),[text(item.wave_request)]));
    }
    if(item.wave!=null)body.push(block(t('Gasto','Spend'),[text(item.wave_tokens==null?t('não medido','not measured'):`${compact(item.wave_tokens)} tokens ${t('na onda','in wave')} ${item.wave}`),
      ...(item.resends?[label(`${t('reenviada','resent')} ${item.resends} ${t(item.resends===1?'vez':'vezes',item.resends===1?'time':'times')}`)]:[])]));
    const history=item.history || [];
    key('history','h',`${state_.history?t('esconder histórico','hide history'):t('histórico','history')} (${history.length} ${t(history.length===1?'versão':'versões',history.length===1?'version':'versions')})`,actions.toggleHistory);
    if(state_.history)body.push(block(`${t('Histórico','History')} · ${history.length} ${t(history.length===1?'versão':'versões',history.length===1?'version':'versions')}`,
      [...history].reverse().map((version,index)=>Box({flexDirection:'column',children:[text(change(version,history.length-1-index)),
        label(join(`${day(version.at)} ${hour(version.at)}`,name(authors[version.author]) ?? version.author))]}))));
    nav('back','v',t('voltar','back'),actions.back);
  }

  function change(version,index) {
    if(index===0 || !version.changed)return t('Primeira versão','First version');
    const changed=version.changed,fields=Object.keys(changed);
    if(changed.wave)return changed.wave.to==null?t('Tirada da onda','Taken out of its wave'):`${t('Posta na onda','Put in wave')} ${changed.wave.to}`;
    if(fields.includes('title'))return t('Título reescrito','Title rewritten');
    if(fields.includes('text'))return t('Pedido reescrito','Request rewritten');
    return `${t('Mudou','Changed')}: ${fields.join(', ')}`;
  }

  function usageScreen(d) {
    body.push(block(null,[text(t('Consumo','Usage'),{bold:true}),
      text(d.measured_through?`${t('medido até','measured through')} ${day(d.measured_through)}`:t('ainda não medido','not measured yet'),{color:'warning'}),
      ...(d.measured_at?[label(`${t('sem atualizar desde','not updated since')} ${day(d.measured_at)}`)]:[]),
      ...(terminal?[]:[measureButton()])]));
    if(d.available===false)body.push(block(null,[label(t('O histórico deste projeto ainda não foi medido. Use Medir agora.','This project history was not measured yet. Use Measure now.'))]));
    body.push(block(`${t('Total','Total')} · ${day(d.period?.first)} ${t('a','to')} ${day(d.period?.last)}`,[
      text(join(t('projeto','project'),`${compact(d.project_tokens)} tokens`)),text(join(t('máquina','machine'),`${compact(d.machine_tokens)} tokens`)),
      ...(d.trend?.ratio!=null?[label(`${t('último dia','last day')} (${day(d.trend.day)}): ${Number(d.trend.ratio).toLocaleString(locale)}× ${t('a média da semana','the week average')}`)]:[])]));
    const cost=d.cost;
    body.push(block(t('Custo estimado','Estimated cost'),[text(`${money(cost.total_micro_usd)} ${t('em','in')} ${int(cost.sessions)} ${t('sessões','sessions')}`),
      ...cost.by_spec.map(item=>text(join(item.name,money(item.micro_usd)))),
      ...(cost.outside_spec_micro_usd!=null?[text(join(t('fora de spec','outside a spec'),money(cost.outside_spec_micro_usd)))]:[]),
      ...(cost.specs_without_cost?[label(`${cost.specs_without_cost} ${t('specs sem custo atribuído','specs without attributed cost')}`)]:[])]));
    const daysShown=d.days || [];
    if(daysShown.length) {
      const top=daysShown.reduce((best,item)=>item.tokens!=null && (best==null || item.tokens>best.tokens)?item:best,null);
      const span=`${day(daysShown[0].day)} ${t('a','to')} ${day(daysShown.at(-1).day)}`;
      const chart=terminal?[row([text(dayBlocks(daysShown)),label(span)])]:[chartSvg(daysShown),row([label(day(daysShown[0].day)),label(day(daysShown[7]?.day)),label(day(daysShown.at(-1).day))],{justifyContent:'space-between'}),
        label(`${span}  ·  ${t('tracejado: sem medida','dashed: no measure')}`)];
      body.push(block(t('Tokens por dia · este projeto','Tokens per day · this project'),[...chart,
        ...(top?[label(`${t('máximo','highest')} ${compact(top.tokens)} ${t('em','on')} ${day(top.day)}`)]:[])]));
    }
    if(d.spec)body.push(block(`${t('Esta spec','This spec')} · ${d.spec.name}`,[text(join(t('ondas','waves'),`${compact(d.spec.wave_tokens)} tokens`)),
      ...(d.spec.waves_without_measure?[label(`${d.spec.waves_without_measure} ${t('ondas sem medida','waves without measure')}`)]:[]),
      text(join(t('condutor','conductor'),d.spec.conductor_tokens==null?t('não medido','not measured'):`${compact(d.spec.conductor_tokens)} tokens`))]));
    if(d.spec?.waves)waveAgents(d.spec);
    if(d.mustard_skills?.length)mustardSkills(d.mustard_skills);
    const tools=d.tools,jev=tools.jev,purposes={'wave-planning':['montagem','planning'],context:['contexto','context'],search:['busca','search'],'spec-search':['busca na spec','spec search']};
    body.push(block(t('Ferramentas','Tools'),[
      text(tools.rtk.configured?join('RTK',tools.rtk.percent!=null?`${percent(tools.rtk.percent)} ${t('a menos','less')}`:t('não medido','not measured')):join('RTK',t('desligado','off'))),
      ...(tools.rtk.commands!=null?[label(join(`${int(tools.rtk.commands)} ${t('comandos','commands')}`,`${compact(tools.rtk.saved_tokens)} ${t('poupados','saved')}`))]:[]),
      text(join('Jev',`${int(jev.calls)} ${t('chamadas','calls')}`)),
      ...(Object.keys(jev.by_purpose || {}).length?[label(Object.entries(jev.by_purpose).sort((a,b)=>b[1]-a[1]).map(([purpose,count])=>`${int(count)} ${name(purposes[purpose]) ?? purpose}`).join(', '))]:[]),
      label(join(jev.input_tokens!=null?`${compact(jev.input_tokens)} tokens`:'',jev.cost_micro_usd==null?t('custo não medido','cost not measured'):money(jev.cost_micro_usd))),
      text(join(t('Busca de código','Code search'),`${int(tools.search.total)} ${t('buscas','searches')}`)),
      label(tools.search.in_spec?`${int(tools.search.in_spec)} ${t('nesta spec','in this spec')}`:t('nenhuma nesta spec','none in this spec'))]));
    const limits=Object.fromEntries((usage?.rateLimits || []).map(limit=>[limit.kind,limit]));
    const elapsed=usage?.startedAt!=null?Math.max(0,now-usage.startedAt):null;
    const duration=elapsed==null?'':`${Math.floor(elapsed/3600000)} h ${Math.floor(elapsed/60000)%60} min`;
    const quota=(kind,caption)=>limits[kind]?[text(join(caption,`${percent(limits[kind].percentUsed)}${limits[kind].resetsAt?`, ${t('renova em','resets in')} ${countdown(limits[kind].resetsAt,now)}`:''}`))]:[];
    body.push(block(`${t('Esta sessão','This session')}${duration?` · ${duration}`:''}`,[
      ...(usage?.context?[text(join(t('contexto','context'),`${int(usage.context.tokens)} ${t('de','of')} ${compact(usage.context.window)}`))]:[]),
      ...quota('five_hour',t('cota de 5 h','5 h quota')),...quota('seven_day',t('semana','week')),
      ...(d.session?.agents!=null?[label(join(`${d.session.agents} ${t('agentes observados','agents observed')}`,`${d.session.working ?? 0} ${t('trabalhando','working')}`))]:[])]));
    const disk=d.copies_disk;
    body.push(block(t('Cópias das ondas','Wave copies'),[disk?text(join(`${(disk.bytes/1e9).toLocaleString(locale,{maximumFractionDigits:disk.bytes<1e10?1:0})} GB`,
      `${t('medido em','measured on')} ${day(disk.measured_at)} ${hour(disk.measured_at)}`)):label(t('não medido; use Medir agora','not measured; use Measure now'))]));
    if(terminal)keys.push(measureButton());
    nav('back','v',t('voltar','back'),actions.back);
  }

  // What each wave's agent got and what went wrong, as the sends recorded
  // it: the model it used (measured) or, unmeasured, the one asked; the
  // effort that held at the launch or, unrecorded, the one asked; the
  // skills; the summary it continued. Then the failures.
  function waveAgents(s) {
    const plural=(n,one,many)=>`${int(n)} ${n===1?one:many}`;
    const reasons={no_credit:['sem crédito','no credit'],missing_key:['sem chave','no key'],key_refused:['chave recusada','key refused'],
      key_in_git:['chave no git','key in git'],network:['rede','network'],busy:['ocupado','busy'],refused:['recusado','refused'],
      timeout:['tempo esgotado','timed out'],unreadable:['resposta ilegível','unreadable answer'],too_large:['pergunta grande demais','request too large']};
    const counted=(map,names={})=>Object.entries(map || {}).sort((a,b)=>b[1]-a[1]).map(([key,count])=>`${name(names[key]) ?? key} (${int(count)})`).join(', ');
    const refused=n=>n?plural(n,t('volta recusada','refused return'),t('voltas recusadas','refused returns')):'';
    const rejected=n=>n?plural(n,t('reprovação da revisão','review rejection'),t('reprovações da revisão','review rejections')):'';
    // The effort the send asked; a wave shows the one that held at the launch instead, when the send recorded it.
    const effort=value=>value?`${t('esforço pedido','effort asked')} ${value}`:'';
    const held=value=>`${t('esforço','effort')} ${value}`;
    const reuse=s.reuse || {};
    body.push(block(t('Agentes das ondas','Wave agents'),[
      ...(s.models || []).map(m=>text(join(m.model,effort(m.effort),plural(m.waves,t('onda','wave'),t('ondas','waves'))))),
      ...(s.skills?.length?[label(`${t('skills entregues','skills delivered')}: ${s.skills.map(k=>`${k.name} (${int(k.waves)})`).join(', ')}`)]:[]),
      text(join(plural(reuse.summaries ?? 0,t('onda continuou um resumo','wave continued a summary'),t('ondas continuaram um resumo','waves continued a summary')),
        reuse.knowledge_calls?`${int(reuse.knowledge_cached)} ${t('de','of')} ${int(reuse.knowledge_calls)} ${t('consultas de conhecimento reaproveitadas','knowledge lookups reused')}`:''))]));
    if(s.waves.length)body.push(block(`${t('Por onda','By wave')} · ${t('últimas','last')} ${s.waves.length} ${t('de','of')} ${int(s.waves_sent)} ${t('ondas enviadas','waves sent')}`,
      s.waves.map(w=>{
        const used=w.model_used ?? (w.model_asked?`${w.model_asked} ${t('(pedido)','(asked)')}`:t('modelo não medido','model not measured'));
        const extra=join((w.skills || []).join(', '),w.summary_reused?t('continuou um resumo','continued a summary'):'',refused(w.refused_returns),rejected(w.review_rejections));
        return Box({flexDirection:'column',children:[text(join(`${t('onda','wave')} ${w.wave}`,used,w.effort_used?held(w.effort_used):effort(w.effort_asked),
          w.tokens!=null?`${compact(w.tokens)} tokens`:'')),
          ...(extra?[label(extra)]:[])]});
      })));
    const f=s.failures || {},lines=[];
    if(f.jev?.calls)lines.push(text(`Jev ${t('falhou','failed')} ${plural(f.jev.calls,t('vez','time'),t('vezes','times'))}: ${counted(f.jev.reasons,reasons)}`));
    if(f.hook_blocks?.total)lines.push(text(`${plural(f.hook_blocks.total,t('bloqueio de gancho','hook block'),t('bloqueios de gancho','hook blocks'))}: ${counted(f.hook_blocks.by_hook)}`));
    const returns=join(refused(f.refused_returns),rejected(f.review_rejections));
    if(returns)lines.push(text(returns));
    body.push(block(t('Falhas','Failures'),lines.length?lines:[label(t('Nenhuma falha gravada.','No failure recorded.'))]));
  }

  // The skills Mustard made: the state, how many times each went with a
  // task, the tokens of the wave that made it and the last measured gain.
  function mustardSkills(skills) {
    const states={active:['ativa','active'],discarded:['descartada','discarded'],retired:['retirada','retired']};
    body.push(block(t('Skills do Mustard','Mustard skills'),skills.map(s=>Box({flexDirection:'column',children:[
      text(join(s.name,name(states[s.state]) ?? s.state,`${int(s.tasks)} ${s.tasks===1?t('tarefa','task'):t('tarefas','tasks')}`,
        s.wave_tokens!=null?`${compact(s.wave_tokens)} ${t('tokens na onda que a criou','tokens in the wave that made it')}`:'')),
      ...(s.gain?[label(s.gain)]:[])]}))));
  }

  function measureButton() {
    return keyButton({id:'measure',hotkey:'m',caption:t('medir agora','measure now'),onPress:actions.measure,disabled:measuring && t('Medindo…','Measuring…')});
  }

  function chartSvg(days) {
    const colors=chartColors(theme),max=Math.max(1,...days.map(item=>item.tokens ?? 0)),width=days.length*20,height=80;
    const bars=days.map((item,index)=>item.tokens==null
      ?`<rect x="${index*20+3}" y="${height-14}" width="14" height="12" fill="none" stroke="${colors.empty}" stroke-dasharray="3 2"/>`
      :`<rect x="${index*20+3}" y="${height-2-Math.max(2,item.tokens/max*(height-4))}" width="14" height="${Math.max(2,item.tokens/max*(height-4))}" rx="2" fill="${colors.bar}"/>`).join('');
    return Svg({source:`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="${width}" height="${height}">${bars}</svg>`,
      alt:`${t('Tokens por dia','Tokens per day')}: ${days.map(item=>`${day(item.day)}: ${item.tokens==null?t('sem medida','no measure'):compact(item.tokens)}`).join('; ')}`,width,height});
  }

  // The project line, then the screen, then the keys: in the terminal's
  // footer, or as tabs on top of the desktop.
  const head=[row([text('■',{color:'claude',bold:true}),text(project?.name || 'Mustard',{bold:true})]),label(project?.branch || t('sem branch','no branch'))];
  if(terminal) {
    nav('quit','q',t('sair','quit'),actions.close);
    const width=Math.max(12,Math.min(46,(columns || 48)-2));
    return Box({flexDirection:'column',rowGap:1,paddingX:1,children:[Box({flexDirection:'column',children:head}),...body,
      Box({flexDirection:'column',children:[label('─'.repeat(width)),row(keys,{columnGap:4})]})]});
  }
  const tab=(id,hotkey,target)=>press(id,name(titles[target]),()=>actions.go(target),{...(hotkey?{hotkey}:{}),...(screen===target?{variant:'primary'}:{})});
  const back=state_.back.at(-1);
  const top=[row([Box({flexDirection:'column',flexGrow:1,children:head}),press('refresh','↻',actions.refresh),
      press('go-settings','⚙',()=>actions.go('settings')),press('quit','✕',actions.close,{hotkey:'q'})],{alignItems:'center'}),
    row([tab('go-overview',undefined,'overview'),tab('go-spec','s','spec'),tab('go-usage','c','usage')],{columnGap:2}),
    ...(screen!=='overview' && back?[press('back',`← ${name(titles[back.screen]) ?? ''}`,actions.back,{hotkey:'v'})]:[])];
  return Box({flexDirection:'column',rowGap:1,padding:1,children:[...top,...body]});
}
