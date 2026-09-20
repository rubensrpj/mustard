//! `prompt_entry` — a entrada da mensagem, o único gancho do
//! `UserPromptSubmit`.
//!
//! Uma chamada por mensagem, com três passos, nesta ordem:
//!
//! 1. **A trava de instalação.** Num projeto sem `mustard.json` na raiz, um
//!    comando `/mustard:*` é barrado com a indicação do `/mustard:upsert`, o
//!    único liberado (é a porta que instala). Sem o arquivo, o idioma do
//!    projeto ainda não é conhecido, então a recusa sai do catálogo nos dois
//!    idiomas, uma linha em cada. O `/mustard` sozinho, sem dois pontos, é a
//!    ajuda e passa. Texto comum nunca é barrado: num projeto sem Mustard os
//!    ganchos ficam calados.
//! 2. **A mensagem, gravada.** Com uma spec atual, a mensagem do usuário vai
//!    para o bloco da conversa, como ele a escreveu. O aviso que o próprio
//!    Claude Code manda pelo mesmo canal (o fim de um comando em segundo
//!    plano, a volta de um subagente) não é mensagem de ninguém e não é
//!    gravado.
//! 3. **A linha curta.** Todo projeto instalado recebe, a cada mensagem, uma
//!    linha escondida de até 100 caracteres, sempre igual: responder no
//!    idioma do usuário, em texto simples. A regra de escrita inteira mora no
//!    estilo de resposta; a linha só lembra, e vai em toda mensagem porque o
//!    que ela rege é sempre a resposta mais nova. É o único texto que uma
//!    mensagem comum coloca na conversa: os textos grandes de regras não vão
//!    mais a cada mensagem. A exceção: depois de uma resposta com erro de
//!    escrita, a linha leva mais uma frase curta com o erro, como "Na última
//!    resposta: frase com 29 palavras.", uma vez só. Quem acha e guarda o erro
//!    é a conferência do fim da resposta (`clarity_check`), que não barra a
//!    resposta: esta frase é o único caminho do erro até o assistente.

use std::path::Path;

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;
use mustard_core::ProjectConfig;

use crate::commands::spec_events::conversation::record_message;
use crate::hooks::session::conversation_size;
use crate::hooks::task::clarity_check::take_next_note;
use crate::shared::prompt::is_harness_notice;

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

/// A linha curta de cada mensagem: responder no idioma do usuário, em texto
/// simples, com frases curtas e nenhum código interno. Até 100 caracteres nos
/// dois idiomas.
///
/// O idioma só é nomeado quando o projeto o declarou (`language.text`). Sem
/// declaração, a linha manda responder no idioma de quem escreve e não nomeia
/// nenhum: o idioma padrão é o pt-BR, e nomeá-lo mandaria um projeto em
/// inglês responder em português. Um `mustard.json` que não se lê não declara
/// nada, e recebe a mesma linha.
///
/// `None` num projeto sem `mustard.json`.
fn message_line(root: &Path) -> Option<String> {
    if !ProjectConfig::exists(root) {
        return None;
    }
    let language = ProjectConfig::load(root).language();
    let key = match language.text {
        Some(_) => "prompt_entry.line",
        None => "prompt_entry.line.undeclared",
    };
    Some(mustard_core::translate(key, language.text_or_default()).to_string())
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
        if !is_harness_notice(prompt) {
            let _ = record_message(root, input.session_id.as_deref(), prompt);
        }
        let Some(mut line) = message_line(root) else {
            return Ok(Verdict::Allow);
        };
        // O erro de escrita da última resposta vai uma vez, junto da linha.
        if let Some(note) = take_next_note(root, input.session_id.as_deref()) {
            line.push(' ');
            line.push_str(&note);
        }
        // O aviso de compactar, a cada novo degrau de 200 mil tokens, sem
        // onda em andamento.
        if let Some(path) = conversation_size::conversation_path(root, input)
            && let Some(tokens) = conversation_size::tokens_in(&path)
            && let Some(notice) = conversation_size::compact_notice(root, input.session_id.as_deref(), tokens)
        {
            line.push(' ');
            line.push_str(&notice);
        }
        Ok(Verdict::Inject { context: line })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_open;
    use crate::shared::spec_state::{stand_on_spec_branch, DiskSpecState};
    use mustard_core::domain::spec_state::SpecState;
    use mustard_core::platform::i18n::Locale;

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

    /// A linha de um projeto que declarou pt-BR.
    const PT_LINE: &str =
        "Responda em português do Brasil, em texto simples: frases curtas e nenhum código interno.";

    /// A linha de um projeto que declarou en-US.
    const EN_LINE: &str = "Answer in US English, in plain text: short sentences and no internal codes.";

    /// A linha de um projeto que não declarou idioma, no idioma padrão das
    /// mensagens do Mustard.
    const UNDECLARED_LINE: &str =
        "Responda no idioma de quem escreve, em texto simples: frases curtas e nenhum código interno.";

    /// A mesma linha, em inglês.
    const UNDECLARED_LINE_EN: &str =
        "Answer in the language the user writes in, in plain text: short sentences and no internal codes.";

    /// Um projeto com este `mustard.json`.
    fn project_with(config: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("mustard.json"), config).expect("write config");
        dir
    }

    /// O veredito de uma mensagem num projeto com este `mustard.json`, pelo
    /// gancho de verdade — nunca pela função auxiliar, para que desligar a
    /// ligação derrube o teste.
    fn verdict_for(config: &str, prompt: &str) -> (tempfile::TempDir, Verdict) {
        let dir = project_with(config);
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        let verdict = PromptEntry.evaluate(&prompt_input(prompt), &c).expect("the gate never errors");
        (dir, verdict)
    }

    /// O texto que o veredito coloca na conversa.
    fn context_of(verdict: Verdict) -> String {
        match verdict {
            Verdict::Inject { context } => context,
            other => panic!("an installed project always gets the line, got {other:?}"),
        }
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
        dir
    }

    /// Uma mensagem comum roda uma chamada só de gancho, coloca até 100
    /// caracteres na conversa e é gravada no bloco da conversa.
    ///
    /// Uma chamada só: o `hooks.json` registra um comando só no
    /// `UserPromptSubmit`, sem nenhum injetável próprio, e o registro tem um
    /// gancho só nesse evento. Até 100 caracteres: as quatro linhas cabem, e
    /// pelo despachante, num projeto com injetáveis declarados e uma spec
    /// atual, o texto colocado é só a linha. Gravada: a mensagem aparece na
    /// spec, como o usuário a escreveu.
    #[test]
    fn the_message_line_fits_in_one_hundred_characters_in_both_languages() {
        for (key, lang, line) in [
            ("prompt_entry.line", Locale::PtBr, PT_LINE),
            ("prompt_entry.line", Locale::EnUs, EN_LINE),
            ("prompt_entry.line.undeclared", Locale::PtBr, UNDECLARED_LINE),
            ("prompt_entry.line.undeclared", Locale::EnUs, UNDECLARED_LINE_EN),
        ] {
            let text = mustard_core::translate(key, lang);
            assert_eq!(text, line, "{key} in {lang}");
            assert!(text.chars().count() <= 100, "{key} in {lang}: {} characters", text.chars().count());
        }

        // Uma chamada só, no manifesto e no registro.
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../../../../../plugin/hooks/hooks.json"))
            .expect("the manifest is JSON");
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

        // Pelo despachante: só a linha entra, e a mensagem fica gravada.
        let dir = project_with_injectables_on("entrada");
        let root = dir.path();
        let input = HookInput {
            cwd: Some(root.to_string_lossy().into_owned()),
            ..prompt_input("como eu faço o login?")
        };
        let outcome = crate::dispatch::run_event(Some(Trigger::UserPromptSubmit), &input);
        let Verdict::Inject { context } = &outcome.verdict else {
            panic!("the message carries the line: {:?}", outcome.verdict);
        };
        assert!(context.chars().count() <= 100, "{} characters: {context}", context.chars().count());
        assert_eq!(context, PT_LINE);
        assert_eq!(messages(root, "entrada"), ["como eu faço o login?"]);
    }

    /// Toda mensagem recebe a linha curta, e só ela, mesmo com injetáveis
    /// declarados e uma spec atual: os textos grandes de regras e o aviso de
    /// spec em curso não vão mais a cada mensagem.
    #[test]
    fn every_message_gets_only_the_short_line() {
        let dir = project_with_injectables_on("so-a-linha");
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        for prompt in ["uma mensagem comum", "e agora?", "/mustard:feature x", "/grill-me"] {
            let verdict = PromptEntry.evaluate(&prompt_input(prompt), &c).expect("the gate never errors");
            assert_eq!(context_of(verdict), PT_LINE, "{prompt}");
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

    /// A linha segue o idioma declarado em `language.text`: pt-BR recebe a
    /// linha em português, en-US a inglesa.
    #[test]
    fn the_line_follows_the_declared_language() {
        for (config, line) in [(PT_PROJECT, PT_LINE), (r#"{"language":{"text":"en-US"}}"#, EN_LINE)] {
            let (_dir, verdict) = verdict_for(config, "uma mensagem comum");
            assert_eq!(context_of(verdict), line, "{config}");
        }
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

    /// Sem `language.text` no `mustard.json`, o idioma nunca é suposto, e as
    /// chaves antigas de idioma não contam como declaração: a linha manda
    /// responder no idioma de quem escreve, sem nomear idioma, e uma resposta
    /// em inglês não ganha erro de idioma na mensagem seguinte. Com pt-BR
    /// declarado, o erro vai.
    #[test]
    fn undeclared_language_is_never_assumed() {
        use crate::hooks::task::end_of_turn_check::EndOfTurnCheck;

        for config in ["{}", r#"{"specLang":"pt-BR","lang":"pt-BR"}"#] {
            let dir = project_with(config);
            let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
            let line = context_of(PromptEntry.evaluate(&prompt_input("uma mensagem comum"), &c).unwrap());
            assert_eq!(line, UNDECLARED_LINE, "{config}");
            for named in ["português", "Brasil", "pt-BR", "en-US"] {
                assert!(!line.contains(named), "{config}: an undeclared language is named ({named}): {line}");
            }
            let on_stop = Ctx { trigger: Some(Trigger::Stop), ..c.clone() };
            assert_eq!(EndOfTurnCheck.evaluate(&english_stop(), &on_stop).unwrap(), Verdict::Allow);
            let next = context_of(PromptEntry.evaluate(&prompt_input("e agora?"), &c).unwrap());
            assert!(!next.contains("resposta em"), "{config}: no language verdict: {next}");
        }

        let dir = project_with(PT_PROJECT);
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::Stop));
        assert_eq!(EndOfTurnCheck.evaluate(&english_stop(), &c).unwrap(), Verdict::Allow);
        let on_prompt = Ctx { trigger: Some(Trigger::UserPromptSubmit), ..c };
        let next = context_of(PromptEntry.evaluate(&prompt_input("e agora?"), &on_prompt).unwrap());
        assert_eq!(next, format!("{PT_LINE} Na última resposta: resposta em en-US."));
    }

    /// A linha curta e a medição de idioma valem para todo projeto com
    /// `mustard.json`. Com ou sem a antiga chave do tom, a mensagem leva a
    /// linha do idioma declarado; uma resposta em inglês num projeto em pt-BR
    /// não é barrada, e a mensagem seguinte leva o erro, uma vez só. Sem
    /// `mustard.json`, nada.
    #[test]
    fn the_language_line_reaches_every_mustard_project() {
        use crate::hooks::task::end_of_turn_check::EndOfTurnCheck;

        for config in [PT_PROJECT, r#"{"language":{"text":"pt-BR"},"tone":"technical"}"#] {
            let dir = project_with(config);
            let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
            let context = context_of(PromptEntry.evaluate(&prompt_input("uma mensagem comum"), &c).unwrap());
            assert_eq!(context, PT_LINE, "{config}");

            let on_stop = Ctx { trigger: Some(Trigger::Stop), ..c.clone() };
            let verdict = EndOfTurnCheck.evaluate(&english_stop(), &on_stop).unwrap();
            assert_eq!(verdict, Verdict::Allow, "{config}: the writing check never blocks");

            let next = context_of(PromptEntry.evaluate(&prompt_input("e agora?"), &c).unwrap());
            assert_eq!(next, format!("{PT_LINE} Na última resposta: resposta em en-US."), "{config}");
            let after = context_of(PromptEntry.evaluate(&prompt_input("e depois?"), &c).unwrap());
            assert_eq!(after, PT_LINE, "{config}: the error goes once");
        }

        let (none, c) = ctx();
        let verdict = PromptEntry.evaluate(&prompt_input("uma mensagem comum"), &c).unwrap();
        assert_eq!(verdict, Verdict::Allow, "an uninstalled project gets no line");
        let on_stop = Ctx { trigger: Some(Trigger::Stop), ..c };
        assert_eq!(EndOfTurnCheck.evaluate(&english_stop(), &on_stop).unwrap(), Verdict::Allow);
        drop(none);
    }

    /// Todo projeto com `mustard.json` leva a linha curta, declare ou não o
    /// idioma; a antiga chave do tom não a desliga. Sem `mustard.json`, nada.
    #[test]
    fn the_line_rides_every_installed_project() {
        for (config, line) in
            [("{}", UNDECLARED_LINE), (r#"{"tone":"technical"}"#, UNDECLARED_LINE), (PT_PROJECT, PT_LINE)]
        {
            let (_dir, verdict) = verdict_for(config, "uma mensagem comum");
            assert_eq!(context_of(verdict), line, "{config}: every installed project carries the line");
        }
        let (_none, c) = ctx();
        let verdict = PromptEntry.evaluate(&prompt_input("uma mensagem comum"), &c).unwrap();
        assert_eq!(verdict, Verdict::Allow, "an uninstalled project declared nothing");
    }

    /// Um comando de barra leva a linha também: ela rege como a resposta é
    /// escrita, e essa resposta é lida pela mesma pessoa.
    #[test]
    fn the_line_rides_a_slash_command_too() {
        let (_dir, verdict) = verdict_for(PT_PROJECT, "/mustard:pr merge");
        assert_eq!(verdict, Verdict::Inject { context: PT_LINE.to_string() });
    }

    /// Sem idioma declarado, a linha não nomeia nenhum, nos dois idiomas.
    #[test]
    fn an_undeclared_language_is_not_named_in_the_line() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let line = mustard_core::translate("prompt_entry.line.undeclared", lang);
            for named in ["português", "Brasil", "English", "pt-BR", "en-US"] {
                assert!(!line.contains(named), "{lang} names {named}: {line}");
            }
        }
        let (_dir, verdict) = verdict_for("{}", "uma mensagem comum");
        assert_eq!(context_of(verdict), UNDECLARED_LINE);
    }

    /// Os parágrafos antigos de escrita e de idioma não vão em mensagem
    /// nenhuma: nem comum, nem comando de barra, com ou sem idioma declarado.
    #[test]
    fn no_message_carries_the_old_writing_paragraph() {
        for config in [PT_PROJECT, r#"{"language":{"text":"en-US"}}"#, "{}"] {
            for prompt in ["uma mensagem comum", "/mustard:pr merge", "/grill-me"] {
                let (_dir, verdict) = verdict_for(config, prompt);
                let context = context_of(verdict);
                for old in ["ONE idea per sentence", "in the language they write in"] {
                    assert!(!context.contains(old), "{config} {prompt}: {old}: {context}");
                }
            }
        }
    }

    /// Um `mustard.json` que não se lê ainda quer dizer projeto instalado: a
    /// linha vai, sem nomear idioma.
    #[test]
    fn a_broken_mustard_json_gets_the_line_without_a_language() {
        let (_dir, verdict) = verdict_for("{ not json", "uma mensagem comum");
        assert_eq!(context_of(verdict), UNDECLARED_LINE);
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

    /// No `UserPromptSubmit` e no `SessionStart`, um só gancho coloca texto:
    /// a linha curta chega uma vez.
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
                let Verdict::Inject { context } = &outcome.verdict else {
                    panic!("the prompt carries the line: {:?}", outcome.verdict);
                };
                assert_eq!(context.matches(PT_LINE).count(), 1, "{context}");
            }
        }
    }

    /// A conversa da transcrição com o uso gravado no último uso somando
    /// `tokens`.
    fn transcript_with(path: &Path, tokens: u64) {
        let line = serde_json::json!({
            "message": {"usage": {
                "input_tokens": tokens, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0,
            }}
        });
        std::fs::write(path, line.to_string()).unwrap();
    }

    /// A entrada da mensagem `session`, com a transcrição em `transcript`.
    fn prompt_with_transcript(root: &Path, session: &str, transcript: &Path) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some(session.to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw: serde_json::json!({ "prompt": "e agora?", "transcript_path": transcript.to_string_lossy() }),
            ..HookInput::default()
        }
    }

    /// O orquestrador é avisado para compactar, com o
    /// comando `/compact` pronto e o resumo do que fica, a cada novo degrau
    /// de 200 mil tokens: na divisa, 199.999 não avisa e 200.000 avisa;
    /// 399.999 não repete o mesmo degrau e 400.000 repete. Com uma onda em
    /// andamento, num degrau novo, o aviso sai do mesmo jeito, mas diz quais
    /// ondas estão rodando em vez do resumo do `resume`.
    #[test]
    fn the_orchestrator_is_told_when_to_compact() {
        let dir = project_with(PT_PROJECT);
        let root = dir.path();
        stand_on_spec_branch(root, "x");
        record_open(root, "x", "feature/x", "dev").expect("open");
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        let c = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        let transcript = root.join("t.jsonl");

        // 199.999: o último tamanho que não avisa.
        transcript_with(&transcript, 199_999);
        let context = context_of(PromptEntry.evaluate(&prompt_with_transcript(root, "s1", &transcript), &c).unwrap());
        assert_eq!(context, PT_LINE, "abaixo do degrau, sem aviso: {context}");

        // 200.000: o primeiro tamanho que já avisa, com /compact e o resumo.
        transcript_with(&transcript, 200_000);
        let context = context_of(PromptEntry.evaluate(&prompt_with_transcript(root, "s1", &transcript), &c).unwrap());
        assert!(context.contains("/compact"), "{context}");
        assert!(context.contains('x'), "traz a spec no resumo: {context}");

        // 399.999: o mesmo degrau, não repete.
        transcript_with(&transcript, 399_999);
        let context = context_of(PromptEntry.evaluate(&prompt_with_transcript(root, "s1", &transcript), &c).unwrap());
        assert_eq!(context, PT_LINE, "o mesmo degrau não repete: {context}");

        // 400.000: um novo degrau, repete.
        transcript_with(&transcript, 400_000);
        let context = context_of(PromptEntry.evaluate(&prompt_with_transcript(root, "s1", &transcript), &c).unwrap());
        assert!(context.contains("/compact"), "um novo degrau avisa de novo: {context}");

        // Com uma onda em andamento, o aviso nunca sai, mesmo num degrau novo
        // e numa sessão que ainda não viu nenhum.
        let said = crate::shared::spec_state::seed_event(
            root,
            "x",
            "message",
            serde_json::json!({"author": "user", "text": "o plano"}),
        );
        let crit = crate::shared::spec_state::seed_event(
            root,
            "x",
            "criterion",
            serde_json::json!({"when": "a", "then": "b", "proof": "p", "origin": said}),
        );
        crate::shared::spec_state::seed_event(
            root,
            "x",
            "wave",
            serde_json::json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "x", "origin": said}),
        );
        crate::shared::spec_state::seed_event(root, "x", "state", serde_json::json!({"phase": "running", "author": "binary"}));
        // O envio leva o pid e a hora de início deste próprio processo, que
        // segue vivo durante o teste: é assim que `waves_in_progress` conta
        // a onda como em andamento, sem depender de um Claude Code de
        // verdade.
        let (pid, started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(
            root,
            "x",
            "send",
            serde_json::json!({"wave": 1, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [crit], "mustard": "0", "author": "binary",
                "claude_pid": pid, "claude_started": started}),
        );
        transcript_with(&transcript, 800_000);
        let context = context_of(PromptEntry.evaluate(&prompt_with_transcript(root, "s2", &transcript), &c).unwrap());
        assert!(context.contains('1'), "onda em andamento: diz qual está rodando: {context}");
        assert!(!context.contains("fase"), "sem resumo de fase com onda em andamento: {context}");
    }

    /// Uma conversa que passa de 400 mil, é compactada para 70 mil (um
    /// `/compact` de verdade) e volta a crescer com uma onda em andamento: o
    /// degrau guardado acompanha o encolhimento, então o aviso sai de novo no
    /// próximo degrau de 200 mil — e diz quais ondas estão rodando e que a
    /// volta delas chega pela rodada. Antes do conserto, o degrau avisado
    /// (2) sobrevivia à compactação e calava o aviso para sempre.
    #[test]
    fn the_compact_notice_comes_back_after_a_compaction_even_with_waves_running() {
        let dir = project_with(PT_PROJECT);
        let root = dir.path();
        stand_on_spec_branch(root, "x");
        record_open(root, "x", "feature/x", "dev").expect("open");
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        let c = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        let transcript = root.join("t.jsonl");

        let said = crate::shared::spec_state::seed_event(
            root,
            "x",
            "message",
            serde_json::json!({"author": "user", "text": "o plano"}),
        );
        let crit = crate::shared::spec_state::seed_event(
            root,
            "x",
            "criterion",
            serde_json::json!({"when": "a", "then": "b", "proof": "p", "origin": said}),
        );
        crate::shared::spec_state::seed_event(
            root,
            "x",
            "wave",
            serde_json::json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "x", "origin": said}),
        );
        crate::shared::spec_state::seed_event(root, "x", "state", serde_json::json!({"phase": "running", "author": "binary"}));
        let (pid, started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(
            root,
            "x",
            "send",
            serde_json::json!({"wave": 1, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [crit], "mustard": "0", "author": "binary",
                "claude_pid": pid, "claude_started": started}),
        );

        // Passa de 400 mil: avisa, mesmo com a onda 1 em andamento.
        transcript_with(&transcript, 400_000);
        let context = context_of(PromptEntry.evaluate(&prompt_with_transcript(root, "s3", &transcript), &c).unwrap());
        assert!(context.contains('1'), "diz qual onda está rodando: {context}");

        // Compactada de verdade: cai bem abaixo do degrau avisado.
        transcript_with(&transcript, 70_000);
        let context = context_of(PromptEntry.evaluate(&prompt_with_transcript(root, "s3", &transcript), &c).unwrap());
        assert_eq!(context, PT_LINE, "abaixo do degrau, sem aviso: {context}");

        // Volta a passar de 200 mil: o degrau avisado recomeçou do atual, e
        // este é um degrau novo — o aviso sai de novo.
        transcript_with(&transcript, 200_000);
        let context = context_of(PromptEntry.evaluate(&prompt_with_transcript(root, "s3", &transcript), &c).unwrap());
        assert!(context.contains('1'), "avisa de novo, com a onda 1 em andamento: {context}");
    }
}
