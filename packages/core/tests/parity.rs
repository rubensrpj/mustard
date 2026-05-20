// This is an integration-test crate, not a `#[cfg(test)]` module of the
// library, so `lib.rs`'s `cfg_attr(test, allow(unwrap_used))` carve-out does
// not reach it. The workspace `unwrap_used = "deny"` lint still applies here —
// allow it explicitly: a panicking `.unwrap()`/`.expect()` in a test *is* the
// test failure, which is the intended behaviour.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Behavioural parity tests — JS `_lib/*.js` vs the `mustard-core` Rust port.
//!
//! b2 Wave 4. The oracle is the JS source under
//! `packages/cli/templates/hooks/_lib/` and its sibling tests under
//! `packages/cli/templates/hooks/__tests__/`. Each test below pins a specific
//! behaviour of a `_lib` file and verifies the Rust port reproduces it,
//! focusing on the edge cases the per-module unit tests do not cover:
//! fail-open, missing input, corrupt JSON, profile/whitespace handling,
//! object-spread collisions, and round-trips against real on-disk fixtures
//! (`.claude/.harness/mustard.db`, `.claude/.pipeline-states/*.json`).
//!
//! Where JS and Rust legitimately diverge, the divergence is documented inline
//! in the test that proves it.

use mustard_core::config::{EnforcementConfig, Mode};
use mustard_core::env::{
    Env, HookProfile, MapEnv, acquire_guard, check_depth, guarded_run, is_in_hook_phase,
    is_self_delegation, is_strict_mode, resolve_cwd, resolve_session_id, should_run,
};
use mustard_core::store::event_store::EventSink;
use mustard_core::store::pipeline_repo::{FsPipelineRepo, PipelineRepo, read_optional};
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::knowledge::{PipelineMetrics, ToolBreakdown, derive_prescription, extract_friction};
use mustard_core::metrics::{MetricLine, emit_metric, metric_file_path};
use mustard_core::model::contract::HookInput;
use mustard_core::model::pipeline::{Phase, Scope};
use serde_json::{Value, json};
use std::path::Path;
use tempfile::tempdir;

// ===========================================================================
// hook-env.js  ->  env.rs
// ===========================================================================

/// `hook-env.js#shouldRun`: `PROFILES[profile]` with an unknown key yields
/// `undefined` ("allow all"). The JS lower-cases but **does not trim** the
/// `MUSTARD_HOOK_PROFILE` value — so `" minimal "` (padded) is an unknown key
/// and falls through to allow-all. The Rust port matches this: a padded
/// profile is NOT treated as `minimal`.
#[test]
fn parity_profile_value_is_not_trimmed() {
    // Exact "minimal" restricts.
    assert!(!should_run(&MapEnv::new().with("MUSTARD_HOOK_PROFILE", "minimal"), "close-gate"));
    // Padded "minimal" is an unknown profile key -> allow all (JS parity).
    assert!(should_run(
        &MapEnv::new().with("MUSTARD_HOOK_PROFILE", " minimal "),
        "close-gate"
    ));
    assert_eq!(HookProfile::parse(" minimal "), HookProfile::Standard);
    // Same for strict: padded value is not strict mode.
    assert!(!is_strict_mode(&MapEnv::new().with("MUSTARD_HOOK_PROFILE", " strict ")));
    assert!(is_strict_mode(&MapEnv::new().with("MUSTARD_HOOK_PROFILE", "STRICT")));
}

/// `shouldRun`: an unknown profile string behaves like `standard` — every hook
/// runs (JS `PROFILES['bogus']` is `undefined`).
#[test]
fn parity_unknown_profile_allows_all() {
    let env = MapEnv::new().with("MUSTARD_HOOK_PROFILE", "paranoid");
    assert!(should_run(&env, "close-gate"));
    assert!(should_run(&env, "bash-safety"));
    assert_eq!(HookProfile::parse("paranoid"), HookProfile::Standard);
}

/// `shouldRun`: `MUSTARD_DISABLED_HOOKS` is split on commas and each entry is
/// trimmed + lower-cased (`split(',').map(trim+lowercase).filter(Boolean)`).
/// Empty entries (`,,`) are dropped; matching is case-insensitive.
#[test]
fn parity_disabled_hooks_csv_is_trimmed_and_filtered() {
    let env = MapEnv::new().with("MUSTARD_DISABLED_HOOKS", " Close-Gate , , model-routing-gate ,");
    assert!(!should_run(&env, "close-gate"));
    assert!(!should_run(&env, "CLOSE-GATE"));
    assert!(!should_run(&env, "model-routing-gate"));
    assert!(should_run(&env, "bash-safety"));
    // An all-empty list disables nothing.
    assert!(should_run(&MapEnv::new().with("MUSTARD_DISABLED_HOOKS", " , , "), "anything"));
}

/// `acquireGuard`: the guard env key is `MUSTARD_HOOK_RUNNING_<UPPER_SNAKE>`
/// and the marker value is the exact string `"1"`. A pre-existing marker
/// rejects the first acquisition; any other value (e.g. `"0"`, `"true"`) does
/// not — JS compares `=== '1'`.
#[test]
fn parity_acquire_guard_only_blocks_on_exact_one() {
    // A marker value other than "1" does not block (JS `=== '1'`).
    let env = MapEnv::new().with("MUSTARD_HOOK_RUNNING_CLOSE_GATE", "0");
    assert!(acquire_guard(&env, "close-gate"));

    let env = MapEnv::new().with("MUSTARD_HOOK_RUNNING_CLOSE_GATE", "1");
    assert!(!acquire_guard(&env, "close-gate"));
}

/// `checkDepth`: `parseInt(process.env.MUSTARD_HOOK_DEPTH || '0', 10)`. A
/// non-numeric depth is treated as `0` (JS `parseInt('abc',10)` is `NaN`,
/// guarded by `|| '0'`); the default `maxDepth` is `3`.
#[test]
fn parity_check_depth_treats_garbage_as_zero() {
    let env = MapEnv::new().with("MUSTARD_HOOK_DEPTH", "not-a-number");
    // Garbage parses as 0 < 3 -> allowed, and the counter is bumped to 1.
    assert!(check_depth(&env, 3));
    assert_eq!(env.get("MUSTARD_HOOK_DEPTH").as_deref(), Some("1"));

    // A depth already at the cap blocks.
    let env = MapEnv::new().with("MUSTARD_HOOK_DEPTH", "3");
    assert!(!check_depth(&env, 3));
}

/// `isSelfDelegation`: parent/child session match only counts when *both*
/// sides are present and non-empty (JS `parentSession && childSession &&
/// parentSession === childSession`). Two missing sessions do not "match".
#[test]
fn parity_self_delegation_requires_both_sessions_present() {
    // Parent set, child missing -> not self-delegation.
    let env = MapEnv::new().with("MUSTARD_SESSION_ID", "sess-1");
    assert!(!is_self_delegation(&env, &HookInput::default()));

    // Neither set -> not self-delegation (two `undefined`s do not match in JS).
    assert!(!is_self_delegation(&MapEnv::new(), &HookInput::default()));

    // Both set and equal -> self-delegation.
    let input = HookInput {
        session_id: Some("sess-1".into()),
        ..HookInput::default()
    };
    assert!(is_self_delegation(&env, &input));
}

/// `isSelfDelegation`: the description signal is case-insensitive and matches
/// any of `subagent-tracker`, `hook-env`, `hook evaluation` as a substring.
#[test]
fn parity_self_delegation_description_substrings() {
    let env = MapEnv::new();
    for desc in ["SubAgent-Tracker run", "porting HOOK-ENV.js", "do a Hook Evaluation now"] {
        let input = HookInput {
            tool_input: json!({ "description": desc }),
            ..HookInput::default()
        };
        assert!(is_self_delegation(&env, &input), "should flag: {desc}");
    }
    // An unrelated description does not flag.
    let input = HookInput {
        tool_input: json!({ "description": "implement a checkout flow" }),
        ..HookInput::default()
    };
    assert!(!is_self_delegation(&env, &input));
}

/// `guardedRun`: short-circuits in the JS order
/// `shouldRun -> acquireGuard -> checkDepth -> !isInHookPhase`. A disabled
/// hook never even reaches the guard; being inside the hook phase blocks last.
#[test]
fn parity_guarded_run_short_circuits_in_order() {
    // Disabled -> false, and the guard is NOT acquired (shouldRun fails first).
    let env = MapEnv::new().with("MUSTARD_DISABLED_HOOKS", "close-gate");
    assert!(!guarded_run(&env, "close-gate", &HookInput::default(), None));
    assert!(env.get("MUSTARD_HOOK_RUNNING_CLOSE_GATE").is_none());

    // In the hook phase -> false even for a clean first invocation.
    let env = MapEnv::new().with("MUSTARD_IN_HOOK_PHASE", "1");
    assert!(!guarded_run(&env, "spec-size-gate", &HookInput::default(), None));
    assert!(is_in_hook_phase(&env));
}

/// `getCurrentSessionId` resolution order: hook input `session_id`, then
/// `MUSTARD_SESSION_ID`, then `CLAUDE_SESSION_ID`. The Rust port returns `None`
/// instead of the JS random `s-<hex>` fallback — id generation is a side
/// effect the caller owns. This is a documented, intentional divergence.
#[test]
fn parity_resolve_session_id_order_and_no_random_fallback() {
    let env = MapEnv::new()
        .with("MUSTARD_SESSION_ID", "mustard")
        .with("CLAUDE_SESSION_ID", "claude");
    let input = HookInput {
        session_id: Some("from-input".into()),
        ..HookInput::default()
    };
    assert_eq!(resolve_session_id(&env, &input).as_deref(), Some("from-input"));
    assert_eq!(
        resolve_session_id(&env, &HookInput::default()).as_deref(),
        Some("mustard")
    );
    // CLAUDE_SESSION_ID only when MUSTARD_SESSION_ID is absent.
    let env = MapEnv::new().with("CLAUDE_SESSION_ID", "claude");
    assert_eq!(resolve_session_id(&env, &HookInput::default()).as_deref(), Some("claude"));
    // Intentional divergence: nothing known -> None (JS would mint `s-<hex>`).
    assert_eq!(resolve_session_id(&MapEnv::new(), &HookInput::default()), None);
}

/// `resolveProjectDir` cwd order: hook input `cwd`, then `CLAUDE_PROJECT_DIR`,
/// then `MUSTARD_PROJECT_DIR`. An empty string is skipped, not returned.
#[test]
fn parity_resolve_cwd_order_skips_empty() {
    let env = MapEnv::new()
        .with("CLAUDE_PROJECT_DIR", "/from/claude")
        .with("MUSTARD_PROJECT_DIR", "/from/mustard");
    // Empty input cwd is skipped -> env wins.
    let input = HookInput {
        cwd: Some(String::new()),
        ..HookInput::default()
    };
    assert_eq!(resolve_cwd(&env, &input).as_deref(), Some("/from/claude"));
    // Nothing known -> None (caller fails open to the process cwd).
    assert_eq!(resolve_cwd(&MapEnv::new(), &HookInput::default()), None);
}

// ===========================================================================
// metrics-emit.js  ->  metrics.rs
// ===========================================================================

/// `emitMetric` line schema: the five fixed fields plus flat-merged `extras`.
/// The snapshot pins the exact compact serialised line `emit_metric` appends
/// to the `.jsonl` shard.
///
/// **Documented JS↔Rust divergence — accepted as cosmetic.** `metrics-emit.js`
/// writes object keys in *insertion* order (`ts, event, tokens_affected,
/// tokens_saved, note, ...extras`); this crate's `serde_json` is built without
/// the `preserve_order` feature, so its `Map` is a `BTreeMap` and serialises
/// keys **alphabetically** (`event, hook, note, role, tokens_affected,
/// tokens_saved, ts`). JSON object key order is semantically meaningless — the
/// only consumer, `metrics-report.js`, does `JSON.parse` and reads keys by
/// name — so parity holds at the value level. The snapshot records the real
/// sorted output rather than pretending the byte order matches.
#[test]
fn parity_metric_line_serialized_shape() {
    let line = MetricLine::new("2026-05-19T00:00:00.000Z", "budget-check")
        .tokens_affected(42)
        .tokens_saved(120)
        .note("blocked")
        .extras(json!({ "hook": "context-budget", "role": "Explore" }));
    let serialized = serde_json::to_string(&line.to_json()).unwrap();
    insta::assert_snapshot!(
        serialized,
        @r#"{"event":"budget-check","hook":"context-budget","note":"blocked","role":"Explore","tokens_affected":42,"tokens_saved":120,"ts":"2026-05-19T00:00:00.000Z"}"#
    );
    // Value-level parity: every JS field is present with the expected value,
    // regardless of key order.
    let parsed: Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(parsed["event"], json!("budget-check"));
    assert_eq!(parsed["tokens_affected"], json!(42));
    assert_eq!(parsed["tokens_saved"], json!(120));
    assert_eq!(parsed["note"], json!("blocked"));
    assert_eq!(parsed["hook"], json!("context-budget"));
}

/// `emitMetric` defaults: a bare line has `tokens_affected`/`tokens_saved`
/// zeroed and `note` empty (the JS `opts` defaults: `Number.isFinite(...) ? ...
/// : 0`, `typeof note === 'string' ? note : ''`), and no extras keys. Keys
/// serialise alphabetically — see `parity_metric_line_serialized_shape`.
#[test]
fn parity_metric_line_defaults() {
    let serialized = serde_json::to_string(&MetricLine::new("ts", "spec-hygiene-move").to_json()).unwrap();
    insta::assert_snapshot!(
        serialized,
        @r#"{"event":"spec-hygiene-move","note":"","tokens_affected":0,"tokens_saved":0,"ts":"ts"}"#
    );
}

/// `emitMetric` extras handling: the JS `...(opts.extras && typeof === 'object'
/// ? opts.extras : {})` spread merges object extras flat AND lets a colliding
/// key win over the fixed field (object-spread order). A non-object `extras`
/// is ignored entirely.
#[test]
fn parity_metric_extras_collision_lets_extras_win() {
    // Colliding `note` key: extras wins (spread is applied last).
    let line = MetricLine::new("ts", "ev").note("from-field").extras(json!({ "note": "from-extras" }));
    assert_eq!(line.to_json()["note"], json!("from-extras"));

    // Non-object extras: ignored, only the five fixed keys remain.
    let line = MetricLine::new("ts", "ev").extras(json!(["not", "an", "object"]));
    assert_eq!(line.to_json().as_object().map(serde_json::Map::len), Some(5));
}

/// `emitMetric` is fail-silent and returns a truthy/falsy result: it appends
/// `<cwd>/.claude/.metrics/<event>.jsonl` (one NDJSON line, newline-terminated)
/// and rejects a falsy/empty `event` *without* touching the filesystem.
#[test]
fn parity_emit_metric_appends_and_rejects_empty_event() {
    let dir = tempdir().unwrap();
    let line = MetricLine::new("2026-05-19T00:00:00.000Z", "rtk-rewrite").note("rewrote");
    assert!(emit_metric(dir.path(), &line));

    let shard = metric_file_path(dir.path(), "rtk-rewrite");
    let contents = std::fs::read_to_string(&shard).unwrap();
    assert!(contents.ends_with('\n'));
    let parsed: Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(parsed["event"], json!("rtk-rewrite"));

    // Empty / whitespace event name -> false, no `.metrics` dir created.
    let dir2 = tempdir().unwrap();
    assert!(!emit_metric(dir2.path(), &MetricLine::new("ts", "   ")));
    assert!(!dir2.path().join(".claude").join(".metrics").exists());
}

/// Per-event sharding: each distinct `event` lands in its own `<event>.jsonl`,
/// so `metrics-report.js`'s "iterate every `*.jsonl`" stays correct.
#[test]
fn parity_emit_metric_shards_per_event() {
    let dir = tempdir().unwrap();
    assert!(emit_metric(dir.path(), &MetricLine::new("t", "budget-check")));
    assert!(emit_metric(dir.path(), &MetricLine::new("t", "rtk-rewrite")));
    assert!(metric_file_path(dir.path(), "budget-check").exists());
    assert!(metric_file_path(dir.path(), "rtk-rewrite").exists());
}

// ===========================================================================
// knowledge-extract.js  ->  knowledge.rs
// ===========================================================================

/// `derivePrescription` heuristic 1 (L0 violation): `bash + edit > 3 * agent`
/// AND `retries > 2`. With `agent == 0` the threshold `3 * agent` is `0`, so
/// any `bash + edit > 0` paired with high retries fires it — matching the JS
/// integer arithmetic.
#[test]
fn parity_derive_prescription_l0_with_zero_agent() {
    let metrics = PipelineMetrics {
        retries: 3,
        api_calls: 5,
        tool_breakdown: ToolBreakdown { bash: 1, edit: 0, write: 0, agent: 0 },
    };
    let p = derive_prescription(&metrics).expect("heuristic 1 fires when agent=0");
    assert!(p.contains("delegate investigation"));
}

/// `derivePrescription`: heuristics are first-match-wins. A pipeline that
/// satisfies both heuristic 1 (L0) and heuristic 2 (fragmentation) yields the
/// heuristic-1 string — the JS returns on the first matching `if`.
#[test]
fn parity_derive_prescription_first_match_wins() {
    let metrics = PipelineMetrics {
        retries: 4, // > 2 (h1) and > 3 (h2)
        api_calls: 60, // > 50 (h2)
        tool_breakdown: ToolBreakdown { bash: 20, edit: 20, write: 0, agent: 1 },
    };
    let p = derive_prescription(&metrics).unwrap();
    assert!(p.contains("delegate investigation"), "heuristic 1 wins over 2");
}

/// `derivePrescription`: the boundary is strict `>`. `retries == 2`,
/// `apiCalls == 50`, `edit == 15` all fail their respective heuristics — the
/// JS conditions are `> 2`, `> 50`, `> 15`.
#[test]
fn parity_derive_prescription_boundaries_are_strict() {
    // retries exactly 2 -> heuristic 1 does not fire. `write: 9` keeps
    // heuristic 3 (`write < 3`) from firing on the high `edit`, isolating the
    // retries boundary as the thing under test.
    let m = PipelineMetrics {
        retries: 2,
        api_calls: 0,
        tool_breakdown: ToolBreakdown { bash: 99, edit: 99, write: 9, agent: 0 },
    };
    assert_eq!(derive_prescription(&m), None);

    // apiCalls exactly 50 -> heuristic 2 does not fire.
    let m = PipelineMetrics {
        retries: 9,
        api_calls: 50,
        tool_breakdown: ToolBreakdown::default(),
    };
    assert_eq!(derive_prescription(&m), None);

    // edit exactly 15 -> heuristic 3 does not fire.
    let m = PipelineMetrics {
        retries: 0,
        api_calls: 0,
        tool_breakdown: ToolBreakdown { bash: 0, edit: 15, write: 0, agent: 999 },
    };
    assert_eq!(derive_prescription(&m), None);
}

/// `from_json` for metrics is fail-open: missing or non-numeric `metrics`
/// fields read as `0` (JS `Number(metrics.retries) || 0`).
#[test]
fn parity_pipeline_metrics_from_json_fail_open() {
    let m = PipelineMetrics::from_json(&json!({ "retries": "garbage", "apiCalls": null }));
    assert_eq!(m.retries, 0);
    assert_eq!(m.api_calls, 0);
    // Absent toolBreakdown -> all zero.
    let m = PipelineMetrics::from_json(&json!({}));
    assert_eq!(m.tool_breakdown.bash, 0);
}

/// `extractFrictionFromStates`: a state with `retries > 2` AND `apiCalls > 50`
/// emits *two* entries (`high-hook-retry-*` and `heavy-pipeline-*`); when a
/// `derivePrescription` heuristic fired, both carry the prescription and the
/// extra `prescriptive` tag.
#[test]
fn parity_extract_friction_two_entries_with_prescription() {
    let states = vec![json!({
        "specName": "add-login",
        "metrics": {
            "retries": 5,
            "apiCalls": 80,
            // bash+edit=40 > 3*agent=3, retries 5 > 2 -> heuristic 1 fires.
            "toolBreakdown": { "Bash": 20, "Edit": 20, "Write": 0, "Agent": 1 }
        }
    })];
    let friction = extract_friction(&states);
    assert_eq!(friction.len(), 2);
    assert_eq!(friction[0].name, "high-hook-retry-add-login");
    assert_eq!(friction[1].name, "heavy-pipeline-add-login");
    // Both carry the prescription and the `prescriptive` tag.
    for entry in &friction {
        assert!(entry.prescription.is_some());
        assert!(entry.tags.contains(&"prescriptive".to_string()));
        assert!(entry.tags.contains(&"friction".to_string()));
    }
}

/// `extractFrictionFromStates` label fallback: `specName`, then `_file`, then
/// the literal `"unknown"`. A non-object entry in the array is skipped
/// (`if (!state || typeof state !== 'object') continue`).
#[test]
fn parity_extract_friction_label_fallback_and_skips_non_objects() {
    let states = vec![
        json!("a bare string is not a state"),
        json!({ "_file": "spec-from-file.json", "metrics": { "retries": 4 } }),
        json!({ "metrics": { "retries": 4 } }),
    ];
    let friction = extract_friction(&states);
    assert_eq!(friction.len(), 2);
    assert_eq!(friction[0].name, "high-hook-retry-spec-from-file.json");
    assert_eq!(friction[1].name, "high-hook-retry-unknown");
}

// ===========================================================================
// config.rs  (replaces the scattered MUSTARD_*_MODE reads in the JS gates)
// ===========================================================================

/// JS gates treat an unset `MUSTARD_<NAME>_MODE` as `strict`
/// (`(process.env.X || 'strict')`). `EnforcementConfig` reproduces that: an
/// unmodelled check resolves to `Mode::Strict`.
#[test]
fn parity_unset_mode_defaults_to_strict() {
    assert_eq!(EnforcementConfig::new().mode_of("close-gate"), Mode::Strict);
}

/// `resolve` layering: defaults -> `mustard.json` `enforcement` -> env. Env
/// always wins over the file; `MUSTARD_DISABLED_HOOKS` forces a check to
/// `Off`. A malformed `mustard.json` is swallowed (fail-open) and defaults
/// stand — a hook is never blocked by a config typo.
#[test]
fn parity_enforcement_resolve_layers_and_fail_open() {
    let env = std::collections::HashMap::from([
        ("MUSTARD_CLOSE_GATE_MODE", "strict"),
        ("MUSTARD_DISABLED_HOOKS", "spec-size"),
    ]);
    let config = EnforcementConfig::resolve(
        Some(r#"{ "enforcement": { "close-gate": "warn" } }"#),
        &["close-gate", "spec-size"],
        |k| env.get(k).map(|s| (*s).to_string()),
    );
    // Env overrode the file: warn -> strict.
    assert_eq!(config.mode_of("close-gate"), Mode::Strict);
    // Disabled via env -> Off, regardless of any mode entry.
    assert_eq!(config.mode_of("spec-size"), Mode::Off);

    // Malformed mustard.json -> swallowed, defaults stand.
    let config = EnforcementConfig::resolve(Some("{ not json"), &["close-gate"], |_| None);
    assert_eq!(config.mode_of("close-gate"), Mode::Strict);
}

// ===========================================================================
// io round-trips against real on-disk fixtures
// ===========================================================================

/// Round-trip against the *real* harness store `.claude/.harness/mustard.db`:
/// `SqliteEventStore::replay` opens the live database without error and every
/// event it yields carries the schema envelope.
///
/// The fixture is the live repo database; the test only reads it (never
/// writes). It is runtime state — skipped gracefully when absent.
#[test]
fn parity_replay_real_harness_store() {
    let db_path = repo_root().join(".claude").join(".harness").join("mustard.db");
    if !db_path.exists() {
        // The harness store is runtime state — skip gracefully if absent.
        return;
    }
    let store = SqliteEventStore::new(&db_path).expect("open real mustard.db");
    let events = store.replay().expect("replay real mustard.db never errors");
    // Every parsed event carries the schema envelope.
    for ev in events.iter().take(50) {
        assert_eq!(ev.v, mustard_core::model::event::SCHEMA_VERSION);
        assert!(!ev.event.is_empty());
    }
}

/// Append-then-replay round-trip through `SqliteEventStore`: each appended
/// event is recovered, in insertion order, with its payload intact. This is
/// the harness event bus written and read back through the `EventSink` trait.
#[test]
fn parity_event_store_append_replay_round_trip() {
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    let dir = tempdir().unwrap();
    let store = SqliteEventStore::new(dir.path().join("mustard.db")).unwrap();

    let mk = |name: &str, i: i64| HarnessEvent {
        v: SCHEMA_VERSION,
        ts: "2026-05-19T00:00:00.000Z".into(),
        session_id: "s-multi".into(),
        wave: 1,
        actor: Actor { kind: ActorKind::Hook, id: Some("harness-init".into()), actor_type: None },
        event: name.into(),
        payload: json!({ "i": i }),
        spec: None,
    };
    for i in 0..5 {
        store.append(&mk("tool.use", i)).unwrap();
    }
    let events = store.replay().unwrap();
    assert_eq!(events.len(), 5);
    for (i, ev) in events.iter().enumerate() {
        assert_eq!(ev.event, "tool.use");
        assert_eq!(ev.payload["i"], json!(i64::try_from(i).unwrap()));
    }
}

/// `replay` is fail-open: a fresh database (or one whose file was just
/// created) replays as an empty `Vec` rather than erroring — an unstarted
/// project simply has no events. The `query`/`specs`/`metrics`/`spans` reads
/// degrade the same way.
#[test]
fn parity_event_store_replay_fresh_db_is_empty() {
    let dir = tempdir().unwrap();
    let store = SqliteEventStore::new(dir.path().join("mustard.db")).unwrap();
    assert!(store.replay().unwrap().is_empty());
    assert!(store.query(None).unwrap().is_empty());
    assert!(store.specs().unwrap().is_empty());
    assert!(store.metrics("absent").unwrap().is_none());
}

/// Round-trip against the *real* pipeline-state file
/// `.claude/.pipeline-states/2026-05-18-b2-mustard-core-crate.json`:
/// `FsPipelineRepo::read` parses the live file the JS hooks
/// (`close-gate.js`, `model-routing-gate.js`) consume.
#[test]
fn parity_read_real_pipeline_state_fixture() {
    let states_dir = repo_root().join(".claude").join(".pipeline-states");
    let spec = "2026-05-18-b2-mustard-core-crate";
    if !states_dir.join(format!("{spec}.json")).exists() {
        return; // runtime state — skip gracefully if absent
    }
    let repo = FsPipelineRepo::new(&states_dir);
    let state = repo.read(spec).expect("real pipeline-state parses");
    assert_eq!(state.spec_name.as_deref(), Some(spec));
    assert_eq!(state.phase, Some(Phase::Execute));
    assert_eq!(state.scope, Some(Scope::Full));
    assert!(state.total_waves >= state.current_wave);
    // The b2 spec defines four waves.
    assert_eq!(state.tasks.len(), 4);
}

/// `read_optional` parity with the JS fail-open read pattern: an absent
/// pipeline-state is `Ok(None)` ("no active pipeline"), a present one is
/// `Ok(Some(_))`. A write is atomic — no `.tmp` siblings linger.
#[test]
fn parity_pipeline_repo_read_optional_and_atomic_write() {
    use mustard_core::model::pipeline::PipelineState;
    let dir = tempdir().unwrap();
    let repo = FsPipelineRepo::new(dir.path());

    assert!(read_optional(&repo, "absent").unwrap().is_none());

    let state = PipelineState {
        spec_name: Some("demo".into()),
        phase: Some(Phase::Execute),
        scope: Some(Scope::Full),
        current_wave: 4,
        total_waves: 4,
        ..PipelineState::default()
    };
    repo.write("demo", &state).unwrap();
    assert!(read_optional(&repo, "demo").unwrap().is_some());

    // Only the destination file remains — atomic write left no temp sibling.
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name())
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0], std::ffi::OsStr::new("demo.json"));
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// The Mustard repo root — `packages/core/` is two levels below it. Used to
/// reach the live `.claude/` fixtures for round-trip parity tests.
fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("packages/core has a repo root two levels up")
        .to_path_buf()
}
