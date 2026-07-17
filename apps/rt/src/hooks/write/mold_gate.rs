//! `mold_gate` — ONE behavior: when a NEW file is being created whose kind
//! matches a `{role}-pattern` skill of its subproject, surface a non-blocking
//! advisory pointing at the mold, so the writer reads the house convention
//! BEFORE the first byte lands.
//!
//! ## Scope
//!
//! A `PreToolUse(Write)` gate. It fires ONLY on file CREATION (the target does
//! not exist yet) — that is the moment a mold matters most, and it bounds the
//! advisory to at most once per new module (no per-edit nagging, no session
//! state). Edits to existing files never fire. Together with the dispatch
//! prompt's `## SKILLS` shelf (before) and the review's mold contract (after),
//! this is the "during" hook of the skill-usage loop — it also covers the
//! paths the dispatch prompt cannot reach: the orchestrator's own direct edits
//! and a plain session with no pipeline at all.
//!
//! ## Matching
//!
//! The owning shelf is the NEAREST ancestor of the target carrying
//! `.claude/skills` (the same directory-scoping shape Claude Code uses to
//! offer skills). For each `*-pattern/SKILL.md` there, the cluster label comes
//! from the frontmatter `appliesTo[0]` (fallback: the last `-`-token of the
//! folder name); the target's filename stem is tokenized (separators + camel
//! humps) and matches when its FIRST or LAST token equals the label — token
//! boundaries, so `user-service.ts` never matches a `use` mold. Purely
//! lexical, deterministic, no model reads.
//!
//! ## Mode — `MUSTARD_MOLD_GATE_MODE` = `off` | `warn`
//!
//! Default `warn` (advisory). There is deliberately no `strict`: an unread
//! mold must never BLOCK a write — the mold may not fit and the writer may
//! know better; enforcement of the mold's content belongs to REVIEW, not to a
//! filename heuristic.
//!
//! ## Fail-open
//!
//! Every failure path — no file path, unreadable dirs/frontmatter, target
//! outside the project, anything under `.claude/` — degrades to
//! [`Verdict::Allow`]. Never panics (crate-wide `unwrap` deny).

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;
use std::path::{Path, PathBuf};

/// The mold-advisory gate module.
pub struct MoldGate;

/// Output mode of the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoldMode {
    /// Disabled — pure no-op.
    Off,
    /// Surface the advisory as a non-blocking warning (default).
    Warn,
}

/// Read `MUSTARD_MOLD_GATE_MODE` (default `warn`). Anything but `off`
/// (including unset) is `warn` — there is no blocking mode by design.
fn mold_mode() -> MoldMode {
    match std::env::var("MUSTARD_MOLD_GATE_MODE").unwrap_or_default().to_ascii_lowercase().as_str() {
        "off" => MoldMode::Off,
        _ => MoldMode::Warn,
    }
}

impl Check for MoldGate {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Defensive: only PreToolUse(Write) should reach us.
        if ctx.trigger != Some(Trigger::PreToolUse)
            || input.tool_name.as_deref() != Some("Write")
        {
            return Ok(Verdict::Allow);
        }
        if mold_mode() == MoldMode::Off {
            return Ok(Verdict::Allow);
        }
        Ok(advise(input, ctx).unwrap_or(Verdict::Allow))
    }
}

/// The gate's real logic — `None` means pass-through (fail-open).
fn advise(input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    let fp = super::boundary_gate::file_path_of(input)?;
    let project = ctx.project_dir_or_cwd(input);
    // Repo work only, and never the harness's own tree (`.claude/` holds the
    // molds themselves, specs, plans — none of that is a module).
    let rel = super::boundary_gate::relative_to_cwd(&project, &fp)?;
    if rel.split('/').any(|seg| seg == ".claude") {
        return None;
    }
    let target = Path::new(&fp);
    let abs = if target.is_absolute() {
        target.to_path_buf()
    } else {
        Path::new(&project).join(target)
    };
    // Creation only: an existing file is an edit — the mold moment has passed.
    if abs.exists() {
        return None;
    }

    let stem_tokens = filename_tokens(abs.file_stem()?.to_string_lossy().as_ref());
    let (first, last) = (stem_tokens.first()?, stem_tokens.last()?);

    let shelf = nearest_shelf(abs.parent()?, Path::new(&project))?;
    let mut hits: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(&shelf).ok()?.flatten() {
        let folder = entry.file_name().to_string_lossy().into_owned();
        if !folder.ends_with("-pattern") {
            continue;
        }
        let skill_md = entry.path().join("SKILL.md");
        let Ok(text) = std::fs::read_to_string(&skill_md) else {
            continue;
        };
        let label = mold_label(&folder, &text);
        if label.is_empty() {
            continue;
        }
        if *first == label || *last == label {
            hits.push((folder, skill_md));
        }
    }
    if hits.is_empty() {
        return None;
    }
    hits.sort();
    let listed = hits
        .iter()
        .take(2)
        .map(|(name, path)| format!("`{name}` ({})", path.display()))
        .collect::<Vec<_>>()
        .join(", ");
    Some(Verdict::Warn {
        message: format!(
            "[mold] This subproject has a mold for this kind of file: {listed}. Read the \
             SKILL.md and follow it before writing — deviations are review findings. \
             (MUSTARD_MOLD_GATE_MODE=off to silence.)"
        ),
    })
}

/// The nearest ancestor of `start` (inclusive), bounded by `project`, that
/// carries a `.claude/skills` shelf. `None` when no ancestor inside the
/// project has one.
fn nearest_shelf(start: &Path, project: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let shelf = d.join(".claude").join("skills");
        if shelf.is_dir() {
            return Some(shelf);
        }
        if d == project {
            return None;
        }
        dir = d.parent();
    }
    None
}

/// The cluster label a mold targets: frontmatter `appliesTo[0]` (lowercased)
/// when present, else the last `-`-token of the folder name minus the
/// `-pattern` suffix (`dataaccess-log-pattern` → `log`).
fn mold_label(folder: &str, skill_md: &str) -> String {
    if let Ok(fm) = mustard_core::domain::skill::frontmatter::parse(skill_md) {
        if let Some(first) = fm.applies_to.first() {
            let label = first.trim().to_ascii_lowercase();
            // Only a bare cluster label works as a token; a glob/path-style
            // appliesTo (hand-authored molds) falls back to the folder name.
            if !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric()) {
                return label;
            }
        }
    }
    folder
        .strip_suffix("-pattern")
        .unwrap_or(folder)
        .rsplit('-')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Tokenize a filename stem: split on `-`/`_`/`.`/spaces AND camel humps,
/// lowercased (`GatewayLog` → `[gateway, log]`; `form-context` →
/// `[form, context]`). Token-boundary matching keeps `user-service` from
/// matching a `use` mold.
fn filename_tokens(stem: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = stem.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c == '-' || c == '_' || c == '.' || c == ' ' {
            if !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if c.is_uppercase() && !cur.is_empty() && chars.get(i - 1).is_some_and(|p| p.is_lowercase()) {
            tokens.push(std::mem::take(&mut cur));
        }
        cur.push(c.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_input(root: &str, file_path: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_path, "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(root.to_string()),
            session_id: Some("sess-mold".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: root.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    fn seed_mold(root: &Path, subproject: &str, slug: &str, label: &str) {
        let dir = root.join(subproject).join(".claude/skills").join(format!("{slug}-pattern"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {slug}-pattern\ndescription: \"Use when adding a {label}.\"\nappliesTo: [{label}]\nsource: scan\n---\nbody\n"),
        )
        .unwrap();
    }

    #[test]
    fn new_file_matching_mold_warns_with_the_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        seed_mold(root, "apps/api", "api-log", "log");
        std::fs::create_dir_all(root.join("apps/api/src")).unwrap();

        let target = root.join("apps/api/src/GatewayLog.cs");
        let (input, ctx) = write_input(root.to_str().unwrap(), target.to_str().unwrap());
        let verdict = MoldGate.evaluate(&input, &ctx).expect("no error");
        match verdict {
            Verdict::Warn { message } => {
                assert!(message.contains("api-log-pattern"), "advisory names the mold: {message}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn token_boundary_blocks_false_positives() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        seed_mold(root, "apps/app", "app-use", "use");
        std::fs::create_dir_all(root.join("apps/app/src")).unwrap();

        // `user-service` must NOT match the `use` mold (no `use` token).
        let target = root.join("apps/app/src/user-service.ts");
        let (input, ctx) = write_input(root.to_str().unwrap(), target.to_str().unwrap());
        assert!(matches!(MoldGate.evaluate(&input, &ctx).expect("no error"), Verdict::Allow));

        // `use-debounce` DOES match (first token).
        let target = root.join("apps/app/src/use-debounce.ts");
        let (input, ctx) = write_input(root.to_str().unwrap(), target.to_str().unwrap());
        assert!(matches!(MoldGate.evaluate(&input, &ctx).expect("no error"), Verdict::Warn { .. }));
    }

    #[test]
    fn existing_file_and_claude_paths_pass_through() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        seed_mold(root, "apps/api", "api-log", "log");
        std::fs::create_dir_all(root.join("apps/api/src")).unwrap();

        // Existing file → edit, not creation → Allow.
        let existing = root.join("apps/api/src/AuditLog.cs");
        std::fs::write(&existing, "old").unwrap();
        let (input, ctx) = write_input(root.to_str().unwrap(), existing.to_str().unwrap());
        assert!(matches!(MoldGate.evaluate(&input, &ctx).expect("no error"), Verdict::Allow));

        // A path under .claude/ (authoring a mold itself) → Allow.
        let mold = root.join("apps/api/.claude/skills/api-log-pattern/SKILL.md");
        let (input, ctx) = write_input(root.to_str().unwrap(), mold.to_str().unwrap());
        assert!(matches!(MoldGate.evaluate(&input, &ctx).expect("no error"), Verdict::Allow));
    }

    #[test]
    fn no_shelf_or_off_mode_is_a_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("apps/bare/src")).unwrap();
        let target = root.join("apps/bare/src/GatewayLog.cs");
        let (input, ctx) = write_input(root.to_str().unwrap(), target.to_str().unwrap());
        assert!(matches!(MoldGate.evaluate(&input, &ctx).expect("no error"), Verdict::Allow));
    }

    #[test]
    fn filename_tokens_split_separators_and_camel() {
        assert_eq!(filename_tokens("GatewayLog"), vec!["gateway", "log"]);
        assert_eq!(filename_tokens("form-context"), vec!["form", "context"]);
        assert_eq!(filename_tokens("user_service"), vec!["user", "service"]);
        assert_eq!(filename_tokens("config"), vec!["config"]);
    }

    #[test]
    fn mold_label_prefers_applies_to_and_falls_back_to_folder() {
        assert_eq!(
            mold_label("x-y-pattern", "---\nname: x\nappliesTo: [log]\n---\n"),
            "log"
        );
        // Glob-style appliesTo (hand mold) → folder fallback.
        assert_eq!(
            mold_label("dataaccess-rule-pattern", "---\nname: x\nappliesTo: [\"a/b/*.cs\"]\n---\n"),
            "rule"
        );
        assert_eq!(mold_label("api-service-pattern", "no frontmatter"), "service");
    }
}
