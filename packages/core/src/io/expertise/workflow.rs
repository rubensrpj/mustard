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

/// The purpose includes the assigned work, not just the broad wave title.
/// Keep planning text out of the source search's literal input arguments.
pub fn wave_task(log:&crate::domain::spec_events::SpecLog,wave:u64)->String {
    let mut seen=BTreeSet::new();
    log.block(crate::domain::spec_events::BlockQuery::Wave(wave)).into_iter().filter(|e|e.event_type=="task")
        .flat_map(|e|["title","text","agent"].into_iter().filter_map(|field|e.str_field(field)))
        .filter(|text|seen.insert(text.split_whitespace().collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join("\n")
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
        .collect();
    // Include every equally valid alternative, plus the abstention option.
    // A partial shortlist could hide the right procedure. An oversized
    // taxonomy stays native; the caller also bounds the complete payload.
    if (2..=254).contains(&peers.len()) {
        peers
    } else {
        Vec::new()
    }
}

pub fn select(root: &Path, files: &[String], preferred: Option<&str>, source: &str) -> Selection {
    select_many(root,files,&preferred.into_iter().map(str::to_owned).collect::<Vec<_>>(),source)
}

/// A typed fit decision can retain complementary procedures for the same
/// files. File coverage alone cannot express distinct task concerns.
pub fn select_many(root: &Path, files: &[String], preferred: &[String], source: &str) -> Selection {
    select_task(root,files,preferred,source,None)
}

fn task_terms(query:&str,files:&[String])->BTreeSet<String> {
    use crate::domain::text::{fold,words,COMMON_WORDS_PT,COMMON_WORDS_EN};
    let file_words:BTreeSet<_>=files.iter().flat_map(|file|words(file).map(fold)).collect();
    words(&fold(query)).filter(|word|word.len()>3 && !COMMON_WORDS_PT.contains(word)
        && !COMMON_WORDS_EN.contains(word) && !file_words.contains(*word)).map(str::to_owned).collect()
}

/// This is lexical evidence only, never a semantic guarantee. No positive
/// task evidence means abstention, rather than loading by path overlap.
pub fn task_fit(profile:&Profile,query:&str,files:&[String])->usize {
    let content=crate::domain::text::fold(&format!("{} {}",profile.description,profile.procedure));
    let terms:BTreeSet<_>=crate::domain::text::words(&content).collect();
    task_terms(query,files).iter().filter(|word|terms.contains(word.as_str())).count()
}

pub fn select_for_task(root:&Path,files:&[String],query:&str,preferred:&[String],source:&str)->Selection {
    select_task(root,files,preferred,source,Some(query))
}

fn select_task(root:&Path,files:&[String],preferred:&[String],source:&str,query:Option<&str>)->Selection {
    let mut available = candidates(root, files);
    let accepted:BTreeSet<_> = preferred.iter().filter(|name|available.iter().any(|profile|profile.name==name.as_str())).cloned().collect();
    if let Some(query)=query {
        available.retain(|profile|profile.origin!="authored" || accepted.contains(&profile.name) || task_fit(profile,query,files)>0);
        available.sort_by_key(|profile|std::cmp::Reverse(task_fit(profile,query,files)));
    }
    available.sort_by_key(|profile|preferred.iter().position(|name|*name==profile.name).unwrap_or(usize::MAX));
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
            !adds.is_empty() || accepted.contains(&profile.name)
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
    } else if !accepted.is_empty() && source.starts_with("jev") {
        "typed choice among current, equally covering procedures; native gates preserved"
    } else {
        "reuse current procedures first, then navigation for uncovered task files"
    };
    let fingerprint = digest(
        &serde_json::to_vec(&(
            files,
            query,
            selected
                .iter()
                .map(|p| review_fingerprint(root, p))
                .collect::<Vec<_>>(),
        ))
        .unwrap_or_default(),
    );
    Selection {
        preferred_agent: selected.iter().find(|p| p.origin == "authored").map(|p| format!("mustard-{}", p.name)),
        profiles: selected.iter().map(|p| p.name.clone()).collect(),
        reason: reason.into(),
        fingerprint,
        pending_review,
        uncovered_files: files
            .iter()
            .filter(|file| !covered.contains(*file))
            .cloned()
            .collect(),
        source: if !accepted.is_empty() {
            source.into()
        } else {
            "native".into()
        },
    }
}

/// Re-check all reviewed path candidates, including a single procedure. A
/// path match alone cannot prove that any procedure fits the current task.
pub fn task_candidates(root:&Path,files:&[String])->Vec<Profile> {
    let mut unique=BTreeSet::new();
    let peers:Vec<_>=candidates(root,files).into_iter().filter(|p|p.origin=="authored")
        .filter(|p|unique.insert((p.procedure.split_whitespace().collect::<Vec<_>>().join(" "),p.paths.clone())))
        .collect();
    if peers.len()<=254 {peers} else {Vec::new()}
}

pub fn restore_for_task(root:&Path,files:&[String],query:&str,stored:Option<&Selection>)->Selection {
    if let Some(stored)=stored {
        let current=select_for_task(root,files,query,&stored.profiles,&stored.source);
        if current.fingerprint==stored.fingerprint {return current;}
    }
    select_for_task(root,files,query,&[],"native")
}

/// A saved preference belongs to the exact reviewed evidence it compared.
/// Re-reviewing a changed procedure with the same name cannot revive it.
pub fn restore(root: &Path, files: &[String], stored: Option<&Selection>) -> Selection {
    if let Some(stored) = stored {
        let current = select_many(
            root,
            files,
            &stored.profiles,
            &stored.source,
        );
        if current.fingerprint == stored.fingerprint {
            return current;
        }
    }
    select(root, files, None, "native")
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
    prepare_selected(root,host,select(root,files,preferred,source))
}

pub fn prepare_for_task(root:&Path,files:&[String],host:&str,query:&str)->Result<Value,String> {
    prepare_selected(root,host,select_for_task(root,files,query,&[],"native"))
}

fn prepare_selected(root:&Path,host:&str,selection:Selection)->Result<Value,String> {
    if !["claude", "codex"].contains(&host) {
        return Err("expertise-unknown-host".into());
    }
    let authored:BTreeSet<_>=list(root)?.into_iter().filter(|profile|profile.origin=="authored").map(|profile|profile.name).collect();
    let exports = selection
        .profiles
        .iter()
        .filter(|name| authored.contains(*name))
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
    if files.is_empty() || query.trim().is_empty() || !crate::io::project_map::model_path(root).is_file() {
        return None;
    }
    let current = candidates(root, files);
    // Respect an explicitly maintained project skill. It is reviewed by the
    // existing wave skill/rule path rather than replaced by a generated one.
    let uncovered: Vec<_> = files.iter().filter(|file| {
        !current.iter().any(|profile| profile.origin == "authored" && matches(profile, &[(*file).clone()]) && task_fit(profile,query,files)>0)
            && !crate::io::wave_prompt::has_scoped_skill(root, &[(*file).clone()])
    }).cloned().collect();
    if uncovered.is_empty() {
        return None;
    }
    // The CLI bounds UTF-8 bytes, not characters. A Portuguese task must
    // produce valid preparation arguments just like an ASCII task does.
    let query = query.char_indices().take_while(|(at, ch)| at + ch.len_utf8() <= 1400).map(|(_, ch)| ch).collect::<String>();
    let pending:Vec<_>=list(root).unwrap_or_default().into_iter().filter(|p|p.origin=="authored" && matches(p,files)
        && task_fit(p,&query,files)>0 && !active(root,p)).map(|p|p.name).collect();
    let fingerprint=preparation_identity(root,files,&query);
    let acknowledged=std::fs::read_to_string(native_ack_path(root,&fingerprint)).ok().as_deref()==Some(&fingerprint);
    let required=!pending.is_empty() && !acknowledged;
    let args = |host: &str| {
        let mut args = vec!["run".to_string(), "expertise".into(), "--prepare".into(), "--host".into(), host.into(), "--query".into(), query.clone()];
        for file in &uncovered { args.extend(["--file".into(), file.clone()]); }
        args
    };
    Some(
        json!({"query":query,"files":uncovered,"required":required,"fingerprint":fingerprint,"native_acknowledged":acknowledged,"model_calls":0,
        "state":if required {"review-required"}else{"native-guidance-available"},
        "pending_review":pending,
        "claude_args":args("claude"),
        "codex_args":args("codex"),
        "next":"If required, inspect the pending draft and current rules before dispatch: update/review its fingerprint, or deliberately use expertise --native --query <same task> --file <scope> after checking native guidance. Otherwise native dispatch is available; prepare a reusable procedure only if needed. Reuse a current draft/analysis; never start another CLI/API."}),
    )
}

fn preparation_identity(root:&Path,files:&[String],query:&str)->String {
    let versions:Vec<_>=files.iter().map(|file|(file,std::fs::read(root.join(file)).ok().map(|bytes|digest(&bytes)))).collect();
    digest(json!({"task":query.split_whitespace().collect::<Vec<_>>().join(" "),"sources":versions,
        "rules":crate::io::knowledge::RuleContext::of(root,files),"procedures":list(root).unwrap_or_default().into_iter()
        .filter(|p|matches(p,files)).map(|p|(p.name.clone(),review_fingerprint(root,&p),p.reviewed)).collect::<Vec<_>>()}).to_string().as_bytes())
}
fn native_ack_path(root:&Path,fingerprint:&str)->std::path::PathBuf {
    root.join(".claude/.cache/expertise-native").join(fingerprint)
}
pub fn acknowledge_native(root:&Path,files:&[String],query:&str)->Result<Value,String> {
    if files.is_empty() || files.len()>256 || query.trim().is_empty() || query.len()>1400 {return Err("expertise-native-needs-task-and-files".into());}
    if files.iter().any(|file|file.len()>512 || file.contains('\\') || std::path::Path::new(file).components()
        .any(|part|!matches!(part,std::path::Component::Normal(_))) || root.join(file).canonicalize().ok()
        .zip(root.canonicalize().ok()).is_some_and(|(file,root)|!file.starts_with(root))) {return Err("expertise-invalid-scope".into());}
    let fingerprint=preparation_identity(root,files,query);
    let path=native_ack_path(root,&fingerprint);let parent=path.parent().ok_or("expertise-invalid-path")?;
    std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    if !parent.canonicalize().map_err(|e|e.to_string())?.starts_with(root.canonicalize().map_err(|e|e.to_string())?) {return Err("expertise-path-outside-project".into());}
    crate::io::fs::write_atomic(&path,fingerprint.as_bytes()).map_err(|e|e.to_string())?;
    Ok(json!({"ok":true,"native_guidance_reviewed":true,"fingerprint":fingerprint,"model_calls":0,
        "limits":"This clears preparation for these task sources/rules only; it does not approve code, tests, scope changes or an authored draft."}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::expertise::{record, review};
    #[test]
    fn task_fit_abstains_from_unrelated_procedure_and_preparation_is_source_bound() {
        let (dir,mut p)=crate::io::expertise::tests::fixture();let root=dir.path();
        p.description="Transactions and rollback".into();p.procedure="Reuse transactions and verify rollback.".into();
        let files=vec!["store.rs".into()];
        let draft=record(root,&p).unwrap();
        assert_eq!(dispatch_preparation(root,&files,"Change transactions").unwrap()["required"],true);
        acknowledge_native(root,&files,"Change transactions").unwrap();
        assert_eq!(dispatch_preparation(root,&files,"Change transactions").unwrap()["required"],false);
        std::fs::write(root.join("AGENTS.md"),"Use current project schemas").unwrap();
        assert_eq!(dispatch_preparation(root,&files,"Change transactions").unwrap()["required"],true);
        // Review against the changed project rules, not the obsolete receipt.
        assert!(review(root,&p.name,draft["fingerprint"].as_str().unwrap()).is_err());
        let draft=record(root,&p).unwrap();review(root,&p.name,draft["fingerprint"].as_str().unwrap()).unwrap();
        assert!(dispatch_preparation(root,&files,"Change transactions").is_none());
        assert!(select_for_task(root,&files,"Adjust visual component",&[],"native").profiles.is_empty());
        let chosen=select_for_task(root,&files,"Change transactions",&[],"native");assert_eq!(chosen.profiles,[p.name.clone()]);
        assert!(restore_for_task(root,&files,"Adjust visual component",Some(&chosen)).profiles.is_empty());
    }
    #[test]
    fn unique_area_without_navigation_still_offers_preparation_without_forcing_inference() {
        let (dir,_)=crate::io::expertise::tests::fixture();
        let proposed=dispatch_preparation(dir.path(),&["unique.rs".into()],"Inspect archive").unwrap();
        assert_eq!(proposed["required"],false);assert_eq!(proposed["model_calls"],0);
        assert_eq!(proposed["files"],json!(["unique.rs"]));
    }
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
        for n in 0..3 {
            p.name = format!("alternative-{n}");
            p.procedure.push_str(" Check another distinct contract.");
            let draft = record(root, &p).unwrap();
            review(root, &p.name, draft["fingerprint"].as_str().unwrap()).unwrap();
        }
        assert_eq!(
            ambiguous(root, &files).len(),
            5,
            "do not silently omit valid procedures"
        );
        std::fs::write(root.join("AGENTS.md"), "Require project schemas.\n").unwrap();
        let stale = select(root, &files, Some(&p.name), "jev");
        assert!(stale.profiles.is_empty());
        assert_eq!(stale.pending_review.len(), 6);
        assert_eq!(stale.uncovered_files, files);
        assert_eq!(stale.source, "native");
    }
    #[test]
    fn unknown_selection_never_activates_a_profile_and_exports_are_reused() {
        let (dir, mut p) = crate::io::expertise::tests::fixture();
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
        let selected = select(root, &files, Some(&p.name), "jev");
        assert_eq!(restore(root, &files, Some(&selected)).source, "jev");
        p.procedure.push_str(" Check the changed procedure.");
        let draft = record(root, &p).unwrap();
        review(root, &p.name, draft["fingerprint"].as_str().unwrap()).unwrap();
        let renewed = restore(root, &files, Some(&selected));
        assert_eq!(renewed.source, "native");
        assert_ne!(renewed.fingerprint, selected.fingerprint);
    }

    #[test]
    fn uncovered_navigation_suggests_scoped_preparation_without_forcing_a_model() {
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
        assert!(dispatch_preparation(root, &files[..1], "Change transactions").is_some());
        let unicode = dispatch_preparation(root, &files, &"é".repeat(1400)).unwrap();
        assert!(unicode["query"].as_str().unwrap().len() <= 1400);
        p.origin = "authored".into();
        p.procedure="Change transactions using existing source contracts.".into();
        p.name = "transactions".into();
        let draft = record(root, &p).unwrap();
        review(root, &p.name, draft["fingerprint"].as_str().unwrap()).unwrap();
        assert!(dispatch_preparation(root, &files, "Change transactions").is_none());
        p.paths = vec![files[0].clone()];
        let draft = record(root, &p).unwrap();
        review(root, &p.name, draft["fingerprint"].as_str().unwrap()).unwrap();
        let partial = dispatch_preparation(root, &files, "Change transactions").unwrap();
        assert_eq!(partial["files"], json!([files[1]]));
        assert!(partial["claude_args"].as_array().unwrap().iter().any(|arg| arg == "--file"));
    }
}
