//! Readers of the `## Guards` block of a subproject instruction file.
//!
//! Older scans seeded a pending Guards block into every subproject
//! `CLAUDE.md`; the scan no longer writes any `CLAUDE.md`, and the commands
//! that listed and filled those blocks are gone. What stays only reads:
//!
//! - [`list`] finds the files still carrying a pending block (the doctor's
//!   `guards-scaffold` advisory) and says which subproject a file belongs to;
//! - [`apply`] reads the `[critical]` Guards the post-edit gate enforces.
//!
//! Both reuse the marker constants from `scan_claude` (single source — no
//! literal drift).

pub mod apply;
pub mod list;
