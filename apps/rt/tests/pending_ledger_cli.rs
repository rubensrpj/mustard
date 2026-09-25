//! A lista de pendências, pelo binário: dois pedidos ao mesmo tempo, que só
//! um processo de verdade prova, a raiz da lista dentro de um worktree e o
//! tamanho de cada resposta numa lista longa.

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

/// Roda `mustard-rt run pending` com `args` e devolve o JSON da resposta;
/// uma recusa derruba o teste.
fn pending(root: &Path, args: &[&str]) -> Value {
    let out = rt(root, &[&["pending"], args].concat()).output().expect("pending");
    assert!(out.status.success(), "pending {args:?}: {}", String::from_utf8_lossy(&out.stdout));
    serde_json::from_slice(&out.stdout).expect("JSON")
}

/// As chaves da resposta, em ordem alfabética.
fn keys(report: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = report.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
    keys.sort_unstable();
    keys
}

/// Uma lista longa, como a de um projeto em uso: 182 pendências, as 170
/// primeiras fechadas, a seguinte descartada, dez abertas recentes e a
/// última aberta e parada desde 2020.
fn a_long_ledger(root: &Path) {
    let items: Vec<Value> = (1..=182)
        .map(|n| {
            let mut item = serde_json::json!({
                "id": format!("P-{n}"),
                "title": format!("Pendência {n}"),
                "detail": "um detalhe combinado na conversa",
                "status": "open",
            });
            match n {
                1..=170 => {
                    item["status"] = "closed".into();
                    item["reason"] = "entregue".into();
                }
                171 => {
                    item["status"] = "dropped".into();
                    item["reason"] = "mudou o plano".into();
                }
                182 => item["created"] = "2020-01-01".into(),
                _ => {}
            }
            item
        })
        .collect();
    let ledger = root.join(".claude").join("pending");
    std::fs::create_dir_all(&ledger).expect("pending dir");
    std::fs::write(ledger.join("ledger.json"), serde_json::json!({ "items": items }).to_string()).expect("ledger");
}

/// Toda gravação responde só o que gravou, o caminho e a linha de contagem:
/// numa lista de 182 pendências, o `--add` devolve a pendência 183 sem as
/// listas, e o mesmo vale para o fechamento, a reabertura, a confirmação da
/// remoção (pelo `--drop` e pelo `--remove`) e o vencimento. As listas
/// inteiras saem só da listagem sem opção.
#[test]
fn every_pending_write_answers_without_the_whole_lists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), b"{}").expect("config");
    a_long_ledger(root);
    // As chaves de toda gravação, com as que a ação devolve, em ordem.
    let written = |extra: &[&'static str]| {
        let mut keys = [&["count_line", "ok", "path"][..], extra].concat();
        keys.sort_unstable();
        keys
    };

    let added = pending(root, &["--add", "--title", "Conferir o lint", "--detail", "O gancho de commit do cliente"]);
    assert_eq!(keys(&added), written(&["added", "id"]), "{added}");
    assert_eq!(added["id"], "P-183", "{added}");
    assert_eq!(added["path"], ".claude/pending/ledger.json", "{added}");
    assert!(added["count_line"].as_str().unwrap_or_default().contains(" 12 "), "twelve open: {added}");

    let closed = pending(root, &["--close", "P-172", "--reason", "feito"]);
    assert_eq!(keys(&closed), written(&["id", "status"]), "{closed}");

    let reopened = pending(root, &["--reopen", "P-171"]);
    assert_eq!(keys(&reopened), written(&["id", "reopened"]), "{reopened}");

    let expected = written(&["removed"]);
    let shown = pending(root, &["--drop", "P-173", "--reason", "mudou"]);
    let token = shown["token"].as_str().expect("the preview code").to_string();
    let dropped = pending(root, &["--drop", "P-173", "--reason", "mudou", "--confirm", &token]);
    assert_eq!(keys(&dropped), expected, "{dropped}");
    assert_eq!(dropped["removed"], serde_json::json!(["P-173"]), "{dropped}");
    let shown = pending(root, &["--remove", "--id", "P-174", "--reason", "mudou"]);
    let token = shown["token"].as_str().expect("the preview code").to_string();
    let removed = pending(root, &["--remove", "--id", "P-174", "--reason", "mudou", "--confirm", &token]);
    assert_eq!(keys(&removed), expected, "{removed}");

    pending(root, &["--stale"]);
    let expired = pending(root, &["--expire"]);
    assert_eq!(keys(&expired), written(&["expired", "kept"]), "{expired}");
    assert_eq!(expired["expired"], serde_json::json!(["P-182"]), "{expired}");

    let listed = pending(root, &[]);
    assert_eq!(keys(&listed), vec!["closed", "count_line", "ok", "open", "path"], "{listed}");
    assert_eq!(listed["open"].as_array().map(Vec::len), Some(9), "{listed}");
    assert_eq!(listed["closed"].as_array().map(Vec::len), Some(174), "{listed}");
}

/// A faxina responde só as pendências paradas e a pergunta, e a prévia da
/// remoção só os itens que sairiam, o código e o texto para o usuário: nem
/// o caminho, nem a contagem, nem as listas aberta e fechada.
#[test]
fn stale_and_the_removal_preview_answer_only_their_own_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), b"{}").expect("config");
    a_long_ledger(root);

    let swept = pending(root, &["--stale"]);
    assert_eq!(keys(&swept), vec!["ok", "question", "stale"], "{swept}");
    let stale: Vec<&str> = swept["stale"].as_array().into_iter().flatten().filter_map(|i| i["id"].as_str()).collect();
    assert_eq!(stale, vec!["P-182"], "only the idle one: {swept}");
    let again = pending(root, &["--stale"]);
    assert_eq!(keys(&again), vec!["ok", "stale"], "nothing left to ask: {again}");
    assert_eq!(again["stale"], serde_json::json!([]), "{again}");

    for args in [
        &["--remove", "--id", "P-175", "--reason", "mudou"][..],
        &["--drop", "P-175", "--reason", "mudou"][..],
    ] {
        let shown = pending(root, args);
        assert_eq!(keys(&shown), vec!["hint", "ok", "preview", "remove", "token"], "{args:?}: {shown}");
        assert_eq!(shown["remove"], serde_json::json!([{ "id": "P-175", "title": "Pendência 175" }]), "{shown}");
    }
    let listed = pending(root, &[]);
    assert_eq!(listed["open"].as_array().map(Vec::len), Some(11), "the preview removed nothing: {listed}");
}
