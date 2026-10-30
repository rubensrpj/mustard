//! O preparo da cópia: só o comando que o projeto declara (`prepareCommand`),
//! rodado dentro da cópia, nunca na pasta principal. O selo mora na pasta de
//! administração do git da cópia, fora do diff, e leva o comando e o conteúdo
//! dos arquivos que a declaração lista como gatilho (`prepareFiles`). Com o
//! selo igual, o preparo não roda de novo; sem gatilho, ele roda uma vez por
//! cópia nova. O Mustard não adivinha dependência de linguagem nenhuma.
use mustard_core::io::{sha256::Sha256, workspace};
use std::path::Path;

/// Roda na cópia `copy` o preparo que o projeto em `root` declara, quando o
/// selo dela não é o do comando e dos gatilhos de agora, e grava o selo novo.
/// Sem preparo declarado, nada roda.
pub(crate) fn prepare(root: &Path, copy: &Path) -> Result<(), String> {
    let config = mustard_core::ProjectConfig::load(root);
    let Some(command) = config.commands().prepare else {
        return Ok(());
    };
    let Some(admin) = workspace::checkout_git_dir(copy) else {
        return Err("Cannot locate copy administration directory".into());
    };
    let stamp = admin.join("mustard-dependencies.sha256");
    let seal = seal(copy, &command, &config.prepare_files())?;
    if std::fs::read_to_string(&stamp).is_ok_and(|recorded| recorded.trim() == seal) {
        return Ok(());
    }
    // O preparo que falha nunca deixa o selo de antes valendo.
    match std::fs::remove_file(&stamp) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot invalidate dependency preparation: {error}")),
    }
    // A pasta de dependências ligada à da pasta principal seria apagada pela
    // instalação através do link: o link sai antes, e o alvo fica.
    let modules = copy.join("node_modules");
    if modules.symlink_metadata().is_ok_and(|meta| meta.file_type().is_symlink()) {
        #[cfg(windows)]
        let removed = if modules.is_dir() { std::fs::remove_dir(&modules) } else { std::fs::remove_file(&modules) };
        #[cfg(not(windows))]
        let removed = std::fs::remove_file(&modules);
        removed.map_err(|error| format!("Cannot replace dependency symlink in copy: {error}"))?;
    }
    let outcome = crate::commands::review::qa_run::run_server_command(&command, copy);
    if outcome.result != "pass" {
        return Err(format!("Preparation failed in {}: {}", copy.display(), crate::shared::secret::without_secrets(&outcome.output)));
    }
    std::fs::write(stamp, seal).map_err(|error| format!("Cannot record dependency preparation: {error}"))
}

/// O selo do preparo `command` na cópia `copy`: o comando e o conteúdo de cada
/// arquivo de `files`, ou a falta dele.
fn seal(copy: &Path, command: &str, files: &[String]) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update(b"dependency-preparation-v3\0");
    hash.update(command.as_bytes());
    for name in files {
        hash.update(b"\0");
        hash.update(name.as_bytes());
        match std::fs::read(copy.join(name)) {
            Ok(bytes) => {
                hash.update(b"\0present\0");
                hash.update(&bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => hash.update(b"\0absent"),
            Err(error) => return Err(format!("Cannot read preparation trigger {name}: {error}")),
        }
    }
    Ok(hash.hex_digest())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Um repositório com `files` comitados e uma cópia ligada a ele.
    fn checkout(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| assert!(mustard_core::platform::git::run(root, args).ok, "git {args:?}");
        git(&["init", "-q"]);
        for (name, text) in files {
            std::fs::create_dir_all(root.join(name).parent().unwrap()).unwrap();
            std::fs::write(root.join(name), text).unwrap();
        }
        git(&["add", "."]);
        git(&["-c", "user.name=fixture", "-c", "user.email=fixture@example.com", "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]);
        let copy = root.join("copy");
        git(&["worktree", "add", "--detach", copy.to_str().unwrap(), "HEAD"]);
        (dir, copy)
    }

    /// O preparo roda o comando declarado na cópia e não roda de novo com o
    /// selo igual. O gatilho que muda pede o preparo de novo; o arquivo que
    /// não é gatilho não pede. Sem gatilho, ele roda uma vez por cópia, e o
    /// comando que muda também conta. A pasta principal não ganha nada.
    #[test]
    fn the_declared_preparation_runs_once_per_seal() {
        let (dir, copy) = checkout(&[("package.json", "{}"), ("package-lock.json", "{}")]);
        let root = dir.path();
        let declare = |files: serde_json::Value| {
            std::fs::write(root.join("mustard.json"), serde_json::json!({"prepareCommand":"printf x >> prepared-count","prepareFiles":files}).to_string()).unwrap();
        };
        let count = || std::fs::read_to_string(copy.join("prepared-count")).unwrap_or_default();
        declare(serde_json::json!(["package-lock.json"]));
        prepare(root, &copy).unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(count(), "x", "o selo igual não roda de novo");
        std::fs::write(copy.join("package.json"), "{\"changed\":true}").unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(count(), "x", "o arquivo que não é gatilho não pede preparo");
        std::fs::write(copy.join("package-lock.json"), "{\"lock\":2}").unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(count(), "xx", "o gatilho mudado pede preparo");
        declare(serde_json::json!([]));
        prepare(root, &copy).unwrap();
        std::fs::write(copy.join("package-lock.json"), "{\"lock\":3}").unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(count(), "xxx", "sem gatilho, uma vez por cópia");
        assert!(!root.join("prepared-count").exists(), "nada roda na pasta principal");
    }

    /// O preparo que falha não deixa o selo de antes valendo, e a pasta de
    /// dependências ligada à da pasta principal perde só o link.
    #[test]
    fn a_failed_preparation_cannot_reuse_the_old_seal_and_a_linked_folder_loses_only_the_link() {
        let (dir, copy) = checkout(&[("package.json", "{}")]);
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), serde_json::json!({"prepareCommand":
            "if [ -f should-fail ]; then exit 1; fi && printf x >> prepared-count"}).to_string()).unwrap();
        let original = root.join("node_modules");
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("sentinel"), "preserve").unwrap();
        std::os::unix::fs::symlink(&original, copy.join("node_modules")).unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(original.join("sentinel")).unwrap(), "preserve");
        assert!(copy.join("node_modules").symlink_metadata().is_err(), "o link saiu");
        std::fs::write(root.join("mustard.json"), serde_json::json!({"prepareCommand":
            "if [ -f should-fail ]; then exit 1; fi && printf y >> prepared-count"}).to_string()).unwrap();
        std::fs::write(copy.join("should-fail"), "").unwrap();
        assert!(prepare(root, &copy).is_err());
        std::fs::remove_file(copy.join("should-fail")).unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy.join("prepared-count")).unwrap(), "xy");
    }
}
