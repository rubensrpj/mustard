//! `mustard-rt run review-prefetch` — pre-structured PR data for LLM review.
//!
//! Shell-outs to `gh pr view --json ...`, parses the response, and re-emits a
//! clean JSON structure ready for the LLM to consume without re-parsing the
//! raw `gh` text output.
//!
//! In `--format table` mode a compact 10-15 line executive summary is printed.
//!
//! ## Routed by the provider IN FORCE — one document, whoever answers
//!
//! Every pull-request WRITE (create/edit/ready) already goes through the
//! provider port ([`crate::shared::pr_provider::PrProvider`], spoken by
//! [`crate::commands::review::pr_publish`]). This READ routes the same way:
//! `github` keeps the `gh pr view` shell-out below; `azure` composes the SAME
//! document out of the REST reads [`crate::shared::pr_azure`] exposes (the PR
//! document, its comment threads, its reviewers) plus the diff counters and
//! file list measured by the LOCAL git — the numbers are identical on every
//! provider and reading them locally spends no API quota. The consumer (the
//! review flow) never learns which provider answered, because the format
//! never changes.
//!
//! ## Fail-open contract
//!
//! - `gh` not in PATH → `{"error":"gh-not-found"}`, exit 0.
//! - `gh` errors (e.g. no auth, invalid PR) → `{"error":"<gh stderr>"}`, exit 0.
//! - Azure REST failures → `{"error":"<stable-prefixed reason>"}`, exit 0.
//! - JSON parse failures → `{"error":"parse-error"}`, exit 0.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::shared::pr_azure;
use crate::shared::pr_provider::{short_ref, PROVIDER_AZURE};

// ---------------------------------------------------------------------------
// Public options struct
// ---------------------------------------------------------------------------

pub struct ReviewPrefetchOpts {
    /// PR reference: a number like `"123"` or a full PR web URL.
    pub pr_ref: String,
    pub format: String,
    /// The repository the provider is resolved FOR (and the cwd of every
    /// local git read).
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
// Azure route — the SAME document, composed from the REST reads + local git
// ---------------------------------------------------------------------------

/// The PR number out of a raw number or an Azure PR web URL
/// (`.../_git/<repo>/pullrequest/<n>`): the digits of the last path segment.
fn azure_pr_number(pr_ref: &str) -> Option<u64> {
    let last = pr_ref.trim().trim_end_matches('/').rsplit('/').next()?;
    let digits: String = last.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Parse `git diff --numstat` output into the document's file rows. A binary
/// file prints `-` in both columns — its path is still listed, its line
/// counts read as zero (the same thing GitHub's `files` field reports).
fn files_from_numstat(numstat: &str) -> Vec<Value> {
    numstat
        .lines()
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let additions = cols.next()?.trim().parse::<i64>().unwrap_or(0);
            let deletions = cols.next()?.trim().parse::<i64>().unwrap_or(0);
            let path = cols.next()?.trim();
            if path.is_empty() {
                return None;
            }
            Some(json!({ "path": path, "additions": additions, "deletions": deletions }))
        })
        .collect()
}

/// One text field of a JSON document, empty when absent — the lenient read
/// every field of the composition uses.
fn text_field<'a>(doc: &'a Value, key: &str) -> &'a str {
    doc.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Map an Azure reviewer's `vote` onto the review-state vocabulary the GitHub
/// document already speaks. The REST contract stores five values: 10
/// (approved) and 5 (approved with suggestions) approve; -5 (waiting for
/// author) and -10 (rejected) ask for changes; 0 is NO vote and the caller
/// filters it out before this map — a reviewer who has not voted is not a
/// review, exactly as GitHub's `reviews` list omits the silent ones.
fn review_state_of_vote(vote: i64) -> &'static str {
    if vote > 0 { "APPROVED" } else { "CHANGES_REQUESTED" }
}

/// Compose the SAME document [`structure_pr`] emits for GitHub, out of the
/// three Azure REST reads plus the file rows the local git measured. Pure —
/// the tests drive it with a fixed table, never a network.
///
/// - `comments` come from the ACTIVE threads only (a resolved thread is
///   feedback already absorbed), skipping Azure's `system` comments (vote and
///   ref updates, not conversation). The thread's file path loses its leading
///   `/` so it matches the git-relative paths in `files`.
/// - `reviews` are the reviewers who VOTED, their vote reduced onto the
///   GitHub state words the table renderer already counts.
/// - `scope` is summed from the file rows — one source for both facts, so the
///   counters can never disagree with the list under them.
fn compose_azure(pr: &Value, threads: &[Value], reviewers: &[Value], files: Vec<Value>) -> Value {
    let author = pr
        .get("createdBy")
        .and_then(|a| a.get("displayName"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let additions: i64 = files.iter().filter_map(|f| f["additions"].as_i64()).sum();
    let deletions: i64 = files.iter().filter_map(|f| f["deletions"].as_i64()).sum();
    let changed_files = files.len() as i64;

    let comments: Vec<Value> = threads
        .iter()
        .filter(|thread| thread.get("status").and_then(Value::as_str) == Some("active"))
        .flat_map(|thread| {
            let context = thread.get("threadContext");
            let path = context
                .and_then(|c| c.get("filePath"))
                .and_then(Value::as_str)
                .map(|p| p.trim_start_matches('/').to_string());
            let line = context
                .and_then(|c| c.get("rightFileStart"))
                .and_then(|s| s.get("line"))
                .and_then(Value::as_i64);
            thread
                .get("comments")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter(|c| {
                            c.get("commentType").and_then(Value::as_str) != Some("system")
                        })
                        .map(|c| {
                            json!({
                                "author": c.get("author").and_then(|a| a.get("displayName")).and_then(Value::as_str).unwrap_or(""),
                                "body": c.get("content").and_then(Value::as_str).unwrap_or(""),
                                "path": path.clone(),
                                "line": line,
                            })
                        })
                        .collect::<Vec<Value>>()
                })
                .unwrap_or_default()
        })
        .collect();

    let reviews: Vec<Value> = reviewers
        .iter()
        .filter_map(|reviewer| {
            let vote = reviewer.get("vote").and_then(Value::as_i64).unwrap_or(0);
            if vote == 0 {
                return None;
            }
            Some(json!({
                "author": reviewer.get("displayName").and_then(Value::as_str).unwrap_or(""),
                "state": review_state_of_vote(vote),
                "body": "",
                "comments": [],
            }))
        })
        .collect();

    json!({
        "title": text_field(pr, "title"),
        "body": text_field(pr, "description"),
        "author": author,
        "baseRef": short_ref(text_field(pr, "targetRefName")),
        "headRef": short_ref(text_field(pr, "sourceRefName")),
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

/// The file rows measured by the LOCAL git: `diff --numstat` over the
/// three-dot range, remote-tracking refs first (the shape right after a
/// fetch), then the local names. When neither pair resolves the read REFUSES,
/// naming both ranges — file rows nobody measured printed as an empty diff
/// would be the unmeasured-as-zero lie this crate exists to end.
fn local_files(root: &Path, base: &str, head: &str) -> Result<Vec<Value>, String> {
    let candidates =
        [(format!("origin/{base}"), format!("origin/{head}")), (base.to_string(), head.to_string())];
    for (b, h) in &candidates {
        let Ok(out) = Command::new("git")
            .args(["diff", "--numstat", &format!("{b}...{h}")])
            .current_dir(root)
            .output()
        else {
            continue;
        };
        if out.status.success() {
            return Ok(files_from_numstat(&String::from_utf8_lossy(&out.stdout)));
        }
    }
    Err(format!(
        "diff-unreadable: neither origin/{base}...origin/{head} nor {base}...{head} resolved \
         locally — fetch the branches first"
    ))
}

/// The Azure fetch: number out of the ref, the three REST documents through
/// [`pr_azure::prefetch`], the diff from the local git, one pure composition.
fn fetch_azure(root: &Path, pr_ref: &str) -> Result<Value, String> {
    let number =
        azure_pr_number(pr_ref).ok_or_else(|| format!("pr-ref-unreadable: {pr_ref}"))?;
    let fetched = pr_azure::prefetch(root, number)?;
    let base = short_ref(text_field(&fetched.pr, "targetRefName")).to_string();
    let head = short_ref(text_field(&fetched.pr, "sourceRefName")).to_string();
    let files = local_files(root, &base, &head)?;
    Ok(compose_azure(&fetched.pr, &fetched.threads, &fetched.reviewers, files))
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
        "Reviews: {approved} APPROVED / {changes} CHANGES_REQUESTED / {dismissed} DISMISSED"
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
    // The provider IN FORCE picks the route — the same resolution every PR
    // write already goes through, reused rather than re-derived. The document
    // is one whoever answers; only the fetch differs.
    let cfg = mustard_core::ProjectConfig::load(&opts.root);
    let provider = mustard_core::resolve_provider(&opts.root, &cfg.git.provider);
    let fetched = if provider == PROVIDER_AZURE {
        fetch_azure(&opts.root, &opts.pr_ref)
    } else {
        fetch_pr(&opts.pr_ref).map(|raw| structure_pr(&raw))
    };
    match fetched {
        Ok(structured) => {
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

    // -----------------------------------------------------------------------
    // The Azure route — the composition is pure and driven by a fixed table
    // -----------------------------------------------------------------------

    fn mock_azure_pr() -> Value {
        json!({
            "pullRequestId": 42,
            "title": "Add authentication module",
            "description": "This PR adds JWT auth.",
            "createdBy": { "displayName": "alice", "uniqueName": "alice@corp" },
            "sourceRefName": "refs/heads/feature/auth",
            "targetRefName": "refs/heads/main",
        })
    }

    fn mock_azure_threads() -> Vec<Value> {
        vec![
            // An active thread on a file: its human comment enters, its
            // system comment (a vote update) does not.
            json!({
                "status": "active",
                "threadContext": {
                    "filePath": "/src/auth.ts",
                    "rightFileStart": { "line": 12 },
                },
                "comments": [
                    { "commentType": "text", "content": "rename this",
                      "author": { "displayName": "bob" } },
                    { "commentType": "system", "content": "bob voted 10",
                      "author": { "displayName": "azure" } },
                ],
            }),
            // A resolved thread is feedback already absorbed — skipped whole.
            json!({
                "status": "fixed",
                "comments": [
                    { "commentType": "text", "content": "old note",
                      "author": { "displayName": "carol" } },
                ],
            }),
            // An active PR-level thread (no file context): path/line are null.
            json!({
                "status": "active",
                "comments": [
                    { "commentType": "text", "content": "LGTM overall",
                      "author": { "displayName": "carol" } },
                ],
            }),
        ]
    }

    fn mock_azure_reviewers() -> Vec<Value> {
        vec![
            json!({ "displayName": "carol", "vote": 10 }),
            json!({ "displayName": "dave", "vote": -5 }),
            // No vote yet: not a review — GitHub's list omits the silent too.
            json!({ "displayName": "erin", "vote": 0 }),
        ]
    }

    fn composed_azure() -> Value {
        let files = files_from_numstat("100\t5\tsrc/auth.ts\n50\t15\tsrc/middleware.ts\n");
        compose_azure(&mock_azure_pr(), &mock_azure_threads(), &mock_azure_reviewers(), files)
    }

    /// T3 — the Azure composition emits the SAME document shape as the GitHub
    /// path, key for key, so the consumer never learns who answered.
    #[test]
    fn azure_document_has_the_same_shape_as_the_github_one() {
        let github = structure_pr(&mock_gh_response());
        let azure = composed_azure();
        let keys = |doc: &Value| -> Vec<String> {
            doc.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default()
        };
        assert_eq!(keys(&github), keys(&azure), "one document, whoever answers");
        assert_eq!(keys(&github["scope"]), keys(&azure["scope"]));
        assert_eq!(
            keys(&github["comments"][0]),
            keys(&azure["comments"][0]),
            "comment rows carry the same fields",
        );
        assert_eq!(
            keys(&github["reviews"][0]),
            keys(&azure["reviews"][0]),
            "review rows carry the same fields",
        );
        // And the same renderer consumes it unchanged.
        let table = render_table(&azure);
        assert!(table.contains("alice"), "got: {table}");
        assert!(table.contains("feature/auth → main"), "got: {table}");
        assert!(table.contains("1 APPROVED / 1 CHANGES_REQUESTED"), "got: {table}");
    }

    /// T3 — the field mappings: refs come out short, the body is Azure's
    /// `description`, comments come from ACTIVE threads only (system rows and
    /// resolved threads never enter), the thread's path loses its leading `/`,
    /// and votes reduce onto the GitHub state words (a 0 vote is no review).
    #[test]
    fn an_azure_document_is_composed_from_the_port() {
        let doc = composed_azure();
        assert_eq!(doc["title"], "Add authentication module");
        assert_eq!(doc["body"], "This PR adds JWT auth.");
        assert_eq!(doc["author"], "alice");
        assert_eq!(doc["headRef"], "feature/auth", "full refs come out short");
        assert_eq!(doc["baseRef"], "main");

        assert_eq!(
            doc["comments"],
            json!([
                { "author": "bob", "body": "rename this", "path": "src/auth.ts", "line": 12 },
                { "author": "carol", "body": "LGTM overall", "path": null, "line": null },
            ]),
            "active threads only, no system rows, git-relative path",
        );
        assert_eq!(
            doc["reviews"],
            json!([
                { "author": "carol", "state": "APPROVED", "body": "", "comments": [] },
                { "author": "dave", "state": "CHANGES_REQUESTED", "body": "", "comments": [] },
            ]),
            "10 approves, -5 asks for changes, 0 is not a review",
        );

        // Scope is summed from the SAME rows it sits above.
        assert_eq!(doc["scope"], json!({ "additions": 150, "deletions": 20, "changedFiles": 2 }));
        assert_eq!(doc["files"][0]["path"], "src/auth.ts");
    }

    /// T3 — the numstat parse: plain rows carry their counts, a binary row
    /// (`-` columns) keeps its path with zero lines, junk lines never enter.
    #[test]
    fn numstat_rows_become_file_rows() {
        let files = files_from_numstat("3\t1\ta.rs\n-\t-\tlogo.png\n\nnot-a-row\n");
        assert_eq!(
            files,
            vec![
                json!({ "path": "a.rs", "additions": 3, "deletions": 1 }),
                json!({ "path": "logo.png", "additions": 0, "deletions": 0 }),
            ],
        );
        assert!(files_from_numstat("").is_empty());
    }

    /// T3 — the Azure PR reference: a bare number, the PR web URL, or junk.
    #[test]
    fn azure_pr_number_reads_the_url_tail() {
        assert_eq!(azure_pr_number("123"), Some(123));
        assert_eq!(
            azure_pr_number("https://dev.azure.com/org/proj/_git/repo/pullrequest/42"),
            Some(42),
        );
        assert_eq!(azure_pr_number("https://dev.azure.com/org/proj/_git/repo/pullrequest/42/"), Some(42));
        assert_eq!(azure_pr_number("not-a-ref"), None);
    }
}
