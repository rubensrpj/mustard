//! Deterministic CLAUDE.md generator for subprojects — no AI, no source reads.
//!
//! Invoked by `scan::run` after `grain.model.json` is written:
//! - `--full`: (re)generates `{root}/{dir}/CLAUDE.md` per subproject. Only the
//!   machine-owned block delimited by [`SENTINEL_OPEN`] / [`SENTINEL_CLOSE`] is
//!   regenerated; every other byte of the file (curated `## Architecture`,
//!   `## Key Paths`, `## Guards`, …) is preserved verbatim.
//! - default: reports files exceeding [`CLAUDE_MD_WARN_BYTES`] as oversized.

use std::fmt::Write as _;
use std::path::Path;

/// Files larger than this threshold trigger a warning in default (non-full) mode.
pub const CLAUDE_MD_WARN_BYTES: usize = 2048;

/// Opening marker of the machine-owned block. Everything between this and
/// [`SENTINEL_CLOSE`] is regenerated on each `--full` pass; everything outside
/// it is curated by humans and never touched.
const SENTINEL_OPEN: &str = "<!-- mustard:scan-map -->";
/// Closing marker of the machine-owned block.
const SENTINEL_CLOSE: &str = "<!-- /mustard:scan-map -->";

/// Result of running the CLAUDE.md pass over a set of projects.
pub struct ClaudeMdResult {
    /// Paths regenerated (full mode).
    pub regenerated: Vec<String>,
    /// Oversized files (default mode): path + byte count.
    pub oversized: Vec<OversizedEntry>,
}

pub struct OversizedEntry {
    pub path: String,
    pub bytes: usize,
}

/// Title-case the first character of `s`, leaving the rest unchanged.
fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut out = first.to_uppercase().to_string();
            out.push_str(chars.as_str());
            out
        }
    }
}

/// Locate the machine-owned block in `content`: the byte span running from the
/// start of the [`SENTINEL_OPEN`] line through the end of the [`SENTINEL_CLOSE`]
/// line (the trailing newline, if any, stays outside the span). Returns `None`
/// when either marker is missing or out of order, so a malformed file is left
/// untouched rather than half-spliced.
fn find_sentinel_span(content: &str) -> Option<(usize, usize)> {
    let open = content.find(SENTINEL_OPEN)?;
    // The close marker must come after the open marker.
    let close_rel = content[open..].find(SENTINEL_CLOSE)?;
    let close_start = open + close_rel;
    let close_end = close_start + SENTINEL_CLOSE.len();
    Some((open, close_end))
}

/// Build the machine-owned block (delimited by the sentinels) for a unit: the
/// orientation map plus the `## Stack` and `## Commands` sections. The returned
/// string starts with [`SENTINEL_OPEN`] and ends with [`SENTINEL_CLOSE`] (no
/// trailing newline) so callers control the surrounding whitespace.
fn build_managed_block(
    kind: &str,
    code_files: usize,
    frameworks: &[String],
    commands: &mustard_core::domain::config::Commands,
) -> String {
    let stack = render_stack(kind, frameworks);
    let commands_block = render_commands(commands);

    let mut block = String::new();
    let _ = writeln!(block, "{SENTINEL_OPEN}");
    let _ = writeln!(block, "Tipo: {kind} · {code_files} arquivos");
    let _ = writeln!(
        block,
        "Pesquise via `mustard-rt run feature` (digest) — não leia o repo direto."
    );
    block.push('\n');
    // `render_stack` already ends in a newline.
    block.push_str(&stack);
    if !commands_block.is_empty() {
        block.push('\n');
        // `render_commands` already ends in a newline.
        block.push_str(&commands_block);
    }
    // Close marker with no trailing newline — the caller owns spacing.
    block.push_str(SENTINEL_CLOSE);
    block
}

/// Render the `## Stack` section: a "Tipo:" line plus a bullet per framework
/// (only when the unit declares any). Frameworks are emitted in the order the
/// scan mined them (frequency-ranked) — caller passes them as-is.
fn render_stack(kind: &str, frameworks: &[String]) -> String {
    let mut out = format!("## Stack\n\nTipo: {kind}\n");
    if !frameworks.is_empty() {
        out.push('\n');
        for fw in frameworks {
            let _ = writeln!(out, "- {fw}");
        }
    }
    out
}

/// Render the `## Commands` markdown table — one row per command the detector
/// resolved to `Some`. An all-`None` set yields no section (returns empty).
/// Rows are emitted in a fixed order for byte-stable output.
fn render_commands(commands: &mustard_core::domain::config::Commands) -> String {
    let rows: Vec<(&str, &Option<String>)> = vec![
        ("Build", &commands.build),
        ("Test", &commands.test),
        ("Lint", &commands.lint),
        ("Type-check", &commands.type_check),
    ];
    let present: Vec<(&str, &str)> =
        rows.iter().filter_map(|(label, val)| val.as_deref().map(|cmd| (*label, cmd))).collect();
    if present.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Commands\n\n| Task | Command |\n|------|---------|\n");
    for (label, cmd) in present {
        let _ = writeln!(out, "| {label} | `{cmd}` |");
    }
    out
}

/// Render a lean CLAUDE.md for a subproject.
///
/// - `name`: subproject name (will be title-cased for the H1 heading)
/// - `kind`: grain project kind (e.g. `rust`, `typescript`, …)
/// - `code_files`: number of code files grain counted
/// - `frameworks`: frequency-ranked frameworks/deps mined for this unit
/// - `commands`: build/test/lint/type-check set detected for this unit
/// - `existing`: current content of the CLAUDE.md (if the file exists)
///
/// Only the machine-owned block between [`SENTINEL_OPEN`] and [`SENTINEL_CLOSE`]
/// is (re)generated. When `existing` already carries the sentinels, just that
/// span is swapped and every other byte is kept verbatim. When `existing` is a
/// legacy file without sentinels, the previously machine-owned `## Stack` and
/// `## Commands` sections are replaced by the new block and all curated content
/// (`## Architecture`, `## Guards`, …) survives. When `existing` is `None`, a
/// fresh scaffold is emitted. The regenerated block is a pure function of the
/// inputs, so re-rendering the output reproduces it byte-for-byte (idempotent).
pub fn render(
    name: &str,
    kind: &str,
    code_files: usize,
    frameworks: &[String],
    commands: &mustard_core::domain::config::Commands,
    existing: Option<&str>,
) -> String {
    let block = build_managed_block(kind, code_files, frameworks, commands);

    match existing {
        // File already has the machine-owned block — splice it in place,
        // preserving the bytes before and after verbatim.
        Some(content) => {
            if let Some((start, end)) = find_sentinel_span(content) {
                let mut out = String::with_capacity(content.len() + block.len());
                out.push_str(&content[..start]);
                out.push_str(&block);
                out.push_str(&content[end..]);
                out
            } else {
                migrate_legacy(content, &block)
            }
        }
        // No file yet — emit a fresh scaffold.
        None => scaffold(name, &block),
    }
}

/// Emit a fresh CLAUDE.md: H1 + Parent line + the machine-owned block, then an
/// empty `## Guards` section outside the block for humans to fill in.
fn scaffold(name: &str, block: &str) -> String {
    let title = title_case(name);
    format!(
        "# {title}\n\
         \n\
         > Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)\n\
         \n\
         {block}\n\
         \n\
         ## Guards\n\
         \n\
         <!-- seed DO/DON'T aqui -->\n"
    )
}

/// Migrate a legacy (sentinel-free) file: drop the previously machine-owned
/// `## Stack` and `## Commands` sections and splice the new block where the
/// first of them began, keeping every other section (Parent line, curated
/// `## Architecture`, `## Guards`, …) verbatim. This guarantees a single block
/// with no Stack/Commands duplication.
fn migrate_legacy(content: &str, block: &str) -> String {
    const MACHINE_OWNED: [&str; 2] = ["## Stack", "## Commands"];

    let lines: Vec<&str> = content.lines().collect();
    let mut out_lines: Vec<String> = Vec::with_capacity(lines.len() + 8);
    let mut block_inserted = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let is_machine_owned = line.starts_with("## ")
            && MACHINE_OWNED.iter().any(|h| line.trim_end() == *h);
        if is_machine_owned {
            // Insert the block at the position of the first machine-owned
            // section; thereafter just drop the old machine-owned sections.
            if !block_inserted {
                for bline in block.lines() {
                    out_lines.push(bline.to_string());
                }
                block_inserted = true;
            }
            // Skip this section's body until the next `## ` heading or EOF.
            i += 1;
            while i < lines.len() && !lines[i].starts_with("## ") {
                i += 1;
            }
            continue;
        }
        out_lines.push(line.to_string());
        i += 1;
    }

    // Legacy file had neither machine-owned section — append the block after a
    // blank line so the map is still attached (e.g. a hand-written stub).
    if !block_inserted {
        if out_lines.last().is_some_and(|l| !l.trim().is_empty()) {
            out_lines.push(String::new());
        }
        for bline in block.lines() {
            out_lines.push(bline.to_string());
        }
    }

    // Collapse any run of blank lines created by removing sections down to one,
    // then re-emit with a single trailing newline.
    let mut joined = String::with_capacity(content.len() + block.len());
    let mut prev_blank = false;
    for line in &out_lines {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        joined.push_str(line);
        joined.push('\n');
        prev_blank = blank;
    }
    joined
}

/// Run the CLAUDE.md pass (full or default) over all subprojects.
///
/// Returns a [`ClaudeMdResult`] whose fields populate the JSON response in
/// `scan::run`. In `full` mode also creates `{root}/{dir}/.claude/` if absent.
pub fn run_pass(
    root: &Path,
    projects: &[mustard_core::domain::scan::Project],
    full: bool,
) -> ClaudeMdResult {
    if full {
        run_full(root, projects)
    } else {
        run_default(root, projects)
    }
}

fn run_full(
    root: &Path,
    projects: &[mustard_core::domain::scan::Project],
) -> ClaudeMdResult {
    let mut regenerated: Vec<String> = Vec::new();

    for project in projects {
        let dir = root.join(&project.dir);
        let claude_md_path = dir.join("CLAUDE.md");
        let claude_dir = dir.join(".claude");

        // Detect this unit's command set. The subproject is probed first; for a
        // JS/TS leaf the package-manager signal may only exist at the scan root
        // (monorepo lockfile), so the detector ascends toward `root` to resolve
        // it, and prefers the unit's own mined scripts over conventional names.
        // Only resolved (Some) stages render as a `## Commands` row.
        let commands = mustard_core::domain::command_detect::detect_commands_for_unit(
            &dir,
            root,
            &project.scripts,
        );

        // Read existing content so curated sections are preserved — fail-open.
        let existing = std::fs::read_to_string(&claude_md_path).ok();
        let content = render(
            &project.name,
            &project.kind,
            project.code_files,
            &project.frameworks,
            &commands,
            existing.as_deref(),
        );

        // Ensure .claude/ subdir exists
        if let Err(e) = std::fs::create_dir_all(&claude_dir) {
            eprintln!(
                "scan --full: could not create {:?}: {e}",
                claude_dir.display()
            );
        }

        // Write CLAUDE.md (use mustard_core atomic write for safety)
        let write_result =
            mustard_core::io::fs::write_atomic(&claude_md_path, content.as_bytes());
        match write_result {
            Ok(()) => {
                regenerated.push(path_to_string(&claude_md_path));
            }
            Err(e) => {
                eprintln!(
                    "scan --full: could not write {:?}: {e}",
                    claude_md_path.display()
                );
            }
        }
    }

    ClaudeMdResult {
        regenerated,
        oversized: Vec::new(),
    }
}

fn run_default(root: &Path, projects: &[mustard_core::domain::scan::Project]) -> ClaudeMdResult {
    let mut oversized: Vec<OversizedEntry> = Vec::new();

    for project in projects {
        let claude_md_path = root.join(&project.dir).join("CLAUDE.md");
        if let Ok(meta) = std::fs::metadata(&claude_md_path) {
            let bytes = meta.len() as usize;
            if bytes > CLAUDE_MD_WARN_BYTES {
                oversized.push(OversizedEntry {
                    path: path_to_string(&claude_md_path),
                    bytes,
                });
            }
        }
    }

    ClaudeMdResult {
        regenerated: Vec::new(),
        oversized,
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use mustard_core::domain::config::Commands;

    fn no_commands() -> Commands {
        Commands::default()
    }

    #[test]
    fn render_without_existing_creates_scaffold() {
        // (d) existing=None → scaffold: H1 + sentinel block + empty `## Guards`
        // outside the block.
        let out = render("dashboard", "typescript", 42, &[], &no_commands(), None);
        assert!(out.contains("# Dashboard"), "header missing: {out}");
        assert!(out.contains("Tipo: typescript · 42 arquivos"), "map missing: {out}");
        assert!(out.contains(SENTINEL_OPEN), "scan-map open missing: {out}");
        assert!(out.contains(SENTINEL_CLOSE), "scan-map close missing: {out}");
        assert!(out.contains("## Stack"), "stack heading missing: {out}");
        assert!(out.contains("## Guards"), "guards heading missing: {out}");
        assert!(out.contains("<!-- seed DO/DON'T aqui -->"), "seed placeholder missing: {out}");
        // `## Guards` must live OUTSIDE (after) the closing sentinel.
        let close = out.find(SENTINEL_CLOSE).unwrap();
        let guards = out.find("## Guards").unwrap();
        assert!(guards > close, "guards must sit outside the managed block: {out}");
        assert!(out.ends_with('\n'), "missing trailing newline");
    }

    #[test]
    fn render_preserves_arbitrary_sections_outside_block() {
        // (a) Legacy file WITHOUT sentinels: old `## Stack`/`## Commands` are
        // replaced by a single managed block; curated `## Architecture` and
        // `## Guards` survive intact, with zero Stack/Commands duplication.
        let existing = "\
# Dashboard

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

Tipo: typescript · 10 arquivos

## Stack

Tipo: typescript

- old-framework

## Commands

| Task | Command |
|------|---------|
| Build | `old build` |

## Architecture

Layered: ui → domain → io.

## Guards

- Never import from `../apps/cli`
- Always use `Result<T, anyhow::Error>`
";
        let commands = Commands {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            lint: None,
            type_check: None,
        };
        let out = render("dashboard", "rust", 99, &[], &commands, Some(existing));
        // Exactly one managed block.
        assert_eq!(out.matches(SENTINEL_OPEN).count(), 1, "expected one block: {out}");
        // Stack + Commands updated and de-duplicated.
        assert_eq!(out.matches("## Stack").count(), 1, "duplicate Stack: {out}");
        assert_eq!(out.matches("## Commands").count(), 1, "duplicate Commands: {out}");
        assert!(out.contains("Tipo: rust · 99 arquivos"), "map not refreshed: {out}");
        assert!(out.contains("Tipo: rust\n"), "stack type not refreshed: {out}");
        assert!(out.contains("| Build | `cargo build` |"), "build row missing: {out}");
        assert!(!out.contains("old-framework"), "old stack survived: {out}");
        assert!(!out.contains("old build"), "old command survived: {out}");
        // Curated sections survive verbatim.
        assert!(out.contains("## Architecture"), "architecture lost: {out}");
        assert!(out.contains("Layered: ui → domain → io."), "architecture body lost: {out}");
        assert!(out.contains("Never import from"), "guard line 1 lost: {out}");
        assert!(out.contains("Always use `Result<T, anyhow::Error>`"), "guard line 2 lost: {out}");
    }

    #[test]
    fn render_with_sentinel_splices_only_the_block() {
        // (b) File already has the sentinel: only the span between markers is
        // regenerated; bytes before and after are preserved verbatim.
        let existing = "\
# Dashboard

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

<!-- mustard:scan-map -->
Tipo: typescript · 10 arquivos
Pesquise via `mustard-rt run feature` (digest) — não leia o repo direto.
## Stack

Tipo: typescript
<!-- /mustard:scan-map -->

## Architecture

Hand-written prose that must NOT move.

## Guards

- keep me
";
        let prefix = &existing[..existing.find(SENTINEL_OPEN).unwrap()];
        let suffix = &existing[existing.find(SENTINEL_CLOSE).unwrap() + SENTINEL_CLOSE.len()..];
        let out = render("dashboard", "rust", 77, &[], &no_commands(), Some(existing));
        // Prefix and suffix preserved byte-for-byte.
        assert!(out.starts_with(prefix), "prefix changed: {out}");
        assert!(out.ends_with(suffix), "suffix changed: {out}");
        // Block regenerated.
        assert!(out.contains("Tipo: rust · 77 arquivos"), "block not refreshed: {out}");
        assert_eq!(out.matches(SENTINEL_OPEN).count(), 1, "duplicate block: {out}");
        assert!(out.contains("Hand-written prose that must NOT move."), "prose moved: {out}");
        assert!(out.contains("- keep me"), "guard lost: {out}");
    }

    #[test]
    fn render_legacy_migration_is_idempotent() {
        // (c) Idempotence over the migration path: render(out_a) == out_a.
        let existing = "\
# Dashboard

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

## Stack

Tipo: typescript

- old

## Commands

| Task | Command |
|------|---------|
| Build | `old` |

## Architecture

Keep me.

## Guards

- keep me too
";
        let commands = Commands {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            lint: None,
            type_check: None,
        };
        let first = render("dashboard", "rust", 5, &[], &commands, Some(existing));
        let second = render("dashboard", "rust", 5, &[], &commands, Some(&first));
        assert_eq!(first, second, "render must be idempotent over migration");
    }

    #[test]
    fn render_emits_stack_frameworks_and_commands_table() {
        let frameworks = vec!["serde".to_string(), "clap".to_string()];
        let commands = Commands {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            lint: None,
            type_check: Some("cargo check".into()),
        };
        let out = render("rt", "rust", 12, &frameworks, &commands, None);
        // Stack lists frameworks in caller order.
        assert!(out.contains("## Stack\n\nTipo: rust\n"), "stack type line missing: {out}");
        assert!(out.contains("- serde\n- clap\n"), "frameworks order/list missing: {out}");
        // Commands table has only the Some rows, in fixed order, no Lint row.
        assert!(out.contains("## Commands"), "commands heading missing: {out}");
        assert!(out.contains("| Build | `cargo build` |"), "build row missing: {out}");
        assert!(out.contains("| Test | `cargo test` |"), "test row missing: {out}");
        assert!(out.contains("| Type-check | `cargo check` |"), "type-check row missing: {out}");
        assert!(!out.contains("| Lint |"), "lint row must be absent (None): {out}");
    }

    #[test]
    fn render_omits_commands_table_when_all_none() {
        let out = render("lib", "rust", 1, &[], &no_commands(), None);
        assert!(!out.contains("## Commands"), "commands section must be absent: {out}");
        // Stack still renders even with no frameworks.
        assert!(out.contains("## Stack\n\nTipo: rust\n"), "stack must still render: {out}");
    }

    #[test]
    fn render_is_idempotent_byte_for_byte() {
        let frameworks = vec!["react".to_string()];
        let commands = Commands {
            build: Some("pnpm run build".into()),
            test: Some("pnpm test".into()),
            lint: Some("pnpm run lint".into()),
            type_check: Some("tsc --noEmit".into()),
        };
        let first = render("dashboard", "typescript", 30, &frameworks, &commands, None);
        // Feeding the previous render back in must reproduce it byte-for-byte:
        // the scaffold's sentinel block round-trips through the splice path
        // unchanged, and the curated `## Guards` seed is preserved outside it.
        let second = render("dashboard", "typescript", 30, &frameworks, &commands, Some(&first));
        assert_eq!(first, second, "render must be idempotent");
    }

    #[test]
    fn default_mode_collects_oversized_and_ignores_small() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Subproject 1: large file
        let sub1 = root.join("apps").join("big");
        std::fs::create_dir_all(&sub1).expect("mkdir big");
        let big_path = sub1.join("CLAUDE.md");
        let big_content = "x".repeat(CLAUDE_MD_WARN_BYTES + 1);
        std::fs::File::create(&big_path)
            .and_then(|mut f| f.write_all(big_content.as_bytes()))
            .expect("write big");

        // Subproject 2: small file
        let sub2 = root.join("apps").join("small");
        std::fs::create_dir_all(&sub2).expect("mkdir small");
        let small_path = sub2.join("CLAUDE.md");
        std::fs::File::create(&small_path)
            .and_then(|mut f| f.write_all(b"tiny"))
            .expect("write small");

        let projects = vec![
            mustard_core::domain::scan::Project {
                name: "big".into(),
                dir: "apps/big".into(),
                kind: "rust".into(),
                code_files: 1,
                frameworks: Vec::new(),
                dependencies: Vec::new(),
                scripts: Vec::new(),
            },
            mustard_core::domain::scan::Project {
                name: "small".into(),
                dir: "apps/small".into(),
                kind: "rust".into(),
                code_files: 1,
                frameworks: Vec::new(),
                dependencies: Vec::new(),
                scripts: Vec::new(),
            },
        ];

        let result = run_default(root, &projects);
        assert_eq!(result.oversized.len(), 1, "only the big file should be flagged");
        assert!(result.oversized[0].path.contains("big"), "wrong file flagged");
        assert!(result.oversized[0].bytes > CLAUDE_MD_WARN_BYTES);
        assert!(result.regenerated.is_empty());
    }
}
