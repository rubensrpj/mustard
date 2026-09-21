//! `conversation_size` — o tamanho da conversa.
//!
//! Uma leitura só: o tamanho é a soma de `input_tokens`,
//! `cache_read_input_tokens` e `cache_creation_input_tokens` do último uso
//! gravado no arquivo de transcrição. Dois usos, os dois no gancho
//! [`WavePauseCheck`]: (1) o agente de onda, depois de cada ferramenta,
//! passando de 200 mil, recebe a ordem de gravar o passo e parar com
//! `<PAUSED>{"wave":n}</PAUSED>`, e a ordem repete a cada ferramenta até ele
//! parar; (2) o orquestrador, antes de cada ferramenta, passando de 200 mil,
//! tem a chamada recusada — com ou sem onda em andamento —, e o motivo da
//! recusa traz o bloco de retomada pronto para colar: a spec, a fase, as
//! ondas entregues, as em andamento, o que falta e o próximo passo. A recusa
//! repete a cada ferramenta, sem controle de "já avisado", porque um aviso
//! que pode ser ignorado já foi tentado e não bastou. Ainda em
//! `prompt_entry`, a entrada de cada mensagem carrega o mesmo bloco, uma vez
//! por novo degrau de 200 mil, como aviso que não barra — o degrau guardado
//! acompanha a conversa que encolheu, então um `/compact` de verdade não
//! cala o próximo aviso.
//!
//! Qual conversa: a do agente de onda quando a chamada é da cópia de uma
//! onda, pela mesma leitura de [`wave_of_call`], do observador do sinal de
//! vida — o arquivo dele fica em `<pasta da sessão>/subagents/agent-<id>.jsonl`,
//! com o `agent_id` do registro quando ele vier, e sem ele o arquivo mais
//! recente da pasta cuja primeira mensagem é o pedido daquela onda. Fora da
//! cópia, a conversa principal, em `transcript_path`. Sem arquivo legível,
//! nada acontece: [`Verdict::Allow`], porque sem tamanho conhecido não há
//! como decidir.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_events::{Block, BlockQuery};
use mustard_core::io::fs;
use mustard_core::platform::error::Error;
use mustard_core::{translate, ClaudePaths, ProjectConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hooks::observe::wave_alive_observer::wave_of_call;

/// O degrau de tamanho que dispara o aviso, em tokens.
pub(crate) const THRESHOLD: u64 = 200_000;

/// A soma de `input_tokens`, `cache_read_input_tokens` e
/// `cache_creation_input_tokens` do último uso gravado no arquivo de
/// transcrição `path` — uma linha JSON por evento, e o uso mora em
/// `message.usage`, ou na raiz da linha quando ela já é o uso. `None` sem
/// arquivo legível ou sem nenhum uso gravado nele.
pub(crate) fn tokens_in(path: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut last = None;
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
        let usage = value.get("message").and_then(|m| m.get("usage")).or_else(|| value.get("usage"));
        let Some(usage) = usage else { continue };
        let field = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
        last = Some(field("input_tokens") + field("cache_read_input_tokens") + field("cache_creation_input_tokens"));
    }
    last
}

/// O texto de `message.content` da primeira linha de `path`, como o pedido de
/// uma onda chega na primeira mensagem da conversa do agente.
fn first_message_text(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let first = text.lines().next()?;
    let value: Value = serde_json::from_str(first).ok()?;
    let content = value.get("message")?.get("content")?;
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let joined: String =
                items.iter().filter_map(|b| b.get("text")).filter_map(Value::as_str).collect();
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

/// O pedido gravado no envio mais novo da onda `wave` da spec `spec`, em
/// `root` — o mesmo texto que a rodada mandou ao agente.
fn recorded_prompt(root: &Path, spec: &str, wave: u64) -> Option<String> {
    let path = mustard_core::io::spec_events::spec_file(root, spec).ok()?;
    let log = mustard_core::io::spec_events::read(&path).ok()??;
    log.block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .rfind(|e| e.event_type == "send" && e.wave() == Some(wave))
        .and_then(|e| e.str_field("text").map(str::to_string))
}

/// O arquivo `.jsonl` mais recente de `dir` cuja primeira mensagem é `wanted`;
/// sem casar nenhum, o mais recente da pasta. `None` numa pasta vazia ou que
/// não existe.
fn pick_subagent_file(dir: &Path, wanted: Option<&str>) -> Option<PathBuf> {
    let mut entries: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("jsonl"))
        .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (e.path(), t)))
        .collect();
    entries.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    if let Some(wanted) = wanted {
        let wanted = wanted.trim();
        if let Some((path, _)) =
            entries.iter().find(|(path, _)| first_message_text(path).as_deref().map(str::trim) == Some(wanted))
        {
            return Some(path.clone());
        }
    }
    entries.into_iter().next().map(|(path, _)| path)
}

/// O arquivo de transcrição que conta para esta chamada: a conversa principal
/// fora da cópia de uma onda, e a do agente de onda dentro dela. `None` sem
/// `transcript_path`, sem `cwd`, ou sem arquivo achável dentro da cópia.
pub(crate) fn conversation_path(root: &Path, input: &HookInput) -> Option<PathBuf> {
    let transcript = input.raw.get("transcript_path").and_then(Value::as_str)?;
    let transcript = Path::new(transcript);
    let Some((spec, wave)) = wave_of_call(root, input) else {
        return Some(transcript.to_path_buf());
    };
    let session_dir = transcript.to_string_lossy();
    let session_dir = session_dir.strip_suffix(".jsonl").unwrap_or(&session_dir);
    let subagents = Path::new(session_dir).join("subagents");
    if let Some(agent_id) = input.agent_id.as_deref().filter(|id| !id.is_empty()) {
        return Some(subagents.join(format!("agent-{agent_id}.jsonl")));
    }
    let wanted = recorded_prompt(root, &spec, wave);
    pick_subagent_file(&subagents, wanted.as_deref())
}

/// A instrução, nos dois idiomas, para o agente de onda `wave` gravar o passo
/// e parar.
fn pause_text(wave: u64, lang: mustard_core::platform::i18n::Locale) -> String {
    translate("conversation_size.pause", lang).replace("{wave}", &wave.to_string())
}

/// O gancho do tamanho da conversa: no agente de onda, depois de cada
/// ferramenta, na cópia dela, passando de [`THRESHOLD`], manda gravar o passo
/// e parar. Em quem conduz, antes de cada ferramenta, no mesmo teto, recusa a
/// chamada com o bloco de retomada pronto para colar. Os dois repetem a cada
/// ferramenta, porque nada aqui grava se o aviso já foi dado — a recusa que
/// pode ser ignorada já foi tentada, e não bastou.
pub struct WavePauseCheck;

impl Check for WavePauseCheck {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        match ctx.trigger {
            Some(Trigger::PostToolUse) => Self::wave_pause(input, &root),
            Some(Trigger::PreToolUse) => Self::orchestrator_block(input, &root),
            _ => Ok(Verdict::Allow),
        }
    }
}

impl WavePauseCheck {
    /// O agente de onda, depois de cada ferramenta na cópia dela, passando de
    /// [`THRESHOLD`], é mandado gravar o passo e parar.
    fn wave_pause(input: &HookInput, root: &Path) -> Result<Verdict, Error> {
        // Só a própria chamada do agente de onda pausa: o orquestrador pode
        // citar o caminho da cópia (um `git -C`, por exemplo) sem ser dela —
        // `wave_of_call` casaria pelo comando, e a ordem de pausa vazaria para
        // a conversa dele. `agent_id` é o sinal do harness que só vem de
        // dentro de uma chamada de subagente.
        if !input.is_subagent() {
            return Ok(Verdict::Allow);
        }
        let Some((_, wave)) = wave_of_call(root, input) else { return Ok(Verdict::Allow) };
        let Some(path) = conversation_path(root, input) else { return Ok(Verdict::Allow) };
        let Some(tokens) = tokens_in(&path) else { return Ok(Verdict::Allow) };
        if tokens < THRESHOLD {
            return Ok(Verdict::Allow);
        }
        let lang = ProjectConfig::load(root).language().text_or_default();
        Ok(Verdict::Inject { context: pause_text(wave, lang) })
    }

    /// Quem conduz, antes de cada ferramenta da própria conversa (nunca a de
    /// um subagente), passando de [`THRESHOLD`], tem a chamada recusada, com
    /// o bloco de retomada no motivo.
    fn orchestrator_block(input: &HookInput, root: &Path) -> Result<Verdict, Error> {
        if input.is_subagent() {
            return Ok(Verdict::Allow);
        }
        let Some(transcript) = input.raw.get("transcript_path").and_then(Value::as_str) else {
            return Ok(Verdict::Allow);
        };
        let Some(tokens) = tokens_in(Path::new(transcript)) else { return Ok(Verdict::Allow) };
        if tokens < THRESHOLD {
            return Ok(Verdict::Allow);
        }
        let Some(reason) = resume_block(root, input.session_id.as_deref()) else { return Ok(Verdict::Allow) };
        Ok(Verdict::Deny { reason })
    }
}

/// O que a sessão `sid`, sob `root`, já avisou: o maior degrau de 200 mil já
/// dado ao orquestrador.
#[derive(Debug, Default, Serialize, Deserialize)]
struct CompactRecord {
    #[serde(default)]
    warned: u64,
}

/// `.claude/.session/<sid>/compact.json`. `None` sem sessão utilizável.
fn record_path(root: &Path, session: Option<&str>) -> Option<PathBuf> {
    let sid = session?.trim();
    if sid.is_empty() || sid == "unknown" || sid.contains(['/', '\\']) || sid.contains("..") {
        return None;
    }
    Some(ClaudePaths::for_project(root).ok()?.claude_dir().join(".session").join(sid).join("compact.json"))
}

fn read_record(path: &Path) -> CompactRecord {
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn write_record(path: &Path, record: &CompactRecord) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(record) {
        let _ = fs::write_atomic(path, &bytes);
    }
}

/// As ondas entregues, as em andamento e as que faltam — planejadas, nem
/// entregues nem em andamento — da spec de `log`.
fn wave_lists(log: &mustard_core::domain::spec_events::SpecLog) -> (Vec<u64>, Vec<u64>, Vec<u64>) {
    let delivered = log.delivered_waves();
    let running: std::collections::BTreeSet<u64> =
        crate::commands::flow::round::waves_in_progress(log).into_keys().collect();
    let planned: std::collections::BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "wave")
        .filter_map(|e| e.wave())
        .collect();
    let missing: Vec<u64> =
        planned.into_iter().filter(|n| !delivered.contains(n) && !running.contains(n)).collect();
    (delivered.into_iter().collect(), running.into_iter().collect(), missing)
}

/// `waves`, separadas por vírgula, ou "nenhuma"/"none" quando vazia.
fn join_waves(waves: &[u64], lang: mustard_core::platform::i18n::Locale) -> String {
    if waves.is_empty() {
        return translate("resume.none", lang).to_string();
    }
    waves.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")
}

/// O bloco de retomada: spec, fase, ondas entregues, ondas em andamento e o
/// que falta — a mesma peça que entra tanto no aviso de compactar quanto no
/// motivo da recusa a quem conduz.
fn resume_block_text(
    spec: &str,
    phase: &str,
    log: &mustard_core::domain::spec_events::SpecLog,
    lang: mustard_core::platform::i18n::Locale,
) -> String {
    let (delivered, running, missing) = wave_lists(log);
    translate("conversation_size.block", lang)
        .replace("{spec}", spec)
        .replace("{phase}", phase)
        .replace("{delivered}", &join_waves(&delivered, lang))
        .replace("{running}", &join_waves(&running, lang))
        .replace("{missing}", &join_waves(&missing, lang))
}

/// A spec atual, a fase dela e o log lido, para `session` sob `root`. `None`
/// sem spec atual ou sem arquivo de eventos legível.
fn active_spec_log(
    root: &Path,
    session: Option<&str>,
) -> Option<(crate::commands::spec_events::Project, String, mustard_core::domain::spec_events::SpecLog)> {
    use mustard_core::domain::spec_state::SpecState;

    let project = crate::commands::spec_events::project(root);
    let spec = crate::shared::spec_state::DiskSpecState::new(&crate::commands::spec_events::read::checkout(root))
        .active(session)?;
    let log = mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(&project.root, &spec).ok()?)
        .ok()??;
    Some((project, spec, log))
}

/// O `command` e o `next` do passo seguinte da spec `spec`, pela mesma
/// leitura do comando `resume`.
fn next_step(root: &Path, spec: &str, session: Option<&str>) -> (String, String) {
    let resume = crate::commands::flow::resume::resume_for(
        &crate::commands::flow::resume::ResumeOpts { root: root.to_path_buf(), spec: Some(spec.to_string()) },
        session,
    );
    (
        resume["command"].as_str().unwrap_or_default().to_string(),
        resume["next"].as_str().unwrap_or_default().to_string(),
    )
}

/// O aviso ao orquestrador, com o comando `/compact` pronto e o bloco de
/// retomada — spec, fase, ondas entregues, em andamento, o que falta e o
/// próximo passo —, quando a conversa em `root`, na sessão `session`, passou
/// de um novo degrau de [`THRESHOLD`]. O bloco sai sempre, com onda em
/// andamento ou sem: a linha das ondas no ar é uma parte dele, não um
/// substituto. `None` sem novo degrau ou sem o que resumir.
///
/// O degrau guardado acompanha a conversa que encolheu (depois de um
/// `/compact` de verdade): quando o tamanho atual já está abaixo do degrau
/// avisado, a conta recomeça dali, para o próximo degrau avisar de novo — sem
/// isso, um degrau avisado antes da compactação calava o aviso para sempre,
/// mesmo a conversa voltando a crescer.
pub(crate) fn compact_notice(root: &Path, session: Option<&str>, tokens: u64) -> Option<String> {
    let degree = tokens / THRESHOLD;
    let path = record_path(root, session)?;
    let mut record = read_record(&path);
    if degree < record.warned {
        record.warned = degree;
        write_record(&path, &record);
    }
    if degree == 0 || degree <= record.warned {
        return None;
    }
    let (project, spec, log) = active_spec_log(root, session)?;
    record.warned = degree;
    write_record(&path, &record);
    let phase = mustard_core::domain::spec_state::State::from_log(&log).phase.unwrap_or("survey");
    let block = resume_block_text(&spec, phase, &log, project.lang);
    let (command, next) = next_step(&project.root, &spec, session);
    Some(
        translate("conversation_size.compact", project.lang)
            .replace("{block}", &block)
            .replace("{command}", &command)
            .replace("{next}", &next),
    )
}

/// O bloco de retomada pronto para colar, sempre — sem o controle de "já
/// avisado" do aviso de compactar: a recusa a quem conduz não pode ser
/// ignorada, então repete a cada ferramenta enquanto a conversa segue acima
/// do teto. `None` sem spec atual ou sem arquivo de eventos.
pub(crate) fn resume_block(root: &Path, session: Option<&str>) -> Option<String> {
    let (project, spec, log) = active_spec_log(root, session)?;
    let phase = mustard_core::domain::spec_state::State::from_log(&log).phase.unwrap_or("survey");
    let block = resume_block_text(&spec, phase, &log, project.lang);
    let (command, next) = next_step(&project.root, &spec, session);
    Some(
        translate("conversation_size.blocked", project.lang)
            .replace("{block}", &block)
            .replace("{command}", &command)
            .replace("{next}", &next),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_usage_line(input: u64, cache_read: u64, cache_creation: u64) -> String {
        serde_json::json!({
            "message": {"usage": {
                "input_tokens": input,
                "cache_read_input_tokens": cache_read,
                "cache_creation_input_tokens": cache_creation,
            }}
        })
        .to_string()
    }

    /// O tamanho é a soma dos três campos do ÚLTIMO uso gravado, não do
    /// primeiro nem de uma soma de todos.
    #[test]
    fn the_size_is_the_sum_of_the_last_recorded_usage() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let lines = [
            write_usage_line(10, 20, 30),
            "not json".to_string(),
            write_usage_line(100_000, 50_000, 49_999),
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();
        assert_eq!(tokens_in(&path), Some(199_999));
    }

    /// Sem arquivo, ou sem uso gravado nele, o tamanho é desconhecido.
    #[test]
    fn without_a_readable_file_or_usage_the_size_is_none() {
        let dir = tempdir().unwrap();
        assert_eq!(tokens_in(&dir.path().join("nao-existe.jsonl")), None);
        let empty = dir.path().join("vazio.jsonl");
        std::fs::write(&empty, "{}\n").unwrap();
        assert_eq!(tokens_in(&empty), None);
    }

    /// O agente de onda, na cópia dela, depois de cada ferramenta, é mandado
    /// gravar o passo e parar ao passar de 200 mil tokens — na divisa,
    /// 199.999 não avisa e 200.000 avisa —, com a linha `<PAUSED>` da onda
    /// certa. Fora da cópia de uma onda, nunca.
    #[test]
    fn the_wave_agent_is_told_to_pause_past_the_threshold() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = root.join(".claude").join("worktrees").join("mustard-x-3");
        std::fs::create_dir_all(&copy).unwrap();
        let subagents = root.join("s1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        let agent_file = subagents.join("agent-a1.jsonl");
        let transcript = root.join("s1.jsonl");

        let input = || HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("Bash".to_string()),
            cwd: Some(copy.to_string_lossy().into_owned()),
            agent_id: Some("a1".to_string()),
            raw: serde_json::json!({ "transcript_path": transcript.to_string_lossy() }),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::PostToolUse));

        std::fs::write(&agent_file, write_usage_line(199_999, 0, 0)).unwrap();
        assert_eq!(WavePauseCheck.evaluate(&input(), &ctx).unwrap(), Verdict::Allow, "abaixo do degrau, sem ordem");

        std::fs::write(&agent_file, write_usage_line(200_000, 0, 0)).unwrap();
        let Verdict::Inject { context } = WavePauseCheck.evaluate(&input(), &ctx).unwrap() else {
            panic!("no degrau, a onda 3 é mandada parar");
        };
        assert!(context.contains("<PAUSED>{\"wave\":3}</PAUSED>"), "{context}");

        // Fora da cópia de uma onda, nada, mesmo passando do degrau.
        let outside =
            HookInput { cwd: Some(root.to_string_lossy().into_owned()), ..input() };
        assert_eq!(WavePauseCheck.evaluate(&outside, &ctx).unwrap(), Verdict::Allow);
    }

    /// O agente de onda lê e edita pelo caminho, sem mudar de pasta: a pasta
    /// de trabalho da chamada é a do projeto, não a da cópia. A pausa aos
    /// 200 mil ainda dispara, achando a onda pelo caminho do arquivo pedido
    /// (`file_path`).
    #[test]
    fn the_wave_is_found_by_the_paths_of_the_call() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = root.join(".claude").join("worktrees").join("mustard-x-3");
        std::fs::create_dir_all(&copy).unwrap();
        let subagents = root.join("s1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        let agent_file = subagents.join("agent-a1.jsonl");
        let transcript = root.join("s1.jsonl");
        std::fs::write(&agent_file, write_usage_line(200_000, 0, 0)).unwrap();

        let input = HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("Read".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            agent_id: Some("a1".to_string()),
            tool_input: serde_json::json!({ "file_path": copy.join("src").join("lib.rs").to_string_lossy() }),
            raw: serde_json::json!({ "transcript_path": transcript.to_string_lossy() }),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::PostToolUse));

        let Verdict::Inject { context } = WavePauseCheck.evaluate(&input, &ctx).unwrap() else {
            panic!("achada pelo file_path, a onda 3 é mandada parar");
        };
        assert!(context.contains("<PAUSED>{\"wave\":3}</PAUSED>"), "{context}");
    }

    /// O orquestrador que só cita o caminho da cópia de uma onda num comando
    /// (por exemplo, um `git -C` para olhar o que ela mudou) não é o agente
    /// dela: sem `agent_id`, com a pasta de trabalho no repositório principal,
    /// a chamada não recebe a ordem de pausa, mesmo com o arquivo do agente de
    /// onda acima do degrau.
    #[test]
    fn the_orchestrators_own_call_is_not_taken_for_the_wave_agent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let subagents = root.join("s1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        let agent_file = subagents.join("agent-a1.jsonl");
        std::fs::write(&agent_file, write_usage_line(200_000, 0, 0)).unwrap();
        let transcript = root.join("s1.jsonl");

        let input = HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("Bash".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            agent_id: None,
            tool_input: serde_json::json!({
                "command": "git -C .claude/worktrees/mustard-x-35 log -1"
            }),
            raw: serde_json::json!({ "transcript_path": transcript.to_string_lossy() }),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::PostToolUse));

        assert_eq!(
            WavePauseCheck.evaluate(&input, &ctx).unwrap(),
            Verdict::Allow,
            "o orquestrador não é a onda 35, mesmo citando a cópia dela no comando",
        );
    }

    /// Um projeto instalado, com a spec `spec` aprovada e o checkout parado
    /// na branch dela — o mesmo que a chamada de quem conduz vê.
    fn open_project(spec: &str) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
        crate::shared::spec_state::stand_on_spec_branch(root, spec);
        crate::commands::spec_events::write::record_open(root, spec, &format!("feature/{spec}"), "dev")
            .expect("open");
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        dir
    }

    /// Grava a onda 1 da spec `spec`, em `root`, em andamento — o mesmo
    /// pedido que a rodada grava, com o pid deste processo, que segue vivo
    /// durante o teste, para que `waves_in_progress` a conte como rodando.
    fn seed_running_wave(root: &Path, spec: &str) {
        let said =
            crate::shared::spec_state::seed_event(root, spec, "message", serde_json::json!({"author": "user", "text": "o plano"}));
        let crit = crate::shared::spec_state::seed_event(
            root,
            spec,
            "criterion",
            serde_json::json!({"when": "a", "then": "b", "proof": "p", "origin": said}),
        );
        crate::shared::spec_state::seed_event(
            root,
            spec,
            "wave",
            serde_json::json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "x", "origin": said}),
        );
        crate::shared::spec_state::seed_event(root, spec, "state", serde_json::json!({"phase": "running", "author": "binary"}));
        let (pid, started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(
            root,
            spec,
            "send",
            serde_json::json!({"wave": 1, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [crit], "mustard": "0", "author": "binary",
                "claude_pid": pid, "claude_started": started}),
        );
    }

    /// A chamada de ferramenta de quem conduz, com a transcrição `transcript`.
    fn conductor_call(root: &Path, transcript: &Path) -> HookInput {
        HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Bash".to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            agent_id: None,
            tool_input: serde_json::json!({ "command": "ls" }),
            raw: serde_json::json!({ "transcript_path": transcript.to_string_lossy() }),
            ..HookInput::default()
        }
    }

    /// Abaixo do teto, a próxima chamada de ferramenta de quem conduz passa;
    /// acima dele, é recusada com `Verdict::Deny`, e o motivo traz o bloco de
    /// retomada — spec, fase e o próximo passo —, com onda rodando ou sem: a
    /// linha das ondas em andamento é uma parte do bloco, não um substituto
    /// que o silencia. O gancho é o mesmo dos dois lados: o próprio
    /// `WavePauseCheck.evaluate`, no `PreToolUse`.
    #[test]
    fn the_conductor_is_denied_with_the_resume_block_whether_or_not_a_wave_runs() {
        let dir = open_project("x");
        let root = dir.path();
        let transcript = root.join("t.jsonl");
        let ctx = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::PreToolUse));

        std::fs::write(&transcript, write_usage_line(199_999, 0, 0)).unwrap();
        assert_eq!(
            WavePauseCheck.evaluate(&conductor_call(root, &transcript), &ctx).unwrap(),
            Verdict::Allow,
            "abaixo do degrau, a chamada passa",
        );

        std::fs::write(&transcript, write_usage_line(200_000, 0, 0)).unwrap();
        let Verdict::Deny { reason } = WavePauseCheck.evaluate(&conductor_call(root, &transcript), &ctx).unwrap()
        else {
            panic!("no degrau, a próxima chamada de quem conduz é recusada");
        };
        assert!(reason.contains('x'), "o bloco traz a spec: {reason}");
        assert!(reason.contains("fase"), "o bloco traz a fase: {reason}");

        // Com uma onda em andamento, o bloco continua saindo, com a onda
        // citada — antes do conserto, só a frase curta das ondas no ar saía,
        // e o bloco de retomada nunca aparecia neste caso, que é o de sempre.
        seed_running_wave(root, "x");
        let Verdict::Deny { reason } = WavePauseCheck.evaluate(&conductor_call(root, &transcript), &ctx).unwrap()
        else {
            panic!("com onda em andamento, a recusa e o bloco continuam saindo");
        };
        assert!(reason.contains("fase"), "o bloco não vira só a frase das ondas no ar: {reason}");
        assert!(reason.contains('1'), "o bloco cita a onda em andamento: {reason}");
    }

    /// Uma linha de uso por ferramenta, como um `transcript_path` de verdade
    /// acumula — texto sem uso misturado no meio, e o tamanho é o do último
    /// uso gravado, não uma soma. A mesma recusa sai desta transcrição
    /// inteira de sessão, não só de um arquivo de uma linha só.
    #[test]
    fn the_conductor_is_denied_with_a_full_session_transcript() {
        let dir = open_project("y");
        let root = dir.path();
        let transcript = root.join("t.jsonl");
        let ctx = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::PreToolUse));

        let session_below = [
            serde_json::json!({"type": "user", "message": {"role": "user", "content": "oi"}}).to_string(),
            write_usage_line(40_000, 10_000, 5_000),
            serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": "ok"}}).to_string(),
            write_usage_line(90_000, 40_000, 20_000),
            write_usage_line(120_000, 50_000, 29_999),
        ];
        std::fs::write(&transcript, session_below.join("\n")).unwrap();
        assert_eq!(
            WavePauseCheck.evaluate(&conductor_call(root, &transcript), &ctx).unwrap(),
            Verdict::Allow,
            "199.999 no total, ainda abaixo do degrau",
        );

        let session_above: Vec<String> =
            session_below.iter().cloned().chain(std::iter::once(write_usage_line(120_000, 50_000, 30_000))).collect();
        std::fs::write(&transcript, session_above.join("\n")).unwrap();
        let Verdict::Deny { reason } = WavePauseCheck.evaluate(&conductor_call(root, &transcript), &ctx).unwrap()
        else {
            panic!("200.000 no total: a chamada é recusada, com a transcrição inteira da sessão");
        };
        assert!(reason.contains('y'), "o bloco traz a spec: {reason}");
        assert!(reason.contains("fase"), "o bloco traz a fase: {reason}");
    }
}
