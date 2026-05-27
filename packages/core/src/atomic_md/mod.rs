//! `atomic_md` — shared atomic markdown I/O layer.
//!
//! Provides the building blocks consumed by memory/knowledge/spec readers and
//! the wikilink footer hook (W3D):
//!
//! - [`store`] — [`MarkdownStore`] + [`MarkdownDoc`]: scan, read, write.
//! - [`frontmatter`] — [`Frontmatter`]: lenient YAML header extraction.
//! - [`wikilink`] — pure functions: extract, resolve, render footer.

pub mod frontmatter;
pub mod store;
pub mod wikilink;

pub use frontmatter::Frontmatter;
pub use store::{MarkdownDoc, MarkdownStore};
pub use wikilink::{find_backlinks, find_outgoing_links, render_footer, resolve};
