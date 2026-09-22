//! `map` — perguntas curtas ao mapa do projeto que o scan grava.
//!
//! `mustard-rt run map <pergunta>`:
//! - `examples --file <alvo>` (ou `--task "<tarefa>"`): 2 ou 3 arquivos que
//!   servem de exemplo, com o motivo de cada um e as receitas do git;
//! - `importers --file <arquivo>`: quem importa o arquivo;
//! - `tests --file <arquivo>`: que testes o cobrem;
//! - `slice --file <arquivo> --name <declaração>`: o trecho da declaração, do
//!   começo ao fim, com o caminho e as linhas de onde ele saiu;
//! - `search --query "<palavras>"`: a busca por conceito;
//! - `summary`: o resumo do início da sessão, até 3 kB;
//! - `skill --path <SKILL.md>`: confere os caminhos que a skill cita e o
//!   tamanho dela.
//!
//! A regra mora em `mustard_core::domain::project_map`; aqui só se leem o
//! mapa, a skill e o arquivo de onde sai o trecho, e se imprime o JSON.

use std::path::{Path, PathBuf};

use clap::ValueEnum;
use mustard_core::domain::project_map::{self as project_map, MapRefusal, ProjectMap};
use mustard_core::io::project_map as store;
use mustard_core::platform::i18n::Locale;
use serde_json::{json, Value};

/// A pergunta feita ao mapa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Question {
    Examples,
    Importers,
    Tests,
    Search,
    Summary,
    Skill,
    Slice,
}

impl Question {
    fn name(self) -> &'static str {
        match self {
            Self::Examples => "examples",
            Self::Importers => "importers",
            Self::Tests => "tests",
            Self::Search => "search",
            Self::Summary => "summary",
            Self::Skill => "skill",
            Self::Slice => "slice",
        }
    }
}

/// As opções de `mustard-rt run map`.
pub struct MapOpts {
    pub root: PathBuf,
    pub question: Question,
    pub file: Option<String>,
    pub task: Option<String>,
    pub query: Option<String>,
    pub path: Option<PathBuf>,
    pub name: Option<String>,
}

fn refused(refusal: &MapRefusal, lang: Locale) -> Value {
    json!({ "ok": false, "reason": refusal.reason(), "hint": refusal.message(lang) })
}

/// O valor de uma opção que a pergunta exige, ou a recusa que diz qual falta.
fn required(value: Option<&str>, question: Question, flag: &str) -> Result<String, MapRefusal> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| MapRefusal::MissingArgument { question: question.name().to_string(), flag: flag.to_string() })
}

/// Responde a pergunta e devolve o JSON; nunca entra em pânico.
pub(crate) fn map_at(opts: &MapOpts) -> Value {
    let project = crate::commands::spec_events::project(&opts.root);
    let lang = project.lang;
    match answer(opts, &project.root, lang) {
        Ok(report) => report,
        Err(refusal) => refused(&refusal, lang),
    }
}

fn answer(opts: &MapOpts, root: &Path, lang: Locale) -> Result<Value, MapRefusal> {
    let question = opts.question;
    if question == Question::Skill {
        return skill(opts, root);
    }
    let map = store::read(root)?;
    match question {
        Question::Importers => {
            let file = required(opts.file.as_deref(), question, "--file")?;
            let importers = project_map::importers(&map, &file)?;
            Ok(json!({ "ok": true, "question": "importers", "file": project_map::clean_path(&file), "importers": importers }))
        }
        Question::Tests => {
            let file = required(opts.file.as_deref(), question, "--file")?;
            let tests = project_map::tests_for(&map, &file)?;
            Ok(json!({
                "ok": true,
                "question": "tests",
                "file": project_map::clean_path(&file),
                "inline": tests.inline,
                "tests": tests.files,
            }))
        }
        Question::Search => {
            let query = required(opts.query.as_deref(), question, "--query")?;
            let files: Vec<Value> = project_map::search(&map, &query)
                .into_iter()
                .map(|found| json!({ "path": found.path, "score": found.score }))
                .collect();
            Ok(json!({ "ok": true, "question": "search", "query": query, "files": files }))
        }
        Question::Summary => {
            let text = project_map::summary(&map, lang);
            Ok(json!({ "ok": true, "question": "summary", "bytes": text.len(), "summary": text }))
        }
        Question::Slice => slice(opts, &map, root),
        Question::Examples => examples(opts, &map, lang),
        Question::Skill => skill(opts, root),
    }
}

/// O trecho da declaração de `--name` no arquivo de `--file`: as linhas dela,
/// do começo ao fim, mais o caminho e as linhas de onde saíram. O mapa diz
/// onde a declaração mora; o arquivo é lido aqui, uma vez, para que quem
/// pergunta não precise abri-lo.
fn slice(opts: &MapOpts, map: &ProjectMap, root: &Path) -> Result<Value, MapRefusal> {
    let question = opts.question;
    let file = required(opts.file.as_deref(), question, "--file")?;
    let name = required(opts.name.as_deref(), question, "--name")?;
    let place = project_map::declaration(map, &file, &name)?;
    let text = std::fs::read_to_string(root.join(&place.file))
        .map_err(|e| MapRefusal::FileUnreadable { file: place.file.clone(), detail: e.to_string() })?;
    Ok(json!({
        "ok": true,
        "question": "slice",
        "file": place.file,
        "name": place.name,
        "kind": place.kind,
        "line": place.line,
        "end_line": place.end_line,
        "doc": place.doc,
        "signature": place.signature,
        "slice": project_map::lines_of(&text, place.line, place.end_line),
    }))
}

/// Os exemplos para o alvo de `--file`; sem ele, para a pasta do arquivo que
/// a busca acha para `--task`.
fn examples(opts: &MapOpts, map: &ProjectMap, lang: Locale) -> Result<Value, MapRefusal> {
    let file = opts.file.as_deref().map(str::trim).filter(|f| !f.is_empty());
    let task = opts.task.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let target = match (file, task) {
        (Some(file), _) => project_map::clean_path(file),
        (None, Some(task)) => match project_map::best_folder(map, task) {
            Some(folder) => folder,
            None => {
                return Ok(json!({
                    "ok": true,
                    "question": "examples",
                    "task": task,
                    "examples": [],
                    "recipes": [],
                    "note": mustard_core::translate("map.no_target", lang),
                }));
            }
        },
        (None, None) => {
            return Err(MapRefusal::MissingArgument { question: "examples".to_string(), flag: "--file".to_string() });
        }
    };
    let got = project_map::examples(map, &target, lang);
    let picks: Vec<Value> = got
        .picks
        .iter()
        .map(|p| {
            json!({
                "path": p.path,
                "loc": p.loc,
                "why": p.why,
                "shared_imports": p.shared_imports,
                "tests": p.tests,
                "inline_tests": p.inline_tests,
                "last_change": p.last_change,
            })
        })
        .collect();
    let recipes: Vec<Value> = got
        .recipes
        .iter()
        .map(|r| json!({ "commit": r.commit, "date": r.date, "added": r.added, "together": r.together }))
        .collect();
    let mut report = json!({
        "ok": true,
        "question": "examples",
        "target": target,
        "folder": got.folder,
        "main_imports": got.main_imports,
        "examples": picks,
        "recipes": recipes,
    });
    if got.picks.is_empty() {
        report["note"] = json!(mustard_core::translate("map.no_examples", lang).replace("{folder}", &got.folder));
    }
    Ok(report)
}

/// Os arquivos que o mapa sugere para uma tarefa descrita em palavras, do
/// mais forte para o menos forte. Vazio quando não há mapa gravado ou quando
/// nada casa: quem pergunta decide o que fazer com a lista, porque o mapa não
/// preenche a tarefa sozinho.
pub(crate) fn suggested_files(root: &Path, task: &str, limit: usize) -> Vec<String> {
    let Ok(map) = store::read(root) else { return Vec::new() };
    project_map::search(&map, task).into_iter().take(limit).map(|found| found.path).collect()
}

/// A pasta que a skill descreve: a de cima do `.claude` onde ela mora.
fn skill_owner(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|a| a.file_name().is_some_and(|n| n == ".claude")).and_then(Path::parent).map(Path::to_path_buf)
}

/// Confere a skill de `--path`: cada caminho citado existe (na raiz do
/// projeto, na pasta da skill ou no mapa) e o texto cabe no limite.
fn skill(opts: &MapOpts, root: &Path) -> Result<Value, MapRefusal> {
    let given = opts.path.as_deref().ok_or_else(|| MapRefusal::MissingArgument {
        question: "skill".to_string(),
        flag: "--path".to_string(),
    })?;
    let path = std::path::absolute(given).unwrap_or_else(|_| given.to_path_buf());
    let shown = given.to_string_lossy().replace('\\', "/");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| MapRefusal::SkillUnreadable { path: shown.clone(), detail: e.to_string() })?;
    let map = store::read(root).ok();
    let owner = skill_owner(&path);
    let exists = |cited: &str| {
        root.join(cited).exists()
            || owner.as_ref().is_some_and(|o| o.join(cited).exists())
            || map.as_ref().is_some_and(|m| project_map::map_knows(m, cited))
    };
    project_map::check_skill(&text, exists)?;
    Ok(json!({
        "ok": true,
        "question": "skill",
        "path": shown,
        "lines": text.lines().count(),
        "cited": project_map::cited_paths(&text),
    }))
}

/// Imprime a resposta e sai com 1 na recusa.
pub fn run(opts: &MapOpts) {
    let report = map_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Um mapa pequeno: uma pasta de comandos com quatro arquivos que
    /// importam o mesmo núcleo, um teste que cobre um deles e um histórico.
    const MODEL: &str = r#"{
      "modules": [
        {"path": "apps/rt/src/commands/pay/read.rs", "loc": 100, "declarations": [{"name": "run"}],
         "deps": ["packages/core/src/pay.rs"], "has_tests": true},
        {"path": "apps/rt/src/commands/pay/write.rs", "loc": 120, "declarations": [{"name": "run"}],
         "deps": ["packages/core/src/pay.rs"], "tests": ["apps/rt/tests/pay_cli.rs"]},
        {"path": "apps/rt/src/commands/pay/index.rs", "loc": 90, "declarations": [{"name": "run"}],
         "deps": ["packages/core/src/pay.rs"]},
        {"path": "apps/rt/src/commands/pay/cli.rs", "loc": 3000, "declarations": [{"name": "dispatch"}],
         "deps": ["packages/core/src/pay.rs"]},
        {"path": "packages/core/src/pay.rs", "loc": 200, "declarations": [{"name": "Payment"}]},
        {"path": "apps/rt/tests/pay_cli.rs", "loc": 80, "deps": ["apps/rt/src/commands/pay/write.rs"]}
      ],
      "history": {
        "paths": ["apps/rt/src/commands/pay/index.rs", "apps/rt/src/commands/pay/read.rs",
                  "apps/rt/src/commands/pay/write.rs", "apps/rt/tests/run_command_surface.rs"],
        "commits": [
          {"id": "c1", "at": 86400, "added": [0, 3]},
          {"id": "c2", "at": 172800, "added": [1], "changed": [3]},
          {"id": "c3", "at": 259200, "changed": [2]}
        ]
      }
    }"#;

    fn project_with_map() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(store::model_path(dir.path()), MODEL).unwrap();
        dir
    }

    fn ask(root: &Path, question: Question) -> MapOpts {
        MapOpts { root: root.to_path_buf(), question, file: None, task: None, query: None, path: None, name: None }
    }

    #[test]
    fn a_task_asking_for_examples_gets_two_or_three_from_the_same_folder_with_the_reason() {
        let dir = project_with_map();
        let mut opts = ask(dir.path(), Question::Examples);
        opts.task = Some("adicionar um comando run".to_string());
        let report = map_at(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["folder"], json!("apps/rt/src/commands/pay"));
        let picks = report["examples"].as_array().unwrap();
        assert!((2..=3).contains(&picks.len()), "{report}");
        for pick in picks {
            assert!(pick["path"].as_str().unwrap().starts_with("apps/rt/src/commands/pay/"), "{pick}");
            assert_eq!(pick["shared_imports"], json!(["packages/core/src/pay.rs"]), "{pick}");
            assert!(!pick["why"].as_array().unwrap().is_empty(), "{pick}");
            assert_ne!(pick["path"], json!("apps/rt/src/commands/pay/cli.rs"), "the oversized file is left out");
        }
        // Com teste e mais recente primeiro.
        assert_eq!(picks[0]["path"], json!("apps/rt/src/commands/pay/write.rs"));
        assert_eq!(picks[1]["path"], json!("apps/rt/src/commands/pay/read.rs"));
        assert_eq!(report["recipes"][0]["added"], json!("apps/rt/src/commands/pay/read.rs"));
    }

    #[test]
    fn importers_tests_search_and_summary_answer_from_the_map() {
        let dir = project_with_map();
        let mut opts = ask(dir.path(), Question::Importers);
        opts.file = Some("packages/core/src/pay.rs".to_string());
        let report = map_at(&opts);
        assert_eq!(report["importers"].as_array().unwrap().len(), 4, "{report}");

        let mut opts = ask(dir.path(), Question::Tests);
        opts.file = Some("apps/rt/src/commands/pay/write.rs".to_string());
        assert_eq!(map_at(&opts)["tests"], json!(["apps/rt/tests/pay_cli.rs"]));

        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("pagamento".to_string());
        let report = map_at(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");

        let report = map_at(&ask(dir.path(), Question::Summary));
        assert!(report["bytes"].as_u64().unwrap() <= 3 * 1024, "{report}");
    }

    #[test]
    fn a_question_without_its_option_or_without_a_map_is_refused_by_name() {
        let dir = tempdir().unwrap();
        let report = map_at(&ask(dir.path(), Question::Summary));
        assert_eq!(report["reason"], json!("map-missing"));
        assert!(report["hint"].as_str().unwrap().contains("mustard-rt run scan"));

        let dir = project_with_map();
        let report = map_at(&ask(dir.path(), Question::Importers));
        assert_eq!(report["reason"], json!("missing-argument"));
        assert!(report["hint"].as_str().unwrap().contains("--file"));

        let mut opts = ask(dir.path(), Question::Tests);
        opts.file = Some("nao/existe.rs".to_string());
        assert_eq!(map_at(&opts)["reason"], json!("unknown-file"));
    }

    /// O trecho de uma declaração vem do mapa mais o arquivo: quem pergunta
    /// não abre o arquivo, e recebe as linhas da declaração, do começo ao fim,
    /// com o caminho e as linhas de onde saíram.
    #[test]
    fn o_mapa_devolve_o_trecho_de_uma_declaracao() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let file = "packages/core/src/pay.rs";
        std::fs::create_dir_all(dir.path().join("packages/core/src")).unwrap();
        std::fs::write(
            dir.path().join(file),
            "// topo do arquivo\n\
             /// Soma o preço do pedido com o frete.\n\
             pub fn total(preco: u32, frete: u32) -> u32 {\n    \
                 preco + frete\n\
             }\n\
             // depois\n",
        )
        .unwrap();
        std::fs::write(
            store::model_path(dir.path()),
            format!(
                r#"{{"modules": [{{"path": "{file}", "loc": 6, "declarations": [
                     {{"kind": "function", "name": "total", "line": 3, "end_line": 5,
                      "doc": "Soma o preço do pedido com o frete.",
                      "signature": "pub fn total(preco: u32, frete: u32) -> u32"}}]}}]}}"#
            ),
        )
        .unwrap();

        let mut opts = ask(dir.path(), Question::Slice);
        opts.file = Some(file.to_string());
        opts.name = Some("total".to_string());
        let report = map_at(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(
            report["slice"],
            json!("pub fn total(preco: u32, frete: u32) -> u32 {\n    preco + frete\n}"),
            "da primeira à última linha da declaração, e nada em volta: {report}"
        );
        assert_eq!(report["file"], json!(file), "{report}");
        assert_eq!(report["line"], json!(3), "{report}");
        assert_eq!(report["end_line"], json!(5), "{report}");
        assert_eq!(report["name"], json!("total"), "{report}");
        assert_eq!(report["doc"], json!("Soma o preço do pedido com o frete."), "{report}");
        assert_eq!(report["signature"], json!("pub fn total(preco: u32, frete: u32) -> u32"), "{report}");

        // Sem o nome da declaração, e com um nome que o arquivo não declara,
        // a recusa diz qual é o caso.
        let mut sem_nome = ask(dir.path(), Question::Slice);
        sem_nome.file = Some(file.to_string());
        let report = map_at(&sem_nome);
        assert_eq!(report["reason"], json!("missing-argument"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("--name"), "{report}");

        opts.name = Some("sumiu".to_string());
        let report = map_at(&opts);
        assert_eq!(report["reason"], json!("unknown-declaration"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("sumiu"), "{report}");
    }

    #[test]
    fn a_skill_citing_a_path_that_does_not_exist_is_refused() {
        let dir = project_with_map();
        let skill_dir = dir.path().join("apps").join("rt").join(".claude").join("skills").join("add-pay");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_path = skill_dir.join("SKILL.md");
        std::fs::write(&skill_path, "Veja `pay/write.rs` e `apps/rt/src/commands/pay/sumiu.rs`.\n").unwrap();
        let mut opts = ask(dir.path(), Question::Skill);
        opts.path = Some(skill_path.clone());
        let report = map_at(&opts);
        assert_eq!(report["reason"], json!("skill-missing-path"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("apps/rt/src/commands/pay/sumiu.rs"));

        std::fs::write(&skill_path, "Veja `pay/write.rs` e `commands/pay/index.rs`.\n").unwrap();
        let report = map_at(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
    }
}
