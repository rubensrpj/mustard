//! Two full reads of the same tree give the same map, byte for byte. The
//! fixtures tree is rich enough to mine roles and conventions, which is where
//! an order taken from a hash map once named the same convention differently
//! on each read.

use std::path::{Path, PathBuf};
use std::process::Command;

fn full_read(root: &Path, out: &Path) -> Vec<u8> {
    let run = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", root.to_str().unwrap(), "--out", out.to_str().unwrap(), "--all", "--json"])
        .output()
        .expect("run scan");
    assert!(run.status.success(), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    std::fs::read(out).expect("read map")
}

#[test]
fn two_full_reads_of_the_same_tree_give_the_same_bytes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures");
    let temp = tempfile::Builder::new().prefix("scan-byte-stable-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let first = full_read(&root, &dir.join("first.json"));
    for round in 0..3 {
        let again = full_read(&root, &dir.join(format!("again-{round}.json")));
        assert!(first == again, "full read {round} gave different bytes");
    }
}
