//! The `run` subcommands for wave plans (`wave/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`WaveCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run wave <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;

use crate::commands::{wave};

/// The `run` subcommands owned by wave plans (`wave/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum WaveCmd {
    /// Render a spec's wave structure as an ASCII or JSON tree.
    #[command(display_order = 18)]
    WaveTree {
        /// Path to the spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: String,
        /// Output format: `ascii` (default) or `json`.
        #[arg(long, default_value = "ascii")]
        format: String,
    },
    /// Analyze file dependencies across waves (topological import DAG).
    ///
    /// Input via `--plan <file>` (preferred — survives the `rtk` wrapper) or
    /// stdin (legacy). Both transports accept BOTH shapes: the derivation form
    /// `{files, projectRoot}` and the rich plan JSON (`{waves: [{files}]}`,
    /// per-wave censuses unioned) that `plan-materialize --plan` consumes.
    #[command(display_order = 19)]
    WaveDependency {
        /// Path to a JSON file: `{files, projectRoot}` or a `--plan`-style
        /// `{waves: [...]}` document. Omit to read the same JSON from stdin.
        #[arg(long)]
        plan: Option<String>,
    },
    /// Return the declared-files count and full markdown body of a wave's
    /// sub-spec (`.claude/spec/{spec}/wave-{wave}-*/spec.md`). Used by the
    /// dashboard "Ondas" tab to show the canon `## Arquivos` count and pop
    /// open a drawer with the wave markdown. Fail-open: missing files →
    /// `{"count":0,"markdown":"","path":null}`.
    #[command(display_order = 20)]
    WaveFiles {
        /// Parent spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: Option<String>,
        /// Wave number (1-based).
        #[arg(long)]
        wave: Option<u32>,
    },
    /// Check whether a spec should be decomposed at EXECUTE entry.
    #[command(display_order = 24)]
    ExecRewaveCheck {
        /// Path to the spec file.
        #[arg(long)]
        spec: Option<String>,
    },
    /// Audit per-wave file/layer counts inside a wave-plan.
    #[command(display_order = 26)]
    WaveSizeCheck {
        /// Path to the spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: Option<String>,
    },
    // The folder name is spelled `wave-<n>-<role>` (angle brackets) throughout
    // this doc comment: a literal brace-n sequence is a clap help-template
    // token (forced line break) and would mangle the rendered --help.
    /// Materialise the canonical SDD wave layout from a declarative JSON plan.
    ///
    /// Renders `wave-plan.md` + each `wave-<n>-<role>/spec.md` (+ `meta.json`
    /// sidecars). Idempotent: existing files are never overwritten.
    ///
    /// Every entry in `waves` REQUIRES two fields (no serde default — omitting
    /// either is a parse error: stdout gains `error` + `hint`, exit 2):
    ///   - `n: u32`       — 1-based wave number, drives the folder name
    ///                      `wave-<n>-<role>`.
    ///   - `role: String` — role label (`general`, `backend`, …), the other
    ///                      half of the folder name.
    ///
    /// Minimal valid plan JSON:
    ///   {
    ///     "waves": [
    ///       { "n": 1, "role": "general", "summary": "…", "depends_on": [] },
    ///       { "n": 2, "role": "general", "summary": "…",
    ///         "depends_on": ["wave-1-general"] }
    ///     ],
    ///     "total_waves": 2,
    ///     "lang": "pt-BR"
    ///   }
    ///
    /// Only the per-wave BODY fields are optional (`#[serde(default)]` — a
    /// summary-only plan still deserialises):
    ///   - `tasks: [String]`      → `## Tasks`/`## Tarefas` (`- [ ] {task}`) in
    ///                              the wave spec, read back by
    ///                              `agent-prompt-render` as `{task_steps}`.
    ///   - `files: [String]`      → `## Files`/`## Arquivos` (`` - `{path}` ``),
    ///                              read back as `{reference_files}`.
    ///   - `acceptance: [String]` → NOT in the wave spec; the union across waves
    ///                              is carried into `wave-plan.md` under
    ///                              `## Acceptance Criteria`/`## Critérios de
    ///                              Aceitação`, where the QA gate reads it.
    /// The Plan agent authors these arrays; the body is never hand-written after
    /// the scaffold. Headings render in the effective language
    /// (`mustard.json#specLang` root-wins, plan `lang` as fallback). A wave with
    /// no `tasks` emits a stderr WARN (visible signal), not a bare heading.
    ///
    /// Inside the pipeline prefer `plan-materialize`, which composes this
    /// scaffold with validation and the PLAN-phase events.
    #[command(verbatim_doc_comment)]
    #[command(display_order = 44)]
    WaveScaffold {
        /// Target spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: Option<String>,
        /// Path to the plan JSON file.
        #[arg(long)]
        plan: Option<String>,
    },
    /// Deterministically merge a wave-plan's decomposition back down — the
    /// "reject decomposition" branch of `plugin/refs/spec/resume-loop.md § A`.
    ///
    /// `--mode full`: collapse N waves into a single `wave-1-{role}/spec.md`
    /// (parent root spec stays the orchestration doc), delete `wave-2..N`,
    /// patch `wave-plan.md` + parent `meta.json` to `totalWaves:1` /
    /// `isWavePlan:true` (NEVER zero waves for Full — the invariant).
    /// `--mode light`: merge every wave's sections into the root `spec.md`,
    /// delete all wave dirs + `wave-plan.md`, patch root `meta.json` to
    /// `isWavePlan:false`. Both set `scopeOverride:"user-rejected-waves"`.
    /// Atomic + idempotent + fail-open: a missing `wave-plan.md` →
    /// `{"ok":false,"reason":"no-wave-plan"}` (exit 0). Merged spec is written
    /// BEFORE any dir is deleted. Reuses `is_heading` / `write_atomic` /
    /// the wave-scaffold renderers.
    #[command(name = "wave-collapse")]
    #[command(display_order = 45)]
    WaveCollapse {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Collapse mode: `full` (→ single wave-1) or `light` (→ single root spec).
        #[arg(long)]
        mode: String,
    },
}

/// Dispatch one `wave`-family `run` subcommand.
pub fn dispatch(cmd: WaveCmd) {
    match cmd {
        WaveCmd::WaveTree { spec_dir, format } => wave::wave_tree::run(&spec_dir, &format),
        WaveCmd::WaveDependency { plan } => wave::wave_dependency::run(plan.as_deref()),
        WaveCmd::WaveFiles { spec, wave } => wave::wave_files::run(spec.as_deref(), wave),
        WaveCmd::ExecRewaveCheck { spec } => wave::exec_rewave_check::run(spec.as_deref()),
        WaveCmd::WaveSizeCheck { spec_dir } => wave::wave_size_check::run(spec_dir.as_deref()),
        WaveCmd::WaveScaffold { spec_dir, plan } => {
            wave::wave_scaffold::run(spec_dir.as_deref(), plan.as_deref());
        }
        WaveCmd::WaveCollapse { spec, mode } => {
            wave::wave_collapse::run(wave::wave_collapse::WaveCollapseOpts { spec, mode });
        }
    }
}
