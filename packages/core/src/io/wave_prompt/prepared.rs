//! Current snippets are assembled locally from explicit read locations.
//! Components cache independently by locator, relevant map facts and actual
//! worktree bytes. No scan, model call or canonical read receipt is fabricated.
use std::collections::BTreeSet;
use std::path::{Component, Path};
use serde_json::{Value, json};
use crate::domain::project_map::MapDecl;
use crate::domain::spec_events::SpecEvent;
use crate::domain::wave_prompt::PreparedSource;
use crate::io::project_map::{Need, blobs_of, model_path, read_for};
use crate::io::sha256::Sha256;

const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;

pub(super) fn for_tasks(root: &Path, tree: &Path, tasks: &[&SpecEvent]) -> Vec<PreparedSource> {
    let locations: BTreeSet<&str> = tasks.iter().flat_map(|task| task.fields.get("must_read").and_then(Value::as_array)
        .into_iter().flatten().filter_map(Value::as_str)).collect();
    locations.into_iter().map(|location| component(root, tree, location)).collect()
}

fn digest(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.hex_digest()
}

fn component(root: &Path, tree: &Path, location: &str) -> PreparedSource {
    let (path, name) = location.split_once('#').unwrap_or((location, ""));
    let path = path.replace('\\', "/");
    let mut part = PreparedSource { source: location.into(), status: "source-unavailable: use original search/read".into(), ..PreparedSource::default() };
    let relative = Path::new(&path);
    let Some(base) = tree.canonicalize().ok() else { return part; };
    if relative.is_absolute() || relative.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return part;
    }
    let sensitive = relative.components().filter_map(|c| c.as_os_str().to_str()).any(|name| {
        let name = name.to_ascii_lowercase();
        name == ".env" || name.starts_with(".env.") || name.contains("credential") || name.ends_with(".pem") || name.ends_with(".key")
    });
    if sensitive { part.status = "sensitive-source: directed authorized read required".into(); return part; }
    let file = tree.join(relative);
    let Some(canonical) = file.canonicalize().ok().filter(|p| p.starts_with(&base) && p.is_file()) else { return part; };
    if !std::fs::metadata(&canonical).is_ok_and(|m| m.len() <= MAX_SOURCE_BYTES) { return part; }
    let Ok(bytes) = std::fs::read(&canonical) else { return part; };
    part.version = digest(&bytes);
    let Ok(text) = std::str::from_utf8(&bytes) else { return part; };
    let stored_blob = blobs_of(&model_path(root), &[&path]).ok().and_then(|blobs| blobs.get(&path).cloned());
    let current = crate::platform::git::run(tree, &["hash-object", "--", &path]);
    if stored_blob.as_deref().is_none_or(str::is_empty) || !current.ok || stored_blob.as_deref() != Some(current.stdout.trim()) {
        part.status = "map-source-unverified: use original search/read; refresh map if useful".into();
        return part;
    }
    if !std::fs::read(&canonical).is_ok_and(|again| again == bytes) {
        part.status = "source-changing: directed read required".into();
        return part;
    }
    let declarations = read_for(root, Need::Declarations { file: Some(&path), name }).ok();
    let selected: Vec<&MapDecl> = declarations.as_ref().and_then(|map| map.module(&path)).into_iter()
        .flat_map(|module| module.declarations.iter().filter(|decl| decl.name == name)).collect();
    let tests = read_for(root, Need::Tests(&path)).ok().and_then(|map| map.module(&path).cloned()).map(|module| module.tests).unwrap_or_default();
    let importers = read_for(root, Need::Importers(&path)).ok().map(|map| map.modules.into_iter()
        .filter(|module| module.deps.contains(&path)).map(|module| module.path).collect::<Vec<_>>()).unwrap_or_default();
    part.knowledge=crate::io::knowledge::for_source(root,tree,&path,name);
    // Optional evidence cannot crowd mandatory rules/items out of the prompt.
    if part.knowledge.to_string().len()>3000 {
        part.knowledge=json!({"evidence_version":part.knowledge["evidence_version"],
            "semantic_proof":false,"detail_compacted":true,
            "expand":"run knowledge --file <source file> --detail"});
    }
    let facts = json!({"declarations":selected,"tests":tests,"importers":importers,"knowledge":part.knowledge});
    let fingerprint = digest(json!({"revision":1,"source":location,"version":part.version,"facts":facts}).to_string().as_bytes());
    let cache = root.join(".claude/mustard/prepared-context");
    let cache_file = cache.join(format!("{}.json", digest(location.as_bytes())));
    if safe_cache(root, &cache) && !std::fs::symlink_metadata(&cache_file).is_ok_and(|m| m.file_type().is_symlink())
        && let Some(stored) = std::fs::read(&cache_file).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(|stored| stored["fingerprint"] == fingerprint && stored["locator"] == location)
            .and_then(|stored| serde_json::from_value::<PreparedSource>(stored["part"].clone()).ok())
            .filter(|stored| stored.version == part.version) {
        return stored;
    }
    part.candidates = tests.into_iter().chain(importers).collect::<BTreeSet<_>>().into_iter().collect();
    if !selected.is_empty() && selected.iter().all(|d| d.line > 0 && d.end_line >= d.line) {
        let ranges = selected.iter().map(|decl| format!("{}-{}", decl.line, decl.end_line)).collect::<Vec<_>>().join(",");
        part.source = format!("{path}:{ranges}#{name}");
        part.excerpt = selected.iter().map(|decl| crate::domain::project_map::lines_of(text, decl.line, decl.end_line)).collect::<Vec<_>>().join("\n");
        part.status = "current-source; syntax relations remain candidates".into();
        if part.excerpt.len() > 8192 {
            // Oversized optional excerpts keep every source range available
            // for expansion; they never crowd mandatory spec/rules out.
            part.excerpt.clear();
            part.status = "current-source; large excerpt: directed read required".into();
        }
    } else {
        part.status = "current-file; declaration unresolved: directed read required".into();
    }
    // Store the original locator separately from its resolved line range.
    let stored = json!({"fingerprint":fingerprint,"locator":location,"part":part});
    if safe_cache(root, &cache) && !std::fs::symlink_metadata(&cache_file).is_ok_and(|m| m.file_type().is_symlink()) {
        let _ = crate::io::fs::write_atomic(cache_file, stored.to_string().as_bytes());
    }
    part
}

fn safe_cache(root: &Path, directory: &Path) -> bool {
    let Ok(relative) = directory.strip_prefix(root) else { return false; };
    let mut parent = root.to_path_buf();
    for piece in relative.components() {
        parent.push(piece);
        if let Ok(meta) = std::fs::symlink_metadata(&parent)
            && (meta.file_type().is_symlink() || !meta.is_dir()) { return false; }
    }
    true
}

/// A known content mismatch makes old line positions unusable. Missing
/// historical blobs retain the legacy named hint with the explicit unknown
/// evidence status above, rather than claiming content equality.
pub(super) fn changed(root: &Path, tree: &Path, location: &str) -> bool {
    let path = location.split('#').next().unwrap_or(location);
    let old = blobs_of(&model_path(root), &[path]).ok().and_then(|blobs| blobs.get(path).cloned());
    let Some(old) = old.filter(|blob| !blob.is_empty()) else { return false; };
    let current = crate::platform::git::run(tree, &["hash-object", "--", path]);
    !current.ok || old != current.stdout.trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_changed_secondary_source_invalidates_the_cached_interpretation() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        assert!(crate::platform::git::run(root,&["init","-q"]).ok);
        let text="fn run() {}\n";
        for file in ["a.rs","b.rs"] {std::fs::write(root.join(file),text).unwrap();}
        let blob=crate::platform::git::run(root,&["hash-object","a.rs"]).stdout.trim().to_string();
        let mut raw=json!({"modules":[{"path":"a.rs","blob":blob,
            "analysis":{"content_sha256":digest(text.as_bytes()),"parse_complete":true},
            "declarations":[{"name":"run","line":1,"end_line":1}]}]});
        crate::domain::knowledge::enrich(&mut raw);
        crate::io::project_map::write_text(root,&raw.to_string()).unwrap();
        let source=|file:&str|crate::domain::knowledge::Source{file:file.into(),line:1,end_line:1,sha256:digest(text.as_bytes())};
        let note=crate::io::knowledge::Interpretation{id:"multi-source".into(),title:"Process".into(),text:"A uses B.".into(),status:"hypothesis".into(),origin:"fixture".into(),sources:vec![source("a.rs"),source("b.rs")]};
        crate::io::knowledge::record(root,&note).unwrap();
        let first=component(root,root,"a.rs#run");
        assert_eq!(first.knowledge["interpretations"][0]["id"],"multi-source");
        assert_eq!(component(root,root,"a.rs#run"),first);
        std::fs::write(root.join("b.rs"),"fn changed() {}\n").unwrap();
        let after=component(root,root,"a.rs#run");
        assert_eq!(after.version,first.version);
        assert!(after.knowledge.is_null());
        assert_eq!(after.excerpt,first.excerpt);
    }
}
