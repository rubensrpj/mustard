import {test,expect,mock} from 'claude-code/testing';

async function fixture($,on,surface='desktop',english=false) {
  const clock=mock.clock(on);mock.env(on,{});
  const calls:any[]=[];
  let reads=0,fail=false,turns=0;
  let stored:any={ok:true,schema_version:1,revision:'revision-1',values:{enabled:true,rtk:false,vectors:false,jev:false,
    jev_search:false,jev_wave_planning:false,jev_context:false,agent_model:'custom-model',agent_effort:'xhigh',search_answer:true},
    index_present:false,jev:{usable_key:false,provider_verified:false}};
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));on('command.register',()=>({value:null}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('session.cwd',()=>({value:'/fixture'}));
  on('session.usage',()=>({value:{}}));on('ui.open',()=>({value:null}));on('ui.close',()=>({value:null}));
  on('prompt.submit',()=>{turns++;throw new Error('No model turn for settings');});
  on('process.run',($,e)=>{
    const command=e.argv[2];
    let result:any;
    if(command==='settings') {
      calls.push(e.argv);
      if(e.argv.includes('--json')) {
        const request=JSON.parse(e.argv[e.argv.indexOf('--json')+1]);
        expect(request.schema_version).toBe(1);expect(request.revision).toBe(stored.revision);
        if(fail)result={ok:false,schema_version:1,reason:'settings-conflict'};
        else {stored={...stored,revision:'revision-'+(calls.length+1),values:{...stored.values,...request.changes}};result={...stored,saved:true};}
      }else {reads++;result=stored;}
    } else if(command==='scan') {calls.push(e.argv);stored={...stored,index_present:true};result={ok:true};}
    else {
      expect(command).toBe('panel');
      result={ok:true,schema_version:1,project:{name:'fixture',language:english?'en-US':'pt-BR'},specs:[],jev:{}};
    }
    return {value:{exitCode:0,stdout:JSON.stringify(result),stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface,isInteractive:true});
  await $.command.run({command:'mustard-panel',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface,component:'Pane',requestId:'mustard-panel',
    viewport:{columns:140,rows:48,docked:true},props:{id:'mustard-panel',title:'Mustard',bodyColumns:120,scroll:{bodyRows:40}}});
  return {pane,clock,calls,get reads(){return reads;},get turns(){return turns;},setFail(value){fail=value;}};
}

test('settings edits stay local through live polls and save only an explicit versioned patch',async ($,on)=>{
  const f=await fixture($,on);const {pane,clock,calls}=f;
  expect(f.reads).toBe(0);
  await pane.press({key:'tab-Configurações'});
  expect(f.reads).toBe(1);
  expect(await pane.find({key:'select-spec'})).toBeUndefined();
  expect(await pane.find({key:'settings-save'})).toBeUndefined();
  await pane.press({key:'setting-vectors-on'});await pane.select({key:'settings-group',value:'Jev'});
  expect(await pane.find({type:'Text',text:/Sem chave disponível/})).toBeDefined();
  await pane.press({key:'setting-jev-on'});await pane.press({key:'setting-jev_search-on'});
  await pane.select({key:'settings-group',value:'Geral'});await pane.press({key:'setting-rtk-on'});
  await pane.select({key:'settings-group',value:'Agentes'});
  await pane.select({key:'setting-agent-model',value:'opus'});
  await clock.advance(6000);expect(f.reads).toBe(1);
  expect(await pane.find({key:'settings-scan'})).toBeUndefined();
  await pane.press({key:'settings-save'});
  const save=calls.find(args=>args.includes('--json'));
  expect(JSON.parse(save[save.indexOf('--json')+1])).toEqual({schema_version:1,revision:'revision-1',changes:{vectors:true,jev:true,jev_search:true,rtk:true,agent_model:'opus'}});
  expect(await pane.find({type:'Text',text:/Configurações salvas/})).toBeDefined();
  expect(await pane.find({key:'settings-save'})).toBeUndefined();expect(f.turns).toBe(0);
  await pane.press({key:'close'});
});

test('a conflict retains choices and explicit reload discards them',async ($,on)=>{
  const f=await fixture($,on);const {pane}=f;
  await pane.press({key:'tab-Configurações'});await pane.press({key:'setting-vectors-on'});
  f.setFail(true);await pane.press({key:'settings-save'});
  expect(await pane.find({type:'Text',text:/configuração mudou fora do painel/})).toBeDefined();
  expect(await pane.find({key:'settings-save'})).toBeDefined();
  await pane.press({key:'settings-reload'});
  expect(await pane.find({key:'settings-save'})).toBeUndefined();expect(f.reads).toBe(2);
  expect(f.turns).toBe(0);await pane.press({key:'close'});
});

test('the index update is explicit and uses the native scan without Jev or a model turn',async ($,on)=>{
  const f=await fixture($,on);const {pane,calls}=f;
  await pane.press({key:'tab-Configurações'});await pane.press({key:'settings-scan'});
  expect(calls.filter(args=>args[2]==='scan').length).toBe(1);
  expect(calls.find(args=>args[2]==='scan').slice(1)).toEqual(['run','scan','--root','/fixture']);
  expect(await pane.find({type:'Text',text:'Índice atualizado.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/Índice disponível/})).toBeDefined();
  expect(f.turns).toBe(0);await pane.press({key:'close'});
});

test('mobile settings use dark native controls and preserve master and purpose choices independently',async ($,on)=>{
  const f=await fixture($,on,'mobile',true);const {pane,calls}=f;
  await pane.press({key:'tab-Configurações'});
  expect(await pane.find({type:'Text',text:/Project settings/})).toBeDefined();
  await pane.press({key:'settings-group-Jev'});
  await pane.press({key:'setting-jev_context-on'});
  await pane.press({key:'settings-group-Agentes'});
  await pane.press({key:'setting-agent-model-opus'});
  await pane.press({key:'settings-save'});
  const save=calls.find(args=>args.includes('--json'));
  expect(JSON.parse(save[save.indexOf('--json')+1]).changes).toEqual({jev_context:true,agent_model:'opus'});
  await pane.press({key:'settings-group-Jev'});
  expect(await pane.find({type:'Text',text:/master switch is off/})).toBeDefined();
  expect(f.turns).toBe(0);await pane.press({key:'close'});
});

test('undoing an edit avoids an unnecessary write and reopening reads current settings',async ($,on)=>{
  const f=await fixture($,on);const {pane,calls}=f;
  await pane.press({key:'tab-Configurações'});await pane.press({key:'setting-vectors-on'});await pane.press({key:'setting-vectors-off'});
  expect(await pane.find({key:'settings-save'})).toBeUndefined();
  await pane.press({key:'close'});
  await $.command.run({command:'mustard-panel',args:''});
  expect(f.reads).toBe(2);expect(calls.some(args=>args.includes('--json'))).toBe(false);
  await pane.press({key:'close'});
});
