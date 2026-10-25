import { test, expect, mock } from 'claude-code/testing';
import {contentPage} from './panel-content.js';
import {analysisSpawn,analysisTool,fullAnalysisInput} from './analysis-policy.js';
import {commandFailed} from './register.js';

// The smallest answers of the overview and usage screens the panel draws.
const overview=(extra:any={})=>({ok:true,schema_version:1,section:'overview',revision:'rev-overview',at:'2026-10-08T12:00:00Z',project:{name:'fixture'},
  selected_spec:null,spec:null,warnings:[],others:{total:0,attention:0,shown:[]},usage:{cost_micro_usd:null,sessions:0,tokens:null},...extra});
const usageScreen=(extra:any={})=>({ok:true,schema_version:1,section:'usage',revision:'rev-usage',project:{name:'fixture'},selected_spec:'demo',
  measured_through:'2026-10-07',measured_at:'2026-10-08T12:00:00Z',available:true,period:{first:'2026-10-01',last:'2026-10-07'},project_tokens:100,machine_tokens:1000,
  trend:null,days:[],cost:{total_micro_usd:null,sessions:0,outside_spec_micro_usd:null,by_spec:[],specs_without_cost:0},spec:null,
  tools:{rtk:{configured:false},jev:{calls:0,by_purpose:{},input_tokens:0,cost_micro_usd:null},search:{total:0,in_spec:null}},
  session:{started_at:null,agents:null,working:null},copies_disk:null,...extra});
// The second page of the waves screen: the oldest wave, with its own spend.
const wavesScreen=(argv:string[])=>({ok:true,schema_version:1,section:'waves',revision:'rev-waves',project:{name:'fixture'},selected_spec:'demo',total:9,
  offset:Number(argv[argv.indexOf('--offset')+1]),limit:8,rows:[{wave:1,title:'Primeira',state:'committed',at:'2026-10-02T11:05:00Z',attention:null,tasks:1,returned:0,
    files:1,delivered_files:1,commit:'abc1234',seconds:7200,model_used:'claude-opus-5-5',model_asked:'opus',effort_asked:'xhigh',effort_used:null,tokens:4321}]});
const answerFor=(argv:string[])=>({usage:usageScreen,waves:()=>wavesScreen(argv)}[argv[argv.indexOf('--section')+1]] ?? overview)();
async function openPane($,on,{saved,surface='desktop'}:{saved?:any,surface?:'desktop'|'terminal'}={}) {
  const clock=mock.clock(on);mock.env(on,{});
  mock.store(on,saved?{'mustard-panel:fixture-session':{schema_version:2,cwd:'/fixture',...saved}}:{});
  let opens=0,turns=0;const receipts:any[]=[],commands:any[]=[];
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));on('command.register',()=>({value:null}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('session.cwd',()=>({value:'/fixture'}));on('session.id',()=>({value:'fixture-session'}));
  on('session.model',()=>({value:'fixture-model'}));on('session.measure',($,e)=>({changed:e.changed}));
  on('session.usage',()=>({value:{context:{tokens:12000,window:200000,percent:6},rateLimits:[{kind:'five_hour',percentUsed:21,resetsAt:'2026-10-13T16:00:00Z'}]}}));
  on('ui.open',()=>{opens++;return {value:null};});on('ui.close',()=>({value:null}));
  on('prompt.submit',()=>{turns++;return {};});on('model.complete',()=>{turns++;throw new Error('Must not call a model');});
  on('turn.complete',()=>({text:''}));
  on('process.run',($,e)=>{
    commands.push(e.argv);
    const value=e.argv[2]==='usage-record'?(receipts.push(JSON.parse(e.init.stdin)),{ok:true}):e.argv[2]==='panel'?answerFor(e.argv):{ok:true};
    return {value:{exitCode:0,stdout:JSON.stringify(value),stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface,isInteractive:true});
  if(!saved?.wanted)await $.command.run({command:'mustard:mods',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface,component:'Pane',requestId:'mustard-panel',
    props:{id:'mustard-panel',title:'Mustard',bodyColumns:120,scroll:{bodyRows:40}}});
  return {pane,clock,receipts,commands,get opens(){return opens;},get turns(){return turns;}};
}

test('analysis stays in an isolated Haiku session and counts cache in its input budget',()=>{
  expect(analysisSpawn({subagentType:'mustard-analysis',fork:true,prompt:'bounded'}).deny).toContain('isolated');
  expect(analysisSpawn({subagentType:'mustard:mustard-analysis',fork:false,prompt:'bounded',model:'opus'}).deny).toContain('small model');
  expect(analysisSpawn({subagentType:'mustard-analysis',fork:false,prompt:'bounded'}).model).toBeUndefined();
  expect(analysisSpawn({subagentType:'mustard-analysis',fork:false,prompt:'bounded',model:'haiku'}).model).toBe('haiku');
  expect(analysisSpawn({subagentType:'mustard-wave',fork:false,prompt:'task',model:'sonnet'}).model).toBe('sonnet');
  expect(fullAnalysisInput({input_tokens:1000,cache_read_input_tokens:87000,cache_creation_input_tokens:2000})).toBe(90000);
  expect(fullAnalysisInput({input_tokens:1000})).toBeNull();
  expect(analysisTool({tool:'Edit',input:{}},{input:1000}).decision).toBe('deny');
  expect(analysisTool({tool:'Read',input:{}},{input:90000}).decision).toBe('deny');
  expect(analysisTool({tool:'Read',input:{}},{input:1000})).toBeNull();
  expect(analysisTool({tool:'SubagentHandback',input:{}},{input:1000})).toBeNull();
  expect(analysisTool({tool:'SubagentHandback',input:{}},{input:95000})).toBeNull();
  expect(analysisTool({tool:'Bash',input:{command:'touch changed'}},null)).toBeNull();
  expect(analysisTool({tool:'mcp__mustard__search',input:{request:{tool:'Bash',input:{command:'touch changed'}}}},{input:1}).decision).toBe('deny');
});

test('Desktop reattachment recreates only a pane the user requested',async ($,on)=>{
  mock.clock(on);mock.env(on,{});let opens=0;
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{}}));
  on('session.surfaces',()=>({value:['terminal','desktop']}));
  on('session.attach',()=>({clientId:'desktop-fixture'}));on('session.detach',()=>({clientId:'desktop-fixture'}));
  on('ui.close',()=>({value:null}));on('ui.open',()=>{opens++;return {value:null};});
  on('process.run',()=>({value:{exitCode:0,stdout:JSON.stringify(overview()),stderr:''}}));
  await $.session.start({cwd:'/fixture',surface:'desktop',isInteractive:true});
  await $.session.attach({surface:'desktop'});
  expect(opens).toBe(0);
  const opening=await $.command.run({command:'mustard:mods',args:''});
  expect(opening.text).toContain('Interfaces conectadas');
  expect(opening.text).toContain('Renderização observada');
  await $.session.detach({surface:'desktop'});
  await $.session.attach({surface:'desktop'});
  expect(opens).toBe(2);
  const reply=await $.command.run({command:'mustard:mods',args:'--diagnose'});
  expect(reply.text).toContain('attach');expect(reply.text).toContain('open-accepted');
});

test('a lost pane reference does not prevent a fresh Desktop opening',async ($,on)=>{
  mock.clock(on);mock.env(on,{});let opens=0,closes=0;
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{}}));
  on('ui.close',()=>{if(++closes===1)throw new Error('pane no longer exists');return {value:null};});
  on('ui.open',()=>{opens++;return {value:null};});
  on('process.run',()=>({value:{exitCode:0,stdout:JSON.stringify(overview()),stderr:''}}));
  await $.session.start({cwd:'/fixture',surface:'desktop',isInteractive:true});
  expect((await $.command.run({command:'mustard:mods',args:''})).text).toContain('solicitada');
  expect(opens).toBe(1);
  const pane=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'quit'});
});

test('reopening recreates the host pane and never measures on its own',async ($,on)=>{
  const clock=mock.clock(on);mock.env(on,{});
  const lifecycle:string[]=[], measured:boolean[]=[];
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{}}));
  on('ui.close',()=>{lifecycle.push('close');return {value:null};});
  on('ui.open',()=>{lifecycle.push('open');return {value:null};});
  on('process.run',($,e)=>{measured.push(e.argv.includes('--refresh-consumption'));
    return {value:{exitCode:0,stdout:JSON.stringify(answerFor(e.argv)),stderr:''}};});
  await $.session.start({cwd:'/fixture',surface:'desktop',isInteractive:true});
  const reply=await $.command.run({command:'mustard:mods',args:''});
  expect(reply.text).toContain('--diagnose');expect(lifecycle).toEqual(['close','open']);
  expect(measured).toEqual([false]);
  await $.command.run({command:'mustard:mods',args:''});
  expect(lifecycle).toEqual(['close','open','close','open']);
  await clock.advance(10000);expect(measured).toEqual([false,false]);
  const pane=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'go-usage'});await clock.advance(60000);
  expect(measured).toEqual([false,false,false]);
  await pane.press({key:'quit'});
  const count=measured.length;await clock.advance(10000);expect(measured.length).toBe(count);
});

test('an opening failure leaves nothing reading and a later command can recover',async ($,on)=>{
  const clock=mock.clock(on);mock.env(on,{});let attempts=0,reads=0;
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{}}));
  on('ui.close',()=>({value:null}));
  on('ui.open',()=>{if(++attempts===1)throw new Error('fixture opening failed');return {value:null};});
  on('process.run',()=>{reads++;return {value:{exitCode:0,
    stdout:JSON.stringify(overview()),stderr:''}};});
  await $.session.start({cwd:'/fixture',surface:'desktop',isInteractive:true});
  const failed=await $.command.run({command:'mustard:mods',args:''});
  expect(failed.text).toContain('não concluiu');
  await clock.advance(10000);expect(reads).toBe(0);
  const recovered=await $.command.run({command:'mustard:mods',args:''});
  expect(recovered.text).toContain('solicitada');expect(reads).toBe(1);
  const pane=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'quit'});
});

test('panel diagnostics reports the loaded host without opening, reading history or claiming Desktop paint',async ($,on)=>{
  mock.env(on,{});mock.clock(on);let operations=0,surfaces=['terminal','desktop'];
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.surfaces',()=>({value:surfaces}));on('session.version',()=>({value:{version:'2.1.296'}}));
  on('ui.panes',()=>({value:[{id:'mustard-panel',title:'Mustard',isShown:true,isFocused:true,isPlaced:true}]}));
  on('ui.open',()=>{operations++;return {value:null};});
  on('process.run',()=>{operations++;throw new Error('must not read history');});
  on('prompt.submit',()=>{operations++;return {};});
  on('tool.call',()=>{operations++;return {};});
  await $.session.start({cwd:'/fixture',surface:'desktop',isInteractive:true});
  const reply=await $.command.run({command:'mustard:mods',args:'--diagnose'});
  expect(reply.text).toContain('2.1.296');expect(reply.text).toContain('terminal, desktop');
  expect(reply.text).toContain('não comprova');expect(reply.context).toBeUndefined();
  surfaces=['terminal'];
  expect((await $.command.run({command:'mustard:mods',args:'--diagnose'})).text).toContain('não informou uma interface Desktop');
  expect(operations).toBe(0);
});

test('content pagination preserves all Unicode text within the official surface size limit',()=>{
  const original='🟡'.repeat(10001)+'\nFinal';
  const first=contentPage(original,0),parts=[];
  for(let n=0;n<first.pages;n++) {
    const page=contentPage(original,n);expect(page.text.length).toBeLessThanOrEqual(10000);parts.push(page.text);
  }
  expect(parts.join('')).toBe(original);
});

test('host measurements persist natively and the host context renders without a model turn', async ($,on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on,{});mock.clock(on);
  let turns=0;
  const recorded:any[]=[];
  on('session.start',($,e)=>({cwd:e.cwd}));on('session.measure',($,e)=>({changed:e.changed}));
  on('command.register',()=>({value:null}));on('session.cwd',()=>({value:'/fixture'}));
  on('session.id',()=>({value:'actual-session'}));on('session.model',()=>({value:'test-model'}));
  on('session.usage',()=>({value:{context:{percent:12,tokens:24000,window:200000},rateLimits:[]}}));
  on('ui.open',()=>({value:null}));on('ui.close',()=>({value:null}));
  on('prompt.submit',()=>{turns++;return {};});
  on('process.run',($,e)=>{
    if(e.argv[2]==='usage-record') {
      expect(e.argv.slice(1)).toEqual(['run','usage-record','--root','/fixture','--session','actual-session']);
      recorded.push(JSON.parse(e.init.stdin));
      return {value:{exitCode:0,stdout:'{"ok":true}',stderr:''}};
    }
    expect(e.argv).toContain('--session');
    return {value:{exitCode:0,stdout:JSON.stringify(overview({usage:{cost_micro_usd:500000,sessions:2,tokens:100}})),stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  await $.session.measure({context:{percent:12,tokens:24000,window:200000},cost:{usd:0.5},rateLimits:[],changed:['cost']});
  expect(recorded).toEqual([{kind:'measure',cost:{usd:0.5},model:'test-model',aborted:false,context:{percent:12,tokens:24000,window:200000},rateLimits:[]}]);
  await $.command.run({command:'mustard:mods',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface:'terminal',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  expect(await pane.find({text:'Contexto'})).toBeDefined();expect(await pane.find({text:'12%'})).toBeDefined();
  // O Uso traz o custo ao lado das sessões, no terminal e no desktop.
  const desktop=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  for(const surface of [pane,desktop]){expect(await surface.find({text:/^US\$ 0,50$/})).toBeDefined();expect(await surface.find({text:/^no projeto · 2 sessões$/})).toBeDefined();}
  expect(turns).toBe(0);
  // O terminal fecha o painel pela marca dele; o fechar que o kit aperta é o do desktop.
  await desktop.press({key:'quit'});
});

test('explicit export without host transport starts no model turn', async ($, on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on, {});
  let turns = 0;
  let publications = 0;
  on('session.start', ($, e) => ({cwd:e.cwd}));
  on('command.register', () => ({value:null}));
  on('session.cwd', () => ({value:'/fixture'}));
  on('process.run', ($, e) => {
    publications++;
    expect(e.argv.slice(1)).toEqual(['run','publish','--root','/fixture','--spec','demo']);
    return {value:{exitCode:0,stdout:JSON.stringify({ok:true,published:false,snapshot_id:'fixture-version',
      page:'/fixture/export/index.html',database:'/fixture/export/snapshot.json',manifest:'/fixture/export/manifest.json'}),stderr:''}};
  });
  on('tool.list', () => ({value:[]}));
  on('prompt.submit', () => { turns++; return {}; });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result = await $.command.run({command:'mustard:pages',args:'spec demo'});
  expect(result.text).toContain('ainda não foram publicados');
  expect(publications).toBe(1);
  expect(turns).toBe(0);
});

test('a Windows query failure keeps the last reading and the English panel', async ($, on) => {
  const clock = mock.clock(on);
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on, {OS:'Windows_NT'});
  let reads = 0;
  on('session.start', ($, e) => ({cwd:e.cwd}));
  on('command.register', () => ({value:null}));
  on('session.cwd', () => ({value:'C:/fixture'}));
  on('session.usage', () => ({value:{rateLimits:[]}}));
  on('ui.open', () => ({value:null}));
  on('ui.close', () => ({value:null}));
  on('turn.complete',()=>({text:''}));
  on('process.run', ($, e) => {
    if(e.argv[2]==='panel-diagnostics')return {value:{exitCode:0,stdout:'{"ok":true}',stderr:''}};
    reads++;
    expect(e.argv[0]).toMatch(/mustard-rt\.exe$/);
    return {value:reads===1 ? {exitCode:0,stdout:JSON.stringify(overview({project:{name:'fixture',language:'en-US'},
      spec:{name:'demo',goal:'Demo',phase:'running',tasks:{total:2,finished:1,backlog:0},criteria:{total:1,passed:0,failed:0,without_proof:1},
        waves:{finished:1,planned:2},next:null,air:{running:0,no_agent:[],free_slots:4,capacity:4,backlog:0}}})),stderr:''}
      : {exitCode:1,stdout:'',stderr:'fixture query failed'}};
  });
  await $.session.start({cwd:'C:/fixture',surface:'terminal',isInteractive:true});
  await $.command.run({command:'mustard:mods',args:''});
  const pane = await $.ui.mount({plugin:'mustard',surface:'terminal',component:'Pane',
    props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  expect(await pane.find({text:'1/2'})).toBeDefined();expect(await pane.find({text:'tasks done'})).toBeDefined();
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'main',reason:'answer'});await clock.advance(1);
  expect(await pane.find({text:'Could not update. The panel shows the last reading.'})).toBeDefined();
  expect(await pane.find({text:'1/2'})).toBeDefined();
  // O terminal não desenha o fechar: a pessoa fecha pela marca do painel, e o kit aperta o do desktop.
  expect(await pane.find({key:'quit'})).toBeUndefined();
  const desktop=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await desktop.press({key:'quit'});
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'late',reason:'answer'});await clock.advance(1);
  expect(reads).toBe(2);
});

test('advertised publication tools do not delegate an unverified upload to a model', async ($, on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on, {});
  let turns = 0;
  let uploads = 0;
  on('session.start', ($, e) => ({cwd:e.cwd}));
  on('command.register', () => ({value:null}));
  on('session.cwd', () => ({value:'/fixture'}));
  on('process.run', () => ({value:{exitCode:0,stderr:'',stdout:JSON.stringify({
    ok:true,published:false,snapshot_id:'local-version',
    page:'/fixture/export/index.html',database:'/fixture/export/snapshot.json',
  })}}));
  on('tool.list', () => ({value:[{name:'Artifact'},{name:'ArtifactData'}]}));
  on('prompt.submit', () => { turns++; return {}; });
  on('tool.call', () => { uploads++; return {}; });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result = await $.command.run({command:'mustard:pages',args:'spec demo'});
  expect(result.text).toContain('Os arquivos permanecem locais');
  expect(turns).toBe(0);
  expect(uploads).toBe(0);
});

test('native publication exposes a confirmed URL without calling the model', async ($, on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on, {});
  let turns=0, toolCalls=0;
  on('session.start', ($,e)=>({cwd:e.cwd}));
  on('command.register', ()=>({value:null}));
  on('session.cwd', ()=>({value:'/fixture'}));
  on('process.run', ($,e)=>{
    expect(e.init.timeoutMs).toBe(75000);
    return {value:{exitCode:0,stderr:'',stdout:JSON.stringify({ok:true,published:true,recorded:true,
      snapshot_id:'confirmed-version',remote_url:'https://abc.example.pages.dev'})}};
  });
  on('prompt.submit',()=>{turns++;return {};});
  on('tool.call',()=>{toolCalls++;return {};});
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result=await $.command.run({command:'mustard:pages',args:'spec demo'});
  expect(result.text).toContain('https://abc.example.pages.dev');
  expect(turns).toBe(0);expect(toolCalls).toBe(0);
});

test('pending deployment tells the user to resume it without advertising a URL', async ($, on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on, {});
  on('session.start', ($,e)=>({cwd:e.cwd}));
  on('command.register', ()=>({value:null}));
  on('session.cwd', ()=>({value:'/fixture'}));
  on('process.run',()=>({value:{exitCode:0,stderr:'',stdout:JSON.stringify({ok:true,published:false,pending:true,
    snapshot_id:'pending-version',deployment_id:'accepted-id',remote_url:null})}}));
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result=await $.command.run({command:'mustard:pages',args:'spec demo'});
  expect(result.text).toContain('accepted-id');expect(result.text).toContain('/mustard:pages spec demo');
  expect(result.text).not.toContain('https://');
});

test('report publication preserves the requested document path without calling the model', async ($,on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on,{});let turns=0;
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));on('session.cwd',()=>({value:'/fixture'}));
  on('process.run',($,e)=> {expect(e.argv.slice(1)).toEqual(['run','publish','--root','/fixture','--document','reports/Resumo gestor.md']);
    return {value:{exitCode:0,stderr:'',stdout:JSON.stringify({ok:true,published:false,snapshot_id:'report-version',page:'index.html',database:'snapshot.json'})}};});
  on('prompt.submit',()=>{turns++;return {};});
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result=await $.command.run({command:'mustard:pages',args:'report "reports/Resumo gestor.md"'});
  expect(result.text).toContain('report-version');expect(turns).toBe(0);
});

test('the panel and publication commands are the plugin files, answered by the module without a model turn', async ($,on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on,{});let turns=0;const registered:string[]=[];
  on('session.start',($,e)=>({cwd:e.cwd}));on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',($,e)=>{registered.push(e.name);return {value:{command:e.name}};});
  on('prompt.submit',()=>{turns++;return {};});
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  expect(registered).toEqual([]);
  const usage=await $.command.run({command:'mustard:pages',args:''});
  expect(usage.text).toContain('/mustard:pages project');expect(turns).toBe(0);
});

test('a command hook that fails answers in text and runs nothing', () => {
  const reply=commandFailed();
  expect(reply.text).toContain('Nada foi executado');expect(reply.text).toContain('/plugin');
  expect(reply.context).toBeUndefined();
});

test('panel and usage failures reach the diagnostics with their cause, once', async ($, on) => {
  const clock = mock.clock(on);
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on, {});
  let reads = 0;
  const diagnostics:any[] = [];
  const replies = [
    {ok:false,reason:'usage-cost-unknown',live:{ok:false,reason:'live-usage-unreadable',detail:'EOF while parsing a string',diagnosed:true}},
    {ok:false,reason:'io error: No space left on device (os error 28)'},
  ];
  on('session.start', ($, e) => ({cwd:e.cwd}));on('session.measure',($,e)=>({changed:e.changed}));
  on('command.register', () => ({value:null}));
  on('session.cwd', () => ({value:'/fixture'}));on('session.id',()=>({value:'actual-session'}));on('session.model',()=>({value:'test-model'}));
  on('session.usage', () => ({value:{rateLimits:[]}}));
  on('ui.open', () => ({value:null}));
  on('ui.close', () => ({value:null}));
  on('turn.complete',()=>({text:''}));
  on('process.run', ($, e) => {
    if(e.argv[2]==='panel-diagnostics') {diagnostics.push(JSON.parse(e.init.stdin));return {value:{exitCode:0,stdout:'{"ok":true}',stderr:''}};}
    if(e.argv[2]==='usage-record')return {value:{exitCode:0,stdout:JSON.stringify(replies.shift()),stderr:''}};
    reads++;
    return {value:reads===1 ? {exitCode:0,stdout:JSON.stringify(overview()),stderr:''} : {exitCode:1,stdout:'',stderr:'database is locked\n'}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  for(const _ of [0,1])await $.session.measure({context:{percent:1,tokens:10,window:200000},cost:{usd:0.1},rateLimits:[],changed:['cost']});
  await $.command.run({command:'mustard:mods',args:''});
  await $.ui.mount({plugin:'mustard',surface:'terminal',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await $.turn.complete({answer:'',durationMs:1,isAborted:false,turnId:'main',reason:'answer'});
  await clock.advance(10);
  expect(diagnostics).toEqual([
    {component:'usage',operation:'record',reason:'usage-record-failed',detail:'io error: No space left on device (os error 28)',session:'actual-session'},
    {component:'panel',operation:'refresh',reason:'panel-query-failed',detail:'exit 1: database is locked',session:'actual-session'},
  ]);
});

test('a module restart restores the requested pane, spec and screen without a model',async ($,on)=>{
  const f=await openPane($,on,{saved:{wanted:true,state:{screen:'usage',spec:'demo'}}});
  expect(f.opens).toBe(1);expect(f.commands[0]).toEqual(expect.arrayContaining(['--section','usage','--spec','demo']));
  expect(await f.pane.find({text:/medido até/i})).toBeDefined();expect(f.turns).toBe(0);await f.pane.press({key:'quit'});
});
test('a module restart reopens the waves screen on its page, each wave with its own spend',async ($,on)=>{
  const f=await openPane($,on,{saved:{wanted:true,state:{screen:'waves',spec:'demo',waveOffset:8}}});
  expect(f.commands[0]).toEqual(expect.arrayContaining(['--section','waves','--spec','demo','--offset','8','--limit','8']));
  for(const text of ['ONDAS · 9','página 2 de 2','Onda 1','comitada em 02/10','4,32 mil tokens · 2 h 0 min · claude-opus-5-5 · esforço pedido xhigh','9 de 9 · da mais nova para a mais antiga'])
    expect(await f.pane.find({text}),text).toBeDefined();
  expect(await f.pane.find({key:'waves-previous'})).toBeDefined();expect(await f.pane.find({key:'waves-next'})).toBeUndefined();
  await f.pane.press({key:'waves-previous'});
  expect(f.commands.at(-1)).toEqual(expect.arrayContaining(['--section','waves','--offset','0']));
  expect(f.turns).toBe(0);await f.pane.press({key:'quit'});
});
test('each reopening records its own render',async ($,on)=>{
  const f=await openPane($,on);
  await f.pane.unmount();
  await $.command.run({command:'mustard:mods',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',requestId:'mustard-panel',
    props:{id:'mustard-panel',title:'Mustard',bodyColumns:120,scroll:{bodyRows:40}}});
  const details=await $.command.run({command:'mustard:mods',args:'--diagnose'});
  expect(details.text.match(/render:desktop/g)?.length).toBe(2);
  expect(f.turns).toBe(0);await pane.press({key:'quit'});
});
test('costless host measurements still reach the native recorder',async ($,on)=>{
  const f=await openPane($,on);
  await $.session.measure({context:{tokens:30000,window:200000,percent:15},rateLimits:[],changed:['context']});
  expect(f.receipts.length).toBe(1);expect(f.receipts[0].context.tokens).toBe(30000);expect(f.receipts[0].cost).toBeUndefined();
  expect(f.turns).toBe(0);await f.pane.press({key:'quit'});
});

test('native model request observers preserve the stream and record only usage metadata',async ($,on)=>{
  on('turn.step',async function* ($,e){return {turnId:e.turnId,index:e.index,answer:'unchanged answer',toolUses:[],stopReason:'end_turn',
    usage:{model:'fixture-model',input_tokens:10,output_tokens:5,cache_read_input_tokens:20,cache_creation_input_tokens:0}};});
  const f=await openPane($,on);
  const stream=$.turn.step({turnId:'turn-fixture',index:0,model:'fixture-model',effort:'medium',messageCount:2,agentId:'agent-fixture'});
  const result=await stream.next();expect(result.done).toBe(true);expect(result.value.answer).toBe('unchanged answer');
  expect(f.receipts.length).toBe(1);expect(f.receipts[0].kind).toBe('step');expect(f.receipts[0].agent_id).toBe('agent-fixture');
  expect(f.receipts[0].effort).toBe('medium');expect(f.receipts[0].usage.input_tokens).toBe(10);
  expect(JSON.stringify(f.receipts)).not.toContain('unchanged answer');expect(f.turns).toBe(0);await f.pane.press({key:'quit'});
});
test('the analysis step pins the effort and leaves the model the agent definition resolved',async ($,on)=>{
  const seen:any[]=[];
  on('agent.spawn',()=>({agentId:'agent-analysis',model:'claude-haiku-5-5'}));
  on('turn.step',async function* ($,e){seen.push(e);return {turnId:e.turnId,index:e.index,answer:'ok',toolUses:[],stopReason:'end_turn',usage:null};});
  const f=await openPane($,on);
  await $.agent.spawn({subagentType:'mustard-analysis',fork:false,prompt:'bounded'});
  await $.turn.step({turnId:'turn-analysis',index:0,model:'claude-haiku-5-5',effort:'high',messageCount:1,agentId:'agent-analysis'}).next();
  expect(seen.length).toBe(1);expect(seen[0].effort).toBe('medium');expect(seen[0].model).toBe('claude-haiku-5-5');
  await f.pane.press({key:'quit'});
});
