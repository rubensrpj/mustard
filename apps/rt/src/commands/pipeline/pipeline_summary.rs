//! `mustard-rt run pipeline-summary` — a port of `scripts/pipeline-summary.js`.
//!
//! Renders a "Done / Left / Next Steps / Manual Follow-ups" summary for a spec
//! at CLOSE. Reads `<spec-dir>/spec.md` (required) and the optional
//! `.claude/.pipeline-states/<basename>.json` (fail-open).
//!
//! `--format markdown` (default) prints the rendered summary; `--format json`
//! prints `{ done, left, nextSteps, followUps }`. No `--format html`.

use crate::commands::spec::spec_sections::is_heading;
use mustard_core::fs;
use mustard_core::spec;
use mustard_core::summary::SpecSummaryDoc;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Lifecycle status word + spec name + language for the summary header line.
struct Header {
    status: String,
    name: String,
    lang: String,
}

/// Parse the spec header. The lifecycle status is resolved through the
/// canonical [`mustard_core::spec`] parser (so the new `### Stage:` header
/// and every legacy shape both work); `Lang` and the `# Title` are read inline
/// since they are not part of the lifecycle header domain. An absent lifecycle
/// header yields an empty status (rendered as `unknown` downstream, unchanged).
fn parse_header(text: &str) -> Header {
    let status = spec::parse_state(text)
        .map(|s| spec::status_word(&s).to_string())
        .unwrap_or_default();
    let mut lang = "en-US".to_string();
    let mut name = "spec".to_string();
    for line in text.split('\n') {
        let t = line.trim_end();
        if let Some(v) = t.strip_prefix("### Lang:") {
            let first = v.trim().to_lowercase();
            let tok = first.split([' ', '|', '\t']).next().unwrap_or("en-US");
            // Tolerant read: accept legacy short forms and BCP-47.
            lang = if tok == "pt" || tok == "pt-br" {
                "pt-BR".to_string()
            } else {
                "en-US".to_string()
            };
        } else if name == "spec" {
            if let Some(v) = t.strip_prefix("# ") {
                if !v.starts_with('#') {
                    name = v.trim().to_string();
                }
            }
        }
    }
    Header { status, name, lang }
}

/// Cut a `## ` section body (heading line excluded) for a canonical key.
fn section_for(text: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines.iter().position(|l| is_heading(l, key))?;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("## ") {
            end = i;
            break;
        }
    }
    Some(lines[start + 1..end].join("\n"))
}

/// One parsed AC item.
struct Ac {
    done: bool,
    id: String,
    text: String,
    command: Option<String>,
}

/// Parse AC lines: `- [ ] AC-N: text — Command: `cmd``.
fn parse_ac(section: &str) -> Vec<Ac> {
    let mut out = Vec::new();
    for raw in section.split('\n') {
        let t = raw.trim_start();
        let Some(rest) = t.strip_prefix("- [") else { continue };
        let mark = rest.chars().next();
        let Some(mark) = mark else { continue };
        if !matches!(mark, ' ' | 'x' | 'X') {
            continue;
        }
        let Some(rest) = rest[mark.len_utf8()..].strip_prefix("] ") else { continue };
        let lower = rest.to_lowercase();
        if !lower.starts_with("ac-") {
            continue;
        }
        let after = &rest[3..];
        let digits_end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
        if digits_end == 0 {
            continue;
        }
        let id = format!("AC-{}", &after[..digits_end]);
        let Some(body) = after[digits_end..].trim_start().strip_prefix(':') else { continue };
        let body = body.trim();
        // Optional `— Command: `cmd``.
        let (text, command) = match body.find("— Command:") {
            Some(idx) => {
                let cmd_part = body[idx + "— Command:".len()..].trim();
                let cmd = cmd_part.trim_matches('`').trim();
                (
                    body[..idx].trim().to_string(),
                    if cmd.is_empty() { None } else { Some(cmd.to_string()) },
                )
            }
            None => (body.to_string(), None),
        };
        out.push(Ac { done: mark == 'x' || mark == 'X', id, text, command });
    }
    out
}

/// Parse non-checkbox bullet lines.
fn parse_bullets(section: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in section.split('\n') {
        let t = raw.trim_start();
        if let Some(rest) = t.strip_prefix("- ") {
            let rest = rest.trim();
            // Strip a `[ ]` / `[x]` checkbox prefix.
            let stripped = if rest.starts_with("[ ]") || rest.starts_with("[x]") || rest.starts_with("[X]") {
                rest[3..].trim()
            } else {
                rest
            };
            if !stripped.is_empty() {
                out.push(stripped.to_string());
            }
        }
    }
    out
}

/// Count `- [ ]` / `- [x]` checkbox items in a section.
fn parse_checklist(section: &str) -> (usize, usize) {
    let (mut total, mut done) = (0, 0);
    for raw in section.split('\n') {
        let t = raw.trim_start();
        if let Some(rest) = t.strip_prefix("- [") {
            if let Some(mark) = rest.chars().next() {
                if matches!(mark, ' ' | 'x' | 'X') {
                    total += 1;
                    if mark == 'x' || mark == 'X' {
                        done += 1;
                    }
                }
            }
        }
    }
    (total, done)
}

/// Parse file paths from a `## Files` section's bullets.
fn parse_files(section: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in section.split('\n') {
        let t = raw.trim_start();
        if let Some(rest) = t.strip_prefix("- ") {
            let token = rest.trim().trim_matches('`');
            let token = token.split_whitespace().next().unwrap_or("");
            let token = token.trim_matches('`');
            if !token.is_empty() && token.chars().any(|c| matches!(c, '\\' | '/' | '.')) {
                out.push(token.to_string());
            }
        }
    }
    out
}

/// Derive manual follow-ups from touched file paths.
fn follow_ups_from_files(files: &[String], pt: bool) -> Vec<String> {
    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |key: &str, msg: String| {
        if seen.insert(key.to_string()) {
            hits.push(msg);
        }
    };
    for f in files {
        let lower = f.to_lowercase();
        if lower.contains(".env") || lower.contains("/env/") || lower.starts_with("env/") {
            add("env", if pt {
                "Adicionar novas variáveis em `.env.example` + cofre de secrets".to_string()
            } else {
                "Add new env vars to `.env.example` + secret manager".to_string()
            });
        }
        if lower.contains("migration") || lower.ends_with(".sql") {
            add("migration", if pt {
                "Rodar migration em staging antes de prod".to_string()
            } else {
                "Run migration on staging before prod".to_string()
            });
        }
        if lower.contains(".schema.") || lower.ends_with("schema.rs") || lower.ends_with(".prisma") {
            add("schema", if pt {
                "Regerar tipos do ORM / atualizar entity-registry".to_string()
            } else {
                "Regenerate ORM types / refresh entity-registry".to_string()
            });
        }
        if lower.contains("docker-compose") {
            add("docker", if pt {
                "Rebuildar containers locais antes de pushar".to_string()
            } else {
                "Rebuild containers locally before pushing".to_string()
            });
        }
    }
    hits
}

/// The rendered model.
struct Model {
    done: Vec<String>,
    left: Vec<String>,
    next_steps: Vec<String>,
    follow_ups: Vec<String>,
}

/// Format a state item (string or `{ id, reason }`).
fn format_state_item(item: &Value) -> String {
    match item {
        Value::String(s) => s.clone(),
        Value::Object(_) => {
            let id = item.get("id").and_then(Value::as_str);
            let reason = item.get("reason").and_then(Value::as_str);
            match (id, reason) {
                (Some(i), Some(r)) => format!("{i}: {r}"),
                (None, Some(r)) => r.to_string(),
                (Some(i), None) => i.to_string(),
                _ => item.to_string(),
            }
        }
        other => other.to_string(),
    }
}

/// Build the Done/Left/Next/Follow-ups model.
fn build_model(header: &Header, text: &str, state: &Value) -> Model {
    let pt = header.lang == "pt-BR";
    let ac_list = section_for(text, "acceptanceCriteria").map(|s| parse_ac(&s)).unwrap_or_default();
    let ac_done: Vec<&Ac> = ac_list.iter().filter(|a| a.done).collect();
    let ac_failed: Vec<&Ac> = ac_list.iter().filter(|a| !a.done).collect();
    let concerns = section_for(text, "concerns").map(|s| parse_bullets(&s)).unwrap_or_default();
    let checklist = section_for(text, "tasks").map_or((0, 0), |s| parse_checklist(&s));
    let files = section_for(text, "files").map(|s| parse_files(&s)).unwrap_or_default();

    // Done lines.
    let mut done = vec![format!(
        "Spec: {} (Status: {})",
        header.name,
        if header.status.is_empty() { "unknown" } else { &header.status }
    )];
    if checklist.0 > 0 {
        done.push(if pt {
            format!("Checklist: {}/{} passos completos", checklist.1, checklist.0)
        } else {
            format!("Checklist: {}/{} steps completed", checklist.1, checklist.0)
        });
    }
    if !ac_list.is_empty() {
        done.push(if pt {
            format!("AC aprovados: {}/{}", ac_done.len(), ac_list.len())
        } else {
            format!("AC passed: {}/{}", ac_done.len(), ac_list.len())
        });
    }
    if !files.is_empty() {
        done.push(if pt {
            format!("Arquivos tocados: {}", files.len())
        } else {
            format!("Files touched: {}", files.len())
        });
    }

    // Left lines.
    let mut left = Vec::new();
    for ac in &ac_failed {
        let suffix = ac.command.as_ref().map(|c| format!(" — Command: `{c}`")).unwrap_or_default();
        left.push(format!("{}: {}{}", ac.id, ac.text, suffix));
    }
    for c in &concerns {
        left.push(format!("Concern: {c}"));
    }
    let arr = |key: &str| -> Vec<Value> {
        state.get("metrics").and_then(|m| m.get(key)).and_then(Value::as_array).cloned().unwrap_or_default()
    };
    for d in arr("deferred") {
        left.push(format!("Deferred: {}", format_state_item(&d)));
    }
    for p in arr("partial") {
        left.push(format!("Partial: {}", format_state_item(&p)));
    }
    for e in state.get("escalations").and_then(Value::as_array).cloned().unwrap_or_default() {
        left.push(format!("Escalation: {}", format_state_item(&e)));
    }

    // Next Steps.
    let mut next_steps = Vec::new();
    if let Some(first) = ac_failed.first() {
        let cmd = first.command.as_ref().map(|c| format!(": `{c}`")).unwrap_or_default();
        next_steps.push(if pt {
            format!("Rerodar AC reprovado ({}){}", first.id, cmd)
        } else {
            format!("Rerun failing AC ({}){}", first.id, cmd)
        });
    }
    if !concerns.is_empty() {
        next_steps.push(if pt {
            "Resolver concerns acumulados antes de fechar".to_string()
        } else {
            "Resolve outstanding concerns before closing".to_string()
        });
    }
    if ac_failed.is_empty() && concerns.is_empty() {
        let happy = if pt {
            [
                "Rodar `git add` nos arquivos modificados",
                "Criar commit (`git commit -m \"...\"`)",
                "Push para o remoto (`git push`)",
                "Abrir PR e solicitar review",
            ]
        } else {
            [
                "Run `git add` on modified files",
                "Create commit (`git commit -m \"...\"`)",
                "Push to remote (`git push`)",
                "Open PR and request review",
            ]
        };
        next_steps.extend(happy.iter().map(|s| s.to_string()));
    } else {
        next_steps.push(if pt {
            "Revalidar suite local (`bun test` ou comando do projeto)".to_string()
        } else {
            "Re-run local test suite (`bun test` or project command)".to_string()
        });
        next_steps.push(if pt {
            "Atualizar checklist na spec após cada correção".to_string()
        } else {
            "Update spec checklist after each fix".to_string()
        });
    }
    next_steps.truncate(5);

    let follow_ups = follow_ups_from_files(&files, pt);
    Model { done, left, next_steps, follow_ups }
}

/// Render the model as markdown.
fn render(model: &Model, pt: bool) -> String {
    let (l_done, l_left, l_next, l_follow, l_nothing) = if pt {
        ("## Feito", "## Falta", "## Próximos Passos", "## Follow-ups Manuais", "Nada pendente.")
    } else {
        ("## What's Done", "## What's Left", "## Next Steps", "## Manual Follow-ups", "Nothing pending.")
    };
    let mut out = Vec::new();
    out.push(l_done.to_string());
    for line in &model.done {
        out.push(format!("- {line}"));
    }
    out.push(String::new());

    out.push(l_left.to_string());
    if model.left.is_empty() {
        out.push(format!("- {l_nothing}"));
    } else {
        for line in &model.left {
            out.push(format!("- {line}"));
        }
    }
    out.push(String::new());

    out.push(l_next.to_string());
    if model.next_steps.is_empty() {
        out.push(format!("1. {l_nothing}"));
    } else {
        for (i, s) in model.next_steps.iter().enumerate() {
            out.push(format!("{}. {s}", i + 1));
        }
    }
    out.push(String::new());

    if !model.follow_ups.is_empty() {
        out.push(l_follow.to_string());
        for line in &model.follow_ups {
            out.push(format!("- {line}"));
        }
        out.push(String::new());
    }
    let joined = out.join("\n");
    format!("{}\n", joined.trim_end())
}

/// Dispatch `mustard-rt run pipeline-summary`.
///
/// When `self_test` is `true`, instantiates a minimal [`SpecSummaryDoc`],
/// serialises it with `serde_json::to_string_pretty`, prints to stdout, and
/// returns immediately (exit 0). Used by AC-1A-1 to verify the summary
/// foundation compiles and the `version` field is numeric.
pub fn run(spec_dir: Option<&str>, format: &str, self_test: bool) {
    if self_test {
        let doc = SpecSummaryDoc {
            version: 1,
            spec: "self-test".into(),
            title: "Self-test".into(),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&doc)
            .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
        println!("{json}");
        return;
    }

    let Some(spec_dir) = spec_dir else {
        eprintln!("pipeline-summary: missing --spec-dir flag");
        std::process::exit(1);
    };
    let spec_file = Path::new(spec_dir).join("spec.md");
    let text = match fs::read_to_string(&spec_file) {
        Ok(t) => t,
        Err(err) => {
            eprintln!("pipeline-summary: cannot read {}: {err}", spec_file.display());
            std::process::exit(1);
        }
    };
    let header = parse_header(&text);

    // pipeline-state (fail-open).
    let mut state = json!({});
    let spec_base = Path::new(spec_dir)
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| {
            Path::new(spec_dir)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        });
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let state_file = ClaudePaths::for_project(&cwd)
        .map(|p| p.pipeline_state_file(&spec_base))
        .unwrap_or_else(|_| cwd.join(format!("{spec_base}.json")));
    if let Ok(t) = fs::read_to_string(&state_file) {
        if let Ok(v) = serde_json::from_str::<Value>(&t) {
            state = v;
        }
    }

    let model = build_model(&header, &text, &state);
    if format == "json" {
        let out = json!({
            "done": model.done,
            "left": model.left,
            "nextSteps": model.next_steps,
            "followUps": model.follow_ups,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string()));
    } else {
        print!("{}", render(&model, header.lang == "pt-BR"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_name_status_lang() {
        // New canonical header: status word is projected from the SpecState.
        // Legacy short form `pt` is tolerated on read and normalised to BCP-47.
        let h = parse_header("# My Spec\n\n### Stage: Close\n### Outcome: Completed\n### Lang: pt\n");
        assert_eq!(h.name, "My Spec");
        assert_eq!(h.status, "completed");
        assert_eq!(h.lang, "pt-BR");
    }

    #[test]
    fn parses_legacy_status_phase_header() {
        // Legacy header still resolves (tolerant core parser); a terminal
        // `### Status: completed` projects to the `completed` word.
        let h = parse_header("# My Spec\n\n### Status: completed | Phase: CLOSE\n### Lang: en-US\n");
        assert_eq!(h.name, "My Spec");
        assert_eq!(h.status, "completed");
        assert_eq!(h.lang, "en-US");
    }

    #[test]
    fn happy_path_yields_git_next_steps() {
        let text = "# Spec\n\n### Status: Done\n### Lang: en-US\n\n## Acceptance Criteria\n- [x] AC-1: x — Command: `true`\n";
        let header = parse_header(text);
        let model = build_model(&header, text, &json!({}));
        assert!(model.left.is_empty());
        assert!(model.next_steps.iter().any(|s| s.contains("git add")));
    }

    #[test]
    fn failing_ac_lands_in_left() {
        let text = "# Spec\n\n### Lang: en-US\n\n## Acceptance Criteria\n- [ ] AC-2: broken — Command: `false`\n";
        let header = parse_header(text);
        let model = build_model(&header, text, &json!({}));
        assert!(model.left.iter().any(|l| l.contains("AC-2")));
    }
}
