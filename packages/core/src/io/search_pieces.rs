//! `search_pieces` — as peças da busca e se cada uma está ligada.
//!
//! A busca do Mustard tem várias peças que o usuário pediu, e um número de
//! medida só se entende sabendo quais estavam ligadas quando ele saiu. Este
//! módulo diz, de um mapa, o estado de cada uma na busca que responde no lugar
//! do `grep`:
//!
//! - **o compilado de cada função**: o texto que junta o nome, a assinatura, a
//!   documentação e a história da declaração; só existe no mapa como o vetor
//!   dela (`decl_vectors`);
//! - **a raiz e os sinônimos das palavras**: a raiz vale sempre; os sinônimos
//!   (as palavras vizinhas) vêm dos vetores de cada palavra do projeto
//!   (`word_vectors`);
//! - **o sentido pelo vetor**: a ordem dos vetores de todas as declarações. A
//!   resposta ao Claude lê só as palavras, e essa ordem entra na lista que
//!   vai ao filtro; por isso só está ligada com o filtro ligado e com os
//!   vetores das funções no mapa;
//! - **as duas línguas**: o índice foi feito em duas línguas, a do código e a
//!   do texto;
//! - **o histórico do git**: a história de cada declaração que o scan lê do
//!   git depois do mapa (`lineage_*`), com os títulos dos commits que mudaram
//!   cada arquivo (`commits`) e cada declaração, que entram nas palavras que a
//!   busca lê. Só está ligada quando `lineage_*` tem linhas: o mapa que só tem
//!   os commits dos arquivos mede a busca sem a história por declaração, como
//!   a das sessões não a tem enquanto a leitura não chega. A ordem por arquivo
//!   mexido há pouco não é peça: ela não entra na busca;
//! - **a conferência dos primeiros candidatos**: reordena os primeiros pela
//!   cobertura das palavras raras da pergunta; a busca da resposta sempre a faz;
//! - **o filtro do Jev**: o serviço que julga os candidatos que o mapa achou.
//!   Liga pela regra da própria busca ([`crate::io::jev_gate::filter_on`]): o
//!   `mustard.json` do projeto do mapa não põe `search.filter` em `none` e há
//!   chave válida, em [`KEY_ENV`] no ambiente de quem mede ou, sem ela, em
//!   `jev.key`; a chave num `mustard.json` que o git guarda não vale. Sem
//!   isso a busca responde sem ele.
//!
//! A lista vai na prova de cada medida ([`crate::io::measure_proof`]): quem
//! mostra um número ao usuário a mostra junto, e cada peça aparece como ligada
//! ou ainda não ligada.

use std::path::Path;

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::domain::config::ProjectConfig;
use crate::io::jev_gate::{self, KEY_ENV};
use crate::io::map_check::root_of;
use crate::io::map_db::table_exists;
use crate::io::project_map::open_existing;
use crate::platform::error::{Error, Result};

/// Uma peça da busca e o estado dela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// O nome da peça, o mesmo em toda prova.
    pub name: &'static str,
    /// A peça está ligada na busca que a medida roda.
    pub on: bool,
    /// Por que está ligada ou ainda não.
    pub why: String,
}

impl Piece {
    fn new(name: &'static str, on: bool, why: &str) -> Self {
        Self { name, on, why: why.to_string() }
    }

    /// O estado como a prova o escreve: `ligada` ou `ainda não ligada`.
    #[must_use]
    pub fn state(&self) -> &'static str {
        if self.on { "ligada" } else { "ainda não ligada" }
    }

    /// A peça como o resultado da régua a guarda.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "on": self.on, "state": self.state(), "why": self.why })
    }
}

/// A linha que a régua imprime com o estado de cada peça do mapa `label`:
/// `PECAS <mapa>: compilado=ligada raiz-e-sinonimos=ainda-nao-ligada ...`.
#[must_use]
pub fn line(label: &str, pieces: &[Piece]) -> String {
    let states: Vec<String> =
        pieces.iter().map(|piece| format!("{}={}", piece.name, piece.state().replace(' ', "-").replace('ã', "a"))).collect();
    format!("PECAS {label}: {}", states.join(" "))
}

/// As peças da busca e o estado de cada uma no mapa gravado em `model`, com o
/// filtro do Jev como a busca do projeto do mapa o liga: a chave em
/// [`KEY_ENV`] no ambiente ou em `jev.key`, e `search.filter` fora de `none`.
///
/// # Errors
/// O mapa que não abre, que não mora no lugar do mapa de um projeto ou cuja
/// leitura falha.
pub fn of_map(model: &Path) -> Result<Vec<Piece>> {
    of_map_with_env(model, std::env::var(KEY_ENV).ok())
}

/// As peças do mapa `model` como em [`of_map`], com `env` no lugar do valor de
/// [`KEY_ENV`]: o teste não depende do ambiente de quem o roda.
///
/// # Errors
/// Os de [`of_map`].
pub fn of_map_with_env(model: &Path, env: Option<String>) -> Result<Vec<Piece>> {
    let root = root_of(model).ok_or_else(|| {
        Error::check_failed(format!(
            "o mapa {} não mora no lugar do mapa de um projeto: sem o mustard.json dele, não há como dizer se o filtro vale",
            model.display()
        ))
    })?;
    let db = open_existing(model)
        .map_err(|refusal| Error::check_failed(format!("o mapa {} não abriu ({})", model.display(), refusal.reason())))?;
    let filter = jev_gate::filter_on(root, &ProjectConfig::load(root), env);
    of_connection(db.conn(), filter)
}

/// As peças no banco aberto `conn`; `filter` diz se a busca chama o Jev.
fn of_connection(conn: &Connection, filter: bool) -> Result<Vec<Piece>> {
    let filled = |table: &str| -> Result<bool> {
        if !table_exists(conn, table)? {
            return Ok(false);
        }
        Ok(conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table})"), [], |row| row.get::<_, bool>(0))?)
    };
    let compiled = filled("decl_vectors")?;
    let words = filled("word_vectors")?;
    let commits = filled("commits")?;
    let lineage = filled("lineage_files")? || filled("lineage_commits")? || filled("lineage_decls")?;
    let languages: Option<String> = if table_exists(conn, "search_meta")? {
        conn.query_row("SELECT value FROM search_meta WHERE key = 'languages'", [], |row| row.get(0)).ok()
    } else {
        None
    };
    let two_languages = languages.as_deref().is_some_and(|codes| codes.split(',').filter(|code| !code.is_empty()).count() >= 2);
    Ok(vec![
        Piece::new(
            "compilado",
            compiled,
            if compiled { "o mapa tem o vetor do texto compilado de cada função" } else { "o mapa não tem o texto compilado das funções" },
        ),
        Piece::new(
            "raiz-e-sinonimos",
            words,
            if words { "a raiz e as palavras vizinhas do projeto" } else { "só a raiz: o mapa não tem as palavras com vetor" },
        ),
        Piece::new(
            "sentido-pelo-vetor",
            filter && compiled,
            match (filter, compiled) {
                (true, true) => "a ordem dos vetores das declarações entra na lista que vai ao filtro",
                (true, false) => "o filtro está ligado, mas o mapa não tem o vetor das funções: não há ordem pelo sentido para a lista",
                (false, _) => "a resposta lê só as palavras; a ordem dos vetores vai à lista do filtro, e sem o filtro ela não entra",
            },
        ),
        Piece::new(
            "duas-linguas",
            two_languages,
            if two_languages { "o índice foi feito nas duas línguas do projeto" } else { "o índice foi feito em uma língua só" },
        ),
        Piece::new(
            "historico",
            lineage,
            match (lineage, commits) {
                (true, true) => "os títulos dos commits entram nas palavras de cada arquivo e de cada declaração",
                (true, false) => "a história de cada declaração foi lida, e o mapa não tem os commits dos arquivos",
                (false, true) => "a história de cada declaração não foi lida (lineage vazio): só os títulos dos commits de cada arquivo entram nas palavras",
                (false, false) => "o mapa não tem a história do git",
            },
        ),
        Piece::new("conferencia", true, "os primeiros candidatos passam pela conferência das palavras raras"),
        Piece::new(
            "filtro-jev",
            filter,
            if filter {
                "a chave do Jev está no ambiente: os candidatos passam pelo filtro"
            } else {
                "sem a chave do Jev no ambiente a busca responde sem o filtro"
            },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::normalize::Languages;
    use crate::io::project_map::{model_path, save_at};
    use rusqlite::params;
    use tempfile::{tempdir, TempDir};

    fn saved(languages: &[&str]) -> (TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        let map = json!({"modules": [{"path": "src/a.rs", "declarations": [
            {"kind": "function", "name": "a", "line": 1, "end_line": 3, "signature": "fn a()"}]}]});
        save_at(&model, &map, "scan 1", &Languages::new(languages.iter().copied())).unwrap();
        (dir, model)
    }

    fn states(pieces: &[Piece]) -> Vec<(&'static str, bool)> {
        pieces.iter().map(|piece| (piece.name, piece.on)).collect()
    }

    /// Um mapa sem vetores, sem commits e feito numa língua só, medido sem a
    /// chave do Jev, tem ligada só a conferência: o resto aparece como ainda
    /// não ligado, o sentido pelo vetor e o filtro inclusive.
    #[test]
    fn a_map_without_vectors_commits_a_second_language_or_the_key_shows_only_the_check_as_on() {
        let (_dir, model) = saved(&["en-US"]);
        let got = of_connection(open_existing(&model).unwrap().conn(), false).unwrap();
        assert_eq!(
            states(&got),
            [
                ("compilado", false),
                ("raiz-e-sinonimos", false),
                ("sentido-pelo-vetor", false),
                ("duas-linguas", false),
                ("historico", false),
                ("conferencia", true),
                ("filtro-jev", false),
            ]
        );
    }

    /// Cada peça liga com o que o mapa tem: os vetores das funções ligam o
    /// compilado, os das palavras ligam os sinônimos, as duas línguas do
    /// índice ligam a peça das línguas e a história de cada declaração liga o
    /// histórico; os commits dos arquivos sozinhos não o ligam.
    #[test]
    fn each_piece_turns_on_with_what_the_map_holds() {
        let (_dir, model) = saved(&["pt-BR", "en-US"]);
        let db = open_existing(&model).unwrap();
        let conn = db.conn();
        conn.execute_batch(
            "CREATE TABLE decl_vectors(file TEXT NOT NULL, name TEXT NOT NULL, nth INTEGER NOT NULL, hash INTEGER NOT NULL, vector BLOB NOT NULL);
             CREATE TABLE word_vectors(word TEXT NOT NULL, forms TEXT NOT NULL, vector BLOB NOT NULL);
             INSERT INTO decl_vectors(file, name, nth, hash, vector) VALUES ('src/a.rs', 'a', 0, 1, x'00');
             INSERT INTO word_vectors(word, forms, vector) VALUES ('a', 'a', x'00');",
        )
        .unwrap();
        conn.execute("INSERT INTO history_paths(path) VALUES ('src/a.rs')", []).unwrap();
        conn.execute(
            "INSERT INTO commits(id, at, title, pr, added, changed) VALUES ('c1', 1, 't', 0, '[0]', '[]')",
            params![],
        )
        .unwrap();
        let got = of_connection(conn, false).unwrap();
        assert_eq!(
            states(&got),
            [
                ("compilado", true),
                ("raiz-e-sinonimos", true),
                ("sentido-pelo-vetor", false),
                ("duas-linguas", true),
                ("historico", false),
                ("conferencia", true),
                ("filtro-jev", false),
            ],
            "the commits of the files alone do not turn the history on"
        );
        assert!(got[4].why.contains("a história de cada declaração não foi lida"), "{}", got[4].why);
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lineage_decls(path TEXT, name TEXT, nth INTEGER, commits TEXT, comments TEXT);
             INSERT INTO lineage_decls(path, name, nth, commits, comments) VALUES ('src/a.rs', 'a', 0, '[]', NULL);",
        )
        .unwrap();
        let read = of_connection(conn, false).unwrap();
        assert!(read[4].on && read[4].why.contains("de cada declaração") && !read[4].why.contains("não foi lida"), "{}", read[4].why);
    }

    /// O histórico só liga quando alguma tabela da história por declaração
    /// (`lineage_*`) tem linhas: o mapa com os commits dos arquivos e as
    /// tabelas vazias, como o do scan antes de a leitura da história chegar,
    /// diz na linha da prova que o histórico ainda não está ligado, e o motivo
    /// diz que a história de cada declaração não foi lida; cada uma das três
    /// tabelas, com uma linha, liga a peça.
    #[test]
    fn the_history_is_on_only_when_the_lineage_tables_have_rows() {
        let (_dir, model) = saved(&["en-US"]);
        let db = open_existing(&model).unwrap();
        let conn = db.conn();
        let history = |conn: &Connection| {
            let pieces = of_connection(conn, false).unwrap();
            (line("m", &pieces), pieces[4].on, pieces[4].why.clone())
        };

        let (line_without, on, why) = history(conn);
        assert!(!on && line_without.contains("historico=ainda-nao-ligada"), "{line_without}");
        assert!(why.contains("o mapa não tem a história do git"), "{why}");

        conn.execute("INSERT INTO history_paths(path) VALUES ('src/a.rs')", []).unwrap();
        conn.execute("INSERT INTO commits(id, at, title, pr, added, changed) VALUES ('c1', 1, 't', 0, '[0]', '[]')", params![]).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lineage_files(path TEXT, base TEXT, last_commit TEXT, tip TEXT, mark TEXT, moves INTEGER, comments INTEGER);
             CREATE TABLE IF NOT EXISTS lineage_commits(path TEXT, id TEXT, at INTEGER, title TEXT, pr INTEGER, files TEXT);
             CREATE TABLE IF NOT EXISTS lineage_decls(path TEXT, name TEXT, nth INTEGER, commits TEXT, comments TEXT);",
        )
        .unwrap();
        let (line_commits_only, on, why) = history(conn);
        assert!(!on && line_commits_only.contains("historico=ainda-nao-ligada"), "{line_commits_only}");
        assert!(why.contains("lineage vazio") && why.contains("não foi lida"), "{why}");

        let inserts = [
            "INSERT INTO lineage_files(path, base, last_commit, tip, mark, moves, comments) VALUES ('src/a.rs', 'main', 'c1', 'c1', 'm', 0, 0)",
            "INSERT INTO lineage_commits(path, id, at, title, pr, files) VALUES ('src/a.rs', 'c1', 1, 't', 0, '{}')",
            "INSERT INTO lineage_decls(path, name, nth, commits, comments) VALUES ('src/a.rs', 'a', 0, '[]', NULL)",
        ];
        for (table, insert) in ["lineage_files", "lineage_commits", "lineage_decls"].iter().zip(inserts) {
            conn.execute(insert, []).unwrap();
            let (with_rows, on, _) = history(conn);
            assert!(on && with_rows.contains("historico=ligada"), "{table}: {with_rows}");
            conn.execute(&format!("DELETE FROM {table}"), []).unwrap();
        }
    }

    /// A linha da prova diz o estado de cada peça com as palavras do usuário,
    /// e o JSON traz o motivo.
    #[test]
    fn the_line_and_the_json_say_the_state_of_each_piece() {
        let (_dir, model) = saved(&["en-US"]);
        let got = of_map(&model).unwrap();
        let line = line("mapa", &got);
        assert!(line.starts_with("PECAS mapa: compilado=ainda-nao-ligada raiz-e-sinonimos=ainda-nao-ligada"), "{line}");
        assert!(line.contains(" conferencia=ligada"), "{line}");
        let json = got[0].to_json();
        assert_eq!((json["name"].as_str(), json["state"].as_str(), json["on"].as_bool()), (Some("compilado"), Some("ainda não ligada"), Some(false)));
        assert!(!json["why"].as_str().unwrap().is_empty());
    }

    /// A chave do Jev no ambiente liga o filtro, e o sentido pelo vetor segue o
    /// filtro: sem a chave, as duas peças aparecem como ainda não ligadas, e a
    /// linha da prova diz `filtro-jev=ainda-nao-ligada`; a chave em branco vale
    /// como ausente. Com a chave e o mapa sem os vetores das funções, o filtro
    /// liga e o sentido pelo vetor não, porque a ordem dele não existe.
    #[test]
    fn the_jev_filter_turns_on_with_the_key_and_the_meaning_order_follows_it() {
        let (_dir, model) = saved(&["en-US"]);
        let named = |pieces: &[Piece], name: &str| pieces.iter().find(|piece| piece.name == name).map(|piece| piece.on);

        let no_vectors = of_map_with_env(&model, Some("key".to_string())).unwrap();
        assert_eq!(named(&no_vectors, "filtro-jev"), Some(true), "the key turns the filter on");
        assert_eq!(named(&no_vectors, "sentido-pelo-vetor"), Some(false), "no vectors, no order by meaning");
        assert!(no_vectors[2].why.contains("não tem o vetor das funções"), "{}", no_vectors[2].why);

        {
            let db = open_existing(&model).unwrap();
            db.conn()
                .execute_batch(
                    "CREATE TABLE decl_vectors(file TEXT NOT NULL, name TEXT NOT NULL, nth INTEGER NOT NULL, hash INTEGER NOT NULL, vector BLOB NOT NULL);
                     INSERT INTO decl_vectors(file, name, nth, hash, vector) VALUES ('src/a.rs', 'a', 0, 1, x'00');",
                )
                .unwrap();
        }
        let with = of_map_with_env(&model, Some("key".to_string())).unwrap();
        assert_eq!((named(&with, "filtro-jev"), named(&with, "sentido-pelo-vetor")), (Some(true), Some(true)));
        assert!(line("m", &with).contains("sentido-pelo-vetor=ligada") && line("m", &with).ends_with("filtro-jev=ligada"), "{}", line("m", &with));

        for absent in [None, Some(""), Some("  ")] {
            let without = of_map_with_env(&model, absent.map(str::to_string)).unwrap();
            assert_eq!((named(&without, "filtro-jev"), named(&without, "sentido-pelo-vetor")), (Some(false), Some(false)), "{absent:?}");
            assert!(line("m", &without).ends_with("filtro-jev=ainda-nao-ligada"), "{}", line("m", &without));
        }
    }

    /// A peça do filtro lê a chave do ambiente do processo, pela variável que
    /// o Jev lê: o estado dela é o de a variável ter valor.
    #[test]
    fn the_filter_piece_of_a_map_reads_the_key_variable_of_the_process() {
        let (_dir, model) = saved(&["en-US"]);
        let in_environment = std::env::var(KEY_ENV).is_ok_and(|value| !value.trim().is_empty());
        let got = of_map(&model).unwrap();
        let filter = got.iter().find(|piece| piece.name == "filtro-jev").expect("the list has the filter");
        assert_eq!(filter.on, in_environment, "{}", filter.why);
    }

    /// Escreve o `mustard.json` do projeto do mapa `dir`.
    fn write_config(dir: &TempDir, config: &Value) {
        std::fs::write(dir.path().join("mustard.json"), config.to_string()).unwrap();
    }

    /// O estado da peça do filtro no mapa `model`, com `env` no lugar da
    /// variável de ambiente.
    fn filter_state(model: &Path, env: Option<&str>) -> bool {
        let pieces = of_map_with_env(model, env.map(str::to_string)).unwrap();
        pieces.iter().find(|piece| piece.name == "filtro-jev").expect("the list has the filter").on
    }

    /// A peça do filtro liga pela regra da busca, com o `mustard.json` do
    /// projeto do mapa: a chave só em `jev.key` liga; `search.filter` igual a
    /// `none` desliga mesmo com a chave, do arquivo ou do ambiente; sem chave
    /// em lugar nenhum, desligada; a chave do arquivo que o git guarda não
    /// vale, e a do ambiente vale do mesmo jeito.
    #[test]
    fn the_filter_piece_follows_the_rule_of_the_search_with_the_project_file() {
        let (dir, model) = saved(&["en-US"]);
        assert!(!filter_state(&model, None), "no key anywhere");

        write_config(&dir, &json!({"jev": {"key": "from-file"}}));
        assert!(filter_state(&model, None), "the key only in jev.key turns the piece on");
        let pieces = of_map_with_env(&model, None).unwrap();
        assert!(line("m", &pieces).ends_with("filtro-jev=ligada"), "{}", line("m", &pieces));

        write_config(&dir, &json!({"search": {"filter": "none"}, "jev": {"key": "from-file"}}));
        assert!(!filter_state(&model, None), "none turns the piece off with the key of the file");
        assert!(!filter_state(&model, Some("from-env")), "none turns the piece off with the key of the environment");
        let pieces = of_map_with_env(&model, Some("from-env".to_string())).unwrap();
        assert!(line("m", &pieces).ends_with("filtro-jev=ainda-nao-ligada"), "{}", line("m", &pieces));

        write_config(&dir, &json!({"search": {"filter": "jev"}, "jev": {"key": "  "}}));
        assert!(!filter_state(&model, None), "a blank key is no key");
        assert!(filter_state(&model, Some("from-env")), "the key of the environment turns it on");

        assert!(crate::platform::git::run(dir.path(), &["init", "-q"]).ok);
        write_config(&dir, &json!({"jev": {"key": "from-file"}}));
        assert!(filter_state(&model, None), "out of git, the key of the file counts");
        assert!(crate::platform::git::run(dir.path(), &["add", "mustard.json"]).ok);
        assert!(!filter_state(&model, None), "git tracks the file: its key does not turn the piece on");
        assert!(filter_state(&model, Some("from-env")), "the key of the environment still turns it on");
    }

    /// Um mapa fora do lugar do mapa de um projeto não diz o estado do filtro:
    /// sem o `mustard.json` dele, a peça recusa em vez de adivinhar.
    #[test]
    fn a_map_outside_the_place_of_a_project_map_is_refused() {
        let (dir, model) = saved(&["en-US"]);
        let moved = dir.path().join("elsewhere.db");
        std::fs::copy(&model, &moved).unwrap();
        let error = of_map_with_env(&moved, Some("key".to_string())).unwrap_err().to_string();
        assert!(error.contains("não mora no lugar do mapa de um projeto"), "{error}");
    }

    /// Um mapa que falta recusa em vez de listar peças que não conferiu.
    #[test]
    fn a_missing_map_is_refused() {
        let dir = tempdir().unwrap();
        let error = of_map(&model_path(dir.path())).unwrap_err().to_string();
        assert!(error.contains("não abriu"), "{error}");
    }
}
