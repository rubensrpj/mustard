//! The harness settings: the seed laid into the local settings layer, the
//! point migrations that reach inside keys the merge preserves, the two
//! switches Mustard keeps there (the rtk hook and Claude Code's own
//! signature), the response style of the project's text language, and the
//! reading of a team's `.claude/settings.json` for the lines the seed wrote
//! into it.
//!
//! Nothing here ever reads or writes `~/.claude/`: every path is under the
//! project's own `.claude/`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::domain::config::ProjectConfig;
use crate::io::claude_paths::ClaudePaths;
use crate::io::fs;
use crate::platform::error::Result;
use crate::platform::harness::PLUGIN_NAME;
use crate::platform::i18n::Locale;
use crate::platform::seeds::SETTINGS_SEED;

use super::{InstallMode, SeedOutcome, SETTINGS_JSON, SETTINGS_LOCAL_JSON};

// ---------------------------------------------------------------------------
// settings.json
// ---------------------------------------------------------------------------

/// Marketplace name older `init` builds planted in the PROJECT
/// `settings.json#extraKnownMarketplaces` (retired — see
/// [`retire_planted_plugin_enablement`]).
const PLUGIN_MARKETPLACE: &str = "mustard";

/// `settings.json#enabledPlugins` key older `init` builds planted (retired).
const PLUGIN_ID: &str = "mustard@mustard";

/// The placeholder URL those older builds wrote. Kept ONLY as the recognition
/// literal for the migration: an `extraKnownMarketplaces.mustard` entry whose
/// url equals this literal is provably ours and safe to remove; any other url
/// is user-authored and survives. Plugin enablement is the USER's choice at
/// user scope (`~/.claude/settings.json`) — the project seed never writes it.
const MARKETPLACE_REPO_URL: &str = "REPLACE_WITH_MUSTARD_PLUGIN_MARKETPLACE_GIT_URL";

/// The `settings.json#env` name an older seed planted for the skill-frontmatter
/// gate. No binary ever read it (retired — see
/// [`rename_dead_skill_validate_key`]).
///
/// Written as a plain literal because it has to be: it is the key the migration
/// matches against what is on disk, and only the name itself can do that.
const SKILL_VALIDATE_DEAD_KEY: &str = "MUSTARD_SKILL_VALIDATE_LINES_MODE";

/// The name that gate actually resolves — `size_gate`'s `skill-validate-gate`
/// mode. The live half of the pair [`rename_dead_skill_validate_key`] joins.
const SKILL_VALIDATE_LIVE_KEY: &str = "MUSTARD_SKILL_VALIDATE_GATE_MODE";

/// The command of rtk's own hook, the one Mustard writes into the local
/// settings while `mustard.json#rtk` is on and takes out when it is off.
pub const RTK_HOOK_COMMAND: &str = "rtk hook claude";

/// The tool the rtk hook rewrites: only shell commands pass through it.
const RTK_HOOK_MATCHER: &str = "Bash";

/// Lines older seeds wrote and the current seed no longer carries, spelled
/// exactly as they were written.
///
/// Two uses, both narrow. The local settings file is Mustard's own, so these
/// rules leave it on the next install. In a team's `.claude/settings.json` they
/// are still lines the seed wrote, so the cleanup lists them with the rest.
///
/// The four branch rules named the bases `main` and `master` by hand; the bases
/// come from `mustard.json#git.flow`, and deleting a base is the command guard's
/// to refuse.
const RETIRED_DENY_RULES: &[&str] = &[
    "Bash(git branch -D main:*)",
    "Bash(git branch -D master:*)",
    "Bash(git branch -d main:*)",
    "Bash(git branch -d master:*)",
];

/// As variáveis do `env` que um molde antigo escrevia e o de hoje não traz
/// mais, cada uma com o valor que o molde escrevia, letra por letra.
///
/// Os mesmos dois usos de [`RETIRED_DENY_RULES`]: a linha sai do arquivo de
/// configurações que a instalação grava, e a limpeza do arquivo da equipe
/// ainda a reconhece como do molde. O valor que a pessoa mudou é dela e fica.
///
/// O modo de tamanho da spec: nenhuma conferência o lia, e o molde deixou de
/// escrevê-lo.
const RETIRED_ENV: &[(&str, &str)] = &[("MUSTARD_SPEC_SIZE_MODE", "warn")];

/// The signature value the seed used to write before it wrote the empty one.
const RETIRED_SIGNATURE: &str = "assistant";

/// A chave das configurações do Claude Code que escolhe o estilo de resposta.
const OUTPUT_STYLE_KEY: &str = "outputStyle";

/// O estilo de resposta que o plugin entregava antes dos dois de hoje, pelo
/// nome com que o Claude Code o chamava.
const RETIRED_OUTPUT_STYLE: &str = "mustard-didactic";

/// A variável do `env` que faz o Claude Code repassar os links da barra quando
/// não reconhece o terminal, como numa conexão remota. O valor mora na semente.
const FORCE_HYPERLINK_KEY: &str = "FORCE_HYPERLINK";

/// Seed the harness settings from the compiled-in [`SETTINGS_SEED`].
///
/// The destination follows `mode`: `.claude/settings.json` when shared,
/// `.claude/settings.local.json` when private (see [`settings_dest`]). The
/// merge semantics are the same either way, applied to whichever file is the
/// target — the other one is never read and never written.
///
/// - Absent (or `overwrite == true`): the seed is the base.
/// - Present under merge: the user's file is the base and any top-level seed
///   key it lacks is backfilled — user edits are never clobbered.
///
/// Both paths pass through the point migrations —
/// [`retire_planted_plugin_enablement`], [`rename_dead_skill_validate_key`],
/// [`backfill_own_permission_rules`] and [`retire_old_rules`] — and through the
/// two switches this file holds: in the local layer, the rtk hook follows `rtk`
/// ([`apply_rtk_hook`]), the response style follows `text`
/// ([`apply_output_style`]) and the seed's link variable reaches an `env` that
/// lacks it ([`backfill_force_hyperlink`]); and Claude Code's own signature is kept off
/// ([`turn_signature_off`]). These are the only writes that reach INSIDE a key
/// the merge preserves. Each one is narrow by construction: the top-level merge
/// refuses to guess what an absent sub-key means, so anything that must reach an
/// installed project earns its own rule and says why. The file is
/// only rewritten when the serialized result differs from what is on disk, so
/// a settled project reports [`SeedOutcome::Preserved`].
///
/// # Errors
///
/// An IO error writing the file, or a serialization failure.
pub fn seed_settings(
    claude_dir: &Path,
    overwrite: bool,
    mode: InstallMode,
    rtk: bool,
    text: Locale,
) -> Result<SeedOutcome> {
    let dest = settings_dest(claude_dir, mode);
    let existing_raw = fs::read_to_string(&dest).ok();
    let existed = existing_raw.is_some();

    let seed = parse_json_object(SETTINGS_SEED);
    let mut settings = if overwrite || !existed {
        seed.clone()
    } else {
        // Merge: the user's file is the base (fail-open: a malformed file
        // degrades to the seed, matching the historical init semantics).
        let mut existing = existing_raw
            .as_deref()
            .map(parse_json_object)
            .unwrap_or_default();
        for (key, value) in seed.clone() {
            existing.entry(key).or_insert(value);
        }
        existing
    };

    retire_planted_plugin_enablement(&mut settings);
    rename_dead_skill_validate_key(&mut settings);
    backfill_own_permission_rules(&mut settings, &seed);
    retire_old_rules(&mut settings);
    // rtk's hook belongs to the local layer only: a shared install writes the
    // team's file, and the hook is a choice of whoever programs.
    if mode.is_private() {
        apply_rtk_hook(&mut settings, rtk);
        apply_output_style(&mut settings, text);
        backfill_force_hyperlink(&mut settings, &seed);
    }
    turn_signature_off(&mut settings);

    let mut serialized = serde_json::to_string_pretty(&Value::Object(settings))?;
    serialized.push('\n');
    if existing_raw.as_deref() == Some(serialized.as_str()) {
        return Ok(SeedOutcome::Preserved);
    }
    fs::write_atomic(&dest, serialized.as_bytes())?;
    Ok(if existed { SeedOutcome::Updated } else { SeedOutcome::Created })
}

/// The settings file `mode` seeds, composed through [`ClaudePaths`] — the
/// single owner of `.claude/` path composition, so no call site joins the name
/// by hand.
///
/// `claude_dir` is already `<root>/.claude`, so the project root is recovered
/// from it rather than re-resolved. [`ClaudePaths::compose_unchecked`] is the
/// right constructor for that: the nested `.claude` guard exists to catch a
/// caller passing
/// `.claude` AS the root, which is precisely the mistake being undone here —
/// there is no untrusted input left for it to reject.
pub(super) fn settings_dest(claude_dir: &Path, mode: InstallMode) -> PathBuf {
    let paths = ClaudePaths::compose_unchecked(claude_dir.parent().unwrap_or(claude_dir));
    if mode.is_private() {
        paths.settings_local_json_path()
    } else {
        paths.settings_json_path()
    }
}

/// The project-root-relative name [`super::upsert_project`] reports for the
/// file [`settings_dest`] wrote — the report half of the same decision, and one
/// of the [`super::footprint_rules`] entries.
pub(super) fn settings_footprint(mode: InstallMode) -> &'static str {
    if mode.is_private() {
        SETTINGS_LOCAL_JSON
    } else {
        SETTINGS_JSON
    }
}

/// Remove the plugin-enablement pair older `init` builds planted in the
/// PROJECT settings — and ONLY that pair:
///
/// - `extraKnownMarketplaces.mustard` goes only when its url is the
///   [`MARKETPLACE_REPO_URL`] placeholder (provably ours; a user-authored
///   mustard marketplace with a real url survives).
/// - `enabledPlugins."mustard@mustard"` goes only when the marketplace entry
///   was ours-or-absent (an alias the user wired to a real marketplace stays).
///
/// Emptied containers are dropped so a clean project carries no residue.
/// Every other marketplace/plugin key is untouched.
pub fn retire_planted_plugin_enablement(settings: &mut Map<String, Value>) {
    let planted_marketplace = settings
        .get("extraKnownMarketplaces")
        .and_then(|m| m.get(PLUGIN_MARKETPLACE))
        .and_then(|e| e.pointer("/source/url"))
        .and_then(Value::as_str)
        == Some(MARKETPLACE_REPO_URL);
    if planted_marketplace
        && let Some(obj) = settings
            .get_mut("extraKnownMarketplaces")
            .and_then(Value::as_object_mut)
        {
            obj.remove(PLUGIN_MARKETPLACE);
        }
    let marketplace_present = settings
        .get("extraKnownMarketplaces")
        .and_then(|m| m.get(PLUGIN_MARKETPLACE))
        .is_some();
    if !marketplace_present
        && let Some(obj) = settings.get_mut("enabledPlugins").and_then(Value::as_object_mut) {
            obj.remove(PLUGIN_ID);
        }
    for container in ["extraKnownMarketplaces", "enabledPlugins"] {
        let emptied = settings
            .get(container)
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty);
        if emptied {
            settings.remove(container);
        }
    }
}

/// Rename the dead skill-validate gate key inside an installed
/// `settings.json#env` — [`SKILL_VALIDATE_DEAD_KEY`] becomes
/// [`SKILL_VALIDATE_LIVE_KEY`], carrying the operator's own value over.
///
/// The seed merge is top-level only: a project that already has an `env` object
/// keeps it verbatim, so the corrected name never arrives and the dead one never
/// leaves. Fusing `env` key by key would read every absent variable as "wanted
/// back", which is the guess this engine refuses to make; renaming ONE key that
/// only an old seed ever wrote guesses nothing.
///
/// Nothing happens when there is no `env` object or no dead key. When BOTH names
/// are present the live one wins and the dead one is simply dropped: it is the
/// name the gate reads, so it is already the operator's effective choice.
fn rename_dead_skill_validate_key(settings: &mut Map<String, Value>) {
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    // `shift_remove`, never `remove`. The workspace builds `serde_json` with
    // `preserve_order` (enabled by `apps/scan`, and cargo features UNIFY across a
    // workspace, so this crate gets it too), and there `Map::remove` is a
    // `swap_remove`: it teleports the LAST key of `env` into the hole. Measured on
    // the shipped binary 2026-09-03 — an operator `env` came back with its final
    // key moved four slots up. JSON order carries no meaning, so nothing resolves
    // wrong; it is churn WE cause in a file that is the operator's, and a diff
    // nobody asked for is how a settings file stops being trusted.
    let Some(value) = env.shift_remove(SKILL_VALIDATE_DEAD_KEY) else {
        return;
    };
    env.entry(SKILL_VALIDATE_LIVE_KEY.to_string()).or_insert(value);
}


/// Backfill the seed's own permission rules into an installed settings file:
/// the `Bash(mustard-rt run …)` allow rules and every deny rule the seed
/// carries, the machine rules among them.
///
/// **Why a default that only reaches a fresh install is not a default.** The
/// seed merge is top-level only: a project that already has a `permissions`
/// object keeps it verbatim, so a rule added to the seed never arrives anywhere
/// it is already installed. Measured 2026-09-07: the operator was prompted for
/// `mustard-rt run pr-merge` on every merge, ran it by hand three times, and the
/// remedy — adding the rule — would have helped only projects that did not exist
/// yet. The same holds for a machine rule the seed gains, like the one that
/// refuses formatting a disk.
///
/// Strictly narrow, so it guesses nothing:
///
/// - only rules the SEED declares; on the allow side, only those naming
///   `mustard-rt run` — the harness's own commands, never anything the
///   operator's project runs;
/// - only ADDS. Nothing is removed, nothing is reordered, and a rule the
///   operator already has (in any spelling that matches exactly) is left alone;
/// - a rule the operator put in another list WINS, because that is a decision
///   and this is a default: an operator who denied a Mustard command is not
///   asking for it back, and one who allowed a command the seed denies chose so.
///
/// Nothing happens when the file declares no `permissions` object.
fn backfill_own_permission_rules(settings: &mut Map<String, Value>, seed: &Map<String, Value>) {
    let seed_list = |list: &str| -> Vec<String> {
        seed.get("permissions")
            .and_then(|p| p.get(list))
            .and_then(Value::as_array)
            .map(|rules| rules.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    };
    let allow: Vec<String> =
        seed_list("allow").into_iter().filter(|r| r.starts_with("Bash(mustard-rt run ")).collect();
    let deny = seed_list("deny");
    let Some(perms) = settings.get_mut("permissions").and_then(Value::as_object_mut) else {
        return;
    };
    for (list, wanted) in [("allow", allow), ("deny", deny)] {
        // A rule the operator filed in any OTHER list is a decision, and a
        // default never overrides a decision.
        let decided: Vec<String> = ["allow", "deny", "ask"]
            .iter()
            .filter(|other| **other != list)
            .filter_map(|other| perms.get(*other))
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        let Some(rules) = perms.get_mut(list).and_then(Value::as_array_mut) else {
            continue;
        };
        let present: Vec<String> = rules.iter().filter_map(Value::as_str).map(str::to_string).collect();
        for rule in wanted {
            if !present.contains(&rule) && !decided.contains(&rule) {
                rules.push(Value::String(rule));
            }
        }
    }
}

/// Tira das configurações locais as regras de bloqueio e as variáveis do
/// `env` que um molde antigo escrevia e o de hoje aposentou
/// ([`RETIRED_DENY_RULES`], [`RETIRED_ENV`]), pelo texto exato. O arquivo
/// local é do Mustard, então a linha que ele não entrega mais sai dele. O
/// `env` que fica vazio fica, vazio: tirá-lo traria o `env` inteiro do molde
/// de volta na instalação seguinte.
fn retire_old_rules(settings: &mut Map<String, Value>) {
    if let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) {
        env.retain(|name, value| !is_retired_env(name, value));
    }
    let Some(deny) = settings
        .get_mut("permissions")
        .and_then(Value::as_object_mut)
        .and_then(|p| p.get_mut("deny"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    deny.retain(|rule| !rule.as_str().is_some_and(|r| RETIRED_DENY_RULES.contains(&r)));
}

/// Put rtk's own hook into the settings when `on`, take it out when not.
///
/// The hook is the one rtk ships (`rtk hook claude`) on the shell tool, never a
/// copy of it. Turned on, it is added once, whatever entry already carries it;
/// turned off, every hook with that command leaves, and an entry, an event or
/// the `hooks` object that ends up empty leaves with it — the file keeps
/// nothing that only existed to hold the hook.
pub fn apply_rtk_hook(settings: &mut Map<String, Value>, on: bool) {
    if on {
        if rtk_hook_present(settings) {
            return;
        }
        let hooks = settings.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
        let Some(hooks) = hooks.as_object_mut() else {
            return;
        };
        let event = hooks.entry("PreToolUse").or_insert_with(|| Value::Array(Vec::new()));
        if let Some(entries) = event.as_array_mut() {
            entries.push(serde_json::json!({
                "matcher": RTK_HOOK_MATCHER,
                "hooks": [{ "type": "command", "command": RTK_HOOK_COMMAND }],
            }));
        }
        return;
    }
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(entries) = hooks.get_mut("PreToolUse").and_then(Value::as_array_mut) {
        for entry in entries.iter_mut() {
            if let Some(list) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                list.retain(|hook| !is_rtk_hook(hook));
            }
        }
        entries.retain(|entry| entry.get("hooks").and_then(Value::as_array).is_none_or(|l| !l.is_empty()));
        if entries.is_empty() {
            hooks.shift_remove("PreToolUse");
        }
    }
    if hooks.is_empty() {
        settings.shift_remove("hooks");
    }
}

/// Whether the settings carry rtk's hook on the tool-call event.
#[must_use]
pub fn rtk_hook_present(settings: &Map<String, Value>) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("hooks").and_then(Value::as_array))
        .flatten()
        .any(is_rtk_hook)
}

fn is_rtk_hook(hook: &Value) -> bool {
    hook.get("command").and_then(Value::as_str).map(str::trim) == Some(RTK_HOOK_COMMAND)
}

/// O estilo de resposta do Mustard no idioma `text`, pelo nome com que o
/// Claude Code chama um estilo de plugin: o nome do plugin, dois-pontos e o
/// `name` do arquivo do estilo (`mustard:mustard-pt-BR`).
#[must_use]
pub fn output_style_for(text: Locale) -> String {
    format!("{PLUGIN_NAME}:mustard-{text}")
}

/// Se o valor da chave é um estilo que só o Mustard escreve: o de um dos dois
/// idiomas, ou o que eles substituíram.
fn is_mustard_style(value: &str) -> bool {
    value == format!("{PLUGIN_NAME}:{RETIRED_OUTPUT_STYLE}")
        || [Locale::PtBr, Locale::EnUs].into_iter().any(|text| value == output_style_for(text))
}

/// Escolhe o estilo de resposta do idioma `text` nas configurações locais.
///
/// A chave ausente, ou com um estilo do próprio Mustard, recebe o do idioma:
/// trocar o `language.text` e instalar de novo troca o estilo. Um estilo que a
/// pessoa escolheu por conta própria fica como está.
pub fn apply_output_style(settings: &mut Map<String, Value>, text: Locale) {
    let chosen_by_person = settings
        .get(OUTPUT_STYLE_KEY)
        .and_then(Value::as_str)
        .is_some_and(|value| !is_mustard_style(value));
    if !chosen_by_person {
        settings.insert(OUTPUT_STYLE_KEY.to_string(), Value::String(output_style_for(text)));
    }
}

/// Leva ao `env` das configurações locais o valor que a semente dá à variável
/// dos links ([`FORCE_HYPERLINK_KEY`]), para o Ctrl+clique na barra funcionar
/// por conexão remota sem a pessoa digitar nada.
///
/// A mescla de cima só completa chaves de topo: o projeto já instalado tem
/// `env` e o guarda inteiro, então a variável que a semente ganhou nunca
/// chegaria a ele. Esta regra põe só essa variável, e só quando ela falta: o
/// valor que a pessoa já deu a ela, como `"0"`, fica. Um `env` que não é
/// objeto também fica como está.
fn backfill_force_hyperlink(settings: &mut Map<String, Value>, seed: &Map<String, Value>) {
    let Some(value) = seed.get("env").and_then(|env| env.get(FORCE_HYPERLINK_KEY)) else {
        return;
    };
    let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    env.entry(FORCE_HYPERLINK_KEY.to_string()).or_insert_with(|| value.clone());
}

/// Keep the signature Claude Code adds to commits and pull requests off: both
/// halves of `attribution` empty, which is how Claude Code reads "none".
fn turn_signature_off(settings: &mut Map<String, Value>) {
    if signature_on(settings) {
        settings.insert("attribution".to_string(), serde_json::json!({ "commit": "", "pr": "" }));
    }
}

/// Whether the settings leave Claude Code's signature on: no `attribution`
/// object (Claude Code's default is to sign), or either half not empty.
#[must_use]
pub fn signature_on(settings: &Map<String, Value>) -> bool {
    let Some(attribution) = settings.get("attribution").and_then(Value::as_object) else {
        return true;
    };
    ["commit", "pr"].iter().any(|half| attribution.get(*half).and_then(Value::as_str) != Some(""))
}

/// Parse a JSON object fail-open: anything that is not a JSON object yields
/// an empty map (mirrors the CLI's historical `read_json_object` semantics).
pub(super) fn parse_json_object(raw: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The switches, as the doctor reads them
// ---------------------------------------------------------------------------

/// The two choices `mustard.json` holds for the project, next to what the
/// local settings really carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switches {
    /// `mustard.json#enabled`: off means no Mustard hook acts in this project.
    pub enabled: bool,
    /// `mustard.json#rtk`: whether rtk's hook should be in the local settings.
    pub rtk: bool,
    /// Whether the local settings carry rtk's hook. `None` when the file is
    /// there and cannot be read as JSON: nothing can be said about it.
    pub rtk_hook: Option<bool>,
    /// Whether the local settings leave Claude Code's signature on. `None`
    /// under the same condition as [`Self::rtk_hook`].
    pub signature_on: Option<bool>,
}

impl Switches {
    /// Read the two choices and the local settings of the project at `root`.
    /// A missing settings file carries neither the hook nor the signature
    /// setting, so it is read as an empty object.
    #[must_use]
    pub fn read(root: &Path) -> Self {
        let config = ProjectConfig::load(root);
        let local = settings_dest(&root.join(".claude"), InstallMode::Private);
        let settings = match fs::read_to_string(&local) {
            Ok(raw) => serde_json::from_str::<Value>(&raw).ok().and_then(|v| v.as_object().cloned()),
            Err(_) => Some(Map::new()),
        };
        Self {
            enabled: config.enabled(),
            rtk: config.rtk(),
            rtk_hook: settings.as_ref().map(rtk_hook_present),
            signature_on: settings.as_ref().map(signature_on),
        }
    }

    /// The choice in `mustard.json` and the hook in the local settings
    /// disagree.
    #[must_use]
    pub fn rtk_diverges(&self) -> bool {
        self.rtk_hook.is_some_and(|hook| hook != self.rtk)
    }
}

// ---------------------------------------------------------------------------
// A team's settings file: the lines the seed wrote into it
// ---------------------------------------------------------------------------

/// `settings` without the lines the seed wrote, and the name of each line
/// taken out, in file order.
///
/// A line is the seed's when its text is exactly the seed's: a top-level key
/// with the seed's value, an `env` variable with the seed's value, an allow or
/// ask rule the seed lists, and the signature an older seed wrote. Anything the
/// team changed, even by one character, is theirs and stays. A container the
/// removal empties goes too.
///
/// Deny rules always stay, even the seed's: a protection rule never leaves the
/// team's file unless someone asks, so a file with one is never emptied.
#[must_use]
pub fn without_seed_lines(settings: &Map<String, Value>) -> (Map<String, Value>, Vec<String>) {
    let seed = parse_json_object(SETTINGS_SEED);
    let mut out = settings.clone();
    let mut removed = Vec::new();
    for (key, value) in settings {
        match key.as_str() {
            "env" => {
                let seed_env = seed.get("env").and_then(Value::as_object);
                if let (Some(env), Some(seed_env)) = (out.get_mut("env").and_then(Value::as_object_mut), seed_env) {
                    let mine: Vec<String> = env
                        .iter()
                        .filter(|(name, v)| seed_value_of_env(seed_env, name) == Some(*v) || is_retired_env(name, v))
                        .map(|(name, _)| name.clone())
                        .collect();
                    for name in mine {
                        env.shift_remove(&name);
                        removed.push(format!("env.{name}"));
                    }
                }
            }
            "permissions" => {
                if let Some(perms) = out.get_mut("permissions").and_then(Value::as_object_mut) {
                    for list in SEED_LINE_LISTS {
                        let seeded = seed_rules(&seed, list);
                        let Some(rules) = perms.get_mut(list).and_then(Value::as_array_mut) else { continue };
                        rules.retain(|rule| {
                            let Some(text) = rule.as_str() else { return true };
                            let ours = seeded.iter().any(|s| s == text);
                            if ours {
                                removed.push(format!("permissions.{list}: {text}"));
                            }
                            !ours
                        });
                    }
                    for list in SEED_LINE_LISTS {
                        if perms.get(list).and_then(Value::as_array).is_some_and(Vec::is_empty) {
                            perms.shift_remove(list);
                        }
                    }
                }
            }
            "attribution" => {
                let retired = serde_json::json!({ "commit": RETIRED_SIGNATURE, "pr": RETIRED_SIGNATURE });
                if seed.get(key) == Some(value) || *value == retired {
                    out.shift_remove(key);
                    removed.push(key.clone());
                }
            }
            _ => {
                if seed.get(key) == Some(value) {
                    out.shift_remove(key);
                    removed.push(key.clone());
                }
            }
        }
    }
    for container in ["env", "permissions"] {
        if out.get(container).and_then(Value::as_object).is_some_and(Map::is_empty) {
            out.shift_remove(container);
        }
    }
    (out, removed)
}

/// The seed's value for an `env` variable, reading the dead skill-validate name
/// as the live one an older seed wrote it under.
fn seed_value_of_env<'a>(seed_env: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    let name = if name == SKILL_VALIDATE_DEAD_KEY { SKILL_VALIDATE_LIVE_KEY } else { name };
    seed_env.get(name)
}

/// `true` para a variável do `env` que um molde antigo escrevia, com o valor
/// que ele escrevia, e que o de hoje aposentou ([`RETIRED_ENV`]).
fn is_retired_env(name: &str, value: &Value) -> bool {
    RETIRED_ENV.iter().any(|(retired, written)| name == *retired && value.as_str() == Some(*written))
}

/// The permission lists whose seed rules leave a team's file. `deny` is not
/// one of them: its rules stay.
const SEED_LINE_LISTS: [&str; 2] = ["allow", "ask"];

/// The rules the seed lists under `permissions.<list>`.
fn seed_rules(seed: &Map<String, Value>, list: &str) -> Vec<String> {
    seed.get("permissions")
        .and_then(|p| p.get(list))
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// The project-root-relative name of the team's settings file.
pub(super) const TEAM_SETTINGS: &str = SETTINGS_JSON;

/// Where the team's settings file of the project at `root` lives.
pub(super) fn team_settings_path(root: &Path) -> PathBuf {
    settings_dest(&root.join(".claude"), InstallMode::Shared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::project_seed::{footprint_rules, upsert_project};
    use serde_json::json;
    use std::fs as std_fs;
    use tempfile::tempdir;

    // --- the harness's own permission rules ---------------------------------

    /// A permission default Mustard ships reaches a project that is ALREADY
    /// installed — and touches nothing else in the operator's file.
    ///
    /// The seed merge is top-level only, so a project that has a `permissions`
    /// object keeps it verbatim and a rule added to the seed lands nowhere it is
    /// already installed. Measured 2026-09-07: the operator was prompted for
    /// `mustard-rt run pr-merge` on every merge and ran it by hand three times;
    /// adding the rule to the seed alone would have helped only projects that did
    /// not exist yet.
    #[test]
    fn an_installed_project_receives_the_harness_own_permission_rules() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::create_dir_all(root.join(".claude")).unwrap();
        // An installed project: it HAS a permissions object, with one Mustard
        // rule from an older seed and one rule the operator wrote themselves.
        std_fs::write(
            root.join(".claude/settings.json"),
            serde_json::to_string_pretty(&json!({
                "permissions": {
                    "allow": ["Read", "Bash(mustard-rt run qa-run:*)", "Bash(npm test:*)"],
                    "deny": ["Bash(rm -rf:*)"]
                }
            }))
            .unwrap()
                + "\n",
        )
        .unwrap();

        seed_settings(&root.join(".claude"), false, InstallMode::Shared, true, Locale::PtBr).unwrap();

        let settings: Value = serde_json::from_str(
            &std_fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        let allow: Vec<&str> = settings["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();

        // Every `mustard-rt run` rule the seed declares is now present.
        for rule in seed_own_rules() {
            assert!(
                allow.contains(&rule.as_str()),
                "{rule} must reach an installed project: {allow:?}",
            );
        }
        // What the operator wrote is untouched, in its own order, once each.
        assert_eq!(allow[0], "Read", "the operator's list keeps its head: {allow:?}");
        assert_eq!(allow[1], "Bash(mustard-rt run qa-run:*)", "{allow:?}");
        assert_eq!(allow[2], "Bash(npm test:*)", "{allow:?}");
        assert_eq!(
            allow.iter().filter(|r| **r == "Bash(mustard-rt run qa-run:*)").count(),
            1,
            "a rule already present is never duplicated: {allow:?}",
        );
        // Nothing else in the file was invented: the operator declared no `env`,
        // and the top-level merge is what backfills it — not this rule.
        assert_eq!(settings["permissions"]["deny"][0], json!("Bash(rm -rf:*)"));
    }

    /// A DENIED Mustard command stays denied. A default never overrides a
    /// decision — an operator who refused a command is not asking for it back.
    #[test]
    fn a_refused_rule_is_not_backfilled() {
        let seed = parse_json_object(SETTINGS_SEED);
        let own = seed_own_rules();
        let refused = own[0].clone();

        let mut settings = parse_json_object(
            &serde_json::to_string(&json!({
                "permissions": { "allow": ["Read"], "deny": [refused.clone()] }
            }))
            .unwrap(),
        );
        backfill_own_permission_rules(&mut settings, &seed);

        let allow: Vec<&str> = settings["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            !allow.contains(&refused.as_str()),
            "a denied rule must not come back: {allow:?}",
        );
        // The rest of the seed's own rules still arrive — the refusal is
        // per-rule, never a blanket opt-out.
        assert!(allow.len() > 1, "the other rules still arrive: {allow:?}");
    }

    /// A file that declares no `permissions` at all is left to the top-level
    /// merge, which plants the seed's whole object. This rule adds nothing.
    #[test]
    fn a_file_without_permissions_is_left_to_the_top_level_merge() {
        let seed = parse_json_object(SETTINGS_SEED);
        let mut settings = parse_json_object(r#"{"env":{"X":"1"}}"#);
        backfill_own_permission_rules(&mut settings, &seed);
        assert!(settings.get("permissions").is_none(), "nothing invented: {settings:?}");
    }

    /// The seed's own rules, read from the seed itself — so the tests above
    /// cannot drift from what actually ships.
    fn seed_own_rules() -> Vec<String> {
        parse_json_object(SETTINGS_SEED)["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .filter(|r| r.starts_with("Bash(mustard-rt run "))
            .map(str::to_string)
            .collect()
    }

    // --- retire_planted_plugin_enablement (moved from the CLI init) ----------

    #[test]
    fn retire_removes_only_the_planted_placeholder_pair() {
        // The exact pair an older init planted: placeholder marketplace URL +
        // the mustard@mustard alias. Both go; the user's own keys survive.
        let mut settings: Map<String, Value> = serde_json::from_str(&format!(
            r#"{{"extraKnownMarketplaces":{{
                    "acme":{{"source":{{"source":"git","url":"x"}}}},
                    "mustard":{{"source":{{"source":"git","url":"{MARKETPLACE_REPO_URL}"}}}}}},
                "enabledPlugins":{{"acme@acme":true,"mustard@mustard":true}}}}"#,
        ))
        .unwrap();

        retire_planted_plugin_enablement(&mut settings);

        assert!(settings["extraKnownMarketplaces"].get("mustard").is_none(), "placeholder gone");
        assert!(settings["enabledPlugins"].get("mustard@mustard").is_none(), "alias gone");
        // Theirs survive.
        assert!(settings["extraKnownMarketplaces"].get("acme").is_some());
        assert_eq!(settings["enabledPlugins"]["acme@acme"], json!(true));
    }

    #[test]
    fn retire_preserves_a_user_authored_mustard_marketplace() {
        // A REAL url under the `mustard` key is the user's wiring — the entry
        // and the alias that resolves against it both stay.
        let mut settings: Map<String, Value> = serde_json::from_str(
            r#"{"extraKnownMarketplaces":{"mustard":{"source":{"source":"git","url":"https://example.com/real.git"}}},
                "enabledPlugins":{"mustard@mustard":true}}"#,
        )
        .unwrap();

        retire_planted_plugin_enablement(&mut settings);

        assert!(settings["extraKnownMarketplaces"].get("mustard").is_some(), "real url stays");
        assert_eq!(settings["enabledPlugins"]["mustard@mustard"], json!(true), "alias stays");
    }

    #[test]
    fn retire_drops_emptied_containers() {
        // A settings.json whose ONLY marketplace/plugin keys were ours ends up
        // with no residue containers at all.
        let mut settings: Map<String, Value> = serde_json::from_str(&format!(
            r#"{{"extraKnownMarketplaces":{{"mustard":{{"source":{{"source":"git","url":"{MARKETPLACE_REPO_URL}"}}}}}},
                "enabledPlugins":{{"mustard@mustard":true}}}}"#,
        ))
        .unwrap();

        retire_planted_plugin_enablement(&mut settings);

        assert!(settings.get("extraKnownMarketplaces").is_none(), "emptied container dropped");
        assert!(settings.get("enabledPlugins").is_none(), "emptied container dropped");
    }

    // --- rename_dead_skill_validate_key --------------------------------------

    #[test]
    fn the_dead_skill_validate_key_is_renamed() {
        // An INSTALLED project: it already has `env`, so the top-level merge
        // preserves that object whole and the corrected name can only arrive
        // through the point migration.
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::create_dir_all(root.join(".claude")).unwrap();
        std_fs::write(
            root.join(".claude/settings.json"),
            format!(r#"{{"env":{{"{SKILL_VALIDATE_DEAD_KEY}":"warn","MY_OWN":"1"}}}}"#),
        )
        .unwrap();

        upsert_project(root, None, InstallMode::Shared).unwrap();

        let settings: Value = serde_json::from_str(
            &std_fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        let env = &settings["env"];
        assert_eq!(env[SKILL_VALIDATE_LIVE_KEY], json!("warn"), "operator's value carried over");
        assert!(env.get(SKILL_VALIDATE_DEAD_KEY).is_none(), "dead name gone");
        assert_eq!(env["MY_OWN"], json!("1"), "the rest of their env survives");
    }

    /// The operator's OWN keys keep the order they were written in.
    ///
    /// `Map::remove` under `preserve_order` is a `swap_remove`: it teleports the
    /// last key into the hole. Measured on the shipped binary 2026-09-03, an
    /// operator's final env key moved four slots up — values intact, file
    /// scrambled. Nothing resolves wrong, and that is exactly why no other test
    /// would ever have caught it: a diff nobody asked for in a file that is theirs.
    #[test]
    fn the_migration_leaves_the_operators_other_keys_where_they_were() {
        let mut settings: Map<String, Value> = serde_json::from_str(&format!(
            r#"{{"env":{{"A_FIRST":"1","{SKILL_VALIDATE_DEAD_KEY}":"warn","B_AFTER":"2","Z_LAST":"3"}}}}"#
        ))
        .unwrap();

        rename_dead_skill_validate_key(&mut settings);

        let env = settings["env"].as_object().expect("env survives as an object");
        let order: Vec<&str> = env.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            vec!["A_FIRST", "B_AFTER", "Z_LAST", SKILL_VALIDATE_LIVE_KEY],
            "the dead key is lifted OUT and the live one appended; nothing else moves",
        );
    }

    #[test]
    fn the_live_skill_validate_key_wins_when_both_names_are_present() {
        // The live name is what the gate reads, so it is already the effective
        // choice: the dead one is dropped without overwriting it.
        let mut settings: Map<String, Value> = serde_json::from_str(&format!(
            r#"{{"env":{{"{SKILL_VALIDATE_DEAD_KEY}":"warn","{SKILL_VALIDATE_LIVE_KEY}":"strict"}}}}"#,
        ))
        .unwrap();

        rename_dead_skill_validate_key(&mut settings);

        assert_eq!(settings["env"][SKILL_VALIDATE_LIVE_KEY], json!("strict"));
        assert!(settings["env"].get(SKILL_VALIDATE_DEAD_KEY).is_none());
    }

    #[test]
    fn a_settings_file_without_the_dead_key_is_untouched() {
        // No `env` at all, and an `env` carrying only the operator's own names:
        // neither gains a key.
        let mut no_env: Map<String, Value> = serde_json::from_str(r#"{"permissions":{}}"#).unwrap();
        rename_dead_skill_validate_key(&mut no_env);
        assert!(no_env.get("env").is_none(), "no env object is invented");

        let mut theirs: Map<String, Value> =
            serde_json::from_str(r#"{"env":{"MY_OWN":"1"}}"#).unwrap();
        rename_dead_skill_validate_key(&mut theirs);
        assert!(theirs["env"].get(SKILL_VALIDATE_LIVE_KEY).is_none(), "no name is planted");
        assert_eq!(theirs["env"]["MY_OWN"], json!("1"));
    }

    // --- install mode ---------------------------------------------------------

    /// The path `seed_settings` writes and the name `upsert_project` reports for
    /// it are two halves of one decision. They are computed by two functions, so
    /// pin them together — a drift here would report a file nobody wrote.
    #[test]
    fn the_settings_destination_and_its_reported_name_agree() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        for mode in [InstallMode::Shared, InstallMode::Private] {
            let dest = settings_dest(&claude, mode);
            let name = settings_footprint(mode);
            assert!(
                dest.ends_with(name.trim_start_matches(".claude/")),
                "{mode:?}: reported {name} but wrote {dest:?}",
            );
            assert!(
                footprint_rules().iter().any(|p| p == name),
                "{mode:?}: {name} is not in the footprint the private mode hides",
            );
        }
        assert_ne!(
            settings_dest(&claude, InstallMode::Shared),
            settings_dest(&claude, InstallMode::Private),
            "the two modes must not share a destination",
        );
    }

    // --- the switches in the local settings ----------------------------------

    fn local_settings(root: &Path) -> Map<String, Value> {
        parse_json_object(&std_fs::read_to_string(root.join(".claude/settings.local.json")).unwrap())
    }

    /// Com a opção ligada, o gancho do rtk entra uma vez só nas configurações
    /// locais; desligada, ele sai, e o que só existia para guardá-lo sai
    /// junto; o gancho que o usuário pôs ao lado fica.
    #[test]
    fn the_rtk_hook_comes_in_once_and_leaves_with_its_empty_containers() {
        let mut settings = parse_json_object(r#"{"env":{"X":"1"}}"#);
        apply_rtk_hook(&mut settings, true);
        apply_rtk_hook(&mut settings, true);
        assert!(rtk_hook_present(&settings));
        let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "added once: {settings:?}");
        assert_eq!(entries[0]["matcher"], json!("Bash"));

        apply_rtk_hook(&mut settings, false);
        assert!(!rtk_hook_present(&settings));
        assert!(settings.get("hooks").is_none(), "nothing kept only to hold the hook: {settings:?}");
        assert_eq!(settings["env"]["X"], json!("1"));

        let mut shared = parse_json_object(
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[
                {"type":"command","command":"rtk hook claude"},
                {"type":"command","command":"my-own-check"}]}]}}"#,
        );
        apply_rtk_hook(&mut shared, false);
        let left = shared["hooks"]["PreToolUse"][0]["hooks"].as_array().unwrap();
        assert_eq!(left.len(), 1, "{shared:?}");
        assert_eq!(left[0]["command"], json!("my-own-check"));
    }

    /// A assinatura do Claude Code fica desligada nas configurações locais:
    /// a de um molde antigo vira vazia, e a leitura diz se ela está ligada.
    #[test]
    fn the_signature_is_turned_off_and_read_back() {
        assert!(signature_on(&Map::new()), "no attribution object means Claude Code signs");
        let mut old = parse_json_object(r#"{"attribution":{"commit":"assistant","pr":"assistant"}}"#);
        assert!(signature_on(&old));
        turn_signature_off(&mut old);
        assert!(!signature_on(&old));
        assert_eq!(old["attribution"], json!({"commit": "", "pr": ""}));
        let half = parse_json_object(r#"{"attribution":{"commit":"","pr":"Made by me"}}"#);
        assert!(signature_on(&half), "one half still signs");
    }

    /// Numa instalação que já existe, as regras de apagar branch com o nome
    /// da base escrito à mão saem, e a regra que recusa formatar disco chega.
    #[test]
    fn an_installed_project_loses_the_retired_rules_and_gets_the_machine_rules() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        std_fs::write(
            claude.join("settings.local.json"),
            r#"{"permissions":{"allow":["Bash(format:*)"],"deny":["Bash(git branch -D main:*)","Bash(mkfs*)","Bash(my-rule:*)"]}}"#,
        )
        .unwrap();

        seed_settings(&claude, false, InstallMode::Private, true, Locale::PtBr).unwrap();

        let settings = local_settings(dir.path());
        let deny: Vec<&str> = settings["permissions"]["deny"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        for retired in RETIRED_DENY_RULES {
            assert!(!deny.contains(retired), "{retired} stayed: {deny:?}");
        }
        assert!(deny.contains(&"Bash(my-rule:*)"), "the operator's own rule stays: {deny:?}");
        assert!(deny.contains(&"Bash(shutdown:*)"), "a machine rule of the seed arrives: {deny:?}");
        assert!(
            !deny.contains(&"Bash(format:*)"),
            "a rule the operator allowed on purpose is not denied behind their back: {deny:?}",
        );
        assert_eq!(deny.iter().filter(|r| **r == "Bash(mkfs*)").count(), 1, "{deny:?}");
    }

    /// O molde traz a regra de formatar disco, e nenhuma regra dele escreve o
    /// nome de uma base.
    #[test]
    fn the_seed_denies_formatting_and_names_no_base() {
        let seed = parse_json_object(SETTINGS_SEED);
        let deny: Vec<&str> = seed["permissions"]["deny"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(deny.contains(&"Bash(format:*)"), "{deny:?}");
        for rule in &deny {
            assert!(!rule.contains("main") && !rule.contains("master"), "a base name is written by hand: {rule}");
        }
        assert!(!signature_on(&seed), "a fresh install signs nothing");
    }

    // --- a team's settings file ---------------------------------------------

    /// Só as linhas com o texto exato do molde saem de um `settings.json` da
    /// equipe; a linha que a equipe mudou fica, e as regras de bloqueio ficam
    /// todas, mesmo as do molde: do arquivo que só tinha o molde sobram elas.
    #[test]
    fn only_the_seed_lines_leave_a_team_settings_file() {
        let seed = parse_json_object(SETTINGS_SEED);
        let (left, removed) = without_seed_lines(&seed);
        assert_eq!(
            Value::Object(left),
            json!({ "permissions": { "deny": seed["permissions"]["deny"].clone() } }),
            "a file that is only the seed keeps only its deny rules",
        );
        assert!(removed.iter().any(|r| r == "statusLine"), "{removed:?}");
        assert!(!removed.iter().any(|r| r.starts_with("permissions.deny")), "{removed:?}");

        let team = parse_json_object(
            r#"{"env":{"MUSTARD_SKILL_SIZE_MODE":"strict","TEAM":"1","MUSTARD_BOUNDARY_MODE":"warn"},
                "permissions":{"allow":["Read","Bash(npm test:*)"],"deny":["Bash(git branch -D main:*)"]},
                "attribution":{"commit":"assistant","pr":"assistant"},
                "cleanupPeriodDays":7,
                "hooks":{"Stop":[]}}"#,
        );
        let (left, removed) = without_seed_lines(&team);
        assert_eq!(
            removed,
            ["env.MUSTARD_BOUNDARY_MODE", "permissions.allow: Read", "attribution"],
        );
        assert_eq!(left["env"], json!({"MUSTARD_SKILL_SIZE_MODE": "strict", "TEAM": "1"}), "a changed value is theirs");
        assert_eq!(
            left["permissions"],
            json!({"allow": ["Bash(npm test:*)"], "deny": ["Bash(git branch -D main:*)"]}),
            "a deny rule an older seed wrote stays too",
        );
        assert_eq!(left["cleanupPeriodDays"], json!(7));
        assert!(left.get("hooks").is_some());
        assert!(left.get("attribution").is_none());
    }

    /// O modo de tamanho da spec saiu do molde: a instalação nova não o
    /// escreve; na instalação que já existe, a linha com o valor que o molde
    /// antigo escrevia sai das configurações locais, e a limpeza do arquivo da
    /// equipe ainda a reconhece como do molde. O valor que a pessoa mudou é
    /// dela e fica, nos dois arquivos.
    #[test]
    fn the_retired_spec_size_line_leaves_and_is_still_known_as_the_seeds() {
        let seed = parse_json_object(SETTINGS_SEED);
        assert!(seed["env"].get("MUSTARD_SPEC_SIZE_MODE").is_none(), "the seed still writes it");

        for (value, leaves) in [("warn", true), ("strict", false)] {
            let dir = tempdir().unwrap();
            let claude = dir.path().join(".claude");
            std_fs::create_dir_all(&claude).unwrap();
            std_fs::write(
                claude.join("settings.local.json"),
                format!(r#"{{"env":{{"MUSTARD_SPEC_SIZE_MODE":"{value}","MY_OWN":"1"}}}}"#),
            )
            .unwrap();
            seed_settings(&claude, false, InstallMode::Private, true, Locale::PtBr).unwrap();
            let env = local_settings(dir.path())["env"].clone();
            assert_eq!(env.get("MUSTARD_SPEC_SIZE_MODE").is_none(), leaves, "{value}: {env}");
            assert_eq!(env["MY_OWN"], json!("1"), "{value}: {env}");

            let team = parse_json_object(&format!(r#"{{"env":{{"MUSTARD_SPEC_SIZE_MODE":"{value}"}}}}"#));
            let (left, removed) = without_seed_lines(&team);
            assert_eq!(removed.contains(&"env.MUSTARD_SPEC_SIZE_MODE".to_string()), leaves, "{value}: {removed:?}");
            assert_eq!(left.is_empty(), leaves, "{value}: {left:?}");
        }
    }

    /// O estilo de resposta segue o idioma do texto: a instalação local grava
    /// o do `language.text`, troca quando o idioma troca, e deixa o estilo que
    /// a pessoa escolheu. O nome gravado é o que o Claude Code dá ao estilo do
    /// plugin: o nome do plugin e o `name` do arquivo do estilo.
    #[test]
    fn the_response_style_follows_the_text_language() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        let style = || -> Value {
            let raw = std_fs::read_to_string(claude.join("settings.local.json")).unwrap();
            serde_json::from_str::<Value>(&raw).unwrap()["outputStyle"].clone()
        };

        seed_settings(&claude, false, InstallMode::Private, true, Locale::EnUs).unwrap();
        assert_eq!(style(), json!("mustard:mustard-en-US"));
        seed_settings(&claude, false, InstallMode::Private, true, Locale::PtBr).unwrap();
        assert_eq!(style(), json!("mustard:mustard-pt-BR"), "a language change swaps the style");

        let mut settings = parse_json_object(r#"{"outputStyle":"Explanatory"}"#);
        apply_output_style(&mut settings, Locale::EnUs);
        assert_eq!(settings["outputStyle"], json!("Explanatory"), "the person's own style stays");
        let mut retired = parse_json_object(r#"{"outputStyle":"mustard:mustard-didactic"}"#);
        apply_output_style(&mut retired, Locale::EnUs);
        assert_eq!(retired["outputStyle"], json!("mustard:mustard-en-US"));

        let shared = dir.path().join("shared/.claude");
        std_fs::create_dir_all(&shared).unwrap();
        seed_settings(&shared, false, InstallMode::Shared, true, Locale::EnUs).unwrap();
        let team: Value = serde_json::from_str(&std_fs::read_to_string(shared.join("settings.json")).unwrap()).unwrap();
        assert!(team.get("outputStyle").is_none(), "the team's file never gets the style");

        // A outra metade: o plugin entrega um estilo com esse nome.
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin");
        let manifest: Value =
            serde_json::from_str(&std_fs::read_to_string(plugin.join(".claude-plugin/plugin.json")).unwrap()).unwrap();
        for text in [Locale::PtBr, Locale::EnUs] {
            let body = std_fs::read_to_string(plugin.join(format!("output-styles/mustard-{text}.md"))).unwrap();
            let name = body.lines().find_map(|l| l.strip_prefix("name: ")).unwrap_or_default();
            assert_eq!(
                format!("{}:{name}", manifest["name"].as_str().unwrap()),
                output_style_for(text),
                "the style the installer names is not the one the plugin ships",
            );
            assert!(!body.contains("force-for-plugin"), "a forced style would override the language choice");
            // O estilo vai para todo projeto: os exemplos dele não citam
            // cliente nem projeto de ninguém.
            for client in ["Suzano", "suzano"] {
                assert!(!body.contains(client), "the {text} style names a real client as an example");
            }
        }
    }

    // --- a variável dos links da barra ------------------------------------------

    /// O `env` das configurações locais depois do upsert do projeto em `root`,
    /// o caminho que o `mustard init` percorre.
    fn env_after_upsert(root: &Path) -> Map<String, Value> {
        upsert_project(root, None, InstallMode::Private).unwrap();
        local_settings(root)["env"].as_object().cloned().expect("env is an object")
    }

    /// O upsert grava `FORCE_HYPERLINK=1` no `env` das configurações locais: no
    /// projeto novo, e no já instalado, cujo `env` a mescla de cima guarda
    /// inteiro. O resto do `env` da pessoa fica como estava.
    #[test]
    fn the_upsert_writes_the_link_variable_into_the_local_env() {
        let fresh = tempdir().unwrap();
        let env = env_after_upsert(fresh.path());
        assert_eq!(env.get(FORCE_HYPERLINK_KEY), Some(&json!("1")), "a fresh install: {env:?}");

        let installed = tempdir().unwrap();
        let claude = installed.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        std_fs::write(
            claude.join("settings.local.json"),
            r#"{"env":{"MUSTARD_SKILL_SIZE_MODE":"strict","MY_OWN":"1"}}"#,
        )
        .unwrap();
        let env = env_after_upsert(installed.path());
        assert_eq!(env.get(FORCE_HYPERLINK_KEY), Some(&json!("1")), "an installed project: {env:?}");
        assert_eq!(env["MUSTARD_SKILL_SIZE_MODE"], json!("strict"), "the person's value stays");
        assert_eq!(env["MY_OWN"], json!("1"));
        assert_eq!(env.len(), 3, "only the link variable arrives: {env:?}");
    }

    /// A variável que a pessoa já tinha, como `"0"`, não é trocada por `"1"`,
    /// nem na primeira volta nem na seguinte.
    #[test]
    fn the_upsert_keeps_the_link_variable_the_person_chose() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        std_fs::write(claude.join("settings.local.json"), r#"{"env":{"FORCE_HYPERLINK":"0"}}"#).unwrap();

        for round in ["first", "second"] {
            let env = env_after_upsert(dir.path());
            assert_eq!(env.get(FORCE_HYPERLINK_KEY), Some(&json!("0")), "the {round} install: {env:?}");
        }
    }
}
