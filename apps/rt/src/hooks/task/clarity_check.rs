//! `clarity_check` — a regra de clareza do fim da resposta ([`ClarityRule`],
//! uma das regras do `end_of_turn_check`): mede o texto final do assistente
//! contra a regra de escrita.
//!
//! ## Por que existe
//!
//! O assistente recebe a regra de escrita em toda mensagem
//! (`prompt_submit_inject::writing_rule`). Nada conferia se ela foi cumprida:
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
//! sigla sem as palavras por extenso, nome inventado sem tradução, código
//! do Mustard ("MSTD-RULE-0005"), tamanho, a nota de Flesch em português e o idioma. Os nomes
//! inventados vêm da semente do output style `mustard-didactic`, nunca de uma
//! lista escrita aqui.
//!
//! - Na primeira resposta que reprova, bloqueia: o assistente recebe os
//!   defeitos e reescreve. A resposta já apareceu na tela, mas o bloqueio é o
//!   único caminho que chega ao assistente no mesmo turno.
//! - Na reescrita (`stop_hook_active`), só avisa o usuário: bloquear de novo
//!   prenderia o turno num laço.
//! - Guarda em `.claude/.session/<sid>/clarity.json` as siglas e os termos já
//!   explicados na sessão, para a próxima medição não cobrar de novo. É a
//!   única coisa que a medição grava: nenhum arquivo de eventos.
//!
//! O bloqueio e o aviso listam no máximo [`MAX_LISTED_DEFECTS`] defeitos, cada
//! um cortado em [`MAX_DEFECT_CHARS`] caracteres; o resto vira uma contagem.
//! Falha de disco só cala o registro; o bloqueio ainda sai.

use std::path::{Path, PathBuf};

use mustard_core::domain::clarity::measure;
use mustard_core::io::fs;
use mustard_core::platform::i18n::Locale;
use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};

use crate::hooks::task::end_of_turn_check::{Finding, Turn, TurnRule};

/// O arquivo da sessão onde a medição guarda o que precisa lembrar.
const RECORD_FILE: &str = "clarity.json";

/// O output style que o assistente recebe no prompt de sistema. É dele que sai
/// a semente dos nomes inventados, para a lista viver num lugar só.
const OUTPUT_STYLE: &str = include_str!("../../../../../plugin/output-styles/mustard-didactic.md");

/// O começo do parágrafo do output style que lista, entre crases, os primeiros
/// nomes inventados do projeto.
const SEED_PARAGRAPH: &str = "**A name this project invented";

/// O que a sessão lembra entre uma resposta e a próxima.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ClarityRecord {
    /// Siglas e termos que alguma resposta desta sessão já explicou.
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
        // O idioma que a prosa precisa ter é só o DECLARADO em `language.text`.
        // O padrão resolvido é pt-BR, e um projeto em inglês que nunca declarou
        // idioma teria toda resposta apontada. Os defeitos saem no idioma
        // resolvido (`turn.lang`).
        let expected = mustard_core::ProjectConfig::load(root).language().text;
        let defects = writing_defects(root, turn, expected);
        if defects.is_empty() {
            return None;
        }
        Some(if turn.retry {
            Finding::Warn(with_head("clarity.note.head", &defects, turn.lang))
        } else {
            Finding::Block(with_head("clarity.block.head", &defects, turn.lang))
        })
    }
}

/// Todas as medições da escrita, com a memória da sessão: o que esta resposta
/// explicou fica guardado. Sem idioma declarado (`expected` vazio), a medição
/// do idioma não dá veredito.
fn writing_defects(root: &Path, turn: &Turn<'_>, expected: Option<Locale>) -> Vec<String> {
    let record_path = record_path(root, turn.session);
    let mut record = record_path.as_deref().map(read_record).unwrap_or_default();
    let report = measure(turn.message, &style_seed(), &record.explained, expected);
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

/// Quantos defeitos o bloqueio e o aviso listam; o resto vira uma contagem.
const MAX_LISTED_DEFECTS: usize = 5;

/// O tamanho máximo de uma linha de defeito, em caracteres.
const MAX_DEFECT_CHARS: usize = 160;

/// O cabeçalho do catálogo seguido de um defeito por linha — no máximo
/// [`MAX_LISTED_DEFECTS`], cada um com até [`MAX_DEFECT_CHARS`] caracteres.
fn with_head(key: &str, defects: &[String], lang: Locale) -> String {
    let mut text = mustard_core::translate(key, lang).to_string();
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

/// Os nomes entre crases do parágrafo dos nomes inventados do output style
/// (`gate`, `wave`, `slug`…).
fn style_seed() -> Vec<String> {
    let Some(paragraph) = OUTPUT_STYLE.lines().find(|line| line.starts_with(SEED_PARAGRAPH)) else {
        return Vec::new();
    };
    let mut terms: Vec<String> = Vec::new();
    for term in paragraph.split('`').skip(1).step_by(2).flat_map(|quoted| quoted.split('/')) {
        let term = term.trim();
        if !term.is_empty() && !terms.iter().any(|known| known == term) {
            terms.push(term.to_string());
        }
    }
    terms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::session::prompt_submit_inject::PromptSubmitInject;
    use crate::hooks::task::end_of_turn_check::run_rules;
    use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
    use serde_json::json;
    use tempfile::tempdir;

    /// Reprova por uma sigla sem as palavras por extenso.
    const FAILING: &str = "O CI falhou de novo.";
    /// Passa: frase curta, sem sigla nem nome inventado.
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

    /// A reescrita que um bloqueio pediu: o mesmo `Stop` com `stop_hook_active`.
    fn rewrite(session: &str, message: &str) -> HookInput {
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

    /// A resposta que reprova é barrada, e o assistente recebe os defeitos no
    /// próprio bloqueio; a reescrita que ainda reprova só avisa o usuário. Os
    /// defeitos não andam mais na mensagem seguinte.
    #[test]
    fn a_failing_reply_blocks_and_its_rewrite_only_warns() {
        let dir = project();
        let root = dir.path();
        match check(root, &stop("s1", FAILING)) {
            Verdict::Deny { reason } => {
                assert!(reason.starts_with("[Mustard] A resposta fugiu da regra de escrita."), "{reason}");
                assert!(reason.contains("\n- CI sem as palavras por extenso"), "{reason}");
            }
            other => panic!("a failing reply blocks, got {other:?}"),
        }
        match check(root, &rewrite("s1", FAILING)) {
            Verdict::Inject { context } => {
                assert!(context.starts_with("Mustard · clareza: a resposta acima ainda foge"), "{context}");
                assert!(context.contains("\n- CI sem as palavras por extenso"), "{context}");
            }
            other => panic!("the rewrite only warns, got {other:?}"),
        }

        let next = match PromptSubmitInject.evaluate(
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
        assert_eq!(check(dir.path(), &rewrite("s1", CLEAR)), Verdict::Allow);
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
    /// aviso na mesma sessão, a única coisa debaixo de `.claude/` é a memória
    /// da sessão. Uma pasta de eventos que já existia fica com os mesmos bytes,
    /// e nada novo nasce nela.
    #[test]
    fn the_clarity_check_writes_no_event_file() {
        let dir = project();
        let root = dir.path();
        assert!(matches!(check(root, &stop("s1", FAILING)), Verdict::Deny { .. }));
        assert!(matches!(check(root, &rewrite("s1", FAILING)), Verdict::Inject { .. }));
        assert_eq!(files_under(&root.join(".claude")), vec![record_file(root, "s1")]);

        let dir = project();
        let root = dir.path();
        let old = root.join(".claude/.events/antigo.ndjson");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        let bytes = b"{\"event\":\"uma linha que ja estava aqui\"}\n";
        std::fs::write(&old, bytes).unwrap();
        assert!(matches!(check(root, &stop("s1", FAILING)), Verdict::Deny { .. }));
        assert!(matches!(check(root, &rewrite("s1", FAILING)), Verdict::Inject { .. }));
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

    /// O bloqueio e o aviso mostram no máximo [`MAX_LISTED_DEFECTS`] defeitos e
    /// ficam abaixo de 1.500 caracteres, qualquer que seja a resposta.
    #[test]
    fn clarity_defects_are_capped() {
        let dir = project();
        let root = dir.path();
        // Sessenta frases longas de palavras compridas: cada defeito passa do
        // corte de [`MAX_DEFECT_CHARS`], e os que sobram viram uma contagem.
        let long = vec!["palavraextraordinariamentecomprida"; 40].join(" ");
        let reply = format!("{long}.\n").repeat(60);
        let Verdict::Deny { reason } = check(root, &stop("s1", &reply)) else {
            panic!("a failing reply blocks");
        };
        let Verdict::Inject { context: note } = check(root, &rewrite("s1", &reply)) else {
            panic!("a failing rewrite warns");
        };
        for text in [reason.as_str(), note.as_str()] {
            let listed: Vec<&str> = text.lines().skip(1).collect();
            let (count, defects) = listed.split_last().unwrap_or_else(|| panic!("{text}"));
            assert!(count.starts_with("- e mais "), "the rest becomes a count: {text}");
            assert_eq!(defects.len(), MAX_LISTED_DEFECTS, "{text}");
            for defect in defects {
                assert_eq!(defect.chars().count(), "- ".len() + MAX_DEFECT_CHARS, "cut: {defect}");
            }
            assert!(text.chars().count() < 1_500, "{} chars: {text}", text.chars().count());
        }
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

    /// Os nomes inventados saem só da semente do output style: um termo da
    /// semente sem tradução reprova, e um termo do glossário do projeto não é
    /// cobrado.
    #[test]
    fn invented_names_come_from_the_output_style() {
        assert_eq!(
            style_seed(),
            ["gate", "wave", "slug", "upsert", "strict", "warn", "boundary"].map(String::from)
        );
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join("CONTEXT.md"), "# Glossário\n\n**Unidade**: um trabalho aberto.\n").unwrap();
        let Verdict::Deny { reason } = check(root, &stop("s1", "Troquei o slug da spec.")) else {
            panic!("a seed term without translation blocks");
        };
        assert!(reason.contains("- slug usado sem tradução"), "{reason}");
        assert_eq!(check(root, &stop("s2", "A Unidade ficou aberta.")), Verdict::Allow);
    }
}
