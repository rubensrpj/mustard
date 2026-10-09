import { test, expect, mock } from 'claude-code/testing';

test('panel updates locally, preserves unknown usage, and stops on close', async ($, on) => {
  const clock = mock.clock(on);
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on, {});
  let reads = 0;
  let measurements = 0;
  let turns = 0;
  let tools = 0;
  const snapshot = {
    ok: true, schema_version: 1, at: '2026-10-08T12:00:00Z',
    project: {name:'fixture',branch:'feature/demo'}, selected_spec:'demo',
    specs:[{name:'demo',phase:'running',goal:'Demo',waves:[],stages:[],
      final_validation_valid:false,review_approved:false,undeclared_commands:[],
      usage:{wave_tokens:null,conductor_tokens:null}}],
    jev:{physical_requests:0,known_input_tokens:0,requests_with_unknown_usage:0,cost_micro_usd:null},
  };
  on('session.start', ($, e) => ({cwd:e.cwd}));
  on('command.register', () => ({value:null}));
  on('session.cwd', () => ({value:'/fixture'}));
  on('session.usage', () => ({value:{context:{window:200000,tokens:12000,percentUsed:6},rateLimits:[],cost:{usd:0.12}}}));
  on('process.run', ($, e) => {
    reads++;
    if(e.argv.includes('--refresh-consumption')) measurements++;
    expect(e.argv.slice(1,5)).toEqual(['run','panel','--root','/fixture']);
    return {value:{exitCode:0,stdout:JSON.stringify(snapshot),stderr:''}};
  });
  on('ui.open', () => ({value:null}));
  on('ui.close', () => ({value:null}));
  on('prompt.submit', () => { turns++; return {}; });
  on('tool.call', () => { tools++; return {}; });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  await $.command.run({command:'mustard-panel',args:''});
  expect(reads).toBe(1);
  const pane = await $.ui.mount({plugin:'mustard',surface:'terminal',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'tab-Consumo'});
  expect(await pane.find({type:'Text',text:'desconhecido'})).toBeDefined();
  await clock.advance(2000);
  expect(reads).toBe(2);
  expect(measurements).toBe(1);
  await pane.press({key:'close'});
  await clock.advance(10000);
  expect(reads).toBe(2);
  expect(turns).toBe(0);
  expect(tools).toBe(0);
});

test('host measurements persist natively and official context percent renders without a model turn', async ($,on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on,{});mock.clock(on);
  let turns=0;
  const recorded:any[]=[];
  const snapshot={ok:true,schema_version:1,project:{name:'fixture'},at:'2026-10-08T12:00:00Z',specs:[],jev:{},
    consumption:{available:true,project_tokens:100,machine_tokens:1000,measured_at:'fixture-time'},
    claude_cost:{known_micro_usd:500000,unattributed_micro_usd:200000}};
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
    return {value:{exitCode:0,stdout:JSON.stringify(snapshot),stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  await $.session.measure({context:{percent:12,tokens:24000,window:200000},cost:{usd:0.5},rateLimits:[],changed:['cost']});
  expect(recorded).toEqual([{cost:{usd:0.5},model:'test-model'}]);
  await $.command.run({command:'mustard-panel',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface:'terminal',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'tab-Consumo'});
  expect(await pane.find({type:'Text',text:'Contexto da sessão: 12% · 24.000/200.000 tokens'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Claude estimado nas sessões observadas: $0.5000'})).toBeDefined();
  expect(turns).toBe(0);
  await pane.press({key:'close'});
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
  const result = await $.command.run({command:'mustard-pages',args:'spec demo'});
  expect(result.text).toContain('ainda não foram publicados');
  expect(publications).toBe(1);
  expect(turns).toBe(0);
});

test('a Windows query failure retains the dated state and the English panel', async ($, on) => {
  const clock = mock.clock(on);
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on, {OS:'Windows_NT'});
  let reads = 0;
  const snapshot = {ok:true,schema_version:1,at:'2026-10-08T12:00:00Z',
    project:{name:'fixture',language:'en-US'},selected_spec:'demo',
    specs:[{name:'demo',phase:'running',goal:'Demo',waves:[],stages:[],final_validation_valid:null,
      review_approved:false,undeclared_commands:[],usage:{wave_tokens:null,conductor_tokens:null}}],
    jev:{physical_requests:null,known_input_tokens:0,requests_with_unknown_usage:1,cost_micro_usd:null}};
  on('session.start', ($, e) => ({cwd:e.cwd}));
  on('command.register', () => ({value:null}));
  on('session.cwd', () => ({value:'C:/fixture'}));
  on('session.usage', () => ({value:{rateLimits:[]}}));
  on('ui.open', () => ({value:null}));
  on('ui.close', () => ({value:null}));
  on('process.run', ($, e) => {
    reads++;
    expect(e.argv[0]).toMatch(/mustard-rt\.exe$/);
    return {value:reads===1 ? {exitCode:0,stdout:JSON.stringify(snapshot),stderr:''}
      : {exitCode:1,stdout:'',stderr:'fixture query failed'}};
  });
  await $.session.start({cwd:'C:/fixture',surface:'terminal',isInteractive:true});
  await $.command.run({command:'mustard-panel',args:''});
  const pane = await $.ui.mount({plugin:'mustard',surface:'terminal',component:'Pane',
    props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'tab-Execução'});
  expect(await pane.find({type:'Text',text:'Final validation: unknown'})).toBeDefined();
  await clock.advance(2000);
  expect(await pane.find({type:'Text',text:'Last state: 2026-10-08T12:00:00Z'})).toBeDefined();
  await pane.press({key:'close'});
  await clock.advance(10000);
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
  const result = await $.command.run({command:'mustard-pages',args:'spec demo'});
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
  const result=await $.command.run({command:'mustard-pages',args:'spec demo'});
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
  const result=await $.command.run({command:'mustard-pages',args:'spec demo'});
  expect(result.text).toContain('accepted-id');expect(result.text).toContain('/mustard-pages spec demo');
  expect(result.text).not.toContain('https://');
});

test('project publication needs no selected spec and shows its result in the pane', async ($,on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on,{});mock.clock(on);
  let publications=0, turns=0;
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{rateLimits:[]}}));
  on('ui.open',()=>({value:null}));on('ui.close',()=>({value:null}));
  on('process.run',($,e)=> {
    if(e.argv[2]==='panel') return {value:{exitCode:0,stderr:'',stdout:JSON.stringify({ok:true,schema_version:1,
      project:{name:'fixture',version:'0.2.7'},specs:[],jev:{},at:'2026-10-08T12:00:00Z'})}};
    publications++;expect(e.argv.slice(1)).toEqual(['run','publish','--root','/fixture','--project']);
    return {value:{exitCode:0,stderr:'',stdout:JSON.stringify({ok:true,published:true,scope:'project',
      snapshot_id:'project-version',remote_url:'https://abc.example.pages.dev'})}};
  });
  on('prompt.submit',()=>{turns++;return {};});
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  await $.command.run({command:'mustard-panel',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface:'terminal',component:'Pane',
    props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'publish-project'});
  expect(await pane.find({type:'Text',text:'Snapshot project-version publicado: https://abc.example.pages.dev'})).toBeDefined();
  expect(publications).toBe(1);expect(turns).toBe(0);
});

test('completed tool events coalesce local refreshes without opening another model turn', async ($,on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on,{});const clock=mock.clock(on);let reads=0;
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{rateLimits:[]}}));
  on('ui.open',()=>({value:null}));on('ui.close',()=>({value:null}));
  on('process.run',()=> {reads++;return {value:{exitCode:0,stderr:'',stdout:JSON.stringify({ok:true,schema_version:1,
    project:{name:'fixture'},specs:[],jev:{},at:'2026-10-08T12:00:00Z'})}};});
  on('tool.call',()=>({result:{text:'fixture read'}}));
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  await $.command.run({command:'mustard-panel',args:''});expect(reads).toBe(1);
  await $.tool.call({tool:'Read',file_path:'/fixture/a'});await $.tool.call({tool:'Read',file_path:'/fixture/b'});
  await clock.advance(200);expect(reads).toBe(2);
  const pane=await $.ui.mount({plugin:'mustard',surface:'terminal',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'close'});await clock.advance(5000);expect(reads).toBe(2);
});

test('report publication preserves the requested document path without calling the model', async ($,on) => {
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  mock.env(on,{});let turns=0;
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));on('session.cwd',()=>({value:'/fixture'}));
  on('process.run',($,e)=> {expect(e.argv.slice(1)).toEqual(['run','publish','--root','/fixture','--document','reports/Resumo gestor.md']);
    return {value:{exitCode:0,stderr:'',stdout:JSON.stringify({ok:true,published:false,snapshot_id:'report-version',page:'index.html',database:'snapshot.json'})}};});
  on('prompt.submit',()=>{turns++;return {};});
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result=await $.command.run({command:'mustard-pages',args:'report "reports/Resumo gestor.md"'});
  expect(result.text).toContain('report-version');expect(turns).toBe(0);
});
