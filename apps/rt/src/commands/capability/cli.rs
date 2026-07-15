//! The `run` subcommands for capability docs (`capability/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`CapabilityCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run capability <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;

use crate::commands::{capability};

/// The `run` subcommands owned by capability docs (`capability/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum CapabilityCmd {
    /// Author or read a durable capability doc under `.claude/capabilities/`.
    ///
    /// `create --slug X --title Y [--status active]` writes
    /// `.claude/capabilities/{slug}.md` (id `cap.{slug}`) — frontmatter
    /// (`id` / `status`) + title + the structural `### Requirement:` /
    /// `#### Scenario:` body + `## Covers` / `## Specs` / `## Related` link
    /// sections. Errors (JSON, exit 0) if the doc already exists.
    ///
    /// `show --slug X` parses the doc and prints the
    /// [`mustard_core::domain::capability::Capability`] as byte-stable JSON.
    ///
    /// `sync-nodes --slug X` materializes a `.claude/graph/{id}.md` node (with
    /// frontmatter `id: {id}`) for each `entity.{name}` cover that exists in the
    /// grain registry, so the EXISTING wikilink resolver dereferences the cover
    /// link; an unknown / typo'd cover is skipped (stays `⚠ unresolved`).
    /// All reuse the core type + the single `[[ ]]` scanner; fail-open.
    #[command(display_order = 63)]
    Capability {
        /// Verb: `create` (default), `show`, or `sync-nodes`.
        subcommand: Option<String>,
        /// Capability slug (the `{slug}` in `cap.{slug}` and the file name).
        #[arg(long)]
        slug: String,
        /// `create` only — human-readable title (narrative locale).
        #[arg(long, default_value = "")]
        title: String,
        /// `create` only — lifecycle word (defaults to `active`).
        #[arg(long, default_value = "active")]
        status: String,
    },
}

/// Dispatch one `capability`-family `run` subcommand.
pub fn dispatch(cmd: CapabilityCmd) {
    match cmd {
        CapabilityCmd::Capability {
            subcommand,
            slug,
            title,
            status,
        } => {
            capability::dispatch(
                subcommand.as_deref(),
                &slug,
                capability::CapabilityCreateOpts { slug: slug.clone(), title, status },
            );
        }
    }
}
