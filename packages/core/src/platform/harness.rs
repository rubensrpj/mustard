//! The version of the RUNNING harness — the single number every stamp, drift
//! check and statusline segment compares against.
//!
//! It is baked into the binary AT BUILD TIME, so a running binary always knows
//! its own version without reading anything at runtime. Truth order:
//! 1. `MUSTARD_RELEASE_VERSION` — injected by the release workflow from the git
//!    tag (`vX.Y.Z`, which the release gate verifies equals
//!    `plugin.json#version`, the line `bump-on-main` advances on every main
//!    merge). A SHIPPED binary carries this — the version follows the release.
//! 2. `CARGO_PKG_VERSION` — the unified `[workspace.package]` version, for a
//!    local / dev / CI build the release env did not stamp. An honest "this is
//!    a dev build" answer, not a stale lie.
//!
//! `mustard.json#version` therefore records which harness last set the project
//! up. The 3.1.x stamps in the field are the pre-plugin CLI era: they read as
//! drift once, and the first `/mustard:upsert` realigns them to this line.
//!
//! The module answers a SECOND question, which the first cannot:
//! [`installed_harness_version`] reads what Claude Code's plugin registry
//! records as installed. A running binary and the stamp it writes always agree,
//! so the pair above can never see the window between an update landing on disk
//! and the operator reloading — during which the session runs the old prose and
//! looks perfectly aligned. Only the registry knows.

/// Resolve the running harness version — release-stamped when shipped, the
/// workspace version otherwise. Never empty.
#[must_use]
pub fn harness_version() -> String {
    option_env!("MUSTARD_RELEASE_VERSION")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string()
}

/// The plugin name this harness ships under, as it appears on the left of the
/// `{plugin}@{marketplace}` key in the registry. The marketplace half varies by
/// how the operator added it (`mustard@mustard`, `mustard@mustard-local`), so
/// only this half is matched.
pub const PLUGIN_NAME: &str = "mustard";

/// Claude Code's registry of installed plugins, relative to its config
/// directory.
pub const INSTALLED_PLUGINS: &str = "plugins/installed_plugins.json";

/// The version the plugin registry records as INSTALLED — what the next
/// session would load, as opposed to [`harness_version`], which is what THIS
/// process is.
///
/// The two are the same number almost always, and differ in exactly the window
/// that matters: between an update landing on disk and the operator reloading,
/// the session keeps running the old plugin. Nothing else in the product can
/// see that window — the stamp in `mustard.json` is written BY the running
/// harness, so it agrees with the running harness by construction and a stale
/// session reads as perfectly aligned.
///
/// `None` when the registry cannot be located, read or parsed, or when it lists
/// no install of this plugin — every one of which means "we cannot prove the
/// session is behind", the only safe direction for an advisory. Never panics.
#[must_use]
pub fn installed_harness_version() -> Option<String> {
    let raw = std::fs::read_to_string(claude_config_dir()?.join(INSTALLED_PLUGINS)).ok()?;
    installed_harness_version_from(&raw)
}

/// The testable half of [`installed_harness_version`]: the registry's JSON text
/// in, the recorded version out.
///
/// The registry keys installs by `{plugin}@{marketplace}` and maps each to an
/// ARRAY — one record per scope (user, project). The highest version among this
/// plugin's records wins: "installed" is what a reload would pick up, and a
/// lower-scoped leftover must not make a current session look stale.
#[must_use]
pub fn installed_harness_version_from(raw: &str) -> Option<String> {
    let doc: serde_json::Value = serde_json::from_str(raw).ok()?;
    let plugins = doc.get("plugins")?.as_object()?;
    plugins
        .iter()
        .filter(|(key, _)| key.split('@').next() == Some(PLUGIN_NAME))
        .filter_map(|(_, records)| records.as_array())
        .flatten()
        .filter_map(|record| record.get("version")?.as_str())
        .filter(|version| !version.is_empty())
        .max_by(|a, b| compare_versions(a, b))
        .map(str::to_string)
}

/// The `mustard-rt` binary a STALE copy should hand its whole invocation to:
/// the one inside the newest install the plugin registry records — and only
/// when that install is strictly newer than THIS process.
///
/// Why this exists: the plugin's own binary self-updates on every release
/// (`mustard-boot` re-downloads it), but the SYSTEM copy — the `.deb`'s
/// `/usr/bin` symlink, the `.exe` installer's PATH entry, the `.pkg`'s — only
/// changes when the operator reinstalls. Every PATH call site (`upsert`, the
/// statusline, a terminal `run`) therefore answers with whatever version the
/// installer left behind, and each answers with its OWN version: a stale
/// `upsert` re-stamps `mustard.json` with the old number and the drift
/// advisory can never converge. Fixing the installers one by one cannot close
/// this — an installer runs once, the plugin updates forever — so the handover
/// lives in the binary itself and every entry door converges on the same,
/// newest answer.
///
/// `None` whenever the handover cannot be PROVEN an upgrade: registry absent,
/// unreadable, no install of this plugin, the newest install not strictly
/// newer (a dev build equal to or ahead of the release stays in charge — the
/// "hands off a dev machine" rule `mustard-boot` already follows), or the
/// recorded directory holding no binary. Never panics; the caller runs itself,
/// which is exactly what happened before this function existed.
#[must_use]
pub fn newer_installed_rt() -> Option<std::path::PathBuf> {
    let raw = std::fs::read_to_string(claude_config_dir()?.join(INSTALLED_PLUGINS)).ok()?;
    let path = newer_installed_rt_from(&raw, &harness_version())?;
    path.is_file().then_some(path)
}

/// The testable half of [`newer_installed_rt`]: registry JSON and the running
/// version in, the newer install's `bin/mustard-rt` path out. Existence on
/// disk is the outer half's business.
#[must_use]
pub fn newer_installed_rt_from(raw: &str, running: &str) -> Option<std::path::PathBuf> {
    let plugin = newest_installed_plugin_from(raw)?;
    is_behind(running, &plugin.version).then(|| plugin.rt_binary())
}

/// The `mustard-rt` inside the newest install the plugin registry records —
/// whatever its version, and whether or not it is newer than this process.
///
/// This is what the statusline self-heal records. "Newest RECORDED install" is
/// the honest name for it: the registry can hold one record per scope, and this
/// takes the highest VERSION across them, which need not be the scope a given
/// project loads. That imprecision is acceptable here and nowhere else — any
/// recorded plugin copy beats the two answers this replaced. Measured 2026-08-28: Claude
/// Code APPENDS the plugin's `bin/` to `PATH` (last of 21 entries), so a bare
/// `mustard-rt` answers with the SYSTEM copy and hides exactly the
/// plugin-vs-system drift the status bar exists to show.
///
/// `None` when the registry is absent, unreadable, records no install of this
/// plugin, or the recorded directory holds no binary. Callers treat that as
/// "cannot tell" and must NOT fall back to inferring a path from the running
/// process — see `statusline_heal_observer` for the incident that rule comes
/// from.
#[must_use]
pub fn installed_plugin_rt() -> Option<std::path::PathBuf> {
    installed_plugin_rt_in(&claude_config_dir()?)
}

/// [`installed_plugin_rt`] with the config directory named explicitly — the
/// seam the tests need. The `is_file()` gate is the ENTIRE safety story for a
/// registry that still records a plugin whose binary is gone (the dormant
/// install: the plugin is registered, `mustard-boot` never ran, `bin/` holds no
/// `mustard-rt`), and a gate reached only through the real `~/.claude` is a gate
/// no test can watch.
#[must_use]
pub fn installed_plugin_rt_in(config_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let path = installed_plugin_in(config_dir)?.rt_binary();
    path.is_file().then_some(path)
}

/// The testable half of [`installed_plugin_rt`]: registry JSON in, the newest
/// install's `bin/mustard-rt` out. Existence on disk is the outer half's
/// business.
#[must_use]
pub fn installed_plugin_rt_from(raw: &str) -> Option<std::path::PathBuf> {
    newest_installed_plugin_from(raw).map(|plugin| plugin.rt_binary())
}

/// One install of this plugin, as the registry records it: the directory the
/// plugin lives in and the version that same record carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    /// The install's directory (`installPath`): `bin/` and the plugin manifest
    /// live under it.
    pub dir: std::path::PathBuf,
    /// The version the record carries.
    pub version: String,
}

impl InstalledPlugin {
    /// The `mustard-rt` inside this install's `bin/`, whether or not it is on
    /// disk.
    #[must_use]
    pub fn rt_binary(&self) -> std::path::PathBuf {
        let exe = if cfg!(windows) { "mustard-rt.exe" } else { "mustard-rt" };
        self.dir.join("bin").join(exe)
    }
}

/// The newest install of this plugin the registry records, read from Claude
/// Code's config directory — the one [`claude_config_dir`] resolves, so an
/// operator who moved it with `CLAUDE_CONFIG_DIR` is followed here too.
///
/// This is what the installation doctor inspects: whether the binary of the
/// version that is SUPPOSED to be here is on disk. `None` when the registry
/// cannot be located, read or parsed, or records no install of this plugin.
#[must_use]
pub fn installed_plugin() -> Option<InstalledPlugin> {
    installed_plugin_in(&claude_config_dir()?)
}

/// [`installed_plugin`] with the config directory named explicitly — the seam
/// the tests need.
#[must_use]
pub fn installed_plugin_in(config_dir: &std::path::Path) -> Option<InstalledPlugin> {
    let raw = std::fs::read_to_string(config_dir.join(INSTALLED_PLUGINS)).ok()?;
    newest_installed_plugin_from(&raw)
}

/// The newest install of this plugin the registry records. The single reader
/// of the registry's shape, shared by the stale-copy handover, the statusline
/// heal and the installation doctor, so a change to that shape can never move
/// one without the others.
///
/// Version and `installPath` come from the SAME record — the one with the
/// highest version. [`installed_harness_version_from`] takes the max over
/// versions alone; pairing its answer with a path picked independently could
/// marry scope A's version to scope B's directory.
fn newest_installed_plugin_from(raw: &str) -> Option<InstalledPlugin> {
    let doc: serde_json::Value = serde_json::from_str(raw).ok()?;
    let (version, install) = doc
        .get("plugins")?
        .as_object()?
        .iter()
        .filter(|(key, _)| key.split('@').next() == Some(PLUGIN_NAME))
        .filter_map(|(_, records)| records.as_array())
        .flatten()
        .filter_map(|record| {
            let version = record.get("version")?.as_str().filter(|v| !v.is_empty())?;
            let install = record.get("installPath")?.as_str().filter(|p| !p.is_empty())?;
            Some((version, install))
        })
        .max_by(|(a, _), (b, _)| compare_versions(a, b))?;
    Some(InstalledPlugin {
        dir: std::path::PathBuf::from(install),
        version: version.to_string(),
    })
}

/// Whether `running` names a version strictly OLDER than `installed`.
///
/// Dotted numeric components, compared left to right and zero-padded to the
/// longer side, so `0.1.9 < 0.1.10` (a plain string comparison gets that
/// backwards). A component that is not a number compares as 0 — a pre-release
/// suffix must never READ as an upgrade, and the honest answer for "we cannot
/// order these" is "not behind", which stays silent.
#[must_use]
pub fn is_behind(running: &str, installed: &str) -> bool {
    compare_versions(running, installed) == std::cmp::Ordering::Less
}

/// Order two dotted version strings by their numeric components. See
/// [`is_behind`] for why a non-numeric component counts as 0.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-', '+'])
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (left, right) = (parts(a), parts(b));
    let width = left.len().max(right.len());
    for i in 0..width {
        let ordering = left.get(i).copied().unwrap_or(0).cmp(&right.get(i).copied().unwrap_or(0));
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    std::cmp::Ordering::Equal
}

/// A pasta pessoal do usuário: `HOME`, ou `USERPROFILE` no Windows, lida sem
/// dependência. A variável vazia vale como ausente: `None`, e quem chama cai
/// no próprio plano B em vez de montar um caminho relativo à pasta em que o
/// comando roda. A pasta de configuração do Claude Code, a pasta das cópias
/// das ondas e o binário `mustard-rt` leem a pasta pessoal por aqui.
#[must_use]
pub fn home_dir() -> Option<std::path::PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|dir| !dir.is_empty()).map(std::path::PathBuf::from)
}

/// Claude Code's config directory: `CLAUDE_CONFIG_DIR` when the operator moved
/// it, `.claude` under [`home_dir`] otherwise. `None` when neither resolves —
/// this crate reads no home directory through a dependency.
pub fn claude_config_dir() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(std::path::PathBuf::from(dir));
    }
    home_dir().map(|home| home.join(".claude"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry never yields an empty string — either the release stamp or
    /// this crate's compiled-in workspace version.
    #[test]
    fn harness_version_is_never_empty() {
        assert!(!harness_version().is_empty());
    }

    /// Absent the release env, the answer is the crate's own `CARGO_PKG_VERSION`
    /// (the workspace version) — the dev/local build path.
    #[test]
    fn harness_version_falls_back_to_cargo_pkg_version() {
        // In the test build `MUSTARD_RELEASE_VERSION` is unset (nothing stamps
        // it), so the fallback is exercised directly.
        if option_env!("MUSTARD_RELEASE_VERSION").is_none() {
            assert_eq!(harness_version(), env!("CARGO_PKG_VERSION"));
        }
    }

    /// The registry keys installs by `{plugin}@{marketplace}` and the
    /// marketplace half varies by how the operator added it, so only the plugin
    /// name is matched — and a foreign plugin's version is never mistaken for
    /// ours.
    #[test]
    fn the_registry_answers_this_plugins_version_whatever_the_marketplace() {
        let raw = r#"{
          "version": 2,
          "plugins": {
            "mustard@mustard-local": [{"scope":"user","version":"0.1.43"}],
            "rust-analyzer-lsp@claude-plugins-official": [{"scope":"user","version":"9.9.9"}]
          }
        }"#;
        assert_eq!(installed_harness_version_from(raw).as_deref(), Some("0.1.43"));
    }

    /// Several records (one per scope) resolve to the HIGHEST: "installed" is
    /// what a reload would pick up, and a lower-scoped leftover must not make a
    /// current session read as stale.
    #[test]
    fn the_highest_recorded_install_wins() {
        let raw = r#"{"plugins":{"mustard@mustard":[
          {"scope":"project","version":"0.1.9"},
          {"scope":"user","version":"0.1.10"}
        ]}}"#;
        assert_eq!(installed_harness_version_from(raw).as_deref(), Some("0.1.10"));
    }

    /// Every unreadable shape is one answer — `None`, "we cannot prove the
    /// session is behind" — because the caller is an advisory.
    #[test]
    fn an_unreadable_registry_proves_nothing() {
        assert_eq!(installed_harness_version_from("not json").as_deref(), None);
        assert_eq!(installed_harness_version_from("{}").as_deref(), None);
        assert_eq!(installed_harness_version_from(r#"{"plugins":{}}"#).as_deref(), None);
        assert_eq!(
            installed_harness_version_from(r#"{"plugins":{"other@m":[{"version":"1.0.0"}]}}"#)
                .as_deref(),
            None
        );
    }

    /// The handover only ever goes from OLD to NEW: a runner equal to or ahead
    /// of the newest recorded install answers for itself. This is what makes a
    /// delegation loop structurally impossible (the delegate re-runs this same
    /// check and finds itself not behind) and what keeps a dev build in charge
    /// of a dev machine.
    #[test]
    fn a_runner_hands_over_only_when_strictly_behind() {
        let raw = r#"{"plugins":{"mustard@mustard-local":[
          {"scope":"user","version":"0.1.51","installPath":"/plug/0.1.51"}
        ]}}"#;
        assert_eq!(
            newer_installed_rt_from(raw, "0.1.50"),
            Some(std::path::PathBuf::from(if cfg!(windows) {
                "/plug/0.1.51/bin/mustard-rt.exe"
            } else {
                "/plug/0.1.51/bin/mustard-rt"
            }))
        );
        assert_eq!(newer_installed_rt_from(raw, "0.1.51"), None, "equal stays in charge");
        assert_eq!(newer_installed_rt_from(raw, "0.1.52"), None, "ahead (dev build) stays in charge");
    }

    /// Version and path come from the SAME record: with two scopes recorded,
    /// the newest install's directory is the one handed over to — never the
    /// other scope's leftover.
    #[test]
    fn the_handover_target_is_the_newest_records_own_directory() {
        let raw = r#"{"plugins":{"mustard@mustard":[
          {"scope":"project","version":"0.1.9","installPath":"/old"},
          {"scope":"user","version":"0.1.10","installPath":"/new"}
        ]}}"#;
        let target = newer_installed_rt_from(raw, "0.1.8");
        assert!(target.is_some_and(|p| p.starts_with("/new")));
    }

    /// Every unreadable or incomplete shape is one answer — `None`, "run
    /// yourself" — because the caller is about to replace its own execution
    /// and must never do so on a guess.
    #[test]
    fn an_unprovable_handover_runs_itself() {
        assert_eq!(newer_installed_rt_from("not json", "0.1.0"), None);
        assert_eq!(newer_installed_rt_from(r#"{"plugins":{}}"#, "0.1.0"), None);
        let no_path = r#"{"plugins":{"mustard@m":[{"version":"9.9.9"}]}}"#;
        assert_eq!(newer_installed_rt_from(no_path, "0.1.0"), None, "a record without installPath names no target");
        let foreign = r#"{"plugins":{"other@m":[{"version":"9.9.9","installPath":"/x"}]}}"#;
        assert_eq!(newer_installed_rt_from(foreign, "0.1.0"), None);
    }

    /// Dotted components compare NUMERICALLY: `0.1.9` is behind `0.1.10`, which
    /// a plain string comparison gets backwards. Equal and ahead are both
    /// "not behind" — the advisory only ever fires on a session running old
    /// prose.
    #[test]
    fn behind_is_a_numeric_component_comparison() {
        assert!(is_behind("0.1.9", "0.1.10"));
        assert!(is_behind("0.1.42", "0.2.0"));
        assert!(!is_behind("0.1.42", "0.1.42"));
        assert!(!is_behind("0.1.43", "0.1.42"));
        assert!(!is_behind("0.1.42", "0.1.42-rc1"), "a pre-release never reads as an upgrade");
    }

    // --- installed_plugin_rt: o caminho que a barra de status grava ---------

    /// Write a registry naming `install_path` at version `version`, and return
    /// the config dir holding it.
    fn seed_registry(dir: &std::path::Path, install_path: &std::path::Path, version: &str) {
        let plugins = dir.join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(
            plugins.join("installed_plugins.json"),
            format!(
                r#"{{"version":2,"plugins":{{"mustard@mustard-local":[{{"scope":"user","installPath":{},"version":"{version}"}}]}}}}"#,
                serde_json::to_string(&install_path.to_string_lossy()).unwrap()
            ),
        )
        .unwrap();
    }

    /// A plugin directory holding a real `bin/mustard-rt`, and its path.
    fn seed_plugin(dir: &std::path::Path) -> std::path::PathBuf {
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(if cfg!(windows) { "mustard-rt.exe" } else { "mustard-rt" });
        std::fs::write(&exe, b"").unwrap();
        exe
    }

    #[test]
    fn the_recorded_plugin_binary_is_answered_when_it_is_on_disk() {
        let home = tempfile::tempdir().unwrap();
        let plugin = home.path().join("cache/mustard/0.1.57");
        let exe = seed_plugin(&plugin);
        seed_registry(home.path(), &plugin, "0.1.57");

        assert_eq!(installed_plugin_rt_in(home.path()), Some(exe));
    }

    /// The DORMANT install: registered, but `mustard-boot` never brought the
    /// binaries down. Answering a path here would put a command in the status
    /// bar pointing at a file that does not exist — a blank bar on exactly the
    /// machine that most needs the bar to say something.
    #[test]
    fn a_registered_plugin_with_no_binary_answers_nothing() {
        let home = tempfile::tempdir().unwrap();
        let plugin = home.path().join("cache/mustard/0.1.57");
        std::fs::create_dir_all(plugin.join("bin")).unwrap();
        seed_registry(home.path(), &plugin, "0.1.57");

        assert_eq!(installed_plugin_rt_in(home.path()), None);
    }

    #[test]
    fn no_registry_at_all_answers_nothing() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(installed_plugin_rt_in(home.path()), None);
    }

    /// Unlike the handover, this answer does NOT depend on being newer than the
    /// running process: the statusline must follow the plugin even when the
    /// plugin is behind — that drift is precisely what the bar exists to show.
    #[test]
    fn an_older_plugin_than_the_runner_is_still_the_answer() {
        let home = tempfile::tempdir().unwrap();
        let plugin = home.path().join("cache/mustard/0.0.1");
        let exe = seed_plugin(&plugin);
        seed_registry(home.path(), &plugin, "0.0.1");

        assert_eq!(installed_plugin_rt_in(home.path()), Some(exe));
        // The handover, given the same registry, correctly declines.
        let raw = std::fs::read_to_string(home.path().join(INSTALLED_PLUGINS)).unwrap();
        assert_eq!(newer_installed_rt_from(&raw, "0.1.57"), None);
    }

    /// O diagnóstico da instalação lê o registro pela pasta de configuração
    /// que recebe, e o registro com duas instalações responde a mais nova,
    /// com a pasta dela — nunca a primeira da lista, e nunca a versão de uma
    /// casada com a pasta da outra. A comparação é por número: `0.1.10` vem
    /// depois de `0.1.9`.
    #[test]
    fn the_newest_install_answers_with_its_own_folder_and_version() {
        let config = tempfile::tempdir().unwrap();
        let older = config.path().join("cache/mustard/0.1.9");
        let newer = config.path().join("cache/mustard/0.1.10");
        let plugins = config.path().join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        let quoted = |path: &std::path::Path| serde_json::to_string(&path.to_string_lossy()).unwrap();
        std::fs::write(
            plugins.join("installed_plugins.json"),
            format!(
                r#"{{"version":2,"plugins":{{"mustard@mustard-local":[
                  {{"scope":"project","installPath":{},"version":"0.1.9"}},
                  {{"scope":"user","installPath":{},"version":"0.1.10"}}
                ]}}}}"#,
                quoted(&older),
                quoted(&newer),
            ),
        )
        .unwrap();

        assert_eq!(
            installed_plugin_in(config.path()),
            Some(InstalledPlugin { dir: newer, version: "0.1.10".to_string() })
        );
        assert_eq!(installed_plugin_in(&config.path().join("sem-registro")), None);
    }

    #[test]
    fn a_registry_naming_another_plugin_answers_nothing() {
        let home = tempfile::tempdir().unwrap();
        let plugins = home.path().join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(
            plugins.join("installed_plugins.json"),
            r#"{"plugins":{"outro@mkt":[{"installPath":"/tmp/x","version":"9.9.9"}]}}"#,
        )
        .unwrap();

        assert_eq!(installed_plugin_rt_in(home.path()), None);
    }

    /// A marca das linhas que o [`home_readers`] imprime.
    const HOME_MARK: &str = "leitura-da-pasta-pessoal";

    /// Não prova comportamento nenhum: é o programa que o teste abaixo roda
    /// com a pasta pessoal que ele escolhe. Rodado pela suíte, só passa.
    /// Imprime o que cada leitora da pasta pessoal respondeu — o ajudante, a
    /// pasta de configuração do Claude Code, a pasta das cópias das ondas, as
    /// pastas de ferramenta do executor da máquina e as pastas de fonte —, com
    /// `-` no lugar da resposta vazia. Uma lista de pastas sai no formato do
    /// `PATH` do sistema.
    #[test]
    fn home_readers() {
        let shown = |path: Option<std::path::PathBuf>| path.map_or_else(|| "-".to_string(), |p| p.display().to_string());
        let listed = |paths: Vec<std::path::PathBuf>| {
            if paths.is_empty() {
                "-".to_string()
            } else {
                std::env::join_paths(paths).expect("pastas sem o separador do PATH").to_string_lossy().into_owned()
            }
        };
        let root = tempfile::tempdir().unwrap();
        println!("{HOME_MARK} home={}", shown(home_dir()));
        println!("{HOME_MARK} config={}", shown(claude_config_dir()));
        println!("{HOME_MARK} copies={}", shown(Some(crate::io::wave_prompt::copies_dir(root.path()))));
        let runner = crate::platform::code_tools::MachineRunner::new("");
        println!("{HOME_MARK} tools={}", listed(runner.user_tool_dirs()));
        println!("{HOME_MARK} fonts={}", listed(crate::platform::fonts::font_dirs()));
    }

    /// Roda o [`home_readers`] num processo próprio, com `HOME` e
    /// `USERPROFILE` valendo `home` e sem as duas variáveis que passam na
    /// frente da pasta pessoal, e devolve cada resposta pelo nome dela.
    fn read_homes(home: &str) -> std::collections::BTreeMap<String, String> {
        let module = module_path!();
        let module = module.split_once("::").map_or(module, |(_, rest)| rest);
        let out = std::process::Command::new(std::env::current_exe().expect("o executável dos testes"))
            .args([&format!("{module}::home_readers"), "--exact", "--nocapture"])
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("MUSTARD_COPIES_DIR")
            .output()
            .expect("o executável dos testes roda");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix(&format!("{HOME_MARK} ")))
            .filter_map(|line| line.split_once('='))
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    /// A pasta pessoal vazia vale como ausente em todas as leitoras: o
    /// ajudante não responde nada, a pasta de configuração do Claude Code
    /// também não, a pasta das cópias cai na pasta temporária do sistema, o
    /// executor da máquina fica sem pasta de ferramenta e as pastas de fonte
    /// ficam só as do sistema — nenhuma monta um caminho relativo à pasta em
    /// que o comando roda. Com a pasta pessoal preenchida, todas partem dela;
    /// só a pasta de fontes do usuário no Windows sai de outra variável.
    #[test]
    fn an_empty_home_counts_as_absent_for_every_reader() {
        let temp = std::env::temp_dir().join("mustard").join("copias");
        let empty = read_homes("");
        let casa = tempfile::tempdir().unwrap();
        let home = casa.path().display().to_string();
        let filled = read_homes(&home);
        let mut wrong: Vec<String> = Vec::new();
        let mut expect = |case: &str, answers: &std::collections::BTreeMap<String, String>, name: &str, ok: &dyn Fn(&str) -> bool| {
            let answer = answers.get(name).map_or("(sem resposta)", String::as_str);
            if !ok(answer) {
                wrong.push(format!("{case}: {name}={answer}"));
            }
        };
        expect("vazia", &empty, "home", &|answer| answer == "-");
        expect("vazia", &empty, "config", &|answer| answer == "-");
        expect("vazia", &empty, "copies", &|answer| std::path::Path::new(answer).starts_with(&temp));
        expect("preenchida", &filled, "home", &|answer| answer == home);
        let config = casa.path().join(".claude");
        expect("preenchida", &filled, "config", &|answer| std::path::Path::new(answer) == config);
        let copies = casa.path().join(".cache").join("mustard").join("copias");
        expect("preenchida", &filled, "copies", &|answer| std::path::Path::new(answer).starts_with(&copies));
        let paths = |answer: &str| std::env::split_paths(answer).collect::<Vec<_>>();
        expect("vazia", &empty, "tools", &|answer| answer == "-");
        expect("vazia", &empty, "fonts", &|answer| answer != "-" && paths(answer).iter().all(|dir| dir.is_absolute()));
        expect("preenchida", &filled, "tools", &|answer| {
            answer != "-" && paths(answer).iter().all(|dir| dir.starts_with(casa.path()))
        });
        expect("preenchida", &filled, "fonts", &|answer| {
            answer != "-" && (cfg!(windows) || paths(answer).iter().any(|dir| dir.starts_with(casa.path())))
        });
        assert!(wrong.is_empty(), "cada leitora responde pela mesma pasta pessoal: {wrong:?}");
    }
}
