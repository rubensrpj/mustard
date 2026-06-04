//! `mustard review` — review a pull request with the Claude API.
//!
//! Ported from `commands/review.ts`. The JS port shelled out to the `claude`
//! CLI; the Rust port calls the Claude Messages API directly over HTTP
//! (`ureq`). The API key comes from the `ANTHROPIC_API_KEY` environment
//! variable — no key file, no interactive prompt.
//!
//! The flow:
//!
//! 1. require a PR number and the `gh` CLI (PR metadata + diff still come from
//!    GitHub — there is no provider-agnostic way to fetch a PR diff);
//! 2. fetch PR metadata (`gh pr view --json`) and the unified diff
//!    (`gh pr diff`), truncating the diff to keep the request bounded;
//! 3. assemble a review prompt that folds in the project's `CLAUDE.md` rules
//!    (the `## Guards` section carries the DO/DON'T rules);
//! 4. POST it to `https://api.anthropic.com/v1/messages`;
//! 5. print the review; in `--ci` mode, post it as a PR comment and exit
//!    non-zero when the review flags a `CRITICAL` issue.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use mustard_core::io::fs as mfs;
use serde_json::{Value, json};

/// The Claude Messages API endpoint.
const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
/// The `anthropic-version` header value — the stable, dated API revision.
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
/// The model used for reviews. A capable default; overridable via env.
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
/// Upper bound on diff characters folded into the prompt.
const MAX_DIFF_CHARS: usize = 50_000;

/// Flags accepted by `mustard review`.
#[derive(Debug, Default, Clone)]
pub struct ReviewOptions {
    /// CI mode: post the review as a PR comment, exit non-zero on a
    /// `CRITICAL` finding.
    pub ci: bool,
    /// The PR number to review.
    pub pr: Option<u64>,
}

/// Run `mustard review` in `cwd`.
pub fn review(cwd: &Path, options: &ReviewOptions) -> Result<()> {
    let pr_number = options
        .pr
        .context("PR number required - usage: mustard review --pr <number>")?;

    if !command_available("gh") {
        bail!("GitHub CLI (gh) is required - install from https://cli.github.com/");
    }

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .context("ANTHROPIC_API_KEY is not set - export it before running `mustard review`")?;

    println!(
        "Reviewing PR #{pr_number}{}...",
        if options.ci { " (CI mode)" } else { "" }
    );

    let pr = fetch_pr_metadata(cwd, pr_number)?;
    let diff = fetch_pr_diff(cwd, pr_number)?;
    let prompt = build_review_prompt(&pr, &truncate_diff(&diff), cwd);

    let review_text = call_claude(&api_key, &prompt)?;
    println!("\nReview Result:\n");
    println!("{review_text}");

    if options.ci && !review_text.trim().is_empty() {
        post_pr_comment(cwd, pr_number, &review_text);
        if mentions_critical(&review_text) {
            println!("Critical issues found.");
            bail!("review flagged critical issues");
        }
    }

    println!("Review complete.");
    Ok(())
}

/// Whether `binary --version` runs successfully.
fn command_available(binary: &str) -> bool {
    Command::new(binary)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Fetch PR metadata as a JSON object via `gh pr view --json`.
fn fetch_pr_metadata(cwd: &Path, pr_number: u64) -> Result<Value> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "title,body,additions,deletions,changedFiles,baseRefName,headRefName",
        ])
        .current_dir(cwd)
        .output()
        .context("running `gh pr view`")?;
    if !output.status.success() {
        bail!(
            "`gh pr view` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("parsing `gh pr view` JSON output")
}

/// Fetch the unified PR diff via `gh pr diff`.
fn fetch_pr_diff(cwd: &Path, pr_number: u64) -> Result<String> {
    let output = Command::new("gh")
        .args(["pr", "diff", &pr_number.to_string()])
        .current_dir(cwd)
        .output()
        .context("running `gh pr diff`")?;
    if !output.status.success() {
        bail!(
            "`gh pr diff` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Truncate the diff to [`MAX_DIFF_CHARS`] characters, on a char boundary.
fn truncate_diff(diff: &str) -> String {
    if diff.len() <= MAX_DIFF_CHARS {
        return diff.to_string();
    }
    let mut end = MAX_DIFF_CHARS;
    while end > 0 && !diff.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n... (diff truncated, showing first {MAX_DIFF_CHARS} chars)",
        &diff[..end]
    )
}

/// Read up to `limit` bytes of `path` (on a char boundary), or `None`.
fn read_capped(path: &Path, limit: usize) -> Option<String> {
    let raw = mfs::read_to_string(path).ok()?;
    if raw.len() <= limit {
        return Some(raw);
    }
    let mut end = limit;
    while end > 0 && !raw.is_char_boundary(end) {
        end -= 1;
    }
    Some(raw[..end].to_string())
}

/// Assemble the review prompt from PR metadata, the diff, and project context.
fn build_review_prompt(pr: &Value, diff: &str, cwd: &Path) -> String {
    let str_field = |k: &str| pr.get(k).and_then(Value::as_str).unwrap_or_default();
    let num_field = |k: &str| pr.get(k).and_then(Value::as_i64).unwrap_or_default();

    let mut parts: Vec<String> = vec![
        "Review this pull request for code quality, security, and correctness.".into(),
        String::new(),
        format!("## PR: {}", str_field("title")),
        format!(
            "Base: {} <- Head: {}",
            str_field("baseRefName"),
            str_field("headRefName")
        ),
        format!(
            "Changes: +{} -{} ({} files)",
            num_field("additions"),
            num_field("deletions"),
            num_field("changedFiles")
        ),
        String::new(),
    ];

    let body = str_field("body");
    if !body.is_empty() {
        parts.push("## Description".into());
        parts.push(body.to_string());
        parts.push(String::new());
    }

    // The project's DO/DON'T rules live in the `## Guards` section of `CLAUDE.md`
    // (the legacy standalone `.claude/commands/guards.md` is no longer generated —
    // `scan` writes guards into the CLAUDE.md sentinel block). Folding CLAUDE.md
    // therefore already carries them.
    if let Some(rules) = read_capped(&cwd.join("CLAUDE.md"), 3000) {
        parts.push("## Project Rules (incl. `## Guards`)".into());
        parts.push(rules);
        parts.push(String::new());
    }

    parts.extend(
        [
            "## Review Checklist",
            "- [ ] No security vulnerabilities (injection, XSS, secrets)",
            "- [ ] Code follows project conventions",
            "- [ ] No unnecessary complexity",
            "- [ ] Error handling is appropriate",
            "- [ ] No breaking changes without migration",
            "",
            "## Diff",
            "```diff",
        ]
        .into_iter()
        .map(String::from),
    );
    parts.push(diff.to_string());
    parts.extend(
        [
            "```",
            "",
            "Provide a structured review with:",
            "1. **Summary**: What this PR does (1-2 sentences)",
            "2. **Issues**: List of issues found (CRITICAL / WARNING / INFO)",
            "3. **Suggestions**: Improvements (optional)",
            "4. **Verdict**: APPROVE / REQUEST_CHANGES / COMMENT",
        ]
        .into_iter()
        .map(String::from),
    );

    parts.join("\n")
}

/// POST `prompt` to the Claude Messages API and return the text response.
fn call_claude(api_key: &str, prompt: &str) -> Result<String> {
    let model = std::env::var("MUSTARD_REVIEW_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let request = json!({
        "model": model,
        "max_tokens": 4096,
        "messages": [{ "role": "user", "content": prompt }],
    });

    let response: Value = ureq::post(CLAUDE_API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_API_VERSION)
        .send_json(&request)
        .context("calling the Claude API")?
        .body_mut()
        .read_json()
        .context("parsing the Claude API response")?;

    // Messages API: { "content": [ { "type": "text", "text": "..." }, ... ] }
    let text = response
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .filter(|t| !t.is_empty())
        .context("Claude API response carried no text content")?;
    Ok(text)
}

/// Post the review as a PR comment. Fail-open: a failure warns, never aborts.
fn post_pr_comment(cwd: &Path, pr_number: u64, review_text: &str) {
    let body = format!("## Automated Review (Mustard)\n\n{review_text}");
    let result = Command::new("gh")
        .args(["pr", "comment", &pr_number.to_string(), "--body", &body])
        .current_dir(cwd)
        .output();
    match result {
        Ok(out) if out.status.success() => {
            println!("\nReview posted as comment on PR #{pr_number}");
        }
        Ok(out) => eprintln!(
            "Warning: could not post review comment: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(err) => eprintln!("Warning: could not post review comment: {err}"),
    }
}

/// Whether the review text flags a `CRITICAL` issue (case-insensitive,
/// word-bounded).
fn mentions_critical(review_text: &str) -> bool {
    review_text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case("critical"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_diff_caps_long_input() {
        let long = "x".repeat(MAX_DIFF_CHARS + 100);
        let out = truncate_diff(&long);
        assert!(out.contains("diff truncated"));
        assert!(out.len() < long.len() + 100);
    }

    #[test]
    fn truncate_diff_leaves_short_input() {
        assert_eq!(truncate_diff("small diff"), "small diff");
    }

    #[test]
    fn mentions_critical_is_word_bounded() {
        assert!(mentions_critical("Issue: CRITICAL - sql injection"));
        assert!(mentions_critical("this is critical."));
        assert!(!mentions_critical("non-criticality of the change"));
        assert!(!mentions_critical("all clear, looks good"));
    }

    #[test]
    fn build_review_prompt_includes_pr_facts() {
        let pr = json!({
            "title": "Add login",
            "body": "implements oauth",
            "additions": 12,
            "deletions": 3,
            "changedFiles": 2,
            "baseRefName": "main",
            "headRefName": "feat/login",
        });
        let tmp = tempfile::tempdir().unwrap();
        let prompt = build_review_prompt(&pr, "diff body", tmp.path());
        assert!(prompt.contains("## PR: Add login"));
        assert!(prompt.contains("Base: main <- Head: feat/login"));
        assert!(prompt.contains("+12 -3 (2 files)"));
        assert!(prompt.contains("implements oauth"));
        assert!(prompt.contains("diff body"));
    }
}
