//! O registro das falhas do projeto: uma linha por reprovação de onda com
//! motivo, em [`FAILURES_FILE`], fora do git como o resto da pasta do Mustard
//! no projeto. A linha nasce sem grupo; a passagem que pergunta ao Jev se ela
//! é a mesma falha de uma anterior dá o grupo, e o grupo que se repete ganha
//! a tarefa de criar a skill que a evita, marcada na linha que a pediu.
//!
//! As candidatas de uma falha nova ([`candidates`]) são as anteriores que
//! dividem com ela um arquivo ou a pasta de um arquivo: o filtro é do código,
//! e só elas vão ao Jev.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::io::fs::lock::LockedFile;

/// O registro das falhas, relativo à raiz do projeto.
pub const FAILURES_FILE: &str = ".claude/mustard/failures.ndjson";

/// Quantas falhas anteriores, no máximo, vão ao Jev como candidatas.
pub const CANDIDATES_MAX: usize = 10;

/// Uma reprovação de onda com motivo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    /// `<spec>:<número do evento>` do evento que trouxe a reprovação.
    pub id: String,
    pub spec: String,
    pub wave: u64,
    pub at: String,
    /// Quem reprovou: `wave_review`, `conductor` ou `final_review`.
    pub source: String,
    /// O motivo, sem segredo.
    pub reason: String,
    #[serde(default)]
    pub files: Vec<String>,
    /// O grupo da mesma falha: o `id` da primeira dele. Ausente enquanto a
    /// passagem não decidiu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// A tarefa de criar a skill que o grupo ganhou, marcada na linha que a
    /// pediu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_task: Option<SkillTask>,
}

impl Failure {
    /// O grupo da falha; sem grupo decidido, ela mesma.
    #[must_use]
    pub fn group_id(&self) -> &str {
        self.group.as_deref().unwrap_or(&self.id)
    }
}

/// A tarefa de criar a skill de um grupo: a spec e o número da tarefa. Sem
/// número, ela espera uma spec aberta onde nascer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTask {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub spec: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

/// O registro das falhas do projeto `root`.
#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(FAILURES_FILE)
}

/// As linhas legíveis de `text`, na ordem.
fn parse(text: &str) -> Vec<Failure> {
    text.lines().filter(|line| !line.trim().is_empty()).filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// As falhas do projeto `root`, na ordem em que foram gravadas. Sem o
/// arquivo, nenhuma; a linha que não se lê fica de fora.
#[must_use]
pub fn read(root: &Path) -> Vec<Failure> {
    parse(&crate::io::fs::read_to_string(path(root)).unwrap_or_default())
}

/// Troca, sob a trava do arquivo, as falhas do projeto `root` pelo que
/// `change` faz com elas, e devolve o que `change` devolveu.
///
/// # Errors
///
/// A falha de leitura ou de gravação do arquivo.
pub fn change<T>(root: &Path, change: impl FnOnce(&mut Vec<Failure>) -> T) -> crate::platform::error::Result<T> {
    let file = path(root);
    crate::io::fs::create_dir_all(file.parent().unwrap_or(root))?;
    let mut lock = LockedFile::exclusive(&file)?;
    let mut rows = parse(&lock.read_to_string()?);
    let before = rows.clone();
    let out = change(&mut rows);
    if rows != before {
        let mut body = String::new();
        for row in &rows {
            body.push_str(&serde_json::to_string(row).map_err(|e| crate::platform::error::Error::Config(e.to_string()))?);
            body.push('\n');
        }
        lock.replace(body.as_bytes())?;
    }
    Ok(out)
}

/// Acrescenta ao registro do projeto `root` as falhas de `new` que ele ainda
/// não tem, pelo `id`, e devolve as acrescentadas.
///
/// # Errors
///
/// A falha de leitura ou de gravação do arquivo.
pub fn append(root: &Path, new: &[Failure]) -> crate::platform::error::Result<Vec<Failure>> {
    if new.is_empty() {
        return Ok(Vec::new());
    }
    change(root, |rows| {
        let mut added = Vec::new();
        for failure in new {
            if !rows.iter().any(|row| row.id == failure.id) {
                rows.push(failure.clone());
                added.push(failure.clone());
            }
        }
        added
    })
}

/// A pasta de `file`, sem a raiz do projeto: o arquivo solto na raiz não dá
/// pasta, senão toda falha da raiz dividiria lugar com as outras.
fn folder(file: &str) -> Option<&str> {
    file.trim_end_matches('/').rsplit_once('/').map(|(folder, _)| folder).filter(|folder| !folder.is_empty())
}

/// `true` quando as duas listas dividem um arquivo ou a pasta de um arquivo.
#[must_use]
pub fn shares_place(a: &[String], b: &[String]) -> bool {
    a.iter().any(|one| {
        b.iter().any(|other| one == other || folder(one).is_some_and(|here| folder(other) == Some(here)))
    })
}

/// As candidatas da falha `new` em `all`: as anteriores a ela que dividem um
/// arquivo ou uma pasta com ela ([`shares_place`]), até [`CANDIDATES_MAX`],
/// as mais recentes, na ordem do registro.
#[must_use]
pub fn candidates<'a>(all: &'a [Failure], new: &Failure) -> Vec<&'a Failure> {
    let before = all.iter().position(|row| row.id == new.id).unwrap_or(all.len());
    let sharing: Vec<&Failure> = all[..before].iter().filter(|row| shares_place(&row.files, &new.files)).collect();
    sharing[sharing.len().saturating_sub(CANDIDATES_MAX)..].to_vec()
}

/// As falhas do grupo `group`, na ordem do registro.
#[must_use]
pub fn members<'a>(all: &'a [Failure], group: &str) -> Vec<&'a Failure> {
    all.iter().filter(|row| row.group.as_deref() == Some(group)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(id: &str, files: &[&str]) -> Failure {
        Failure {
            id: id.to_string(),
            spec: "x".into(),
            wave: 1,
            at: "2026-10-20T10:00:00Z".into(),
            source: "conductor".into(),
            reason: format!("motivo {id}"),
            files: files.iter().map(|file| (*file).to_string()).collect(),
            ..Failure::default()
        }
    }

    /// Só as anteriores que dividem um arquivo ou uma pasta com a falha nova
    /// são candidatas, e no máximo as dez mais recentes.
    #[test]
    fn candidates_share_a_file_or_folder_and_are_the_ten_most_recent() {
        let mut all = vec![
            failure("x:1", &["src/a.rs"]),
            failure("x:2", &["docs/b.md"]),
            failure("x:3", &["src/c.rs"]),
            failure("x:4", &["Cargo.toml"]),
        ];
        all.push(failure("x:5", &["src/a.rs", "Cargo.lock"]));
        let ids: Vec<&str> = candidates(&all, &all[4]).iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["x:1", "x:3"], "a root file is no shared folder, and a later failure is no candidate");

        let mut many: Vec<Failure> = (1..=12).map(|n| failure(&format!("y:{n}"), &["src/same.rs"])).collect();
        many.push(failure("y:13", &["src/same.rs"]));
        let picked: Vec<&str> = candidates(&many, &many[12]).iter().map(|row| row.id.as_str()).collect();
        assert_eq!(picked.len(), CANDIDATES_MAX);
        assert_eq!((picked[0], picked[9]), ("y:3", "y:12"));
    }

    /// O registro acrescenta só a falha que ele ainda não tem, e a troca
    /// grava o grupo no lugar.
    #[test]
    fn the_registry_appends_each_failure_once_and_changes_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(read(root).is_empty());
        let added = append(root, &[failure("x:1", &["src/a.rs"])]).unwrap();
        assert_eq!(added.len(), 1);
        assert!(append(root, &[failure("x:1", &["src/a.rs"])]).unwrap().is_empty());
        change(root, |rows| rows[0].group = Some("x:1".into())).unwrap();
        let rows = read(root);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].group.as_deref(), Some("x:1"));
        assert_eq!(members(&rows, "x:1").len(), 1);
    }
}
