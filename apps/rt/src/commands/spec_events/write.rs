//! `mustard-rt run write <tipo> --spec <nome> --json '{…}'` — grava um evento
//! no arquivo de eventos da spec, a única porta de escrita dele.
//!
//! O binário põe a versão do formato, o número, o código do item, a hora com o
//! fuso e o campo de busca; o resto vem do `--json`, que não pode trazer o
//! código. Quem aponta um item (`replaces`, os alvos de `remove` e `purge`)
//! usa o número do evento ou o código que a página mostra. A saída diz o
//! número e o código gravados e, num `remove` ou num `purge`, os números
//! afetados:
//!
//! ```text
//! {"ok": true, "spec": "teste", "id": 41, "type": "remove", "code": "MSTD-RMV-0002", "removed": [12, 13]}
//! ```
//!
//! A página e o `.md` da spec e a linha dela no índice das specs são refeitos
//! a cada gravação, ainda com a trava do arquivo de eventos presa.
//!
//! Com o tipo `lesson`, a gravação vai para o banco de lições
//! (`.claude/spec/lessons.ndjson`), e não para a spec: a classe vem em
//! `class`, a lição diz onde vale (`applies_to`) e onde nasceu (`found_in`), e
//! o `--spec`, opcional só aqui, diz a spec em que ela nasceu quando a lição
//! não diz. A página e o índice não mudam:
//!
//! ```text
//! {"ok": true, "id": 8, "type": "lesson", "class": "defect"}
//! ```
//!
//! Num worktree, o evento vai para o arquivo do checkout principal. As
//! citações de arquivo de um ponto são conferidas a partir de onde o comando
//! roda.
//!
//! Uma spec aberta pelo `spec-draft` tem o `spec.md` escrito por ele, ao lado
//! do `meta.json`. Ali a página e o `.md` não são refeitos: o `.md` é o
//! documento do rascunho, e refazê-lo do arquivo de eventos apagaria o texto
//! da spec. O evento e a linha do índice são gravados do mesmo jeito.
//!
//! Quem grava por dentro do binário, como a testemunha da aprovação e o
//! `spec-draft`, usa [`record`], a mesma gravação deste comando.
//!
//! Um `state` com a fase `approved` é recusado aqui: a aprovação nasce só da
//! resposta do usuário à pergunta de aprovação, e quem a grava é a
//! testemunha, pelo [`record`].

use std::path::{Path, PathBuf};

use mustard_core::domain::lessons::LESSON;
use mustard_core::domain::spec_events::{type_spec, Refusal, PHASES};
use mustard_core::domain::spec_index;
use mustard_core::domain::spec_state::SpecState;
use mustard_core::io::{lessons, spec_events as store};
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};

use super::pages::SpecPages;
use crate::shared::spec_state::DiskSpecState;

/// Options for `mustard-rt run write`.
pub struct WriteOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec que recebe o evento; na lição, a spec em que ela nasceu.
    pub spec: Option<String>,
    pub event_type: String,
    /// Os campos do evento, num objeto JSON.
    pub json: String,
}

/// O núcleo testável de [`run`]: o relatório da gravação ou a recusa. Nunca
/// entra em pânico.
pub(crate) fn write_at(opts: &WriteOpts) -> Value {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let draft = match serde_json::from_str::<Value>(&opts.json) {
        Ok(Value::Object(map)) => map,
        Ok(other) => {
            let shown: String = other.to_string().chars().take(80).collect();
            return refuse(Refusal::NotAnObject { detail: shown });
        }
        Err(e) => return refuse(Refusal::NotAnObject { detail: e.to_string() }),
    };
    let event_type = opts.event_type.trim();
    if event_type == LESSON {
        return write_lesson(&project, opts.spec.as_deref(), draft);
    }
    let Some(spec) = opts.spec.as_deref() else {
        // Sem spec, um tipo que não existe continua recusado pelo nome.
        return refuse(if type_spec(event_type).is_some() {
            Refusal::SpecRequired { event_type: event_type.to_string() }
        } else {
            Refusal::UnknownType { found: event_type.to_string() }
        });
    };
    if event_type == "state"
        && draft.get("phase").and_then(Value::as_str).map(str::trim) == Some("approved")
    {
        return refuse(Refusal::ApprovalByWitnessOnly { spec: spec.trim().to_string() });
    }
    match record_in(&project, &opts.root, spec, event_type, draft) {
        Ok(Recorded { written, pages }) => {
            let mut report = json!({
                "ok": true,
                "spec": spec.trim(),
                "id": written.id,
                "type": event_type,
            });
            if let Some(code) = &written.code {
                report["code"] = json!(code);
            }
            if !written.removed.is_empty() {
                report["removed"] = json!(written.removed);
            }
            if !written.purged.is_empty() {
                report["purged"] = json!(written.purged);
            }
            // Se não deu para gravar a página e o `.md`, ou para refazer a
            // linha da spec no índice, o evento já está no arquivo: fica o
            // aviso.
            let mut warnings = Vec::new();
            if let Some(Err(refusal)) = &pages {
                warnings.push(refusal.message(lang));
            }
            if let Some(refusal) = &written.index_warning {
                warnings.push(spec_index::write_warning(refusal, lang));
            }
            if !warnings.is_empty() {
                report["warnings"] = json!(warnings);
            }
            report
        }
        Err(refusal) => refuse(refusal),
    }
}

/// O que uma gravação deixou: o evento e, quando a página e o `.md` foram
/// refeitos, onde eles estão ou por que não foram gravados.
pub(crate) struct Recorded {
    pub(crate) written: store::Written,
    /// `None` quando a spec tem o `spec.md` do `spec-draft`, que fica como
    /// está.
    pub(crate) pages: Option<Result<SpecPages, Refusal>>,
}

/// Grava um evento da spec `spec`, vista de `start`, pela mesma gravação do
/// `run write`: a linha no arquivo de eventos, a linha da spec no índice e a
/// página e o `.md`, quando a spec não é um rascunho do `spec-draft`.
pub(crate) fn record(
    start: &Path,
    spec: &str,
    event_type: &str,
    draft: Map<String, Value>,
) -> Result<Recorded, Refusal> {
    record_in(&super::project(start), start, spec, event_type, draft)
}

fn record_in(
    project: &super::Project,
    start: &Path,
    spec: &str,
    event_type: &str,
    draft: Map<String, Value>,
) -> Result<Recorded, Refusal> {
    let path = store::spec_file(&project.root, spec)?;
    let roots = store::citation_roots(start, &project.root);
    let drafted = super::pages::drafted_by_spec_draft(&project.root, spec);
    // A página e o `.md` acompanham cada gravação e são refeitos antes de a
    // trava soltar, do que acabou de ser gravado: a gravação seguinte, de
    // outra sessão, só entra depois, e refaz os dois por último.
    let mut pages = None;
    let written = store::write_then(&path, event_type, draft, &roots, |log| {
        if !drafted {
            pages = Some(super::pages::rebuild(&project.root, spec, log, project.lang));
        }
    })?;
    Ok(Recorded { written, pages })
}

/// A ponte até os gravadores definitivos do fechamento e do merge: grava no
/// estado da spec `spec`, vista de `start`, a fase `phase` (`closed` no
/// fechamento, `delivered` no merge), pela mesma gravação do `run write`.
///
/// Só grava quando a spec tem arquivo de eventos (uma branch que o Mustard
/// não abriu fica como está) e quando a fase de agora vem antes de `phase` na
/// ordem das fases: repetir um fechamento não grava outro, e uma spec entregue
/// não volta a fechada. `true` quando gravou.
pub(crate) fn record_phase(start: &Path, spec: &str, phase: &str) -> bool {
    let order = |name: &str| PHASES.iter().position(|known| *known == name);
    let Some(target) = order(phase) else {
        return false;
    };
    let Some(state) = DiskSpecState::new(start).state(spec) else {
        return false;
    };
    if state.phase.and_then(order).is_some_and(|now| now >= target) {
        return false;
    }
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!(phase));
    draft.insert("author".to_string(), json!("binary"));
    record(start, spec, "state", draft).is_ok()
}

/// O nascimento de uma spec aberta fora do arquivo de eventos — pelo
/// `spec-draft`, pelo `tactical-fix-create` ou, numa spec aberta antes dele,
/// pela testemunha da aprovação: um `state` na fase `plan`, com a branch da
/// spec e a base de que ela foi cortada, quando se sabem, pela mesma gravação
/// do `run write`.
///
/// A branch é `branch`, quando o chamador a sabe (a que o rascunho cortou, a
/// da spec-mãe de um tactical fix); senão, a do checkout, quando ela é a
/// desta spec. A base vem do `meta.json` da spec. Só na primeira vez: uma spec
/// que já tem fase, aprovada ou não, fica como está, e um rascunho refeito
/// nunca desfaz uma aprovação. `Ok(true)` quando gravou.
pub(crate) fn record_birth(start: &Path, spec: &str, branch: Option<&str>) -> Result<bool, Refusal> {
    if DiskSpecState::new(start).state(spec).is_some_and(|state| state.phase.is_some()) {
        return Ok(false);
    }
    let branch = branch.map(str::to_string).or_else(|| branch_of_spec(start, spec));
    let base = ClaudePaths::for_project(start)
        .and_then(|paths| paths.for_spec(spec.trim()))
        .ok()
        .and_then(|paths| mustard_core::read_meta(&paths.meta_json_path()))
        .and_then(|meta| meta.base);
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("plan"));
    draft.insert("author".to_string(), json!("binary"));
    if let Some(branch) = branch {
        draft.insert("branch".to_string(), json!(branch));
    }
    if let Some(base) = base {
        draft.insert("base".to_string(), json!(base));
    }
    record(start, spec, "state", draft).map(|_| true)
}

/// A branch do checkout em `start`, quando ela é a da spec `spec`.
fn branch_of_spec(start: &Path, spec: &str) -> Option<String> {
    use crate::commands::event::work_branch::{current_branch, slug_of_work_branch};
    let config = mustard_core::ProjectConfig::load(start);
    let vcs = config.vcs()?;
    let current = current_branch(&vcs, &start.to_string_lossy())?;
    (slug_of_work_branch(&current, &config).as_deref() == Some(spec.trim())).then_some(current)
}

/// Grava uma lição no banco de lições do projeto. `spec`, quando vem, diz em
/// que spec a lição nasceu.
fn write_lesson(project: &super::Project, spec: Option<&str>, draft: Map<String, Value>) -> Value {
    let refuse = |refusal: Refusal| super::refused(&refusal, project.lang);
    let path = match ClaudePaths::for_project(&project.root) {
        Ok(paths) => paths.lessons_path(),
        Err(e) => return refuse(Refusal::Io { detail: e.to_string() }),
    };
    match lessons::write(&path, draft, spec) {
        Ok(written) => json!({ "ok": true, "id": written.id, "type": LESSON, "class": written.class }),
        Err(refusal) => refuse(refusal),
    }
}

/// Run `write` and print the JSON report; exit 1 on a refusal.
pub fn run(opts: &WriteOpts) {
    let report = write_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_to(root: &std::path::Path, spec: Option<&str>, event_type: &str, json: &str) -> Value {
        write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: spec.map(str::to_string),
            event_type: event_type.into(),
            json: json.into(),
        })
    }

    fn write(root: &std::path::Path, event_type: &str, json: &str) -> Value {
        write_to(root, Some("teste"), event_type, json)
    }

    #[test]
    fn a_write_reports_its_number_and_what_a_removal_took_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let first = write(root, "message", r#"{"author":"user","text":"um"}"#);
        assert_eq!(
            first,
            json!({"ok": true, "spec": "teste", "id": 1, "type": "message", "code": "MSTD-MSG-0001"})
        );
        write(root, "message", r#"{"author":"user","text":"dois"}"#);
        let removal = write(root, "remove", r#"{"targets":[1,2],"reason":"engano"}"#);
        assert_eq!(removal["removed"], json!([1, 2]), "{removal}");
        assert!(root.join(".claude").join("spec").join("teste").join("spec.ndjson").is_file());
    }

    /// Cada gravação refaz a página e o `.md` da spec. Uma decisão revista
    /// mostra só a versão nova fora da conversa, onde a antiga aparece
    /// marcada como substituída; um item removido some dos dois e continua
    /// no arquivo de eventos, com o motivo.
    #[test]
    fn every_write_rebuilds_the_page_and_the_md() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"decida"}"#);
        write(root, "decision", r#"{"text":"Texto antigo.","keys":["k"],"why":"w","origin":1}"#);
        let revised =
            write(root, "decision", r#"{"text":"Texto novo.","keys":["k"],"why":"w","origin":1,"replaces":2}"#);
        assert_eq!(revised["code"], json!("MSTD-DEC-0001"), "the new version keeps the code");
        write(root, "note", r#"{"text":"Anotação que sai.","keys":["n"],"origin":1}"#);
        let removal = write(root, "remove", r#"{"targets":[4],"reason":"engano"}"#);
        assert!(removal.get("warnings").is_none(), "{removal}");

        let spec = root.join(".claude").join("spec").join("teste");
        let md = std::fs::read_to_string(spec.join("spec.md")).unwrap();
        let html = std::fs::read_to_string(spec.join("spec.html")).unwrap();
        let (html_before, html_talk) = html.split_once("<section id=\"conversation\">").unwrap();
        let (md_before, md_talk) = md.rsplit_once("\n## ").unwrap();
        for (before, talk) in [(html_before, html_talk), (md_before, md_talk)] {
            assert!(before.contains("Texto novo.") && !before.contains("Texto antigo."), "{before}");
            assert!(talk.contains("Texto antigo."), "{talk}");
            assert!(!before.contains("Anotação que sai.") && !talk.contains("Anotação que sai."));
        }
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert!(events.contains("Anotação que sai.") && events.contains("engano"), "{events}");
    }

    /// Remover pelo código que a página mostra tira o item da leitura, da
    /// página e do `.md`, e ele continua no arquivo com o motivo. Um código
    /// que não existe é recusado citando o código, e nada é gravado.
    #[test]
    fn removing_by_the_code_takes_the_item_out_of_the_reading_the_page_and_the_md() {
        use crate::commands::spec_events::read::{read_at, ReadOpts};
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"combine as regras"}"#);
        for text in ["Regra um.", "Regra dois.", "Regra três."] {
            let json = json!({"text": text, "keys": ["k"], "example": "e", "origin": 1}).to_string();
            assert_eq!(write(root, "rule", &json)["ok"], json!(true));
        }
        let removal = write(root, "remove", r#"{"targets":["MSTD-RULE-0002"],"reason":"Regra repetida."}"#);
        assert_eq!(removal["removed"], json!([3]), "{removal}");
        assert!(removal.get("warnings").is_none(), "{removal}");

        let agreed = read_at(&ReadOpts {
            root: root.to_path_buf(),
            spec: Some("teste".into()),
            block: "agreed".into(),
            term: None,
        })
        .unwrap();
        assert!(!agreed.contains("Regra dois.") && agreed.contains("Regra três."), "{agreed}");
        let spec = root.join(".claude").join("spec").join("teste");
        for page in ["spec.md", "spec.html"] {
            let shown = std::fs::read_to_string(spec.join(page)).unwrap();
            assert!(!shown.contains("Regra dois."), "{page}: {shown}");
            assert!(shown.contains("Regra um.") && shown.contains("Regra três."), "{page}");
        }
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert!(events.contains("Regra dois.") && events.contains("Regra repetida."), "{events}");

        let unknown = write(root, "remove", r#"{"targets":["MSTD-RULE-0009"],"reason":"r"}"#);
        assert_eq!(unknown["reason"], json!("unknown-target"), "{unknown}");
        assert!(unknown["hint"].as_str().unwrap().contains("MSTD-RULE-0009"), "{unknown}");
        let revised = write(root, "rule", r#"{"text":"Regra três, revista.","keys":["k"],"example":"e","origin":1,"replaces":"MSTD-RULE-0003"}"#);
        assert_eq!(revised["code"], json!("MSTD-RULE-0003"), "{revised}");
        let with_code = write(root, "note", r#"{"text":"t","keys":["k"],"origin":1,"code":"MSTD-NOTE-0001"}"#);
        assert_eq!(with_code["reason"], json!("binary-only-field"), "{with_code}");
        let after = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert_eq!(after.lines().count(), events.lines().count() + 1, "only the revision was written");
    }

    /// Numa spec aberta pelo `spec-draft`, o `spec.md` é o documento do
    /// rascunho: gravar um evento não o refaz, e a página não nasce. O evento
    /// e a linha do índice são gravados do mesmo jeito.
    #[test]
    fn a_spec_drafted_by_spec_draft_keeps_its_md() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = root.join(".claude").join("spec").join("teste");
        std::fs::create_dir_all(&spec).unwrap();
        std::fs::write(spec.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();
        std::fs::write(spec.join("spec.md"), "# Rascunho\n\n## Contexto\n\nO texto da spec.\n").unwrap();

        let out = write(root, "state", r#"{"phase":"plan"}"#);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(
            std::fs::read_to_string(spec.join("spec.md")).unwrap(),
            "# Rascunho\n\n## Contexto\n\nO texto da spec.\n",
            "the draft's document is left alone",
        );
        assert!(!spec.join("spec.html").exists(), "no page over a draft");
        assert!(std::fs::read_to_string(spec.join("spec.ndjson")).unwrap().contains("\"plan\""));
        let index = std::fs::read_to_string(root.join(".claude").join("spec").join("index.ndjson")).unwrap();
        assert!(index.contains("\"teste\""), "{index}");
    }

    /// A aprovação não se grava à mão: um `state` com a fase `approved` é
    /// recusado pelo `run write`, e nada é gravado; a porta da testemunha, o
    /// `record`, grava.
    #[test]
    fn an_approval_is_never_written_by_hand() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "state", r#"{"phase":"plan"}"#);
        let forged = write(
            root,
            "state",
            r#"{"phase":" approved","author":"user","witness":{"question":"Aprovar esta spec?","answer":"Aprovar"}}"#,
        );
        assert_eq!(forged["reason"], json!("approval-by-witness-only"), "{forged}");
        assert!(forged["hint"].as_str().unwrap().contains("teste"), "{forged}");
        let events = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        assert_eq!(std::fs::read_to_string(&events).unwrap().lines().count(), 1, "nothing was written");

        let draft = json!({
            "phase": "approved",
            "author": "user",
            "witness": { "question": "Aprovar esta spec?", "answer": "Aprovar" }
        });
        assert!(record(root, "teste", "state", draft.as_object().cloned().unwrap()).is_ok());
        assert_eq!(std::fs::read_to_string(&events).unwrap().lines().count(), 2);
    }

    #[test]
    fn what_is_not_a_json_object_is_refused() {
        let dir = tempdir().unwrap();
        for json in ["[1,2]", "{quebrado", "\"texto\""] {
            let out = write(dir.path(), "note", json);
            assert_eq!(out["reason"], json!("not-an-object"), "{json}: {out}");
        }
        assert!(!dir.path().join(".claude").exists(), "a refusal writes nothing");
    }

    /// Um tipo que não existe e um campo que falta são recusados pelo nome; um
    /// tipo da spec sem `--spec` pede a spec, e nada é gravado.
    #[test]
    fn an_unknown_type_and_a_missing_field_are_refused_by_name() {
        let dir = tempdir().unwrap();
        let unknown = write(dir.path(), "licao", r#"{"text":"x"}"#);
        assert_eq!(unknown["reason"], json!("unknown-type"));
        assert!(unknown["hint"].as_str().unwrap().contains("licao"));
        let missing = write(dir.path(), "rule", r#"{"text":"t","keys":["k"],"origin":1}"#);
        assert_eq!(missing["reason"], json!("missing-field"));
        assert!(missing["hint"].as_str().unwrap().contains("example"));
        let no_spec = write_to(dir.path(), None, "rule", r#"{"text":"t","keys":["k"],"example":"e","origin":1}"#);
        assert_eq!(no_spec["reason"], json!("spec-required"), "{no_spec}");
        assert!(no_spec["hint"].as_str().unwrap().contains("--spec"), "{no_spec}");
        let unknown_no_spec = write_to(dir.path(), None, "licao", "{}");
        assert_eq!(unknown_no_spec["reason"], json!("unknown-type"), "{unknown_no_spec}");
        assert!(!dir.path().join(".claude").exists(), "a refusal writes nothing");
    }

    /// A lição vai para o banco de lições, com a spec do `--spec` dizendo
    /// onde ela nasceu; o arquivo de eventos, a página, o `.md` e o índice
    /// ficam como estavam. Sem `--spec`, a lição diz sozinha onde nasceu.
    #[test]
    fn writing_a_lesson_goes_to_the_bank_and_leaves_the_spec_untouched() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"um"}"#);
        let specs = root.join(".claude").join("spec");
        let files = [specs.join("teste").join("spec.ndjson"), specs.join("teste").join("spec.md"), specs.join("teste").join("spec.html"), specs.join("index.ndjson")];
        let before: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();

        let lesson = r#"{"class":"defect","text":"Um rm -rf na pasta errada perde trabalho.","keys":["apagar","rm"],"applies_to":{"subproject":"apps/rt"}}"#;
        assert_eq!(write(root, "lesson", lesson), json!({"ok": true, "id": 1, "type": "lesson", "class": "defect"}));
        let after: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();
        assert!(before == after, "the spec's files did not move");
        let bank = std::fs::read_to_string(specs.join("lessons.ndjson")).unwrap();
        assert!(bank.contains(r#""found_in":{"spec":"teste"}"#) && bank.contains(r#""type":"defect""#), "{bank}");

        let everywhere = r#"{"class":"user_preference","text":"Resposta curta.","keys":["resposta"],"applies_to":{"files":["**"]},"found_in":{"source":"CLAUDE.md"}}"#;
        let second = write_to(root, None, "lesson", everywhere);
        assert_eq!(second["id"], json!(2), "{second}");
        let no_origin = r#"{"class":"defect","text":"t","keys":["k"],"applies_to":{"skill":"s"}}"#;
        let refused = write_to(root, None, "lesson", no_origin);
        assert_eq!(refused["reason"], json!("lesson-origin-missing"), "{refused}");
        assert_eq!(std::fs::read_to_string(specs.join("lessons.ndjson")).unwrap().lines().count(), 2);
    }

    /// O `search` é gravado no arquivo de eventos e nunca aparece na página
    /// nem no `.md`: os dois mostram só o texto original.
    #[test]
    fn the_page_and_the_md_never_show_the_search_field() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"combine"}"#);
        let rule = r#"{"text":"Apagando a pasta, a trava barra o comando.","keys":["apagar","trava"],"example":"rm -rf pasta","origin":1}"#;
        assert_eq!(write(root, "rule", rule)["ok"], json!(true));
        let spec = root.join(".claude").join("spec").join("teste");
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        let line = events.lines().find(|l| l.contains("\"type\":\"rule\"")).unwrap();
        let search = serde_json::from_str::<Value>(line).unwrap()["search"].as_str().unwrap().to_string();
        assert!(search.contains(' '), "{search}");
        for page in ["spec.md", "spec.html"] {
            let shown = std::fs::read_to_string(spec.join(page)).unwrap();
            assert!(shown.contains("Apagando a pasta, a trava barra o comando."), "{page}");
            assert!(!shown.contains(&search) && !shown.contains("\"search\""), "{page} shows the search field");
        }
    }
}
