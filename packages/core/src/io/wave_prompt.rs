//! O pedido de cada onda, montado do disco: o arquivo de eventos da spec, o
//! banco de lições e os arquivos das skills que as tarefas nomeiam.
//!
//! A regra de escrever o pedido mora em `domain::wave_prompt`, sem disco;
//! aqui ficam só as leituras. Os dois leitores do pedido — o passo do plano,
//! que confere antes da pergunta de aprovação, e a página, que mostra cada
//! linha dele — passam por esta função, então não podem discordar sobre o
//! mesmo pedido.
//!
//! O pedido não copia o texto da skill: ele recomenda o arquivo dela, que o
//! agente da onda lê no disco. O texto ainda é lido aqui porque é nele que a
//! conferência acontece — a skill que cita um caminho que não existe ou passa
//! do tamanho máximo é recusada — e porque o "quando usar" da linha sai da
//! descrição do frontmatter. A skill cujo exemplo mudou no git depois dela
//! sai marcada como a revisar.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::domain::lessons::{in_scope, Scope};
use crate::domain::project_map::{check_skill, file_history, MapRefusal, ProjectMap};
use crate::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog, Step};
use crate::domain::wave_prompt::{self, Material, Skill};
use crate::platform::i18n::Locale;

/// O pedido de uma onda, como o disco o entrega.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavePrompt {
    /// O número da onda.
    pub wave: u64,
    /// O texto do pedido, sempre: a página mostra mesmo o pedido recusado,
    /// que é justamente o que precisa ser visto antes da aprovação.
    pub text: String,
    /// O pedido do revisor da onda: a mesma lista de itens e os critérios que
    /// ele confere, mais os defeitos já vistos nos arquivos da onda.
    pub review: String,
    /// Quantas linhas ele tem.
    pub lines: usize,
    /// A recusa do teto de linhas, quando o pedido passa dele.
    pub too_long: Option<Refusal>,
    /// As skills que a conferência recusou, pelo nome.
    pub bad_skills: Vec<(String, MapRefusal)>,
    /// As skills que a onda usa e que precisam de revisão, pelo nome.
    pub stale_skills: Vec<String>,
}

/// Os pedidos de todas as ondas do plano, em ordem de número.
#[must_use]
pub fn prompts(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Vec<WavePrompt> {
    let bank = crate::ClaudePaths::for_project(root)
        .ok()
        .and_then(|paths| crate::io::lessons::read(&paths.lessons_path()).ok().flatten());
    let map = crate::io::project_map::read(root).ok();
    waves_of(log).into_iter().map(|n| one(root, spec, log, n, bank.as_ref(), map.as_ref(), lang)).collect()
}

/// Os números das ondas do plano, em ordem.
fn waves_of(log: &SpecLog) -> Vec<u64> {
    let mut numbers: BTreeSet<u64> = BTreeSet::new();
    for event in log.block(BlockQuery::Block(Block::Waves)) {
        if event.event_type == "wave"
            && let Some(n) = event.wave()
        {
            numbers.insert(n);
        }
    }
    numbers.into_iter().collect()
}

fn one(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    wave: u64,
    bank: Option<&SpecLog>,
    map: Option<&ProjectMap>,
    lang: Locale,
) -> WavePrompt {
    let read = log.step(&Step::Dispatch { wave });
    let of_type = |name: &str| -> Vec<&SpecEvent> {
        read.iter().copied().filter(|e| e.event_type == name).collect()
    };
    let block: Vec<&SpecEvent> = read
        .iter()
        .copied()
        // O envio e o entregou são o REGISTRO de um pedido, não parte dele:
        // repeti-los dentro do pedido novo seria contar a mesma coisa duas
        // vezes. O entregou das ondas de que esta depende entra à parte.
        .filter(|e| e.block() == Some(Block::Waves) && !matches!(e.event_type.as_str(), "send" | "delivered"))
        .collect();
    let delivered: Vec<&SpecEvent> = read
        .iter()
        .copied()
        .filter(|e| e.event_type == "delivered" && e.wave() != Some(wave))
        .collect();
    let agreed: Vec<&SpecEvent> = read
        .iter()
        .copied()
        .filter(|e| e.block() == Some(Block::Agreed))
        .collect();
    let specification: Vec<&SpecEvent> = read
        .iter()
        .copied()
        .filter(|e| e.block() == Some(Block::Specification))
        .collect();

    let files = wave_files(log, wave);
    let named = skills_named(log, wave);
    let lessons = bank
        .map(|bank| {
            let mut found: Vec<&SpecEvent> = Vec::new();
            for skill in std::iter::once(None).chain(named.iter().map(|s| Some(s.clone()))) {
                let scope = Scope { files: files.clone(), subproject: None, skill };
                for lesson in in_scope(bank, &scope) {
                    if !found.iter().any(|seen| seen.id == lesson.id) {
                        found.push(lesson);
                    }
                }
            }
            found.sort_by_key(|lesson| lesson.id);
            found
        })
        .unwrap_or_default();

    // O revisor recebe os defeitos já vistos nestes arquivos, que o agente da
    // onda não recebe: é olhando o erro que já aconteceu ali que ele começa.
    let defects = bank
        .map(|bank| {
            let scope = Scope { files: files.clone(), subproject: None, skill: None };
            crate::domain::lessons::defects_in_scope(bank, &scope)
        })
        .unwrap_or_default();

    let mut skills = Vec::new();
    let mut bad_skills = Vec::new();
    let mut stale_skills = Vec::new();
    for name in &named {
        let Some((text, path)) = read_skill(root, &files, name) else {
            bad_skills.push((name.clone(), MapRefusal::SkillMissingPaths { paths: vec![skill_file(name)] }));
            continue;
        };
        if let Err(refusal) = check_skill(&text, |cited| cited_exists(root, cited)) {
            bad_skills.push((name.clone(), refusal));
            continue;
        }
        let stale = examples_changed_after(log, map, name);
        if stale {
            stale_skills.push(name.clone());
        }
        skills.push(Skill { name: name.clone(), when: when_to_use(&text), path, stale });
    }

    let material = Material {
        spec: spec.to_string(),
        wave,
        block,
        criteria: of_type("criterion"),
        specification,
        agreed,
        delivered,
        lessons,
        defects,
        skills,
        codes: log.codes(),
    };
    let text = wave_prompt::write(&material, lang);
    let review = wave_prompt::write_review(&material, lang);
    let lines = wave_prompt::count_lines(&text);
    let too_long =
        (lines > wave_prompt::MAX_LINES).then(|| wave_prompt::too_long(&material, lines, lang));
    WavePrompt { wave, text, review, lines, too_long, bad_skills, stale_skills }
}

/// Os caminhos que as tarefas de uma onda declaram, em ordem, sem repetir.
fn wave_files(log: &SpecLog, wave: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for task in log.block(BlockQuery::Wave(wave)).iter().filter(|e| e.event_type == "task") {
        let files = task.fields.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
        for file in files {
            let path = file.as_str().or_else(|| file.get("path").and_then(Value::as_str)).unwrap_or_default();
            if !path.is_empty() && !out.iter().any(|seen| seen == path) {
                out.push(path.to_string());
            }
        }
    }
    out
}

/// As skills que as tarefas de uma onda nomeiam, em ordem de nome.
fn skills_named(log: &SpecLog, wave: u64) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for task in log.block(BlockQuery::Wave(wave)).iter().filter(|e| e.event_type == "task") {
        if let Some(name) = task.str_field("skill").map(str::trim).filter(|s| !s.is_empty()) {
            names.insert(name.to_string());
        }
    }
    names.into_iter().collect()
}

/// O caminho de uma skill dentro do subprojeto que a tem.
fn skill_file(name: &str) -> String {
    format!(".claude/skills/{name}/SKILL.md")
}

/// O arquivo de uma skill, procurado nas pastas dos arquivos da onda e, por
/// último, na raiz do projeto: a skill mora no subprojeto em que a tarefa
/// mexe. Devolve o texto, que a conferência lê, e o caminho do arquivo a
/// partir da raiz do projeto, que é o que o pedido recomenda.
fn read_skill(root: &Path, files: &[String], name: &str) -> Option<(String, String)> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for file in files {
        let mut folder = root.join(file);
        while folder.pop() && folder.starts_with(root) {
            if !folders.contains(&folder) {
                folders.push(folder.clone());
            }
        }
    }
    if !folders.contains(&root.to_path_buf()) {
        folders.push(root.to_path_buf());
    }
    for folder in folders {
        let path = folder.join(".claude").join("skills").join(name).join("SKILL.md");
        if let Ok(text) = std::fs::read_to_string(&path) {
            return Some((text, from_root(root, &path)));
        }
    }
    None
}

/// Um caminho a partir da raiz do projeto, sempre com barras normais: o
/// pedido é lido por gente e por agente, e a barra invertida do Windows não
/// serve para nenhum dos dois.
fn from_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// Quando usar a skill, tirado da descrição do frontmatter dela e reduzido a
/// uma linha. A skill sem frontmatter, ou com a descrição vazia, entra no
/// pedido só com o nome e o caminho.
fn when_to_use(text: &str) -> String {
    let Ok(front) = crate::domain::skill::frontmatter::parse(text) else {
        return String::new();
    };
    front.description.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Um caminho citado pela skill existe? A skill pode citar só o fim do
/// caminho, então a busca é pelo que o projeto tem.
fn cited_exists(root: &Path, cited: &str) -> bool {
    if root.join(cited).exists() {
        return true;
    }
    crate::io::project_map::read(root)
        .is_ok_and(|map| map.modules.iter().any(|m| m.path.ends_with(cited)))
}

/// Algum exemplo que a skill usou mudou no git depois de ela ter sido
/// gravada?
fn examples_changed_after(log: &SpecLog, map: Option<&ProjectMap>, name: &str) -> bool {
    let Some(map) = map else { return false };
    let Some(event) = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "skill" && e.str_field("name") == Some(name))
    else {
        return false;
    };
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(event.at()) else {
        return false;
    };
    let written = at.timestamp();
    event
        .fields
        .get("examples")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|example| example.get("path").and_then(Value::as_str))
        .any(|path| file_history(&map.history, path).is_some_and(|h| h.last_at > written))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{parse_log, render_line, stamp};
    use serde_json::json;
    use tempfile::tempdir;

    fn log_of(events: &[(&str, Value)]) -> SpecLog {
        let mut content = String::new();
        for (i, (event_type, body)) in events.iter().enumerate() {
            let mut map = crate::domain::spec_events::normalize(
                body.as_object().cloned().unwrap_or_default(),
                event_type,
            );
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, i as u64 + 1, None, "2026-09-15T10:00:00-03:00")));
            content.push('\n');
        }
        parse_log(&content)
    }

    fn plan_log() -> SpecLog {
        log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            (
                "task",
                json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}], "skill": "somar"}),
            ),
        ])
    }

    fn write_skill(root: &Path, subproject: &str, name: &str, text: &str) {
        let dir = root.join(subproject).join(".claude").join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), text).unwrap();
    }

    /// A skill que a tarefa nomeia entra no pedido pelo caminho do arquivo e
    /// pelo quando usar da descrição dela, sem o texto; e ela é procurada no
    /// subprojeto em que a tarefa mexe.
    #[test]
    fn the_skill_a_task_names_reaches_the_request_by_path_and_never_by_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_skill(
            root,
            "apps/rt",
            "somar",
            "---\nname: somar\ndescription: Use ao somar dois números no motor.\n---\n\n# Somar\n\nUm passo por linha.\n",
        );
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr);
        assert_eq!(built.len(), 1);
        assert!(built[0].bad_skills.is_empty(), "{:?}", built[0].bad_skills);
        assert!(
            built[0].text.contains("`apps/rt/.claude/skills/somar/SKILL.md`"),
            "{}",
            built[0].text
        );
        assert!(built[0].text.contains("Use ao somar dois números no motor."), "{}", built[0].text);
        assert!(!built[0].text.contains("Um passo por linha."), "{}", built[0].text);
    }

    /// A skill sem frontmatter entra mesmo assim: a linha fica só com o nome e
    /// o caminho, sem o trecho do quando usar.
    #[test]
    fn a_skill_without_frontmatter_still_reaches_the_request_by_path() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_skill(root, "apps/rt", "somar", "# Somar\n\nUm passo por linha.\n");
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr);
        assert!(built[0].bad_skills.is_empty(), "{:?}", built[0].bad_skills);
        assert!(
            built[0].text.contains("- **somar** — `apps/rt/.claude/skills/somar/SKILL.md`"),
            "{}",
            built[0].text
        );
    }

    /// A skill que cita um caminho que não existe, e a que passa do tamanho
    /// máximo, são recusadas, e o texto delas não entra no pedido.
    #[test]
    fn a_skill_citing_a_missing_path_or_too_long_is_refused() {
        let long = format!("# O molde\n\n{}", "linha do molde\n".repeat(600));
        for (text, reason) in [
            ("# O molde\n\nVeja `apps/rt/src/nao-existe.rs`.\n", "skill-missing-path"),
            (long.as_str(), "skill-too-long"),
        ] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            write_skill(root, "apps/rt", "somar", text);
            let built = prompts(root, "teste", &plan_log(), Locale::PtBr);
            assert_eq!(built[0].bad_skills.len(), 1, "{reason}");
            assert_eq!(built[0].bad_skills[0].0, "somar");
            assert_eq!(built[0].bad_skills[0].1.reason(), reason);
            assert!(!built[0].bad_skills[0].1.message(Locale::EnUs).is_empty());
            assert!(!built[0].text.contains("O molde"), "{reason}: a skill recusada não entra");
        }
    }

    /// O pedido recomenda exatamente as skills que as tarefas nomeiam. Uma
    /// skill que está na prateleira do subprojeto e que nenhuma tarefa nomeia
    /// não entra: a escolha é sempre pela tarefa, nunca pelo caminho.
    #[test]
    fn only_the_skills_the_tasks_name_reach_the_request() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_skill(root, "apps/rt", "somar", "# Somar\n\nUm passo por linha.\n");
        write_skill(root, "apps/rt", "subtrair", "# Subtrair\n\nOutro molde da mesma pasta.\n");
        let built = prompts(root, "teste", &plan_log(), Locale::PtBr);
        assert!(built[0].text.contains("skills/somar/SKILL.md"), "{}", built[0].text);
        assert!(!built[0].text.contains("skills/subtrair/SKILL.md"), "{}", built[0].text);
        assert!(!built[0].text.contains("MOLDS FOR THIS WAVE"), "{}", built[0].text);
    }

    /// Um item revisto entra na lista do pedido só na versão nova; a linha da
    /// antiga fica fora, e o número dela não aparece.
    #[test]
    fn a_revised_item_reaches_the_request_only_in_its_new_version() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_skill(root, "apps/rt", "somar", "# Somar\n\nUm passo por linha.\n");
        let log = log_of(&[
            ("limit", json!({"text": "O pedido cabe em 400 linhas.", "value": "400 linhas", "keys": ["pedido"]})),
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}], "skill": "somar"})),
            ("limit", json!({"text": "O pedido cabe em 500 linhas.", "value": "500 linhas", "keys": ["pedido"], "replaces": 1})),
        ]);
        let built = prompts(root, "teste", &log, Locale::PtBr);
        assert!(built[0].text.contains("--term MSTD-LIMIT-0001"), "{}", built[0].text);
        assert_eq!(built[0].text.matches("MSTD-LIMIT-0001").count(), 2, "{}", built[0].text);
        assert!(!built[0].text.contains("linhas."), "nenhum texto de item entra: {}", built[0].text);
    }

    /// O pedido do revisor de uma onda leva os defeitos já vistos nos
    /// arquivos dela; o pedido do agente da onda não os leva.
    #[test]
    fn the_reviewer_of_a_wave_gets_the_defects_already_seen_in_those_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let bank = root.join(".claude").join("spec");
        std::fs::create_dir_all(&bank).unwrap();
        let line = |id: u64, class: &str, text: &str, files: &str| {
            format!(
                r#"{{"v":1,"id":{id},"at":"2026-09-15T10:00:00-03:00","type":"{class}","author":"assistant","text":"{text}","keys":["k"],"applies_to":{{"files":["{files}"]}},"found_in":{{"spec":"s"}}}}"#
            )
        };
        std::fs::write(
            bank.join("lessons.ndjson"),
            format!(
                "{}
{}
",
                line(1, "defect", "Apagar a pasta perde trabalho.", "apps/rt/src/**"),
                line(2, "project_rule", "O comentário vai em português.", "apps/rt/src/**")
            ),
        )
        .unwrap();
        let log = log_of(&[
            ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
            ("task", json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}]})),
        ]);

        let built = prompts(root, "teste", &log, Locale::PtBr);
        let heading = crate::platform::i18n::translate("prompt.part.defects", Locale::PtBr);
        let (before, defects) = built[0].review.split_once(heading).expect("a seção dos defeitos");
        assert!(defects.contains("Apagar a pasta perde trabalho."), "{defects}");
        assert!(!defects.contains("O comentário vai em português."), "só defeito entra: {defects}");
        assert!(!before.contains("Apagar a pasta"), "{before}");
        assert!(!built[0].text.contains(heading), "o pedido da onda não tem a seção: {}", built[0].text);
    }

    /// A skill cujo arquivo de exemplo mudou no git depois dela sai marcada
    /// como a revisar; a que não mudou não sai marcada.
    #[test]
    fn a_skill_whose_example_changed_after_it_is_marked_for_review() {
        let written = chrono::DateTime::parse_from_rfc3339("2026-09-15T10:00:00-03:00").unwrap().timestamp();
        for (moved, marked) in [(written + 3_600, true), (written - 3_600, false)] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            write_skill(root, "apps/rt", "somar", "# O molde\n\nUm passo por linha.\n");
            std::fs::create_dir_all(root.join(".claude")).unwrap();
            std::fs::write(
                crate::io::project_map::model_path(root),
                json!({
                    "history": {
                        "paths": ["apps/rt/src/exemplo.rs"],
                        "commits": [{"id": "abc1234", "at": moved, "changed": [0]}],
                    }
                })
                .to_string(),
            )
            .unwrap();
            let mut events = vec![
                ("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})),
                (
                    "task",
                    json!({"wave": 1, "text": "Somar", "files": [{"path": "apps/rt/src/a.rs"}], "skill": "somar"}),
                ),
            ];
            events.push((
                "skill",
                json!({"name": "somar", "action": "create", "text": "O molde", "sha": "3f9a1c2e",
                       "examples": [{"path": "apps/rt/src/exemplo.rs", "why": "mesma pasta"}]}),
            ));
            let built = prompts(root, "teste", &log_of(&events), Locale::PtBr);
            assert!(built[0].bad_skills.is_empty(), "{:?}", built[0].bad_skills);
            assert_eq!(built[0].stale_skills.is_empty(), !marked, "mudou em {moved}");
            let review = crate::platform::i18n::translate("prompt.skill.stale", Locale::PtBr);
            assert_eq!(built[0].text.contains(&format!("**somar** ({review})")), marked);
            assert!(built[0].text.contains("skills/somar/SKILL.md"), "{}", built[0].text);
        }
    }

    /// A skill que a tarefa nomeia e que não está no disco é recusada, com o
    /// caminho em que ela devia estar.
    #[test]
    fn a_named_skill_that_is_not_on_disk_is_refused_with_the_path() {
        let dir = tempdir().unwrap();
        let built = prompts(dir.path(), "teste", &plan_log(), Locale::PtBr);
        assert_eq!(built[0].bad_skills.len(), 1);
        assert_eq!(built[0].bad_skills[0].1.reason(), "skill-missing-path");
        assert!(built[0].bad_skills[0].1.message(Locale::PtBr).contains("somar"));
    }
}
