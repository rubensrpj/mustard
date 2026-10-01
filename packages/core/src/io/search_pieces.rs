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
//!   vai ao filtro, que a medida sem filtro não tem;
//! - **as duas línguas**: o índice foi feito em duas línguas, a do código e a
//!   do texto;
//! - **o histórico do git**: os títulos dos commits que mudaram cada arquivo
//!   (`commits`) e, com a história de cada declaração lida (`lineage_decls`),
//!   os de cada declaração, que entram nas palavras que a busca lê. A ordem
//!   por arquivo mexido há pouco não é peça: ela não entra na busca;
//! - **a conferência dos primeiros candidatos**: reordena os primeiros pela
//!   cobertura das palavras raras da pergunta; a busca da resposta sempre a faz.
//!
//! A lista vai na prova de cada medida ([`crate::io::measure_proof`]): quem
//! mostra um número ao usuário a mostra junto, e cada peça aparece como ligada
//! ou ainda não ligada.

use std::path::Path;

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::io::map_db::table_exists;
use crate::io::project_map::open_existing;
use crate::platform::error::Result;

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

/// As peças da busca e o estado de cada uma no mapa gravado em `model`.
///
/// # Errors
/// O mapa que não abre ou cuja leitura falha.
pub fn of_map(model: &Path) -> Result<Vec<Piece>> {
    let db = open_existing(model).map_err(|refusal| {
        crate::platform::error::Error::check_failed(format!("o mapa {} não abriu ({})", model.display(), refusal.reason()))
    })?;
    of_connection(db.conn())
}

/// As peças no banco aberto `conn`.
fn of_connection(conn: &Connection) -> Result<Vec<Piece>> {
    let filled = |table: &str| -> Result<bool> {
        if !table_exists(conn, table)? {
            return Ok(false);
        }
        Ok(conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table})"), [], |row| row.get::<_, bool>(0))?)
    };
    let compiled = filled("decl_vectors")?;
    let words = filled("word_vectors")?;
    let commits = filled("commits")?;
    let declarations = filled("lineage_decls")?;
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
            false,
            "a resposta lê só as palavras; a ordem dos vetores vai à lista do filtro, que a medida não tem",
        ),
        Piece::new(
            "duas-linguas",
            two_languages,
            if two_languages { "o índice foi feito nas duas línguas do projeto" } else { "o índice foi feito em uma língua só" },
        ),
        Piece::new(
            "historico",
            commits,
            match (commits, declarations) {
                (true, true) => "os títulos dos commits entram nas palavras de cada arquivo e de cada declaração",
                (true, false) => "os títulos dos commits entram nas palavras de cada arquivo; a história de cada declaração não foi lida",
                (false, _) => "o mapa não tem a história do git",
            },
        ),
        Piece::new("conferencia", true, "os primeiros candidatos passam pela conferência das palavras raras"),
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

    /// Um mapa sem vetores, sem commits e feito numa língua só tem ligadas
    /// só a raiz e a conferência: o resto aparece como ainda não ligado, e o
    /// sentido pelo vetor, que a resposta não lê, nunca aparece ligado.
    #[test]
    fn a_map_without_vectors_commits_or_a_second_language_shows_only_the_check_as_on() {
        let (_dir, model) = saved(&["en-US"]);
        let got = of_connection(open_existing(&model).unwrap().conn()).unwrap();
        assert_eq!(
            states(&got),
            [
                ("compilado", false),
                ("raiz-e-sinonimos", false),
                ("sentido-pelo-vetor", false),
                ("duas-linguas", false),
                ("historico", false),
                ("conferencia", true),
            ]
        );
    }

    /// Cada peça liga com o que o mapa tem: os vetores das funções ligam o
    /// compilado, os das palavras ligam os sinônimos, as duas línguas do
    /// índice ligam a peça das línguas e os commits ligam o histórico.
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
        let got = of_connection(conn).unwrap();
        assert_eq!(
            states(&got),
            [
                ("compilado", true),
                ("raiz-e-sinonimos", true),
                ("sentido-pelo-vetor", false),
                ("duas-linguas", true),
                ("historico", true),
                ("conferencia", true),
            ]
        );
        assert!(got[4].why.contains("a história de cada declaração não foi lida"), "{}", got[4].why);
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lineage_decls(path TEXT, name TEXT, nth INTEGER, commits TEXT, comments TEXT);
             INSERT INTO lineage_decls(path, name, nth, commits, comments) VALUES ('src/a.rs', 'a', 0, '[]', NULL);",
        )
        .unwrap();
        let read = of_connection(conn).unwrap();
        assert!(read[4].on && read[4].why.contains("de cada declaração") && !read[4].why.contains("não foi lida"), "{}", read[4].why);
    }

    /// A linha da prova diz o estado de cada peça com as palavras do usuário,
    /// e o JSON traz o motivo.
    #[test]
    fn the_line_and_the_json_say_the_state_of_each_piece() {
        let (_dir, model) = saved(&["en-US"]);
        let got = of_map(&model).unwrap();
        let line = line("mapa", &got);
        assert!(line.starts_with("PECAS mapa: compilado=ainda-nao-ligada raiz-e-sinonimos=ainda-nao-ligada"), "{line}");
        assert!(line.ends_with("conferencia=ligada"), "{line}");
        let json = got[0].to_json();
        assert_eq!((json["name"].as_str(), json["state"].as_str(), json["on"].as_bool()), (Some("compilado"), Some("ainda não ligada"), Some(false)));
        assert!(!json["why"].as_str().unwrap().is_empty());
    }

    /// Um mapa que falta recusa em vez de listar peças que não conferiu.
    #[test]
    fn a_missing_map_is_refused() {
        let dir = tempdir().unwrap();
        let error = of_map(&model_path(dir.path())).unwrap_err().to_string();
        assert!(error.contains("não abriu"), "{error}");
    }
}
