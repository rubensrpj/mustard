//! **Deprecated location.** The fail-open filesystem primitives moved to the
//! crate-wide canonical seam [`crate::fs`] (the single `std::fs` call site for
//! the whole monorepo). This module is now a thin re-export so existing
//! `crate::store::fs::…` / `mustard_core::store::fs::…` references keep
//! compiling; new code should import from [`crate::fs`] directly.
//!
//! See [`crate::fs`] for the [`Fs`](crate::fs::Fs) port, [`RealFs`](crate::fs::real::RealFs),
//! the in-memory [`FakeFs`](crate::fs::memory::FakeFs), and the module-level
//! free functions.

pub use crate::fs::{append_line, exists, read_to_string, write_atomic};
