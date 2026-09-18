//! `clarity_check` — a regra de clareza do fim da resposta ([`ClarityRule`],
//! uma das regras do `end_of_turn_check`): mede o texto final do assistente
//! contra a regra de escrita.
//!
//! ## Por que existe
//!
//! O assistente recebe a regra de escrita no estilo de resposta, e toda
//! mensagem leva uma linha curta que a lembra
//! (`prompt_submit_inject::message_line`). Nada conferia se ela foi cumprida:
//! em 09/09/2026 o usuário reclamou duas vezes de respostas difíceis de
//! entender, com a regra ativa, e nenhum gancho percebeu.
//!
//! ## Quando mede
//!
//! O `end_of_turn_check` só chama as regras no `Stop` da sessão principal.
//! Aqui, dois fatos a mais:
//!
//! 1. O projeto tem `mustard.json`. Nele todas as medições da escrita rodam
//!    sempre: o tom é um só, didático, e não há chave que o desligue. O idioma
//!    da resposta só é julgado quando o projeto DECLAROU o idioma do texto
//!    (`language.text`); sem ele não há veredito de idioma, porque o padrão
//!    resolvido não é uma escolha.
//! 2. O `Stop` trouxe texto final.
//!
//! ## O que faz
//!
//! Mede o texto com o medidor do núcleo (`domain::clarity`): frase longa,
//! sigla sem as palavras por extenso, código do Mustard ("MSTD-RULE-0005"),
//! tamanho, a nota de Flesch em português e o idioma.
//!
//! - Na primeira resposta que reprova, bloqueia: o assistente recebe os
//!   defeitos e escreve logo abaixo da resposta um complemento curto sobre
//!   eles, sem reescrevê-la. A resposta já apareceu na tela, e nenhum gancho a
//!   segura antes; reescrevê-la inteira a mostraria duas vezes. O bloqueio é o
//!   único caminho que chega ao assistente no mesmo turno.
//! - O complemento chega com `stop_hook_active` e não é conferido: não barra,
//!   o que prenderia o turno num laço, nem avisa o usuário.
//! - Guarda em `.claude/.session/<sid>/clarity.json` as siglas já explicadas
//!   na sessão, para a próxima medição não cobrar de novo. O complemento
//!   também é medido para isso: a sigla que ele explica fica guardada. É a
//!   única coisa que a medição grava: nenhum arquivo de eventos.
//! - As siglas do dia a dia do projeto, listadas em `acronyms` no
//!   `mustard.json`, vão ao medidor junto das já explicadas na sessão: passam
//!   sem o nome por extenso, e a sigla fora da lista continua cobrada.
//!
//! O bloqueio lista no máximo [`MAX_LISTED_DEFECTS`] defeitos, cada um cortado
//! em [`MAX_DEFECT_CHARS`] caracteres; o resto vira uma contagem. Falha de
//! disco só cala o registro; o bloqueio ainda sai.
//!
//! A gravação de lição (`run write lesson`) mede o texto da lição pela mesma
//! medição ([`measure_in_project`]), com o que o projeto declara.

use std::path::{Path, PathBuf};

use mustard_core::domain::clarity::{measure, ClarityReport};
use mustard_core::io::fs;
use mustard_core::platform::i18n::Locale;
use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};

use crate::hooks::task::end_of_turn_check::{Finding, Turn, TurnRule};

/// O arquivo da sessão onde a medição guarda o que precisa lembrar.
const RECORD_FILE: &str = "clarity.json";

/// O que a sessão lembra entre uma resposta e a próxima.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ClarityRecord {
    /// As siglas que alguma resposta desta sessão já explicou. Um registro
    /// antigo também traz termos aqui; eles só ficam guardados.
    #[serde(default)]
    explained: Vec<String>,
}

/// A regra de clareza do fim da resposta.
pub struct ClarityRule;

impl TurnRule for ClarityRule {
    fn check(&self, turn: &Turn<'_>) -> Option<Finding> {
        let root = Path::new(turn.project_dir);
        if !mustard_core::ProjectConfig::exists(root) || turn.message.trim().is_empty() {
            return None;
        }
        // Os defeitos saem no idioma resolvido (`turn.lang`).
        let defects = writing_defects(root, turn);
        // O complemento (`stop_hook_active`) foi medido só para a memória da
        // sessão: ele não barra nem avisa.
        if defects.is_empty() || turn.retry {
            return None;
        }
        Some(Finding::Block(with_head(&defects, turn.lang)))
    }
}

/// Todas as medições da escrita, com a memória da sessão: o que esta resposta
/// explicou fica guardado.
fn writing_defects(root: &Path, turn: &Turn<'_>) -> Vec<String> {
    let record_path = record_path(root, turn.session);
    let mut record = record_path.as_deref().map(read_record).unwrap_or_default();
    let report = measure_in_project(root, turn.message, &record.explained);
    for term in &report.explained {
        if !record.explained.contains(term) {
            record.explained.push(term.clone());
        }
    }
    if let Some(path) = &record_path {
        write_record(path, &record);
    }
    report.defects(turn.lang)
}

/// A medição da escrita de `text` com o que o projeto em `root` declara. O
/// idioma que a prosa precisa ter é só o DECLARADO em `language.text`: o
/// padrão resolvido é pt-BR, e um projeto em inglês que nunca declarou idioma
/// teria todo texto apontado. As siglas do projeto (`acronyms` no
/// `mustard.json`) vão ao medidor junto das `explained`, as já explicadas na
/// sessão, mas não entram na memória dela: a sigla que sai do `mustard.json`
/// volta a ser cobrada. A conferência do fim da resposta e a gravação de
/// lição medem por aqui, e não discordam sobre o mesmo texto.
pub(crate) fn measure_in_project(root: &Path, text: &str, explained: &[String]) -> ClarityReport {
    let config = mustard_core::ProjectConfig::load(root);
    let known: Vec<String> = explained.iter().cloned().chain(config.acronyms()).collect();
    measure(text, &known, config.language().text)
}

/// Quantos defeitos o bloqueio lista; o resto vira uma contagem.
const MAX_LISTED_DEFECTS: usize = 5;

/// O tamanho máximo de uma linha de defeito, em caracteres.
const MAX_DEFECT_CHARS: usize = 160;

/// O pedido do complemento, do catálogo, seguido de um defeito por linha — no
/// máximo [`MAX_LISTED_DEFECTS`], cada um com até [`MAX_DEFECT_CHARS`]
/// caracteres.
fn with_head(defects: &[String], lang: Locale) -> String {
    let mut text = mustard_core::translate("clarity.block.head", lang).to_string();
    for defect in defects.iter().take(MAX_LISTED_DEFECTS) {
        text.push_str("\n- ");
        text.extend(defect.chars().take(MAX_DEFECT_CHARS));
    }
    let rest = defects.len().saturating_sub(MAX_LISTED_DEFECTS);
    if rest > 0 {
        text.push_str("\n- ");
        text.push_str(&mustard_core::translate("clarity.more", lang).replace("{count}", &rest.to_string()));
    }
    text
}

/// `.claude/.session/<sid>/clarity.json`, para um id de sessão utilizável — a
/// mesma base dos marcadores de entrega dos injetáveis. `None` sem sessão, com
/// `"unknown"` ou com um id que sairia da pasta da sessão.
fn record_path(root: &Path, session: Option<&str>) -> Option<PathBuf> {
    let sid = session?.trim();
    if sid.is_empty() || sid == "unknown" || sid.contains(['/', '\\']) || sid.contains("..") {
        return None;
    }
    Some(
        ClaudePaths::for_project(root)
            .ok()?
            .claude_dir()
            .join(".session")
            .join(sid)
            .join(RECORD_FILE),
    )
}

/// O registro da sessão; vazio quando o arquivo falta ou não se lê. Um
/// registro antigo, com a lista de defeitos que a mensagem seguinte levava,
/// ainda se lê: o campo que sobra é ignorado.
fn read_record(path: &Path) -> ClarityRecord {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Grava o registro. Falha de disco só perde a memória da sessão: a próxima
/// medição cobra de novo um termo já explicado.
fn write_record(path: &Path, record: &ClarityRecord) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(record) {
        let _ = fs::write_atomic(path, &bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::session::prompt_entry::PromptEntry;
    use crate::hooks::task::end_of_turn_check::run_rules;
    use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
    use serde_json::json;
    use tempfile::tempdir;

    /// Reprova por uma sigla sem as palavras por extenso.
    const FAILING: &str = "O CI falhou de novo.";
    /// Passa: frase curta e sem sigla.
    const CLEAR: &str = "A resposta ficou curta e clara.";

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

    /// O complemento que um bloqueio pediu: o `Stop` com `stop_hook_active`.
    fn complement(session: &str, message: &str) -> HookInput {
        let mut input = stop(session, message);
        input.raw["stop_hook_active"] = json!(true);
        input
    }

    /// A regra de clareza sozinha, como a conferência do fim da resposta a roda.
    fn check(root: &Path, input: &HookInput) -> Verdict {
        run_rules(&[&ClarityRule], input, &ctx(root, Trigger::Stop))
    }

    fn record_file(root: &Path, session: &str) -> PathBuf {
        root.join(".claude/.session").join(session).join(RECORD_FILE)
    }

    /// Cada linha de qualquer arquivo de eventos (`.ndjson`) sob `root/.claude`.
    fn event_rows(root: &Path) -> Vec<String> {
        files_under(&root.join(".claude"))
            .into_iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "ndjson"))
            .flat_map(|path| {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                text.lines().map(str::to_string).collect::<Vec<_>>()
            })
            .collect()
    }

    /// Todos os arquivos sob `dir`, em ordem, com o caminho inteiro.
    fn files_under(dir: &Path) -> Vec<PathBuf> {
        fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, files);
                } else {
                    files.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(dir, &mut files);
        files.sort();
        files
    }

    /// A resposta que reprova é barrada, e o assistente recebe no próprio
    /// bloqueio o pedido de um complemento sobre os defeitos; o complemento,
    /// que chega com `stop_hook_active`, não é barrado nem vira aviso, mesmo
    /// reprovando. Os defeitos não andam na mensagem seguinte.
    #[test]
    fn a_failing_reply_asks_for_a_complement_and_the_complement_is_not_judged() {
        let dir = project();
        let root = dir.path();
        match check(root, &stop("s1", FAILING)) {
            Verdict::Deny { reason } => assert_eq!(
                reason,
                "Mustard: complemento abaixo. Sem reescrever a resposta, escreva logo abaixo dela \
                 um complemento curto sobre estes pontos:\n\
                 - CI é uma sigla sem explicação; diga o nome por extenso"
            ),
            other => panic!("a failing reply blocks, got {other:?}"),
        }
        assert_eq!(check(root, &complement("s1", FAILING)), Verdict::Allow, "the complement is not judged");

        let next = match PromptEntry.evaluate(
            &HookInput {
                hook_event_name: Some("UserPromptSubmit".to_string()),
                session_id: Some("s1".to_string()),
                raw: json!({ "prompt": "e agora?" }),
                ..HookInput::default()
            },
            &ctx(root, Trigger::UserPromptSubmit),
        ) {
            Ok(Verdict::Inject { context }) => context,
            _ => String::new(),
        };
        assert!(!next.contains("CI sem as palavras"), "the next prompt carries no defect: {next}");
    }

    /// A resposta que passa não barra nem avisa.
    #[test]
    fn a_clear_reply_passes() {
        let dir = project();
        assert_eq!(check(dir.path(), &stop("s1", CLEAR)), Verdict::Allow);
        assert_eq!(check(dir.path(), &complement("s1", CLEAR)), Verdict::Allow);
    }

    /// Uma resposta em inglês num projeto em português reprova pelo idioma.
    #[test]
    fn a_reply_in_another_language_blocks() {
        let dir = project();
        let reply = "The wave is done and the tests pass.\n\
            The check now compares the language of the reply with the language of the project.\n\
            It counts the common words of each language.\n\
            A short reply is not judged at all.";
        let Verdict::Deny { reason } = check(dir.path(), &stop("s1", reply)) else {
            panic!("a reply in another language blocks");
        };
        assert!(reason.contains("- resposta em en-US; o idioma do projeto e do usuário é pt-BR"), "{reason}");
    }

    /// A escrita é medida em todo projeto com `mustard.json`, declare ou não o
    /// idioma, e a antiga chave do tom não desliga nada: ela não é mais lida.
    /// Sem `mustard.json` nada é medido. Um subagente e outro evento nunca
    /// são medidos.
    #[test]
    fn writing_is_measured_in_every_project() {
        for config in [r#"{"language":{"text":"pt-BR"}}"#, "{}", r#"{"tone":"technical"}"#] {
            let dir = project_with(config);
            let root = dir.path();
            assert!(matches!(check(root, &stop("s1", FAILING)), Verdict::Deny { .. }), "{config}");
            assert!(record_file(root, "s1").is_file(), "{config}: recorded");
            assert!(event_rows(root).is_empty(), "{config}: no event line: {:?}", event_rows(root));
        }
        let bare = tempdir().unwrap();
        assert_eq!(check(bare.path(), &stop("s1", FAILING)), Verdict::Allow, "no mustard.json, no measurement");

        let dir = project();
        let mut sub = stop("s1", FAILING);
        sub.agent_id = Some("child".to_string());
        assert_eq!(check(dir.path(), &sub), Verdict::Allow);
        let pre = ctx(dir.path(), Trigger::PreToolUse);
        assert_eq!(run_rules(&[&ClarityRule], &stop("s1", FAILING), &pre), Verdict::Allow);
    }

    /// A medição não grava arquivo de eventos: depois de um bloqueio e de um
    /// complemento na mesma sessão, a única coisa debaixo de `.claude/` é a
    /// memória da sessão. Uma pasta de eventos que já existia fica com os
    /// mesmos bytes, e nada novo nasce nela.
    #[test]
    fn the_clarity_check_writes_no_event_file() {
        let dir = project();
        let root = dir.path();
        assert!(matches!(check(root, &stop("s1", FAILING)), Verdict::Deny { .. }));
        assert_eq!(check(root, &complement("s1", FAILING)), Verdict::Allow);
        assert_eq!(files_under(&root.join(".claude")), vec![record_file(root, "s1")]);

        let dir = project();
        let root = dir.path();
        let old = root.join(".claude/.events/antigo.ndjson");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        let bytes = b"{\"event\":\"uma linha que ja estava aqui\"}\n";
        std::fs::write(&old, bytes).unwrap();
        assert!(matches!(check(root, &stop("s1", FAILING)), Verdict::Deny { .. }));
        assert_eq!(check(root, &complement("s1", FAILING)), Verdict::Allow);
        assert_eq!(files_under(&root.join(".claude")), vec![old.clone(), record_file(root, "s1")]);
        assert_eq!(std::fs::read(&old).unwrap(), bytes, "the old event file is untouched");
    }

    /// Sem sessão, ou com a sessão `unknown`, a medição não grava nada: nenhuma
    /// pasta nasce debaixo do projeto. O bloqueio sai do mesmo jeito.
    #[test]
    fn without_a_session_the_clarity_check_writes_nothing() {
        let mut no_session = stop("s1", FAILING);
        no_session.session_id = None;
        for input in [no_session, stop("unknown", FAILING)] {
            let dir = project();
            let root = dir.path();
            assert!(matches!(check(root, &input), Verdict::Deny { .. }), "the block still comes out");
            assert!(!root.join(".claude").exists(), "{:?}", files_under(&root.join(".claude")));
        }
    }

    /// O bloqueio mostra no máximo [`MAX_LISTED_DEFECTS`] defeitos e fica
    /// abaixo de 1.500 caracteres, qualquer que seja a resposta.
    #[test]
    fn clarity_defects_are_capped() {
        let dir = project();
        let root = dir.path();
        // Sessenta frases longas de palavras compridas: cada defeito passa do
        // corte de [`MAX_DEFECT_CHARS`], e os que sobram viram uma contagem.
        let long = vec!["palavraextraordinariamentecomprida"; 40].join(" ");
        let reply = format!("{long}.\n").repeat(60);
        let Verdict::Deny { reason: text } = check(root, &stop("s1", &reply)) else {
            panic!("a failing reply blocks");
        };
        let listed: Vec<&str> = text.lines().skip(1).collect();
        let (count, defects) = listed.split_last().unwrap_or_else(|| panic!("{text}"));
        assert!(count.starts_with("- e mais "), "the rest becomes a count: {text}");
        assert_eq!(defects.len(), MAX_LISTED_DEFECTS, "{text}");
        for defect in defects {
            assert_eq!(defect.chars().count(), "- ".len() + MAX_DEFECT_CHARS, "cut: {defect}");
        }
        assert!(text.chars().count() < 1_500, "{} chars: {text}", text.chars().count());
    }

    /// A sigla explicada numa resposta não é cobrada na seguinte: o registro
    /// da sessão guarda o que já foi explicado.
    #[test]
    fn explained_acronym_carries_across_replies() {
        let dir = project();
        let root = dir.path();
        assert_eq!(check(root, &stop("s1", "O CI (integração contínua) falhou.")), Verdict::Allow);
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        // Outra sessão começa do zero.
        assert!(matches!(check(root, &stop("s2", FAILING)), Verdict::Deny { .. }));
    }

    /// As palavras que o estilo de resposta antigo listava como nomes
    /// inventados não são mais cobradas: "slug" e "gate" passam, mesmo num
    /// projeto com glossário.
    #[test]
    fn a_reply_that_says_slug_or_gate_is_not_blocked() {
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join("CONTEXT.md"), "# Glossário\n\n**Slug**: o nome curto da spec.\n").unwrap();
        for reply in ["Troquei o slug da spec.", "O gate da onda passou."] {
            assert_eq!(check(root, &stop("s1", reply)), Verdict::Allow, "{reply}");
        }
    }

    /// Uma palavra da lista antiga escrita em maiúsculas é medida como
    /// qualquer sigla: sem as palavras por extenso, barra.
    #[test]
    fn an_uppercase_word_of_the_old_seed_is_measured_as_any_acronym() {
        let dir = project();
        let Verdict::Deny { reason } = check(dir.path(), &stop("s1", "O GATE falhou.")) else {
            panic!("an acronym without its full words blocks");
        };
        assert!(reason.contains("\n- GATE é uma sigla sem explicação"), "{reason}");
    }

    /// O complemento não é julgado, mas a sigla que ele explica fica na
    /// memória da sessão: a resposta seguinte que a usa não é cobrada de novo.
    #[test]
    fn what_the_complement_explains_is_remembered() {
        let dir = project();
        let root = dir.path();
        assert!(matches!(check(root, &stop("s1", FAILING)), Verdict::Deny { .. }));
        let explains = "O CI (integração contínua) roda os testes.";
        assert_eq!(check(root, &complement("s1", explains)), Verdict::Allow);
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow, "CI was explained in the complement");
    }

    /// O `Stop` como o Claude Code o manda ao `mustard-rt on Stop`, com todos
    /// os campos, pelo despachante inteiro, e a resposta JSON que volta a ele;
    /// `Value::Null` quando nada barra.
    fn stop_event(root: &Path, session: &str, message: &str) -> serde_json::Value {
        let payload = json!({
            "session_id": session,
            "transcript_path": root.join(format!("{session}.jsonl")),
            "cwd": root,
            "permission_mode": "default",
            "hook_event_name": "Stop",
            "stop_hook_active": false,
            "last_assistant_message": message,
        });
        let input: HookInput = serde_json::from_value(payload).expect("a Stop payload");
        let outcome = crate::dispatch::run_event(Trigger::from_event_name("Stop"), &input);
        crate::hook_output::hook_specific_output("Stop", &outcome)
            .map(|out| serde_json::from_str(&out).expect("valid JSON"))
            .unwrap_or(serde_json::Value::Null)
    }

    /// As siglas do dia a dia listadas no `mustard.json` passam sem o nome
    /// por extenso, e a sigla fora da lista continua cobrada, sozinha. Sem a
    /// lista, a mesma resposta cobra as duas do projeto. A lista não entra na
    /// memória da sessão. Tudo pelo `Stop` de verdade.
    #[test]
    fn the_project_acronyms_pass_and_others_are_still_charged() {
        let listed = project_with(r#"{"language":{"text":"pt-BR"},"acronyms":["PI","PCP"]}"#);
        let root = listed.path();
        assert_eq!(stop_event(root, "s1", "A PI da fábrica mudou, e o PCP já sabe."), serde_json::Value::Null);
        assert!(!std::fs::read_to_string(record_file(root, "s1")).unwrap_or_default().contains("PI"));

        let blocked = stop_event(root, "s2", "A PI da fábrica mudou, e o PCP e o MRP já sabem.");
        assert_eq!(blocked["decision"], json!("block"), "{blocked}");
        assert_eq!(
            blocked["reason"],
            json!(
                "Mustard: complemento abaixo. Sem reescrever a resposta, escreva logo abaixo dela \
                 um complemento curto sobre estes pontos:\n\
                 - MRP é uma sigla sem explicação; diga o nome por extenso"
            )
        );

        let unlisted = project();
        let blocked = stop_event(unlisted.path(), "s1", "A PI da fábrica mudou, e o PCP já sabe.");
        let reason = blocked["reason"].as_str().unwrap_or_else(|| panic!("{blocked}"));
        assert!(
            reason.contains("\n- PI é uma sigla sem explicação") && reason.contains("\n- PCP é uma sigla sem explicação"),
            "{reason}"
        );
    }

    /// Um registro antigo da sessão, que também guardava termos na lista do
    /// que já foi explicado, continua lido: a sigla que ele traz não volta a
    /// ser cobrada, e os termos antigos continuam guardados.
    #[test]
    fn an_old_session_record_with_explained_terms_still_reads() {
        let dir = project();
        let root = dir.path();
        let path = record_file(root, "s1");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"explained":["slug","CI"]}"#).unwrap();
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        let kept: ClarityRecord = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(kept.explained, ["slug", "CI"].map(String::from));
    }
}
