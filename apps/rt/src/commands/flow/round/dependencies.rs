//! Run the declared preparation natively once per dependency fingerprint.
//! Receipts live in Git's worktree administration directory, outside the diff.
use mustard_core::io::{sha256::Sha256, workspace};
use std::path::Path;

pub(crate) fn prepare(root: &Path, copy: &Path) -> Result<(), String> {
    let Some(command) = mustard_core::ProjectConfig::load(root).commands().prepare else {
        return Ok(());
    };
    let Some(admin) = workspace::checkout_git_dir(copy) else {
        return Err("Cannot locate copy administration directory".into());
    };
    let stamp = admin.join("mustard-dependencies.sha256");
    let mut hash = Sha256::new();
    hash.update(command.as_bytes());
    let names = [
        "package.json",
        "pnpm-lock.yaml",
        "package-lock.json",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        ".npmrc",
        "Cargo.toml",
        "Cargo.lock",
        "pyproject.toml",
        "uv.lock",
        "requirements.txt",
        "poetry.lock",
        "global.json",
        "NuGet.config",
        "packages.lock.json",
        "Directory.Packages.props",
        "prisma/schema.prisma",
    ];
    let mut inputs: std::collections::BTreeSet<_> = names.iter().map(|name| (*name).to_string()).collect();
    let tracked = mustard_core::platform::git::run(copy, &["ls-files", "-z"]).out().unwrap_or_default();
    inputs.extend(
        tracked
            .split('\0')
            .filter(|path| {
                names.contains(&path.rsplit('/').next().unwrap_or(path))
                    || path.ends_with(".csproj")
                    || path.ends_with(".fsproj")
                    || path.ends_with(".prisma")
            })
            .map(str::to_string),
    );
    for name in inputs {
        hash.update(name.as_bytes());
        match std::fs::read(copy.join(&name)) {
            Ok(bytes) => {
                hash.update(b"present");
                hash.update(&bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => hash.update(b"absent"),
            Err(error) => return Err(format!("Cannot read dependency input {name}: {error}")),
        }
    }
    let fingerprint = hash.hex_digest();
    let modules = copy.join("node_modules");
    let node = copy.join("package.json").is_file();
    let symlink = modules.symlink_metadata().is_ok_and(|meta| meta.file_type().is_symlink());
    if std::fs::read_to_string(&stamp).ok().as_deref() == Some(&fingerprint) && (!node || modules.is_dir() && !symlink) {
        return Ok(());
    }
    if symlink {
        #[cfg(windows)]
        let result = if modules.is_dir() { std::fs::remove_dir(&modules) } else { std::fs::remove_file(&modules) };
        #[cfg(not(windows))]
        let result = std::fs::remove_file(&modules);
        result.map_err(|error| format!("Cannot replace dependency symlink in copy: {error}"))?;
    }
    let outcome = crate::commands::review::qa_run::run_server_command(&command, copy);
    if outcome.result != "pass" {
        return Err(format!("Preparation failed in {}: {}", copy.display(), crate::shared::secret::without_secrets(&outcome.output)));
    }
    if node && (!modules.is_dir() || modules.symlink_metadata().is_ok_and(|meta| meta.file_type().is_symlink())) {
        return Err("prepareCommand succeeded but did not create a local node_modules directory; configure a complete dependency installation instead of a symlink to the main checkout".into());
    }
    std::fs::write(stamp, fingerprint).map_err(|error| format!("Cannot record dependency preparation: {error}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn preparation_reuses_unchanged_inputs_and_replaces_only_the_dependency_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| assert!(mustard_core::platform::git::run(root, args).ok, "git {args:?}");
        git(&["init", "-q"]);
        std::fs::write(root.join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("prisma/models")).unwrap();
        std::fs::write(root.join("prisma/models/plan.prisma"), "model Plan {}").unwrap();
        git(&["add", "package.json", "prisma"]);
        git(&["-c", "user.name=fixture", "-c", "user.email=fixture@example.com", "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]);
        let copy = root.join("copy");
        git(&["worktree", "add", "--detach", copy.to_str().unwrap(), "HEAD"]);
        std::fs::write(
            root.join("mustard.json"),
            serde_json::json!({"prepareCommand":"mkdir -p node_modules && printf x >> prepared-count"}).to_string(),
        )
        .unwrap();
        let original = root.join("node_modules");
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("sentinel"), "preserve").unwrap();
        std::os::unix::fs::symlink(&original, copy.join("node_modules")).unwrap();
        prepare(root, &copy).unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy.join("prepared-count")).unwrap(), "x");
        assert_eq!(std::fs::read_to_string(original.join("sentinel")).unwrap(), "preserve");
        assert!(!copy.join("node_modules").symlink_metadata().unwrap().file_type().is_symlink());
        std::fs::write(copy.join("package.json"), "{\"changed\":true}").unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy.join("prepared-count")).unwrap(), "xx");
        std::fs::write(copy.join("prisma/models/plan.prisma"), "model Plan { id String @id }").unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy.join("prepared-count")).unwrap(), "xxx", "split Prisma schemas invalidate the preparation receipt");
        std::fs::write(root.join("mustard.json"), "{\"prepareCommand\":\"exit 2\"}").unwrap();
        assert!(prepare(root, &copy).is_err());
    }
}
