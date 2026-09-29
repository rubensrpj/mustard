//! `map_format` — o mapa que outra compilação do scan gravou.
//!
//! Cada bloco que a passada do scan grava leva a marca da compilação que o
//! gravou: a versão e o resumo das fontes do scan. Uma compilação nova pode
//! tirar de um arquivo o que a anterior não tirava — o parâmetro do
//! construtor primário deixa de ser campo, por exemplo — sem que arquivo
//! nenhum do projeto mude. O commit e o conteúdo são os da passada, e só a
//! marca mostra que o mapa guarda menos, ou outra coisa, do que a compilação
//! de agora guardaria.
//!
//! A marca que se espera é a do scan que refaria o mapa: só ele a sabe, e o
//! núcleo a pede a ele ([`Scan::format`](crate::domain::scan::Scan::format)).
//! O scan, que só toma do mapa anterior o bloco de marca igual à dele, e
//! quem confere ([`crate::io::project_map::is_behind`]) julgam pela marca de
//! cada bloco de [`BLOCKS`].

use crate::io::map_db::MapDb;
use crate::io::project_map::BLOCKS;
use crate::platform::error::Result;

/// O mapa em `db` foi gravado por um scan que não é o de `format`: algum
/// bloco de [`BLOCKS`] traz uma marca, e ela é outra. `format` só se pede
/// quando há marca com que comparar. O bloco sem marca — o que voltou vazio
/// numa troca de formato — é de [`crate::io::map_fill::unfilled`], e o mapa
/// escrito à mão, sem marca em bloco nenhum, não tem passada de scan a que
/// comparar.
pub(crate) fn written_by_another(db: &MapDb, format: &dyn Fn() -> Option<String>) -> Result<bool> {
    let mut marks = Vec::new();
    for block in &BLOCKS {
        marks.extend(db.mark(block.name())?.filter(|mark| !mark.is_empty()));
    }
    if marks.is_empty() {
        return Ok(false);
    }
    Ok(format().is_some_and(|format| marks.iter().any(|mark| *mark != format)))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;

    use serde_json::{json, Value};
    use tempfile::{tempdir, TempDir};

    use crate::domain::normalize::Languages;
    use crate::io::project_map::{self, is_behind, model_path};

    fn git(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Um repositório com um commit e o mapa da passada que o leu, o mesmo
    /// mapa de uma função, gravado com a marca `mark`.
    fn mapped_by(mark: &str) -> (TempDir, Value) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        std::fs::write(root.join("a.txt"), "x").unwrap();
        git(root, &["add", "a.txt"]);
        git(root, &["commit", "-q", "-m", "semente"]);
        let now = project_map::listing(root).unwrap();
        let map = json!({
            "state": {"head": now.head, "listing": now.digest()},
            "modules": [{"path": "src/pedido.rs", "loc": 10, "declarations": [
                {"kind": "function", "name": "gravar_pedido", "line": 1, "end_line": 3}]}]
        });
        save(root, &map, mark);
        (dir, map)
    }

    fn save(root: &Path, map: &Value, mark: &str) {
        project_map::save_at(&model_path(root), map, mark, &Languages::new(["pt-BR", "en-US"])).unwrap();
    }

    /// Com o commit e o conteúdo da passada, o mapa só está atrás quando o
    /// scan que o refaria diz uma marca que não é a dos blocos: a mesma marca
    /// deixa o mapa em dia, outra o põe atrás sem arquivo nenhum mudar, e o
    /// scan que não responde não o julga. Gravado de novo pela marca de
    /// agora, ele volta a estar em dia.
    #[test]
    fn a_map_written_under_another_mark_is_behind_with_the_project_parked() {
        let (dir, map) = mapped_by("scan 1");
        let root = dir.path();
        assert!(!is_behind(root, &|| Some("scan 1".into())), "the same mark");
        assert!(is_behind(root, &|| Some("scan 2".into())), "another build of the scan, nothing else changed");
        assert!(!is_behind(root, &|| None), "a scan that does not answer does not judge the map");

        save(root, &map, "scan 2");
        assert!(!is_behind(root, &|| Some("scan 2".into())), "written again by the new mark");
        assert!(is_behind(root, &|| Some("scan 1".into())), "and behind for the old one");
    }

    /// Dentro do git e sem o arquivo do mapa, o mapa está atrás de tudo e a
    /// marca do scan nem se pede: falta criá-lo. Fora do git não há de onde
    /// ler o mapa, e ele não está atrás. Um arquivo que não é um banco
    /// continua sem julgamento, e criado o mapa, ele volta a estar em dia.
    #[test]
    fn a_project_in_git_without_a_map_is_behind_and_one_outside_git_is_not() {
        let asked = Cell::new(0);
        let format = || {
            asked.set(asked.get() + 1);
            Some("scan 1".to_string())
        };

        let (dir, map) = mapped_by("scan 1");
        let root = dir.path();
        assert!(!is_behind(root, &format), "the map of the project is there and up to date");
        asked.set(0);
        std::fs::remove_file(model_path(root)).unwrap();
        assert!(is_behind(root, &format), "the map was deleted: it has to be created");
        assert_eq!(asked.get(), 0, "there is no mark to compare");

        save(root, &map, "scan 1");
        assert!(!is_behind(root, &format), "created again by the pass, it is up to date");

        std::fs::write(model_path(root), "not a database").unwrap();
        assert!(!is_behind(root, &format), "a file that is not a database is not created over");

        let outside = tempdir().unwrap();
        assert!(!is_behind(outside.path(), &format), "no git, nothing to read the map from");
    }

    /// O scan só se pergunta quando o mapa traz marca com que comparar e o
    /// conteúdo ainda não o pôs atrás: sem mapa, com o mapa escrito à mão e
    /// com um arquivo editado, a marca não se pede.
    #[test]
    fn the_scan_is_only_asked_when_the_map_has_a_mark_to_compare() {
        let asked = Cell::new(0);
        let format = || {
            asked.set(asked.get() + 1);
            Some("scan 2".to_string())
        };

        let empty = tempdir().unwrap();
        git(empty.path(), &["init", "-q"]);
        assert!(is_behind(empty.path(), &format), "no map: it is missing, not stale by mark");

        let (dir, map) = mapped_by("scan 1");
        let root = dir.path();
        project_map::write_text(root, &map.to_string()).unwrap();
        assert!(!is_behind(root, &format), "a hand-written map has no pass to compare with");
        assert_eq!(asked.get(), 0, "neither a missing map nor a hand-written one asks the scan");

        save(root, &map, "scan 1");
        std::fs::write(root.join("a.txt"), "y").unwrap();
        assert!(is_behind(root, &format), "the edited file puts the map behind");
        assert_eq!(asked.get(), 0, "the content already answers");

        std::fs::write(root.join("a.txt"), "x").unwrap();
        assert!(is_behind(root, &format), "the content is back and the mark is another");
        assert_eq!(asked.get(), 1, "the scan is asked once");
    }
}
