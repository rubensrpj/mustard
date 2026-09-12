//! `mustard-rt run wave-size-check` — a port of `scripts/wave-size-check.js`.
//!
//! Advisory audit of per-wave size inside a wave-plan. `exec-rewave-check` only
//! decomposes a flat spec; once a spec is a wave-plan nothing flags an
//! oversized individual wave. This audits each wave and WARNS (never blocks).
//!
//! O audit tem DUAS pontas. O teto (`oversized`) diz que a onda não cabe numa
//! passada; o piso (`undersized`) diz que ela não paga o próprio despacho.
//!
//! Output: one JSON line. The `oversizedCount` field is parsed downstream, so
//! the shape is preserved exactly — `undersizedCount` is ADDED alongside it,
//! nunca no lugar dele.
//!
//! Port note: the JS version shelled to `wave-tree.js` and `scope-decompose.js`.
//! Both are now in this binary — this port calls the Rust logic directly.

use crate::commands::spec::scope_decompose::decide;
use crate::commands::wave::wave_lib::{detect_role_with, load_role_patterns, parse_files_section};
use mustard_core::RolePattern;
use mustard_core::io::fs;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// Resolve the file-count threshold (default 10, floor 3).
fn resolve_limit() -> usize {
    env_limit("MUSTARD_WAVE_SIZE_LIMIT", 10)
}

/// Resolve the task-count threshold (default 10, floor 3).
///
/// A wave is ONE agent in ONE pass. Files measure how wide it reaches; tasks
/// measure how long it has to stay coherent, and they are not the same number —
/// measured in the field, a wave was accepted at 19 files AND 13 tasks, and the
/// audit only ever looked at the files. The failure mode tasks catch is specific:
/// quality falls off at the end of a long list, and a failure at task 11 wastes
/// the ten before it.
fn resolve_task_limit() -> usize {
    env_limit("MUSTARD_WAVE_TASK_LIMIT", 10)
}

/// A `usize` threshold from `var`, floored at 3, defaulting to `default`.
fn env_limit(var: &str, default: usize) -> usize {
    env_limit_from(std::env::var(var).ok().as_deref(), default)
}

/// Deterministic core of [`env_limit`]: the env value arrives as a parameter, so
/// the clamp is a pure function of its inputs and testable without mutating
/// process env — which needs `unsafe` under Rust 2024, forbidden in this crate.
fn env_limit_from(raw: Option<&str>, default: usize) -> usize {
    raw.and_then(|v| v.parse::<i64>().ok())
        .map_or(default, |n| if n < 3 { 3 } else { n as usize })
}

/// Resolve a MASSA MÍNIMA de arquivos antes que o sinal `multi-layer` valha
/// para uma ONDA (default 6).
///
/// O audit chama, literalmente, o decisor que responde outra pergunta: "esta
/// SPEC deve virar várias ondas?". Lá `layerCount >= 2 && fileCount >= 3` é a
/// resposta certa. Perguntado de uma ONDA, é o conselho invertido — três
/// arquivos são o átomo e não existe divisão que satisfaça o aviso. Medido em
/// 07/09/2026: perseguir esse aviso levou um plano de 4 para 14 ondas. Daí o
/// piso PRÓPRIO, separado do da spec: a onda precisa ser genuinamente larga
/// (mais da metade do teto de 10) antes que as camadas importem.
fn resolve_layer_floor() -> usize {
    env_floor("MUSTARD_WAVE_LAYER_FLOOR", 6)
}

/// Resolve o teto de arquivos ABAIXO do qual a onda é pequena demais
/// (default 2, inclusivo).
fn resolve_min_files() -> usize {
    env_floor("MUSTARD_WAVE_MIN_FILES", 2)
}

/// Resolve o teto de tarefas ABAIXO do qual a onda é pequena demais
/// (default 2, inclusivo).
fn resolve_min_tasks() -> usize {
    env_floor("MUSTARD_WAVE_MIN_TASKS", 2)
}

/// A `usize` threshold from `var`, defaulting to `default`, clamped at 0.
///
/// Gêmeo de [`env_limit`] SEM o piso de 3 daquele: os limiares do piso valem 2
/// por padrão, um valor que `env_limit` não conseguiria sequer expressar.
fn env_floor(var: &str, default: usize) -> usize {
    env_floor_from(std::env::var(var).ok().as_deref(), default)
}

/// Deterministic core of [`env_floor`], for the same reason [`env_limit_from`]
/// exists. Zero is a LEGAL setting here: it disables the floor outright, which
/// is precisely what the ceiling twin cannot express.
fn env_floor_from(raw: Option<&str>, default: usize) -> usize {
    raw.and_then(|v| v.parse::<i64>().ok())
        .map_or(default, |n| if n < 0 { 0 } else { n as usize })
}

/// An enumerated wave folder.
struct WaveFolder {
    folder: String,
}

/// Enumerate wave folders for a spec dir, or `None` when it is not a wave-plan.
fn enumerate_waves(spec_dir: &Path) -> Option<Vec<WaveFolder>> {
    if !spec_dir.join("wave-plan.md").exists() {
        return None;
    }
    let mut folders: Vec<String> = fs::read_dir(spec_dir)
        .map(|entries| {
            entries
                .into_iter()
                .filter(|e| e.is_dir)
                .map(|e| e.file_name)
                .filter(|n| {
                    // `^wave-\d+`
                    let lower = n.to_lowercase();
                    lower.starts_with("wave-")
                        && lower[5..].chars().next().is_some_and(|c| c.is_ascii_digit())
                })
                .collect()
        })
        .unwrap_or_default();
    folders.sort_by_key(|f| wave_number_of(f).unwrap_or(0));
    if folders.is_empty() {
        return None;
    }
    Some(folders.into_iter().map(|folder| WaveFolder { folder }).collect())
}

/// Extract a wave number from a folder name.
fn wave_number_of(name: &str) -> Option<u32> {
    let start = name.find(|c: char| c.is_ascii_digit())?;
    let end = name[start..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(name.len(), |e| start + e);
    name[start..end].parse().ok()
}

/// Try to extract a wave's file list from `wave-plan.md` (for stub waves).
fn files_from_wave_plan(spec_dir: &Path, wave_num: Option<u32>) -> Option<Vec<String>> {
    let wave_num = wave_num?;
    let text = fs::read_to_string(spec_dir.join("wave-plan.md")).ok()?;
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();

    // 1. `### Wave N` section → `Files (N): a, b, c`.
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if !is_wave_header(t, wave_num) {
            continue;
        }
        for next in lines.iter().skip(i + 1) {
            let l = next.trim();
            if l.starts_with("## ") || l.starts_with("### ") || l.starts_with("#### ") {
                break;
            }
            if let Some(rest) = strip_files_prefix(l) {
                let parts: Vec<String> = rest
                    .split(',')
                    .map(|s| s.trim().trim_matches('`').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !parts.is_empty() {
                    return Some(parts);
                }
            }
        }
    }

    // 2. table row `| W3 | ... |` with a file-list cell.
    for line in &lines {
        let t = line.trim();
        if !is_table_row_for_wave(t, wave_num) {
            continue;
        }
        let cells: Vec<&str> = t.split('|').map(str::trim).filter(|c| !c.is_empty()).collect();
        for c in cells {
            if (c.contains('/') || c.contains('\\')) && c.contains(',') {
                let parts: Vec<String> = c
                    .split(',')
                    .map(|s| s.trim().trim_matches('`').to_string())
                    .filter(|s| s.contains('/') || s.contains('\\'))
                    .collect();
                if !parts.is_empty() {
                    return Some(parts);
                }
            }
        }
    }
    None
}

/// `^#{2,4}\s*Wave\s*N\b`
fn is_wave_header(line: &str, wave_num: u32) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if !(2..=4).contains(&hashes) {
        return false;
    }
    let rest = line[hashes..].trim_start();
    let lower = rest.to_lowercase();
    let Some(after) = lower.strip_prefix("wave") else {
        return false;
    };
    let after = after.trim_start();
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok() == Some(wave_num)
        && after[digits.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
}

/// `^Files\s*\(\d+\)\s*:\s*(.+)$`
fn strip_files_prefix(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("Files").or_else(|| line.strip_prefix("files"))?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('(')?;
    let close = rest.find(')')?;
    if rest[..close].is_empty() || !rest[..close].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let rest = rest[close + 1..].trim_start();
    let rest = rest.strip_prefix(':')?;
    let body = rest.trim_start();
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

/// `^\|\s*W?N\b`
fn is_table_row_for_wave(line: &str, wave_num: u32) -> bool {
    let Some(rest) = line.strip_prefix('|') else {
        return false;
    };
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(['W', 'w']).unwrap_or(rest);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok() == Some(wave_num)
        && rest[digits.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
}

/// Audit a single wave.
fn audit_wave(
    wave: &WaveFolder,
    spec_dir: &Path,
    limit: usize,
    task_limit: usize,
    role_patterns: &[RolePattern],
    model_path: &Path,
    project_root: &Path,
) -> Value {
    let folder = &wave.folder;
    let wave_num = wave_number_of(folder);

    // Prefer the wave's own spec.md `## Files` section.
    let mut files: Option<Vec<String>> = None;
    let mut source: Option<&str> = None;
    let mut task_count = 0usize;
    let wave_spec_path = spec_dir.join(folder).join("spec.md");
    if wave_spec_path.exists()
        && let Ok(text) = fs::read_to_string(&wave_spec_path) {
            task_count = count_task_items(&text);
            if let Some(parsed) = parse_files_section(&text)
                && !parsed.is_empty() {
                    files = Some(parsed);
                    source = Some("wave-spec");
                }
        }
    if files.is_none()
        && let Some(plan_files) = files_from_wave_plan(spec_dir, wave_num)
            && !plan_files.is_empty() {
                files = Some(plan_files);
                source = Some("wave-plan");
            }

    let Some(files) = files else {
        let status = if wave_spec_path.exists() {
            "unknown"
        } else {
            "stub"
        };
        return json!({ "wave": wave_num, "folder": folder, "status": status });
    };

    let file_count = files.len();
    let roles: BTreeSet<String> = files.iter().map(|f| detect_role_with(f, role_patterns)).collect();
    let layer_count = if roles.len() == 1 && roles.contains("lib") {
        1
    } else {
        roles.len()
    };

    // Stack-awareness: the role→layer `multi-layer` signal is only trustworthy
    // where the gate was tuned (JS/TS). On a foreign-language wave (C#, Python,
    // …) it fires on every intrinsically cross-layer backend feature — a
    // guaranteed false alarm. Loosen there: keep only the language-agnostic
    // file-count reason. Resolved from the wave's `## Files` extensions plus the
    // repo model's detected stacks (both fail-open, so a JS/TS or undetected
    // wave keeps the historical layer signal).
    let langs = mustard_core::resolve_target_languages(&files, model_path, project_root);
    let understood = mustard_core::target_understood(&langs);

    // O sinal de camadas só vale numa onda já LARGA. `decide` responde "esta
    // SPEC deve virar várias ondas?" e diz sim com 2 camadas e 3 arquivos —
    // certo para uma spec, invertido para uma onda. O piso próprio (ajustável
    // por `MUSTARD_WAVE_LAYER_FLOOR`) é o que separa as duas perguntas.
    let layer_floor = resolve_layer_floor();

    let mut reasons: Vec<String> = Vec::new();
    if understood && file_count >= layer_floor {
        let decision = decide(&json!({
            "fileCount": file_count,
            "layerCount": layer_count,
            "newEntityCount": 0,
            "knowledgeMatches": [],
        }));
        if decision.get("decompose").and_then(Value::as_bool) == Some(true)
            && let Some(reason) = decision.get("reason").and_then(Value::as_str) {
                reasons.push(reason.to_string());
            }
    }
    if file_count > limit {
        reasons.push(format!("file-count:{file_count}>{limit}"));
    }
    if task_count > task_limit {
        reasons.push(format!("task-count:{task_count}>{task_limit}"));
    }
    let oversized = !reasons.is_empty();

    // A outra ponta. Um despacho custa um prompt renderizado, uma passada de
    // agente e um relatório; uma onda de 1 arquivo e 1 tarefa não paga isso.
    // Só uma onda MATERIALIZADA é julgada aqui: com `source: wave-plan` não há
    // `spec.md`, logo `task_count` é 0 por ausência de arquivo, e um esboço
    // seria acusado de pequeno por uma contagem que ninguém escreveu ainda.
    let min_files = resolve_min_files();
    let min_tasks = resolve_min_tasks();
    let undersized =
        source == Some("wave-spec") && file_count <= min_files && task_count <= min_tasks;
    if undersized {
        reasons.push(format!(
            "too-small:{file_count}f/{task_count}t<={min_files}f/{min_tasks}t"
        ));
    }

    json!({
        "wave": wave_num,
        "folder": folder,
        "fileCount": file_count,
        "taskCount": task_count,
        "layerCount": layer_count,
        "languages": langs.into_iter().collect::<Vec<_>>(),
        "oversized": oversized,
        "undersized": undersized,
        "reason": reasons.join("; "),
        "source": source,
    })
}

/// Count the checklist items under a wave spec's `## Tasks` / `## Tarefas`
/// heading — top-level `- ` bullets only, so a sub-bullet elaborating one task
/// is not counted as another task.
fn count_task_items(text: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|l| crate::commands::spec::spec_sections::is_heading(l, "tasks"))
    else {
        return 0;
    };
    lines
        .iter()
        .skip(start + 1)
        .take_while(|l| !l.starts_with("## "))
        .filter(|l| l.starts_with("- "))
        .count()
}

/// Dispatch `mustard-rt run wave-size-check`.
pub fn run(spec_dir_arg: Option<&str>) {
    let emit = |v: Value| println!("{v}");
    let Some(spec_dir_arg) = spec_dir_arg else {
        emit(json!({ "action": "skip", "reason": "no-spec-dir-arg" }));
        return;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    // Accept the three spec-dir spellings (a directory, a `…/spec.md` path, a
    // bare slug) through the shared normaliser before the cwd join.
    // Root resolution matches the sibling call sites (`wave_tree`,
    // `pipeline_summary`, `plan_materialize`): `project_dir()` honours
    // `CLAUDE_PROJECT_DIR`, so a bare slug resolves identically across all four
    // commands the normaliser exists to unify.
    let resolved = crate::shared::context::normalise_spec_dir(
        Path::new(&crate::shared::context::project_dir()),
        spec_dir_arg,
    );
    let spec_dir = if resolved.is_absolute() {
        resolved
    } else {
        cwd.join(resolved)
    };
    if !spec_dir.exists() {
        emit(json!({
            "action": "skip",
            "reason": "error-fallback",
            "error": "spec-dir-not-found",
        }));
        return;
    }

    emit(audit(&spec_dir));
}

/// Audit every wave of `spec_dir` and return the report — the miolo of [`run`],
/// callable IN-PROCESS.
///
/// It is `pub(crate)` for one reason: this audit computed its numbers and no
/// step of the pipeline ever looked at them. `wave-size-check` shipped as a
/// command nobody called, so a plan was accepted with a 19-file, 13-task wave
/// with no warning at any stage — the plan report even printed `widestWave: 19`
/// and did nothing with it. [`warn_oversized_waves`] is the caller that closes
/// that gap.
pub(crate) fn audit(spec_dir: &Path) -> Value {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let Some(waves) = enumerate_waves(spec_dir) else {
        return json!({ "action": "skip", "reason": "not-a-wave-plan" });
    };

    let limit = resolve_limit();
    let task_limit = resolve_task_limit();
    // F0-e: honour `mustard.json#rolePatterns` so non-English / non-JS layers
    // classify correctly. Resolve from the workspace anchor, fail-open to cwd.
    let project_root = crate::shared::context::workspace_root_strict().unwrap_or(cwd);
    let role_patterns = load_role_patterns(&project_root);
    let model_path = project_root.join(".claude").join("grain.model.json");
    let audited: Vec<Value> = waves
        .iter()
        .map(|w| {
            audit_wave(
                w,
                spec_dir,
                limit,
                task_limit,
                &role_patterns,
                &model_path,
                &project_root,
            )
        })
        .collect();
    let oversized_count = audited
        .iter()
        .filter(|w| w.get("oversized").and_then(Value::as_bool) == Some(true))
        .count();
    let undersized_count = audited
        .iter()
        .filter(|w| w.get("undersized").and_then(Value::as_bool) == Some(true))
        .count();

    json!({
        "action": "audited",
        "specDir": spec_dir.to_string_lossy(),
        "limit": limit,
        "taskLimit": task_limit,
        "oversizedCount": oversized_count,
        "undersizedCount": undersized_count,
        "waves": audited,
    })
}

/// The line a wave's shape earns — the ceiling's instruction or the floor's.
///
/// Pure, and separated from the `eprintln!` for ONE reason: the BRANCH is the
/// whole fix. The two messages point in OPPOSITE directions, so an edit that
/// swaps them — or that drops the floor's `Do NOT split it further` — changes
/// nothing a compiler or a green suite would notice, and puts the operator back
/// on the spiral that took a plan from 4 waves to 14.
fn shape_warning(folder: &str, files: u64, tasks: u64, reason: &str, too_big: bool) -> String {
    if too_big {
        format!(
            "[wave-size] WARN: {folder} is oversized ({files} files, {tasks} tasks — {reason}). \
             A wave is ONE agent in ONE pass: split off the tasks that share no file with the \
             rest — they are a wave of their own, and they can run in parallel."
        )
    } else {
        format!(
            "[wave-size] WARN: {folder} is undersized ({files} files, {tasks} tasks — {reason}). \
             A dispatch costs a rendered prompt, an agent pass and a report — more than the work \
             this wave carries: FOLD it into a neighbouring wave. Do NOT split it further."
        )
    }
}

/// Run [`audit`] over a freshly materialised plan and WARN on stderr for each
/// oversized wave. Advisory — it never blocks, and it never touches stdout (the
/// materialise report is machine-read and must stay byte-stable).
///
/// **Why the warning is worth a line each.** A wave is one agent in one pass.
/// Past the ceiling the last tasks get the worst work, and a failure late in the
/// list throws away everything before it. The operator cannot see that from a
/// plan that materialised successfully — every artefact is there, every file is
/// listed, nothing failed. This is the only moment the shape of the plan is
/// visible and still cheap to change.
///
/// **Por que as duas pontas saem daqui.** Quem persegue só o teto não tem onde
/// parar: no campo, o plano foi de 4 para 14 ondas e terminou numa onda de um
/// arquivo. As duas mensagens mandam para lados OPOSTOS de propósito — a onda
/// grande se DIVIDE, a onda pequena se DOBRA numa vizinha.
///
/// **Sob os defaults elas não colidem, e não é UMA coisa que garante isso.** O
/// piso de camadas (6) fica acima do piso de tamanho (2), e o mínimo duro de 3
/// em [`env_limit`] impede que o teto de arquivos ou de tarefas desça até a
/// faixa do piso. Quem reconfigura os pisos PODE cruzá-los: com
/// `MUSTARD_WAVE_MIN_FILES=6`, uma onda de 6 arquivos sai `oversized` E
/// `undersized`, e só a mensagem do teto é impressa. O audit é consultivo e a
/// configuração é do operador, então isso se documenta aqui — não se corrige no
/// código, que não tem como saber qual das duas o operador quis.
pub(crate) fn warn_oversized_waves(spec_dir: &Path) {
    let report = audit(spec_dir);
    let over = report.get("oversizedCount").and_then(Value::as_u64).unwrap_or(0);
    let under = report.get("undersizedCount").and_then(Value::as_u64).unwrap_or(0);
    if over == 0 && under == 0 {
        return;
    }
    let Some(waves) = report.get("waves").and_then(Value::as_array) else {
        return;
    };
    for w in waves {
        let too_big = w.get("oversized").and_then(Value::as_bool) == Some(true);
        let too_small = w.get("undersized").and_then(Value::as_bool) == Some(true);
        if !too_big && !too_small {
            continue;
        }
        let folder = w.get("folder").and_then(Value::as_str).unwrap_or("?");
        let files = w.get("fileCount").and_then(Value::as_u64).unwrap_or(0);
        let tasks = w.get("taskCount").and_then(Value::as_u64).unwrap_or(0);
        let reason = w.get("reason").and_then(Value::as_str).unwrap_or("");
        eprintln!("{}", shape_warning(folder, files, tasks, reason, too_big));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_limit_floors_at_three() {
        // Default applies when env unset.
        assert!(resolve_limit() >= 3);
    }

    #[test]
    fn wave_number_extraction() {
        assert_eq!(wave_number_of("wave-3-backend"), Some(3));
        assert_eq!(wave_number_of("wave-12"), Some(12));
    }

    #[test]
    fn audits_wave_plan_with_oversized_wave() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::write(spec_dir.join("wave-plan.md"), "# plan\n").unwrap();
        let wave_dir = spec_dir.join("wave-1-backend");
        std::fs::create_dir_all(&wave_dir).unwrap();
        let mut files = String::from("## Files\n");
        for i in 0..14 {
            files.push_str(&format!("- src/api/h{i}.ts\n"));
        }
        std::fs::write(wave_dir.join("spec.md"), files).unwrap();
        let waves = enumerate_waves(spec_dir).unwrap();
        // No grain model on disk → detected-stacks signal is empty; the `.ts`
        // extensions carry the (understood) language, so the file-count reason
        // still fires.
        let no_model = spec_dir.join("no-model.json");
        let audited = audit_wave(&waves[0], spec_dir, 10, 10, &[], &no_model, spec_dir);
        assert_eq!(audited["oversized"], json!(true));
        assert_eq!(audited["fileCount"], json!(14));
        assert_eq!(audited["languages"], json!(["typescript"]));
    }

    #[test]
    fn not_a_wave_plan_skips() {
        let dir = tempdir().unwrap();
        assert!(enumerate_waves(dir.path()).is_none());
    }

    /// A wave is oversized by its TASK count too, not only by its files.
    ///
    /// Measured in the field: a plan was accepted carrying a wave of 19 files
    /// AND 13 tasks, and this audit only ever looked at the files — so a wave
    /// that is narrow but very long slipped through every stage in silence.
    /// A wave is one agent in one pass: past the ceiling the last tasks get the
    /// worst work, and a failure at task 11 wastes the ten before it.
    #[test]
    fn a_long_task_list_is_oversized_even_with_few_files() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::write(spec_dir.join("wave-plan.md"), "# plan\n").unwrap();
        let wave_dir = spec_dir.join("wave-1-backend");
        std::fs::create_dir_all(&wave_dir).unwrap();
        let tasks: String = (1..=13).fold(String::new(), |mut acc, i| {
            use std::fmt::Write as _;
            let _ = writeln!(acc, "- [ ] task {i}");
            acc
        });
        std::fs::write(
            wave_dir.join("spec.md"),
            format!("## Files\n- src/a.rs\n- src/b.rs\n\n## Tasks\n{tasks}"),
        )
        .unwrap();
        let waves = enumerate_waves(spec_dir).unwrap();
        let no_model = spec_dir.join("no-model.json");
        let audited = audit_wave(&waves[0], spec_dir, 10, 10, &[], &no_model, spec_dir);
        assert_eq!(audited["fileCount"], json!(2), "well under the file limit: {audited}");
        assert_eq!(audited["taskCount"], json!(13), "{audited}");
        assert_eq!(audited["oversized"], json!(true), "{audited}");
        assert!(
            audited["reason"].as_str().unwrap_or_default().contains("task-count:13>10"),
            "the reason names WHICH ceiling was crossed: {audited}"
        );
    }

    #[test]
    fn foreign_language_wave_suppresses_multi_layer_but_keeps_file_count() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::write(spec_dir.join("wave-plan.md"), "# plan\n").unwrap();
        let wave_dir = spec_dir.join("wave-1-backend");
        std::fs::create_dir_all(&wave_dir).unwrap();
        // A C# wave spanning DTOs + Services + Controllers — layerCount >= 2, and
        // seis arquivos: ACIMA do piso de camadas (6) e abaixo do teto (10), de
        // modo que só a língua pode calar o alarme. Eram três; com o piso novo,
        // três seriam absolvidos pelo tamanho e o teste deixaria de exercer a
        // supressão por língua que ele existe para fixar.
        let files = "## Files\n\
            - backend/App/DTOs/Payable.cs\n\
            - backend/App/DTOs/Recurrence.cs\n\
            - backend/App/Services/Recurrence.cs\n\
            - backend/App/Services/Payable.cs\n\
            - backend/App/Controllers/PayableController.cs\n\
            - backend/App/Controllers/RecurrenceController.cs\n";
        std::fs::write(wave_dir.join("spec.md"), files).unwrap();
        let waves = enumerate_waves(spec_dir).unwrap();
        let no_model = spec_dir.join("no-model.json");
        let audited = audit_wave(&waves[0], spec_dir, 10, 10, &[], &no_model, spec_dir);
        assert!(audited["layerCount"].as_u64().unwrap() >= 2, "C# folders span layers: {audited}");
        assert_eq!(audited["languages"], json!(["csharp"]));
        assert_eq!(audited["oversized"], json!(false), "multi-layer suppressed for foreign lang: {audited}");
        assert_eq!(audited["reason"], json!(""));
    }

    #[test]
    fn js_ts_wave_still_flags_multi_layer() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::write(spec_dir.join("wave-plan.md"), "# plan\n").unwrap();
        let wave_dir = spec_dir.join("wave-1-app");
        std::fs::create_dir_all(&wave_dir).unwrap();
        // The layer signal is preserved for the language the gate was tuned for
        // — desde que a onda esteja ACIMA do piso de arquivos próprio dela
        // (`MUSTARD_WAVE_LAYER_FLOOR`, 6). Eram três arquivos aqui, exatamente a
        // largura que o piso passou a absolver; seis mantêm a intenção original
        // do teste (JS/TS ainda ouve `multi-layer`) sem reafirmar o alarme que a
        // spec veio calar.
        let files = "## Files\n\
            - src/schema/user.ts\n\
            - src/schema/session.ts\n\
            - src/api/users.ts\n\
            - src/api/sessions.ts\n\
            - src/components/UserCard.tsx\n\
            - src/components/SessionList.tsx\n";
        std::fs::write(wave_dir.join("spec.md"), files).unwrap();
        let waves = enumerate_waves(spec_dir).unwrap();
        let no_model = spec_dir.join("no-model.json");
        let audited = audit_wave(&waves[0], spec_dir, 10, 10, &[], &no_model, spec_dir);
        assert!(audited["layerCount"].as_u64().unwrap() >= 2);
        assert_eq!(audited["oversized"], json!(true), "multi-layer preserved for JS/TS: {audited}");
        assert!(audited["reason"].as_str().unwrap().contains("multi-layer"));
    }

    /// The field case, 2026-09-07: `wave-14-frontend` — THREE files, ONE task,
    /// all three in the same folder — was reported `oversized (multi-layer)`.
    ///
    /// The audit reused the decision function that answers a DIFFERENT question:
    /// "should this SPEC become several waves?" says yes at 2 layers and 3 files,
    /// and that is right for a spec. Asked of a WAVE it is the opposite advice —
    /// three files is the atom, and there is no split that satisfies the warning.
    /// Chasing it took a plan from 4 waves to 14, ending with a wave of one file
    /// and one task: a full dispatch, a twenty-thousand-character prompt, an
    /// execution and a report, to edit one file.
    #[test]
    fn a_three_file_wave_across_roles_is_not_oversized() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::write(spec_dir.join("wave-plan.md"), "# plan\n").unwrap();
        let wave_dir = spec_dir.join("wave-14-frontend");
        std::fs::create_dir_all(&wave_dir).unwrap();
        // Three siblings in ONE folder. The role keywords still split them
        // (`service` -> api, `view` -> ui), so layerCount >= 2 — which is
        // exactly why the file floor, not the layer count, has to decide.
        let files = "## Files\n\
            - packages/core/src/client/hooks/useTitleService.ts\n\
            - packages/core/src/client/hooks/useForecastView.ts\n\
            - packages/core/src/client/hooks/useDrift.ts\n\
            \n## Tasks\n- [ ] wire the three hooks\n";
        std::fs::write(wave_dir.join("spec.md"), files).unwrap();
        let waves = enumerate_waves(spec_dir).unwrap();
        let no_model = spec_dir.join("no-model.json");
        let audited = audit_wave(&waves[0], spec_dir, 10, 10, &[], &no_model, spec_dir);
        assert!(audited["layerCount"].as_u64().unwrap() >= 2, "the roles do split: {audited}");
        assert_eq!(
            audited["oversized"],
            json!(false),
            "three files is the atom, not an oversized wave: {audited}"
        );
    }

    /// The missing floor. A wave of one file and one task costs a whole
    /// dispatch — a rendered prompt, an agent pass, a report — to edit one file.
    /// Nothing in the binary reclaimed against that, so an operator following
    /// the (correct) size ceiling had no signal telling them where to stop, and
    /// walked straight past it into the opposite excess.
    #[test]
    fn a_one_file_one_task_wave_is_flagged_undersized() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::write(spec_dir.join("wave-plan.md"), "# plan\n").unwrap();
        let wave_dir = spec_dir.join("wave-13-core");
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(
            wave_dir.join("spec.md"),
            "## Files\n- src/lonely.ts\n\n## Tasks\n- [ ] the only task\n",
        )
        .unwrap();
        let waves = enumerate_waves(spec_dir).unwrap();
        let no_model = spec_dir.join("no-model.json");
        let audited = audit_wave(&waves[0], spec_dir, 10, 10, &[], &no_model, spec_dir);
        assert_eq!(audited["oversized"], json!(false), "not too big: {audited}");
        assert_eq!(
            audited["undersized"],
            json!(true),
            "one file and one task does not pay for a dispatch: {audited}"
        );
        assert!(
            audited["reason"].as_str().unwrap_or_default().contains("too-small"),
            "the reason names WHICH floor was crossed: {audited}"
        );
    }

    /// O piso só julga uma onda MATERIALIZADA.
    ///
    /// Uma onda ainda em esboço tem os arquivos no `wave-plan.md` e nenhum
    /// `spec.md`, então `taskCount` é 0 por AUSÊNCIA de arquivo, não por
    /// escassez de trabalho. Medir o piso ali acusaria de pequena toda onda que
    /// ninguém escreveu ainda — o contrário de um sinal.
    #[test]
    fn a_stub_wave_is_never_judged_undersized() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::write(
            spec_dir.join("wave-plan.md"),
            "# plan\n\n### Wave 1\nFiles (1): src/lonely.ts\n",
        )
        .unwrap();
        std::fs::create_dir_all(spec_dir.join("wave-1-core")).unwrap();
        let waves = enumerate_waves(spec_dir).unwrap();
        let no_model = spec_dir.join("no-model.json");
        let audited = audit_wave(&waves[0], spec_dir, 10, 10, &[], &no_model, spec_dir);
        assert_eq!(audited["source"], json!("wave-plan"), "{audited}");
        assert_eq!(audited["fileCount"], json!(1), "{audited}");
        assert_eq!(
            audited["undersized"],
            json!(false),
            "an unwritten wave has no task count to measure: {audited}"
        );
        assert_eq!(audited["reason"], json!(""), "{audited}");
    }

    /// The branch split IS the fix, so it gets a test of its own.
    ///
    /// The two ends give OPPOSITE instructions, and the floor's must never read
    /// as "split". Answering an undersized wave by splitting it is exactly the
    /// spiral measured in the field: 4 waves became 14, and the last of them
    /// held one file.
    #[test]
    fn the_two_ends_give_opposite_instructions() {
        let big = shape_warning("wave-1-backend", 19, 13, "file-count:19>10", true);
        assert!(big.contains("is oversized"), "{big}");
        assert!(big.contains("split off the tasks"), "{big}");

        let small = shape_warning("wave-13-core", 1, 1, "too-small:1f/1t<=2f/2t", false);
        assert!(small.contains("is undersized"), "{small}");
        assert!(small.contains("FOLD it into a neighbouring wave"), "{small}");
        assert!(
            small.contains("Do NOT split it further"),
            "the floor must never be answered by splitting: {small}"
        );
        assert!(!small.contains("split off"), "no split instruction on the floor: {small}");
    }

    /// The floor knobs have NO hard minimum, and that is the entire reason
    /// [`env_floor`] exists beside [`env_limit`]: the size floor defaults to 2,
    /// a value the ceiling's clamp of 3 cannot even express. Zero is legal — it
    /// DISABLES the floor rather than silently becoming 3.
    #[test]
    fn floor_knobs_reach_below_the_ceiling_clamp() {
        assert_eq!(env_floor_from(None, 2), 2, "unset takes the default");
        assert_eq!(env_floor_from(Some("6"), 2), 6);
        assert_eq!(env_floor_from(Some("0"), 2), 0, "zero disables the floor");
        assert_eq!(env_floor_from(Some("-4"), 2), 0, "negative clamps to zero, not to the default");
        assert_eq!(env_floor_from(Some("nao-numero"), 2), 2, "unparseable falls back to the default");

        // The contrast that justifies the twin: the ceiling knob cannot go there.
        assert_eq!(env_limit_from(Some("2"), 10), 3, "the ceiling clamps at 3");
        assert_eq!(env_limit_from(Some("0"), 10), 3, "…and zero cannot disable it");
        assert_eq!(env_limit_from(None, 10), 10);
        assert_eq!(env_limit_from(Some("14"), 10), 14);
    }
}
