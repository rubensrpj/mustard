//! Deterministic CLAUDE.md generator for subprojects — no AI, no source reads.
//!
//! Invoked by `scan::run` after `grain.model.json` is written:
//! - `--full`: (re)generates `{root}/{dir}/CLAUDE.md` per subproject. Mustard
//!   owns exactly ONE block, delimited by [`SENTINEL_OPEN`] / [`SENTINEL_CLOSE`]
//!   and kept at the END of the file; each pass removes the prior block from
//!   wherever it sat and re-appends the fresh one at the tail. Every other byte
//!   (curated `## Architecture`, `## Guards`, a hand-written legacy CLAUDE.md, …)
//!   is preserved verbatim and never stripped or reordered — the safe path for
//!   adopting an existing human-authored file.
//! - default: reports files exceeding [`CLAUDE_MD_WARN_BYTES`] as oversized.

use std::fmt::Write as _;
use std::path::Path;

use mustard_core::domain::vocabulary::stacks::StackDetection;

/// Files larger than this threshold trigger a warning in default (non-full) mode.
pub const CLAUDE_MD_WARN_BYTES: usize = 2048;

/// Hard ceiling on a generated CLAUDE.md in `--full` mode. A file this large is
/// no longer a lean orientation map — it is curated prose run amok or a runaway
/// machine block — so `run_full` refuses to write it and reports a deterministic
/// error instead of silently shipping a bloated file. Chosen well above the
/// scaffold + a reasonable hand-written `## Architecture`/`## Guards`, so only a
/// genuine outlier trips it.
pub const CLAUDE_MD_HARD_CAP_BYTES: usize = 8192;

/// Opening marker of the machine-owned block. Everything between this and
/// [`SENTINEL_CLOSE`] is regenerated on each `--full` pass; everything outside
/// it is curated by humans and never touched.
const SENTINEL_OPEN: &str = "<!-- mustard:scan-map -->";
/// Closing marker of the machine-owned block.
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

/// Result of running the CLAUDE.md pass over a set of projects.
pub struct ClaudeMdResult {
    /// Paths regenerated (full mode).
    pub regenerated: Vec<String>,
    /// Oversized files (default mode): path + byte count.
    pub oversized: Vec<OversizedEntry>,
    /// Full mode: rendered files that exceeded [`CLAUDE_MD_HARD_CAP_BYTES`] and
    /// were therefore NOT written (path + byte count). A non-empty list is a hard
    /// failure the caller must surface.
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

/// Build the machine-owned block (delimited by the sentinels) for a unit: a
/// terse orientation map (kind + size + the digest pointer) plus the
/// `## Commands` section — and only when the caller passes NON-DEFAULT commands
/// (it zeroes the conventional language defaults, so `render_commands` omits the
/// section). The dependency `## Stack` was dropped on purpose: a dep list is
/// auto-inferable from the manifest, so it is token noise, not signal. The
/// returned string starts with [`SENTINEL_OPEN`] and ends with [`SENTINEL_CLOSE`]
/// (no trailing newline) so callers control the surrounding whitespace.
fn build_managed_block(
    kind: &str,
    code_files: usize,
    commands: &mustard_core::domain::config::Commands,
) -> String {
    let commands_block = render_commands(commands);

    let mut block = String::new();
    let _ = writeln!(block, "{SENTINEL_OPEN}");
    let _ = writeln!(block, "Tipo: {kind} · {code_files} arquivos");
    let _ = writeln!(
        block,
        "O terreno já está na sua janela (o census de orientação): leia os pontos de entrada indicados. Use `mustard-rt run feature` (digest) só para localizar por conceito ALÉM do census; depois leia os arquivos apontados — o digest acha onde olhar, não substitui ler."
    );
    if !commands_block.is_empty() {
        block.push('\n');
        // `render_commands` already ends in a newline.
        block.push_str(&commands_block);
    }
    // Close marker with no trailing newline — the caller owns spacing.
    block.push_str(SENTINEL_CLOSE);
    block
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

/// Render a lean CLAUDE.md for a subproject.
///
/// - `name`: subproject name (will be title-cased for the H1 heading)
/// - `kind`: grain project kind (e.g. `rust`, `typescript`, …)
/// - `code_files`: number of code files grain counted
/// - `frameworks`: frequency-ranked frameworks/deps mined for this unit
/// - `stacks`: registry-inferred stack detections for this unit (additive next
///   to `frameworks`; empty when nothing was inferred / older model)
/// - `commands`: build/test/lint/type-check set detected for this unit
/// - `existing`: current content of the CLAUDE.md (if the file exists)
/// - `is_root`: the workspace-root unit (empty `dir`). The root is EXCLUDED from
///   enrich, so a freshly-scaffolded root gets the legacy human seed instead of
///   the `pending` Guards block — only subprojects carry the Wave-2 marker.
///
/// Mustard owns exactly ONE block (between [`SENTINEL_OPEN`] and
/// [`SENTINEL_CLOSE`]), kept at the END of the file. When `existing` carries the
/// sentinels the old block is removed from wherever it sat and the fresh one is
/// re-appended at the tail; when `existing` is a legacy/human file without
/// sentinels the block is simply appended at the end. Either way every other byte
/// is preserved verbatim — nothing is stripped or reordered. When `existing` is
/// `None`, a fresh scaffold is emitted (block at the end). The block is a pure
/// function of the inputs, so re-rendering the output reproduces it byte-for-byte
/// (idempotent).
pub fn render(
    name: &str,
    kind: &str,
    code_files: usize,
    frameworks: &[String],
    stacks: &[StackDetection],
    scripts: &[String],
    commands: &mustard_core::domain::config::Commands,
    existing: Option<&str>,
    is_root: bool,
) -> String {
    let block = build_managed_block(kind, code_files, commands);

    match existing {
        // File exists — remove any prior managed block (wherever it sat) and
        // re-append the fresh one at the END. Every other byte is preserved
        // verbatim: a human/legacy CLAUDE.md is never stripped or reordered, and
        // a previously-managed file simply has its block relocated to the tail.
        Some(content) => {
            let without_block = match find_sentinel_span(content) {
                Some((start, end)) => format!("{}{}", &content[..start], &content[end..]),
                None => content.to_string(),
            };
            // Turn an un-curated `## Guards` placeholder (empty / `(populated by
            // /scan)` / legacy seed) into a fresh `pending` enrich block so the
            // optional enrich step picks the subproject up. Curated guards and an
            // already pending/done block survive untouched; root is never reseeded.
            // Reseed BEFORE re-appending the managed block, so the block (now at the
            // tail) never bleeds into the `## Guards` body detection.
            let reseeded =
                reseed_guards_if_placeholder(&without_block, kind, frameworks, stacks, scripts, is_root);
            append_managed_block(&reseeded, &block)
        }
        // No file yet — emit a fresh scaffold.
        None => scaffold(name, &block, kind, frameworks, stacks, scripts, is_root),
    }
}

/// Emit a fresh CLAUDE.md: H1 + Parent line + the machine-owned block, then a
/// `## Guards` section. Subprojects get the enrichable `pending` Guards block
/// (Wave 2 fills it); the root gets the inert human seed (root is excluded from
/// enrich).
fn scaffold(
    name: &str,
    block: &str,
    kind: &str,
    frameworks: &[String],
    stacks: &[StackDetection],
    scripts: &[String],
    is_root: bool,
) -> String {
    let title = title_case(name);
    let guards = if is_root {
        String::from("## Guards\n\n<!-- seed DO/DON'T aqui -->\n")
    } else {
        build_guards_block(kind, frameworks, stacks, scripts)
    };
    // The managed block lives at the END — identifiable and replaceable, with the
    // human-owned `## Guards` (and any curated prose) above it.
    format!(
        "# {title}\n\
         \n\
         > Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)\n\
         \n\
         {guards}\n\
         {block}\n"
    )
}

/// Append the managed block to a file at the very END, after exactly one blank
/// line, preserving every existing byte above it verbatim. The safe path for
/// both a freshly-relocated managed file and a human-authored legacy CLAUDE.md:
/// Mustard adds only its identifiable block at the tail and never touches the
/// content above. Trailing whitespace on the input is normalised so the result
/// is byte-stable across re-renders (idempotence).
fn append_managed_block(content: &str, block: &str) -> String {
    let body = content.trim_end();
    if body.is_empty() {
        return format!("{block}\n");
    }
    format!("{body}\n\n{block}\n")
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
/// `pending`/`done` block are left byte-for-byte untouched; the workspace root
/// is never reseeded (it is excluded from enrich).
fn reseed_guards_if_placeholder(
    text: &str,
    kind: &str,
    frameworks: &[String],
    stacks: &[StackDetection],
    scripts: &[String],
    is_root: bool,
) -> String {
    const PLACEHOLDERS: [&str; 2] = ["(populated by /scan)", "<!-- seed DO/DON'T aqui -->"];
    if is_root {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let Some(g) = lines.iter().position(|l| l.trim_end() == "## Guards") else {
        return text.to_string();
    };
    let body_start = g + 1;
    let body_end = lines[body_start..]
        .iter()
        .position(|l| l.starts_with("## "))
        .map_or(lines.len(), |off| body_start + off);
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
/// gets an Orchestrator-only line pointing at `.claude/CLAUDE.md`.
fn breadcrumb(dir: &str) -> String {
    let depth = dir.split('/').filter(|s| !s.is_empty()).count();
    if depth == 0 {
        return "> Orchestrator: [.claude/CLAUDE.md](.claude/CLAUDE.md)".to_string();
    }
    let up = "../".repeat(depth);
    format!("> Parent: [{up}CLAUDE.md]({up}CLAUDE.md) | Orchestrator: [{up}.claude/CLAUDE.md]({up}.claude/CLAUDE.md)")
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
    let mut over_cap: Vec<OversizedEntry> = Vec::new();

    for project in projects {
        let dir = root.join(&project.dir);
        let claude_md_path = dir.join("CLAUDE.md");
        let claude_dir = dir.join(".claude");
        // The workspace-root unit (empty `dir`) is excluded from enrich — it gets
        // the inert human seed, never the Wave-2 `pending` Guards block.
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

        // Read existing content so curated sections are preserved — fail-open.
        let existing = std::fs::read_to_string(&claude_md_path).ok();
        let content = render(
            &project.name,
            &project.kind,
            project.code_files,
            &project.frameworks,
            &project.detected_stacks,
            &project.scripts,
            &commands,
            existing.as_deref(),
            is_root,
        );
        // The Parent/Orchestrator breadcrumb is a function of the unit's depth
        // below the scan root, not a fixed `../`. Regenerate it so deep
        // subprojects (`apps/dashboard`, depth 2 → `../../CLAUDE.md`) link to the
        // real targets instead of the non-existent `../CLAUDE.md`, and the root
        // drops the meaningless `> Parent`. Self-healing on every `--full`.
        let content = fix_breadcrumb(&content, &project.dir);

        // Hard cap: refuse to ship a bloated CLAUDE.md. Record the overage and
        // skip the write so the existing (smaller) file is left intact rather
        // than overwritten with something over the ceiling. Deterministic — the
        // outcome is a pure function of the rendered byte length.
        if content.len() > CLAUDE_MD_HARD_CAP_BYTES {
            eprintln!(
                "scan --full: refusing to write {:?}: {} bytes exceeds hard cap of {} — trim curated prose",
                claude_md_path.display(),
                content.len(),
                CLAUDE_MD_HARD_CAP_BYTES,
            );
            over_cap.push(OversizedEntry {
                path: path_to_string(&claude_md_path),
                bytes: content.len(),
            });
            continue;
        }

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
        over_cap,
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
        over_cap: Vec::new(),
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
        // (d) existing=None on the ROOT (is_root=true) → scaffold: H1 + sentinel
        // block + inert human `## Guards` seed (the root is excluded from enrich,
        // so no `pending` marker here).
        let out = render("dashboard", "typescript", 42, &[], &[], &[], &no_commands(), None, true);
        assert!(out.contains("# Dashboard"), "header missing: {out}");
        assert!(out.contains("Tipo: typescript · 42 arquivos"), "map missing: {out}");
        assert!(out.contains(SENTINEL_OPEN), "scan-map open missing: {out}");
        assert!(out.contains(SENTINEL_CLOSE), "scan-map close missing: {out}");
        assert!(out.contains("## Guards"), "guards heading missing: {out}");
        assert!(out.contains("<!-- seed DO/DON'T aqui -->"), "seed placeholder missing: {out}");
        // Root never carries the Wave-2 `pending` marker.
        assert!(!out.contains(GUARDS_PENDING_OPEN), "root must not get pending guards: {out}");
        // `## Guards` lives ABOVE the managed block now (the block is at the END).
        let open = out.find(SENTINEL_OPEN).unwrap();
        let guards = out.find("## Guards").unwrap();
        assert!(guards < open, "guards must sit above the managed block: {out}");
        assert!(out.trim_end().ends_with(SENTINEL_CLOSE), "managed block must be at the end: {out}");
        assert!(out.ends_with('\n'), "missing trailing newline");
    }

    #[test]
    fn guards_pending() {
        // A fresh SUBPROJECT (is_root=false) gets the enrichable `## Guards`
        // block: a `pending` sentinel carrying the deterministic facts (kind,
        // frameworks) in a comment for the Wave-2 enrich agent, OUTSIDE the
        // scan-map block.
        let frameworks = vec!["serde".to_string(), "clap".to_string()];
        let out = render("rt", "rust", 12, &frameworks, &[], &[], &no_commands(), None, false);
        assert!(out.contains(GUARDS_PENDING_OPEN), "pending open marker missing: {out}");
        assert!(out.contains(GUARDS_CLOSE), "guards close marker missing: {out}");
        // Facts live in a comment inside the block — context, not content.
        assert!(out.contains("<!-- facts: kind=rust; frameworks=serde, clap -->"), "facts comment missing: {out}");
        // The marker carries the literal `pending` token Wave 2 matches on.
        assert!(GUARDS_PENDING_OPEN.contains("pending"), "open marker lost its pending token");
        // The guards block sits ABOVE the scan-map block (the block is at the END).
        let open = out.find(SENTINEL_OPEN).unwrap();
        let pending = out.find(GUARDS_PENDING_OPEN).unwrap();
        assert!(pending < open, "guards block must sit above the managed block: {out}");
        // No frameworks → facts still render with an explicit (none).
        let bare = render("lib", "rust", 1, &[], &[], &[], &no_commands(), None, false);
        assert!(bare.contains("<!-- facts: kind=rust; frameworks=(none) -->"), "empty frameworks facts: {bare}");
        // Idempotence: re-rendering the scaffold preserves the pending block.
        let again = render("rt", "rust", 12, &frameworks, &[], &[], &no_commands(), Some(&out), false);
        assert!(again.contains(GUARDS_PENDING_OPEN), "pending marker lost on re-render: {again}");
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
        let out = render("rt", "rust", 12, &frameworks, &stacks, &[], &no_commands(), None, false);
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
        let out = render("rt", "rust", 12, &frameworks, &[], &scripts, &no_commands(), None, false);
        assert!(out.contains("scripts=generate:api, build"), "render did not thread scripts: {out}");
    }

    #[test]
    fn render_preserves_arbitrary_sections_outside_block() {
        // (a) Legacy file WITHOUT sentinels: the managed block is APPENDED at the
        // end and NOTHING above is stripped or reordered — `## Stack`/`## Commands`
        // may be hand-written, so they survive verbatim alongside `## Architecture`
        // and `## Guards`.
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
        let out = render("dashboard", "rust", 99, &[], &[], &[], &commands, Some(existing), false);
        // Exactly one managed block, appended at the END.
        assert_eq!(out.matches(SENTINEL_OPEN).count(), 1, "expected one block: {out}");
        assert!(out.trim_end().ends_with(SENTINEL_CLOSE), "block must be at the end: {out}");
        assert!(out.contains("Tipo: rust · 99 arquivos"), "map not refreshed: {out}");
        // NOTHING above the block is stripped — the legacy human sections survive
        // verbatim, including any `## Stack` / `## Commands` (they may be human).
        assert!(out.contains("## Stack"), "human Stack stripped: {out}");
        assert!(out.contains("old-framework"), "human Stack body stripped: {out}");
        assert!(out.contains("## Architecture"), "architecture lost: {out}");
        assert!(out.contains("Layered: ui → domain → io."), "architecture body lost: {out}");
        assert!(out.contains("Never import from"), "guard line 1 lost: {out}");
        assert!(out.contains("Always use `Result<T, anyhow::Error>`"), "guard line 2 lost: {out}");
        // Idempotent: re-rendering relocates/changes nothing further.
        let again = render("dashboard", "rust", 99, &[], &[], &[], &commands, Some(&out), false);
        assert_eq!(out, again, "render must be idempotent");
    }

    #[test]
    fn render_relocates_block_to_end_preserving_everything_else() {
        // (b) File already has the sentinel mid-file with prose + guards after it.
        // The render removes the block and re-appends it at the END; the
        // H1/breadcrumb stay on top and everything outside the block is preserved.
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
        let out = render("dashboard", "rust", 77, &[], &[], &[], &no_commands(), Some(existing), false);
        assert_eq!(out.matches(SENTINEL_OPEN).count(), 1, "duplicate block: {out}");
        assert!(out.trim_end().ends_with(SENTINEL_CLOSE), "block must be at the end: {out}");
        assert!(out.starts_with("# Dashboard"), "header moved: {out}");
        assert!(out.contains("Tipo: rust · 77 arquivos"), "block not refreshed: {out}");
        assert!(out.contains("Hand-written prose that must NOT move."), "prose lost: {out}");
        assert!(out.contains("- keep me"), "guard lost: {out}");
        // Idempotent.
        let again = render("dashboard", "rust", 77, &[], &[], &[], &no_commands(), Some(&out), false);
        assert_eq!(out, again, "render not idempotent");
    }

    #[test]
    fn render_preserves_content_outside_block_and_relocates_to_end() {
        // A file with the sentinel block plus other sections beside it. The render
        // removes the old block, re-appends the fresh one at the END, and PRESERVES
        // every other byte — the prior `purge` behaviour is gone: Mustard never
        // touches content outside its block (it may be hand-written).
        let existing = "\
# Root

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)

<!-- mustard:scan-map -->
Tipo: cargo · 0 arquivos
Pesquise via `mustard-rt run feature` (digest) — não leia o repo direto.
<!-- /mustard:scan-map -->

## Stack

Tipo: cargo

- anyhow
- clap

## Commands

| Task | Command |
|------|---------|
| Build | `cargo build` |

## Guards

- keep me
";
        let commands = Commands {
            build: Some("cargo build".into()),
            test: None,
            lint: None,
            type_check: None,
        };
        let out = render("root", "npm", 11, &["serde".into()], &[], &[], &commands, Some(existing), true);
        assert_eq!(out.matches(SENTINEL_OPEN).count(), 1, "duplicate sentinel: {out}");
        assert!(out.trim_end().ends_with(SENTINEL_CLOSE), "block must be relocated to the end: {out}");
        assert!(out.contains("Tipo: npm · 11 arquivos"), "block not refreshed: {out}");
        // Content outside the old block is PRESERVED (not purged).
        assert!(out.contains("## Stack"), "outside Stack wrongly purged: {out}");
        assert!(out.contains("- anyhow"), "outside Stack body wrongly purged: {out}");
        assert!(out.contains("- keep me"), "guard lost: {out}");
        // Idempotent.
        let again = render("root", "npm", 11, &["serde".into()], &[], &[], &commands, Some(&out), true);
        assert_eq!(out, again, "render not idempotent");
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
        let out = render("dashboard", "npm", 1, &["react".into()], &[], &[], &no_commands(), Some(stub), false);
        assert!(out.contains(GUARDS_PENDING_OPEN), "stub not reseeded to pending: {out}");
        assert!(!out.contains("(populated by /scan)"), "stub survived: {out}");
        // Idempotent: a re-render keeps the pending block (does not re-reseed).
        let again = render("dashboard", "npm", 1, &["react".into()], &[], &[], &no_commands(), Some(&out), false);
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
        let out2 = render("cli", "cargo", 1, &[], &[], &[], &no_commands(), Some(curated), false);
        assert!(out2.contains("- Real human rule that must stay."), "curated guard lost: {out2}");
        assert!(!out2.contains(GUARDS_PENDING_OPEN), "curated guards wrongly reseeded: {out2}");

        // The root is never reseeded, even carrying the seed placeholder.
        let root = "\
# (root)

<!-- mustard:scan-map -->
Tipo: npm · 1 arquivos
<!-- /mustard:scan-map -->

## Guards

<!-- seed DO/DON'T aqui -->
";
        let out3 = render("(root)", "npm", 1, &[], &[], &[], &no_commands(), Some(root), true);
        assert!(out3.contains("<!-- seed DO/DON'T aqui -->"), "root seed wrongly reseeded: {out3}");
        assert!(!out3.contains(GUARDS_PENDING_OPEN), "root must not get pending: {out3}");
    }

    #[test]
    fn breadcrumb_depth_and_fix() {
        // Depth drives the number of `../` hops.
        assert_eq!(breadcrumb(""), "> Orchestrator: [.claude/CLAUDE.md](.claude/CLAUDE.md)");
        assert!(breadcrumb("apps/dashboard").contains("[../../CLAUDE.md](../../CLAUDE.md)"));
        assert!(breadcrumb("apps/dashboard").contains("../../.claude/CLAUDE.md"));
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
        assert!(root_fixed.contains("> Orchestrator: [.claude/CLAUDE.md](.claude/CLAUDE.md)"), "root orch link wrong: {root_fixed}");
        assert!(!root_fixed.contains("> Parent:"), "root must not carry Parent: {root_fixed}");
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
        let first = render("dashboard", "rust", 5, &[], &[], &[], &commands, Some(existing), false);
        let second = render("dashboard", "rust", 5, &[], &[], &[], &commands, Some(&first), false);
        assert_eq!(first, second, "render must be idempotent over migration");
    }

    #[test]
    fn render_emits_commands_table_with_only_some_rows() {
        let frameworks = vec!["serde".to_string(), "clap".to_string()];
        let commands = Commands {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            lint: None,
            type_check: Some("cargo check".into()),
        };
        let out = render("rt", "rust", 12, &frameworks, &[], &[], &commands, None, false);
        // Commands table has only the Some rows, in fixed order, no Lint row.
        assert!(out.contains("## Commands"), "commands heading missing: {out}");
        assert!(out.contains("| Build | `cargo build` |"), "build row missing: {out}");
        assert!(out.contains("| Test | `cargo test` |"), "test row missing: {out}");
        assert!(out.contains("| Type-check | `cargo check` |"), "type-check row missing: {out}");
        assert!(!out.contains("| Lint |"), "lint row must be absent (None): {out}");
    }

    #[test]
    fn render_omits_commands_table_when_all_none() {
        let out = render("lib", "rust", 1, &[], &[], &[], &no_commands(), None, false);
        assert!(!out.contains("## Commands"), "commands section must be absent: {out}");
        // After the Stack cut there is no `## Stack` section at all.
        assert!(!out.contains("## Stack"), "stack section must be dropped: {out}");
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
        let first = render("dashboard", "typescript", 30, &frameworks, &[], &[], &commands, None, false);
        // Feeding the previous render back in must reproduce it byte-for-byte:
        // the scaffold's sentinel block round-trips through the splice path
        // unchanged, and the `pending` Guards block is preserved outside it.
        let second = render("dashboard", "typescript", 30, &frameworks, &[], &[], &commands, Some(&first), false);
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
                detected_stacks: Vec::new(),
            },
            mustard_core::domain::scan::Project {
                name: "small".into(),
                dir: "apps/small".into(),
                kind: "rust".into(),
                code_files: 1,
                frameworks: Vec::new(),
                dependencies: Vec::new(),
                scripts: Vec::new(),
                detected_stacks: Vec::new(),
            },
        ];

        let result = run_default(root, &projects);
        assert_eq!(result.oversized.len(), 1, "only the big file should be flagged");
        assert!(result.oversized[0].path.contains("big"), "wrong file flagged");
        assert!(result.oversized[0].bytes > CLAUDE_MD_WARN_BYTES);
        assert!(result.regenerated.is_empty());
        assert!(result.over_cap.is_empty(), "default mode never enforces the hard cap");
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
    fn claude_md_hard_cap() {
        // `run_full` refuses to write a CLAUDE.md whose rendered size exceeds the
        // hard cap: the over-cap file is recorded, NOT regenerated, and its prior
        // (curated) content is left intact on disk.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Over-cap unit: a curated CLAUDE.md with sentinels + a bloated
        // `## Architecture` so the spliced render still exceeds the ceiling.
        let bloated = format!(
            "# Big\n\n{SENTINEL_OPEN}\nTipo: rust · 1 arquivos\n{SENTINEL_CLOSE}\n\n## Architecture\n\n{}\n",
            "x".repeat(CLAUDE_MD_HARD_CAP_BYTES + 1)
        );
        let big_dir = root.join("apps").join("big");
        std::fs::create_dir_all(&big_dir).expect("mkdir big");
        let big_path = big_dir.join("CLAUDE.md");
        std::fs::write(&big_path, &bloated).expect("write big");

        // Under-cap unit: no file yet (fresh scaffold, well within the cap).
        std::fs::create_dir_all(root.join("apps").join("small")).expect("mkdir small");

        let projects = vec![project("big", "apps/big"), project("small", "apps/small")];
        let result = run_full(root, &projects);

        // The over-cap file is recorded, deterministically, by its byte length.
        assert_eq!(result.over_cap.len(), 1, "exactly the bloated file trips the cap: {:?}", result.over_cap);
        assert!(result.over_cap[0].path.contains("big"), "wrong file flagged: {:?}", result.over_cap);
        assert!(result.over_cap[0].bytes > CLAUDE_MD_HARD_CAP_BYTES);
        // It was NOT regenerated …
        assert!(!result.regenerated.iter().any(|p| p.contains("big")), "over-cap file must not be written");
        // … and its on-disk content is untouched.
        assert_eq!(std::fs::read_to_string(&big_path).unwrap(), bloated, "prior content clobbered");
        // The small unit still scaffolds and writes fine.
        assert!(result.regenerated.iter().any(|p| p.contains("small")), "under-cap unit must write");
    }
}
