//! `mustard-rt run pr-open` / `pr-edit` / `pr-ready` — the pull-request
//! PUBLISH actions, behind the provider port.
//!
//! Until this module, the `/mustard:pr` and `/git` prose told the model to run
//! `rtk gh pr create/edit/ready` directly — `github` fixed in text no test
//! covers. Each command here resolves the provider IN FORCE through
//! [`provider_for`] (`git.provider` declared wins, then the `origin` remote,
//! then the fallback) and speaks only the [`PrProvider`] port, so WHICH
//! provider answers is an internal detail: the prose names this command, never
//! a CLI.
//!
//! ## The contract of every answer
//!
//! One JSON document, always exit 0: [`PrPublishReport`] with `ok`, the
//! `action` taken, the `provider` that was asked, and — on success — the
//! `number`/`url` the action itself proved. Failure degrades into the `error`
//! FIELD (the port's stable tokens, or the CLI's own stderr), never a panic
//! and never a non-zero exit: `clippy::unwrap_used` is `deny` crate-wide and
//! the caller is prose that must keep reading JSON whatever the network did.
//!
//! ## The title is read out of the body file
//!
//! `pr-open` takes no `--title` knob: the ritual already writes
//! `<spec>/pr-body.md` BEFORE opening (it is committed with the spec), and
//! that document's first heading IS the pull request's title. Deriving it here
//! ([`title_from_body`]) keeps the two from drifting apart — a title typed
//! separately describes the body it was typed against, not the one that ships.
//! A body with no usable line falls back to the head branch name, so the
//! create never fails for want of a word.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::commands::review::pr_door::project_root;
use crate::shared::pr_provider::{provider_for, PrProvider, PrToOpen};

/// The `action` vocabulary — closed, `&'static str` so the report stays
/// byte-stable and no caller re-parses a free string.
const ACTION_OPEN: &str = "open";
const ACTION_EDIT: &str = "edit";
const ACTION_READY: &str = "ready";

/// The stable token of a `--body-file` this process could not read. The path
/// itself came from argv, so the operator already knows it; the token is what
/// the prose branches on.
const BODY_FILE_UNREADABLE: &str = "body-file-unreadable";

// ---------------------------------------------------------------------------
// The report — the one JSON document each command answers
// ---------------------------------------------------------------------------

/// What one publish action answered.
///
/// `ok: false` means the provider did not perform the action — the reason is
/// in `error`, verbatim (the port's stable tokens such as
/// `provider-unsupported` / `gh-not-found`, or the provider CLI's own stderr).
/// Optional fields are skipped when absent so the emitted JSON stays
/// byte-identical run over run.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct PrPublishReport {
    /// False only when the action was NOT performed.
    pub ok: bool,
    /// Which action was asked: `open`, `edit` or `ready`.
    pub action: &'static str,
    /// The provider token that was asked (`resolve_provider`'s vocabulary) —
    /// named even on failure, so the operator knows WHO refused.
    pub provider: String,
    /// The PR number the action proved (`open`) or acted on (`edit`/`ready`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// The PR's web URL — only `open` learns it, from the create itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Why the action did not happen. Absent on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PrPublishReport {
    /// The failure shape every builder degrades to: the action and provider
    /// still named, the reason in the field, `number` echoing what the caller
    /// pointed at (when it pointed at one).
    fn failed(action: &'static str, provider: String, number: Option<u64>, error: String) -> Self {
        Self { ok: false, action, provider, number, url: None, error: Some(error) }
    }
}

// ---------------------------------------------------------------------------
// Builders — pure over the port, so a table of fakes proves every shape
// ---------------------------------------------------------------------------

/// Ask `provider` to open `pr` and fold the answer into the report.
///
/// On success the report carries exactly what the create itself proved
/// (number + URL, per [`crate::shared::pr_provider::PrOpened`]); on failure
/// there is no number to echo — nothing was created to point at.
#[must_use]
pub(crate) fn open_report(provider: &dyn PrProvider, pr: &PrToOpen) -> PrPublishReport {
    let name = provider.provider().to_string();
    match provider.open(pr) {
        Ok(opened) => PrPublishReport {
            ok: true,
            action: ACTION_OPEN,
            provider: name,
            number: Some(opened.number),
            url: Some(opened.url),
            error: None,
        },
        Err(error) => PrPublishReport::failed(ACTION_OPEN, name, None, error),
    }
}

/// Ask `provider` to replace the body of PR `number`.
///
/// The number is echoed on BOTH outcomes — it identifies which PR the edit
/// was for, and the caller handed it in.
#[must_use]
pub(crate) fn edit_report(provider: &dyn PrProvider, number: u64, body: &str) -> PrPublishReport {
    let name = provider.provider().to_string();
    match provider.edit_body(number, body) {
        Ok(()) => PrPublishReport {
            ok: true,
            action: ACTION_EDIT,
            provider: name,
            number: Some(number),
            url: None,
            error: None,
        },
        Err(error) => PrPublishReport::failed(ACTION_EDIT, name, Some(number), error),
    }
}

/// Ask `provider` to mark draft PR `number` ready for review.
#[must_use]
pub(crate) fn ready_report(provider: &dyn PrProvider, number: u64) -> PrPublishReport {
    let name = provider.provider().to_string();
    match provider.ready(number) {
        Ok(()) => PrPublishReport {
            ok: true,
            action: ACTION_READY,
            provider: name,
            number: Some(number),
            url: None,
            error: None,
        },
        Err(error) => PrPublishReport::failed(ACTION_READY, name, Some(number), error),
    }
}

/// The pull request's title, read out of its own body document.
///
/// The first non-empty line wins; a markdown heading sheds its `#` markers
/// (the `pr-body.md` ritual opens with one). A body with no usable line —
/// empty, or nothing but `#` rulers — falls back to the head branch name, so
/// the create is never refused for want of a title.
fn title_from_body(body: &str, head: &str) -> String {
    for line in body.lines() {
        let stripped = line.trim().trim_start_matches('#').trim();
        if !stripped.is_empty() {
            return stripped.to_string();
        }
    }
    head.to_string()
}

// ---------------------------------------------------------------------------
// CLI faces — resolve the root, pick the adapter, build the report, print it
// ---------------------------------------------------------------------------

/// Print one report as the single JSON document the command answers with.
fn emit(report: &PrPublishReport) {
    println!("{}", serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string()));
}

/// The title and body `--fill` derives from the commits `base..head` carries —
/// the shape the submodule flow needs, where no `pr-body.md` ritual exists:
/// title = the newest commit's subject (a submodule unit usually has one), body
/// = the whole `git log --oneline` of the range. Answers `Err` with git's own
/// words when the range cannot be read, so the report names the reason.
fn fill_from_commits(repo: &Path, base: &str, head: &str) -> Result<(String, String), String> {
    let out = std::process::Command::new("git")
        .args(["log", "--format=%s", &format!("{base}..{head}")])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git-log-failed: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "git-log-failed".to_string() } else { err });
    }
    let subjects: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let Some(title) = subjects.first().cloned() else {
        return Err(format!("nothing-to-fill: {base}..{head} carries no commits"));
    };
    let body = subjects.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n");
    Ok((title, body))
}

/// Dispatch `mustard-rt run pr-open`. Exactly ONE body source: `--body-file`
/// (the pr-body.md ritual) or `--fill` (title/body from the commits, the
/// submodule flow's shape) — neither, or both, is refused in the report.
pub fn run_open(
    root: &Path,
    base: &str,
    head: &str,
    body_file: Option<&Path>,
    fill: bool,
    draft: bool,
) {
    let repo = project_root(root);
    let provider = provider_for(&repo);
    let sourced = match (body_file, fill) {
        (Some(_), true) => Err("body-file-and-fill-are-exclusive".to_string()),
        (None, false) => Err("body-source-missing: pass --body-file or --fill".to_string()),
        (Some(path), false) => fs::read_to_string(path)
            .map(|body| (title_from_body(&body, head), body))
            .map_err(|_| BODY_FILE_UNREADABLE.to_string()),
        (None, true) => fill_from_commits(&repo, base, head),
    };
    let report = match sourced {
        Ok((title, body)) => {
            let pr = PrToOpen {
                title,
                body,
                head: head.to_string(),
                base: base.to_string(),
                draft,
            };
            open_report(provider.as_ref(), &pr)
        }
        Err(error) => {
            PrPublishReport::failed(ACTION_OPEN, provider.provider().to_string(), None, error)
        }
    };
    emit(&report);
}

/// Dispatch `mustard-rt run pr-edit`.
pub fn run_edit(root: &Path, number: u64, body_file: &Path) {
    let repo = project_root(root);
    let provider = provider_for(&repo);
    let report = match fs::read_to_string(body_file) {
        Ok(body) => edit_report(provider.as_ref(), number, &body),
        Err(_) => PrPublishReport::failed(
            ACTION_EDIT,
            provider.provider().to_string(),
            Some(number),
            BODY_FILE_UNREADABLE.to_string(),
        ),
    };
    emit(&report);
}

/// Dispatch `mustard-rt run pr-ready`.
pub fn run_ready(root: &Path, number: u64) {
    let repo = project_root(root);
    let provider = provider_for(&repo);
    emit(&ready_report(provider.as_ref(), number));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use crate::shared::pr_provider::{PrOpened, PrView};

    /// A [`PrProvider`] answering from a table, like `branch_state`'s
    /// `FakePr`: each operation's answer is set up front, and every call is
    /// recorded so a test can assert WHAT the builder handed the port.
    struct FakePub {
        name: &'static str,
        open: Result<PrOpened, String>,
        edit: Result<(), String>,
        ready: Result<(), String>,
        seen: RefCell<Vec<String>>,
    }

    impl FakePub {
        /// A provider on which every operation succeeds.
        fn green(name: &'static str) -> Self {
            Self {
                name,
                open: Ok(PrOpened { number: 7, url: "https://example.test/pr/7".into() }),
                edit: Ok(()),
                ready: Ok(()),
                seen: RefCell::new(Vec::new()),
            }
        }

        /// A provider on which every operation answers `token` — the shape of
        /// an absent CLI, a refused create, or an unadapted provider.
        fn red(name: &'static str, token: &str) -> Self {
            Self {
                name,
                open: Err(token.to_string()),
                edit: Err(token.to_string()),
                ready: Err(token.to_string()),
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl PrProvider for FakePub {
        fn provider(&self) -> &str {
            self.name
        }

        fn open(&self, pr: &PrToOpen) -> Result<PrOpened, String> {
            self.seen.borrow_mut().push(format!(
                "open title={} head={} base={} draft={}",
                pr.title, pr.head, pr.base, pr.draft
            ));
            self.open.clone()
        }

        fn edit_body(&self, number: u64, body: &str) -> Result<(), String> {
            self.seen.borrow_mut().push(format!("edit {number} body={body}"));
            self.edit.clone()
        }

        fn ready(&self, number: u64) -> Result<(), String> {
            self.seen.borrow_mut().push(format!("ready {number}"));
            self.ready.clone()
        }

        fn view(&self, _number: Option<u64>) -> Result<PrView, String> {
            Err("view-not-under-test".to_string())
        }
    }

    fn to_open() -> PrToOpen {
        PrToOpen {
            title: "the unit".into(),
            body: "b".into(),
            head: "feature/my-unit".into(),
            base: "dev".into(),
            draft: true,
        }
    }

    /// A successful open answers exactly what the create proved — number and
    /// URL — names the provider, and carries no error field at all.
    #[test]
    fn pr_open_reports_through_the_port() {
        let fake = FakePub::green("github");
        let report = open_report(&fake, &to_open());
        assert_eq!(
            report,
            PrPublishReport {
                ok: true,
                action: "open",
                provider: "github".into(),
                number: Some(7),
                url: Some("https://example.test/pr/7".into()),
                error: None,
            }
        );
        assert_eq!(
            fake.seen.borrow().as_slice(),
            ["open title=the unit head=feature/my-unit base=dev draft=true"],
            "the port receives the request verbatim",
        );
        let json = serde_json::to_string(&report).unwrap_or_default();
        assert!(!json.contains("error"), "absent fields are skipped, byte-stably: {json}");
    }

    /// Every failure — table-driven over the three actions — degrades into the
    /// `error` field with the provider still named: `ok:false`, exit stays 0,
    /// and `open` echoes NO number because nothing was created to point at.
    #[test]
    fn failures_degrade_into_the_error_field_never_a_panic() {
        let fake = FakePub::red("azure", "provider-unsupported");
        let cases: Vec<(PrPublishReport, &str, Option<u64>)> = vec![
            (open_report(&fake, &to_open()), "open", None),
            (edit_report(&fake, 12, "new body"), "edit", Some(12)),
            (ready_report(&fake, 12), "ready", Some(12)),
        ];
        for (report, action, number) in cases {
            assert_eq!(
                report,
                PrPublishReport {
                    ok: false,
                    action: match action {
                        "open" => ACTION_OPEN,
                        "edit" => ACTION_EDIT,
                        _ => ACTION_READY,
                    },
                    provider: "azure".into(),
                    number,
                    url: None,
                    error: Some("provider-unsupported".into()),
                },
                "{action} must fail honestly",
            );
        }
    }

    /// `edit` and `ready` echo the number the caller pointed at and hand the
    /// port the body verbatim — no re-reading, no rewriting.
    #[test]
    fn edit_and_ready_echo_the_number_and_pass_the_body_through() {
        let fake = FakePub::green("github");
        let edited = edit_report(&fake, 42, "line one\nline two");
        assert_eq!((edited.ok, edited.number, edited.url), (true, Some(42), None));
        let readied = ready_report(&fake, 42);
        assert_eq!((readied.ok, readied.action, readied.number), (true, "ready", Some(42)));
        assert_eq!(
            fake.seen.borrow().as_slice(),
            ["edit 42 body=line one\nline two", "ready 42"],
        );
    }

    /// The title comes out of the body document itself: the first heading
    /// sheds its markers, a plain first line serves as-is, and a body with no
    /// usable line falls back to the head branch so the create never starves.
    #[test]
    fn fill_reads_title_and_body_from_the_commit_range() {
        // A real repo: two commits on a branch off the base.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .expect("spawn git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "dev", "."]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["commit", "-q", "--allow-empty", "-m", "seed"]);
        git(&["checkout", "-q", "-b", "fix/x"]);
        git(&["commit", "-q", "--allow-empty", "-m", "first change"]);
        git(&["commit", "-q", "--allow-empty", "-m", "the newest subject"]);

        let (title, body) = fill_from_commits(root, "dev", "fix/x").expect("range readable");
        assert_eq!(title, "the newest subject", "title = newest commit subject");
        assert!(body.contains("- first change") && body.contains("- the newest subject"));

        // An empty range refuses with the range named, never an empty PR.
        let err = fill_from_commits(root, "fix/x", "fix/x").expect_err("nothing to fill");
        assert!(err.contains("nothing-to-fill"), "{err}");
    }

    #[test]
    fn the_title_is_read_out_of_the_body_file() {
        assert_eq!(title_from_body("# feat: the unit\n\nprose", "feature/x"), "feat: the unit");
        assert_eq!(title_from_body("\n\n## Deep heading\nrest", "feature/x"), "Deep heading");
        assert_eq!(title_from_body("plain first line\nmore", "feature/x"), "plain first line");
        assert_eq!(title_from_body("###\n\n", "feature/x"), "feature/x");
        assert_eq!(title_from_body("", "feature/x"), "feature/x");
    }
}
