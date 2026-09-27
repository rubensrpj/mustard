//! `map` — perguntas curtas ao mapa do projeto que o scan grava.
//!
//! `mustard-rt run map <pergunta>`:
//! - `examples --file <alvo>` (ou `--task "<tarefa>"`): 2 ou 3 arquivos que
//!   servem de exemplo, com o motivo de cada um e as receitas do git;
//! - `importers --file <arquivo>`: quem importa o arquivo;
//! - `tests --file <arquivo>`: que testes o cobrem;
//! - `slice --file <arquivo> --name <declaração>`: o trecho da declaração, do
//!   começo ao fim, com o caminho e as linhas de onde ele saiu;
//! - `users --name <declaração>` (com `--file`, só a desse arquivo): quem usa
//!   a declaração, como `arquivo:linha:quem chama`;
//! - `search --query "<palavras>"`: a busca por conceito;
//! - `summary`: o resumo do início da sessão, até 3 kB;
//! - `skill --path <SKILL.md>`: confere os caminhos que a skill cita e o
//!   tamanho dela;
//! - `dump`: o banco do mapa tabela por tabela, em ordem fixa, para depurar.
//!
//! A regra mora em `mustard_core::domain::project_map`, e a leitura do banco
//! na porta `mustard_core::io::project_map`; a busca lê o índice de palavras
//! do mapa, por `mustard_core::io::map_search`, sem o mapa inteiro. Aqui só
//! se leem o mapa, a skill e o arquivo de onde sai o trecho, e se imprime o
//! JSON.

use std::path::{Path, PathBuf};

use clap::ValueEnum;
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::{self as project_map, MapRefusal, ProjectMap};
use mustard_core::domain::scan::ScanReport;
use mustard_core::domain::search::TOP;
use mustard_core::io::map_search;
use mustard_core::io::project_map::{self as store, Need};
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
    Users,
    Dump,
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
            Self::Users => "users",
            Self::Dump => "dump",
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

/// A releitura do mapa: roda o scan sobre a raiz, gravando no caminho dado.
pub(crate) type Mine<'m> = dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport> + 'm;

/// Responde a pergunta e devolve o JSON; nunca entra em pânico. Antes, a
/// conferência do mapa com o conteúdo de agora relê por `mine` o que mudou
/// desde a passada que o gravou.
pub(crate) fn map_at(opts: &MapOpts, mine: &Mine<'_>) -> Value {
    let project = crate::commands::spec_events::project(&opts.root);
    crate::commands::flow::round::refresh_map_if_stale(&project.root, mine);
    let lang = project.lang;
    match answer(opts, &project.root, lang, &project.languages) {
        Ok(report) => report,
        Err(refusal) => refused(&refusal, lang),
    }
}

/// O banco do mapa tabela por tabela, como a porta o lê.
fn dump(root: &Path) -> Result<Value, MapRefusal> {
    Ok(json!({ "ok": true, "question": "dump", "tables": store::dump(root)? }))
}

fn answer(opts: &MapOpts, root: &Path, lang: Locale, languages: &Languages) -> Result<Value, MapRefusal> {
    answer_from(opts, root, lang, languages, &|need| store::read_for(root, need))
}

/// Como o mapa se lê para uma pergunta: pela porta, só as tabelas de que ela
/// precisa.
type Reader<'r> = dyn Fn(Need<'_>) -> Result<ProjectMap, MapRefusal> + 'r;

/// O valor de uma opção que a pergunta exige; sem ele, primeiro as recusas
/// do mapa, como quando o mapa se lia antes de olhar as opções, e depois a
/// que diz qual opção falta.
fn after_the_map<T>(value: Result<T, MapRefusal>, read: &Reader<'_>) -> Result<T, MapRefusal> {
    value.or_else(|missing| {
        read(Need::Nothing)?;
        Err(missing)
    })
}

/// A resposta da pergunta com o mapa lido por `read`.
fn answer_from(
    opts: &MapOpts,
    root: &Path,
    lang: Locale,
    languages: &Languages,
    read: &Reader<'_>,
) -> Result<Value, MapRefusal> {
    let question = opts.question;
    match question {
        Question::Importers => {
            let file = after_the_map(required(opts.file.as_deref(), question, "--file"), read)?;
            let importers = project_map::importers(&read(Need::Importers(&file))?, &file)?;
            Ok(json!({ "ok": true, "question": "importers", "file": project_map::clean_path(&file), "importers": importers }))
        }
        Question::Tests => {
            let file = after_the_map(required(opts.file.as_deref(), question, "--file"), read)?;
            let tests = project_map::tests_for(&read(Need::Tests(&file))?, &file)?;
            Ok(json!({
                "ok": true,
                "question": "tests",
                "file": project_map::clean_path(&file),
                "inline": tests.inline,
                "tests": tests.files,
            }))
        }
        Question::Search => {
            let query = after_the_map(required(opts.query.as_deref(), question, "--query"), read)?;
            let files: Vec<Value> = map_search::search(root, &query, languages, TOP)?
                .into_iter()
                .map(|found| json!({ "path": found.path, "score": found.score }))
                .collect();
            Ok(json!({ "ok": true, "question": "search", "query": query, "files": files }))
        }
        Question::Summary => {
            let text = project_map::summary(&read(Need::Summary)?, lang);
            Ok(json!({ "ok": true, "question": "summary", "bytes": text.len(), "summary": text }))
        }
        Question::Slice => slice(opts, root, read),
        Question::Users => users(opts, lang, read),
        Question::Examples => examples(opts, lang, languages, read),
        Question::Skill => skill(opts, root, read),
        Question::Dump => dump(root),
    }
}

/// O trecho da declaração de `--name` no arquivo de `--file`: as linhas dela,
/// do começo ao fim, mais o caminho e as linhas de onde saíram. O mapa diz
/// onde a declaração mora; o arquivo é lido aqui, uma vez, para que quem
/// pergunta não precise abri-lo.
fn slice(opts: &MapOpts, root: &Path, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let question = opts.question;
    let file = after_the_map(required(opts.file.as_deref(), question, "--file"), read)?;
    let name = after_the_map(required(opts.name.as_deref(), question, "--name"), read)?;
    let map = read(Need::Declarations { file: Some(&file), name: &name })?;
    let place = project_map::declaration(&map, &file, &name)?;
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

/// Quem usa a declaração de `--name`: cada declaração com esse nome no mapa
/// (só a do arquivo de `--file`, quando ele vem), com os usos que o scan
/// gravou, como `arquivo:linha:quem chama`. A que ninguém usa leva a nota que
/// diz isso, para que a lista vazia não pareça um mapa sem a informação.
fn users(opts: &MapOpts, lang: Locale, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let name = after_the_map(required(opts.name.as_deref(), opts.question, "--name"), read)?;
    let file = opts.file.as_deref().map(str::trim).filter(|f| !f.is_empty());
    let found = project_map::users(&read(Need::Declarations { file, name: &name })?, file, &name)?;
    let declarations: Vec<Value> = found
        .iter()
        .map(|d| {
            let mut entry = json!({
                "file": d.file,
                "name": d.name,
                "kind": d.kind,
                "line": d.line,
                "end_line": d.end_line,
                "used_by": d.used_by,
            });
            if d.used_by.is_empty() {
                entry["note"] = json!(mustard_core::translate("map.users.none", lang)
                    .replace("{name}", &d.name)
                    .replace("{file}", &d.file));
            }
            entry
        })
        .collect();
    let mut report = json!({
        "ok": true,
        "question": "users",
        "name": name.trim(),
        "head": mustard_core::translate("map.users.head", lang).replace("{name}", name.trim()),
        "declarations": declarations,
    });
    if let Some(file) = file {
        report["file"] = json!(project_map::clean_path(file));
    }
    Ok(report)
}

/// Os exemplos para o alvo de `--file`; sem ele, para a pasta do arquivo que
/// a busca acha para `--task`, nas línguas `languages`.
fn examples(opts: &MapOpts, lang: Locale, languages: &Languages, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let file = opts.file.as_deref().map(str::trim).filter(|f| !f.is_empty());
    let task = opts.task.as_deref().map(str::trim).filter(|t| !t.is_empty());
    // A pasta que a tarefa acha sai dos nomes que cada arquivo declara; o
    // alvo dado pelo caminho não precisa deles. Sem nenhum dos dois, só as
    // recusas do mapa vêm antes da que diz o que falta.
    let need = match (file, task) {
        (None, None) => Need::Nothing,
        (file, _) => Need::Examples { words: file.is_none() },
    };
    let map = read(need)?;
    let target = match (file, task) {
        (Some(file), _) => project_map::clean_path(file),
        (None, Some(task)) => match project_map::best_folder(&map, task, languages) {
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
    let got = project_map::examples(&map, &target, lang);
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
/// mais forte para o menos forte, pelo índice de busca do mapa. Vazio quando
/// não há mapa gravado ou quando nada casa: quem pergunta decide o que fazer
/// com a lista, porque o mapa não preenche a tarefa sozinho. A busca corta as
/// palavras nas línguas `languages`.
pub(crate) fn suggested_files(root: &Path, task: &str, limit: usize, languages: &Languages) -> Vec<String> {
    map_search::search(root, task, languages, limit).unwrap_or_default().into_iter().map(|found| found.path).collect()
}

/// A pasta que a skill descreve: a de cima do `.claude` onde ela mora.
fn skill_owner(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|a| a.file_name().is_some_and(|n| n == ".claude")).and_then(Path::parent).map(Path::to_path_buf)
}

/// Confere a skill de `--path`: cada caminho citado existe (na raiz do
/// projeto, na pasta da skill ou no mapa) e o texto cabe no limite.
fn skill(opts: &MapOpts, root: &Path, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let given = opts.path.as_deref().ok_or_else(|| MapRefusal::MissingArgument {
        question: "skill".to_string(),
        flag: "--path".to_string(),
    })?;
    let path = std::path::absolute(given).unwrap_or_else(|_| given.to_path_buf());
    let shown = given.to_string_lossy().replace('\\', "/");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| MapRefusal::SkillUnreadable { path: shown.clone(), detail: e.to_string() })?;
    let map = read(Need::Paths).ok();
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
    let report = map_at(opts, &|root, out| mustard_core::Scan::locate().scan(root, out));
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A resposta do comando a um mapa de teste, que ninguém relê: fora do
    /// git, a conferência não chama o scan.
    fn answered(opts: &MapOpts) -> Value {
        map_at(opts, &|_, _| panic!("a map outside git is never read again"))
    }

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
        store::write_text(dir.path(), MODEL).unwrap();
        dir
    }

    fn ask(root: &Path, question: Question) -> MapOpts {
        MapOpts { root: root.to_path_buf(), question, file: None, task: None, query: None, path: None, name: None }
    }

    /// Num projeto que escreve em português e programa em inglês, a busca do
    /// mapa corta a pergunta também como inglês: "users" acha o arquivo que
    /// declara `UserRepository`. Com o código declarado em português, a mesma
    /// pergunta não acha nada, porque o português não tira o plural de
    /// "users".
    #[test]
    fn users_finds_user_repository_when_the_project_codes_in_english() {
        let dir = tempdir().unwrap();
        store::write_text(
            dir.path(),
            r#"{"modules": [
              {"path": "src/storage.rs", "loc": 40, "declarations": [{"name": "UserRepository"}]},
              {"path": "src/billing.rs", "loc": 40, "declarations": [{"name": "Payment"}]}
            ]}"#,
        )
        .unwrap();
        let config = dir.path().join("mustard.json");
        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("users".to_string());

        std::fs::write(&config, r#"{"language": {"text": "pt-BR", "code": "en-US"}}"#).unwrap();
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["files"], json!([{"path": "src/storage.rs", "score": report["files"][0]["score"]}]), "{report}");

        std::fs::write(&config, r#"{"language": {"text": "pt-BR", "code": "pt-BR"}}"#).unwrap();
        let report = answered(&opts);
        assert_eq!(report["files"], json!([]), "{report}");
    }

    /// O índice da busca guarda as línguas em que foi feito: quando o projeto
    /// passa a programar em inglês, a busca seguinte o refaz, e "processed"
    /// acha o arquivo que declara `Processing`, que o índice só em português
    /// não achava.
    #[test]
    fn a_change_in_the_project_languages_remakes_the_search_index() {
        let dir = tempdir().unwrap();
        store::write_text(
            dir.path(),
            r#"{"modules": [
              {"path": "src/queue.rs", "loc": 40, "declarations": [{"name": "Processing"}]},
              {"path": "src/billing.rs", "loc": 40, "declarations": [{"name": "Payment"}]}
            ]}"#,
        )
        .unwrap();
        let config = dir.path().join("mustard.json");
        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("processed".to_string());

        std::fs::write(&config, r#"{"language": {"text": "pt-BR", "code": "pt-BR"}}"#).unwrap();
        let report = answered(&opts);
        assert_eq!(report["files"], json!([]), "{report}");

        std::fs::write(&config, r#"{"language": {"text": "pt-BR", "code": "en-US"}}"#).unwrap();
        let report = answered(&opts);
        assert_eq!(report["files"][0]["path"], json!("src/queue.rs"), "{report}");
    }

    /// Um mapa em que uma coluna que a busca não lê guarda o tipo errado:
    /// texto onde se espera a lista dos arquivos importados. O mapa inteiro
    /// não se lê, e a busca lê só o índice de palavras.
    const WRONG_TYPE: &str = r#"{"modules": [
      {"path": "src/pedido.rs", "loc": 40, "deps": "texto no lugar da lista", "declarations": [{"name": "buscarPedido"}]},
      {"path": "src/cliente.rs", "loc": 40, "declarations": [{"name": "Cliente"}]}
    ]}"#;

    #[test]
    fn the_search_question_answers_a_map_with_a_column_it_does_not_read_in_the_wrong_type() {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), WRONG_TYPE).unwrap();
        assert!(store::read(dir.path()).is_err(), "the whole map does not read");
        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("buscar pedido".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["files"][0]["path"], json!("src/pedido.rs"), "{report}");
    }

    #[test]
    fn suggested_files_come_from_a_map_with_a_column_they_do_not_read_in_the_wrong_type() {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), WRONG_TYPE).unwrap();
        assert!(store::read(dir.path()).is_err(), "the whole map does not read");
        let languages = Languages::new(["pt-BR", "en-US"]);
        assert_eq!(suggested_files(dir.path(), "buscar pedido", 3, &languages), ["src/pedido.rs"]);
    }

    #[test]
    fn a_task_asking_for_examples_gets_two_or_three_from_the_same_folder_with_the_reason() {
        let dir = project_with_map();
        let mut opts = ask(dir.path(), Question::Examples);
        opts.task = Some("adicionar um comando run".to_string());
        let report = answered(&opts);
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
        let report = answered(&opts);
        assert_eq!(report["importers"].as_array().unwrap().len(), 4, "{report}");

        let mut opts = ask(dir.path(), Question::Tests);
        opts.file = Some("apps/rt/src/commands/pay/write.rs".to_string());
        assert_eq!(answered(&opts)["tests"], json!(["apps/rt/tests/pay_cli.rs"]));

        let mut opts = ask(dir.path(), Question::Search);
        opts.query = Some("pagamento".to_string());
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");

        let report = answered(&ask(dir.path(), Question::Summary));
        assert!(report["bytes"].as_u64().unwrap() <= 3 * 1024, "{report}");
    }

    #[test]
    fn a_question_without_its_option_or_without_a_map_is_refused_by_name() {
        let dir = tempdir().unwrap();
        let report = answered(&ask(dir.path(), Question::Summary));
        assert_eq!(report["reason"], json!("map-missing"));
        assert!(report["hint"].as_str().unwrap().contains("mustard-rt run scan"));

        let dir = project_with_map();
        let report = answered(&ask(dir.path(), Question::Importers));
        assert_eq!(report["reason"], json!("missing-argument"));
        assert!(report["hint"].as_str().unwrap().contains("--file"));

        let mut opts = ask(dir.path(), Question::Tests);
        opts.file = Some("nao/existe.rs".to_string());
        assert_eq!(answered(&opts)["reason"], json!("unknown-file"));
    }

    /// O trecho de uma declaração vem do mapa mais o arquivo: quem pergunta
    /// não abre o arquivo, e recebe as linhas da declaração, do começo ao fim,
    /// com o caminho e as linhas de onde saíram.
    #[test]
    fn o_mapa_devolve_o_trecho_de_uma_declaracao() {
        let dir = tempdir().unwrap();
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
        store::write_text(
            dir.path(),
            &format!(
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
        let report = answered(&opts);
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
        let report = answered(&sem_nome);
        assert_eq!(report["reason"], json!("missing-argument"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("--name"), "{report}");

        opts.name = Some("sumiu".to_string());
        let report = answered(&opts);
        assert_eq!(report["reason"], json!("unknown-declaration"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("sumiu"), "{report}");
    }

    /// Um mapa com tudo o que as perguntas leem: línguas, subprojetos, os
    /// mais importados, a história, um arquivo escrito por máquina, o mesmo
    /// nome declarado em vários arquivos (fora da ordem dos caminhos) e duas
    /// vezes no mesmo, e os usos de cada declaração.
    const EVERY_PART: &str = r#"{
      "modules": [
        {"path": "apps/rt/src/commands/pay/write.rs", "language": "rust", "loc": 120,
         "declarations": [
           {"kind": "function", "name": "run", "line": 2, "end_line": 4, "doc": "Grava.", "signature": "pub fn run()",
            "used_by": ["apps/rt/src/main.rs:9:main"]},
           {"kind": "function", "name": "run", "line": 6, "end_line": 7}
         ],
         "deps": ["packages/core/src/pay.rs"], "tests": ["apps/rt/tests/pay_cli.rs"]},
        {"path": "apps/rt/src/commands/pay/read.rs", "language": "rust", "loc": 100, "has_tests": true,
         "declarations": [{"kind": "function", "name": "run", "line": 1, "end_line": 3}],
         "deps": ["packages/core/src/pay.rs"]},
        {"path": "apps/rt/src/commands/pay/index.rs", "language": "rust", "loc": 90,
         "declarations": [{"kind": "function", "name": "run", "line": 5}, {"kind": "struct", "name": "Index", "line": 1}],
         "deps": ["packages/core/src/pay.rs", "apps/rt/src/commands/pay/read.rs"]},
        {"path": "apps/rt/src/commands/pay/cli.rs", "language": "rust", "loc": 3000,
         "declarations": [{"name": "dispatch", "line": 1}], "deps": ["packages/core/src/pay.rs"]},
        {"path": "apps/rt/src/commands/pay/gen.rs", "language": "rust", "loc": 95, "file_class": "generated",
         "declarations": [{"name": "run", "line": 1}], "deps": ["packages/core/src/pay.rs"]},
        {"path": "packages/core/src/pay.rs", "language": "rust", "loc": 200,
         "declarations": [{"kind": "struct", "name": "Payment", "line": 1, "end_line": 3, "used_by": ["a.rs:1"]}]},
        {"path": "apps/rt/tests/pay_cli.rs", "language": "rust", "loc": 80, "deps": ["apps/rt/src/commands/pay/write.rs"]},
        {"path": "web/src/pay.ts", "language": "typescript", "loc": 60, "declarations": [{"name": "run", "line": 3}]}
      ],
      "projects": [
        {"name": "rt", "dir": "apps/rt", "kind": "cargo", "code_files": 6},
        {"name": "core", "dir": "packages/core", "kind": "cargo", "code_files": 1},
        {"name": "web", "dir": "web", "kind": "npm", "code_files": 1}
      ],
      "languages": [{"language": "rust", "files": 7, "loc": 3595}, {"language": "typescript", "files": 1, "loc": 60}],
      "graph": {"top_fan_in": [{"module": "packages/core/src/pay.rs", "degree": 5}, {"module": "apps/rt/src/commands/pay/read.rs", "degree": 1}]},
      "skeleton": [{"dir": "apps/rt", "role": "L1"}, {"dir": "packages/core", "role": "L0"}],
      "history": {
        "paths": ["apps/rt/src/commands/pay/index.rs", "apps/rt/src/commands/pay/read.rs",
                  "apps/rt/src/commands/pay/write.rs", "web/src/pay.ts"],
        "commits": [
          {"id": "c1", "at": 86400, "added": [0, 3]},
          {"id": "c2", "at": 172800, "added": [1], "changed": [3]},
          {"id": "c3", "at": 259200, "changed": [2]}
        ]
      }
    }"#;

    /// Cada pergunta, com as opções dela: as que acham, as que não acham e
    /// as que faltam.
    fn every_question(root: &Path, skill: &Path) -> Vec<MapOpts> {
        let with = |question: Question, file: Option<&str>, name: Option<&str>, task: Option<&str>| MapOpts {
            file: file.map(str::to_string),
            name: name.map(str::to_string),
            task: task.map(str::to_string),
            ..ask(root, question)
        };
        let mut skill_opts = ask(root, Question::Skill);
        skill_opts.path = Some(skill.to_path_buf());
        vec![
            ask(root, Question::Summary),
            with(Question::Importers, Some("packages/core/src/pay.rs"), None, None),
            with(Question::Importers, Some("./apps/rt/src/commands/pay/read.rs"), None, None),
            with(Question::Importers, Some("web/src/pay.ts"), None, None),
            with(Question::Importers, Some("nao/existe.rs"), None, None),
            with(Question::Importers, None, None, None),
            with(Question::Tests, Some("apps/rt/src/commands/pay/write.rs"), None, None),
            with(Question::Tests, Some("apps/rt/src/commands/pay/read.rs"), None, None),
            with(Question::Tests, Some("nao/existe.rs"), None, None),
            with(Question::Tests, None, None, None),
            with(Question::Slice, Some("apps/rt/src/commands/pay/write.rs"), Some(" run "), None),
            with(Question::Slice, Some("apps/rt/src/commands/pay/write.rs"), Some("sumiu"), None),
            with(Question::Slice, Some("nao/existe.rs"), Some("run"), None),
            with(Question::Slice, Some("apps/rt/src/commands/pay/write.rs"), None, None),
            with(Question::Slice, None, Some("run"), None),
            with(Question::Users, None, Some("run"), None),
            with(Question::Users, Some("apps/rt/src/commands/pay/write.rs"), Some("run"), None),
            with(Question::Users, Some("packages/core/src/pay.rs"), Some("Payment"), None),
            with(Question::Users, Some("packages/core/src/pay.rs"), Some("run"), None),
            with(Question::Users, Some("nao/existe.rs"), Some("run"), None),
            with(Question::Users, None, Some("sumiu"), None),
            with(Question::Users, None, None, None),
            with(Question::Examples, Some("apps/rt/src/commands/pay/novo.rs"), None, None),
            with(Question::Examples, Some("apps/rt/src/commands/pay/index.rs"), None, None),
            with(Question::Examples, Some("apps/rt/src/commands/pay"), None, None),
            with(Question::Examples, None, None, Some("adicionar um comando run")),
            with(Question::Examples, None, None, Some("nada casa com isto")),
            with(Question::Examples, None, None, None),
            skill_opts,
        ]
    }

    /// A resposta de cada pergunta com o mapa inteiro lido, como era antes
    /// de cada pergunta ler só as tabelas dela.
    fn answered_from_the_whole_map(opts: &MapOpts) -> Value {
        let project = crate::commands::spec_events::project(&opts.root);
        let whole = |_: Need<'_>| store::read(&project.root);
        answer_from(opts, &project.root, project.lang, &project.languages, &whole)
            .unwrap_or_else(|refusal| refused(&refusal, project.lang))
    }

    /// Cada pergunta lê só as tabelas dela e responde o mesmo que respondia
    /// com o mapa inteiro, byte a byte: com o mapa cheio, e sem mapa.
    #[test]
    fn every_question_reading_only_its_tables_answers_as_the_whole_map_did() {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), EVERY_PART).unwrap();
        let file = dir.path().join("apps/rt/src/commands/pay/write.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "// topo\npub fn run() {\n    gravar();\n}\n\nfn run() {}\n").unwrap();
        let skill_dir = dir.path().join("apps/rt/.claude/skills/add-pay");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill = skill_dir.join("SKILL.md");
        std::fs::write(&skill, "Veja `pay/index.rs`, `commands/pay/` e `apps/rt/src/commands/pay/sumiu.rs`.\n").unwrap();

        let mut found = 0;
        for opts in every_question(dir.path(), &skill) {
            let now = answered(&opts);
            assert_eq!(now, answered_from_the_whole_map(&opts), "{:?} {:?} {:?} {:?}", opts.question, opts.file, opts.name, opts.task);
            found += usize::from(now["ok"] == json!(true));
        }
        assert!(found >= 12, "the questions that find something must answer: {found}");

        let empty = tempdir().unwrap();
        for opts in every_question(empty.path(), &skill) {
            let now = answered(&opts);
            assert_eq!(now, answered_from_the_whole_map(&opts), "{:?} {:?} {:?}", opts.question, opts.file, opts.name);
        }
    }

    /// Antes de cada resposta, o mapa se confere com o conteúdo de agora:
    /// um arquivo editado sem commit é relido, e a pergunta seguinte já
    /// responde a linha nova; sem nada mudado, o scan não roda.
    #[test]
    fn a_question_after_an_edit_without_commit_answers_the_new_line() {
        let scan = mustard_core::Scan::locate();
        assert!(
            scan.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q"]);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() -> u32 {\n    1\n}\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        let calls = std::cell::Cell::new(0);
        let mine = |root: &Path, out: &Path| {
            calls.set(calls.get() + 1);
            scan.scan(root, out)
        };
        scan.scan(root, &store::model_path(root)).expect("the first pass writes the map");

        let mut opts = ask(root, Question::Users);
        opts.name = Some("alpha".to_string());
        assert_eq!(map_at(&opts, &mine)["declarations"][0]["line"], json!(1));
        assert_eq!(calls.get(), 0, "nothing changed: the scan does not run");

        std::fs::write(root.join("src/lib.rs"), "// topo\n\npub fn alpha() -> u32 {\n    1\n}\n").unwrap();
        let report = map_at(&opts, &mine);
        assert_eq!(report["declarations"][0]["line"], json!(3), "{report}");
        assert_eq!(calls.get(), 1, "the edit is read once");
        assert_eq!(map_at(&opts, &mine)["declarations"][0]["line"], json!(3));
        assert_eq!(calls.get(), 1, "and not again while nothing else changes");
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
        let report = answered(&opts);
        assert_eq!(report["reason"], json!("skill-missing-path"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("apps/rt/src/commands/pay/sumiu.rs"));

        std::fs::write(&skill_path, "Veja `pay/write.rs` e `commands/pay/index.rs`.\n").unwrap();
        let report = answered(&opts);
        assert_eq!(report["ok"], json!(true), "{report}");
    }
}
