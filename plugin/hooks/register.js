import searchSchema from './search-schema.js';
import {completeDelivery} from './delivery-check.js';
// Local observation only. Runtime events remain the source of truth; the
// poller coalesces overlapping reads and never calls a model or search tool.
let timer, eventTimer, busy = false, opened = false, data, problem, selected, usage, revision = 0;
let tab = 'Projeto';
let publishing = false, publicationMessage;
let runtimeBinary;
let consumptionMeasuredAt;
let measurementQueue = Promise.resolve();
const searchPurposes = ['locate','understand','spec','implement','validate'];
const searchTools = ['rg','grep','git','Grep','Glob','Read','Symbol','Trace','Structure','References'];
let evidenceEpoch, evidenceAcknowledged = new Map();
function resetEvidence() {
  evidenceEpoch=Date.now().toString(36)+'-'+Math.random().toString(36).slice(2);
  evidenceAcknowledged.clear();
}
resetEvidence();
function searchContractError(request) {
  if (!request || typeof request !== 'object' || Array.isArray(request)) return 'request object required';
  for (const field of ['tool','input','intent','purpose']) {
    if (!Object.hasOwn(request,field)) return `missing ${field}`;
  }
  if (Object.keys(request).some(key=>!['tool','input','intent','purpose','choose'].includes(key))) return 'unknown request field';
  if (!searchTools.includes(request.tool)) return 'unsupported tool';
  if (!request.input || typeof request.input !== 'object' || Array.isArray(request.input)) return 'input object required';
  if (!searchPurposes.includes(request.purpose)) return 'invalid purpose';
  if (typeof request.intent !== 'string') return 'intent string required';
  if (Object.hasOwn(request,'choose') && typeof request.choose !== 'boolean') return 'choose boolean required';
  if ((request.purpose !== 'locate' || request.choose) && !request.intent.trim()) return 'provide the specific question in intent';
  const branch=searchSchema.properties.request.oneOf.find(branch=>branch.properties.tool.const===request.tool);
  return inputContractError(branch.properties.input,request.input,'input');
}
function inputContractError(schema,value,path) {
  const type=schema.type;
  const valid=type==='object' ? value!==null && typeof value==='object' && !Array.isArray(value)
    : type==='array' ? Array.isArray(value) : type==='integer' ? Number.isSafeInteger(value) && value>=0 : typeof value===type;
  if (!valid || schema.enum && !schema.enum.includes(value) || schema.minimum!==undefined && value<schema.minimum) return `invalid ${path}`;
  if (type==='array') {
    if (schema.minItems!==undefined && value.length<schema.minItems) return `invalid ${path}`;
    for (const item of value) { const error=inputContractError(schema.items,item,path);if(error)return error; }
  }
  if (type==='object') {
    for (const field of schema.required||[]) if (!Object.hasOwn(value,field)) return `missing ${path}.${field}`;
    for (const [field,item] of Object.entries(value)) {
      if (!Object.hasOwn(schema.properties,field)) return `unknown ${path}.${field}`;
      const error=inputContractError(schema.properties[field],item,`${path}.${field}`);if(error)return error;
    }
  }
}
async function runtime($) {
  if (!runtimeBinary) runtimeBinary = `${$.plugin.root}/bin/mustard-rt${await $.env.get('OS') === 'Windows_NT' ? '.exe' : ''}`;
  return runtimeBinary;
}
const id = 'mustard-panel';
function t(pt,en) { return data?.project?.language === 'en-US' ? en : pt; }
async function refresh($) {
    if (busy || !opened) return;
    const ticket = revision;
    busy = true;
    try {
      const cwd = await $.session.cwd();
      let session;
      try { session = await $.session.id(); } catch { /* Session observation may be unavailable. */ }
      const measure = consumptionMeasuredAt === undefined || Date.now() - consumptionMeasuredAt >= 30000;
      const result = await $.process.run(
        [await runtime($), 'run', 'panel', '--root', cwd, ...(selected ? ['--spec', selected] : []), ...(session ? ['--session', session] : []), ...(measure ? ['--refresh-consumption'] : [])],
        { cwd, timeoutMs: measure ? 30000 : 10000 });
      if (result.exitCode !== 0) throw new Error(result.stderr || 'Consulta local falhou');
      const next = JSON.parse(result.stdout);
      if (next.ok !== true || next.schema_version !== 1) throw new Error('Versão do painel incompatível');
      if (!opened || ticket !== revision) return;
      data = next;
      if (measure) consumptionMeasuredAt = Date.now();
      if (!selected || !data.specs.some(s => s.name === selected)) selected = data.selected_spec || data.specs[0]?.name;
      usage = await $.session.usage();
      problem = undefined;
    } catch (error) {
      if (opened && ticket === revision) problem = String(error.message || error);
    } finally {
      busy = false;
      if (opened && ticket === revision) $.ui.invalidate('ui.render');
    }
}
function recordMeasurement($, measurement) {
  // Preserve arrival order. A repeated cumulative reading replaces no spend,
  // and the runtime resolves attribution from the checkout, not the pane tab.
  measurementQueue = measurementQueue.catch(() => {}).then(async () => {
    if (measurement.cost?.usd == null) return;
    try {
      const cwd = await $.session.cwd();
      const session = await $.session.id();
      const model = await $.session.model();
      await $.process.run([await runtime($), 'run', 'usage-record', '--root', cwd, '--session', session],
        {cwd, timeoutMs:5000, stdin:JSON.stringify({cost:measurement.cost,model})});
    } catch { /* Missing host data stays unknown; never interfere with work. */ }
  });
  return measurementQueue;
}
async function openPanel($, name) {
    revision++;
    if (name) selected = name;
    opened = true;
    await $.ui.open({ id, title: 'Mustard', focus: true, closeOnEscape: true });
    timer?.cancel();
    timer = $.clock.every(2000, () => { void refresh($); });
    await refresh($);
}
function stopPanel() {
    revision++;
    opened = false;
    timer?.cancel();
    eventTimer?.cancel();
    timer = eventTimer = undefined;
}
function requestRefresh($) {
    if (!opened) return;
    eventTimer?.cancel();
    eventTimer=$.clock.after(200,()=>{eventTimer=undefined;void refresh($);});
}
async function publishPage($,rawArgs) {
    const raw=rawArgs.trim();
    const args=raw.split(/\s+/);
    const scope=args[0];
    const report=scope==='report' ? raw.slice(scope.length).trim().replace(/^(["'])(.*)\1$/,'$2') : undefined;
    const explicit=args[1];
    if (!['project','spec','report'].includes(scope) || scope==='report' && !report || scope!=='report' && (args.length>2 || scope==='project' && explicit)) {
      return {text:'Use /mustard-pages project, /mustard-pages spec [nome] ou /mustard-pages report arquivo.md.'};
    }
    let name = scope === 'project' ? 'project' : scope === 'report' ? 'report' : explicit || data?.selected_spec || 'current';
    if (publishing) return { text: t('Uma publicação já está em andamento.','A publication is already in progress.') };
    publishing = true;
    const finish = message => {
      publicationMessage = {name, text:message};
      if (opened) $.ui.invalidate('ui.render');
      return {text:message};
    };
    publicationMessage = {name,text:t('Preparando publicação…','Preparing publication…')};
    if (opened) $.ui.invalidate('ui.render');
    try {
      const cwd = await $.session.cwd();
      const result = await $.process.run([await runtime($), 'run', 'publish', '--root', cwd, ...(scope === 'project' ? ['--project'] : scope === 'report' ? ['--document',report] : explicit ? ['--spec',explicit] : [])], { cwd, timeoutMs: 75000 });
      let prepared;
      try { prepared = JSON.parse(result.stdout); } catch { return finish(t('A exportação falhou; nenhum sucesso remoto foi registrado.','Export failed; no remote success was recorded.')); }
      if (scope === 'spec' && prepared.spec) name = prepared.spec;
      if (!prepared.ok) return finish(`${t('Exportação recusada','Export refused')}: ${prepared.reason || 'unknown error'}`);
      if (prepared.published && prepared.remote_url) {
        return finish(`Snapshot ${prepared.snapshot_id} ${t('publicado','published')}: ${prepared.remote_url}${prepared.recorded === false ? t(' · O link não pôde ser registrado na spec; consulte record_reason.',' · The link could not be recorded in the spec; see record_reason.') : ''}`);
      }
      if (prepared.pending) return finish(t(`Publicação ${prepared.deployment_id} aceita e ainda em processamento. Repita /mustard-pages ${scope}${scope === 'spec' ? ' ' + name : scope === 'report' ? ' '+JSON.stringify(report) : ''} para consultar a mesma publicação.`,`Deployment ${prepared.deployment_id} accepted and still processing. Repeat /mustard-pages ${scope}${scope === 'spec' ? ' ' + name : scope === 'report' ? ' '+JSON.stringify(report) : ''} to check the same deployment.`));
      return finish(t(`Snapshot ${prepared.snapshot_id} preparado. Página: ${prepared.page}. Banco: ${prepared.database}. Os arquivos permanecem locais; ainda não foram publicados. ${prepared.hint || prepared.reason || ''}`,`Snapshot ${prepared.snapshot_id} prepared. Page: ${prepared.page}. Database: ${prepared.database}. Files remain local; not published yet. ${prepared.reason || ''}`));
    } catch {
      return finish(t('Não foi possível consultar o resultado. Nenhuma publicação foi confirmada no painel.','Could not read the result. No publication was confirmed in the panel.'));
    } finally {
      publishing = false;
      if (opened) $.ui.invalidate('ui.render');
    }
}
export function register(on) {
  on('session.start', async ($, e, next) => {
    resetEvidence();
    await $.command.register({ name: 'mustard-panel', description: 'Projeto, specs, execução e consumo local', argumentHint: '[spec]', immediate: true });
    await $.command.register({ name: 'mustard-pages', description: 'Publicar projeto ou spec sob pedido explícito', argumentHint: 'project | spec [nome] | report <arquivo.md>', immediate: true });
    await $.tool.register({name:'search',description:'Use Mustard for project code searches and reads. Preserve original arguments and scope. Provide intent (local resource/provider + operation, without repeating the whole project goal) and purpose. locate returns occurrences; understand/spec/implement/validate investigate current source. Once a declaration is found, ask for its scoped evidence instead of opening its whole file. Reuse complete bodies; expand indicated missing ranges before relying on omitted code. Read preserves the requested offset/limit and result. Symbol/Trace use {file_path,symbol,direction?,depth?,target?,limit?} with current cards[].id or owners.symbols[].id (References symbol IDs are provider-specific); Trace follows current static connections, not runtime flow. Structure uses {file_path,query} with a Tree-sitter query. References uses {file_path,line,column,relation?,limit?}; line is one-based and column is a zero-based UTF-8 byte offset; uses a current imported SCIP index or the installed language server from init/doctor. No model is called. Cold server queries can take longer; use them for exact definitions/references, not literal greps. If unavailable, the result includes a native text-search fallback with explicit limitations. Additional candidates/tests have native expansion commands. choose=true permits optional Jev only for unresolved responsibility alternatives; exact identities stay native. The adapter supplies the contract version.',
      inputSchema:searchSchema});
    return next(e);
  });
  on('command.run', { command: 'mustard-panel' }, async ($, e) => {
    await openPanel($, e.args.trim() || undefined);
    return {};
  });
  on('command.run', { command: 'mustard-pages' }, async ($,e)=>publishPage($,e.args));
  on('tool.call',{tool:'mcp__mustard__search'},async ($,e)=>{
    const request=e.request;
    const problem=searchContractError(request);
    if(problem) return {result:{content:[{type:'text',text:`search-contract: ${problem}. Retry with {request:{tool,input,intent,purpose,choose?}}; preserve original search arguments.`}],isError:true}};
    const cwd=await $.session.cwd();
    // MCP arguments are fields of e; the nested request avoids reserved tool.
    // Use Bash through the host tool API so
    // permissions, classic hooks and the shell sandbox still apply. A process
    // launched directly by a mod would bypass that boundary.
    const quote=value=>"'"+String(value).replace(/'/g,"'\\''")+"'";
    const envelope={schema_version:1,request};
    const agent=e.agentId || 'main';
    const epoch=evidenceEpoch;
    try {
      const session=await $.session.id();
      if ([session,agent,epoch].every(value=>typeof value==='string' && /^[a-zA-Z0-9_-]{1,160}$/.test(value))) {
        envelope.context={session,agent,epoch,acknowledged:[...(evidenceAcknowledged.get(agent)||[])]};
      }
    } catch { /* Without a host delivery identity every body is delivered again. */ }
    const command=[quote(await runtime($)),'run','search','--root',quote(cwd),'--request',quote(JSON.stringify(envelope)),'--shell-output'].join(' ');
    const result=await $.tool.call({tool:'Bash',command,description:request.intent || 'Search current project code through Mustard'});
    if(result.deny) return {deny:result.deny};
    let stdout=result.result?.stdout ?? result.text ?? '';
    const receipt=/\n# mustard-delivery:([a-f0-9]{64}):(\d+):([a-f0-9]{16})\n?$/.exec(stdout);
    if (receipt && envelope.context && epoch===evidenceEpoch && result.isError!==true && result.result?.interrupted!==true
        && completeDelivery(stdout.slice(0,receipt.index),receipt)) {
      const acknowledged=evidenceAcknowledged.get(agent)||new Set();
      acknowledged.add(receipt[1]);
      if(acknowledged.size>128) acknowledged.delete(acknowledged.values().next().value);
      evidenceAcknowledged.set(agent,acknowledged);
    }
    if(receipt)stdout=stdout.slice(0,receipt.index);
    const stderr=result.result?.stderr || '';
    return {result:{content:[{type:'text',text:stdout+(stderr?(stdout?'\nstderr:\n':'')+stderr:'')}],isError:result.isError===true}};
  });
  // Observe completed operations without replacing permissions or the
  // native orchestration. Coalesce bursts; retain polling for external edits.
  on('tool.call', async ($, e, next) => {
    const result=await next(e);
    requestRefresh($);
    return result;
  });
  on('turn.complete', async ($, e, next) => {
    const result=await next(e);
    requestRefresh($);
    return result;
  });
  on('session.compact', async ($, e, next) => {
    resetEvidence();
    const result=await next(e);
    requestRefresh($);
    return result;
  });
  on('classic.SubagentStop', async ($, e, next) => {
    const result=await next(e);
    requestRefresh($);
    return result;
  });
  on('session.measure', async ($, e, next) => {
    usage = e;
    await recordMeasurement($, e);
    if (opened) $.ui.invalidate('ui.render');
    requestRefresh($);
    return next(e);
  });
  on('ui.close', { id }, async ($, e, next) => {
    stopPanel();
    return next(e);
  });
  on('session.end', async ($, e, next) => {
    resetEvidence();
    stopPanel();
    data = undefined;
    consumptionMeasuredAt = undefined;
    publicationMessage = undefined;
    return next(e);
  });
  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== id) return next(e);
    const { Box, Text, Button } = await $.ui.resolve(e);
    const accent = '#E1AD01';
    const clean = value => String(value ?? '').replace(/[\u0000-\u0008\u000b-\u001f\u007f]/g, '');
    const text = (value, props = {}) => Text({ children: clean(value), wrap: 'wrap', ...props });
    const button = (key, label, action, props = {}) => Button({ key, label: clean(label), onPress: action, ...props });
    const redraw = () => $.ui.invalidate('ui.render');
    const fmt = n => n === null || n === undefined ? t('desconhecido','unknown') : Number(n).toLocaleString(data?.project?.language || 'pt-BR');
    const money = n => n == null ? t('desconhecido','unknown') : '$' + (n / 1000000).toFixed(4);
    const state = name => {
      const names = {
        survey:['Levantamento','Survey'], plan:['Planejamento','Planning'], planned:['Planejada','Planned'],
        approved:['Aprovada','Approved'], running:['Em execução','Running'], received:['Retorno recebido','Return received'],
        integrated:['Integrada','Integrated'], committed:['Comitada','Committed'], 'repair-required':['Conserto necessário','Repair required'],
        qa:['Validação','Validation'], reviewing:['Revisão','Review'], review:['Revisão','Review'],
        pr:['Pull request','Pull request'], closed:['Fechada','Closed'], delivered:['Entregue','Delivered'],
        discarded:['Descartada','Discarded'], discarded_auto:['Descartada','Discarded'],
      };
      return names[name] ? t(...names[name]) : clean(name || t('desconhecido','unknown'));
    };
    const section = (title, children) => Box({ flexDirection:'column', borderStyle:'round', borderDimColor:true,
      paddingX:1, children:[text(title,{bold:true,color:accent}), ...children] });
    const card = (label, value, detail) => Box({ flexDirection:'column', flexGrow:1, flexShrink:1, minWidth:16,
      borderStyle:'round', borderDimColor:true, paddingX:1, children:[text(label,{dimColor:true}),
      text(value,{bold:true}), ...(detail ? [text(detail,{dimColor:true})] : [])] });
    const progress = (done, total) => {
      if (!total) return t('Nenhuma onda planejada','No planned waves');
      const filled = Math.round(Math.min(1,done/total)*12);
      return `${'━'.repeat(filled)}${'─'.repeat(12-filled)} ${done}/${total}`;
    };
    const spec = data?.specs.find(s => s.name === selected);
    const tabs = ['Projeto','Specs','Execução','Consumo'];
    const rows = [Box({flexDirection:'column',children:[
      text('MUSTARD',{bold:true,color:accent}),
      text(t('Projeto · especificações · execução','Project · specifications · execution'),{dimColor:true}),
    ]}), Box({flexDirection:'row',flexWrap:'wrap',columnGap:2,children:tabs.map((name,i) =>
      button(`tab-${name}`, t(name,({Projeto:'Project',Specs:'Specs',Execução:'Execution',Consumo:'Usage'})[name]),
        () => {tab=name;redraw();},{plain:true,dimColor:tab!==name,hotkey:String(i+1)}))})];
    if (problem) rows.push(section(t('Consulta indisponível','Query unavailable'),[
      text(problem),text(`${t('Último estado','Last state')}: ${data?.at || t('nenhum','none')}.`,{dimColor:true})]));
    if (!data) return Box({flexDirection:'column',rowGap:1,children:[...rows,text(t('Aguardando estado local…','Waiting for local state…'))]});
    rows.push(text(`${data.project.name || 'Mustard'} · ${data.project.branch || t('sem branch','no branch')}`,{bold:true}));
    if (data.project.version) rows.push(text(`Mustard ${data.project.version}`,{dimColor:true}));
    rows.push(text(`${t('Atualização local','Local update')} · ${data.at}`,{dimColor:true}));
    if (tab === 'Projeto') {
      if (data.statusline) rows.push(section(t('Statusline da sessão','Session statusline'),[
        text(`${t('Modelo','Model')}: ${data.statusline.model || fmt(null)} · ${t('duração','duration')}: ${data.statusline.duration_ms == null ? fmt(null) : Math.round(data.statusline.duration_ms/60000)+' min'}`),
        text(`${t('RTK poupou','RTK saved')}: ${fmt(data.statusline.rtk?.percent)}% · ${fmt(data.statusline.rtk?.saved_tokens)} tokens`),
        text(`${t('Última observação','Last observation')}: ${data.statusline.measured_at}`,{dimColor:true}),
      ]));
      const waves = data.specs.flatMap(s => s.waves);
      const done = waves.filter(w => ['integrated','committed'].includes(w.status)).length;
      rows.push(Box({flexDirection:'row',flexWrap:'wrap',children:[
        card('Specs',fmt(data.specs.length)),card(t('Ondas concluídas','Completed waves'),progress(done,waves.length)),
      ]}));
      rows.push(section(t('Especificações do projeto','Project specifications'), data.specs.length
        ? data.specs.map(s => button(`project-${s.name}`,`${s.name} · ${state(s.phase)}`,() => {
          selected=s.name;revision++;tab='Execução';redraw();void refresh($);
        },{plain:true})) : [text(t('Nenhuma spec registrada','No recorded specs'),{dimColor:true})]));
    }
    if (tab === 'Specs' || tab === 'Execução') {
      rows.push(section(t('Selecionar spec','Select a spec'),data.specs.length ? data.specs.map(s =>
        button(`spec-${s.name}`,`${s.name===selected?'●':'○'} ${s.name} · ${state(s.phase)}`,() => {
          selected=s.name;revision++;tab='Execução';redraw();void refresh($);
        },{plain:true})) : [text(t('Nenhuma spec registrada','No recorded specs'),{dimColor:true})]));
      if (spec) {
        rows.push(section(spec.name,[text(spec.goal || spec.name),
          text(`${t('Validação final','Final validation')}: ${spec.final_validation_valid == null ? t('desconhecida','unknown') : spec.final_validation_valid ? t('válida','valid') : t('pendente','pending')}`),
          text(`${t('Revisão','Review')}: ${spec.review_approved ? t('aprovada','approved') : t('pendente','pending')}`)]));
        if (spec.scheduling) {
          const queue=spec.scheduling;
          const reasons={ready:['Pronta para agrupamento','Ready for grouping'],dependencies:['Aguarda dependências','Waiting for dependencies'],
            'file-reservation':['Arquivo reservado','File reserved'],capacity:['Aguarda vaga','Waiting for capacity'],
            'missing-criteria':['Falta vínculo com critério','Missing acceptance criterion'],
            'cleanup-last':['Limpeza aguarda implementação','Cleanup waits for implementation'],
            'spec-not-running':['Spec fora de execução','Spec is not running']};
          rows.push(section(t('Fila de trabalho','Work queue'),[
            text(`${fmt(queue.backlog)} ${t('tarefas no backlog','backlog tasks')} · ${fmt(queue.ready)} ${t('prontas','ready')}`),
            text(`${t('Vagas de ondas','Wave slots')}: ${fmt(queue.occupied_wave_slots)}/${fmt(queue.capacity)}`,{dimColor:true}),
            ...queue.tasks.map(item=>text(`${item.task} · ${item.title || ''} · ${reasons[item.reason] ? t(...reasons[item.reason]) : clean(item.reason)}${item.dependencies?.length ? ': '+item.dependencies.join(', ') : ''}${item.holding_waves?.length ? ': '+item.holding_waves.join(', ') : ''}`)),
            ...(queue.omitted_tasks ? [text(`${fmt(queue.omitted_tasks)} ${t('outras tarefas','other tasks')}`,{dimColor:true})] : []),
            text(t('Relações do código são conferidas ao despachar.','Code relationships are verified at dispatch.'),{dimColor:true}),
          ]));
        }
        const done=spec.waves.filter(w => ['integrated','committed'].includes(w.status)).length;
        rows.push(section(t('Ondas','Waves'),[text(progress(done,spec.waves.length),{color:accent}),
          ...spec.waves.map(w => Box({flexDirection:'column',marginBottom:1,children:[
            text(`${t('Onda','Wave')} ${w.wave} · ${state(w.status)}`,{bold:true}),
            text(`${fmt(w.tokens)} tokens · ${w.model || w.configured_model || t('modelo desconhecido','unknown model')}`,{dimColor:true}),
            ...(w.commit ? [text(w.commit,{dimColor:true})] : [])]}))]));
        if (spec.stages.length) rows.push(section(t('Validação por etapa','Stage validation'),spec.stages.map(s =>
          text(`${s.phase} · ${s.stage}: ${s.result} · ${fmt(s.ms)} ms`))));
        if (spec.undeclared_commands.length) rows.push(text(`${t('Comandos não declarados','Undeclared commands')}: ${spec.undeclared_commands.join(', ')}`,{dimColor:true}));
      }
    }
    if (tab === 'Consumo') {
      rows.push(section(t('Consumo geral','Overall usage'),[
        text(`${t('Projeto','Project')}: ${data.consumption?.available ? fmt(data.consumption.project_tokens) : fmt(null)} tokens · ${t('máquina','machine')}: ${data.consumption?.available ? fmt(data.consumption.machine_tokens) : fmt(null)} tokens`),
        text(`${t('Última medição','Last measurement')}: ${data.consumption?.measured_at || fmt(null)} · ${t('hoje é parcial','today is partial')}`,{dimColor:true}),
        text(`${t('Claude estimado nas sessões observadas','Estimated Claude cost in observed sessions')}: ${money(data.claude_cost?.known_micro_usd)}`),
        text(`${t('Custo sem atribuição segura a spec','Cost without safe spec attribution')}: ${money(data.claude_cost?.unattributed_micro_usd)}`,{dimColor:true}),
        text(t('Estimativa equivalente de API; não é cobrança da assinatura. Histórico sem medição de custo permanece desconhecido.','API-equivalent estimate, not subscription billing. Historical cost without a measurement remains unknown.'),{dimColor:true}),
      ]));
      rows.push(Box({flexDirection:'row',flexWrap:'wrap',children:[
        card(t('Chamadas Jev','Jev requests'),fmt(data.jev.physical_requests)),
        card(t('Custo Jev estimado','Estimated Jev cost'),money(data.jev.cost_micro_usd)),
      ]}));
      rows.push(text(`${fmt(data.jev.known_input_tokens)} ${t('tokens conhecidos','known tokens')} · ${fmt(data.jev.requests_with_unknown_usage)} ${t('chamadas com uso desconhecido','requests with unknown usage')}`,{dimColor:true}));
      if (spec) rows.push(section(spec.name,[
        text(`${t('Ondas','Waves')}: ${fmt(spec.usage.wave_tokens)} tokens · ${t('condutor','conductor')}: ${fmt(spec.usage.conductor_tokens)} tokens`),
        text(`${t('Jev atribuído à spec','Jev attributed to spec')}: ${fmt(spec.jev?.known_physical_requests)} ${t('tentativas físicas','physical attempts')} · ${money(spec.jev?.cost_micro_usd)}`),
        text(`${t('Claude estimado atribuído à spec','Estimated Claude cost attributed to spec')}: ${money(spec.usage.claude_cost?.known_micro_usd)}`),
        ...(spec.usage.known_wave_breakdown ? [text(`${t('Entrada','Input')}: ${fmt(spec.usage.known_wave_breakdown.input_tokens)} · ${t('saída','output')}: ${fmt(spec.usage.known_wave_breakdown.output_tokens)} · ${t('cache criado/lido','cache written/read')}: ${fmt(spec.usage.known_wave_breakdown.cache_creation_input_tokens)}/${fmt(spec.usage.known_wave_breakdown.cache_read_input_tokens)}`),
          text(`${fmt(spec.usage.waves_with_unknown_breakdown)} ${t('ondas sem detalhamento','waves without usage details')} · ${fmt(spec.usage.known_wave_breakdown.responses_with_partial_usage)} ${t('respostas com uso parcial','responses with partial usage')}`,{dimColor:true})] : []),
      ]));
      const session = [];
      if (usage?.context) session.push(text(`${t('Contexto da sessão','Session context')}: ${fmt(usage.context.percent ?? usage.context.percentUsed)}% · ${fmt(usage.context.tokens)}/${fmt(usage.context.window)} tokens`));
      if (usage?.cost?.usd != null) session.push(text(`${t('Custo estimado informado pelo Claude','Estimated cost reported by Claude')}: $${usage.cost.usd}`));
      if (data.claude_cost?.model) session.push(text(`${t('Modelo','Model')}: ${data.claude_cost.model}`));
      for (const limit of usage?.rateLimits || []) session.push(text(`${limit.kind || limit.label || limit.name || t('Janela','Window')}: ${fmt(limit.percentUsed)}% · reset ${limit.resetsAt || t('desconhecido','unknown')}`));
      if (session.length) rows.push(section(t('Sessão atual','Current session'),session));
    }
    if (data.project.publication_url) rows.push(text(`${t('Projeto publicado','Published project')}: ${data.project.publication_url}`));
    if (spec?.publication?.url) rows.push(text(`${t('Última publicação','Last publication')}: ${spec.publication.url}`));
    if (publicationMessage && (publicationMessage.name === selected || ['project','report'].includes(publicationMessage.name))) rows.push(text(publicationMessage.text));
    rows.push(text(t('Atualização local a cada 2 s · publicação somente a pedido','Local update every 2 s · publish on request'),{dimColor:true}));
    rows.push(Box({flexDirection:'row',flexWrap:'wrap',columnGap:1,children:[
      button('refresh',t('Atualizar','Refresh'),() => {void refresh($);},{hotkey:'r'}),
      button('publish',t('Publicar spec','Publish spec'),async () => {
        if (selected) await publishPage($,'spec '+selected);
      },{hotkey:'p',disabled:publishing || !selected}),
      button('publish-project',t('Publicar projeto','Publish project'),async ()=>{await publishPage($,'project');},{disabled:publishing}),
      button('close',t('Fechar','Close'),async () => {stopPanel();await $.ui.close({id});},{hotkey:'q'}),
    ]}));
    return Box({flexDirection:'column',rowGap:1,children:rows});
  });
}
