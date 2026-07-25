// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! A counter with nothing behind it says so instead of publishing a number.
//!
//! `pass1` ("specs that finished on the first attempt") divided by a retry count
//! the projection never filled. Every spec looked retry-free, so every project
//! in existence reported a 100% first-pass rate — a measurement nobody took,
//! trusted precisely because it looked like one.
//!
//! Three halves, matching the three ways the number can be wrong:
//!
//! 1. a spec whose projection carries no retry count makes the total unknowable;
//! 2. a real retry count is counted, and a spec that retried is not a pass-1;
//! 3. a rate with an empty denominator is undefined, never `0`.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt metrics_counters_declare_unknown_when_underived --
//! --exact`, and libtest matches `--exact` against the FULL test path — which
//! equals the bare function name only at the root of an integration-test binary.

use mustard_rt::commands::economy::metrics::pipelines_from_specs;
use serde_json::{json, Value};

/// One row of the spec list `metrics collect` folds, with the retry count either
/// measured (`Some`) or absent from the projection (`None`).
fn spec_row(name: &str, retries: Option<i64>) -> Value {
    let metrics = match retries {
        Some(n) => json!({ "apiCalls": 0, "retries": n }),
        None => json!({ "apiCalls": 0 }),
    };
    json!({ "name": name, "metrics": metrics, "isOrphaned": false })
}

#[test]
fn metrics_counters_declare_unknown_when_underived() {
    // --- 1. One unmeasured spec makes the total a guess --------------------
    let doc = pipelines_from_specs(vec![
        spec_row("alpha", Some(0)),
        spec_row("beta", None), // projection published no retry count
    ]);

    assert_eq!(
        doc["tracked"],
        json!(2),
        "the specs themselves WERE read: {doc:#}"
    );
    for counter in ["pass1", "pass1Pct"] {
        assert_eq!(
            doc[counter],
            json!("unknown"),
            "`{counter}` has no readable source and must say so: {doc:#}"
        );
        assert!(
            !doc[counter].is_number(),
            "`{counter}` must not publish a number it never derived: {doc:#}"
        );
    }

    // --- 2. A measured retry count is honoured ----------------------------
    let doc = pipelines_from_specs(vec![
        spec_row("alpha", Some(0)),
        spec_row("beta", Some(2)),
        spec_row("gamma", Some(0)),
    ]);
    assert_eq!(
        doc["pass1"],
        json!(2),
        "the spec that retried is not a first-pass: {doc:#}"
    );
    assert_eq!(doc["pass1Pct"], json!(66), "{doc:#}");
    assert_ne!(
        doc["pass1Pct"],
        json!(100),
        "the old defect: every project reported 100%: {doc:#}"
    );

    // --- 3. A rate over nothing is undefined, not zero --------------------
    let doc = pipelines_from_specs(Vec::new());
    assert_eq!(doc["tracked"], json!(0), "{doc:#}");
    assert_eq!(
        doc["pass1Pct"],
        json!("unknown"),
        "no spec means no rate — `0%` would read as a bad result: {doc:#}"
    );
}
