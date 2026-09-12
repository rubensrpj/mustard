//! `end_of_turn_check` — a conferência do fim da resposta.
//!
//! No `Stop` da sessão principal, o texto final do turno
//! (`last_assistant_message`) passa por uma lista de regras ([`TurnRule`]), e o
//! que elas acham sai num bloqueio só. Hoje são duas, cada uma no módulo do
//! assunto:
//!
//! - as pendências ([`PendingRule`], `pending_gate.rs`): o turno em que uma
//!   unidade fechou não termina sem citar cada pendência aberta;
//! - a clareza ([`ClarityRule`], `clarity_check.rs`): a resposta segue a regra
//!   de escrita e sai no idioma que o projeto declarou.
//!
//! ## Um bloqueio só
//!
//! Antes, cada regra era um gancho do `Stop`, e o primeiro bloqueio vencia: o
//! bloqueio de um engolia o do outro, e o assistente reescrevia sem saber de
//! tudo. Agora todas as regras rodam, e cada achado entra no mesmo texto, na
//! ordem de [`RULES`].
//!
//! ## Cada regra guarda a sua política
//!
//! Uma regra devolve [`Finding::Block`] (barra: o texto vai ao assistente, que
//! reescreve) ou [`Finding::Warn`] (só avisa: o texto vai ao usuário, e a
//! resposta termina). Na reescrita que um bloqueio pediu, o `Stop` chega com
//! `stop_hook_active` ([`Turn::retry`]): a clareza passa a só avisar, e as
//! pendências cobram até o contador delas. Com algum bloqueio, o texto leva
//! também os avisos; só com avisos, eles saem ao usuário como `systemMessage`
//! (um `Inject` no `Stop`).
//!
//! ## Tempo
//!
//! O `hooks.json` dá 30 segundos ao `Stop`. As regras leem arquivos pequenos e
//! medem texto: nenhuma roda processo nem rede.
//!
//! ## Acrescentar uma regra
//!
//! Um tipo que implementa [`TurnRule`], no módulo do assunto, e uma linha em
//! [`RULES`], na posição em que o texto dela deve aparecer.

use std::path::Path;

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;

use crate::hooks::task::clarity_check::ClarityRule;
use crate::hooks::task::pending_gate::PendingRule;

/// A resposta que as regras conferem, lida uma vez do `Stop`.
pub struct Turn<'a> {
    /// O texto final do turno; vazio quando o `Stop` não o trouxe.
    pub message: &'a str,
    /// A raiz do projeto.
    pub project_dir: &'a str,
    /// O id da sessão, quando o `Stop` trouxe um.
    pub session: Option<&'a str>,
    /// `stop_hook_active`: esta resposta é a reescrita que um bloqueio pediu.
    pub retry: bool,
    /// O idioma das mensagens do Mustard neste projeto.
    pub lang: Locale,
}

/// O que uma regra achou na resposta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Barra o fim da resposta: o texto vai ao assistente, que reescreve.
    Block(String),
    /// Só avisa: o texto vai ao usuário, e a resposta termina.
    Warn(String),
}

impl Finding {
    fn text(&self) -> &str {
        match self {
            Self::Block(text) | Self::Warn(text) => text,
        }
    }
}

/// Uma regra do fim da resposta.
pub trait TurnRule {
    /// Confere a resposta. `None` quando a regra não tem o que dizer. Uma regra
    /// nunca falha: o que ela não consegue ler vira `None`.
    fn check(&self, turn: &Turn<'_>) -> Option<Finding>;
}

/// As regras, na ordem em que o texto do bloqueio as mostra.
pub(crate) const RULES: &[&dyn TurnRule] = &[&PendingRule, &ClarityRule];

/// A conferência do fim da resposta: todas as [`RULES`], um veredito.
pub struct EndOfTurnCheck;

impl Check for EndOfTurnCheck {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        Ok(run_rules(RULES, input, ctx))
    }
}

/// Roda `rules` sobre o `Stop` de `input` e junta os achados num veredito. Só
/// o `Stop` da sessão principal é conferido — nunca o de um subagente.
pub(crate) fn run_rules(rules: &[&dyn TurnRule], input: &HookInput, ctx: &Ctx) -> Verdict {
    if ctx.trigger != Some(Trigger::Stop) || input.is_subagent() {
        return Verdict::Allow;
    }
    let project_dir = ctx.project_dir_or_cwd(input);
    let turn = Turn {
        message: input.last_assistant_message().unwrap_or_default(),
        project_dir: &project_dir,
        session: input.session_id.as_deref(),
        retry: input.stop_hook_active(),
        lang: mustard_core::ProjectConfig::load(Path::new(&project_dir)).language().text_or_default(),
    };
    let findings: Vec<Finding> = rules.iter().filter_map(|rule| rule.check(&turn)).collect();
    verdict(&findings)
}

/// Um bloqueio só: com algum [`Finding::Block`], o motivo leva todos os
/// achados, na ordem das regras; só com avisos, eles saem ao usuário.
fn verdict(findings: &[Finding]) -> Verdict {
    if findings.is_empty() {
        return Verdict::Allow;
    }
    let text = findings.iter().map(Finding::text).collect::<Vec<_>>().join("\n\n");
    if findings.iter().any(|finding| matches!(finding, Finding::Block(_))) {
        Verdict::Deny { reason: text }
    } else {
        Verdict::Inject { context: text }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::event::pending::{pending_at, PendingOpts};
    use crate::hook_output::hook_specific_output;
    use crate::registry::Registry;
    use crate::shared::context::mark_unit_closed;
    use mustard_core::domain::model::contract::Outcome;
    use serde_json::{json, Value};
    use std::time::{Duration, Instant};

    /// O manifesto de ganchos que o plugin entrega.
    const HOOKS_JSON: &str = include_str!("../../../../../plugin/hooks/hooks.json");

    /// O tempo que o Claude Code dá ao `Stop`, em segundos.
    const STOP_BUDGET_SECS: u64 = 30;

    /// Uma frase de 40 palavras, com o começo que o defeito mostra.
    const FORTY_WORDS: &str = "Depois de ler todos os arquivos do projeto e conferir cada teste \
        que ainda falhava na máquina do usuário, eu ajustei a leitura do idioma e a contagem das \
        linhas para que a resposta final saia bem curta e clara.";

    /// Prosa em inglês com palavras bastantes para o idioma ser julgado, e
    /// clara: as medidas da escrita rodam em todo projeto, e só o idioma deve
    /// decidir o veredito.
    const ENGLISH_REPLY: &str = "The work is done and the tests pass.\n\
        The check now compares the language of the reply with the language of the project.\n\
        It counts the common words of each language.\n\
        A short reply is not judged at all.";

    /// Um projeto que declarou o português do Brasil como idioma do texto.
    const PT_PROJECT: &str = r#"{"language":{"text":"pt-BR"}}"#;

    fn project(config: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("mustard.json"), config).expect("config");
        dir
    }

    fn ctx(root: &Path) -> Ctx {
        Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::Stop))
    }

    /// O `Stop` da sessão principal com o texto final do turno; `retry` é o
    /// `stop_hook_active` da reescrita que um bloqueio pediu.
    fn stop(session: &str, message: &str, retry: bool) -> HookInput {
        HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some(session.to_string()),
            raw: json!({ "last_assistant_message": message, "stop_hook_active": retry }),
            ..HookInput::default()
        }
    }

    /// Roda o `Stop` como o binário roda: cada gancho registrado no `Stop`,
    /// juntado pelo `fold`, e a resposta JSON que vai ao Claude Code.
    fn run_stop(root: &Path, input: &HookInput) -> Value {
        let c = ctx(root);
        let mut outcome = Outcome::allow();
        for module in Registry::new().applicable(Trigger::Stop, None) {
            if let Some(check) = &module.check {
                outcome.fold(check.evaluate(input, &c).unwrap_or(Verdict::Allow));
            }
        }
        hook_specific_output("Stop", &outcome)
            .map(|json| serde_json::from_str(&json).expect("valid JSON"))
            .unwrap_or(Value::Null)
    }

    /// Um projeto com as pendências abertas "Humanize" e "HTML padrao da spec".
    fn project_with_open_items(config: &str) -> tempfile::TempDir {
        let dir = project(config);
        for title in ["Humanize", "HTML padrao da spec"] {
            let out = pending_at(&PendingOpts {
                root: dir.path().to_path_buf(),
                add: true,
                title: Some(title.into()),
                detail: Some("combinado".into()),
                close: None,
                drop: None,
                reason: None,
            });
            assert_eq!(out["ok"], json!(true), "seed: {out}");
        }
        dir
    }

    /// A resposta final com um código como "MSTD-RULE-0008" e uma frase de 40
    /// palavras é barrada uma vez, com os defeitos em português simples; a
    /// reescrita que chega com `stop_hook_active` e ainda reprova só avisa o
    /// usuário. Tudo pelo `Stop` de verdade (registro, `fold` e a resposta
    /// JSON), bem dentro dos 30 segundos que o `hooks.json` dá.
    #[test]
    fn a_reply_with_an_internal_code_and_a_long_sentence_blocks_once_then_warns() {
        assert_eq!(FORTY_WORDS.split_whitespace().count(), 40);
        let dir = project(PT_PROJECT);
        let root = dir.path();
        let reply = format!("A regra MSTD-RULE-0008 ficou pronta.\n{FORTY_WORDS}");
        let started = Instant::now();

        let blocked = run_stop(root, &stop("s1", &reply, false));
        assert_eq!(blocked["decision"], json!("block"), "{blocked}");
        let reason = blocked["reason"].as_str().unwrap_or_else(|| panic!("{blocked}"));
        assert!(
            reason.starts_with(
                "[Mustard] A resposta fugiu da regra de escrita. Reescreva-a em linguagem \
                 simples, corrigindo estes pontos:"
            ),
            "{reason}"
        );
        assert!(reason.contains("\n- MSTD-RULE-0008 é um código interno; diga o assunto pelo nome"), "{reason}");
        assert!(
            reason.contains("\n- frase com 40 palavras: \"Depois de ler todos os arquivos do projeto…\""),
            "{reason}"
        );

        let warned = run_stop(root, &stop("s1", &reply, true));
        assert!(warned.get("decision").is_none(), "the rewrite is released: {warned}");
        let note = warned["systemMessage"].as_str().unwrap_or_else(|| panic!("{warned}"));
        assert!(
            note.starts_with("Mustard · clareza: a resposta acima ainda foge da regra de escrita:"),
            "{note}"
        );
        assert!(note.contains("\n- MSTD-RULE-0008 é um código interno"), "{note}");
        assert!(note.contains("\n- frase com 40 palavras"), "{note}");

        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(STOP_BUDGET_SECS), "{elapsed:?} for two Stops");
    }

    /// O `Stop` tem 30 segundos no `hooks.json`, não mais 5.
    #[test]
    fn the_stop_hook_has_thirty_seconds() {
        let manifest: Value = serde_json::from_str(HOOKS_JSON).expect("hooks.json");
        let timeouts: Vec<u64> = manifest["hooks"]["Stop"]
            .as_array()
            .expect("a Stop entry")
            .iter()
            .flat_map(|matcher| matcher["hooks"].as_array().cloned().unwrap_or_default())
            .map(|hook| hook["timeout"].as_u64().expect("a timeout"))
            .collect();
        assert_eq!(timeouts, vec![STOP_BUDGET_SECS]);
    }

    /// A resposta que sai noutro idioma que não o de `language.text` reprova,
    /// e o idioma vem da configuração: o mesmo inglês passa num projeto em
    /// inglês, e o português reprova nele, com a mensagem no idioma dele. A
    /// chave antiga de idioma não é mais lida: um projeto que só tem ela não
    /// declarou idioma, e nenhuma resposta é julgada pelo idioma.
    #[test]
    fn a_reply_in_another_language_fails_the_end_of_turn_check() {
        let pt = project(PT_PROJECT);
        match EndOfTurnCheck.evaluate(&stop("s1", ENGLISH_REPLY, false), &ctx(pt.path())).expect("never errors") {
            Verdict::Deny { reason } => assert!(
                reason.contains("\n- resposta em en-US; o idioma do projeto e do usuário é pt-BR"),
                "{reason}"
            ),
            other => panic!("an English reply in a pt-BR project must fail, got {other:?}"),
        }

        let en = project(r#"{"language":{"text":"en-US"}}"#);
        let english = EndOfTurnCheck.evaluate(&stop("s1", ENGLISH_REPLY, false), &ctx(en.path()));
        assert_eq!(english.expect("never errors"), Verdict::Allow);
        let portuguese = "A onda terminou e os testes passaram.\n\
            A medição agora compara o idioma da resposta com o idioma do projeto.\n\
            Ela conta as palavras comuns de cada idioma.\n\
            Uma resposta curta não é julgada por ela.";
        match EndOfTurnCheck.evaluate(&stop("s1", portuguese, false), &ctx(en.path())).expect("never errors") {
            Verdict::Deny { reason } => assert!(
                reason.contains("\n- reply in pt-BR; the language of the project and the user is en-US"),
                "{reason}"
            ),
            other => panic!("a Portuguese reply in an en-US project must fail, got {other:?}"),
        }

        let old_key = project(r#"{"specLang":"pt-BR"}"#);
        let verdict = EndOfTurnCheck.evaluate(&stop("s1", ENGLISH_REPLY, false), &ctx(old_key.path()));
        let text = match verdict.expect("never errors") {
            Verdict::Deny { reason } => reason,
            Verdict::Inject { context } => context,
            _ => String::new(),
        };
        assert!(!text.contains("resposta em en-US"), "the old key declares no language: {text}");
    }

    /// Lado a lado — a cobrança de pendências mudou de gancho próprio para
    /// regra da conferência. Pela regra sozinha e pela conferência inteira, o
    /// mesmo fechamento dá o mesmo bloqueio, com o mesmo texto.
    #[test]
    fn the_pending_rule_blocks_the_same_inside_the_end_of_turn_check() {
        let config = PT_PROJECT;
        let (alone, inside) = (project_with_open_items(config), project_with_open_items(config));
        let message = "Fechei a unidade; segue o html padrao da spec.";
        for dir in [&alone, &inside] {
            mark_unit_closed(&dir.path().to_string_lossy(), "s-close");
        }

        let by_rule = run_rules(&[&PendingRule], &stop("s-close", message, false), &ctx(alone.path()));
        let by_check = EndOfTurnCheck
            .evaluate(&stop("s-close", message, false), &ctx(inside.path()))
            .expect("never errors");
        let Verdict::Deny { reason } = &by_rule else {
            panic!("the rule blocks a closure that omits an item, got {by_rule:?}");
        };
        assert!(reason.contains("Humanize"), "{reason}");
        assert_eq!(by_check, by_rule, "one closure, one block, whichever path runs it");
    }

    /// As duas regras dividem um bloqueio só, na ordem de [`RULES`]: as
    /// pendências e depois a clareza. Na reescrita, a pendência citada libera,
    /// e a clareza que ainda reprova só avisa.
    #[test]
    fn pending_and_clarity_share_one_block() {
        let dir = project_with_open_items(PT_PROJECT);
        let root = dir.path();
        mark_unit_closed(&root.to_string_lossy(), "s-both");

        let first = "Fechei a unidade MSTD-RULE-0008; segue o html padrao da spec.";
        let Verdict::Deny { reason } =
            EndOfTurnCheck.evaluate(&stop("s-both", first, false), &ctx(root)).expect("never errors")
        else {
            panic!("both rules have something to say");
        };
        let pending = reason.find("[Mustard] Uma unidade fechou").unwrap_or_else(|| panic!("{reason}"));
        let clarity = reason.find("[Mustard] A resposta fugiu").unwrap_or_else(|| panic!("{reason}"));
        assert!(pending < clarity, "the rules keep their order: {reason}");
        assert!(reason.contains("Humanize") && reason.contains("- MSTD-RULE-0008 é um código interno"), "{reason}");

        let rewrite = "Fechei a unidade MSTD-RULE-0008; seguem o Humanize e o html padrao da spec.";
        match EndOfTurnCheck.evaluate(&stop("s-both", rewrite, true), &ctx(root)).expect("never errors") {
            Verdict::Inject { context } => {
                assert!(context.contains("- MSTD-RULE-0008 é um código interno"), "{context}");
                assert!(!context.contains("Uma unidade fechou"), "the cited items passed: {context}");
            }
            other => panic!("the rewrite only warns, got {other:?}"),
        }
    }

    /// Letra com número fora do formato do Mustard é texto comum: o nome de
    /// um produto ou o tamanho de uma folha não é barrado.
    #[test]
    fn letters_and_numbers_outside_the_code_format_pass() {
        let dir = project(PT_PROJECT);
        let reply = "Guardei o arquivo no R2 da Cloudflare, no S3 e numa folha A4.";
        let verdict = EndOfTurnCheck.evaluate(&stop("s1", reply, false), &ctx(dir.path())).expect("never errors");
        assert_eq!(verdict, Verdict::Allow);
    }

    /// Fora do `Stop` da sessão principal nada é conferido.
    #[test]
    fn only_the_main_session_stop_is_checked() {
        let dir = project(PT_PROJECT);
        let reply = format!("A regra MSTD-RULE-0008 ficou pronta.\n{FORTY_WORDS}");
        let mut sub = stop("s1", &reply, false);
        sub.agent_id = Some("child".to_string());
        assert_eq!(EndOfTurnCheck.evaluate(&sub, &ctx(dir.path())).expect("never errors"), Verdict::Allow);
        let pre = Ctx::for_test(dir.path().to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        let other = EndOfTurnCheck.evaluate(&stop("s1", &reply, false), &pre).expect("never errors");
        assert_eq!(other, Verdict::Allow);
    }
}
