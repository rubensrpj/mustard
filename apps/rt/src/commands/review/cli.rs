//! The `run` subcommands for the REVIEW and QA gates (`review/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`ReviewCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run review <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{review};

/// The `run` subcommands owned by the REVIEW and QA gates (`review/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum ReviewCmd {
    /// Validate a spec's structure (WARN-level — never blocks).
    #[command(display_order = 16)]
    AnalyzeValidation {
        /// Path to the spec file.
        #[arg(long, alias = "from-spec")]
        spec: Option<String>,
    },
    /// Pre-dispatch factual gate: greps the spec's subproject for every JSX
    /// symbol and named import it references, and reports those whose
    /// `export` is missing. Self-created paths (declared in `## Files`) are
    /// excluded. Output is single-line JSON; exit code is always 0
    /// (fail-open) — the orchestrator decides whether to block dispatch.
    #[command(display_order = 25)]
    DependencyPrecheck {
        /// Path to the spec file or its containing directory (resolves
        /// `<dir>/spec.md`).
        #[arg(long)]
        spec: Option<String>,
        /// Override the auto-detected subproject scan root
        /// (`apps/<name>` / `packages/<name>` common ancestor of `## Files`).
        #[arg(long)]
        subproject: Option<String>,
    },
    /// Spec A v4 / W4 — run the behavior-regression gate at the requested moment.
    ///
    /// Reads the spec's `plan.txt` (or `spec.md` body) as the Moment-1 plan
    /// text and dispatches to `review::gate_regression_check::run`. Moments 2 and 3
    /// require external `diff` + snapshots that the bare CLI does not
    /// collect today — those moments are exercised via the W5 span-level
    /// integration.
    /// Exit code mirrors the verdict: Green/Amber ⇒ 0, Red ⇒ 2.
    #[command(name = "gate-regression-check")]
    #[command(display_order = 27)]
    GateRegressionCheck {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Moment to evaluate: 1 (pre-edit), 2 (during diff), 3 (after child return).
        #[arg(long, default_value_t = 1)]
        moment: u8,
        /// W5#3 — wave directory (e.g. `.claude/spec/<spec>/wave-5-rt`) used
        /// only with `--moment 3`. When set, the subcommand inspects that
        /// wave's `_review-spans.md` ledger via
        /// `review::review_spans::check_consolidation` and exits non-zero (2) when any
        /// row registered a red verdict. Lets close-gate scripts invoke the
        /// span-level decision without going through the `SubagentStop` hook.
        #[arg(long = "wave-dir")]
        wave_dir: Option<String>,
    },
    /// Execute a spec's Acceptance Criteria; emit a `qa.result` event.
    #[command(display_order = 28)]
    QaRun {
        /// Spec name (resolved under `.claude/specs` or `.claude/spec` — flat layout).
        #[arg(long, alias = "from-spec")]
        spec: String,
        /// Output format: `json` (default) or `html` (extra artifact).
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Record a REVIEW-phase verdict (emits a `review.result` event + metric).
    #[command(display_order = 35)]
    ReviewResult {
        /// Spec name.
        #[arg(long)]
        spec: Option<String>,
        /// Verdict: `approved` or `rejected`.
        #[arg(long)]
        verdict: Option<String>,
        /// Count of critical findings.
        #[arg(long, default_value_t = 0)]
        critical: i64,
        /// Subproject the review targeted.
        #[arg(long)]
        subproject: Option<String>,
        /// Optional file whose content is persisted to
        /// `<spec>/review/findings.md` (folded into the retry prompt's
        /// `## RETRY CONTEXT` on re-dispatch). Absent ⇒ no findings file is
        /// written; the `review.result` event is unaffected.
        #[arg(long = "findings-file")]
        findings_file: Option<PathBuf>,
    },
    /// Scan a project tree for committed secrets + misconfigurations.
    #[command(display_order = 37)]
    SecurityScan {
        /// Directory to scan. Defaults to the current directory.
        dir: Option<String>,
        /// Emit the machine-readable JSON report.
        #[arg(long)]
        json: bool,
    },
    /// Prefetch a GitHub Pull Request into a structured JSON document.
    ///
    /// Shell-outs to `gh pr view --json ...` and re-emits a clean structure
    /// ready for the LLM to consume. `--format table` prints a compact
    /// executive summary (title, author, scope, comments, review states).
    /// Fail-open: if `gh` is not in the PATH, emits `{"error":"gh-not-found"}`.
    #[command(display_order = 51)]
    ReviewPrefetch {
        /// PR reference: a number (`123`) or GitHub URL.
        pr_ref: Option<String>,
        /// Output format: `json` (default) or `table`.
        #[arg(long, default_value = "json")]
        format: String,
        /// Project root override (optional).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// W5.T5.2 — Orchestrate the REVIEW phase steps (prefetch + diff + DORA emits).
    #[command(name = "review-dispatch")]
    #[command(display_order = 66)]
    ReviewDispatch {
        /// PR number.
        #[arg(long)]
        pr: u64,
        /// Spec slug for event attribution.
        #[arg(long)]
        spec: Option<String>,
        /// Subproject to scope the diff to.
        #[arg(long)]
        subproject: Option<String>,
    },
}

/// Dispatch one `review`-family `run` subcommand.
pub fn dispatch(cmd: ReviewCmd) {
    match cmd {
        ReviewCmd::AnalyzeValidation { spec } => review::analyze_validation::run(spec.as_deref()),
        ReviewCmd::DependencyPrecheck { spec, subproject } => {
            review::dependency_precheck::run(spec.as_deref(), subproject.as_deref());
        }
        ReviewCmd::GateRegressionCheck {
            spec,
            moment,
            wave_dir,
        } => {
            use crate::commands::review::gate_regression_check::{GateInput, Moment};
            // W5#3: Moment-3 + --wave-dir path consults the on-disk
            // `_review-spans.md` ledger via `review::review_spans::check_consolidation`.
            // Exits 0 when consolidation is allowed (no red rows) and 2 when
            // blocked. This is the close-gate path; ledger lives on disk so
            // we don't need diff + snapshots in argv.
            if moment == 3 {
                if let Some(wd) = wave_dir {
                    use crate::commands::review::review_spans::{check_consolidation, ConsolidationCheck};
                    let path = std::path::PathBuf::from(wd);
                    match check_consolidation(&path) {
                        ConsolidationCheck::Allowed => std::process::exit(0),
                        ConsolidationCheck::Blocked { .. } => std::process::exit(2),
                    }
                }
            }
            let spec_path = std::path::PathBuf::from(".claude/spec").join(&spec).join("spec.md");
            let plan_text = std::fs::read_to_string(&spec_path).unwrap_or_default();
            let moment_enum = match moment {
                1 => Moment::One,
                2 => Moment::Two,
                3 => Moment::Three,
                _ => Moment::One,
            };
            let input = GateInput {
                spec_path,
                plan_text,
                diff: Vec::new(),
                declared_fns: Vec::new(),
                before_snapshot: None,
                after_snapshot: None,
            };
            match review::gate_regression_check::run(input, moment_enum) {
                Ok(_) => std::process::exit(0),
                Err(_) => std::process::exit(2),
            }
        }
        ReviewCmd::QaRun { spec, format } => review::qa_run::run(&spec, &format),
        ReviewCmd::ReviewResult {
            spec,
            verdict,
            critical,
            subproject,
            findings_file,
        } => review::review_result::run(
            spec.as_deref(),
            verdict.as_deref(),
            critical,
            subproject.as_deref(),
            findings_file.as_deref(),
        ),
        ReviewCmd::SecurityScan { dir, json } => review::security_scan::run(dir.as_deref(), json),
        ReviewCmd::ReviewPrefetch { pr_ref, format, root: _ } => {
            let pr_ref = pr_ref.unwrap_or_default();
            if pr_ref.is_empty() {
                println!("{}",
                    serde_json::to_string_pretty(&serde_json::json!({"error":"pr-ref-required"}))
                        .unwrap_or_default()
                );
            } else {
                review::review_prefetch::run(review::review_prefetch::ReviewPrefetchOpts { pr_ref, format });
            }
        }
        ReviewCmd::ReviewDispatch { pr, spec, subproject } => {
            review::review_dispatch::run(review::review_dispatch::ReviewDispatchOpts { pr, spec, subproject });
        }
    }
}
