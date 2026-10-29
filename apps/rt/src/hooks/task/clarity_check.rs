//! `clarity_check` — a regra de idioma do fim da resposta ([`ClarityRule`],
//! uma das regras do `end_of_turn_check`): a resposta fora do idioma do
//! projeto é barrada, e o assistente a escreve de novo no idioma certo, no
//! mesmo turno.
//!
//! ## Quando mede
//!
//! O `end_of_turn_check` só chama as regras no `Stop` da sessão principal.
//! Aqui, dois fatos a mais: o projeto tem `mustard.json`, e o `Stop` trouxe
//! texto final. O idioma só é julgado quando o projeto DECLAROU o idioma do
//! texto (`language.text`): o padrão resolvido não é uma escolha.
//!
//! ## O que faz
//!
//! Mede o texto com o medidor do núcleo (`domain::clarity`) e barra só pelo
//! idioma. A frase longa, a sigla e o código interno não barram nem voltam
//! na mensagem seguinte: a regra de escrita mora no estilo de resposta. Na
//! volta que o bloqueio pede (`stop_hook_active`), a regra nunca barra de
//! novo, para não prender o turno. Nada é gravado.
//!
//! A gravação de lição (`run write lesson`) mede o texto da lição pela mesma
//! medição ([`measure_in_project`]), com o que o projeto declara.

use std::path::Path;

use mustard_core::domain::clarity::{measure, ClarityReport, WrongLanguage};
use mustard_core::platform::i18n::Locale;
use mustard_core::ProjectConfig;

use crate::hooks::task::end_of_turn_check::{Finding, Turn, TurnRule};

/// A regra de idioma do fim da resposta.
pub struct ClarityRule;

impl TurnRule for ClarityRule {
    fn check(&self, turn: &Turn<'_>) -> Option<Finding> {
        let root = Path::new(turn.project_dir);
        if !ProjectConfig::exists(root) || turn.message.trim().is_empty() {
            return None;
        }
        // Só o idioma barra; na volta que o bloqueio pediu, nada barra de novo.
        let wrong = measure_in_project(root, turn.message, &[]).wrong_language?;
        (!turn.retry).then(|| Finding::Block(wrong_language_block(wrong, turn.lang)))
    }
}

/// O texto que barra o fim da resposta fora do idioma do projeto, no idioma
/// `lang`: diz em que idioma a resposta saiu e pede que seja escrita de novo no
/// do projeto.
fn wrong_language_block(wrong: WrongLanguage, lang: Locale) -> String {
    mustard_core::translate("clarity.wrong_language_block", lang)
        .replace("{found}", wrong.found.as_str())
        .replace("{expected}", wrong.expected.as_str())
}

/// A medição da escrita de `text` com o que o projeto em `root` declara. O
/// idioma que a prosa precisa ter é só o DECLARADO em `language.text`: o
/// padrão resolvido é pt-BR, e um projeto em inglês que nunca declarou idioma
/// teria todo texto apontado. As siglas do projeto (`acronyms` no
/// `mustard.json`) vão ao medidor junto das `explained`. A conferência do fim
/// da resposta e a gravação de lição medem por aqui, e não discordam sobre o
/// mesmo texto.
pub(crate) fn measure_in_project(root: &Path, text: &str, explained: &[String]) -> ClarityReport {
    let config = ProjectConfig::load(root);
    let known: Vec<String> = explained.iter().cloned().chain(config.acronyms()).collect();
    measure(text, &known, config.language().text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::task::end_of_turn_check::run_rules;
    use mustard_core::domain::model::contract::{Ctx, HookInput, Trigger, Verdict};
    use serde_json::{json, Value};
    use tempfile::tempdir;

    /// Reprova por uma sigla sem as palavras por extenso.
    const FAILING: &str = "O CI falhou de novo.";

    /// Prosa em inglês com palavras bastantes para o idioma ser julgado, e
    /// clara: só o idioma pode reprovar.
    const ENGLISH_REPLY: &str = "The wave is done and the tests pass.\n\
        The check now compares the language of the reply with the language of the project.\n\
        It counts the common words of each language.\n\
        A short reply is not judged at all.";
    /// O mesmo conteúdo em português, para o projeto que declarou en-US.
    const PORTUGUESE_REPLY: &str = "A onda terminou e os testes passaram.\n\
        A medição agora compara o idioma da resposta com o idioma do projeto.\n\
        Ela conta as palavras comuns de cada idioma.\n\
        Uma resposta curta não é julgada por ela.";
    /// O texto que barra a resposta em inglês num projeto em pt-BR.
    const PT_BLOCK: &str =
        "A resposta saiu em en-US, e o idioma do projeto e do usuário é pt-BR. Escreva a resposta de novo em pt-BR.";
    /// O texto que barra a resposta em português num projeto em en-US.
    const EN_BLOCK: &str = "The reply came out in pt-BR, and the language of the project and the user is en-US. \
        Write the reply again in en-US.";
    /// Uma frase de 26 palavras: longa, mas no idioma certo.
    const TWENTY_SIX_WORDS: &str = "Eu li os arquivos do projeto e conferi cada teste que ainda falhava \
        na máquina do usuário antes de ajustar a leitura do idioma hoje cedo.";

    /// Um projeto que declarou o português do Brasil como idioma do texto.
    fn project() -> tempfile::TempDir {
        project_with(r#"{"language":{"text":"pt-BR"}}"#)
    }

    /// Um projeto com este `mustard.json`.
    fn project_with(config: &str) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), config).unwrap();
        dir
    }

    fn ctx(root: &Path, trigger: Trigger) -> Ctx {
        Ctx::for_test(root.to_string_lossy().into_owned(), Some(trigger))
    }

    /// O `Stop` da sessão principal com o texto final do turno.
    fn stop(session: &str, message: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some(session.to_string()),
            raw: json!({ "last_assistant_message": message }),
            ..HookInput::default()
        }
    }

    /// A volta que um bloqueio pediu: o `Stop` com `stop_hook_active`.
    fn retry(session: &str, message: &str) -> HookInput {
        let mut input = stop(session, message);
        input.raw["stop_hook_active"] = json!(true);
        input
    }

    /// A regra de clareza sozinha, como a conferência do fim da resposta a roda.
    fn check(root: &Path, input: &HookInput) -> Verdict {
        run_rules(&[&ClarityRule], input, &ctx(root, Trigger::Stop))
    }

    /// O `Stop` como o Claude Code o manda ao `mustard-rt on Stop`, pelo
    /// despachante inteiro, e a resposta JSON que volta a ele; `Value::Null`
    /// quando nada volta.
    fn stop_event_at(root: &Path, session: &str, message: &str, retry: bool) -> Value {
        let payload = json!({
            "session_id": session,
            "transcript_path": root.join(format!("{session}.jsonl")),
            "cwd": root,
            "permission_mode": "default",
            "hook_event_name": "Stop",
            "stop_hook_active": retry,
            "last_assistant_message": message,
        });
        let input: HookInput = serde_json::from_value(payload).expect("a Stop payload");
        let outcome = crate::dispatch::run_event(Trigger::from_event_name("Stop"), &input);
        crate::hook_output::hook_specific_output("Stop", &outcome)
            .map(|out| serde_json::from_str(&out).expect("valid JSON"))
            .unwrap_or(Value::Null)
    }

    /// Uma resposta em inglês num projeto em português é barrada, pelo `Stop`
    /// de verdade, com o texto que pede a resposta de novo em português. Num
    /// projeto em inglês, a resposta em português é barrada com o texto em
    /// inglês; o mesmo inglês passa no projeto em inglês.
    #[test]
    fn a_reply_in_another_language_is_blocked() {
        let dir = project();
        let root = dir.path();
        let blocked = stop_event_at(root, "s1", ENGLISH_REPLY, false);
        assert_eq!(blocked["decision"], json!("block"), "{blocked}");
        assert_eq!(blocked["reason"], json!(PT_BLOCK));
        assert_eq!(check(root, &stop("s2", ENGLISH_REPLY)), Verdict::Deny { reason: PT_BLOCK.to_string() });

        let en = project_with(r#"{"language":{"text":"en-US"}}"#);
        let blocked = stop_event_at(en.path(), "s1", PORTUGUESE_REPLY, false);
        assert_eq!(blocked["decision"], json!("block"), "{blocked}");
        assert_eq!(blocked["reason"], json!(EN_BLOCK));
        assert_eq!(stop_event_at(en.path(), "s2", ENGLISH_REPLY, false), Value::Null, "the same English passes in en-US");
    }

    /// A volta que o bloqueio de idioma pediu nunca barra de novo, mesmo
    /// ainda fora do idioma, para não prender o turno. Só uma resposta nova,
    /// que não é volta, volta a ser barrada.
    #[test]
    fn the_retry_after_a_language_block_is_never_blocked_again() {
        let dir = project();
        let root = dir.path();
        assert_eq!(stop_event_at(root, "s1", ENGLISH_REPLY, false)["decision"], json!("block"));
        assert_eq!(stop_event_at(root, "s1", ENGLISH_REPLY, true), Value::Null, "still English, but a retry");
        assert_eq!(check(root, &retry("s1", ENGLISH_REPLY)), Verdict::Allow);
        assert_eq!(stop_event_at(root, "s1", ENGLISH_REPLY, false)["decision"], json!("block"), "a new reply");
    }

    /// Só o idioma barra: a resposta no idioma certo, com frase longa, sigla e
    /// código interno, segue sem bloqueio, e o texto curto demais para o
    /// idioma ser julgado também. A medição não grava nada, com sessão ou sem
    /// ela, e a mensagem seguinte não recebe correção.
    #[test]
    fn only_the_language_blocks_and_nothing_is_recorded() {
        let dir = project();
        let root = dir.path();
        let reply = format!("A regra MSTD-RULE-0008 ficou pronta no CI.\n{TWENTY_SIX_WORDS}");
        assert_eq!(stop_event_at(root, "s1", &reply, false), Value::Null);
        assert_eq!(stop_event_at(root, "s1", "The test passed.", false), Value::Null, "too short to judge");
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        assert!(!root.join(".claude/.session").exists(), "the check keeps no session record");
        let mut no_session = stop("s1", ENGLISH_REPLY);
        no_session.session_id = None;
        assert_eq!(check(root, &no_session), Verdict::Deny { reason: PT_BLOCK.to_string() });
    }

    /// Sem `mustard.json` nada é medido, e um subagente e outro evento nunca
    /// são medidos.
    #[test]
    fn only_the_main_reply_of_a_mustard_project_is_measured() {
        let bare = tempdir().unwrap();
        assert_eq!(check(bare.path(), &stop("s1", ENGLISH_REPLY)), Verdict::Allow);
        let dir = project();
        let mut sub = stop("s1", ENGLISH_REPLY);
        sub.agent_id = Some("child".to_string());
        assert_eq!(check(dir.path(), &sub), Verdict::Allow);
        let pre = ctx(dir.path(), Trigger::PreToolUse);
        assert_eq!(run_rules(&[&ClarityRule], &stop("s1", ENGLISH_REPLY), &pre), Verdict::Allow);
    }
}
