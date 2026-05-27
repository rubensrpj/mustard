//! Wave 6B fixture-mode rewrite of `top_files_today_test.rs`.
//!
//! Legacy: built an `events` SQLite table with `pipeline.tool_use` rows and
//! asserted that `aggregate_activity_from_db` returned the right "top
//! files" buckets. Wave 6A retired that path — `aggregate_activity_from_db`
//! is now a facade stub returning `Ok(vec![])`. This rewrite exercises the
//! public signature on a clean repo and asserts the empty contract.

use mustard_dashboard_lib::db;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn aggregate_activity_signature_compiles_against_facade() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".claude").join("spec")).unwrap();
    let base = PathBuf::from(tmp.path());

    // The facade's `with_db` returns None pre-emptively, so the closure is
    // never invoked. The test guards against the closure's *type* drifting —
    // a future signature change would surface here as a compile error.
    let outcome: Option<Result<usize, String>> =
        db::with_db(&base, |conn| db::aggregate_activity_from_db(conn, None, 10).map(|v| v.len()));
    assert!(outcome.is_none(), "facade must short-circuit before closure runs");
}
