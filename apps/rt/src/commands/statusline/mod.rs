//! `mustard-rt run statusline` — render the Claude Code status bar.
//!
//! Reads the harness payload JSON from stdin and prints the bar on stdout.
//! On any failure (bad JSON, missing fields, panicking I/O) we print
//! `Claude` and exit cleanly — the harness must never see a non-zero exit.
//!
//! O que a barra mostra: na primeira linha, o nome do projeto como link da
//! página dele, a branch, a spec como link da página dela, a fase e o
//! andamento das ondas como contagem ("1 de 4 ondas", entregues de total), o
//! uso da conversa, o tempo, as linhas
//! mudadas, o custo e o aviso vermelho de quando o Mustard está desligado; na
//! segunda, a economia do rtk e o modelo. Nenhuma versão aparece: a do
//! Mustard vai para o `doctor` e para o aviso do início da sessão, e a do
//! Claude Code o próprio Claude Code já mostra.
//!
//! Submodules:
//! - [`segment`] — pure data ([`segment::Segment`]) and per-kind builders.
//! - [`theme`]   — palette / separator / `render_line`.
//! - [`preview`] — handler for the `--preview` flag.
//!
//! Theme selection: see [`theme::ENV_VAR`]. Default = `catppuccin` (powerline,
//! requires a Nerd Font). Users without Nerd Font set
//! `MUSTARD_STATUSLINE_THEME=default`.

pub mod cli;

pub mod preview;
pub mod segment;
// `theme` stays crate-internal: the module became `pub` when the `run` CLI
// split moved `StatuslineCmd` into `statusline::cli`, and its public items
// (`ThemeId::theme`, `render_line`, `DEFAULT`) hand out the crate-private
// `Theme` type - capping the module keeps that honest without leaking it.
pub(crate) mod theme;

use segment::{
    context_segment, cost_segment, diff_segment, duration_segment, git_segment, inert_segment, model_segment,
    module_segment, savings_segment, unit_segment, Segment,
};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use theme::{render_line, ThemeId};

/// Build the ordered segment list from the parsed payload.
///
/// Builders that return `None` (zero duration, no `total_cost_usd`, etc.) are
/// quietly skipped, so the line stays compact when state is sparse.
fn build_segments(data: &Value) -> Vec<Segment> {
    let cwd: PathBuf = data
        .get("workspace")
        .and_then(|w| w.get("current_dir"))
        .or_else(|| data.get("cwd"))
        .and_then(Value::as_str)
        .map_or_else(
            || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            PathBuf::from,
        );

    // A primeira linha: onde o trabalho está e o que a sessão gastou.
    let mut segs = vec![module_segment(&cwd)];
    segs.extend(git_segment(&cwd));
    segs.extend(unit_segment(&cwd));
    segs.extend(context_segment(data));
    segs.extend(duration_segment(data));
    segs.extend(diff_segment(data));
    segs.extend(cost_segment(data));
    // Vermelho: o plugin está desligado, e nenhuma trava roda. Sem ele, esse
    // estado parece com um saudável em todo o resto da barra.
    segs.extend(inert_segment(&cwd));
    // A segunda linha: a economia do rtk e o modelo.
    segs.extend(savings_segment());
    segs.push(model_segment(data));
    segs
}

/// A linha de cada segmento: a economia do rtk e o modelo vão para a
/// segunda; todo o resto, para a primeira.
///
/// O Claude Code mostra uma linha por linha impressa (documentado), então são
/// dois `println!`, e não um truque de desenho. Uma linha sem segmento nenhum
/// não é impressa.
const fn is_place_row(kind: segment::SegmentKind) -> bool {
    use segment::SegmentKind as K;
    !matches!(kind, K::Savings | K::Model)
}

/// Render the statusline from a parsed payload: one row per non-empty group,
/// no trailing newline. A row whose segments are all absent is dropped rather
/// than printed blank — with a sparse payload the bar stays a single line, the
/// shape it had before this split.
fn render(data: &Value) -> Vec<String> {
    let theme = ThemeId::from_env().theme();
    let (place, spend): (Vec<Segment>, Vec<Segment>) =
        build_segments(data).into_iter().partition(|s| is_place_row(s.kind));

    [place, spend]
        .into_iter()
        .filter(|group| !group.is_empty())
        .map(|group| render_line(theme, &group))
        .filter(|line| !line.is_empty())
        .collect()
}

/// Dispatch `mustard-rt run statusline`.
///
/// `preview = true` short-circuits to the [`preview`] handler (no stdin read,
/// no JSON parse). `preview = false` does the live render.
pub fn run(preview: bool) {
    if preview {
        preview::run();
        return;
    }
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        println!("Claude");
        return;
    }
    match serde_json::from_str::<Value>(&buf) {
        Ok(data) => {
            for line in render(&data) {
                println!("{line}");
            }
        }
        Err(_) => println!("Claude"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A full payload renders TWO rows, split by role.
    ///
    /// This replaces a test that pinned the bar to one row. That pin recorded a
    /// 2026-05-21 decision to drop a pipeline BANNER — a second line that
    /// repeated state the bar already showed. What is here now is not that: the
    /// same segments, redistributed, because one row had grown to 171 visible
    /// characters and terminals cut the tail. Claude Code documents one row per
    /// printed line, so two rows is a supported layout, not a workaround.
    #[test]
    fn a_full_payload_renders_two_rows_split_by_role() {
        let data = json!({
            "workspace": { "current_dir": ".", "project_dir": "." },
            "model": { "display_name": "Opus 4.7" },
            "version": "2.1.146",
            "cost": {
                "total_duration_ms": 1000,
                "total_lines_added": 10,
                "total_lines_removed": 2,
                "total_cost_usd": 0.42
            },
            "context_window": {
                "remaining_percentage": 70,
                "total_input_tokens": 50000,
                "total_output_tokens": 10000
            }
        });
        let lines = render(&data);
        assert_eq!(lines.len(), 2, "place row + spend row — got {lines:?}");
        assert!(lines.iter().all(|l| !l.is_empty()), "no blank row is printed: {lines:?}");

        let (first, second) = (&lines[0], &lines[1]);
        assert!(first.contains("$0.42") && first.contains("70%"), "cost and context go on the first row: {first}");
        assert!(second.contains("Opus 4.7") && !second.contains("$0.42"), "the model goes on the second: {second}");
        assert!(!first.contains("2.1.146") && !second.contains("2.1.146"), "no version: {lines:?}");
    }

    /// O texto que o terminal mostra: a sequência do link sai, o rótulo fica.
    fn visible(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find('\u{1b}') {
            out.push_str(&rest[..start]);
            let tail = &rest[start..];
            let end = if tail.starts_with("\u{1b}]8;") {
                tail.find("\u{1b}\\").map(|i| i + 2)
            } else {
                tail.find('m').map(|i| i + 1)
            };
            let Some(end) = end else { return out };
            rest = &tail[end..];
        }
        out.push_str(rest);
        out
    }

    /// Retrato da barra com uma spec em execução na onda 2 de 4: aparecem o
    /// link da página do projeto, a branch, o nome da spec como link, a fase e
    /// o andamento como contagem — "1 de 4 ondas", uma entregue das quatro, nos
    /// dois idiomas, e nunca o número da onda que vem; nenhuma versão aparece —
    /// nem a do Mustard, nem a do Claude Code.
    #[test]
    fn a_session_with_a_spec_running_wave_two_of_four_shows_the_links_the_phase_and_the_progress() {
        use crate::shared::spec_state::seed_event;
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("loja");
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| assert!(mustard_core::platform::git::run(&root, args).ok, "git {args:?}");
        git(&["init", "-q", "."]);
        git(&["symbolic-ref", "HEAD", "refs/heads/feature/checkout"]);
        git(&["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false",
            "commit", "-q", "--allow-empty", "-m", "semente"]);
        std::fs::write(root.join("mustard.json"), r#"{"version":"0.0.1-velha","language":{"text":"pt-BR"}}"#).unwrap();
        let project_url = "https://claude.ai/code/artifacts/projeto-loja";
        let spec_url = "https://claude.ai/code/artifacts/spec-checkout";
        let said = seed_event(&root, "checkout", "message", json!({"author": "user", "text": "o plano"}));
        seed_event(&root, "checkout", "state", json!({"phase": "plan", "branch": "feature/checkout", "base": "dev"}));
        for n in 1..=4 {
            seed_event(&root, "checkout", "wave", json!({"n": n, "text": format!("Onda {n}."), "criteria": [said],
                "done_when": "x", "origin": said}));
        }
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("checkout"));
        seed_event(&root, "checkout", "state", json!({"phase": "running", "author": "binary"}));
        seed_event(&root, "checkout", "delivered", json!({"wave": 1, "text": "Pronta.", "files": ["a.rs"], "author": "wave"}));
        seed_event(&root, "checkout", "publish",
            json!({"page": "spec", "milestone": "round", "ok": true, "url": spec_url}));
        std::fs::write(
            root.join(".claude").join("spec").join("index.ndjson"),
            format!("{}\n", mustard_core::domain::spec_index::project_line(Some(project_url))),
        )
        .unwrap();

        let data = serde_json::json!({
            "workspace": { "current_dir": root.to_string_lossy() },
            "model": { "display_name": "Claude Opus 5" },
            "version": "2.1.267",
        });
        let segs = build_segments(&data);
        let text = |kind: segment::SegmentKind| {
            segs.iter().find(|s| s.kind == kind).map(|s| s.text.clone()).unwrap_or_else(|| panic!("no {kind:?}: {segs:?}"))
        };
        let link = |url: &str, label: &str| format!("\u{1b}]8;;{url}\u{1b}\\{label}\u{1b}]8;;\u{1b}\\");
        assert_eq!(text(segment::SegmentKind::Module), link(project_url, "loja"), "the project page link");
        assert!(text(segment::SegmentKind::Git).starts_with("\u{2387} feature/checkout"), "the branch");
        assert_eq!(
            text(segment::SegmentKind::Unit),
            format!("\u{25b8} {} running 1 de 4 ondas", link(spec_url, "checkout")),
            "the spec name as a link, the phase and the count of delivered waves"
        );

        let lines: Vec<String> = render(&data).iter().map(|line| visible(line)).collect();
        let shown = lines.join("\n");
        for expected in ["loja", "feature/checkout", "checkout running 1 de 4 ondas", "Opus 5"] {
            assert!(shown.contains(expected), "{expected} is on the bar: {shown}");
        }
        assert!(!shown.contains("onda 2"), "the number of the next wave stays in the resume line: {shown}");
        for version in ["2.1.267", "v2.1", "0.0.1-velha", &format!("m{}", mustard_core::harness_version())] {
            assert!(!shown.contains(version), "no version on the bar ({version}): {shown}");
        }
        assert!(!shown.contains('\u{2702}'), "no branch count to prune: {shown}");

        // Em inglês, a mesma contagem.
        std::fs::write(root.join("mustard.json"), r#"{"version":"0.0.1-velha","language":{"text":"en-US"}}"#).unwrap();
        let english = segment::unit_segment(&root).expect("the spec is on the bar");
        assert!(english.text.ends_with(" running 1 of 4 waves"), "{}", english.text);
    }

    /// Every segment kind lands on exactly one row — a kind added later without
    /// a home would silently vanish from the bar, which is the failure mode a
    /// partition invites.
    #[test]
    fn every_rendered_segment_reaches_a_row() {
        let data = json!({
            "workspace": { "current_dir": ".", "project_dir": "." },
            "model": { "display_name": "Opus 4.7" },
            "version": "2.1.146",
            "cost": { "total_cost_usd": 0.42, "total_duration_ms": 1000 }
        });
        let built = build_segments(&data);
        let placed = built.iter().filter(|s| is_place_row(s.kind)).count();
        let spent = built.iter().filter(|s| !is_place_row(s.kind)).count();
        assert_eq!(placed + spent, built.len(), "the partition is total by construction");
        assert!(placed > 0 && spent > 0, "a full payload feeds both rows");
    }

    /// A sparse payload keeps the bar at ONE row: an empty group is dropped, not
    /// printed blank. This is what the split must not cost — a fresh session
    /// with no numbers yet should look exactly as it did before.
    #[test]
    fn render_falls_back_to_one_row_with_minimal_payload() {
        let lines = render(&json!({ "model": { "id": "claude-opus" } }));
        assert!(!lines.is_empty(), "the bar never disappears");
        assert!(lines.iter().all(|l| !l.is_empty()), "no blank row: {lines:?}");
    }

    #[test]
    fn build_segments_includes_cost_when_present() {
        let data = json!({ "cost": { "total_cost_usd": 0.42 } });
        let segs = build_segments(&data);
        assert!(segs.iter().any(|s| s.kind == segment::SegmentKind::Cost));
    }

    #[test]
    fn build_segments_skips_cost_when_zero() {
        let data = json!({ "cost": { "total_cost_usd": 0.0 } });
        let segs = build_segments(&data);
        assert!(!segs.iter().any(|s| s.kind == segment::SegmentKind::Cost));
    }
}
