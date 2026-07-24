//! `change_request_log` — durable per-spec record of user requests made WHILE a
//! spec is active (mid-pipeline).
//!
//! ## Why this exists
//!
//! The post-close amendment window ([`super::amend_window_inject`]) captures
//! user intent ONLY after a pipeline closes (its window opens on
//! `pipeline.complete`). During the active pipeline (ANALYZE → EXECUTE → REVIEW
//! → QA → CLOSE) a user's chat request to change something was recorded
//! NOWHERE — no event, no spec entry — it simply vanished. This observer closes
//! that gap.
//!
//! On every `UserPromptSubmit` while the resolved spec's `meta.json#outcome` is
//! `Active`, it:
//! 1. appends the prompt to `.claude/spec/{id}/change-requests.ndjson` — a
//!    durable, greppable record that lives WITH the spec; and
//! 2. emits a `pipeline.change.request` harness event for the event bus /
//!    dashboard.
//!
//! ## Boundaries
//!
//! - **Active pipeline only.** A terminal (`Completed`/`Cancelled`/…) spec is
//!   the amendment window's territory (post-close), so this observer skips it —
//!   the two never double-record.
//! - **Pure side-effect** ([`Observer`]): never returns a verdict, never blocks.
//! - **Fail-open.** Any resolution / IO error is swallowed; telemetry must never
//!   block a turn.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::time::now_iso8601;
use mustard_core::ClaudePaths;
use serde_json::json;
use std::io::Write;

use crate::shared::context::{current_spec, current_wave, spec_for_session};
use crate::shared::prompt::is_harness_notice;

/// Filename of the per-spec durable change-request log (machine-readable).
const LOG_FILE: &str = "change-requests.ndjson";

/// Filename of the human-readable change-log rendered beside the spec — the
/// VISIBLE record ("documentada na spec") whose machine twin is [`LOG_FILE`].
const CHANGE_LOG_MD: &str = "change-log.md";

/// The mid-pipeline change-request recorder.
pub struct ChangeRequestLog;


/// Resolve the spec this session is working on — the canonical session→spec
/// marker first ([`spec_for_session`]), then the env / legacy fallback
/// ([`current_spec`]). `None` when no spec is in scope (a chat with no active
/// pipeline has nothing to attribute a request to).
fn resolve_spec(project_dir: &str, session_id: Option<&str>) -> Option<String> {
    if let Some(sid) = session_id.filter(|s| !s.is_empty() && *s != "unknown") {
        if let Some(spec) = spec_for_session(project_dir, sid) {
            return Some(spec);
        }
    }
    current_spec(project_dir)
}

/// `true` when the spec's `meta.json#outcome` is `Active` — the pipeline is
/// still in flight. Fail-CLOSED: an unreadable / absent meta returns `false` so
/// we never log against a spec we cannot prove is live.
fn spec_is_active(project_dir: &str, spec: &str) -> bool {
    read_outcome_stage(project_dir, spec)
        .and_then(|(outcome, _)| outcome)
        .and_then(|o| mustard_core::Outcome::parse(&o))
        .map(|o| o == mustard_core::Outcome::Active)
        .unwrap_or(false)
}

/// Read `(outcome, stage)` from the spec's `meta.json`. `None` on any error.
fn read_outcome_stage(project_dir: &str, spec: &str) -> Option<(Option<String>, Option<String>)> {
    let cp = ClaudePaths::for_project(project_dir).ok()?;
    let sp = cp.for_spec(spec).ok()?;
    let meta = mustard_core::domain::meta::read_meta_beside(&sp.spec_md_path())?;
    Some((meta.outcome, meta.stage))
}

/// Append one change-request line to `.claude/spec/{id}/change-requests.ndjson`.
/// Best-effort: a missing spec dir is created; any IO error is dropped.
fn append_change_request(
    project_dir: &str,
    spec: &str,
    session_id: &str,
    stage: Option<&str>,
    prompt: &str,
) {
    let Ok(cp) = ClaudePaths::for_project(project_dir) else {
        return;
    };
    let Ok(sp) = cp.for_spec(spec) else {
        return;
    };
    let dir = sp.dir();
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let mut line = match serde_json::to_string(&json!({
        "ts": now_iso8601(),
        "session_id": session_id,
        "spec": spec,
        "stage": stage,
        "prompt": prompt,
    })) {
        Ok(s) => s,
        Err(_) => return,
    };
    line.push('\n');
    // Append-only NDJSON: UserPromptSubmit is serialised per session, so a plain
    // append (not atomic write) is the correct, idiomatic log primitive here.
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(LOG_FILE))
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `true` when `path` holds at least one byte and the last one is not a
/// newline — i.e. an append would fuse onto the existing final line.
///
/// Reads only the final byte (`seek` to `len - 1`), so the cost does not grow
/// with the log. Any IO failure answers `false`: the append then behaves exactly
/// as it did before this check existed, which is the fail-open direction.
fn last_byte_is_not_newline(path: &std::path::Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(len) = f.metadata().map(|m| m.len()) else {
        return false;
    };
    if len == 0 {
        return false;
    }
    if f.seek(SeekFrom::Start(len - 1)).is_err() {
        return false;
    }
    let mut last = [0u8; 1];
    f.read_exact(&mut last).is_ok() && last[0] != b'\n'
}

/// Append a human-readable bullet to `.claude/spec/{id}/change-log.md` — the
/// VISIBLE record of mid-pipeline requests that lives WITH the spec. The frozen
/// `spec.md` narrative is never touched; this is a separate, append-only doc, so
/// SDD purity holds while the request is still documented in the spec folder.
/// Best-effort, fail-open.
fn append_change_log_md(project_dir: &str, spec: &str, stage: Option<&str>, prompt: &str) {
    let Ok(cp) = ClaudePaths::for_project(project_dir) else {
        return;
    };
    let Ok(sp) = cp.for_spec(spec) else {
        return;
    };
    let dir = sp.dir();
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join(CHANGE_LOG_MD);
    let header_needed = !path.exists();
    // An append assumes the file ends in a newline, and a hand-edited or
    // externally-rewritten log may not — the two bullets then fuse into one
    // line and the older entry is no longer greppable on its own. Observed in
    // the field. Cheap to check, and the read is bounded to the last byte.
    let needs_separator = !header_needed && last_byte_is_not_newline(&path);
    // Collapse whitespace so a multi-line prompt stays a single bullet.
    let one_line = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let stage_tag = stage.map(|s| format!(" _({s})_")).unwrap_or_default();
    let bullet = format!("- **{}**{} — {}\n", now_iso8601(), stage_tag, one_line);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        if needs_separator {
            let _ = f.write_all(b"\n");
        }
        if header_needed {
            let _ = f.write_all(
                format!(
                    "# Change Log — {spec}\n\n_Solicitações registradas automaticamente \
                     durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO \
                     é alterado; dobre o que muda comportamento em `## Acceptance Criteria` \
                     e rode o QA de novo._\n\n"
                )
                .as_bytes(),
            );
        }
        let _ = f.write_all(bullet.as_bytes());
    }
}

/// Emit a `pipeline.change.request` harness event (best-effort, NDJSON route).
fn emit_event(project_dir: &str, session_id: &str, spec: &str, stage: Option<&str>, prompt: &str) {
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id.to_string(),
        wave: u32::try_from(current_wave().unwrap_or(0)).unwrap_or(0),
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("change_request_log".to_string()),
            actor_type: None,
        },
        event: "pipeline.change.request".to_string(),
        payload: json!({ "spec": spec, "stage": stage, "prompt": prompt }),
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(project_dir, &ev);
}

impl Observer for ChangeRequestLog {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::UserPromptSubmit) {
            return;
        }
        let project_dir = ctx.project_dir_or_cwd(input);
        let prompt = input
            .raw
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        if prompt.is_empty() {
            return;
        }
        // Skip slash-commands (`/close`, `/qa`, `/approve`, …): pipeline CONTROL
        // actions, not change requests. Recording them would pollute the log AND
        // let a `/close` register itself as an unaddressed request at the
        // QA-composition gate (a self-inflicted deadlock).
        if prompt.starts_with('/') {
            return;
        }
        // Skip runtime notices for the same reason: a background task reporting
        // that it finished is not a request to change anything. It arrives on
        // this trigger only because the runtime speaks through the user channel.
        if is_harness_notice(&prompt) {
            return;
        }
        let Some(spec) = resolve_spec(&project_dir, input.session_id.as_deref()) else {
            return; // not inside a spec → nothing to attribute the request to.
        };
        if !spec_is_active(&project_dir, &spec) {
            return; // terminal spec → post-close territory (amend_window owns it).
        }
        let session_id = input.session_id.as_deref().unwrap_or("unknown");
        let stage = read_outcome_stage(&project_dir, &spec).and_then(|(_, stage)| stage);
        append_change_request(&project_dir, &spec, session_id, stage.as_deref(), &prompt);
        append_change_log_md(&project_dir, &spec, stage.as_deref(), &prompt);
        emit_event(&project_dir, session_id, &spec, stage.as_deref(), &prompt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::context::bind_session_spec;
    use serde_json::Value;
    use tempfile::tempdir;

    /// Seed a spec with a `meta.json` (outcome/stage) under `cwd`.
    fn seed_spec(cwd: &std::path::Path, spec: &str, outcome: &str, stage: &str) {
        let sp = ClaudePaths::for_project(cwd).unwrap().for_spec(spec).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            json!({ "scope": "light", "stage": stage, "outcome": outcome }).to_string(),
        )
        .unwrap();
    }

    fn prompt_input(session_id: &str, cwd: &std::path::Path, prompt: &str) -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: Value::Null,
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            raw: json!({ "prompt": prompt }),
            ..HookInput::default()
        }
    }

    fn ctx_for(cwd: &std::path::Path) -> Ctx {
        Ctx {
            project_dir: cwd.to_string_lossy().into_owned(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
        }
    }

    fn log_contents(cwd: &std::path::Path, spec: &str) -> Option<String> {
        let sp = ClaudePaths::for_project(cwd).unwrap().for_spec(spec).unwrap();
        std::fs::read_to_string(sp.dir().join(LOG_FILE)).ok()
    }

    /// An ACTIVE spec records the user's change request to the durable log.
    #[test]
    fn records_request_for_active_spec() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        seed_spec(cwd, "my-feature", "Active", "Execute");
        let cwd_str = cwd.to_string_lossy().into_owned();
        bind_session_spec(&cwd_str, "sess-1", "my-feature");

        let input = prompt_input("sess-1", cwd, "muda o campo status para enum");
        ChangeRequestLog.observe(&input, &ctx_for(cwd));

        let body = log_contents(cwd, "my-feature").expect("log file must exist");
        assert!(body.contains("muda o campo status para enum"), "got: {body}");
        assert!(body.contains("\"stage\":\"Execute\""), "stage recorded: {body}");

        // The human-readable change-log.md is documented beside the spec.
        let sp = ClaudePaths::for_project(cwd).unwrap().for_spec("my-feature").unwrap();
        let md = std::fs::read_to_string(sp.dir().join(CHANGE_LOG_MD))
            .expect("change-log.md must exist");
        assert!(md.contains("# Change Log — my-feature"), "md header: {md}");
        assert!(md.contains("muda o campo status para enum"), "md bullet: {md}");
    }

    /// A runtime notice is NOT a change request. Both markers are rejected, the
    /// log file is never even created, and a real request in the same session
    /// still lands — so the guard cannot be satisfied by recording nothing.
    #[test]
    fn skips_harness_notices_but_keeps_real_requests() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        seed_spec(cwd, "my-feature", "Active", "Execute");
        let cwd_str = cwd.to_string_lossy().into_owned();
        bind_session_spec(&cwd_str, "sess-1", "my-feature");

        for notice in [
            "[SYSTEM NOTIFICATION - NOT USER INPUT]\nThis is an automated background-task event.",
            "<task-notification>\n<task-id>abc</task-id>\n<status>completed</status>\n</task-notification>",
        ] {
            ChangeRequestLog.observe(&prompt_input("sess-1", cwd, notice), &ctx_for(cwd));
        }
        assert!(
            log_contents(cwd, "my-feature").is_none(),
            "a runtime notice must not even create the log"
        );

        // The two-sided half: a real request in the same session still records,
        // so the assertion above cannot pass by the observer being inert.
        ChangeRequestLog.observe(
            &prompt_input("sess-1", cwd, "troca o gate para warn"),
            &ctx_for(cwd),
        );
        let body = log_contents(cwd, "my-feature").expect("a real request still records");
        assert!(body.contains("troca o gate para warn"), "got: {body}");
        assert!(!body.contains("task-notification"), "no notice leaked in: {body}");
    }

    /// A log whose last line has no newline gets a separator, so the new bullet
    /// never fuses onto the previous entry. A log that already ends cleanly gets
    /// no blank line — the guard must not cost a gap on the normal path.
    #[test]
    fn appending_never_fuses_onto_an_unterminated_line() {
        for (seed, tail) in [("- **t1** — first", "no trailing newline"), ("- **t1** — first\n", "clean")] {
            let dir = tempdir().unwrap();
            let cwd = dir.path();
            seed_spec(cwd, "my-feature", "Active", "Execute");
            let sp = ClaudePaths::for_project(cwd).unwrap().for_spec("my-feature").unwrap();
            std::fs::create_dir_all(sp.dir()).unwrap();
            std::fs::write(sp.dir().join(CHANGE_LOG_MD), seed).unwrap();
            let cwd_str = cwd.to_string_lossy().into_owned();
            bind_session_spec(&cwd_str, "sess-1", "my-feature");

            ChangeRequestLog.observe(&prompt_input("sess-1", cwd, "segundo pedido"), &ctx_for(cwd));

            let md = std::fs::read_to_string(sp.dir().join(CHANGE_LOG_MD)).unwrap();
            assert!(
                !md.contains("first- **"),
                "the two entries fused ({tail} seed): {md}"
            );
            let bullets = md.lines().filter(|l| l.starts_with("- **")).count();
            assert_eq!(bullets, 2, "both entries must stay separate ({tail} seed): {md}");
        }
    }

    /// A TERMINAL spec is NOT recorded here — that is the amendment window's
    /// territory (post-close), so the two never double-record.
    #[test]
    fn skips_terminal_spec() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        seed_spec(cwd, "done-feature", "Completed", "Close");
        let cwd_str = cwd.to_string_lossy().into_owned();
        bind_session_spec(&cwd_str, "sess-2", "done-feature");

        let input = prompt_input("sess-2", cwd, "muda algo depois do close");
        ChangeRequestLog.observe(&input, &ctx_for(cwd));

        assert!(
            log_contents(cwd, "done-feature").is_none(),
            "terminal spec must not be recorded by this observer"
        );
    }

    /// No active spec bound to the session → nothing is recorded (a chat with no
    /// pipeline has nothing to attribute the request to).
    #[test]
    fn skips_when_no_spec_in_scope() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        // A spec exists on disk but the session is NOT bound to it and no
        // pipeline-state points at it → resolve_spec yields None.
        let input = prompt_input("sess-3", cwd, "qualquer coisa");
        ChangeRequestLog.observe(&input, &ctx_for(cwd));
        // Nothing created under .claude/spec.
        assert!(
            ClaudePaths::for_project(cwd)
                .ok()
                .map(|p| !p.claude_dir().join("spec").exists())
                .unwrap_or(true),
            "no spec dir should be created when no spec is in scope"
        );
    }

    /// A slash-command prompt (`/close`, `/qa`, …) is a control action, not a
    /// change request — it must not be recorded.
    #[test]
    fn skips_slash_command_prompts() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        seed_spec(cwd, "feat", "Active", "Execute");
        let cwd_str = cwd.to_string_lossy().into_owned();
        bind_session_spec(&cwd_str, "sess-5", "feat");
        let input = prompt_input("sess-5", cwd, "/close");
        ChangeRequestLog.observe(&input, &ctx_for(cwd));
        assert!(
            log_contents(cwd, "feat").is_none(),
            "slash-command must not be recorded as a change request"
        );
    }

    /// An empty prompt is ignored (no log line).
    #[test]
    fn ignores_empty_prompt() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        seed_spec(cwd, "feat", "Active", "Execute");
        let cwd_str = cwd.to_string_lossy().into_owned();
        bind_session_spec(&cwd_str, "sess-4", "feat");

        let input = prompt_input("sess-4", cwd, "   ");
        ChangeRequestLog.observe(&input, &ctx_for(cwd));

        assert!(log_contents(cwd, "feat").is_none(), "empty prompt must not record");
    }
}
