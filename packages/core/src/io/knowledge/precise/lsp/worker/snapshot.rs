//! Source hashes guarded by native metadata, plus dependency metadata.
//! Missing/bounded inventory disables warm reuse rather than asserting freshness.
use crate::io::{sha256::Sha256,knowledge::investigation::safe_read};
use crate::domain::knowledge::resources::Registry;
use std::collections::{BTreeSet,BTreeMap};
use std::hash::{Hash,Hasher};
use std::path::{Path,PathBuf};
use std::time::{Instant,Duration};

#[derive(Default)]
pub(super) struct Snapshot {sources:BTreeMap<String,(String,String)>,queried:BTreeSet<String>}
impl Snapshot {
pub(super) fn observe(&mut self,file:&str) {self.queried.insert(file.into());}
pub(super) fn fingerprint(&mut self,tree:&Path)->Result<String,String> {
    let start=Instant::now();
    let inventory=crate::platform::git::run(tree,&["ls-files","--cached","--others","--exclude-standard","-z"]);
    if !inventory.ok {return Err("lsp-warm-native-inventory-unavailable".into());}
    let registry=Registry::load()?;
    let mut digest=Sha256::new();
    // Explicitly queried ignored files still participate in buffer freshness.
    let paths=inventory.stdout.split('\0').filter(|p|!p.is_empty()).chain(self.queried.iter().map(String::as_str)).collect::<BTreeSet<_>>();
    self.sources.retain(|path,_|paths.contains(path.as_str()));
    for path in paths {
        if start.elapsed()>Duration::from_secs(6) {return Err("lsp-context-inventory-timeout".into());}
        if !registry.admits_path(path) {continue;}
        // Cache validation hashes only. Native metadata guards reuse; Unix
        // ctime/inode also detect equal-length writes with restored mtime.
        let meta=std::fs::metadata(tree.join(path)).ok();
        let stamp=meta.as_ref().map(stamp).unwrap_or_else(||"missing".into());
        let hash=if let Some((_,hash))=self.sources.get(path).filter(|(previous,_)|previous==&stamp) {
            hash.clone()
        } else {
            let hash=if let Some(text)=safe_read(tree,path,&registry) {
                let mut hash=Sha256::new();hash.update(text.as_bytes());hash.hex_digest()
            } else {stamp.clone()};
            self.sources.insert(path.into(),(stamp,hash.clone()));hash
        };
        digest.update(path.as_bytes());digest.update(hash.as_bytes());
    }
    for args in [&["rev-parse","HEAD"][..],&["symbolic-ref","-q","HEAD"][..]] {
        digest.update(crate::platform::git::run(tree,args).stdout.as_bytes());
    }
    let mut visited=BTreeSet::new();let mut count=0;
    let mut metadata=std::collections::hash_map::DefaultHasher::new();
    // Common installed dependency directories; names are infrastructure, not
    // project terminology. External symlink targets contribute metadata only.
    for name in ["node_modules","vendor",".venv","venv"] {
        metadata_tree(&tree.join(name),&mut visited,&mut count,&mut metadata,start)?;
    }
    digest.update(&metadata.finish().to_le_bytes());
    Ok(digest.hex_digest())
}
}
fn stamp(meta:&std::fs::Metadata)->String {
    #[cfg(unix)] {use std::os::unix::fs::MetadataExt;format!("{}:{:?}:{}:{}:{}:{}",meta.len(),meta.modified().ok(),meta.dev(),meta.ino(),meta.ctime(),meta.ctime_nsec())}
    #[cfg(not(unix))] {format!("{}:{:?}:{:?}",meta.len(),meta.modified().ok(),meta.created().ok())}
}
fn metadata_tree(path:&Path,visited:&mut BTreeSet<PathBuf>,count:&mut usize,digest:&mut impl Hasher,start:Instant)->Result<(),String> {
    if !path.exists() {path.hash(digest);"missing".hash(digest);return Ok(());}
    if *count>=100_000 || start.elapsed()>Duration::from_secs(6) {return Err("lsp-dependency-inventory-incomplete".into());}
    *count+=1;
    let meta=std::fs::metadata(path).map_err(|e|e.to_string())?;
    path.hash(digest);meta.len().hash(digest);meta.modified().ok().hash(digest);
    #[cfg(unix)] {use std::os::unix::fs::MetadataExt;meta.ctime().hash(digest);meta.ctime_nsec().hash(digest);meta.ino().hash(digest);}
    if meta.is_dir() {
        let canonical=path.canonicalize().map_err(|e|e.to_string())?;
        if !visited.insert(canonical) {return Ok(());}
        let mut children=std::fs::read_dir(path).map_err(|e|e.to_string())?
            .map(|entry|entry.map(|e|e.path())).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        children.sort();
        for child in children {metadata_tree(&child,visited,count,digest,start)?;}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_source_config_new_files_and_dependency_changes_invalidate_reuse() {
        let dir=tempfile::tempdir().unwrap();let tree=dir.path();
        assert!(crate::platform::git::run(tree,&["init","-q"]).ok);
        std::fs::write(tree.join("mustard.json"),"{}").unwrap();
        std::fs::write(tree.join("a.ts"),"const a=1;").unwrap();
        let mut snapshot=Snapshot::default();
        let first=snapshot.fingerprint(tree).unwrap();
        assert_eq!(first,snapshot.fingerprint(tree).unwrap());
        std::fs::write(tree.join("a.ts"),"const a=2;").unwrap();
        let second=snapshot.fingerprint(tree).unwrap();assert_ne!(first,second);
        #[cfg(unix)] {
            let path=tree.join("a.ts");let modified=std::fs::metadata(&path).unwrap().modified().unwrap();
            std::fs::write(&path,"const a=3;").unwrap();
            std::fs::File::open(&path).unwrap().set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
            assert_ne!(second,snapshot.fingerprint(tree).unwrap(),"Restored mtime must not reuse the previous source digest");
        }
        std::fs::write(tree.join("tsconfig.json"),"{\"strict\":true}").unwrap();
        let config=snapshot.fingerprint(tree).unwrap();assert_ne!(second,config);
        std::fs::create_dir_all(tree.join("node_modules/package")).unwrap();
        std::fs::write(tree.join("node_modules/package/index.d.ts"),"declare const a:number").unwrap();
        let installed=snapshot.fingerprint(tree).unwrap();assert_ne!(config,installed);
        std::fs::write(tree.join("node_modules/package/index.d.ts"),"declare const a:string").unwrap();
        assert_ne!(installed,snapshot.fingerprint(tree).unwrap());
        std::fs::write(tree.join(".gitignore"),"ignored.ts\n").unwrap();
        std::fs::write(tree.join("ignored.ts"),"const a=1;").unwrap();snapshot.observe("ignored.ts");
        let ignored=snapshot.fingerprint(tree).unwrap();
        std::fs::write(tree.join("ignored.ts"),"const a=2;").unwrap();
        assert_ne!(ignored,snapshot.fingerprint(tree).unwrap());
    }
}
