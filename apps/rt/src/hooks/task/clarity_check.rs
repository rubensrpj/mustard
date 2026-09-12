//! `clarity_check` — no fim de cada resposta, mede o texto do assistente contra
//! a regra de tom didático que ele recebeu.
//!
//! ## Por que existe
//!
//! Com `tone: didactic`, o assistente recebe a regra de escrita em toda
//! mensagem (`prompt_submit_inject::tone_rule`). Nada conferia se ela foi
//! cumprida: em 09/09/2026 o usuário reclamou duas vezes de respostas difíceis
//! de entender, com a regra ativa, e nenhum gancho percebeu (K-1).
//!
//! ## Os fatos, todos necessários
//!
//! 1. É o `Stop` da sessão principal — nunca o de um subagente.
//! 2. O projeto tem `mustard.json`. Nele o idioma da resposta é medido sempre
//!    que o projeto DECLAROU um (`lang`/`specLang`), qualquer que seja o tom;
//!    sem idioma declarado não há veredito de idioma. As quatro medições do tom didático só rodam
//!    quando o projeto DECLAROU `tone: didactic` — o campo cru, pela mesma
//!    leitura da regra ([`declares_didactic`]); o padrão resolvido não é uma
//!    escolha.
//! 3. O `Stop` trouxe `last_assistant_message`, o texto final do turno.
//!
//! ## O que faz
//!
//! Mede o texto com o medidor do núcleo (`domain::clarity`). Os nomes
//! inventados vêm de três fontes, nunca de uma lista escrita aqui (K-4): o
//! parágrafo do output style `mustard-didactic` (a semente), as Definições da
//! spec ativa e os termos do glossário `CONTEXT.md`.
//!
//! - Guarda em `.claude/.session/<sid>/clarity.json` os termos já explicados na
//!   sessão, para a próxima medição não cobrar de novo, e os defeitos desta
//!   resposta. A mensagem seguinte do usuário os leva ao assistente e os apaga
//!   ([`take_feedback`], K-2).
//! - Registra um evento `assistant.clarity` com as contagens e o resultado —
//!   nunca o texto (K-5).
//! - Quando reprova, devolve um `Inject`, que no `Stop` vira `systemMessage`:
//!   uma nota curta ao usuário com os defeitos. O `fold` junta os `Inject` do
//!   mesmo `Stop`, então a nota e o link do documento saem juntos (K-6).
//!
//! A nota e a mensagem seguinte listam no máximo [`MAX_LISTED_DEFECTS`]
//! defeitos, cada um cortado em [`MAX_DEFECT_CHARS`] caracteres; o resto vira
//! uma contagem. A lista da mensagem seguinte divide com o injetável do irmão
//! eleito o teto de 10.000 caracteres de uma resposta de gancho, e uma resposta
//! com cem frases longas não pode tirar o roteador da janela.
//!
//! Nunca bloqueia: a resposta já apareceu na tela, e barrar para reescrever
//! deixaria o texto confuso e a reescrita juntos (K-2). Falha de disco só cala
//! o registro; a nota ainda sai.

use std::path::{Path, PathBuf};

use mustard_core::domain::clarity::{measure, measure_language, ClarityReport};
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::io::fs;
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use mustard_core::time::now_iso8601;
use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commands::economy::context_slice::parse_term_blocks;
use crate::commands::spec::spec_sections::section_block;
use crate::hooks::session::prompt_submit_inject::declares_didactic;
use crate::shared::context::current_spec;

/// O arquivo da sessão onde a medição guarda o que precisa lembrar.
const RECORD_FILE: &str = "clarity.json";

/// O evento de cada resposta medida.
const EVENT: &str = "assistant.clarity";

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
    /// Os defeitos da última resposta medida, ainda não entregues ao assistente.
    #[serde(default)]
    defects: Vec<String>,
}

/// A medição de clareza no fim de cada resposta.
pub struct ClarityCheck;

impl Check for ClarityCheck {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Fato 1 — o `Stop` da sessão principal.
        if ctx.trigger != Some(Trigger::Stop) || input.is_subagent() {
            return Ok(Verdict::Allow);
        }
        let project_dir = ctx.project_dir_or_cwd(input);
        let root = Path::new(&project_dir);

        // Fato 2 — o Mustard está instalado. O tom didático decide só quais
        // medições rodam, mais abaixo.
        if !mustard_core::ProjectConfig::exists(root) {
            return Ok(Verdict::Allow);
        }

        // Fato 3 — há texto para medir.
        let Some(message) =
            input.last_assistant_message().filter(|text| !text.trim().is_empty())
        else {
            return Ok(Verdict::Allow);
        };

        let session = input.session_id.as_deref();
        let config = mustard_core::ProjectConfig::load(root);
        // Os defeitos saem no idioma resolvido; o idioma que a prosa precisa
        // ter é só o DECLARADO. O padrão resolvido é pt-BR, e um projeto em
        // inglês que nunca declarou idioma teria toda resposta apontada.
        let lang = config.i18n().lang;
        let expected = config.declared_locale();
        if !declares_didactic(root) {
            return Ok(language_only(root, session, message, expected, lang));
        }

        let record_path = record_path(root, session);
        let mut record = record_path.as_deref().map(read_record).unwrap_or_default();
        let report = measure(message, &invented_terms(root, &project_dir), &record.explained, expected);
        let defects = report.defects(lang);

        for term in &report.explained {
            if !record.explained.contains(term) {
                record.explained.push(term.clone());
            }
        }
        // Vale a resposta mais nova: um `Stop` repetido no mesmo turno (depois
        // do bloqueio de outra trava) troca os defeitos pelos da reescrita.
        record.defects.clone_from(&defects);
        if let Some(path) = &record_path {
            write_record(path, &record);
        }
        emit_metrics(&project_dir, session, &report);

        if report.passed {
            return Ok(Verdict::Allow);
        }
        Ok(Verdict::Inject { context: with_head("clarity.note.head", &defects, lang) })
    }
}

/// Fora do tom didático só o idioma é medido. O defeito segue os mesmos
/// caminhos: a nota ao usuário e o registro que a mensagem seguinte leva ao
/// assistente. O registro só é gravado quando há defeito a guardar ou um
/// defeito antigo a apagar — a reescrita no idioma certo, no mesmo turno,
/// limpa o anterior. Nenhum evento é registrado: `assistant.clarity` traz as
/// contagens da medição didática inteira, que aqui não rodou. Sem idioma
/// declarado (`expected` vazio) não há veredito: nada é apontado.
fn language_only(
    root: &Path,
    session: Option<&str>,
    message: &str,
    expected: Option<Locale>,
    lang: Locale,
) -> Verdict {
    let defects: Vec<String> = expected
        .and_then(|expected| measure_language(message, expected))
        .map(|wrong| wrong.defect(lang))
        .into_iter()
        .collect();
    if let Some(path) = record_path(root, session) {
        let mut record = read_record(&path);
        if !defects.is_empty() || !record.defects.is_empty() {
            record.defects.clone_from(&defects);
            write_record(&path, &record);
        }
    }
    if defects.is_empty() {
        return Verdict::Allow;
    }
    Verdict::Inject { context: with_head("clarity.note.head", &defects, lang) }
}

/// Os defeitos da última resposta, prontos para a mensagem seguinte do usuário
/// levar ao assistente — e só uma vez: ler apaga os defeitos do registro (os
/// termos já explicados ficam). `None` quando a última resposta passou, quando
/// nada foi medido ou quando não há sessão.
pub(crate) fn take_feedback(project_dir: &str, session: Option<&str>) -> Option<String> {
    let root = Path::new(project_dir);
    let path = record_path(root, session)?;
    let mut record = read_record(&path);
    if record.defects.is_empty() {
        return None;
    }
    let lang = mustard_core::ProjectConfig::load(root).i18n().lang;
    let text = with_head("clarity.next.head", &record.defects, lang);
    record.defects.clear();
    write_record(&path, &record);
    Some(text)
}

/// Quantos defeitos a nota e a mensagem seguinte listam; o resto vira uma
/// contagem.
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

/// O registro da sessão; vazio quando o arquivo falta ou não se lê.
fn read_record(path: &Path) -> ClarityRecord {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Grava o registro. Falha de disco só perde a memória da sessão: a próxima
/// medição cobra de novo um termo já explicado, e nada bloqueia.
fn write_record(path: &Path, record: &ClarityRecord) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(record) {
        let _ = fs::write_atomic(path, &bytes);
    }
}

/// Registra `assistant.clarity` com as métricas da medição.
fn emit_metrics(project_dir: &str, session: Option<&str>, report: &ClarityReport) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session.unwrap_or_default().to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("clarity_check".to_string()),
            actor_type: None,
        },
        event: EVENT.to_string(),
        payload: metrics(report),
        spec: None,
    };
    let _ = crate::shared::events::route::emit(project_dir, &event);
}

/// As métricas de uma medição: contagens e resultado. Nunca o texto, nem os
/// nomes das siglas e termos — são pedaços da conversa (K-5).
fn metrics(report: &ClarityReport) -> Value {
    json!({
        "passed": report.passed,
        "long_sentences": report.long_sentences.len(),
        "unexpanded_acronyms": report.unexpanded_acronyms.len(),
        "unexplained_terms": report.unexplained_terms.len(),
        "prose_lines": report.prose_lines,
        "too_long": report.too_long,
        "wrong_language": report.wrong_language.is_some(),
    })
}

// ---------------------------------------------------------------------------
// Os nomes inventados
// ---------------------------------------------------------------------------

/// Os nomes inventados do projeto, sem repetição: a semente do output style,
/// as Definições da spec ativa e os termos do glossário `CONTEXT.md`.
fn invented_terms(root: &Path, project_dir: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    let sources = style_seed()
        .into_iter()
        .chain(spec_definitions(root, project_dir))
        .chain(glossary_terms(root));
    for term in sources {
        let term = term.trim();
        if !term.is_empty() && !terms.iter().any(|known| known == term) {
            terms.push(term.to_string());
        }
    }
    terms
}

/// Os nomes entre crases do parágrafo dos nomes inventados do output style
/// (`gate`, `wave`, `slug`…).
fn style_seed() -> Vec<String> {
    let Some(paragraph) = OUTPUT_STYLE.lines().find(|line| line.starts_with(SEED_PARAGRAPH)) else {
        return Vec::new();
    };
    paragraph
        .split('`')
        .skip(1)
        .step_by(2)
        .flat_map(|quoted| quoted.split('/'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect()
}

/// Os termos da seção de Definições da spec ativa, pelo leitor único de seções.
fn spec_definitions(root: &Path, project_dir: &str) -> Vec<String> {
    let Some(spec) = current_spec(project_dir).filter(|spec| !spec.is_empty()) else {
        return Vec::new();
    };
    let spec_md = ClaudePaths::spec_dir_or_unchecked(root, &spec).join("spec.md");
    let Ok(text) = fs::read_to_string(&spec_md) else {
        return Vec::new();
    };
    section_block(&text, "definitions")
        .map(|block| block.lines().filter_map(bold_term).collect())
        .unwrap_or_default()
}

/// O termo em negrito de um item de lista: `- **wave** — …` ou
/// `- [D-1] **wave** — …`.
fn bold_term(line: &str) -> Option<String> {
    let item = line.trim_start().strip_prefix(['-', '*'])?;
    let after = &item[item.find("**")? + 2..];
    let term = after[..after.find("**")?].trim();
    (!term.is_empty()).then(|| term.to_string())
}

/// Os termos do glossário, lidos como o `subagent_inject` e o `context-slice`
/// os leem (`CONTEXT.md` mais o que o `CONTEXT-MAP.md` apontar).
fn glossary_terms(root: &Path) -> Vec<String> {
    parse_term_blocks(&read_context_md(root))
        .iter()
        .map(|block| block.term().to_string())
        .collect()
}

/// Read the project's glossary in full — no size cap. Relevance, not size,
/// decides what is injected. CONTEXT-MAP-aware: when the project carries a
/// `CONTEXT-MAP.md`, it is resolved through the SAME map-expanding resolver the
/// slicer/coverage use (`resolve_context_files`), so the hook sees every
/// `*context.md` the map links — not just a single root `CONTEXT.md`. The
/// resolved bodies are concatenated; a project with only a root `CONTEXT.md`
/// behaves exactly as before. Empty string when nothing resolves.
pub(crate) fn read_context_md(project: &Path) -> String {
    // Resolve the root CONTEXT.md plus a CONTEXT-MAP.md (when present) — the
    // resolver dedups, expands the map, and silently skips missing files.
    let mut requested: Vec<String> = Vec::new();
    let map = project.join("CONTEXT-MAP.md");
    if map.is_file() {
        requested.push(map.to_string_lossy().into_owned());
    }
    requested.push(project.join("CONTEXT.md").to_string_lossy().into_owned());

    let bodies: Vec<String> =
        crate::commands::economy::context_slice::resolve_context_files(&requested)
            .iter()
            .filter_map(|p| fs::read_to_string(p).ok())
            .collect();
    bodies.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook_output::hook_specific_output;
    use crate::hooks::session::prompt_submit_inject::PromptSubmitInject;
    use crate::registry::Registry;
    use mustard_core::domain::model::contract::Outcome;
    use tempfile::tempdir;

    /// Reprova por uma sigla sem as palavras por extenso.
    const FAILING: &str = "O CI falhou de novo.";
    /// Passa: frase curta, sem sigla nem nome inventado.
    const CLEAR: &str = "A resposta ficou curta e clara.";

    /// Um projeto que declara `tone` (ou nenhum tom, com `None`).
    fn project(tone: Option<&str>) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let config = match tone {
            Some(tone) => format!(r#"{{"specLang":"pt-BR","tone":"{tone}"}}"#),
            None => r#"{"specLang":"pt-BR"}"#.to_string(),
        };
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

    fn prompt(session: &str, text: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some(session.to_string()),
            raw: json!({ "prompt": text }),
            ..HookInput::default()
        }
    }

    /// O contexto que a mensagem seguinte do usuário leva ao assistente.
    fn next_context(root: &Path, session: &str) -> String {
        match PromptSubmitInject.evaluate(&prompt(session, "e agora?"), &ctx(root, Trigger::UserPromptSubmit)) {
            Ok(Verdict::Inject { context }) => context,
            _ => String::new(),
        }
    }

    fn record_file(root: &Path, session: &str) -> PathBuf {
        root.join(".claude/.session").join(session).join(RECORD_FILE)
    }

    /// Cada linha de evento `assistant.clarity` gravada sob `root/.claude`.
    fn clarity_rows(root: &Path) -> Vec<String> {
        fn walk(dir: &Path, rows: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, rows);
                } else if path.extension().is_some_and(|ext| ext == "ndjson") {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    rows.extend(text.lines().filter(|l| l.contains(EVENT)).map(str::to_string));
                }
            }
        }
        let mut rows = Vec::new();
        walk(&root.join(".claude"), &mut rows);
        rows
    }

    /// O objeto das métricas dentro de uma linha de evento, qualquer que seja o
    /// envelope do gravador.
    fn find_metrics(value: &Value) -> Option<&Value> {
        match value {
            Value::Object(map) if map.contains_key("passed") && map.contains_key("prose_lines") => {
                Some(value)
            }
            Value::Object(map) => map.values().find_map(find_metrics),
            Value::Array(items) => items.iter().find_map(find_metrics),
            _ => None,
        }
    }

    /// Uma unidade aberta que o `current_spec` acha: a do ambiente, quando o
    /// shell exporta uma, ou `demo` pelo arquivo de estado.
    fn open_unit(root: &Path, spec_body: &str) {
        let from_env = std::env::var("MUSTARD_ACTIVE_SPEC").ok().filter(|s| !s.is_empty());
        let spec = from_env.clone().unwrap_or_else(|| "demo".to_string());
        if from_env.is_none() {
            let paths = ClaudePaths::for_project(root).unwrap();
            std::fs::create_dir_all(paths.pipeline_states_dir()).unwrap();
            std::fs::write(paths.pipeline_state_file(&spec), "{}").unwrap();
        }
        let dir = root.join(".claude/spec").join(&spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","lang":"pt-BR"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("spec.md"), spec_body).unwrap();
    }

    /// AC-5 — a resposta que reprova volta ao assistente na mensagem seguinte,
    /// logo depois da regra de tom, e só uma vez; outra sessão nunca a recebe.
    #[test]
    fn failed_reply_defects_reach_next_prompt() {
        let dir = project(Some("didactic"));
        let root = dir.path();
        let verdict = ClarityCheck.evaluate(&stop("s1", FAILING), &ctx(root, Trigger::Stop)).unwrap();
        assert!(matches!(verdict, Verdict::Inject { .. }), "{verdict:?}");

        assert!(!next_context(root, "s2").contains("reprovou"), "another session hears nothing");

        let first = next_context(root, "s1");
        assert!(first.contains("A sua resposta anterior reprovou"), "{first}");
        assert!(first.contains("- CI sem as palavras por extenso"), "{first}");
        let rule = first.find("ONE idea per sentence").expect("the writing rule rides along");
        let defects = first.find("reprovou").expect("the defects ride along");
        assert!(rule < defects, "the defects follow the rule: {first}");

        let second = next_context(root, "s1");
        assert!(second.contains("ONE idea per sentence"), "the rule keeps riding: {second}");
        assert!(!second.contains("reprovou"), "delivered once: {second}");
    }

    /// AC-6 — a resposta que passa não mostra nota nem injeta nada; e a
    /// reescrita limpa, no mesmo turno, apaga os defeitos da anterior.
    #[test]
    fn clear_reply_injects_nothing() {
        let dir = project(Some("didactic"));
        let root = dir.path();
        let verdict = ClarityCheck.evaluate(&stop("s1", CLEAR), &ctx(root, Trigger::Stop)).unwrap();
        assert_eq!(verdict, Verdict::Allow, "no note for a clear reply");
        assert!(!next_context(root, "s1").contains("reprovou"));

        let failed = ClarityCheck.evaluate(&stop("s1", FAILING), &ctx(root, Trigger::Stop)).unwrap();
        assert!(matches!(failed, Verdict::Inject { .. }));
        let rewritten = ClarityCheck.evaluate(&stop("s1", CLEAR), &ctx(root, Trigger::Stop)).unwrap();
        assert_eq!(rewritten, Verdict::Allow);
        assert!(!next_context(root, "s1").contains("reprovou"), "the newest reply rules");
    }

    /// AC-7 — com a resposta reprovada e o documento da spec mudado, o fim é
    /// barrado pela ordem de publicar: todas as travas do `Stop`, na ordem do
    /// registro, pelo `fold` e pela saída de verdade. A nota ao usuário não sai
    /// nesse fim — o bloqueio vence o `fold` —, mas a medição roda e os defeitos
    /// ficam guardados para a mensagem seguinte; a continuação que a ordem pede,
    /// solta pelo `stop_hook_active`, leva a nota ao usuário. O nome ficou o de
    /// antes porque a spec `humanize` o cita num critério.
    #[test]
    fn clarity_note_and_doc_link_share_the_stop_message() {
        let dir = project(Some("didactic"));
        let root = dir.path();
        open_unit(root, "# Demo\n\n## Contexto\n\nPrimeira versão.\n");
        let c = ctx(root, Trigger::Stop);
        let run_stop = |input: &HookInput| {
            let mut outcome = Outcome::allow();
            for module in Registry::new().applicable(Trigger::Stop, None) {
                if let Some(check) = &module.check {
                    outcome.fold(check.evaluate(input, &c).unwrap_or(Verdict::Allow));
                }
            }
            let json = hook_specific_output("Stop", &outcome).expect("the Stop speaks");
            serde_json::from_str::<Value>(&json).unwrap()
        };

        // A página mudou: a ordem de publicar barra o fim, sem nota ao usuário.
        let blocked = run_stop(&stop("s1", FAILING));
        assert_eq!(blocked["decision"].as_str(), Some("block"), "{blocked}");
        let order = blocked["reason"].as_str().unwrap_or_else(|| panic!("{blocked}"));
        assert!(order.contains("resumo.html") && order.contains("claude.ai"), "{order}");
        assert!(blocked.get("systemMessage").is_none(), "{blocked}");
        assert!(next_context(root, "s1").contains("reprovou"), "the defects still wait for the assistant");

        // A continuação que a ordem pediu é solta, e a nota chega ao usuário.
        let mut again = stop("s1", FAILING);
        again.raw["stop_hook_active"] = serde_json::json!(true);
        let released = run_stop(&again);
        assert!(released.get("decision").is_none(), "the continuation is released: {released}");
        let message = released["systemMessage"].as_str().unwrap_or_else(|| panic!("{released}"));
        assert!(message.contains("Mustard · clareza"), "{message}");
        assert!(message.contains("- CI sem as palavras por extenso"), "{message}");
    }

    /// Uma resposta em inglês num projeto em português reprova pelo idioma, e
    /// o defeito segue pelos caminhos de sempre: a nota ao usuário e a
    /// mensagem seguinte.
    #[test]
    fn a_reply_in_another_language_reaches_the_note_and_the_next_prompt() {
        let dir = project(Some("didactic"));
        let root = dir.path();
        let reply = "The wave is done and the tests pass.\n\
            The check now compares the language of the reply with the language of the project.\n\
            It counts the common words of each language.\n\
            A short reply is not judged at all.";
        let Verdict::Inject { context: note } =
            ClarityCheck.evaluate(&stop("s1", reply), &ctx(root, Trigger::Stop)).unwrap()
        else {
            panic!("a reply in another language speaks");
        };
        let defect = "- resposta em en-US; o idioma do projeto e do usuário é pt-BR";
        assert!(note.contains(defect), "{note}");
        assert!(next_context(root, "s1").contains(defect), "the defect waits for the assistant");
    }

    /// AC-8 — só um projeto que declarou o tom didático tem as respostas
    /// medidas; nos outros nada é gravado nem registrado.
    #[test]
    fn clarity_check_only_runs_for_didactic_tone() {
        for tone in [Some("technical"), Some("concise"), None] {
            let dir = project(tone);
            let root = dir.path();
            let verdict = ClarityCheck.evaluate(&stop("s1", FAILING), &ctx(root, Trigger::Stop)).unwrap();
            assert_eq!(verdict, Verdict::Allow, "{tone:?} asked for no measurement");
            assert!(!record_file(root, "s1").exists(), "{tone:?}: nothing recorded");
            assert!(clarity_rows(root).is_empty(), "{tone:?}: no event");
        }
        let bare = tempdir().unwrap();
        let verdict = ClarityCheck.evaluate(&stop("s1", FAILING), &ctx(bare.path(), Trigger::Stop)).unwrap();
        assert_eq!(verdict, Verdict::Allow, "no mustard.json, no measurement");

        for tone in ["didactic", "didático"] {
            let dir = project(Some(tone));
            let verdict = ClarityCheck.evaluate(&stop("s1", FAILING), &ctx(dir.path(), Trigger::Stop)).unwrap();
            assert!(matches!(verdict, Verdict::Inject { .. }), "{tone}: {verdict:?}");
            assert!(record_file(dir.path(), "s1").is_file(), "{tone}: recorded");
        }

        let dir = project(Some("didactic"));
        let mut sub = stop("s1", FAILING);
        sub.agent_id = Some("child".to_string());
        assert_eq!(ClarityCheck.evaluate(&sub, &ctx(dir.path(), Trigger::Stop)).unwrap(), Verdict::Allow);
        let pre = ctx(dir.path(), Trigger::PreToolUse);
        assert_eq!(ClarityCheck.evaluate(&stop("s1", FAILING), &pre).unwrap(), Verdict::Allow);
    }

    /// AC-9 — o evento traz as contagens e o resultado, e nada do texto.
    #[test]
    fn clarity_event_records_metrics_not_text() {
        let dir = project(Some("didactic"));
        let root = dir.path();
        let reply = "O CI falhou com a senha zebra-quartzo-sete no log.";
        let _ = ClarityCheck.evaluate(&stop("s1", reply), &ctx(root, Trigger::Stop)).unwrap();

        let rows = clarity_rows(root);
        assert_eq!(rows.len(), 1, "one measured reply, one event: {rows:?}");
        let row: Value = serde_json::from_str(&rows[0]).unwrap();
        let metrics = find_metrics(&row).unwrap_or_else(|| panic!("no metrics in {row}"));
        assert_eq!(metrics["passed"], json!(false));
        assert_eq!(metrics["long_sentences"], json!(0));
        assert_eq!(metrics["unexpanded_acronyms"], json!(1));
        assert_eq!(metrics["unexplained_terms"], json!(0));
        assert_eq!(metrics["prose_lines"], json!(1));
        assert_eq!(metrics["too_long"], json!(false));
        for fragment in ["zebra-quartzo-sete", "falhou", "senha"] {
            assert!(!rows[0].contains(fragment), "the reply text leaked ({fragment}): {}", rows[0]);
        }
    }

    /// AC-11 — a nota e o texto levado à mensagem seguinte mostram no máximo
    /// [`MAX_LISTED_DEFECTS`] defeitos e ficam abaixo de 1.500 caracteres,
    /// qualquer que seja a resposta: o texto da mensagem seguinte divide o teto
    /// de 10.000 caracteres com o injetável do irmão eleito.
    #[test]
    fn clarity_defects_are_capped() {
        let dir = project(Some("didactic"));
        let root = dir.path();
        // Sessenta frases longas de palavras compridas: cada defeito passa do
        // corte de [`MAX_DEFECT_CHARS`], e os que sobram viram uma contagem.
        let long = vec!["palavraextraordinariamentecomprida"; 40].join(" ");
        let reply = format!("{long}.\n").repeat(60);
        let Verdict::Inject { context: note } =
            ClarityCheck.evaluate(&stop("s1", &reply), &ctx(root, Trigger::Stop)).unwrap()
        else {
            panic!("a failing reply speaks");
        };
        let next = next_context(root, "s1");
        let block = &next[next.find("A sua resposta anterior").unwrap_or_else(|| panic!("{next}"))..];
        for text in [note.as_str(), block] {
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

    /// O `fold` junta os `Inject` de uma invocação, e no `UserPromptSubmit` e
    /// no `SessionStart` um só `Check` injeta: a regra e os defeitos chegam uma
    /// vez, na ordem de quem os compõe, e nada se junta ao que já mediu o teto.
    #[test]
    fn prompt_and_session_start_have_one_injecting_check() {
        let dir = project(Some("didactic"));
        let root = dir.path();
        let _ = ClarityCheck.evaluate(&stop("s1", FAILING), &ctx(root, Trigger::Stop)).unwrap();

        let registry = Registry::new();
        let on_prompt = prompt("s1", "e agora?");
        let on_start = HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some("s1".to_string()),
            ..HookInput::default()
        };
        for (name, trigger, input) in [
            ("UserPromptSubmit", Trigger::UserPromptSubmit, &on_prompt),
            ("SessionStart", Trigger::SessionStart, &on_start),
        ] {
            let c = ctx(root, trigger);
            let mut outcome = Outcome::allow();
            let mut injecting = Vec::new();
            for module in registry.applicable(trigger, None) {
                let Some(check) = &module.check else { continue };
                let verdict = check.evaluate(input, &c).unwrap_or(Verdict::Allow);
                if matches!(verdict, Verdict::Inject { .. }) {
                    injecting.push(module.id);
                }
                outcome.fold(verdict);
            }
            assert!(injecting.len() <= 1, "{name}: {injecting:?} would share one response");
            if name == "UserPromptSubmit" {
                let Verdict::Inject { context } = &outcome.verdict else {
                    panic!("the prompt carries the rule: {:?}", outcome.verdict);
                };
                assert_eq!(context.matches("ONE idea per sentence").count(), 1, "{context}");
                assert_eq!(context.matches("reprovou").count(), 1, "{context}");
            }
        }
    }

    /// A sigla explicada numa resposta não é cobrada na seguinte: o registro
    /// da sessão guarda o que já foi explicado.
    #[test]
    fn explained_acronym_carries_across_replies() {
        let dir = project(Some("didactic"));
        let root = dir.path();
        let c = ctx(root, Trigger::Stop);
        let first = ClarityCheck.evaluate(&stop("s1", "O CI (integração contínua) falhou."), &c).unwrap();
        assert_eq!(first, Verdict::Allow);
        assert_eq!(ClarityCheck.evaluate(&stop("s1", FAILING), &c).unwrap(), Verdict::Allow);
        // Outra sessão começa do zero.
        assert!(matches!(ClarityCheck.evaluate(&stop("s2", FAILING), &c).unwrap(), Verdict::Inject { .. }));
    }

    /// Os nomes inventados saem das três fontes: a semente do output style, as
    /// Definições da spec ativa e o glossário.
    #[test]
    fn invented_names_come_from_style_spec_and_glossary() {
        assert_eq!(
            style_seed(),
            ["gate", "wave", "slug", "upsert", "strict", "warn", "boundary"].map(String::from)
        );
        assert_eq!(bold_term("- [D-1] **onda** — uma passada"), Some("onda".to_string()));
        assert_eq!(bold_term("- **porta única** — a entrada"), Some("porta única".to_string()));
        assert_eq!(bold_term("texto com **negrito** solto"), None);

        let dir = project(Some("didactic"));
        let root = dir.path();
        open_unit(root, "# Demo\n\n## Definitions\n\n- [D-1] **trava** — o gancho que barra\n");
        std::fs::write(root.join("CONTEXT.md"), "# Glossário\n\n**Unidade**: um trabalho aberto.\n")
            .unwrap();
        let project_dir = root.to_string_lossy().into_owned();
        let terms = invented_terms(root, &project_dir);
        for want in ["slug", "trava", "Unidade"] {
            assert!(terms.iter().any(|t| t == want), "missing {want}: {terms:?}");
        }
    }
}
