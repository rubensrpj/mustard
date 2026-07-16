//! The `run` subcommands for repo-model retrieval (`feature` / `orient` / glossary).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`ContextCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run context <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{feature, orient, glossary_coverage, grill_capture};

/// The `run` subcommands owned by repo-model retrieval (`feature` / `orient` / glossary).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum ContextCmd {
    /// Research a feature request against the repo via the `scan` digest (no
    /// source reading) and emit the structured insumos for decomposition +
    /// `scan spec`. The grounding step of the elicitation loop.
    #[command(display_order = 3)]
    Feature {
        /// The free-text feature/bugfix request to research. The orchestration
        /// layer passes any cross-lingual translation INSIDE this text
        /// (`--intent "<user prompt> <english translation>"`); the command stays
        /// pure deterministic and queries the DISTINCT union of the tokens.
        #[arg(long)]
        intent: String,
        /// Workspace root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Project `.claude/grain.model.json` into the orientation census — the
    /// terrain map the AI reads instead of cold-starting with `grep`.
    ///
    /// One line per architectural subproject (`name · kind · Nf — role`),
    /// reusing the same `Project.kind` / `Project.code_files` the
    /// subproject-`CLAUDE.md` footer renders, with the architectural layer
    /// (`L0`/`L1`/`L2`) joined from grain's `skeleton[]`. Fail-open: a
    /// missing / unreadable model prints nothing, exit 0. Byte-stable output.
    /// (The per-prompt Level-2 entrypoints were removed: lexical prompt×path
    /// matching measured 1 useful hit in 17 across two field sessions.)
    #[command(display_order = 4)]
    Orient {
        /// Workspace root (holds `.claude/grain.model.json`). Defaults to `.`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Deterministic check of how well a `CONTEXT.md` domain glossary covers the
    /// repo-vocabulary terms a feature intent touches (the digest's matched
    /// terms). Emits byte-stable JSON `{verdict, present, termsTotal,
    /// termsCovered, coveragePct, uncovered, contextFile}` for the `/feature`
    /// ANALYZE glossary loop — `uncovered` is the weak/missing terms to grill,
    /// `contextFile` the resolved destination `grill-capture` writes them into.
    /// Reuses the exact term matcher `context-slice` uses. Fail-open: a missing
    /// model / unreadable glossary degrades to `verdict: "na"`, exit 0.
    #[command(name = "glossary-coverage")]
    #[command(display_order = 5)]
    GlossaryCoverage {
        /// The free-text feature request whose domain terms are scored.
        #[arg(long)]
        intent: String,
        /// A `CONTEXT.md` / `CONTEXT-MAP.md` glossary path. Repeatable.
        #[arg(long)]
        context: Vec<String>,
        /// Workspace root (holds `.claude/grain.model.json`). Defaults to `.`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Persist ONE confirmed glossary term into a `CONTEXT.md` — the write half
    /// of the `/feature` ANALYZE glossary loop. The orchestrator runs the
    /// lightweight inline grill (asks the user for a definition of each
    /// `uncovered` term from `glossary-coverage`) and records every confirmed
    /// pair here, one call per term. Glossary-only; update-not-duplicate (a term
    /// that already has a block is replaced in place, parsed the SAME way the
    /// slicer parses blocks); the target is resolved CONTEXT-MAP-aware. Emits
    /// byte-stable JSON `{ok, action, term, contextFile, reason?}`
    /// (`action ∈ {appended, updated}`). Fail-open: no `--context` destination →
    /// `{ok:false, reason:"no-context-target"}`, exit 0.
    #[command(name = "grill-capture")]
    #[command(display_order = 6)]
    GrillCapture {
        /// The domain term being defined (becomes the block heading). Optional
        /// with `--finalize` (which needs no term).
        #[arg(long, default_value = "")]
        term: String,
        /// The confirmed one-line definition for the term. Optional with
        /// `--finalize`.
        #[arg(long, default_value = "")]
        definition: String,
        /// A `CONTEXT.md` / `CONTEXT-MAP.md` glossary path. Repeatable. The
        /// first resolved (or first requested) path is the write target.
        #[arg(long)]
        context: Vec<String>,
        /// Clarify-finalize (F6): mint `<spec>/.clarified` for the spec — the
        /// marker `approve-spec` requires before a Full plan may be approved —
        /// then exit. Needs no term; the SINGLE explicit "clarification complete"
        /// action (a term capture never mints it).
        #[arg(long)]
        finalize: bool,
        /// The spec to finalize (with `--finalize`). Explicit and robust (mirrors
        /// `approve-spec --spec`); absent, the active spec is resolved from the
        /// session binding. Ignored without `--finalize`.
        #[arg(long, default_value = "")]
        spec: String,
        /// Workspace root. Defaults to `.`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `context`-family `run` subcommand.
pub fn dispatch(cmd: ContextCmd) {
    match cmd {
        ContextCmd::Feature { intent, root } => feature::run(&intent, &root),
        ContextCmd::Orient { root } => orient::run(&root),
        ContextCmd::GlossaryCoverage {
            intent,
            context,
            root,
        } => glossary_coverage::run(&intent, &context, &root),
        ContextCmd::GrillCapture {
            term,
            definition,
            context,
            finalize,
            spec,
            root,
        } => grill_capture::run(&term, &definition, &context, &spec, finalize, &root),
    }
}
