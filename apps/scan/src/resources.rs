//! Ingest non-code evidence through the same walk and source receipts. Format
//! choices live in the registry; the engine only reads accepted text.
use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::Path;

use mustard_core::domain::knowledge::resources::{self, File, Registry};
use mustard_core::io::project_map::Listing;
use mustard_core::io::sha256::Sha256;

pub(crate) fn unchanged(paths: &[String], previous: &[File], listing: &Listing) -> anyhow::Result<bool> {
    let registry = Registry::load().map_err(anyhow::Error::msg)?;
    let known: BTreeMap<_, _> = previous.iter().map(|file| (file.path.as_str(), file.blob.as_str())).collect();
    let candidates: Vec<_> = paths.iter().filter(|path| registry.format(path).is_some()).collect();
    Ok(candidates.len() == known.len() && candidates.iter().all(|path|
        known.get(path.as_str()).is_some_and(|blob| !blob.is_empty() && listing.blobs.get(*path).map(String::as_str) == Some(*blob))))
}

pub(crate) fn read(root: &Path, paths: &[String], previous: &[File], listing: Option<&Listing>) -> anyhow::Result<(Vec<File>, Vec<String>)> {
    let registry = Registry::load().map_err(anyhow::Error::msg)?;
    let known: BTreeMap<_, _> = previous.iter().map(|file| (file.path.as_str(), file)).collect();
    let mut files = Vec::new();
    let mut read = Vec::new();
    for path in paths {
        let Some(format) = registry.format(path) else { continue };
        let blob = listing.and_then(|listing| listing.blobs.get(path)).cloned().unwrap_or_default();
        if !blob.is_empty()
            && let Some(previous) = known.get(path.as_str()).filter(|previous| previous.blob == blob)
        {
            files.push((*previous).clone());
            continue;
        }
        let mut file = File { path: path.clone(), blob, kind: format.kind.clone(), ..File::default() };
        match content(root, path, registry.max_bytes) {
            Ok(text) if registry.sensitive(&text) => file.issue = "possible-sensitive-content".into(),
            Ok(text) => {
                let mut hash = Sha256::new();
                hash.update(text.as_bytes());
                file.sha256 = hash.hex_digest();
                file.sections = resources::sections(&text, format.outline, path);
                read.push(path.clone());
            }
            Err(reason) => file.issue = reason.into(),
        }
        files.push(file);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    read.sort();
    Ok((files, read))
}

fn content(root: &Path, file: &str, max: u64) -> Result<String, &'static str> {
    let canonical = root.join(file).canonicalize().map_err(|_| "unreadable")?;
    if !canonical.starts_with(root.canonicalize().map_err(|_| "unreadable")?) {
        return Err("outside-project");
    }
    let opened = std::fs::File::open(canonical).map_err(|_| "unreadable")?;
    if opened.metadata().map_err(|_| "unreadable")?.len() > max {
        return Err("too-large");
    }
    let mut bytes = Vec::new();
    opened.take(max + 1).read_to_end(&mut bytes).map_err(|_| "unreadable")?;
    if bytes.len() as u64 > max {
        return Err("too-large");
    }
    if bytes.contains(&0) {
        return Err("binary");
    }
    String::from_utf8(bytes).map_err(|_| "non-utf8")
}
