//! Onde mora o skill-creator, o procedimento oficial de criar skills que a
//! tarefa de criar skill leva inteiro no bloco dela: o plugin oficial
//! instalado na máquina, pela pasta do catálogo ou pela do cache, e, sem ele,
//! a cópia que o próprio plugin do Mustard leva em `third-party/`.

use std::path::{Path, PathBuf};

/// O nome pelo qual a tarefa chama o skill-creator.
pub const NAME: &str = "skill-creator";

/// A pasta do skill-creator no catálogo oficial, sob a configuração do
/// Claude Code.
const MARKETPLACE: &str = "plugins/marketplaces/claude-plugins-official/plugins/skill-creator/skills/skill-creator";

/// A pasta das versões do plugin oficial no cache, sob a configuração do
/// Claude Code; cada versão traz `skills/skill-creator/`.
const CACHE: &str = "plugins/cache/claude-plugins-official/skill-creator";

/// A cópia que o plugin do Mustard leva, sob a pasta dele.
const BUNDLED: &str = "third-party/skill-creator";

#[cfg(test)]
thread_local! {
    /// A pasta que um teste põe no lugar da procura, só nesta linha de
    /// execução.
    static IN_TEST: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Põe `folder` no lugar da procura do skill-creator, nesta linha de
/// execução; `None` volta à procura de verdade.
#[cfg(test)]
pub fn folder_in_test(folder: Option<PathBuf>) {
    IN_TEST.with(|slot| *slot.borrow_mut() = folder);
}

/// A pasta do skill-creator nesta máquina, a que tem o `SKILL.md`: a do
/// plugin oficial instalado e, sem ela, a cópia do plugin do Mustard — o que
/// a variável `CLAUDE_PLUGIN_ROOT` aponta, a instalação que o registro de
/// plugins conhece e a pasta acima do programa que roda. `None` sem nenhuma.
#[must_use]
pub fn folder() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(folder) = IN_TEST.with(|slot| slot.borrow().clone()) {
        return Some(folder);
    }
    let config = crate::platform::harness::claude_config_dir();
    let mut plugins: Vec<PathBuf> = Vec::new();
    if let Some(root) = std::env::var_os("CLAUDE_PLUGIN_ROOT").filter(|root| !root.is_empty()) {
        plugins.push(PathBuf::from(root));
    }
    if let Some(installed) = config.as_deref().and_then(crate::platform::harness::installed_plugin_in) {
        plugins.push(installed.dir);
    }
    if let Some(above) = std::env::current_exe().ok().as_deref().and_then(Path::parent).and_then(Path::parent) {
        plugins.push(above.to_path_buf());
    }
    folder_in(config.as_deref(), &plugins)
}

/// [`folder`] com a configuração do Claude Code `config` e as pastas do
/// plugin do Mustard `plugins` dadas: o catálogo oficial, depois a versão mais
/// nova do cache, depois a cópia de cada pasta do plugin, na ordem.
#[must_use]
pub fn folder_in(config: Option<&Path>, plugins: &[PathBuf]) -> Option<PathBuf> {
    let has = |dir: &Path| dir.join("SKILL.md").is_file();
    if let Some(config) = config {
        let market = config.join(MARKETPLACE);
        if has(&market) {
            return Some(market);
        }
        let mut versions: Vec<(String, PathBuf)> = std::fs::read_dir(config.join(CACHE))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry.path().join("skills").join(NAME)))
            .filter(|(_, dir)| has(dir))
            .collect();
        versions.sort_by(|(a, _), (b, _)| crate::platform::harness::compare_versions(a, b));
        if let Some((_, newest)) = versions.pop() {
            return Some(newest);
        }
    }
    plugins.iter().map(|plugin| plugin.join(BUNDLED)).find(|dir| has(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_at(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "---\nname: skill-creator\n---\nCorpo.\n").unwrap();
    }

    /// O plugin oficial instalado vem primeiro, pelo catálogo ou pela versão
    /// mais nova do cache; sem ele, a cópia que o plugin do Mustard leva.
    #[test]
    fn the_installed_skill_creator_comes_first_and_the_bundled_copy_after() {
        let dir = tempfile::tempdir().unwrap();
        let (config, plugin) = (dir.path().join("config"), dir.path().join("plugin"));
        skill_at(&plugin.join(BUNDLED));
        assert_eq!(folder_in(Some(&config), std::slice::from_ref(&plugin)), Some(plugin.join(BUNDLED)));
        for version in ["1.2.0", "1.10.0", "1.9.3"] {
            skill_at(&config.join(CACHE).join(version).join("skills").join(NAME));
        }
        assert_eq!(folder_in(Some(&config), std::slice::from_ref(&plugin)), Some(config.join(CACHE).join("1.10.0/skills/skill-creator")));
        skill_at(&config.join(MARKETPLACE));
        assert_eq!(folder_in(Some(&config), std::slice::from_ref(&plugin)), Some(config.join(MARKETPLACE)));
        assert_eq!(folder_in(None, &[dir.path().join("nada")]), None);
    }

    /// No test reads the person's own Claude Code configuration: the test
    /// environment names a configuration folder inside the build folder, and
    /// every search for the skill-creator starts there.
    #[test]
    fn the_tests_never_read_the_real_claude_config() {
        let config = crate::platform::harness::claude_config_dir().expect("the test environment names a configuration folder");
        let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        // Lexical on both sides: canonicalizing only one gives the Windows
        // `\\?\` prefix to it and never to the other.
        let (config, target) = (lexical(&config), lexical(&target));
        assert!(config.starts_with(&target), "the tests read {} instead of a folder under {}", config.display(), target.display());
    }

    /// `path` with `.` dropped and each `..` taking the part before it, without
    /// touching the disk.
    fn lexical(path: &Path) -> std::path::PathBuf {
        let mut out = std::path::PathBuf::new();
        for part in path.components() {
            match part {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        out
    }

    /// A marca da linha que o [`folder_reader`] imprime.
    const FOLDER_MARK: &str = "procura-do-skill-creator";

    /// Não prova comportamento nenhum: é o programa que o teste abaixo roda
    /// com a configuração que ele escolhe. Rodado pela suíte, só passa.
    #[test]
    fn folder_reader() {
        println!("{FOLDER_MARK} {}", folder().map_or_else(|| "-".to_string(), |dir| dir.display().to_string()));
    }

    /// O [`folder_reader`] num processo próprio, com a configuração do Claude
    /// Code em `config` e a pasta do plugin do Mustard em `plugin`.
    fn folder_with(config: &Path, plugin: &Path) -> String {
        let module = module_path!();
        let module = module.split_once("::").map_or(module, |(_, rest)| rest);
        let out = std::process::Command::new(std::env::current_exe().expect("the test executable"))
            .args([&format!("{module}::folder_reader"), "--exact", "--nocapture"])
            .env("CLAUDE_CONFIG_DIR", config)
            .env("CLAUDE_PLUGIN_ROOT", plugin)
            .output()
            .expect("the test executable runs");
        let stdout = String::from_utf8_lossy(&out.stdout);
        stdout.lines().find_map(|line| line.strip_prefix(&format!("{FOLDER_MARK} "))).unwrap_or("(no answer)").to_string()
    }

    /// The search reads the configuration folder the environment names: with
    /// a fake official plugin in it, that one; without, the copy the Mustard
    /// plugin carries.
    #[test]
    fn the_search_follows_the_configuration_folder_of_the_environment() {
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin");
        let config = tempfile::tempdir().unwrap();
        assert_eq!(folder_with(config.path(), &plugin), plugin.join(BUNDLED).display().to_string());
        skill_at(&config.path().join(MARKETPLACE));
        assert_eq!(folder_with(config.path(), &plugin), config.path().join(MARKETPLACE).display().to_string());
    }

    /// A cópia levada no plugin do Mustard traz o procedimento, a licença e a
    /// nota de origem.
    #[test]
    fn the_plugin_carries_the_skill_creator_with_its_license_and_notice() {
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin");
        let bundled = folder_in(None, std::slice::from_ref(&plugin)).expect("the plugin carries the skill-creator");
        for file in ["SKILL.md", "LICENSE.txt", "NOTICE", "scripts/run_eval.py", "agents/grader.md"] {
            assert!(bundled.join(file).is_file(), "{file} is missing from {}", bundled.display());
        }
    }
}
