//! `conversation_size` — o tamanho da conversa.
//!
//! Uma leitura só: o tamanho é a soma de `input_tokens`,
//! `cache_read_input_tokens` e `cache_creation_input_tokens` do último uso
//! gravado no arquivo de transcrição. Dois usos, os dois em `prompt_entry` e
//! no gancho de onda: (1) o agente de onda, depois de cada ferramenta,
//! passando de 200 mil, recebe a ordem de gravar o passo e parar com
//! `<PAUSED>{"wave":n}</PAUSED>`, e a ordem repete a cada ferramenta até ele
//! parar; (2) o orquestrador, na entrada de cada mensagem, sem onda em
//! andamento, ganha o aviso com o comando `/compact` pronto e o resumo do que
//! fica, a cada novo degrau de 200 mil.
//!
//! Qual conversa: a do agente de onda quando a pasta de trabalho é a cópia de
//! uma onda, pela mesma leitura de [`wave_of_copy`], do observador do sinal de
//! vida — o arquivo dele fica em `<pasta da sessão>/subagents/agent-<id>.jsonl`,
//! com o `agent_id` do registro quando ele vier, e sem ele o arquivo mais
//! recente da pasta cuja primeira mensagem é o pedido daquela onda. Fora da
//! cópia, a conversa principal, em `transcript_path`. Sem arquivo legível,
//! nada acontece: [`Verdict::Allow`], porque o gancho só avisa e nunca trava
//! a resposta por conta própria.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_events::{Block, BlockQuery};
use mustard_core::io::fs;
use mustard_core::platform::error::Error;
use mustard_core::{translate, ClaudePaths, ProjectConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hooks::observe::wave_alive_observer::wave_of_copy;

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
        .filter(|e| e.event_type == "send" && e.wave() == Some(wave))
        .next_back()
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
    entries.sort_by(|a, b| b.1.cmp(&a.1));
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
    let Some(cwd) = input.cwd.as_deref().filter(|c| !c.is_empty()) else {
        return Some(transcript.to_path_buf());
    };
    let Some((spec, wave)) = wave_of_copy(root, Path::new(cwd)) else {
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

/// O gancho de onda: depois de cada ferramenta, na cópia de uma onda, passando
/// de [`THRESHOLD`], manda o agente gravar o passo e parar. Repete a cada
/// ferramenta, porque nada aqui grava se o aviso já foi dado.
pub struct WavePauseCheck;

impl Check for WavePauseCheck {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return Ok(Verdict::Allow);
        }
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        let Some(cwd) = input.cwd.as_deref().filter(|c| !c.is_empty()) else { return Ok(Verdict::Allow) };
        let Some((_, wave)) = wave_of_copy(&root, Path::new(cwd)) else { return Ok(Verdict::Allow) };
        let Some(path) = conversation_path(&root, input) else { return Ok(Verdict::Allow) };
        let Some(tokens) = tokens_in(&path) else { return Ok(Verdict::Allow) };
        if tokens < THRESHOLD {
            return Ok(Verdict::Allow);
        }
        let lang = ProjectConfig::load(&root).language().text_or_default();
        Ok(Verdict::Inject { context: pause_text(wave, lang) })
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

/// O aviso ao orquestrador, com o comando `/compact` pronto e o resumo do que
/// fica — a spec, a fase, o próximo passo e o que espera o usuário, pela
/// mesma leitura do comando `resume` — quando a conversa em `root`, na sessão
/// `session`, passou de um novo degrau de [`THRESHOLD`] e nenhuma onda está em
/// andamento na spec atual. `None` sem novo degrau, com onda em andamento, ou
/// sem o que resumir.
pub(crate) fn compact_notice(root: &Path, session: Option<&str>, tokens: u64) -> Option<String> {
    use mustard_core::domain::spec_state::SpecState;

    let degree = tokens / THRESHOLD;
    if degree == 0 {
        return None;
    }
    let Some(path) = record_path(root, session) else { return None };
    let mut record = read_record(&path);
    if degree <= record.warned {
        return None;
    }
    let project = crate::commands::spec_events::project(root);
    let spec = crate::shared::spec_state::DiskSpecState::new(&crate::commands::spec_events::read::checkout(root))
        .active(session)?;
    let log = mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(&project.root, &spec).ok()?)
        .ok()??;
    if !crate::commands::flow::round::waves_in_progress(&log).is_empty() {
        return None;
    }
    let resume = crate::commands::flow::resume::resume_for(
        &crate::commands::flow::resume::ResumeOpts { root: project.root.clone(), spec: Some(spec.clone()) },
        session,
    );
    let phase = resume["phase"].as_str().unwrap_or_default();
    let next = resume["next"].as_str().unwrap_or_default();
    let command = resume["command"].as_str().unwrap_or_default();
    record.warned = degree;
    write_record(&path, &record);
    Some(
        translate("conversation_size.compact", project.lang)
            .replace("{spec}", &spec)
            .replace("{phase}", phase)
            .replace("{command}", command)
            .replace("{next}", next),
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
        let lines = vec![
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
}
