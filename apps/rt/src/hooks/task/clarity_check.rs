//! `clarity_check` — a regra de clareza do fim da resposta ([`ClarityRule`],
//! uma das regras do `end_of_turn_check`): mede o texto final do assistente
//! contra a regra de escrita, e o erro que ela acha vai junto da mensagem
//! seguinte do usuário.
//!
//! ## Por que existe
//!
//! O assistente recebe a regra de escrita no estilo de resposta, e toda
//! mensagem leva uma linha curta que a lembra (`prompt_entry`). Nada conferia
//! se ela foi cumprida: em 09/09/2026 o usuário reclamou duas vezes de
//! respostas difíceis de entender, com a regra ativa, e nenhum gancho
//! percebeu.
//!
//! ## Quando mede
//!
//! O `end_of_turn_check` só chama as regras no `Stop` da sessão principal.
//! Aqui, três fatos a mais:
//!
//! 1. O projeto tem `mustard.json`. Nele todas as medições da escrita rodam
//!    sempre: o tom é um só, didático, e não há chave que o desligue. O idioma
//!    da resposta só é julgado quando o projeto DECLAROU o idioma do texto
//!    (`language.text`); sem ele não há veredito de idioma, porque o padrão
//!    resolvido não é uma escolha.
//! 2. O `Stop` trouxe texto final.
//! 3. O `Stop` trouxe uma sessão que se usa: é na pasta dela que o erro
//!    espera a mensagem seguinte.
//!
//! ## O que faz
//!
//! Mede o texto com o medidor do núcleo (`domain::clarity`): frase longa,
//! sigla sem as palavras por extenso, código do Mustard ("MSTD-RULE-0005"),
//! tamanho, a nota de Flesch em português e o idioma.
//!
//! - Nunca barra a resposta, e nada aparece na tela. A barragem aparecia duas
//!   vezes no terminal e custava outra rodada; na Suzano foram 22.
//! - Guarda em `.claude/.session/<sid>/clarity.json` o erro de cada defeito,
//!   como "frase com 29 palavras": a linha do defeito até os dois-pontos ou o
//!   ponto e vírgula, sem o jeito de consertar, que o estilo de resposta já
//!   diz ([`error_of`]). A mensagem seguinte do usuário leva os erros numa
//!   frase curta, junto da linha escondida, e os apaga ([`take_next_note`]):
//!   o assistente corrige na resposta seguinte. Uma resposta sem erro apaga o
//!   que estava guardado, porque a frase fala só da última resposta.
//! - A volta que o bloqueio das pendências pede chega com
//!   `stop_hook_active`, no mesmo turno: os erros dela somam aos da resposta
//!   barrada, porque o usuário leu as duas.
//! - Guarda também as siglas já explicadas na sessão, para a próxima medição
//!   não cobrar de novo. É tudo o que a medição grava: nenhum arquivo de
//!   eventos.
//! - As siglas do dia a dia do projeto, listadas em `acronyms` no
//!   `mustard.json`, vão ao medidor junto das já explicadas na sessão: passam
//!   sem o nome por extenso, e a sigla fora da lista continua cobrada.
//!
//! A frase lista no máximo [`MAX_LISTED_ERRORS`] erros, sem repetir nenhum; o
//! resto vira uma contagem. Falha de disco só perde o que a sessão lembra.
//!
//! A gravação de lição (`run write lesson`) mede o texto da lição pela mesma
//! medição ([`measure_in_project`]), com o que o projeto declara.

use std::path::{Path, PathBuf};

use mustard_core::domain::clarity::{measure, ClarityReport};
use mustard_core::io::fs;
use mustard_core::platform::i18n::Locale;
use mustard_core::{ClaudePaths, ProjectConfig};
use serde::{Deserialize, Serialize};

use crate::hooks::task::end_of_turn_check::{Finding, Turn, TurnRule};

/// O arquivo da sessão onde a medição guarda o que precisa lembrar.
const RECORD_FILE: &str = "clarity.json";

/// Quantos erros a frase da mensagem seguinte lista; o resto vira uma
/// contagem.
const MAX_LISTED_ERRORS: usize = 5;

/// O que a sessão lembra entre uma resposta e a próxima.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ClarityRecord {
    /// As siglas que alguma resposta desta sessão já explicou. Um registro
    /// antigo também traz termos aqui; eles só ficam guardados.
    #[serde(default)]
    explained: Vec<String>,
    /// Os erros da última resposta, que a mensagem seguinte leva. Vazio quando
    /// ela passou ou quando uma mensagem já os levou.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
}

/// A regra de clareza do fim da resposta.
pub struct ClarityRule;

impl TurnRule for ClarityRule {
    fn check(&self, turn: &Turn<'_>) -> Option<Finding> {
        let root = Path::new(turn.project_dir);
        if !ProjectConfig::exists(root) || turn.message.trim().is_empty() {
            return None;
        }
        if let Some(path) = record_path(root, turn.session) {
            keep_errors(root, &path, turn);
        }
        // A escrita nunca barra: o erro espera a mensagem seguinte.
        None
    }
}

/// Mede a resposta com a memória da sessão e guarda nela, em `path`, as siglas
/// que a resposta explicou e os erros que ela teve.
fn keep_errors(root: &Path, path: &Path, turn: &Turn<'_>) {
    let mut record = read_record(path);
    let report = measure_in_project(root, turn.message, &record.explained);
    for term in &report.explained {
        if !record.explained.contains(term) {
            record.explained.push(term.clone());
        }
    }
    // A volta que um bloqueio pediu é o mesmo turno, e soma; uma resposta nova
    // troca o que estava guardado.
    if !turn.retry {
        record.errors.clear();
    }
    for error in report.defects(turn.lang).iter().map(|defect| error_of(defect)) {
        if !record.errors.contains(&error) {
            record.errors.push(error);
        }
    }
    write_record(path, &record);
}

/// O erro de uma linha de defeito: o trecho antes dos primeiros dois-pontos ou
/// do primeiro ponto e vírgula. "frase com 29 palavras: \"…\"; diga…" vira
/// "frase com 29 palavras". Cada linha do catálogo abre com o erro e deixa o
/// jeito de consertar para depois dessa pontuação.
fn error_of(defect: &str) -> String {
    defect.split([':', ';']).next().unwrap_or(defect).trim().to_string()
}

/// A frase curta com os erros da última resposta da sessão `session`, no
/// idioma do projeto em `root`, para a linha escondida da mensagem seguinte.
/// `None` quando a resposta não teve erro, quando outra mensagem já levou a
/// frase ou sem sessão que se use. Levar apaga os erros: a frase vai uma vez
/// só.
pub(crate) fn take_next_note(root: &Path, session: Option<&str>) -> Option<String> {
    let path = record_path(root, session)?;
    let mut record = read_record(&path);
    if record.errors.is_empty() {
        return None;
    }
    let lang = ProjectConfig::load(root).language().text_or_default();
    let note = next_note(&record.errors, lang);
    record.errors.clear();
    write_record(&path, &record);
    Some(note)
}

/// A frase do catálogo com até [`MAX_LISTED_ERRORS`] erros, separados por
/// ponto e vírgula, e a contagem do resto.
fn next_note(errors: &[String], lang: Locale) -> String {
    let mut listed: Vec<String> = errors.iter().take(MAX_LISTED_ERRORS).cloned().collect();
    let rest = errors.len().saturating_sub(MAX_LISTED_ERRORS);
    if rest > 0 {
        listed.push(mustard_core::translate("clarity.more", lang).replace("{count}", &rest.to_string()));
    }
    mustard_core::translate("clarity.next.head", lang).replace("{errors}", &listed.join("; "))
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
    let config = ProjectConfig::load(root);
    let known: Vec<String> = explained.iter().cloned().chain(config.acronyms()).collect();
    measure(text, &known, config.language().text)
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
/// registro de antes de 12/09, com a lista de defeitos que a mensagem seguinte
/// levava naquela época (`defects`), ainda se lê: o campo que sobra é
/// ignorado.
fn read_record(path: &Path) -> ClarityRecord {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Grava o registro. Falha de disco só perde a memória da sessão: a próxima
/// medição cobra de novo um termo já explicado, e a frase de um erro pode ir
/// de novo ou não ir.
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
    use crate::hooks::task::end_of_turn_check::run_rules;
    use mustard_core::domain::clarity::{LongSentence, WrongLanguage};
    use mustard_core::domain::model::contract::{Ctx, HookInput, Trigger, Verdict};
    use serde_json::{json, Value};
    use tempfile::tempdir;

    /// Reprova por uma sigla sem as palavras por extenso.
    const FAILING: &str = "O CI falhou de novo.";
    /// O erro de [`FAILING`].
    const FAILING_ERROR: &str = "CI é uma sigla sem explicação";
    /// Passa: frase curta e sem sigla.
    const CLEAR: &str = "A resposta ficou curta e clara.";

    /// A linha escondida de um projeto que declarou pt-BR, a mesma de antes.
    const PT_LINE: &str =
        "Responda em português do Brasil, em texto simples: frases curtas e nenhum código interno.";
    /// A linha escondida de um projeto que declarou en-US.
    const EN_LINE: &str = "Answer in US English, in plain text: short sentences and no internal codes.";

    /// Uma frase de 25 palavras, o limite: o último tamanho que passa.
    const TWENTY_FIVE_WORDS: &str = "Eu li os arquivos do projeto e conferi cada teste que ainda falhava \
        na máquina do usuário antes de ajustar a leitura do idioma hoje.";
    /// A mesma frase com uma palavra a mais: o primeiro tamanho que já não passa.
    const TWENTY_SIX_WORDS: &str = "Eu li os arquivos do projeto e conferi cada teste que ainda falhava \
        na máquina do usuário antes de ajustar a leitura do idioma hoje cedo.";
    /// Uma frase de 26 palavras em inglês.
    const TWENTY_SIX_WORDS_EN: &str = "I read the files of the project and checked each test that still \
        failed on the machine of the user before I fixed the language today.";

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

    fn record_file(root: &Path, session: &str) -> PathBuf {
        root.join(".claude/.session").join(session).join(RECORD_FILE)
    }

    /// Os erros que a sessão guarda para a mensagem seguinte.
    fn kept_errors(root: &Path, session: &str) -> Vec<String> {
        read_record(&record_file(root, session)).errors
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

    /// O `Stop` como o Claude Code o manda ao `mustard-rt on Stop`, com todos
    /// os campos, pelo despachante inteiro, e a resposta JSON que volta a ele;
    /// `Value::Null` quando nada volta: nem bloqueio, nem aviso na tela.
    fn stop_event(root: &Path, session: &str, message: &str) -> Value {
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
            .unwrap_or(Value::Null)
    }

    /// A mensagem do usuário como o Claude Code a manda ao `mustard-rt on
    /// UserPromptSubmit`, com todos os campos, pelo despachante inteiro. Devolve
    /// o texto escondido que vai junto dela ao assistente.
    fn next_line(root: &Path, session: &str, prompt: &str) -> String {
        let payload = json!({
            "session_id": session,
            "transcript_path": root.join(format!("{session}.jsonl")),
            "cwd": root,
            "permission_mode": "default",
            "hook_event_name": "UserPromptSubmit",
            "prompt": prompt,
        });
        let input: HookInput = serde_json::from_value(payload).expect("a UserPromptSubmit payload");
        let outcome = crate::dispatch::run_event(Trigger::from_event_name("UserPromptSubmit"), &input);
        let out: Value = crate::hook_output::hook_specific_output("UserPromptSubmit", &outcome)
            .map(|out| serde_json::from_str(&out).expect("valid JSON"))
            .unwrap_or(Value::Null);
        assert_ne!(out["decision"], json!("block"), "a message is never blocked here: {out}");
        out["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_default().to_string()
    }

    /// Uma frase de 25 palavras, o limite, não tem erro: nada barra, e a linha
    /// escondida da mensagem seguinte fica igual à de antes. Com 26 palavras,
    /// a resposta também não é barrada e nada volta para a tela; a linha da
    /// mensagem seguinte leva mais uma frase curta com o erro, uma vez só. Nos
    /// dois idiomas, pelos ganchos de verdade.
    #[test]
    fn a_long_sentence_goes_with_the_next_message_instead_of_blocking() {
        assert_eq!(TWENTY_FIVE_WORDS.split_whitespace().count(), 25);
        assert_eq!(TWENTY_SIX_WORDS.split_whitespace().count(), 26);
        assert_eq!(TWENTY_SIX_WORDS_EN.split_whitespace().count(), 26);
        let dir = project();
        let root = dir.path();

        assert_eq!(stop_event(root, "s1", TWENTY_FIVE_WORDS), Value::Null, "25 words pass");
        assert_eq!(next_line(root, "s1", "e agora?"), PT_LINE, "no error, the line is the same");

        assert_eq!(stop_event(root, "s1", TWENTY_SIX_WORDS), Value::Null, "26 words are not blocked");
        assert_eq!(
            next_line(root, "s1", "e agora?"),
            format!("{PT_LINE} Na última resposta: frase com 26 palavras.")
        );
        assert_eq!(next_line(root, "s1", "e depois?"), PT_LINE, "the phrase goes once");

        let en = project_with(r#"{"language":{"text":"en-US"}}"#);
        assert_eq!(stop_event(en.path(), "s1", TWENTY_SIX_WORDS_EN), Value::Null);
        assert_eq!(
            next_line(en.path(), "s1", "and now?"),
            format!("{EN_LINE} In the last reply: sentence with 26 words.")
        );
    }

    /// A resposta que passa não guarda erro, e apaga o da resposta anterior
    /// que nenhuma mensagem levou: a frase fala só da última resposta.
    #[test]
    fn a_clear_reply_keeps_no_error_and_drops_the_old_one() {
        let dir = project();
        let root = dir.path();
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        assert_eq!(kept_errors(root, "s1"), [FAILING_ERROR]);
        assert_eq!(check(root, &stop("s1", CLEAR)), Verdict::Allow);
        assert_eq!(kept_errors(root, "s1"), Vec::<String>::new());
        assert_eq!(next_line(root, "s1", "e agora?"), PT_LINE);
    }

    /// Uma resposta em inglês num projeto em português não é barrada; o erro
    /// de idioma fica guardado para a mensagem seguinte.
    #[test]
    fn a_reply_in_another_language_is_kept_for_the_next_message() {
        let dir = project();
        let reply = "The wave is done and the tests pass.\n\
            The check now compares the language of the reply with the language of the project.\n\
            It counts the common words of each language.\n\
            A short reply is not judged at all.";
        assert_eq!(check(dir.path(), &stop("s1", reply)), Verdict::Allow);
        assert_eq!(kept_errors(dir.path(), "s1"), ["resposta em en-US"]);
    }

    /// A escrita é medida em todo projeto com `mustard.json`, declare ou não o
    /// idioma, e a antiga chave do tom não desliga nada: ela não é mais lida.
    /// Sem `mustard.json` nada é medido. Um subagente e outro evento nunca
    /// são medidos. Nada disso barra.
    #[test]
    fn writing_is_measured_in_every_project() {
        for config in [r#"{"language":{"text":"pt-BR"}}"#, "{}", r#"{"tone":"technical"}"#] {
            let dir = project_with(config);
            let root = dir.path();
            assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow, "{config}");
            assert_eq!(kept_errors(root, "s1"), [FAILING_ERROR], "{config}");
            assert!(event_rows(root).is_empty(), "{config}: no event line: {:?}", event_rows(root));
        }
        let bare = tempdir().unwrap();
        assert_eq!(check(bare.path(), &stop("s1", FAILING)), Verdict::Allow);
        assert!(!bare.path().join(".claude").exists(), "no mustard.json, no measurement");

        let dir = project();
        let mut sub = stop("s1", FAILING);
        sub.agent_id = Some("child".to_string());
        assert_eq!(check(dir.path(), &sub), Verdict::Allow);
        let pre = ctx(dir.path(), Trigger::PreToolUse);
        assert_eq!(run_rules(&[&ClarityRule], &stop("s1", FAILING), &pre), Verdict::Allow);
        assert!(!dir.path().join(".claude").exists(), "a subagent and another event are not measured");
    }

    /// A medição não grava arquivo de eventos: depois de uma resposta e de uma
    /// volta na mesma sessão, a única coisa debaixo de `.claude/` é a memória
    /// da sessão. Uma pasta de eventos que já existia fica com os mesmos
    /// bytes, e nada novo nasce nela.
    #[test]
    fn the_clarity_check_writes_no_event_file() {
        let dir = project();
        let root = dir.path();
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        assert_eq!(check(root, &retry("s1", FAILING)), Verdict::Allow);
        assert_eq!(files_under(&root.join(".claude")), vec![record_file(root, "s1")]);

        let dir = project();
        let root = dir.path();
        let old = root.join(".claude/.events/antigo.ndjson");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        let bytes = b"{\"event\":\"uma linha que ja estava aqui\"}\n";
        std::fs::write(&old, bytes).unwrap();
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        assert_eq!(check(root, &retry("s1", FAILING)), Verdict::Allow);
        assert_eq!(files_under(&root.join(".claude")), vec![old.clone(), record_file(root, "s1")]);
        assert_eq!(std::fs::read(&old).unwrap(), bytes, "the old event file is untouched");
    }

    /// Sem sessão, ou com a sessão `unknown`, a medição não grava nada: nenhuma
    /// pasta nasce debaixo do projeto, e a resposta não é barrada.
    #[test]
    fn without_a_session_the_clarity_check_writes_nothing() {
        let mut no_session = stop("s1", FAILING);
        no_session.session_id = None;
        for input in [no_session, stop("unknown", FAILING)] {
            let dir = project();
            let root = dir.path();
            assert_eq!(check(root, &input), Verdict::Allow);
            assert!(!root.join(".claude").exists(), "{:?}", files_under(&root.join(".claude")));
        }
    }

    /// A frase da mensagem seguinte lista no máximo [`MAX_LISTED_ERRORS`]
    /// erros, numa linha só, e o resto vira uma contagem, qualquer que seja a
    /// resposta. Sessenta frases longas, cada uma de um tamanho, dão sessenta
    /// erros, e as sessenta linhas dão mais um.
    #[test]
    fn the_next_note_lists_at_most_five_errors() {
        let dir = project();
        let root = dir.path();
        let reply = (26..86).map(|words| vec!["palavra"; words].join(" ")).collect::<Vec<_>>().join(".\n");
        assert_eq!(check(root, &stop("s1", &reply)), Verdict::Allow);
        assert_eq!(kept_errors(root, "s1").len(), 61);
        assert_eq!(
            next_line(root, "s1", "e agora?"),
            format!(
                "{PT_LINE} Na última resposta: frase com 26 palavras; frase com 27 palavras; \
                 frase com 28 palavras; frase com 29 palavras; frase com 30 palavras; e mais 56."
            )
        );
    }

    /// A sigla explicada numa resposta não é cobrada na seguinte: o registro
    /// da sessão guarda o que já foi explicado.
    #[test]
    fn explained_acronym_carries_across_replies() {
        let dir = project();
        let root = dir.path();
        assert_eq!(check(root, &stop("s1", "O CI (integração contínua) falhou.")), Verdict::Allow);
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        assert_eq!(kept_errors(root, "s1"), Vec::<String>::new());
        // Outra sessão começa do zero.
        assert_eq!(check(root, &stop("s2", FAILING)), Verdict::Allow);
        assert_eq!(kept_errors(root, "s2"), [FAILING_ERROR]);
    }

    /// As palavras que o estilo de resposta antigo listava como nomes
    /// inventados não são mais cobradas: "slug" e "gate" passam, mesmo num
    /// projeto com glossário.
    #[test]
    fn a_reply_that_says_slug_or_gate_keeps_no_error() {
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join("CONTEXT.md"), "# Glossário\n\n**Slug**: o nome curto da spec.\n").unwrap();
        for reply in ["Troquei o slug da spec.", "O gate da onda passou."] {
            assert_eq!(check(root, &stop("s1", reply)), Verdict::Allow, "{reply}");
            assert_eq!(kept_errors(root, "s1"), Vec::<String>::new(), "{reply}");
        }
    }

    /// Uma palavra da lista antiga escrita em maiúsculas é medida como
    /// qualquer sigla: sem as palavras por extenso, vira erro.
    #[test]
    fn an_uppercase_word_of_the_old_seed_is_measured_as_any_acronym() {
        let dir = project();
        assert_eq!(check(dir.path(), &stop("s1", "O GATE falhou.")), Verdict::Allow);
        assert_eq!(kept_errors(dir.path(), "s1"), ["GATE é uma sigla sem explicação"]);
    }

    /// A volta que um bloqueio pediu é o mesmo turno: os erros dela somam aos
    /// da resposta barrada, sem repetir, e a sigla que ela explica fica na
    /// memória da sessão. A resposta nova seguinte troca o que estava
    /// guardado.
    #[test]
    fn a_retry_adds_its_errors_and_what_it_explains_is_remembered() {
        let dir = project();
        let root = dir.path();
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        assert_eq!(check(root, &retry("s1", "O MRP mudou, e o CI também.")), Verdict::Allow);
        assert_eq!(kept_errors(root, "s1"), [FAILING_ERROR, "MRP é uma sigla sem explicação"]);
        assert_eq!(check(root, &retry("s1", "O CI (integração contínua) roda os testes.")), Verdict::Allow);
        assert_eq!(kept_errors(root, "s1"), [FAILING_ERROR, "MRP é uma sigla sem explicação"]);

        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        assert_eq!(kept_errors(root, "s1"), Vec::<String>::new(), "CI was explained in the retry");
    }

    /// As siglas do dia a dia listadas no `mustard.json` passam sem o nome
    /// por extenso, e a sigla fora da lista continua cobrada, sozinha. Sem a
    /// lista, a mesma resposta cobra as duas do projeto. A lista não entra na
    /// memória da sessão. Tudo pelos ganchos de verdade, sem barrar.
    #[test]
    fn the_project_acronyms_pass_and_others_are_still_charged() {
        let listed = project_with(r#"{"language":{"text":"pt-BR"},"acronyms":["PI","PCP"]}"#);
        let root = listed.path();
        assert_eq!(stop_event(root, "s1", "A PI da fábrica mudou, e o PCP já sabe."), Value::Null);
        assert!(!std::fs::read_to_string(record_file(root, "s1")).unwrap_or_default().contains("PI"));
        assert_eq!(next_line(root, "s1", "e agora?"), PT_LINE);

        assert_eq!(stop_event(root, "s2", "A PI da fábrica mudou, e o PCP e o MRP já sabem."), Value::Null);
        assert_eq!(
            next_line(root, "s2", "e agora?"),
            format!("{PT_LINE} Na última resposta: MRP é uma sigla sem explicação.")
        );

        let unlisted = project();
        assert_eq!(stop_event(unlisted.path(), "s1", "A PI da fábrica mudou, e o PCP já sabe."), Value::Null);
        assert_eq!(
            next_line(unlisted.path(), "s1", "e agora?"),
            format!(
                "{PT_LINE} Na última resposta: PI é uma sigla sem explicação; PCP é uma sigla sem explicação."
            )
        );
    }

    /// Um registro antigo da sessão, que também guardava termos na lista do
    /// que já foi explicado, continua lido: a sigla que ele traz não volta a
    /// ser cobrada, e os termos antigos continuam guardados. A lista de
    /// defeitos de antes de 12/09 é ignorada: a mensagem seguinte não a leva.
    #[test]
    fn an_old_session_record_with_explained_terms_still_reads() {
        let dir = project();
        let root = dir.path();
        let path = record_file(root, "s1");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"explained":["slug","CI"],"defects":["um defeito antigo"]}"#).unwrap();
        assert_eq!(next_line(root, "s1", "e agora?"), PT_LINE, "the old defects are not carried");
        assert_eq!(check(root, &stop("s1", FAILING)), Verdict::Allow);
        let kept = read_record(&path);
        assert_eq!(kept.explained, ["slug", "CI"].map(String::from));
        assert_eq!(kept.errors, Vec::<String>::new());
    }

    /// Cada linha de defeito do catálogo abre com o erro, nos dois idiomas: o
    /// trecho antes da primeira pontuação que fecha o erro. As linhas vêm do
    /// próprio catálogo, lidas por `ClarityReport::defects`, não de cópias
    /// escritas à mão: se o catálogo mudar a pontuação de uma linha, o
    /// recorte muda e este teste cai. Longa e difícil de ler ao mesmo tempo
    /// vira uma linha só, a junta; cada uma sozinha continua como sempre.
    #[test]
    fn every_catalog_defect_line_yields_its_error_name() {
        let full_report = |found: Locale, expected: Locale| ClarityReport {
            long_sentences: vec![LongSentence { words: 29, opening: "Depois de ler".to_string() }],
            unexpanded_acronyms: vec!["CI".to_string()],
            internal_codes: vec!["MSTD-RULE-0008".to_string()],
            prose_lines: 16,
            lines: 16,
            too_long: true,
            reading_ease: Some(12),
            hard_to_read: true,
            wrong_language: Some(WrongLanguage { found, expected }),
            passed: false,
            explained: Vec::new(),
        };
        for (lang, report, errors) in [
            (
                Locale::PtBr,
                full_report(Locale::EnUs, Locale::PtBr),
                vec![
                    "frase com 29 palavras",
                    "CI é uma sigla sem explicação",
                    "MSTD-RULE-0008 é um código interno",
                    "resposta com 16 linhas (o limite é 15) e difícil de ler",
                    "resposta em en-US",
                ],
            ),
            (
                Locale::EnUs,
                full_report(Locale::PtBr, Locale::EnUs),
                vec![
                    "sentence with 29 words",
                    "CI is an unexplained acronym",
                    "MSTD-RULE-0008 is an internal code",
                    "reply with 16 lines (the limit is 15) and hard to read",
                    "reply in pt-BR",
                ],
            ),
        ] {
            let lines = report.defects(lang);
            assert_eq!(lines.len(), errors.len(), "{lang:?}: {lines:?}");
            for (line, error) in lines.iter().zip(&errors) {
                assert_eq!(&error_of(line), error, "{lang:?}: {line}");
            }
        }

        // Só a resposta longa, sem ser difícil de ler: a linha própria dela,
        // sem juntar com nada.
        let only_too_long = ClarityReport {
            long_sentences: Vec::new(),
            unexpanded_acronyms: Vec::new(),
            internal_codes: Vec::new(),
            prose_lines: 16,
            lines: 16,
            too_long: true,
            reading_ease: None,
            hard_to_read: false,
            wrong_language: None,
            passed: false,
            explained: Vec::new(),
        };
        let lines = only_too_long.defects(Locale::PtBr);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(error_of(&lines[0]), "resposta com 16 linhas, e o limite é 15");

        // Só difícil de ler, sem ser longa: a linha própria dela, sem juntar
        // com nada.
        let only_hard_to_read = ClarityReport {
            long_sentences: Vec::new(),
            unexpanded_acronyms: Vec::new(),
            internal_codes: Vec::new(),
            prose_lines: 8,
            lines: 8,
            too_long: false,
            reading_ease: Some(12),
            hard_to_read: true,
            wrong_language: None,
            passed: false,
            explained: Vec::new(),
        };
        let lines = only_hard_to_read.defects(Locale::PtBr);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(error_of(&lines[0]), "texto difícil de ler");

        let dir = project();
        let reply = "A regra MSTD-RULE-0008 ficou pronta.";
        assert_eq!(check(dir.path(), &stop("s1", reply)), Verdict::Allow);
        assert_eq!(kept_errors(dir.path(), "s1"), ["MSTD-RULE-0008 é um código interno"]);
    }
}
