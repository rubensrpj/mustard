import { test, expect, mock } from 'claude-code/testing';

const envelopeOf=(argv:string[])=>JSON.parse(argv[argv.indexOf('--request')+1]);

test('investigative discovery permits native ambiguity decisions while exact requests and opt-outs stay native',async ($,on)=>{
  mock.env(on,{});
  on('session.start',($,e)=>({cwd:e.cwd}));on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',()=>({value:null}));on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  const routed=[];
  on('process.run',($,e)=>{
    routed.push(envelopeOf(e.argv).request);
    return {value:{exitCode:0,stdout:'native evidence',stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const requests=[
    {tool:'Grep',input:{pattern:'import'},intent:'',purpose:'locate'},
    {tool:'Grep',input:{pattern:'import'},intent:'Which method completes import?',purpose:'spec'},
    {tool:'Grep',input:{pattern:'import'},intent:'Which method completes import?',purpose:'spec',choose:false},
    {tool:'Read',input:{file_path:'/fixture/service.ts',offset:10,limit:5},intent:'Read the implementation',purpose:'understand'},
  ];
  for(const request of requests)await $.tool.call({tool:'mcp__mustard__search',request});
  expect(routed.map(r=>r.choose)).toEqual([false,true,false,false]);
  for(let i=0;i<requests.length;i++)expect(routed[i].input).toEqual(requests[i].input);
});

test('gateway tool runs the Mustard binary directly with the task evidence request',async ($,on)=>{
  mock.env(on,{});
  let processCalls=0,registrations=0;
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture with spaces'}));
  on('session.id',()=>({value:'fixture-session'}));
  on('command.register',()=>({value:null}));
  on('tool.register',($,e)=>{
    expect(e.name).toBe('search');
    expect(e.inputSchema.required).toEqual(['request']);
    expect(e.inputSchema.properties.request.required).toEqual(['tool','input','intent','purpose']);
    registrations++;
    return {value:{tool:'mcp__mustard__search'}};
  });
  on('process.run',($,e)=>{
    processCalls++;
    expect(e.argv[0]).toMatch(/\/bin\/mustard-rt$/);
    expect(e.argv.slice(1)).toEqual(['run','search','--root','/fixture with spaces','--request',e.argv[6],'--shell-output']);
    const envelope=envelopeOf(e.argv);
    expect(envelope.schema_version).toBe(1);
    expect(envelope.request).toEqual({tool:'Grep',input:{pattern:'save|restore',output_mode:'content','-n':true},intent:'repair persistence',purpose:'spec',choose:false});
    expect(envelope.context.session).toBe('fixture-session');
    expect(e.init.cwd).toBe('/fixture with spaces');
    expect(e.init.timeoutMs).toBe(120000);
    expect(e.init.env).toEqual({MUSTARD_SESSION_ID:'fixture-session'});
    return {value:{exitCode:0,stdout:'# task evidence (Spec)\n@ src/store\n12 | save',stderr:''}};
  });
  await $.session.start({cwd:'/fixture with spaces',surface:'terminal',isInteractive:true});
  const result=await $.tool.call({tool:'mcp__mustard__search',request:{tool:'Grep',input:{pattern:'save|restore',output_mode:'content','-n':true},intent:'repair persistence',purpose:'spec',choose:false}});
  expect(result.result).toBe('# task evidence (Spec)\n@ src/store\n12 | save');
  expect(result.isError).toBeUndefined();
  expect(processCalls).toBe(1);
  expect(registrations).toBe(1);
});

test('search never routes through the Bash tool',async ($,on)=>{
  mock.env(on,{});
  let shellCalls=0;
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('tool.call',{tool:'Bash'},()=>{shellCalls++;throw new Error('Search must not route through the Bash tool');});
  on('process.run',()=>({value:{exitCode:0,stdout:'src/store.ts:12:save',stderr:''}}));
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result=await $.tool.call({tool:'mcp__mustard__search',request:{tool:'rg',input:{args:['-n','save','src']},intent:'',purpose:'locate'}});
  expect(result.result).toBe('src/store.ts:12:save');
  expect(result.isError).toBeUndefined();
  expect(shellCalls).toBe(0);
});

test('gateway returns native search errors instead of dropping stderr',async ($,on)=>{
  mock.env(on,{});
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('process.run',()=>({value:{exitCode:2,stdout:'',stderr:'rg: regex parse error'}}));
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result=await $.tool.call({tool:'mcp__mustard__search',request:{tool:'rg',input:{args:['[','src']},intent:'',purpose:'locate'}});
  expect(result.result).toBe('rg: regex parse error');
  expect(result.isError).toBe(true);
});

test('a failed process run never becomes gateway evidence',async ($,on)=>{
  mock.env(on,{});
  const receipt='b'.repeat(64),contexts=[];let exitCode=1;
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('session.id',()=>({value:'fixture-session'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('process.run',($,e)=>{
    contexts.push(envelopeOf(e.argv).context);
    return {value:{exitCode,stdout:`partial\n# mustard-delivery:${receipt}:7:b5d43e6c2878bc7a\n`,stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const request={tool:'rg',input:{args:['-n','alpha','a.ts']},intent:'inspect alpha',purpose:'implement'};
  const failed=await $.tool.call({tool:'mcp__mustard__search',request});
  expect(failed.result).toBe('partial');
  expect(failed.isError).toBe(true);
  exitCode=0;
  await $.tool.call({tool:'mcp__mustard__search',request});
  expect(contexts[1].acknowledged).toEqual([]);
  // The same complete delivery is accepted once the run succeeds.
  await $.tool.call({tool:'mcp__mustard__search',request});
  expect(contexts[2].acknowledged).toEqual([receipt]);
});

test('invalid investigations get a corrective result before any host execution',async ($,on)=>{
  mock.env(on,{});
  let executions=0;
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('process.run',()=>{executions++;throw new Error('No process for invalid input');});
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  for(const request of [
    {tool:'rg',input:{args:['save','src']},intent:'question'},
    {tool:'rg',input:{args:['save','src']},intent:' ',purpose:'implement'},
    {tool:'rg',input:{args:['save','src']},intent:'',purpose:'locate',choose:true},
    {tool:'rg',input:{args:['save','src']},intent:'question',purpose:'spec',choose:'true'},
    {tool:'rg',input:{args:['save','src']},intent:'question',purpose:'spec',invented_evidence:true},
    {tool:'References',input:{file_path:'a.ts',line:0,column:0},intent:'definition',purpose:'locate'},
    {tool:'References',input:{file_path:'a.ts',line:1},intent:'definition',purpose:'locate'},
    {tool:'Read',input:{file_path:'a.ts',pattern:'save'},intent:'definition',purpose:'locate'},
    {tool:'rg',input:{args:[7]},intent:'definition',purpose:'locate'},
  ]){
    const result=await $.tool.call({tool:'mcp__mustard__search',request});
    expect(result.isError).toBe(true);
    expect(result.result).toContain('search-contract:');
  }
  expect(executions).toBe(0);
});

test('delivery receipts are acknowledged only in the same agent and uncompressed host context',async ($,on)=>{
  mock.env(on,{});
  const receipt='a'.repeat(64),contexts=[];
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('session.id',()=>({value:'fixture-session'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.compact',()=>({skip:'test controls context reset'}));
  on('process.run',($,e)=>{
    contexts.push(envelopeOf(e.argv).context);
    return {value:{exitCode:0,stdout:`evidence\n# mustard-delivery:${receipt}:8:ae70fc8f5906c564\n`,stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const request={tool:'Symbol',input:{file_path:'a.ts',symbol:'alpha'},intent:'inspect alpha',purpose:'implement'};
  const first=await $.tool.call({tool:'mcp__mustard__search',request});
  expect(first.result).toBe('evidence');
  await $.tool.call({tool:'mcp__mustard__search',request});
  await $.tool.call({tool:'mcp__mustard__search',agentId:'worker-A',request});
  expect(contexts[0].acknowledged).toEqual([]);
  expect(contexts[1].acknowledged).toEqual([receipt]);
  expect(contexts[2].agent).toBe('worker-A');
  expect(contexts[2].acknowledged).toEqual([]);
  await $.session.compact({});
  await $.tool.call({tool:'mcp__mustard__search',request});
  expect(contexts[3].acknowledged).toEqual([]);
  expect(contexts[3].epoch===contexts[1].epoch).toBe(false);
});

test('truncated deliveries never acknowledge their receipt',async ($,on)=>{
  mock.env(on,{});
  const receipt='b'.repeat(64),contexts=[];
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('session.id',()=>({value:'fixture-session'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('process.run',($,e)=>{
    contexts.push(envelopeOf(e.argv).context);
    return {value:{exitCode:0,stdout:`cut\n# mustard-delivery:${receipt}:7:b5d43e6c2878bc7a\n`,stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const request={tool:'rg',input:{args:['-n','alpha','a.ts']},intent:'inspect alpha',purpose:'implement'};
  await $.tool.call({tool:'mcp__mustard__search',request});
  await $.tool.call({tool:'mcp__mustard__search',request});
  expect(contexts[1].acknowledged).toEqual([]);
});

test('minimal Grep and Read requests return registered-tool text rather than an MCP envelope',async ($,on)=>{
  mock.env(on,{});
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  const output='50 | async generateOriginalMenuUrl() {\n51 |   return storage.url;\n52 | }';
  on('process.run',()=>({value:{exitCode:0,stdout:output,stderr:''}}));
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  for(const request of [
    {tool:'Grep',input:{pattern:'generateOriginalMenuUrl',output_mode:'content','-n':true},intent:'Locate the original menu generator',purpose:'locate'},
    {tool:'Grep',input:{pattern:'generateOriginalMenuUrl'},intent:'',purpose:'locate'},
    {tool:'Read',input:{file_path:'/fixture/src/puzzle/services/puzzle.service.ts',offset:50,limit:15},intent:'Corpo de generateOriginalMenuUrl',purpose:'locate'},
  ]) {
    const answer=await $.tool.call({tool:'mcp__mustard__search',request});
    expect(typeof answer.result).toBe('string');
    expect(answer.result).toBe(output);
    expect(answer.isError).toBeUndefined();
  }
});

test('silent failures and unfinished runs remain errors with their reason at the tool-call boundary',async ($,on)=>{
  mock.env(on,{});
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  let unfinished=false;
  on('process.run',()=>{
    // A skipped answer leaves the host run rejected, as a timeout does.
    if(unfinished)throw new Error('run never finished');
    return {value:{exitCode:2,stdout:'',stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const request={tool:'Grep',input:{pattern:'generateOriginalMenuUrl'},intent:'',purpose:'locate'};
  const silent=await $.tool.call({tool:'mcp__mustard__search',request});
  expect(silent.result).toBe('Process exited with code 2');
  expect(silent.isError).toBe(true);
  unfinished=true;
  const aborted=await $.tool.call({tool:'mcp__mustard__search',request});
  expect(aborted.result).toMatch(/^Mustard search did not finish: \S/);
  expect(aborted.isError).toBe(true);
});

test('successful empty output and stderr warnings preserve their native text without an error envelope',async ($,on)=>{
  mock.env(on,{});
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  let warning=false;
  on('process.run',()=>({value:{exitCode:0,stdout:'',stderr:warning?'rg: warning':''}}));
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const request={tool:'Grep',input:{pattern:'notPresent'},intent:'',purpose:'locate'};
  const empty=await $.tool.call({tool:'mcp__mustard__search',request});
  expect(empty.result).toBe('');
  expect(empty.isError).toBeUndefined();
  warning=true;
  const warned=await $.tool.call({tool:'mcp__mustard__search',request});
  expect(warned.result).toBe('rg: warning');
  expect(warned.isError).toBeUndefined();
});
