import { test, expect, mock } from 'claude-code/testing';
import {contentPage,waveText} from './panel-content.js';

test('a lost pane reference does not prevent a fresh Desktop opening',async ($,on)=>{
  mock.clock(on);mock.env(on,{});let opens=0,closes=0;
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{}}));
  on('ui.close',()=>{if(++closes===1)throw new Error('pane no longer exists');return {value:null};});
  on('ui.open',()=>{opens++;return {value:null};});
  on('process.run',()=>({value:{exitCode:0,stdout:JSON.stringify({ok:true,schema_version:1,project:{name:'fixture'},specs:[],jev:{}}),stderr:''}}));
  await $.session.start({cwd:'/fixture',surface:'desktop',isInteractive:true});
  expect((await $.command.run({command:'mustard-panel',args:''})).text).toContain('solicitada');
  expect(opens).toBe(1);
  const pane=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'close'});
});

test('wave specialization distinguishes delivery from independent reading evidence',()=>{
  const content=waveText({expertise:{source:'jev-cache',reason:'fit',preferred_agent:'mustard-database',profiles:['database'],pending_review:[],uncovered_files:[]},
    skills:[{name:'database',path:'.claude/skills/database/SKILL.md',read_confirmed:null}]},'Perfil',false);
  expect(content).toContain('mustard-database');expect(content).toContain('database');
  expect(content).toContain('sem confirmação independente');expect(content).toContain('Critério da escolha');
});

test('reopening recreates the host pane and history recovery waits for Usage',async ($,on)=>{
  const clock=mock.clock(on);mock.env(on,{});
  const lifecycle:string[]=[], measured:boolean[]=[];
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{}}));
  on('ui.close',()=>{lifecycle.push('close');return {value:null};});
  on('ui.open',()=>{lifecycle.push('open');return {value:null};});
  on('process.run',($,e)=>{measured.push(e.argv.includes('--refresh-consumption'));
    return {value:{exitCode:0,stdout:JSON.stringify({ok:true,schema_version:1,project:{name:'fixture'},specs:[],jev:{}}),stderr:''}};});
  await $.session.start({cwd:'/fixture',surface:'desktop',isInteractive:true});
  const reply=await $.command.run({command:'mustard-panel',args:''});
  expect(reply.text).toContain('--diagnose');expect(lifecycle).toEqual(['close','open']);
  expect(measured).toEqual([false]);
  await $.command.run({command:'mustard-panel',args:''});
  expect(lifecycle).toEqual(['close','open','close','open']);
  await clock.advance(2000);expect(measured).toEqual([false,false,false]);
  const pane=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'tab-Consumo'});await clock.advance(2000);
  expect(measured).toEqual([false,false,false,true]);
  await clock.advance(2000);expect(measured.at(-1)).toBe(false);
  await pane.press({key:'close'});
  const count=measured.length;await clock.advance(10000);expect(measured.length).toBe(count);
});

test('an opening failure leaves no poller and a later command can recover',async ($,on)=>{
  const clock=mock.clock(on);mock.env(on,{});let attempts=0,reads=0;
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{}}));
  on('ui.close',()=>({value:null}));
  on('ui.open',()=>{if(++attempts===1)throw new Error('fixture opening failed');return {value:null};});
  on('process.run',()=>{reads++;return {value:{exitCode:0,
    stdout:JSON.stringify({ok:true,schema_version:1,project:{name:'fixture'},specs:[],jev:{}}),stderr:''}};});
  await $.session.start({cwd:'/fixture',surface:'desktop',isInteractive:true});
  const failed=await $.command.run({command:'mustard-panel',args:''});
  expect(failed.text).toContain('não concluiu');
  await clock.advance(10000);expect(reads).toBe(0);
  const recovered=await $.command.run({command:'mustard-panel',args:''});
  expect(recovered.text).toContain('solicitada');expect(reads).toBe(1);
  const pane=await $.ui.mount({plugin:'mustard',surface:'desktop',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  await pane.press({key:'close'});
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
  const reply=await $.command.run({command:'mustard-panel',args:'--diagnose'});
  expect(reply.text).toContain('2.1.296');expect(reply.text).toContain('terminal, desktop');
  expect(reply.text).toContain('não comprova');expect(reply.context).toBeUndefined();
  surfaces=['terminal'];
  expect((await $.command.run({command:'mustard-panel',args:'--diagnose'})).text).toContain('não informou uma interface Desktop');
  expect(operations).toBe(0);
});

async function clarityFixture($,on,surface:'terminal'|'desktop'|'mobile'='terminal',columns=100,bodyRows=48) {
  const clock=mock.clock(on);mock.env(on,{});
  const counts={queries:0,publications:0,turns:0};
  const snapshot:any={ok:true,schema_version:1,at:'2026-10-10T23:04:12.166754188Z',
    project:{name:'portal-florestal-backend',branch:'feature/pi-cardapio-original',version:'0.2.10',language:'pt-BR',publication_url:'https://example.com/project'},
    selected_spec:'pi-cardapio-original',statusline:{model:'Sonnet 5.5',duration_ms:191000,rtk:null,measured_at:'2026-10-10T23:04:12.166754188Z'},
    consumption:{available:true,project_tokens:2178526628,machine_tokens:26067747296,measured_at:'2026-10-10T23:04:12.166754188Z'},
    jev:{physical_requests:0,cost_micro_usd:null},specs:[
      {name:'pi-cardapio-original',phase:'running',goal:'Baixar o cardápio original.',counted_waves:[0,1],waves:[{wave:1,status:'running'}],usage:{},
        definitions:{context:[{code:'MSTD-CTX-0001',title:'Objetivo',text:'Gerar quatro arquivos Excel.'}],
          rule:[{code:'MSTD-RULE-0001',title:'Colunas originais',text:'Preservar a ordem das colunas.'},{code:'MSTD-RULE-0002',title:'Dados atuais',text:'Usar os dados atuais do banco.'}]},stages:[]},
      {name:'outra-spec',phase:'pr_open',counted_waves:[40,40],waves:[],usage:{},definitions:{context:[{code:'MSTD-CTX-0002',title:'Outro objetivo',text:'Definição da outra spec.'}]},stages:[]},
    ]};
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));on('session.start',($,e)=>({cwd:e.cwd}));
  on('command.register',()=>({value:null}));on('session.cwd',()=>({value:'/fixture'}));
  on('session.usage',()=>({value:{context:{percent:7,tokens:14000,window:200000}}}));
  on('ui.open',($,e)=>{expect(e.rows).toBe(36);expect(e.columns).toBe(100);return {value:null};});
  on('ui.close',()=>({value:null}));on('prompt.submit',()=>{counts.turns++;return {};});
  on('process.run',($,e)=>{
    if(e.argv[2]==='publish'){counts.publications++;return {value:{exitCode:1,stdout:'{}',stderr:''}};}
    counts.queries++;
    const result={...snapshot};
    if(e.argv.includes('--wave'))result.wave_detail={spec:e.argv[e.argv.indexOf('--spec')+1],wave:1,
      plan:{text:'Criar os caminhos dos arquivos.',done_when:'Os quatro caminhos estão disponíveis.'},request:'# Input integral\n\nPedido técnico exato.',
      tasks:[{code:'MSTD-TASK-0001',title:'Gerar caminhos'}],criteria:[],steps:[]};
    return {value:{exitCode:0,stdout:JSON.stringify(result),stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface,isInteractive:true});
  await $.command.run({command:'mustard-panel',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface,component:'Pane',requestId:'mustard-panel',
    viewport:{columns:180,rows:48,docked:true},props:{id:'mustard-panel',title:'Mustard',bodyColumns:columns,scroll:{bodyRows}}});
  return {pane,clock,counts,snapshot};
}

test('analysis receipts and profile reuse stay separate from net savings and Jev billing',async ($,on)=>{
  const {pane,clock,counts,snapshot}=await clarityFixture($,on);
  snapshot.knowledge={analysis:{completed_analyses:2,reused_operations:5,failures:1,reported_input_tokens:9000,reported_output_tokens:300,analyses_with_unknown_input:1,over_target:0},catalog:{available:true,active:3,inactive:1,native_navigation:2}};
  await clock.advance(2000);await pane.press({key:'tab-Consumo'});
  await pane.select({key:'usage-section',value:'Análises'});
  expect(await pane.find({type:'Text',text:'Consultas que reutilizaram análise válida: 5'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Perfis válidos: 3 · inativos ou aguardando revisão: 1'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/não somar novamente ao consumo/})).toBeDefined();
  expect(counts.turns).toBe(0);expect(counts.publications).toBe(0);await pane.press({key:'close'});
});

test('usage shows incomplete wave subtotals and explains disabled Jev without making calls',async ($,on)=>{
  const {pane,clock,counts,snapshot}=await clarityFixture($,on);
  snapshot.specs[0].usage={known_wave_tokens:1234,waves_with_unknown_usage:3,measured_at:'2026-10-10T23:04:12Z'};
  snapshot.jev.configuration={purposes:{search:{reason:'ai-fallback-disabled'},'wave-planning':{reason:'purpose-not-configured'}}};
  await clock.advance(2000);await pane.press({key:'tab-Consumo'});
  await pane.select({key:'usage-section',value:'Ondas'});
  expect(await pane.find({type:'Text',text:'Subtotal das ondas medidas: 1.234 tokens'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'3 ondas ainda sem medição; subtotal incompleto.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Medição por onda'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/Totais acumulados.*cache relido/})).toBeDefined();
  await pane.select({key:'usage-section',value:'Jev'});
  expect(await pane.find({type:'Text',text:'Busca: Desligado: ai.fallback não está habilitado.'})).toBeDefined();
  snapshot.jev.configuration.purposes.search={reason:'configured',enabled:true};
  await clock.advance(2000);
  expect(await pane.find({type:'Text',text:'Busca: Habilitado sob critério; não é acionado em toda busca.'})).toBeDefined();
  expect(counts.turns).toBe(0);expect(counts.publications).toBe(0);await pane.press({key:'close'});
});

test('usage separates measured RTK output and Jev cost from unproven total savings',async ($,on)=>{
  const {pane,clock,counts,snapshot}=await clarityFixture($,on,'desktop',72,20);
  snapshot.rtk={status:'measured',configured:true,hook_configured:true,scope:'project-directory',
    commands:8635,input_tokens:7456308,output_tokens:4576286,saved_tokens:2880729,percent:38.6,
    measured_at:'2026-10-11T00:01:06Z'};
  snapshot.jev={physical_requests:1,cost_micro_usd:1044,
    configuration:{purposes:{search:{reason:'configured',enabled:true}}},
    last_attempt:{purpose:'search',status:200,at:'2026-10-11T00:01:06Z'}};
  snapshot.specs[0].operations={search:{calls:33,decision_coverage:2,choice_gates:{eligible:1,'exact-read':1},
    local_model_calls:1,cache_hits:1,accepted_choices:1}};
  await clock.advance(2000);await pane.select({key:'panel-section',value:'Consumo'});
  expect(await pane.find({type:'Text',text:'Economia total do modelo: ainda não comprovada.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'RTK: 38,6% de redução estimada na saída deste projeto'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Jev: 1 chamadas no projeto · $0.0010 estimados'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Histórico observado'})).toBeUndefined();
  await pane.select({key:'usage-section',value:'RTK'});
  expect(await pane.find({type:'Text',text:'Comandos registrados: 8.635'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Hook configurado para comandos compatíveis.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/não é redução da fatura/})).toBeDefined();
  await pane.select({key:'usage-section',value:'Jev'});
  expect(await pane.find({type:'Text',text:/Última chamada: search · sucesso/})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Buscas com diagnóstico nesta spec: 2/33'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Leitura exata: 1'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Cache reaproveitado: 1 · escolhas aceitas: 1'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/abrir este painel não faz chamada paga/})).toBeDefined();
  expect(counts.turns).toBe(0);expect(counts.publications).toBe(0);await pane.press({key:'close'});
});

test('project scope, unknown measurements and explicit publication remain clear',async ($,on)=>{
  const {pane,counts}=await clarityFixture($,on);
  expect(await pane.find({type:'Text',text:'0/1 ondas concluídas'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Ondas de todas as specs'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'40/41 concluídas'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'outra-spec · PR aberto'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/RTK|desconhecido|T23:04|https:\/\/example/})).toBeUndefined();
  expect(await pane.find({key:'publish'})).toBeUndefined();
  await pane.press({key:'tab-Consumo'});
  expect(await pane.find({type:'Text',text:'Economia total do modelo: ainda não comprovada.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'RTK: medição deste projeto indisponível.'})).toBeDefined();
  await pane.select({key:'usage-section',value:'Histórico'});
  expect(await pane.find({type:'Text',text:'Histórico observado'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Acumulado do histórico disponível; inclui hoje parcialmente.'})).toBeDefined();
  await pane.select({key:'usage-section',value:'Jev'});
  expect(await pane.find({type:'Text',text:'Nenhuma chamada registrada neste projeto.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/desconhecido%|T23:04/})).toBeUndefined();
  expect(counts.publications).toBe(0);expect(counts.turns).toBe(0);
  await pane.press({key:'close'});
});

test('native category picker and search filter definitions and reset on spec change',async ($,on)=>{
  const {pane,clock,counts}=await clarityFixture($,on);
  await pane.press({key:'tab-Definições'});
  await pane.select({key:'definition-group',value:'rule'});
  await pane.input({key:'definition-search',text:'MSTD-RULE-0002',kind:'change'});
  expect(await pane.find({type:'Text',text:'Usar os dados atuais do banco.'})).toBeDefined();
  expect(await pane.find({key:'item-MSTD-RULE-0001'})).toBeUndefined();
  await pane.input({key:'definition-search',text:'inexistente',kind:'change'});
  expect(await pane.find({type:'Text',text:'Nenhum item corresponde à busca.'})).toBeDefined();
  await pane.select({key:'select-spec',value:'outra-spec'});await clock.advance(2000);
  expect(await pane.find({type:'Text',text:'Definição da outra spec.'})).toBeDefined();
  expect((await pane.find({key:'definition-search'})).props.value).toBe('');
  expect(counts.publications).toBe(0);expect(counts.turns).toBe(0);await pane.press({key:'close'});
});

test('wave plan stays readable and the exact input is a separate explicit view',async ($,on)=>{
  const {pane,clock,counts}=await clarityFixture($,on);
  await pane.press({key:'tab-Ondas'});await pane.press({key:'wave-1'});await clock.advance(2000);
  expect(await pane.find({type:'Text',text:'Criar os caminhos dos arquivos.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/Gerar caminhos/})).toBeDefined();
  expect(await pane.find({type:'Text',text:/Pedido técnico exato/})).toBeUndefined();
  await pane.press({key:'wave-view-Pedido'});
  expect(await pane.find({type:'Text',text:'# Input integral\n\nPedido técnico exato.'})).toBeDefined();
  expect(counts.turns).toBe(0);await pane.press({key:'close'});
});

test('desktop uses shared dark tokens and narrow panes stack using their own width',async ($,on)=>{
  const {pane}=await clarityFixture($,on,'desktop',64);
  expect((await pane.drawn()).props.backgroundColor).toBe('#292735');
  await pane.select({key:'panel-section',value:'Definições'});
  const boxes=await pane.findAll({type:'Box'});
  expect(boxes.some(box=>box.props.width==='38%')).toBe(false);
  expect(await pane.find({type:'Select',key:'definition-group'})).toBeDefined();
  await pane.press({key:'close'});
});

test('mobile retains spec and category navigation without unsupported inputs',async ($,on)=>{
  const {pane}=await clarityFixture($,on,'mobile',48);
  await pane.press({key:'tab-Definições'});
  expect(await pane.find({type:'Input'})).toBeUndefined();expect(await pane.find({type:'Select'})).toBeUndefined();
  await pane.press({key:'definition-group-rule'});
  expect(await pane.find({type:'Text',text:'Preservar a ordem das colunas.'})).toBeDefined();
  await pane.press({key:'close'});
});

test('short inline panes compact controls instead of filling the window with the header',async ($,on)=>{
  const {pane}=await clarityFixture($,on,'terminal',80,14);
  expect((await pane.drawn()).props.rowGap).toBe(0);
  expect((await pane.find({key:'panel-section'})).props.value).toBe('Projeto');
  await pane.select({key:'panel-section',value:'Ondas'});
  expect(await pane.find({type:'Text',text:'Criar os caminhos dos arquivos.'})).toBeDefined();
  expect((await pane.findAll({type:'Box'})).some(box=>box.props.borderStyle==='round' && box.props.key==='control-wave-view-Plano')).toBe(false);
  await pane.press({key:'close'});
});

test('dark panel preserves definitions and exact wave inputs even with a light host theme',async ($,on)=>{
  const clock=mock.clock(on);mock.env(on,{});
  on('tool.register',()=>({value:{tool:'mcp__mustard__search'}}));
  on('session.start',($,e)=>({cwd:e.cwd}));on('command.register',()=>({value:null}));
  on('session.cwd',()=>({value:'/fixture'}));on('session.usage',()=>({value:{}}));
  on('config.list',()=>({value:[{key:'theme',value:'light',kind:'choice',label:'Theme'}]}));
  on('ui.open',()=>({value:null}));on('ui.close',()=>({value:null}));
  let turns=0,details=0;
  on('prompt.submit',()=>{turns++;return {};});
  const input='Começo do pedido.\n'+ 'A'.repeat(8000)+'\nFim exato do pedido.';
  const snapshot:any={ok:true,schema_version:1,at:'2026-10-10T12:00:00Z',project:{name:'fixture',language:'pt-BR'},
    selected_spec:'demo',jev:{},specs:[{name:'demo',phase:'running',goal:'Baixar o arquivo original.',
      waves:[{wave:1,status:'running'}],stages:[],undeclared_commands:[],usage:{},
      definitions:{rule:[{code:'MSTD-RULE-0001',title:'Formato original',text:'O arquivo mantém o formato original.',
        example:'As colunas permanecem na ordem.',agent:'Parte técnica.\n'+'B'.repeat(8000)+'\nFim técnico.'}],
        task:[{code:'MSTD-TASK-0001',title:'Gerar arquivo',text:'Entregar a cópia.'}]}}]};
  on('process.run',($,e)=>{
    const result={...snapshot};
    if(e.argv.includes('--wave')) {
      details++;
      expect(e.argv[e.argv.indexOf('--wave')+1]).toBe('1');
      result.wave_detail={spec:'demo',wave:1,plan:{text:'Onda para gerar o arquivo.',done_when:'Arquivo conferido.'},
        request:input,delivery:{text:'Retorno real registrado.'},delivery_integrated:false,
        steps:[{item:'MSTD-TASK-0001',text:'Teste mínimo aprovado.'}]};
    }
    return {value:{exitCode:0,stdout:JSON.stringify(result),stderr:''}};
  });
  await $.session.start({cwd:'/fixture',surface:'terminal',isInteractive:true});
  await $.command.run({command:'mustard-panel',args:''});
  const pane=await $.ui.mount({plugin:'mustard',surface:'terminal',component:'Pane',props:{id:'mustard-panel',title:'Mustard'},requestId:'mustard-panel'});
  expect((await pane.drawn()).props.backgroundColor).toBe('black');
  await pane.press({key:'tab-Definições'});
  expect((await pane.find({type:'Text',text:/O arquivo mantém o formato original/})).props.color).toBe('whiteBright');
  await pane.press({key:'technical-MSTD-RULE-0001'});
  await pane.press({key:'definition-technical-next'});
  await pane.press({key:'definition-technical-next'});
  expect(await pane.find({type:'Text',text:/Fim técnico/})).toBeDefined();
  await pane.press({key:'tab-Ondas'});
  expect(details).toBe(1);
  await pane.press({key:'wave-1'});await clock.advance(2000);
  expect(await pane.find({type:'Text',text:/Onda para gerar o arquivo/})).toBeDefined();
  await pane.press({key:'wave-view-Pedido'});
  const parts=[];
  parts.push((await pane.find({type:'Text',text:contentPage(input,0).text})).text);
  await pane.press({key:'content-next'});parts.push((await pane.find({type:'Text',text:contentPage(input,1).text})).text);
  await pane.press({key:'content-next'});parts.push((await pane.find({type:'Text',text:contentPage(input,2).text})).text);
  expect(parts.join('')).toBe(input);
  await pane.press({key:'wave-view-Entrega'});
  expect(await pane.find({type:'Text',text:'Retorno real registrado.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/aguardando validação/})).toBeDefined();
  await pane.press({key:'wave-view-Testes'});
  expect(await pane.find({type:'Text',text:/Teste mínimo aprovado/})).toBeDefined();
  expect(await pane.find({type:'Text',text:/não comprovam execução independente/})).toBeDefined();
  expect(await pane.find({type:'Text',text:/Nenhuma prova de critério executada pelo Mustard/})).toBeDefined();
  expect(turns).toBe(0);await pane.press({key:'close'});
});

test('content pagination preserves all Unicode text within the official surface size limit',()=>{
  const original='🟡'.repeat(10001)+'\nFinal';
  const first=contentPage(original,0),parts=[];
  for(let n=0;n<first.pages;n++) {
    const page=contentPage(original,n);expect(page.text.length).toBeLessThanOrEqual(10000);parts.push(page.text);
  }
  expect(parts.join('')).toBe(original);
});

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
      scheduling:{backlog:2,ready:1,capacity:4,occupied_wave_slots:1,free_wave_slots:3,omitted_tasks:0,
        tasks:[{task:"T-2",title:"Consumer",reason:"file-reservation",dependencies:[],holding_waves:[1]},{task:"T-3",title:"Docs",reason:"ready",dependencies:[],holding_waves:[]}]},
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
  await pane.press({key:'tab-Execução'});
  expect(await pane.find({type:'Text',text:'2 tarefas no backlog · 1 prontas'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'T-2 · Consumer'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'Arquivo reservado: 1'})).toBeDefined();
  await pane.press({key:'tab-Consumo'});
  await pane.select({key:'usage-section',value:'Ondas'});
  expect(await pane.find({type:'Text',text:'Consumo das ondas ainda não medido.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:/desconhecido%/})).toBeUndefined();
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
  await pane.select({key:'usage-section',value:'Sessão'});
  expect(await pane.find({type:'Text',text:'Contexto da sessão: 12% · 24.000/200.000 tokens'})).toBeDefined();
  await pane.select({key:'usage-section',value:'Histórico'});
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
  expect(await pane.find({type:'Text',text:'Final validation: Not checked'})).toBeDefined();
  await clock.advance(2000);
  expect(await pane.find({type:'Text',text:/Last state:/})).toBeDefined();
  expect(await pane.find({type:'Text',text:/T12:00:00Z/})).toBeUndefined();
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
  expect(await pane.find({key:'publish-project'})).toBeUndefined();
  await pane.press({key:'publications'});
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


test('installation details do not present an old receipt as verification of this version',async ($,on)=>{
  const {pane,clock,snapshot,counts}=await clarityFixture($,on);
  (snapshot as any).installation={version:'0.0.0',complete:true,checks:[{component:'scan',ready:false,remedy:'run mustard-rt run scan'}]};
  await clock.advance(2000);await pane.press({key:'tab-Consumo'});
  await pane.select({key:'usage-section',value:'Detalhes'});
  expect(await pane.find({type:'Text',text:'Instalação incompleta: execute /mustard:upsert e confira as pendências.'})).toBeDefined();
  expect(await pane.find({type:'Text',text:'scan: run mustard-rt run scan'})).toBeDefined();
  (snapshot as any).installation={version:snapshot.project.version,complete:true,checks:[]};
  await clock.advance(2000);
  expect(await pane.find({type:'Text',text:'Instalação conferida para esta versão.'})).toBeDefined();
  expect(counts.turns).toBe(0);expect(counts.publications).toBe(0);
  await pane.press({key:'close'});
});
