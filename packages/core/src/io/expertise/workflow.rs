//! Task-scoped preparation, reused by planning, dispatch and the panel. A
//! classification can choose a current procedure, never grant authority.
use super::{Profile, active, digest, export, list, matches, review_fingerprint};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub profiles: Vec<String>,
    pub preferred_agent: Option<String>,
    pub reason: String,
    pub fingerprint: String,
    pub pending_review: Vec<String>,
    pub uncovered_files: Vec<String>,
    pub source: String,
}

fn coverage(profile: &Profile, files: &[String]) -> usize {
    files
        .iter()
        .filter(|file| matches(profile, &[(*file).clone()]))
        .count()
}

/// A broad navigation recipe cannot displace a reviewed project procedure.
/// Only equally covering authored candidates need a semantic comparison.
pub fn candidates(root: &Path, files: &[String]) -> Vec<Profile> {
    let mut candidates: Vec<_> = list(root)
        .unwrap_or_default()
        .into_iter()
        .filter(|profile| active(root, profile) && matches(profile, files))
        .collect();
    candidates.sort_by(|a, b| {
        (b.origin == "authored")
            .cmp(&(a.origin == "authored"))
            .then_with(|| coverage(b, files).cmp(&coverage(a, files)))
            .then_with(|| a.paths.len().cmp(&b.paths.len()))
            .then_with(|| a.name.cmp(&b.name))
    });
    candidates
}

pub fn ambiguous(root: &Path, files: &[String]) -> Vec<Profile> {
    let candidates = candidates(root, files);
    let Some(first) = candidates.first().filter(|p| p.origin == "authored") else {
        return Vec::new();
    };
    let count = coverage(first, files);
    let mut procedures = BTreeSet::new();
    let peers: Vec<_> = candidates
        .into_iter()
        .filter(|p| p.origin == "authored" && coverage(p, files) == count)
        .filter(|p| {
            let mut paths = p.paths.clone();
            paths.sort();
            paths.dedup();
            procedures.insert((
                p.procedure.split_whitespace().collect::<Vec<_>>().join(" "),
                paths,
            ))
        })
        .take(4)
        .collect();
    if peers.len() > 1 { peers } else { Vec::new() }
}

pub fn select(root: &Path, files: &[String], preferred: Option<&str>, source: &str) -> Selection {
    let mut available = candidates(root, files);
    let accepted = preferred.and_then(|name| available.iter().position(|p| p.name == name));
    if let Some(at) = accepted {
        let first = available.remove(at);
        available.insert(0, first);
    }
    let mut covered = BTreeSet::new();
    let selected: Vec<_> = available
        .into_iter()
        .filter(|profile| {
            let adds: Vec<_> = files
                .iter()
                .filter(|file| !covered.contains(*file) && matches(profile, &[(*file).clone()]))
                .cloned()
                .collect();
            covered.extend(adds.iter().cloned());
            !adds.is_empty()
        })
        .collect();
    let pending_review = list(root)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.origin == "authored" && matches(p, files) && !active(root, p))
        .map(|p| p.name)
        .collect();
    let reason = if selected.is_empty() {
        "generic executor; no current matching specialization"
    } else if accepted.is_some() && source.starts_with("jev") {
        "typed choice among current, equally covering procedures; native gates preserved"
    } else {
        "reuse current procedures first, then navigation for uncovered task files"
    };
    let fingerprint = digest(
        &serde_json::to_vec(&(
            files,
            selected
                .iter()
                .map(|p| review_fingerprint(root, p))
                .collect::<Vec<_>>(),
        ))
        .unwrap_or_default(),
    );
    Selection {
        preferred_agent: selected.first().map(|p| format!("mustard-{}", p.name)),
        profiles: selected.iter().map(|p| p.name.clone()).collect(),
        reason: reason.into(),
        fingerprint,
        pending_review,
        uncovered_files: files
            .iter()
            .filter(|file| !covered.contains(*file))
            .cloned()
            .collect(),
        source: if accepted.is_some() {
            source.into()
        } else {
            "native".into()
        },
    }
}

/// Export only the selected assets. Manual files are preserved by the export
/// ledger. File creation is not proof that a live host discovered an agent.
pub fn prepare(
    root: &Path,
    files: &[String],
    host: &str,
    preferred: Option<&str>,
    source: &str,
) -> Result<Value, String> {
    if !["claude", "codex"].contains(&host) {
        return Err("expertise-unknown-host".into());
    }
    let selection = select(root, files, preferred, source);
    let exports = selection
        .profiles
        .iter()
        .map(|name| export(root, name, host))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(
        json!({"ok":true,"selection":selection,"exports":exports,"model_calls":0,
        "executor_fallback":"mustard-wave",
        "next":"Use preferred_agent only if listed by the current host; otherwise use mustard-wave with these skills. Review pending drafts by exact fingerprint. Enrich only a missing or changed procedure, in the current isolated session; never launch another model process."}),
    )
}

/// Suggest preparation for an indexed area without a reviewed procedure.
/// File count is only a cheap candidate filter, never authority to add model
/// work or block dispatch. The conductor evaluates whether the native evidence
/// already suffices and whether a reusable isolated procedure adds value.
pub fn dispatch_preparation(root: &Path, files: &[String], query: &str) -> Option<Value> {
    if files.len() < 2 || !crate::io::project_map::model_path(root).is_file() {
        return None;
    }
    let current = candidates(root, files);
    if current.is_empty() || current.iter().any(|p| p.origin == "authored") {
        return None;
    }
    // Respect an explicitly maintained project skill. It is reviewed by the
    // existing wave skill/rule path rather than replaced by a generated one.
    if crate::io::wave_prompt::has_scoped_skill(root, files) {
        return None;
    }
    let query = query.chars().take(1400).collect::<String>();
    Some(
        json!({"query":query,"files":files,"required":false,"model_calls":0,
        "pending_review":select(root,files,None,"native").pending_review,
        "claude_args":["run","expertise","--prepare","--host","claude","--query",query],
        "codex_args":["run","expertise","--prepare","--host","codex","--query",query],
        "next":"Optional: use the current host's arguments only if source-based native guidance is insufficient and a reusable procedure is needed. File count alone does not justify a model call. Reuse a current draft or analysis before delegating. Complete a bounded native-session receipt and review its exact fingerprint before using a new procedure. Native navigation remains available. Do not launch another CLI/API."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::expertise::{record, review};
    #[test]
    fn current_procedures_win_and_changed_rules_withdraw_them() {
        let (dir, mut p) = crate::io::expertise::tests::fixture();
        let root = dir.path();
        let draft = record(root, &p).unwrap();
        review(root, &p.name, draft["fingerprint"].as_str().unwrap()).unwrap();
        let files = vec!["store.rs".into()];
        assert_eq!(
            select(root, &files, None, "native").profiles,
            vec![p.name.clone()]
        );
        p.name = "duplicate-database".into();
        p.procedure = format!(" {}\n", p.procedure);
        let duplicate = record(root, &p).unwrap();
        review(root, &p.name, duplicate["fingerprint"].as_str().unwrap()).unwrap();
        assert!(ambiguous(root, &files).is_empty());
        p.name = "another-database".into();
        p.procedure.push_str(" Check transactions.");
        let draft = record(root, &p).unwrap();
        review(root, &p.name, draft["fingerprint"].as_str().unwrap()).unwrap();
        assert_eq!(ambiguous(root, &files).len(), 2);
        let choice = select(root, &files, Some(&p.name), "jev");
        assert_eq!(choice.profiles, vec![p.name.clone()]);
        assert_eq!(choice.source, "jev");
        std::fs::write(root.join("AGENTS.md"), "Require project schemas.\n").unwrap();
        let stale = select(root, &files, Some(&p.name), "jev");
        assert!(stale.profiles.is_empty());
        assert_eq!(stale.pending_review.len(), 3);
        assert_eq!(stale.uncovered_files, files);
        assert_eq!(stale.source, "native");
    }
    #[test]
    fn unknown_selection_never_activates_a_profile_and_exports_are_reused() {
        let (dir, p) = crate::io::expertise::tests::fixture();
        let root = dir.path();
        let draft = record(root, &p).unwrap();
        review(root, &p.name, draft["fingerprint"].as_str().unwrap()).unwrap();
        let files = vec!["store.rs".into()];
        let first = prepare(root, &files, "codex", Some("forged"), "jev").unwrap();
        assert_eq!(first["selection"]["source"], "native");
        let second = prepare(root, &files, "codex", None, "native").unwrap();
        assert_eq!(second["exports"][0]["changed"], 0);
        assert_eq!(
            first["selection"]["fingerprint"],
            second["selection"]["fingerprint"]
        );
        assert!(prepare(root, &files, "unknown", None, "native").is_err());
    }

    #[test]
    fn multi_file_navigation_suggests_preparation_without_forcing_a_model_or_blocking_dispatch() {
        let (dir, mut p) = crate::io::expertise::tests::fixture();
        let root = dir.path();
        p.paths = vec![
            "store.rs".into(),
            "area0/storage.service.rs".into(),
            "area1/storage.service.rs".into(),
        ];
        p.reviewed = None;
        // Native profiles are normally produced by sync; use that source path.
        let text = "fn read() {}\n";
        let mut modules = Vec::new();
        for n in 0..5 {
            let folder = format!("area{n}");
            std::fs::create_dir_all(root.join(&folder)).unwrap();
            let file = format!("{folder}/storage.service.rs");
            std::fs::write(root.join(&file), text).unwrap();
            modules.push(json!({"path":file,"analysis":{"content_sha256":digest(text.as_bytes())},"declarations":[{"name":"read","kind":"function","line":1,"end_line":1}]}));
        }
        let mut map = json!({"modules":modules});
        crate::domain::knowledge::enrich(&mut map);
        crate::io::project_map::write_text(root, &map.to_string()).unwrap();
        super::super::sync(root).unwrap();
        let files = vec![
            "area0/storage.service.rs".into(),
            "area1/storage.service.rs".into(),
        ];
        let suggested = dispatch_preparation(root, &files, "Change transactions").unwrap();
        assert_eq!(suggested["required"], false);
        assert_eq!(suggested["model_calls"], 0);
        assert!(dispatch_preparation(root, &files[..1], "Change transactions").is_none());
        p.origin = "authored".into();
        p.name = "transactions".into();
        let draft = record(root, &p).unwrap();
        review(root, &p.name, draft["fingerprint"].as_str().unwrap()).unwrap();
        assert!(dispatch_preparation(root, &files, "Change transactions").is_none());
    }
}
