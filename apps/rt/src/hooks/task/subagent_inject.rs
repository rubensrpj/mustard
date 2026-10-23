//! `subagent_inject` — o despacho de um agente, conferido pelo título da onda.
//!
//! O pedido canônico da onda é montado pelo Mustard, na rodada, e gravado no
//! envio dela. O despacho a um agente do Mustard (`PreToolUse` de `Task` ou
//! `Agent`) que abre com o título do pedido de uma onda sai só com o que a
//! rodada monta para ela: o título e o comando que lê o pedido. O texto a
//! mais do condutor é gravado no pedido antes do despacho; falha nessa
//! gravação impede o envio. O suplemento não muda escopo nem regras. O
//! despacho não diz que o agente começou: a permissão ainda pode negá-lo, e
//! quem grava o lançamento é a observação do hospedeiro, na primeira prova de
//! que o agente existe. O despacho que já é esse texto passa como veio, e o
//! título de uma onda que a spec não tem em andamento é barrado com o motivo.
//! A exceção é a onda cuja volta a rodada recusou depois de o Claude Code que
//! a mandou fechar, levando o agente dela, e a onda cuja volta quem conduz a
//! obra reprovou: ela sai a um agente novo, com o mesmo texto seguido do
//! trecho de conserto — na reprovação, o motivo de quem reprovou —, e ele
//! trabalha na mesma cópia. O despacho dele fica gravado na spec, e outro
//! agente novo é barrado enquanto o Claude Code que o abriu segue aberto: a
//! cópia nunca tem dois agentes. O título segue na primeira linha: é por ele
//! que a rodada acha a conversa do agente e soma o gasto da onda. A mensagem
//! do condutor a um agente de onda já aberto (`SendMessage`), depois de a
//! conferência depois da onda recusar a volta dele, sai com o trecho que a
//! rodada devolveu e as orientações adicionais gravadas; o agente de destino
//! é reconhecido pelo título da primeira mensagem da conversa dele. A
//! mensagem ao agente de uma onda cuja volta quem conduz a obra reprovou,
//! quando a conversa dele começou antes da volta reprovada, é barrada, com o
//! Claude Code do envio aberto ou fechado: a onda saiu dele, e o motivo vai
//! só ao agente novo. A mensagem a outro agente, ou sem trecho gravado para a
//! volta da onda, passa como veio.
//!
//! Um outro despacho a um agente do Mustard (`mustard-wave` ou
//! `mustard-review`), num projeto com `mustard.json`, ganha no topo a linha
//! dos dois idiomas do projeto, lida da configuração: é assim que os
//! consertos e as revisões cujo texto o orquestrador escreve recebem o idioma
//! dos nomes. O texto que já traz a linha, como o pedido da rodada,
//! passa como veio. O despacho ao `mustard-review` que o orquestrador
//! escreve — a revisão do levantamento e a do pull request — ganha no fim as
//! regras do projeto, os arquivos de regras da raiz e das pastas onde a obra
//! mexe, porque o revisor é instalado sem eles; o que manda ler o pedido da
//! revisão final não ganha, porque esse pedido já as traz. Todo despacho ao
//! `mustard-review` ganha no fim o caminho da cópia do desenho aprovado
//! quando a pasta da spec revisada tem uma. Qualquer outro
//! despacho é uma tarefa qualquer e também passa como veio: o gancho não
//! escolhe skill, não injeta memória, não monta pedido de onda e não avalia
//! a volta do agente. O pedido ao agente de exploração do Claude Code sobe
//! com a resposta curta do mapa no topo só quando a busca cravou; a resposta
//! parcial não vai. O lançamento do agente de onda sai com o modelo do último
//! envio dela, e o do revisor com o que o `mustard.json` declara para ele,
//! sempre no esforço `xhigh`, no lugar do que a chamada pede; o recado diz
//! quando a chamada pedia outro, e o envio da onda grava o esforço que valeu.
//! O revisor final, que lê o pedido gravado da revisão, sai no modelo que a
//! chamada pede, e o envio dessa revisão grava o modelo que valeu. O agente
//! novo de uma volta recusada ou reprovada sai um modelo acima.

use std::path::Path;

use mustard_core::domain::config::{AgentSettings, DEFAULT_AGENT_EFFORT, DEFAULT_AGENT_MODEL};
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_events::{Refusal, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState as _};
use mustard_core::domain::wave_prompt::{carries_project_rules, language_line, project_rules_section, wave_title};
use mustard_core::io::spec_events as store;
use mustard_core::io::transcript::agent_request;
use mustard_core::io::wave_prompt::{project_rules, touched_files};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use mustard_core::{AGENT_NAMES, ProjectConfig};
use serde_json::{Value, json};

use crate::commands::spec_events::write::record_locked;
use crate::hooks::write::write_gate::say;
use crate::shared::spec_state::DiskSpecState;
use crate::shared::word_search;

/// A ferramenta com que o condutor manda mais uma mensagem a um agente já
/// aberto: `to` é o agente, `message` o texto.
const SEND_MESSAGE: &str = "SendMessage";

/// O pedido do subagente, no despacho de um agente.
pub struct SubagentInject;

/// Os eventos da spec `spec` do projeto em `root`, ou o motivo de não os ter:
/// o nome que não é de spec e a spec sem arquivo de eventos.
fn spec_log(root: &Path, spec: &str, lang: Locale) -> Result<SpecLog, String> {
    let refused = |refusal: Refusal| refusal.message(lang);
    let path = store::spec_file(root, spec).map_err(refused)?;
    store::read(&path).map_err(refused)?.ok_or_else(|| refused(Refusal::NoSpecFile { spec: spec.to_string() }))
}

/// O despacho da onda `wave` da spec `spec` com o texto que a rodada monta
/// para ela ([`crate::commands::flow::round::wave_dispatch`]) no lugar do que
/// o condutor escreveu. A onda recusada cujo agente se foi com o Claude Code
/// que a mandou sai a um agente novo, um só ([`new_agent_fix`]), com o mesmo
/// texto seguido do trecho de conserto; `host` acha o hospedeiro que
/// despacha, gravado no envio para segurar a onda com o agente novo, e só é
/// chamado quando a onda não está em andamento. Fora isso, a onda que a spec
/// não tem em andamento é barrada.
fn wave_dispatch(input: &HookInput, start: &Path, spec: &str, wave: u64, host: fn() -> Option<(u32, u64)>) -> Verdict {
    let project = crate::commands::spec_events::project(start);
    let lang = project.lang;
    let log = match spec_log(&project.root, spec, lang) {
        Ok(log) => log,
        Err(reason) => return Verdict::Deny { reason },
    };
    let text = crate::commands::flow::round::wave_dispatch(&project.root, spec, wave, lang);
    if let Some(sent) = crate::commands::flow::round::waves_in_progress(&log).get(&wave).copied() {
        // A onda cujo agente já nasceu — pelo lançamento gravado no envio, ou
        // pela observação do hospedeiro quando essa gravação falhou — e deu
        // sinal de vida dentro do prazo não aceita um segundo agente na mesma
        // cópia.
        use crate::commands::flow::round::queue::{SILENT_AFTER_MINUTES, agent_known, silent_minutes};
        let alive = silent_minutes(&project.root, spec, wave, &log, sent).is_some_and(|minutes| minutes < SILENT_AFTER_MINUTES);
        if alive && agent_known(&project.root, spec, &log, wave, sent) {
            return Verdict::Deny { reason: say("subagent.wave_has_agent", lang, &[("{spec}", spec), ("{wave}", &wave.to_string())]) };
        }
        let recorded = log.last_by_wave("send").get(&wave).and_then(|id| log.send_text(*id));
        let base = recorded.filter(|request| dispatch_prompt(input).starts_with(request.trim_end())).unwrap_or(&text);
        let extra = supplemental_instructions(dispatch_prompt(input), base);
        if !extra.is_empty() {
            if !record_supplement(&project.root, spec, wave, &extra, lang) {
                return supplement_failure(lang);
            }
            return with_field(input, "prompt", text, Some(match lang {
                Locale::PtBr => format!("[Mustard] As instruções adicionais foram gravadas no pedido da onda {wave}, que o agente deve ler inteiro."),
                Locale::EnUs => format!("[Mustard] Additional instructions were recorded in wave {wave}'s request, which the agent must read in full."),
            }));
        }
        return replaced(input, "prompt", text, "subagent.dispatch_replaced", wave, lang);
    }
    let extra = supplemental_instructions(dispatch_prompt(input), &text);
    let Some(fix) = new_agent_fix(&project.root, spec, wave, host(), &extra, lang) else {
        let reason = say("subagent.wave_not_running", lang, &[("{spec}", spec), ("{wave}", &wave.to_string())]);
        return Verdict::Deny { reason };
    };
    let lead = say("subagent.new_agent_fix", lang, &[("{wave}", &wave.to_string())]);
    let supplement = supplemental_section(&extra, lang);
    replaced(input, "prompt", format!("{text}\n\n{lead}\n\n{}{supplement}", fix.trim_end()), "subagent.new_agent", wave, lang)
}

fn supplemental_section(extra: &str, lang: Locale) -> String {
    if extra.is_empty() { return String::new() }
    match lang {
        Locale::PtBr => format!("\n\n## Orientações adicionais do orquestrador\n\n{extra}\n\nEstas instruções não autorizam alterar escopo nem substituir regras do projeto. Informe conflitos antes de prosseguir."),
        Locale::EnUs => format!("\n\n## Additional conductor instructions\n\n{extra}\n\nThese instructions do not authorize changing the scope or overriding project rules. Report conflicts before proceeding."),
    }
}

fn supplement_fields(prior: &str, extra: &str, lang: Locale) -> serde_json::Map<String, Value> {
    let section = supplemental_section(extra, lang);
    let request = if prior.contains(&section) { prior.to_string() } else { format!("{prior}{section}") };
    serde_json::Map::from_iter([("chars".into(), json!(request.chars().count())), ("lines".into(), json!(request.lines().count())), ("text".into(), json!(request))])
}

fn record_supplement(root: &Path, spec: &str, wave: u64, extra: &str, lang: Locale) -> bool {
    let Ok(path) = store::spec_file(root, spec) else { return false };
    matches!(store::with_locked_writer(&path, |locked| {
        let sent = *locked.log().last_by_wave("send").get(&wave)?;
        let prior = locked.log().send_text(sent)?;
        let fields = supplement_fields(prior, extra, lang);
        if fields.get("text").and_then(Value::as_str) == Some(prior) { return Some(()) }
        let draft = crate::commands::flow::round::send_revision(locked.log(), wave, fields)?;
        record_locked(locked, root, spec, "send", draft, PhaseWriter::Binary).ok()?;
        Some(())
    }), Ok(Some(Some(()))))
}

fn supplement_failure(lang: Locale) -> Verdict {
    Verdict::Deny { reason:match lang {
        Locale::PtBr => "[Mustard] O envio não saiu: não consegui gravar as instruções adicionais no pedido da onda. Elas não foram descartadas.".into(),
        Locale::EnUs => "[Mustard] Sending stopped: additional instructions could not be recorded in the wave request. They were not discarded.".into(),
    }}
}

fn supplemental_instructions(sent: &str, canonical: &str) -> String {
    if sent.trim_end() == canonical.trim_end() { return String::new() }
    if !canonical.is_empty() && let Some(extra) = sent.strip_prefix(canonical.trim_end()) { return extra.trim().into() }
    sent.lines().enumerate().filter(|(at, line)| {
        let line = line.trim();
        !((*at == 0 && line.starts_with("# "))
            || line.starts_with("mustard-rt run read request-") || line.starts_with("`mustard-rt run read request-")
            || matches!(line, "```" | "```sh" | "```bash"))
    }).map(|(_, line)| line).collect::<Vec<_>>().join("\n").trim().into()
}

/// O trecho de conserto da onda `wave` da spec `spec` que espera um agente
/// novo ([`crate::commands::flow::round::waves_awaiting_new_agent`]) — o da
/// conferência que recusou a volta, ou o motivo de quem a reprovou —, com o
/// despacho dele gravado: o envio da onda ganha uma versão com o hospedeiro
/// que despacha (`host`; no gancho, o que
/// [`crate::commands::flow::stuck::sender_process`] acha), e outro despacho
/// é barrado enquanto esse hospedeiro segue aberto; fechado também ele, a
/// onda volta a aceitar um agente novo. Sem hospedeiro reconhecido, a versão
/// sai sem ele, e a onda segue aceitando um agente novo, como antes do
/// despacho. A versão guarda o lugar em que o envio despachou a onda, então a
/// onda segue voltada e o trecho segue valendo. Ler, conferir e gravar correm
/// com a trava do arquivo de eventos presa: de dois despachos na mesma
/// resposta, só um passa. Nada sem a gravação feita.
fn new_agent_fix(root: &Path, spec: &str, wave: u64, host: Option<(u32, u64)>, instruction: &str, lang: Locale) -> Option<String> {
    let path = store::spec_file(root, spec).ok()?;
    let taken = store::with_locked_writer(&path, |locked| {
        let file = crate::commands::flow::round::waves_awaiting_new_agent(root, spec, locked.log()).remove(&wave)?;
        let fix = std::fs::read_to_string(file).ok()?;
        // O agente novo vem de uma volta recusada ou reprovada: ele sai um
        // modelo acima do envio.
        let mut extra = crate::commands::flow::round::new_agent_escalation(root, locked.log(), wave);
        if host.is_some() {
            extra.extend(crate::commands::flow::round::sender_fields(host));
        }
        if !instruction.is_empty() {
            let sent = *locked.log().last_by_wave("send").get(&wave)?;
            extra.extend(supplement_fields(locked.log().send_text(sent)?, instruction, lang));
        }
        let draft = crate::commands::flow::round::send_revision(locked.log(), wave, extra)?;
        record_locked(locked, root, spec, "send", draft, PhaseWriter::Binary).ok()?;
        Some(fix)
    });
    taken.ok().flatten().flatten()
}

/// A mensagem do condutor a um agente já aberto. Quando o destino é o agente
/// de uma onda — a primeira mensagem da conversa dele abre com o título do
/// pedido da onda — que a reprovação de quem conduz tirou dela
/// ([`crate::commands::flow::round::replaced_by_rejection`]), a mensagem é
/// barrada ([`replaced_agent_reason`]): o motivo vai só ao agente novo, e a
/// cópia nunca tem dois agentes. Toda chamada daquele agente já é recusada
/// pela trava do tamanho da conversa, também a que ele faria acordado pela
/// mensagem; a recusa aqui poupa essa volta e diz a quem conduz para onde
/// mandar o motivo. Quando a rodada gravou o trecho da
/// conferência depois da onda para a volta do agente da onda
/// ([`crate::commands::flow::round::fix_file`]), a mensagem sai com esse
/// trecho e as orientações adicionais, gravadas no pedido. O resto passa como veio.
fn fix_message(input: &HookInput, ctx: &Ctx) -> Verdict {
    let root = ctx.project_dir_or_cwd(input);
    let project = crate::commands::spec_events::project(Path::new(&root));
    let lang = project.lang;
    let agent = input.tool_input.get("to").and_then(Value::as_str).unwrap_or_default();
    let destination = input.transcript_path().and_then(|transcript| agent_request(Path::new(transcript), agent));
    let Some(((spec, wave), started)) = destination.and_then(|(text, started)| {
        let read = crate::shared::host_observation::wave_named(&project.root, &text)?;
        Some(((read.spec, read.wave), started))
    }) else {
        return Verdict::Allow;
    };
    let Ok(log) = spec_log(&project.root, &spec, lang) else {
        return Verdict::Allow;
    };
    if crate::commands::flow::round::replaced_by_rejection(&log, wave, started) {
        return Verdict::Deny { reason: replaced_agent_reason(&project.root, &spec, &log, wave, lang) };
    }
    let fix = crate::commands::flow::round::fix_file(&project.root, &spec, &log, wave).and_then(|file| std::fs::read_to_string(file).ok());
    match fix {
        Some(text) => {
            let sent = input.tool_input.get("message").and_then(Value::as_str).unwrap_or_default();
            if sent.trim_end() == text.trim_end() { return Verdict::Allow }
            let extra = sent.strip_prefix(text.trim_end()).unwrap_or(sent).trim();
            if !record_supplement(&project.root, &spec, wave, extra, lang) { return supplement_failure(lang) }
            let section = supplemental_section(extra, lang);
            replaced(input, "message", format!("{}{section}", text.trim_end()), "subagent.fix_replaced", wave, lang)
        },
        None => Verdict::Allow,
    }
}

/// O motivo de barrar a mensagem ao agente que a reprovação de quem conduz
/// tirou da onda `wave` da spec `spec`: com a onda à espera do agente novo
/// ([`crate::commands::flow::round::waves_awaiting_new_agent`]), o título que
/// o despacha; com o agente novo já despachado, que ele recebeu o motivo.
fn replaced_agent_reason(root: &Path, spec: &str, log: &SpecLog, wave: u64, lang: Locale) -> String {
    let number = wave.to_string();
    if crate::commands::flow::round::waves_awaiting_new_agent(root, spec, log).contains_key(&wave) {
        let title = wave_title(spec, wave, lang);
        return say("subagent.rejected_agent", lang, &[("{wave}", &number), ("{title}", &title)]);
    }
    say("subagent.rejected_agent_working", lang, &[("{wave}", &number)])
}

/// O campo `field` da ferramenta trocado por `text`, com o recado `note` da
/// onda `wave` ao condutor; o que já é `text` passa como veio. O despacho que
/// traz só linhas de `text`, na mesma ordem — as duas que o condutor escreve,
/// sem a do meio —, é trocado sem o recado de que algo saiu: nada saiu.
fn replaced(input: &HookInput, field: &str, text: String, note: &str, wave: u64, lang: Locale) -> Verdict {
    let sent = input.tool_input.get(field).and_then(Value::as_str).unwrap_or_default();
    if sent.trim_end() == text.trim_end() {
        return Verdict::Allow;
    }
    let mut lines = text.lines().map(str::trim);
    let nothing_lost = note == "subagent.dispatch_replaced"
        && sent.lines().map(str::trim).filter(|line| !line.is_empty()).all(|line| lines.any(|own| own == line));
    with_field(input, field, text, (!nothing_lost).then(|| say(note, lang, &[("{wave}", &wave.to_string())])))
}

/// O texto da tarefa, no `tool_input.prompt`.
fn dispatch_prompt(input: &HookInput) -> &str {
    input.tool_input.get("prompt").and_then(Value::as_str).unwrap_or_default()
}

/// Se o despacho vai a um agente do Mustard: o `subagent_type` é `mustard-`
/// seguido do nome de um dos agentes que o instalador grava.
fn to_mustard_agent(input: &HookInput) -> bool {
    input.tool_input.get("subagent_type").and_then(Value::as_str).is_some_and(|kind|
        kind.strip_prefix("mustard-").is_some_and(|name| AGENT_NAMES.contains(&name)))
}

/// O despacho com `text` no lugar do texto da tarefa e os outros campos como
/// vieram.
fn with_prompt(input: &HookInput, text: String) -> Verdict {
    with_field(input, "prompt", text, None)
}

/// A ferramenta com `text` no campo `field`, os outros campos como vieram e
/// o recado `note` a quem a chamou.
fn with_field(input: &HookInput, field: &str, text: String, note: Option<String>) -> Verdict {
    let mut tool_input = input.tool_input.clone();
    match tool_input.as_object_mut() {
        Some(fields) => {
            fields.insert(field.to_string(), Value::String(text));
            Verdict::Rewrite { tool_input, note }
        }
        None => Verdict::Allow,
    }
}

/// Se o despacho vai ao agente de exploração do Claude Code.
fn to_explore_agent(input: &HookInput) -> bool {
    input.tool_input.get("subagent_type").and_then(Value::as_str).is_some_and(|kind| kind.trim().eq_ignore_ascii_case("explore"))
}

/// O pedido ao agente de exploração com a resposta curta do mapa no topo,
/// quando o mapa cravou a resposta; senão, como veio.
fn explore(input: &HookInput, ctx: &Ctx) -> Verdict {
    let root = ctx.project_dir_or_cwd(input);
    let prompt = dispatch_prompt(input);
    match word_search::hook_ask(&root, input, ctx, prompt) {
        Some(answer) => with_prompt(input, format!("{answer}\n\n{prompt}")),
        None => Verdict::Allow,
    }
}

/// O despacho de uma tarefa: a um agente do Mustard, num projeto com
/// `mustard.json`, o despacho de uma onda, que abre com o título do pedido
/// dela, sai com o texto que a rodada monta ([`wave_dispatch`]); qualquer
/// outro ganha no topo a linha dos idiomas, salvo quando já a traz. O
/// despacho ao revisor ganha também, no fim, as regras do projeto
/// ([`review_rules`]) e o caminho do desenho aprovado ([`approved_design`]).
/// O resto passa como veio.
fn dispatched(input: &HookInput, ctx: &Ctx) -> Verdict {
    let root = ctx.project_dir_or_cwd(input);
    if !to_mustard_agent(input) || !ProjectConfig::exists(Path::new(&root)) {
        return Verdict::Allow;
    }
    let language = ctx.config.language();
    let prompt = dispatch_prompt(input);
    if let Some(read) = crate::shared::host_observation::wave_named(Path::new(&root), prompt) {
        return wave_dispatch(input, Path::new(&root), &read.spec, read.wave, crate::commands::flow::stuck::sender_process);
    }
    let line = language_line(&language);
    let mut text = if prompt.contains(&line) { prompt.to_string() } else { format!("{line}\n\n{prompt}") };
    if let Some(rules) = review_rules(input, Path::new(&root), prompt, language.text_or_default()) {
        text = format!("{}\n\n{rules}", text.trim_end());
    }
    if let Some(design) = approved_design(input, Path::new(&root), prompt, language.text_or_default()) {
        text = format!("{}\n\n{design}", text.trim_end());
    }
    if text == prompt {
        return Verdict::Allow;
    }
    with_prompt(input, text)
}

/// As regras do projeto que o despacho ao revisor ganha no fim, quando o
/// texto é do orquestrador — a revisão do levantamento e a do pull request:
/// o revisor é instalado sem os `CLAUDE.md`, e o pedido que ele recebe é o
/// único caminho delas. São os arquivos de regras da raiz e das pastas dos
/// arquivos que a obra da spec atual mexe ([`touched_files`]), a mesma
/// leitura da revisão final. Nada ao agente de onda, ao texto que já traz a
/// seção e ao que manda ler o pedido gravado da revisão final ou o de uma
/// onda, que já as trazem;
/// nada também sem arquivo de regras com texto.
fn review_rules(input: &HookInput, root: &Path, prompt: &str, lang: Locale) -> Option<String> {
    let to_reviewer = input.tool_input.get("subagent_type").and_then(Value::as_str) == Some("mustard-review");
    let recorded = reads_final_request(prompt) || crate::commands::flow::round::is_wave_review_dispatch(prompt);
    if !to_reviewer || recorded || carries_project_rules(prompt, lang) {
        return None;
    }
    let disk = DiskSpecState::new(root);
    let log = disk.active(input.session_id.as_deref()).and_then(|spec| disk.log(&spec));
    project_rules_section(&project_rules(root, &touched_files(root, log.as_ref())), lang)
}

/// Se o despacho manda ler o pedido gravado da revisão final
/// (`mustard-rt run read request-review`): é o que distingue o revisor final
/// do texto que o orquestrador escreve para a revisão do levantamento ou do
/// pull request.
fn reads_final_request(prompt: &str) -> bool {
    prompt.contains("run read request-review")
}

/// O nome da cópia do desenho que o usuário aprovou, na pasta da spec. Quem
/// conduz a obra a salva ali depois do sim do usuário, e o arquivo existir é
/// o sinal de que a spec tem referência visual: nenhum item nem trava a mais.
const APPROVED_DESIGN: &str = "desenho.html";

/// A seção que aponta ao revisor a cópia do desenho aprovado da spec que ele
/// revisa, para o fim de todo despacho a ele — o pedido gravado da revisão
/// de uma onda, o da revisão final e o texto de quem conduz —, quando a
/// pasta da spec tem a cópia: o revisor não abre o link da página, só o
/// arquivo. A spec é a do pedido gravado que o despacho manda ler
/// ([`read_spec`]) ou, no texto de quem conduz, a spec atual. Nada ao texto
/// que já traz o caminho.
fn approved_design(input: &HookInput, root: &Path, prompt: &str, lang: Locale) -> Option<String> {
    if input.tool_input.get("subagent_type").and_then(Value::as_str) != Some("mustard-review") {
        return None;
    }
    let spec = read_spec(prompt).map(str::to_string).or_else(|| DiskSpecState::new(root).active(input.session_id.as_deref()))?;
    let design = store::spec_file(&store::spec_root(root), &spec).ok()?.with_file_name(APPROVED_DESIGN);
    let shown = mustard_core::io::wave_prompt::shown(&design);
    if !design.is_file() || prompt.contains(&shown) {
        return None;
    }
    let heading = say("prompt.part.approved_design", lang, &[]);
    Some(format!("## {heading}\n\n{}", say("prompt.approved_design", lang, &[("{path}", &shown)])))
}

/// A spec do pedido gravado que o despacho manda ler: o `--spec` do
/// `mustard-rt run read` do texto.
fn read_spec(prompt: &str) -> Option<&str> {
    prompt.lines().filter(|line| line.contains("mustard-rt run read ")).find_map(|line| {
        let (_, rest) = line.split_once(" --spec ")?;
        rest.split(|c: char| c.is_whitespace() || c == '`').next().filter(|spec| !spec.is_empty())
    })
}

/// Os apelidos que a ferramenta Agent do Claude Code aceita no campo `model`.
const MODEL_ALIASES: [&str; 4] = ["sonnet", "opus", "haiku", "fable"];

/// O apelido de `model` que a ferramenta Agent aceita: o próprio ou o que o
/// nome inteiro contém (`claude-opus-5-5` é `opus`); `None` sem apelido.
fn model_alias(model: &str) -> Option<&'static str> {
    let lower = model.to_ascii_lowercase();
    MODEL_ALIASES.into_iter().find(|alias| lower.contains(alias))
}

/// A variável do Claude Code que fixa o esforço da sessão inteira: definida
/// no ambiente do gancho, é ela o esforço que vale para o agente lançado.
const EFFORT_VARIABLE: &str = "CLAUDE_CODE_EFFORT_LEVEL";

/// `true` quando `agent` escreve código no projeto, e por isso nunca sai no
/// modelo pequeno da sessão. O revisor, que só lê, fica de fora.
fn writes_code(agent: &str) -> bool {
    agent == "mustard-wave"
}

/// O modelo que recebe a onda quando o que iria valer é o modelo pequeno da
/// sessão: o usuário decidiu em 20/10/2026 que código nunca vai no modelo
/// pequeno, e que o simples vai ao Sonnet.
///
/// Sai do apelido que o Mustard já escolheu para a onda, e cai no padrão dos
/// agentes do projeto ([`DEFAULT_AGENT_MODEL`]) quando o escolhido não tem
/// apelido ou é o próprio pequeno. Nenhum valor novo nasce aqui: um piso
/// escrito neste arquivo precisaria de recompilação para mudar.
fn code_floor(declared: &str, small: &str) -> &'static str {
    model_alias(declared).filter(|alias| *alias != small).unwrap_or(DEFAULT_AGENT_MODEL)
}

/// [`launch_settings_with`] com o valor de [`EFFORT_VARIABLE`] do ambiente.
fn launch_settings(verdict: Verdict, input: &HookInput, ctx: &Ctx) -> Verdict {
    launch_settings_with(verdict, input, ctx, std::env::var(EFFORT_VARIABLE).ok())
}

/// O lançamento do agente de onda ou do revisor com o modelo que o Mustard
/// escolheu para ele — o da onda, o do último envio dela, que sai da
/// dificuldade das tarefas e sobe na reprovação; o do revisor final, o que a
/// chamada pede e, sem pedido, o de `agents.review`; o do revisor de uma
/// onda, o de `agents.wave_review`; o do texto que o orquestrador escreve
/// para o revisor, o de `agents.review`; o da onda sem envio legível, o de
/// `agents` — e sempre com o esforço `xhigh`, no lugar do que a chamada
/// pede: o `model` da chamada vence o cabeçalho do agente, então o escolhido
/// vai nela. O modelo que o usuário pediu para o revisor final fica: só a
/// onda segue o da rodada. O nome inteiro do modelo vai pelo apelido que
/// contém ([`model_alias`]); sem apelido, o modelo fica como veio e, se a
/// chamada pediu outro, sai o aviso. O campo que a chamada não traz vale o
/// escolhido; o modelo bate quando um nome contém o outro. O recado diz o
/// que a chamada pedia quando era outro. O lançamento de uma onda grava no
/// envio dela o esforço que vale (`effort_used`): `effort_env`, o valor de
/// [`EFFORT_VARIABLE`], quando definido, ou o imposto; o do revisor final
/// grava no envio do pedido da revisão o modelo que vale (`model_used`).
/// Nada para outro agente, para outro hospedeiro que não o Claude Code, fora
/// de um projeto com `mustard.json`, nem para o despacho recusado.
fn launch_settings_with(verdict: Verdict, input: &HookInput, ctx: &Ctx, effort_env: Option<String>) -> Verdict {
    if input.raw["mustard_host"].as_str().is_some_and(|host| host != "claude") {
        return verdict;
    }
    let Some(agent) = input.tool_input.get("subagent_type").and_then(Value::as_str) else { return verdict };
    if agent.rsplit(':').next() == Some("mustard-analysis") {
        return analysis_effort(verdict, input);
    }
    let configured = match agent {
        "mustard-wave" => ctx.config.agent_settings().model,
        "mustard-review" if crate::commands::flow::round::is_wave_review_dispatch(dispatch_prompt(input)) => ctx.config.wave_review_settings().model,
        "mustard-review" => ctx.config.review_settings().model,
        _ => return verdict,
    };
    let root = ctx.project_dir_or_cwd(input);
    if !ProjectConfig::exists(Path::new(&root)) {
        return verdict;
    }
    let project = crate::commands::spec_events::project(Path::new(&root));
    let wave = (agent == "mustard-wave").then(|| crate::shared::host_observation::wave_named(&project.root, dispatch_prompt(input))).flatten();
    let sent_model: Option<String> = wave.as_ref().and_then(|read| {
        let log = spec_log(&project.root, &read.spec, project.lang).ok()?;
        let sent = *log.last_by_wave("send").get(&read.wave)?;
        log.send_field(sent, "model").and_then(Value::as_str).filter(|model| *model != "inherit").map(str::to_string)
    });
    let asked = |field: &str| input.tool_input.get(field).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty());
    // O revisor final do pedido gravado vale o modelo que a chamada pediu: a
    // escolha é de quem a fez, e o Mustard não a troca pelo do `mustard.json`.
    // A onda segue o modelo da rodada, e o texto do orquestrador, o da
    // configuração.
    let final_review = agent == "mustard-review" && reads_final_request(dispatch_prompt(input)) && !crate::commands::flow::round::is_wave_review_dispatch(dispatch_prompt(input));
    let kept = asked("model").filter(|_| final_review);
    let declared = AgentSettings { model: kept.or(sent_model.as_deref()).unwrap_or(configured), effort: DEFAULT_AGENT_EFFORT };
    let lower = |text: &str| text.to_ascii_lowercase();
    let model = asked("model").unwrap_or(declared.model);
    let effort = asked("effort").unwrap_or(declared.effort);
    let same_model = lower(model).contains(&lower(declared.model)) || lower(declared.model).contains(&lower(model));
    let alias = model_alias(declared.model);
    // O modelo que vai valer no lançamento: o apelido do declarado, ou o que a
    // chamada pediu quando o declarado não tem apelido conhecido e por isso
    // passa como veio. O agente que escreve código nunca sai no modelo pequeno
    // da sessão, e sobe para [`CODE_MODEL_FLOOR`] — enquanto a decisão do
    // usuário não estava aqui, uma chamada que pedia o modelo pequeno passava
    // inteira nesse segundo caso (campo, 21/10/2026).
    let effective = alias.unwrap_or(model);
    let small = mustard_core::platform::session_models::CLAUDE_SESSION_MODEL;
    let raised = writes_code(agent) && model_alias(effective) == Some(small);
    let floor = code_floor(declared.model, small);
    let lang = ctx.config.language().text_or_default();
    let warning = match alias {
        Some(_) if !(same_model && effort.eq_ignore_ascii_case(declared.effort)) => {
            let slots = [("{agent}", agent), ("{asked_model}", model), ("{asked_effort}", effort), ("{model}", declared.model), ("{effort}", declared.effort)];
            Some(say("subagent.launch_settings_forced", lang, &slots))
        }
        // Sem apelido, só o modelo segue como veio: o esforço já sai trocado.
        None if !same_model => {
            let slots = [("{agent}", agent), ("{asked_model}", model), ("{asked_effort}", declared.effort), ("{model}", declared.model), ("{effort}", declared.effort)];
            Some(say("subagent.launch_settings", lang, &slots))
        }
        _ => None,
    };
    // A subida de modelo é dita sempre, e some junto com o aviso de modelo
    // trocado: quem lê precisa saber qual modelo entrou e por quê.
    let warning = if raised {
        let slots = [("{agent}", agent), ("{asked_model}", effective), ("{model}", floor)];
        let raised_text = say("subagent.no_small_model_for_code", lang, &slots);
        Some(match warning {
            Some(before) => format!("{raised_text} {before}"),
            None => raised_text,
        })
    } else {
        warning
    };
    let (mut tool_input, note) = match verdict {
        Verdict::Allow => (input.tool_input.clone(), None),
        Verdict::Rewrite { tool_input, note } => (tool_input, note),
        other => return with_warning(other, warning),
    };
    if let Some(read) = &wave {
        let held = effort_env.as_deref().map(str::trim).filter(|effort| !effort.is_empty()).unwrap_or(declared.effort);
        record_on_send(&project.root, &read.spec, EFFORT_USED, held, |log| log.last_by_wave("send").get(&read.wave).copied());
    }
    if final_review && let Some(spec) = read_spec(dispatch_prompt(input)) {
        record_on_send(&project.root, spec, MODEL_USED, effective, crate::commands::flow::round::awaited_review);
    }
    let Some(fields) = tool_input.as_object_mut() else { return with_warning(Verdict::Allow, warning) };
    if raised {
        fields.insert("model".into(), json!(floor));
    } else if let Some(alias) = alias {
        fields.insert("model".into(), json!(alias));
    }
    fields.insert("effort".into(), json!(declared.effort));
    if note.is_none() && warning.is_none() && tool_input == input.tool_input {
        return Verdict::Allow;
    }
    with_warning(Verdict::Rewrite { tool_input, note }, warning)
}

/// O campo do envio com o esforço que valeu no lançamento do agente.
const EFFORT_USED: &str = "effort_used";

/// O campo do envio da revisão final com o modelo que valeu no lançamento do
/// revisor.
const MODEL_USED: &str = "model_used";

/// Grava `field` com `value` no envio que `newest` acha no arquivo de eventos
/// da spec `spec`, de `root`, numa versão nova do envio, com a trava do
/// arquivo presa: o esforço que valeu no lançamento do agente da onda e o
/// modelo que valeu no do revisor final. O envio que já traz o valor, o
/// pedido que `newest` não acha — a revisão final que a última mudança da
/// obra deixou velha, que não pode voltar a valer só por ganhar uma versão —
/// e a falha de gravação não gravam nada, e nenhum segura o lançamento.
fn record_on_send(root: &Path, spec: &str, field: &str, value: &str, newest: impl Fn(&SpecLog) -> Option<u64>) {
    let Ok(path) = store::spec_file(root, spec) else { return };
    let _ = store::with_locked_writer(&path, |locked| {
        let sent = newest(locked.log())?;
        if locked.log().send_field(sent, field).and_then(Value::as_str) == Some(value) {
            return Some(());
        }
        let draft = crate::commands::flow::round::queue::revise_send(locked.log(), sent, serde_json::Map::from_iter([(field.to_string(), json!(value))]))?;
        record_locked(locked, root, spec, "send", draft, PhaseWriter::Binary).ok().map(|_| ())
    });
}

/// O lançamento do agente de análise com o esforço máximo, sobre o que a
/// chamada pedir; o modelo fica como veio, porque o pequeno é o dele e a
/// política do plugin já recusa outra família. A recusa passa como veio.
fn analysis_effort(verdict: Verdict, input: &HookInput) -> Verdict {
    let (mut tool_input, note) = match verdict {
        Verdict::Allow => (input.tool_input.clone(), None),
        Verdict::Rewrite { tool_input, note } => (tool_input, note),
        other => return other,
    };
    let Some(fields) = tool_input.as_object_mut() else { return Verdict::Allow };
    fields.insert("effort".into(), json!(DEFAULT_AGENT_EFFORT));
    if note.is_none() && tool_input == input.tool_input {
        return Verdict::Allow;
    }
    Verdict::Rewrite { tool_input, note }
}

/// `verdict` com o aviso `warning` junto: no recado da troca, no aviso que já
/// havia, ou como aviso do lançamento que passaria como veio. A recusa segue
/// como veio.
fn with_warning(verdict: Verdict, warning: Option<String>) -> Verdict {
    let Some(warning) = warning else { return verdict };
    let joined = |before: Option<String>| Some(before.map_or(warning.clone(), |before| format!("{before}\n\n{warning}")));
    match verdict {
        Verdict::Allow => Verdict::Warn { message: warning },
        Verdict::Warn { message } => Verdict::Warn { message: format!("{message}\n\n{warning}") },
        Verdict::Rewrite { tool_input, note } => Verdict::Rewrite { tool_input, note: joined(note) },
        other => other,
    }
}

impl Check for SubagentInject {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if input.tool_name.as_deref() == Some(SEND_MESSAGE) {
            return Ok(fix_message(input, ctx));
        }
        if to_explore_agent(input) {
            return Ok(explore(input, ctx));
        }
        Ok(launch_settings(dispatched(input, ctx), input, ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{WriteOpts, record_open, seed_at};
    use mustard_core::platform::i18n::translate;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(root: &Path) -> Ctx {
        let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        ctx.config = mustard_core::ProjectConfig::load(root);
        ctx
    }

    fn dispatch(root: &Path, prompt: &str) -> Verdict {
        dispatch_to(root, "general-purpose", prompt)
    }

    /// O despacho de `prompt` ao agente `agent`, como o Claude Code o manda.
    fn task_input(agent: &str, prompt: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "prompt": prompt, "subagent_type": agent, "description": "onda" }),
            ..HookInput::default()
        }
    }

    /// O gancho diante do despacho de `prompt` ao agente `agent`.
    fn dispatch_to(root: &Path, agent: &str, prompt: &str) -> Verdict {
        SubagentInject.evaluate(&task_input(agent, prompt), &ctx(root)).expect("never errors")
    }

    /// O despacho de `prompt` ao agente da onda 1 da spec `x`, com o processo
    /// do próprio teste como o hospedeiro que despacha: aberto até o fim do
    /// teste, ele não depende de quem lançou a suíte.
    #[cfg(target_os = "linux")]
    fn dispatch_from_this_process(root: &Path, prompt: &str) -> Verdict {
        wave_dispatch(&task_input("mustard-wave", prompt), root, "x", 1, || Some(crate::commands::flow::stuck::this_process()))
    }

    /// O texto que o despacho reescrito leva ao agente.
    fn rewritten(verdict: Verdict) -> String {
        match verdict {
            Verdict::Rewrite { tool_input, .. } => tool_input["prompt"].as_str().unwrap_or_default().to_string(),
            other => panic!("the dispatch is rewritten, got {other:?}"),
        }
    }

    fn write(root: &Path, event_type: &str, body: Value) -> u64 {
        let out = seed_at(&WriteOpts { root: root.to_path_buf(), spec: Some("x".to_string()), event_type: event_type.into(), json: body.to_string() });
        out["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {out}"))
    }

    /// Uma spec `x` com uma onda de uma tarefa, ainda no levantamento.
    fn planned(root: &Path) {
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = write(root, "message", json!({"author": "user", "text": "o objetivo"}));
        let crit = write(
            root,
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "form": "ubiquitous",
                "origin": said}),
        );
        write(
            root,
            "wave",
            json!({"n": 1, "text": "Onda 1.", "criteria": [crit],
            "done_when": "A suíte passa.", "origin": said}),
        );
        write(root, "task", json!({"wave": 1, "text": "Tarefa 0.", "files": [], "depends_on": [], "origin": said}));
    }

    /// Uma linha crua, direto no arquivo da spec, sem passar pela gravação:
    /// a tarefa sem onda que o teste das regras do revisor acrescenta.
    fn append_raw(root: &Path, event_type: &str, body: Value, id: u64) {
        let mut map = mustard_core::domain::spec_events::normalize(body.as_object().cloned().unwrap_or_default(), event_type);
        map.insert("type".into(), json!(event_type));
        let line = mustard_core::domain::spec_events::render_line(&mustard_core::domain::spec_events::stamp(map, id, None, "2026-09-20T10:00:00-03:00"));
        use std::io::Write as _;
        let path = store::spec_file(root, "x").unwrap();
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "{line}").unwrap();
    }

    fn approve(root: &Path) {
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
    }

    /// O pedido que a rodada e a página montam para a onda 1.
    fn assembled(root: &Path) -> String {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        use mustard_core::io::wave_prompt::{Flight, prompts};
        prompts(root, "x", &log, Locale::PtBr, &Flight::default(), &log.planned_waves()).into_iter().find(|p| p.wave == 1).expect("the wave's request").text
    }

    fn denied(verdict: Verdict) -> String {
        match verdict {
            Verdict::Deny { reason } => reason,
            other => panic!("the dispatch is refused, got {other:?}"),
        }
    }

    /// O lançamento do agente de onda ou do revisor com modelo diferente do
    /// que o `mustard.json` declara para ele, ou com esforço diferente do
    /// `xhigh` que todo agente leva, sai com um aviso. O que bate, ou que não
    /// diz modelo nem esforço, sai sem aviso.
    #[test]
    fn a_launch_with_another_model_or_effort_than_the_config_warns() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let agents = json!({"model": "sonnet", "effort": "high", "review": {"model": "opus", "effort": "max"}});
        std::fs::write(root.join("mustard.json"), json!({ "agents": agents }).to_string()).unwrap();
        let launch = |agent: &str, asked: Value| {
            let mut input = task_input(agent, "Confira a obra.");
            for (key, value) in asked.as_object().unwrap() {
                input.tool_input[key] = value.clone();
            }
            match SubagentInject.evaluate(&input, &ctx(root)).expect("never errors") {
                Verdict::Rewrite { note, .. } => note.unwrap_or_default(),
                Verdict::Warn { message } => message,
                Verdict::Allow => String::new(),
                other => panic!("the launch goes out, got {other:?}"),
            }
        };
        let warned = launch("mustard-review", json!({"model": "sonnet"}));
        assert!(warned.contains("mustard-review") && warned.contains("sonnet") && warned.contains("opus"), "{warned}");
        let warned = launch("mustard-wave", json!({"model": "opus", "effort": "max"}));
        assert!(warned.contains("mustard-wave") && warned.contains("opus") && warned.contains("high"), "{warned}");
        assert_eq!(launch("mustard-review", json!({"model": "opus", "effort": "XHIGH"})), "", "the effort is always xhigh");
        assert!(launch("mustard-review", json!({"model": "opus", "effort": "max"})).contains("xhigh"), "even over the configured max");
        assert_eq!(launch("mustard-wave", json!({})), "");
    }

    /// O lançamento do agente de onda sem envio, ou do revisor, sai com o
    /// modelo que o `mustard.json` declara para ele e o esforço `xhigh`, com
    /// os outros campos como vieram. O nome inteiro do modelo vira o apelido
    /// que ele contém; sem apelido, o modelo fica como veio e o aviso segue.
    /// Outro agente e outro hospedeiro passam como vieram.
    #[test]
    fn a_launch_goes_out_with_the_model_and_effort_the_config_declares() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let agents = json!({"model": "claude-opus-5-5", "effort": "xhigh", "review": {"model": "sonnet", "effort": "max"}});
        std::fs::write(root.join("mustard.json"), json!({ "agents": agents }).to_string()).unwrap();
        let launch = |agent: &str, asked: Value, host: Option<&str>| {
            let mut input = task_input(agent, "Confira a obra.");
            input.tool_input["run_in_background"] = json!(true);
            for (key, value) in asked.as_object().unwrap() {
                input.tool_input[key] = value.clone();
            }
            if let Some(host) = host {
                input.raw = json!({"mustard_host": host});
            }
            SubagentInject.evaluate(&input, &ctx(root)).expect("never errors")
        };
        let sent = |verdict: Verdict| match verdict {
            Verdict::Rewrite { tool_input, note } => (tool_input, note.unwrap_or_default()),
            other => panic!("the launch is rewritten, got {other:?}"),
        };
        let (wave, note) = sent(launch("mustard-wave", json!({"model": "sonnet", "effort": "low"}), None));
        assert_eq!((wave["model"].as_str(), wave["effort"].as_str()), (Some("opus"), Some("xhigh")), "{wave}");
        assert_eq!(wave["description"], "onda");
        assert_eq!(wave["subagent_type"], "mustard-wave");
        assert_eq!(wave["run_in_background"], true);
        assert!(wave["prompt"].as_str().unwrap().ends_with("Confira a obra."), "{wave}");
        assert!(note.contains("sonnet") && note.contains("opus"), "{note}");
        let (review, _) = sent(launch("mustard-review", json!({}), None));
        assert_eq!((review["model"].as_str(), review["effort"].as_str()), (Some("sonnet"), Some("xhigh")), "{review}");
        match launch("general-purpose", json!({"model": "haiku"}), None) {
            Verdict::Allow => {}
            other => panic!("another agent goes as it came, got {other:?}"),
        }
        match launch("mustard-wave", json!({"model": "haiku"}), Some("codex")) {
            Verdict::Rewrite { tool_input, .. } => assert_eq!(tool_input["model"], "haiku"),
            Verdict::Allow => {}
            other => panic!("another host keeps the model, got {other:?}"),
        }
        std::fs::write(root.join("mustard.json"), json!({"agents": {"model": "custom-model-1", "effort": "high"}}).to_string()).unwrap();
        // O modelo configurado não tem apelido conhecido, então o pedido da
        // chamada passava como veio — e uma onda pedida no modelo pequeno
        // saía nele. O usuário decidiu em 20/10/2026 que código nunca vai no
        // modelo pequeno: a onda sobe para o Sonnet, e o aviso diz por quê.
        let (custom, note) = sent(launch("mustard-wave", json!({"model": "haiku"}), None));
        assert_eq!((custom["model"].as_str(), custom["effort"].as_str()), (Some("sonnet"), Some("xhigh")), "{custom}");
        assert!(note.contains("custom-model-1"), "the warning stays: {note}");
        assert!(note.contains("haiku") && note.contains("sonnet"), "the warning says which model went out: {note}");
        // O revisor só lê, e segue aceitando o modelo pequeno.
        let (review, _) = sent(launch("mustard-review", json!({"model": "haiku"}), None));
        assert_eq!(review["model"].as_str(), Some("haiku"), "{review}");
    }

    /// The wave reviewer, dispatched with the title and the command that
    /// reads its request, goes out on `agents.wave_review` (Sonnet by
    /// default) at effort xhigh, while the final reviewer goes on the model
    /// its call asked for, and on `agents.review` when the call asks for
    /// none; the wave review request already carries the project rules, so
    /// the dispatch gains none.
    #[test]
    fn the_wave_reviewer_launches_on_its_own_settings_and_the_final_one_on_the_model_it_was_asked() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("CLAUDE.md"), "Never duplicate types.\n").unwrap();
        let launch = |prompt: &str, model: Option<&str>| {
            let mut input = task_input("mustard-review", prompt);
            if let Some(model) = model {
                input.tool_input["model"] = json!(model);
            }
            input.tool_input["effort"] = json!("low");
            match SubagentInject.evaluate(&input, &ctx(root)).expect("never errors") {
                Verdict::Rewrite { tool_input, .. } => tool_input,
                other => panic!("the launch is rewritten, got {other:?}"),
            }
        };
        let wave = "# x — revisão da onda 2\n\nmustard-rt run read review-request-2 --root /r --spec x";
        let last = "# x — agente de teste dedicado\n\nmustard-rt run read request-review --root /r --spec x";
        for (agents, wave_model, final_model) in [
            (json!({"review": {"model": "opus"}}), "sonnet", "opus"),
            (json!({"review": {"model": "opus"}, "wave_review": {"model": "haiku"}}), "haiku", "opus"),
        ] {
            std::fs::write(root.join("mustard.json"), json!({ "agents": agents }).to_string()).unwrap();
            let sent = launch(wave, Some("haiku"));
            assert_eq!((sent["model"].as_str(), sent["effort"].as_str()), (Some(wave_model), Some("xhigh")), "{agents}: {sent}");
            assert!(!sent["prompt"].as_str().unwrap().contains("Never duplicate types."), "{sent}");
            let sent = launch(last, Some("haiku"));
            assert_eq!((sent["model"].as_str(), sent["effort"].as_str()), (Some("haiku"), Some("xhigh")), "{agents}: the call's model stays: {sent}");
            let sent = launch(last, None);
            assert_eq!((sent["model"].as_str(), sent["effort"].as_str()), (Some(final_model), Some("xhigh")), "{agents}: {sent}");
        }
    }

    /// The model the user asked for the final reviewer stays, at the usual
    /// effort and without the warning of a swapped model, while the wave
    /// asked on another model goes on the one its send recorded, and the text
    /// the conductor writes for the reviewer goes on the configured one.
    #[test]
    fn a_model_asked_for_the_final_reviewer_stays_while_the_wave_follows_the_round() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root);
        std::fs::write(root.join("mustard.json"), json!({"agents": {"model": "opus", "review": {"model": "sonnet"}}}).to_string()).unwrap();
        approve(root);
        let (pid, started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave":1,"role":"wave","text":assembled(root),"lines":1,"chars":8,"mustard":"0",
            "author":"binary","model":"sonnet","effort":"xhigh","claude_pid":pid,"claude_started":started}));
        let canonical = crate::commands::flow::round::wave_dispatch(root, "x", 1, Locale::PtBr);
        let launch = |agent: &str, prompt: &str, asked: Value| {
            let mut input = task_input(agent, prompt);
            for (key, value) in asked.as_object().unwrap() {
                input.tool_input[key] = value.clone();
            }
            let ctx = ctx(root);
            match launch_settings_with(dispatched(&input, &ctx), &input, &ctx, None) {
                Verdict::Rewrite { tool_input, note } => (tool_input, note.unwrap_or_default()),
                other => panic!("the launch is rewritten, got {other:?}"),
            }
        };
        let final_review = "mustard-rt run read request-review --root /r --spec x";

        let (wave, note) = launch("mustard-wave", &canonical, json!({"model": "opus"}));
        assert_eq!((wave["model"].as_str(), wave["effort"].as_str()), (Some("sonnet"), Some("xhigh")), "{wave}");
        assert!(note.contains("opus") && note.contains("sonnet"), "the wave is forced and the warning says so: {note}");

        let (review, note) = launch("mustard-review", final_review, json!({"model": "opus"}));
        assert_eq!((review["model"].as_str(), review["effort"].as_str()), (Some("opus"), Some("xhigh")), "{review}");
        assert_eq!(note, "", "no model was swapped, so there is nothing to warn about");

        let (review, note) = launch("mustard-review", final_review, json!({"model": "opus", "effort": "low"}));
        assert_eq!((review["model"].as_str(), review["effort"].as_str()), (Some("opus"), Some("xhigh")), "{review}");
        assert!(note.contains("low") && note.contains("xhigh") && !note.contains("sonnet"), "only the effort is said to be swapped: {note}");

        let (survey, _) = launch("mustard-review", "Confira a obra.", json!({"model": "opus"}));
        assert_eq!(survey["model"].as_str(), Some("sonnet"), "the conductor's own text keeps the configured model: {survey}");
    }

    /// The wave agent goes out on the model its last send recorded, not the
    /// configured one, always at effort xhigh, and the send records the
    /// effort that held: the imposed one, or the value of
    /// `CLAUDE_CODE_EFFORT_LEVEL` when the hook's environment defines it.
    #[test]
    fn the_wave_launch_goes_on_its_send_model_at_effort_xhigh_and_records_the_effort_used() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root);
        std::fs::write(root.join("mustard.json"), json!({"agents": {"model": "opus", "effort": "low"}}).to_string()).unwrap();
        approve(root);
        let (pid, started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave":1,"role":"wave","text":assembled(root),"lines":1,"chars":8,"mustard":"0",
            "author":"binary","model":"sonnet","effort":"xhigh","claude_pid":pid,"claude_started":started}));
        let canonical = crate::commands::flow::round::wave_dispatch(root, "x", 1, Locale::PtBr);
        let launch = |env: Option<&str>| {
            let mut input = task_input("mustard-wave", &canonical);
            input.tool_input["model"] = json!("opus");
            let ctx = ctx(root);
            launch_settings_with(dispatched(&input, &ctx), &input, &ctx, env.map(str::to_string))
        };
        let effort_used = || {
            let log = spec_log(root, "x", Locale::PtBr).unwrap();
            log.get(log.last_by_wave("send")[&1]).and_then(|send| send.str_field("effort_used").map(str::to_string))
        };
        match launch(None) {
            Verdict::Rewrite { tool_input, note } => {
                assert_eq!((tool_input["model"].as_str(), tool_input["effort"].as_str()), (Some("sonnet"), Some("xhigh")), "{tool_input}");
                assert!(note.unwrap_or_default().contains("sonnet"), "the call asked another model");
            }
            other => panic!("the launch is rewritten, got {other:?}"),
        }
        assert_eq!(effort_used().as_deref(), Some("xhigh"));
        assert!(matches!(launch(Some("medium")), Verdict::Rewrite { .. }));
        assert_eq!(effort_used().as_deref(), Some("medium"), "the variable is the effort that holds");
    }

    /// The new agent of a wave whose return the round refused, with the
    /// sender's Claude Code closed, goes one model up from its send, once for
    /// that return: the send records where the model came from, and the
    /// launch goes on the new model.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_new_agent_after_a_refused_return_goes_one_model_up_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (pid, started) = crate::commands::flow::round::closed_process();
        let (fix, _) = refused_wave(root, pid, started);
        std::fs::write(&fix, format!("{SECTION}\n")).unwrap();
        let log = spec_log(root, "x", Locale::PtBr).unwrap();
        let back = log.events.iter().rfind(|e| e.event_type == "delivered").map(|e| e.id).expect("the return");
        let draft = crate::commands::flow::round::send_revision(&log, 1, json!({"model": "haiku", "host": "claude"}).as_object().cloned().unwrap()).unwrap();
        crate::shared::spec_state::seed_event(root, "x", "send", Value::Object(draft));
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let sent = format!("{title}\n\nConserte a onda.");
        let last = || {
            let log = spec_log(root, "x", Locale::PtBr).unwrap();
            log.get(log.last_by_wave("send")[&1]).cloned().unwrap()
        };
        match dispatch_to(root, "mustard-wave", &sent) {
            Verdict::Rewrite { tool_input, .. } => assert_eq!(tool_input["model"], json!("sonnet"), "{tool_input}"),
            other => panic!("the dispatch goes to a new agent, got {other:?}"),
        }
        let send = last();
        assert_eq!(send.str_field("model"), Some("sonnet"), "{send:?}");
        assert_eq!(send.fields.get("escalated_from"), Some(&json!({"model": "haiku", "cause": back})), "{send:?}");
        let log = spec_log(root, "x", Locale::PtBr).unwrap();
        assert!(crate::commands::flow::round::new_agent_escalation(root, &log, 1).is_empty(), "once per return");
    }

    /// O agente de análise sai sempre no esforço máximo, com o modelo que a
    /// chamada trouxer: o modelo pequeno é dele, o esforço não se negocia.
    #[test]
    fn the_analysis_launch_goes_out_at_the_highest_effort() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), json!({"agents": {"model": "sonnet", "effort": "high"}}).to_string()).unwrap();
        for agent in ["mustard-analysis", "mustard:mustard-analysis"] {
            for asked in [json!({}), json!({"effort": "medium", "model": "haiku"})] {
                let mut input = task_input(agent, "Analise o pacote.");
                for (key, value) in asked.as_object().unwrap() {
                    input.tool_input[key] = value.clone();
                }
                match SubagentInject.evaluate(&input, &ctx(root)).expect("never errors") {
                    Verdict::Rewrite { tool_input, .. } => {
                        assert_eq!(tool_input["effort"], "xhigh", "{agent} {tool_input}");
                        assert_eq!(tool_input["model"], asked.get("model").cloned().unwrap_or(Value::Null), "{agent} keeps its model");
                    }
                    other => panic!("the {agent} launch is rewritten, got {other:?}"),
                }
            }
        }
    }

    #[test]
    fn additional_dispatch_instructions_are_recorded_once_in_the_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root);
        approve(root);
        let out = crate::commands::flow::round::round_for(&crate::commands::flow::round::RoundOpts {
            root:root.into(), spec:Some("x".into()), report:None,
        }, None);
        assert_eq!(out["ok"], true, "{out}");
        let (pid, started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave":1,"role":"wave","text":assembled(root),"lines":1,"chars":8,"mustard":"0","author":"binary","claude_pid":pid,"claude_started":started}));
        let canonical = crate::commands::flow::round::wave_dispatch(root, "x", 1, Locale::PtBr);
        let instruction = "Use taskset -c 0-3 for the declared Jest command.";
        let sent = format!("{canonical}\n\n{instruction}");
        for _ in 0..2 {
            let result = dispatch_to(root, "mustard-wave", &sent);
            assert!(matches!(result, Verdict::Rewrite { .. }), "{result:?}");
        }
        let log = spec_log(root, "x", Locale::PtBr).unwrap();
        let latest = log.send_text(*log.last_by_wave("send").get(&1).unwrap()).unwrap();
        assert_eq!(latest.matches(instruction).count(), 1);
        assert!(latest.contains("não autorizam alterar escopo"));
    }

    /// A onda em andamento cujo agente a observação do hospedeiro já ligou ao
    /// envio não aceita um segundo agente, mesmo quando a gravação do
    /// lançamento falhou e o envio não o marca.
    #[test]
    fn a_second_agent_on_a_running_wave_is_refused_even_without_the_launch_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root);
        approve(root);
        let (pid, started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave":1,"role":"wave","text":assembled(root),"lines":1,"chars":8,"mustard":"0","author":"binary","claude_pid":pid,"claude_started":started}));
        let canonical = crate::commands::flow::round::wave_dispatch(root, "x", 1, Locale::PtBr);
        assert!(matches!(dispatch_to(root, "mustard-wave", &canonical), Verdict::Rewrite { .. } | Verdict::Allow), "the first agent goes");

        let log = spec_log(root, "x", Locale::PtBr).unwrap();
        let dispatch = log.dispatch_position(*log.last_by_wave("send").get(&1).unwrap());
        let folder = root.join(".claude/mustard/host-transcripts");
        std::fs::create_dir_all(&folder).unwrap();
        let record = json!({"host": "claude", "spec": "x", "wave": 1, "dispatch": dispatch, "launched": false});
        std::fs::write(folder.join("claude-sessao-agente.json"), record.to_string()).unwrap();

        let refused = denied(dispatch_to(root, "mustard-wave", &canonical));
        assert_eq!(refused, say("subagent.wave_has_agent", Locale::PtBr, &[("{spec}", "x"), ("{wave}", "1")]));
    }

    /// Num projeto que declara o código em português, o despacho a um agente
    /// do Mustard abre com a linha dos dois idiomas lida da configuração, e o
    /// resto do texto segue como veio. O texto que já traz a linha passa sem
    /// repeti-la; o despacho a outro agente passa como veio; e o pedido da
    /// onda traz a linha uma vez.
    #[test]
    fn a_mustard_agent_dispatch_opens_with_the_project_languages() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root);
        approve(root);
        std::fs::write(root.join("mustard.json"), r#"{"language":{"code":"pt-BR"}}"#).unwrap();
        let line = translate("prompt.languages", Locale::PtBr).replace("{text}", "pt-BR").replace("{code}", "pt-BR");

        let fix = rewritten(dispatch_to(root, "mustard-wave", "Conserte o teste da soma."));
        assert_eq!(fix, format!("{line}\n\nConserte o teste da soma."));

        assert_eq!(rewritten(dispatch_to(root, "mustard-wave", &fix)), fix, "the line is never repeated");
        let round = assembled(root);
        assert_eq!(round.matches(&line).count(), 1, "{round}");

        assert_eq!(dispatch_to(root, "general-purpose", "Investigue o gancho."), Verdict::Allow);
    }

    /// O despacho que o orquestrador escreve ao revisor — a revisão do
    /// levantamento, a do pull request — ganha no fim as regras do projeto: o
    /// texto do `CLAUDE.md` da raiz e, quando a pasta de um arquivo das
    /// tarefas da spec da branch tem regras, também as dela, cada uma sob o
    /// caminho; sem arquivo de regras, sai como antes, só com a linha dos
    /// idiomas. Não as ganham: o texto que já traz a seção, o despacho que
    /// manda ler o pedido gravado da revisão final, que já as traz, o conserto
    /// ao agente de onda e o pedido da onda.
    #[test]
    fn a_review_dispatch_written_by_the_conductor_ends_with_the_project_rules() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root);
        append_raw(root, "task", json!({"text": "Mexer na biblioteca.", "files": [{"path": "lib/sub/a.rs"}], "depends_on": []}), 900);
        approve(root);
        let init = std::process::Command::new("git").args(["init", "-q", "-b", "feature/x"]).current_dir(root).output().unwrap();
        assert!(init.status.success(), "{init:?}");
        std::fs::write(root.join("mustard.json"), r#"{"language":{"code":"pt-BR"}}"#).unwrap();
        let line = translate("prompt.languages", Locale::PtBr).replace("{text}", "pt-BR").replace("{code}", "pt-BR");
        let survey = "Confira o levantamento inteiro da spec x.";
        assert_eq!(rewritten(dispatch_to(root, "mustard-review", survey)), format!("{line}\n\n{survey}"), "no file");

        let rules = "# Regras\n\n- O instalador nunca grava na configuração do git.";
        std::fs::write(root.join("CLAUDE.md"), format!("{rules}\n")).unwrap();
        let review = rewritten(dispatch_to(root, "mustard-review", survey));
        let section = format!("## Regras do projeto\n\n{}\n\n{rules}", translate("prompt.project_rules.source", Locale::PtBr));
        assert_eq!(review, format!("{line}\n\n{survey}\n\n{section}"));
        assert_eq!(rewritten(dispatch_to(root, "mustard-review", &review)), review, "the section is never repeated");

        std::fs::create_dir_all(root.join("lib/sub")).unwrap();
        std::fs::write(root.join("lib/sub/CLAUDE.md"), "- A biblioteca não lê o disco.\n").unwrap();
        let nested = rewritten(dispatch_to(root, "mustard-review", survey));
        let sources = translate("prompt.project_rules.sources", Locale::PtBr);
        let both = format!("## Regras do projeto\n\n{sources}\n\n### `CLAUDE.md`\n\n{rules}\n\n### `lib/sub/CLAUDE.md`\n\n- A biblioteca não lê o disco.");
        assert_eq!(nested, format!("{line}\n\n{survey}\n\n{both}"));

        let final_review = format!("{line}\n\nmustard-rt run read request-review --root /r --spec x");
        assert_eq!(rewritten(dispatch_to(root, "mustard-review", &final_review)), final_review, "the recorded request has them");
        let fix = rewritten(dispatch_to(root, "mustard-wave", "Conserte o teste da soma."));
        assert_eq!(fix, format!("{line}\n\nConserte o teste da soma."));
        let wave = assembled(root);
        assert!(
            wave.contains("configuração do git") && !wave.contains("lê o disco"),
            "the wave receives the root rules and only its touched directory rules: {wave}"
        );
    }

    /// O despacho que abre com o título de uma onda que a spec não tem em
    /// andamento é barrado com o motivo: a onda ainda não saiu, a spec não
    /// existe. O que não abre com o título de uma onda segue ganhando a linha
    /// dos idiomas: o conserto, o título de outro assunto, o de uma onda que
    /// não existe e o título que não está na primeira linha.
    #[test]
    fn a_wave_title_that_is_not_in_progress_is_refused_and_other_texts_gain_the_language_line() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root);
        approve(root);
        std::fs::write(root.join("mustard.json"), r#"{"language":{"code":"pt-BR"}}"#).unwrap();
        let line = translate("prompt.languages", Locale::PtBr).replace("{text}", "pt-BR").replace("{code}", "pt-BR");
        let title = assembled(root).lines().next().unwrap_or_default().to_string();
        assert_eq!(title, "# x — onda 1");
        let command = "Leia o seu pedido inteiro com o comando abaixo:\n\nmustard-rt run read request-1 --root /r --spec x";
        let read_only=format!("Leia o seu pedido com: mustard-rt run read request-1 --root {} --spec x",mustard_core::io::wave_prompt::shown(root));
        assert_eq!(denied(dispatch_to(root,"mustard-wave",&read_only)),say("subagent.wave_not_running",Locale::PtBr,&[("{spec}","x"),("{wave}","1")]));

        for agent in ["mustard-wave", "mustard-review"] {
            assert_eq!(
                denied(dispatch_to(root, agent, &format!("{title}\n\n{command}"))),
                say("subagent.wave_not_running", Locale::PtBr, &[("{spec}", "x"), ("{wave}", "1")]),
                "{agent}"
            );
        }
        assert_eq!(
            denied(dispatch_to(root, "mustard-wave", &format!("# outra-obra — onda 12  \n\n{command}"))),
            Refusal::NoSpecFile { spec: "outra-obra".into() }.message(Locale::PtBr)
        );

        for not_a_wave in [
            format!("Conserte o teste da soma.\n\n{command}"),
            format!("# Conserte o teste da soma\n\n{command}"),
            format!("# x — onda 0\n\n{command}"),
            format!("Antes de tudo:\n{title}\n\n{command}"),
        ] {
            assert_eq!(rewritten(dispatch_to(root, "mustard-wave", &not_a_wave)), format!("{line}\n\n{not_a_wave}"));
        }
    }

    /// A spec `x` aprovada, com a onda 1 enviada pelo Claude Code `pid`, que
    /// começou em `started`, e a volta que o agente dela gravou depois. Devolve
    /// o arquivo do trecho de conserto dessa volta, ainda fora do disco, e o
    /// motivo com que o despacho de um agente novo é barrado.
    fn refused_wave(root: &Path, pid: u32, started: u64) -> (std::path::PathBuf, String) {
        planned(root);
        approve(root);
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        seed(
            "send",
            json!({"wave": 1, "role": "wave", "text": "o pedido", "lines": 1, "chars": 8, "mustard": "0",
            "author": "binary", "claude_pid": pid, "claude_started": started}),
        );
        seed("delivered", json!({"wave": 1, "text": "Feito.", "files": [], "returned": true}));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let fix = crate::commands::flow::round::fix_file(root, "x", &log, 1).expect("the pending return");
        std::fs::create_dir_all(fix.parent().unwrap()).unwrap();
        (fix, say("subagent.wave_not_running", Locale::PtBr, &[("{spec}", "x"), ("{wave}", "1")]))
    }

    /// O trecho que a rodada devolveu para a onda 1.
    const SECTION: &str = "Onda 1, rodada de conserto 1 de 2:\n- `src/a.rs` linha 1 importa o que a regra barra.";

    /// A onda recusada ganha um agente novo só: de dois despachos ao mesmo
    /// tempo, um passa com o trecho e o outro é barrado, e o despacho seguinte
    /// também, enquanto o Claude Code que abriu o agente novo segue aberto.
    /// Fechado também ele, a onda volta a aceitar um agente novo. O hospedeiro
    /// que despacha é o processo do próprio teste.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_second_new_agent_is_refused_while_the_first_ones_claude_code_is_open() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (pid, started) = crate::commands::flow::round::closed_process();
        let (fix, refused) = refused_wave(root, pid, started);
        std::fs::write(&fix, format!("{SECTION}\n")).unwrap();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let sent = format!("{title}\n\nConserte a onda.");
        let lead = say("subagent.new_agent_fix", Locale::PtBr, &[("{wave}", "1")]);
        let new_agent = format!(
            "{}\n\n{lead}\n\n{SECTION}{}",
            crate::commands::flow::round::wave_dispatch(root, "x", 1, Locale::PtBr),
            supplemental_section("Conserte a onda.", Locale::PtBr)
        );

        let together = std::sync::Barrier::new(2);
        let verdicts: Vec<Verdict> = std::thread::scope(|scope| {
            let dispatches: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        together.wait();
                        dispatch_from_this_process(root, &sent)
                    })
                })
                .collect();
            dispatches.into_iter().map(|dispatch| dispatch.join().unwrap()).collect()
        });
        let (passed, barred): (Vec<Verdict>, Vec<Verdict>) = verdicts.into_iter().partition(|verdict| matches!(verdict, Verdict::Rewrite { .. }));
        assert_eq!(passed.into_iter().map(rewritten).collect::<Vec<_>>(), vec![new_agent.clone()], "one new agent");
        assert_eq!(barred.into_iter().map(denied).collect::<Vec<_>>(), vec![refused.clone()]);
        assert_eq!(denied(dispatch_from_this_process(root, &sent)), refused, "the new agent's Claude Code is open");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let (pid, started) = crate::commands::flow::round::closed_process();
        let closed = json!({"claude_pid": pid, "claude_started": started}).as_object().cloned().unwrap();
        let draft = crate::commands::flow::round::send_revision(&log, 1, closed).expect("the wave's send");
        crate::shared::spec_state::seed_event(root, "x", "send", Value::Object(draft));
        assert_eq!(rewritten(dispatch_from_this_process(root, &sent)), new_agent, "its Claude Code closed too");
    }

    /// A onda cuja volta a rodada recusou, com o trecho de conserto em disco
    /// e o Claude Code do envio já fechado, sai a um agente novo: o título, o
    /// comando que lê o pedido, a frase de que o código já está na cópia e o
    /// trecho, com o recado ao condutor. Sem o trecho em disco, segue barrada.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_refused_wave_whose_sender_closed_goes_to_a_new_agent_with_the_fix() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (pid, started) = crate::commands::flow::round::closed_process();
        let (fix, refused) = refused_wave(root, pid, started);
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let sent = format!("{title}\n\nConserte a onda.");
        assert_eq!(denied(dispatch_to(root, "mustard-wave", &sent)), refused, "no section on disk");

        std::fs::write(&fix, format!("{SECTION}\n")).unwrap();
        let canonical = crate::commands::flow::round::wave_dispatch(root, "x", 1, Locale::PtBr);
        let lead = say("subagent.new_agent_fix", Locale::PtBr, &[("{wave}", "1")]);
        match dispatch_to(root, "mustard-wave", &sent) {
            Verdict::Rewrite { tool_input, note } => {
                assert_eq!(tool_input["prompt"], json!(format!("{canonical}\n\n{lead}\n\n{SECTION}{}", supplemental_section("Conserte a onda.", Locale::PtBr))));
                assert_eq!(note, Some(say("subagent.new_agent", Locale::PtBr, &[("{wave}", "1")])));
            }
            other => panic!("the dispatch goes to a new agent, got {other:?}"),
        }
    }

    /// Com o Claude Code do envio ainda aberto, o conserto é do agente que fez
    /// a onda: o agente novo segue barrado, mesmo com o trecho em disco.
    #[test]
    fn a_refused_wave_whose_sender_is_open_still_refuses_a_new_agent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (pid, started) = crate::commands::flow::stuck::this_process();
        let (fix, refused) = refused_wave(root, pid, started);
        std::fs::write(&fix, format!("{SECTION}\n")).unwrap();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        assert_eq!(denied(dispatch_to(root, "mustard-wave", &format!("{title}\n\nConserte a onda."))), refused);
    }

    /// A volta que a rodada recusou, com o trecho de conserto em disco e o par
    /// do envio já fechado, sai a um agente novo quando o envio é do Claude
    /// Code, de versão antiga ou não, ou de um Codex reconhecido. O envio de
    /// versão antiga feito no Codex, com o par da própria rodada e sem a marca
    /// do par do hospedeiro, conta como aberto: a volta recusada não espera
    /// agente novo, o despacho dele é barrado, e o conserto fica com o agente
    /// que fez a onda. O trecho cita um arquivo TypeScript.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_refused_return_of_an_older_codex_send_does_not_wait_for_a_new_agent() {
        let (pid, started) = crate::commands::flow::round::closed_process();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let sent = format!("{title}\n\nConserte a onda.");
        let refused = say("subagent.wave_not_running", Locale::PtBr, &[("{spec}", "x"), ("{wave}", "1")]);
        for (host, marked, new_agent) in [("codex", false, false), ("codex", true, true), ("claude", false, true), ("claude", true, true)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            planned(root);
            approve(root);
            let mut send = json!({"wave": 1, "role": "wave", "text": "o pedido", "lines": 1, "chars": 8, "mustard": "0",
                "author": "binary", "host": host, "claude_pid": pid, "claude_started": started});
            if marked {
                send["host_pair"] = json!(true);
            }
            crate::shared::spec_state::seed_event(root, "x", "send", send);
            crate::shared::spec_state::seed_event(root, "x", "delivered", json!({"wave": 1, "text": "Feito.", "files": [], "returned": true}));
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let fix = crate::commands::flow::round::fix_file(root, "x", &log, 1).expect("the pending return");
            std::fs::create_dir_all(fix.parent().unwrap()).unwrap();
            std::fs::write(&fix, "Onda 1, rodada de conserto 1 de 2:\n- `web/app.ts` linha 1 importa o que a regra barra.\n").unwrap();

            let verdict = dispatch_to(root, "mustard-wave", &sent);
            if new_agent {
                assert!(matches!(verdict, Verdict::Rewrite { .. }), "{host} marked={marked}: {verdict:?}");
            } else {
                assert_eq!(denied(verdict), refused, "{host} marked={marked}");
            }
        }
    }

    /// A onda de um envio de versão antiga feito no Codex, que guardava o
    /// processo da própria rodada no lugar do hospedeiro, e que quem conduz a
    /// obra reprovou, sai a um agente novo. O despacho grava o hospedeiro do
    /// agente novo como hospedeiro reconhecido: enquanto ele segue aberto, o
    /// outro agente novo é barrado; fechado ele, a onda volta a aceitar um. O
    /// hospedeiro que despacha é o processo do próprio teste.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_new_agent_of_an_older_codex_send_holds_the_wave_only_while_its_host_is_open() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root);
        approve(root);
        let (pid, started) = crate::commands::flow::round::closed_process();
        let seed = |event_type: &str, body: Value| crate::shared::spec_state::seed_event(root, "x", event_type, body);
        seed(
            "send",
            json!({"wave": 1, "role": "wave", "text": "o pedido", "lines": 1, "chars": 8, "mustard": "0",
            "author": "binary", "host": "codex", "claude_pid": pid, "claude_started": started}),
        );
        let back = seed("delivered", json!({"wave": 1, "text": "Feito.", "files": [], "returned": true}));
        let path = store::spec_file(root, "x").unwrap();
        let rejected = json!({"rejected": {"delivered": back, "reason": "Faltou o teste."}}).as_object().cloned().unwrap();
        let mut draft = crate::commands::flow::round::send_revision(&store::read(&path).unwrap().unwrap(), 1, rejected).expect("the wave's send");
        draft.remove("claude_pid");
        draft.remove("claude_started");
        seed("send", Value::Object(draft));
        let fix = crate::commands::flow::round::fix_file(root, "x", &store::read(&path).unwrap().unwrap(), 1).expect("the pending return");
        std::fs::create_dir_all(fix.parent().unwrap()).unwrap();
        std::fs::write(&fix, format!("{SECTION}\n")).unwrap();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let sent = format!("{title}\n\nConserte a onda.");
        let refused = say("subagent.wave_not_running", Locale::PtBr, &[("{spec}", "x"), ("{wave}", "1")]);

        assert!(matches!(dispatch_from_this_process(root, &sent), Verdict::Rewrite { .. }), "the rejected wave goes to a new agent");
        assert_eq!(denied(dispatch_from_this_process(root, &sent)), refused, "the new agent's host is open");
        let closed = json!({"claude_pid": pid, "claude_started": started}).as_object().cloned().unwrap();
        let draft = crate::commands::flow::round::send_revision(&store::read(&path).unwrap().unwrap(), 1, closed).expect("the wave's send");
        seed("send", Value::Object(draft));
        assert!(matches!(dispatch_from_this_process(root, &sent), Verdict::Rewrite { .. }), "the new agent's host closed");
    }

    /// O pedido ao agente de exploração ganha no topo a resposta curta do
    /// mapa, com o arquivo, a função e as linhas, e o resto do texto segue
    /// como veio; o mesmo pedido a outro agente, o pedido sem nome de código,
    /// o que o mapa não acha, o sem mapa e o com a chave `search.answer`
    /// desligada passam como vieram.
    #[test]
    fn the_request_to_an_explorer_opens_with_the_short_answer_of_the_map() {
        let (_dir, root) = crate::shared::word_search::fixture::repo("{}");
        let prompt = "Mapeie o cálculo do frete. Onde `calcular_frete` é usado? Leia src/frete.rs e responda em português.";
        let sent = rewritten(dispatch_to(&root, "Explore", prompt));
        assert!(sent.starts_with("Antes de explorar, o Mustard consultou o mapa com este pedido.\n"), "{sent}");
        assert!(sent.contains("src/frete.rs\n  2-6 calcular_frete"), "{sent}");
        assert!(sent.ends_with(&format!("\n\n{prompt}")), "the request follows as it came: {sent}");
        assert_eq!(dispatch_to(&root, "general-purpose", prompt), Verdict::Allow);
        assert_eq!(dispatch_to(&root, "Explore", "Explore o repositório inteiro e resuma."), Verdict::Allow);
        assert_eq!(dispatch_to(&root, "Explore", "Onde fica `zzyzx_quebrada`?"), Verdict::Allow);
        let (_off, off) = crate::shared::word_search::fixture::repo(r#"{"search":{"answer":false}}"#);
        assert_eq!(dispatch_to(&off, "Explore", prompt), Verdict::Allow);
        let (_bare, bare) = crate::shared::word_search::fixture::repo("{}");
        std::fs::remove_file(mustard_core::io::project_map::model_path(&bare)).unwrap();
        assert_eq!(dispatch_to(&bare, "Explore", prompt), Verdict::Allow, "no map");
    }

    /// O pedido ao agente de exploração cuja busca o mapa acha só em parte
    /// (a palavra está no comentário da função, não no nome) passa como veio,
    /// sem o bloco do mapa; o pedido cuja busca o mapa crava ganha o bloco, e
    /// o mesmo pedido parcial a outro agente também passa como veio.
    #[test]
    fn a_partial_request_to_an_explorer_goes_without_the_block_of_the_map() {
        let (_dir, root) = crate::shared::word_search::fixture::repo("{}");
        let partial = "Onde fica o `imposto` do frete? Responda em português.";
        let pinned = "Onde `calcular_frete` é usado? Leia src/frete.rs e responda em português.";
        assert_eq!(dispatch_to(&root, "Explore", partial), Verdict::Allow, "the partial answer stays out of the request");
        assert_eq!(dispatch_to(&root, "general-purpose", partial), Verdict::Allow);
        let sent = rewritten(dispatch_to(&root, "Explore", pinned));
        assert!(sent.starts_with("Antes de explorar, o Mustard consultou o mapa com este pedido.\n"), "{sent}");
        assert!(sent.contains("Cravado.") && sent.contains("src/frete.rs\n  2-6 calcular_frete"), "{sent}");
        assert!(sent.ends_with(&format!("\n\n{pinned}")), "the request follows as it came: {sent}");
    }

    /// A linha `MUSTARD-WAVE:` de antes é só texto: o despacho que a traz
    /// passa como veio, sem o gancho montar pedido nenhum.
    #[test]
    fn an_old_wave_ticket_is_plain_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        planned(root);
        approve(root);
        assert_eq!(dispatch(root, "MUSTARD-WAVE: x 1"), Verdict::Allow);
    }

    /// Uma tarefa qualquer passa como veio, e o gancho não age fora do
    /// despacho. Fora de um projeto com `mustard.json`, nem o despacho a um
    /// agente do Mustard ganha a linha dos idiomas.
    #[test]
    fn a_plain_task_passes_untouched() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert_eq!(dispatch(root, "Investigue o gancho.\nSKILL: foo"), Verdict::Allow);
        assert_eq!(dispatch_to(root, "mustard-wave", "Conserte o teste da soma."), Verdict::Allow);
        let input = HookInput { tool_name: Some("Task".to_string()), tool_input: json!({ "prompt": "Faça a onda." }), ..HookInput::default() };
        let after = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PostToolUse));
        assert_eq!(SubagentInject.evaluate(&input, &after).unwrap(), Verdict::Allow);
    }
}
