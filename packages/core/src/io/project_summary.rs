//! O resumo do projeto no disco: uma linha por spec entregue em
//! [`SUMMARY_FILE`], e as regras que ela deixa ao Claude Code em
//! [`RULES_DIR`]. Os dois ficam fora do git como as specs: a pasta do Mustard
//! no projeto e `.claude/rules/mustard/` estão na marca da instalação
//! privada. As regras da pessoa em `.claude/rules/` nunca são tocadas.
//!
//! A passagem única das specs antigas deixa a marca em [`LEGACY_PASS_FILE`],
//! ao lado da lista delas.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::domain::project_summary::{Entry, RuleFile, RuleGroup, render_rules};
use crate::io::fs::lock::LockedFile;
use crate::platform::i18n::Locale;

/// O resumo do projeto, relativo à raiz.
pub const SUMMARY_FILE: &str = ".claude/mustard/project-summary.ndjson";

/// A pasta das regras que as specs entregues deixam, relativa à raiz.
pub const RULES_DIR: &str = ".claude/rules/mustard";

/// A marca da passagem única das specs antigas, relativa à raiz.
pub const LEGACY_PASS_FILE: &str = ".claude/mustard/legacy-summary.json";

/// As specs entregues cujas regras do projeto ainda esperam a decisão do
/// Jev, relativo à raiz.
pub const PENDING_RULES_FILE: &str = ".claude/mustard/rules-pending.json";

/// O resumo do projeto `root`.
#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(SUMMARY_FILE)
}

/// As linhas do resumo do projeto `root`, a entrega mais recente primeiro.
/// A linha que não se lê fica de fora; sem o arquivo, nenhuma.
#[must_use]
pub fn read(root: &Path) -> Vec<Entry> {
    let text = crate::io::fs::read_to_string(path(root)).unwrap_or_default();
    let mut entries = parse(&text);
    entries.sort_by(|a, b| moment(&b.delivered_at).cmp(&moment(&a.delivered_at)).then_with(|| a.spec.cmp(&b.spec)));
    entries
}

/// As linhas legíveis de `text`, na ordem.
fn parse(text: &str) -> Vec<Entry> {
    text.lines().filter(|line| !line.trim().is_empty()).filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// O instante de `at` para ordenar; a hora ilegível vem por último.
fn moment(at: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(at.trim()).ok()
}

/// Grava a linha `entry` no resumo do projeto `root`, no lugar da linha da
/// mesma spec quando ela já existe: a spec entregue de novo não se repete.
///
/// # Errors
///
/// A falha de leitura ou de gravação do arquivo.
pub fn upsert(root: &Path, entry: &Entry) -> crate::platform::error::Result<()> {
    let file = path(root);
    crate::io::fs::create_dir_all(file.parent().unwrap_or(root))?;
    let mut lock = LockedFile::exclusive(&file)?;
    let text = lock.read_to_string()?;
    let mut entries: Vec<Entry> = parse(&text).into_iter().filter(|old| old.spec != entry.spec).collect();
    entries.push(entry.clone());
    let mut body = String::new();
    for entry in &entries {
        body.push_str(&serde_json::to_string(entry).map_err(|e| crate::platform::error::Error::Config(e.to_string()))?);
        body.push('\n');
    }
    lock.replace(body.as_bytes())
}

/// O nome do `n`-ésimo arquivo de regras da spec `spec`.
fn rules_name(spec: &str, n: usize) -> String {
    format!("{spec}-{n}.md")
}

/// `true` quando `name` é um arquivo de regras da spec `spec`
/// (`<spec>-<n>.md`), e não o de outra spec cujo nome começa igual.
fn is_rules_of(spec: &str, name: &str) -> bool {
    name.strip_prefix(spec)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.strip_suffix(".md"))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// Grava as regras `groups` da spec `spec` em [`RULES_DIR`] do projeto
/// `root`, um arquivo por grupo, depois de tirar os que a spec deixou antes:
/// a spec entregue de novo troca as regras dela, e as de outras specs ficam.
/// Sem grupo, a spec fica sem arquivo. Devolve o que gravou.
///
/// # Errors
///
/// A falha de leitura da pasta ou de gravação de um arquivo.
pub fn write_rules(root: &Path, spec: &str, groups: &[RuleGroup], lang: Locale) -> crate::platform::error::Result<Vec<RuleFile>> {
    let dir = root.join(RULES_DIR);
    match crate::io::fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries.into_iter().filter(|entry| !entry.is_dir && is_rules_of(spec, &entry.file_name)) {
                crate::io::fs::remove_file(dir.join(&entry.file_name))?;
            }
        }
        Err(crate::platform::error::Error::NotFound(_)) => {}
        Err(error) => return Err(error),
    }
    let mut written = Vec::with_capacity(groups.len());
    for (at, group) in groups.iter().enumerate() {
        let name = rules_name(spec, at + 1);
        crate::io::fs::write_atomic(dir.join(&name), render_rules(spec, group, lang).as_bytes())?;
        written.push(RuleFile { file: format!("{RULES_DIR}/{name}"), paths: group.paths.clone(), rules: group.lines.clone() });
    }
    Ok(written)
}

/// A passagem única das specs antigas já foi feita no projeto `root`.
#[must_use]
pub fn legacy_pass_done(root: &Path) -> bool {
    crate::io::fs::exists(root.join(LEGACY_PASS_FILE))
}

/// Deixa a marca de que a passagem única das specs antigas foi feita no
/// projeto `root`, com a versão `version` e quantas specs ganharam linha.
///
/// # Errors
///
/// A falha de gravação do arquivo.
pub fn mark_legacy_pass(root: &Path, version: &str, summarized: usize) -> crate::platform::error::Result<()> {
    let body = json!({ "version": version, "at": chrono::Utc::now().to_rfc3339(), "summarized": summarized });
    crate::io::fs::write_atomic(root.join(LEGACY_PASS_FILE), format!("{body}\n").as_bytes())
}

/// As specs do projeto `root` cujas regras esperam a decisão do Jev, em
/// ordem. Sem o arquivo, ou com ele ilegível, nenhuma.
#[must_use]
pub fn pending_rules(root: &Path) -> Vec<String> {
    let text = crate::io::fs::read_to_string(root.join(PENDING_RULES_FILE)).unwrap_or_default();
    pending_in(&text).into_iter().collect()
}

/// As specs pendentes do texto `text` do arquivo.
fn pending_in(text: &str) -> std::collections::BTreeSet<String> {
    let value: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    value["specs"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_str).map(str::to_string).collect()
}

/// Troca, sob a trava do arquivo, as specs pendentes do projeto `root` pelo
/// que `change` faz com elas.
fn change_pending(root: &Path, change: impl FnOnce(&mut std::collections::BTreeSet<String>)) -> crate::platform::error::Result<()> {
    let file = root.join(PENDING_RULES_FILE);
    crate::io::fs::create_dir_all(file.parent().unwrap_or(root))?;
    let mut lock = LockedFile::exclusive(&file)?;
    let mut specs = pending_in(&lock.read_to_string()?);
    change(&mut specs);
    lock.replace(format!("{}\n", json!({ "specs": specs })).as_bytes())
}

/// Deixa as regras da spec `spec` do projeto `root` à espera da decisão do
/// Jev.
///
/// # Errors
///
/// A falha de leitura ou de gravação do arquivo.
pub fn add_pending_rules(root: &Path, spec: &str) -> crate::platform::error::Result<()> {
    change_pending(root, |specs| {
        specs.insert(spec.trim().to_string());
    })
}

/// Tira as specs `done` da espera, no projeto `root`.
///
/// # Errors
///
/// A falha de leitura ou de gravação do arquivo.
pub fn remove_pending_rules(root: &Path, done: &[String]) -> crate::platform::error::Result<()> {
    if done.is_empty() {
        return Ok(());
    }
    change_pending(root, |specs| specs.retain(|spec| !done.contains(spec)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(spec: &str, at: &str, title: &str) -> Entry {
        Entry {
            spec: spec.into(),
            delivered_at: at.into(),
            pr: None,
            title: title.into(),
            delivered: "Saiu.".into(),
            rules: vec![],
            skills: vec![],
            cost: None,
        }
    }

    /// A spec entregue de novo troca a própria linha, sem repetir; a leitura
    /// traz a entrega mais recente primeiro.
    #[test]
    fn the_same_spec_is_rewritten_in_place_and_the_newest_comes_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        upsert(root, &entry("a", "2026-10-01T10:00:00Z", "Primeira")).unwrap();
        upsert(root, &entry("b", "2026-10-02T10:00:00-03:00", "Segunda")).unwrap();
        upsert(root, &entry("a", "2026-10-03T10:00:00Z", "Primeira de novo")).unwrap();
        let text = std::fs::read_to_string(path(root)).unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        let read: Vec<(String, String)> = read(root).into_iter().map(|e| (e.spec, e.title)).collect();
        assert_eq!(read, vec![("a".into(), "Primeira de novo".into()), ("b".into(), "Segunda".into())]);
    }

    /// As regras da spec trocam as que ela deixou antes; as da spec de nome
    /// parecido e as da pessoa ficam.
    #[test]
    fn writing_the_rules_replaces_only_the_files_of_that_spec() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rules = root.join(RULES_DIR);
        std::fs::create_dir_all(&rules).unwrap();
        for name in ["obra-1.md", "obra-2.md", "obra-nova-1.md"] {
            std::fs::write(rules.join(name), "velha\n").unwrap();
        }
        std::fs::write(root.join(".claude/rules/minha.md"), "da pessoa\n").unwrap();
        let group = RuleGroup { paths: vec!["src/a.rs".into()], lines: vec!["Regra.".into()] };
        let written = write_rules(root, "obra", &[group], Locale::PtBr).unwrap();
        assert_eq!(written, vec![RuleFile { file: ".claude/rules/mustard/obra-1.md".into(), paths: vec!["src/a.rs".into()], rules: vec!["Regra.".into()] }]);
        let mut names: Vec<String> = std::fs::read_dir(&rules).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, vec!["obra-1.md", "obra-nova-1.md"]);
        assert!(std::fs::read_to_string(rules.join("obra-1.md")).unwrap().starts_with("---\npaths:\n  - \"src/a.rs\"\n---\n"));
        assert_eq!(std::fs::read_to_string(root.join(".claude/rules/minha.md")).unwrap(), "da pessoa\n");
        assert!(write_rules(root, "obra", &[], Locale::PtBr).unwrap().is_empty());
        assert!(!rules.join("obra-1.md").exists(), "a spec delivered again without rules leaves none");
    }

    /// A spec entra na espera uma vez e sai só quando é tirada; as outras
    /// ficam.
    #[test]
    fn the_pending_rules_keep_each_spec_once_until_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(pending_rules(root).is_empty());
        add_pending_rules(root, "b").unwrap();
        add_pending_rules(root, "a").unwrap();
        add_pending_rules(root, "b").unwrap();
        assert_eq!(pending_rules(root), vec!["a", "b"]);
        remove_pending_rules(root, &["b".to_string()]).unwrap();
        assert_eq!(pending_rules(root), vec!["a"]);
    }

    /// A marca da passagem única só existe depois de gravada.
    #[test]
    fn the_legacy_pass_is_marked_once_written() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!legacy_pass_done(dir.path()));
        mark_legacy_pass(dir.path(), "0.2.25", 2).unwrap();
        assert!(legacy_pass_done(dir.path()));
    }
}
