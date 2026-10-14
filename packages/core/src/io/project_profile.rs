//! Persist compact project observations in the scanner transaction. Naming and
//! architecture are descriptive evidence, never mandatory implementation rules.
use crate::{
    domain::{ast::is_test_path, pattern, project_map::ProjectMap},
    io::{project_map, sha256::Sha256},
    platform::error::{Error, Result},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
pub(crate) const VERSION:u32=2;

fn digest(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.hex_digest()
}

fn naming(name: &str) -> &'static str {
    if name.contains('-') {
        "kebab"
    } else if name.contains('_') {
        if name
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(char::is_uppercase)
        {
            "upper-snake"
        } else {
            "snake"
        }
    } else if name.chars().next().is_some_and(char::is_uppercase) {
        "pascal"
    } else if name.chars().any(char::is_uppercase) {
        "camel"
    } else {
        "lower"
    }
}

fn declaration_observations(conn: &Connection, patterns: &pattern::Pattern) -> Result<Vec<Value>> {
    let mut kinds = BTreeMap::<String, BTreeMap<String, u64>>::new();
    let mut signatures = BTreeMap::<String, Vec<Value>>::new();
    let mut decls=conn.prepare("SELECT file,kind,name,line,signature FROM decls WHERE name IS NOT NULL AND line IS NOT NULL ORDER BY file,line")?;
    for row in decls.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, u32>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })? {
        let (file, kind, name, line, signature) = row?;
        if is_test_path(&file) {
            continue;
        }
        let area = patterns
            .roles
            .get(&file)
            .cloned()
            .unwrap_or_else(|| "project".into());
        *kinds
            .entry(area.clone())
            .or_default()
            .entry(kind.unwrap_or_else(|| "unknown".into()))
            .or_default() += 1;
        if let Some(signature) = signature {
            let examples = signatures.entry(area).or_default();
            if examples.len() < 3 {
                examples.push(json!({"file":file,"line":line,"name":name,"signature":signature.chars().take(240).collect::<String>()}));
            }
        }
    }
    Ok(kinds.into_iter().map(|(area,kinds)|json!({"area":area,"kinds":kinds,
        "signature_examples":signatures.get(&area),"status":"observed syntax and annotations; not runtime contracts or rules"})).collect())
}

fn specifier_observations(conn: &Connection, patterns: &pattern::Pattern) -> Result<Vec<Value>> {
    let mut imports = BTreeMap::<String, BTreeMap<String, (u64, Vec<String>)>>::new();
    let mut files =
        conn.prepare("SELECT path,imports FROM files WHERE imports IS NOT NULL ORDER BY path")?;
    for row in files.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (file, text) = row?;
        if is_test_path(&file) {
            continue;
        }
        let Ok(specifiers) = serde_json::from_str::<Vec<String>>(&text) else {
            continue;
        };
        let area = patterns
            .roles
            .get(&file)
            .cloned()
            .unwrap_or_else(|| "project".into());
        for specifier in specifiers
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
        {
            let count = imports
                .entry(area.clone())
                .or_default()
                .entry(specifier)
                .or_default();
            count.0 += 1;
            if count.1.len() < 3 {
                count.1.push(file.clone());
            }
        }
    }
    Ok(imports.into_iter().map(|(area,imports)| {
        let mut rows:Vec<_>=imports.into_iter().collect();rows.sort_by(|a,b|b.1.0.cmp(&a.1.0).then_with(||a.0.cmp(&b.0)));
        let total=rows.len();rows.truncate(8);json!({"area":area,"distinct_specifiers":total,
            "top":rows.into_iter().map(|(name,(count,files))|json!({"specifier":name,"file_count":count,"examples":files})).collect::<Vec<_>>(),
            "status":"observed import paths; may be external libraries or internal aliases"})
    }).collect())
}

pub(crate) fn sync(conn: &Connection, map: &ProjectMap) -> Result<()> {
    let patterns = pattern::learn(map);
    let mut naming_by_area = BTreeMap::<String, BTreeMap<&str, (u64, Vec<Value>)>>::new();
    let mut names=conn.prepare("SELECT file,name,line FROM decls WHERE name IS NOT NULL AND line IS NOT NULL ORDER BY file,line")?;
    for row in names.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u32>(2)?,
        ))
    })? {
        let (file, name, line) = row?;
        if is_test_path(&file) || name.is_empty() {
            continue;
        }
        let area = patterns.roles.get(&file).cloned().unwrap_or_else(|| {
            file.rsplit_once('/')
                .map_or("project", |(dir, _)| dir)
                .into()
        });
        let stat = naming_by_area
            .entry(area)
            .or_default()
            .entry(naming(&name))
            .or_default();
        stat.0 += 1;
        if stat.1.len() < 3 {
            stat.1.push(json!({"file":file,"line":line,"name":name}));
        }
    }
    let naming:Vec<_>=naming_by_area.into_iter().map(|(area,styles)| {
        let total:u64=styles.values().map(|(n,_)|n).sum();
        json!({"area":area,"samples":total,"styles":styles.into_iter().map(|(style,(count,examples))|
            json!({"style":style,"count":count,"of":total,"examples":examples})).collect::<Vec<_>>(),"status":"observed; not a rule"})
    }).collect();
    let mut versions = conn.prepare("SELECT path,blob FROM files ORDER BY path")?;
    let source_versions: Vec<_> = versions
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let census: Option<String> = conn
        .query_row("SELECT coverage_report FROM census LIMIT 1", [], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    let coverage = census
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(Value::Null);
    let strong:Vec<_>=patterns.strong.iter().map(|direction|json!({"from":direction.from,"to":direction.to,
        "along":direction.along,"against":direction.against,"of":direction.along+direction.against,
        "examples":map.modules.iter().filter(|m|patterns.roles.get(&m.path)==Some(&direction.from)).flat_map(|m|
            m.deps.iter().filter(|dep|patterns.roles.get(*dep)==Some(&direction.to)).map(|dep|json!({"file":m.path,"imports":dep}))).take(3).collect::<Vec<_>>() })).collect();
    let declarations:u32=conn.query_row("SELECT count(*) FROM decls",[],|row|row.get(0))?;
    let payload = json!({"version":VERSION,"size":{"code_files":map.modules.len(),"lines":map.modules.iter().map(|m|m.loc).sum::<usize>(),
        "declarations":declarations,"projects":map.projects.len()},
        "languages":map.languages,"projects":map.projects,"structure":map.skeleton,"coverage":coverage,
        "patterns":patterns,"import_conventions":strong,"naming_conventions":naming,
        "declaration_conventions":declaration_observations(conn,&patterns)?,"import_specifiers":specifier_observations(conn,&patterns)?,
        "entry_candidates":map.graph.top_fan_in.iter().take(8).collect::<Vec<_>>(),
        "status":"observed-static-snapshot","mandatory_rules":false,
        "limits":["Import relationships do not prove runtime order.","Frequency is an observed convention, not authorization or a business rule.",
            "Dynamic behavior requires current source or cited session interpretation."],"model_calls":0});
    let fingerprint = digest(
        json!({"sources":source_versions,"profile":payload})
            .to_string()
            .as_bytes(),
    );
    let old: Option<String> = conn
        .query_row("SELECT fingerprint FROM project_profile LIMIT 1", [], |r| {
            r.get(0)
        })
        .optional()?;
    if old.as_deref() == Some(&fingerprint) {
        return Ok(());
    }
    conn.execute("DELETE FROM project_profile", [])?;
    conn.execute(
        "INSERT INTO project_profile(fingerprint,payload) VALUES(?1,?2)",
        params![fingerprint, payload.to_string()],
    )?;
    Ok(())
}

/// Read only: the UI and agents never trigger a scan while inspecting a profile.
pub fn snapshot(root: &Path) -> Value {
    let found = (|| {
        let db = project_map::read_only_existing(&project_map::model_path(root))
            .map_err(|e| Error::config(format!("{e:?}")))?;
        let row: Option<(String, String)> = db
            .conn()
            .query_row(
                "SELECT fingerprint,payload FROM project_profile LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (fingerprint, text) = row.ok_or_else(|| Error::config("profile-not-yet-scanned"))?;
        let data: Value = serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;
        if data["status"] == "unavailable" {
            return Ok(json!({"available":false,"reason":data["reason"],"model_calls":0}));
        }
        Ok::<_, Error>(
            json!({"available":true,"fingerprint":fingerprint,"data":data,"model_calls":0}),
        )
    })();
    found.unwrap_or_else(
        |_| json!({"available":false,"reason":"profile-not-yet-scanned","model_calls":0}),
    )
}

/// Progressive disclosure: persist the whole observed profile, return only
/// the selected area's conventions to a task. No business rule is inferred.
pub fn summary(root: &Path, files: &[String]) -> Value {
    let mut view = snapshot(root);
    if view["available"] != true {
        return view;
    }
    let roles = view["data"]["patterns"]["roles"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let areas: std::collections::BTreeSet<_> = files
        .iter()
        .filter_map(|file| roles.get(file).and_then(Value::as_str))
        .collect();
    view["data"]
        .as_object_mut()
        .map(|data| data.remove("patterns"));
    for key in [
        "naming_conventions",
        "import_conventions",
        "declaration_conventions",
        "import_specifiers",
    ] {
        if let Some(rows) = view["data"][key].as_array_mut() {
            if !files.is_empty() {
                rows.retain(|row| {
                    areas.contains(row["area"].as_str().unwrap_or_default())
                        || areas.contains(row["from"].as_str().unwrap_or_default())
                        || areas.contains(row["to"].as_str().unwrap_or_default())
                });
            }
            let total = rows.len();
            rows.truncate(8);
            view[format!("{key}_total")] = json!(total);
        }
    }
    view["scope"] = json!(files);
    // Structural tables are already available through map/knowledge. Do not
    // load the complete tree or manifests into every wave or UI refresh.
    if let Some(data) = view["data"].as_object_mut() {
        data.remove("structure");
        data.remove("projects");
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reading_an_old_schema_does_not_migrate_or_recreate_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        project_map::write_text(root, "{\"modules\":[]}").unwrap();
        {
            let db = project_map::open_existing(&project_map::model_path(root)).unwrap();
            db.conn().execute("DROP TABLE project_profile", []).unwrap();
        }
        let before = std::fs::read(project_map::model_path(root)).unwrap();
        assert_eq!(snapshot(root)["available"], false);
        assert_eq!(
            std::fs::read(project_map::model_path(root)).unwrap(),
            before
        );
        let db = project_map::read_only_existing(&project_map::model_path(root)).unwrap();
        assert_eq!(
            db.conn()
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name='project_profile'",
                    [],
                    |r| r.get::<_, u32>(0)
                )
                .unwrap(),
            0
        );
    }
    #[test]
    fn profile_persists_real_conventions_reuses_unchanged_sources_and_changes_with_the_graph() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut map = json!({"languages":[{"language":"rust","files":1,"loc":8}],"modules":[{"path":"source.rs","loc":8,
            "declarations":[{"kind":"function","name":"read_record","line":2,"end_line":4}]}]});
        project_map::write_text(root, &map.to_string()).unwrap();
        let first = snapshot(root);
        assert!(first["available"].as_bool().unwrap());
        assert_eq!(
            first["data"]["naming_conventions"][0]["styles"][0]["style"],
            "snake"
        );
        assert_eq!(
            first["data"]["naming_conventions"][0]["styles"][0]["examples"][0]["line"],
            2
        );
        project_map::save_at(
            &project_map::model_path(root),
            &map,
            "",
            &crate::domain::normalize::Languages::new([]),
        )
        .unwrap();
        assert_eq!(snapshot(root)["fingerprint"], first["fingerprint"]);
        map["modules"][0]["declarations"][0]["name"] = json!("readRecord");
        project_map::save_at(
            &project_map::model_path(root),
            &map,
            "",
            &crate::domain::normalize::Languages::new([]),
        )
        .unwrap();
        let changed = snapshot(root);
        assert_ne!(changed["fingerprint"], first["fingerprint"]);
        assert_eq!(
            changed["data"]["naming_conventions"][0]["styles"][0]["style"],
            "camel"
        );
        assert_eq!(changed["data"]["mandatory_rules"], false);
    }
    #[test]
    fn missing_profile_reads_do_not_create_a_database() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(snapshot(dir.path())["available"], false);
        assert!(!project_map::model_path(dir.path()).exists());
    }
}
