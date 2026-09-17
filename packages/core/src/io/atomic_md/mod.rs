//! `atomic_md` — shared atomic markdown I/O layer.
//!
//! Provides the building blocks consumed by memory/knowledge/spec readers and
//! the wikilink footer hook (W3D):
//!
//! - [`frontmatter`] — [`Frontmatter`]: lenient YAML header extraction.
//! - [`wikilink`] — pure functions: extract, resolve, render footer.

pub mod wikilink;

pub use wikilink::{find_outgoing_links, render_footer, resolve, scan_links};
