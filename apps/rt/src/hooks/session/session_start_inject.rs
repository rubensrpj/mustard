//! `session_start_inject` — the consolidated `SessionStart` lifecycle module.
//!
//! ## Scope (session family)
//!
//! This module consolidates the `SessionStart` concerns. Each is a distinct
//! *concern* kept as its own internal section — consolidation regroups, it
//! does not merge logic:
//!
//! - `harness-init.js` — bootstraps the harness event bus: ensures
//!   `.claude/.harness/` exists, prunes legacy archived sessions older than
//!   30 days, and emits a `session.start` event. Events live in per-spec /
//!   per-session NDJSON logs under `.claude/` (the `mustard.db` SQLite store
//!   was retired).
//! - terrain census — projects `grain.model.json` into a once-per-session
//!   terrain map injected as `additionalContext` (the only injection; the
//!   legacy persistent-memory block was retired — durable prose knowledge is
//!   Claude Code native auto-memory now).
//! - `spec-hygiene.js` — auto-moves stale completed/cancelled specs from
//!   `spec/{name}/` (flat layout — lifecycle status lives in each spec's
//!   `meta.json` sidecar, no bucket moves).
//! - declared injectables (orchestrator-redesign) — the `mustard.json#inject`
//!   entries with `on: sessionStart` are appended AFTER the terrain census,
//!   blank-line separated, in the same single `Inject` verdict. On a
//!   window-refreshing `SessionStart` — `source == "compact"` (auto-compaction)
//!   or `source == "clear"` (the user ran `/clear`) — the session's
//!   `injected-*` markers are cleared first (so the `once` entries of
//!   `userPromptSubmit` re-deliver on the next prompt) and the `sessionStart`
//!   entries re-inject immediately (markers ignored): the refreshed window
//!   lost them, so they must ride back in.
//! - version drift advisory — an installed project (`mustard.json` present)
//!   whose `version` stamp differs from the running harness gets a one-line
//!   nudge toward `/mustard:upsert`. Advisory, never blocking.
//! - stale plugin advisory — the running harness compared against the version
//!   the plugin registry records as INSTALLED; strictly older gets one line
//!   saying only a reload picks the new one up, since an upsert cannot. The
//!   drift advisory above cannot see this: it compares the stamp against the
//!   running harness, so a session on an old plugin reads as aligned.
//! - pending-prune advisory — delivered work units still carrying a live
//!   branch get one line naming what is owed. Advisory, never blocking.
//! - leftovers advisory — the old disposable copies `scratch-gc` would delete,
//!   when they pass the limit (5 GiB, adjustable through
//!   `MUSTARD_SCRATCH_WARN_BYTES`), become one line with the total and the
//!   command that cleans them. Below the limit, nothing. Never blocks.
//! - pending advisory — the agreed work that stays open in the ledger
//!   (`.claude/pending/ledger.json`) comes last, in a single line, with the
//!   count and the command that shows the whole list: a long list at the start
//!   of every session would stop being read. Never blocks; the hard charge
//!   lives in the `pending_gate` of `Stop`.
//!
//! ## Contract shape
//!
//! `harness-init` and `spec-hygiene` are pure side effects (`Observer`).
//! The terrain census + injectables produce an `additionalContext` payload,
//! surfaced as a [`Verdict::Inject`] so the single `emit_outcome` owns the
//! only stdout write. `SessionStartInject` is a `Check`.
//!
//! ## The one machine read is an argument
//!
//! Everything above works out of the project directory it is handed — except
//! the two plugin advisories, which need Claude Code's plugin registry, and
//! that lives in the operator's `~/.claude`, outside any project. So the
//! `Check` half reads it ONCE and hands the answer to [`session_start_core`],
//! which decides. Keeping the read inside the decision put the developer's own
//! machine into every verdict: a test building a temporary project to isolate
//! itself could not reach past it, and three went red on a box mid-upgrade
//! while staying green on the runner, where no plugin is installed at all.
//!
//! ## Profile gate
//!
//! `harness-init` / `spec-hygiene` each called
//! `shouldRun()` from `_lib/hook-env.js`. The dispatcher has no profile
//! awareness (see spec Concern "Profile gate") — under `MUSTARD_HOOK_PROFILE=minimal`
//! these now run where the JS auto-skipped. They are all fail-open side
//! effects with no verdict impact, so the change is observably inert.

use mustard_core::platform::error::Error;
use mustard_core::io::fs;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use mustard_core::I18n;
use mustard_core::SupportedLocale;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::commands::maint::scratch_gc::{human_bytes, survey, ScratchRoots};
use crate::shared::branch_state::{awaiting_prune, PrQuery};

use mustard_core::time::now_iso8601;

/// Archived sessions older than this are pruned on `SessionStart` (30 days).
const RETENTION_MS: u128 = 30 * 24 * 60 * 60 * 1000;

/// The consolidated `SessionStart` module.
pub struct SessionStartInject;

// ===========================================================================
// harness-init — SessionStart event-bus bootstrap
// ===========================================================================

/// The `.claude/.harness` directory for a project.
fn harness_dir(cwd: &str) -> PathBuf {
    ClaudePaths::for_project(cwd)
        .map(|p| p.harness_dir())
        .unwrap_or_default()
}

/// The `.claude/.harness/sessions` directory for a project.
fn sessions_dir(cwd: &str) -> PathBuf {
    harness_dir(cwd).join("sessions")
}

/// The current session id for an invocation. Mirrors `getCurrentSessionId`:
/// the `session_id` field, else `"unknown"` (the consolidated dispatcher has
/// no env-var fallback — telemetry, not load-bearing).
fn current_session_id(input: &HookInput) -> String {
    input
        .session_id
        .clone()
        .or_else(|| {
            input
                .raw
                .get("sessionId")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// `harness-init`: ensure the harness dirs exist, prune legacy archived
/// sessions, and emit a `session.start` event. The harness event bus is a
/// single WAL-mode `SQLite` store, so there is no per-session NDJSON log to
/// rotate. Pure side effect — fail-open throughout.
fn run_harness_init(input: &HookInput, cwd: &str) {
    let harness = harness_dir(cwd);
    let sessions = sessions_dir(cwd);
    let _ = fs::create_dir_all(&harness);
    let _ = fs::create_dir_all(&sessions);

    let current_id = current_session_id(input);
    // Clean up legacy NDJSON session archives; WAL needs no file rotation.
    prune_old_sessions(&sessions);

    // Emit `session.start`.
    let source = input
        .raw
        .get("source")
        .or_else(|| input.raw.get("matcher"))
        .cloned()
        .unwrap_or(Value::Null);
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: current_id,
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("harness-init".to_string()),
            actor_type: None,
        },
        event: "session.start".to_string(),
        payload: json!({ "cwd": cwd, "source": source }),
        spec: None,
    };
    // `session.start` is non-pipeline → per-spec NDJSON (or session fallback
    // when there is no active spec yet) via the event router.
    let _ = crate::shared::events::route::emit(cwd, &event);
}

/// Delete archived `sessions/*.jsonl` files older than the retention window.
fn prune_old_sessions(sessions_dir: &Path) {
    let Ok(entries) = fs::read_dir(sessions_dir) else {
        return;
    };
    let now = mustard_core::time::now_unix_millis() as u128;
    for entry in entries {
        if !std::path::Path::new(&entry.file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl")) {
            continue;
        }
        let Ok(modified) = fs::modified(&entry.path) else {
            continue;
        };
        let mtime_ms = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        if now.saturating_sub(mtime_ms) > RETENTION_MS {
            let _ = fs::remove_file(&entry.path);
        }
    }
}

// ===========================================================================
// spec-hygiene — flat layout; no-op
// ===========================================================================

/// `spec-hygiene`: flat layout — spec status lives in the `SQLite` event store;
/// no bucket directories to move specs between (wave-2 removed them).
/// Retained as a no-op so call sites remain stable while a future wave may
/// add SQLite-driven hygiene (e.g. pruning stale orphan pipeline-state files).
/// Pure side effect — fail-open throughout. Port of `runHygiene`.
fn run_spec_hygiene(_cwd: &str) {
    // No-op under the flat layout: specs no longer move between bucket
    // directories.
}

// ===========================================================================
// Contract impls
// ===========================================================================

impl Check for SessionStartInject {
    /// On `SessionStart`: bootstrap the event bus, run spec hygiene, and inject
    /// the terrain census. The first two are side effects; the terrain payload
    /// is the verdict — `Inject` when a grain model exists, else `Allow`.
    ///
    /// Any non-`SessionStart` trigger self-allows.
    ///
    /// This half exists to READ the machine and nothing else: Claude Code's
    /// plugin registry is asked ONCE here and the answer is handed to
    /// [`session_start_core`], which decides. Both plugin advisories used to
    /// take that read themselves, deep inside the decision, and a test that
    /// built a temporary project to isolate everything could not reach past it
    /// — the read escaped the temporary directory onto the developer's own
    /// `~/.claude`, so three tests went red on any machine mid-upgrade and
    /// stayed green on the runner, where no plugin is installed at all. The
    /// argument IS the seam.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // O diretório temporário é a segunda leitura da máquina e segue o
        // mesmo caminho: montada aqui, entregue como argumento.
        let scratch = ScratchProbe::from_env(input);
        session_start_core(
            input,
            ctx,
            mustard_core::installed_harness_version().as_deref(),
            Some(&scratch),
        )
    }
}

/// The deciding half of [`SessionStartInject`]'s check, with the one machine
/// read injected: `installed` is the version Claude Code's plugin registry
/// records, or `None` when the registry could not answer.
///
/// `None` is also what every test passes, and it is the honest shape of "no
/// registry" — the case both plugin advisories already read as nothing to say.
/// A test asserting silence then gets that silence from the code, not from
/// whichever machine happens to run it.
///
/// `scratch` é a outra leitura da máquina, pelo mesmo motivo: onde varrer as
/// cópias descartáveis e a partir de quanto avisar. `None` — o que todo teste
/// que não fala de sobras passa — cala o aviso por construção, e as sobras
/// reais do desenvolvedor nunca entram num teste de silêncio.
fn session_start_core(
    input: &HookInput,
    ctx: &Ctx,
    installed: Option<&str>,
    scratch: Option<&ScratchProbe>,
) -> Result<Verdict, Error> {
    if ctx.trigger != Some(Trigger::SessionStart) {
        return Ok(Verdict::Allow);
    }
    let cwd = ctx.project_dir_or_cwd(input);
    run_harness_init(input, &cwd);
    run_spec_hygiene(&cwd);
    // Advisory probe for drift in the project's `.claude/` directory.
    // Read-only; emits a single stderr warning when one or more children
    // classify as `ORPHAN` (no declared consumer in
    // `apps/{rt,cli,dashboard}`) — the underlying audit now derives its
    // documented-directory set from `mustard_core::ClaudePaths::documented_dirs`,
    // the single canonical catalog. Fail-open — never blocks.
    crate::commands::maint::claude_dir_prune::check_orphans(Path::new(&cwd));
    // Terrain: project `grain.model.json` into a
    // once-per-session terrain map so the AI opens the session already
    // knowing the subprojects instead of grepping to orient. Fail-open: a
    // missing / unreadable model yields no terrain.
    let i18n = I18n::new(crate::shared::context::project_config_cached(Path::new(&cwd)).language().text_or_default());
    let terrain_lang = i18n.lang;
    let terrain = crate::commands::orient::render_terrain(
        &crate::commands::orient::compute_orientation(Path::new(&cwd)),
        terrain_lang,
    );
    // Declared injectables (`mustard.json#inject`, `on: sessionStart`).
    // A window-refreshing SessionStart first clears the session's
    // `injected-*` markers, then re-injects the sessionStart entries
    // immediately (markers ignored). Two sources refresh the window:
    // `compact` (auto-compaction) and `clear` (the user ran `/clear`) —
    // both drop every earlier injection, so the `once` userPromptSubmit
    // entries must re-deliver on the next prompt and the sessionStart
    // entries must ride back in. Fail-open throughout.
    let session = current_session_id(input);
    let source_refreshes_window = input
        .raw
        .get("source")
        .and_then(|v| v.as_str())
        .is_some_and(|s| {
            s.eq_ignore_ascii_case("compact") || s.eq_ignore_ascii_case("clear")
        });
    if source_refreshes_window {
        crate::hooks::session::injectables::clear_markers(&cwd, Some(session.as_str()));
    }
    let injected = crate::hooks::session::injectables::collect(
        &cwd,
        Some(session.as_str()),
        "sessionstart",
        source_refreshes_window,
        None,
    );
    // The `userPromptSubmit` family is NOT folded in here, and that is
    // deliberate. Doing so was tried and MEASURED at 11,973 characters on
    // this repository — past the 10,000 a hook RESPONSE carries, so the very
    // router it meant to rescue became a file path instead of text in
    // force. Worse, `collect` records the delivery markers, so each sibling
    // hook would then skip on the next prompt: the self-healing path would
    // be disarmed by the attempt to help it.
    //
    // Clearing the markers above IS the fix. The prompt family re-delivers
    // on the operator's next prompt, through its own sibling hooks, each
    // measured alone against its own ceiling. The window between the
    // compaction and that prompt carries no router — accepted, because the
    // alternative measured worse: no router at all, for the rest of the
    // session.
    // Version drift advisory: an installed project whose `mustard.json`
    // stamp differs from the running harness gets a one-paragraph nudge
    // toward `/mustard:upsert`. Advisory only — the user decides.
    let drift = version_drift_notice(Path::new(&cwd));
    // Stale-plugin advisory: the drift check above compares the stamp with
    // the RUNNING harness, and the running harness is what wrote the stamp
    // — so it is blind to a session still carrying a plugin an update has
    // already replaced on disk. This is the only line that can see it.
    let running = mustard_core::harness_version();
    let stale = stale_plugin_line(&running, installed);
    // Plugin-behind-binary advisory: the two above compare the stamp with
    // the running harness, and the running plugin with the registry. A
    // package install moves neither pair — it replaces the SYSTEM binary
    // and leaves the plugin where it was, which is the shape the operator
    // hit on 2026-08-26.
    let behind = plugin_behind_binary_line(&running, installed);
    // Pending-prune advisory: delivered work units whose branch is still
    // alive. The prune command already existed and worked; what was missing
    // was anyone SAYING it was owed, so six units piled up unnoticed.
    let prune = prune_pending_notice(Path::new(&cwd), terrain_lang);
    // Aviso de sobras: as cópias descartáveis antigas que passam do limite, no
    // idioma e no tom do projeto. Vem antes das pendências, que fecham o bloco.
    let residue = scratch_notice(Path::new(&cwd), scratch, i18n);
    // Aviso de pendências: o que foi combinado e segue aberto. Um combinado que
    // só existe na memória da conversa se perde quando ela acaba; relido aqui,
    // ele atravessa a sessão.
    let pending = pending_notice(Path::new(&cwd), terrain_lang);
    // ONE composed Inject. This is the only `Check` that injects on
    // `SessionStart`, so the order below is the order the window reads (the
    // dispatcher fold joins Injects in registry order and has nothing to join
    // here): terrain first, injectables after, the advisories last —
    // blank-line separated. All of it is ONE response under the 10,000
    // character ceiling, which is why the prompt family stays out (above).
    let parts: Vec<String> = [terrain, injected, drift, stale, behind, prune, residue, pending]
        .into_iter()
        .flatten()
        .collect();
    Ok(if parts.is_empty() {
        Verdict::Allow
    } else {
        Verdict::Inject { context: parts.join("\n\n") }
    })
}

/// One-paragraph advisory when the project's `mustard.json#version` stamp
/// differs from the running harness ([`mustard_core::harness_version`] — the
/// installed plugin's manifest, or the core line outside the plugin).
///
/// `None` when the project has no `mustard.json` (not installed — the
/// prompt-gate story covers that) or when the stamp matches. A missing
/// `version` key on an installed project counts as drift: it predates the
/// stamp and the first `/mustard:upsert` writes one.
fn version_drift_notice(root: &Path) -> Option<String> {
    if !mustard_core::ProjectConfig::exists(root) {
        return None;
    }
    let stamped = mustard_core::ProjectConfig::load(root).version;
    let current = mustard_core::harness_version();
    if stamped.as_deref() == Some(current.as_str()) {
        return None;
    }
    let label = stamped.unwrap_or_else(|| "unstamped (pre-version era)".to_string());
    Some(format!(
        "[Mustard] Harness version drift — project stamp: {label}; running harness: \
         {current}. Tell the user this project's Mustard footprint is out of date and \
         suggest running /mustard:upsert to realign (a notice that persists after an \
         upsert means the plugin itself needs updating)."
    ))
}

/// One line when the plugin THIS session loaded is behind the one Claude Code's
/// registry records as installed — the session is running old prose and only a
/// reload changes that. Running version in, registry answer in, advisory out.
///
/// The gap this closes: `/mustard:upsert` installs a new plugin version, and
/// the running session keeps every command, skill and agent file of the old one
/// until the operator reloads. Nothing said so. [`version_drift_notice`]
/// structurally cannot: the stamp it reads was written by the running harness,
/// so the two agree by construction and a stale session reads as aligned.
///
/// `None` unless the registry ANSWERED and the running version is strictly
/// older: an unreadable registry, a registry that does not list this plugin,
/// and a session already on the installed version all mean the same thing —
/// nothing to say. An advisory that cannot prove its claim stays quiet.
fn stale_plugin_line(running: &str, installed: Option<&str>) -> Option<String> {
    let installed = installed.filter(|latest| mustard_core::is_behind(running, latest))?;
    Some(format!(
        "[Mustard] Stale plugin — this session loaded {running}; {installed} is installed. \
         Tell the user the session is running the OLD commands, skills and agents, and that \
         only reloading Claude Code picks up {installed} — an upsert alone does not."
    ))
}

/// One line when the plugin Claude Code would load is behind the harness that
/// is RUNNING. The running harness's version in, the registry's answer in, the
/// advisory out.
///
/// The third DIRECTION on the same pair, and neither of the two above reports
/// it.
/// [`version_drift_notice`] compares the project stamp with the running
/// harness; [`stale_plugin_line`] compares the running plugin with the
/// registry. Both are blind to the case where the SYSTEM binary was updated and
/// the plugin was not — which is exactly what a package install does: `dpkg -i`
/// (or the Windows installer) replaces `/usr/lib/mustard/bin/mustard-rt` and
/// touches nothing under `~/.claude/plugins/`.
///
/// Found in the field, 2026-08-26: the operator installed 0.1.50 and the plugin
/// stayed on 0.1.49. Every version they could see said 0.1.50, and the harness
/// that actually ran their hooks was the old one. Nothing said so.
///
/// `None` whenever the claim cannot be proven — no registry, no answer, or the
/// two agree. An advisory that guesses is worse than silence.
///
/// The direction is the opposite of [`stale_plugin_line`], and that is the
/// whole point. There, the session is BEHIND what the registry records, and a
/// reload fixes it. Here, the registry is behind the running harness, and a
/// reload changes nothing — only `/mustard:upsert` refreshes the plugin.
///
/// The two read the same pair through the antisymmetric `is_behind`, so exactly
/// one can ever be true; they cannot contradict each other. What this one must
/// NOT claim is which binary the hooks are executing — the emitting process IS
/// the newer one, and an earlier wording said the opposite (found in review).
fn plugin_behind_binary_line(binary: &str, plugin: Option<&str>) -> Option<String> {
    let plugin = plugin.filter(|p| mustard_core::is_behind(p, binary))?;
    Some(format!(
        "[Mustard] Plugin behind the running harness — this harness is {binary} and the \
         Claude Code plugin registry records {plugin}. A package install replaces the system \
         binary and does NOT touch the plugin, so the plugin's commands, skills and agents \
         are still the {plugin} ones. Tell the user to run `/mustard:upsert`, which refreshes \
         the plugin, and then restart Claude Code."
    ))
}

/// How many unit names the advisory spells out before it just counts the rest.
/// Six units piled up in the field report; a list that long stops being read.
const PRUNE_NOTICE_NAMES: usize = 4;

/// One line when delivered work units still carry a live branch — the missing
/// half of the exit ritual.
///
/// The command that prunes them already existed and worked; across six
/// consecutive units nobody ran it, because nothing ever said it was owed.
/// This is that saying, and nothing more: advisory, never blocking.
///
/// The count comes from the ONE classifier
/// ([`crate::shared::branch_state::awaiting_prune`]) with the lookup that asks
/// no provider — a session must not open a network connection per branch
/// before it starts, so only merges LOCAL ancestry proves are counted. Under-
/// reporting costs a nudge; over-reporting would point at branches nobody
/// verified. `shared` may not import the git primitive (it lives in the
/// `commands` face), so the read is injected here, exactly as `branch_state`
/// documents.
///
/// `None` for a project with no `mustard.json` (never installed — the harness
/// does not nag it) and whenever nothing is owed. Fail-open throughout: a git
/// that cannot answer yields no advisory.
///
/// **Session start only, since the `stop_gate` left.** It also called this at
/// the end of every turn, because the debt is BORN mid-session, at the merge;
/// that end-of-turn copy left with it, and mid-session
/// the statusline still shows the same count live to the human.
pub(crate) fn prune_pending_notice(root: &Path, lang: SupportedLocale) -> Option<String> {
    if !mustard_core::ProjectConfig::exists(root) {
        return None;
    }
    let config = crate::shared::context::project_config_cached(root);
    // ROOTED: the sweep classifies REAL branches of THIS repository, and a unit
    // whose base only its own directory recorded (an emergency in a project
    // declaring several candidates) reads as base-less through the pure
    // derivation — `BranchEnumerator` then files it under an empty base, which
    // is a base group `refs_ahead_of_base` never measures.
    let flow = crate::shared::work_kind::BaseFlow::of_at(&config.git, root);
    let pending = awaiting_prune(root, PrQuery::Skip, &flow);
    if pending.is_empty() {
        return None;
    }
    let named: Vec<&str> =
        pending.iter().take(PRUNE_NOTICE_NAMES).map(|state| state.branch.as_str()).collect();
    let rest = pending.len() - named.len();
    let branches = if rest > 0 {
        format!("{listed} (+{rest})", listed = named.join(", "))
    } else {
        named.join(", ")
    };
    Some(
        mustard_core::translate("prune.pending.notice", lang)
            .replace("{count}", &pending.len().to_string())
            .replace("{branches}", &branches),
    )
}

/// A single line with the count of open pending items and the command that
/// shows the whole list ([`count_line`](crate::commands::event::pending::count_line),
/// the same line as the `run pending` listing).
///
/// `None` for a project without `mustard.json` (never installed — the harness
/// does not bother it) and when nothing is open. An unreadable ledger stays
/// silent too: `run pending` is the one that refuses and explains the fix.
pub(crate) fn pending_notice(root: &Path, lang: SupportedLocale) -> Option<String> {
    if !mustard_core::ProjectConfig::exists(root) {
        return None;
    }
    crate::commands::event::pending::count_line(root, lang)
}

/// Variável que ajusta, em bytes, a partir de quanto o aviso de sobras aparece
/// — existe para teste e para máquina com disco apertado.
const SCRATCH_WARN_ENV: &str = "MUSTARD_SCRATCH_WARN_BYTES";

/// Limite padrão do aviso de sobras: 5 GiB. Abaixo disso as sobras não pesam
/// no disco, e um aviso que aparece sem haver problema vira ruído.
const DEFAULT_SCRATCH_WARN_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// O que o aviso de sobras precisa da máquina: onde varrer e a partir de
/// quanto avisar. Montado no [`Check`], entregue ao [`session_start_core`] —
/// a mesma costura do registro de plugins.
struct ScratchProbe {
    roots: ScratchRoots,
    warn_bytes: u64,
}

impl ScratchProbe {
    /// As raízes reais desta máquina e o limite de `MUSTARD_SCRATCH_WARN_BYTES`
    /// quando é um número.
    ///
    /// A sessão que está abrindo é a sessão atual (filtro 4 da varredura): o
    /// `scratchpad/` dela nunca conta como sobra. A compilação compartilhada
    /// fica fora: o aviso soma só as cópias, e medi-la seria mais uma passada
    /// pela árvore inteira no início de toda sessão.
    fn from_env(input: &HookInput) -> Self {
        let mut roots = ScratchRoots::from_env();
        let session = current_session_id(input);
        if session != "unknown" {
            roots.current_session = session;
        }
        roots.shared_target = None;
        let warn_bytes = std::env::var(SCRATCH_WARN_ENV)
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_SCRATCH_WARN_BYTES);
        Self { roots, warn_bytes }
    }
}

/// Uma linha quando as cópias descartáveis antigas passam do limite: o total,
/// quantas pastas e o comando que lista e apaga.
///
/// No molde do [`prune_pending_notice`] e do [`pending_notice`]: projeto sem
/// `mustard.json` não é importunado, e abaixo do limite nada aparece. A conta
/// sai da MESMA varredura do `scratch-gc` e do `doctor --residue`
/// ([`survey`]), então o total do aviso é exatamente o que o
/// `scratch-gc --apply` apagaria.
///
/// `None` também quando a leitura da máquina não foi entregue. O texto sai do
/// catálogo (`scratch.residue.notice`), no idioma e no tom do projeto. Nunca
/// bloqueia.
fn scratch_notice(root: &Path, scratch: Option<&ScratchProbe>, i18n: I18n) -> Option<String> {
    let probe = scratch?;
    if !mustard_core::ProjectConfig::exists(root) {
        return None;
    }
    let found = survey(&probe.roots);
    let total = found.candidates_bytes();
    if total <= probe.warn_bytes {
        return None;
    }
    Some(
        i18n.render("scratch.residue.notice")
            .replace("{total}", &human_bytes(total))
            .replace("{count}", &found.candidates.len().to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    // `session.start` lands in the per-session NDJSON sink.
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx::for_test(dir.to_string(), Some(Trigger::SessionStart))
    }

    fn session_input(session_id: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some(session_id.to_string()),
            ..HookInput::default()
        }
    }


    /// A renewed window RE-ARMS the prompt family; it does not fold it
    /// into this response.
    ///
    /// An earlier revision did fold it in, and the response measured 11,973
    /// characters on this repository — over the 10,000 a hook response carries,
    /// so the router became a file path instead of text in force. `collect`
    /// also records the delivery markers, so each sibling hook would then skip
    /// on the next prompt: the self-healing path disarmed by the attempt to
    /// help it.
    ///
    /// Clearing the markers IS the fix, and this measures exactly that: after a
    /// compaction the markers are gone, so the next prompt re-delivers through
    /// the siblings, each against its own ceiling.
    #[test]
    fn compact_rearms_the_prompt_family_without_folding_it_in() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"inject":[{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true}]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/mustard")).unwrap();
        std::fs::write(dir.path().join(".claude/mustard/orchestrator.md"), "ROUTER-RULES").unwrap();

        // Burn the marker, as a first delivery would.
        let session = dir.path().join(".claude/.session/s2");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("injected-orchestrator.md"), "x").unwrap();

        let mut compacted = session_input("s2");
        compacted.raw = serde_json::json!({"source": "compact"});
        let verdict = SessionStartInject.evaluate(&compacted, &ctx(project)).expect("no error");

        // The router is NOT in this response — folding it here is what
        // overflowed it.
        let text = match verdict {
            Verdict::Inject { ref context } => context.clone(),
            _ => String::new(),
        };
        assert!(
            !text.contains("ROUTER-RULES"),
            "the prompt family must not ride the SessionStart response: {text}",
        );

        // The marker is gone, so the next prompt re-delivers through the
        // sibling hook that owns it.
        assert!(
            !session.join("injected-orchestrator.md").exists(),
            "a renewed window must clear the delivery markers",
        );
    }

    // --- routing -----------------------------------------------------------

    #[test]
    fn non_session_start_trigger_allows() {
        let input = session_input("s1");
        let other = Ctx::for_test(".".to_string(), Some(Trigger::PreToolUse));
        assert_eq!(
            SessionStartInject.evaluate(&input, &other).expect("no error"),
            Verdict::Allow
        );
    }

    // --- version drift advisory --------------------------------------------

    #[test]
    fn drift_notice_absent_without_mustard_json() {
        let dir = tempdir().unwrap();
        assert_eq!(version_drift_notice(dir.path()), None);
    }

    #[test]
    fn drift_notice_absent_when_stamp_matches() {
        let dir = tempdir().unwrap();
        let current = mustard_core::harness_version();
        std::fs::write(
            dir.path().join("mustard.json"),
            format!(r#"{{"version":"{current}"}}"#),
        )
        .unwrap();
        assert_eq!(version_drift_notice(dir.path()), None);
    }

    #[test]
    fn drift_notice_fires_on_mismatch_and_names_upsert() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), r#"{"version":"0.0.0-test"}"#)
            .unwrap();
        let notice = version_drift_notice(dir.path()).expect("drift must fire");
        assert!(notice.contains("0.0.0-test"), "names the stamped version: {notice}");
        assert!(notice.contains("/mustard:upsert"), "points at the realign door: {notice}");
    }

    #[test]
    fn drift_notice_fires_on_missing_stamp() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), r#"{"buildCommand":"make"}"#).unwrap();
        let notice = version_drift_notice(dir.path()).expect("unstamped must fire");
        assert!(notice.contains("unstamped"), "labels the pre-version era: {notice}");
    }

    // --- stale-plugin advisory ----------------------------------------------

    /// A session whose loaded plugin is behind the one the registry
    /// records as installed says so in ONE line, and says that reloading is
    /// what fixes it. The drift advisory cannot reach this case: the stamp it
    /// compares was written BY the running harness.
    #[test]
    fn stale_plugin_is_announced_at_session_start() {
        let notice = stale_plugin_line("0.1.42", Some("0.1.43")).expect("stale must fire");
        assert_eq!(notice.lines().count(), 1, "one line, not a paragraph: {notice}");
        assert!(notice.contains("0.1.42"), "names what the session loaded: {notice}");
        assert!(notice.contains("0.1.43"), "names what is installed: {notice}");
        assert!(
            notice.to_lowercase().contains("reload"),
            "says the reload is the missing step: {notice}"
        );
    }

    /// The three silences, which are the same silence: a session already on the
    /// installed version, one AHEAD of it (a local build), and a registry that
    /// could not answer at all.
    #[test]
    fn stale_plugin_notice_stays_quiet_without_proof() {
        assert_eq!(stale_plugin_line("0.1.43", Some("0.1.43")), None);
        assert_eq!(stale_plugin_line("0.2.0", Some("0.1.43")), None);
        assert_eq!(stale_plugin_line("0.1.42", None), None);
    }

    // --- pending-prune advisory ---------------------------------------------

    /// Run git in `dir`, failing the test loudly — a half-built fixture would
    /// make the assertions below prove nothing.
    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must be on PATH for this test");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn prune_advisory_absent_without_mustard_json() {
        let dir = tempdir().unwrap();
        assert_eq!(prune_pending_notice(dir.path(), SupportedLocale::default()), None);
    }

    /// The field cause, closed: a unit whose work landed but whose branch is
    /// still around gets SAID OUT LOUD at session start. The unmerged unit in
    /// the same repo is the control — the advisory names what is owed, never
    /// everything that exists.
    #[test]
    fn prune_advisory_names_units_whose_branch_outlived_the_merge() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        git(root, &["add", "-A", "-f", "."]);
        git(root, &["commit", "-m", "seed"]);

        // One unit delivered: merged into its base, branch still alive.
        git(root, &["checkout", "-b", "dev_landed"]);
        git(root, &["commit", "--allow-empty", "-m", "work"]);
        git(root, &["checkout", "dev"]);
        git(root, &["merge", "--no-ff", "-m", "merge", "dev_landed"]);
        // One unit still in flight: nothing is owed for it.
        git(root, &["branch", "dev_live"]);
        git(root, &["checkout", "dev_live"]);
        git(root, &["commit", "--allow-empty", "-m", "in flight"]);
        git(root, &["checkout", "dev"]);

        let notice = prune_pending_notice(root, SupportedLocale::default())
            .expect("a delivered unit with a live branch must be surfaced");
        assert!(notice.contains("dev_landed"), "names the unit owed a prune: {notice}");
        assert!(
            !notice.contains("dev_live"),
            "an unmerged unit is not owed anything: {notice}"
        );
        assert!(notice.contains('1'), "carries the count: {notice}");
        assert!(
            notice.contains("git-settle"),
            "points at the command that was never called: {notice}"
        );
    }

    // --- aviso de pendências -----------------------------------------------

    /// Grava uma pendência aberta no ledger de `root`, pelo mesmo passe de
    /// `run pending`.
    fn add_pending(root: &Path, title: &str) {
        let out = crate::commands::event::pending::pending_at(
            &crate::commands::event::pending::PendingOpts {
                root: root.to_path_buf(),
                add: true,
                title: Some(title.to_string()),
                detail: Some("combinado na conversa".to_string()),
                ..crate::commands::event::pending::PendingOpts::default()
            },
        );
        assert_eq!(out["ok"], json!(true), "seed: {out}");
    }

    /// A session that opens with open pending items gets a single line, with
    /// the count and the command that shows the list; no title goes in.
    #[test]
    fn the_session_start_shows_one_line_with_the_open_count() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("mustard.json"),
            format!(r#"{{"version":"{}"}}"#, mustard_core::harness_version()),
        )
        .unwrap();
        add_pending(root, "HTML padrao da spec");
        add_pending(root, "Humanize");

        let verdict =
            session_start_core(&session_input("s-pend"), &ctx(root.to_str().unwrap()), NO_REGISTRY, NO_SCRATCH)
                .unwrap();
        let Verdict::Inject { context } = verdict else {
            panic!("open pending items must reach the session: {verdict:?}");
        };
        let line = mustard_core::translate("pending.count.many", SupportedLocale::default())
            .replace("{count}", "2");
        assert!(context.contains(&line), "the count line: {context}");
        assert!(!context.contains("Humanize") && !context.contains("P-1"), "no item is listed: {context}");
    }

    /// One pending item gives the line in the singular and eight give the
    /// count, with no title; with no install or nothing open, it stays silent.
    #[test]
    fn the_pending_notice_counts_and_stays_quiet_when_empty() {
        let lang = SupportedLocale::default();
        let bare = tempdir().unwrap();
        assert_eq!(pending_notice(bare.path(), lang), None, "not installed");

        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        assert_eq!(pending_notice(root, lang), None, "nothing open");

        add_pending(root, "trabalho 1");
        assert_eq!(pending_notice(root, lang).as_deref(), Some(mustard_core::translate("pending.count.one", lang)));
        for n in 2..=8 {
            add_pending(root, &format!("trabalho {n}"));
        }
        let notice = pending_notice(root, lang).expect("eight open items");
        assert!(notice.starts_with("[Mustard] 8 "), "carries the count: {notice}");
        assert!(!notice.contains("trabalho") && !notice.contains('\n'), "one line, no item: {notice}");
    }

    // --- harness-init parity -----------------------------------------------

    #[test]
    fn harness_init_creates_dirs_and_emits_session_start() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = session_input("s-new");
        SessionStartInject.evaluate(&input, &ctx(project)).unwrap();
        assert!(dir.path().join(".claude/.harness/sessions").is_dir());

        // `session.start` is non-pipeline → lands in the per-session NDJSON
        // sink under `<project>/.claude/.session/<slug>/.events/`.
        let session_root = dir.path().join(".claude").join(".session");
        let mut found = false;
        if session_root.exists() {
            for entry in std::fs::read_dir(&session_root).unwrap() {
                let events_dir = entry.unwrap().path().join(".events");
                if !events_dir.exists() {
                    continue;
                }
                for f in std::fs::read_dir(&events_dir).unwrap() {
                    let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
                    if body.lines().any(|l| {
                        serde_json::from_str::<serde_json::Value>(l)
                            .ok()
                            .and_then(|v| v["event"].as_str().map(str::to_string))
                            .as_deref()
                            == Some("session.start")
                    }) {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "session.start NDJSON line must be present");
    }

    #[test]
    fn harness_init_creates_harness_dir_no_jsonl() {
        // `session.start` is non-pipeline → it lands in the per-session
        // NDJSON sink, NOT in `mustard.db`. The harness directory still gets
        // created so later pipeline.* events can land there.
        // No event-store seeding required.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        SessionStartInject
            .evaluate(&session_input("new-session"), &ctx(project))
            .unwrap();
        assert!(dir.path().join(".claude/.harness").is_dir());
        assert!(!dir.path().join(".claude/.harness/events.jsonl").exists());
    }

    // --- spec-hygiene parity -----------------------------------------------

    /// Write a spec with the given `spec.md` body (flat layout — no active/ bucket).
    fn write_active_spec(dir: &Path, name: &str, body: &str) {
        let spec_dir = dir.join(".claude/spec").join(name);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), body).unwrap();
    }

    #[test]
    fn hygiene_noop_completed_spec_stays_flat() {
        // Flat layout: no bucket moves — spec stays in spec/{name}/ regardless of status.
        let dir = tempdir().unwrap();
        write_active_spec(
            dir.path(),
            "done-spec",
            "# Spec\n### Status: completed | Phase: CLOSE\n\n## Checklist\n- [x] One\n- [x] Two\n",
        );
        SessionStartInject
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(dir.path().join(".claude/spec/done-spec").exists());
    }

    #[test]
    fn hygiene_noop_implementing_spec_stays_flat() {
        let dir = tempdir().unwrap();
        write_active_spec(
            dir.path(),
            "wip-spec",
            "# Spec\n### Status: implementing\n\n## Checklist\n- [x] One\n- [ ] Two\n",
        );
        SessionStartInject
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(dir.path().join(".claude/spec/wip-spec").exists());
    }

    #[test]
    fn hygiene_noop_blocked_spec_stays_flat() {
        let dir = tempdir().unwrap();
        write_active_spec(
            dir.path(),
            "blocked-spec",
            "# Spec\n### Status: completed\n\n## Concerns\n- BLOCKED on infra\n\n## Checklist\n- [x] One\n",
        );
        SessionStartInject
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(dir.path().join(".claude/spec/blocked-spec").exists());
    }

    // --- terrain injection ---------------------------------------------------

    #[test]
    fn no_grain_model_returns_allow() {
        // No `grain.model.json` and no declared injectables → nothing to
        // inject → the verdict degrades to Allow.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let verdict =
            session_start_core(&session_input("s"), &ctx(dir.path().to_str().unwrap()), NO_REGISTRY, NO_SCRATCH)
                .unwrap();
        assert!(
            matches!(verdict, Verdict::Allow),
            "nothing to inject must stay quiet: {verdict:?}"
        );
    }

    /// What the plugin registry answers when a test asks it: nothing.
    ///
    /// A test that asks this module for a verdict builds a temporary project to
    /// isolate what it measures, and the two plugin advisories were the one
    /// thing that escaped that temporary directory — they read the REAL
    /// `~/.claude` of the machine running the suite. On a box mid-upgrade the
    /// advisory fired and three tests failed for a reason none of them is
    /// about; on the CI runner, with no plugin installed at all, they passed
    /// without proving anything.
    ///
    /// [`session_start_core`] takes that answer as an argument, so a test hands
    /// it the honest "the registry did not answer" and both advisories are
    /// silent by construction, on every machine.
    const NO_REGISTRY: Option<&str> = None;

    /// A leitura do diretório temporário que um teste entrega: nenhuma. O aviso
    /// de sobras cala por construção, e as sobras reais da máquina que roda a
    /// suíte nunca entram num teste que não fala delas.
    const NO_SCRATCH: Option<&ScratchProbe> = None;

    /// Com as sobras acima do limite, o início da sessão mostra o total
    /// e o comando que limpa; abaixo do limite, nada aparece.
    #[test]
    fn session_start_warns_when_scratch_is_large() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("mustard.json"),
            format!(r#"{{"version":"{}"}}"#, mustard_core::harness_version()),
        )
        .unwrap();

        // Um diretório temporário falso com uma cópia descartável de um dia
        // atrás — a árvore inteira envelhecida pelo mtime, o relógio das
        // fixtures (o ctime não recua).
        let temp_root = dir.path().join("tmp");
        let old = temp_root.join("tmp.old");
        std::fs::create_dir_all(old.join("apps").join("rt")).unwrap();
        std::fs::write(old.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(old.join("apps").join("rt").join("big.bin"), vec![0u8; 4096]).unwrap();
        crate::commands::maint::scratch_gc::backdate_tree(&old, 24);
        let total = std::fs::metadata(old.join("Cargo.toml")).unwrap().len() + 4096;

        let probe = |warn_bytes: u64| ScratchProbe {
            roots: ScratchRoots {
                temp_root: temp_root.clone(),
                shared_target: None,
                cap_bytes: u64::MAX,
                current_session: "s-scratch".to_string(),
                current_dir: None,
                home: None,
                clock: crate::commands::maint::scratch_gc::AgeClock::Modified,
                owner_uid: crate::commands::maint::scratch_gc::current_uid(),
                now: std::time::SystemTime::now(),
            },
            warn_bytes,
        };
        let context_of = |probe: &ScratchProbe| {
            match session_start_core(
                &session_input("s-scratch"),
                &ctx(root.to_str().unwrap()),
                NO_REGISTRY,
                Some(probe),
            )
            .unwrap()
            {
                Verdict::Inject { context } => context,
                _ => String::new(),
            }
        };

        let above = context_of(&probe(1024));
        assert!(above.contains(&human_bytes(total)), "carries the total: {above}");
        assert!(above.contains("mustard-rt run scratch-gc"), "names the cleanup command: {above}");

        // No limite exato ainda não passou dele: silêncio.
        let below = context_of(&probe(total));
        assert!(!below.contains("scratch-gc"), "below the limit nothing shows: {below}");
        assert!(old.exists(), "the notice only reads");
    }

    /// A package install moves the system binary and leaves the plugin behind,
    /// and until this advisory nothing said so.
    ///
    /// Measured in the field on 2026-08-26: `dpkg -i` put 0.1.50 on the machine
    /// and `~/.claude/plugins/` stayed on 0.1.49. Every version the operator
    /// could read said 0.1.50; the harness running their hooks was 0.1.49.
    ///
    /// The direction matters and is the opposite of the stale-plugin line: this
    /// one fires when the PLUGIN is behind, and a reload does not fix it —
    /// only `/mustard:upsert` refreshes the plugin.
    #[test]
    fn a_plugin_left_behind_by_a_package_install_is_named() {
        let notice = plugin_behind_binary_line("0.1.50", Some("0.1.49"))
            .expect("a plugin behind the binary must be named");
        assert!(notice.contains("0.1.50") && notice.contains("0.1.49"), "{notice}");
        assert!(notice.contains("upsert"), "it must name the one action that fixes it: {notice}");

        // Silent whenever the claim cannot be proven, or there is nothing to
        // claim. An advisory that guesses is worse than one that stays quiet.
        assert_eq!(plugin_behind_binary_line("0.1.50", Some("0.1.50")), None, "aligned");
        assert_eq!(plugin_behind_binary_line("0.1.49", Some("0.1.50")), None, "plugin AHEAD");
        assert_eq!(plugin_behind_binary_line("0.1.50", None), None, "no registry answer");
        // Numeric, not lexical: 0.1.9 is behind 0.1.10, which a string
        // comparison gets backwards.
        assert!(plugin_behind_binary_line("0.1.10", Some("0.1.9")).is_some(), "0.1.9 < 0.1.10");
    }

    // --- declared injectables (orchestrator-redesign) ------------------------

    /// Declare one `on: sessionStart, once: true` injectable + its file.
    fn seed_session_injectable(dir: &Path, body: &str) {
        // The fixture stamps the CURRENT harness version so the drift advisory
        // stays silent — these tests exercise the injectable path, not drift.
        std::fs::write(
            dir.join("mustard.json"),
            format!(
                r#"{{"version":"{}","inject":[{{"on":"sessionStart","file":".claude/mustard/response-style.md","once":true}}]}}"#,
                mustard_core::harness_version()
            ),
        )
        .unwrap();
        let mustard_dir = dir.join(".claude").join("mustard");
        std::fs::create_dir_all(&mustard_dir).unwrap();
        std::fs::write(mustard_dir.join("response-style.md"), body).unwrap();
    }

    fn session_input_with_source(session_id: &str, source: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some(session_id.to_string()),
            raw: json!({ "source": source }),
            ..HookInput::default()
        }
    }

    #[test]
    fn session_start_injects_declared_file_once() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_session_injectable(dir.path(), "STYLE-BODY\n");

        // Startup: the declared file rides the SessionStart inject.
        let v = session_start_core(
            &session_input_with_source("s1", "startup"),
            &ctx(project),
            NO_REGISTRY,
            NO_SCRATCH,
        )
        .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("STYLE-BODY"), "injectable missing: {context}");
            }
            other => panic!("expected Inject, got {other:?}"),
        }
        assert!(
            dir.path()
                .join(".claude/.session/s1/injected-response-style.md")
                .is_file(),
            "delivery marker recorded"
        );

        // A resume of the SAME session finds the marker → no re-delivery (no
        // terrain here, so the verdict degrades to Allow).
        let v = session_start_core(
            &session_input_with_source("s1", "resume"),
            &ctx(project),
            NO_REGISTRY,
            NO_SCRATCH,
        )
        .unwrap();
        assert!(
            matches!(v, Verdict::Allow),
            "once injectable must not re-deliver on resume: {v:?}"
        );
    }

    #[test]
    fn compact_resets_markers_and_reinjects() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_session_injectable(dir.path(), "STYLE-BODY\n");
        // Plant a userPromptSubmit marker too — compact must clear BOTH so the
        // next prompt re-delivers its own once entries.
        let session = dir.path().join(".claude/.session/s1");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("injected-orchestrator.md"), "x").unwrap();

        // First startup burns the sessionStart marker.
        let _ = SessionStartInject
            .evaluate(&session_input_with_source("s1", "startup"), &ctx(project))
            .unwrap();
        assert!(session.join("injected-response-style.md").is_file());

        // Compact: prompt-side marker cleared AND the sessionStart entry
        // re-injects despite its (now cleared) marker.
        let v = SessionStartInject
            .evaluate(&session_input_with_source("s1", "compact"), &ctx(project))
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("STYLE-BODY"), "compact must re-inject: {context}");
            }
            other => panic!("expected re-inject on compact, got {other:?}"),
        }
        assert!(
            !session.join("injected-orchestrator.md").exists(),
            "compact clears the prompt-side once markers"
        );
        assert!(
            session.join("injected-response-style.md").is_file(),
            "the re-delivered sessionStart entry re-records its marker"
        );
    }

    #[test]
    fn clear_resets_markers_and_reinjects() {
        // A `/clear` refreshes the window exactly like a compaction: the
        // sessionStart entries must ride back in and the prompt-side `once`
        // markers must be cleared so the orchestrator re-delivers next prompt.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_session_injectable(dir.path(), "STYLE-BODY\n");
        let session = dir.path().join(".claude/.session/s1");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("injected-orchestrator.md"), "x").unwrap();

        // First startup burns the sessionStart marker.
        let _ = SessionStartInject
            .evaluate(&session_input_with_source("s1", "startup"), &ctx(project))
            .unwrap();
        assert!(session.join("injected-response-style.md").is_file());

        // Clear: prompt-side marker cleared AND the sessionStart entry
        // re-injects despite its (now cleared) marker.
        let v = SessionStartInject
            .evaluate(&session_input_with_source("s1", "clear"), &ctx(project))
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("STYLE-BODY"), "clear must re-inject: {context}");
            }
            other => panic!("expected re-inject on clear, got {other:?}"),
        }
        assert!(
            !session.join("injected-orchestrator.md").exists(),
            "clear clears the prompt-side once markers"
        );
        assert!(
            session.join("injected-response-style.md").is_file(),
            "the re-delivered sessionStart entry re-records its marker"
        );
    }

    #[test]
    fn missing_declared_file_degrades_to_allow() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        // Stamped with the current harness version: the drift advisory stays
        // silent, isolating the missing-file behaviour under test.
        std::fs::write(
            dir.path().join("mustard.json"),
            format!(
                r#"{{"version":"{}","inject":[{{"on":"sessionStart","file":".claude/mustard/gone.md","once":true}}]}}"#,
                mustard_core::harness_version()
            ),
        )
        .unwrap();
        let v = session_start_core(
            &session_input_with_source("s1", "startup"),
            &ctx(project),
            NO_REGISTRY,
            NO_SCRATCH,
        )
        .unwrap();
        assert!(matches!(v, Verdict::Allow), "missing declared file must fail open: {v:?}");
    }
}
