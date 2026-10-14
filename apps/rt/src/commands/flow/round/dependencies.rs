//! Run the declared preparation natively once per dependency fingerprint.
//! Receipts live in Git's worktree administration directory, outside the diff.
use mustard_core::io::{sha256::Sha256, workspace};
use std::path::Path;

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
enum Layout {
    Opaque,
    Modules,
    PlugAndPlay,
}

impl Layout {
    fn of(copy: &Path, command: &str) -> Self {
        if !copy.join("package.json").is_file() {
            return Self::Opaque;
        }
        let manager = crate::hooks::bash::lex::segments(command).into_iter().find_map(|segment| {
            let name = segment.name();
            let install = segment.args.first().is_some_and(|arg| matches!(arg.text.as_str(), "install" | "ci"));
            (matches!(name, "npm" | "pnpm" | "yarn" | "bun") && (install || name == "yarn" && segment.args.is_empty()))
                .then(|| name.to_owned())
        });
        if manager.as_deref() == Some("yarn") {
            let config = std::fs::read_to_string(copy.join(".yarnrc.yml")).unwrap_or_default();
            let linker = config.lines().find_map(|line| {
                line.trim().strip_prefix("nodeLinker:").map(|value| value.split('#').next().unwrap_or("").trim().trim_matches(['\'', '"']))
            });
            if linker == Some("node-modules") {
                return Self::Modules;
            }
            let manifest: serde_json::Value = std::fs::read(copy.join("package.json")).ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
            let modern = manifest["packageManager"].as_str().and_then(|value| value.strip_prefix("yarn@"))
                .and_then(|version| version.split('.').next()?.parse::<u32>().ok()).is_some_and(|major| major >= 2);
            if linker == Some("pnp") || modern || copy.join(".pnp.cjs").is_file() {
                return Self::PlugAndPlay;
            }
        }
        if manager.is_some() || copy.join("node_modules").symlink_metadata().is_ok() {
            Self::Modules
        } else {
            // A prepare hook may generate sources or restore another stack.
            // Its successful exit is its contract; package.json alone cannot
            // impose a dependency layout on an arbitrary project command.
            Self::Opaque
        }
    }

    fn ready(self, copy: &Path) -> bool {
        match self {
            Self::Opaque => true,
            Self::PlugAndPlay => copy.join(".pnp.cjs").is_file(),
            Self::Modules => copy.join("node_modules").is_dir()
                && copy.join("node_modules").symlink_metadata().is_ok_and(|meta| !meta.file_type().is_symlink()),
        }
    }
}

pub(crate) fn prepare(root: &Path, copy: &Path) -> Result<(), String> {
    let Some(command) = mustard_core::ProjectConfig::load(root).commands().prepare else {
        return Ok(());
    };
    let Some(admin) = workspace::checkout_git_dir(copy) else {
        return Err("Cannot locate copy administration directory".into());
    };
    let stamp = admin.join("mustard-dependencies.sha256");
    let mut hash = Sha256::new();
    hash.update(b"dependency-preparation-v2");
    hash.update(command.as_bytes());
    let names = [
        "package.json",
        "pnpm-lock.yaml",
        "package-lock.json",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        ".npmrc",
        ".yarnrc.yml",
        ".yarnrc",
        "pnpm-workspace.yaml",
        "bunfig.toml",
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
        "Directory.Build.props",
        "Directory.Build.targets",
        "go.mod",
        "go.sum",
        "go.work",
        "go.work.sum",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
        "gradle.properties",
        "gradle.lockfile",
        "Gemfile",
        "Gemfile.lock",
        "composer.json",
        "composer.lock",
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
    let previous: serde_json::Value = std::fs::read(&stamp).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let same_inputs = previous["fingerprint"].as_str() == Some(&fingerprint);
    let detected = Layout::of(copy, &command);
    let layout = if matches!(detected, Layout::Opaque) && same_inputs {
        serde_json::from_value(previous["layout"].clone()).unwrap_or(detected)
    } else { detected };
    let modules = copy.join("node_modules");
    let symlink = modules.symlink_metadata().is_ok_and(|meta| meta.file_type().is_symlink());
    if same_inputs && layout.ready(copy) {
        return Ok(());
    }
    // A failed retry must never leave the preceding receipt reusable.
    match std::fs::remove_file(&stamp) {
        Ok(()) => {},
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(error) => return Err(format!("Cannot invalidate dependency preparation: {error}")),
    }
    if symlink && matches!(layout, Layout::Modules) {
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
    if !layout.ready(copy) {
        return Err(match layout {
            Layout::PlugAndPlay => "prepareCommand succeeded but the declared Yarn Plug'n'Play installation has no .pnp.cjs loader",
            _ => "prepareCommand succeeded but did not create a local node_modules directory; configure a complete dependency installation instead of a symlink to the main checkout",
        }.into());
    }
    let layout = if matches!(layout, Layout::Opaque) { Layout::of(copy, &command) } else { layout };
    std::fs::write(stamp, serde_json::json!({"fingerprint":fingerprint,"layout":layout}).to_string())
        .map_err(|error| format!("Cannot record dependency preparation: {error}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn checkout(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| assert!(mustard_core::platform::git::run(root, args).ok, "git {args:?}");
        git(&["init", "-q"]);
        for (name, text) in files {
            std::fs::write(root.join(name), text).unwrap();
        }
        git(&["add", "."]);
        git(&["-c", "user.name=fixture", "-c", "user.email=fixture@example.com", "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]);
        let copy = root.join("copy");
        git(&["worktree", "add", "--detach", copy.to_str().unwrap(), "HEAD"]);
        (dir, copy)
    }

    #[test]
    fn arbitrary_prepare_and_non_node_dependency_changes_follow_the_declared_contract() {
        let (dir, copy) = checkout(&[("package.json", "{}"), ("go.mod", "module example.com/service"), (".yarnrc.yml", "nodeLinker: pnp")]);
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), serde_json::json!({"prepareCommand":"printf x >> prepared-count"}).to_string()).unwrap();
        prepare(root, &copy).unwrap();
        prepare(root, &copy).unwrap();
        assert!(!copy.join("node_modules").exists());
        assert_eq!(std::fs::read_to_string(copy.join("prepared-count")).unwrap(), "x");
        std::fs::write(copy.join("go.mod"), "module example.com/changed").unwrap();
        prepare(root, &copy).unwrap();
        std::fs::write(copy.join(".yarnrc.yml"), "nodeLinker: node-modules").unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy.join("prepared-count")).unwrap(), "xxx");
    }

    #[test]
    fn yarn_pnp_requires_its_loader_instead_of_node_modules() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, copy) = checkout(&[("package.json", r#"{"packageManager":"yarn@4.2.0"}"#)]);
        let root = dir.path();
        let yarn = root.join("yarn");
        std::fs::write(&yarn, "#!/bin/sh\nprintf x >> prepared-count\ntouch .pnp.cjs\n").unwrap();
        std::fs::set_permissions(&yarn, std::fs::Permissions::from_mode(0o755)).unwrap();
        let command = format!("'{}' install", yarn.display());
        std::fs::write(root.join("mustard.json"), serde_json::json!({"prepareCommand":command}).to_string()).unwrap();
        prepare(root, &copy).unwrap();
        prepare(root, &copy).unwrap();
        assert!(!copy.join("node_modules").exists());
        assert_eq!(std::fs::read_to_string(copy.join("prepared-count")).unwrap(), "x");
        std::fs::remove_file(copy.join(".pnp.cjs")).unwrap();
        prepare(root, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy.join("prepared-count")).unwrap(), "xx");
        std::fs::write(copy.join(".yarnrc.yml"), "nodeLinker: 'node-modules' # explicit override\n").unwrap();
        assert!(prepare(root, &copy).unwrap_err().contains("node_modules"));
    }

    #[test]
    fn failed_repair_cannot_reuse_an_old_success_receipt() {
        let (dir, copy) = checkout(&[("package.json", "{}")]);
        std::fs::create_dir(copy.join("node_modules")).unwrap();
        std::fs::write(dir.path().join("mustard.json"), serde_json::json!({"prepareCommand":
            "mkdir -p node_modules && if [ -f should-fail ]; then exit 1; fi && printf x >> prepared-count"}).to_string()).unwrap();
        prepare(dir.path(), &copy).unwrap();
        std::fs::remove_dir(copy.join("node_modules")).unwrap();
        std::fs::write(copy.join("should-fail"), "").unwrap();
        assert!(prepare(dir.path(), &copy).is_err());
        std::fs::remove_file(copy.join("should-fail")).unwrap();
        prepare(dir.path(), &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy.join("prepared-count")).unwrap(), "xx");
    }

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
