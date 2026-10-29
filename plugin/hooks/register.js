import searchSchema from './search-schema.js';
import {completeDelivery} from './delivery-check.js';
import {renderPanel,initialPanelState,screenQuery,PAGE,FILTERS} from './panel-view.js';
import {hostReceipt} from './panel-runtime.js';
import {analysisSpawn,analysisTool,fullAnalysisInput,isAnalysis} from './analysis-policy.js';
// Local observation only. Runtime events remain the source of truth; the
// panel reads the binary when it opens, when its screen changes and when a
// turn of the conductor or an agent ends, and never calls a model or search.

const id = 'mustard-panel';
// The panel's place (screen, spec, kind, search, filter, rows, item and the
// way back) lives in the host's state, which survives a reload of this code,
// with a copy in the store for the next start of the session.
const panelState = {plugin:'mustard',key:'panel'};
let mirror=initialPanelState();
let opened=false, busy=false, queued=false, ticket=0, problem, usage, project, themeName, sessionId;
// The last answer of each screen, with the arguments it was asked with and
// its revision for the conditional read.
let cache={};
let measuring=false, measureWanted=false;
let publishing=false;
// The panel's own publication: running, then the answer /mustard:pages gave.
let publication;
let settings,settingsRevision=0;
let runtimeBinary;
let panelWanted=false;
// The key of the element holding the pane's focus ring, as `ui.focus` names it.
let focused;
let openingQueue=Promise.resolve(), presentationQueue=Promise.resolve(), stateQueue=Promise.resolve();
async function panel($) {
  try {const {value}=await $.state.get(panelState);if(value)mirror={...initialPanelState(),...value};}catch{/* Older hosts keep the module's copy. */}
  return mirror;
}
function setPanel($,patch) {
  stateQueue=stateQueue.catch(()=>{}).then(async ()=>{
    const current=await panel($);
    mirror={...current,...(typeof patch==='function'?patch(current):patch)};
    try {await $.state.set(panelState,mirror);}catch{$.ui.invalidate('ui.render');}
    void persist($);
  });
  return stateQueue;
}
function persist($) {
  presentationQueue=presentationQueue.catch(()=>{}).then(async ()=>{
    try {const session=await $.session.id();await $.store.set('mustard-panel:'+session,{schema_version:2,cwd:await $.session.cwd(),state:mirror,wanted:panelWanted});}
    catch{/* Optional on older hosts. */}
  });
  return presentationQueue;
}
async function restorePresentation($) {
  try {const value=await $.store.get('mustard-panel:'+await $.session.id());
    return value?.schema_version===2 && value.cwd===await $.session.cwd()?value:undefined;
  }catch{return undefined;}
}
// The cause of a failure on one line, short enough for the diagnostics record.
function cause(error) {return String(error?.message ?? error ?? '').replace(/\s+/g,' ').trim().slice(0,300) || undefined;}
async function diagnostic($,component,operation,reason,detail) {
  try {const cwd=await $.session.cwd();await $.process.run([await runtime($),'run','panel-diagnostics','--root',cwd,'--record'],
    {cwd,timeoutMs:3000,stdin:JSON.stringify({component,operation,reason,...(detail?{detail}:{}),session:await $.session.id()})});}catch{/* Diagnostics never blocks work. */}
}

const panelEvents=[];
function panelEvent(kind,surface) {
  panelEvents.push({kind,at:new Date().toISOString(),...(surface?{surface}: {})});
  if(panelEvents.length>32)panelEvents.shift();
}
let measurementQueue = Promise.resolve();
const searchPurposes = ['locate','understand','spec','implement','validate'];
const searchTools = ['rg','grep','git','Grep','Glob','Read'];
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
  const mine=settingsRevision;
  settings={...settings,busy:true,error:undefined,message:undefined};
  $.ui.invalidate('ui.render');
  try {
    const cwd=await $.session.cwd();
    const args=['run','settings','--root',cwd,'--full'];
    // The patch may carry the token: it goes on standard input, never as an
    // argument another process could read.
    const patch=operation==='save'?JSON.stringify({schema_version:1,revision:settings.revision,changes:settings.changes}):undefined;
    if(patch)args.push('--stdin');
    const result=await $.process.run([await runtime($),...args],{cwd,timeoutMs:10000,...(patch?{stdin:patch}:{})});
    if(!opened || mine!==settingsRevision)return;
    if(result.exitCode!==0)throw new Error('settings-operation-failed');
    const value=JSON.parse(result.stdout);
    if(value.ok!==true || value.schema_version!==1) {
      settings={...settings,...(value.saved?{...value,loaded:true}:{}),error:typeof value.reason==='string'?value.reason:'settings-operation-failed'};
      return;
    }
    // A save answers the values read back: the token is never among them.
    settings={...value,loaded:true,changes:{},message:operation==='save'?
      ['Configurações salvas. Mudanças de agentes valem nas próximas execuções.','Settings saved. Agent changes apply to future runs.']:undefined};
  } catch {if(opened && mine===settingsRevision)settings={...settings,error:'settings-operation-failed'};}
  finally {if(opened && mine===settingsRevision){settings={...settings,busy:false};$.ui.invalidate('ui.render');}}
}
function t(pt,en) { return project?.language === 'en-US' ? en : pt; }
/** What a screen was asked with: the screen, the spec, the session and its arguments. */
function identityOf(state) {return JSON.stringify([state.screen,state.spec,sessionId,screenQuery(state)?.args ?? null]);}
function answerOf(result) {
  if (result.exitCode !== 0) throw new Error(`exit ${result.exitCode}: ${(result.stderr || '').trim() || 'no error output'}`);
  const value = JSON.parse(result.stdout);
  if (value.ok !== true || value.schema_version !== 1) throw new Error(`incompatible panel answer: ok ${value.ok}, schema_version ${value.schema_version}${typeof value.reason==='string'?', reason '+value.reason:''}`);
  return value;
}
async function ask($,state,section,args,timeoutMs=10000) {
  const cwd=await $.session.cwd();
  return answerOf(await $.process.run([await runtime($),'run','panel','--root',cwd,'--section',section,...(state.spec?['--spec',state.spec]:[]),
    ...(sessionId?['--session',sessionId]:[]),...args],{cwd,timeoutMs}));
}
/** Reads the visible screen: conditionally when it was read with the same arguments. */
async function refresh($,{force=false}={}) {
  if(!opened)return;
  if(busy){queued=true;return;}
  const mine=ticket;busy=true;
  const measure=measureWanted;measureWanted=false;
  try {
    try {sessionId=await $.session.id();} catch { /* Session observation may be unavailable. */ }
    const state=await panel($);
    const query=screenQuery(state);
    if(!query)return;
    const identity=identityOf(state),held=cache[state.screen];
    const conditional=!force && !measure && held?.identity===identity && held.revision;
    // The measure is the one read that refreshes the consumption history and
    // the copies' disk; only the Measure now button asks for it.
    const next=await ask($,state,query.section,[...query.args,...(conditional?['--if-revision',held.revision]:[]),...(measure?['--refresh-consumption']:[])],measure?120000:10000);
    if(!opened || mine!==ticket)return;
    if(!next.unchanged) {
      // A list longer than one page of the binary comes in pages of 50.
      for(let offset=50;state.screen==='items' && offset<state.rows && next.rows.length===offset;offset+=50)
        next.rows.push(...(await ask($,state,'items',['--kind',state.kind,'--filter',state.filter,'--offset',String(offset),
          '--limit',String(Math.min(50,state.rows-offset)),...(state.query.trim()?['--query',state.query.trim()]:[])])).rows);
      if(!opened || mine!==ticket)return;
      cache[state.screen]={identity,revision:next.revision,value:next};
      project=next.project ?? project;
    }
    try {usage={...usage,...await $.session.usage()};} catch { /* Keep the last native observation. */ }
    try {themeName=(await $.config.list()).find(row=>row.key==='theme')?.value;} catch { /* The chart keeps its dark colors. */ }
    problem=undefined;
  } catch (error) {
    if (opened && mine === ticket) {problem=t('Não foi possível atualizar. O painel mostra a última leitura.','Could not update. The panel shows the last reading.');
      void diagnostic($,'panel','refresh','panel-query-failed',cause(error));}
  } finally {
    busy=false;
    if(measure)measuring=false;
    if(opened)$.ui.invalidate('ui.render');
    if(opened && (queued || mine!==ticket || measureWanted)){queued=false;void refresh($);}
  }
}
async function navigate($,screen,patch={}) {
  ticket++;
  // Opening a list from a counter starts it over: first page, no search.
  const fresh=screen==='items' && patch.kind?{rows:PAGE,query:'',filter:'all'}:{};
  await setPanel($,state=>({...fresh,...patch,screen,history:false,waveRequest:false,searching:false,pickSpec:false,
    back:[...state.back,{screen:state.screen,item:state.item,kind:state.kind}].slice(-20)}));
  await show($,screen);
}
async function show($,screen) {
  if(screen!=='settings')return refresh($);
  if(settings?.loaded)$.ui.invalidate('ui.render');else await settingsOperation($);
}
async function goBack($) {
  const state=await panel($),previous=state.back.at(-1);
  if(!previous)return;
  ticket++;
  await setPanel($,{screen:previous.screen,item:previous.item,kind:previous.kind,back:state.back.slice(0,-1),history:false,waveRequest:false,searching:false,pickSpec:false});
  await show($,previous.screen);
}
async function reread($,patch) {
  ticket++;
  await setPanel($,patch);
  await refresh($);
}
/** Mostrar mais: the next eight rows, added under the ones shown. */
async function showMore($) {
  const state=await panel($),held=cache.items;
  if(!held || state.screen!=='items')return;
  try {
    const page=await ask($,state,'items',['--kind',state.kind,'--filter',state.filter,'--offset',String(held.value.rows.length),'--limit',String(PAGE),
      ...(state.query.trim()?['--query',state.query.trim()]:[])]);
    if(cache.items!==held)return;
    const value={...held.value,rows:[...held.value.rows,...page.rows],total:page.total};
    const rows=value.rows.length;
    // The rows shown now were not read in one go: the next reading is whole.
    cache.items={identity:identityOf({...state,rows}),revision:undefined,value};
    await setPanel($,{rows});
  } catch (error) {
    problem=t('Não foi possível trazer mais linhas.','Could not bring more rows.');void diagnostic($,'panel','more','panel-query-failed',cause(error));
  }
  $.ui.invalidate('ui.render');
}
function measureNow($) {
  if(measuring)return;
  measuring=true;measureWanted=true;
  $.ui.invalidate('ui.render');
  return refresh($);
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
function openPanel($, name, fresh=false) {
    panelWanted=true;
    void persist($);
    // A command and a Desktop attach may arrive together. Serialize their
    // lifecycles so a slower first opening cannot leave two panes reading.
    openingQueue=openingQueue.catch(()=>{}).then(()=>panelWanted?performOpen($,name,fresh):{text:t('Painel fechado.','Pane closed.')});
    return openingQueue;
}
async function performOpen($, name, fresh) {
    panelEvent('open-request');
    stopPanel();
    const mine=ticket;
    cache={};
    // The command opens on the overview; a reload or a reattached Desktop
    // keeps the screen the person was on.
    if(fresh || name)await setPanel($,{...(fresh?{screen:'overview',back:[],item:null,history:false,waveRequest:false,searching:false,pickSpec:false}:{}),...(name?{spec:name}:{})});
    try {
      // open() on an existing id only focuses/retitles it. Explicitly reopen
      // our pane so a reattached Desktop gets a fresh lifecycle too, including
      // after the module was reloaded while the host retained its pane.
      try { await $.ui.close({id}); } catch { /* A detached/reloaded host may no longer know this pane. Still attempt open. */ }
      await $.ui.open({ id, title: 'Mustard', focus: true, closeOnEscape: true, rows:36, columns:100 });
      if(!panelWanted || mine!==ticket){
        await $.ui.close({id});
        return {text:t('A abertura foi cancelada.','Opening was cancelled.')};
      }
      opened = true;
      panelEvent('open-accepted');
      await show($,(await panel($)).screen);
      const observed=await Promise.allSettled([$.session.surfaces(),$.ui.panes()]);
      const surfaces=observed[0].status==='fulfilled'?observed[0].value:[];
      const registered=observed[1].status==='fulfilled' && observed[1].value?.some(pane=>pane.id===id);
      const requestIndex=panelEvents.findLastIndex(event=>event.kind==='open-request');
      const rendered=[...new Set(panelEvents.slice(requestIndex+1).filter(event=>event.kind==='render').map(event=>event.surface).filter(Boolean))];
      return {text:[
        registered?t('Painel registrado nesta sessão.','Pane registered in this session.'):t('Abertura solicitada; o host ainda não confirmou o painel.','Opening requested; the host has not confirmed the pane yet.'),
        `${t('Interfaces conectadas','Connected interfaces')}: ${surfaces?.join(', ') || t('não informadas','not reported')}.`,
        `${t('Renderização observada','Observed rendering')}: ${rendered.join(', ') || t('aguardando o primeiro desenho','waiting for the first render')}.`,
        t('Se não aparecer na interface desejada, execute /mustard:mods --diagnose.','If it does not appear in the intended interface, run /mustard:mods --diagnose.'),
      ].join('\n')};
    } catch {
      panelEvent('open-failed');
      stopPanel();
      return {text:t('O Claude não concluiu a abertura do painel. Execute /mustard:mods --diagnose nesta mesma sessão; confira /plugin e /status. Nenhuma publicação foi feita.',
        'Claude did not complete opening the panel. Run /mustard:mods --diagnose in this session; check /plugin and /status. Nothing was published.')};
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
    ticket++;
    opened = false;
    measuring=measureWanted=queued=false;
    publication=undefined;
}
/** Publicar no Cloudflare: the project publication of /mustard:pages, only on the press. */
async function publishFromPanel($) {
    if(publishing || publication?.busy)return;
    // A screen change keeps the publication; closing the panel drops it.
    const pending={busy:true};
    publication=pending;
    $.ui.invalidate('ui.render');
    const answer=await publishPage($,'project');
    if(publication!==pending)return;
    publication={text:answer.text};
    if(opened)$.ui.invalidate('ui.render');
}

async function publishPage($,rawArgs) {
    const raw=rawArgs.trim();
    const args=raw.split(/\s+/);
    const scope=args[0];
    const report=scope==='report' ? raw.slice(scope.length).trim().replace(/^(["'])(.*)\1$/,'$2') : undefined;
    const explicit=args[1];
    if (!['project','spec','report'].includes(scope) || scope==='report' && !report || scope!=='report' && (args.length>2 || scope==='project' && explicit)) {
      return {text:'Use /mustard:pages project, /mustard:pages spec [nome] ou /mustard:pages report arquivo.md.'};
    }
    let name = scope === 'project' ? 'project' : scope === 'report' ? 'report' : explicit || mirror.spec || 'current';
    if (publishing) return { text: t('Uma publicação já está em andamento.','A publication is already in progress.') };
    publishing = true;
    const finish = message => ({text:message});
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
      if (prepared.pending) return finish(t(`Publicação ${prepared.deployment_id} aceita e ainda em processamento. Repita /mustard:pages ${scope}${scope === 'spec' ? ' ' + name : scope === 'report' ? ' '+JSON.stringify(report) : ''} para consultar a mesma publicação.`,`Deployment ${prepared.deployment_id} accepted and still processing. Repeat /mustard:pages ${scope}${scope === 'spec' ? ' ' + name : scope === 'report' ? ' '+JSON.stringify(report) : ''} to check the same deployment.`));
      return finish(t(`Snapshot ${prepared.snapshot_id} preparado. Página: ${prepared.page}. Banco: ${prepared.database}. Os arquivos permanecem locais; ainda não foram publicados. ${prepared.hint || prepared.reason || ''}`,`Snapshot ${prepared.snapshot_id} prepared. Page: ${prepared.page}. Database: ${prepared.database}. Files remain local; not published yet. ${prepared.reason || ''}`));
    } catch {
      return finish(t('Não foi possível consultar o resultado. Nenhuma publicação foi confirmada no painel.','Could not read the result. No publication was confirmed in the panel.'));
    } finally {
      publishing = false;
    }
}
/** The answer of a command hook that failed: text, so nothing is handed to the model. */
export function commandFailed() {
  return {text:t('O módulo do Mustard não concluiu o comando. Nada foi executado; confira /plugin e /status.',
    'The Mustard module did not complete the command. Nothing was executed; check /plugin and /status.')};
}
export function register(on) {
  on('session.start', async ($, e, next) => {
    resetEvidence();
    const saved=await restorePresentation($);
    if(saved){await setPanel($,{...initialPanelState(),...saved.state});panelWanted=saved.wanted===true;}
    await $.tool.register({name:'search',description:'Use Mustard for project code searches and reads. Preserve original arguments and scope. Provide intent (local resource/provider + operation, without repeating the whole project goal) and purpose. locate returns occurrences; understand/spec/implement/validate investigate current source. Once a declaration is found, ask for its scoped evidence instead of opening its whole file. Reuse complete bodies; expand indicated missing ranges before relying on omitted code. Read preserves the requested offset/limit and result. Additional candidates/tests have native expansion commands. Investigative Grep/rg/Glob requests permit optional Jev for unresolved source-backed responsibility alternatives when the project enables it; choose=false opts out. Literal lookup and exact reads stay native. The adapter supplies the contract version.',
      inputSchema:searchSchema});
    const result=await next(e);
    if(panelWanted)await openPanel($);
    return result;
  });
  // The two commands are the plugin's files commands/mods.md and pages.md,
  // which the host names `mustard:<file>`. A registered command cannot carry
  // the `:`. Answering here runs no command, so the file never reaches the
  // model; a hook that fails would hand it over, and the catch answers instead.
  on('command.run', { command: 'mustard:mods' }, async ($, e) => {
    if (e.args.trim()==='--diagnose') return panelDiagnostics($);
    return openPanel($, e.args.trim() || undefined, true);
  }).catch(commandFailed);
  on('command.run', { command: 'mustard:pages' }, async ($,e)=>publishPage($,e.args)).catch(commandFailed);
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
  // Observe failed operations for the diagnostics, without replacing
  // permissions or the native orchestration. No tool reads the panel again.
  on('tool.call', async ($, e, next) => {
    const result=await next(e);
    if(result?.isError || result?.deny)void diagnostic($,'tool',e.tool,result.deny?'tool-denied':'tool-failed');
    return result;
  });
  // The end of the conductor's turn rereads the visible screen; an agent's
  // turn only records its measure, and its stop rereads.
  on('turn.complete', async ($, e, next) => {
    const result=await next(e);
    if(e.agentId){await recordMeasurement($,hostReceipt(e));analysisAgents.delete(e.agentId);}
    else void refresh($);
    return result;
  });
  on('session.compact', async ($, e, next) => {
    resetEvidence();
    return next(e);
  });
  on('classic.SubagentStop', async ($, e, next) => {
    const result=await next(e);
    void refresh($);
    return result;
  });
  on('session.measure', async ($, e, next) => {
    usage = {...usage,...e};
    await recordMeasurement($, hostReceipt(e));
    if (opened) $.ui.invalidate('ui.render');
    return next(e);
  });
  on('ui.close', { id }, async ($, e, next) => {
    panelEvent('close',e.origin?.kind);
    // Local closes already call stopPanel before the API. A delayed event
    // from the preceding pane must not stop the replacement's reads.
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
    cache={};mirror=initialPanelState();
    project=usage=problem=sessionId=undefined;
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
    // Stop reading while detached; retain the user's open request.
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
    return result;
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
    const result=yield* next(analysis?{...e,effort:'medium'}:e);
    if(analysis)analysis.input=fullAnalysisInput(result.usage);
    if(result.usage)await recordMeasurement($,hostReceipt(e,result));
    return result;
  });
  // The element holding the pane's focus ring: the terminal's list marks its row.
  on('ui.focus',{requestId:id},async ($,e,next)=>{
    const result=await next(e);
    if(!result?.deny && focused!==e.element){focused=e.element;$.ui.invalidate('ui.render');}
    return result;
  });
  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== id) return next(e);
    // Observe every opening independently, without logging each redraw.
    const requestIndex=panelEvents.findLastIndex(event=>event.kind==='open-request');
    if(!panelEvents.slice(requestIndex+1).some(row=>row.kind==='render' && row.surface===e.surface))panelEvent('render',e.surface);
    const state=await panel($);
    // A screen shows only the answer read for what it shows now.
    const screens=Object.fromEntries(Object.entries(cache).filter(([screen,held])=>screen!==state.screen || held.identity===identityOf(state)).map(([screen,held])=>[screen,held.value]));
    return renderPanel({elements:await $.ui.resolve(e),surface:e.surface,columns:e.props.bodyColumns ?? e.viewport?.columns,
      state,screens,project,usage,settings,measuring,publication,problem,theme:themeName,focused,actions:{
        go:(screen,patch)=>navigate($,screen,patch),
        back:()=>goBack($),
        openItem:code=>navigate($,'item',{item:code}),
        setKind:kind=>reread($,{kind,rows:PAGE,query:'',filter:'all',searching:false}),
        setFilter:filter=>reread($,{filter,rows:PAGE}),
        cycleFilter:async ()=>{const now=await panel($),order=now.kind==='task'?FILTERS:['all','attention'];
          return reread($,{filter:order[(order.indexOf(now.filter)+1)%order.length],rows:PAGE});},
        openSearch:()=>setPanel($,{searching:true}),
        search:query=>reread($,{query:String(query ?? '').trim(),rows:PAGE,searching:false}),
        more:()=>showMore($),
        pageWaves:offset=>reread($,{waveOffset:Math.max(0,offset)}),
        toggleWaveRequest:()=>setPanel($,now=>({waveRequest:!now.waveRequest})),
        toggleSpecs:()=>setPanel($,now=>({pickSpec:!now.pickSpec})),
        toggleDelivered:()=>setPanel($,now=>({delivered:!now.delivered})),
        selectSpec:async name=>{
          ticket++;cache={};
          await setPanel($,{spec:name,item:null,rows:PAGE,query:'',filter:'all',waveOffset:0,pickSpec:false,history:false,waveRequest:false});
          if((await panel($)).screen==='overview')return navigate($,'spec');
          return refresh($);
        },
        measure:()=>measureNow($),
        publish:()=>publishFromPanel($),
        refresh:()=>refresh($,{force:true}),
        editSetting:(field,value)=>{
          if(!settings?.loaded || settings.busy)return;
          const changes={...settings.changes};
          if(value===settings.values[field])delete changes[field];else changes[field]=value;
          settings={...settings,changes,error:undefined,message:undefined};$.ui.invalidate('ui.render');
        },
        reloadSettings:()=>settingsOperation($),
        saveSettings:()=>settingsOperation($,'save'),
        close:async()=>{panelWanted=false;await persist($);stopPanel();await $.ui.close({id});},
      }});
  });
}
