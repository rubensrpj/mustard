import {test,expect,mock} from 'claude-code/testing';

// The answers of `mustard-rt run panel --section ...` as the binary gave them
// for this repository on 18/10, cut to what the screens read. The list and
// the measure are the only made-up parts: a row that asks for attention, the
// pages after the first and the copies' disk once measured.
const project={name:'mustard',branch:'fix/auditoria-spec-e-ondas',version:'0.2.24',language:'pt-BR'};
const NEXT='Esperar o veredito da revisão final';
const overview={ok:true,schema_version:1,section:'overview',revision:'rev-overview',at:'2026-10-18T12:55:32Z',project,selected_spec:'ajuste-na-spec-fechada',
  spec:{name:'ajuste-na-spec-fechada',goal:'Spec fechada aceita pedido novo na mesma branch.',phase:'running',tasks:{total:584,finished:584,backlog:0},
    criteria:{total:42,passed:41,failed:0,without_proof:1},waves:{finished:367,planned:367},
    next:{command:'mustard-rt run round --dispatch --spec ajuste-na-spec-fechada',text:NEXT},
    air:{running:0,no_agent:[],free_slots:5,capacity:5,backlog:0}},
  warnings:[{kind:'pr-merged',pr:289,at:'2026-10-05T08:22:49-03:00',phase:'running'},
    {kind:'review-reopened',approved_at:'2026-10-02T14:09:35-03:00',reopened_at:'2026-10-02T17:13:51-03:00'}],
  others:{total:14,attention:2,shown:[{name:'falha-fora-da-obra',phase:'running',updated:'2026-10-18T01:38:29-03:00',warning:{kind:'pr-merged',pr:326,at:'2026-10-17T16:52:14-03:00'}},
    {name:'mustard-enxuto',phase:'pr_open',updated:'2026-09-17T19:21:15-03:00',warning:{kind:'pr-merged',pr:277,at:'2026-09-17T21:23:52-03:00'}}]},
  usage:{cost_micro_usd:1571885643,sessions:38,tokens:21904658267,measured_through:'2026-10-13',session_agents:null}};
const spec={ok:true,schema_version:1,section:'spec',revision:'rev-spec',project,selected_spec:'ajuste-na-spec-fechada',
  spec:{name:'ajuste-na-spec-fechada',goal:'Spec fechada aceita pedido novo na mesma branch.',phase:'running',branch:'fix/ajuste-na-spec-fechada',base:'dev',
    opened_at:'2026-09-24T13:50:59-03:00',last_commit:{at:'2026-10-05T07:51:49-03:00',sha:'209a1a26'},commits:303,
    counts:{tasks:{total:584,finished:584,backlog:0},criteria:{total:42,passed:41,failed:0,without_proof:1},questions:{total:44,open:22},decisions:154,rules:66,waves:367},
    waves:{planned:367,finished:367,committed:361,integrated:6,running:0,todo:0,no_agent:[],rejected:[]},
    review:{valid:false,approved_at:'2026-10-02T14:09:35-03:00',reopened_at:'2026-10-02T17:13:51-03:00'},pr:{number:289,merged_at:'2026-10-05T08:22:49-03:00',source:'local-git'}},
  specs:[{name:'ajuste-na-spec-fechada',phase:'running',created:'2026-09-24T13:50:59-03:00'},{name:'falha-fora-da-obra',phase:'running',created:'2026-10-14T11:45:25-03:00'},
    {name:'mustard-enxuto',phase:'pr_open',created:'2026-09-12T10:22:00-03:00'},{name:'nova-versao',phase:'survey',created:'2026-10-05T08:59:18-03:00'},
    {name:'zerar-as-pendencias',phase:'delivered',created:'2026-09-20T10:36:27-03:00'},{name:'validar-a-busca',phase:'discarded',created:'2026-09-23T09:20:05-03:00'}],discarded:1};
const row=(n:number,extra:any={})=>({code:`MSTD-TASK-${String(n).padStart(4,'0')}`,title:`Tarefa ${n}`,wave:n,state:'committed',at:'2026-09-24T16:17:30-03:00',
  files:2,covers:1,resends:0,unmet:0,attention:null,...extra});
const stopped=row(193,{title:'Arquivo novo ou apagado: receita diferente',state:'no-agent',at:null,resends:2,attention:'wave-no-agent'});
function items(argv:string[]) {
  const at=(flag:string)=>argv.includes(flag)?argv[argv.indexOf(flag)+1]:undefined;
  const offset=Number(at('--offset') ?? 0),limit=Number(at('--limit') ?? 8),filter=at('--filter') ?? 'all',query=at('--query');
  const all=query?[row(7,{title:'Extra gamma'})]:filter==='attention'?[stopped]:[stopped,row(3,{title:'Aprovação final só vale depois da última reabertura'}),...Array.from({length:30},(_,n)=>row(10+n))];
  return {ok:true,schema_version:1,section:'items',revision:`rev-items-${filter}-${query}-${offset}-${limit}`,project,selected_spec:'ajuste-na-spec-fechada',kind:at('--kind') ?? 'task',filter,query:query ?? null,
    counts:{task:584,criterion:42,decision:154,rule:66,question:44},filters:{all:584,attention:1,committed:575,uncommitted:9},
    total:query?1:filter==='attention'?1:584,offset,limit,rows:all.slice(offset,offset+limit)};
}
const item={ok:true,schema_version:1,section:'item',revision:'rev-item',project,selected_spec:'ajuste-na-spec-fechada',item:{code:'MSTD-TASK-0003',kind:'task',
  title:'Aprovação final só vale depois da última reabertura',wave:1,state:'committed',at:'2026-09-24T16:17:30-03:00',
  request:{text:'Ponto decidido no levantamento: a aprovação final dada antes da última reabertura não vale.',cut:false},
  done:{text:'Reabertura conta como mudança: a aprovação final anterior deixa de valer.',cut:false},
  result:{unmet:0,agreed:[{item:'MSTD-CRIT-0007',kind:'criterion',title:'A spec tem uma aprovação final e depois é reaberta, sem entrega nova.',met:true}]},
  done_when:'cargo test -p mustard-core -- a_final_approval_before_the_last_reopen_does_not_count',files:['packages/core/src/domain/spec_state.rs'],delivered_files:10,
  wave_request:'# ajuste-na-spec-fechada — onda 1\n\nRode de dentro da cópia.\n'+'Linha do pedido da onda. '.repeat(200)+'\nÚltima linha do pedido.',
  tests:[{criterion:'MSTD-CRIT-0007',title:'A spec tem uma aprovação final e depois é reaberta, sem entrega nova.',proof:'cargo test -p mustard-core -- reopen',result:'pass',at:'2026-09-24T17:00:00-03:00',ms:5990}],
  covers:[{code:'MSTD-CRIT-0007',kind:'criterion',title:'A spec tem uma aprovação final e depois é reaberta, sem entrega nova.'},
    {code:'MSTD-DEC-0001',kind:'decision',title:'A spec fechada e a spec com o pull request aberto são reabertas pelo reopen.'}],wave_tokens:220008,resends:0,
  history:[{at:'2026-09-24T15:39:38-03:00',author:'assistant',wave:null,changed:null},{at:'2026-09-24T15:50:49-03:00',author:'assistant',wave:1,changed:{wave:{from:null,to:1}}}]}};
const days=['30','01','02','03','04','05','06','07','08','09','10','11','12','13'];
const usage={ok:true,schema_version:1,section:'usage',revision:'rev-usage',project,selected_spec:'ajuste-na-spec-fechada',measured_through:'2026-10-13',measured_at:'2026-10-15T01:54:12Z',
  available:true,period:{first:'2026-08-29',last:'2026-10-13'},project_tokens:21904658267,machine_tokens:27063778343,
  trend:{day:'2026-10-14',tokens:807800555,average_tokens:178094204,ratio:4.5},
  days:[886229689,461112921,554397719,675064976,781248087,605012735,312215102,57772003,null,null,2328619,null,null,null]
    .map((tokens,n)=>({day:`2026-${n?'10':'09'}-${days[n]}`,tokens})),
  cost:{total_micro_usd:1571885643,sessions:38,outside_spec_micro_usd:372189819,by_spec:[{name:'falha-fora-da-obra',micro_usd:1199695824}],specs_without_cost:13},
  spec:{name:'ajuste-na-spec-fechada',wave_tokens:3794138764,waves_without_measure:25,conductor_tokens:2138804723,
    // The waves' agents are made up: what each send recorded and what failed.
    waves_sent:367,waves:[
      {wave:367,model_used:'claude-opus-5-5',model_asked:'opus',effort_asked:'xhigh',skills:['add-run-command'],summary_reused:true,tokens:220008,refused_returns:2,review_rejections:1},
      {wave:366,model_used:null,model_asked:'opus',effort_asked:'xhigh',skills:[],summary_reused:false,tokens:null,refused_returns:0,review_rejections:0}],
    models:[{model:'claude-opus-5-5',effort:'xhigh',waves:340}],skills:[{name:'add-run-command',waves:12}],
    reuse:{summaries:30,knowledge_calls:10,knowledge_cached:4},
    failures:{jev:{calls:3,reasons:{no_credit:3}},hook_blocks:{total:5,by_hook:{command_guard:5}},refused_returns:9,review_rejections:2}},
  tools:{rtk:{configured:true,status:'measured',commands:54112,saved_tokens:71580138,percent:68.90729102135397},
    jev:{calls:1083,by_purpose:{search:22,'wave-planning':853,context:208},input_tokens:5438822,cost_micro_usd:null},search:{total:8732,in_spec:0}},
  session:{started_at:null,agents:null,working:null},copies_disk:null};
const settingsValues={enabled:true,rtk:true,vectors:true,jev:true,agent_model:'opus',agent_effort:'xhigh',search_answer:true,session_claude_model:'haiku',
  session_codex_model:'gpt-6-luna',jev_search:true,jev_wave_planning:true,jev_context:true,jev_spec_search:true,max_compiling_waves:5,review_model:'opus',
  review_effort:'xhigh',cloudflare_account:null,cloudflare_project:null,cloudflare_token:null};

const SURFACES=['terminal','desktop'] as const;
const TOKEN='synthetic-cloudflare-token-0001';
const NO_TOKEN='Grave o token em publication.apiToken do mustard.json, ou forneça CLOUDFLARE_API_TOKEN no ambiente, que vence. Os arquivos continuam locais.';

async function open($,on,surface,{theme='dark',conflict=false}:{theme?:string,conflict?:boolean}={}) {
  const clock=mock.clock(on);mock.env(on,{});mock.store(on,{});
  const commands:string[][]=[],patches:any[]=[],during:any[]=[];
  let mounted:any;
  let tokenSaved=false,measured=false;
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));on('command.register',()=>({value:null}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('session.cwd',()=>({value:'/fixture'}));on('session.id',()=>({value:'fixture-session'}));
  on('session.model',()=>({value:'fixture-model'}));
  on('session.usage',()=>({value:{startedAt:0,context:{tokens:499669,window:1000000,percent:50},
    rateLimits:[{kind:'five_hour',percentUsed:7,resetsAt:'2026-10-18T14:30:00Z'},{kind:'seven_day',percentUsed:33,resetsAt:'2026-10-22T12:00:00Z'}]}}));
  on('config.list',()=>({value:[{key:'theme',label:'Theme',kind:'choice',value:theme,provider:{kind:'engine'},isLocked:false}]}));
  on('ui.open',()=>({value:null}));on('ui.close',()=>({value:null}));on('tool.call',()=>({result:{text:'fixture read'}}));
  on('prompt.submit',()=>{throw new Error('the panel never starts a model turn');});
  on('turn.complete',()=>({text:''}));on('classic.SubagentStop',()=>({}));
  on('process.run',async ($,e)=>{
    commands.push(e.argv);
    const at=(flag:string)=>e.argv.includes(flag)?e.argv[e.argv.indexOf(flag)+1]:undefined;
    let value:any;
    if(e.argv[2]==='settings') {
      // The patch, with the token in it, comes only on standard input.
      const patch=e.argv.includes('--stdin')?JSON.parse(e.init.stdin):undefined;
      if(patch){patches.push(patch);if(patch.changes.cloudflare_token)tokenSaved=true;}
      if(patch && conflict)value={ok:false,schema_version:1,reason:'settings-conflict'};
      else value={ok:true,schema_version:1,revision:`settings-${patches.length}`,values:{...settingsValues,...(patch?.changes ?? {}),cloudflare_token:null},
        publication:{token_saved:tokenSaved,mustard_json_out_of_git:true},
        jev:{fallback_enabled:true,usable_key:true,purposes:{'spec-search':{enabled:false,reason:'purpose-not-configured',chosen:false}}},rtk:{own_hook_left:false},index_present:true,...(patch?{saved:true}:{})};
    } else if(e.argv[2]==='panel') {
      const section=at('--section');
      // What the pane shows while the measure runs.
      if(e.argv.includes('--refresh-consumption')){during.push({button:await mounted?.find({key:'measure'}),note:await mounted?.find({text:/Medindo/})});measured=true;}
      value=section==='overview'?overview:section==='spec'?spec:section==='items'?items(e.argv):section==='item'?item
        :section==='usage'?{...usage,copies_disk:measured?{bytes:126000000000,measured_at:'2026-10-18T13:00:00Z'}:null}:{ok:false,reason:'panel-invalid-section'};
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
  const pane=await $.ui.mount({plugin:'mustard',surface,component:'Pane',requestId:'mustard-panel',
    props:{id:'mustard-panel',title:'Mustard',bodyColumns:100,scroll:{bodyRows:40}}});
  mounted=pane;
  const reads=()=>commands.filter(argv=>argv[2]==='panel');
  return {pane,clock,commands,patches,during,reads,section:()=>{const last=reads().at(-1)!;return last[last.indexOf('--section')+1];}};
}
const has=async (pane,text)=>expect(await pane.find({text}),`${text}`).toBeDefined();
const lacks=async (pane,text)=>expect(await pane.find({text}),`${text}`).toBeUndefined();
const walk=(node:any,visit:(node:any)=>void)=>{if(Array.isArray(node))node.forEach(child=>walk(child,visit));
  else if(node && typeof node==='object'){visit(node);for(const value of Object.values(node))walk(value,visit);}};
// Every string the person reads: the text, the button captions, the field
// labels and hints, the chart's description. Keys are not read.
const shown=(tree:any)=>{const out:string[]=[];
  walk(tree,node=>{for(const prop of ['children','label','placeholder','alt','value'])for(const value of [node[prop]].flat())if(typeof value==='string')out.push(value);});return out;};
const CODE=/\b[A-Z]{2,}-[A-Z]+-\d{3,}\b/;

for(const surface of SURFACES)test(`the overview shows the counts of the binary and only the warnings it gave · ${surface}`,async ($,on)=>{
  const {pane,reads}=await open($,on,surface);
  expect(reads().length).toBe(1);expect(reads()[0]).toContain('overview');expect(reads()[0]).not.toContain('--refresh-consumption');
  for(const figure of surface==='terminal'?['584 de 584 tarefas feitas','41 de 42 critérios','367 de 367 ondas']:['584/584','41/42','367/367'])await has(pane,figure);
  await has(pane,/PR #289 na base desde 05\/10/);await has(pane,/revisão aprovada em 02\/10 e reaberta/i);
  await lacks(pane,/onda 193/);await lacks(pane,/sem agente/);
  await has(pane,'falha-fora-da-obra');await has(pane,/contexto 50%/i);await has(pane,/US\$\s?1\.572/);
  await pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`the next step reads as the binary's sentence, never as its command · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);
  await has(f.pane,'Próximo passo');await has(f.pane,NEXT);
  expect(shown(await f.pane.drawn()).filter(value=>/mustard-rt|--dispatch/.test(value))).toEqual([]);
  if(surface==='desktop'){await f.pane.press({key:'next-see'});expect(f.section()).toBe('spec');}
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`"Publicar no Cloudflare" publishes the project only on its press, with the command's own answer · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);
  const publishes=()=>f.commands.filter(argv=>argv[2]==='publish');
  const button=await f.pane.find({key:'publish'});
  expect(button).toBeDefined();
  if(surface==='terminal')expect(button!.props.hotkey).toBe('p');else expect(button!.props.label).toBe('Publicar no Cloudflare');
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'main-turn',reason:'answer'});await f.clock.advance(60000);
  expect(publishes()).toEqual([]);
  await f.pane.press({key:'publish'});
  expect(publishes()).toEqual([[expect.stringMatching(/mustard-rt$/),'run','publish','--root','/fixture','--project']]);
  expect(f.during.at(-1).publishing).toBeDefined();
  await has(f.pane,/Os arquivos permanecem locais; ainda não foram publicados/);await has(f.pane,/Grave o token em publication\.apiToken/);
  const command=await $.command.run({command:'mustard:pages',args:'project'});
  expect(publishes().length).toBe(2);
  await has(f.pane,command.text);
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`no screen shows an internal code · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);
  const codes=async (where:string)=>expect(shown(await f.pane.drawn()).filter(value=>CODE.test(value)),where).toEqual([]);
  await codes('overview');
  await f.pane.press({key:'go-spec'});await codes('spec');
  for(const kind of ['criterion','decision','rule','question']) {
    await f.pane.press({key:`items-${kind}`});expect(f.section()).toBe('items');await codes(`items ${kind}`);
    await f.pane.press({key:'back'});
  }
  await f.pane.press({key:'go-items'});await codes('items task');
  await f.pane.press({key:'item-MSTD-TASK-0003'});await codes('item');
  await f.pane.press({key:'history'});await codes('item history');
  await f.pane.press({key:'back'});await f.pane.press({key:'back'});await f.pane.press({key:'back'});
  await f.pane.press({key:'go-usage'});await codes('usage');
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`the keys s, t, c and v move between screens and read only the visible one · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);
  await f.pane.press({key:'go-spec'});expect(f.section()).toBe('spec');await has(f.pane,/fix\/ajuste-na-spec-fechada/);
  await f.pane.press({key:'go-items'});expect(f.section()).toBe('items');
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--kind','task','--limit','8']));
  await f.pane.press({key:'back'});expect(f.section()).toBe('spec');expect(f.reads().at(-1)).toContain('--if-revision');
  await f.pane.press({key:'back'});expect(f.section()).toBe('overview');await has(f.pane,/367 de 367|367\/367/);
  await f.pane.press({key:'go-usage'});expect(f.section()).toBe('usage');expect(f.reads().at(-1)).not.toContain('--refresh-consumption');
  await f.pane.press({key:'back'});expect(f.section()).toBe('overview');
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`the list puts attention first, filters, searches and shows eight more · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-items'});
  const rows=async ()=>(await f.pane.findAll({type:'Button'})).filter(button=>button.key?.startsWith('item-')).map(button=>button.key);
  expect((await rows())[0]).toBe('item-MSTD-TASK-0193');expect((await rows()).length).toBe(8);
  await has(f.pane,/onda 193\s+·\s+parada sem agente/);await has(f.pane,/1 a 8 de 584/);
  await f.pane.press({key:'more'});
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--offset','8','--limit','8']));
  expect((await rows()).length).toBe(16);await has(f.pane,/1 a 16 de 584/);
  if(surface==='terminal')await f.pane.press({key:'filter'});else await f.pane.press({key:'filter-attention'});
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--filter','attention','--offset','0','--limit','8']));
  expect(await rows()).toEqual(['item-MSTD-TASK-0193']);
  if(surface==='terminal')await f.pane.press({key:'search-open'});
  await f.pane.input({key:'search',text:'gamma'});
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--query','gamma']));
  expect(await rows()).toEqual(['item-MSTD-TASK-0007']);
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`a task shows the state of its wave and every version on h · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-items'});await f.pane.press({key:'item-MSTD-TASK-0003'});
  expect(f.reads().at(-1)).toEqual(expect.arrayContaining(['--section','item','--item','MSTD-TASK-0003']));
  await has(f.pane,/onda 1\s+·\s+comitada/);await has(f.pane,'Pedido');await has(f.pane,/aprovação final dada antes/);
  await has(f.pane,'O critério "A spec tem uma aprovação final e depois é reaberta, sem entrega nova."');
  await has(f.pane,'A decisão "A spec fechada e a spec com o pull request aberto são reabertas pelo reopen."');
  await has(f.pane,/^O critério "A spec tem uma aprovação final e depois é reaberta, sem entrega nova\."\s+·\s+atendido$/);
  await has(f.pane,'packages/core/src/domain/spec_state.rs');await has(f.pane,/220 mil tokens na onda 1/);
  await lacks(f.pane,'Primeira versão');
  await f.pane.press({key:'history'});
  await has(f.pane,'Primeira versão');await has(f.pane,/Posta na onda 1/);
  await f.pane.press({key:'back'});expect(f.section()).toBe('items');
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`a task shows the tests run and, on p, its wave's whole request · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-items'});await f.pane.press({key:'item-MSTD-TASK-0003'});
  await has(f.pane,'Testes');await has(f.pane,'cargo test -p mustard-core -- reopen');
  await has(f.pane,/^O critério "A spec tem uma aprovação final e depois é reaberta, sem entrega nova\."\s+·\s+passou\s+·\s+24\/09$/);
  await lacks(f.pane,/Última linha do pedido/);
  const button=await f.pane.find({key:'wave-request'});
  expect(button).toBeDefined();if(surface==='terminal')expect(button!.props.hotkey).toBe('p');
  await f.pane.press({key:'wave-request'});
  await has(f.pane,'Pedido da onda');await has(f.pane,/Rode de dentro da cópia/);await has(f.pane,/Última linha do pedido\.$/);
  expect(shown(await f.pane.drawn()).some(value=>value.includes('Linha do pedido da onda. '.repeat(200))),'the whole request').toBe(true);
  await f.pane.press({key:'back'});await f.pane.press({key:'item-MSTD-TASK-0003'});await lacks(f.pane,/Última linha do pedido/);
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`a light theme draws no fixed background and colors only by theme keys · ${surface}`,async ($,on)=>{
  const keys=new Set(['text','subtle','success','error','warning','claude','suggestion','inactive']);
  const f=await open($,on,surface,{theme:'light'});
  for(const screen of ['overview','go-spec','back','go-usage']) {
    if(screen!=='overview')await f.pane.press({key:screen});
    walk(await f.pane.drawn(),node=>{
      expect(node.backgroundColor,`${surface} ${screen}`).toBeUndefined();
      for(const prop of ['color','borderColor'])if(typeof node[prop]==='string')expect(keys.has(node[prop]),`${prop} ${node[prop]}`).toBe(true);
    });
  }
  if(surface==='desktop')expect(String((await f.pane.find({type:'Svg'}))?.props.source)).toContain('#9a7400');
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`the days draw in Svg on desktop and in blocks on the terminal · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-usage'});
  const svg=await f.pane.find({type:'Svg'});
  if(surface==='desktop') {
    expect(svg).toBeDefined();expect(String(svg!.props.alt)).toContain('30/09: 886 mi');expect(String(svg!.props.alt)).toContain('08/10: sem medida');
    expect(String(svg!.props.source)).toContain('stroke-dasharray');
  } else {
    expect(svg).toBeUndefined();await has(f.pane,/^[▁▂▃▄▅▆▇█]{8}··▁···$/);
  }
  await has(f.pane,/30\/09 a 13\/10/);await has(f.pane,/máximo 886 mi em 30\/09/);
  await has(f.pane,/último dia \(14\/10\): 4,5× a média da semana/);
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`the usage shows each wave's model, effort, skills, reuse and failures · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-usage'});
  await has(f.pane,/^onda 367\s+·\s+claude-opus-5-5\s+·\s+esforço pedido xhigh\s+·\s+220 mil tokens$/);
  await has(f.pane,/^onda 366\s+·\s+opus \(pedido\)\s+·\s+esforço pedido xhigh$/);
  await has(f.pane,/add-run-command\s+·\s+continuou um resumo\s+·\s+2 voltas recusadas\s+·\s+1 reprovação da revisão/);
  await has(f.pane,/claude-opus-5-5\s+·\s+esforço pedido xhigh\s+·\s+340 ondas/);
  await has(f.pane,/últimas 2 de 367 ondas enviadas/);
  await has(f.pane,/30 ondas continuaram um resumo\s+·\s+4 de 10 consultas de conhecimento reaproveitadas/);
  await has(f.pane,/Jev falhou 3 vezes: sem crédito \(3\)/);
  await has(f.pane,/5 bloqueios de gancho: command_guard \(5\)/);
  await has(f.pane,/9 voltas recusadas\s+·\s+2 reprovações da revisão/);
  await f.pane.press({key:'quit'});
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
  await f.pane.press({key:'quit'});await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'late',reason:'answer'});await f.clock.advance(1);
  expect(f.reads().length).toBe(before+1);
});

for(const surface of SURFACES)test(`"Medir agora" measures only from its button and stays off while it measures · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-usage'});
  const measures=()=>f.commands.filter(argv=>argv.includes('--refresh-consumption')).length;
  expect(measures()).toBe(0);await has(f.pane,/não medido/);
  await f.pane.press({key:'measure'});
  expect(measures()).toBe(1);expect(f.during.length).toBe(1);
  expect(f.during[0].button).toBeUndefined();expect(f.during[0].note).toBeDefined();
  expect(await f.pane.find({key:'measure'})).toBeDefined();await has(f.pane,/126 GB/);
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'main-turn',reason:'answer'});await f.clock.advance(1);
  expect(measures()).toBe(1);expect(f.reads().at(-1)).toContain('--if-revision');
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`settings show Jev in four purposes and a token that never comes back · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface);await f.pane.press({key:'go-settings'});
  expect(f.commands.at(-1)).toEqual(expect.arrayContaining(['run','settings','--full']));
  for(const purpose of ['jev_wave_planning','jev_context','jev_search','jev_spec_search'])expect(await f.pane.find({key:`setting-${purpose}`}),purpose).toBeDefined();
  await has(f.pane,'Busca na spec');await has(f.pane,/nenhuma escolha gravada/);
  expect(await f.pane.find({key:'settings-save'})).toBeUndefined();
  await f.pane.press({key:'setting-jev_spec_search'});
  await f.pane.input({key:'setting-cloudflare_token',text:TOKEN});
  expect(JSON.stringify(await f.pane.drawn())).not.toContain(TOKEN);
  await f.pane.press({key:'settings-save'});
  expect(f.patches.at(-1).changes).toEqual({jev_spec_search:false,cloudflare_token:TOKEN});
  expect(f.commands.filter(argv=>argv.some(arg=>arg.includes(TOKEN))),'the token is never a process argument').toEqual([]);
  expect(JSON.stringify(await f.pane.drawn())).not.toContain(TOKEN);await has(f.pane,/guardado no mustard\.json/);
  await f.pane.press({key:'back'});expect(f.section()).toBe('overview');
  await f.pane.press({key:'quit'});
});

for(const surface of SURFACES)test(`a settings conflict keeps the choices until an explicit discard · ${surface}`,async ($,on)=>{
  const f=await open($,on,surface,{conflict:true});await f.pane.press({key:'go-settings'});
  await f.pane.press({key:'setting-vectors'});await f.pane.press({key:'setting-vectors'});
  expect(await f.pane.find({key:'settings-save'})).toBeUndefined();
  await f.pane.press({key:'setting-rtk'});await f.pane.press({key:'settings-save'});
  expect(f.patches.at(-1).changes).toEqual({rtk:false});await has(f.pane,/mudou fora do painel/);
  expect(await f.pane.find({key:'settings-save'})).toBeDefined();await has(f.pane,/RTK · enxuga a saída dos comandos\s+·\s+desligado/);
  await f.pane.press({key:'settings-reload'});
  expect(await f.pane.find({key:'settings-save'})).toBeUndefined();await has(f.pane,/RTK · enxuga a saída dos comandos\s+·\s+ligado/);
  expect(f.patches.length).toBe(1);
  await f.pane.press({key:'quit'});
});
