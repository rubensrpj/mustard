//! Build script — sole job: tell Cargo to recompile this crate when the release
//! workflow sets or changes `MUSTARD_RELEASE_VERSION`.
//!
//! `platform::harness::harness_version` reads that env via `option_env!`, which
//! the compiler evaluates at build time. Without this hint Cargo does not track
//! arbitrary env vars, so a cached artifact would keep a stale (or unset)
//! version even after the env changes. A clean release build is unaffected;
//! this makes incremental builds honest too.

fn main() {
    println!("cargo:rerun-if-env-changed=MUSTARD_RELEASE_VERSION");
}
