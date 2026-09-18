//! A lista de pendências, pelo binário: dois pedidos ao mesmo tempo, que só
//! um processo de verdade prova, e a raiz da lista dentro de um worktree.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn rt(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
    cmd.arg("run").args(args).arg("--root").arg(root).current_dir(root);
    cmd
}

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(root)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Dois pedidos gravados ao mesmo tempo, em dois processos, nunca ganham o
/// mesmo número: a lista fica com dois itens e dois números diferentes.
#[test]
fn two_pending_items_written_at_the_same_time_never_get_the_same_number() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), b"{}").expect("config");

    let children: Vec<std::process::Child> = (0..6)
        .map(|n| {
            rt(root, &["pending", "--add", "--title", &format!("Pendência {n}"), "--detail", "um detalhe"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn")
        })
        .collect();
    let mut ids: Vec<String> = Vec::new();
    for child in children {
        let out = child.wait_with_output().expect("wait");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        let report: Value = serde_json::from_slice(&out.stdout).expect("JSON");
        ids.push(report["id"].as_str().unwrap_or_default().to_string());
    }

    let unique: BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "dois pedidos ganharam o mesmo número: {ids:?}");

    let listed = rt(root, &["pending"]).output().expect("list");
    let report: Value = serde_json::from_slice(&listed.stdout).expect("JSON");
    let items = report["open"].as_array().cloned().unwrap_or_default();
    assert_eq!(items.len(), ids.len(), "toda gravação entrou na lista: {report}");
}

/// Dentro de um worktree ligado, sem `mustard.json` próprio, a lista continua
/// sendo a do checkout principal, e não a da pasta em que o processo está.
#[test]
fn inside_a_worktree_the_ledger_is_still_the_one_of_the_main_checkout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("principal");
    std::fs::create_dir_all(&main).expect("main");
    std::fs::write(main.join("a.txt"), b"um\n").expect("file");
    git(&main, &["init", "-q"]);
    git(&main, &["add", "-A"]);
    git(&main, &["commit", "-q", "-m", "semente"]);
    // A configuração fica fora do commit: assim o worktree nasce sem ela, que
    // é o caso em que a lista caía na pasta do processo.
    std::fs::write(main.join("mustard.json"), b"{}").expect("config");

    let tree = dir.path().join("arvore");
    git(&main, &["worktree", "add", "-q", "-b", "feature/x", tree.to_str().expect("path")]);
    assert!(!tree.join("mustard.json").is_file(), "o worktree não tem config própria");

    let added = rt(&tree, &["pending", "--add", "--title", "Nascida no worktree", "--detail", "um detalhe"])
        .output()
        .expect("add");
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stdout));

    assert!(
        main.join(".claude").join("pending").join("ledger.json").is_file(),
        "a lista ficou no checkout principal"
    );
    assert!(!tree.join(".claude").join("pending").exists(), "nada foi criado dentro do worktree");
}
