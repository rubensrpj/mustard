import searchSchema from './search-schema.js';
import {completeDelivery} from './delivery-check.js';
import {renderPanel,initialPanelView} from './panel-view.js';
// Local observation only. Runtime events remain the source of truth; the
// poller coalesces overlapping reads and never calls a model or search tool.
let timer, eventTimer, busy = false, opened = false, data, problem, selected, usage, revision = 0;
let view=initialPanelView();
let publishing = false, publicationMessage;
let settings,settingsRevision=0;
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
async function settingsOperation($,operation='load') {
  if(settings?.busy || !opened)return;
  const ticket=settingsRevision;
  settings={...settings,busy:true,error:undefined,message:undefined};
  $.ui.invalidate('ui.render');
  try {
    const cwd=await $.session.cwd();
    let args=['run','settings','--root',cwd],scan=false;
    if(operation==='save')args.push('--json',JSON.stringify({schema_version:1,revision:settings.revision,changes:settings.changes}));
    if(operation==='scan') {scan=true;args=['run','scan','--root',cwd];}
    const result=await $.process.run([await runtime($),...args],{cwd,timeoutMs:scan?110000:10000});
    if(!opened || ticket!==settingsRevision)return;
    if(result.exitCode!==0)throw new Error('settings-operation-failed');
    const value=JSON.parse(result.stdout);
    if(value.ok!==true || (!scan && value.schema_version!==1)) {
      settings={...settings,...(value.saved?{...value,loaded:true}:{}),error:typeof value.reason==='string'?value.reason:'settings-operation-failed'};
      return;
    }
    if(scan) {
      settings={...settings,busy:false,message:['Índice atualizado.','Index updated.']};
      // Refresh safe availability without disturbing saved choices.
      const fresh=await $.process.run([await runtime($),'run','settings','--root',cwd],{cwd,timeoutMs:10000});
      if(!opened || ticket!==settingsRevision)return;
      const next=JSON.parse(fresh.stdout);
      if(fresh.exitCode===0 && next.ok===true && next.schema_version===1)settings={...next,loaded:true,changes:{},message:settings.message};
    } else {
      settings={...value,loaded:true,changes:{},message:operation==='save'?
        ['Configurações salvas. Mudanças de agentes valem nas próximas execuções.','Settings saved. Agent changes apply to future runs.']:undefined};
    }
    void refresh($);
  } catch {if(opened && ticket===settingsRevision)settings={...settings,error:'settings-operation-failed'};}
  finally {if(opened && ticket===settingsRevision){settings={...settings,busy:false};$.ui.invalidate('ui.render');}}
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
      // Historical transcript recovery can be expensive on a remote host.
      // Opening the project must not wait for it. Live host receipts remain
      // available in every view; recover history while Usage is visible.
      const measure = view.tab === 'Consumo' && (consumptionMeasuredAt === undefined || Date.now() - consumptionMeasuredAt >= 30000);
      const result = await $.process.run(
        [await runtime($), 'run', 'panel', '--root', cwd, ...(selected ? ['--spec', selected] : []), ...(session ? ['--session', session] : []),
          ...(view.tab==='Ondas' && view.selectedWave!==undefined ? ['--wave',String(view.selectedWave)] : []), ...(measure ? ['--refresh-consumption'] : [])],
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
    stopPanel();
    if (name) selected = name;
    try {
      // open() on an existing id only focuses/retitles it. Explicitly reopen
      // our pane so a reattached Desktop gets a fresh lifecycle too, including
      // after the module was reloaded while the host retained its pane.
      await $.ui.close({id});
      await $.ui.open({ id, title: 'Mustard', focus: true, closeOnEscape: true, rows:36, columns:100 });
      opened = true;
      timer = $.clock.every(2000, () => { void refresh($); });
      await refresh($);
      if(view.tab==='Configurações')await settingsOperation($);
      return {text:t('Abertura do painel solicitada nesta sessão. Se não aparecer, execute /mustard-panel --diagnose.',
        'Panel opening requested in this session. If it does not appear, run /mustard-panel --diagnose.')};
    } catch {
      stopPanel();
      return {text:t('O Claude não concluiu a abertura do painel. Execute /mustard-panel --diagnose nesta mesma sessão; confira /plugin e /status. Nenhuma publicação foi feita.',
        'Claude did not complete opening the panel. Run /mustard-panel --diagnose in this session; check /plugin and /status. Nothing was published.')};
    }
}
async function panelDiagnostics($) {
    const details=await Promise.allSettled([$.session.surfaces(),$.session.version(),$.ui.panes()]);
    const [surfaces,version,panes]=details.map(result=>result.status==='fulfilled'?result.value:undefined);
    const pane=panes?.find(pane=>pane.id===id);
    const interfaces=surfaces?.length ? surfaces.join(', ') : t('nenhuma informada','none reported');
    return {text:[
      `Mustard · Claude Code ${version?.version || t('não informado','not reported')}`,
      `${t('Plugin carregado','Loaded plugin')}: ${$.plugin.root}`,
      `${t('Interfaces reconhecidas','Recognized interfaces')}: ${interfaces}`,
      `${t('Painel registrado no host','Pane registered in host')}: ${pane?t('sim','yes'):t('não','no')}`,
      ...(pane?[t(`Host: visível=${pane.isShown}, posicionado=${pane.isPlaced}. Isso não comprova que o Desktop desenhou o painel.`,
        `Host: shown=${pane.isShown}, placed=${pane.isPlaced}. This does not prove Desktop painted the pane.`)]:[]),
      ...(!surfaces?.includes('desktop')?[t('O host não informou uma interface Desktop. Em SSH, confira a sessão, /status e a versão do plugin no Desktop; Remote Control não espelha painéis.',
        'The host did not report a Desktop interface. Over SSH, check the session, /status and the plugin version in Desktop; Remote Control does not mirror panes.')]:[]),
      t('Este diagnóstico não lê conversas, chama IA ou publica páginas.','This diagnostic does not read conversations, call AI or publish pages.'),
    ].join('\n')};
}
function stopPanel() {
    settingsRevision++;settings=undefined;
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
    await $.command.register({ name: 'mustard-panel', description: 'Projeto, specs, execução e consumo local', argumentHint: '[spec] | --diagnose', immediate: true });
    await $.command.register({ name: 'mustard-pages', description: 'Publicar projeto ou spec sob pedido explícito', argumentHint: 'project | spec [nome] | report <arquivo.md>', immediate: true });
    await $.tool.register({name:'search',description:'Use Mustard for project code searches and reads. Preserve original arguments and scope. Provide intent (local resource/provider + operation, without repeating the whole project goal) and purpose. locate returns occurrences; understand/spec/implement/validate investigate current source. Once a declaration is found, ask for its scoped evidence instead of opening its whole file. Reuse complete bodies; expand indicated missing ranges before relying on omitted code. Read preserves the requested offset/limit and result. Symbol/Trace use {file_path,symbol,direction?,depth?,target?,limit?} with current cards[].id or owners.symbols[].id (References symbol IDs are provider-specific); Trace follows current static connections, not runtime flow. Structure uses {file_path,query} with a Tree-sitter query. References uses {file_path,line,column,relation?,limit?}; line is one-based and column is a zero-based UTF-8 byte offset; uses a current imported SCIP index or the installed language server from init/doctor. No model is called. Cold server queries can take longer; use them for exact definitions/references, not literal greps. If unavailable, the result includes a native text-search fallback with explicit limitations. Additional candidates/tests have native expansion commands. Investigative Grep/rg/Glob requests permit optional Jev for unresolved source-backed responsibility alternatives when the project enables it; choose=false opts out. Literal lookup and exact reads stay native. The adapter supplies the contract version.',
      inputSchema:searchSchema});
    return next(e);
  });
  on('command.run', { command: 'mustard-panel' }, async ($, e) => {
    if (e.args.trim()==='--diagnose') return panelDiagnostics($);
    return openPanel($, e.args.trim() || undefined);
  });
  on('command.run', { command: 'mustard-pages' }, async ($,e)=>publishPage($,e.args));
  on('tool.call',{tool:'mcp__mustard__search'},async ($,e)=>{
    const request=e.request;
    const problem=searchContractError(request);
    if(problem) return {result:`search-contract: ${problem}. Retry with {request:{tool,input,intent,purpose,choose?}}; preserve original search arguments.`,isError:true};
    const cwd=await $.session.cwd();
    // MCP arguments are fields of e; the nested request avoids reserved tool.
    // Use Bash through the host tool API so
    // permissions, classic hooks and the shell sandbox still apply. A process
    // launched directly by a mod would bypass that boundary.
    const quote=value=>"'"+String(value).replace(/'/g,"'\\''")+"'";
    // The binary decides whether a source-backed ambiguity actually exists.
    // Allow that decision for investigations without requiring the conductor
    // to remember a flag. Literal lookup, exact reads and explicit opt-outs
    // keep the native path. Project-level Jev authorization still applies.
    const choose=request.choose ?? (request.purpose!=='locate' && ['rg','grep','Grep','Glob'].includes(request.tool));
    const envelope={schema_version:1,request:{...request,choose}};
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
    const failed=result.isError===true || result.result?.interrupted===true;
    let stdout=result.result?.stdout ?? result.text ?? (typeof result.result==='string' ? result.result : '');
    const receipt=/\n# mustard-delivery:([a-f0-9]{64}):(\d+):([a-f0-9]{16})\n?$/.exec(stdout);
    if (receipt && envelope.context && epoch===evidenceEpoch && !failed
        && completeDelivery(stdout.slice(0,receipt.index),receipt)) {
      const acknowledged=evidenceAcknowledged.get(agent)||new Set();
      acknowledged.add(receipt[1]);
      if(acknowledged.size>128) acknowledged.delete(acknowledged.values().next().value);
      evidenceAcknowledged.set(agent,acknowledged);
    }
    if(receipt)stdout=stdout.slice(0,receipt.index);
    const stderr=result.result?.stderr || '';
    // Mods validates the registered tool's result as content, not as an MCP
    // tools/call envelope. Error status belongs to the tool.call reply itself.
    return {result:stdout+(stderr?(stdout?'\nstderr:\n':'')+stderr:''),...(failed ? {isError:true} : {})};
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
    selected=usage=problem=undefined;
    view=initialPanelView();
    settings=undefined;settingsRevision++;
    consumptionMeasuredAt = undefined;
    publicationMessage = undefined;
    return next(e);
  });
  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== id) return next(e);
    const redraw=()=>$.ui.invalidate('ui.render');
    return renderPanel({elements:await $.ui.resolve(e),surface:e.surface,columns:e.props.bodyColumns ?? e.viewport?.columns,bodyRows:e.props.scroll?.bodyRows,
      data,usage,problem,selected,view,publicationMessage,publishing,settings,actions:{
        change:(patch,query=false)=>{
          Object.assign(view,patch);
          if(query)revision++;
          redraw();
          if(patch.tab==='Configurações' && !settings?.loaded)void settingsOperation($);
          if(query)void refresh($);
        },
        selectSpec:name=>{
          selected=name;const tab=view.tab;view={...initialPanelView(),tab};
          if(tab==='Ondas'){const waves=data?.specs?.find(spec=>spec.name===name)?.waves || [];
            view.selectedWave=(waves.find(wave=>['running','repair-required','received'].includes(wave.status)) || waves[0])?.wave;}
          revision++;redraw();void refresh($);
        },
        refresh:()=>{void refresh($);},
        editSetting:(field,value)=>{
          if(!settings?.loaded || settings.busy)return;
          const changes={...settings.changes};
          if(value===settings.values[field])delete changes[field];else changes[field]=value;
          settings={...settings,changes,error:undefined,message:undefined};redraw();
        },
        reloadSettings:()=>{void settingsOperation($);},
        saveSettings:()=>{void settingsOperation($,'save');},
        scanSettings:()=>{void settingsOperation($,'scan');},
        publish:args=>publishPage($,args),
        close:async()=>{stopPanel();await $.ui.close({id});},
      }});
  });
}
