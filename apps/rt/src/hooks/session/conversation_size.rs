//! `conversation_size` — o bloco de retomada antes de compactar.
//!
//! Um gancho só, [`PrecompactNotice`], no evento `PreCompact`: antes de toda
//! compactação — manual (`/compact`) ou automática, a que o próprio Claude
//! Code dispara sozinho quando a conversa cresce —, a conversa recebe o
//! bloco de retomada pronto para colar: a spec, a fase, as ondas entregues,
//! as em andamento e o que falta, com o próximo comando. Sem controle de "já
//! avisado": o próprio `PreCompact` já é o degrau, então cada compactação
//! merece o bloco de novo, e não há como ele ficar velho.
//!
//! O degrau de 200 mil tokens que media a conversa por conta própria foi
//! retirado, dos dois lugares onde ele agia: a pausa ao agente de onda e a
//! recusa à chamada de quem conduz. Quem cuida do tamanho agora é a
//! compactação automática do Claude Code; este gancho não mede nada, só
//! injeta o bloco sempre que uma compactação vai acontecer, para que a
//! retomada nunca dependa de guardar o número certo no meio do caminho.
//!
//! `None` (e o gancho deixa passar, [`Verdict::Allow`]) sem spec atual ou sem
//! arquivo de eventos legível: sem o que resumir, não há bloco para colar.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_events::{Block, BlockQuery};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use mustard_core::translate;

/// O valor de compactação que esta versão instalada do Mustard recomenda —
/// verificado contra o binário do Claude Code 2.1.278, que ainda honra
/// `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`
/// (`docs/2026-07-25-revisao-portoes-pipeline-ondas.md`, seção 9).
const RECOMMENDED_AUTOCOMPACT_PCT: &str = "15";

/// O aviso, antes de compactar: o bloco de retomada pronto para colar.
/// Dispara em toda compactação, manual ou automática, sem controle de "já
/// avisado" — o próprio `PreCompact` é o degrau.
pub struct PrecompactNotice;

impl Check for PrecompactNotice {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreCompact) {
            return Ok(Verdict::Allow);
        }
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        let Some(context) = resume_block(&root, input.session_id.as_deref()) else {
            return Ok(Verdict::Allow);
        };
        Ok(Verdict::Inject { context })
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

/// A linha que compara o valor de compactação configurado na máquina (a
/// variável `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, quando presente) com o que
/// esta versão instalada do Mustard recomenda, dizendo os dois valores e o
/// que fazer quando divergem. `machine` chega como parâmetro, e não por
/// `std::env::var` direto aqui dentro, para o teste poder variá-lo sem
/// `std::env::set_var` — `unsafe` no Rust 2024 e vedado neste crate.
fn autocompact_line(machine: Option<&str>, lang: Locale) -> String {
    let machine_display = match machine {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => translate("resume.none", lang).to_string(),
    };
    translate("conversation_size.autocompact", lang)
        .replace("{machine}", &machine_display)
        .replace("{installed}", RECOMMENDED_AUTOCOMPACT_PCT)
}

/// `waves`, separadas por vírgula, ou "nenhuma"/"none" quando vazia.
fn join_waves(waves: &[u64], lang: mustard_core::platform::i18n::Locale) -> String {
    if waves.is_empty() {
        return translate("resume.none", lang).to_string();
    }
    waves.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")
}

/// O bloco de retomada: spec, fase, ondas entregues, ondas em andamento e o
/// que falta.
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

/// O bloco de retomada pronto para colar, antes de compactar: spec, fase,
/// ondas entregues, em andamento e o que falta, com o próximo comando.
/// `None` sem spec atual ou sem arquivo de eventos.
pub(crate) fn resume_block(root: &Path, session: Option<&str>) -> Option<String> {
    let (project, spec, log) = active_spec_log(root, session)?;
    let phase = mustard_core::domain::spec_state::State::from_log(&log).phase.unwrap_or("survey");
    let block = resume_block_text(&spec, phase, &log, project.lang);
    let (command, next) = next_step(&project.root, &spec, session);
    let machine = std::env::var("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE").ok();
    let autocompact = autocompact_line(machine.as_deref(), project.lang);
    Some(
        translate("conversation_size.precompact", project.lang)
            .replace("{block}", &block)
            .replace("{command}", &command)
            .replace("{next}", &next)
            .replace("{autocompact}", &autocompact),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um projeto instalado, com a spec `spec` aprovada e o checkout parado
    /// na branch dela — o mesmo que a chamada de quem conduz vê.
    fn open_project(spec: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
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
            serde_json::json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}),
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

    /// Numa máquina configurada para `33`, que não bate com o `15` que a
    /// versão instalada recomenda, o aviso diz os dois valores e o que
    /// fazer. Sem nada configurado, a máquina aparece como "nenhum", nunca
    /// como um número inventado.
    #[test]
    fn o_aviso_compara_o_valor_de_compactacao_com_a_versao_instalada() {
        let line = autocompact_line(Some("33"), Locale::PtBr);
        assert!(line.contains("33"), "{line}");
        assert!(line.contains(RECOMMENDED_AUTOCOMPACT_PCT), "{line}");
        assert!(line.contains("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"), "{line}");
        assert!(line.contains("settings.json"), "{line}");

        let unset = autocompact_line(None, Locale::PtBr);
        assert!(unset.contains("nenhum"), "{unset}");
        assert!(unset.contains(RECOMMENDED_AUTOCOMPACT_PCT), "{unset}");
    }

    /// O aviso de compactar chega pelo gancho de `PreCompact`, com o bloco de
    /// retomada — spec, fase e o próximo passo —, com onda rodando ou sem: a
    /// linha das ondas em andamento é uma parte do bloco, não um substituto.
    /// E nenhuma chamada de ferramenta é mais recusada por tamanho: o mesmo
    /// registro inteiro, com uma transcrição de 200 mil tokens (o antigo
    /// degrau), deixa passar um `PreToolUse` comum.
    #[test]
    fn aviso_de_compactar_chega_no_gancho_e_ninguem_mais_e_barrado_por_tamanho() {
        let dir = open_project("x");
        let root = dir.path();

        // O caminho de verdade: o evento `PreCompact` inteiro, pelo
        // despachante e pelo registro — não a função auxiliar direto — é
        // quem precisa injetar o bloco.
        let precompact = HookInput {
            hook_event_name: Some("PreCompact".to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        let outcome = crate::dispatch::run_event(Some(Trigger::PreCompact), &precompact);
        let Verdict::Inject { context } = outcome.verdict else {
            panic!("antes de compactar, o bloco de retomada é injetado");
        };
        assert!(context.contains('x'), "o bloco traz a spec: {context}");
        assert!(context.contains("fase"), "o bloco traz a fase: {context}");
        assert!(
            context.contains(RECOMMENDED_AUTOCOMPACT_PCT),
            "o aviso cita o valor de compactação que a versão instalada recomenda: {context}"
        );
        assert!(
            context.contains("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"),
            "o aviso diz o que fazer quando o valor da máquina não bate: {context}"
        );

        // Com uma onda em andamento, o bloco continua saindo, com a onda
        // citada.
        seed_running_wave(root, "x");
        let outcome = crate::dispatch::run_event(Some(Trigger::PreCompact), &precompact);
        let Verdict::Inject { context } = outcome.verdict else {
            panic!("com onda em andamento, o bloco continua saindo");
        };
        assert!(context.contains("fase"), "o bloco não vira só a frase das ondas no ar: {context}");
        assert!(context.contains('1'), "o bloco cita a onda em andamento: {context}");

        // Uma transcrição de 200 mil tokens — o antigo degrau — não recusa
        // mais nenhuma chamada de ferramenta de quem conduz: o mesmo
        // despachante, no `PreToolUse`, deixa passar.
        let transcript = root.join("t.jsonl");
        std::fs::write(
            &transcript,
            serde_json::json!({
                "message": {"usage": {
                    "input_tokens": 200_000, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0,
                }}
            })
            .to_string(),
        )
        .unwrap();
        let outcome = crate::dispatch::run_event(Some(Trigger::PreToolUse), &conductor_call(root, &transcript));
        assert_eq!(outcome.verdict, Verdict::Allow, "sem degrau de tokens, nada barra mais a chamada");
    }
}
