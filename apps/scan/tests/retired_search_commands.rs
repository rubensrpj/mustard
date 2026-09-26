//! A busca antiga do scan não existe mais: `digest` e `feature-bundle` são
//! recusados como comando desconhecido, mesmo sobre um modelo válido, e o
//! `facts` sobre o mesmo modelo continua respondendo.

use std::path::Path;
use std::process::{Command, Output};

/// Roda o scan com `args` e devolve a saída inteira.
fn scan(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scan")).args(args).output().expect("run scan")
}

/// Um modelo válido, escrito pelo próprio scan sobre um projeto de um arquivo.
fn model_in(dir: &Path) -> String {
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("invoice.ts"), "export class Invoice {}\n").unwrap();
    let model = dir.join("grain.model.json");
    let out = scan(&["scan", project.to_str().unwrap(), "--out", model.to_str().unwrap(), "--json"]);
    assert!(out.status.success(), "scan: {}", String::from_utf8_lossy(&out.stderr));
    model.to_string_lossy().into_owned()
}

#[test]
fn the_old_search_commands_are_refused_as_unknown() {
    let temp = tempfile::Builder::new().prefix("scan-retired-").tempdir().unwrap();
    let model = model_in(temp.path());

    for args in [
        vec!["digest", model.as_str()],
        vec!["digest", model.as_str(), "--query", "invoice"],
        vec!["feature-bundle", model.as_str(), "--query", "invoice"],
    ] {
        let out = scan(&args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "`{}` must be refused: {stderr}", args.join(" "));
        assert!(
            stderr.contains(&format!("unrecognized subcommand '{}'", args[0])),
            "`{}` is refused as an unknown command: {stderr}",
            args[0]
        );
        assert!(out.stdout.is_empty(), "a refused command prints nothing on stdout");
    }

    // O mesmo modelo segue servindo ao comando que ficou.
    let facts = scan(&["facts", model.as_str()]);
    assert!(facts.status.success(), "facts: {}", String::from_utf8_lossy(&facts.stderr));
    let v: serde_json::Value = serde_json::from_slice(&facts.stdout).expect("facts prints JSON");
    assert_eq!(v["entities"], serde_json::json!(["Invoice"]), "facts: {v}");
}
