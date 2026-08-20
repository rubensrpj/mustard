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
    /// Prefetch a Pull Request into a structured JSON document, through the
    /// provider IN FORCE.
    ///
    /// GitHub shells out to `gh pr view --json ...`; Azure composes the SAME
    /// document from the REST reads (PR + threads + reviewers) plus the local
    /// git diff. `--format table` prints a compact executive summary (title,
    /// author, scope, comments, review states). Fail-open: a provider that
    /// could not answer emits `{"error":"..."}` and exits 0.
    #[command(display_order = 51)]
    ReviewPrefetch {
        /// PR reference: a number (`123`) or the PR's web URL.
        pr_ref: Option<String>,
        /// Output format: `json` (default) or `table`.
        #[arg(long, default_value = "json")]
        format: String,
        /// Project root override (optional).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Prove every acceptance criterion is ABLE to fail: run each one against
    /// the tree as it is NOW and require it to come back red.
    ///
    /// A criterion clears the proof ONLY by failing; green, timed out, never
    /// attempted or still carrying an unfilled `<…>` placeholder is UNPROVEN.
    /// The trailing criterion is exempt (the build-green safety net). Writes the
    /// proof ledger `<spec-dir>/ac-proof.json` either way, prints one JSON
    /// document on stdout, and exits 2 when any criterion is unproven.
    ///
    /// With `--confirm` it takes the SECOND half instead: once the work has
    /// landed, every criterion that cleared the red proof is run AGAIN and must
    /// now come back GREEN. One still red is reported unproven — it does not
    /// clear on its earlier failure alone.
    ///
    /// With `--removal` it takes the THIRD transition: each confirmed criterion
    /// runs against a scratch checkout with the work the waves recorded taken
    /// away. One that stays green SURVIVED the removal — it verifies something
    /// OUTSIDE the work, which neither of the first two passes can tell apart.
    /// A criterion whose OWN EVIDENCE — the command or the `Expect:` regex the
    /// executor grades with — names a word the strip itself deleted is DECLINED
    /// rather than run: its red was guaranteed, so it would say nothing.
    #[command(name = "ac-negative-check")]
    #[command(display_order = 81)]
    AcNegativeCheck {
        /// Spec slug under `.claude/spec/`, or a path to the spec markdown or
        /// its directory.
        #[arg(long, alias = "from-spec")]
        spec: Option<String>,
        /// Take the CONFIRMATION pass (green after the work) instead of the RED
        /// proof pass (red before it). Run it after a wave's work has landed.
        #[arg(long)]
        confirm: bool,
        /// Take the REMOVAL pass — the third transition. Each criterion that
        /// was CONFIRMED green is run against a scratch checkout with the work
        /// the waves recorded taken away. One that stays green SURVIVED the
        /// removal: it verifies something the work never did. One whose own
        /// evidence — its command OR its `Expect:` regex — names a word the
        /// strip deleted is declined, not run. Wins over `--confirm` when both
        /// are given.
        #[arg(long)]
        removal: bool,
        /// The revision the removal restores the work to. Omitted: the merge
        /// base of `HEAD` and the project's primary integration base.
        #[arg(long)]
        from: Option<String>,
    },
    /// The `/mustard:pr` door's LIST step: every open pull request of the base
    /// the checkout is standing on, with its number, title, the provider's own
    /// mergeable word, whether it is a draft and the head branch its unit lives
    /// on. Runs ONLY from a `git.flow` integration base — from a work branch it
    /// refuses and names the base to switch to. Fail-open on `gh`.
    #[command(name = "pr-list")]
    #[command(display_order = 86)]
    PrList {
        /// Any directory inside the repo (worktrees welcome — the command
        /// resolves the main checkout itself). Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The `/mustard:pr` door's REVIEW step: resolve a pull request to its work
    /// unit and print the review brief — the spec the unit belongs to, the
    /// subproject its `## Files` name, and that subproject's skill shelf (the
    /// same molds the implementer was dispatched with). With `--verdict` it
    /// also RECORDS the outcome through the `review-result` path, which is what
    /// `pr-merge` reads back.
    #[command(name = "pr-review")]
    #[command(display_order = 87)]
    PrReview {
        /// PR number. Omitted: the open PR of the current branch.
        #[arg(long)]
        pr: Option<u64>,
        /// Verdict to record: `approved` or `rejected`. Omitted: the brief is
        /// printed and nothing is recorded.
        #[arg(long)]
        verdict: Option<String>,
        /// Count of critical findings (0 when `approved`).
        #[arg(long, default_value_t = 0)]
        critical: i64,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The `/mustard:pr` door's MERGE step: merge the pull request, then hand
    /// the pruning to `git-settle` (back to the base, pull it, remove the
    /// worktree, delete the local + remote branch). A unit whose review did not
    /// come back `approved` is WARNED about and ASKED — the command answers
    /// `action:"confirm"` and touches nothing; it never refuses. `--confirm` is
    /// the operator's answer coming back.
    #[command(name = "pr-merge")]
    #[command(display_order = 88)]
    PrMerge {
        /// PR number. Omitted: the open PR of the current branch.
        #[arg(long)]
        pr: Option<u64>,
        /// The operator's answer to the unreviewed-merge question. Without it
        /// an unreviewed unit is asked about, never merged and never refused.
        #[arg(long)]
        confirm: bool,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The `/mustard:pr` door's OPEN step: open the unit's pull request through
    /// the provider IN FORCE (`git.provider` declared, else the `origin`
    /// remote, else the fallback) — the prose names this command, never a
    /// provider CLI. The title is the body file's first heading. Answers one
    /// JSON report (`ok`/`provider`/`number`/`url`); failure degrades into the
    /// `error` field with exit 0, never a panic.
    #[command(name = "pr-open")]
    #[command(display_order = 94)]
    PrOpen {
        /// The integration base the PR targets (short branch name).
        #[arg(long)]
        base: String,
        /// The work branch the PR is opened FROM (short branch name).
        #[arg(long)]
        head: String,
        /// File whose content becomes the PR body (`<spec>/pr-body.md`); its
        /// first heading becomes the title. Unreadable ⇒ `ok:false` +
        /// `error:"body-file-unreadable"`, nothing is opened. Exactly one body
        /// source: this or `--fill`.
        #[arg(long = "body-file")]
        body_file: Option<PathBuf>,
        /// Derive title/body from the commits `base..head` carries (title =
        /// newest subject, body = the subject list) — the submodule flow's
        /// shape, where no `pr-body.md` exists. Exclusive with `--body-file`.
        #[arg(long)]
        fill: bool,
        /// Open as a draft — the parent of a monorepo unit while any submodule
        /// PR is still open.
        #[arg(long)]
        draft: bool,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The push ritual's body re-send: replace the body of pull request
    /// `--number` with the rewritten `<spec>/pr-body.md`, through the provider
    /// in force. Same report shape as `pr-open`; failure degrades into the
    /// `error` field with exit 0.
    #[command(name = "pr-edit")]
    #[command(display_order = 95)]
    PrEdit {
        /// The PR number whose body is replaced.
        #[arg(long)]
        number: u64,
        /// File whose content becomes the new PR body. Unreadable ⇒ `ok:false`
        /// + `error:"body-file-unreadable"`, nothing is edited.
        #[arg(long = "body-file")]
        body_file: PathBuf,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// The finish ritual's un-draft: mark draft pull request `--number` ready
    /// for review (what requests the code owners), through the provider in
    /// force. Same report shape as `pr-open`; failure degrades into the
    /// `error` field with exit 0.
    #[command(name = "pr-ready")]
    #[command(display_order = 96)]
    PrReady {
        /// The draft PR number to mark ready.
        #[arg(long)]
        number: u64,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Seed a spec's `meta.json#findings` from the two producers that already
    /// wrote their discoveries to disk: the reviewer's `review/findings*.md`
    /// (one finding per file) and the `removal` column of `ac-proof.json` (one
    /// per criterion that SURVIVED the removal or whose own evidence the strip
    /// took away, carrying the reason the ledger already wrote in full).
    ///
    /// Reconciles rather than overwrites: a finding whose destination was
    /// already declared keeps it, one whose source is gone is dropped, and a new
    /// one enters with no destination — the OPEN position a close gate reads.
    /// A spec with neither producer collects zero and leaves the sidecar
    /// untouched. Output is one byte-stable JSON document; this command decides
    /// nothing.
    #[command(name = "finding-collect")]
    #[command(display_order = 91)]
    FindingCollect {
        /// Spec slug under `.claude/spec/`, or a path to the spec markdown or
        /// its directory.
        #[arg(long, alias = "from-spec")]
        spec: Option<String>,
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
        ReviewCmd::ReviewPrefetch { pr_ref, format, root } => {
            let pr_ref = pr_ref.unwrap_or_default();
            if pr_ref.is_empty() {
                println!("{}",
                    serde_json::to_string_pretty(&serde_json::json!({"error":"pr-ref-required"}))
                        .unwrap_or_default()
                );
            } else {
                review::review_prefetch::run(review::review_prefetch::ReviewPrefetchOpts { pr_ref, format, root });
            }
        }
        ReviewCmd::AcNegativeCheck { spec, confirm, removal, from } => {
            review::ac_negative_check::run(spec.as_deref(), confirm, removal, from.as_deref());
        }
        ReviewCmd::PrList { root } => review::pr_door::run_list(&root),
        ReviewCmd::PrReview { pr, verdict, critical, root } => {
            review::pr_door::run_review(&root, pr, verdict.as_deref(), critical);
        }
        ReviewCmd::PrMerge { pr, confirm, root } => {
            review::pr_door::run_merge(&root, pr, confirm);
        }
        ReviewCmd::PrOpen { base, head, body_file, fill, draft, root } => {
            review::pr_publish::run_open(&root, &base, &head, body_file.as_deref(), fill, draft);
        }
        ReviewCmd::PrEdit { number, body_file, root } => {
            review::pr_publish::run_edit(&root, number, &body_file);
        }
        ReviewCmd::PrReady { number, root } => review::pr_publish::run_ready(&root, number),
        ReviewCmd::FindingCollect { spec } => review::finding_collect::run(spec.as_deref()),
        ReviewCmd::ReviewDispatch { pr, spec, subproject } => {
            review::review_dispatch::run(review::review_dispatch::ReviewDispatchOpts { pr, spec, subproject });
        }
    }
}
