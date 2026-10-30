//! O que sobra da obra quando ela sai, pelo fechamento, pelo descarte ou
//! pela entrega feita fora da porta do pull request: os avisos do que a
//! remoção das cópias da obra fez e a remoção da pasta de compilação que o
//! projeto declarou descartável na pasta principal.
//!
//! Este módulo não chama o fluxo nem o painel: quem tira as cópias entrega o
//! resultado pelo [`CopiesRemoved`], e o fechamento, o descarte e a entrega
//! chamam daqui sem passar um pelo outro.

use std::path::Path;

use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

/// O que a remoção das cópias da obra fez, como os avisos dela o leem.
pub(crate) trait CopiesRemoved {
    /// Cada cópia que não saiu, com o motivo que o git deu.
    fn left(&self) -> &[(String, String)];
    /// Cada cópia que ficou intacta porque o código que ela tem além do
    /// commit não pôde ser guardado antes, com o motivo.
    fn unkept(&self) -> &[(String, String)];
    /// O texto de cada código que a remoção guardou antes de apagar, no
    /// idioma `lang`: de quem era, onde ficou e como trazê-lo de volta.
    fn kept_hints(&self, lang: Locale) -> Vec<String>;
}

/// O aviso das cópias da obra que não saíram (`left`, cada uma com o motivo
/// que o git deu), no idioma `lang`. `None` quando todas saíram.
fn copies_kept_hint(left: &[(String, String)], lang: Locale) -> Option<String> {
    if left.is_empty() {
        return None;
    }
    let copies: Vec<String> = left.iter().map(|(copy, _)| format!("`{copy}`")).collect();
    let details: Vec<&str> = left.iter().map(|(_, detail)| detail.as_str()).collect();
    Some(translate("close.copies_kept", lang).replace("{copies}", &copies.join(", ")).replace("{detail}", &details.join("; ")))
}

/// Os avisos do que a remoção das cópias da obra fez, cada um com o motivo da
/// resposta: as cópias que não saíram, as que ficaram porque o código delas
/// não pôde ser guardado, e o código que ficou guardado, com o comando para
/// trazê-lo de volta.
pub(crate) fn removal_warnings(removal: &impl CopiesRemoved, lang: Locale) -> Vec<(&'static str, String)> {
    let mut warnings = Vec::new();
    if let Some(hint) = copies_kept_hint(removal.left(), lang) {
        warnings.push(("copies-kept", hint));
    }
    if !removal.unkept().is_empty() {
        let copies: Vec<String> = removal.unkept().iter().map(|(copy, _)| format!("`{copy}`")).collect();
        let details: Vec<&str> = removal.unkept().iter().map(|(_, detail)| detail.as_str()).collect();
        let hint = translate("close.code_not_kept", lang).replace("{copies}", &copies.join(", ")).replace("{detail}", &details.join("; "));
        warnings.push(("code-not-kept", hint));
    }
    warnings.extend(removal.kept_hints(lang).into_iter().map(|hint| ("code-kept", hint)));
    warnings
}

/// O que [`remove_build_output`] fez com as pastas de compilação declaradas:
/// as que saíram, como o projeto as declarou, e o aviso de cada uma que ficou.
pub(crate) struct BuildOutputSwept {
    removed: Vec<String>,
    kept: Vec<String>,
}

impl BuildOutputSwept {
    /// Põe na resposta `out` as pastas apagadas (`build_output_removed`) e um
    /// aviso para cada uma que ficou no disco.
    pub(crate) fn tell(&self, out: &mut Value) {
        if !self.removed.is_empty() {
            out["build_output_removed"] = json!(self.removed);
        }
        for hint in &self.kept {
            crate::commands::spec_events::pages::push_warning(out, "build-output-kept", hint);
        }
    }
}

/// Apaga da pasta principal `root` cada pasta de compilação que o projeto
/// declarou descartável (`buildOutput`, no `mustard.json`), no fim do
/// descarte, depois que as cópias da obra saíram. O fechamento não a chama. Nada mais
/// da pasta principal é apagado, e sem a declaração nada sai.
///
/// A pasta só sai quando é um caminho relativo que fica dentro da raiz, que
/// não é a raiz nem o `.git`, que o git ignora e onde ele não guarda arquivo
/// nenhum; a que não passa fica, com um aviso. A que não existe não é erro. A
/// remoção segura a trava do passo do git, a mesma que a rodada segura ao
/// compilar antes do commit, e assim não corta a compilação de outra rodada.
/// A falha ao apagar vira aviso e não derruba quem chamou.
pub(crate) fn remove_build_output(root: &Path, lang: Locale) -> BuildOutputSwept {
    remove_build_output_with(root, lang, &folder_bytes)
}

/// Decimal GB, per declared principal build folder. Worktree retention is
/// independent of this policy.
const BUILD_RETENTION_BYTES: u64 = 15_000_000_000;

fn folder_bytes(path: &Path) -> std::io::Result<u64> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(0);
    }
    if meta.is_file() {
        return Ok(meta.len());
    }
    let mut total = 0_u64;
    for entry in std::fs::read_dir(path)? {
        total = total.checked_add(folder_bytes(&entry?.path())?).ok_or_else(|| std::io::Error::other("build folder size overflow"))?;
    }
    Ok(total)
}

fn remove_build_output_with(root: &Path, lang: Locale, measure: &dyn Fn(&Path) -> std::io::Result<u64>) -> BuildOutputSwept {
    let mut swept = BuildOutputSwept { removed: Vec::new(), kept: Vec::new() };
    let mut held = None;
    for folder in mustard_core::ProjectConfig::load(root).commands().build_output {
        let warn = |key: &str| translate(key, lang).replace("{folder}", &folder);
        let Some(rel) = declared_folder(&folder) else {
            swept.kept.push(warn("close.build_output_unsafe"));
            continue;
        };
        let path = root.join(&rel);
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        if !path.is_dir() || !resolves_inside(root, &path) {
            swept.kept.push(warn("close.build_output_unsafe"));
            continue;
        }
        if !ignored_and_untracked(root, &rel) {
            swept.kept.push(warn("close.build_output_not_ignored"));
            continue;
        }
        if held.is_none() {
            match crate::commands::git_settle::git_step_lock(root) {
                Ok(lock) => held = Some(lock),
                Err(detail) => {
                    swept.kept.push(warn("close.build_output_failed").replace("{detail}", &detail));
                    continue;
                }
            }
        }
        // Recheck after waiting: a concurrent operation may have replaced
        // this path. Never measure/follow a declared symlink.
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) || !resolves_inside(root, &path) || !ignored_and_untracked(root, &rel) {
            swept.kept.push(warn("close.build_output_unsafe"));
            continue;
        }
        match measure(&path) {
            Ok(bytes) if bytes <= BUILD_RETENTION_BYTES => {
                swept.kept.push(warn("close.build_output_retained").replace("{bytes}", &bytes.to_string()));
                continue;
            }
            Err(err) => {
                swept.kept.push(warn("close.build_output_failed").replace("{detail}", &err.to_string()));
                continue;
            }
            Ok(_) => {}
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => swept.removed.push(folder.clone()),
            Err(err) => swept.kept.push(warn("close.build_output_failed").replace("{detail}", &err.to_string())),
        }
    }
    swept
}

/// A pasta declarada `folder` como caminho relativo à raiz, com as partes
/// separadas por `/`. `None` quando ela sai da raiz (caminho absoluto ou com
/// `..`), quando é a própria raiz ou quando passa pelo `.git`.
fn declared_folder(folder: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in Path::new(folder).components() {
        match part {
            std::path::Component::Normal(name) if !name.to_string_lossy().eq_ignore_ascii_case(".git") => {
                parts.push(name.to_string_lossy().to_string());
            }
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// A pasta `path`, com cada atalho resolvido, fica dentro da raiz `root`, não
/// é a raiz e não passa pelo `.git`: um atalho no caminho não leva a remoção
/// para fora do projeto.
fn resolves_inside(root: &Path, path: &Path) -> bool {
    let (Ok(root), Ok(path)) = (std::fs::canonicalize(root), std::fs::canonicalize(path)) else {
        return false;
    };
    path.strip_prefix(&root)
        .is_ok_and(|rel| {
        rel.components().next().is_some() && rel.components().all(|part| !part.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
    })
}

/// O git da raiz `root` ignora a pasta `rel` e não guarda nenhum arquivo
/// dentro dela. Sem git, a pasta não passa.
fn ignored_and_untracked(root: &Path, rel: &str) -> bool {
    use mustard_core::platform::git;
    if !git::run(root, &["check-ignore", "-q", "--", rel]).ok {
        return false;
    }
    git::run(root, &["--literal-pathspecs", "ls-files", "-z", "--", rel]).out().is_some_and(|tracked| tracked.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// Roda o `git` em `root` e falha o teste se o git recusar.
    fn git_at(root: &Path, args: &[&str]) {
        assert!(mustard_core::platform::git::run(root, args).ok, "git {args:?}");
    }

    /// Um projeto git com uma pasta de compilação `target` que o git ignora,
    /// com um arquivo dentro, e o `mustard.json` com a declaração `config`.
    fn project_with_build(root: &Path, config: &str) -> PathBuf {
        crate::shared::test_fixture::seeded_repo(root, &[("src/lib.rs", "fn one() {}\n"), (".gitignore", "target/\n")]);
        std::fs::write(root.join("mustard.json"), config).unwrap();
        std::fs::create_dir_all(root.join("target").join("debug")).unwrap();
        let built = root.join("target").join("debug").join("programa");
        std::fs::write(&built, "compilado").unwrap();
        built
    }

    /// Os avisos de pasta que ficou, na resposta `swept` posta num objeto.
    fn kept_of(swept: &BuildOutputSwept) -> Vec<String> {
        let mut out = json!({});
        swept.tell(&mut out);
        out["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|w| w["reason"] == json!("build-output-kept"))
            .filter_map(|w| w["hint"].as_str().map(str::to_string))
            .collect()
    }

    /// Sem a declaração no `mustard.json`, nada da pasta principal sai, nem
    /// a pasta de compilação que o git ignora; a declaração vazia também não
    /// apaga nada.
    #[test]
    fn without_a_declaration_nothing_is_removed() {
        for config in ["{}", r#"{"buildOutput":[]}"#] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let built = project_with_build(root, config);
            let swept = remove_build_output(root, Locale::PtBr);
            assert!(swept.removed.is_empty() && swept.kept.is_empty(), "{config}");
            assert!(built.is_file(), "{config}: the build stays");
        }
    }

    #[test]
    fn build_retention_has_a_decimal_boundary_and_keeps_unknown_measurements() {
        for bytes in [BUILD_RETENTION_BYTES - 1, BUILD_RETENTION_BYTES, BUILD_RETENTION_BYTES + 1] {
            let dir = tempdir().unwrap();
            let built = project_with_build(dir.path(), r#"{"buildOutput":["target"]}"#);
            let swept = remove_build_output_with(dir.path(), Locale::PtBr, &|_| Ok(bytes));
            assert_eq!(built.is_file(), bytes <= BUILD_RETENTION_BYTES);
            assert_eq!(swept.removed.is_empty(), bytes <= BUILD_RETENTION_BYTES);
        }
        let dir = tempdir().unwrap();
        let built = project_with_build(dir.path(), r#"{"buildOutput":["target"]}"#);
        let swept = remove_build_output_with(dir.path(), Locale::PtBr, &|_| Err(std::io::Error::other("unreadable")));
        assert!(built.is_file() && swept.removed.is_empty());
        assert!(kept_of(&swept)[0].contains("unreadable"));
    }

    /// A declaração que sai da raiz, que é a própria raiz, que passa pelo
    /// `.git`, que é um caminho absoluto ou que tem `..` — mesmo quando ele
    /// acaba numa pasta ignorada dentro da raiz — não apaga nada: fica e vira
    /// o aviso do caminho. A que aponta uma pasta que o git não ignora ou uma
    /// pasta com arquivo versionado também fica, com o aviso do git. A pasta
    /// que não existe não é erro nem aviso. A pasta boa da mesma lista sai.
    #[test]
    fn an_unsafe_declaration_removes_nothing_and_warns() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("obra");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let outside = dir.path().join("fora");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("arquivo"), "x").unwrap();
        let built = project_with_build(root, "{}");
        // Ignorada, mas com um arquivo versionado dentro.
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/versionado"), "x").unwrap();
        git_at(root, &["add", "-f", "dist/versionado"]);
        git_at(root, &["commit", "-q", "-m", "dist"]);
        // Ignorada e sem nada versionado, alcançada só por um caminho torto.
        std::fs::write(root.join(".gitignore"), "target/\ndist/\nlixo/\n").unwrap();
        std::fs::create_dir_all(root.join("lixo")).unwrap();
        std::fs::write(root.join("lixo/sobra"), "x").unwrap();
        // Não ignorada.
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::write(root.join("build/saida"), "x").unwrap();
        let outside_abs = outside.to_string_lossy().to_string();
        let inside_abs = root.join("lixo").to_string_lossy().to_string();
        let by_path = ["../fora", outside_abs.as_str(), inside_abs.as_str(), "src/../lixo", ".", "./", ".git", "src/../.."];
        let by_git = ["dist", "build"];
        let declared: Vec<&str> = by_path.iter().chain(&by_git).copied().chain(["nao-existe", "target"]).collect();
        std::fs::write(root.join("mustard.json"), json!({"buildOutput": declared}).to_string()).unwrap();

        let swept = remove_build_output_with(root, Locale::PtBr, &|_| Ok(BUILD_RETENTION_BYTES + 1));
        assert_eq!(swept.removed, vec!["target".to_string()], "only the good folder leaves");
        assert!(!built.exists());
        let kept = kept_of(&swept);
        let hint = |key: &str, folder: &str| translate(key, Locale::PtBr).replace("{folder}", folder);
        let expected: Vec<String> = by_path
            .iter()
            .map(|folder| hint("close.build_output_unsafe", folder))
            .chain(by_git.iter().map(|folder| hint("close.build_output_not_ignored", folder)))
            .collect();
        assert_eq!(kept, expected, "each folder that stays gets its own warning, and the missing one none");
        assert!(outside.join("arquivo").is_file(), "nothing outside the root is touched");
        assert!(root.join("lixo/sobra").is_file(), "a crooked path to an ignored folder removes nothing");
        assert!(root.join(".git").join("HEAD").is_file() && root.join("src/lib.rs").is_file());
        assert!(root.join("dist/versionado").is_file() && root.join("build/saida").is_file());
    }

    /// Um atalho declarado como pasta de compilação, que leva para fora do
    /// projeto, não apaga nada do lugar para onde ele aponta.
    #[cfg(unix)]
    #[test]
    fn a_declared_link_out_of_the_root_removes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("obra");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("fora");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("arquivo"), "x").unwrap();
        project_with_build(&root, r#"{"buildOutput":["saida"]}"#);
        std::fs::write(root.join(".gitignore"), "target/\nsaida\n").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("saida")).unwrap();

        let swept = remove_build_output(&root, Locale::PtBr);
        assert!(swept.removed.is_empty(), "the link is not removed");
        let warned = translate("close.build_output_unsafe", Locale::PtBr).replace("{folder}", "saida");
        assert_eq!(kept_of(&swept), vec![warned]);
        assert!(root.join("saida").exists(), "the link stays");
        assert!(outside.join("arquivo").is_file(), "the link's target stays");
    }

    /// A remoção espera a trava do passo do git, que a rodada segura ao
    /// compilar antes do commit: enquanto outra rodada a segura, a pasta fica;
    /// solta a trava, ela sai. Sem relógio: o teste segue quando a lista de
    /// travas do sistema mostra a remoção parada na trava, ou falha quando a
    /// remoção termina antes, sem ter esperado.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_removal_waits_for_the_git_step_lock() {
        use crate::commands::git_settle::waiting_for_lock;
        use std::os::unix::fs::MetadataExt;
        use std::sync::mpsc;
        let dir = tempdir().unwrap();
        let root = dir.path();
        let built = project_with_build(root, r#"{"buildOutput":["target"]}"#);
        let specs = root.join(".claude").join("spec");
        std::fs::create_dir_all(&specs).unwrap();

        let held = crate::commands::git_settle::git_step_lock(root).unwrap();
        let lock: Vec<PathBuf> = std::fs::read_dir(&specs).unwrap().flatten().map(|entry| entry.path()).collect();
        assert_eq!(lock.len(), 1, "the lock is the only file in the spec folder: {lock:?}");
        let inode = std::fs::metadata(&lock[0]).unwrap().ino();
        std::thread::scope(|scope| {
            let (done_tx, done_rx) = mpsc::channel();
            scope.spawn(move || done_tx.send(remove_build_output_with(root, Locale::PtBr, &|_| Ok(BUILD_RETENTION_BYTES + 1)).removed));
            while !waiting_for_lock(inode) {
                if let Ok(removed) = done_rx.try_recv() {
                    panic!("the removal ran while another round held the lock: {removed:?}");
                }
                std::thread::yield_now();
            }
            assert!(built.is_file(), "the build of the other round stays while it holds the lock");
            drop(held);
            assert_eq!(crate::wait_for::wait_for_message("a remoção da compilação", &done_rx), Ok(vec!["target".to_string()]));
        });
        assert!(!built.exists());
    }
}
