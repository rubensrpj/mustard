//! Two full reads of the same tree give the same map, byte for byte. The
//! fixtures tree is rich enough to mine roles and conventions, which is where
//! an order taken from a hash map once named the same convention differently
//! on each read.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;
#[path = "support/model.rs"]
mod model;

use std::path::Path;

/// Uma leitura inteira de `root`, com o mapa gravado na pasta `out`: os bytes
/// do mapa, como estão no disco.
fn full_read(root: &Path, out: &Path) -> Vec<u8> {
    model::scan(root, out, &["--all"]);
    model::read_bytes(out)
}

#[test]
fn two_full_reads_of_the_same_tree_give_the_same_bytes() {
    let root = manifest_dir::manifest_dir().join("tests").join("fixtures");
    let temp = tempfile::Builder::new().prefix("scan-byte-stable-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let first = full_read(&root, &dir.join("first"));
    for round in 0..3 {
        let again = full_read(&root, &dir.join(format!("again-{round}")));
        assert!(first == again, "full read {round} gave different bytes");
    }
}
