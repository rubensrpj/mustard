//! `prompt_entry` — a entrada da mensagem, o único gancho do
//! `UserPromptSubmit`.
//!
//! Uma chamada por mensagem, com dois passos, nesta ordem:
//!
//! 1. **A trava de instalação.** Num projeto sem `mustard.json` na raiz, um
//!    comando `/mustard:*` é barrado com a indicação do `/mustard:upsert`, o
//!    único liberado (é a porta que instala). Sem o arquivo, o idioma do
//!    projeto ainda não é conhecido, então a recusa sai do catálogo nos dois
//!    idiomas, uma linha em cada. O `/mustard` sozinho, sem dois pontos, é a
//!    ajuda e passa. Texto comum nunca é barrado: num projeto sem Mustard os
//!    ganchos ficam calados.
//! 2. **A mensagem, gravada.** Na sessão que a abertura ou a retomada ligou
//!    a uma spec, a mensagem do usuário vai para o bloco da conversa dela,
//!    como ele a escreveu. O aviso que o próprio Claude Code manda pelo mesmo
//!    canal (o fim de um comando em segundo plano, a volta de um subagente)
//!    não é mensagem de ninguém e não é gravado, e o lembrete do sistema que
//!    vem dentro da fala sai dela. A mensagem dita antes de a spec existir, a
//!    do turno da abertura inclusive, quem grava é a abertura (`run open`),
//!    lida da conversa da sessão com o mesmo filtro do aviso; daqui saem só
//!    as seguintes, sem repetir nenhuma.
//!
//! A mensagem nunca leva texto à conversa: a regra de escrita mora no estilo
//! de resposta, dita uma vez.

use std::path::Path;

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;
use mustard_core::ProjectConfig;

use crate::commands::spec_events::conversation::record_message;
use crate::shared::prompt::person_text;

/// A entrada da mensagem.
pub struct PromptEntry;

/// `true` quando `prompt` chama um comando `/mustard:`. O `/mustard` sozinho,
/// sem dois pontos, é a ajuda e não casa: ela precisa funcionar num projeto
/// sem instalação. Só os comandos do Mustard contam: barrar o comando de
/// outro plugin por falta do `mustard.json` quebraria o que não é dele.
fn is_mustard_command(prompt: &str) -> bool {
    prompt.trim_start().to_ascii_lowercase().starts_with("/mustard:")
}

/// `true` quando `prompt` chama o `/mustard:upsert`, a porta que instala.
/// `/mustard:upsertish` é outro comando e não passa.
fn is_upsert_prompt(prompt: &str) -> bool {
    let t = prompt.trim_start().to_ascii_lowercase();
    let Some(rest) = t.strip_prefix("/mustard:") else {
        return false;
    };
    const CMD: &str = "upsert";
    rest.starts_with(CMD)
        && rest.as_bytes().get(CMD.len()).is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'))
}

/// A recusa da trava de instalação, do catálogo, uma linha em cada idioma:
/// sem `mustard.json` o Mustard ainda não sabe qual é o idioma do projeto.
fn not_installed_reason() -> String {
    format!(
        "{}\n{}",
        mustard_core::translate("install_lock.not_installed", mustard_core::SupportedLocale::PtBr),
        mustard_core::translate("install_lock.not_installed", mustard_core::SupportedLocale::EnUs),
    )
}

impl Check for PromptEntry {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::UserPromptSubmit) {
            return Ok(Verdict::Allow);
        }
        let prompt = input.user_prompt().unwrap_or_default();
        let cwd = ctx.project_dir_or_cwd(input);
        let root = Path::new(&cwd);
        if !ProjectConfig::exists(root) {
            if is_mustard_command(prompt) && !is_upsert_prompt(prompt) {
                return Ok(Verdict::Deny { reason: not_installed_reason() });
            }
            return Ok(Verdict::Allow);
        }
        if let Some(text) = person_text(prompt) {
            let _ = record_message(root, input.session_id.as_deref(), &text);
        }
        Ok(Verdict::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_open;
    use crate::shared::spec_state::{stand_on_spec_branch, DiskSpecState};
    use mustard_core::domain::spec_state::SpecState;

    /// Um contexto numa pasta temporária sem nada: nem `mustard.json`, nem
    /// branch, nem spec.
    fn ctx() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        (dir, ctx)
    }

    fn prompt_input(prompt: &str) -> HookInput {
        prompt_input_with_session(prompt, "s1")
    }

    fn prompt_input_with_session(prompt: &str, session: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some(session.to_string()),
            raw: serde_json::json!({ "prompt": prompt }),
            ..HookInput::default()
        }
    }

    /// Um projeto que declarou o português do Brasil como idioma do texto.
    const PT_PROJECT: &str = r#"{"language":{"text":"pt-BR"}}"#;

    /// Um projeto com este `mustard.json`.
    fn project_with(config: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("mustard.json"), config).expect("write config");
        dir
    }

    /// Um evento do Claude Code na sessão `s1` do projeto em `root`, pelo
    /// despachante inteiro, como o `mustard-rt on <evento>` o roda; devolve o
    /// veredito que volta a ele.
    fn event(root: &Path, name: &str, raw: serde_json::Value) -> Verdict {
        let input = HookInput {
            hook_event_name: Some(name.to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw,
            ..HookInput::default()
        };
        crate::dispatch::run_event(Trigger::from_event_name(name), &input).verdict
    }

    /// As mensagens de usuário gravadas na spec `spec`.
    fn messages(root: &Path, spec: &str) -> Vec<String> {
        DiskSpecState::new(root)
            .log(spec)
            .map(|log| {
                log.visible()
                    .into_iter()
                    .filter(|e| e.event_type == "message" && e.str_field("author") == Some("user"))
                    .filter_map(|e| e.str_field("text").map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Um projeto instalado, com injetáveis declarados para a mensagem e
    /// escritos no disco, parado na branch de uma spec aberta.
    fn project_with_injectables_on(spec: &str) -> tempfile::TempDir {
        let dir = project_with(
            r#"{"language":{"text":"pt-BR"},"inject":[{"on":"userPromptSubmit","file":".claude/mustard/session-map.md","once":true}]}"#,
        );
        let root = dir.path();
        let mustard_dir = root.join(".claude").join("mustard");
        std::fs::create_dir_all(&mustard_dir).unwrap();
        std::fs::write(
            mustard_dir.join("session-map.md"),
            mustard_core::session_map(mustard_core::platform::i18n::Locale::PtBr),
        )
        .unwrap();
        stand_on_spec_branch(root, spec);
        record_open(root, spec, &format!("feature/{spec}"), "dev").expect("open");
        // A conversa das mensagens destes testes abriu a spec.
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), "s1", spec);
        dir
    }

    /// Uma mensagem comum roda uma chamada só de gancho, não coloca texto na
    /// conversa e é gravada no bloco da conversa.
    ///
    /// Uma chamada só: o `hooks.json` registra um comando só no
    /// `UserPromptSubmit`, sem nenhum injetável próprio, e o registro tem um
    /// gancho só nesse evento. Sem texto: pelo despachante, num projeto com
    /// injetáveis declarados e uma spec atual, a mensagem passa sem nada.
    /// Gravada: a mensagem aparece na spec, como o usuário a escreveu.
    #[test]
    fn a_message_runs_one_hook_and_carries_no_text() {
        // Uma chamada só, no manifesto e no registro.
        // Lido da cópia que roda o teste: o `plugin/` fica fora do pacote.
        let hooks = crate::manifest_dir::manifest_dir().join("../../plugin/hooks/hooks.json");
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&hooks).expect("the manifest reads")).expect("the manifest is JSON");
        let commands: Vec<&str> = manifest["hooks"]["UserPromptSubmit"]
            .as_array()
            .expect("the message has a hook")
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
            .filter_map(|hook| hook["command"].as_str())
            .collect();
        assert_eq!(commands.len(), 1, "one hook call per message: {commands:?}");
        assert!(!commands[0].contains("--inject"), "the call carries no injectable: {commands:?}");
        let registry = crate::registry::Registry::new();
        let on_prompt: Vec<&str> =
            registry.applicable(Trigger::UserPromptSubmit, None).iter().map(|m| m.id).collect();
        assert_eq!(on_prompt, ["prompt_entry"]);

        // Pelo despachante: nada entra, e a mensagem fica gravada.
        let dir = project_with_injectables_on("entrada");
        let root = dir.path();
        let input = HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            ..prompt_input("como eu faço o login?")
        };
        let outcome = crate::dispatch::run_event(Some(Trigger::UserPromptSubmit), &input);
        assert_eq!(outcome.verdict, Verdict::Allow, "an ordinary message carries no text");
        assert_eq!(messages(root, "entrada"), ["como eu faço o login?"]);
    }

    /// Uma mensagem sem erro para corrigir não leva texto nenhum, mesmo com
    /// injetáveis declarados e uma spec atual: a regra de escrita mora no
    /// estilo de resposta, e nem ela, nem os textos grandes de regras, nem o
    /// aviso de spec em curso vão a cada mensagem.
    #[test]
    fn an_ordinary_message_carries_no_text() {
        let dir = project_with_injectables_on("sem-texto");
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        for prompt in ["uma mensagem comum", "e agora?", "/mustard:feature x", "/outra-skill"] {
            let verdict = PromptEntry.evaluate(&prompt_input(prompt), &c).expect("the gate never errors");
            assert_eq!(verdict, Verdict::Allow, "{prompt}");
        }
        assert!(
            !dir.path().join(".claude/.session/s1/injected-session-map.md").exists(),
            "no injectable is delivered, so no marker is burned",
        );
    }

    /// A mensagem vai para o bloco da conversa da spec atual. O aviso do
    /// próprio Claude Code não é gravado, e sem spec atual nada é gravado.
    #[test]
    fn the_message_is_recorded_in_the_current_spec() {
        let dir = project_with_injectables_on("gravada");
        let root = dir.path();
        let c = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        for prompt in [
            "arrume o botão",
            "<task-notification>\n<status>completed</status>",
            "[SYSTEM NOTIFICATION - NOT USER INPUT]\ncorpo",
            "/mustard:continue",
        ] {
            let _ = PromptEntry.evaluate(&prompt_input(prompt), &c).expect("the gate never errors");
        }
        assert_eq!(messages(root, "gravada"), ["arrume o botão", "/mustard:continue"]);

        let (bare, _) = ctx();
        std::fs::write(bare.path().join("mustard.json"), "{}").unwrap();
        let c = Ctx::for_test(bare.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        let _ = PromptEntry.evaluate(&prompt_input("sem spec"), &c).expect("the gate never errors");
        assert!(!bare.path().join(".claude").exists(), "no spec, nothing recorded");
    }

    /// A volta de um subagente, que começa com `<agent-message from=`, não é
    /// gravada como fala do usuário; a fala de verdade continua gravada.
    #[test]
    fn a_subagent_report_is_not_recorded_as_the_user() {
        let dir = project_with_injectables_on("subagente");
        let root = dir.path();
        let c = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        for prompt in [
            "<agent-message from=\"onda-39\">relatório da onda</agent-message>",
            "arrume o botão",
        ] {
            let _ = PromptEntry.evaluate(&prompt_input(prompt), &c).expect("the gate never errors");
        }
        assert_eq!(messages(root, "subagente"), ["arrume o botão"]);
    }

    /// Uma frase de 29 palavras, quatro acima do limite de 25.
    const TWENTY_NINE_WORDS: &str = "Eu li os arquivos do projeto e conferi cada teste que ainda falhava \
        na máquina do usuário antes de ajustar a leitura do idioma hoje cedo com toda calma.";

    /// A mensagem que segue uma resposta com erro de escrita não recebe
    /// correção nenhuma: a frase longa não é barrada, e a mensagem seguinte
    /// passa sem texto. Tudo pelos ganchos de verdade, no despachante.
    #[test]
    fn the_message_after_a_writing_error_carries_no_correction() {
        assert_eq!(TWENTY_NINE_WORDS.split_whitespace().count(), 29);
        let dir = project_with(PT_PROJECT);
        let root = dir.path();
        let prompt = |text: &str| event(root, "UserPromptSubmit", serde_json::json!({ "prompt": text }));
        let reply = |text: &str| {
            event(root, "Stop", serde_json::json!({ "last_assistant_message": text, "stop_hook_active": false }))
        };
        assert_eq!(reply(TWENTY_NINE_WORDS), Verdict::Allow, "the long sentence is not blocked");
        assert_eq!(prompt("e agora?"), Verdict::Allow, "no correction rides the next message");
        assert!(!root.join(".claude/.session/s1/clarity.json").exists(), "nothing is kept for the next message");
    }

    /// O `Stop` da sessão `s1` com uma resposta em inglês, prosa bastante
    /// para o idioma ser julgado.
    fn english_stop() -> HookInput {
        let english = "The wave is done and the tests pass.\n\
            The check now compares the language of the reply with the language of the project.\n\
            It counts the common words of each language.\n\
            A short reply is not judged at all.";
        HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s1".to_string()),
            raw: serde_json::json!({ "last_assistant_message": english }),
            ..HookInput::default()
        }
    }

    /// O veredito que barra uma resposta fora do idioma do projeto: o motivo
    /// nomeia o idioma que o projeto declarou, pt-BR.
    fn assert_blocked_for_language(verdict: Verdict) {
        let Verdict::Deny { reason } = verdict else { panic!("a reply in another language is blocked: {verdict:?}") };
        assert!(reason.contains("en-US") && reason.contains("pt-BR"), "{reason}");
    }

    /// Sem `language.text` no `mustard.json`, o idioma nunca é suposto, e as
    /// chaves antigas de idioma não contam como declaração: uma resposta em
    /// inglês não é barrada. Com pt-BR declarado, ela é barrada, e a mensagem
    /// seguinte passa sem texto. Sem `mustard.json`, nada.
    #[test]
    fn undeclared_language_is_never_assumed() {
        use crate::hooks::task::end_of_turn_check::EndOfTurnCheck;

        for config in ["{}", r#"{"specLang":"pt-BR","lang":"pt-BR"}"#] {
            let dir = project_with(config);
            let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::Stop));
            assert_eq!(EndOfTurnCheck.evaluate(&english_stop(), &c).unwrap(), Verdict::Allow, "{config}");
        }

        let dir = project_with(PT_PROJECT);
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::Stop));
        assert_blocked_for_language(EndOfTurnCheck.evaluate(&english_stop(), &c).unwrap());
        let on_prompt = Ctx { trigger: Some(Trigger::UserPromptSubmit), ..c };
        assert_eq!(PromptEntry.evaluate(&prompt_input("e agora?"), &on_prompt).unwrap(), Verdict::Allow);

        let (none, c) = ctx();
        let on_stop = Ctx { trigger: Some(Trigger::Stop), ..c };
        assert_eq!(EndOfTurnCheck.evaluate(&english_stop(), &on_stop).unwrap(), Verdict::Allow);
        drop(none);
    }

    /// Sem `mustard.json`, todo comando `/mustard:*` é barrado com a
    /// indicação da porta que instala e o nome do arquivo que falta.
    #[test]
    fn gate_denies_mustard_command_without_installation() {
        let (_dir, c) = ctx();
        for prompt in ["/mustard:feature x", "/mustard:git", "  /MUSTARD:QA"] {
            match PromptEntry.evaluate(&prompt_input(prompt), &c).unwrap() {
                Verdict::Deny { reason } => {
                    assert!(reason.contains("/mustard:upsert"), "{reason}");
                    assert!(reason.contains("mustard.json"), "{reason}");
                }
                other => panic!("expected Deny for {prompt:?} without mustard.json, got {other:?}"),
            }
        }
    }

    /// Sem `mustard.json` o idioma do projeto ainda não é conhecido: a
    /// recusa sai do catálogo com uma linha em português e outra em inglês,
    /// e não com o texto fixo que morava no próprio arquivo.
    #[test]
    fn the_install_lock_speaks_both_languages() {
        let (_dir, c) = ctx();
        let Verdict::Deny { reason } = PromptEntry.evaluate(&prompt_input("/mustard:feature x"), &c).unwrap() else {
            panic!("expected Deny without installation");
        };
        let pt = mustard_core::translate("install_lock.not_installed", mustard_core::SupportedLocale::PtBr);
        let en = mustard_core::translate("install_lock.not_installed", mustard_core::SupportedLocale::EnUs);
        assert_ne!(pt, en, "the two lines must differ");
        assert_eq!(reason, format!("{pt}\n{en}"));
    }

    /// A porta que instala passa sem instalação; `/mustard:upsertish` é outro
    /// comando e continua barrado.
    #[test]
    fn gate_allows_upsert_without_installation() {
        let (_dir, c) = ctx();
        assert_eq!(PromptEntry.evaluate(&prompt_input("/mustard:upsert"), &c).unwrap(), Verdict::Allow);
        assert!(matches!(
            PromptEntry.evaluate(&prompt_input("/mustard:upsertish"), &c).unwrap(),
            Verdict::Deny { .. }
        ));
    }

    /// O `/mustard` sozinho é a ajuda e precisa funcionar sem instalação.
    #[test]
    fn gate_allows_bare_mustard_help_without_installation() {
        let (_dir, c) = ctx();
        assert_eq!(PromptEntry.evaluate(&prompt_input("/mustard"), &c).unwrap(), Verdict::Allow);
    }

    /// Texto comum nunca é barrado num projeto sem instalação.
    #[test]
    fn gate_ignores_normal_prompts_without_installation() {
        let (_dir, c) = ctx();
        assert_eq!(PromptEntry.evaluate(&prompt_input("hello there"), &c).unwrap(), Verdict::Allow);
    }

    /// Fora do `UserPromptSubmit`, o gancho passa.
    #[test]
    fn non_user_prompt_submit_trigger_allows() {
        let other = Ctx::for_test(".".to_string(), Some(Trigger::PreToolUse));
        assert_eq!(PromptEntry.evaluate(&prompt_input("/mustard:feature x"), &other).unwrap(), Verdict::Allow);
    }

    /// No `UserPromptSubmit` e no `SessionStart`, no máximo um gancho coloca
    /// texto, e a mensagem não leva nenhum.
    #[test]
    fn prompt_and_session_start_have_one_injecting_check() {
        use crate::registry::Registry;
        use mustard_core::domain::model::contract::Outcome;

        let dir = project_with(PT_PROJECT);
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), None);
        let registry = Registry::new();
        let on_prompt = prompt_input_with_session("e agora?", "s1");
        let on_start = HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some("s1".to_string()),
            ..HookInput::default()
        };
        for (name, trigger, input) in [
            ("UserPromptSubmit", Trigger::UserPromptSubmit, &on_prompt),
            ("SessionStart", Trigger::SessionStart, &on_start),
        ] {
            let at = Ctx { trigger: Some(trigger), ..c.clone() };
            let mut outcome = Outcome::allow();
            let mut injecting = Vec::new();
            for module in registry.applicable(trigger, None) {
                let Some(check) = &module.check else { continue };
                let verdict = check.evaluate(input, &at).unwrap_or(Verdict::Allow);
                if matches!(verdict, Verdict::Inject { .. }) {
                    injecting.push(module.id);
                }
                outcome.fold(verdict);
            }
            assert!(injecting.len() <= 1, "{name}: {injecting:?} would share one response");
            if name == "UserPromptSubmit" {
                assert!(injecting.is_empty(), "the message carries no text: {injecting:?}");
            }
        }
    }

}
