//! Explicit, sanitized snapshots. Export success is never remote publication.
use std::path::Path;

use mustard_core::io::sha256::Sha256;
use serde_json::{Value, json};

pub(crate) fn prepare(start: &Path, name: &str, include_consumption: bool) -> Value {
    if name.is_empty()
        || name.contains("..")
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return json!({"ok":false,"reason":"invalid-spec-name"});
    }
    let local = super::snapshot(start, Some(name));
    let Some(spec) = local["specs"]
        .as_array()
        .and_then(|specs| specs.iter().find(|s| s["name"] == name))
    else {
        return json!({"ok":false,"reason":"spec-not-found"});
    };
    // Deliberate allowlist: task prose, paths, code, conversations and
    // authentication never enter this externally shareable document.
    let project = local["project"]["name"]
        .as_str()
        .filter(|name| !name.contains(['/', '\\']) && !name.chars().any(char::is_control))
        .map(crate::shared::secret::without_secrets)
        .unwrap_or_else(|| "Projeto".into());
    let mut public = json!({"schema_version":1,"project":project,"spec":name,
        "language":local["project"]["language"],"phase":spec["phase"],"at":local["at"],"review_approved":spec["review_approved"],
        "final_validation_valid":spec["final_validation_valid"],
        "waves":spec["waves"].as_array().into_iter().flatten().map(|w|json!({"wave":w["wave"],"status":w["status"]})).collect::<Vec<_>>()});
    if include_consumption {
        public["consumption"] = spec["usage"].clone();
    }
    let root = mustard_core::io::spec_events::spec_root(start);
    let mut prepared = crate::shared::publication::prepare_snapshot(
        &root,
        &root.join(".claude/mustard/publications/specs").join(name),
        &public,
        crate::report::public_snapshot,
    );
    prepared["scope"] = json!("spec");
    prepared["spec"] = json!(name);
    prepared["language"] = local["project"]["language"].clone();
    prepared
}

pub(crate) fn publish(start: &Path, name: &str, include_consumption: bool) -> Value {
    let root = mustard_core::io::spec_events::spec_root(start);
    let prepared = prepare(start, name, include_consumption);
    let mut hash = Sha256::new();
    hash.update(name.as_bytes());
    let scope = format!("spec-{}", &hash.hex_digest()[..16]);
    let mut answer = crate::shared::publication::upload_prepared(
        &root,
        &root,
        prepared,
        &scope,
        crate::report::public_snapshot,
    );
    if answer["published"] == true {
        if let Err(reason) = record_publication(&root, name, &answer) {
            answer["recorded"] = json!(false);
            answer["record_reason"] = json!(reason);
        } else {
            answer["recorded"] = json!(true);
        }
    }
    answer
}

fn record_publication(root: &Path, name: &str, receipt: &Value) -> Result<(), String> {
    if receipt["published"] != true
        || receipt["remote_url"].as_str().is_none()
        || receipt["deployment_id"].as_str().is_none()
    {
        return Err("publication-not-confirmed".into());
    }
    let path = mustard_core::io::spec_events::spec_file(root, name)
        .map_err(|_| "publication-spec-unavailable")?;
    let written = mustard_core::io::spec_events::with_locked_writer(&path, |locked| {
        let already_recorded = locked.log().visible().iter().any(|event| {
            event.event_type == "publish"
                && event.str_field("deployment_id") == receipt["deployment_id"].as_str()
                && event.str_field("snapshot_id") == receipt["snapshot_id"].as_str()
                && event.str_field("url") == receipt["remote_url"].as_str()
        });
        if already_recorded { return Ok(()); }
        let draft = json!({"page":"spec","milestone":"explicit","ok":true,"url":receipt["remote_url"],
            "provider":receipt["provider"],"deployment_id":receipt["deployment_id"],"snapshot_id":receipt["snapshot_id"]});
        let draft = draft.as_object().cloned().ok_or("publication-invalid-receipt")?;
        locked.write_guarded("publish",draft,&[],&|_|Vec::new(),|_,_|Ok(()),|_|{})
            .map(|_|()).map_err(|_|"publication-event-write-failed".to_string())
    }).map_err(|_|"publication-spec-lock-error")?;
    written.ok_or("publication-spec-unavailable")?
}

pub(crate) fn prepare_project(start: &Path, include_consumption: bool) -> Value {
    let root = mustard_core::io::spec_events::spec_root(start);
    let local = super::snapshot(start, None);
    let project = local["project"]["name"]
        .as_str()
        .filter(|name| !name.contains(['/', '\\']) && !name.chars().any(char::is_control))
        .map(crate::shared::secret::without_secrets)
        .unwrap_or_else(|| "Projeto".into());
    let specs = local["specs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(
            |spec| json!({"name":spec["name"],"phase":spec["phase"],"waves":spec["counted_waves"]}),
        )
        .collect::<Vec<_>>();
    let mut public = json!({"schema_version":1,"kind":"project","project":project,"language":local["project"]["language"],"at":local["at"],"specs":specs});
    if include_consumption {
        public["consumption"] = local["jev"].clone();
    }
    let mut prepared = crate::shared::publication::prepare_snapshot(
        &root,
        &root.join(".claude/mustard/publications/project"),
        &public,
        crate::report::public_project_snapshot,
    );
    prepared["scope"] = json!("project");
    prepared["language"] = local["project"]["language"].clone();
    prepared
}

pub(crate) fn publish_project(start: &Path, include_consumption: bool) -> Value {
    let root = mustard_core::io::spec_events::spec_root(start);
    let prepared = prepare_project(start, include_consumption);
    let mut hash = Sha256::new();
    hash.update(root.to_string_lossy().as_bytes());
    let mut answer = crate::shared::publication::upload_prepared(
        &root,
        &root,
        prepared,
        &format!("project-{}", &hash.hex_digest()[..16]),
        crate::report::public_project_snapshot,
    );
    if answer["published"] == true {
        let path = root.join(".claude/mustard/publications/project/latest.json");
        // Confirmed native receipts are separate from historical page events,
        // so publishing a project does not require inventing a spec.
        let safe =
            !std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink());
        let receipt = json!({"published":true,"url":answer["remote_url"],"deployment_id":answer["deployment_id"],"snapshot_id":answer["snapshot_id"],"provider":answer["provider"]});
        answer["recorded"] = json!(
            safe && mustard_core::io::fs::write_atomic(&path, receipt.to_string().as_bytes())
                .is_ok()
        );
    }
    answer
}

pub(crate) fn publish_report(start: &Path, document: &Path) -> Value {
    let root = mustard_core::io::spec_events::spec_root(start);
    let path = if document.is_absolute() {
        document.to_path_buf()
    } else {
        start.join(document)
    };
    let Ok(body) = std::fs::read_to_string(&path) else {
        return json!({"ok":false,"reason":"publication-unreadable-document"});
    };
    if !crate::shared::secret::secret_excerpts(&body).is_empty() {
        return json!({"ok":false,"reason":"publication-secret-in-document"});
    }
    let Some((title, body)) = crate::report::markdown::leading_title(&body) else {
        return json!({"ok":false,"reason":"publication-document-needs-title"});
    };
    let public = json!({"schema_version":1,"kind":"report","language":crate::commands::spec_events::project(&root).lang.to_string(),
        "title":title,"body":body,"at":chrono::Utc::now().to_rfc3339()});
    let mut hash = Sha256::new();
    hash.update(title.as_bytes());
    let scope = format!("report-{}", &hash.hex_digest()[..16]);
    let prepared = crate::shared::publication::prepare_snapshot(
        &root,
        &root
            .join(".claude/mustard/publications/reports")
            .join(&scope),
        &public,
        crate::report::public_report_snapshot,
    );
    let mut answer = crate::shared::publication::upload_prepared(
        &root,
        &root,
        prepared,
        &scope,
        crate::report::public_report_snapshot,
    );
    answer["scope"] = json!("report");
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_export_works_without_an_open_spec_and_never_publishes_automatically() {
        let dir=tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"),"{}").unwrap();
        let local=super::super::snapshot(dir.path(),None);
        assert_eq!(local["specs"],json!([]));assert!(!dir.path().join(".claude").exists());
        let result=publish_project(dir.path(),false);
        assert_eq!(result["ok"],true,"{result}");assert_eq!(result["published"],false);
        let data:Value=serde_json::from_slice(&std::fs::read(result["database"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(data["kind"],"project");assert_eq!(data["specs"],json!([]));
        assert!(data.get("consumption").is_none() && data.get("spec").is_none());
        assert!(mustard_core::io::spec_index::project_page_url(dir.path()).is_none());
        let html=std::fs::read_to_string(result["page"].as_str().unwrap()).unwrap();assert!(html.contains("status-grid"));
    }

    #[test]
    fn a_manager_report_uses_native_layout_and_contains_only_the_requested_document() {
        let dir=project();let path=dir.path().join("report.md");
        std::fs::write(&path,"# Resultado para gestores\n\n## Benefícios\n\nEntrega conferida.\n\n| Etapa | Status |\n| --- | --- |\n| Revisão | Concluída |\n").unwrap();
        let result=publish_report(dir.path(),&path);assert_eq!(result["ok"],true,"{result}");assert_eq!(result["published"],false);
        let html=std::fs::read_to_string(result["page"].as_str().unwrap()).unwrap();assert!(html.contains("Resultado para gestores") && html.contains("<table"));
        assert!(!html.contains("PRIVATE CODE"));
        let data:Value=serde_json::from_slice(&std::fs::read(result["database"].as_str().unwrap()).unwrap()).unwrap();
        assert!(data.get("path").is_none() && !data.to_string().contains(&dir.path().to_string_lossy().to_string()));
        std::fs::write(&path,"# Relatório\n\nTYPESAFE_API_KEY=sk-abcdefghijklmnopqrstuvwxyz0123456789").unwrap();
        assert_eq!(publish_report(dir.path(),&path)["reason"],"publication-secret-in-document");
    }

    #[test]
    fn only_confirmed_publications_update_the_spec_link_and_retries_do_not_duplicate_events() {
        let dir = project();
        let receipt = json!({"published":true,"provider":"cloudflare-pages","deployment_id":"accepted-id",
            "snapshot_id":"content-version","remote_url":"https://abc.example.pages.dev"});
        let mut pending = receipt.clone();
        pending["published"] = json!(false);
        assert!(record_publication(dir.path(), "demo", &pending).is_err());
        record_publication(dir.path(), "demo", &receipt).unwrap();
        record_publication(dir.path(), "demo", &receipt).unwrap();
        let path = mustard_core::io::spec_events::spec_file(dir.path(), "demo").unwrap();
        let log =
            mustard_core::domain::spec_events::parse_log(&std::fs::read_to_string(path).unwrap());
        assert_eq!(
            log.visible()
                .iter()
                .filter(|e| e.event_type == "publish")
                .count(),
            1
        );
        let rows = mustard_core::io::spec_index::read_rows(dir.path());
        assert_eq!(
            rows[0].url.as_deref(),
            Some("https://abc.example.pages.dev")
        );
        let before = prepare(dir.path(), "demo", false);
        assert_eq!(
            before["published"], false,
            "exporting alone never records another publication"
        );
    }

    #[test]
    fn invalid_names_cannot_escape_the_publication_directory() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["../x", "a/b", "/tmp/out", "a..b", ""] {
            assert_eq!(
                prepare(dir.path(), name, false)["reason"],
                "invalid-spec-name"
            );
        }
        assert!(!dir.path().join(".claude").exists());
    }

    #[test]
    fn export_is_explicit_sanitized_and_retryable_without_a_remote_claim() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let folder = dir.path().join(".claude/spec/demo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("spec.ndjson"),
            concat!(
                "{\"v\":1,\"id\":1,\"type\":\"context\",\"text\":\"PRIVATE CODE /home/person KEY=secret\"}\n",
                "{\"v\":1,\"id\":2,\"type\":\"state\",\"phase\":\"survey\",\"author\":\"binary\"}\n"
            ),
        )
        .unwrap();
        let _ = super::super::snapshot(dir.path(), Some("demo"));
        assert!(!dir.path().join(".claude/mustard/publications").exists());
        let first = prepare(dir.path(), "demo", false);
        assert_eq!(first["ok"], true, "{first}");
        assert_eq!(first["published"], false);
        let database = std::fs::read_to_string(first["database"].as_str().unwrap()).unwrap();
        assert!(
            !database.contains("PRIVATE")
                && !database.contains("/home/")
                && !database.contains("secret")
        );
        assert!(!database.contains("consumption"));
        assert_eq!(
            serde_json::from_str::<Value>(&database).unwrap()["phase"],
            "survey"
        );
        let next = prepare(dir.path(), "demo", false);
        assert_eq!(first["snapshot_id"], next["snapshot_id"]);
        assert_eq!(first["page"], next["page"]);
    }
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let folder = dir.path().join(".claude/spec/demo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("spec.ndjson"),
            "{\"v\":1,\"id\":1,\"type\":\"state\",\"phase\":\"survey\",\"author\":\"binary\"}\n",
        )
        .unwrap();
        dir
    }
    #[test]
    fn parallel_preparations_reuse_one_complete_snapshot_and_repair_damaged_resources() {
        let dir = project();
        let results = std::thread::scope(|scope| {
            let jobs = (0..4)
                .map(|_| scope.spawn(|| prepare(dir.path(), "demo", true)))
                .collect::<Vec<_>>();
            jobs.into_iter()
                .map(|job| job.join().unwrap())
                .collect::<Vec<_>>()
        });
        for result in &results {
            assert_eq!(result["ok"], true, "{result}");
            assert_eq!(result["snapshot_id"], results[0]["snapshot_id"]);
        }
        let database = results[0]["database"].as_str().unwrap();
        assert!(
            std::fs::read_to_string(database)
                .unwrap()
                .contains("consumption")
        );
        let page = results[0]["page"].as_str().unwrap();
        std::fs::write(page, "broken page").unwrap();
        let repaired = prepare(dir.path(), "demo", true);
        assert_eq!(repaired["snapshot_id"], results[0]["snapshot_id"]);
        assert_ne!(std::fs::read_to_string(page).unwrap(), "broken page");
        std::fs::remove_file(database).unwrap();
        assert_eq!(prepare(dir.path(), "demo", true)["ok"], true);
        assert!(std::path::Path::new(database).is_file());
    }
    #[cfg(unix)]
    #[test]
    fn publication_does_not_follow_an_external_ancestor_or_resource_link() {
        let dir = project();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join(".claude/mustard")).unwrap();
        assert_eq!(prepare(dir.path(), "demo", false)["ok"], false);
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        std::fs::remove_file(dir.path().join(".claude/mustard")).unwrap();
        let exported = prepare(dir.path(), "demo", false);
        assert_eq!(exported["ok"], true);
        let page = std::path::Path::new(exported["page"].as_str().unwrap());
        let target = outside.path().join("keep");
        std::fs::write(&target, "kept").unwrap();
        std::fs::remove_file(page).unwrap();
        std::os::unix::fs::symlink(&target, page).unwrap();
        assert_eq!(prepare(dir.path(), "demo", false)["ok"], false);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "kept");
    }
}
