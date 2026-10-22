import {test,expect,mock} from 'claude-code/testing';

// The answers of `mustard-rt run panel --section ...` with the example data
// of the approved layout (the six screens of the layout-unico canvas), in
// the shape the binary gives them. Where the layout shows a figure the binary
// does not send, the answer goes without it. The pages after the first, the
// search and the copies' disk once measured are the only parts added.
const project={name:'mustard',branch:'fix/auditoria-spec-e-ondas',version:'0.2.24',language:'pt-BR'};
const NEXT='Encerrar a onda 193 e entregar a spec';
const overview={ok:true,schema_version:1,section:'overview',revision:'rev-overview',at:'2026-10-21T11:00:00Z',project,selected_spec:'ajuste-na-spec-fechada',
  spec:{name:'ajuste-na-spec-fechada',goal:'Spec fechada aceita pedido novo na mesma branch.',phase:'running',tasks:{total:584,finished:584,backlog:0},
    criteria:{total:42,passed:41,failed:0,without_proof:1},waves:{finished:366,planned:367},
    next:{command:'mustard-rt run round --dispatch --spec ajuste-na-spec-fechada',text:NEXT},
    air:{running:0,no_agent:[193],free_slots:5,capacity:5,backlog:0}},
  // The binary sends the stopped wave first; the screen orders them as the layout does.
  warnings:[{kind:'wave-no-agent',waves:[193]},{kind:'pr-merged',pr:289,at:'2026-10-05T08:22:49-03:00',phase:'running'},
    {kind:'review-reopened',approved_at:'2026-10-02T14:09:35-03:00',reopened_at:'2026-10-02T17:13:51-03:00'}],
  others:{total:14,attention:2,shown:[{name:'falha-fora-da-obra',phase:'running',updated:'2026-10-21T01:38:29-03:00',warning:{kind:'pr-merged',pr:326,at:'2026-10-20T16:52:14-03:00'}},
    {name:'mustard-enxuto',phase:'pr_open',updated:'2026-09-17T19:21:15-03:00',warning:{kind:'pr-merged',pr:277,at:'2026-09-18T09:23:52-03:00'}}]},
  deliveries:{total:8,shown:[]},
  usage:{cost_micro_usd:1499270000,sessions:38,tokens:21904658267,measured_through:'2026-10-13',session_agents:1}};
// The project summary the layout does not show, for the test of its block.
const deliveries={total:12,shown:[{spec:'falha-fora-da-obra',delivered_at:'2026-10-17',pr:326,title:'Recibo nomeia o programa atrás do plugin',
  delivered:'O recibo diz qual programa respondeu atrás do plugin.',rules:2},
  {spec:'mustard-enxuto',delivered_at:'2026-09-17',pr:null,title:'Mustard mais enxuto',delivered:'Tira o que perdeu o sentido.',rules:0}]};
// A spec still in its survey, as the binary answers it: nothing counted yet.
const survey={...overview,selected_spec:'painel-mods-igual-ao-desenho',warnings:[],others:{total:14,attention:0,shown:[]},deliveries:{total:0,shown:[]},
  spec:{name:'painel-mods-igual-ao-desenho',goal:'Refazer o painel mods igual ao desenho aprovado.',phase:'survey',tasks:{total:0,finished:0,backlog:0},
    criteria:{total:0,passed:0,failed:0,without_proof:0},waves:{finished:0,planned:0},
    next:{command:'mustard-rt run grill',text:'Seguir o levantamento e responder os pontos em aberto'},air:{running:0,no_agent:[],free_slots:5,capacity:5,backlog:0}}};
const specs=[['falha-fora-da-obra','running','2026-10-14'],['ajuste-na-spec-fechada','running','2026-09-24'],['mustard-enxuto','pr_open','2026-09-12'],
  ['validacao-leve-por-trecho','survey','2026-10-07'],['nova-versao','survey','2026-10-05'],
  ...Array.from({length:8},(_,n)=>[`entregue-${n+1}`,'delivered',`2026-09-0${n+1}`]),['descartada-1','discarded','2026-09-20']]
  .map(([name,phase,created])=>({name,phase,created:`${created}T10:00:00-03:00`}));
const spec={ok:true,schema_version:1,section:'spec',revision:'rev-spec',project,selected_spec:'ajuste-na-spec-fechada',
  spec:{name:'ajuste-na-spec-fechada',goal:'Spec fechada aceita pedido novo na mesma branch.',phase:'running',branch:'fix/ajuste-na-spec-fechada',base:'dev',
    opened_at:'2026-09-24T13:50:59-03:00',last_commit:{at:'2026-10-05T07:51:49-03:00',sha:'209a1a26'},commits:303,
    counts:{tasks:{total:584,finished:584,backlog:0},criteria:{total:42,passed:41,failed:0,without_proof:1},questions:{total:44,open:22},decisions:154,rules:66,waves:367},
    waves:{planned:367,finished:366,committed:361,integrated:5,running:0,todo:0,no_agent:[193],rejected:[]},
    review:{valid:false,approved_at:'2026-10-02T14:09:35-03:00',reopened_at:'2026-10-02T17:13:51-03:00'},pr:{number:289,merged_at:'2026-10-05T08:22:49-03:00',source:'local-git'}},
  specs,discarded:1};
const task=(n:number,title:string,state:string,extra:any={})=>({code:`MSTD-TASK-${String(n).padStart(4,'0')}`,title,wave:n,state,at:null,files:null,covers:null,
  resends:0,unmet:0,attention:null,...extra});
const stopped=task(193,'Arquivo novo ou apagado: receita diferente no comando e no pedido','no-agent',{resends:2,attention:'wave-no-agent'});
const board=[stopped,task(1,'Reabertura de spec fechada volta à execução','committed',{at:'2026-09-24T16:17:30-03:00',files:10,covers:14}),
  task(81,'A ligação nova de imports medida nos projetos da prova','integrated'),
  task(133,'A mesma pergunta em português e em inglês acha o mesmo código','integrated',{unmet:1}),
  task(254,'O veredito gravado durante o fechamento não se perde mais','committed',{at:'2026-09-30T10:00:00-03:00',files:2}),
  task(260,'Medir se notas de sentido melhoram a busca do scan','integrated',{files:0}),
  task(400,'Decisões antigas ainda citam o comando de medida','integrated',{files:0}),
  task(479,'Quatro testes deixam de depender do sistema em que rodam','committed',{at:'2026-10-05T10:00:00-03:00',files:4})];
function items(argv:string[]) {
  const at=(flag:string)=>argv.includes(flag)?argv[argv.indexOf(flag)+1]:undefined;
  const offset=Number(at('--offset') ?? 0),limit=Number(at('--limit') ?? 8),filter=at('--filter') ?? 'all',query=at('--query');
  const all=query?[task(7,'Extra gamma','committed')]:filter==='attention'?[stopped]
    :[...board,...Array.from({length:30},(_,n)=>task(500+n,`Tarefa ${500+n}`,'committed',{at:'2026-10-06T10:00:00-03:00',files:1}))];
  return {ok:true,schema_version:1,section:'items',revision:`rev-items-${filter}-${query}-${offset}-${limit}`,project,selected_spec:'ajuste-na-spec-fechada',kind:at('--kind') ?? 'task',filter,query:query ?? null,
    counts:{task:584,criterion:42,decision:154,rule:66,question:44},filters:{all:584,attention:1,committed:575,uncommitted:9},
    total:query?1:filter==='attention'?1:584,offset,limit,rows:all.slice(offset,offset+limit)};
}
const item={ok:true,schema_version:1,section:'item',revision:'rev-item',project,selected_spec:'ajuste-na-spec-fechada',item:{code:'MSTD-TASK-0133',kind:'task',
  title:'A mesma pergunta em português e em inglês acha o mesmo código',wave:133,state:'integrated',at:'2026-09-28T06:38:00-03:00',
  request:{text:'A pergunta em português deve achar o mesmo código que a pergunta em inglês. Exemplo: a validação do documento do cliente, cujo código se chama `validateDocument`.',cut:false},
  done:{text:'Mediu nos três projetos da prova, nas duas línguas, sem mudar o produto e sem gastar Jev. Entregou um relatório, sem commit.',cut:false},
  result:{unmet:1,agreed:[{item:'MSTD-RULE-0012',kind:'rule',title:'O scan e a busca funcionam em qualquer idioma do Mustard',met:false}]},
  done_when:'A mesma pergunta nas duas línguas acha o mesmo código.',files:['.claude/plans/poc-busca/medida-duas-linguas.md'],delivered_files:1,
  covers:[{code:'MSTD-RULE-0012',kind:'rule',title:'O scan e a busca funcionam em qualquer idioma do Mustard'}],wave_tokens:5040000,resends:0,tests:[],review:null,wave_request:null,
  // The layout names who made only the newest version.
  history:[{at:'2026-09-26T21:57:00-03:00',wave:null,changed:null},{at:'2026-09-26T22:00:00-03:00',wave:null,changed:{depends_on:{added:['MSTD-TASK-0132']},files:{added:['a.md']}}},
    {at:'2026-09-28T06:20:00-03:00',author:'round',wave:133,changed:{wave:{from:null,to:133}}}]}};
// What the task screen shows after the layout's blocks: the tests, the wave
// reviewer's verdict and the wave's whole request.
const extras={tests:[{criterion:'MSTD-CRIT-0007',title:'A mesma pergunta acha o mesmo código nas duas línguas.',proof:'cargo test -p mustard-core -- bilingual',result:'pass',at:'2026-09-28T07:00:00-03:00',ms:5990}],
  review:{result:'rejected',reason:'Falta a medida no projeto C#.',files:['packages/core/src/search/tests.rs'],at:'2026-09-28T06:30:00-03:00'},
  wave_request:'# ajuste-na-spec-fechada — onda 133\n\nRode de dentro da cópia.\n'+'Linha do pedido da onda. '.repeat(200)+'\nÚltima linha do pedido.'};
const days=['30','01','02','03','04','05','06','07','08','09','10','11','12','13'];
const usage={ok:true,schema_version:1,section:'usage',revision:'rev-usage',project,selected_spec:'ajuste-na-spec-fechada',measured_through:'2026-10-13',measured_at:'2026-10-15T01:54:12Z',
  available:true,period:{first:'2026-08-29',last:'2026-10-13'},project_tokens:21904658267,machine_tokens:27063778343,
  trend:{day:'2026-10-14',tokens:807800555,average_tokens:178094204,ratio:4.5},
  days:[886229689,461112921,554397719,675064976,781248087,605012735,312215102,57772003,null,null,2328619,null,null,null]
    .map((tokens,n)=>({day:`2026-${n?'10':'09'}-${days[n]}`,tokens})),
  cost:{total_micro_usd:1499270000,sessions:38,outside_spec_micro_usd:299580000,by_spec:[{name:'falha-fora-da-obra',micro_usd:1199700000}],specs_without_cost:12},
  spec:{name:'ajuste-na-spec-fechada',wave_tokens:3790000000,waves_without_measure:25,conductor_tokens:2140000000},
  tools:{rtk:{configured:true,status:'measured',commands:54112,saved_tokens:71600000,percent:68.9},
    jev:{calls:1083,by_purpose:{'wave-planning':853,context:208,search:22},input_tokens:5400000,cost_micro_usd:null},search:{total:8732,in_spec:0}},
  session:{started_at:null,agents:16,working:1},copies_disk:null,mustard_skills:[]};
// What the usage screen shows after the layout's blocks: each wave's agent and the skills Mustard made.
const agents={waves_sent:367,waves:[
    {wave:367,model_used:'claude-opus-5-5',model_asked:'opus',effort_asked:'xhigh',effort_used:'high',skills:['add-run-command'],summary_reused:true,tokens:220008,refused_returns:2,review_rejections:1},
    {wave:366,model_used:null,model_asked:'opus',effort_asked:'xhigh',effort_used:null,skills:[],summary_reused:false,tokens:null,refused_returns:0,review_rejections:0}],
  models:[{model:'claude-opus-5-5',effort:'xhigh',waves:340}],skills:[{name:'add-run-command',waves:12}],reuse:{summaries:30,knowledge_calls:10,knowledge_cached:4},
  failures:{jev:{calls:3,reasons:{no_credit:3}},hook_blocks:{total:5,by_hook:{command_guard:5}},refused_returns:9,review_rejections:2}};
// The waves screen of the approved model of the waves, page by page: the
// first page with the model's own waves, the others committed, 208 in all.
const wave=(n:number,title:string,extra:any={})=>({wave:n,title,state:'committed',at:'2026-10-16T12:00:00-03:00',attention:null,tasks:1,returned:0,files:1,delivered_files:1,
  commit:'9b91cd2a0e',seconds:3900,model_used:'claude-opus-5-5',model_asked:'opus',effort_asked:'xhigh',effort_used:null,tokens:1000000,...extra});
const stoppedWave={state:'no-agent',at:'2026-10-17T15:03:00-03:00',attention:'wave-no-agent',delivered_files:null,commit:null,seconds:null,model_used:null,tokens:null};
const modelWaves=[wave(208,'A onda lê cada item só com o título e a parte do agente',{...stoppedWave,files:2}),
  wave(207,'Busca única na spec por tipo, código ou texto, com o Jev na dúvida',{...stoppedWave,files:13}),
  wave(206,'Recibo da instalação nomeia o programa atrás do plugin',{at:'2026-10-17T10:00:00-03:00',commit:'15db4cd7aa',seconds:660,tokens:1090000}),
  wave(205,'Recibo de prova guarda o resultado do cargo mesmo atrás do pipe',{tasks:6,returned:5,files:10,delivered_files:6,tokens:8850000}),
  wave(204,'Entrega recusa prova verde mais velha que o arquivo entregue',{tasks:6,returned:5,files:9,delivered_files:4,tokens:6150000}),
  wave(203,'Retomada lista o que a rodada fez desde a última mensagem',{tasks:3,returned:2,files:4,delivered_files:3,tokens:6290000}),
  wave(202,'Busca explícita responde sem esperar a trava da passada',{tasks:3,returned:1,files:8,delivered_files:4,tokens:7500000}),
  wave(201,'Rodada parte a pergunta ao Jev em lotes que cabem no teto',{tasks:5,returned:3,files:20,delivered_files:5,tokens:9400000}),
  ...Array.from({length:200},(_,n)=>wave(200-n,`Onda de exemplo ${200-n}`))];
function waves(argv:string[]) {
  const at=(flag:string)=>argv.includes(flag)?argv[argv.indexOf(flag)+1]:undefined;
  const offset=Number(at('--offset') ?? 0),limit=Number(at('--limit') ?? 8);
  return {ok:true,schema_version:1,section:'waves',revision:`rev-waves-${offset}-${limit}`,project,selected_spec:'ajuste-na-spec-fechada',
    total:modelWaves.length,offset,limit,rows:modelWaves.slice(offset,offset+limit)};
}
const skills=[{name:'falta-teste',state:'active',tasks:12,gain:'Acertou 3 de 3 com a skill.',wave_tokens:220008},{name:'parada',state:'retired',tasks:0,gain:null,wave_tokens:null}];
// As the binary answers a project with no spec search choice saved: off.
const settingsValues={enabled:true,rtk:true,vectors:true,jev:true,agent_model:'opus',agent_effort:'xhigh',search_answer:true,session_claude_model:'haiku',
  session_codex_model:'gpt-6-luna',jev_search:true,jev_wave_planning:true,jev_context:true,jev_spec_search:false,max_compiling_waves:5,review_model:'opus',
  review_effort:'xhigh',cloudflare_account:'[ID DA CONTA]',cloudflare_project:'[NOME DO PROJETO]',cloudflare_token:null};

const SURFACES=['terminal','desktop'] as const;
const TOKEN='synthetic-cloudflare-token-0001';
const NO_TOKEN='Grave o token em publication.apiToken do mustard.json, ou forneça CLOUDFLARE_API_TOKEN no ambiente, que vence. Os arquivos continuam locais.';

type Options={theme?:string,conflict?:boolean,review?:any,task?:any,columns?:number,early?:boolean,patch?:Record<string,any>};
async function open($,on,surface,{theme='dark',conflict=false,review,task,columns=100,early=false,patch={}}:Options={}) {
  const clock=mock.clock(on);mock.env(on,{});mock.store(on,{});
  const commands:string[][]=[],patches:any[]=[],during:any[]=[];
  let mounted:any,asked:any;
  let tokenSaved=true,measured=false;
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));on('command.register',()=>({value:null}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('session.cwd',()=>({value:'/fixture'}));on('session.id',()=>({value:'fixture-session'}));
  on('session.model',()=>({value:'fixture-model'}));
  on('session.usage',()=>({value:{startedAt:0,context:{tokens:499669,window:1000000,percent:50},
    rateLimits:[{kind:'five_hour',percentUsed:7,resetsAt:'2026-10-21T14:30:00Z'},{kind:'seven_day',percentUsed:33,resetsAt:'2026-10-22T12:00:00Z'}]}}));
  on('config.list',()=>({value:[{key:'theme',label:'Theme',kind:'choice',value:theme,provider:{kind:'engine'},isLocked:false}]}));
  on('ui.open',($,e)=>{asked=e;return {value:null};});on('ui.close',()=>({value:null}));on('tool.call',()=>({result:{text:'fixture read'}}));
  on('prompt.submit',()=>{throw new Error('the panel never starts a model turn');});
  on('turn.complete',()=>({text:''}));on('classic.SubagentStop',()=>({}));
  on('process.run',async ($,e)=>{
    commands.push(e.argv);
    const at=(flag:string)=>e.argv.includes(flag)?e.argv[e.argv.indexOf(flag)+1]:undefined;
    let value:any;
    if(e.argv[2]==='settings') {
      // The patch, with the token in it, comes only on standard input.
      const change=e.argv.includes('--stdin')?JSON.parse(e.init.stdin):undefined;
      if(change){patches.push(change);if(change.changes.cloudflare_token)tokenSaved=true;}
      if(change && conflict)value={ok:false,schema_version:1,reason:'settings-conflict'};
      else value={ok:true,schema_version:1,revision:`settings-${patches.length}`,values:{...settingsValues,...(change?.changes ?? {}),cloudflare_token:null},
        publication:{token_saved:tokenSaved,mustard_json_out_of_git:true},
        jev:{fallback_enabled:true,usable_key:true,purposes:{'spec-search':{enabled:false,reason:'purpose-not-configured',chosen:false}}},rtk:{own_hook_left:false},index_present:true,...(change?{saved:true}:{})};
    } else if(e.argv[2]==='panel') {
      const section=at('--section')!;
      // What the pane shows while the measure runs.
      if(e.argv.includes('--refresh-consumption')){during.push({button:await mounted?.find({key:'measure'}),note:await mounted?.find({text:/Medindo/})});measured=true;}
      value=section==='overview'?early?survey:overview:section==='spec'?spec:section==='items'?items(e.argv):section==='item'?{...item,item:{...item.item,...(review?{review}:{}),...task}}
        :section==='usage'?{...usage,copies_disk:measured?{bytes:126000000000,measured_at:'2026-10-18T13:00:00Z'}:null}:section==='waves'?waves(e.argv):{ok:false,reason:'panel-invalid-section'};
      if(patch[section])value={...value,...patch[section]};
      if(at('--if-revision')===value.revision)value={ok:true,schema_version:1,unchanged:true,revision:value.revision,section};
    } else if(e.argv[2]==='publish') {
      // What the binary answers without a Cloudflare token: nothing leaves.
      during.push({publishing:await mounted?.find({text:/Publicando/})});
      value={ok:true,scope:'project',published:false,transport_required:true,reason:'publication-token-missing',hint:NO_TOKEN,
        snapshot_id:'project-snapshot',page:'/fixture/.claude/mustard/publications/project/index.html',database:'/fixture/.claude/mustard/publications/project/snapshot.json'};
    } else if(['usage-record','panel-diagnostics'].includes(e.argv[2]))value={ok:true};
    else throw new Error('unexpected command '+e.argv.join(' '));
    return {value:{exitCode:0,stdout:JSON.stringify(value),stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface,isInteractive:true});
  await $.command.run({command:'mustard:mods',args:''});
  const props={id:'mustard-panel',title:'Mustard',bodyColumns:columns,scroll:{bodyRows:40}};
  const pane=await $.ui.mount({plugin:'mustard',surface,component:'Pane',requestId:'mustard-panel',props});
  mounted=pane;
  const reads=()=>commands.filter(argv=>argv[2]==='panel');
  // Fecha o painel como a pessoa. O terminal não desenha o fechar: ele fecha
  // pela marca do painel e pelo Esc, que o kit não aperta; o fechar é o mesmo
  // nas duas superfícies, então no terminal o kit aperta o do desktop.
  const close=async ()=>{const desk=surface==='desktop'?pane:await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',requestId:'mustard-panel',props});
    await desk.press({key:'quit'});};
  // The rows and columns the panel asked for when it opened.
  return {pane,clock,commands,patches,during,reads,close,asked:()=>asked,section:()=>{const last=reads().at(-1)!;return last[last.indexOf('--section')+1];}};
}
const has=async (pane,text)=>expect(await pane.find({text}),`${text}`).toBeDefined();
const lacks=async (pane,text)=>expect(await pane.find({text}),`${text}`).toBeUndefined();
const walk=(node:any,visit:(node:any)=>void)=>{if(Array.isArray(node))node.forEach(child=>walk(child,visit));
  else if(node && typeof node==='object'){visit(node);for(const value of Object.values(node))walk(value,visit);}};
// Every string the person reads: the text, the button captions, the field
// labels and hints, the drawings' descriptions. Keys are not read.
const shown=(tree:any)=>{const out:string[]=[];
  walk(tree,node=>{for(const prop of ['children','label','placeholder','alt','value'])for(const value of [node[prop]].flat())if(typeof value==='string')out.push(value);});return out;};
const CODE=/\b[A-Z]{2,}-[A-Z]+-\d{3,}\b/;

// A paleta do layout, que é a do tema escuro nas duas superfícies: toda cor
// que a árvore pinta vem dela. A clara é a variante.
const DARK={page:'#0B0B0A',header:'#0F0F0E',card:'#141413',box:'#1C1B19',line:'#2A2926',dashed:'#3A3834',empty:'#4A4843',ink:'#F5F3EC',soft:'#A9A69B',muted:'#77746B',
  mustard:'#F2B705',accent:'#FFC933',deep:'#8A6D0A',buttonInk:'#1A1500',tint:'#26200B',tintLine:'#5A4811',warning:'#F5A15C',warningSoft:'#26190F',warningLine:'#58391F',
  warningInk:'#F9C79B',attention:'#221810',success:'#5BCB8A'};
const LIGHT={page:'#FAFAF7',header:'#FFFFFF',box:'#F5F3EC',line:'#E6E4DC',track:'#F0EEE7',dashed:'#D9D6CC',empty:'#CFCBBF',ink:'#2B2B29',soft:'#6B6A64',muted:'#8A877D',
  mustard:'#E1AD01',accent:'#8A6700',deep:'#EFD27A',buttonInk:'#1A1500',tint:'#FBF1CF',warning:'#B4570B',warningSoft:'#FCEFE3',warningLine:'#F0C9A4',warningInk:'#7A3B07',
  attention:'#FFF8F1',success:'#2F7D4F'};
// Toda cor que a árvore pinta, as dos desenhos Svg incluídas.
const painted=(tree:any)=>{const out:string[]=[];
  walk(tree,node=>{for(const prop of ['color','backgroundColor','borderColor'])if(typeof node[prop]==='string')out.push(node[prop]);
    if(typeof node.source==='string')out.push(...(node.source.match(/#[0-9A-Fa-f]{6}\b/g) ?? []));});return out;};
const outside=(tree:any,palette:Record<string,string>)=>[...new Set(painted(tree))].filter(color=>!Object.values(palette).includes(color));
// O estilo do primeiro texto que mostra exatamente isso.
async function style(pane:any,value:string|RegExp) {
  const found=(await pane.findAll({type:'Text'})).find((node:any)=>typeof value==='string'?node.text===value:value.test(node.text));
  expect(found,`${value}`).toBeDefined();return found?.props ?? {};
}
// Os títulos dos blocos como o layout os desenha: pequenos, em seminegrito, em maiúsculas e no texto terciário.
const headings=async (pane:any)=>(await pane.findAll({type:'Text'})).filter((node:any)=>node.props.size==='caption' && node.props.weight==='semibold' && node.props.color===DARK.muted
  && /\p{Lu}/u.test(node.text) && node.text===node.text.toLocaleUpperCase('pt-BR')).map((node:any)=>node.text);
const svgs=async (pane:any)=>(await pane.findAll({type:'Svg'})).map((node:any)=>node.props);
const boxes=async (pane:any,props:Record<string,unknown>)=>(await pane.findAll({type:'Box'})).filter((node:any)=>Object.entries(props).every(([key,value])=>node.props[key]===value));
const texts=async (pane:any)=>(await pane.findAll({type:'Text'})).map((node:any)=>node.text);
// Cada valor aparece depois do anterior, na ordem em que a tela os mostra.
function inOrder(values:string[],expected:(string|RegExp)[]) {
  let at=-1;
  for(const want of expected) {
    const next=values.findIndex((value,n)=>n>at && (typeof want==='string'?value===want:want.test(value)));
    expect(next,`${want} depois de ${values[at] ?? 'o começo'}`).toBeGreaterThan(at);at=next;
  }
}
// Um texto que o terminal parte nas pausas dele: uma linha de textos, sem
// espaço entre eles, em que cada um, menos o último, leva o espaço do fim.
const phrase=(node:any)=>{if(node?.type!=='Box' || (node.props?.flexDirection ?? 'row')!=='row' || cells(node.props?.columnGap,false)!==0)return undefined;const kids=kidsOf(node);
  if(kids.length<2 || !kids.every((kid:any)=>kid?.type==='Text'))return undefined;
  const ends=kids.every((kid:any,n:number)=>n===kids.length-1 || inline(kid).endsWith(' ')),starts=kids.every((kid:any,n:number)=>!n || inline(kid).startsWith(' '));
  return ends || starts?kids.map(inline).join('').trim():undefined;};
// O primeiro pedaço de um texto que o terminal parte nas pausas, com o espaço do fim; no desktop, o texto inteiro.
const first=(surface:string,value:string)=>{const parts=value.split(/(?<=[,.·]) (?=\S)/);return surface==='terminal' && parts.length>1?`${parts[0]} `:value;};
// O que a tela mostra, na ordem, sem os espaços das pontas; o texto partido nas pausas, inteiro.
const reading=(tree:any)=>{const out:string[]=[];
  const go=(node:any)=>{if(Array.isArray(node))return node.forEach(go);if(!node || typeof node!=='object')return;
    const whole=phrase(node);if(whole!=null){out.push(whole);return;}
    for(const prop of ['children','label','placeholder','alt','value'])for(const value of [node[prop]].flat())if(typeof value==='string' && value.trim())out.push(value.trim());
    for(const value of Object.values(node))go(value);};
  go(tree);return out;};
// O texto de cada Text da árvore, com o que ele aninha, sem os espaços das pontas; o partido nas pausas, inteiro.
const textsOf=(tree:any)=>{const out:string[]=[];
  const go=(node:any)=>{if(Array.isArray(node))return node.forEach(go);if(!node || typeof node!=='object')return;
    const whole=phrase(node);if(whole!=null){out.push(whole);return;}
    if(node.type==='Text')out.push(inline(node).trim());for(const value of Object.values(node))go(value);};
  go(tree);return out.filter(Boolean);};
// Os nós de um tipo, na ordem em que a tela os desenha.
const nodesOf=(tree:any,type:string)=>{const out:any[]=[];walk(tree,node=>{if(node.type===type)out.push(node);});return out;};
// Os títulos dos blocos no terminal: em maiúsculas, em negrito e no texto terciário.
const termHeadings=(tree:any)=>nodesOf(tree,'Text').filter((node:any)=>node.props?.bold && node.props.color===DARK.muted && /\p{Lu}/u.test(inline(node))
  && inline(node)===inline(node).toLocaleUpperCase('pt-BR')).map((node:any)=>inline(node));

// A tela do terminal como as linhas que o Ink desenha na largura do painel:
// uma coluna empilha os filhos com rowGap linhas em branco entre eles; uma
// linha os põe lado a lado com columnGap colunas entre eles, cada um na
// largura dele (o número, a porcentagem da linha ou o que o conteúdo pede),
// a caixa que cresce leva o que sobra, a que passa encolhe, e flexWrap desce
// o que não cabe; o texto quebra nas palavras na largura que lhe resta. A
// borda toma uma coluna de cada lado e uma linha em cima e embaixo, e a
// altura corta o que passa dela. Um degrau vale as células que a documentação dá.
const STEPS:Record<string,[number,number]>={xs:[1,0],sm:[1,0],md:[2,1],lg:[3,1],xl:[4,2]};
const cells=(value:any,down:boolean)=>typeof value==='number'?value:typeof value==='string'?STEPS[value]?.[down?1:0] ?? 0:0;
const inline=(node:any):string=>node==null || node===false?'':typeof node==='string' || typeof node==='number'?String(node)
  :Array.isArray(node)?node.map(inline).join(''):node.type==='Text'?inline(node.children):'';
const kidsOf=(node:any)=>[node.children].flat(Infinity).filter((kid:any)=>kid!=null && kid!==false && kid!=='');
const widest=(lines:string[])=>Math.max(0,...lines.map(line=>line.length));
// O texto quebrado nas palavras, com o recuo e os espaços dele; a palavra mais
// larga que a linha, cortada nela, e anotada em `split`.
const split:string[]=[];
// O que passa da largura da linha mesmo depois de encolher o que encolhe.
const spill:string[]=[];
function wrapped(value:string,width:number):string[] {
  return value.split('\n').flatMap(paragraph=>{
    if(width<=0 || paragraph.length<=width)return [paragraph];
    const lead=paragraph.match(/^ */)![0],words=paragraph.slice(lead.length).split(/( +)/),out:string[]=[];
    let line=lead,empty=true;
    for(let n=0;n<words.length;n+=2) {
      const word=words[n],gap=n?words[n-1]:'';
      if(empty){line+=word;empty=false;}
      else if((line+gap+word).length<=width)line+=gap+word;
      else {out.push(line);line=word;}
      while(line.length>width){if(!/^(.)\1+$/u.test(line))split.push(`${line} em ${width}`);out.push(line.slice(0,width));line=line.slice(width);}
    }
    out.push(line);
    return out;
  });
}
// O que uma folha desenha: o texto; o botão sem moldura, com a tecla e os dois-pontos antes do rótulo.
function leaf(node:any):string {
  const p=node.props ?? {};
  if(node.type==='Text')return inline(node.children);
  if(node.type==='Button'){const caption=kidsOf(node).length?inline(node.children):p.label ?? '';return p.plain?p.hotkey?`${p.hotkey}: ${caption}`:caption:`[ ${caption} ]`;}
  if(node.type==='Select')return `${p.label?`${p.label}: `:''}${(p.options ?? []).find((option:any)=>option.value===p.value)?.label ?? p.value ?? ''} ▾`;
  if(node.type==='Input')return `${p.label?`${p.label}: `:''}${p.value || p.placeholder || ''}`;
  return '';
}
// A largura que a própria caixa pede: o número, ou a porcentagem da que ela tem.
const sized=(own:any,room:number)=>typeof own.width==='number'?own.width:typeof own.width==='string' && own.width.endsWith('%')?Math.floor(parseFloat(own.width)*room/100):undefined;
const edgeOf=(p:any)=>p.borderStyle?1:0;
const sideOf=(p:any,one:string,axis:string,down:boolean)=>cells(p[one] ?? p[axis] ?? p.padding,down);
// O que o conteúdo pede sem quebrar: o texto inteiro numa linha, as linhas lado a lado.
function natural(node:any):number {
  if(node==null || node===false)return 0;
  if(typeof node==='string' || typeof node==='number')return widest(String(node).split('\n'));
  if(node.type!=='Box')return widest(leaf(node).split('\n'));
  const p=node.props ?? {};
  if(typeof p.width==='number')return p.width;
  const kids=kidsOf(node),gap=cells(p.columnGap ?? p.gap,false);
  const inner=p.flexDirection==='column'?Math.max(0,...kids.map(natural)):kids.reduce((sum:number,kid:any)=>sum+(sized(kid?.props ?? {},0)===0?0:natural(kid)),0)+gap*Math.max(0,kids.length-1);
  return inner+sideOf(p,'paddingLeft','paddingX',false)+sideOf(p,'paddingRight','paddingX',false)+2*edgeOf(p);
}
// Uma caixa esticada pela linha em que está ganha as linhas que faltam na
// caixa de dentro que cresce, como o Ink faz; sem uma que cresça, no fim.
function drawnLines(node:any,room=100,tall=0):string[] {
  if(node==null || node===false)return [];
  if(typeof node==='string' || typeof node==='number')return wrapped(String(node),room);
  if(node.type!=='Box')return wrapped(leaf(node),room);
  const p=node.props ?? {},edge=edgeOf(p);
  const left=sideOf(p,'paddingLeft','paddingX',false),right=sideOf(p,'paddingRight','paddingX',false);
  const above=sideOf(p,'paddingTop','paddingY',true),below=sideOf(p,'paddingBottom','paddingY',true);
  const inner=Math.max(1,room-left-right-2*edge),kids=kidsOf(node);
  let lines:string[];
  if(p.flexDirection==='column') {
    const parts=kids.map((kid:any)=>drawnLines(kid,sized(kid?.props ?? {},inner) ?? inner));
    const missing=tall-above-below-2*edge-parts.reduce((sum:number,part:string[])=>sum+part.length,0)-cells(p.rowGap ?? p.gap,true)*Math.max(0,kids.length-1);
    const grows=kids.findIndex((kid:any)=>kid?.type==='Box' && kid.props?.flexGrow>0);
    if(missing>0 && grows>=0)parts[grows]=[...parts[grows],...Array(missing).fill('')];
    lines=parts.flatMap((part:string[],n:number)=>[...(n?Array(cells(p.rowGap ?? p.gap,true)).fill(''):[]),...part]);
  } else lines=rowLines(kids,p,inner);
  lines=[...Array(above).fill(''),...lines,...Array(below).fill('')].map(line=>' '.repeat(left)+line.padEnd(inner).slice(0,inner)+' '.repeat(right));
  if(edge) {
    const [across,down]=p.borderStyle==='dashed'?['╌','╎']:['─','│'];
    lines=[`╭${across.repeat(room-2)}╮`,...lines.map(line=>`${down}${line}${down}`),`╰${across.repeat(room-2)}╯`];
  }
  if(typeof p.height==='number')lines=Array.from({length:p.height},(_,n)=>lines[n] ?? '');
  return lines.map(line=>line.padEnd(room).slice(0,room));
}
function rowLines(kids:any[],p:any,room:number):string[] {
  const gap=cells(p.columnGap ?? p.gap,false);
  const items=kids.map((kid:any)=>{const own=kid?.props ?? {},fixed=sized(own,room);
    return {kid,grow:kid?.type==='Box'?own.flexGrow ?? 0:0,shrink:(own.flexShrink ?? 1)>0,base:fixed ?? natural(kid)};});
  const groups:any[][]=[];
  for(const item of items) {
    const group=groups.at(-1);
    if(group && (p.flexWrap!=='wrap' || group.reduce((sum,other)=>sum+other.base+gap,0)+item.base<=room))group.push(item);else groups.push([item]);
  }
  return groups.flatMap(group=>{
    const widths=group.map(item=>item.base),growth=group.reduce((sum,item)=>sum+item.grow,0);
    const extra=room-widths.reduce((sum,width)=>sum+width,0)-gap*(group.length-1);
    let lead=0,between=gap;
    if(extra>0 && growth){let left=extra;group.forEach((item,n)=>{if(!item.grow)return;const more=Math.floor(extra*item.grow/growth);widths[n]+=more;left-=more;});
      const last=group.map(item=>item.grow>0).lastIndexOf(true);widths[last]+=left;}
    else if(extra>0)({'flex-end':()=>{lead=extra;},center:()=>{lead=Math.floor(extra/2);},'space-between':()=>{if(group.length>1)between=gap+Math.floor(extra/(group.length-1));}} as any)[p.justifyContent]?.();
    let over=-extra;
    for(const n of group.map((_,n)=>n).filter(n=>group[n].shrink).sort((a,b)=>widths[b]-widths[a])){if(over<=0)break;const cut=Math.min(over,widths[n]-1);widths[n]-=cut;over-=cut;}
    if(over>0)spill.push(`${over} colunas: ${group.map(item=>reading(item.kid)[0]).join(' | ')}`);
    let parts=group.map((item,n)=>drawnLines(item.kid,Math.max(1,widths[n])));
    const tallest=Math.max(0,...parts.map(part=>part.length));
    if(!p.alignItems || p.alignItems==='stretch')parts=group.map((item,n)=>item.kid?.type==='Box' && item.kid.props?.height==null && parts[n].length<tallest?drawnLines(item.kid,Math.max(1,widths[n]),tallest):parts[n]);
    // No meio da linha, a caixa mais baixa desce metade do que falta, como o Ink a centra.
    if(p.alignItems==='center')parts=parts.map(part=>[...Array(Math.floor((tallest-part.length)/2)).fill(''),...part]);
    return Array.from({length:Math.max(0,...parts.map(part=>part.length))},(_,at)=>' '.repeat(lead)+parts.map((part,n)=>(part[at] ?? '').padEnd(widths[n]).slice(0,widths[n])).join(' '.repeat(between)));
  });
}
// As linhas da tela na largura que a tela pede no painel, sem os espaços do fim.
const screenLines=(tree:any,columns=100)=>drawnLines(tree,sized(tree.props,columns) ?? columns).map(line=>line.trimEnd());
// As linhas do cabeçalho e do primeiro bloco: o que a pessoa vê ao abrir a
// tela. As Configurações chegam numa coluna só, e o primeiro bloco é o dela.
const opening=(tree:any,columns=100)=>{const width=sized(tree.props,columns) ?? columns,[top,main]=tree.children,whole=kidsOf(main);
  const first=whole.length===1 && whole[0].props?.flexDirection==='column'?kidsOf(whole[0])[0]:whole[0];
  return drawnLines(top,width).length+cells(main.props.paddingTop ?? main.props.paddingY ?? main.props.padding,true)+drawnLines(first,width-2*cells(main.props.paddingX ?? main.props.padding,false)).length;};
// No desktop duas caixas vizinhas numa coluna nunca se encostam: a coluna põe
// espaço entre elas ou uma delas, sem borda nem fundo, tem espaço por dentro.
// O espaço de dentro de um cartão fica dentro da borda e não o afasta do vizinho.
const touching=(tree:any)=>{const found:string[]=[];
  const spaced=(value:any)=>value!=null && value!==0;
  const padded=(node:any)=>!node.props?.borderStyle && !node.props?.backgroundColor && ['padding','paddingY','paddingTop','paddingBottom'].some(prop=>spaced(node.props?.[prop]));
  walk(tree,node=>{if(node.type!=='Box' || node.props?.flexDirection!=='column' || spaced(node.props.rowGap) || spaced(node.props.gap))return;
    const kids=[node.children].flat().filter((kid:any)=>kid && typeof kid==='object');
    kids.forEach((kid:any,n:number)=>{const next=kids[n+1];
      if(next && kid.type==='Box' && next.type==='Box' && !padded(kid) && !padded(next))found.push(`${shown(kid).slice(0,1)} | ${shown(next).slice(0,1)}`);});});
  return found;};
// No terminal nenhum texto encosta no vizinho: numa linha com dois ou mais
// textos há ao menos uma coluna entre eles. As barras, que não têm texto, se emendam.
const BAR=/^[▄▀─━█▁▂▃▅▆▇╌ ]+$/;
const glued=(tree:any)=>{const found:string[]=[];
  walk(tree,node=>{if(node.type!=='Box' || (node.props?.flexDirection ?? 'row')!=='row' || cells(node.props.columnGap ?? node.props.gap,false)>0 || phrase(node)!=null)return;
    const worded=kidsOf(node).filter((kid:any)=>reading(kid).some(value=>!BAR.test(value)));
    if(worded.length>1)found.push(worded.map((kid:any)=>reading(kid)[0]).join(' | '));});
  return found;};
// No terminal duas superfícies cheias, uma embaixo da outra, ficam a uma linha uma da outra.
const merged=(tree:any)=>{const found:string[]=[];
  walk(tree,node=>{if(node.type!=='Box' || node.props?.flexDirection!=='column' || cells(node.props.rowGap ?? node.props.gap,true)>0)return;
    const kids=kidsOf(node).filter((kid:any)=>typeof kid==='object');
    kids.forEach((kid:any,n:number)=>{const next=kids[n+1],ground=(box:any)=>box?.type==='Box'?box.props.backgroundColor ?? kidsOf(box).find((inner:any)=>inner?.type==='Box' && inner.props?.backgroundColor)?.props.backgroundColor:undefined;
      if(next && ground(kid) && ground(kid)===ground(next))found.push(`${reading(kid)[0]} | ${reading(next)[0]}`);});});
  return found;};
// As teclas que o desktop desenharia ao lado de um botão.
const hotkeys=(tree:any)=>{const found:string[]=[];walk(tree,node=>{if(node.type==='Button' && node.props?.hotkey)found.push(`${node.props.key}: ${node.props.hotkey}`);});return found;};
// As tarefas se abrem pela spec, como no layout; no terminal, pelas teclas s e t.
async function toItems(f:any) {
  await f.pane.press({key:'go-spec'});
  await f.pane.press({key:'go-items'});
}
// As telas como a pessoa as abre, cada uma depois da anterior: pelas abas e pela volta.
async function eachScreen(f:any,look:(screen:string,tree:any)=>void) {
  look('overview',await f.pane.drawn());
  await f.pane.press({key:'go-spec'});look('spec',await f.pane.drawn());
  await f.pane.press({key:'go-items'});look('items',await f.pane.drawn());
  await f.pane.press({key:'item-MSTD-TASK-0133'});look('item',await f.pane.drawn());
  await f.pane.press({key:'back'});await f.pane.press({key:'back'});
  await f.pane.press({key:'go-usage'});look('usage',await f.pane.drawn());
  await f.pane.press({key:'go-overview'});
  await f.pane.press({key:'go-settings'});look('settings',await f.pane.drawn());
}

// O que cada tela do layout mostra, palavra por palavra e na ordem dele,
// com os dados de exemplo dele. Onde o binário não manda o dado do layout, o
// texto é o que o painel escreve com o dado que tem: o PR de outra spec sem o
// disco das cópias dela, a tarefa de relatório sem a palavra, o resultado sem
// as barras da medida e a busca na spec com a nota da escolha gravada.
const BOARD:Record<string,(string|RegExp)[]>={
  overview:['mustard','fix/auditoria-spec-e-ondas','Visão geral','Spec','Consumo','AGORA','ajuste-na-spec-fechada','Execução','Spec fechada aceita pedido novo na mesma branch.',
    '584','/584','tarefas feitas','41','/42','critérios passando','366','/367','ondas na base',
    'O PR #289 entrou na base em 05/10, mas a spec segue em execução.','A onda 193 consta em execução, sem agente nem commit. Ela segura a entrega.',
    'PRÓXIMO PASSO',NEXT,'Ver','ONDAS NO AR','5 vagas livres · fila vazia','Nenhuma onda trabalhando','1 agente na sessão',
    'OUTRAS SPECS QUE PEDEM ATENÇÃO','ver as 14','falha-fora-da-obra','Execução · hoje 01:38','PR #326 entrou na base em 20/10',
    'mustard-enxuto','Pull request · 17/09','PR #277 entrou na base em 18/09',
    'USO','consumo completo','Contexto','50%','Cota 5 h','7%','Semana','33%','US$ 1.499','no projeto · 38 sessões','21,9 bi tokens',
    'Publicar no Cloudflare',/^atualizado às \d{2}:\d{2}$/],
  spec:['Visão geral','Spec','Consumo','ajuste-na-spec-fechada','Execução','Spec fechada aceita pedido novo na mesma branch.','Branch','fix/ajuste-na-spec-fechada','Sai de','dev',
    'Aberta','24/09, 13:50','Último commit','05/10, 07:51 · 303 commits','O QUE FOI COMBINADO','584','tarefas','todas feitas','42','critérios','1 sem prova',
    '22','perguntas','marcadas abertas','154','decisões','66','regras','367','ondas','ONDAS','361 comitadas','5 integradas','1 em execução sem agente (onda 193)',
    'FECHAMENTO','Revisão final','aprovada em 02/10, reaberta às 17:13','Pull request #289','entrou na base em 05/10','TROCAR DE SPEC · 14',
    'EM ANDAMENTO','falha-fora-da-obra','Execução','ajuste-na-spec-fechada','Execução','mustard-enxuto','Pull request',
    'LEVANTAMENTO','validacao-leve-por-trecho','desde 07/10','nova-versao','desde 05/10','8 entregues · 1 descartada','›'],
  items:['Visão geral','Spec','Consumo','ajuste-na-spec-fechada','Tarefas 584','Critérios 42','Decisões 154','Mais','Buscar pelo título ou arquivo',
    'Todas 584','Pedem atenção 1','Comitadas','Sem commit',
    'Arquivo novo ou apagado: receita diferente no comando e no pedido','Onda 193 · em execução sem agente · reenviada 2 vezes',
    'Reabertura de spec fechada volta à execução','Onda 1 · comitada em 24/09 · 10 arquivos · cobre 14 itens',
    'A ligação nova de imports medida nos projetos da prova','Onda 81 · integrada, sem commit',
    'A mesma pergunta em português e em inglês acha o mesmo código','Onda 133 · integrada, sem commit · combinado não atendido',
    'O veredito gravado durante o fechamento não se perde mais','Onda 254 · comitada em 30/09 · 2 arquivos',
    'Medir se notas de sentido melhoram a busca do scan','Onda 260 · integrada, sem commit · nenhum arquivo',
    'Decisões antigas ainda citam o comando de medida','Onda 400 · integrada, sem commit · nenhum arquivo',
    'Quatro testes deixam de depender do sistema em que rodam','Onda 479 · comitada em 05/10 · 4 arquivos',
    '8 de 584 · as que pedem atenção vêm primeiro','Mostrar mais'],
  item:['Visão geral','Spec','Consumo','Tarefas','A mesma pergunta em português e em inglês acha o mesmo código','Integrada, sem commit','Onda 133','Entregue em 28/09, 06:38',
    'O QUE FOI PEDIDO',/^A pergunta em português deve achar o mesmo código que a pergunta em inglês\. Exemplo: a validação do documento do cliente, cujo código se chama$/,
    'validateDocument','O QUE O AGENTE FEZ','Mediu nos três projetos da prova, nas duas línguas, sem mudar o produto e sem gastar Jev. Entregou um relatório, sem commit.',
    'RESULTADO','combinado não atendido','A regra “O scan e a busca funcionam em qualquer idioma do Mustard” · não atendido',
    'Arquivo','.claude/plans/poc-busca/medida-duas-linguas.md','Término','A mesma pergunta nas duas línguas acha o mesmo código.',
    'Cobre','A regra “O scan e a busca funcionam em qualquer idioma do Mustard”. Nenhum critério ligado.','Gasto','5,04 mi tokens na onda 133',
    'HISTÓRICO · 3 VERSÕES','Posta na onda 133','28/09, 06:20 · pela rodada','Reescrita com dependência e arquivo','26/09, 22:00','Primeira versão','26/09, 21:57'],
  usage:['Visão geral','Spec','Consumo','Medido até 13/10. O histórico não é atualizado desde 15/10.','Medir agora','TOTAL · 29/08 A 13/10',
    'Este projeto','21,9 bi','tokens','A máquina inteira','27,1 bi','+354% na última semana','CUSTO ESTIMADO DO CLAUDE','US$ 1.499,27','em 38 sessões',
    'falha-fora-da-obra','US$ 1.199,70','fora de spec','US$ 299,58','As outras 12 specs não têm custo atribuído.',
    'TOKENS POR DIA · ESTE PROJETO','máximo 886 mi em 30/09','sem medida','30/09','07/10','13/10',
    'ESTA SPEC · AJUSTE-NA-SPEC-FECHADA','Ondas','3,79 bi','25 ondas sem medida','Condutor','2,14 bi','custo não medido',
    'FERRAMENTAS','Filtro de saída (RTK)','−68,9%','54.112 comandos · 71,6 mi tokens poupados','Jev','1.083 chamadas',
    '853 montagem · 208 contexto · 22 busca · 5,4 mi tokens · custo não medido','Busca de código','8.732 buscas','nenhuma nesta spec',
    /^ESTA SESSÃO · \d+ H \d+ MIN$/,'Contexto · 499.669 de 1 mi','50%',/^Cota de 5 h · renova às \d{2}:\d{2}$/,'7%','Cota da semana · renova em 22/10','33%',
    '16 agentes observados · 1 trabalhando agora'],
  settings:['Configurações','salvo no mustard.json','AGENTES','Agente de onda','Revisor final','Ondas ao mesmo tempo','quantas compilam juntas','−','5','+',
    'JEV','cada pergunta é paga','Montar as ondas','Escolher o contexto do pedido','Busca de código com dúvida','Busca na spec','nenhuma escolha gravada',
    'FERRAMENTAS','Modelo local da busca','RTK','enxuga a saída dos comandos','PUBLICAÇÃO · CLOUDFLARE PAGES',
    'Conta','[ID DA CONTA]','Projeto no Cloudflare','[NOME DO PROJETO]','Token','guardado no mustard.json, que está fora do git'],
};
// Os títulos dos blocos de cada tela, na ordem do layout.
const HEADINGS:Record<string,string[]>={
  overview:['AGORA','ONDAS NO AR','OUTRAS SPECS QUE PEDEM ATENÇÃO','USO'],
  spec:['O QUE FOI COMBINADO','ONDAS','FECHAMENTO','TROCAR DE SPEC · 14','EM ANDAMENTO','LEVANTAMENTO'],
  items:[],
  item:['O QUE FOI PEDIDO','O QUE O AGENTE FEZ','RESULTADO','HISTÓRICO · 3 VERSÕES'],
  settings:['AGENTES','JEV','FERRAMENTAS','PUBLICAÇÃO · CLOUDFLARE PAGES'],
};
const USAGE_HEADINGS=['TOTAL · 29/08 A 13/10','CUSTO ESTIMADO DO CLAUDE','TOKENS POR DIA · ESTE PROJETO','ESTA SPEC · AJUSTE-NA-SPEC-FECHADA','FERRAMENTAS',/^ESTA SESSÃO/];
async function reach(f:any,screen:string) {
  if(screen==='spec')await f.pane.press({key:'go-spec'});
  if(['items','item'].includes(screen))await toItems(f);
  if(screen==='item')await f.pane.press({key:'item-MSTD-TASK-0133'});
  if(screen==='usage')await f.pane.press({key:'go-usage'});
  if(screen==='settings')await f.pane.press({key:'go-settings'});
  return f.pane.drawn();
}

// Cada tela nas duas superfícies: os textos do layout na ordem dele, os
// títulos dos blocos, a paleta e, no terminal, nada encostado e nenhuma
// superfície emendada na vizinha.
for(const screen of ['overview','spec','items','item','usage','settings'])for(const surface of SURFACES)
  test(`the ${screen} screen shows the layout's words in its order, in its palette · ${surface}`,async ($,on)=>{
    const f=await open($,on,surface);const tree:any=await reach(f,screen);
    inOrder(reading(tree),BOARD[screen]);
    const titles=surface==='terminal'?termHeadings(tree):await headings(f.pane);
    if(screen==='usage')inOrder(titles,USAGE_HEADINGS);else expect(titles).toEqual(HEADINGS[screen]);
    expect(tree.props.backgroundColor).toBe(DARK.page);expect(tree.children[0].props.backgroundColor).toBe(DARK.header);
    expect(outside(tree,DARK)).toEqual([]);
    if(surface==='terminal'){expect(glued(tree)).toEqual([]);expect(merged(tree)).toEqual([]);expect(tree.children[1].props.rowGap).toBe(1);}
    else expect(touching(tree)).toEqual([]);
    await f.close();
  });

test('the desktop overview draws the layout: the brand, the open tab, the pill, the boxes, the warnings and the main buttons',async ($,on)=>{
  const f=await open($,on,'desktop'),pane=f.pane,tree:any=await pane.drawn();
  // O cabeçalho: a marca mostarda com o quadrado escuro, o nome em negrito, a branch em fonte fixa.
  const [logo]=await svgs(pane);expect(logo.source).toContain(`fill="${DARK.mustard}"`);expect(logo.source).toContain(`fill="${DARK.page}"`);
  expect(await style(pane,'mustard')).toMatchObject({weight:'bold'});
  expect(await style(pane,'fix/auditoria-spec-e-ondas')).toMatchObject({mono:true,size:'caption',color:DARK.muted});
  // Atualizar e configurações: os ícones apagados; o botão do aplicativo não aceita desenho dentro.
  for(const [key,glyph] of [['refresh','↻'],['go-settings','⚙']])expect(kidsOf((await pane.find({key}))!)[0]).toMatchObject({type:'Text',props:{color:DARK.soft},children:[glyph]});
  // As abas: a aberta no texto e no seminegrito, com o sublinhado mostarda; as outras no terciário.
  expect(await style(pane,'Visão geral')).toMatchObject({weight:'semibold',color:DARK.ink});expect(await style(pane,'Consumo')).toMatchObject({weight:'medium',color:DARK.muted});
  expect((await boxes(pane,{backgroundColor:DARK.mustard})).length).toBe(1);
  expect(tree.children[0].children.at(-1)).toMatchObject({type:'Svg',props:{alt:'divisória'}});
  // A etiqueta da fase na tinta mostarda, com o ponto; os três quadros de colunas iguais.
  expect((await boxes(pane,{backgroundColor:DARK.tint,borderColor:DARK.tintLine,borderStyle:'round'})).map(box=>reading(box))).toEqual([['●','Execução'],['PRÓXIMO PASSO',NEXT,'Ver']]);
  expect((await boxes(pane,{backgroundColor:DARK.box,width:'30%',flexGrow:1})).length).toBe(3);
  expect(await style(pane,'584')).toMatchObject({size:'heading',weight:'bold',color:DARK.accent,tabular:true});
  expect(await style(pane,'/584')).toMatchObject({color:DARK.muted});
  // Os dois avisos no laranja, com o ícone do layout.
  const alerts=await boxes(pane,{backgroundColor:DARK.warningSoft,borderColor:DARK.warningLine});
  expect(alerts.map((box:any)=>kidsOf(box)[0].props.alt)).toEqual(['aviso','aviso']);
  expect(kidsOf(alerts[0])[0].props.source).toContain('M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z');
  expect(await style(pane,'O PR #289 entrou na base em 05/10, mas a spec segue em execução.')).toMatchObject({size:'footnote',color:DARK.warningInk});
  // Ver e Publicar no Cloudflare são o botão principal do aplicativo; o layout não tem tecla ao lado de nenhum botão.
  for(const key of ['next-see','publish'])expect((await pane.find({key}))!.props.variant).toBe('primary');
  expect(hotkeys(tree)).toEqual([]);
  // Ondas no ar: o cartão tracejado, sem fundo; o rodapé no fundo do cabeçalho.
  expect((await boxes(pane,{borderStyle:'dashed'})).map((box:any)=>[box.props.borderColor,box.props.backgroundColor])).toEqual([[DARK.dashed,undefined]]);
  expect(tree.children.at(-1).props.backgroundColor).toBe(DARK.header);
  // As barras do uso enchem a coluna, as partes em porcentagem, no mostarda sobre o trilho.
  const gauges=(await svgs(pane)).filter(svg=>['Contexto 50%','Cota 5 h 7%','Semana 33%'].includes(svg.alt));
  expect(gauges.map(svg=>[svg.width,/<rect width="100%"/.test(svg.source),svg.source.includes(DARK.mustard)])).toEqual(Array(3).fill([undefined,true,true]));
  expect(await style(pane,'US$ 1.499')).toMatchObject({size:'title',weight:'bold'});
  await f.close();
});

test('the terminal overview draws the layout in cells: filled surfaces, the pill, the thin bars and the keys',async ($,on)=>{
  const f=await open($,on,'terminal'),pane=f.pane,tree:any=await pane.drawn();
  // A marca: quatro colunas por duas linhas de mostarda, o quadrado escuro no meio em meios blocos.
  expect(nodesOf(tree.children[0],'Text').slice(0,2).map((node:any)=>[inline(node),node.props.color,node.props.backgroundColor]))
    .toEqual([[' ▄▄ ',DARK.page,DARK.mustard],[' ▀▀ ',DARK.page,DARK.mustard]]);
  expect(await style(pane,'mustard')).toMatchObject({bold:true});
  expect(await style(pane,'fix/auditoria-spec-e-ondas')).toMatchObject({color:DARK.muted,wrap:'truncate-end'});
  // Atualizar e configurações: o ícone sobre o fundo do cartão; configurações na tecla a. O terminal não desenha o fechar.
  expect(kidsOf((await pane.find({key:'refresh'}))!)[0]).toMatchObject({props:{color:DARK.soft,backgroundColor:DARK.card},children:[' ↻ ']});
  expect((await pane.find({key:'go-settings'}))!.props.hotkey).toBe('a');expect(await pane.find({key:'quit'})).toBeUndefined();
  // Os cartões com a borda fina e redonda do layout; o de Ondas no ar, tracejado; os quadros de dentro, só o fundo.
  expect(nodesOf(tree,'Box').filter((node:any)=>node.props.borderStyle).map((node:any)=>[node.props.borderStyle,node.props.borderColor]))
    .toEqual([['round',DARK.line],['dashed',DARK.dashed],['round',DARK.line],['round',DARK.line]]);
  expect((await boxes(pane,{backgroundColor:DARK.card})).length).toBeGreaterThanOrEqual(3);
  expect((await boxes(pane,{backgroundColor:DARK.box,width:'30%'})).length).toBe(3);
  for(const color of [DARK.warningSoft,DARK.tint])expect((await boxes(pane,{backgroundColor:color})).length,color).toBeGreaterThan(0);
  // A etiqueta da fase: o ponto mostarda e o texto de destaque sobre a tinta.
  const pill=nodesOf(tree,'Text').find((node:any)=>inline(node)===' ● Execução ')!;
  expect(pill.props.backgroundColor).toBe(DARK.tint);expect(kidsOf(pill).filter((kid:any)=>typeof kid==='object').map((kid:any)=>kid.props.color)).toEqual([DARK.mustard,DARK.accent]);
  // O número feito no destaque e em negrito, o total apagado; o aviso com o ▲ laranja.
  expect(await style(pane,'584')).toMatchObject({bold:true,color:DARK.accent});expect(await style(pane,'/584')).toMatchObject({color:DARK.muted});
  expect(await style(pane,'▲')).toMatchObject({color:DARK.warning});
  // Ver e Publicar no Cloudflare no mostarda cheio, o texto escuro em negrito; publicar na tecla p.
  for(const caption of [' Ver ',' Publicar no Cloudflare '])expect(await style(pane,caption)).toMatchObject({backgroundColor:DARK.mustard,color:DARK.buttonInk,bold:true});
  expect((await pane.find({key:'publish'}))!.props.hotkey).toBe('p');
  // As barras finas: meio bloco, a parte cheia em mostarda na porcentagem dela e o trilho no resto.
  const runs=nodesOf(tree.children[1],'Box').filter((node:any)=>node.props.height===1 && node.props.overflow==='hidden' && /^▄+$/.test(inline(kidsOf(node)[0])));
  expect(runs.slice(-6).map((node:any)=>[node.props.width,kidsOf(node)[0].props.color])).toEqual([['50%',DARK.mustard],[0,DARK.line],['7%',DARK.mustard],[0,DARK.line],
    ['33%',DARK.mustard],[0,DARK.line]]);
  // Os quadros dos números: a barra fina em mostarda; a que não enche leva o trilho no resto.
  expect(runs.slice(0,-6).map((node:any)=>[node.props.width,kidsOf(node)[0].props.color])).toEqual([['100%',DARK.mustard],['98%',DARK.mustard],[0,DARK.line],['100%',DARK.mustard],[0,DARK.line]]);
  // O rodapé no fundo do cabeçalho.
  expect(tree.children.at(-1).props.backgroundColor).toBe(DARK.header);
  await f.close();
});

for(const surface of SURFACES)test(`the spec screen draws the picker, the six boxes, the waves bar and the chosen spec in the mustard tint · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface),pane=f.pane,tree:any=await reach(f,'spec');
  // O seletor: o nome em negrito, a fase no destaque e a seta; no terminal, na tecla o.
  expect(await style(pane,'Execução')).toMatchObject({color:DARK.accent});
  expect((await pane.find({key:'spec-picker'}))!.props).toMatchObject({label:'ajuste-na-spec-fechada',...(surface==='terminal'?{hotkey:'o'}:{})});
  if(surface==='desktop')expect((await svgs(pane)).find(svg=>svg.alt==='trocar de spec')?.source).toContain('M6 9l6 6 6-6');else expect(await style(pane,'▾')).toMatchObject({color:DARK.muted});
  // As seis caixas no fundo do cartão; o estado colorido; as tarefas também na tecla t no terminal.
  for(const figure of ['584','42','22','154','66','367'])expect(await style(pane,figure)).toMatchObject(surface==='terminal'?{bold:true}:{size:'title',weight:'bold'});
  expect(await style(pane,'todas feitas')).toMatchObject({color:DARK.success});expect(await style(pane,'1 sem prova')).toMatchObject({color:DARK.warning});
  expect(await style(pane,'marcadas abertas')).toMatchObject({color:DARK.muted});
  if(surface==='terminal')expect((await pane.find({key:'go-items'}))!.props.hotkey).toBe('t');
  // As ondas numa barra só: as comitadas no texto claro, as integradas em mostarda, a parada em laranja.
  if(surface==='desktop') {
    const waves=(await svgs(pane)).find(svg=>/361 comitadas/.test(svg.alt))!;
    for(const color of [DARK.ink,DARK.mustard,DARK.warning])expect(waves.source).toContain(color);
  } else {
    const parts=nodesOf(tree.children[1],'Box').filter((node:any)=>node.props.overflow==='hidden' && /^▄+$/.test(inline(kidsOf(node)[0])));
    expect(parts.map((node:any)=>[node.props.width,kidsOf(node)[0].props.color])).toEqual([['98%',DARK.ink],['1%',DARK.mustard],['1%',DARK.warning],[0,DARK.line]]);
  }
  expect(await style(pane,'1 em execução sem agente (onda 193)')).toMatchObject({color:DARK.warning});
  expect(await style(pane,surface==='terminal'?'aprovada em 02/10,':'aprovada em 02/10, reaberta às 17:13')).toMatchObject({color:DARK.warning});
  expect(await style(pane,'entrou na base em 05/10')).toMatchObject({color:DARK.success});
  // A spec em uso na tinta mostarda, o nome em negrito e a fase no destaque.
  const chosen=await boxes(pane,{backgroundColor:DARK.tint});expect(chosen.map((box:any)=>reading(box)[0])).toEqual(['ajuste-na-spec-fechada']);
  expect((await pane.find({key:'delivered'}))!.props.label).toBe('8 entregues · 1 descartada');
  // A troca abre logo abaixo do seletor, e a seta vira para cima.
  await pane.press({key:'spec-picker'});
  const titles=surface==='terminal'?termHeadings(await pane.drawn()):await headings(pane);expect(titles[0]).toBe('TROCAR DE SPEC · 14');
  if(surface==='desktop')expect((await svgs(pane)).find(svg=>svg.alt==='esconder a troca de spec')?.source).toContain('M6 15l6-6 6 6');else await style(pane,'▴');
  await f.close();
});

for(const surface of SURFACES)test(`the task list draws the way back, the segmented kinds, the search, the filter pills and the rows · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface),pane=f.pane,tree:any=await reach(f,'items');
  // A volta em mostarda, com a seta; no terminal na tecla v.
  if(surface==='desktop') {
    expect((await pane.find({key:'back'}))!.props.label).toBe('ajuste-na-spec-fechada');
    expect((await svgs(pane)).find(svg=>svg.alt==='voltar')?.source).toContain('M15 18l-6-6 6-6');
  } else expect((await pane.find({key:'back'}))!.props).toMatchObject({label:'‹ ajuste-na-spec-fechada',hotkey:'v'});
  expect(await style(pane,'ajuste-na-spec-fechada')).toMatchObject({color:DARK.accent});
  // As abas do tipo no cartão: a aberta no mostarda cheio, as outras no secundário, o resto em Mais.
  expect((await boxes(pane,{backgroundColor:DARK.card})).some((box:any)=>reading(box).includes('Critérios 42') && reading(box).includes('Tarefas 584'))).toBe(true);
  if(surface==='desktop')expect((await pane.find({key:'kind-task'}))!.props.variant).toBe('primary');
  else expect((await boxes(pane,{backgroundColor:DARK.mustard})).map((box:any)=>reading(box)[0])).toEqual(['Tarefas 584']);
  expect(await style(pane,'Critérios 42')).toMatchObject({color:DARK.soft});
  expect((await pane.find({key:'kind-more'}))!.props.options.map((option:any)=>option.label)).toEqual(['Mais','Regras 66','Perguntas 44']);
  // A busca no cartão, com a lupa; no terminal ela abre na tecla b.
  if(surface==='desktop')expect((await pane.find({key:'search'}))!.props.placeholder).toBe('Buscar pelo título ou arquivo');
  else expect((await pane.find({key:'search-open'}))!.props).toMatchObject({label:'Buscar pelo título ou arquivo',hotkey:'b'});
  // Os filtros: a escolhida na tinta mostarda (no desktop, o botão principal), a de atenção no laranja.
  if(surface==='terminal') {
    expect(await style(pane,' Todas 584 ')).toMatchObject({backgroundColor:DARK.tint,color:DARK.accent});
    expect(await style(pane,' Pedem atenção 1 ')).toMatchObject({backgroundColor:DARK.warningSoft,color:DARK.warningInk});
    expect(await style(pane,' Comitadas ')).toMatchObject({backgroundColor:DARK.card,color:DARK.soft});
  } else {
    expect((await pane.find({key:'filter-all'}))!.props.variant).toBe('primary');
    expect(await style(pane,'Pedem atenção 1')).toMatchObject({color:DARK.warningInk});
  }
  // As linhas: o ponto da cor do estado, a que pede atenção no fundo dela e o título em seminegrito.
  const dots=(await pane.findAll({type:'Text'})).filter((node:any)=>node.text==='●').map((node:any)=>node.props.color);
  expect(dots).toEqual([DARK.warning,DARK.success,DARK.mustard,DARK.mustard,DARK.success,DARK.mustard,DARK.mustard,DARK.success]);
  expect((await boxes(pane,{backgroundColor:DARK.attention})).map((box:any)=>reading(box).find(value=>/Arquivo novo/.test(value)))).toEqual([board[0].title]);
  expect(await style(pane,board[0].title)).toMatchObject(surface==='terminal'?{bold:true}:{weight:'semibold'});
  expect(await style(pane,first(surface,'Onda 193 · em execução sem agente · reenviada 2 vezes'))).toMatchObject({color:DARK.warning});
  expect(await style(pane,first(surface,'Onda 254 · comitada em 30/09 · 2 arquivos'))).toMatchObject({color:DARK.muted});
  expect((await pane.find({key:'more'}))!.props).toMatchObject({label:'Mostrar mais',...(surface==='terminal'?{hotkey:'m'}:{})});
  if(surface==='terminal')for(const key of ['↑↓','enter'])expect(await style(pane,` ${key} `)).toMatchObject({backgroundColor:DARK.box,color:DARK.accent});
  else expect(hotkeys(tree)).toEqual([]);
  await f.close();
});

for(const surface of SURFACES)test(`a task draws the title, the state pill, the cards, the sheet and the history dots · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface),pane=f.pane,tree:any=await reach(f,'item');
  expect(await style(pane,'A mesma pergunta em português e em inglês acha o mesmo código')).toMatchObject(surface==='terminal'?{bold:true}:{size:'heading',weight:'bold'});
  // O estado da onda na etiqueta mostarda; o resultado não atendido na de aviso.
  if(surface==='desktop') {
    expect((await boxes(pane,{backgroundColor:DARK.tint,borderColor:DARK.tintLine})).map((box:any)=>reading(box))).toEqual([['●','Integrada, sem commit']]);
    expect((await boxes(pane,{backgroundColor:DARK.warningSoft,borderColor:DARK.warningLine})).map((box:any)=>reading(box))).toEqual([['combinado não atendido']]);
  } else {
    expect(nodesOf(tree,'Text').filter((node:any)=>[DARK.tint,DARK.warningSoft].includes(node.props.backgroundColor) && inline(node).startsWith(' '))
      .map((node:any)=>[inline(node),node.props.backgroundColor])).toEqual([[' ● Integrada, sem commit ',DARK.tint],[' combinado não atendido ',DARK.warningSoft]]);
  }
  // O código no texto do pedido: no quadro escuro e no destaque.
  expect(await style(pane,'validateDocument')).toMatchObject({backgroundColor:DARK.box,color:DARK.accent,...(surface==='desktop'?{mono:true}:{})});
  // A ficha: o rótulo apagado em cima e o valor embaixo, o arquivo em fonte fixa.
  inOrder(await texts(pane),['Arquivo','.claude/plans/poc-busca/medida-duas-linguas.md','Término','Cobre','Gasto']);
  expect(await style(pane,'Término')).toMatchObject({color:DARK.muted});
  if(surface==='desktop')expect(await style(pane,'.claude/plans/poc-busca/medida-duas-linguas.md')).toMatchObject({mono:true});
  // O histórico: a versão nova com o ponto mostarda, as outras com o ponto apagado.
  const dots=(await pane.findAll({type:'Text'})).filter((node:any)=>node.text==='●' && node.props.color!==DARK.mustard || node.text==='●').map((node:any)=>node.props.color);
  expect(dots.slice(-3)).toEqual([DARK.mustard,DARK.dashed,DARK.dashed]);
  expect(await style(pane,'Posta na onda 133')).toMatchObject(surface==='terminal'?{bold:true}:{weight:'semibold'});
  // A volta e nada além do que o layout tem: sem testes, veredito nem pedido da onda nesta tarefa.
  expect((await pane.find({key:'back'}))!.props.label).toBe(surface==='terminal'?'‹ Tarefas':'Tarefas');
  expect(await pane.find({key:'wave-request'})).toBeUndefined();await lacks(pane,/^TESTES$/);await lacks(pane,/^Revisão:/);
  await f.close();
});

for(const surface of SURFACES)test(`the usage screen draws the warning with its button, the cards, the bars and the chart in mustard · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface),pane=f.pane,tree:any=await reach(f,'usage');
  // O aviso da medida no laranja, com o ícone; medir no mostarda cheio, na tecla m no terminal.
  expect(await style(pane,'Medido até 13/10. O histórico não é atualizado desde 15/10.')).toMatchObject({color:DARK.warningInk});
  if(surface==='desktop'){expect((await pane.find({key:'measure'}))!.props.variant).toBe('primary');expect((await svgs(pane)).find(svg=>svg.alt==='aviso')?.source).toContain(DARK.warning);}
  else {expect(await style(pane,' Medir agora ')).toMatchObject({backgroundColor:DARK.mustard,color:DARK.buttonInk});expect((await pane.find({key:'measure'}))!.props.hotkey).toBe('m');}
  // Os cartões do total lado a lado: este projeto no destaque, a máquina no texto, a alta em laranja.
  expect(await style(pane,'21,9 bi')).toMatchObject({color:DARK.accent});expect(await style(pane,'27,1 bi')).toMatchObject({color:DARK.ink});
  expect(await style(pane,'+354% na última semana')).toMatchObject({color:DARK.warning});
  // O custo: a spec em mostarda, fora de spec no mostarda escuro.
  if(surface==='desktop') {
    const drawings=await svgs(pane);
    expect(drawings.find(svg=>svg.alt==='falha-fora-da-obra US$ 1.199,70')?.source).toContain(DARK.mustard);
    expect(drawings.find(svg=>svg.alt==='fora de spec US$ 299,58')?.source).toContain(DARK.deep);
    // O gráfico: barras mostarda arredondadas só em cima, o dia sem medida tracejado.
    const chart=drawings.find(svg=>/^Tokens por dia/.test(svg.alt))!;
    expect(chart.height).toBe(120);expect(chart.source).toContain(`fill="${DARK.mustard}"`);expect(chart.source).toContain('stroke-dasharray');
    expect((chart.source.match(/<path d="[^"]+"/g) ?? []).length).toBe(9);
  } else {
    const runs=nodesOf(tree,'Box').filter((node:any)=>node.props.height===1 && /^▄+$/.test(inline(kidsOf(node)[0])));
    expect(runs.slice(0,4).map((node:any)=>[node.props.width,kidsOf(node)[0].props.color])).toEqual([['80%',DARK.mustard],[0,DARK.line],['20%',DARK.deep],[0,DARK.line]]);
    const days=nodesOf(tree,'Box').filter((node:any)=>node.props.flexDirection==='column' && node.props.width===0 && node.props.flexGrow===1);
    expect(days.map((node:any)=>inline(kidsOf(kidsOf(node).at(-1))[0] ?? '').charAt(0)).join('')).toBe('███████▂╌╌▁╌╌╌');
    expect(nodesOf(tree,'Text').filter((node:any)=>/^[█▁▂▃▄▅▆▇]+$/.test(inline(node)) && node.props.color===DARK.mustard).length).toBeGreaterThan(14);
  }
  expect(await style(pane,'−68,9%')).toMatchObject({color:DARK.success});
  expect(await style(pane,'Filtro de saída (RTK)')).toMatchObject(surface==='terminal'?{bold:true}:{weight:'semibold'});
  await f.close();
});

for(const surface of SURFACES)test(`the settings draw their own header, the pickers, the counter, the switches and the fields · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface),pane=f.pane,tree:any=await reach(f,'settings');
  // O cabeçalho delas: voltar, o título e onde fica salvo, sem as abas.
  expect(await pane.find({key:'go-overview'})).toBeUndefined();
  expect((await pane.find({key:'back'}))!.props).toMatchObject({label:'Voltar',...(surface==='terminal'?{hotkey:'v'}:{})});
  expect(await style(pane,'Configurações')).toMatchObject(surface==='terminal'?{bold:true}:{weight:'bold'});
  expect(await style(pane,'salvo no mustard.json')).toMatchObject({color:DARK.muted});
  // O modelo e o esforço em seletores sem rótulo, o esforço na ordem dele; no terminal no quadro escuro.
  for(const field of ['agent_model','agent_effort','review_model','review_effort'])expect((await pane.find({key:`setting-${field}`}))!.props.label,field).toBeUndefined();
  expect((await pane.find({key:'setting-agent_model'}))!.props.options).toContainEqual({value:'opus',label:'Opus'});
  expect((await pane.find({key:'setting-agent_effort'}))!.props.options.map((option:any)=>option.value)).toEqual(['low','medium','high','xhigh','max']);
  if(surface==='terminal')expect((await boxes(pane,{backgroundColor:DARK.box})).length).toBeGreaterThanOrEqual(4);
  // O contador: menos e mais, o número no destaque.
  expect(await style(pane,'5')).toMatchObject({color:DARK.accent,...(surface==='terminal'?{bold:true}:{weight:'bold'})});
  for(const key of ['setting-waves-less','setting-waves-more'])expect(await pane.find({key}),key).toBeDefined();
  // Os interruptores: ligado, o trilho mostarda com o botão escuro à direita; desligado, o apagado à esquerda.
  const knobs=nodesOf(tree,'Button').filter((node:any)=>/^setting-(jev|vectors|rtk)/.test(node.props.key)).map((node:any)=>kidsOf(node)[0])
    .map((knob:any)=>[inline(knob).trim(),inline(knob).endsWith('●'),knob.props.backgroundColor,knob.props.color]);
  expect(knobs).toEqual([...Array(3).fill(['●',true,DARK.mustard,DARK.page]),['●',false,DARK.line,DARK.muted],...Array(2).fill(['●',true,DARK.mustard,DARK.page])]);
  expect((await pane.find({key:'setting-jev_spec_search'}))!.props.label).toBe('Busca na spec · desligado');
  // Os campos: o rótulo em cima, o valor gravado no campo, o token só em pontos.
  expect((await pane.find({key:'setting-cloudflare_account'}))!.props).toMatchObject({value:'[ID DA CONTA]'});
  expect((await pane.find({key:'setting-cloudflare_account'}))!.props.label).toBeUndefined();
  expect((await pane.find({key:'setting-cloudflare_token'}))!.props).toMatchObject({value:'',placeholder:'••••••••••••••••'});
  expect(await style(pane,'guardado no mustard.json, que está fora do git')).toMatchObject({color:DARK.muted});
  // Sem nada a salvar, nem salvar nem descartar.
  expect(await pane.find({key:'settings-reload'})).toBeUndefined();expect(await pane.find({key:'settings-save'})).toBeUndefined();
  if(surface==='desktop')expect(hotkeys(tree)).toEqual([]);
  await f.close();
});

// O terminal mostra as mesmas telas do aplicativo: os mesmos títulos de bloco
// e todo texto do desktop, na mesma ordem. O que ele tem a mais são as
// traduções do que não desenha: o símbolo no lugar do ícone, o meio bloco no
// lugar da barra, a tecla antes do botão.
test('the terminal shows every block and every text of the desktop, in the same order',async ($,on)=>{
  const f=await open($,on,'terminal');
  const desktop=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',requestId:'mustard-panel',
    props:{id:'mustard-panel',title:'Mustard',bodyColumns:100,scroll:{bodyRows:40}}});
  const same=async (screen:string)=>{const terminal=await f.pane.drawn(),board_=await desktop.drawn();
    expect(termHeadings(terminal),screen).toEqual(await headings(desktop));inOrder(textsOf(terminal),textsOf(board_));};
  await same('overview');
  await f.pane.press({key:'go-spec'});await same('spec');
  await f.pane.press({key:'go-items'});await same('items');
  await f.pane.press({key:'item-MSTD-TASK-0133'});await same('item');
  await f.pane.press({key:'back'});await f.pane.press({key:'back'});
  await f.pane.press({key:'go-usage'});await same('usage');
  await f.pane.press({key:'go-overview'});await f.pane.press({key:'go-settings'});await same('settings');
  await desktop.press({key:'quit'});
});

test('the open tab is marked by the mustard line under it, in the line that closes the header, on every screen',async ($,on)=>{
  const f=await open($,on,'terminal');
  await eachScreen(f,(screen,tree)=>{
    if(screen==='settings')return;
    const open_={overview:'Visão geral',spec:'Spec',items:'Spec',item:'Spec',usage:'Consumo'}[screen]!;
    const lines=screenLines(tree),at=lines.findIndex(line=>line.includes('Visão geral') && line.includes('Consumo')),mark=lines[at+1],start=mark.indexOf('━');
    // O traço mostarda começa onde a aba aberta começa e tem o tamanho dela; o resto é a linha do cabeçalho.
    expect([start,mark.lastIndexOf('━')-start+1],screen).toEqual([lines[at].indexOf(open_),open_.length]);
    expect(mark.replace(/━+/,''),screen).toMatch(/^─+$/);
    expect(nodesOf(tree.children[0],'Text').filter((node:any)=>/^━+$/.test(inline(node))).map((node:any)=>node.props.color),screen).toEqual([DARK.mustard]);
    // A aba aberta em negrito na cor do texto; as outras no terciário.
    const tabs=['go-overview','go-spec','go-usage'].map(key=>nodesOf(tree,'Button').find((node:any)=>node.props.key===key)).map((node:any)=>kidsOf(node)[0]);
    expect(tabs.map((tab:any)=>[inline(tab),Boolean(tab.props.bold),tab.props.color]),screen).toEqual(['Visão geral','Spec','Consumo'].map(caption=>caption===open_
      ?[caption,true,DARK.ink]:[caption,false,DARK.muted]));
  });
  await f.close();
});

test('every terminal button carries its approved key, screen by screen',async ($,on)=>{
  const f=await open($,on,'terminal'),seen:Record<string,string[]>={};
  await eachScreen(f,(screen,tree)=>{seen[screen]=hotkeys(tree);});
  // A Visão geral leva o v onde ela é a volta; nas outras telas o v é o caminho de volta.
  expect(seen).toEqual({
    overview:['go-settings: a','go-spec: s','go-usage: c','publish: p'],
    spec:['go-settings: a','go-overview: v','go-usage: c','spec-picker: o','go-items: t'],
    items:['go-settings: a','go-usage: c','back: v','search-open: b','filter: f','more: m'],
    item:['go-settings: a','go-usage: c','back: v'],
    usage:['go-settings: a','go-overview: v','go-spec: s','measure: m'],
    settings:['back: v'],
  });
  await f.close();
});

test('the terminal draws no Svg and no text size, face or digits, spaces only in whole cells and lets no text touch another',async ($,on)=>{
  const f=await open($,on,'terminal');
  await eachScreen(f,(screen,tree)=>{
    expect(nodesOf(tree,'Svg'),screen).toEqual([]);
    expect(nodesOf(tree,'Text').flatMap((node:any)=>['size','weight','mono','tabular'].filter(prop=>prop in (node.props ?? {}))),screen).toEqual([]);
    // Todo espaço é um número de células: um degrau xs ou sm no vertical não daria linha nenhuma.
    const steps:string[]=[];
    walk(tree,node=>{if(node.type==='Box')for(const [prop,value] of Object.entries(node.props ?? {}))if(/^(padding|margin|gap|rowGap|columnGap)/.test(prop) && typeof value!=='number')steps.push(`${prop}: ${value}`);});
    expect(steps,screen).toEqual([]);
    // Cada bloco da tela a uma linha em branco do seguinte; nenhum texto encostado no vizinho.
    expect(tree.children[1].props.rowGap,screen).toBe(1);
    expect(glued(tree),screen).toEqual([]);expect(merged(tree),screen).toEqual([]);
  });
  await f.close();
});

// O que a pessoa vê ao abrir cada tela, sem rolar: o cabeçalho, com as teclas,
// e o primeiro bloco, num painel largo e num estreito. O resto rola.
for(const columns of [100,56,48])test(`every terminal screen opens with its header and first block inside the rows the panel asks for · ${columns} columns`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns});
  const rows=f.asked()?.rows;expect(rows).toBe(36);
  await eachScreen(f,(screen,tree)=>expect(opening(tree,columns),screen).toBeLessThanOrEqual(rows));
  await f.close();
});

// A tela ocupa a largura do painel até 110 colunas. Os quadros dos números
// ficam lado a lado sempre, como no layout; as medidas do uso, lado a lado
// a partir de 54 colunas e empilhadas abaixo disso, sem partir o rótulo. Os
// botões do cabeçalho ficam inteiros.
for(const columns of [48,100,140])test(`the terminal takes the panel's width up to 110 columns · ${columns} columns`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns});
  await eachScreen(f,(screen,tree)=>expect(tree.props.width,screen).toBe(Math.min(columns,110)));
  await f.pane.press({key:'back'});const lines=screenLines(await f.pane.drawn(),columns);
  expect(lines.find(line=>line.includes('584/584'))).toMatch(/584\/584.+41\/42.+366\/367/);
  expect(lines.find(line=>line.includes('Contexto'))).toMatch(columns>=54?/Contexto.+50%.+Cota 5 h.+7%.+Semana.+33%/:/Contexto\s+50%\s+│$/);
  expect(lines.find(line=>line.includes('mustard'))).toMatch(/↻\s+a:\s+⚙\s*$/);
  await f.close();
});

for(const surface of SURFACES)test(`a task with the tests, the verdict and the wave request shows them after the layout's blocks · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{task:extras});await toItems(f);await f.pane.press({key:'item-MSTD-TASK-0133'});
  const titles=surface==='terminal'?termHeadings(await f.pane.drawn()):await headings(f.pane);
  expect(titles).toEqual([...HEADINGS.item,'TESTES']);
  inOrder(reading(await f.pane.drawn()),['26/09, 21:57','TESTES','Revisão: reprovada — Falta a medida no projeto C#.',/^pedido da onda \(/]);
  expect((await boxes(f.pane,{backgroundColor:DARK.warningSoft})).some((box:any)=>reading(box).some(value=>value.startsWith('Revisão: reprovada')))).toBe(true);
  expect((await f.pane.find({key:'wave-request'}))!.props.hotkey).toBe('p');
  await f.close();
});

for(const surface of SURFACES)test(`a task without its wave request has no key for it · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{task:{...extras,wave_request:null}});await toItems(f);await f.pane.press({key:'item-MSTD-TASK-0133'});
  expect(await f.pane.find({key:'wave-request'})).toBeUndefined();await has(f.pane,'Primeira versão');
  await f.close();
});

test('the desktop never lets two neighbouring boxes touch on any screen',async ($,on)=>{
  const f=await open($,on,'desktop');
  await eachScreen(f,(screen,tree)=>expect(touching(tree),screen).toEqual([]));
  await f.close();
});

for(const surface of SURFACES)test(`a version's change reads in words, never by the names of its fields · ${surface}`,async ($,on)=>{
  const task_={history:[{at:'2026-09-26T21:57:00-03:00',author:'assistant',wave:null,changed:null},
    {at:'2026-09-26T22:00:00-03:00',author:'assistant',wave:null,changed:{depends_on:{added:['MSTD-TASK-0002']},files:{added:['a.rs']}}},
    {at:'2026-09-27T10:00:00-03:00',author:'user',wave:null,changed:{priority:{from:null,to:'antes da entrega'}}},
    {at:'2026-09-27T11:00:00-03:00',author:'round',wave:null,changed:{internal_marker:true}}]};
  const f=await open($,on,surface,{task:task_});await toItems(f);await f.pane.press({key:'item-MSTD-TASK-0133'});
  inOrder(await texts(f.pane),['Reescrita','Reescrita com prioridade','Reescrita com dependência e arquivo','Primeira versão']);
  expect(shown(await f.pane.drawn()).filter(value=>/Mudou|depends_on|internal_marker|priority/.test(value))).toEqual([]);
  await f.close();
});

test('the terminal marks the row that holds the focus with the mustard bar and the mustard tint',async ($,on)=>{
  // The person moves the ring with the arrows; the engine lands it where asked.
  on('ui.focus',()=>({}));
  const f=await open($,on,'terminal');await toItems(f);
  await $.ui.focus({component:'Pane',requestId:'mustard-panel',element:'item-MSTD-TASK-0001',origin:{kind:'person'}} as any);
  const tree:any=await f.pane.drawn(),lines=screenLines(tree);
  // Só a linha do foco leva a tinta e a barra mostarda na primeira coluna; as outras guardam a coluna em branco.
  const focused=(await boxes(f.pane,{backgroundColor:DARK.tint})).map((box:any)=>{const bar=kidsOf(kidsOf(box)[0])[0];return [inline(bar),bar.props.color,reading(box).find(value=>/Reabertura/.test(value))];});
  expect(focused).toEqual([['▌',DARK.mustard,'Reabertura de spec fechada volta à execução']]);
  expect(lines.filter(line=>line.includes('▌'))).toHaveLength(1);
  expect(lines.find(line=>line.includes('▌'))).toMatch(/│▌\s*●\s+Reabertura de spec fechada volta à execução/);
  expect(lines.find(line=>line.includes('O veredito gravado'))).toMatch(/│\s+●\s+O veredito gravado/);
  await f.close();
});

// A paleta do layout é a do tema escuro, nas duas superfícies; o tema claro
// é a variante, com as mesmas superfícies nas cores claras.
for(const [theme,palette] of [['dark',DARK],['light',LIGHT]] as const)for(const surface of SURFACES)
  test(`the ${theme} theme paints every screen only in its palette, the page included · ${surface}`,async ($,on)=>{
    const f=await open($,on,surface,{theme});
    await eachScreen(f,(screen,tree)=>{
      expect(tree.props.backgroundColor,screen).toBe(palette.page);
      expect(outside(tree,palette),`${surface} ${screen}`).toEqual([]);
    });
    await f.close();
  });

// A spec no levantamento ainda não tem tarefas nem ondas: no lugar do número, o
// traço, e uma linha que diz quando elas chegam.
for(const surface of SURFACES)test(`a spec in its survey shows dashes and when its tasks and waves are born · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{early:true}),pane=f.pane;
  await has(pane,'painel-mods-igual-ao-desenho');await has(pane,'Levantamento');
  expect((await pane.findAll({type:'Text'})).filter((node:any)=>node.text==='—').length).toBe(3);
  for(const caption of ['tarefas','critérios','ondas'])await has(pane,caption);
  await has(pane,'As tarefas e as ondas nascem depois do levantamento.');
  await lacks(pane,/^0\/0$/);await lacks(pane,'tarefas feitas');
  await has(pane,'Seguir o levantamento e responder os pontos em aberto');
  await f.close();
});

for(const surface of SURFACES)test(`the overview shows the counts of the binary and only the warnings it gave · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface),pane=f.pane,reads=f.reads;
  expect(reads().length).toBe(1);expect(reads()[0]).toContain('overview');expect(reads()[0]).not.toContain('--refresh-consumption');
  for(const figure of ['584/584','41/42','366/367'])await has(pane,figure);
  // A revisão reaberta não é aviso da visão geral: ela fica no Fechamento da spec.
  inOrder(reading(await pane.drawn()),[/PR #289 entrou na base em 05\/10/,/^A onda 193 consta em execução/]);await lacks(pane,/reaberta/);
  await has(pane,'falha-fora-da-obra');await has(pane,'Contexto');await has(pane,/US\$\s?1\.499/);
  await f.close();
});

for(const surface of SURFACES)test(`the overview lists the project's deliveries after the layout's blocks, the newest first · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{patch:{overview:{deliveries}}}),pane=f.pane;
  inOrder(reading(await pane.drawn()),['USO','ENTREGAS DO PROJETO','2 de 12 entregas','Recibo nomeia o programa atrás do plugin',/17\/10\s+·\s+PR #326\s+·\s+2 regras/,
    'O recibo diz qual programa respondeu atrás do plugin.','Mustard mais enxuto']);
  await lacks(pane,/PR #null/);
  await f.close();
});

for(const surface of SURFACES)test(`the next step reads as the binary's sentence, never as its command · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);
  await has(f.pane,'PRÓXIMO PASSO');await has(f.pane,NEXT);
  expect(shown(await f.pane.drawn()).filter(value=>/mustard-rt|--dispatch/.test(value))).toEqual([]);
  await f.pane.press({key:'next-see'});expect(f.section()).toBe('spec');
  await f.close();
});

for(const surface of SURFACES)test(`"Publicar no Cloudflare" publishes the project only on its press, with the command's own answer · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);
  const publishes=()=>f.commands.filter(argv=>argv[2]==='publish');
  const button=await f.pane.find({key:'publish'});
  expect(button).toBeDefined();
  expect(button!.props).toMatchObject(surface==='terminal'?{hotkey:'p',label:'Publicar no Cloudflare'}:{label:'Publicar no Cloudflare',variant:'primary'});
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'main-turn',reason:'answer'});await f.clock.advance(60000);
  expect(publishes()).toEqual([]);
  await f.pane.press({key:'publish'});
  expect(publishes()).toEqual([[expect.stringMatching(/mustard-rt$/),'run','publish','--root','/fixture','--project']]);
  expect(f.during.at(-1).publishing).toBeDefined();
  await has(f.pane,/Os arquivos permanecem locais; ainda não foram publicados/);await has(f.pane,/Grave o token em publication\.apiToken/);
  const command=await $.command.run({command:'mustard:pages',args:'project'});
  expect(publishes().length).toBe(2);
  await has(f.pane,command.text);
  await f.close();
});

for(const surface of SURFACES)test(`no screen shows an internal code · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{task:extras});
  const codes=async (where:string)=>expect(shown(await f.pane.drawn()).filter(value=>CODE.test(value)),where).toEqual([]);
  await codes('overview');
  await f.pane.press({key:'go-spec'});await codes('spec');
  for(const kind of ['criterion','decision','rule','question']) {
    await f.pane.press({key:`items-${kind}`});expect(f.section()).toBe('items');await codes(`items ${kind}`);
    await f.pane.press({key:'back'});
  }
  await f.pane.press({key:'go-items'});await codes('items task');
  await f.pane.press({key:'item-MSTD-TASK-0133'});await codes('item');
  await f.pane.press({key:'back'});await f.pane.press({key:'back'});
  await f.pane.press({key:'go-usage'});await codes('usage');
  await f.close();
});

for(const surface of SURFACES)test(`the keys s, t, c and v move between screens and read only the visible one · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);
  await f.pane.press({key:'go-spec'});expect(f.section()).toBe('spec');await has(f.pane,/fix\/ajuste-na-spec-fechada/);
  await f.pane.press({key:'go-items'});expect(f.section()).toBe('items');
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--kind','task','--limit','8']));
  await f.pane.press({key:'back'});expect(f.section()).toBe('spec');expect(f.reads().at(-1)).toContain('--if-revision');
  // A spec e o consumo são abas: a volta é pela aba Visão geral, que no terminal leva a tecla v.
  if(surface==='terminal')expect((await f.pane.find({key:'go-overview'}))!.props.hotkey).toBe('v');
  await f.pane.press({key:'go-overview'});expect(f.section()).toBe('overview');await has(f.pane,'584/584');
  await f.pane.press({key:'go-usage'});expect(f.section()).toBe('usage');expect(f.reads().at(-1)).not.toContain('--refresh-consumption');
  await f.pane.press({key:'go-overview'});expect(f.section()).toBe('overview');
  // Atualizar relê a tela à vista inteira, sem a condição da revisão.
  await f.pane.press({key:'refresh'});expect(f.section()).toBe('overview');expect(f.reads().at(-1)).not.toContain('--if-revision');
  await f.close();
});

for(const surface of SURFACES)test(`the list puts attention first, filters, searches and shows one more page · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await toItems(f);
  const rows=async ()=>(await f.pane.findAll({type:'Button'})).filter(button=>button.key?.startsWith('item-')).map(button=>button.key);
  // A leitura é de oito por página, e as duas superfícies desenham as oito, como o layout.
  expect((await rows())[0]).toBe('item-MSTD-TASK-0193');expect((await rows()).length).toBe(8);
  await has(f.pane,/Onda 193\s+·\s+em execução sem agente/);await has(f.pane,/^8 de 584/);
  await f.pane.press({key:'more'});
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--offset','8','--limit','8']));
  expect((await rows()).length).toBe(16);await has(f.pane,/^16 de 584/);
  // No terminal a tecla f troca o filtro para o seguinte; no desktop, a pílula dele.
  await f.pane.press({key:surface==='terminal'?'filter':'filter-attention'});
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--filter','attention','--offset','0','--limit','8']));
  expect(await rows()).toEqual(['item-MSTD-TASK-0193']);
  if(surface==='terminal')await f.pane.press({key:'search-open'});
  await f.pane.input({key:'search',text:'gamma'});
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--query','gamma']));
  expect(await rows()).toEqual(['item-MSTD-TASK-0007']);
  // Outro tipo, na aba dele ou em Mais: a lista recomeça na primeira página.
  await f.pane.press({key:'kind-criterion'});expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--kind','criterion','--filter','all']));
  await f.pane.select({key:'kind-more',value:'rule'});expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--kind','rule']));
  await f.close();
});

for(const surface of SURFACES)test(`a task shows the state of its wave and every version, the history open as in the layout · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await toItems(f);await f.pane.press({key:'item-MSTD-TASK-0133'});
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--section','item','--item','MSTD-TASK-0133']));
  await has(f.pane,'Onda 133');await has(f.pane,'Integrada, sem commit');await has(f.pane,'O QUE FOI PEDIDO');
  await has(f.pane,/pergunta em português deve achar/);
  await has(f.pane,'A regra “O scan e a busca funcionam em qualquer idioma do Mustard”. Nenhum critério ligado.');
  await has(f.pane,'.claude/plans/poc-busca/medida-duas-linguas.md');
  await has(f.pane,/^A regra “O scan e a busca funcionam em qualquer idioma do Mustard”\s+·\s+não atendido$/);
  await has(f.pane,/5,04 mi tokens na onda 133/);
  expect(await f.pane.find({key:'history'})).toBeUndefined();
  await has(f.pane,'Primeira versão');await has(f.pane,/Posta na onda 133/);
  await f.pane.press({key:'back'});expect(f.section()).toBe('items');
  await f.close();
});

for(const surface of SURFACES)test(`a task shows the tests run and, on p, its wave's whole request · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{task:extras});await toItems(f);await f.pane.press({key:'item-MSTD-TASK-0133'});
  await has(f.pane,/^testes$/i);await has(f.pane,'cargo test -p mustard-core -- bilingual');
  // O resultado e a data se leem juntos, ao lado do critério, sem a data sozinha numa linha.
  await has(f.pane,/^O critério “A mesma pergunta acha o mesmo código nas duas línguas\.”\s+·\s+passou em 28\/09$/);
  await lacks(f.pane,/^28\/09$/);
  await lacks(f.pane,/Última linha do pedido/);
  const button=await f.pane.find({key:'wave-request'});
  expect(button).toBeDefined();expect(button!.props.hotkey).toBe('p');
  await f.pane.press({key:'wave-request'});
  await has(f.pane,/^pedido da onda$/i);await has(f.pane,/Rode de dentro da cópia/);await has(f.pane,/Última linha do pedido\.$/);
  expect(shown(await f.pane.drawn()).some(value=>value.includes('Linha do pedido da onda. '.repeat(200))),'the whole request').toBe(true);
  await f.pane.press({key:'back'});await f.pane.press({key:'item-MSTD-TASK-0133'});await lacks(f.pane,/Última linha do pedido/);
  await f.close();
});

for(const surface of SURFACES)test(`a task shows its wave reviewer's rejection with the reason and the files pointed at · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{task:extras});await toItems(f);await f.pane.press({key:'item-MSTD-TASK-0133'});
  await has(f.pane,/^Revisão: reprovada — Falta a medida no projeto C#\.$/);
  await has(f.pane,'packages/core/src/search/tests.rs');
  await lacks(f.pane,'Revisão: aprovada');
  await f.close();
});

for(const surface of SURFACES)test(`a task shows its wave reviewer's approval · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{review:{result:'approved',reason:'Conferi o diff e as regras.',files:[],at:'2026-09-28T06:30:00-03:00'}});
  await toItems(f);await f.pane.press({key:'item-MSTD-TASK-0133'});
  expect(await style(f.pane,'Revisão: aprovada')).toMatchObject({color:DARK.success});
  await lacks(f.pane,/reprovada/);await lacks(f.pane,/Conferi o diff/);
  await f.close();
});

for(const surface of SURFACES)test(`the days draw in Svg on desktop and in blocks on the terminal · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-usage'});
  if(surface==='desktop') {
    const svg=(await svgs(f.pane)).find(drawing=>/^Tokens por dia/.test(drawing.alt));
    expect(svg).toBeDefined();expect(String(svg!.alt)).toContain('30/09: 886 mi');expect(String(svg!.alt)).toContain('08/10: sem medida');
    expect(String(svg!.source)).toContain('stroke-dasharray');
  } else {
    expect(await f.pane.find({type:'Svg'})).toBeUndefined();await has(f.pane,'╌╌');
    expect((await f.pane.findAll({type:'Text'})).filter((node:any)=>/^█+$/.test(node.text)).length).toBeGreaterThan(0);
  }
  // O layout põe a primeira, a do meio e a última data embaixo do gráfico, e o dia mais alto em cima.
  await has(f.pane,/^30\/09$/);await has(f.pane,/^07\/10$/);await has(f.pane,/^13\/10$/);await has(f.pane,/máximo 886 mi em 30\/09/);
  // O último dia contra a média da semana, como o layout escreve: 807,8 mi sobre 178,1 mi.
  await has(f.pane,/^\+354% na última semana$/);
  await f.close();
});

// Os agentes das ondas e as skills do Mustard vêm depois dos blocos da tela
// Consumo do layout, nas duas superfícies.
for(const surface of SURFACES)test(`the usage shows each wave's model, effort, skills, reuse and failures after the layout's blocks · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{patch:{usage:{spec:{...usage.spec,...agents}}}});await f.pane.press({key:'go-usage'});
  const titles=surface==='terminal'?termHeadings(await f.pane.drawn()):await headings(f.pane);
  inOrder(titles,[...USAGE_HEADINGS,'AGENTES DAS ONDAS',/^POR ONDA/,'FALHAS','CÓPIAS DAS ONDAS']);
  await has(f.pane,/^onda 367\s+·\s+claude-opus-5-5\s+·\s+esforço high\s+·\s+220 mil tokens$/);
  await has(f.pane,/^onda 366\s+·\s+opus \(pedido\)\s+·\s+esforço pedido xhigh$/);
  await has(f.pane,/add-run-command\s+·\s+continuou um resumo\s+·\s+2 voltas recusadas\s+·\s+1 reprovação da revisão/);
  await has(f.pane,/claude-opus-5-5\s+·\s+esforço pedido xhigh\s+·\s+340 ondas/);
  await has(f.pane,/últimas 2 de 367 ondas enviadas/i);
  await has(f.pane,/30 ondas continuaram um resumo\s+·\s+4 de 10 consultas de conhecimento reaproveitadas/);
  await has(f.pane,/Jev falhou 3 vezes: sem crédito \(3\)/);
  await has(f.pane,/5 bloqueios de gancho: command_guard \(5\)/);
  await has(f.pane,/9 voltas recusadas\s+·\s+2 reprovações da revisão/);
  await f.close();
});

for(const surface of SURFACES)test(`the usage lists the skills Mustard made, with state, tasks, gain and the wave that made each · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{patch:{usage:{mustard_skills:skills}}});await f.pane.press({key:'go-usage'});
  await has(f.pane,/^skills do mustard$/i);
  await has(f.pane,/^falta-teste\s+·\s+ativa\s+·\s+12 tarefas\s+·\s+220 mil tokens na onda que a criou$/);
  await has(f.pane,/^Acertou 3 de 3 com a skill\.$/);
  await has(f.pane,/^parada\s+·\s+retirada\s+·\s+0 tarefas$/);
  await f.close();
});

for(const surface of SURFACES)test(`an idle panel reads nothing and the end of a turn rereads only the visible screen · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);expect(f.reads().length).toBe(1);
  await f.clock.advance(60000);expect(f.reads().length).toBe(1);
  await $.tool.call({tool:'Read',file_path:'/fixture/a'});await f.clock.advance(1000);expect(f.reads().length).toBe(1);
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'agent-turn',agentId:'agent-1',reason:'answer'});await f.clock.advance(1);
  expect(f.reads().length).toBe(1);
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'main-turn',reason:'answer'});await f.clock.advance(1);
  expect(f.reads().length).toBe(2);expect(f.reads()[1]).toEqual(expect.arrayContaining(['--section','overview','--if-revision','rev-overview']));
  await f.pane.press({key:'go-spec'});const before=f.reads().length;
  await $.classic.SubagentStop({stop_hook_active:false,agent_id:'agent-1',agent_transcript_path:'/fixture/agent.jsonl',agent_type:'mustard-wave'});await f.clock.advance(1);
  expect(f.reads().length).toBe(before+1);expect(f.section()).toBe('spec');
  await f.close();const closed=f.reads().length;
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'late',reason:'answer'});await f.clock.advance(1);
  expect(f.reads().length).toBe(closed);
});

for(const surface of SURFACES)test(`"Medir agora" measures only from its button and stays off while it measures · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-usage'});
  const measures=()=>f.commands.filter(argv=>argv.includes('--refresh-consumption')).length;
  // O disco das cópias, ainda não medido, se lê depois da medida.
  expect(measures()).toBe(0);await has(f.pane,/não medido; use Medir agora/);
  await f.pane.press({key:'measure'});
  expect(measures()).toBe(1);expect(f.during.length).toBe(1);
  expect(f.during[0].button).toBeUndefined();expect(f.during[0].note).toBeDefined();
  expect(await f.pane.find({key:'measure'})).toBeDefined();await has(f.pane,/126 GB/);
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'main-turn',reason:'answer'});await f.clock.advance(1);
  expect(measures()).toBe(1);expect(f.reads().at(-1)).toContain('--if-revision');
  await f.close();
});

for(const surface of SURFACES)test(`settings show Jev in four purposes, the counter and a token that never comes back · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-settings'});
  expect(f.commands.at(-1)).toEqual(expect.arrayContaining(['run','settings','--full']));
  for(const purpose of ['jev_wave_planning','jev_context','jev_search','jev_spec_search'])expect(await f.pane.find({key:`setting-${purpose}`}),purpose).toBeDefined();
  await has(f.pane,'Busca na spec');await has(f.pane,/nenhuma escolha gravada/i);
  // Sem escolha gravada ela está desligada: tocar a liga.
  expect(String((await f.pane.find({key:'setting-jev_spec_search'}))!.props.label)).toMatch(/desligado$/);
  expect(await f.pane.find({key:'settings-save'})).toBeUndefined();
  await f.pane.press({key:'setting-jev_spec_search'});
  expect(String((await f.pane.find({key:'setting-jev_spec_search'}))!.props.label)).toMatch(/\bligado$/);await lacks(f.pane,/nenhuma escolha gravada/i);
  // O contador: menos uma onda e mais duas, nunca abaixo de uma.
  await f.pane.press({key:'setting-waves-less'});await has(f.pane,/^4$/);
  await f.pane.press({key:'setting-waves-more'});await f.pane.press({key:'setting-waves-more'});await has(f.pane,/^6$/);
  await f.pane.input({key:'setting-cloudflare_token',text:TOKEN});
  expect(JSON.stringify(await f.pane.drawn())).not.toContain(TOKEN);
  await has(f.pane,/token novo •+ pronto para salvar/);
  await f.pane.press({key:'settings-save'});
  expect(f.patches.at(-1).changes).toEqual({jev_spec_search:true,max_compiling_waves:6,cloudflare_token:TOKEN});
  expect(f.commands.filter(argv=>argv.some(arg=>arg.includes(TOKEN))),'the token is never a process argument').toEqual([]);
  expect(JSON.stringify(await f.pane.drawn())).not.toContain(TOKEN);await has(f.pane,/guardado no mustard\.json/);
  await f.pane.press({key:'back'});expect(f.section()).toBe('overview');
  await f.close();
});

for(const surface of SURFACES)test(`a pending choice brings save and discard, and a conflict keeps the choices until an explicit discard · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{conflict:true});await f.pane.press({key:'go-settings'});
  await f.pane.press({key:'setting-vectors'});await f.pane.press({key:'setting-vectors'});
  expect(await f.pane.find({key:'settings-save'})).toBeUndefined();
  await f.pane.press({key:'setting-rtk'});await has(f.pane,'1 alteração pendente');
  // Salvar no mostarda cheio e descartar ao lado; no terminal nas teclas s e d.
  if(surface==='terminal') {
    expect((await f.pane.find({key:'settings-save'}))!.props.hotkey).toBe('s');expect((await f.pane.find({key:'settings-reload'}))!.props.hotkey).toBe('d');
    expect(await style(f.pane,' Salvar ')).toMatchObject({backgroundColor:DARK.mustard,color:DARK.buttonInk});
  } else expect((await f.pane.find({key:'settings-save'}))!.props.variant).toBe('primary');
  await f.pane.press({key:'settings-save'});
  // A chave do RTK diz o estado no nome do botão; o botão desenha só o interruptor.
  const rtk=async ()=>String((await f.pane.find({key:'setting-rtk'}))!.props.label);
  expect(f.patches.at(-1).changes).toEqual({rtk:false});await has(f.pane,/mudou fora do painel/);
  expect(await f.pane.find({key:'settings-save'})).toBeDefined();expect(await rtk()).toMatch(/^RTK · enxuga a saída dos comandos\s+·\s+desligado$/);
  await f.pane.press({key:'settings-reload'});
  expect(await f.pane.find({key:'settings-save'})).toBeUndefined();expect(await rtk()).toMatch(/^RTK · enxuga a saída dos comandos\s+·\s+ligado$/);
  expect(f.patches.length).toBe(1);
  await f.close();
});

// O que a revisão do layout pediu, um teste por pedido.

// 1. As outras specs: o nome em negrito, texto solto como no layout, não botão.
for(const surface of SURFACES)test(`the other specs read their name as bold text, never as a button · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);
  for(const name of ['falha-fora-da-obra','mustard-enxuto']) {
    expect(await f.pane.find({key:`spec-${name}`}),name).toBeUndefined();
    expect(nodesOf(await f.pane.drawn(),'Button').filter((node:any)=>reading(node).includes(name)),name).toEqual([]);
    expect(await style(f.pane,name)).toMatchObject(surface==='terminal'?{bold:true}:{weight:'semibold'});
  }
  await f.close();
});

// 2. Os seis cartões do combinado são desenhados do mesmo jeito: a mesma caixa,
// e o número de cada um é o botão dele; o das ondas abre a tela das ondas.
for(const surface of SURFACES)test(`the six boxes of the agreement are drawn alike, each number its own button · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);const tree:any=await reach(f,'spec');
  const tiles=nodesOf(tree,'Box').filter((node:any)=>node.props.width==='30%' && node.props.backgroundColor===DARK.card);
  expect(tiles.length).toBe(6);
  const look=(box:any)=>({...box.props,children:undefined});
  for(const box of tiles)expect(look(box)).toEqual(look(tiles[0]));
  expect(tiles.map((box:any)=>[kidsOf(box)[0].type,inline(kidsOf(kidsOf(box)[0])[0])])).toEqual([['Button','584'],['Button','42'],['Button','22'],['Button','154'],['Button','66'],['Button','367']]);
  await f.pane.press({key:'go-waves'});expect(f.section()).toBe('waves');
  await f.close();
});

// 3 e 6. As abas do tipo: cada uma inteira numa linha, nunca encolhida pela
// vizinha; no terminal, com ar visível entre elas.
test('the desktop kind tabs never shrink below their words',async ($,on)=>{
  const f=await open($,on,'desktop');const tree:any=await reach(f,'items');
  const tabs=nodesOf(tree,'Box').filter((node:any)=>kidsOf(node).some((kid:any)=>/^kind-(task|criterion|decision)$/.test(kid.props?.key ?? '')));
  expect(tabs.map((box:any)=>[box.props.flexShrink,box.props.width])).toEqual(Array(3).fill([0,undefined]));
  await f.close();
});
for(const columns of [56,100])test(`the terminal kind tabs stay whole on one line, with air between them · ${columns} columns`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns});const lines=screenLines(await reach(f,'items'),columns);
  const line=lines.find(value=>value.includes('Tarefas 584'))!;
  expect(line).toMatch(/Tarefas 584\s{2,}Critérios 42\s{2,}Decisões 154\s{2,}Mais ▾/);
  await f.close();
});

// 4. A spec no levantamento, com os dados reais dela: zero de zero não diz
// "todas feitas" nem "todos com prova", e o bloco das ondas não aparece vazio.
const surveySpec={spec:{name:'painel-mods-igual-ao-desenho',goal:'Refazer o painel mods igual ao desenho aprovado',phase:'survey',branch:'fix/painel-mods-igual-ao-desenho',base:'dev',
  opened_at:'2026-10-20T18:16:29.486033244-03:00',last_commit:null,commits:0,
  counts:{tasks:{total:0,finished:0,backlog:0},criteria:{total:0,passed:0,failed:0,without_proof:0},questions:{total:6,open:5},decisions:1,rules:5,waves:0},
  waves:{planned:0,finished:0,committed:0,integrated:0,running:0,todo:0,no_agent:[],rejected:[]},review:{valid:false,approved_at:null,reopened_at:null},pr:null},
  selected_spec:'painel-mods-igual-ao-desenho'};
for(const surface of SURFACES)test(`a spec in its survey states nothing for zero tasks or criteria and draws no empty waves block · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{patch:{spec:surveySpec}});const tree:any=await reach(f,'spec');
  for(const value of ['todas feitas','todos com prova',/feitas$/])await lacks(f.pane,value);
  expect(reading(tree).filter(value=>value==='ONDAS')).toEqual([]);
  expect(nodesOf(tree,surface==='terminal'?'Box':'Svg').filter((node:any)=>/comitadas|integradas/.test(node.props?.alt ?? ''))).toEqual([]);
  // As seis caixas seguem, com o número e o tipo.
  for(const figure of ['0','5','1'])await has(f.pane,figure);
  await f.close();
});

// 5. A 56 colunas o terminal quebra só onde o layout faz pausa, e o que cabe fica numa linha.
test('at 56 columns the terminal keeps on one line what fits and breaks the rest only at its pauses',async ($,on)=>{
  const f=await open($,on,'terminal',{columns:56});
  const at=async (screen:string)=>screenLines(await reach(f,screen),56);
  let lines=await at('overview');
  // As barras dos três quadros na mesma linha, mesmo com legendas de alturas diferentes.
  expect(lines.filter(line=>/▄{4,}/.test(line))[0]).toMatch(/▄{4,}\s+▄{4,}\s+▄{4,}/);
  for(const whole of ['Encerrar a onda 193 e entregar a spec'])expect(lines.some(line=>line.includes(whole)),whole).toBe(true);
  expect(lines.some(line=>/● Nenhuma onda trabalhando\s+1 agente na sessão/.test(line))).toBe(true);
  await f.pane.press({key:'go-spec'});lines=screenLines(await f.pane.drawn(),56);
  expect(lines.some(line=>line.includes('reaberta às 17:13'))).toBe(true);
  expect(lines.filter(line=>/^[│\s]*17:13[│\s]*$/.test(line))).toEqual([]);
  await f.pane.press({key:'go-items'});lines=screenLines(await f.pane.drawn(),56);
  expect(lines.some(line=>line.includes('as que pedem atenção vêm primeiro'))).toBe(true);
  await f.pane.press({key:'item-MSTD-TASK-0133'});lines=screenLines(await f.pane.drawn(),56);
  // A etiqueta, a onda e a data nunca se partem: o que não cabe desce inteiro.
  for(const whole of ['● Integrada, sem commit','Onda 133','Entregue em 28/09, 06:38'])expect(lines.some(line=>line.includes(whole)),whole).toBe(true);
  await f.pane.press({key:'back'});await f.pane.press({key:'back'});await f.pane.press({key:'go-usage'});lines=screenLines(await f.pane.drawn(),56);
  // O aviso com o ▲ na primeira linha do texto, e o número do Jev junto da unidade.
  expect(lines.find(line=>line.includes('Medido até'))).toMatch(/▲ Medido até/);
  expect(lines.some(line=>line.includes('5,4 mi tokens'))).toBe(true);
  await f.close();
});

// 5. O aviso da medida, quando o texto toma três linhas, leva o ▲ na primeira delas.
test('the usage warning keeps its icon on the first line of a long text on the terminal',async ($,on)=>{
  const f=await open($,on,'terminal',{columns:48});const lines=screenLines(await reach(f,'usage'),48);
  const at=lines.findIndex(line=>line.includes('▲'));
  expect(lines[at]).toMatch(/▲ Medido até/);expect(lines[at+2]).toMatch(/\S/);
  await f.close();
});

// 7. O número de cada cartão nunca encosta no fundo: a borda fica uma linha
// acima e uma abaixo dele, e dentro dela não há linha vazia, que a borda já é a folga.
test('the agreement boxes and the spec cards of the usage sit right inside their border on the terminal',async ($,on)=>{
  const f=await open($,on,'terminal');
  const tiles=nodesOf(await reach(f,'spec'),'Box').filter((node:any)=>node.props.width==='30%' && node.props.backgroundColor===DARK.card);
  expect(tiles.map((box:any)=>[box.props.borderStyle,box.props.paddingY])).toEqual(Array(6).fill(['round',0]));
  await f.pane.press({key:'go-usage'});
  const cards=nodesOf(await f.pane.drawn(),'Box').filter((node:any)=>node.props.width==='40%');
  expect(cards.map((box:any)=>[reading(box)[0],box.props.borderStyle,box.props.paddingY])).toEqual([['Este projeto','round',0],['A máquina inteira','round',0],['Ondas','round',0],['Condutor','round',0]]);
  await f.close();
});

// 8. Todo cartão do terminal tem a borda fina e redonda do layout, em #2A2926.
test('every terminal card has the thin round border of the layout',async ($,on)=>{
  const f=await open($,on,'terminal');
  await eachScreen(f,(screen,tree)=>{
    const cards=nodesOf(tree,'Box').filter((node:any)=>node.props.backgroundColor===DARK.card && node.props.flexDirection==='column');
    expect(cards.length,screen).toBeGreaterThan(0);
    expect(cards.filter((box:any)=>box.props.borderStyle!=='round' || box.props.borderColor!==DARK.line).map((box:any)=>reading(box)[0]),screen).toEqual([]);
  });
  await f.close();
});

// 9. Mais se lê "Mais ▾": o próprio nome é a primeira opção, sem rótulo antes do valor.
for(const surface of SURFACES)test(`the Mais picker reads its own name and no label before it · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await reach(f,'items');
  const more=(await f.pane.find({key:'kind-more'}))!;
  expect(more.props.label).toBeUndefined();expect(more.props.value).toBe('more');expect(more.props.options[0]).toEqual({value:'more',label:'Mais'});
  if(surface==='terminal')expect(screenLines(await f.pane.drawn()).find(line=>line.includes('Mais'))).toMatch(/Mais ▾/);
  // Escolher Mais de novo não relê nada; Regras abre a lista delas e o seletor passa a mostrá-las.
  const before=f.reads().length;await f.pane.select({key:'kind-more',value:'more'});expect(f.reads().length).toBe(before);
  await f.pane.select({key:'kind-more',value:'rule'});expect((await f.pane.find({key:'kind-more'}))!.props.value).toBe('rule');
  await f.close();
});

// 10. Nenhuma palavra se parte e nenhuma linha passa da tela, de 40 a 100 colunas.
for(const columns of [40,56,80,100])test(`no word is split and no line passes the screen on any terminal screen · ${columns} columns`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns});
  const found:Record<string,string[]>={};
  await eachScreen(f,(screen,tree)=>{split.length=0;spill.length=0;screenLines(tree,columns);found[screen]=[...split,...spill];});
  expect(found).toEqual({overview:[],spec:[],items:[],item:[],usage:[],settings:[]});
  await f.close();
});

// 11. O ponto de cor da linha da lista não encolhe quando o título é longo.
for(const surface of SURFACES)test(`the colored dot of a list row never shrinks · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);const tree:any=await reach(f,'items');
  const dots=nodesOf(tree,'Box').filter((node:any)=>kidsOf(node).length===1 && kidsOf(node)[0].type==='Text' && inline(kidsOf(node)[0])==='●');
  expect(dots.length).toBe(8);expect(dots.map((box:any)=>box.props.flexShrink)).toEqual(Array(8).fill(0));
  await f.close();
});

// 12. "hoje" compara os dois instantes no relógio de quem olha: a leitura em
// UTC já no dia seguinte e a spec atualizada um minuto antes ainda são de hoje.
for(const surface of SURFACES)test(`"hoje" compares the reading and the spec on the viewer's clock · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{patch:{overview:{at:'2026-10-21T01:30:00Z',others:{total:14,attention:1,shown:[{name:'falha-fora-da-obra',phase:'running',
    updated:'2026-10-20T22:29:00-03:00',warning:{kind:'pr-merged',pr:326,at:'2026-10-20T16:52:14-03:00'}}]}}}});
  await has(f.pane,'Execução · hoje 22:29');await lacks(f.pane,'Execução · 20/10');
  await f.close();
});

// A segunda revisão do layout, um teste por pedido.

// As respostas reais do binário para este repositório em 21/10, com a spec no
// levantamento: a branch longa, as entregas do projeto e o custo de duas specs.
const real={project:{name:'mustard',branch:'fix/auditoria-spec-e-ondas',version:'0.2.25',language:'pt-BR'}};
const realOverview={ok:true,schema_version:1,section:'overview',revision:'real-overview',at:'2026-10-21T03:52:35.917447181+00:00',project:real.project,selected_spec:'painel-mods-igual-ao-desenho',
  spec:{name:'painel-mods-igual-ao-desenho',goal:'Refazer o painel mods igual ao desenho aprovado',phase:'survey',tasks:{total:0,finished:0,backlog:0},criteria:{total:0,passed:0,failed:0,without_proof:0},
    waves:{finished:0,planned:0},next:{command:'mustard-rt run grill --spec painel-mods-igual-ao-desenho',text:'Seguir o levantamento e responder os pontos em aberto'},
    air:{running:0,no_agent:[],free_slots:5,capacity:5,backlog:0}},warnings:[],others:{total:1,attention:0,shown:[]},
  deliveries:{total:9,shown:[{spec:'regras-de-bloqueio-do-git',delivered_at:'2026-10-06',pr:293,title:'Três regras de bloqueio de git passam a barrar de verdade.',
      delivered:'Três regras de bloqueio de git do molde do instalador estavam erradas e não barravam; foram reescritas, e a atualização passa a respeitar a regra que a pessoa moveu para outra lista. A resposta de…',rules:0},
    {spec:'levantamento-sem-script',delivered_at:'2026-10-06',pr:291,title:'O levantamento é a conversa do começo de uma spec.',
      delivered:'Leitura do levantamento, sem mudança; Grill grava pontos e lembra o tipo; Grill descarta fato de arquivo apagado; Onda parada antes de editar; Conserta testes do grill que grava pontos; Despacho de…',rules:0},
    {spec:'cesta-de-tarefas',delivered_at:'2026-09-23',pr:285,title:'Objetivo: o binário forma as ondas a partir do backlog.',delivered:'Objetivo: o binário forma as ondas a partir do backlog.',rules:3}]},
  usage:{cost_micro_usd:2618127458,sessions:48,tokens:21904658267,measured_through:'2026-10-13',session_agents:null}};
const realSpec={ok:true,schema_version:1,section:'spec',revision:'real-spec',project:real.project,selected_spec:'painel-mods-igual-ao-desenho',...surveySpec,
  specs:[{name:'painel-mods-igual-ao-desenho',phase:'survey',created:'2026-10-20T18:16:29.486033244-03:00',updated:'2026-10-20T18:40:25.375863737-03:00'}],discarded:0};
const realUsage={...usage,revision:'real-usage',project:real.project,selected_spec:'painel-mods-igual-ao-desenho',machine_tokens:30856359881,
  trend:{day:'2026-10-19',tokens:927047130,average_tokens:538755942,ratio:1.7},
  cost:{total_micro_usd:2618127458,sessions:48,outside_spec_micro_usd:1415632808,by_spec:[{name:'falha-fora-da-obra',micro_usd:1199695824},{name:'painel-mods-igual-ao-desenho',micro_usd:2798826}],specs_without_cost:0},
  spec:{name:'painel-mods-igual-ao-desenho',wave_tokens:0,waves_without_measure:0,conductor_tokens:null,waves_sent:0,waves:[],models:[],skills:[],reuse:{summaries:0,knowledge_calls:1,knowledge_cached:0},
    failures:{jev:{calls:0,reasons:{},reserves:0},hook_blocks:{total:3,by_hook:{command_guard:1,write_gate:2}},refused_returns:0,review_rejections:0}},
  tools:{rtk:{configured:true,status:'measured',commands:54112,saved_tokens:71580138,percent:68.90729102135397},
    jev:{calls:1164,by_purpose:{search:22,'wave-planning':853,context:289},input_tokens:5629502,cost_micro_usd:null},search:{total:8732,in_spec:10}},
  session:{started_at:null,agents:null,working:null}};
const realPatch={overview:realOverview,spec:realSpec,usage:realUsage};

// C. Nenhuma palavra se parte nem linha passa da tela com os dados reais, de 40 a
// 100 colunas: a branch longa da spec quebra depois de uma barra ou de um hífen.
for(const columns of [40,56,80,100])test(`no word is split with the binary's real answers · ${columns} columns`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns,patch:realPatch});
  const found:Record<string,string[]>={};
  await eachScreen(f,(screen,tree)=>{split.length=0;spill.length=0;screenLines(tree,columns);found[screen]=[...split,...spill];});
  expect(found).toEqual({overview:[],spec:[],items:[],item:[],usage:[],settings:[]});
  // A branch da spec, quando quebra, quebra só depois de uma barra ou de um hífen.
  await f.pane.press({key:'back'});await f.pane.press({key:'go-spec'});
  const branch=nodesOf(await f.pane.drawn(),'Text').map(inline).find((value:string)=>value.replace(/\n/g,'')==='fix/painel-mods-igual-ao-desenho')!;
  expect(branch).toBeDefined();expect(branch.split('\n').slice(0,-1).every((line:string)=>/[\/-]$/.test(line))).toBe(true);
  await f.close();
});

// A. O estado da revisão fica à direita, como o do pull request: numa linha
// quando cabe e, a 56 colunas, quebrado depois da vírgula, cada linha rente à borda.
// Quando não cabe ao lado do nome, o estado desce inteiro para a linha de
// baixo, à direita, e só quebra na vírgula se nem sozinho couber: no máximo duas linhas.
for(const columns of [100,56,40])test(`the review state sits on the right of the closing card · ${columns} columns`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns});const lines=screenLines(await reach(f,'spec'),columns);
  const at=lines.findIndex(line=>line.includes('Revisão final')),below=lines.slice(at+1,lines.findIndex((line,n)=>n>at && /│─+│/.test(line)));
  if(columns===100) {
    expect(lines[at]).toMatch(/Revisão final\s{3,}aprovada em 02\/10, reaberta às 17:13 {2}│$/);expect(below).toEqual([]);
    expect(lines.find(line=>line.includes('Pull request #289'))).toMatch(/Pull request #289\s{3,}entrou na base em 05\/10 {2}│$/);
  } else {
    expect(lines[at]).toMatch(/Revisão final\s+│$/);
    expect(below,'no máximo duas linhas, rentes à borda').toEqual(columns===56?[expect.stringMatching(/^\s+│\s{3,}aprovada em 02\/10, reaberta às 17:13 {2}│$/)]
      :[expect.stringMatching(/^\s+│\s{3,}aprovada em 02\/10, {2}│$/),expect.stringMatching(/^\s+│\s{3,}reaberta às 17:13 {2}│$/)]);
  }
  await f.close();
});

// B. Onde o layout afasta dois textos 12 px ou mais, o terminal os afasta duas colunas ou mais.
test('texts the layout keeps 12 px apart or more stay two columns apart or more on the terminal',async ($,on)=>{
  const f=await open($,on,'terminal',{columns:100});
  const all=async (screen:string)=>screenLines(await reach(f,screen),100).join('\n');
  let text_=await all('overview');
  for(const pair of [/50%\s{2,}Cota 5 h/,/7%\s{2,}Semana/,/↻ \s{2,}a:/,/trabalhando\s{2,}\d agente/,/Visão geral\s{3,}s: Spec/])expect(text_,String(pair)).toMatch(pair);
  await f.pane.press({key:'go-spec'});text_=screenLines(await f.pane.drawn(),100).join('\n');
  expect(text_).toMatch(/361 comitadas\s{2,}■ 5 integradas/);
  await f.pane.press({key:'go-items'});text_=screenLines(await f.pane.drawn(),100).join('\n');
  expect(text_).toMatch(/●\s{2,}Arquivo novo ou apagado/);
  await f.pane.press({key:'item-MSTD-TASK-0133'});text_=screenLines(await f.pane.drawn(),100).join('\n');
  for(const pair of [/Integrada, sem commit \s{2,}Onda 133/,/Onda 133\s{2,}Entregue em 28\/09, 06:38/,/●\s{2,}Posta na onda 133/])expect(text_,String(pair)).toMatch(pair);
  await f.close();
});

// D. Cada cartão fica no máximo 1,25 vez a altura do mesmo cartão no layout,
// e cada tela 1,1 vez, a 100 colunas. A altura do layout é a medida no Chrome,
// em px, dividida pela linha dele, de 21 px.
const LAYOUT:Record<string,{screen:number,cards:number[]}>={
  overview:{screen:1158,cards:[438,53,137,124]},spec:{screen:1288,cards:[201,115,79,98,97,298]},items:{screen:1196,cards:[742]},
  item:{screen:1441,cards:[118,97,249,312,179]},usage:{screen:1531,cards:[113,198,204,100,223,186]},settings:{screen:1092,cards:[187,218,116,274]}};
// A altura de cada cartão do terminal, da linha da borda de cima à de baixo, na ordem da tela.
const heights=(lines:string[])=>{const out:number[]=[];let open_:number|undefined;
  lines.forEach((line,n)=>{const t=line.trim();if(/^[╭]/.test(t) && open_==null)open_=n;else if(/^[╰]/.test(t) && open_!=null){out.push(n-open_+1);open_=undefined;}});return out;};
// A 56 colunas o Fechamento é a exceção: o estado da revisão (36 colunas) não
// cabe ao lado do nome (13) nas 46 do cartão e desce para a linha de baixo,
// e o cartão fica com 6 linhas, 1,30 vez as 4,6 do layout. Sem tirar a
// divisória do layout entre as duas linhas, ele não fica menor.
const ABOVE:Record<string,number>={'56 spec 5':6};
for(const columns of [100,56])test(`every terminal card stays within 1.25 times its layout height and every screen within 1.1 times · ${columns} columns`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns});
  const measured:Record<string,any>={};
  await eachScreen(f,(screen,tree)=>{const lines=screenLines(tree,columns);measured[screen]={screen:lines.length,cards:heights(lines)};});
  for(const [screen,want] of Object.entries(LAYOUT)) {
    const got=measured[screen];
    expect(got.screen/(want.screen/21),`${screen}: ${got.screen} linhas`).toBeLessThanOrEqual(1.1);
    // Os cartões do layout vêm primeiro; o que vem depois deles (as cópias das ondas) não tem par no layout.
    want.cards.forEach((px,n)=>{const exception=ABOVE[`${columns} ${screen} ${n+1}`];
      if(exception)expect(got.cards[n],`${screen} cartão ${n+1}`).toBe(exception);
      else expect(got.cards[n]/(px/21),`${screen} cartão ${n+1}: ${got.cards[n]} linhas`).toBeLessThanOrEqual(1.25);});
  }
  await f.close();
});

// E. No terminal, que mede em células, a soma das larguras mínimas das três
// abas e do Mais cabe nas 56 colunas, a largura do layout na letra fixa, e
// as quatro ficam numa linha.
test('the kind tabs and Mais add up to no more than the 56 columns of the layout width on the terminal',async ($,on)=>{
  const f=await open($,on,'terminal',{columns:56});const tree:any=await reach(f,'items');
  const bar=nodesOf(tree,'Box').find((node:any)=>kidsOf(node).some((kid:any)=>kidsOf(kid).some((inner:any)=>inner.props?.key==='kind-task')))!;
  const least=kidsOf(bar).reduce((sum:number,kid:any)=>sum+natural(kid),0)+cells(bar.props.columnGap,false)*(kidsOf(bar).length-1)+2*cells(bar.props.paddingX ?? bar.props.padding,false);
  const room=56-2*cells(tree.children[1].props.paddingX,false);
  expect(least).toBeLessThanOrEqual(room);
  expect(screenLines(tree,56).some(line=>/Tarefas 584\s+Critérios 42\s+Decisões 154\s+Mais ▾/.test(line))).toBe(true);
  await f.close();
});
// No desktop o teste prova só a construção: a linha das abas quebra e nada
// nela encolhe abaixo do próprio texto, então nenhuma aba parte em duas linhas
// e o seletor não se corta. Ele não prova que as quatro cabem numa linha: a
// largura do botão e do seletor nativos é do aplicativo, e o kit não a desenha.
test('on the desktop nothing in the kind tabs shrinks below its words and what does not fit drops whole',async ($,on)=>{
  const f=await open($,on,'desktop');const tree:any=await reach(f,'items');
  const bar=nodesOf(tree,'Box').find((node:any)=>kidsOf(node).some((kid:any)=>kidsOf(kid).some((inner:any)=>inner.props?.key==='kind-task')))!;
  expect(bar.props.flexWrap).toBe('wrap');
  expect(kidsOf(bar).map((kid:any)=>[kid.props.flexShrink,kid.props.width])).toEqual(Array(4).fill([0,undefined]));
  expect(kidsOf(kidsOf(bar).at(-1))[0].props.key).toBe('kind-more');
  await f.close();
});

// 56 colunas: tudo que o layout põe numa linha fica numa linha ou quebra só numa pausa.
for(const [answers,patch] of [['example',{}],['real',realPatch]] as const)test(`at 56 columns every pause-broken text keeps each piece whole on a line · ${answers} answers`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns:56,patch});
  const broken_:Record<string,string[]>={};
  await eachScreen(f,(screen,tree)=>{const lines=screenLines(tree,56);
    const pieces:string[]=[];walk(tree,node=>{if(phrase(node)!=null)for(const kid of kidsOf(node))pieces.push(inline(kid).trim());});
    broken_[screen]=pieces.filter(piece=>!lines.some(line=>line.includes(piece)));});
  expect(broken_).toEqual({overview:[],spec:[],items:[],item:[],usage:[],settings:[]});
  await f.close();
});

// A terceira revisão do layout.

// 1. No desktop o nome longo da spec se corta com reticências e a etiqueta da
// fase fica à direita dele, na mesma linha, como no layout.
test('the desktop overview keeps the phase pill on the right of a long spec name',async ($,on)=>{
  const f=await open($,on,'desktop',{patch:{overview:realOverview}});const tree:any=await f.pane.drawn();
  const title=nodesOf(tree,'Box').find((node:any)=>kidsOf(node).some((kid:any)=>kidsOf(kid).some((inner:any)=>inline(inner)==='painel-mods-igual-ao-desenho')))!;
  expect(title.props.flexWrap).toBe('nowrap');
  expect(kidsOf(title)[0].props).toMatchObject({flexShrink:1,minWidth:0});expect(kidsOf(kidsOf(title)[0])[0].props).toMatchObject({wrap:'truncate-end'});
  expect(kidsOf(title)[1].props.flexShrink).toBe(0);expect(reading(kidsOf(title)[1])).toEqual(['●','Levantamento']);
  await f.close();
});

// 3. A 56 colunas os quatro dados da Spec ficam numa grade de duas colunas: a
// branch quebra no hífen e nenhuma palavra inteira deixa de caber na metade.
for(const [answers,patch] of [['example',{}],['real',realPatch]] as const)test(`the spec facts stay in two columns at 56 columns · ${answers} answers`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns:56,patch});const lines=screenLines(await reach(f,'spec'),56);
  expect(lines.find(line=>line.includes('Branch'))).toMatch(/Branch\s+Sai de/);
  expect(lines.find(line=>line.includes('Aberta'))).toMatch(/Aberta\s+Último commit/);
  await f.close();
});

// 4. Em "Esta sessão" a nota dos agentes fica a uma linha em branco da última barra.
test('the session note keeps one blank line below the last bar on the terminal',async ($,on)=>{
  const f=await open($,on,'terminal',{columns:56});const lines=screenLines(await reach(f,'usage'),56);
  const at=lines.findIndex(line=>line.includes('agentes observados'));
  expect(lines[at-1]).toMatch(/^\s+│\s+│$/);expect(lines[at-2]).toMatch(/▄{10,}/);
  await f.close();
});

// 5. "Ondas no ar": a 56 colunas o agente cabe ao lado; a 55 ele desce inteiro
// para a linha de baixo, à direita, como no layout.
for(const columns of [56,55])test(`the waves in the air card breaks well · ${columns} columns`,async ($,on)=>{
  const f=await open($,on,'terminal',{columns});const lines=screenLines(await f.pane.drawn(),columns);
  const at=lines.findIndex(line=>line.includes('Nenhuma onda trabalhando'));
  if(columns===56)expect(lines[at]).toMatch(/● Nenhuma onda trabalhando\s{2,}1 agente na sessão {2}╎$/);
  else {expect(lines[at]).toMatch(/● Nenhuma onda trabalhando\s+╎$/);expect(lines[at+1]).toMatch(/^\s+╎\s{10,}1 agente na sessão {2}╎$/);}
  await f.close();
});

// A tela das ondas, a sétima: o cartão das ondas da spec a abre, cada onda
// traz o que levou, o que entregou e o próprio consumo, e as páginas trazem
// todas, da mais nova para a mais antiga.
const dotColors=async (pane:any)=>(await pane.findAll({type:'Text'})).filter((node:any)=>node.text==='●').map((node:any)=>node.props.color);
const WAVES:(string|RegExp)[]=['Visão geral','Spec','Consumo','ajuste-na-spec-fechada','ONDAS · 208','página 1 de 26',
  'Onda 208','em execução sem agente desde 17/10','A onda lê cada item só com o título e a parte do agente','levou','1 tarefa · 2 arquivos',
  'entregou','nada ainda · sem commit','gastou','— tokens · tempo — · opus (pedido) · esforço pedido xhigh',
  'Onda 207','em execução sem agente desde 17/10','1 tarefa · 13 arquivos',
  'Onda 206','comitada em 17/10','Recibo da instalação nomeia o programa atrás do plugin','1 tarefa · 1 arquivo','1 feita · 1 arquivo · commit 15db4cd',
  '1,09 mi tokens · 11 min · claude-opus-5-5 · esforço pedido xhigh',
  'Onda 205','comitada em 16/10','Recibo de prova guarda o resultado do cargo mesmo atrás do pipe','· e mais 5','6 tarefas · 10 arquivos',
  '1 feita, 5 voltaram · 6 arquivos · commit 9b91cd2','8,85 mi tokens · 1 h 5 min · claude-opus-5-5 · esforço pedido xhigh',
  'Onda 202','2 feitas, 1 voltou · 4 arquivos · commit 9b91cd2','7,5 mi tokens · 1 h 5 min · claude-opus-5-5 · esforço pedido xhigh',
  'Onda 201','5 tarefas · 20 arquivos','9,4 mi tokens · 1 h 5 min · claude-opus-5-5 · esforço pedido xhigh','8 de 208 · da mais nova para a mais antiga'];
for(const surface of SURFACES)test(`the waves card of the spec opens the waves screen, each wave with its own spend, in the layout's palette · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await reach(f,'spec');
  await f.pane.press({key:'go-waves'});const tree:any=await f.pane.drawn();
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--section','waves','--offset','0','--limit','8']));
  inOrder(reading(tree),[...WAVES,surface==='terminal'?'Próxima':'Próxima ›']);
  expect(surface==='terminal'?termHeadings(tree):await headings(f.pane)).toEqual(['ONDAS · 208']);
  expect(outside(tree,DARK)).toEqual([]);
  // As que pedem atenção no fundo delas, o ponto e o estado no laranja; as comitadas no verde.
  expect((await boxes(f.pane,{backgroundColor:DARK.attention})).map((box:any)=>reading(box)[1])).toEqual(['Onda 208','Onda 207']);
  expect(await dotColors(f.pane)).toEqual([DARK.warning,DARK.warning,...Array(6).fill(DARK.success)]);
  expect(await style(f.pane,'comitada em 17/10')).toMatchObject({color:DARK.success});
  // A aba Spec segue acesa e a volta leva o nome da spec; no terminal, a volta no v e a página seguinte no m.
  expect((await f.pane.find({key:'back'}))!.props.label).toBe(surface==='terminal'?'‹ ajuste-na-spec-fechada':'ajuste-na-spec-fechada');
  if(surface==='terminal'){expect(hotkeys(tree)).toEqual(['go-settings: a','go-usage: c','back: v','waves-next: m']);expect(glued(tree)).toEqual([]);expect(merged(tree)).toEqual([]);}
  else {expect(hotkeys(tree)).toEqual([]);expect(touching(tree)).toEqual([]);}
  await f.close();
});

for(const surface of SURFACES)test(`the waves screen pages through every wave, newest first · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await reach(f,'spec');await f.pane.press({key:'go-waves'});
  await lacks(f.pane,/Anterior/);
  const seen:number[]=[];
  for(let page=1;page<=26;page++) {
    const tree:any=await f.pane.drawn();
    seen.push(...reading(tree).filter(value=>/^Onda \d+$/.test(value)).map(value=>Number(value.slice(5))));
    await has(f.pane,`página ${page} de 26`);
    // Entre a primeira e a última, as duas páginas: no terminal a anterior no r e a seguinte no m.
    if(page===2)expect(hotkeys(tree)).toEqual(surface==='terminal'?['go-settings: a','go-usage: c','back: v','waves-previous: r','waves-next: m']:[]);
    if(page<26)await f.pane.press({key:'waves-next'});
  }
  expect(seen).toEqual(Array.from({length:208},(_,n)=>208-n));
  expect(await f.pane.find({key:'waves-next'})).toBeUndefined();await has(f.pane,'208 de 208 · da mais nova para a mais antiga');
  await f.pane.press({key:'waves-previous'});await has(f.pane,'página 25 de 26');
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--section','waves','--offset','192']));
  // A volta leva à spec, e o cartão abre de novo na primeira página.
  await f.pane.press({key:'back'});expect(f.section()).toBe('spec');
  await f.pane.press({key:'go-waves'});await has(f.pane,'página 1 de 26');
  await f.close();
});

// A resposta real do binário para esta spec em 22/10: os números das ondas
// com lacunas, as ondas no ar, as comitadas com o commit inteiro.
const realWaves={"ok": true, "schema_version": 1, "section": "waves", "revision": "real-waves", "at": "2026-10-22T19:48:45.752808036+00:00", "project": {"name": "mustard", "branch": "fix/uso-real-da-0-2-27", "version": "0.2.27", "language": "pt-BR"}, "selected_spec": "uso-real-da-0-2-27", "total": 12, "offset": 0, "limit": 8, "rows": [{"wave": 19, "title": "Recusar uma onda não trava o despacho dela", "state": "running", "at": "2026-10-22T16:47:56.112626918-03:00", "attention": null, "tasks": 2, "returned": 0, "files": 9, "delivered_files": null, "commit": null, "seconds": null, "model_used": null, "model_asked": "opus", "effort_asked": "xhigh", "effort_used": "xhigh", "tokens": null}, {"wave": 18, "title": "Tela das ondas no painel, com o consumo de cada uma", "state": "running", "at": "2026-10-22T16:35:32.926424962-03:00", "attention": null, "tasks": 3, "returned": 0, "files": 5, "delivered_files": null, "commit": null, "seconds": null, "model_used": null, "model_asked": "opus", "effort_asked": "xhigh", "effort_used": "xhigh", "tokens": null}, {"wave": 17, "title": "reused_results do ponto pelo código na leitura enxuta", "state": "running", "at": "2026-10-22T16:30:06.046529344-03:00", "attention": null, "tasks": 2, "returned": 0, "files": 4, "delivered_files": null, "commit": null, "seconds": null, "model_used": null, "model_asked": "sonnet", "effort_asked": "xhigh", "effort_used": "xhigh", "tokens": null}, {"wave": 15, "title": "O aviso do início sai uma vez e a limpeza alcança tudo", "state": "running", "at": "2026-10-22T16:29:37.901515611-03:00", "attention": null, "tasks": 1, "returned": 0, "files": 5, "delivered_files": null, "commit": null, "seconds": null, "model_used": null, "model_asked": "opus", "effort_asked": "xhigh", "effort_used": "xhigh", "tokens": null}, {"wave": 14, "title": "Só a conversa ligada à spec grava nela", "state": "running", "at": "2026-10-22T16:25:05.189026816-03:00", "attention": null, "tasks": 3, "returned": 0, "files": 12, "delivered_files": null, "commit": null, "seconds": null, "model_used": null, "model_asked": "opus", "effort_asked": "xhigh", "effort_used": "xhigh", "tokens": null}, {"wave": 13, "title": "Tarefa e critério citam o arquivo que outra tarefa cria", "state": "todo", "at": null, "attention": null, "tasks": 3, "returned": 0, "files": 8, "delivered_files": null, "commit": null, "seconds": null, "model_used": null, "model_asked": "opus", "effort_asked": "xhigh", "effort_used": "xhigh", "tokens": null}, {"wave": 12, "title": "O fim do levantamento mostra cada mensagem sem destino uma vez", "state": "committed", "at": "2026-10-22T16:40:31.691207035-03:00", "attention": null, "tasks": 1, "returned": 0, "files": 3, "delivered_files": 7, "commit": "c695d03b50e6e81e7c3a3e0b876a80a22aec69ac", "seconds": 1717, "model_used": "claude-sonnet-5-5", "model_asked": "sonnet", "effort_asked": "xhigh", "effort_used": "xhigh", "tokens": 3547767}, {"wave": 10, "title": "O painel mostra a spec pedida ou recusa a spec antiga", "state": "committed", "at": "2026-10-22T16:23:18.493453754-03:00", "attention": null, "tasks": 2, "returned": 0, "files": 7, "delivered_files": 9, "commit": "352ab4cce5de37f09276548ec76c19e3aab07583", "seconds": 2578, "model_used": "claude-opus-5-5", "model_asked": "opus", "effort_asked": "xhigh", "effort_used": "xhigh", "tokens": 10535032}]};
for(const [answers,patch] of [['example',{}],['real',{waves:realWaves}]] as const)for(const columns of [40,56,80,100])
  test(`no word is split and no line passes the screen on the waves screen · ${answers} answers · ${columns} columns`,async ($,on)=>{
    const f=await open($,on,'terminal',{columns,patch});await reach(f,'spec');await f.pane.press({key:'go-waves'});
    split.length=0;spill.length=0;const lines=screenLines(await f.pane.drawn(),columns);
    expect([...split,...spill]).toEqual([]);expect(widest(lines)).toBeLessThanOrEqual(Math.min(columns,110));
    if(answers==='real')await has(f.pane,`ONDAS · ${realWaves.total}`);
    await f.close();
  });
