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
//! ## Nobody writes the title or the body
//!
//! `pr-open` takes no `--title` and no `--body-file`. The message is BUILT
//! from the spec's own event file — the goal for the title, the recorded
//! summary and what each wave delivered for the body — by
//! [`mustard_core::domain::spec_events::pr_message`], the one place the
//! template and its limits live. A hand-written `pr-body.md` was a fourth file
//! in the spec folder, committed with it, and stale the moment the next round
//! landed; a gate existed only to notice it had gone stale.
//!
//! A repository with no spec of its own — a submodule — has no such file to
//! read, and that is what `--fill` is for: the title and body come from the
//! commits the branch carries.
//!
//! ## An open pull request is EDITED, never opened twice
//!
//! When the head branch already has a pull request, `pr-open` rewrites its
//! body instead of asking the provider to create a second one. That is also
//! how each round refreshes the body: the same call, from the same door.

use std::path::Path;

use serde::Serialize;

use mustard_core::domain::spec_events::pr_message;

use crate::commands::review::pr_door::project_root;
use crate::shared::pr_provider::{provider_for, PrProvider, PrToOpen};
use mustard_core::domain::spec_state::SpecState;

use crate::shared::spec_state::DiskSpecState;

/// The `action` vocabulary — closed, `&'static str` so the report stays
/// byte-stable and no caller re-parses a free string.
const ACTION_OPEN: &str = "open";
const ACTION_EDIT: &str = "edit";
const ACTION_READY: &str = "ready";

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
    /// What the operator should know even though the action HAPPENED — today,
    /// that the spec's criteria have not all passed.
    ///
    /// It used to be a Bash-tool stage that watched for a typed `gh pr create`
    /// and warned there. That only ever reached the person who typed the
    /// provider's command line by hand; the door that really opens the pull
    /// request said nothing. The warning belongs where the action is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl PrPublishReport {
    /// The failure shape every builder degrades to: the action and provider
    /// still named, the reason in the field, `number` echoing what the caller
    /// pointed at (when it pointed at one).
    fn failed(action: &'static str, provider: String, number: Option<u64>, error: String) -> Self {
        Self {
            ok: false,
            action,
            provider,
            number,
            url: None,
            error: Some(error),
            warning: None,
        }
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
            warning: None,
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
            warning: None,
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
            warning: None,
        },
        Err(error) => PrPublishReport::failed(ACTION_READY, name, Some(number), error),
    }
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

/// What the operator must be told even when the pull request DOES open: the
/// spec's criteria have not all passed.
///
/// `None` only when every criterion of the spec has a passing last run — the
/// same reading the close doors refuse on, so the warning and the refusal can
/// never disagree about whether the unit is verified. A spec whose event file
/// cannot be read, or which states no criterion at all, is NOT verified, and
/// saying nothing there would be the one silence that matters.
pub(crate) fn qa_warning(repo: &Path, spec: &str) -> Option<String> {
    let lang = mustard_core::ProjectConfig::load(repo).language().text_or_default();
    let qa = DiskSpecState::new(repo)
        .log(spec)
        .map(|log| mustard_core::domain::spec_state::qa(&log))
        .unwrap_or_default();
    if qa.passed_all() {
        return None;
    }
    Some(
        mustard_core::platform::i18n::translate("pr.qa_pending", lang)
            .replace("{spec}", spec)
            .replace("{passed}", &qa.passed.to_string())
            .replace("{criteria}", &qa.criteria.to_string()),
    )
}

/// The pull request's title and body for `spec`, built from its event file.
///
/// The refusal travels as the stable reason plus the sentence the operator
/// reads, because both halves matter here: prose branches on the reason and a
/// person has to be told WHICH excerpt is the problem.
pub(crate) fn message_of(repo: &Path, spec: &str) -> Result<(String, String), String> {
    let lang = mustard_core::ProjectConfig::load(repo).language().text_or_default();
    let Some(log) = DiskSpecState::new(repo).log(spec) else {
        return Err(format!("spec-events-unreadable: {spec}"));
    };
    pr_message(&log).map_err(|refusal| format!("{}: {}", refusal.reason(), refusal.message(lang)))
}

/// Dispatch `mustard-rt run pr-open`.
///
/// The body comes from the spec's event file, or from the commits with
/// `--fill` (a submodule, which has no spec of its own). A head branch that
/// already carries a pull request has its body REWRITTEN — the door never asks
/// for a second one.
pub fn run_open(root: &Path, base: &str, head: &str, spec: Option<&str>, fill: bool, draft: bool) {
    let repo = project_root(root);
    let provider = provider_for(&repo);
    let sourced = if fill {
        fill_from_commits(&repo, base, head)
    } else {
        match spec.map(str::trim).filter(|s| !s.is_empty()) {
            Some(slug) => message_of(&repo, slug),
            None => Err("spec-missing: pass --spec or --fill".to_string()),
        }
    };
    let warning = spec.map(str::trim).filter(|s| !s.is_empty()).and_then(|slug| qa_warning(&repo, slug));
    let mut report = match sourced {
        Ok((title, body)) => match provider.view(None) {
            // Já existe pull request para esta branch: o corpo é reescrito, e
            // nenhum segundo pull request nasce.
            Ok(view) => edit_report(provider.as_ref(), view.number, &body),
            Err(_) => {
                let pr = PrToOpen {
                    title,
                    body,
                    head: head.to_string(),
                    base: base.to_string(),
                    draft,
                };
                open_report(provider.as_ref(), &pr)
            }
        },
        Err(error) => {
            PrPublishReport::failed(ACTION_OPEN, provider.provider().to_string(), None, error)
        }
    };
    report.warning = warning;
    emit(&report);
}

/// Dispatch `mustard-rt run pr-edit`: rewrite the body of pull request
/// `number` from the spec's event file — the same text `pr-open` builds, so
/// the two can never describe the unit differently.
pub fn run_edit(root: &Path, number: u64, spec: &str) {
    let repo = project_root(root);
    let provider = provider_for(&repo);
    let report = match message_of(&repo, spec) {
        Ok((_, body)) => edit_report(provider.as_ref(), number, &body),
        Err(error) => PrPublishReport::failed(
            ACTION_EDIT,
            provider.provider().to_string(),
            Some(number),
            error,
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

    use crate::shared::pr_provider::{PrChecks, PrOpened, PrView};

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

        fn checks(&self, _number: u64) -> Result<PrChecks, String> {
            // Publishing never asks the provider's checks — the merge door
            // does. Answering an `Err` here keeps that visible: a version that
            // started asking would fail loudly instead of reading a green.
            Err("checks-not-under-test".to_string())
        }

        fn branch_protection(&self, _branch: &str) -> Result<bool, String> {
            // Publicar não pergunta o que o servidor protege — o diagnóstico
            // pergunta. Responder erro aqui mantém isso visível.
            Err("protection-not-under-test".to_string())
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
                warning: None,
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
                    warning: None,
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

    /// O título e o corpo saem do arquivo de eventos da spec, e a recusa do
    /// montador chega inteira a quem chamou: o motivo estável e a frase.
    ///
    /// Uma spec sem arquivo de eventos não abre pull request nenhum — a porta
    /// não tem de onde tirar o texto e diz isso, em vez de inventar um corpo.
    #[test]
    fn a_mensagem_sai_do_arquivo_de_eventos_da_spec() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").expect("cfg");

        let sem_spec = message_of(root, "naoexiste").expect_err("nada a ler");
        assert!(sem_spec.contains("spec-events-unreadable"), "{sem_spec}");

        let path = mustard_core::io::spec_events::spec_file(root, "trava").expect("caminho");
        std::fs::create_dir_all(path.parent().expect("pasta")).expect("pasta da spec");
        let draft = |value: serde_json::Value| {
            value.as_object().cloned().expect("um objeto")
        };
        let said = mustard_core::io::spec_events::write(
            &path,
            "message",
            draft(serde_json::json!({"text": "preciso barrar o comando"})),
            &[],
        )
        .expect("mensagem");
        mustard_core::io::spec_events::write(
            &path,
            "context",
            draft(serde_json::json!({
                "text": "Barrar comando que apaga trabalho. E mais.",
                "origin": said.id,
            })),
            &[],
        )
        .expect("contexto");
        mustard_core::io::spec_events::write(
            &path,
            "pr_summary",
            draft(serde_json::json!({"text": "O portão lê o estado."})),
            &[],
        )
        .expect("resumo");

        let (title, body) = message_of(root, "trava").expect("a spec tem objetivo");
        assert_eq!(title, "Barrar comando que apaga trabalho.");
        assert!(body.contains("O portão lê o estado."), "{body}");
    }
}
