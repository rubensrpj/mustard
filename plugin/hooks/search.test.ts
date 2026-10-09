import { test, expect, mock } from 'claude-code/testing';

test('gateway tool transports task evidence and intent through host permissions',async ($,on)=>{
  mock.env(on,{});
  let shellCalls=0,processCalls=0;
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture with spaces'}));
  on('command.register',()=>({value:null}));
  on('tool.register',($,e)=>{
    expect(e.tool.name).toBe('search');
    expect(e.tool.inputSchema.required).toEqual(['request']);
    expect(e.tool.inputSchema.properties.request.required).toEqual(['tool','input','intent','purpose']);
    return {value:{tool:'mcp__mustard__search'}};
  });
  on('process.run',()=>{processCalls++;throw new Error('Search must not bypass host tools');});
  on('tool.call',{tool:'Bash'},($,e)=>{
    shellCalls++;
    expect(e.command).toContain('run search --root');
    expect(e.command).toContain('--shell-output');
    expect(e.command).toContain('"pattern":"save|restore"');
    expect(e.command).toContain('"intent":"repair persistence"');
    expect(e.command).toContain('"purpose":"spec"');
    expect(e.command).toContain('"choose":false');
    expect(e.command).toContain('"schema_version":1');
    return {result:{stdout:'# task evidence (Spec)\n@ src/store\n12 | save',stderr:'',interrupted:false,isImage:false}};
  });
  await $.session.start({cwd:'/fixture with spaces',surface:'terminal',isInteractive:true});
  const result=await $.tool.call({tool:'mcp__mustard__search',request:{tool:'Grep',input:{pattern:'save|restore',output_mode:'content','-n':true},intent:'repair persistence',purpose:'spec',choose:false}});
  expect(result.result.content[0].text).toBe('# task evidence (Spec)\n@ src/store\n12 | save');
  expect(shellCalls).toBe(1);
  expect(processCalls).toBe(0);
});

test('gateway returns native search errors instead of dropping stderr',async ($,on)=>{
  mock.env(on,{});
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('tool.call',{tool:'Bash'},()=>({result:{stdout:'',stderr:'rg: regex parse error'},isError:true}));
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result=await $.tool.call({tool:'mcp__mustard__search',request:{tool:'rg',input:{args:['[','src']},intent:'',purpose:'locate'}});
  expect(result.result.content[0].text).toBe('rg: regex parse error');
  expect(result.result.isError).toBe(true);
});

test('a refused host execution never becomes gateway evidence',async ($,on)=>{
  mock.env(on,{});
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('tool.call',{tool:'Bash'},()=>({deny:'fixture permission refusal'}));
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result=await $.tool.call({tool:'mcp__mustard__search',request:{tool:'rg',input:{args:['-n','save','src']},intent:'',purpose:'locate'}});
  expect(result.deny).toBe('fixture permission refusal');
});

test('invalid investigations get a corrective result before any host execution',async ($,on)=>{
  mock.env(on,{});
  let executions=0;
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('process.run',()=>{executions++;throw new Error('No process for invalid input');});
  on('tool.call',{tool:'Bash'},()=>{executions++;throw new Error('No search for invalid input');});
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
    expect(result.result.isError).toBe(true);
    expect(result.result.content[0].text).toContain('search-contract:');
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
  on('tool.call',{tool:'Bash'},($,e)=>{
    const encoded=e.command.match(/--request '(.*?)' --shell-output/)[1];
    contexts.push(JSON.parse(encoded).context);
    return {result:{stdout:`evidence\n# mustard-delivery:${receipt}:8:ae70fc8f5906c564\n`,stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const request={tool:'Symbol',input:{file_path:'a.ts',symbol:'alpha'},intent:'inspect alpha',purpose:'implement'};
  const first=await $.tool.call({tool:'mcp__mustard__search',request});
  expect(first.result.content[0].text).toBe('evidence');
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

test('failed, interrupted or truncated host deliveries never acknowledge their receipt',async ($,on)=>{
  mock.env(on,{});
  const receipt='b'.repeat(64),contexts=[];let mode='error';
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('session.id',()=>({value:'fixture-session'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('tool.call',{tool:'Bash'},($,e)=>{
    contexts.push(JSON.parse(e.command.match(/--request '(.*?)' --shell-output/)[1]).context);
    return {result:{stdout:`${mode==='truncated'?'cut':'partial'}\n# mustard-delivery:${receipt}:7:b5d43e6c2878bc7a\n`,stderr:'failed',interrupted:mode==='interrupted'},isError:mode==='error'};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const request={tool:'rg',input:{args:['-n','alpha','a.ts']},intent:'inspect alpha',purpose:'implement'};
  await $.tool.call({tool:'mcp__mustard__search',request});
  await $.tool.call({tool:'mcp__mustard__search',request});
  expect(contexts[1].acknowledged).toEqual([]);
  for (const next of ['truncated','interrupted']) {
    mode=next;await $.tool.call({tool:'mcp__mustard__search',request});await $.tool.call({tool:'mcp__mustard__search',request});
    expect(contexts.at(-1).acknowledged).toEqual([]);
  }
});
