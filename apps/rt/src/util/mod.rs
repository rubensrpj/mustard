//! Shared, dependency-free helpers used across the enforcement modules.
//!
//! It is `mustard-rt`-local — it does not touch `mustard-core`.

pub mod platform;
// The SHA-256 lives in `mustard-core`, where the build scripts and the
// measurement proof share it; the doctor reaches it by this name.
pub use mustard_core::io::sha256;

// Timestamp helpers (`now_iso8601`, `now_unix_millis`) live in the single
// canonical home `mustard_core::time`, and the user's home folder in
// `mustard_core::platform::harness::home_dir` — call them directly, no
// rt-side alias.
