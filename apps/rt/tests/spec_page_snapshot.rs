//! A página e o `.md` de uma spec saem do `spec.ndjson` pelo binário, iguais
//! byte a byte a cada vez, em qualquer pasta, e iguais ao retrato guardado em
//! `tests/fixtures/spec_page/`. O arquivo de eventos do retrato tem os 33
//! tipos, uma decisão revista, um item removido e uma mensagem expurgada.
//!
//! Depois de uma mudança de propósito na página, refaça o retrato rodando este
//! teste com `MUSTARD_BLESS=1` e confira a diferença no git.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spec_page").join(name)
}

/// Um projeto novo com a spec `demo` do retrato; devolve o `.md` e o `.html`
/// que o `page --spec` gerou, duas vezes seguidas.
fn generate(root: &Path) -> [(String, String); 2] {
    fs::write(root.join("mustard.json"), r#"{"specLang":"pt-BR"}"#).expect("config");
    let spec = root.join(".claude").join("spec").join("demo");
    fs::create_dir_all(&spec).expect("spec dir");
    fs::copy(fixture("spec.ndjson"), spec.join("spec.ndjson")).expect("events");
    [0, 1].map(|_| {
        let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "page", "--spec", "demo", "--root"])
            .arg(root)
            .current_dir(root)
            .output()
            .expect("run page");
        let report: Value = serde_json::from_slice(&out.stdout).expect("a JSON report");
        assert!(out.status.success(), "{report}");
        assert_eq!(report["md"], ".claude/spec/demo/spec.md", "{report}");
        assert_eq!(report["html"], ".claude/spec/demo/spec.html", "{report}");
        let read = |name: &str| fs::read_to_string(spec.join(name)).expect(name);
        (read("spec.md"), read("spec.html"))
    })
}

fn matches_the_snapshot(name: &str, got: &str) {
    let path = fixture(name);
    if std::env::var_os("MUSTARD_BLESS").is_some() {
        fs::write(&path, got).expect("bless");
        return;
    }
    let want = fs::read_to_string(&path).unwrap_or_default();
    assert!(want == got, "{name} changed; rerun with MUSTARD_BLESS=1 if it was on purpose\n{got}");
}

#[test]
fn the_spec_md_and_page_are_the_same_bytes_every_time() {
    let events = fs::read_to_string(fixture("spec.ndjson")).expect("fixture");
    let types: BTreeSet<String> = events
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["type"].as_str().map(str::to_string))
        .collect();
    assert_eq!(types.len(), 33, "the fixture holds every event type: {types:?}");

    let (one, two) = (tempfile::tempdir().expect("tempdir"), tempfile::tempdir().expect("tempdir"));
    let [first, again] = generate(one.path());
    let [elsewhere, _] = generate(two.path());
    assert!(first == again, "two runs over the same events differ");
    assert!(first == elsewhere, "the folder changed the output");

    let (md, html) = first;
    for page in [&md, &html] {
        let folder = one.path().to_string_lossy();
        assert!(!page.contains(folder.as_ref()), "a machine path leaked into the page");
    }
    matches_the_snapshot("spec.md", &md);
    matches_the_snapshot("spec.html", &html);
}
