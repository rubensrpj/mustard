//! `spec_index` — o índice das specs no disco (`.claude/spec/index.ndjson`).
//!
//! Quem grava um evento numa pasta de spec do projeto refaz a linha daquela
//! spec no índice dentro da mesma gravação (`io::spec_events`), sem custo de
//! tokens. O [`rebuild`] refaz o índice inteiro a partir dos arquivos de
//! eventos, quando ele falta ou diverge, e recalcula o campo `search` das
//! linhas. O [`divergence`] só lê e diz onde o índice difere do que os
//! arquivos de eventos dariam.
//!
//! As travas são pegas sempre na mesma ordem: primeiro a da spec, depois a do
//! índice. Nenhum código segura a trava do índice enquanto espera a de uma
//! spec, então duas gravações ao mesmo tempo nunca se travam uma à outra.
//!
//! As regras de cada linha moram em `domain::spec_index`; aqui ficam o disco
//! e a trava.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::domain::spec_events::{self as model, Refusal, SpecLog};
use crate::domain::spec_index as index;
use crate::io::claude_paths::{ClaudePaths, SPEC_INDEX_FILE};
use crate::io::fs::lock::{read_shared, LockedFile};
use crate::platform::error::Error;

/// O nome do arquivo de eventos de cada spec.
const EVENTS_FILE: &str = "spec.ndjson";

/// O índice de um arquivo de eventos e o nome da spec dele, quando o caminho
/// tem a forma de uma spec do projeto: `<raiz>/.claude/spec/<nome>/spec.ndjson`.
/// Qualquer outro caminho não tem índice: um arquivo de eventos solto numa
/// pasta temporária nunca grava um índice fora dela.
#[must_use]
pub fn index_for(events: &Path) -> Option<(PathBuf, String)> {
    if events.file_name()? != EVENTS_FILE {
        return None;
    }
    let spec_dir = events.parent()?;
    let name = spec_dir.file_name()?.to_str()?;
    let specs = spec_dir.parent()?;
    let claude = specs.parent()?;
    if specs.file_name()? != "spec" || claude.file_name()? != ".claude" {
        return None;
    }
    Some((specs.join(SPEC_INDEX_FILE), name.to_string()))
}

/// Refaz a linha da spec `name` no índice `index_path` a partir de `log`, o
/// arquivo de eventos como acabou de ficar. Pega a trava do índice, e o
/// arquivo só é reescrito quando muda. Quem chama já segura a trava da spec.
pub fn refresh_line(index_path: &Path, name: &str, log: &SpecLog) -> Result<(), Refusal> {
    let line = index::spec_line(name, log);
    let mut file = LockedFile::exclusive(index_path).map_err(io_refusal)?;
    let current = file.read_to_string().map_err(io_refusal)?;
    let next = index::merge(&current, name, line.as_deref());
    if next != current {
        file.replace(next.as_bytes()).map_err(io_refusal)?;
    }
    Ok(())
}

/// O que o [`rebuild`] fez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebuilt {
    /// O caminho do índice.
    pub index: PathBuf,
    /// Quantas specs entraram no índice.
    pub specs: usize,
    /// Quantas linhas dos arquivos de eventos tiveram o `search` recalculado.
    pub search_updated: usize,
    /// Quantas linhas do banco de lições tiveram o `search` recalculado.
    pub lessons_search_updated: usize,
    /// As pastas de `.claude/spec/` que ficaram fora: sem arquivo de eventos
    /// (o formato antigo) ou com um nome que não serve para spec.
    pub skipped: Vec<String>,
}

/// Refaz o índice inteiro do projeto `root` a partir dos arquivos de eventos.
///
/// Para cada spec, em ordem de nome: pega a trava do arquivo de eventos,
/// recalcula o `search` das linhas (e reescreve o arquivo só se algo mudou),
/// refaz a linha dela no índice e solta a trava. Por último, só com a trava
/// do índice, tira as linhas de specs que não existem mais e as que não se
/// entendem, e garante a linha do projeto, que fica como estava.
///
/// Sem spec, grava só a linha do projeto. Recusa só quando a trava ou a
/// escrita falham, com [`Refusal::Io`].
pub fn rebuild(root: &Path) -> Result<Rebuilt, Refusal> {
    let paths = ClaudePaths::for_project(root).map_err(|e| Refusal::Io { detail: e.to_string() })?;
    let index_path = paths.spec_index_path();
    let listing = list_specs(&paths)?;
    let mut out = Rebuilt {
        index: index_path.clone(),
        specs: 0,
        search_updated: 0,
        lessons_search_updated: 0,
        skipped: listing.skipped,
    };
    for (name, events) in &listing.specs {
        let mut file = match LockedFile::existing(events) {
            Ok(file) => file,
            Err(Error::NotFound(_)) => {
                out.skipped.push(name.clone());
                continue;
            }
            Err(e) => return Err(io_refusal(e)),
        };
        let content = file.read_to_string().map_err(io_refusal)?;
        let (fixed, changed) = model::refresh_search_lines(&content);
        if changed > 0 {
            file.replace(fixed.as_bytes()).map_err(io_refusal)?;
        }
        refresh_line(&index_path, name, &model::parse_log(&fixed))?;
        drop(file);
        out.specs += 1;
        out.search_updated += changed;
    }

    let mut file = LockedFile::exclusive(&index_path).map_err(io_refusal)?;
    let current = file.read_to_string().map_err(io_refusal)?;
    let next = index::prune(&current, |name| paths.for_spec(name).is_ok_and(|s| s.spec_ndjson_path().is_file()));
    if next != current {
        file.replace(next.as_bytes()).map_err(io_refusal)?;
    }
    drop(file);
    out.skipped.sort();
    out.skipped.dedup();
    Ok(out)
}

/// Onde o índice difere do que os arquivos de eventos dariam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// Quantas specs têm arquivo de eventos.
    pub specs: usize,
    /// O índice existe.
    pub index_exists: bool,
    /// As specs cuja linha falta, sobra ou é outra, e `#<n>` para cada linha
    /// que não se entende (veja `domain::spec_index::diff`). Vazio quando o
    /// índice não existe.
    pub diverged: Vec<String>,
    /// Quantas linhas, nos arquivos de eventos e no banco de lições, têm o
    /// `search` calculado por outro redutor.
    pub stale_search: usize,
}

/// Monta em memória o índice que os arquivos de eventos dariam e compara com
/// o gravado. Só lê, cada arquivo com a trava compartilhada, e nunca segura
/// duas travas ao mesmo tempo.
pub fn divergence(root: &Path) -> Result<Divergence, Refusal> {
    let paths = ClaudePaths::for_project(root).map_err(|e| Refusal::Io { detail: e.to_string() })?;
    let listing = list_specs(&paths)?;
    let mut lines = BTreeMap::new();
    let mut specs = 0usize;
    let mut stale_search = 0usize;
    for (name, events) in &listing.specs {
        let content = match read_shared(events) {
            Ok(content) => content,
            Err(Error::NotFound(_)) => continue,
            Err(e) => return Err(io_refusal(e)),
        };
        specs += 1;
        stale_search += model::refresh_search_lines(&content).1;
        if let Some(line) = index::spec_line(name, &model::parse_log(&content)) {
            lines.insert(name.clone(), line);
        }
    }
    let (current, index_exists) = match read_shared(&paths.spec_index_path()) {
        Ok(content) => (content, true),
        Err(Error::NotFound(_)) => (String::new(), false),
        Err(e) => return Err(io_refusal(e)),
    };
    let diverged = if index_exists { index::diff(&current, &index::canonical(&current, &lines)) } else { Vec::new() };
    Ok(Divergence { specs, index_exists, diverged, stale_search })
}

/// As pastas de `.claude/spec/`, em ordem de nome.
struct Listing {
    /// O nome e o arquivo de eventos de cada spec que tem um.
    specs: Vec<(String, PathBuf)>,
    /// As pastas sem arquivo de eventos ou com nome que não serve.
    skipped: Vec<String>,
}

fn list_specs(paths: &ClaudePaths) -> Result<Listing, Refusal> {
    let mut entries = match crate::io::fs::read_dir(paths.spec_dir()) {
        Ok(entries) => entries,
        Err(Error::NotFound(_)) => Vec::new(),
        Err(e) => return Err(io_refusal(e)),
    };
    entries.retain(|e| e.is_dir);
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    let mut listing = Listing { specs: Vec::new(), skipped: Vec::new() };
    for entry in entries {
        match paths.for_spec(&entry.file_name).map(|s| s.spec_ndjson_path()) {
            Ok(events) if events.is_file() => listing.specs.push((entry.file_name, events)),
            _ => listing.skipped.push(entry.file_name),
        }
    }
    Ok(listing)
}

fn io_refusal(error: Error) -> Refusal {
    Refusal::Io { detail: error.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::spec_events::{write_at, Written};
    use serde_json::{json, Map, Value};

    fn obj(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    fn at(hm: &str) -> String {
        format!("2026-09-11T{hm}:00-03:00")
    }

    fn events(root: &Path, spec: &str) -> PathBuf {
        root.join(".claude").join("spec").join(spec).join("spec.ndjson")
    }

    fn index_file(root: &Path) -> PathBuf {
        root.join(".claude").join("spec").join("index.ndjson")
    }

    fn put(root: &Path, spec: &str, event_type: &str, hm: &str, draft: Value) -> Written {
        write_at(&events(root, spec), event_type, obj(draft), &[], &at(hm))
            .unwrap_or_else(|r| panic!("{event_type} was refused: {r:?}"))
    }

    fn line_of(root: &Path, spec: &str) -> Value {
        let raw = std::fs::read_to_string(index_file(root)).unwrap();
        let line = raw
            .lines()
            .find(|l| l.contains(&format!("\"name\":\"{spec}\"")))
            .unwrap_or_else(|| panic!("no line for {spec}: {raw}"));
        serde_json::from_str(line).unwrap()
    }

    /// Duas specs com alguns eventos, gravadas pelo gravador de verdade.
    fn two_specs(root: &Path) {
        put(root, "trava", "state", "08:40", json!({"author": "binary", "phase": "survey", "branch": "feature/trava", "base": "dev"}));
        put(root, "trava", "message", "08:41", json!({"author": "user", "text": "A trava confere o programa"}));
        put(root, "trava", "context", "08:42", json!({"text": "Barrar comando que apaga trabalho. Sem falso positivo.", "origin": 2}));
        put(root, "trava", "rule", "08:43", json!({"text": "A trava confere o programa, nunca o texto entre aspas.", "example": "e", "keys": ["trava"], "origin": 2}));
        put(root, "busca", "message", "09:00", json!({"author": "user", "text": "Quero buscar lições"}));
        put(root, "busca", "decision", "09:01", json!({"text": "**BM25.** Sem embeddings.", "why": "w", "keys": ["busca"], "origin": 1}));
    }

    /// A gravação de um evento muda a linha da spec no índice na mesma
    /// gravação, e só a dela.
    #[test]
    fn a_write_changes_the_spec_line_in_the_index_in_the_same_write() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        two_specs(root);
        let before = line_of(root, "trava");
        assert_eq!(before["updated"], json!(at("08:43")));
        assert_eq!(before["phase"], json!("survey"));
        let other = line_of(root, "busca");

        put(root, "trava", "state", "10:00", json!({"author": "binary", "phase": "plan"}));
        let after = line_of(root, "trava");
        assert_eq!(after["updated"], json!(at("10:00")));
        assert_eq!(after["phase"], json!("plan"));
        assert_eq!(after["goal"], json!("Barrar comando que apaga trabalho."));
        assert_eq!(line_of(root, "busca"), other, "the other spec's line did not move");
        let raw = std::fs::read_to_string(index_file(root)).unwrap();
        assert!(raw.starts_with("{\"v\":1,\"type\":\"project\"}\n"), "{raw}");
    }

    /// Apagado o índice, o `rebuild` devolve o arquivo com os mesmos bytes
    /// que as gravações tinham deixado; rodar de novo não muda nada.
    #[test]
    fn rebuilding_a_deleted_index_gives_back_the_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        two_specs(root);
        let written = std::fs::read(index_file(root)).unwrap();
        std::fs::remove_file(index_file(root)).unwrap();

        let rebuilt = rebuild(root).unwrap();
        assert_eq!((rebuilt.specs, rebuilt.search_updated), (2, 0));
        assert_eq!(std::fs::read(index_file(root)).unwrap(), written);
        rebuild(root).unwrap();
        assert_eq!(std::fs::read(index_file(root)).unwrap(), written);
        let quiet = divergence(root).unwrap();
        assert_eq!(quiet, Divergence { specs: 2, index_exists: true, diverged: Vec::new(), stale_search: 0 });
    }

    /// Um `search` calculado por outro redutor é recalculado; as outras
    /// linhas do arquivo de eventos ficam byte a byte.
    #[test]
    fn rebuilding_recomputes_a_stale_search_and_leaves_the_other_lines_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        two_specs(root);
        let path = events(root, "trava");
        let original = std::fs::read_to_string(&path).unwrap();
        let rule = original.lines().find(|l| l.contains("\"type\":\"rule\"")).unwrap().to_string();
        let (head, _) = rule.rsplit_once(",\"search\":").unwrap();
        let stale = format!("{head},\"search\":\"redutor antigo\"}}");
        std::fs::write(&path, original.replace(&rule, &stale)).unwrap();
        assert_eq!(divergence(root).unwrap().stale_search, 1);

        let rebuilt = rebuild(root).unwrap();
        assert_eq!(rebuilt.search_updated, 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original, "only the stale line changed back");
        assert_eq!(divergence(root).unwrap().stale_search, 0);
    }

    #[test]
    fn the_project_line_is_kept_when_the_index_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        two_specs(root);
        let raw = std::fs::read_to_string(index_file(root)).unwrap();
        let project = index::project_line(Some("https://example.com/projeto"));
        let edited = raw.replacen(&index::project_line(None), &project, 1);
        std::fs::write(index_file(root), format!("{edited}lixo\n")).unwrap();
        assert_eq!(divergence(root).unwrap().diverged, ["#4"]);

        rebuild(root).unwrap();
        assert_eq!(std::fs::read_to_string(index_file(root)).unwrap(), edited, "the address stays, the garbage goes");
    }

    /// Uma pasta do formato antigo, sem arquivo de eventos, fica fora do
    /// índice e aparece em `skipped`; uma spec apagada perde a linha.
    #[test]
    fn a_folder_without_an_event_file_stays_out_of_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        two_specs(root);
        std::fs::create_dir_all(root.join(".claude").join("spec").join("velha").join(".events")).unwrap();
        std::fs::remove_dir_all(root.join(".claude").join("spec").join("busca")).unwrap();
        assert_eq!(divergence(root).unwrap().diverged, ["busca"]);

        let rebuilt = rebuild(root).unwrap();
        assert_eq!(rebuilt.specs, 1);
        assert_eq!(rebuilt.skipped, ["velha"]);
        let raw = std::fs::read_to_string(index_file(root)).unwrap();
        assert_eq!(raw.lines().count(), 2, "{raw}");
        assert!(!raw.contains("velha") && !raw.contains("busca"), "{raw}");
    }

    #[test]
    fn a_project_without_specs_gets_only_the_project_line() {
        let dir = tempfile::tempdir().unwrap();
        let rebuilt = rebuild(dir.path()).unwrap();
        assert_eq!((rebuilt.specs, rebuilt.skipped.len()), (0, 0));
        assert_eq!(std::fs::read_to_string(index_file(dir.path())).unwrap(), format!("{}\n", index::project_line(None)));
        assert_eq!(divergence(dir.path()).unwrap().specs, 0);
    }

    /// Duas specs gravadas ao mesmo tempo: as duas linhas chegam ao índice, e
    /// cada uma é a do último estado do arquivo dela.
    #[test]
    fn two_specs_written_at_once_both_land_in_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        let writers: Vec<_> = ["um", "dois"]
            .into_iter()
            .map(|spec| {
                let root = root.clone();
                let start = std::sync::Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    for i in 0..15 {
                        let text = format!("gravação {i}");
                        put(&root, spec, "message", &format!("10:{i:02}"), json!({"author": "user", "text": text}));
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        assert_eq!(line_of(&root, "um")["updated"], json!(at("10:14")));
        assert_eq!(line_of(&root, "dois")["updated"], json!(at("10:14")));
        assert!(divergence(&root).unwrap().diverged.is_empty());
    }

    /// Um arquivo de eventos fora da forma `.claude/spec/<nome>/spec.ndjson`
    /// não grava índice nenhum.
    #[test]
    fn a_write_outside_the_spec_folder_shape_touches_no_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let loose = [
            root.join("spec.ndjson"),
            root.join("spec").join("n").join("spec.ndjson"),
            root.join(".claude").join("specs").join("n").join("spec.ndjson"),
            root.join(".claude").join("spec").join("n").join("eventos.ndjson"),
        ];
        for path in &loose {
            assert!(index_for(path).is_none(), "{}", path.display());
            let written = write_at(path, "message", obj(json!({"author": "user", "text": "solto"})), &[], &at("10:00")).unwrap();
            assert!(written.index_warning.is_none());
        }
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.file_name().is_some_and(|n| n == SPEC_INDEX_FILE) {
                    found.push(path);
                }
            }
        }
        assert!(found.is_empty(), "{found:?}");
        assert_eq!(index_for(&events(root, "s")), Some((index_file(root), "s".to_string())));
    }

    /// Quando o índice não pode ser gravado, o evento fica gravado e a
    /// gravação diz por quê.
    #[test]
    fn an_index_that_cannot_be_written_leaves_the_event_written_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(index_file(root)).unwrap();
        let written = put(root, "s", "message", "10:00", json!({"author": "user", "text": "fica gravado"}));
        assert!(matches!(written.index_warning, Some(Refusal::Io { .. })), "{written:?}");
        assert!(std::fs::read_to_string(events(root, "s")).unwrap().contains("fica gravado"));
        assert!(matches!(rebuild(root), Err(Refusal::Io { .. })));
        assert!(matches!(divergence(root), Err(Refusal::Io { .. })));
    }
}
