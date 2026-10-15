import searchSchema from './search-schema.js';
import {completeDelivery} from './delivery-check.js';
import {renderPanel,initialPanelView} from './panel-view.js';
import {sections,hostReceipt} from './panel-runtime.js';
import {analysisSpawn,analysisTool,fullAnalysisInput,isAnalysis} from './analysis-policy.js';
import {shellImpactPane} from './panel-extras.js';
// Local observation only. Runtime events remain the source of truth; the
// poller coalesces overlapping reads and never calls a model or search tool.
async function savePresentation($,value) {
  try {const session=await $.session.id();await $.store.set('mustard-panel:'+session,{schema_version:1,cwd:await $.session.cwd(),...value});}catch{/* Optional on older hosts. */}
}
async function restorePresentation($) {
  try {const value=await $.store.get('mustard-panel:'+await $.session.id());
    return value?.schema_version===1 && value.cwd===await $.session.cwd()?value:undefined;
  }catch{return undefined;}
}

let timer, eventTimer, busy = false, opened = false, data, problem, selected, usage, revision = 0;
let view=initialPanelView();
let publishing = false, publicationMessage;
let settings,settingsRevision=0;
let runtimeBinary;
let consumptionMeasuredAt;
let panelWanted=false;
let refreshQueued=false,sourceRevision,queryIdentity;
let restoredView=false;
let openingQueue=Promise.resolve();
let impact,impactAction,operationBusy=false;
let shellImpact;
const impactId='mustard-impact';
let presentationQueue=Promise.resolve();
function persist($) {presentationQueue=presentationQueue.catch(()=>{}).then(()=>savePresentation($,{selected,view:{...view},wanted:panelWanted}));return presentationQueue;}
async function localOperation($,command,request) {
  const cwd=await $.session.cwd();
  const result=await $.process.run([await runtime($),'run',command,'--root',cwd,...(request?['--json',JSON.stringify(request)]:[])],{cwd,timeoutMs:10000});
  const value=JSON.parse(result.stdout);if(result.exitCode!==0 || value.ok!==true)throw new Error(value.reason || 'operation-failed');return value;
}
// The cause of a failure on one line, short enough for the diagnostics record.
function cause(error) {return String(error?.message ?? error ?? '').replace(/\s+/g,' ').trim().slice(0,300) || undefined;}
async function diagnostic($,component,operation,reason,detail) {
  try {const cwd=await $.session.cwd();await $.process.run([await runtime($),'run','panel-diagnostics','--root',cwd,'--record'],
    {cwd,timeoutMs:3000,stdin:JSON.stringify({component,operation,reason,...(detail?{detail}:{}),session:await $.session.id()})});}catch{/* Diagnostics never blocks work. */}
}
async function previewImpact($,request,action) {
  try {impact=await localOperation($,'impact',request);impact.canExecute=typeof action==='function';impactAction=action?{run:action,request}:undefined;}
  catch {impact={ok:false,reason:'impact-unavailable'};impactAction=undefined;}
  $.ui.invalidate('ui.render');
}
const panelEvents=[];
function panelEvent(kind,surface) {
  panelEvents.push({kind,at:new Date().toISOString(),...(surface?{surface}: {})});
  if(panelEvents.length>32)panelEvents.shift();
}
let measurementQueue = Promise.resolve();
const searchPurposes = ['locate','understand','spec','implement','validate'];
const searchTools = ['rg','grep','git','Grep','Glob','Read','Symbol','Trace','Structure','References'];
let evidenceEpoch, evidenceAcknowledged = new Map();
const analysisAgents=new Map();
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
    let args=['run','settings','--root',cwd,'--full'],scan=false;
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
      settings={...settings,message:['Índice atualizado.','Index updated.']};
      // Refresh safe availability without disturbing saved choices.
      const fresh=await $.process.run([await runtime($),'run','settings','--root',cwd,'--full'],{cwd,timeoutMs:10000});
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
    if(!opened)return;
    if(busy){refreshQueued=true;return;}
    const ticket = revision;
    busy = true;
    try {
      const cwd = await $.session.cwd();
      let session;
      try { session = await $.session.id(); } catch { /* Session observation may be unavailable. */ }
      // Historical transcript recovery can be expensive on a remote host.
      // Opening the project must not wait for it. Live host receipts remain
      // available in every view; recover history while Usage is visible.
      const measure = view.tab === 'Consumo' && ['Ondas','Histórico','Detalhes'].includes(view.usageView) && (consumptionMeasuredAt === undefined || Date.now() - consumptionMeasuredAt >= 30000);
      const section=sections[view.tab] || 'project';
      const identity=JSON.stringify([section,selected,session,view.tab==='Ondas'?view.selectedWave:null]);
      const result = await $.process.run(
        [await runtime($), 'run', 'panel', '--root', cwd, ...(selected ? ['--spec', selected] : []), ...(session ? ['--session', session] : []),
          '--section',section,...(sourceRevision && queryIdentity===identity && !measure?['--if-revision',sourceRevision]:[]),
          ...(view.tab==='Ondas' && view.selectedWave!==undefined ? ['--wave',String(view.selectedWave)] : []), ...(measure ? ['--refresh-consumption'] : [])],
        { cwd, timeoutMs: measure ? 30000 : 10000 });
      if (result.exitCode !== 0) throw new Error(`exit ${result.exitCode}: ${(result.stderr || '').trim() || 'no error output'}`);
      const next = JSON.parse(result.stdout);
      if (next.ok !== true || next.schema_version !== 1) throw new Error(`incompatible panel answer: ok ${next.ok}, schema_version ${next.schema_version}${typeof next.reason==='string'?', reason '+next.reason:''}`);
      if (!opened || ticket !== revision) return;
      if(!next.unchanged)data=next;
      if(data)data.checked_at=new Date().toISOString();
      sourceRevision=next.revision;queryIdentity=identity;
      if (measure) consumptionMeasuredAt = Date.now();
      if (!selected || !data.specs.some(s => s.name === selected)) selected = data.selected_spec || data.specs[0]?.name;
      try {usage=await $.session.usage();}catch{/* Keep the last native observation. */}
      problem = undefined;
    } catch (error) {
      if (opened && ticket === revision) {problem=t('Não foi possível atualizar. Tente novamente ou abra Diagnóstico.','Could not update. Retry or open Diagnostics.');
        void diagnostic($,'panel','refresh','panel-query-failed',cause(error));}
    } finally {
      busy = false;
      if (opened && ticket === revision) $.ui.invalidate('ui.render');
      if(opened && (refreshQueued || ticket!==revision)){refreshQueued=false;requestRefresh($);}
    }
}
function recordMeasurement($, measurement) {
  // Preserve arrival order. A repeated cumulative reading replaces no spend,
  // and the runtime resolves attribution from the checkout, not the pane tab.
  measurementQueue = measurementQueue.catch(() => {}).then(async () => {
    let failure='usage-record-failed',detail;
    try {
      const cwd = await $.session.cwd();
      const session = await $.session.id();
      const model = await $.session.model();
      const result=await $.process.run([await runtime($), 'run', 'usage-record', '--root', cwd, '--session', session],
        {cwd, timeoutMs:5000, stdin:JSON.stringify({...measurement,model:measurement.model || model})});
      const reply=JSON.parse(result.stdout);
      // The runtime already recorded a live-usage failure, with its cause.
      if(result.exitCode!==0 || (reply.ok!==true && reply.live?.diagnosed!==true)) {
        const reason=reply.live?.reason || reply.reason;
        if(typeof reason==='string' && /^(usage|live-usage)-[a-z-]{1,80}$/.test(reason))failure=reason;
        detail=reply.live?.detail || reply.detail || (typeof reason==='string' && reason!==failure?reason:`exit ${result.exitCode}`);
        throw new Error('usage-record-failed');
      }
    } catch(error) {void diagnostic($,'usage','record',failure,detail ?? cause(error));}
  });
  return measurementQueue;
}
function openPanel($, name) {
    panelWanted=true;
    void persist($);
    // A command and a Desktop attach may arrive together. Serialize their
    // lifecycles so a slower first opening cannot install a second poller.
    openingQueue=openingQueue.catch(()=>{}).then(()=>panelWanted?performOpen($,name):{text:t('Painel fechado.','Pane closed.')});
    return openingQueue;
}
async function performOpen($, name) {
    panelEvent('open-request');
    stopPanel();
    const ticket=revision;
    if (name) selected = name;
    try {
      // open() on an existing id only focuses/retitles it. Explicitly reopen
      // our pane so a reattached Desktop gets a fresh lifecycle too, including
      // after the module was reloaded while the host retained its pane.
      try { await $.ui.close({id}); } catch { /* A detached/reloaded host may no longer know this pane. Still attempt open. */ }
      await $.ui.open({ id, title: 'Mustard', focus: true, closeOnEscape: true, rows:36, columns:100 });
      if(!panelWanted || ticket!==revision){
        await $.ui.close({id});
        return {text:t('A abertura foi cancelada.','Opening was cancelled.')};
      }
      opened = true;
      panelEvent('open-accepted');
      timer = $.clock.every(10000, () => { void refresh($); });
      await refresh($);
      if(!restoredView && data?.preferences?.ok) {
        restoredView=true;const profile=data.preferences.profiles.find(profile=>profile.id===data.preferences.selected);
        if(profile && profile.tab!==view.tab){view.tab=profile.tab;revision++;await refresh($);}
      }
      if(view.tab==='Configurações')await settingsOperation($);
      const observed=await Promise.allSettled([$.session.surfaces(),$.ui.panes()]);
      const surfaces=observed[0].status==='fulfilled'?observed[0].value:[];
      const registered=observed[1].status==='fulfilled' && observed[1].value?.some(pane=>pane.id===id);
      const requestIndex=panelEvents.findLastIndex(event=>event.kind==='open-request');
      const rendered=[...new Set(panelEvents.slice(requestIndex+1).filter(event=>event.kind==='render').map(event=>event.surface).filter(Boolean))];
      return {text:[
        registered?t('Painel registrado nesta sessão.','Pane registered in this session.'):t('Abertura solicitada; o host ainda não confirmou o painel.','Opening requested; the host has not confirmed the pane yet.'),
        `${t('Interfaces conectadas','Connected interfaces')}: ${surfaces?.join(', ') || t('não informadas','not reported')}.`,
        `${t('Renderização observada','Observed rendering')}: ${rendered.join(', ') || t('aguardando o primeiro desenho','waiting for the first render')}.`,
        t('Se não aparecer na interface desejada, execute /mustard-panel --diagnose.','If it does not appear in the intended interface, run /mustard-panel --diagnose.'),
      ].join('\n')};
    } catch {
      panelEvent('open-failed');
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
      `${t('Eventos recentes do painel','Recent panel events')}: ${panelEvents.slice(-8).map(e=>`${e.kind}${e.surface?':'+e.surface:''} @ ${e.at}`).join('; ') || t('nenhum','none')}`,
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
    const saved=await restorePresentation($);
    if(saved){selected=saved.selected;view={...initialPanelView(),...saved.view};panelWanted=saved.wanted===true;restoredView=true;}
    await $.command.register({ name: 'mustard-panel', description: 'Projeto, specs, execução e consumo local', argumentHint: '[spec] | --diagnose', immediate: true });
    await $.command.register({ name: 'mustard-pages', description: 'Publicar projeto ou spec sob pedido explícito', argumentHint: 'project | spec [nome] | report <arquivo.md>', immediate: true });
    await $.tool.register({name:'search',description:'Use Mustard for project code searches and reads. Preserve original arguments and scope. Provide intent (local resource/provider + operation, without repeating the whole project goal) and purpose. locate returns occurrences; understand/spec/implement/validate investigate current source. Once a declaration is found, ask for its scoped evidence instead of opening its whole file. Reuse complete bodies; expand indicated missing ranges before relying on omitted code. Read preserves the requested offset/limit and result. Symbol/Trace use {file_path,symbol,direction?,depth?,target?,limit?} with current cards[].id or owners.symbols[].id (References symbol IDs are provider-specific); Trace follows current static connections, not runtime flow. Structure uses {file_path,query} with a Tree-sitter query. References uses {file_path,line,column,relation?,limit?}; line is one-based and column is a zero-based UTF-8 byte offset; uses a current imported SCIP index or the installed language server from init/doctor. No model is called. Cold server queries can take longer; use them for exact definitions/references, not literal greps. If unavailable, the result includes a native text-search fallback with explicit limitations. Additional candidates/tests have native expansion commands. Investigative Grep/rg/Glob requests permit optional Jev for unresolved source-backed responsibility alternatives when the project enables it; choose=false opts out. Literal lookup and exact reads stay native. The adapter supplies the contract version.',
      inputSchema:searchSchema});
    const result=await next(e);
    if(panelWanted)await openPanel($);
    return result;
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
    // The binary decides whether a source-backed ambiguity actually exists.
    // Allow that decision for investigations without requiring the conductor
    // to remember a flag. Literal lookup, exact reads and explicit opt-outs
    // keep the native path. Project-level Jev authorization still applies.
    const choose=request.choose ?? (request.purpose!=='locate' && ['rg','grep','Grep','Glob'].includes(request.tool));
    const envelope={schema_version:1,request:{...request,choose}};
    const agent=e.agentId || 'main';
    const epoch=evidenceEpoch;
    let env;
    try {
      const session=await $.session.id();
      const identity=value=>typeof value==='string' && /^[a-zA-Z0-9_-]{1,160}$/.test(value);
      // The binary records the call under this session, as a shell run did.
      if (identity(session)) env={MUSTARD_SESSION_ID:session};
      if ([session,agent,epoch].every(identity)) {
        envelope.context={session,agent,epoch,acknowledged:[...(evidenceAcknowledged.get(agent)||[])]};
      }
    } catch { /* Without a host delivery identity every body is delivered again. */ }
    // The host already approved this tool's own call, so the binary runs
    // directly, without a shell. A Bash call made here would carry no model
    // request for the auto mode classifier to judge. The command and read
    // guards still run inside the binary before any search executes.
    let run;
    try {
      run=await $.process.run([await runtime($),'run','search','--root',cwd,'--request',JSON.stringify(envelope),'--shell-output'],
        {cwd,timeoutMs:120000,...(env?{env}:{})});
    } catch (error) {
      // A timeout or a binary that never started delivers no evidence.
      return {result:`Mustard search did not finish: ${error?.message || error}`,isError:true};
    }
    // Exit 1 is how rg, grep, git grep, Grep and Glob say nothing matched, and
    // the binary's own report already counts it as ok. Read has no such
    // answer, so its exit 1 stays a failure.
    const unmatched=run.exitCode===1 && request.tool!=='Read';
    const failed=run.exitCode!==0 && !unmatched;
    let stdout=run.stdout || '';
    const receipt=/\n# mustard-delivery:([a-f0-9]{64}):(\d+):([a-f0-9]{16})\n?$/.exec(stdout);
    if (receipt && envelope.context && epoch===evidenceEpoch && !failed
        && completeDelivery(stdout.slice(0,receipt.index),receipt)) {
      const acknowledged=evidenceAcknowledged.get(agent)||new Set();
      acknowledged.add(receipt[1]);
      if(acknowledged.size>128) acknowledged.delete(acknowledged.values().next().value);
      evidenceAcknowledged.set(agent,acknowledged);
    }
    if(receipt)stdout=stdout.slice(0,receipt.index);
    const stderr=run.stderr || '';
    const text=stdout+(stderr?(stdout?'\nstderr:\n':'')+stderr:'');
    // Mods validates the registered tool's result as content, not as an MCP
    // tools/call envelope. Error status belongs to the tool.call reply itself.
    // A silent failure still names its exit code; so does a silent search
    // that matched nothing, without the error status.
    if(failed) return {result:text || `Process exited with code ${run.exitCode}`,isError:true};
    return {result:text || (unmatched?'No matches (exit code 1)':'')};
  });
  // Observe completed operations without replacing permissions or the
  // native orchestration. Coalesce bursts; retain polling for external edits.
  on('tool.call', async ($, e, next) => {
    const result=await next(e);
    if(result?.isError || result?.deny)void diagnostic($,'tool',e.tool,result.deny?'tool-denied':'tool-failed');
    requestRefresh($);
    return result;
  });
  on('tool.call',{tool:'Bash'},async ($,e,next)=>{
    // Only a display observer. The native command guard and host permission
    // checks still decide; no new prompt, rewritten command or approval path.
    let previewed=false;
    if(/\b(rm|rmdir|Remove-Item|git\s+(?:clean|reset|restore|checkout|push|branch))\b/.test(e.command || '')) {
      try {
        const preview=await localOperation($,'impact',{action:'shell',command:e.command});
        if(preview.relevant || preview.effects?.length){shellImpact=preview;previewed=true;
          await $.ui.open({id:impactId,title:'Mustard · Impacto',focus:false,closeOnEscape:true,rows:24,columns:90});}
      }catch{/* Display failures never replace an existing permission decision. */}
    }
    try {const result=await next(e);
      if(previewed){shellImpact.toolState=result.deny?'bloqueada':result.isError?'retornou com erro':'retornou';$.ui.invalidate('ui.render');}
      return result;
    }catch(error){if(previewed){shellImpact.toolState='interrompida ou falhou';$.ui.invalidate('ui.render');}throw error;}
  });
  on('turn.complete', async ($, e, next) => {
    const result=await next(e);
    if(e.agentId)await recordMeasurement($,hostReceipt(e));
    if(e.agentId)analysisAgents.delete(e.agentId);
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
    await recordMeasurement($, hostReceipt(e));
    if (opened) $.ui.invalidate('ui.render');
    requestRefresh($);
    return next(e);
  });
  on('ui.close', { id }, async ($, e, next) => {
    panelEvent('close',e.origin?.kind);
    // Local closes already call stopPanel before the API. A delayed event
    // from the preceding pane must not stop the replacement's poller.
    if(e.origin?.kind==='plugin')return next(e);
    // Reload unloads the visual tree, not the user's request to keep it open.
    if(e.origin?.kind!=='unload')panelWanted=false;
    void persist($);
    stopPanel();
    return next(e);
  });
  on('session.end', async ($, e, next) => {
    panelWanted=false;
    void persist($);
    panelEvent('session-end');
    resetEvidence();
    stopPanel();
    data = undefined;
    selected=usage=problem=undefined;
    view=initialPanelView();
    settings=undefined;settingsRevision++;
    consumptionMeasuredAt = undefined;
    publicationMessage = undefined;
    try {await presentationQueue;await $.store.delete('mustard-panel:'+await $.session.id());}catch{/* Old hosts may lack storage. */}
    return next(e);
  });
  on('session.attach', async ($,e,next)=>{
    panelEvent('attach');
    const result=await next(e);
    if(panelWanted)await openPanel($);
    return result;
  });
  on('session.detach', async ($,e,next)=>{
    panelEvent('detach');
    // Suspend polling while detached; retain the user's open request.
    const surfaces=await $.session.surfaces().catch(()=>undefined);
    if(!surfaces?.length)stopPanel();
    return next(e);
  });
  on('ui.fault',async ($,e,next)=>{panelEvent('ui-fault',e.surface);void diagnostic($,'panel','render','ui-fault');return next(e);});
  on('agent.spawn',async ($,e,next)=>{
    const prepared=analysisSpawn(e);
    if(prepared.deny)return prepared;
    const result=await next(prepared);
    if(result.agentId && isAnalysis(e.subagentType))analysisAgents.set(result.agentId,{input:null,model:result.model});
    if(result.agentId && !result.deny)await recordMeasurement($,{kind:'spawn',agent_id:result.agentId,model:result.model,agent_type:e.subagentType});
    requestRefresh($);return result;
  });
  on('tool.check',async ($,e,next)=>{
    const blocked=analysisTool(e,analysisAgents.get(e.agentId));
    return blocked || next(e);
  });
  on('turn.step',async function* ($,e,next){
    const analysis=analysisAgents.get(e.agentId);
    if(analysis?.input>=90000) {
      const answer='[Mustard] Analysis stopped before another model request to preserve the fixed 100000-token input budget. Use current source evidence; do not persist an invented interpretation.';
      yield {kind:'text',index:0,text:answer};
      return {turnId:e.turnId,index:e.index,answer,toolUses:[],stopReason:'end_turn',usage:null};
    }
    const result=yield* next(analysis?{...e,model:'haiku',effort:'medium'}:e);
    if(analysis)analysis.input=fullAnalysisInput(result.usage);
    if(result.usage)await recordMeasurement($,hostReceipt(e,result));
    requestRefresh($);return result;
  });
  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if(e.requestId===impactId)return shellImpactPane(await $.ui.resolve(e),shellImpact,async()=>{shellImpact=undefined;await $.ui.close({id:impactId});});
    if (e.requestId !== id) return next(e);
    // Observe every opening independently, without logging each redraw.
    const requestIndex=panelEvents.findLastIndex(event=>event.kind==='open-request');
    if(!panelEvents.slice(requestIndex+1).some(row=>row.kind==='render' && row.surface===e.surface))panelEvent('render',e.surface);
    const redraw=()=>$.ui.invalidate('ui.render');
    return renderPanel({elements:await $.ui.resolve(e),surface:e.surface,columns:e.props.bodyColumns ?? e.viewport?.columns,bodyRows:e.props.scroll?.bodyRows,
      data,usage,problem,selected,view,publicationMessage,publishing,settings,impact,operationBusy,actions:{
        change:(patch,query=false)=>{
          Object.assign(view,patch);
          query=query || !!patch.tab;
          if(query)revision++;
          void persist($);
          redraw();
          if(patch.tab==='Configurações' && !settings?.loaded)void settingsOperation($);
          if(query)void refresh($);
        },
        selectSpec:name=>{
          selected=name;const previous=view,tab=view.tab;view={...previous,...initialPanelView(),tab,usageView:previous.usageView,settingsGroup:previous.settingsGroup};
          if(tab==='Ondas'){const waves=data?.specs?.find(spec=>spec.name===name)?.waves || [];
            view.selectedWave=(waves.find(wave=>['running','repair-required','received'].includes(wave.status)) || waves[0])?.wave;}
          revision++;redraw();void refresh($);
          void persist($);
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
        publish:args=>{
          const scope=args.split(/\s+/)[0];
          return previewImpact($,{action:'publish',scope,spec:scope==='spec'?selected:undefined,document:scope==='report'?args.slice(7).trim():undefined},()=>publishPage($,args));
        },
        preview:request=>previewImpact($,request),
        cancelImpact:()=>{impact=impactAction=undefined;redraw();},
        confirmImpact:async()=>{
          const action=impactAction;if(!action)return;
          try {const current=await localOperation($,'impact',action.request);
            if(current.token!==impact.token){impact={...current,canExecute:true,changed:true};redraw();return;}
            impact=impactAction=undefined;await action.run();
          }catch{impact={ok:false,reason:'impact-unavailable'};impactAction=undefined;}redraw();
        },
        idea:async patch=>{
          if(operationBusy)return;operationBusy=true;redraw();
          try {const value=await localOperation($,'ideas',{schema_version:1,revision:data.ideas.revision,...patch});
            data.ideas=value;view.ideaTitle=view.ideaDetail='';view.selectedIdea=undefined;problem=undefined;}
          catch(error){problem=t('A ideia não foi salva. Recarregue para conferir alterações concorrentes.','Idea was not saved. Reload to check concurrent edits.');}
          finally{operationBusy=false;redraw();void refresh($);}
        },
        startIdea:async item=>{
          try {await $.prompt.fill({text:t(`Iniciar o levantamento de uma nova spec para a ideia ${item.id}: ${item.title}\n${item.detail}`,`Start a new spec survey for idea ${item.id}: ${item.title}\n${item.detail}`),mode:'append'});}
          catch{problem=t('Não foi possível preencher o pedido. Copie a ideia para iniciar o levantamento.','Could not fill the request. Copy the idea to start the survey.');redraw();}
        },
        profile:async patch=>{
          if(operationBusy)return;operationBusy=true;
          try {const value=await localOperation($,'panel-preferences',{schema_version:1,revision:data.preferences.revision,...patch});
            data.preferences=value;
            if(patch.selected){const profile=value.profiles.find(p=>p.id===patch.selected);view.tab=profile.tab;view.waveView=patch.selected==='review'?'Entrega':'Plano';
              view.profileName=view.profileTabs=view.profileTab=undefined;revision++;void refresh($);}
            void persist($);
          }catch{problem=t('O perfil não foi salvo. Recarregue antes de tentar novamente.','Profile was not saved. Reload before retrying.');}
          finally{operationBusy=false;redraw();}
        },
        exportDiagnostics:async()=>{
          try {const cwd=await $.session.cwd();const result=await $.process.run([await runtime($),'run','panel-diagnostics','--root',cwd,'--export'],{cwd,timeoutMs:10000});
            const value=JSON.parse(result.stdout);problem=value.ok?t(`Diagnóstico local: ${value.path}`,`Local diagnostic: ${value.path}`):t('Falha ao exportar diagnóstico.','Diagnostic export failed.');}
          catch{problem=t('Falha ao exportar diagnóstico.','Diagnostic export failed.');}redraw();
        },
        close:async()=>{panelWanted=false;await persist($);stopPanel();await $.ui.close({id});},
      }});
  });
}
