//! O perfil observado do projeto, guardado no mapa. Ele lê o projeto inteiro,
//! e por isso a gravação que troca só as linhas de alguns arquivos não o
//! refaz: deixa-o marcado como atrasado. Quem o lê enquanto ele está atrasado
//! o calcula na hora, sem gravar, e a passada seguinte que não muda nada o põe
//! em dia, uma vez só para todas as gravações que passaram.

use rusqlite::Connection;
use serde_json::Value;

use super::{MapGraph, PROJECT_PROFILE, ProjectMap, example_modules, fan_in, languages, projects, skeleton};
use crate::io::map_db;
use crate::platform::error::Result;

/// A marca do bloco do perfil quando uma gravação o deixou para depois.
const BEHIND: &str = "behind";

/// Deixa o perfil para depois, na transação `tx` da gravação.
pub(super) fn leave_behind(tx: &Connection) -> Result<()> {
    map_db::set_mark(tx, PROJECT_PROFILE.name(), BEHIND)
}

/// Se uma gravação deixou o perfil do mapa aberto em `conn` para depois.
pub(crate) fn left_behind(conn: &Connection) -> bool {
    map_db::mark_of(conn, PROJECT_PROFILE.name()).is_ok_and(|mark| mark.as_deref() == Some(BEHIND))
}

/// Se o perfil guardado não vale para o mapa: falta, é de outra versão, não
/// se pôde fazer ou ficou para depois.
pub(super) fn stale(conn: &Connection) -> bool {
    left_behind(conn) || missing(conn)
}

/// Se o mapa aberto em `conn` não guarda um perfil que se leia: falta, é de
/// outra versão ou não se pôde fazer. Sem ele, não há o que deixar para
/// depois, e a gravação o faz na hora.
pub(super) fn missing(conn: &Connection) -> bool {
    let profile: Option<String> = conn.query_row("SELECT payload FROM project_profile LIMIT 1", [], |row| row.get(0)).ok();
    profile.as_deref().and_then(|text| serde_json::from_str::<Value>(text).ok()).is_none_or(|data| {
        data["version"] != crate::io::project_profile::VERSION || data["status"] == "unavailable"
    })
}

/// O perfil do mapa aberto em `conn`, calculado na hora: a impressão digital
/// e o conteúdo.
pub(crate) fn observed(conn: &Connection) -> Result<(String, Value)> {
    // Read compact structural columns, never the per-file analysis/body packs.
    let map = ProjectMap {
        modules: example_modules(conn, false)?,
        projects: projects(conn)?,
        languages: languages(conn)?,
        skeleton: skeleton(conn)?,
        graph: MapGraph { top_fan_in: fan_in(conn)? },
        ..ProjectMap::default()
    };
    crate::io::project_profile::observe(conn, &map)
}

/// Grava o perfil do mapa, na transação `conn`, e tira a marca de atrasado.
pub(super) fn sync(conn: &Connection) -> Result<()> {
    let result = observed(conn).and_then(|(fingerprint, payload)| crate::io::project_profile::store(conn, &fingerprint, &payload));
    if result.is_err() {
        // A derived projection cannot break writes/reads of independent map
        // blocks. Withdraw its old evidence rather than leave a stale profile
        // appearing current when a structural column cannot be interpreted.
        conn.execute("DELETE FROM project_profile", [])?;
        conn.execute(
            "INSERT INTO project_profile(fingerprint,payload) VALUES ('unavailable',?1)",
            ["{\"status\":\"unavailable\",\"reason\":\"incomplete-profile-input\",\"model_calls\":0}"],
        )?;
    }
    map_db::set_mark(conn, PROJECT_PROFILE.name(), "")
}
