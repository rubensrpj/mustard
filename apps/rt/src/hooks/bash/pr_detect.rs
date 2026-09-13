//! `pr_detect` — DORA telemetry on `gh pr` commands (PostToolUse(Bash)).
//!
//! Classification plus a best-effort `pr.opened` / `pr.merged` harness event.
//! Never affects a verdict. Ported from `pr-detect.js`, since corrected where
//! the port faithfully carried the original's blind spots: a command chain hid
//! the PR verb, and spec attribution read a directory the harness stopped
//! writing.
//!
//! Um `gh pr merge` digitado no terminal também é um merge. Depois de um
//! comando com ele que terminou bem, em qualquer forma, o gancho não tenta
//! descobrir a branch pelo texto do comando (número, endereço, `-R`, `--auto`,
//! `--delete-branch`). Ele faz ao `gh` uma pergunta só, com prazo: os pull
//! requests mergeados mais recentes, com a branch de origem e a hora do merge.
//! A resposta é comparada de uma vez com todas as specs candidatas, e só a
//! spec cuja branch aparece mergeada há pouco ganha, pela ponte, a fase
//! `delivered` e a cobrança das pendências, como no `pr-merge`
//! ([`delivered_specs`]).

use mustard_core::domain::model::contract::HookInput;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::domain::spec_state::{is_approved_phase, SpecState};
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::time::now_iso8601;
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::lex::truncate;
use crate::shared::proc::{run_shell_with_deadline, ShellOutcome};
use crate::shared::spec_state::DiskSpecState;

/// Classify a command as a PR event.
///
/// The predecessor read only the FIRST token of the whole command string, so it
/// saw `gh pr create` alone and nothing else: `git push && gh pr create --fill`,
/// a `cd`-prefixed invocation, or a merge on the second line of a multi-line
/// command all classified to `None` and vanished from the DORA report. Every
/// segment is now classified — the command is split on shell separators exactly
/// as bash reads them (via [`super::lex::is_cmd_separator`], with quoted
/// operators masked first so a `-m "a || b"` message cannot forge a boundary),
/// and each segment is judged on ITS first token.
///
/// Still conservative in the way that matters: the verb must be the segment's
/// leading word, so `echo gh pr create` is not a PR event.
pub(super) fn classify_pr(command: &str) -> Option<&'static str> {
    let masked = super::lex::mask_quoted_operators(command);
    masked
        .split(super::lex::is_cmd_separator)
        .find_map(classify_pr_segment)
}

/// Classify ONE shell segment (no separators inside). A leading `rtk ` wrapper
/// is transparent — the project routes every command through it.
fn classify_pr_segment(segment: &str) -> Option<&'static str> {
    let cleaned = super::lex::strip_leading_rtk(segment.trim()).trim_start();
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.len() >= 3 && tokens[0].eq_ignore_ascii_case("gh") && tokens[1] == "pr" {
        match tokens[2] {
            "create" => return Some("pr.opened"),
            "merge" => return Some("pr.merged"),
            _ => {}
        }
    }
    None
}

/// A pergunta ao `gh`: os pull requests mergeados, dos atualizados por último
/// para os mais velhos, com a branch de origem e a hora do merge.
const MERGED_LIST: &str =
    "gh pr list --state merged --search sort:updated-desc --limit 50 --json headRefName,mergedAt,number";

/// O prazo da pergunta ao `gh`: com a rede lenta, o gancho desiste bem antes
/// dos 30 segundos que o Claude Code lhe dá, e nada é gravado.
const GH_DEADLINE: Duration = Duration::from_secs(8);

/// Quanto antes de agora um merge ainda conta como o do comando que acabou de
/// rodar, em milissegundos. O gancho não sabe a hora em que o comando começou:
/// a janela cobre a duração dele e mais uns minutos, e um merge mais antigo da
/// mesma branch não conta.
const RECENT_MERGE_MS: i64 = 15 * 60 * 1000;

/// Um pull request mergeado, como o `gh` o lista: a branch de origem e a hora
/// do merge, em milissegundos desde 1970.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergedPr {
    pub(crate) head: String,
    pub(crate) merged_ms: i64,
}

/// Roda `command` no repositório de `root` com o prazo `timeout` e lê a lista
/// de pull requests mergeados. `None` quando o prazo estoura, o `gh` falha ou
/// a resposta não se lê.
fn list_merged(root: &Path, command: &str, timeout: Duration) -> Option<Vec<MergedPr>> {
    match run_shell_with_deadline(command, root, timeout) {
        ShellOutcome::Exited { status, stdout, .. } if status.success() => parse_merged(&stdout),
        _ => None,
    }
}

/// A lista do `gh pr list --json headRefName,mergedAt`. Um item sem branch ou
/// sem hora de merge é pulado.
fn parse_merged(stdout: &str) -> Option<Vec<MergedPr>> {
    let listed: Value = serde_json::from_str(stdout.trim()).ok()?;
    Some(
        listed
            .as_array()?
            .iter()
            .filter_map(|pr| {
                Some(MergedPr {
                    head: pr.get("headRefName")?.as_str()?.to_string(),
                    merged_ms: mustard_core::time::parse_iso_millis(pr.get("mergedAt")?.as_str()?)?,
                })
            })
            .collect(),
    )
}

/// Agora, em milissegundos desde 1970.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
}

/// As specs que podem ter acabado de entrar no merge, em ordem de nome, com a
/// branch de cada uma: as da pasta das specs que têm arquivo de eventos, uma
/// branch gravada no estado e uma fase aprovada que ainda não é `delivered`.
/// A fase e a branch saem do estado dobrado de cada uma.
fn candidates(project: &Path) -> Vec<(String, String)> {
    let main = mustard_core::io::spec_events::spec_root(project);
    let Ok(paths) = ClaudePaths::for_project(&main) else {
        return Vec::new();
    };
    let Ok(entries) = mustard_core::io::fs::read_dir(paths.spec_dir()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .into_iter()
        .filter(|entry| entry.path.join("spec.ndjson").is_file())
        .map(|entry| entry.file_name)
        .collect();
    names.sort();
    let disk = DiskSpecState::new(project);
    names
        .into_iter()
        .filter_map(|name| {
            let state = disk.state(&name)?;
            let phase = state.phase?;
            if !is_approved_phase(phase) || phase == "delivered" {
                return None;
            }
            Some((name, state.branch?))
        })
        .collect()
}

/// As specs que um merge acabou de entregar: cada candidata cuja branch
/// aparece na lista de mergeados com a hora do merge dentro da janela antes de
/// `now`. Um `--auto` ainda não mergeou; a promoção de `dev` para `main`
/// mergeia uma base, que não é a branch de spec nenhuma; e um merge antigo da
/// mesma branch não conta.
fn delivered_specs(candidates: &[(String, String)], merged: &[MergedPr], now: i64) -> Vec<String> {
    let since = now.saturating_sub(RECENT_MERGE_MS);
    candidates
        .iter()
        .filter(|(_, branch)| merged.iter().any(|pr| pr.head == *branch && pr.merged_ms >= since))
        .map(|(spec, _)| spec.clone())
        .collect()
}

/// The git branch via `git rev-parse --abbrev-ref HEAD`. Fail-open `None`.
fn detect_branch(project_dir: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

/// The spec this PR belongs to, for the DORA pairing key: the one current-spec
/// ladder every door shares (the environment override, then the checkout's
/// branch, then the session binding). Fail-open `None` — a PR with no
/// resolvable spec still pairs by branch.
pub(crate) fn detect_recent_spec(project_dir: &str, session_id: Option<&str>) -> Option<String> {
    crate::shared::spec_state::active_spec(project_dir, session_id)
}

/// `true` when the Bash tool reported a non-zero exit code. Mirrors the
/// `tool_response.exit_code` check in `pr-detect.js` — permissive: a missing
/// exit code is treated as success.
pub(super) fn bash_failed(input: &HookInput) -> bool {
    input
        .raw
        .get("tool_response")
        .and_then(|r| r.get("exit_code"))
        .and_then(serde_json::Value::as_i64)
        .is_some_and(|code| code != 0)
}

/// Emit a `pr.opened` / `pr.merged` harness event. Best-effort telemetry.
pub(super) fn emit_pr_event(
    project_dir: &str,
    session_id: Option<&str>,
    event: &str,
    command: &str,
) {
    let list = |root: &Path| list_merged(root, MERGED_LIST, GH_DEADLINE);
    emit_pr_event_with(project_dir, session_id, event, command, &list, now_ms());
}

/// [`emit_pr_event`] com a lista de mergeados e o relógio dados por quem
/// chama: os testes passam uma lista pronta e uma hora fixa.
fn emit_pr_event_with(
    project_dir: &str,
    session_id: Option<&str>,
    event: &str,
    command: &str,
    list: &dyn Fn(&Path) -> Option<Vec<MergedPr>>,
    now: i64,
) {
    let branch = detect_branch(project_dir);
    let spec = detect_recent_spec(project_dir, session_id);
    let command_field = if command.len() > 200 {
        format!("{}...", truncate(command, 200))
    } else {
        command.to_string()
    };
    let harness_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id.unwrap_or("unknown").to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("pr-detect".to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload: json!({
            "branch": branch,
            "spec": spec,
            "command": command_field,
        }),
        spec: spec.clone(),
    };
    // `pr.detect` family events go to the per-spec NDJSON sink through the router.
    let _ = crate::shared::events::route::emit(project_dir, &harness_event);
    // A ponte do merge, como no `pr-merge`: a fase `delivered` em cada spec cuja
    // branch o `gh` mostra mergeada há pouco, e a cobrança das pendências
    // armada para a sessão que mergeou. Sem spec candidata, o `gh` nem é
    // perguntado.
    if event != "pr.merged" {
        return;
    }
    let project = Path::new(project_dir);
    let candidates = candidates(project);
    if candidates.is_empty() {
        return;
    }
    let Some(merged) = list(project) else {
        return;
    };
    for delivered in delivered_specs(&candidates, &merged, now) {
        let _ = crate::commands::spec_events::write::record_phase_by(project, &delivered, "delivered", session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// `gh pr create` / `gh pr merge` classify to the right DORA events.
    #[test]
    fn pr_detect_classifies_pr_commands() {
        assert_eq!(classify_pr("gh pr create --fill"), Some("pr.opened"));
        assert_eq!(classify_pr("gh pr merge 42 --squash"), Some("pr.merged"));
        // Tolerates a leading `rtk` wrapper.
        assert_eq!(classify_pr("rtk gh pr create"), Some("pr.opened"));
    }

    /// A non-PR command classifies to nothing.
    #[test]
    fn pr_detect_ignores_non_pr_commands() {
        assert_eq!(classify_pr("gh pr view 42"), None);
        assert_eq!(classify_pr("git commit -m x"), None);
        assert_eq!(classify_pr("gh issue list"), None);
        assert_eq!(classify_pr("echo gh pr create"), None);
    }

    /// A PR command CHAINED after another command is still a PR event. The
    /// first-token-only reader saw none of these, which is how a report can
    /// under-count what opened and show nothing merged over a period that had
    /// merges — every `gh pr` issued as part of a chain was invisible.
    #[test]
    fn pr_detect_sees_through_command_chains() {
        assert_eq!(
            classify_pr("git push -u origin dev && gh pr create --fill"),
            Some("pr.opened"),
        );
        assert_eq!(classify_pr("cd apps/rt; gh pr merge 103 --squash"), Some("pr.merged"));
        // Multi-line commands: bash treats the newline like `;`, so must we.
        assert_eq!(
            classify_pr("echo opening\nrtk gh pr create --fill --base main"),
            Some("pr.opened"),
        );
        // A quoted operator inside a commit message is not a segment boundary,
        // and the quoted text is not a command.
        assert_eq!(classify_pr("git commit -m \"gh pr create || nope\""), None);
        // The verb must still LEAD its own segment — a mention is not an event.
        assert_eq!(classify_pr("echo run && echo gh pr create"), None);
    }

    /// A hora fixa dos testes.
    fn now() -> i64 {
        mustard_core::time::parse_iso_millis("2026-09-13T12:00:00Z").expect("a fixed now")
    }

    /// Um pull request da branch `head` mergeado `ago` milissegundos antes de
    /// [`now`].
    fn merged(head: &str, ago: i64) -> MergedPr {
        MergedPr { head: head.to_string(), merged_ms: now() - ago }
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git").args(args).current_dir(dir).output().map(|o| o.status.success()).unwrap_or(false);
        assert!(ok, "git {args:?} failed in {}", dir.display());
    }

    /// Grava na spec `spec` o estado `fields`, criando a pasta dela.
    fn state(root: &Path, spec: &str, fields: Value) {
        let path = mustard_core::io::spec_events::spec_file(root, spec).expect("spec file");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("spec folder");
        mustard_core::io::spec_events::write(&path, "state", fields.as_object().cloned().expect("object"), &[])
            .expect("state");
    }

    /// Um projeto do fluxo `dev`/`main`, parado em `branch`, com a spec `trava`
    /// em andamento na branch `feature/trava`, uma pendência nascida nela e a
    /// sessão `s-pr` ligada a ela; e a spec `plano`, ainda em plano, com a
    /// branch `feature/plano`.
    fn project_on(branch: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).expect("cfg");
        git(root, &["init", "-q"]);
        git(root, &["checkout", "-q", "-b", branch]);
        git(root, &["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "root"]);
        let added = crate::commands::event::pending::pending_at(&crate::commands::event::pending::PendingOpts {
            root: root.to_path_buf(),
            add: true,
            title: Some("Humanize".into()),
            detail: Some("nasceu na spec".into()),
            ..crate::commands::event::pending::PendingOpts::default()
        });
        assert_eq!(added["ok"], json!(true), "{added}");
        crate::hooks::task::pending_gate::seed_spec(root, "trava", &[1], "s-pr");
        state(root, "trava", json!({ "phase": "running", "branch": "feature/trava" }));
        state(root, "plano", json!({ "phase": "plan", "branch": "feature/plano" }));
        dir
    }

    fn phase(root: &Path, spec: &str) -> Option<&'static str> {
        DiskSpecState::new(root).state(spec).and_then(|state| state.phase)
    }

    /// O merge digitado confere o fato na lista de mergeados, pela branch da
    /// spec, e não o texto do comando. Pelo número, pelo endereço e com
    /// `--delete-branch` sem nome, parado já na base, grava `delivered` e arma
    /// a cobrança para a sessão; com `--auto`, o pull request ainda não
    /// mergeou, e nada é gravado; na promoção de `dev` para `main`, a branch
    /// mergeada é uma base, e nada é gravado. O `gh` é perguntado uma vez só.
    #[test]
    fn a_typed_merge_is_checked_against_the_merged_pull_requests() {
        let recent = || vec![merged("feature/trava", 60_000)];
        let cases: [(&str, &str, Vec<MergedPr>, &str); 5] = [
            ("gh pr merge 42 --merge", "feature/trava", recent(), "delivered"),
            ("gh pr merge https://github.com/o/r/pull/42 --squash", "feature/trava", recent(), "delivered"),
            ("gh pr merge --auto --merge", "feature/trava", vec![], "running"),
            ("gh pr merge --merge --delete-branch", "dev", recent(), "delivered"),
            ("gh pr merge 50 --merge", "dev", vec![merged("dev", 30_000)], "running"),
        ];
        for (command, checkout, answer, expected) in cases {
            let dir = project_on(checkout);
            let root = dir.path();
            let calls = Cell::new(0);
            let list = |_: &Path| {
                calls.set(calls.get() + 1);
                Some(answer.clone())
            };
            emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.merged", command, &list, now());
            assert_eq!(phase(root, "trava"), Some(expected), "{command} on {checkout}");
            assert_eq!(phase(root, "plano"), Some("plan"), "{command}: a spec in plan is never delivered");
            assert_eq!(calls.get(), 1, "{command}: one call to gh");
            let armed = crate::commands::event::pending::armed_charges(root);
            if expected == "delivered" {
                assert_eq!(armed.len(), 1, "{command}: the charge is armed: {armed:?}");
                assert_eq!(armed[0].session.as_deref(), Some("s-pr"), "{command}: for the session that merged");
            } else {
                assert!(armed.is_empty(), "{command}: nothing is armed: {armed:?}");
            }
        }
    }

    /// Um merge antigo da branch da spec não conta: a spec cuja onda foi
    /// mergeada antes não vira `delivered` no merge de outro pull request.
    #[test]
    fn an_old_merge_of_the_spec_branch_does_not_count() {
        let dir = project_on("dev");
        let root = dir.path();
        let two_days = 2 * 24 * 60 * 60 * 1000;
        let list = |_: &Path| Some(vec![merged("feature/trava", two_days), merged("feature/outra", 60_000)]);
        emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.merged", "gh pr merge 60", &list, now());
        assert_eq!(phase(root, "trava"), Some("running"), "an old merge of the spec branch is not this one");
    }

    /// Com o `gh` lento, a pergunta estoura o prazo e nada é gravado.
    #[test]
    fn a_slow_gh_runs_out_of_time_and_records_nothing() {
        let dir = project_on("feature/trava");
        let root = dir.path();
        let slow = if cfg!(windows) { "ping -n 6 127.0.0.1" } else { "sleep 5" };
        let started = std::time::Instant::now();
        let list = |root: &Path| list_merged(root, slow, Duration::from_millis(300));
        emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.merged", "gh pr merge 42", &list, now());
        assert!(started.elapsed() < Duration::from_secs(4), "the deadline cut it: {:?}", started.elapsed());
        assert_eq!(phase(root, "trava"), Some("running"), "a gh that did not answer records nothing");
        assert!(crate::commands::event::pending::armed_charges(root).is_empty());
    }

    /// Com mais de cinco specs candidatas, todas são comparadas: a mergeada é
    /// achada mesmo sendo a primeira de sete.
    #[test]
    fn more_than_five_candidates_are_all_checked() {
        let dir = project_on("dev");
        let root = dir.path();
        for n in 0..7 {
            state(root, &format!("spec-{n}"), json!({ "phase": "running", "branch": format!("feature/spec-{n}") }));
        }
        let list = |_: &Path| Some(vec![merged("feature/spec-0", 60_000)]);
        emit_pr_event_with(&root.to_string_lossy(), None, "pr.merged", "gh pr merge 70", &list, now());
        assert_eq!(phase(root, "spec-0"), Some("delivered"), "the merged one is found among seven");
        for n in 1..7 {
            assert_eq!(phase(root, &format!("spec-{n}")), Some("running"), "spec-{n} was not merged");
        }
    }

    /// A lista do `gh` se lê pela branch de origem e pela hora do merge; um
    /// item sem uma das duas é pulado.
    #[test]
    fn the_merged_list_reads_the_branch_and_the_merge_time() {
        let listed = r#"[
            {"headRefName":"feature/trava","mergedAt":"2026-09-13T11:59:00Z","number":7},
            {"headRefName":"feature/sem-hora","number":8},
            {"mergedAt":"2026-09-13T11:00:00Z","number":9}
        ]"#;
        assert_eq!(parse_merged(listed), Some(vec![merged("feature/trava", 60_000)]));
        assert_eq!(parse_merged("não é json"), None);
    }

    /// Uma spec já entregue não é candidata de novo, e sem candidata o `gh`
    /// nem é perguntado; um `gh pr create` não pergunta nada.
    #[test]
    fn a_delivered_spec_is_not_asked_again_and_an_opened_pr_asks_nothing() {
        let dir = project_on("feature/trava");
        let root = dir.path();
        let calls = Cell::new(0);
        let list = |_: &Path| {
            calls.set(calls.get() + 1);
            Some(vec![merged("feature/trava", 60_000)])
        };
        emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.opened", "gh pr create --fill", &list, now());
        assert_eq!(calls.get(), 0, "opening a pull request asks nothing");
        emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.merged", "gh pr merge 42", &list, now());
        assert_eq!(phase(root, "trava"), Some("delivered"));
        emit_pr_event_with(&root.to_string_lossy(), Some("s-pr"), "pr.merged", "gh pr merge 43", &list, now());
        assert_eq!(calls.get(), 1, "with no candidate left, gh is not asked again");
    }
}
