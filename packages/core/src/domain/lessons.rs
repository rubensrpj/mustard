//! `lessons` — o banco de lições (`.claude/spec/lessons.ndjson`).
//!
//! Uma lição é o que o projeto aprendeu e não pode esquecer: um defeito que
//! pode se repetir (`defect`), uma regra do projeto (`project_rule`), uma
//! armadilha do ambiente (`environment_trap`) ou uma preferência do usuário
//! (`user_preference`). O banco fica fora das pastas das specs e é escrito só
//! pelo binário, pelo `write lesson`. Cada linha tem o mesmo envelope dos
//! eventos da spec (`v`, `id`, `at`, `type`, `author`), sem código de item e
//! sem `origin`: o `type` guarda a classe da lição, e a lição é apontada pelo
//! número dela no banco.
//!
//! Toda lição diz onde vale (`applies_to`: o subprojeto, os arquivos ou a
//! skill) e onde nasceu (`found_in`: a spec, a branch e o commit, ou o arquivo
//! de onde veio, em `source`). A lição que vale no projeto todo diz isso com
//! os arquivos `["**"]`, e é achada para qualquer caminho, subprojeto ou
//! skill.
//!
//! Duas buscas, só do Rust: por escopo ([`in_scope`]), pelo caminho dos
//! arquivos, pelo subprojeto e pela skill; e por palavras ([`matching`]), com
//! o BM25 de `domain::search` sobre o `search`, que devolve as 5 mais fortes.
//! A mesma busca por palavras limita as lições que o pedido de uma onda e o
//! da revisão levam ([`related_to_tasks`]): de cada classe, só as mais
//! ligadas às tarefas, porque uma pasta pode ter centenas delas.
//! Quem mostra uma lição mostra o texto original ([`shown`]), nunca o
//! `search`.
//!
//! Uma lição nunca entra repetida: a que tem o mesmo texto de outra já
//! guardada, depois de igualar espaços, maiúsculas e acentos
//! ([`comparable`]), é recusada apontando a que existe ([`repeated`]). A
//! gravação do assistente e a importação das instruções do projeto passam
//! pela mesma comparação.
//!
//! Função pura: sem disco e sem relógio. A gravação mora em `io::lessons`.

use serde_json::{Map, Value};

use std::collections::BTreeMap;

use crate::domain::config::glob_matches;
use crate::domain::search::{self, Hit};
use crate::domain::text::fold;
use crate::domain::spec_events::{
    check_field, is_empty, opt, req, shown_line, Kind, Refusal, SpecEvent, SpecLog, AUTHORS, BINARY_FIELDS,
    DEFAULT_AUTHOR, PURGED_FIELD, REFUSED_FIELDS,
};

/// O tipo com que o `write` recebe uma lição.
pub const LESSON: &str = "lesson";

/// As classes de lição, gravadas no `type` da linha.
pub const CLASSES: &[&str] = &["defect", "project_rule", "environment_trap", "user_preference"];

/// O padrão de arquivos da lição que vale no projeto todo.
pub const WHOLE_PROJECT: &str = "**";

/// Os campos de `applies_to`: onde a lição vale.
const SCOPE_FIELDS: &[(&str, Kind)] = &[("subproject", Kind::Text), ("files", Kind::Texts), ("skill", Kind::Text)];

/// Os campos de `found_in`: onde a lição nasceu.
const ORIGIN_FIELDS: &[&str] = &["spec", "branch", "commit", "source"];

/// O rascunho de quem grava, pronto para a conferência: sem os campos que só
/// o binário escreve, com a classe (`class`) no `type`, com o autor (o
/// assistente, quando quem grava não diz) e, quando o `write` recebeu uma
/// spec, com ela em `found_in.spec`, se faltava.
#[must_use]
pub fn normalize(mut draft: Map<String, Value>, spec: Option<&str>) -> Map<String, Value> {
    for field in BINARY_FIELDS.iter().filter(|f| !REFUSED_FIELDS.contains(f)) {
        draft.remove(*field);
    }
    draft.remove(PURGED_FIELD);
    let class = draft.remove("class");
    draft.remove("type");
    if let Some(class) = class {
        draft.insert("type".into(), class);
    }
    if draft.get("author").is_none_or(is_empty) {
        draft.insert("author".into(), Value::String(DEFAULT_AUTHOR.into()));
    }
    if let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty()) {
        match draft.get_mut("found_in") {
            Some(Value::Object(found)) => {
                if found.get("spec").is_none_or(is_empty) {
                    found.insert("spec".into(), Value::from(spec));
                }
            }
            None | Some(Value::Null) => {
                let mut found = Map::new();
                found.insert("spec".into(), Value::from(spec));
                draft.insert("found_in".into(), Value::Object(found));
            }
            Some(_) => {}
        }
    }
    draft
}

/// Confere uma lição sozinha: a classe é uma das quatro, o texto e as chaves
/// estão preenchidos, ela diz onde vale e onde nasceu, e cada campo tem a
/// forma certa. O que depende do banco (a lição substituída existe) fica para
/// [`check_against`].
pub fn validate(event: &Map<String, Value>) -> Result<(), Refusal> {
    if let Some(field) = REFUSED_FIELDS.iter().find(|f| event.contains_key(**f)) {
        return Err(Refusal::BinaryOnlyField { field: (*field).to_string() });
    }
    match event.get("type") {
        Some(class) if !is_empty(class) => {
            if !Kind::OneOf(CLASSES).accepts(class) {
                return Err(invalid("class", Kind::OneOf(CLASSES)));
            }
        }
        _ => return Err(missing("class")),
    }
    for field in [
        req("author", Kind::OneOf(AUTHORS)),
        req("text", Kind::Text),
        req("keys", Kind::Texts),
        opt("label", Kind::Text),
        opt("replaces", Kind::Int),
    ] {
        check_field(event, LESSON, field)?;
    }
    check_applies_to(event)?;
    check_found_in(event)
}

fn missing(field: &str) -> Refusal {
    Refusal::MissingField { event_type: LESSON.to_string(), field: field.to_string() }
}

fn invalid(field: &str, expected: Kind) -> Refusal {
    Refusal::InvalidValue { event_type: LESSON.to_string(), field: field.to_string(), expected }
}

/// Um campo de dentro de `owner`, quando veio, tem a forma `kind`.
fn check_inner(object: &Map<String, Value>, owner: &str, name: &str, kind: Kind) -> Result<(), Refusal> {
    match object.get(name) {
        Some(value) if !is_empty(value) && !kind.accepts(value) => Err(invalid(&format!("{owner}.{name}"), kind)),
        _ => Ok(()),
    }
}

/// Onde a lição vale: um objeto com pelo menos o subprojeto, os arquivos ou
/// a skill. Sem exceção: a que vale no projeto todo diz os arquivos `["**"]`.
fn check_applies_to(event: &Map<String, Value>) -> Result<(), Refusal> {
    let Some(value) = event.get("applies_to").filter(|v| !is_empty(v)) else {
        return Err(missing("applies_to"));
    };
    let Some(scope) = value.as_object() else {
        return Err(invalid("applies_to", Kind::Object));
    };
    for (name, kind) in SCOPE_FIELDS {
        check_inner(scope, "applies_to", name, *kind)?;
    }
    if SCOPE_FIELDS.iter().all(|(name, _)| scope.get(*name).is_none_or(is_empty)) {
        return Err(missing("applies_to"));
    }
    Ok(())
}

/// Onde a lição nasceu: a spec, a branch e o commit, ou o arquivo de onde ela
/// veio, com pelo menos um deles.
fn check_found_in(event: &Map<String, Value>) -> Result<(), Refusal> {
    let Some(value) = event.get("found_in").filter(|v| !is_empty(v)) else {
        return Err(Refusal::LessonOriginMissing);
    };
    let Some(found) = value.as_object() else {
        return Err(invalid("found_in", Kind::Object));
    };
    for name in ORIGIN_FIELDS {
        check_inner(found, "found_in", name, Kind::Text)?;
    }
    if ORIGIN_FIELDS.iter().all(|name| found.get(*name).is_none_or(is_empty)) {
        return Err(Refusal::LessonOriginMissing);
    }
    Ok(())
}

/// Confere a lição contra o banco como está: a lição que ela substitui
/// (`replaces`) existe, e nenhuma outra lição guardada tem o mesmo texto. A
/// que ela substitui não conta: a versão nova pode só pôr os acentos.
pub fn check_against(bank: &SpecLog, event: &Map<String, Value>) -> Result<(), Refusal> {
    let replaces = event.get("replaces").and_then(Value::as_u64);
    if let Some(id) = replaces
        && bank.get(id).is_none()
    {
        return Err(Refusal::UnknownLesson { id });
    }
    let text = event.get("text").and_then(Value::as_str).unwrap_or_default();
    match repeated(bank, text, replaces) {
        Some(same) => Err(Refusal::LessonRepeated {
            id: same.id,
            text: same.str_field("text").unwrap_or_default().to_string(),
        }),
        None => Ok(()),
    }
}

/// O texto na forma em que duas lições se comparam: minúsculas, sem acento e
/// com um espaço só entre as palavras. `"Não  REPITA"` e `"nao repita"` são o
/// mesmo texto.
#[must_use]
pub fn comparable(text: &str) -> String {
    fold(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A lição vigente do banco cujo texto é o mesmo de `text`, pela forma de
/// [`comparable`], de qualquer classe. `except` é a lição que a nova
/// substitui, que não conta. Texto vazio não repete nada.
#[must_use]
pub fn repeated<'a>(bank: &'a SpecLog, text: &str, except: Option<u64>) -> Option<&'a SpecEvent> {
    let wanted = comparable(text);
    if wanted.is_empty() {
        return None;
    }
    bank.visible()
        .into_iter()
        .filter(|lesson| Some(lesson.id) != except)
        .find(|lesson| lesson.str_field("text").is_some_and(|own| comparable(own) == wanted))
}

/// Onde se procura lição: os arquivos, o subprojeto e a skill de uma onda.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    pub files: Vec<String>,
    pub subproject: Option<String>,
    pub skill: Option<String>,
}

/// As lições vigentes que valem para `scope`, em ordem de número. Entra a
/// lição do projeto todo e toda lição em que um arquivo do escopo casa um
/// padrão dos arquivos dela, em que o subprojeto é o dela ou um arquivo do
/// escopo fica dentro dele, ou em que a skill é a dela.
#[must_use]
pub fn in_scope<'a>(bank: &'a SpecLog, scope: &Scope) -> Vec<&'a SpecEvent> {
    let mut found: Vec<&SpecEvent> = bank.visible().into_iter().filter(|lesson| applies(lesson, scope)).collect();
    found.sort_by_key(|lesson| lesson.id);
    found
}

/// Os defeitos já vistos nos arquivos de `scope`, em ordem de número: as
/// lições da classe do defeito que valem ali. É o que o revisor de uma onda
/// precisa ver antes de olhar o código — o erro que já aconteceu naqueles
/// arquivos é o que tem mais chance de se repetir.
#[must_use]
pub fn defects_in_scope<'a>(bank: &'a SpecLog, scope: &Scope) -> Vec<&'a SpecEvent> {
    in_scope(bank, scope).into_iter().filter(|lesson| lesson.event_type == DEFECT).collect()
}

/// A classe da lição que guarda um defeito que pode se repetir.
pub const DEFECT: &str = "defect";

/// A classe da lição que guarda uma regra do projeto: vale sempre, então
/// não vira pergunta no levantamento.
pub const PROJECT_RULE: &str = "project_rule";

/// O "onde vale" de um evento casa com `scope`? A mesma leitura serve à lição
/// e ao item combinado, que declaram o campo do mesmo jeito: sem ela, o
/// recorte dos itens por onda e a busca de lições discordariam sobre o mesmo
/// campo.
#[must_use]
pub fn applies_to(event: &SpecEvent, scope: &Scope) -> bool {
    applies(event, scope)
}

fn applies(lesson: &SpecEvent, scope: &Scope) -> bool {
    let Some(at) = lesson.fields.get("applies_to").and_then(Value::as_object) else {
        return false;
    };
    let patterns: Vec<String> = at
        .get("files")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(clean_path).collect())
        .unwrap_or_default();
    if patterns.iter().any(|p| !p.is_empty() && p.chars().all(|c| c == '*')) {
        return true;
    }
    let files: Vec<String> = scope.files.iter().map(|f| clean_path(f)).filter(|f| !f.is_empty()).collect();
    if files.iter().any(|file| patterns.iter().any(|p| path_matches(p, file))) {
        return true;
    }
    if let Some(sub) = at.get("subproject").and_then(Value::as_str).map(clean_path).filter(|s| !s.is_empty()) {
        if scope.subproject.as_deref().map(clean_path).is_some_and(|s| s == sub) {
            return true;
        }
        if files.iter().any(|file| path_matches(&sub, file)) {
            return true;
        }
    }
    let skill = at.get("skill").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    skill.is_some_and(|skill| scope.skill.as_deref().map(str::trim) == Some(skill))
}

/// O caminho com barras normais, sem `./` no começo e sem barra no fim.
fn clean_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let path = path.strip_prefix("./").unwrap_or(&path);
    path.trim_end_matches('/').to_string()
}

/// `file` casa o padrão com `*`, ou é o próprio caminho, ou fica dentro dele.
fn path_matches(pattern: &str, file: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    if pattern.contains('*') {
        return glob_matches(pattern, file);
    }
    file == pattern || file.strip_prefix(pattern).is_some_and(|rest| rest.starts_with('/'))
}

/// As lições vigentes cujo `search` casa as palavras do pedido, as 5 mais
/// fortes, pelo BM25.
#[must_use]
pub fn matching(bank: &SpecLog, words: &str) -> Vec<Hit> {
    matching_among(&bank.visible(), words)
}

/// A mesma busca de [`matching`], só entre `lessons`: quem já separou as
/// lições que interessam não deixa as outras tomarem o lugar delas entre as
/// 5 mais fortes.
#[must_use]
pub fn matching_among(lessons: &[&SpecEvent], words: &str) -> Vec<Hit> {
    search::search(lessons.iter().map(|lesson| (lesson.id, lesson.str_field("search").unwrap_or_default())), words)
}

/// As lições que o pedido de uma onda e o da revisão levam, entre as `found`
/// que valem para ela: de cada classe, só as 5 mais ligadas às palavras das
/// tarefas (`words`), pela busca de [`matching`]. A lição que não tem palavra
/// nenhuma em comum com as tarefas fica fora, seja qual for a classe. Em
/// ordem de número.
#[must_use]
pub fn related_to_tasks<'a>(found: Vec<&'a SpecEvent>, words: &str) -> Vec<&'a SpecEvent> {
    let mut by_class: BTreeMap<&str, Vec<&SpecEvent>> = BTreeMap::new();
    for lesson in found {
        by_class.entry(lesson.event_type.as_str()).or_default().push(lesson);
    }
    let mut kept: Vec<&SpecEvent> = Vec::new();
    for lessons in by_class.into_values() {
        let related = matching_among(&lessons, words);
        kept.extend(lessons.into_iter().filter(|lesson| related.iter().any(|hit| hit.id == lesson.id)));
    }
    kept.sort_by_key(|lesson| lesson.id);
    kept
}

/// A lição como é mostrada: a linha com o texto original, sem o `search`.
#[must_use]
pub fn shown(lesson: &SpecEvent) -> String {
    shown_line(&lesson.fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{parse_log, render_line, stamp};
    use crate::platform::i18n::Locale;
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    fn checked(draft: Value) -> Result<(), Refusal> {
        validate(&normalize(obj(draft), None))
    }

    /// Uma lição como o gravador deixa, com o número `id`.
    fn lesson(id: u64, draft: Value) -> String {
        let event = normalize(obj(draft), None);
        validate(&event).unwrap_or_else(|r| panic!("lesson {id} refused: {r:?}"));
        format!("{}\n", render_line(&stamp(event, id, None, "2026-09-12T10:00:00-03:00")))
    }

    fn base(applies_to: Value) -> Value {
        json!({"class": "project_rule", "text": "t", "keys": ["k"], "applies_to": applies_to, "found_in": {"source": "apps/rt/CLAUDE.md"}})
    }

    fn bank() -> SpecLog {
        parse_log(
            &[
                lesson(1, base(json!({"files": ["apps/rt/src/hooks/**"]}))),
                lesson(2, base(json!({"subproject": "packages/core"}))),
                lesson(3, base(json!({"skill": "add-run-command"}))),
                lesson(4, json!({"class": "user_preference", "text": "Resposta curta.", "keys": ["resposta"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}})),
                lesson(5, base(json!({"files": ["apps/cli/src/main.rs"]}))),
            ]
            .concat(),
        )
    }

    fn found(bank: &SpecLog, scope: &Scope) -> Vec<u64> {
        in_scope(bank, scope).iter().map(|l| l.id).collect()
    }

    fn files(paths: &[&str]) -> Scope {
        Scope { files: paths.iter().map(|p| (*p).to_string()).collect(), ..Scope::default() }
    }

    /// A lição é achada pelo padrão dos arquivos, pelo subprojeto e pela
    /// skill; a do projeto todo é achada em qualquer escopo. Um caminho exato
    /// não casa por pedaço.
    #[test]
    fn a_lesson_is_found_by_file_pattern_subproject_and_skill() {
        let bank = bank();
        assert_eq!(found(&bank, &files(&["apps/rt/src/hooks/write/scope_guard.rs"])), [1, 4]);
        assert_eq!(found(&bank, &files(&["apps\\rt\\src\\hooks\\x.rs"])), [1, 4], "a Windows path matches too");
        let sub = Scope { subproject: Some("packages/core".into()), ..Scope::default() };
        assert_eq!(found(&bank, &sub), [2, 4]);
        let skill = Scope { skill: Some("add-run-command".into()), ..Scope::default() };
        assert_eq!(found(&bank, &skill), [3, 4]);
        assert_eq!(found(&bank, &files(&["apps/cli/src/main.rs"])), [4, 5]);
        assert_eq!(found(&bank, &files(&["apps/cli/src/main.rs.bak"])), [4], "an exact path does not match a piece");
    }

    #[test]
    fn a_file_inside_the_lesson_subproject_finds_it() {
        let bank = bank();
        assert_eq!(found(&bank, &files(&["./packages/core/src/domain/search.rs"])), [2, 4]);
        assert_eq!(found(&bank, &files(&["packages/core-extra/src/lib.rs"])), [4]);
    }

    #[test]
    fn a_replaced_lesson_is_no_longer_found() {
        let mut content = [
            lesson(1, json!({"class": "defect", "text": "Apagar a pasta perde trabalho.", "keys": ["apagar"], "applies_to": {"files": ["apps/rt/src/hooks/**"]}, "found_in": {"spec": "s"}})),
        ]
        .concat();
        content.push_str(&lesson(2, json!({"class": "defect", "text": "Remover a pasta perde trabalho.", "keys": ["remover"], "applies_to": {"files": ["apps/rt/src/hooks/**"]}, "found_in": {"spec": "s"}, "replaces": 1})));
        let bank = parse_log(&content);
        assert_eq!(found(&bank, &files(&["apps/rt/src/hooks/x.rs"])), [2]);
        assert!(matching(&bank, "apagando a pasta").iter().all(|hit| hit.id != 1));
    }

    /// A lição achada pelas palavras é mostrada com o texto original, e o
    /// campo de busca não aparece.
    #[test]
    fn a_found_lesson_shows_the_original_text_never_the_search_field() {
        let content = [
            lesson(1, base(json!({"subproject": "apps/rt"}))),
            lesson(2, json!({"class": "defect", "text": "Um rm -rf na pasta errada perde trabalho.", "keys": ["apagar", "rm"], "applies_to": {"subproject": "apps/rt"}, "found_in": {"spec": "s", "branch": "b", "commit": "abc1234"}})),
        ]
        .concat();
        let bank = parse_log(&content);
        let hits = matching(&bank, "apagando a pasta");
        assert_eq!(hits.first().map(|h| h.id), Some(2), "{hits:?}");
        let lesson = bank.get(2).unwrap();
        let shown = shown(lesson);
        assert!(shown.contains("\"text\":\"Um rm -rf na pasta errada perde trabalho.\""), "{shown}");
        assert!(!shown.contains("search"), "{shown}");
        assert!(!shown.contains(lesson.str_field("search").unwrap()), "{shown}");
    }

    /// De cada classe ficam só as 5 mais ligadas às palavras das tarefas, e a
    /// lição sem palavra em comum com elas sai, seja qual for a classe; tudo
    /// volta em ordem de número. Com seis defeitos ligados, o sexto, o mais
    /// fraco, perde o lugar: cinco é o último número que passa inteiro.
    #[test]
    fn of_each_class_only_the_five_lessons_closest_to_the_tasks_are_kept() {
        let of = |id: u64, class: &str, text: &str| lesson(id, json!({"class": class, "text": text, "keys": ["k"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"source": "CLAUDE.md"}}));
        let mut content = vec![of(1, "environment_trap", "O cargo não está no PATH.")];
        for id in 2..=7 {
            content.push(of(id, "project_rule", &format!("A fatura soma o total {id}.")));
        }
        content.push(of(8, "project_rule", "O módulo declara o dono."));
        for id in 9..=13 {
            content.push(of(id, "defect", &format!("A fatura somou o total errado no relatório {id}.")));
        }
        content.push(of(14, "defect", "A fatura antiga ficou de fora."));
        content.push(of(15, "defect", "Apagar a pasta perde trabalho."));
        content.push(of(16, "user_preference", "O total da fatura sai em reais."));
        let bank = parse_log(&content.concat());
        let kept: Vec<u64> =
            related_to_tasks(bank.visible(), "Somar o total da fatura no relatório").iter().map(|l| l.id).collect();
        let rules: Vec<u64> = kept.iter().copied().filter(|id| (2..=8).contains(id)).collect();
        assert_eq!(rules.len(), 5, "cinco regras: {kept:?}");
        assert!(!kept.contains(&8), "a regra sem palavra em comum sai: {kept:?}");
        let defects: Vec<u64> = kept.iter().copied().filter(|id| (9..=15).contains(id)).collect();
        assert_eq!(defects, [9, 10, 11, 12, 13], "os cinco defeitos mais ligados; o sexto e o sem palavra saem: {kept:?}");
        assert!(!kept.contains(&1), "a armadilha sem palavra em comum sai: {kept:?}");
        assert!(kept.contains(&16), "a preferência ligada fica: {kept:?}");
        assert!(kept.windows(2).all(|w| w[0] < w[1]), "{kept:?}");
    }

    /// O mesmo texto, com outros espaços, maiúsculas e acentos, é a mesma
    /// lição: a nova é recusada apontando a que existe, de qualquer classe. A
    /// que ela substitui não conta, e a que já foi substituída também não.
    #[test]
    fn a_lesson_repeating_the_text_of_one_already_kept_is_refused_naming_it() {
        let bank = parse_log(
            &[
                lesson(1, json!({"class": "defect", "text": "Não apague a pasta.", "keys": ["apagar"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}})),
                lesson(2, json!({"class": "defect", "text": "Rode em primeiro plano.", "keys": ["rodar"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}})),
                lesson(3, json!({"class": "defect", "text": "Rode tudo em primeiro plano.", "keys": ["rodar"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}, "replaces": 2})),
            ]
            .concat(),
        );
        let draft = |text: &str, extra: Value| {
            let mut event = normalize(obj(json!({"class": "project_rule", "text": text, "keys": ["k"], "applies_to": {"subproject": "apps/rt"}, "found_in": {"source": "apps/rt/CLAUDE.md"}})), None);
            if let Value::Object(more) = extra {
                event.extend(more);
            }
            event
        };
        let refusal = check_against(&bank, &draft("  NAO   apague a PASTA. ", json!({}))).unwrap_err();
        assert_eq!(refusal, Refusal::LessonRepeated { id: 1, text: "Não apague a pasta.".into() });
        assert_eq!(refusal.reason(), "lesson-repeated");
        assert!(refusal.message(Locale::PtBr).contains("lição 1"), "{}", refusal.message(Locale::PtBr));
        assert!(refusal.message(Locale::PtBr).contains("Não apague a pasta."), "{}", refusal.message(Locale::PtBr));
        assert!(refusal.message(Locale::EnUs).contains("lesson 1"), "{}", refusal.message(Locale::EnUs));
        assert!(check_against(&bank, &draft("Não apague a pasta errada.", json!({}))).is_ok(), "outro texto entra");
        assert!(check_against(&bank, &draft("Nao apague a pasta.", json!({"replaces": 1}))).is_ok(), "a versão nova da própria lição entra");
        assert!(check_against(&bank, &draft("Rode em primeiro plano.", json!({}))).is_ok(), "a lição substituída não conta");
    }

    #[test]
    fn a_lesson_without_class_text_keys_or_origin_is_refused_by_name() {
        let full = json!({"class": "defect", "text": "t", "keys": ["k"], "applies_to": {"skill": "s"}, "found_in": {"spec": "x"}});
        assert!(checked(full.clone()).is_ok());
        for (field, refusal) in [
            ("class", missing("class")),
            ("text", missing("text")),
            ("keys", missing("keys")),
            ("applies_to", missing("applies_to")),
            ("found_in", Refusal::LessonOriginMissing),
        ] {
            let mut draft = full.clone();
            draft.as_object_mut().unwrap().remove(field);
            assert_eq!(checked(draft).unwrap_err(), refusal, "without {field}");
        }
        let empty_scope = json!({"class": "defect", "text": "t", "keys": ["k"], "applies_to": {"file": "x"}, "found_in": {"spec": "x"}});
        assert_eq!(checked(empty_scope).unwrap_err(), missing("applies_to"));
        let no_origin = json!({"class": "defect", "text": "t", "keys": ["k"], "applies_to": {"skill": "s"}, "found_in": {"why": "x"}});
        let refusal = checked(no_origin).unwrap_err();
        assert_eq!(refusal, Refusal::LessonOriginMissing);
        assert_eq!(refusal.reason(), "lesson-origin-missing");
        assert!(refusal.message(Locale::PtBr).contains("onde nasceu"), "{}", refusal.message(Locale::PtBr));
        assert!(refusal.message(Locale::EnUs).contains("where it was born"), "{}", refusal.message(Locale::EnUs));
        let with_code = json!({"class": "defect", "text": "t", "keys": ["k"], "applies_to": {"skill": "s"}, "found_in": {"spec": "x"}, "code": "MSTD-X-0001"});
        assert_eq!(checked(with_code).unwrap_err(), Refusal::BinaryOnlyField { field: "code".into() });
    }

    #[test]
    fn an_unknown_class_is_refused_with_the_four_accepted_ones() {
        let draft = json!({"class": "bug", "text": "t", "keys": ["k"], "applies_to": {"skill": "s"}, "found_in": {"spec": "x"}});
        let refusal = checked(draft).unwrap_err();
        assert_eq!(refusal, invalid("class", Kind::OneOf(CLASSES)));
        for lang in [Locale::PtBr, Locale::EnUs] {
            let message = refusal.message(lang);
            assert!(message.contains("defect, project_rule, environment_trap, user_preference"), "{message}");
        }
    }

    /// A spec que o `write` recebeu diz onde a lição nasceu, quando a lição
    /// não diz; a que a lição diz fica.
    #[test]
    fn the_spec_given_to_the_write_fills_where_the_lesson_was_found() {
        let bare = normalize(obj(json!({"class": "defect", "text": "t", "keys": ["k"]})), Some("minha-spec"));
        assert_eq!(bare["found_in"], json!({"spec": "minha-spec"}));
        assert_eq!(bare["type"], json!("defect"));
        let own = normalize(obj(json!({"found_in": {"spec": "outra", "commit": "abc"}})), Some("minha-spec"));
        assert_eq!(own["found_in"], json!({"spec": "outra", "commit": "abc"}));
        let partial = normalize(obj(json!({"found_in": {"commit": "abc"}})), Some("minha-spec"));
        assert_eq!(partial["found_in"], json!({"spec": "minha-spec", "commit": "abc"}));
    }
}
