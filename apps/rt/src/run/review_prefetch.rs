//! `mustard-rt run review-prefetch` — pre-structured PR data for LLM review.
//!
//! Shell-outs to `gh pr view --json ...`, parses the response, and re-emits a
//! clean JSON structure ready for the LLM to consume without re-parsing the
//! raw `gh` text output.
//!
//! In `--format table` mode a compact 10-15 line executive summary is printed.
//!
//! ## Fail-open contract
//!
//! - `gh` not in PATH → `{"error":"gh-not-found"}`, exit 0.
//! - `gh` errors (e.g. no auth, invalid PR) → `{"error":"<gh stderr>"}`, exit 0.
//! - JSON parse failures → `{"error":"parse-error"}`, exit 0.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Public options struct
// ---------------------------------------------------------------------------

pub struct ReviewPrefetchOpts {
    /// PR reference: a number like `"123"` or a full GitHub URL.
    pub pr_ref: String,
    pub format: String,
    /// Optional project root (currently unused but kept for future remote resolution).
    pub root: PathBuf,
}

// ---------------------------------------------------------------------------
// PR ref normalisation
// ---------------------------------------------------------------------------

/// Extract the PR number string from either a raw number or a GitHub URL.
/// E.g. `"123"` → `"123"`, `"https://github.com/owner/repo/pull/123"` → `"123"`.
fn normalise_pr_ref(pr_ref: &str) -> String {
    let trimmed = pr_ref.trim();
    // If it looks like a URL, take the last path segment
    if trimmed.contains("github.com") && trimmed.contains("/pull/") {
        if let Some(after_pull) = trimmed.rsplit("/pull/").next() {
            // Remove query strings / fragments
            let num: String = after_pull
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !num.is_empty() {
                return num;
            }
        }
    }
    trimmed.to_string()
}

// ---------------------------------------------------------------------------
// Shell-out to gh
// ---------------------------------------------------------------------------

const GH_FIELDS: &str =
    "title,body,author,baseRefName,headRefName,additions,deletions,changedFiles,comments,reviews,files";

fn fetch_pr(pr_ref: &str) -> Result<Value, String> {
    let ref_str = normalise_pr_ref(pr_ref);

    let output = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "gh", "pr", "view", &ref_str, "--json", GH_FIELDS])
            .output()
    } else {
        Command::new("gh")
            .args(["pr", "view", &ref_str, "--json", GH_FIELDS])
            .output()
    };

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") || msg.contains("No such file") || msg.contains("program") {
                return Err("gh-not-found".to_string());
            }
            return Err(format!("exec-error: {msg}"));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Distinguish "gh not installed" from "command failed"
        if stderr.contains("command not found") || output.status.code() == Some(127) {
            return Err("gh-not-found".to_string());
        }
        return Err(stderr.trim().to_string());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(text.trim()).map_err(|_| "parse-error".to_string())
}

// ---------------------------------------------------------------------------
// JSON structuring
// ---------------------------------------------------------------------------

fn structure_pr(raw: &Value) -> Value {
    let title = raw.get("title").and_then(Value::as_str).unwrap_or("");
    let body = raw.get("body").and_then(Value::as_str).unwrap_or("");
    let author = raw
        .get("author")
        .and_then(|a| a.get("login"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let base_ref = raw.get("baseRefName").and_then(Value::as_str).unwrap_or("");
    let head_ref = raw.get("headRefName").and_then(Value::as_str).unwrap_or("");
    let additions = raw.get("additions").and_then(Value::as_i64).unwrap_or(0);
    let deletions = raw.get("deletions").and_then(Value::as_i64).unwrap_or(0);
    let changed_files = raw.get("changedFiles").and_then(Value::as_i64).unwrap_or(0);

    // Files list
    let files: Vec<Value> = raw
        .get("files")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|f| {
                    json!({
                        "path": f.get("path").and_then(Value::as_str).unwrap_or(""),
                        "additions": f.get("additions").and_then(Value::as_i64).unwrap_or(0),
                        "deletions": f.get("deletions").and_then(Value::as_i64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Comments
    let comments: Vec<Value> = raw
        .get("comments")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|c| {
                    json!({
                        "author": c.get("author").and_then(|a| a.get("login")).and_then(Value::as_str).unwrap_or(""),
                        "body": c.get("body").and_then(Value::as_str).unwrap_or(""),
                        "path": c.get("path").and_then(Value::as_str),
                        "line": c.get("line").and_then(Value::as_i64),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Reviews
    let reviews: Vec<Value> = raw
        .get("reviews")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|r| {
                    let review_comments: Vec<Value> = r
                        .get("comments")
                        .and_then(Value::as_array)
                        .map(|rc| {
                            rc.iter()
                                .map(|c| {
                                    json!({
                                        "author": c.get("author").and_then(|a| a.get("login")).and_then(Value::as_str).unwrap_or(""),
                                        "body": c.get("body").and_then(Value::as_str).unwrap_or(""),
                                        "path": c.get("path").and_then(Value::as_str),
                                        "line": c.get("line").and_then(Value::as_i64),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    json!({
                        "author": r.get("author").and_then(|a| a.get("login")).and_then(Value::as_str).unwrap_or(""),
                        "state": r.get("state").and_then(Value::as_str).unwrap_or(""),
                        "body": r.get("body").and_then(Value::as_str).unwrap_or(""),
                        "comments": review_comments,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    json!({
        "title": title,
        "body": body,
        "author": author,
        "baseRef": base_ref,
        "headRef": head_ref,
        "scope": {
            "additions": additions,
            "deletions": deletions,
            "changedFiles": changed_files,
        },
        "files": files,
        "comments": comments,
        "reviews": reviews,
    })
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_json(pr: &Value) -> String {
    serde_json::to_string_pretty(pr)
        .unwrap_or_else(|_| r#"{"error":"serialize"}"#.to_string())
}

fn render_table(pr: &Value) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Title  : {}",
        pr["title"].as_str().unwrap_or("")
    ));
    lines.push(format!(
        "Author : {}",
        pr["author"].as_str().unwrap_or("")
    ));
    lines.push(format!(
        "Branch : {} → {}",
        pr["headRef"].as_str().unwrap_or(""),
        pr["baseRef"].as_str().unwrap_or("")
    ));
    let scope = &pr["scope"];
    lines.push(format!(
        "Scope  : +{} / -{} lines across {} file(s)",
        scope["additions"].as_i64().unwrap_or(0),
        scope["deletions"].as_i64().unwrap_or(0),
        scope["changedFiles"].as_i64().unwrap_or(0),
    ));
    let comment_count = pr["comments"].as_array().map_or(0, |a| a.len());
    lines.push(format!("Comments: {comment_count}"));

    // Review summary
    let reviews = pr["reviews"].as_array().cloned().unwrap_or_default();
    let approved = reviews
        .iter()
        .filter(|r| r["state"].as_str() == Some("APPROVED"))
        .count();
    let changes = reviews
        .iter()
        .filter(|r| r["state"].as_str() == Some("CHANGES_REQUESTED"))
        .count();
    let dismissed = reviews
        .iter()
        .filter(|r| r["state"].as_str() == Some("DISMISSED"))
        .count();
    lines.push(format!(
        "Reviews: {} APPROVED / {} CHANGES_REQUESTED / {} DISMISSED",
        approved, changes, dismissed
    ));

    // Top 5 changed files
    if let Some(files) = pr["files"].as_array() {
        if !files.is_empty() {
            lines.push(String::new());
            lines.push("Top changed files:".to_string());
            for f in files.iter().take(5) {
                let path = f["path"].as_str().unwrap_or("");
                let add = f["additions"].as_i64().unwrap_or(0);
                let del = f["deletions"].as_i64().unwrap_or(0);
                lines.push(format!("  {path}  (+{add}/-{del})"));
            }
        }
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(opts: ReviewPrefetchOpts) {
    match fetch_pr(&opts.pr_ref) {
        Ok(raw) => {
            let structured = structure_pr(&raw);
            match opts.format.as_str() {
                "table" => println!("{}", render_table(&structured)),
                _ => println!("{}", render_json(&structured)),
            }
        }
        Err(e) => {
            let err_doc = json!({"error": e});
            println!(
                "{}",
                serde_json::to_string_pretty(&err_doc)
                    .unwrap_or_else(|_| format!(r#"{{"error":"{e}"}}"#))
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_gh_response() -> Value {
        json!({
            "title": "Add authentication module",
            "body": "This PR adds JWT auth.",
            "author": {"login": "alice"},
            "baseRefName": "main",
            "headRefName": "feature/auth",
            "additions": 150,
            "deletions": 20,
            "changedFiles": 5,
            "comments": [
                {"author": {"login": "bob"}, "body": "LGTM", "path": null, "line": null}
            ],
            "reviews": [
                {"author": {"login": "carol"}, "state": "APPROVED", "body": "", "comments": []}
            ],
            "files": [
                {"path": "src/auth.ts", "additions": 100, "deletions": 5},
                {"path": "src/middleware.ts", "additions": 50, "deletions": 15}
            ]
        })
    }

    #[test]
    fn structure_pr_extracts_title_and_author() {
        let raw = mock_gh_response();
        let pr = structure_pr(&raw);
        assert_eq!(pr["title"], "Add authentication module");
        assert_eq!(pr["author"], "alice");
    }

    #[test]
    fn structure_pr_extracts_scope() {
        let raw = mock_gh_response();
        let pr = structure_pr(&raw);
        assert_eq!(pr["scope"]["additions"], 150);
        assert_eq!(pr["scope"]["deletions"], 20);
        assert_eq!(pr["scope"]["changedFiles"], 5);
    }

    #[test]
    fn structure_pr_extracts_files() {
        let raw = mock_gh_response();
        let pr = structure_pr(&raw);
        let files = pr["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["path"], "src/auth.ts");
    }

    #[test]
    fn structure_pr_extracts_reviews() {
        let raw = mock_gh_response();
        let pr = structure_pr(&raw);
        let reviews = pr["reviews"].as_array().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0]["state"], "APPROVED");
        assert_eq!(reviews[0]["author"], "carol");
    }

    #[test]
    fn normalise_pr_ref_extracts_number_from_url() {
        assert_eq!(
            normalise_pr_ref("https://github.com/owner/repo/pull/42"),
            "42"
        );
    }

    #[test]
    fn normalise_pr_ref_passthrough_number() {
        assert_eq!(normalise_pr_ref("123"), "123");
        assert_eq!(normalise_pr_ref("  456  "), "456");
    }

    #[test]
    fn render_json_is_valid_json() {
        let raw = mock_gh_response();
        let pr = structure_pr(&raw);
        let out = render_json(&pr);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.get("title").is_some());
    }

    #[test]
    fn render_table_contains_title_and_author() {
        let raw = mock_gh_response();
        let pr = structure_pr(&raw);
        let out = render_table(&pr);
        assert!(out.contains("Add authentication module"), "got: {out}");
        assert!(out.contains("alice"), "got: {out}");
    }

    #[test]
    fn render_table_summarises_reviews() {
        let raw = mock_gh_response();
        let pr = structure_pr(&raw);
        let out = render_table(&pr);
        assert!(out.contains("1 APPROVED"), "got: {out}");
    }
}
