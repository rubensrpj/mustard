//! Deterministic scan-map generator for subprojects — no AI, no source reads.
//!
//! Invoked by `scan::run` (`--full`) after `grain.model.json` is written. The
//! ownership split is the contract:
//!
//! - **`CLAUDE.md` belongs to the PROJECT.** For a SUBPROJECT Mustard writes
//!   at most three things into it, all idempotent and minimal: the one-line
//!   [`MAP_IMPORT_LINE`] at the top (Claude Code's native `@path` import, so
//!   the map still auto-loads with the file), a `## Guards` seed when the
//!   section is an un-curated placeholder (humans curate it afterwards), and a
//!   depth-correct breadcrumb heal. Its SIZE is the human's business — never
//!   measured, never refused.
//! - **The WORKSPACE-ROOT `CLAUDE.md` is never touched** (orchestrator
//!   redesign): no scaffold, no import line, no breadcrumb heal — root
//!   orientation is injected by the session hooks (`mustard.json#inject` +
//!   the terrain census), so the root file is fully the user's. The root's
//!   `.claude/scan-map.md` IS still written — that file is Mustard's.
//! - **`.claude/scan-map.md` belongs to MUSTARD.** The machine map (kind +
//!   size + digest pointer + detected `## Commands`) is regenerated there on
//!   every pass, capped by [`SCAN_MAP_HARD_CAP_BYTES`] as a guard against a
//!   runaway generator.
//!
//! Migration: older passes spliced the map INTO `CLAUDE.md` between
//! [`SENTINEL_OPEN`] / [`SENTINEL_CLOSE`]. That block is machine-owned by
//! definition, so the first new pass removes it and the import line takes its
//! place; every byte outside the sentinels is preserved verbatim.

use std::fmt::Write as _;
use std::path::Path;

use crate::commands::spec::spec_sections::section_end;
use mustard_core::domain::vocabulary::stacks::StackDetection;

/// Hard ceiling on the machine-owned `.claude/scan-map.md`. The map is a terse
/// orientation file (~200 bytes + an optional commands table); a file this
/// large means the GENERATOR ran away, so `run_full` refuses to write it and
/// reports a deterministic error. Curated `CLAUDE.md` prose is never measured
/// against this (or any) ceiling.
pub const SCAN_MAP_HARD_CAP_BYTES: usize = 8192;

/// The mustard-owned map file, relative to each unit's directory
/// (forward-slashed — it doubles as the import target in [`MAP_IMPORT_LINE`]).

/// The single line mustard injects at the TOP of a unit's `CLAUDE.md`: Claude
/// Code's native `@path` import (resolved relative to the importing file), so
/// the map loads into context together with the CLAUDE.md exactly as the old
/// inline block did. Injected once — a file already carrying the line is left
/// unchanged. NOTE: the path must stay bare (no backticks) — Claude Code skips
/// imports inside code spans.
pub const MAP_IMPORT_LINE: &str = "@.claude/scan-map.md";

/// Opening marker of the LEGACY inline machine block. Kept only so the
/// migration can find and remove the block older passes spliced into
/// `CLAUDE.md`; new passes never write these markers.
const SENTINEL_OPEN: &str = "<!-- mustard:scan-map -->";
/// Closing marker of the legacy inline machine block.
const SENTINEL_CLOSE: &str = "<!-- /mustard:scan-map -->";

/// Opening marker of the enrichable `## Guards` block. The literal ` pending`
/// suffix is the WAVE-2 hand-off contract: `scan-guards-list` finds every block
/// still carrying it, and `scan-guards-apply` swaps it for the enriched guards.
/// Keep the open marker and its `pending` token byte-stable — downstream tooling
/// matches on this exact string. `pub` so `scan_guards` reuses it as the single
/// source of the marker literal (no drift).
pub const GUARDS_PENDING_OPEN: &str = "<!-- mustard:guards pending -->";
/// Opening marker of an ALREADY-enriched `## Guards` block — the `pending`
/// token dropped. `scan-guards-apply` swaps [`GUARDS_PENDING_OPEN`] for this on
/// first enrich so a re-run of `scan-guards-list` no longer matches the block
/// (idempotence). Defined here, beside its sibling, as the single source.
pub const GUARDS_DONE_OPEN: &str = "<!-- mustard:guards -->";
/// Closing marker of the enrichable `## Guards` block (pairs with
/// [`GUARDS_PENDING_OPEN`]). Wave 2 rewrites the span between the two markers.
/// `pub` so `scan_guards` reuses it (single source of the literal).
pub const GUARDS_CLOSE: &str = "<!-- /mustard:guards -->";

/// Result of running the scan-map pass over a set of projects.
pub struct ClaudeMdResult {
    /// Paths (re)written this pass: every `.claude/scan-map.md`, plus any
    /// `CLAUDE.md` that actually changed (import injected, legacy block
    /// migrated out, Guards placeholder reseeded, breadcrumb healed).
    pub regenerated: Vec<String>,
    /// Machine map files whose RENDERED size exceeded
    /// [`SCAN_MAP_HARD_CAP_BYTES`] and were therefore NOT written (path + byte
    /// count) — a runaway-generator guard. A non-empty list is a hard failure
    /// the caller must surface. `CLAUDE.md` is never measured.
    pub over_cap: Vec<OversizedEntry>,
}

#[derive(Debug)]
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

/// Render the mustard-owned `.claude/scan-map.md` for a unit: a terse
/// orientation map (kind + size + the digest pointer) plus the `## Commands`
/// section — and only when the caller passes NON-DEFAULT commands (it zeroes
/// the conventional language defaults, so `render_commands` omits the
/// section). The dependency `## Stack` was dropped on purpose: a dep list is
/// auto-inferable from the manifest, so it is token noise, not signal. The
/// whole FILE is machine-owned, so no sentinels are needed; ends in exactly
/// one newline (byte-stable).
pub(crate) fn render_map(
    kind: &str,
    code_files: usize,
    commands: &mustard_core::domain::config::Commands,
) -> String {
    let commands_block = render_commands(commands);

    let mut out = String::new();
    let _ = writeln!(out, "Tipo: {kind} · {code_files} arquivos");
    let _ = writeln!(
        out,
        "O terreno já está na sua janela (o census de orientação injetado no início da sessão). Para localizar: `grep` para termo exato conhecido; `mustard-rt run feature` (digest) para conceito; depois leia os arquivos apontados — o digest acha onde olhar, não substitui ler."
    );
    if !commands_block.is_empty() {
        out.push('\n');
        // `render_commands` already ends in a newline.
        out.push_str(&commands_block);
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

/// Build the enrichable `## Guards` section for a SUBPROJECT: a `pending`
/// sentinel block ([`GUARDS_PENDING_OPEN`] … [`GUARDS_CLOSE`]) whose body carries
/// the deterministic facts (kind, frameworks, detected stacks) the Wave-2 enrich
/// agent needs as context, tucked inside an HTML comment so they never render as
/// prose. The returned string is a complete section (`## Guards\n\n` + block)
/// ending in a newline. The block stays empty of guards on purpose — Wave 2
/// fills it; the `pending` marker is the contract that it has not been enriched
/// yet.
///
/// The `stacks=` segment (`stacks=laravel(0.95),nextjs(0.65)`) is emitted only
/// when `stacks` is non-empty, so a unit without detections renders the legacy
/// line byte-for-byte. `frameworks=` stays regardless — it is the raw
/// frequency-ranked dep list, a different signal than the inferred stacks.
/// `pub(crate)` so `scan_guards::list` can round-trip the real generator output
/// through its `parse_facts` in tests (generator/parser never drift).
pub(crate) fn build_guards_block(
    kind: &str,
    frameworks: &[String],
    stacks: &[StackDetection],
    scripts: &[String],
) -> String {
    let fw = if frameworks.is_empty() { "(none)".to_string() } else { frameworks.join(", ") };
    let mut facts = format!("kind={kind}; frameworks={fw}");
    if !stacks.is_empty() {
        let joined = stacks
            .iter()
            // `{:.2}` keeps the segment byte-stable (engine confidences are
            // two-decimal by construction) and round-trip-safe for the parser.
            .map(|s| format!("{}({:.2})", s.name, s.confidence))
            .collect::<Vec<_>>()
            .join(",");
        let _ = write!(facts, "; stacks={joined}");
    }
    // Mined build/codegen scripts (manifest-declared) — emitted only when the
    // unit has any, so a script-less unit renders the legacy line byte-for-byte.
    // The enrich agent grounds codegen rules on these (e.g. "X is a codegen
    // step — regenerate, never hand-edit its output"); a fact, mined by
    // recurrence, never named knowledge. Order-preserving (manifest order).
    if !scripts.is_empty() {
        let _ = write!(facts, "; scripts={}", scripts.join(", "));
    }
    let mut out = String::from("## Guards\n\n");
    let _ = writeln!(out, "{GUARDS_PENDING_OPEN}");
    // Facts for the enrich agent — kept in a comment so they are context, not
    // content. Wave 2 (`scan-guards-apply`) reads these to ground the guards.
    let _ = writeln!(out, "<!-- facts: {facts} -->");
    out.push_str(GUARDS_CLOSE);
    out.push('\n');
    out
}

/// Render a SUBPROJECT's CLAUDE.md — mustard's footprint is minimal and
/// idempotent. (The workspace-root `CLAUDE.md` never reaches this function:
/// `run_full` skips the root entirely — orchestrator redesign.)
///
/// - `name`: subproject name (will be title-cased for the H1 heading)
/// - `kind`/`frameworks`/`stacks`/`scripts`: deterministic facts for the
///   Guards seed
/// - `existing`: current content of the CLAUDE.md (if the file exists)
///
/// When `existing` is present: (1) the LEGACY inline machine block (between
/// the sentinels) is removed — migration; the map now lives in
/// `.claude/scan-map.md`; (2) an un-curated `## Guards` placeholder is
/// reseeded to the `pending` enrich block; (3) the [`MAP_IMPORT_LINE`] is
/// ensured at the top. Every other byte is the project's and is preserved
/// verbatim; file size is never measured. When `existing` is `None`, a fresh
/// minimal scaffold is emitted. Pure function of its inputs — re-rendering the
/// output reproduces it byte-for-byte (idempotent).
pub fn render_claude_md(
    name: &str,
    kind: &str,
    frameworks: &[String],
    stacks: &[StackDetection],
    scripts: &[String],
    existing: Option<&str>,
) -> String {
    match existing {
        Some(content) => {
            let migrated = strip_sentinel_block(content);
            // Turn an un-curated `## Guards` placeholder (empty / `(populated by
            // /scan)` / legacy seed) into a fresh `pending` enrich block so the
            // enrich step picks the subproject up. Curated guards and an already
            // pending/done block survive untouched.
            let reseeded =
                reseed_guards_if_placeholder(&migrated, kind, frameworks, stacks, scripts);
            ensure_import_line(&reseeded)
        }
        // No file yet — emit a fresh minimal scaffold.
        None => scaffold(name, kind, frameworks, stacks, scripts),
    }
}

/// Emit a fresh subproject CLAUDE.md: the map import line on top, then H1 +
/// Parent line + the enrichable `pending` `## Guards` block (the enrich fills
/// it). Everything below the import line is the project's to grow.
fn scaffold(
    name: &str,
    kind: &str,
    frameworks: &[String],
    stacks: &[StackDetection],
    scripts: &[String],
) -> String {
    let title = title_case(name);
    let guards = build_guards_block(kind, frameworks, stacks, scripts);
    format!(
        "{MAP_IMPORT_LINE}\n\
         \n\
         # {title}\n\
         \n\
         > Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/mustard/orchestrator.md](../.claude/mustard/orchestrator.md)\n\
         \n\
         {guards}"
    )
}

/// Remove the LEGACY inline machine block (sentinel span) from `content`,
/// tidying the whitespace the removal leaves behind (runs of 3+ newlines
/// collapse to a blank line; exactly one trailing newline). A file without the
/// sentinels — including every file already migrated — passes through with
/// only the trailing-newline normalisation, so the migration is idempotent.
fn strip_sentinel_block(content: &str) -> String {
    let without = match find_sentinel_span(content) {
        Some((start, end)) => format!("{}{}", &content[..start], &content[end..]),
        None => content.to_string(),
    };
    let mut tidy = without;
    while tidy.contains("\n\n\n") {
        tidy = tidy.replace("\n\n\n", "\n\n");
    }
    let body = tidy.trim_end();
    if body.is_empty() {
        String::new()
    } else {
        format!("{body}\n")
    }
}

/// Ensure the [`MAP_IMPORT_LINE`] is present — injected as the FIRST line
/// (followed by a blank) when missing; a file already carrying it anywhere is
/// returned unchanged (idempotent). Exact-line match, so a backticked mention
/// in prose does not count as presence.
fn ensure_import_line(content: &str) -> String {
    if content.lines().any(|l| l.trim() == MAP_IMPORT_LINE) {
        return content.to_string();
    }
    if content.trim().is_empty() {
        return format!("{MAP_IMPORT_LINE}\n");
    }
    format!("{MAP_IMPORT_LINE}\n\n{content}")
}

/// Join `lines`, collapsing runs of blank lines to a single blank and trimming a
/// leading/trailing blank, with exactly one trailing newline. Keeps the spliced
/// output tidy after sections are purged.
fn collapse_blanks(lines: &[String]) -> String {
    let mut out = String::new();
    let mut prev_blank = false;
    for line in lines {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        out.push_str(line);
        out.push('\n');
        prev_blank = blank;
    }
    format!("{}\n", out.trim_matches('\n'))
}

/// Reseed an un-curated `## Guards` section with a fresh `pending` enrich block.
///
/// A subproject whose `## Guards` is empty, the `(populated by /scan)` stub, or
/// the legacy `<!-- seed … -->` human seed has never been enriched, yet the
/// non-destructive render preserves it as "existing content" — so the enrich
/// worklist (`scan-guards-list`, which matches only `pending`) never sees it.
/// Replacing such a placeholder with a `pending` block lets `/scan --enrich`
/// pick the subproject up. Curated human guards (any real line) and an already
/// `pending`/`done` block are left byte-for-byte untouched. (The workspace
/// root never reaches this — `run_full` skips it.)
fn reseed_guards_if_placeholder(
    text: &str,
    kind: &str,
    frameworks: &[String],
    stacks: &[StackDetection],
    scripts: &[String],
) -> String {
    const PLACEHOLDERS: [&str; 2] = ["(populated by /scan)", "<!-- seed DO/DON'T aqui -->"];
    let lines: Vec<&str> = text.lines().collect();
    let Some(g) = lines.iter().position(|l| l.trim_end() == "## Guards") else {
        return text.to_string();
    };
    let body_start = g + 1;
    let body_end = section_end(&lines, g);
    let body = &lines[body_start..body_end];
    // Already an enrich-managed block (pending or done) — never overwrite it.
    if body.iter().any(|l| l.contains(GUARDS_PENDING_OPEN) || l.contains(GUARDS_DONE_OPEN)) {
        return text.to_string();
    }
    // Curated iff any non-blank body line is NOT a known placeholder stub. Any
    // real line means a human wrote it — preserve verbatim.
    let curated = body
        .iter()
        .map(|l| l.trim())
        .any(|l| !l.is_empty() && !PLACEHOLDERS.contains(&l));
    if curated {
        return text.to_string();
    }
    // Swap the whole placeholder section for a fresh `pending` block.
    let mut out: Vec<String> = lines[..g].iter().map(|s| (*s).to_string()).collect();
    out.extend(build_guards_block(kind, frameworks, stacks, scripts).lines().map(str::to_string));
    out.extend(lines[body_end..].iter().map(|s| (*s).to_string()));
    collapse_blanks(&out)
}

/// The Parent/Orchestrator breadcrumb line for a unit at `dir` (relative to the
/// scan root, forward-slashed). The number of `../` hops equals the unit's depth,
/// so a depth-2 subproject (`apps/dashboard`) links to `../../CLAUDE.md`, not the
/// non-existent `../CLAUDE.md`. The workspace root (depth 0) has no parent, so it
/// gets an Orchestrator-only line. The orchestrator lives at
/// `.claude/mustard/orchestrator.md` (the injectable the hooks deliver — the
/// planted `.claude/CLAUDE.md` is gone).
fn breadcrumb(dir: &str) -> String {
    let depth = dir.split('/').filter(|s| !s.is_empty()).count();
    if depth == 0 {
        return "> Orchestrator: [.claude/mustard/orchestrator.md](.claude/mustard/orchestrator.md)"
            .to_string();
    }
    let up = "../".repeat(depth);
    format!(
        "> Parent: [{up}CLAUDE.md]({up}CLAUDE.md) | Orchestrator: \
         [{up}.claude/mustard/orchestrator.md]({up}.claude/mustard/orchestrator.md)"
    )
}

/// Replace the existing `> Parent:` / `> Orchestrator:` breadcrumb line with the
/// depth-correct one for `dir`, in place. A file last written with the old fixed
/// `../` breadcrumb is healed on the next render; an already-correct line is a
/// no-op (byte-identical). When no breadcrumb line exists the text is returned
/// unchanged (a fresh scaffold already carries one).
fn fix_breadcrumb(text: &str, dir: &str) -> String {
    let want = breadcrumb(dir);
    let lines: Vec<&str> = text.lines().collect();
    let Some(i) = lines.iter().position(|l| {
        let t = l.trim_start();
        t.starts_with("> Parent:") || t.starts_with("> Orchestrator:")
    }) else {
        return text.to_string();
    };
    if lines[i] == want {
        return text.to_string();
    }
    let mut out: Vec<String> = lines.iter().map(|s| (*s).to_string()).collect();
    out[i] = want;
    format!("{}\n", out.join("\n"))
}

/// Run the scan-map pass over all subprojects (only `--full` does work — the
/// default mode has nothing to check: `CLAUDE.md` size is the project's
/// business, and the machine map is only rewritten by a full pass).
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
        ClaudeMdResult { regenerated: Vec::new(), over_cap: Vec::new() }
    }
}

fn run_full(
    root: &Path,
    projects: &[mustard_core::domain::scan::Project],
) -> ClaudeMdResult {
    let mut regenerated: Vec<String> = Vec::new();
    let mut over_cap: Vec<OversizedEntry> = Vec::new();

    for project in projects {
        let dir = root.join(&project.dir);
        let claude_md_path = dir.join("CLAUDE.md");
        let claude_dir = dir.join(".claude");
        let map_path = claude_dir.join("scan-map.md");
        // The workspace-root unit (empty `dir`): its CLAUDE.md is NEVER
        // touched (orchestrator redesign — root orientation is injected by
        // the session hooks, not written into the user's file). Only the
        // mustard-owned `.claude/scan-map.md` is still produced for it.
        let is_root = project.dir.is_empty();

        // Detect this unit's command set. The subproject is probed first; for a
        // JS/TS leaf the package-manager signal may only exist at the scan root
        // (monorepo lockfile), so the detector ascends toward `root` to resolve
        // it, and prefers the unit's own mined scripts over conventional names.
        // Only resolved (Some) stages render as a `## Commands` row.
        let (detected, commands_custom) = mustard_core::domain::command_detect::detect_commands_for_unit(
            &dir,
            root,
            &project.scripts,
        );
        // `## Commands` earns its place only when the unit has NON-DEFAULT commands
        // (mined from real scripts). Conventional language defaults (`cargo build`,
        // …) are auto-inferable noise, so they are zeroed here and the section is
        // omitted by `render_commands`.
        let commands = if commands_custom {
            detected
        } else {
            mustard_core::domain::config::Commands::default()
        };

        // Ensure .claude/ subdir exists
        if let Err(e) = std::fs::create_dir_all(&claude_dir) {
            eprintln!(
                "scan --full: could not create {:?}: {e}",
                claude_dir.display()
            );
        }

        // --- 1. The mustard-owned map file ---------------------------------
        // Hard cap guards MUSTARD's own output only: a map this large means the
        // generator ran away, so refuse the write and surface it. Deterministic
        // — the outcome is a pure function of the rendered byte length.
        let map = render_map(&project.kind, project.code_files, &commands);
        if map.len() > SCAN_MAP_HARD_CAP_BYTES {
            eprintln!(
                "scan --full: refusing to write {:?}: {} bytes exceeds hard cap of {} — runaway machine map",
                map_path.display(),
                map.len(),
                SCAN_MAP_HARD_CAP_BYTES,
            );
            over_cap.push(OversizedEntry {
                path: path_to_string(&map_path),
                bytes: map.len(),
            });
        } else {
            match mustard_core::io::fs::write_atomic(&map_path, map.as_bytes()) {
                Ok(()) => regenerated.push(path_to_string(&map_path)),
                Err(e) => eprintln!(
                    "scan --full: could not write {:?}: {e}",
                    map_path.display()
                ),
            }
        }

        // --- 2. The project's CLAUDE.md (SUBPROJECTS ONLY) -----------------
        // The workspace root is skipped entirely: no scaffold, no import
        // line, no breadcrumb heal — the root file belongs to the user
        // (orchestrator redesign; the hooks inject the orientation instead).
        if is_root {
            continue;
        }
        // Mustard's footprint here is minimal: import line + legacy-block
        // migration + Guards reseed + breadcrumb heal. Never measured against
        // any size ceiling — the file (and its size) belongs to the project.
        let existing = std::fs::read_to_string(&claude_md_path).ok();
        let content = render_claude_md(
            &project.name,
            &project.kind,
            &project.frameworks,
            &project.detected_stacks,
            &project.scripts,
            existing.as_deref(),
        );
        // The Parent/Orchestrator breadcrumb is a function of the unit's depth
        // below the scan root, not a fixed `../`. Regenerate it so deep
        // subprojects (`apps/dashboard`, depth 2 → `../../CLAUDE.md`) link to the
        // real targets instead of the non-existent `../CLAUDE.md`. Self-healing
        // on every `--full`.
        let content = fix_breadcrumb(&content, &project.dir);

        // Only touch the project's file when something actually changed
        // (first import injection, migration, reseed, heal) — a settled
        // CLAUDE.md is not rewritten.
        if existing.as_deref() != Some(content.as_str()) {
            match mustard_core::io::fs::write_atomic(&claude_md_path, content.as_bytes()) {
                Ok(()) => regenerated.push(path_to_string(&claude_md_path)),
                Err(e) => eprintln!(
                    "scan --full: could not write {:?}: {e}",
                    claude_md_path.display()
                ),
            }
        }
    }

    ClaudeMdResult { regenerated, over_cap }
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
        // existing=None on a subproject → minimal scaffold: import line on
        // TOP, H1, breadcrumb, enrichable `pending` Guards block. No inline
        // machine block — the map lives in `.claude/scan-map.md`.
        let out = render_claude_md("dashboard", "typescript", &[], &[], &[], None);
        assert!(out.starts_with(MAP_IMPORT_LINE), "import line must open the file: {out}");
        assert!(out.contains("# Dashboard"), "header missing: {out}");
        assert!(out.contains("## Guards"), "guards heading missing: {out}");
        assert!(out.contains(GUARDS_PENDING_OPEN), "pending guards block missing: {out}");
        // No legacy sentinels in fresh output — the map is not inlined anymore.
        assert!(!out.contains(SENTINEL_OPEN), "no inline machine block in new scaffolds: {out}");
        assert!(out.ends_with('\n'), "missing trailing newline");
    }

    #[test]
    fn guards_pending() {
        // A fresh SUBPROJECT (is_root=false) gets the enrichable `## Guards`
        // block: a `pending` sentinel carrying the deterministic facts (kind,
        // frameworks) in a comment for the Wave-2 enrich agent.
        let frameworks = vec!["serde".to_string(), "clap".to_string()];
        let out = render_claude_md("rt", "rust", &frameworks, &[], &[], None);
        assert!(out.contains(GUARDS_PENDING_OPEN), "pending open marker missing: {out}");
        assert!(out.contains(GUARDS_CLOSE), "guards close marker missing: {out}");
        // Facts live in a comment inside the block — context, not content.
        assert!(out.contains("<!-- facts: kind=rust; frameworks=serde, clap -->"), "facts comment missing: {out}");
        // The marker carries the literal `pending` token Wave 2 matches on.
        assert!(GUARDS_PENDING_OPEN.contains("pending"), "open marker lost its pending token");
        // No frameworks → facts still render with an explicit (none).
        let bare = render_claude_md("lib", "rust", &[], &[], &[], None);
        assert!(bare.contains("<!-- facts: kind=rust; frameworks=(none) -->"), "empty frameworks facts: {bare}");
        // Idempotence: re-rendering the scaffold preserves the pending block.
        let again = render_claude_md("rt", "rust", &frameworks, &[], &[], Some(&out));
        assert!(again.contains(GUARDS_PENDING_OPEN), "pending marker lost on re-render: {again}");
        assert_eq!(out, again, "scaffold must round-trip byte-for-byte");
    }

    #[test]
    fn stacks_facts_guards_block_emits_segment() {
        let frameworks = vec!["serde".to_string()];
        let stacks = vec![
            StackDetection {
                name: "laravel".into(),
                confidence: 0.95,
                signals: vec!["dep:laravel/framework".into()],
            },
            StackDetection { name: "nextjs".into(), confidence: 0.65, signals: vec!["dep:next".into()] },
        ];
        // With detections the facts line gains the `stacks=` segment —
        // name(confidence) tokens, comma-joined; signals stay off the line so
        // the comment stays terse. `frameworks=` survives beside it.
        let with = build_guards_block("rust", &frameworks, &stacks, &[]);
        assert!(
            with.contains("<!-- facts: kind=rust; frameworks=serde; stacks=laravel(0.95),nextjs(0.65) -->"),
            "stacks segment missing or malformed: {with}"
        );
        // Without detections the whole block is byte-identical to the legacy form.
        let without = build_guards_block("rust", &frameworks, &[], &[]);
        assert_eq!(
            without,
            format!(
                "## Guards\n\n{GUARDS_PENDING_OPEN}\n<!-- facts: kind=rust; frameworks=serde -->\n{GUARDS_CLOSE}\n"
            ),
            "empty stacks must reproduce the legacy block exactly"
        );
        // End-to-end through render: a scaffolded subproject carries the segment.
        let out = render_claude_md("rt", "rust", &frameworks, &stacks, &[], None);
        assert!(out.contains("stacks=laravel(0.95),nextjs(0.65)"), "render did not thread stacks: {out}");
    }

    #[test]
    fn scripts_facts_guards_block_emits_segment() {
        // Mined codegen/build scripts ride the facts line as a `scripts=`
        // segment so the enrich agent can ground a "X is codegen — regenerate,
        // never hand-edit its output" rule. Order-preserving; emitted only when
        // present, so a script-less unit reproduces the legacy line byte-for-byte.
        let frameworks = vec!["serde".to_string()];
        let scripts = vec!["generate:api".to_string(), "build".to_string()];
        let with = build_guards_block("rust", &frameworks, &[], &scripts);
        assert!(
            with.contains("<!-- facts: kind=rust; frameworks=serde; scripts=generate:api, build -->"),
            "scripts segment missing or malformed: {with}"
        );
        // Sits AFTER the stacks segment when both are present (terse, stable order).
        let stacks = vec![StackDetection { name: "laravel".into(), confidence: 0.95, signals: vec![] }];
        let both = build_guards_block("php", &[], &stacks, &scripts);
        assert!(
            both.contains("stacks=laravel(0.95); scripts=generate:api, build -->"),
            "scripts must follow stacks: {both}"
        );
        // Script-less unit is byte-identical to the legacy line.
        let without = build_guards_block("rust", &frameworks, &[], &[]);
        assert!(
            without.contains("<!-- facts: kind=rust; frameworks=serde -->"),
            "no scripts ⇒ legacy line: {without}"
        );
        // End-to-end through render threads the unit's scripts.
        let out = render_claude_md("rt", "rust", &frameworks, &[], &scripts, None);
        assert!(out.contains("scripts=generate:api, build"), "render did not thread scripts: {out}");
    }

    #[test]
    fn render_injects_import_and_preserves_human_file() {
        // A legacy HUMAN file without sentinels: mustard's only footprint is the
        // import line at the top — every human section (`## Stack`, `## Commands`,
        // `## Architecture`, curated `## Guards`) survives verbatim, whatever its
        // size. No inline machine block is ever appended again.
        let existing = "\
# Dashboard

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

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
        let out = render_claude_md("dashboard", "rust", &[], &[], &[], Some(existing));
        assert!(out.starts_with(MAP_IMPORT_LINE), "import line must open the file: {out}");
        assert!(!out.contains(SENTINEL_OPEN), "no inline machine block anymore: {out}");
        // Human sections survive verbatim — even ones that look machine-ish.
        assert!(out.contains("## Stack"), "human Stack stripped: {out}");
        assert!(out.contains("old-framework"), "human Stack body stripped: {out}");
        assert!(out.contains("| Build | `old build` |"), "human Commands stripped: {out}");
        assert!(out.contains("Layered: ui → domain → io."), "architecture body lost: {out}");
        assert!(out.contains("Never import from"), "guard line 1 lost: {out}");
        assert!(out.contains("Always use `Result<T, anyhow::Error>`"), "guard line 2 lost: {out}");
        // Idempotent: the import is injected exactly once.
        let again = render_claude_md("dashboard", "rust", &[], &[], &[], Some(&out));
        assert_eq!(out, again, "render must be idempotent");
        assert_eq!(out.matches(MAP_IMPORT_LINE).count(), 1, "import injected once: {out}");
    }

    #[test]
    fn render_migrates_legacy_inline_block_out() {
        // A file carrying the LEGACY inline machine block (sentinels): the
        // migration removes the whole block — it is machine-owned by definition —
        // injects the import line, and preserves every byte outside the span.
        let existing = "\
# Dashboard

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

<!-- mustard:scan-map -->
Tipo: typescript · 10 arquivos
Pesquise via `mustard-rt run feature` (digest) — não leia o repo direto.
<!-- /mustard:scan-map -->

## Architecture

Hand-written prose that must NOT move.

## Guards

- keep me
";
        let out = render_claude_md("dashboard", "rust", &[], &[], &[], Some(existing));
        assert!(out.starts_with(MAP_IMPORT_LINE), "import line must open the file: {out}");
        assert!(!out.contains(SENTINEL_OPEN), "legacy block must be migrated out: {out}");
        assert!(!out.contains("Tipo: typescript · 10 arquivos"), "legacy map body must go: {out}");
        assert!(out.contains("# Dashboard"), "header lost: {out}");
        assert!(out.contains("Hand-written prose that must NOT move."), "prose lost: {out}");
        assert!(out.contains("- keep me"), "guard lost: {out}");
        // Idempotent over the migration.
        let again = render_claude_md("dashboard", "rust", &[], &[], &[], Some(&out));
        assert_eq!(out, again, "migration must be idempotent");
    }

    #[test]
    fn render_reseeds_placeholder_guards_for_subproject() {
        // A subproject whose `## Guards` is the `(populated by /scan)` stub is
        // reseeded to a `pending` block so the enrich worklist picks it up.
        let stub = "\
# Dashboard

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

<!-- mustard:scan-map -->
Tipo: npm · 1 arquivos
<!-- /mustard:scan-map -->

## Guards

(populated by /scan)
";
        let out = render_claude_md("dashboard", "npm", &["react".into()], &[], &[], Some(stub));
        assert!(out.contains(GUARDS_PENDING_OPEN), "stub not reseeded to pending: {out}");
        assert!(!out.contains("(populated by /scan)"), "stub survived: {out}");
        // Idempotent: a re-render keeps the pending block (does not re-reseed).
        let again = render_claude_md("dashboard", "npm", &["react".into()], &[], &[], Some(&out));
        assert_eq!(out, again, "reseed must be idempotent");

        // Curated human guards are preserved, never reseeded.
        let curated = "\
# Cli

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

<!-- mustard:scan-map -->
Tipo: cargo · 1 arquivos
<!-- /mustard:scan-map -->

## Guards

- Real human rule that must stay.
";
        let out2 = render_claude_md("cli", "cargo", &[], &[], &[], Some(curated));
        assert!(out2.contains("- Real human rule that must stay."), "curated guard lost: {out2}");
        assert!(!out2.contains(GUARDS_PENDING_OPEN), "curated guards wrongly reseeded: {out2}");
        // (The workspace-root CLAUDE.md never reaches the renderer at all —
        // see `run_full_never_touches_the_root_claude_md`.)
    }

    #[test]
    fn breadcrumb_depth_and_fix() {
        // Depth drives the number of `../` hops.
        assert_eq!(
            breadcrumb(""),
            "> Orchestrator: [.claude/mustard/orchestrator.md](.claude/mustard/orchestrator.md)"
        );
        assert!(breadcrumb("apps/dashboard").contains("[../../CLAUDE.md](../../CLAUDE.md)"));
        assert!(breadcrumb("apps/dashboard").contains("../../.claude/mustard/orchestrator.md"));
        assert!(breadcrumb("apps/dashboard/src-tauri").contains("[../../../CLAUDE.md]"));

        // fix_breadcrumb heals a wrong fixed-`../` line to the depth-correct one.
        let wrong = "# Dashboard\n\n> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)\n\n## Stack\n";
        let fixed = fix_breadcrumb(wrong, "apps/dashboard");
        assert!(fixed.contains("[../../CLAUDE.md](../../CLAUDE.md)"), "not fixed: {fixed}");
        assert!(!fixed.contains("[../CLAUDE.md]"), "old depth-1 link survived: {fixed}");
        // Idempotent: re-fixing an already-correct file is a byte-for-byte no-op.
        assert_eq!(fix_breadcrumb(&fixed, "apps/dashboard"), fixed);

        // The root drops the meaningless `> Parent` entirely.
        let root_wrong = "# (root)\n\n> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)\n\n## Stack\n";
        let root_fixed = fix_breadcrumb(root_wrong, "");
        assert!(
            root_fixed.contains(
                "> Orchestrator: [.claude/mustard/orchestrator.md](.claude/mustard/orchestrator.md)"
            ),
            "root orch link wrong: {root_fixed}"
        );
        assert!(!root_fixed.contains("> Parent:"), "root must not carry Parent: {root_fixed}");
    }

    #[test]
    fn render_legacy_migration_is_idempotent() {
        // Idempotence over the migration path: render(out_a) == out_a.
        let existing = "\
# Dashboard

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

## Architecture

Keep me.

## Guards

- keep me too
";
        let first = render_claude_md("dashboard", "rust", &[], &[], &[], Some(existing));
        let second = render_claude_md("dashboard", "rust", &[], &[], &[], Some(&first));
        assert_eq!(first, second, "render must be idempotent over migration");
    }

    #[test]
    fn map_emits_commands_table_with_only_some_rows() {
        let commands = Commands {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            lint: None,
            type_check: Some("cargo check".into()),
        };
        let out = render_map("rust", 12, &commands);
        assert!(out.contains("Tipo: rust · 12 arquivos"), "map header missing: {out}");
        // Commands table has only the Some rows, in fixed order, no Lint row.
        assert!(out.contains("## Commands"), "commands heading missing: {out}");
        assert!(out.contains("| Build | `cargo build` |"), "build row missing: {out}");
        assert!(out.contains("| Test | `cargo test` |"), "test row missing: {out}");
        assert!(out.contains("| Type-check | `cargo check` |"), "type-check row missing: {out}");
        assert!(!out.contains("| Lint |"), "lint row must be absent (None): {out}");
    }

    #[test]
    fn map_omits_commands_table_when_all_none() {
        let out = render_map("rust", 1, &no_commands());
        assert!(!out.contains("## Commands"), "commands section must be absent: {out}");
        // After the Stack cut there is no `## Stack` section at all.
        assert!(!out.contains("## Stack"), "stack section must be dropped: {out}");
        assert!(out.ends_with('\n'), "map must end in a newline");
    }

    #[test]
    fn map_is_byte_stable() {
        let commands = Commands {
            build: Some("pnpm run build".into()),
            test: Some("pnpm test".into()),
            lint: Some("pnpm run lint".into()),
            type_check: Some("tsc --noEmit".into()),
        };
        assert_eq!(
            render_map("typescript", 30, &commands),
            render_map("typescript", 30, &commands),
            "two renders must produce identical bytes"
        );
    }

    fn project(name: &str, dir: &str) -> mustard_core::domain::scan::Project {
        mustard_core::domain::scan::Project {
            name: name.into(),
            dir: dir.into(),
            kind: "rust".into(),
            code_files: 1,
            frameworks: Vec::new(),
            dependencies: Vec::new(),
            scripts: Vec::new(),
            detected_stacks: Vec::new(),
        }
    }

    #[test]
    fn giant_curated_prose_never_blocks_the_pass() {
        // A CLAUDE.md far beyond any ceiling — because the PROSE is the
        // human's — is still processed: the legacy inline block migrates out,
        // the import line lands on top, the map file is written, and no
        // over_cap entry is recorded. The project's file size is not
        // mustard's business.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        let bloated = format!(
            "# Big\n\n{SENTINEL_OPEN}\nTipo: rust · 1 arquivos\n{SENTINEL_CLOSE}\n\n## Architecture\n\n{}\n",
            "x".repeat(SCAN_MAP_HARD_CAP_BYTES + 1)
        );
        let big_dir = root.join("apps").join("big");
        std::fs::create_dir_all(&big_dir).expect("mkdir big");
        let big_path = big_dir.join("CLAUDE.md");
        std::fs::write(&big_path, &bloated).expect("write big");

        let projects = vec![project("big", "apps/big")];
        let result = run_full(root, &projects);

        assert!(result.over_cap.is_empty(), "curated prose must never trip the cap: {:?}", result.over_cap);
        // The map file was written beside it…
        let map_path = big_dir.join(".claude").join("scan-map.md");
        assert!(map_path.is_file(), "scan-map.md must be written");
        assert!(result.regenerated.iter().any(|p| p.contains("scan-map.md")), "map reported");
        // …and the giant CLAUDE.md was UPDATED (import in, legacy block out),
        // with the human prose intact.
        let updated = std::fs::read_to_string(&big_path).unwrap();
        assert!(updated.starts_with(MAP_IMPORT_LINE), "import line missing: truncated? {}", &updated[..80]);
        assert!(!updated.contains(SENTINEL_OPEN), "legacy block must migrate out");
        assert!(updated.contains(&"x".repeat(64)), "human prose must survive");
    }

    #[test]
    fn run_full_never_touches_the_root_claude_md() {
        // Orchestrator redesign: the workspace-root unit (empty `dir`) gets
        // its mustard-owned `.claude/scan-map.md`, but its `CLAUDE.md` is
        // never created, imported-into, or healed — the file is the user's.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Case A: no root CLAUDE.md exists → the pass must not create one.
        let projects = vec![project("(root)", "")];
        let first = run_full(root, &projects);
        assert!(
            root.join(".claude/scan-map.md").is_file(),
            "the root's mustard-owned map is still written"
        );
        assert!(
            !root.join("CLAUDE.md").exists(),
            "no root CLAUDE.md may be scaffolded"
        );
        assert!(
            !first.regenerated.iter().any(|p| p.ends_with("CLAUDE.md")),
            "no CLAUDE.md reported for the root: {:?}",
            first.regenerated
        );

        // Case B: a user-authored root CLAUDE.md exists WITHOUT the import
        // line → it survives byte-for-byte (no import injected, no heal).
        let user_file = "# My project\n\nMy own rules, my own layout.\n";
        std::fs::write(root.join("CLAUDE.md"), user_file).expect("write root md");
        let second = run_full(root, &projects);
        assert_eq!(
            std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
            user_file,
            "the user's root CLAUDE.md must be byte-identical after the pass"
        );
        assert!(
            !second.regenerated.iter().any(|p| p.ends_with("CLAUDE.md")),
            "the root CLAUDE.md must never be reported as regenerated: {:?}",
            second.regenerated
        );
    }

    #[test]
    fn run_full_writes_map_and_settles_claude_md() {
        // End-to-end steady state: first pass writes the map + updates the
        // CLAUDE.md; a second pass rewrites only the map (the CLAUDE.md is
        // settled and untouched).
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("apps").join("small")).expect("mkdir small");

        let projects = vec![project("small", "apps/small")];
        let first = run_full(root, &projects);
        let claude_md = root.join("apps/small/CLAUDE.md");
        let map = root.join("apps/small/.claude/scan-map.md");
        assert!(claude_md.is_file() && map.is_file(), "both files written on first pass");
        assert!(first.regenerated.iter().any(|p| p.contains("CLAUDE.md")), "fresh scaffold reported");
        let settled = std::fs::read_to_string(&claude_md).unwrap();
        assert!(settled.starts_with(MAP_IMPORT_LINE), "scaffold opens with the import");

        let second = run_full(root, &projects);
        assert!(
            !second.regenerated.iter().any(|p| p.ends_with("CLAUDE.md")),
            "a settled CLAUDE.md is not rewritten: {:?}",
            second.regenerated
        );
        assert!(second.regenerated.iter().any(|p| p.contains("scan-map.md")), "map always refreshes");
        assert_eq!(std::fs::read_to_string(&claude_md).unwrap(), settled, "CLAUDE.md byte-identical");
    }
}
