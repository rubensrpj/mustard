//! Deterministic scan-map generator for subprojects — no AI, no source reads.
//!
//! Invoked by `scan::run` (`--full`) after `grain.model.json` is written. It
//! writes one file per unit, `.claude/scan-map.md` (kind + size + the digest
//! pointer + detected `## Commands`), capped by [`SCAN_MAP_HARD_CAP_BYTES`] as
//! a guard against a runaway generator. That file is Mustard's and stays out
//! of git. No `CLAUDE.md` (nor `CLAUDE.local.md`) is ever read or written:
//! those files belong to the project.

use std::fmt::Write as _;
use std::path::Path;

use mustard_core::{translate, SupportedLocale};

/// Hard ceiling on the machine-owned `.claude/scan-map.md`. The map is a terse
/// orientation file (~200 bytes + an optional commands table); a file this
/// large means the GENERATOR ran away, so `run_full` refuses to write it and
/// reports a deterministic error. Curated `CLAUDE.md` prose is never measured
/// against this (or any) ceiling.
pub const SCAN_MAP_HARD_CAP_BYTES: usize = 8192;

/// Result of running the scan-map pass over a set of projects.
pub struct ClaudeMdResult {
    /// Paths (re)written this pass: every `.claude/scan-map.md`, and nothing
    /// else.
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

/// Render the mustard-owned `.claude/scan-map.md` for a unit: a terse
/// orientation map (kind + size + the digest pointer) plus the `## Commands`
/// section — and only when the caller passes NON-DEFAULT commands (it zeroes
/// the conventional language defaults, so `render_commands` omits the
/// section). The dependency `## Stack` was dropped on purpose: a dep list is
/// auto-inferable from the manifest, so it is token noise, not signal. The
/// whole FILE is machine-owned, so no sentinels are needed; ends in exactly
/// one newline (byte-stable).
///
/// Language follows the project's text language (`language.text` in
/// `mustard.json`): the map is DISPLAYED to the developer and injected as
/// orientation, so it is user-facing text, not an internal index. A project
/// with no declared language gets `pt-BR`
/// ([`mustard_core::Language::text_or_default`]).
pub(crate) fn render_map(
    kind: &str,
    code_files: usize,
    commands: &mustard_core::domain::config::Commands,
    lang: SupportedLocale,
) -> String {
    let commands_block = render_commands(commands);

    let mut out = String::new();
    let type_line = translate("scan.map.type_line", lang)
        .replace("{kind}", kind)
        .replace("{count}", &code_files.to_string());
    let _ = writeln!(out, "{type_line}");
    let _ = writeln!(out, "{}", translate("scan.map.pointer", lang));
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

    // The scan-map language follows the project's text language — resolved
    // once at the scan root, applied to every unit. Fail-open: no or
    // unreadable config gives the `pt-BR` default.
    let lang = crate::shared::context::config::project_config_cached(root).language().text_or_default();

    for project in projects {
        let dir = root.join(&project.dir);
        let claude_dir = dir.join(".claude");
        let map_path = claude_dir.join("scan-map.md");

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
        let map = render_map(&project.kind, project.code_files, &commands, lang);
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
    fn map_emits_commands_table_with_only_some_rows() {
        let commands = Commands {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            lint: None,
            type_check: Some("cargo check".into()),
            prepare: None,
        };
        let out = render_map("rust", 12, &commands, SupportedLocale::PtBr);
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
        let out = render_map("rust", 1, &no_commands(), SupportedLocale::PtBr);
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
            prepare: None,
        };
        assert_eq!(
            render_map("typescript", 30, &commands, SupportedLocale::PtBr),
            render_map("typescript", 30, &commands, SupportedLocale::PtBr),
            "two renders must produce identical bytes"
        );
    }

    #[test]
    fn map_follows_declared_locale_en() {
        // An `en-US` project gets an English map;
        // a project with no declared locale keeps the pt-BR default (asserted
        // by the sibling tests). The header + pointer both route through i18n.
        let out = render_map("rust", 12, &no_commands(), SupportedLocale::EnUs);
        assert!(out.contains("Type: rust · 12 files"), "EN header missing: {out}");
        assert!(!out.contains("arquivos"), "no pt-BR bytes in an EN map: {out}");
        assert!(
            out.contains("The terrain is already in your window"),
            "EN pointer missing: {out}"
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
            own_git_root: false,
        }
    }

    #[test]
    fn the_pass_writes_the_maps_and_never_touches_a_claude_md() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let small = root.join("apps").join("small");
        std::fs::create_dir_all(&small).expect("mkdir small");
        std::fs::create_dir_all(root.join("apps").join("fresh")).expect("mkdir fresh");
        let user_file = "# Small\n\nMy own rules, my own layout.\n";
        std::fs::write(small.join("CLAUDE.md"), user_file).expect("write md");

        let projects = vec![project("(root)", ""), project("small", "apps/small"), project("fresh", "apps/fresh")];
        let result = run_full(root, &projects);

        assert!(result.over_cap.is_empty(), "{:?}", result.over_cap);
        for unit in [root.to_path_buf(), small.clone(), root.join("apps").join("fresh")] {
            assert!(unit.join(".claude").join("scan-map.md").is_file(), "map missing under {}", unit.display());
            assert!(!unit.join("CLAUDE.local.md").exists(), "no local layer under {}", unit.display());
        }
        assert_eq!(std::fs::read_to_string(small.join("CLAUDE.md")).unwrap(), user_file, "the file is the project's");
        assert!(!root.join("CLAUDE.md").exists(), "no CLAUDE.md is created at the root");
        assert!(!root.join("apps").join("fresh").join("CLAUDE.md").exists(), "nor in a subproject");
        assert!(result.regenerated.iter().all(|p| p.ends_with("scan-map.md")), "{:?}", result.regenerated);
    }
}
