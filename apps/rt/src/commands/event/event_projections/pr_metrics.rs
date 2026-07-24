//! `pr-metrics` DORA projection. Extracted from `event_projections` (F3 PERF-D split).

use mustard_core::domain::model::event::HarnessEvent;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

/// Newest merge commits inspected when reading merges from git. A DORA window is
/// at most a few months; a bound keeps the read cheap on an old repository.
const GIT_MERGE_SCAN_LIMIT: &str = "500";

/// One dated moment in a pull request's life, normalised away from its source so
/// the pairing math never has to know whether it came from the harness event log
/// or from git history.
#[derive(Clone)]
struct Moment {
    /// ISO-8601 UTC timestamp — lexicographically comparable, the same shape
    /// `parse_iso_millis` consumes.
    ts: String,
    /// Every name this moment can be paired on — the spec AND the branch, not
    /// one in preference to the other. Two moments pair when their sets
    /// intersect, so an opening event recorded under a spec still meets the
    /// merge commit, which can only ever name a branch.
    keys: BTreeSet<String>,
    /// `linesChanged`, when the source carried it (opening events only).
    lines_changed: Option<i64>,
}

/// The names an event can be paired on: its payload spec and its payload branch.
///
/// The predecessor read `spec` and fell back to `branch` with `or_else` — but
/// `pr-detect` writes `"spec": null` when it cannot resolve one, and
/// `Value::get` answers `Some(Null)` for a present-but-null key, so the fallback
/// never fired and EVERY real `pr.opened` event paired on nothing. Nulls are
/// filtered here, and both names are kept rather than ranked.
fn pair_keys(ev: &HarnessEvent) -> BTreeSet<String> {
    ["spec", "branch"]
        .iter()
        .filter_map(|field| ev.payload.get(*field).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Render a UNIX timestamp as the canonical seconds-precision ISO sentinel this
/// projection compares on. Mirrors the same conversion in
/// [`super::pipeline::build_active_pipelines`] — the calendar math itself lives
/// in `mustard_core::time`, never here.
fn iso_from_unix_secs(secs: i64) -> String {
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = mustard_core::time::civil_from_days(secs.div_euclid(86_400));
    let (h, mi, s) = (tod / 3_600, (tod % 3_600) / 60, tod % 60);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// The head branch a merge commit closed, read from its subject line.
///
/// Recognises the two subjects that carry one: GitHub's
/// `Merge pull request #N from <owner>/<branch>` and git's own
/// `Merge branch '<branch>'`. A hand-written merge subject names no branch and
/// yields `None` — the merge still COUNTS, it just cannot be paired to an
/// opening event, which is strictly better than dropping it.
fn merged_branch(subject: &str) -> Option<String> {
    if let Some(rest) = subject.split(" from ").nth(1) {
        let head = rest.split_whitespace().next().unwrap_or("");
        // `owner/branch` → `branch`; a bare `branch` is taken as-is.
        let branch = head.split_once('/').map_or(head, |(_, b)| b);
        if !branch.is_empty() {
            return Some(branch.to_string());
        }
    }
    let (_, after) = subject.split_once('\'')?;
    let (branch, _) = after.split_once('\'')?;
    (!branch.is_empty()).then(|| branch.to_string())
}

/// Merges as GIT recorded them, newest-first, or `None` when git cannot answer.
///
/// `pr.merged` is emitted by a PostToolUse(Bash) observer, so it can only ever
/// witness a merge **typed into this terminal**. Merges performed with GitHub's
/// merge button — the normal way this project integrates — leave no event at
/// all, and the report published `merged: 0` for a window holding nine of them.
/// Git holds every one of them locally, in the merge commits themselves, so it
/// is the source that actually knows; the event log remains the fallback for a
/// workspace with no git (or none of its own merges yet).
///
/// `%ct` (UNIX seconds) is read rather than `%cI` so no timezone offset ever has
/// to be parsed — the timestamp is rendered into the canonical UTC shape here.
fn merges_from_git(cwd: &Path) -> Option<Vec<Moment>> {
    let output = Command::new("git")
        .args([
            "-C",
            cwd.to_str()?,
            "log",
            "--merges",
            "--max-count",
            GIT_MERGE_SCAN_LIMIT,
            "--format=%ct%x1f%s",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        // Not a repository, or git is unavailable — "cannot look", so the
        // caller falls back rather than publishing an empty answer as a fact.
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.lines()
            .filter_map(|line| {
                let (secs, subject) = line.split_once('\u{1f}')?;
                let secs: i64 = secs.trim().parse().ok()?;
                Some(Moment {
                    ts: iso_from_unix_secs(secs),
                    keys: merged_branch(subject).into_iter().collect(),
                    lines_changed: None,
                })
            })
            .collect(),
    )
}

/// `buildPRMetrics` — DORA-style metrics from `pr.opened` / `review.start` /
/// `review.complete` events within the last `days`, plus the merges git itself
/// recorded (see [`merges_from_git`]).
pub(super) fn build_pr_metrics(events: &[HarnessEvent], cwd: &Path, days: i64, now_ms: i64) -> Value {
    // `now_ms` is injected (not read from the wall clock here) so the window is
    // a pure function of the inputs — deterministically testable. The production
    // caller passes `now_unix_millis()`.
    let from_ms = now_ms - days * 86_400_000;
    let in_window = |ts: &str| -> bool {
        mustard_core::time::parse_iso_millis(ts)
            .is_some_and(|t| t >= from_ms && t <= now_ms)
    };

    let (mut opened, mut merged_events, mut review_start, mut review_complete): (
        Vec<Moment>,
        Vec<Moment>,
        Vec<Moment>,
        Vec<Moment>,
    ) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for ev in events {
        if ev.ts.is_empty() || !in_window(&ev.ts) {
            continue;
        }
        let moment = Moment {
            ts: ev.ts.clone(),
            keys: pair_keys(ev),
            lines_changed: ev.payload.get("linesChanged").and_then(Value::as_i64),
        };
        match ev.event.as_str() {
            "pr.opened" => opened.push(moment),
            "pr.merged" => merged_events.push(moment),
            "review.start" => review_start.push(moment),
            "review.complete" => review_complete.push(moment),
            _ => {}
        }
    }

    // Merges: git when it can answer, the event log otherwise. `mergedSource`
    // publishes which one spoke, so a reader can tell a terminal-only count from
    // the repository's real integration history.
    let (mut merged, merged_source) = match merges_from_git(cwd) {
        Some(from_git) => (
            from_git.into_iter().filter(|m| in_window(&m.ts)).collect::<Vec<_>>(),
            "git",
        ),
        None => (merged_events, "events"),
    };

    // Pair opened → merged (earliest opener first; one merge per opener).
    let pair_durations = |starts: &mut Vec<Moment>, ends: &[Moment]| -> Vec<i64> {
        starts.sort_by(|a, b| a.ts.cmp(&b.ts));
        let mut sorted_ends: Vec<Moment> = ends.to_vec();
        sorted_ends.sort_by(|a, b| a.ts.cmp(&b.ts));
        let mut used = vec![false; sorted_ends.len()];
        let mut durations = Vec::new();
        for s in starts.iter() {
            if s.keys.is_empty() {
                continue;
            }
            let Some(s_ms) = mustard_core::time::parse_iso_millis(&s.ts) else {
                continue;
            };
            for (i, e) in sorted_ends.iter().enumerate() {
                if used[i] {
                    continue;
                }
                let Some(e_ms) = mustard_core::time::parse_iso_millis(&e.ts) else {
                    continue;
                };
                // Any shared name pairs them — the opener may be keyed by spec,
                // the merge commit only ever by branch.
                if e_ms < s_ms || s.keys.is_disjoint(&e.keys) {
                    continue;
                }
                durations.push(e_ms - s_ms);
                used[i] = true;
                break;
            }
        }
        durations
    };
    merged.sort_by(|a, b| a.ts.cmp(&b.ts));
    let lead_times = pair_durations(&mut opened, &merged);
    let review_times = pair_durations(&mut review_start, &review_complete);
    let sizes: Vec<i64> = opened
        .iter()
        .filter_map(|m| m.lines_changed)
        .filter(|n| *n > 0)
        .collect();

    let stat = |arr: &[i64]| -> Value {
        if arr.is_empty() {
            return json!({ "count": 0, "p50": Value::Null, "p90": Value::Null, "max": Value::Null });
        }
        let mut sorted = arr.to_vec();
        sorted.sort_unstable();
        let pct = |p: usize| -> i64 {
            let idx = ((p as f64 / 100.0) * sorted.len() as f64).floor() as usize;
            sorted[idx.min(sorted.len() - 1)]
        };
        json!({
            "count": sorted.len(),
            "p50": pct(50),
            "p90": pct(90),
            "max": *sorted.last().unwrap_or(&0),
        })
    };
    let bucket_by_day = |arr: &[Moment]| -> Value {
        let mut map: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for m in arr {
            let day: String = m.ts.chars().take(10).collect();
            if !day.is_empty() {
                *map.entry(day).or_insert(0) += 1;
            }
        }
        Value::Array(
            map.into_iter()
                .map(|(date, count)| json!({ "date": date, "count": count }))
                .collect(),
        )
    };

    json!({
        "window": { "days": days },
        "mergedSource": merged_source,
        "totals": {
            "opened": opened.len(),
            "merged": merged.len(),
            "reviewsStarted": review_start.len(),
            "reviewsCompleted": review_complete.len(),
        },
        "leadTimeMs": stat(&lead_times),
        "reviewTimeMs": stat(&review_times),
        "prSize": stat(&sizes),
        "openedByDay": bucket_by_day(&opened),
        "mergedByDay": bucket_by_day(&merged),
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};

    fn ev(event: &str, spec: Option<&str>, payload: Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-19T00:00:00.000Z".to_string(),
            session_id: "s1".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: event.to_string(),
            payload,
            spec: spec.map(str::to_string),
        }
    }

    #[test]
    fn pr_metrics_pairs_lead_time() {
        let events = vec![
            ev("pr.opened", None, json!({ "spec": "auth", "linesChanged": 40 })),
            {
                let mut e = ev("pr.merged", None, json!({ "spec": "auth" }));
                e.ts = "2026-05-19T01:00:00.000Z".to_string();
                e
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        // Deterministic window: anchor "now" just after the fixtures so the
        // opened (2026-05-19T00:00Z, the `ev` default) and merged
        // (2026-05-19T01:00Z) events both fall inside the 30-day window — no
        // dependence on the wall clock. This test once rotted silently: the
        // projection read the real clock, so once it passed 2026-05-19 + 30d the
        // hardcoded fixtures fell out of the window and `opened` dropped to 0.
        let now_ms = mustard_core::time::parse_iso_millis("2026-05-19T02:00:00.000Z").unwrap();
        let m = build_pr_metrics(&events, dir.path(), 30, now_ms);
        assert_eq!(m["totals"]["opened"], json!(1));
        assert_eq!(m["totals"]["merged"], json!(1));
        assert_eq!(m["leadTimeMs"]["count"], json!(1));
        assert_eq!(m["prSize"]["count"], json!(1));
        // A tempdir is no git repository, so the merge source degrades to the
        // event log rather than reporting an empty git answer as a fact.
        assert_eq!(m["mergedSource"], json!("events"));
    }

    /// `pr-detect` writes `"spec": null` when it cannot resolve one, and
    /// `Value::get` answers `Some(Null)` for a present-but-null key — so the old
    /// `get("spec").or_else(get("branch"))` never reached the branch and every
    /// real opening event paired on nothing. Both names are now kept.
    #[test]
    fn pair_keys_survive_a_null_spec_and_keep_both_names() {
        // The exact payload shape on disk: a null spec beside a real branch.
        let null_spec = ev("pr.opened", None, json!({ "branch": "dev_x", "spec": null }));
        let keys = pair_keys(&null_spec);
        assert!(
            keys.contains("dev_x"),
            "a null spec must not swallow the branch: {keys:?}"
        );
        assert_eq!(keys.len(), 1, "null is not a name: {keys:?}");

        // With both present, NEITHER is dropped: the opener has to meet a merge
        // commit, which can only ever name the branch.
        let both = ev("pr.opened", None, json!({ "branch": "dev_x", "spec": "feature-x" }));
        let keys = pair_keys(&both);
        assert!(keys.contains("dev_x") && keys.contains("feature-x"), "{keys:?}");
    }

    /// End to end: an opener recorded under a null spec pairs with the merge git
    /// recorded, and the lead time is real. This is the pairing that silently
    /// produced `leadTimeMs.count: 0` on a repository full of merged PRs.
    #[test]
    fn an_opener_pairs_with_the_merge_git_recorded() {
        let opened = {
            let mut e = ev("pr.opened", None, json!({ "branch": "feature", "spec": null }));
            e.ts = "2026-05-19T00:00:00.000Z".to_string();
            e
        };
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        if !seed_repo_with_merge(repo) {
            return;
        }
        let now_ms = mustard_core::time::now_unix_millis() as u128 as i64;
        let m = build_pr_metrics(&[opened], repo, 3650, now_ms);
        assert_eq!(m["mergedSource"], json!("git"), "{m}");
        assert_eq!(
            m["leadTimeMs"]["count"],
            json!(1),
            "the opener must pair with the merge on the shared branch name: {m}"
        );
    }

    /// The subject line is the only place a merge commit names the branch it
    /// closed — both the GitHub form and git's own.
    #[test]
    fn merged_branch_reads_both_subject_forms() {
        assert_eq!(
            merged_branch("Merge pull request #103 from acme/dev_fase-1").as_deref(),
            Some("dev_fase-1"),
        );
        // A head with no owner prefix is taken whole.
        assert_eq!(
            merged_branch("Merge pull request #7 from hotfix-login").as_deref(),
            Some("hotfix-login"),
        );
        assert_eq!(merged_branch("Merge branch 'dev'").as_deref(), Some("dev"));
        // A hand-written merge names no branch: unpairable, but still counted.
        assert!(merged_branch("merge: bring the fix under Pilar 1a").is_none());
    }

    /// Git's merge history is the source of truth for what merged: a merge made
    /// with GitHub's merge button emits no `pr.merged` event at all, and the old
    /// projection published `0` for a window full of them.
    #[test]
    fn merges_come_from_git_history_not_only_from_typed_commands() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        if !seed_repo_with_merge(repo) {
            // No usable git in this environment — the fallback path is already
            // covered by `pr_metrics_pairs_lead_time`.
            return;
        }
        // NO `pr.merged` event exists: the merge was never typed in a terminal.
        let events: Vec<HarnessEvent> = Vec::new();
        let now_ms = mustard_core::time::now_unix_millis() as u128 as i64;
        let m = build_pr_metrics(&events, repo, 30, now_ms);
        assert_eq!(m["mergedSource"], json!("git"), "{m}");
        assert_eq!(
            m["totals"]["merged"],
            json!(1),
            "the merge git recorded must be counted: {m}"
        );
        assert_eq!(
            m["mergedByDay"].as_array().map(Vec::len),
            Some(1),
            "and bucketed by its real date: {m}"
        );
    }

    /// Build a throwaway repository holding exactly one merge commit whose
    /// subject carries a branch.
    ///
    /// `false` ONLY when git cannot create a repository at all (no git in the
    /// environment) — the one condition that legitimately skips the test. Every
    /// later step asserts, so a half-built fixture fails loudly instead of
    /// letting the test pass without exercising anything.
    fn seed_repo_with_merge(repo: &std::path::Path) -> bool {
        let git = |args: &[&str]| -> bool {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        let write = |name: &str, body: &str| {
            std::fs::write(repo.join(name), body).unwrap();
        };
        if !git(&["init", "--initial-branch=main"]) {
            return false;
        }
        // Identity is set locally so the fixture never depends on a global config.
        assert!(git(&["config", "user.email", "test@example.invalid"]), "git config email");
        assert!(git(&["config", "user.name", "Test"]), "git config name");
        write("a.txt", "a");
        assert!(git(&["add", "-A"]) && git(&["commit", "-m", "base"]), "base commit");
        assert!(git(&["checkout", "-b", "feature"]), "branch");
        write("b.txt", "b");
        assert!(git(&["add", "-A"]) && git(&["commit", "-m", "work"]), "work commit");
        assert!(git(&["checkout", "main"]), "back to main");
        // `--no-ff` forces a real merge commit; the subject mimics GitHub's.
        assert!(
            git(&[
                "merge",
                "--no-ff",
                "feature",
                "-m",
                "Merge pull request #1 from acme/feature",
            ]),
            "merge commit",
        );
        true
    }
}
