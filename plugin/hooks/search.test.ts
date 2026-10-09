import { test, expect, mock } from 'claude-code/testing';

test('gateway tool transports original inputs and intent through host permissions',async ($,on)=>{
  mock.env(on,{});
  let shellCalls=0,processCalls=0;
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture with spaces'}));
  on('command.register',()=>({value:null}));
  on('tool.register',($,e)=>{
    expect(e.tool.name).toBe('search');
    expect(e.tool.inputSchema.required).toEqual(['request']);
    return {value:{tool:'mcp__mustard__search'}};
  });
  on('process.run',()=>{processCalls++;throw new Error('Search must not bypass host tools');});
  on('tool.call',{tool:'Bash'},($,e)=>{
    shellCalls++;
    expect(e.command).toContain('run search --root');
    expect(e.command).toContain('"pattern":"save|restore"');
    expect(e.command).toContain('"intent":"repair persistence"');
    expect(e.command).toContain('"choose":false');
    return {result:{stdout:'{"result":{"content":"src/store:12:save"},"evidence":null}',stderr:'',interrupted:false,isImage:false}};
  });
  await $.session.start({cwd:'/fixture with spaces',surface:'terminal',isInteractive:true});
  const result=await $.tool.call({tool:'mcp__mustard__search',request:{tool:'Grep',input:{pattern:'save|restore',output_mode:'content','-n':true},intent:'repair persistence',choose:false}});
  expect(result.result.content[0].text).toContain('src/store:12:save');
  expect(shellCalls).toBe(1);
  expect(processCalls).toBe(0);
});

test('a refused host execution never becomes gateway evidence',async ($,on)=>{
  mock.env(on,{});
  on('session.start',($,e)=>({cwd:e.cwd}));
  on('session.cwd',()=>({value:'/fixture'}));
  on('command.register',()=>({value:null}));
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('tool.call',{tool:'Bash'},()=>({deny:'fixture permission refusal'}));
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  const result=await $.tool.call({tool:'mcp__mustard__search',request:{tool:'rg',input:{args:['-n','save','src']}}});
  expect(result.deny).toBe('fixture permission refusal');
});
