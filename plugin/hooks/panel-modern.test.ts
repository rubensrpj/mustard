import {test,expect,mock} from 'claude-code/testing';
import {usageFigures,countdown} from './panel-runtime.js';
import {renderPanel,initialPanelView} from './panel-view.js';

async function fixture($,on,saved?) {
  const clock=mock.clock(on);mock.env(on,{});const storage:any={};
  if(saved)storage['mustard-panel:fixture-session']={schema_version:1,cwd:'/fixture',...saved};
  mock.store(on,storage);
  let opens=0,turns=0,publications=0,queries=0;
  const receipts:any[]=[],commands:any[]=[];
  const snapshot:any={ok:true,schema_version:1,revision:'source-1',at:'2026-10-13T12:00:00Z',project:{name:'fixture',version:'0.2.18',language:'pt-BR'},
    selected_spec:'demo',specs:[{name:'demo',phase:'running',goal:'Testar painel',waves:[],counted_waves:[0,0],definitions:{},stages:[],usage:{}}],jev:{},
    preferences:{ok:true,schema_version:1,revision:0,selected:'overview',profiles:[
      {id:'overview',name:'Visão geral',tab:'Projeto',tabs:['Projeto','Definições','Ondas','Execução','Consumo','Ideias','Diagnóstico','Configurações']},
      {id:'usage',name:'Consumo',tab:'Consumo',tabs:['Projeto','Consumo','Configurações']}]},
    ideas:{ok:true,schema_version:1,revision:0,items:[]},diagnostics:{events:[{component:'jev',operation:'search',reason:'provider-unavailable',at:'2026-10-13T12:00:00Z'}]}};
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));on('command.register',()=>({value:null}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('session.cwd',()=>({value:'/fixture'}));on('session.id',()=>({value:'fixture-session'}));
  on('session.model',()=>({value:'fixture-model'}));on('session.measure',($,e)=>({changed:e.changed}));
  on('session.usage',()=>({value:{context:{tokens:12000,window:200000,percent:6},rateLimits:[{kind:'five_hour',percentUsed:21,resetsAt:'2026-10-13T16:00:00Z'}]}}));
  on('ui.open',()=>{opens++;return {value:null};});on('ui.close',()=>({value:null}));
  on('prompt.submit',()=>{turns++;return {};});on('model.complete',()=>{turns++;throw new Error('Must not call a model');});
  on('prompt.fill',()=>({isFilled:true}));
  on('process.run',($,e)=>{
    const command=e.argv[2];commands.push(e.argv);
    const patch=e.argv.includes('--json')?JSON.parse(e.argv[e.argv.indexOf('--json')+1]):null;
    let value:any;
    if(command==='panel') {queries++;value=e.argv.includes('--if-revision')?{ok:true,schema_version:1,unchanged:true,revision:snapshot.revision}:snapshot;}
    else if(command==='usage-record'){receipts.push(JSON.parse(e.init.stdin));value={ok:true};}
    else if(command==='panel-preferences') {snapshot.preferences={...snapshot.preferences,...patch,revision:snapshot.preferences.revision+1};value=snapshot.preferences;}
    else if(command==='ideas') {
      expect(patch.revision).toBe(snapshot.ideas.revision);
      if(patch.action==='add')snapshot.ideas.items.push({id:'P-1',title:patch.title,detail:patch.detail,idea:true,status:'open'});
      else if(patch.action==='archive')snapshot.ideas.items[0].status='dropped';
      snapshot.ideas.revision++;value=snapshot.ideas;
    }
    else if(command==='impact')value={ok:true,action:'publish',effects:[{kind:'publication',scope:patch.scope}],working_changes:[],active_waves:[],unknown:[],token:'same'};
    else if(command==='publish'){publications++;value={ok:true,published:true,snapshot_id:'published',remote_url:'https://fixture.pages.dev'};}
    else if(command==='panel-diagnostics')value={ok:true,path:'/fixture/diagnostics.json'};
    else throw new Error('Unexpected command '+command);
    return {value:{exitCode:0,stdout:JSON.stringify(value),stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'desktop',isInteractive:true});
  if(!saved?.wanted)await $.command.run({command:'mustard-panel',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',requestId:'mustard-panel',
    props:{id:'mustard-panel',title:'Mustard',bodyColumns:120,scroll:{bodyRows:40}}});
  return {pane,clock,snapshot,receipts,commands,get opens(){return opens;},get turns(){return turns;},get publications(){return publications;},get queries(){return queries;}};
}

test('unchanged polling uses conditional reads and creates no model turn',async ($,on)=>{
  const f=await fixture($,on);expect(f.queries).toBe(1);
  await f.clock.advance(60000);expect(f.queries).toBe(7);
  expect(f.commands.filter(argv=>argv.includes('--if-revision')).length).toBe(5);
  expect(await f.pane.find({type:'Text',text:'Testar painel'})).toBeDefined();
  expect(f.turns).toBe(0);expect(f.publications).toBe(0);await f.pane.press({key:'close'});
});
test('a module restart restores the requested pane, selection and section without a model',async ($,on)=>{
  const f=await fixture($,on,{wanted:true,selected:'demo',view:{tab:'Ideias'}});
  expect(f.opens).toBe(1);expect(f.commands[0]).toContain('ideas');
  expect(await f.pane.find({type:'Text',text:'Ideias do projeto'})).toBeDefined();expect(f.turns).toBe(0);await f.pane.press({key:'close'});
});
test('each reopening records its own render and preserves a single poller',async ($,on)=>{
  const f=await fixture($,on);
  await f.pane.unmount();
  await $.command.run({command:'mustard-panel',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',requestId:'mustard-panel',
    props:{id:'mustard-panel',title:'Mustard',bodyColumns:120,scroll:{bodyRows:40}}});
  const details=await $.command.run({command:'mustard-panel',args:'--diagnose'});
  expect(details.text.match(/render:desktop/g)?.length).toBe(2);
  const before=f.queries;await f.clock.advance(10000);
  expect(f.queries).toBe(before+1);
  expect(f.turns).toBe(0);await pane.press({key:'close'});
});
test('concurrent opening requests leave only one refresh timer',async ($,on)=>{
  const f=await fixture($,on);
  await Promise.all([$.command.run({command:'mustard-panel',args:''}),$.command.run({command:'mustard-panel',args:''})]);
  expect(f.opens).toBe(3);
  const before=f.queries;await f.clock.advance(10000);
  expect(f.queries).toBe(before+1);
  expect(f.turns).toBe(0);await f.pane.press({key:'close'});
});
test('costless host measurements still reach the native recorder',async ($,on)=>{
  const f=await fixture($,on);
  await $.session.measure({context:{tokens:30000,window:200000,percent:15},rateLimits:[],changed:['context']});
  expect(f.receipts.length).toBe(1);expect(f.receipts[0].context.tokens).toBe(30000);expect(f.receipts[0].cost).toBeUndefined();
  expect(f.turns).toBe(0);await f.pane.press({key:'close'});
});
test('ideas survive a refresh and require an explicit action before being linked or archived',async ($,on)=>{
  const f=await fixture($,on);await f.pane.press({key:'tab-Ideias'});
  await f.pane.input({key:'idea-title',text:'Melhorar download',kind:'change'});
  await f.pane.input({key:'idea-detail',text:'Preservar as colunas.',kind:'change'});
  await f.clock.advance(10000);expect((await f.pane.find({key:'idea-title'})).props.value).toBe('Melhorar download');
  await f.pane.press({key:'idea-save'});expect(f.snapshot.ideas.items[0].status).toBe('open');
  expect(await f.pane.find({type:'Text',text:'Ideia · ainda não aprovada'})).toBeDefined();
  await f.pane.press({key:'idea-start-P-1'});expect(f.turns).toBe(0);
  await f.pane.press({key:'idea-archive-P-1'});expect(f.snapshot.ideas.items[0].status).toBe('dropped');
  expect(f.commands.some(argv=>argv.includes('round') || argv.includes('write'))).toBe(false);await f.pane.press({key:'close'});
});
test('presentation profiles switch visible information without editing execution settings',async ($,on)=>{
  const f=await fixture($,on);await f.pane.select({key:'panel-profile',value:'usage'});
  expect(await f.pane.find({type:'Text',text:'O Mustard está economizando?'})).toBeDefined();
  expect(f.commands.filter(argv=>argv[2]==='panel-preferences').length).toBe(1);
  expect(f.commands.some(argv=>argv[2]==='settings' || argv[2]==='round')).toBe(false);
  expect(f.turns).toBe(0);await f.pane.press({key:'close'});
});
test('profile editing keeps a valid home section and cannot remove the last section',async ($,on)=>{
  const f=await fixture($,on);await f.pane.press({key:'profile-customize'});
  for(const tab of ['Projeto','Definições','Ondas','Execução','Consumo','Ideias','Diagnóstico'])await f.pane.press({key:'profile-toggle-'+tab});
  const home=await f.pane.find({key:'profile-home'});expect(home.props.value).toBe('Configurações');expect(home.props.options.length).toBe(1);
  expect(await f.pane.find({key:'profile-toggle-Configurações'})).toBeUndefined();
  await f.pane.input({key:'profile-name',text:'',kind:'change'});expect(await f.pane.find({key:'profile-save'})).toBeUndefined();
  expect(f.turns).toBe(0);await f.pane.press({key:'close'});
});
test('publication preview can be cancelled and never uploads before explicit confirmation',async ($,on)=>{
  const f=await fixture($,on);await f.pane.press({key:'publications'});await f.pane.press({key:'publish-project'});
  expect(f.publications).toBe(0);expect(await f.pane.find({type:'Text',text:'Prévia de impacto'})).toBeDefined();
  await f.pane.press({key:'impact-cancel'});expect(f.publications).toBe(0);
  await f.pane.press({key:'publish-project'});await f.pane.press({key:'impact-confirm'});expect(f.publications).toBe(1);
  expect(f.turns).toBe(0);await f.pane.press({key:'close'});
});
test('diagnostic filters and local export do not ask a model or publish content',async ($,on)=>{
  const f=await fixture($,on);await f.pane.press({key:'tab-Diagnóstico'});
  expect(await f.pane.find({type:'Text',text:'provider-unavailable'})).toBeDefined();
  await f.pane.input({key:'diagnostic-search',text:'missing-wave',kind:'change'});
  expect(await f.pane.find({type:'Text',text:'provider-unavailable'})).toBeUndefined();
  await f.pane.press({key:'diagnostic-export'});expect(f.commands.some(argv=>argv.includes('--export'))).toBe(true);
  expect(f.publications).toBe(0);expect(f.turns).toBe(0);await f.pane.press({key:'close'});
});
test('cache ratios use observed input categories and unknown reset dates remain unknown',()=>{
  expect(usageFigures({input_tokens:20,cache_read_input_tokens:60,cache_creation_input_tokens:20}).cachePercent).toBe(60);
  expect(countdown(undefined)).toBe('');expect(usageFigures(null)).toBe(null);
});

test('economy overview exposes scope and incomplete waves without claiming net savings',async ($,on)=>{
  const f=await fixture($,on);
  f.snapshot.rtk={status:'measured',percent:40,saved_tokens:400,input_tokens:1000,output_tokens:600,commands:10,period_start:'2026-10-01',period_end:'2026-10-13'};
  f.snapshot.jev={physical_requests:17,cost_micro_usd:1428,by_purpose:{'wave-planning':11,context:6}};
  f.snapshot.specs[0].waves=[{wave:1,tokens:1200,status:'committed',files_changed:1},{wave:2,tokens:null,status:'running'}];
  f.snapshot.specs[0].usage={known_wave_tokens:1200,waves_with_unknown_usage:1,conductor_tokens:2500};
  f.snapshot.specs[0].operations={calls:20,refusals:4,search:{calls:1,enriched:1,fallback:0,local_model_calls:1,remote_model_calls:0,
    paired_calls:1,native_bytes:100,delivered_bytes:150,reduced_calls:0,expanded_calls:1}};
  await f.pane.press({key:'tab-Consumo'});
  for(const text of ['Economia líquida: ainda não comprovada','Spec selecionada · consumo e atrito','Scan · buscas desta spec','Projeto · histórico separado da spec'])
    expect(await f.pane.find({type:'Text',text})).toBeDefined();
  expect(await f.pane.find({type:'Text',text:'Saída nativa → entregue nas buscas medidas: 100 → 150 bytes'})).toBeDefined();
  expect(await f.pane.find({type:'Text',text:'50% mais texto; verificar se o contexto adicional ajudou.'})).toBeDefined();
  expect(await f.pane.find({type:'Text',text:'RTK · período: 01/10/2026 → 13/10/2026'})).toBeDefined();
  await f.pane.press({key:'economy-waves'});
  expect(await f.pane.find({type:'Text',text:'1 arquivos na entrega'})).toBeDefined();
  expect(await f.pane.find({type:'Text',text:'Consumo não medido'})).toBeDefined();
  expect(f.turns).toBe(0);expect(f.publications).toBe(0);await f.pane.press({key:'close'});
});

test('diagnostics separates historical counts from explained tool outcomes',async ($,on)=>{
  const f=await fixture($,on);
  f.snapshot.integration_health={hosts:{observations:[{host:'claude',last_event:'PreToolUse',at:'2026-10-13T12:00:00Z',tool_failures:6,unexplained_tool_failures:5,
    recent_tool_failures:[{tool:'Read',reason:'search-redirect',category:'blocked',at:'2026-10-13T12:00:00Z'}]}]}};
  await f.pane.press({key:'tab-Diagnóstico'});
  expect(await f.pane.find({type:'Text',text:'6 ocorrências na sessão'})).toBeDefined();
  expect(await f.pane.find({type:'Text',text:'Último evento recebido: PreToolUse. Este evento não é a causa da contagem.'})).toBeDefined();
  expect(f.turns).toBe(0);expect(f.publications).toBe(0);await f.pane.press({key:'close'});
});

test('diagnostics identifies an old SSH plugin separately from local installation health',async ($,on)=>{
  const f=await fixture($,on);
  f.snapshot.diagnostics.plugin_loaders={supported:true,observations:[{pid:11,plugin_version:'0.1.68',
    plugin_root:'/remote/plugins/old-copy',mods_module_present:false,matches_running_version:false}]};
  await f.pane.press({key:'tab-Diagnóstico'});
  expect(await f.pane.find({type:'Text',text:'Esta cópia não contém o módulo Mods.'})).toBeDefined();
  expect(await f.pane.find({type:'Text',text:/Atualize o plugin no cliente que abriu esta sessão SSH/})).toBeDefined();
  expect(f.turns).toBe(0);await f.pane.press({key:'close'});
});
test('a slow section or spec transition shows loading rather than a false empty plan',()=>{
  const element=type=>props=>({type,props});
  const elements=Object.fromEntries(['Box','Text','Button','Select'].map(type=>[type,element(type)]));
  const data={ok:true,section:'project',project:{name:'fixture'},specs:[{name:'demo',summary_only:true}],selected_spec:'demo'};
  const pane=renderPanel({elements,surface:'desktop',columns:100,bodyRows:40,data,selected:'demo',view:{...initialPanelView(),tab:'Ondas'},
    actions:{change:()=>{},refresh:()=>{},close:()=>{},selectSpec:()=>{}}});
  expect(JSON.stringify(pane)).toContain('Carregando esta seção');expect(JSON.stringify(pane)).not.toContain('Sem ondas planejadas');
});

test('native model request observers preserve the stream and record only usage metadata',async ($,on)=>{
  on('turn.step',async function* ($,e){return {turnId:e.turnId,index:e.index,answer:'unchanged answer',toolUses:[],stopReason:'end_turn',
    usage:{model:'fixture-model',input_tokens:10,output_tokens:5,cache_read_input_tokens:20,cache_creation_input_tokens:0}};});
  const f=await fixture($,on);
  const stream=$.turn.step({turnId:'turn-fixture',index:0,model:'fixture-model',effort:'medium',messageCount:2,agentId:'agent-fixture'});
  const result=await stream.next();expect(result.done).toBe(true);expect(result.value.answer).toBe('unchanged answer');
  expect(f.receipts.length).toBe(1);expect(f.receipts[0].kind).toBe('step');expect(f.receipts[0].agent_id).toBe('agent-fixture');
  expect(f.receipts[0].effort).toBe('medium');expect(f.receipts[0].usage.input_tokens).toBe(10);
  expect(JSON.stringify(f.receipts)).not.toContain('unchanged answer');expect(f.turns).toBe(0);await f.pane.press({key:'close'});
});
test('the analysis step pins the effort and leaves the model the agent definition resolved',async ($,on)=>{
  const seen:any[]=[];
  on('agent.spawn',()=>({agentId:'agent-analysis',model:'claude-haiku-5-5'}));
  on('turn.step',async function* ($,e){seen.push(e);return {turnId:e.turnId,index:e.index,answer:'ok',toolUses:[],stopReason:'end_turn',usage:null};});
  const f=await fixture($,on);
  await $.agent.spawn({subagentType:'mustard-analysis',fork:false,prompt:'bounded'});
  await $.turn.step({turnId:'turn-analysis',index:0,model:'claude-haiku-5-5',effort:'high',messageCount:1,agentId:'agent-analysis'}).next();
  expect(seen.length).toBe(1);expect(seen[0].effort).toBe('medium');expect(seen[0].model).toBe('claude-haiku-5-5');
  await f.pane.press({key:'close'});
});
test('impact observation preserves a denied Bash call and never reruns or approves it',async ($,on)=>{
  let executions=0;
  on('tool.call',{tool:'Bash'},()=>{executions++;return {deny:'Existing native protection'};});
  const f=await fixture($,on);
  const result=await $.tool.call({tool:'Bash',command:'git reset --hard'});
  expect(result.deny).toBe('Existing native protection');expect(executions).toBe(1);expect(f.turns).toBe(0);
  expect(f.commands.some(argv=>argv[2]==='impact')).toBe(true);await f.pane.press({key:'close'});
});
