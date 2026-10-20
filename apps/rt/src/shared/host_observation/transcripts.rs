//! Private host-provided transcript locations, bound to an exact dispatch.
//! Never exported to the panel, never treated as approval or a verified fact.
use super::{digest, token};
use mustard_core::{domain::{model::contract::HookInput, spec_events::{SpecEvent, SpecLog}, wave_prompt::ReadDispatch}, io::{fs, transcript}};
use serde_json::{Value, json};
use std::{io::Read, path::{Path, PathBuf}};

pub(crate) struct Measured {
    pub usage:transcript::Usage,
    pub steps:Option<u64>,
    /// Um despacho anterior da onda ficou sem registro e contou como não
    /// recebido. Quem tem outra testemunha dos agentes da onda a prefere.
    pub skipped:bool,
}

fn directory(root:&Path)->PathBuf { root.join(".claude/mustard/host-transcripts") }
fn inside(root:&Path,path:&Path)->bool {
    let mut ancestor=path.to_path_buf();
    while !ancestor.exists() { if !ancestor.pop() {return false;} }
    ancestor.canonicalize().ok().zip(root.canonicalize().ok()).is_some_and(|(p,r)|p.starts_with(r))
}
fn read(path:&Path)->Option<Value> {
    if path.symlink_metadata().ok()?.file_type().is_symlink() || path.metadata().ok()?.len()>32_768 {return None;}
    serde_json::from_str(&fs::lock::read_shared(path).ok()?).ok()
}
fn opening(path:&Path)->Option<(String,String)> {
    let mut bytes=Vec::new();
    std::fs::File::open(path).ok()?.take(262_144).read_to_end(&mut bytes).ok()?;
    for line in String::from_utf8_lossy(&bytes).lines() {
        let Ok(row)=serde_json::from_str::<Value>(line) else {continue};
        let message=if row["type"]=="response_item" {&row["payload"]} else {&row["message"]};
        if message["role"]!="user" {continue;}
        let content=&message["content"];
        let text=content.as_str().map(str::to_string).unwrap_or_else(||content.as_array().into_iter().flatten().filter_map(|p|p["text"].as_str()).collect::<Vec<_>>().join("\n"));
        if ReadDispatch::of(&text).is_some() || heading(&text).is_some() {
            return Some((text,row["timestamp"].as_str()?.into()));
        }
    }
    None
}

/// A régua única que reconhece a onda que um texto despacha — a do despacho,
/// a da mensagem ao agente aberto, a da conversa do agente e a da ligação
/// dele: o título do pedido, no idioma do projeto, e, sem título, o comando
/// que lê o pedido com a raiz das specs deste projeto. O pedido no idioma que
/// o projeto não usa e o comando de outra raiz não são onda deste projeto.
pub(crate) fn wave_named(root:&Path,text:&str)->Option<ReadDispatch> {
    let project=crate::commands::spec_events::project(root);
    let shown=mustard_core::io::wave_prompt::shown(&project.root);
    match mustard_core::domain::wave_prompt::wave_of_title(mustard_core::io::transcript::heading_of(text),project.lang) {
        Some((spec,wave))=>Some(ReadDispatch {spec,wave,root:shown}),
        None=>ReadDispatch::of(text).filter(|read|read.root==shown),
    }
}

fn heading(text:&str)->Option<(String,u64)> {
    let line=mustard_core::io::transcript::heading_of(text);
    use mustard_core::platform::i18n::Locale;
    mustard_core::domain::wave_prompt::wave_of_title(line,Locale::PtBr)
        .or_else(||mustard_core::domain::wave_prompt::wave_of_title(line,Locale::EnUs))
}

/// Um agente já ligado pela observação do hospedeiro ao despacho `dispatch`
/// da onda `wave` da spec `spec`: a prova de que ele nasceu, mesmo quando a
/// gravação do lançamento no envio falhou.
pub(crate) fn dispatch_has_agent(root:&Path,spec:&str,wave:u64,dispatch:u64)->bool {
    let folder=directory(root);
    if !inside(root,&folder) {return false;}
    let Ok(entries)=std::fs::read_dir(folder) else {return false;};
    entries.flatten().take(4096).filter_map(|entry|read(&entry.path())).any(|record| record["spec"]==spec && record["wave"]==wave && record["dispatch"]==dispatch)
}

/// O envio nativo a que a ferramenta de um agente de onda pertence.
pub(crate) struct Binding {
    pub(crate) spec: String,
    pub(crate) wave: u64,
    pub(crate) dispatch: u64,
    /// O envio da onda, já lido: quem confere a cópia ou o procedimento usa
    /// este, sem ler o arquivo de eventos de novo.
    pub(crate) send: SpecEvent,
}

/// O envio ligado à ferramenta do agente. A ligação guarda o envio e o ponto
/// do arquivo de eventos até onde o leu ([`refresh_bound`]): a ferramenta
/// seguinte lê só as linhas que o arquivo ganhou depois, e o arquivo inteiro
/// só quando uma delas pode mexer no envio da onda.
pub(crate) fn tool_binding(root:&Path,host:&str,input:&HookInput)->Option<Binding> {
    let session=input.session_id.as_deref()?;
    let agent=input.agent_id.as_deref()?;
    let record=read(&directory(root).join(format!("{host}-{}-{}.json",digest(session),digest(agent))))?;
    let spec=record["spec"].as_str()?;
    let wave=record["wave"].as_u64()?;
    let dispatch=record["dispatch"].as_u64()?;
    let file=mustard_core::io::spec_events::spec_file(root,spec).ok()?;
    let send=match kept(&file,&record) {
        Some((send,_))=>send?,
        None=>bound_send(&file,wave,dispatch)?.0?,
    };
    Some(Binding{spec:spec.into(),wave,dispatch,send})
}

/// Quantos bytes antes do ponto lido entram na impressão dele.
const MARK_SPAN:usize=256;
/// Acima disto de linhas novas, a leitura inteira sai mais simples.
const ADDED_LIMIT:u64=4*1024*1024;
/// Os campos do envio que a ligação guarda: os que quem confere a cópia e o
/// procedimento lê. O texto do pedido fica no arquivo de eventos.
const BOUND_FIELDS:&[&str]=&["v","id","type","at","wave","copy","host","skills","resends","replaces"];

fn fingerprint(bytes:&[u8])->String {
    let mut hash=mustard_core::io::sha256::Sha256::new();
    hash.update(&bytes[bytes.len().saturating_sub(MARK_SPAN)..]);
    hash.hex_digest()
}

/// A leitura inteira do arquivo de eventos `file`: o envio da onda `wave`
/// quando ele ainda despacha na posição `dispatch`, e o ponto até onde o
/// arquivo foi lido.
fn bound_send(file:&Path,wave:u64,dispatch:u64)->Option<(Option<SpecEvent>,Value)> {
    let content=fs::lock::read_shared(file).ok()?;
    let log=mustard_core::domain::spec_events::parse_log(&content);
    let send=log.last_by_wave("send").get(&wave).copied().filter(|id|log.dispatch_position(*id)==dispatch).and_then(|id| {
        // A versão nova do envio não repete o pedido: as skills vêm da versão
        // que as gravou.
        let mut send=log.get(id).cloned()?;
        if let Some(skills)=log.send_field(id,"skills") {send.fields.insert("skills".into(),skills.clone());}
        Some(send)
    });
    Some((send,json!({"len":content.len(),"tail":fingerprint(content.as_bytes())})))
}

/// O envio guardado na ligação de `record`, conferido só pelas linhas que o
/// arquivo de eventos `file` ganhou desde o ponto guardado, com o ponto novo.
/// `Some(None)` é a ligação que já sabia que o despacho passou. `None` quando
/// só a leitura inteira responde: sem ligação guardada, arquivo regravado
/// (a impressão antes do ponto mudou) ou linha nova que pode mexer no envio
/// da onda — outro envio dela, uma remoção, um expurgo.
fn kept(file:&Path,record:&Value)->Option<(Option<SpecEvent>,Value)> {
    use std::io::{Seek, SeekFrom};
    let bound=&record["bound"];
    if bound["key"]!=json!([record["spec"],record["wave"],record["dispatch"]]) {return None;}
    let wave=record["wave"].as_u64()?;
    let (len,tail)=(bound["len"].as_u64()?,bound["tail"].as_str()?);
    let send=match &bound["send"] {
        Value::Null=>None,
        fields=>Some(mustard_core::domain::spec_events::parse_log(&fields.to_string()).events.into_iter().next()?),
    };
    let mut handle=std::fs::File::open(file).ok()?;
    let size=handle.metadata().ok()?.len();
    if size<len || size-len>ADDED_LIMIT {return None;}
    let start=len.saturating_sub(MARK_SPAN as u64);
    handle.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes=Vec::new();
    handle.take(size-start).read_to_end(&mut bytes).ok()?;
    let before=usize::try_from(len-start).ok()?;
    if bytes.len()<before || fingerprint(&bytes[..before])!=tail {return None;}
    // Só as linhas inteiras: a que outra gravação ainda escreve fica para a
    // ferramenta seguinte.
    let added=&bytes[before..];
    let complete=added.iter().rposition(|byte|*byte==b'\n').map_or(0,|end|end+1);
    for line in added[..complete].split(|byte|*byte==b'\n').filter(|line|!line.iter().all(u8::is_ascii_whitespace)) {
        let event:Value=serde_json::from_slice(line).ok()?;
        let kind=event["type"].as_str()?;
        if matches!(kind,"remove"|"purge") || (kind=="send" && event["wave"].as_u64()==Some(wave)) {return None;}
    }
    let end=before+complete;
    Some((send,json!({"len":len+complete as u64,"tail":fingerprint(&bytes[..end])})))
}

/// Guarda em `record` o envio do despacho ligado e o ponto do arquivo de
/// eventos até onde ele foi lido, para a ferramenta seguinte do agente não
/// reler o arquivo. A ligação que não cabe no registro não fica guardada.
fn refresh_bound(root:&Path,record:&mut Value) {
    let fresh=(|| {
        let (spec,wave,dispatch)=(record["spec"].as_str()?,record["wave"].as_u64()?,record["dispatch"].as_u64()?);
        let file=mustard_core::io::spec_events::spec_file(root,spec).ok()?;
        kept(&file,record).or_else(||bound_send(&file,wave,dispatch))
    })();
    let Some(object)=record.as_object_mut() else {return};
    object.remove("bound");
    if let Some((send,mut bound))=fresh {
        bound["send"]=send.map_or(Value::Null,|send|Value::Object(send.fields.into_iter().filter(|(field,_)|BOUND_FIELDS.contains(&field.as_str())).collect()));
        bound["key"]=json!([object.get("spec"),object.get("wave"),object.get("dispatch")]);
        object.insert("bound".into(),bound);
    }
    if record.to_string().len()>30_000 && let Some(object)=record.as_object_mut() {object.remove("bound");}
}

/// O que o texto de um despacho diz do envio de uma onda.
enum Named {
    /// O texto não nomeia onda nenhuma.
    Nothing,
    /// Nomeia uma onda, e não confere com o envio dela.
    Refused,
    /// Confere com o envio da onda, que despachou na posição `dispatch`;
    /// `launched` diz se o envio ficou marcando o lançamento.
    Send { identity: ReadDispatch, dispatch: u64, launched: bool },
}

/// O envio da onda que `text` despacha, conferido: o comando que lê o pedido
/// ou o pedido inteiro com o título, a raiz deste projeto, o hospedeiro do
/// envio e, com a conversa do agente (`initial`), a hora dela depois do
/// envio. Quem chama já tem a prova de que o agente existe, e o envio passa a
/// marcar o lançamento ([`crate::commands::flow::round::queue::record_launch`]);
/// a gravação que falhou volta como `launched` falso, para a chamada seguinte
/// do agente tentar de novo.
fn named_send(root:&Path,host:&str,text:&str,initial:Option<&(String,String)>)->Named {
    let Some(identity)=wave_named(root,text) else {
        return if ReadDispatch::of(text).is_some() {Named::Refused} else {Named::Nothing};
    };
    let Some(log)=mustard_core::io::spec_events::spec_file(root,&identity.spec).ok().and_then(|p|mustard_core::io::spec_events::read(&p).ok().flatten()) else {return Named::Refused;};
    let Some(send)=log.last_by_wave("send").get(&identity.wave).and_then(|id|log.get(*id)) else {return Named::Refused;};
    // The title alone is never enough. A native rewritten prompt must
    // contain the authoritative dispatch, not a similarly named task.
    if ReadDispatch::of(text).is_none() {
        let Some(expected)=log.send_text(send.id).filter(|s|!s.trim().is_empty()) else {return Named::Refused;};
        if !text.trim().strip_prefix(expected.trim()).is_some_and(|rest|rest.is_empty() || rest.starts_with('\n')) {return Named::Refused;}
    }
    if send.str_field("host").is_some_and(|h|h!=host) {return Named::Refused;}
    if let Some((_,stamp))=initial {
        let time=chrono::DateTime::parse_from_rfc3339(stamp).ok();
        let origin = log.get(log.dispatch_position(send.id)).unwrap_or(send);
        let sent=chrono::DateTime::parse_from_rfc3339(origin.at()).ok();
        if time.zip(sent).is_none_or(|(at,since)|at<since) {return Named::Refused;}
    }
    let dispatch=log.dispatch_position(send.id);
    let launched=crate::commands::flow::round::queue::agent_launched(&log,send.id)
        || crate::commands::flow::round::queue::record_launch(root,&identity.spec,identity.wave,dispatch);
    Named::Send {identity,dispatch,launched}
}

pub(super) fn observe(root:&Path,host:&str,event:&str,input:&HookInput) {
    if !["SubagentStart", "SubagentStop", "PreToolUse","PostToolUse"].contains(&event) {return;}
    let Some(session)=input.session_id.as_deref().filter(|s|!s.is_empty()) else {return;};
    let agent=input.agent_id.as_deref().or_else(||input.raw.pointer("/tool_response/agent_id").or_else(||input.raw.pointer("/tool_response/agentId")).and_then(Value::as_str)).filter(|s|token(s));
    // Native tool completion provides exact request -> agent identity. The stop
    // event supplies its real file even when the host stores it elsewhere.
    let prompt=input.tool_input.get("prompt").or_else(||input.tool_input.get("message")).and_then(Value::as_str);
    let Some(agent)=agent else {
        // O despacho nativo que terminou prova que o agente da onda nasceu,
        // mesmo quando a resposta dele não traz o número do agente.
        if event=="PostToolUse" && matches!(input.tool_name.as_deref(),Some("Agent"|"Task")) && let Some(text)=prompt {
            named_send(root,host,text,None);
        }
        return;
    };
    let folder=directory(root);
    if !inside(root,&folder) {return;}
    let path=folder.join(format!("{host}-{}-{}.json",digest(session),digest(agent)));
    let Some(mut lock)=fs::lock::LockedFile::exclusive_if_free(&path).ok().flatten() else {return;};
    let mut record:Value=lock.read_to_string().ok().filter(|s|s.len()<=32_768).and_then(|s|serde_json::from_str(&s).ok()).unwrap_or_else(||json!({"host":host}));
    let explicit =input.raw.get("agent_transcript_path").and_then(Value::as_str).filter(|s|s.len()<=4096).map(PathBuf::from);
    // Claude's versioned local transcript layout supplies the dispatch before
    // the first write, rather than waiting until SubagentStop. Unknown hosts
    // keep their explicit adapter; no directory-wide transcript scan is done.
    let transcript = explicit.or_else(|| {
        (host == "claude" && input.agent_id.is_some())
            .then(|| {
                let parent = Path::new(input.transcript_path()?);
                Some(parent.with_extension("").join("subagents").join(format!("agent-{}.jsonl", agent.strip_prefix("agent-").unwrap_or(agent))))
            })
            .flatten()
    });
    // O registro já ligado ao despacho não relê a conversa, salvo quando a
    // gravação do lançamento falhou: a chamada seguinte tenta de novo.
    let initial = if event != "SubagentStop" && prompt.is_none() && record["dispatch"].as_u64().is_some() && record["launched"]!=json!(false) {
        None
    } else {
        transcript.as_deref().and_then(opening)
    };
    let text=prompt.or_else(||initial.as_ref().map(|p|p.0.as_str()));
    // A sessão do agente trocou no meio da onda: o registro desta sessão nasce
    // sem despacho. Quando o mesmo agente relê o pedido da onda, ele volta ao
    // despacho que já tinha noutra sessão, se esse despacho é o atual.
    if record["dispatch"].as_u64().is_none() {
        let reread=input.tool_input.get("command").and_then(Value::as_str).and_then(ReadDispatch::of);
        if let Some((identity,dispatch))=reread.and_then(|identity|rebound(root,&folder,host,agent,&path,&identity).map(|dispatch|(identity,dispatch))) {
            record["spec"]=json!(identity.spec); record["wave"]=json!(identity.wave); record["dispatch"]=json!(dispatch);
        }
    }
    match text.map_or(Named::Nothing,|text|named_send(root,host,text,initial.as_ref())) {
        Named::Nothing => {}
        Named::Refused => return,
        Named::Send {identity,dispatch,launched} => {
            record["spec"]=json!(identity.spec); record["wave"]=json!(identity.wave);
            record["dispatch"]=json!(dispatch); record["launched"]=json!(launched);
        }
    }
    refresh_bound(root,&mut record);
    if let Some(transcript)=transcript.filter(|path| path.is_file()) {record["path"]=json!(transcript);}
    if let Some(model)=input.raw.get("model").and_then(Value::as_str).filter(|s|token(s)) {record["model"]=json!(model);}
    record["at"]=json!(chrono::Utc::now().to_rfc3339());
    let _=lock.replace(record.to_string().as_bytes());
}

/// O despacho atual da onda que `identity` relê, quando outro registro do
/// mesmo `agent` no mesmo `host`, de outra sessão, já está preso a ele.
fn rebound(root:&Path,folder:&Path,host:&str,agent:&str,own:&Path,identity:&ReadDispatch)->Option<u64> {
    if identity.root!=mustard_core::io::wave_prompt::shown(root) {return None;}
    let log=mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root,&identity.spec).ok()?).ok()??;
    let current=log.dispatch_position(log.last_by_wave("send").get(&identity.wave).copied()?);
    let (prefix,suffix)=(format!("{host}-"),format!("-{}.json",digest(agent)));
    let entries=std::fs::read_dir(folder).ok()?.collect::<Result<Vec<_>,_>>().ok()?;
    if entries.len()>4096 {return None;}
    entries.iter().map(std::fs::DirEntry::path).filter(|path| path!=own && path.file_name().and_then(|name|name.to_str())
        .is_some_and(|name|name.starts_with(&prefix) && name.ends_with(&suffix)))
        .filter_map(|path|read(&path))
        .any(|record|record["spec"]==json!(identity.spec) && record["wave"]==json!(identity.wave) && record["dispatch"].as_u64()==Some(current))
        .then_some(current)
}

/// Só os registros do gancho de cada despacho da onda; formato desconhecido
/// ou ausente fica desconhecido. O último despacho é o do agente que
/// entregou: sem registro dele a onda fica sem medição, e o consumo das
/// tentativas anteriores nunca passa pelo da onda. Um despacho anterior sem
/// registro conta como não recebido — a onda foi reenviada antes de nascer
/// agente dele — e a medição diz que o pulou ([`Measured::skipped`]).
pub(crate) fn wave_usage(root:&Path,spec:&str,log:&SpecLog,wave:u64)->Option<Measured> {
    let folder=directory(root);
    if !inside(root,&folder) {return None;}
    let entries=std::fs::read_dir(folder).ok()?.collect::<Result<Vec<_>,_>>().ok()?;
    if entries.len()>4096 {return None;}
    let sends:std::collections::BTreeSet<_>=log.events.iter().filter(|e|e.event_type=="send" && e.wave()==Some(wave)).map(|e|log.dispatch_position(e.id)).collect();
    let mut found=std::collections::BTreeMap::<u64,Vec<Value>>::new();
    let all_records=entries.iter().filter_map(|entry|read(&entry.path())).collect::<Vec<_>>();
    for record in &all_records {
        if record["spec"]!=spec || record["wave"]!=wave {continue;}
        let dispatch=record["dispatch"].as_u64()?;
        if sends.contains(&dispatch) {found.entry(dispatch).or_default().push(record.clone());}
    }
    if !found.contains_key(sends.last()?) {return None;}
    let skipped=found.len()<sends.len();
    let mut total=transcript::Usage::default();
    let mut claude_files=std::collections::BTreeSet::new();
    let mut codex_files=std::collections::BTreeSet::new();
    for records in found.values() {for record in records {
        let path=PathBuf::from(record["path"].as_str()?);
        if all_records.iter().any(|other| {
                other["path"]==record["path"] && (other["spec"]!=spec || other["wave"]!=wave || other["dispatch"]!=record["dispatch"])
            }) {return None;}
        match record["host"].as_str()? {
            "claude"=> {claude_files.insert(path);}
                "codex"=> {codex_files.insert(path); if let Some(model)=record["model"].as_str() {total.model=Some(model.into());}}
                _=>return None,
        }
    }}
    // Sharing a rollout across waves is not independently attributable.
    let has_codex=!codex_files.is_empty();
    let has_claude=!claude_files.is_empty();
    for path in codex_files {
        let value=super::codex_usage(&path)?;
        let input=value["input_tokens"].as_u64()?;
        let cached=value["cached_input_tokens"].as_u64()?;
        let output=value["output_tokens"].as_u64()?;
        total.tokens=total.tokens.checked_add(value["total_tokens"].as_u64()?)?;
        total.breakdown.input_tokens=total.breakdown.input_tokens.checked_add(input.checked_sub(cached)?)?;
        total.breakdown.cache_read_input_tokens=total.breakdown.cache_read_input_tokens.checked_add(cached)?;
        total.breakdown.output_tokens=total.breakdown.output_tokens.checked_add(output)?;
    }
    let texts=claude_files.iter().map(std::fs::read_to_string).collect::<Result<Vec<_>,_>>().ok()?;
    if !texts.is_empty() {
        let measured=transcript::usage_of(texts.iter().flat_map(|s|s.lines()));
        if measured.model.is_none() && measured.tokens==0 {return None;}
        total.model=measured.model; total.steps=measured.steps;
        total.tokens=total.tokens.checked_add(measured.tokens)?; total.breakdown.combine(&measured.breakdown);
    }
    if has_codex && has_claude {total.model=None;}
    let steps=(!has_codex).then_some(total.steps);
    (total.tokens>0 || total.model.is_some()).then_some(Measured {usage:total,steps,skipped})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_tool_binds_the_native_request_from_the_agents_local_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let path = mustard_core::io::spec_events::spec_file(root, "work").unwrap();
        let text = format!("mustard-rt run read request-1 --root {} --spec work", mustard_core::io::wave_prompt::shown(root));
        mustard_core::io::spec_events::write(
            &path,
            "send",
            json!({"wave":1,"role":"wave","lines":1,"chars":text.len(),"mustard":"test","host":"claude","text":text}).as_object().unwrap().clone(),
            &[],
        )
        .unwrap();
        let parent = root.join("session.jsonl");
        let transcript = root.join("session/subagents/agent-worker.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(
            &transcript,
            json!({"type":"user","timestamp":chrono::Utc::now().to_rfc3339(),"message":{"role":"user","content":text}}).to_string(),
        )
        .unwrap();
        let input = serde_json::from_value(json!({"session_id":"session","agent_id":"worker","transcript_path":parent,"tool_name":"Write"})).unwrap();
        observe(root, "claude", "PreToolUse", &input);
        assert!(tool_binding(root, "claude", &input).is_some());
    }
    /// A ferramenta do agente já ligado não relê o arquivo de eventos: o
    /// envio guardado na ligação vale enquanto o arquivo só ganha linhas que
    /// não mexem no envio da onda. O reenvio da onda desfaz a ligação.
    #[test]
    fn a_bound_agent_tool_does_not_reread_the_event_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let path = mustard_core::io::spec_events::spec_file(root, "work").unwrap();
        let send = |wave: u64, extra: Value| {
            let text = format!("mustard-rt run read request-{wave} --root {} --spec work", mustard_core::io::wave_prompt::shown(root));
            let mut fields = json!({"wave":wave,"role":"wave","lines":1,"chars":text.len(),"mustard":"test","host":"claude","text":format!("{text}\n{}","x".repeat(400)),"copy":"/copies/work/a"});
            fields.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            mustard_core::io::spec_events::write(&path, "send", fields.as_object().unwrap().clone(), &[]).unwrap()
        };
        let first = send(1, json!({}));
        send(2, json!({}));
        let command = format!("mustard-rt run read request-1 --root {} --spec work", mustard_core::io::wave_prompt::shown(root));
        let started: HookInput = serde_json::from_value(json!({"session_id":"s","tool_name":"Agent","tool_input":{"prompt":command},"tool_response":{"agent_id":"worker"}})).unwrap();
        observe(root, "claude", "PostToolUse", &started);
        let tool: HookInput = serde_json::from_value(json!({"session_id":"s","agent_id":"worker","tool_name":"Write"})).unwrap();
        assert_eq!(tool_binding(root, "claude", &tool).unwrap().send.str_field("copy"), Some("/copies/work/a"));
        // A linha do envio vira espaços, do mesmo tamanho: só quem relê o
        // arquivo inteiro deixa de achar o envio.
        let content = std::fs::read_to_string(&path).unwrap();
        let line = content.lines().find(|line| line.contains(&format!("\"id\":{}", first.id)) && line.contains("\"type\":\"send\"")).unwrap();
        std::fs::write(&path, content.replace(line, &" ".repeat(line.len()))).unwrap();
        observe(root, "claude", "PreToolUse", &tool);
        assert!(tool_binding(root, "claude", &tool).is_some(), "the bound tool read the whole event file again");
        send(2, json!({"resends":2}));
        assert!(tool_binding(root, "claude", &tool).is_some(), "a line of another wave keeps the binding");
        send(1, json!({"resends":first.id}));
        assert!(tool_binding(root, "claude", &tool).is_none(), "a resend of the wave undoes the binding");
    }
    #[test]
    fn full_rewritten_claude_prompts_require_canonical_content_not_a_matching_title() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let path=mustard_core::io::spec_events::spec_file(root,"work").unwrap();
        let text=format!("{}\n\nCanonical bounded task.",mustard_core::domain::wave_prompt::wave_title("work",1,mustard_core::platform::i18n::Locale::PtBr));
        mustard_core::io::spec_events::write(&path,"send",json!({"wave":1,"role":"wave","lines":3,"chars":text.len(),"mustard":"test","host":"claude","text":text}).as_object().unwrap().clone(),&[]).unwrap();
        let log=mustard_core::io::spec_events::read(&path).unwrap().unwrap();
        let rollout=root.join("nonstandard-agent.jsonl");
        let save=|prompt:&str| {
            std::fs::write(&rollout,format!("{}\n{}\n",json!({"type":"user","timestamp":chrono::Utc::now().to_rfc3339(),"message":{"role":"user","content":prompt}}),json!({"type":"assistant","message":{"id":"a1","model":"claude-sonnet-test","usage":{"input_tokens":10,"cache_read_input_tokens":20,"output_tokens":5}}}))).unwrap();
        };
        let input=serde_json::from_value(json!({"cwd":root,"hook_event_name":"SubagentStop","session_id":"s","agent_id":"a","agent_transcript_path":rollout}),
        ).unwrap();
        save(&text.replace("Canonical bounded task.","A different task."));
        let _=crate::dispatch::run_event(Some(mustard_core::domain::model::contract::Trigger::SubagentStop),&input);
        assert!(wave_usage(root,"work",&log,1).is_none());
        save(&text);
        let _=crate::dispatch::run_event(Some(mustard_core::domain::model::contract::Trigger::SubagentStop),&input);
        assert_eq!(wave_usage(root,"work",&log,1).unwrap().usage.tokens,35);
    }
    #[test]
    fn native_stop_locates_nonstandard_rollout_and_never_reuses_it_for_a_resend() {
        let dir=tempfile::tempdir().unwrap(); let root=dir.path();
        std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let path=mustard_core::io::spec_events::spec_file(root,"work").unwrap();
        let command=format!("mustard-rt run read request-1 --root {} --spec work",mustard_core::io::wave_prompt::shown(root));
        let written=mustard_core::io::spec_events::write(&path,"send",json!({"wave":1,"role":"wave","lines":1,"chars":command.len(),"mustard":"test","host":"codex","text":command}).as_object().unwrap().clone(),&[]).unwrap();
        let log=mustard_core::io::spec_events::read(&path).unwrap().unwrap();
        let transcript=root.join("native-thread.jsonl");
        std::fs::write(&transcript,format!("{}\n{}\n",json!({"timestamp":chrono::Utc::now().to_rfc3339(),"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":command}]}}),json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":20,"total_tokens":120}}}}))).unwrap();
        let input=serde_json::from_value(json!({"cwd":root,"hook_event_name":"SubagentStop","session_id":"s","agent_id":"agent","agent_transcript_path":transcript})).unwrap();
        crate::host::codex("SubagentStop",&input);
        let measured=wave_usage(root,"work",&log,1).unwrap();
        assert!(measured.steps.is_none());let usage=measured.usage;
        assert_eq!(usage.tokens,120); assert_eq!(usage.breakdown.input_tokens,20); assert_eq!(usage.breakdown.cache_read_input_tokens,80);
        let panel=crate::commands::flow::round::usage_snapshot(root,&log,None,None);
        assert_eq!(panel["waves"][0]["usage"]["tokens"],120);
        assert!(panel["waves"][0]["usage"].get("steps").is_none(),"the rollout does not expose tool steps");
        let second=directory(root).join("conflicting.json");
        let original=std::fs::read_dir(directory(root)).unwrap().find_map(|entry|read(&entry.ok()?.path())).unwrap();
        let mut conflicting=original.clone(); conflicting["wave"]=json!(2);
        std::fs::write(&second,conflicting.to_string()).unwrap();
        assert!(wave_usage(root,"work",&log,1).is_none(),"a shared rollout is not attributable to one wave");
        std::fs::remove_file(second).unwrap();
        let mut next=log.clone();
        let mut send=next.get(written.id).unwrap().clone(); send.id+=1; send.fields.insert("resends".into(),json!(written.id)); next.events.push(send);
        assert!(wave_usage(root,"work",&next,1).is_none());
        assert!(wave_usage(root,"other",&log,1).is_none());
    }
    /// A onda Codex saiu e foi reenviada antes de nascer agente do primeiro
    /// envio: só o agente do reenvio deixa registro, e o painel mostra o
    /// consumo dele.
    #[test]
    fn a_codex_resend_is_measured_when_the_earlier_send_left_no_record() {
        let dir=tempfile::tempdir().unwrap(); let root=dir.path();
        std::fs::write(root.join("mustard.json"),"{}").unwrap();
        let path=mustard_core::io::spec_events::spec_file(root,"work").unwrap();
        let command=format!("mustard-rt run read request-1 --root {} --spec work",mustard_core::io::wave_prompt::shown(root));
        let send=|extra:Value| {
            let mut fields=json!({"wave":1,"role":"wave","lines":1,"chars":command.len(),"mustard":"test","host":"codex","text":command});
            fields.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            mustard_core::io::spec_events::write(&path,"send",fields.as_object().unwrap().clone(),&[]).unwrap()
        };
        let first=send(json!({}));
        send(json!({"resends":first.id}));
        let log=mustard_core::io::spec_events::read(&path).unwrap().unwrap();
        let rollout=root.join("resent-thread.jsonl");
        std::fs::write(&rollout,format!("{}\n{}\n",json!({"timestamp":chrono::Utc::now().to_rfc3339(),"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":command}]}}),json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":20,"total_tokens":120}}}}))).unwrap();
        let input=serde_json::from_value(json!({"cwd":root,"hook_event_name":"SubagentStop","session_id":"s","agent_id":"agent","agent_transcript_path":rollout})).unwrap();
        crate::host::codex("SubagentStop",&input);
        let panel=crate::commands::flow::round::usage_snapshot(root,&log,None,None);
        assert_eq!(panel["waves"][0]["usage"]["tokens"],120,"{panel}");
    }
}
