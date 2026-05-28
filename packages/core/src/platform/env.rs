//! Hook runtime environment — a behavioural port of `_lib/hook-env.js`.
//!
//! The JS `hook-env.js` is the shared gate every hook calls before doing any
//! work: it answers "should this hook run at all?", detects a hook recursing
//! into itself, and resolves cwd / session identity. This module reproduces
//! that behaviour with **parity** as the goal.
//!
//! ## Why an [`Env`] trait
//!
//! `hook-env.js` reads *and mutates* `process.env` (the re-entrancy guard sets
//! `MUSTARD_HOOK_RUNNING_<NAME>=1`, the depth counter bumps
//! `MUSTARD_HOOK_DEPTH`). Reading and mutating real process environment is a
//! side effect and untestable in isolation. So the environment is abstracted
//! behind the [`Env`] trait: [`ProcessEnv`] is the production implementation
//! over `std::env`, and tests inject [`MapEnv`].
//!
//! ## Parity surface
//!
//! Ported from `hook-env.js`: [`HookProfile`] (the `MUSTARD_HOOK_PROFILE`
//! values and the `minimal` allow-list), [`should_run`], [`is_strict_mode`],
//! [`acquire_guard`], [`check_depth`], [`is_self_delegation`],
//! [`is_in_hook_phase`], and [`guarded_run`]. The `pickRuntime` runtime shim is
//! intentionally out of scope — it is a Bun-vs-Node detail with no equivalent
//! in a Rust binary.

use crate::domain::model::contract::HookInput;

/// Default maximum hook recursion depth — `checkDepth`'s `maxDepth` default in
/// `hook-env.js`.
pub const DEFAULT_MAX_DEPTH: u32 = 3;

/// An abstraction over the process environment.
///
/// Production code uses [`ProcessEnv`]; tests use [`MapEnv`]. Mutation is part
/// of the trait because `hook-env.js`'s guard and depth counter write back to
/// `process.env`.
pub trait Env {
    /// Read an environment variable, `None` when unset.
    fn get(&self, key: &str) -> Option<String>;
    /// Set an environment variable for the remainder of the process.
    fn set(&self, key: &str, value: &str);
}

/// The production [`Env`] over `std::env`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl Env for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        // The overlay is consulted first so the guard/depth round-trip (set
        // then get of the same key) sees its own writes; if a key was never
        // set through this `Env`, the real process environment answers.
        OVERLAY
            .with(|overlay| overlay.borrow().get(key).cloned())
            .or_else(|| std::env::var(key).ok())
    }

    fn set(&self, key: &str, value: &str) {
        // SAFETY note: `std::env::set_var` is `unsafe` since Rust 2024 because
        // it is not thread-safe. Hooks are single-threaded short-lived
        // processes, so this is sound — but `unsafe_code` is forbidden
        // crate-wide, so the mutation is delegated to a thread-local map
        // instead of touching the real process environment. `ProcessEnv`
        // reads stay backed by `std::env`; writes stay process-local and
        // visible only to this `Env` for the rest of the run, which is exactly
        // what the JS guard needs (it only ever reads back its own writes).
        OVERLAY.with(|overlay| {
            overlay
                .borrow_mut()
                .insert(key.to_string(), value.to_string());
        });
    }
}

thread_local! {
    /// Process-local overlay for [`ProcessEnv::set`] — see the safety note
    /// there. Reads in [`ProcessEnv::get`] do *not* consult it (real env is
    /// the source of truth for inputs); only the guard/depth round-trips,
    /// which write then read the same key, rely on it. To keep that round-trip
    /// correct, [`ProcessEnv::get`] checks the overlay first.
    static OVERLAY: std::cell::RefCell<std::collections::HashMap<String, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// An in-memory [`Env`] for tests.
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    inner: std::cell::RefCell<std::collections::HashMap<String, String>>,
}

impl MapEnv {
    /// An empty environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a variable, builder-style.
    #[must_use]
    pub fn with(self, key: &str, value: &str) -> Self {
        self.inner
            .borrow_mut()
            .insert(key.to_string(), value.to_string());
        self
    }
}

impl Env for MapEnv {
    fn get(&self, key: &str) -> Option<String> {
        self.inner.borrow().get(key).cloned()
    }

    fn set(&self, key: &str, value: &str) {
        self.inner
            .borrow_mut()
            .insert(key.to_string(), value.to_string());
    }
}

/// The hook profile selected by `MUSTARD_HOOK_PROFILE`.
///
/// Mirrors `hook-env.js`'s `PROFILES` table. Under [`HookProfile::Minimal`]
/// only an allow-list of safety-critical hooks runs; `Standard` and `Strict`
/// run every hook (they differ only in whether [`is_strict_mode`] is `true`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookProfile {
    /// Only `bash-safety` and `file-guard` run — the `minimal` allow-list.
    Minimal,
    /// Every hook runs. The default when `MUSTARD_HOOK_PROFILE` is unset.
    Standard,
    /// Every hook runs; gates additionally treat marginal cases strictly.
    Strict,
}

impl HookProfile {
    /// Parse the `MUSTARD_HOOK_PROFILE` value. Unknown values fall back to
    /// [`HookProfile::Standard`], matching the JS `PROFILES[profile]` lookup
    /// which yields `undefined` (treated as "allow all") for an unknown key.
    ///
    /// Parity note: `hook-env.js` lower-cases the variable but does **not**
    /// trim it (`(process.env.MUSTARD_HOOK_PROFILE || 'standard').toLowerCase()`).
    /// So `" minimal "` is *not* a recognised profile in the JS and falls
    /// through to "allow all" — this port reproduces that exactly, without a
    /// `trim()`.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "minimal" => Self::Minimal,
            "strict" => Self::Strict,
            _ => Self::Standard,
        }
    }

    /// The allow-list for this profile, or `None` when every hook is allowed.
    /// Only [`HookProfile::Minimal`] restricts; mirrors `PROFILES`.
    #[must_use]
    fn allow_list(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Minimal => Some(&["bash-safety", "file-guard"]),
            Self::Standard | Self::Strict => None,
        }
    }
}

/// Parse a comma-separated, lowercased list (the `MUSTARD_DISABLED_HOOKS`
/// format). Empty entries are dropped — matches `hook-env.js`'s
/// `.split(',').map(trim+lowercase).filter(Boolean)`.
fn parse_csv_lower(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Whether `hook_name` should run, given the current profile and the
/// `MUSTARD_DISABLED_HOOKS` list. Port of `shouldRun`.
///
/// A hook listed in `MUSTARD_DISABLED_HOOKS` never runs. Otherwise, under the
/// `minimal` profile only allow-listed hooks run; under `standard`/`strict`
/// every non-disabled hook runs. Comparison is case-insensitive.
#[must_use]
pub fn should_run<E: Env>(env: &E, hook_name: &str) -> bool {
    let profile = HookProfile::parse(&env.get("MUSTARD_HOOK_PROFILE").unwrap_or_default());
    let disabled = parse_csv_lower(&env.get("MUSTARD_DISABLED_HOOKS").unwrap_or_default());
    // Parity: JS `shouldRun` does `hookName.toLowerCase()` — case-folded but
    // not trimmed. The `MUSTARD_DISABLED_HOOKS` *entries* are trimmed (by
    // `parse_csv_lower`), but the hook name passed in is not.
    let name = hook_name.to_ascii_lowercase();

    if disabled.contains(&name) {
        return false;
    }
    match profile.allow_list() {
        Some(allowed) => allowed.iter().any(|h| *h == name),
        None => true,
    }
}

/// Whether `MUSTARD_HOOK_PROFILE` is exactly `strict`. Port of `isStrictMode`.
///
/// Parity note: `isStrictMode` is `(process.env.MUSTARD_HOOK_PROFILE || '')
/// .toLowerCase() === 'strict'` — case-insensitive but **not** trimmed. A
/// value with surrounding whitespace is not `strict` in the JS, so this port
/// does not trim either.
#[must_use]
pub fn is_strict_mode<E: Env>(env: &E) -> bool {
    env.get("MUSTARD_HOOK_PROFILE")
        .is_some_and(|v| v.eq_ignore_ascii_case("strict"))
}

/// The env-var key the re-entrancy guard uses for `hook_name`:
/// `MUSTARD_HOOK_RUNNING_<UPPER_SNAKE>`. Mirrors `acquireGuard`'s key build.
#[must_use]
fn guard_key(hook_name: &str) -> String {
    format!(
        "MUSTARD_HOOK_RUNNING_{}",
        hook_name.to_ascii_uppercase().replace('-', "_")
    )
}

/// Re-entrancy guard. Port of `acquireGuard`.
///
/// Returns `false` if this hook is already marked running (its guard env var
/// is `"1"`); otherwise sets the marker and returns `true`. The first caller
/// wins; a recursive invocation is rejected.
pub fn acquire_guard<E: Env>(env: &E, hook_name: &str) -> bool {
    let key = guard_key(hook_name);
    if env.get(&key).as_deref() == Some("1") {
        return false;
    }
    env.set(&key, "1");
    true
}

/// Depth counter. Port of `checkDepth`.
///
/// Reads `MUSTARD_HOOK_DEPTH` (default `0`). Returns `false` once depth has
/// reached `max_depth`; otherwise increments the counter and returns `true`.
/// A non-numeric `MUSTARD_HOOK_DEPTH` is treated as `0`, matching JS
/// `parseInt(... , 10)` falling through the `|| '0'` default.
pub fn check_depth<E: Env>(env: &E, max_depth: u32) -> bool {
    let depth: u32 = env
        .get("MUSTARD_HOOK_DEPTH")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    if depth >= max_depth {
        return false;
    }
    env.set("MUSTARD_HOOK_DEPTH", &(depth + 1).to_string());
    true
}

/// Self-delegation detection. Port of `isSelfDelegation`.
///
/// Returns `true` when the invocation is a hook delegating into itself, by
/// either signal `hook-env.js` checks:
///
/// 1. the child `session_id` equals the parent `MUSTARD_SESSION_ID`; or
/// 2. the Task `description` mentions hook internals (`subagent-tracker`,
///    `hook-env`, or `hook evaluation`), case-insensitively.
#[must_use]
pub fn is_self_delegation<E: Env>(env: &E, input: &HookInput) -> bool {
    if let (Some(parent), Some(child)) =
        (env.get("MUSTARD_SESSION_ID"), input.session_id.as_deref())
    {
        if !parent.is_empty() && parent == child {
            return true;
        }
    }

    let description = input
        .tool_input
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    description.contains("subagent-tracker")
        || description.contains("hook-env")
        || description.contains("hook evaluation")
}

/// Whether the harness is inside the dedicated hook phase. Port of
/// `isInHookPhase` — `MUSTARD_IN_HOOK_PHASE` is exactly `"1"`.
#[must_use]
pub fn is_in_hook_phase<E: Env>(env: &E) -> bool {
    env.get("MUSTARD_IN_HOOK_PHASE").as_deref() == Some("1")
}

/// Combined guard. Port of `guardedRun`.
///
/// Returns `true` only if all four checks pass, in the same short-circuit
/// order as the JS: [`should_run`] → [`acquire_guard`] → [`check_depth`] →
/// `!`[`is_in_hook_phase`]. `max_depth` defaults to [`DEFAULT_MAX_DEPTH`] when
/// `None`.
pub fn guarded_run<E: Env>(
    env: &E,
    hook_name: &str,
    input: &HookInput,
    max_depth: Option<u32>,
) -> bool {
    let _ = input; // parity: JS `guardedRun` takes `data` but does not use it
    if !should_run(env, hook_name) {
        return false;
    }
    if !acquire_guard(env, hook_name) {
        return false;
    }
    if !check_depth(env, max_depth.unwrap_or(DEFAULT_MAX_DEPTH)) {
        return false;
    }
    !is_in_hook_phase(env)
}

/// Resolve the working directory for a hook invocation.
///
/// Prefers the harness-supplied `cwd` on the [`HookInput`], then falls back to
/// the `CLAUDE_PROJECT_DIR` env var, then to `MUSTARD_PROJECT_DIR`. Returns
/// `None` when nothing is known — the caller fails open (e.g. uses the real
/// process cwd). Mirrors the cwd resolution in `harness-event.js`'s
/// `resolveProjectDir`.
#[must_use]
pub fn resolve_cwd<E: Env>(env: &E, input: &HookInput) -> Option<String> {
    if let Some(cwd) = input.cwd.as_deref() {
        if !cwd.is_empty() {
            return Some(cwd.to_string());
        }
    }
    env.get("CLAUDE_PROJECT_DIR")
        .filter(|s| !s.is_empty())
        .or_else(|| env.get("MUSTARD_PROJECT_DIR").filter(|s| !s.is_empty()))
}

/// Resolve the session id for a hook invocation.
///
/// Prefers the [`HookInput`]'s `session_id`, then `MUSTARD_SESSION_ID`, then
/// `CLAUDE_SESSION_ID`. Returns `None` when no session is known — unlike
/// `harness-event.js`'s `getCurrentSessionId`, no random fallback id is
/// generated here (id generation is a side effect; the caller decides). Order
/// matches `getCurrentSessionId`.
#[must_use]
pub fn resolve_session_id<E: Env>(env: &E, input: &HookInput) -> Option<String> {
    if let Some(id) = input.session_id.as_deref() {
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    env.get("MUSTARD_SESSION_ID")
        .filter(|s| !s.is_empty())
        .or_else(|| env.get("CLAUDE_SESSION_ID").filter(|s| !s.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input_with(session: Option<&str>, description: Option<&str>) -> HookInput {
        HookInput {
            session_id: session.map(str::to_string),
            tool_input: description.map_or(json!({}), |d| json!({ "description": d })),
            ..HookInput::default()
        }
    }

    #[test]
    fn should_run_default_profile_allows_all() {
        let env = MapEnv::new();
        assert!(should_run(&env, "any-hook"));
    }

    #[test]
    fn should_run_respects_disabled_hooks() {
        let env = MapEnv::new().with("MUSTARD_DISABLED_HOOKS", "close-gate, model-routing-gate");
        assert!(!should_run(&env, "close-gate"));
        assert!(!should_run(&env, "Model-Routing-Gate")); // case-insensitive
        assert!(should_run(&env, "bash-safety"));
    }

    #[test]
    fn should_run_minimal_profile_only_allows_listed() {
        let env = MapEnv::new().with("MUSTARD_HOOK_PROFILE", "minimal");
        assert!(should_run(&env, "bash-safety"));
        assert!(should_run(&env, "file-guard"));
        assert!(!should_run(&env, "close-gate"));
    }

    #[test]
    fn is_strict_mode_only_true_for_strict() {
        assert!(is_strict_mode(
            &MapEnv::new().with("MUSTARD_HOOK_PROFILE", "strict")
        ));
        assert!(!is_strict_mode(
            &MapEnv::new().with("MUSTARD_HOOK_PROFILE", "standard")
        ));
        assert!(!is_strict_mode(&MapEnv::new()));
    }

    #[test]
    fn acquire_guard_rejects_second_acquisition() {
        let env = MapEnv::new();
        assert!(acquire_guard(&env, "close-gate"));
        assert!(!acquire_guard(&env, "close-gate"));
        // A different hook still acquires.
        assert!(acquire_guard(&env, "spec-size-gate"));
    }

    #[test]
    fn check_depth_blocks_at_max() {
        let env = MapEnv::new();
        assert!(check_depth(&env, 3)); // 0 -> 1
        assert!(check_depth(&env, 3)); // 1 -> 2
        assert!(check_depth(&env, 3)); // 2 -> 3
        assert!(!check_depth(&env, 3)); // 3 >= 3 -> blocked
    }

    #[test]
    fn is_self_delegation_by_session_match() {
        let env = MapEnv::new().with("MUSTARD_SESSION_ID", "sess-1");
        assert!(is_self_delegation(&env, &input_with(Some("sess-1"), None)));
        assert!(!is_self_delegation(&env, &input_with(Some("sess-2"), None)));
    }

    #[test]
    fn is_self_delegation_by_description() {
        let env = MapEnv::new();
        assert!(is_self_delegation(
            &env,
            &input_with(None, Some("Run HOOK EVALUATION pass"))
        ));
        assert!(is_self_delegation(
            &env,
            &input_with(None, Some("port hook-env.js"))
        ));
        assert!(!is_self_delegation(
            &env,
            &input_with(None, Some("add a login form"))
        ));
    }

    #[test]
    fn guarded_run_passes_clean_invocation() {
        let env = MapEnv::new();
        let input = HookInput::default();
        assert!(guarded_run(&env, "close-gate", &input, None));
        // Second call is rejected by the re-entrancy guard.
        assert!(!guarded_run(&env, "close-gate", &input, None));
    }

    #[test]
    fn guarded_run_blocked_in_hook_phase() {
        let env = MapEnv::new().with("MUSTARD_IN_HOOK_PHASE", "1");
        assert!(!guarded_run(&env, "close-gate", &HookInput::default(), None));
    }

    #[test]
    fn resolve_cwd_prefers_input_then_env() {
        let env = MapEnv::new().with("CLAUDE_PROJECT_DIR", "/from/env");
        let with_cwd = HookInput {
            cwd: Some("/from/input".into()),
            ..HookInput::default()
        };
        assert_eq!(
            resolve_cwd(&env, &with_cwd).as_deref(),
            Some("/from/input")
        );
        assert_eq!(
            resolve_cwd(&env, &HookInput::default()).as_deref(),
            Some("/from/env")
        );
    }

    #[test]
    fn resolve_session_id_order() {
        let env = MapEnv::new()
            .with("MUSTARD_SESSION_ID", "mustard")
            .with("CLAUDE_SESSION_ID", "claude");
        let with_id = HookInput {
            session_id: Some("input".into()),
            ..HookInput::default()
        };
        assert_eq!(resolve_session_id(&env, &with_id).as_deref(), Some("input"));
        assert_eq!(
            resolve_session_id(&env, &HookInput::default()).as_deref(),
            Some("mustard")
        );
    }
}
