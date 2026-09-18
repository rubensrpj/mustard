//! `end_of_turn_check` — a conferência do fim da resposta.
//!
//! No `Stop` da sessão principal, o texto final do turno
//! (`last_assistant_message`) passa por uma lista de regras ([`TurnRule`]), e o
//! que elas acham sai num bloqueio só. Hoje são duas, cada uma no módulo do
//! assunto:
//!
//! - as pendências ([`PendingRule`], `pending_gate.rs`): o turno em que a
//!   spec fechou, ou entrou no merge, não termina sem citar cada pendência
//!   aberta nascida nela. Esta barra;
//! - a clareza ([`ClarityRule`], `clarity_check.rs`): a resposta segue a regra
//!   de escrita e sai no idioma que o projeto declarou. Esta nunca barra: o
//!   erro que ela acha fica guardado na pasta da sessão, e a linha escondida
//!   da mensagem seguinte do usuário o leva numa frase curta.
//!
//! ## Um bloqueio só
//!
//! Antes, cada regra era um gancho do `Stop`, e o primeiro bloqueio vencia: o
//! bloqueio de um engolia o do outro, e o assistente respondia sem saber de
//! tudo. Agora todas as regras rodam, e cada achado entra no mesmo texto, na
//! ordem de [`RULES`].
//!
//! ## Cada regra guarda a sua política
//!
//! Uma regra que barra devolve [`Finding::Block`]: o texto vai ao assistente,
//! que responde no mesmo turno. A volta que um bloqueio pediu chega com
//! `stop_hook_active` ([`Turn::retry`]), e cada regra decide o que faz com
//! ela: a clareza soma o erro da volta ao da resposta barrada, e as
//! pendências cobram até o contador delas. Nenhuma regra fala com o usuário:
//! sem bloqueio, a resposta termina calada.
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
    /// `stop_hook_active`: esta resposta é a volta que um bloqueio pediu.
    pub retry: bool,
    /// O idioma das mensagens do Mustard neste projeto.
    pub lang: Locale,
}

/// O que uma regra achou na resposta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Barra o fim da resposta: o texto vai ao assistente, que responde no
    /// mesmo turno.
    Block(String),
}

impl Finding {
    fn text(&self) -> &str {
        let Self::Block(text) = self;
        text
    }
}

/// Uma regra do fim da resposta.
pub trait TurnRule {
    /// Confere a resposta. `None` quando a regra não barra; a que não barra
    /// pode guardar o que achou para depois, como a clareza. Uma regra nunca
    /// falha: o que ela não consegue ler vira `None`.
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

/// Um bloqueio só: o motivo leva todos os achados, na ordem das regras.
fn verdict(findings: &[Finding]) -> Verdict {
    if findings.is_empty() {
        return Verdict::Allow;
    }
    let reason = findings.iter().map(Finding::text).collect::<Vec<_>>().join("\n\n");
    Verdict::Deny { reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::event::pending::{pending_at, PendingOpts};
    use crate::hook_output::hook_specific_output;
    use crate::commands::spec_events::write::record_phase;
    use crate::hooks::task::pending_gate::seed_spec;
    use crate::registry::Registry;
    use mustard_core::domain::model::contract::Outcome;
    use serde_json::{json, Value};
    use std::time::{Duration, Instant};

    /// O manifesto de ganchos que o plugin entrega.
    const HOOKS_JSON: &str = include_str!("../../../../../plugin/hooks/hooks.json");

    /// Os estilos de resposta que o plugin entrega, um por idioma.
    const STYLE_PT: &str = include_str!("../../../../../plugin/output-styles/mustard-pt-BR.md");
    const STYLE_EN: &str = include_str!("../../../../../plugin/output-styles/mustard-en-US.md");

    /// O tempo que o Claude Code dá ao `Stop`, em segundos.
    const STOP_BUDGET_SECS: u64 = 30;

    /// Uma frase de 40 palavras, com o começo que o defeito mostra.
    const FORTY_WORDS: &str = "Depois de ler todos os arquivos do projeto e conferir cada teste \
        que ainda falhava na máquina do usuário, eu ajustei a leitura do idioma e a contagem das \
        linhas para que a resposta final saia bem curta e clara.";

    /// Prosa em inglês com palavras bastantes para o idioma ser julgado, e
    /// clara: as medidas da escrita rodam em todo projeto, e só o idioma deve
    /// decidir o erro.
    const ENGLISH_REPLY: &str = "The work is done and the tests pass.\n\
        The check now compares the language of the reply with the language of the project.\n\
        It counts the common words of each language.\n\
        A short reply is not judged at all.";

    /// Um projeto que declarou o português do Brasil como idioma do texto.
    const PT_PROJECT: &str = r#"{"language":{"text":"pt-BR"}}"#;

    /// Um projeto que declarou o inglês dos Estados Unidos.
    const EN_PROJECT: &str = r#"{"language":{"text":"en-US"}}"#;

    /// A linha escondida de cada mensagem num projeto em pt-BR, a mesma de
    /// antes.
    const PT_LINE: &str =
        "Responda em português do Brasil, em texto simples: frases curtas e nenhum código interno.";

    /// A mesma linha num projeto em en-US.
    const EN_LINE: &str = "Answer in US English, in plain text: short sentences and no internal codes.";

    /// A mesma linha num projeto que não declarou idioma.
    const UNDECLARED_LINE: &str =
        "Responda no idioma de quem escreve, em texto simples: frases curtas e nenhum código interno.";

    fn project(config: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("mustard.json"), config).expect("config");
        dir
    }

    fn ctx(root: &Path) -> Ctx {
        Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::Stop))
    }

    /// O `Stop` da sessão principal com o texto final do turno; `retry` é o
    /// `stop_hook_active` da volta que um bloqueio pediu.
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

    /// Um evento como o Claude Code o manda ao `mustard-rt on <evento>`, com
    /// todos os campos de uma sessão, pelo despachante inteiro, e a resposta
    /// JSON que volta a ele; `Value::Null` quando nada volta.
    fn hook_event(root: &Path, event: &str, session: &str, fields: Value) -> Value {
        let mut payload = json!({
            "session_id": session,
            "transcript_path": root.join(format!("{session}.jsonl")),
            "cwd": root,
            "permission_mode": "default",
            "hook_event_name": event,
        });
        if let (Some(all), Some(more)) = (payload.as_object_mut(), fields.as_object()) {
            all.extend(more.clone());
        }
        let input: HookInput = serde_json::from_value(payload).expect("a hook payload");
        let outcome = crate::dispatch::run_event(Trigger::from_event_name(event), &input);
        hook_specific_output(event, &outcome)
            .map(|out| serde_json::from_str(&out).expect("valid JSON"))
            .unwrap_or(Value::Null)
    }

    /// O `Stop` de verdade, com o texto final e o `stop_hook_active`.
    fn stop_event(root: &Path, session: &str, message: &str, retry: bool) -> Value {
        hook_event(root, "Stop", session, json!({ "last_assistant_message": message, "stop_hook_active": retry }))
    }

    /// O texto escondido que a mensagem seguinte do usuário leva ao
    /// assistente, pelo gancho de verdade.
    fn next_line(root: &Path, session: &str) -> String {
        let out = hook_event(root, "UserPromptSubmit", session, json!({ "prompt": "e agora?" }));
        out["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_default().to_string()
    }

    /// Fecha, pela porta do binário, uma spec em que as duas pendências de
    /// [`project_with_open_items`] nasceram, com a sessão `session` ligada a ela.
    fn close_spec_with_both_items(root: &Path, session: &str) {
        seed_spec(root, "trava", &[1, 2], session);
        // Sem sessão: a do processo de teste, vinda do ambiente, não entra.
        assert!(record_phase(root, "trava", "closed", None), "the binary door records the close");
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
                ..PendingOpts::default()
            });
            assert_eq!(out["ok"], json!(true), "seed: {out}");
        }
        dir
    }

    /// A resposta final com uma frase de 40 palavras, uma sigla sem explicação
    /// e um código como "MSTD-RULE-0008" não é barrada, e nada volta para a
    /// tela: nem bloqueio, nem aviso. A mensagem seguinte leva os três erros
    /// numa frase curta. Tudo pelo `Stop` de verdade (registro, `fold` e a
    /// resposta JSON), bem dentro dos 30 segundos que o `hooks.json` dá.
    #[test]
    fn a_reply_that_misses_the_writing_rule_is_not_blocked() {
        assert_eq!(FORTY_WORDS.split_whitespace().count(), 40);
        let dir = project(PT_PROJECT);
        let root = dir.path();
        let reply = format!("A regra MSTD-RULE-0008 ficou pronta no CI.\n{FORTY_WORDS}");
        let started = Instant::now();
        assert_eq!(run_stop(root, &stop("s1", &reply, false)), Value::Null, "neither blocked nor warned about");
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(STOP_BUDGET_SECS), "{elapsed:?} for one Stop");
        assert_eq!(
            next_line(root, "s1"),
            format!(
                "{PT_LINE} Na última resposta: frase com 40 palavras; CI é uma sigla sem explicação; \
                 MSTD-RULE-0008 é um código interno."
            )
        );
    }

    /// A resposta com 16 linhas, uma a mais que o limite de 15, e sem nenhum
    /// outro defeito não é barrada pelo `Stop` de verdade; a mensagem seguinte
    /// leva o erro, nos dois idiomas. Com 15 linhas nada vai: a conta das
    /// linhas não mudou. O jeito de consertar mora no estilo de resposta de
    /// cada idioma, que manda o JSON, a tabela ou o documento pedido para a
    /// página avulsa.
    #[test]
    fn a_reply_over_fifteen_lines_goes_with_the_next_message() {
        let cases = [
            (PT_PROJECT, "Uma linha curta.", PT_LINE, "Na última resposta: resposta com 16 linhas, e o limite é 15."),
            (EN_PROJECT, "The test runs fine.", EN_LINE, "In the last reply: reply with 16 lines, and the limit is 15."),
        ];
        for style in [STYLE_PT, STYLE_EN] {
            assert!(style.contains("`mustard-rt run page`"), "the answer style names the page: {style}");
        }
        for (config, line, hidden, note) in cases {
            let dir = project(config);
            let root = dir.path();
            let fits = vec![line; 15].join("\n");
            assert_eq!(run_stop(root, &stop("s1", &fits, false)), Value::Null, "{config}");
            assert_eq!(next_line(root, "s1"), hidden, "15 lines carry nothing: {config}");

            let over = vec![line; 16].join("\n");
            assert_eq!(run_stop(root, &stop("s1", &over, false)), Value::Null, "16 lines are not blocked: {config}");
            assert_eq!(next_line(root, "s1"), format!("{hidden} {note}"));
        }
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

    /// A resposta que sai noutro idioma que não o de `language.text` não é
    /// barrada, e o erro vai na mensagem seguinte. O idioma vem da
    /// configuração: o mesmo inglês passa num projeto em inglês, e o português
    /// vira erro nele, no idioma dele. A chave antiga de idioma não é mais
    /// lida: um projeto que só tem ela não declarou idioma, e nenhuma resposta
    /// ganha erro de idioma.
    #[test]
    fn a_reply_in_another_language_goes_with_the_next_message() {
        let pt = project(PT_PROJECT);
        let verdict = EndOfTurnCheck.evaluate(&stop("s1", ENGLISH_REPLY, false), &ctx(pt.path()));
        assert_eq!(verdict.expect("never errors"), Verdict::Allow);
        assert_eq!(next_line(pt.path(), "s1"), format!("{PT_LINE} Na última resposta: resposta em en-US."));

        let en = project(EN_PROJECT);
        let english = EndOfTurnCheck.evaluate(&stop("s1", ENGLISH_REPLY, false), &ctx(en.path()));
        assert_eq!(english.expect("never errors"), Verdict::Allow);
        assert_eq!(next_line(en.path(), "s1"), EN_LINE);
        let portuguese = "A onda terminou e os testes passaram.\n\
            A medição agora compara o idioma da resposta com o idioma do projeto.\n\
            Ela conta as palavras comuns de cada idioma.\n\
            Uma resposta curta não é julgada por ela.";
        let verdict = EndOfTurnCheck.evaluate(&stop("s1", portuguese, false), &ctx(en.path()));
        assert_eq!(verdict.expect("never errors"), Verdict::Allow);
        assert_eq!(next_line(en.path(), "s1"), format!("{EN_LINE} In the last reply: reply in pt-BR."));

        let old_key = project(r#"{"specLang":"pt-BR"}"#);
        let verdict = EndOfTurnCheck.evaluate(&stop("s1", ENGLISH_REPLY, false), &ctx(old_key.path()));
        assert_eq!(verdict.expect("never errors"), Verdict::Allow);
        assert_eq!(next_line(old_key.path(), "s1"), UNDECLARED_LINE, "the old key declares no language");
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
            close_spec_with_both_items(dir.path(), "s-close");
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

    /// A conferência da escrita deixou de barrar, e a das pendências continua
    /// barrando, pelos ganchos de verdade. A spec fecha, e a resposta final
    /// que não cita "Humanize", nascida nela, é barrada com o texto das
    /// pendências e só ele, mesmo com uma frase longa e um código interno. A
    /// volta que cita as duas passa. O erro de escrita das duas respostas vai
    /// na mensagem seguinte, e o fechamento não cobra de novo.
    #[test]
    fn the_pending_rule_still_blocks_after_the_clarity_change() {
        let dir = project_with_open_items(PT_PROJECT);
        let root = dir.path();
        close_spec_with_both_items(root, "s-both");

        let first = format!("Fechei a unidade MSTD-RULE-0008; segue o html padrao da spec.\n{FORTY_WORDS}");
        let blocked = stop_event(root, "s-both", &first, false);
        assert_eq!(blocked["decision"], json!("block"), "{blocked}");
        let pending = mustard_core::translate("pending.gate.block", Locale::PtBr)
            .replace("{spec}", "trava")
            .replace("{count}", "1")
            .replace("{items}", "P-1 \"Humanize\"");
        assert_eq!(blocked["reason"], json!(pending), "the block carries the pending text and nothing else");

        let rewrite = "Fechei a unidade; seguem o Humanize e o html padrao da spec.";
        assert_eq!(stop_event(root, "s-both", rewrite, true), Value::Null, "the cited items pass");
        assert_eq!(
            next_line(root, "s-both"),
            format!("{PT_LINE} Na última resposta: frase com 40 palavras; MSTD-RULE-0008 é um código interno.")
        );
        assert_eq!(stop_event(root, "s-both", "Fechei a unidade.", false), Value::Null, "the closure settled");
    }

    /// Letra com número fora do formato do Mustard é texto comum: o nome de
    /// um produto ou o tamanho de uma folha não vira erro.
    #[test]
    fn letters_and_numbers_outside_the_code_format_pass() {
        let dir = project(PT_PROJECT);
        let reply = "Guardei o arquivo no R2 da Cloudflare, no S3 e numa folha A4.";
        let verdict = EndOfTurnCheck.evaluate(&stop("s1", reply, false), &ctx(dir.path())).expect("never errors");
        assert_eq!(verdict, Verdict::Allow);
        assert_eq!(next_line(dir.path(), "s1"), PT_LINE);
    }

    /// Fora do `Stop` da sessão principal nada é conferido nem guardado.
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
        assert_eq!(next_line(dir.path(), "s1"), PT_LINE, "nothing was kept");
    }
}
