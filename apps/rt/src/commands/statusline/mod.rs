//! Compact session status: active spec and progress, model, context, and a
//! disabled-plugin warning. Detail and historical measurements live in Mods.
//! Reads the host JSON from stdin. Malformed input always falls back to Claude.

pub mod cli;
pub mod preview;
pub mod segment;
pub(crate) mod theme;

use segment::{Segment, context_segment, git_segment, inert_segment, model_segment, module_segment, unit_segment};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use theme::{Theme, ThemeId, render_line};

fn build_segments(data: &Value) -> Vec<Segment> {
    let cwd: PathBuf = data
        .get("workspace")
        .and_then(|w| w.get("current_dir"))
        .or_else(|| data.get("cwd"))
        .and_then(Value::as_str)
        .map_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), PathBuf::from);
    let lang = mustard_core::ProjectConfig::load(&cwd).language().text_or_default();
    // Naming the spec directly avoids duplicating it inside a long branch.
    let mut segments = if let Some(unit) = unit_segment(&cwd, None) {
        vec![unit]
    } else {
        let branch = mustard_core::current_branch(&cwd);
        let mut place = vec![module_segment(&cwd)];
        place.extend(git_segment(&cwd, branch.as_deref()));
        place
    };
    segments.push(model_segment(data));
    segments.extend(context_segment(data, lang));
    segments.extend(inert_segment(&cwd));
    segments
}

fn rows(theme: &Theme, segments: Vec<Segment>) -> Vec<String> {
    let line = render_line(theme, &segments);
    if line.is_empty() { Vec::new() } else { vec![line] }
}

pub fn run(preview: bool) {
    if preview {
        preview::run();
        return;
    }
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        println!("Claude");
        return;
    }
    match serde_json::from_str::<Value>(&input) {
        Ok(data) => {
            // Keep recording the host measurements for the detailed panel.
            crate::commands::panel::observe_statusline(&data);
            for line in rows(ThemeId::from_env().theme(), build_segments(&data)) {
                println!("{line}");
            }
        }
        Err(_) => println!("Claude"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::spec_state::seed_event;
    use serde_json::json;
    use std::path::Path;

    fn project(root: &Path, lang: &str) {
        std::fs::create_dir_all(root).unwrap();
        assert!(mustard_core::platform::git::run(root, &["init", "-q", "."]).ok);
        assert!(mustard_core::platform::git::run(root, &["symbolic-ref", "HEAD", "refs/heads/feature/checkout"]).ok);
        assert!(
            mustard_core::platform::git::run(
                root,
                &[
                    "-c",
                    "user.name=fixture",
                    "-c",
                    "user.email=fixture@example.com",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "fixture"
                ]
            )
            .ok
        );
        std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{lang}"}}}}"#)).unwrap();
    }

    fn payload(root: &Path) -> Value {
        json!({"cwd":root,"model":{"display_name":"Claude Sonnet 5.5"},
            "context_window":{"remaining_percentage":93},"version":"2.1.296",
            "cost":{"total_duration_ms":191_000,"total_cost_usd":12.5}})
    }

    #[test]
    fn active_spec_uses_one_row_with_a_clear_context_label_and_no_historical_noise() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        project(&root, "pt-BR");
        seed_event(&root, "checkout", "state", json!({"phase":"running","branch":"feature/checkout"}));
        let segments = build_segments(&payload(&root));
        let output = rows(ThemeId::Mustard.theme(), segments.clone());
        assert_eq!(output.len(), 1);
        let shown = &output[0];
        for value in ["checkout", "em execução", "Sonnet 5.5", "contexto 7%"] {
            assert!(shown.contains(value), "{shown}");
        }
        for absent in ["feature/checkout", "/mustard-panel", "Mustard 0.", "rtk poupou", "vs média", "191000", "$12.5"] {
            assert!(!shown.contains(absent), "{shown}");
        }
        assert!(segments.iter().all(|segment| matches!(
            segment.kind,
            segment::SegmentKind::Unit | segment::SegmentKind::Model | segment::SegmentKind::Context | segment::SegmentKind::Inert
        )));
    }

    #[test]
    fn no_active_spec_retains_project_and_branch_and_english_context() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        project(&root, "en-US");
        let line = rows(ThemeId::Mustard.theme(), build_segments(&payload(&root))).join("");
        for value in ["project", "feature/checkout", "context 7%", "Sonnet 5.5"] {
            assert!(line.contains(value), "{line}");
        }
    }

    #[test]
    fn missing_context_is_omitted_and_every_theme_renders_one_row() {
        let dir = tempfile::tempdir().unwrap();
        let data = json!({"cwd":dir.path(),"model":{"display_name":"Sonnet 5.5"}});
        let segments = build_segments(&data);
        assert!(!segments.iter().any(|segment| segment.kind == segment::SegmentKind::Context));
        for theme in ThemeId::ALL {
            let lines = rows(theme.theme(), segments.clone());
            assert_eq!(lines.len(), 1);
            assert!(!lines[0].contains('\n'));
        }
    }

    #[test]
    fn an_empty_segment_list_creates_no_blank_row() {
        assert!(rows(ThemeId::Mustard.theme(), Vec::new()).is_empty());
    }
}
