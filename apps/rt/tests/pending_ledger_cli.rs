//! A lista de pendências, pelo binário: dois pedidos ao mesmo tempo, que só
//! um processo de verdade prova, a raiz da lista dentro de um worktree e o
//! tamanho de cada resposta numa lista longa.

#[path = "support/binary.rs"]
mod binary;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn rt(root: &Path, args: &[&str]) -> Command {
    let mut cmd = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"));
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
    assert_eq!(keys(&listed), vec!["closed_count", "count_line", "hint", "ok", "open", "path"], "{listed}");
    assert_eq!(listed["open"].as_array().map(Vec::len), Some(9), "{listed}");
    assert_eq!(listed["closed_count"], 174, "{listed}");
    let everything = pending(root, &["--all"]);
    assert_eq!(keys(&everything), vec!["closed", "count_line", "ok", "open", "path"], "{everything}");
    assert_eq!(everything["open"].as_array().map(Vec::len), Some(9), "{everything}");
    assert_eq!(everything["closed"].as_array().map(Vec::len), Some(174), "{everything}");
}

/// Uma lista com `open` abertas e `closed` fechadas, cada uma com um detalhe
/// e um motivo de quatro frases, como as de um projeto em uso. O título de
/// metade delas cita o painel e o da outra metade, o gancho. Três palavras
/// aparecem em um item só: `zarabatana` no título do 7º (fechado),
/// `alabastro` no detalhe do 9º (fechado) e `xilofone` no título do 3º aberto.
fn a_ledger_of(root: &Path, open: usize, closed: usize) {
    let detail = "O comando devolvia a lista inteira de uma vez, abertas e fechadas, e a conversa de quem lia \
                  enchia com o que ele não pediu. A busca por palavras sobre o título e o detalhe resolve o caso.";
    let items: Vec<Value> = (1..=closed + open)
        .map(|n| {
            let theme = if n % 2 == 0 { "painel" } else { "gancho" };
            let extra = match n {
                7 => " e a zarabatana",
                n if n == closed + 3 => " e o xilofone",
                _ => "",
            };
            let mut item = serde_json::json!({
                "id": format!("P-{n}"),
                "title": format!("Pendência {n} sobre o tema {theme}{extra}"),
                "detail": if n == 9 { format!("{detail} Cita o alabastro.") } else { detail.to_string() },
                "status": "open",
                "created": "2026-10-01",
            });
            if n <= closed {
                item["status"] = "closed".into();
                item["reason"] = "Entregue na obra anterior, com o teste que falha se o defeito voltar, e conferido pelo revisor.".into();
            }
            item
        })
        .collect();
    let ledger = root.join(".claude").join("pending");
    std::fs::create_dir_all(&ledger).expect("pending dir");
    std::fs::write(ledger.join("ledger.json"), serde_json::json!({ "items": items }).to_string()).expect("ledger");
}

/// A listagem sem opção traz as abertas inteiras e só a contagem das
/// fechadas: numa lista de 13 abertas e 300 fechadas, a resposta cabe numa
/// fração da de `--all`, que continua trazendo tudo.
#[test]
fn the_plain_listing_carries_the_open_items_and_only_the_count_of_the_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), b"{}").expect("config");
    a_ledger_of(root, 13, 300);
    let bytes = |args: &[&str]| {
        let out = rt(root, &[&["pending"], args].concat()).output().expect("pending");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        out.stdout.len()
    };

    let plain = pending(root, &[]);
    assert_eq!(plain["open"].as_array().map(Vec::len), Some(13), "{plain}");
    let whole = plain["open"].as_array().into_iter().flatten().all(|i| i["title"].is_string() && i["detail"].is_string() && i["created"].is_string());
    assert!(whole, "the open items come complete: {plain}");
    assert_eq!(plain["closed_count"], 300, "{plain}");
    assert!(plain.get("closed").is_none(), "{plain}");
    assert!(plain["hint"].as_str().is_some_and(|hint| hint.contains("300") && hint.contains("--term") && hint.contains("--all")), "{plain}");

    let everything = pending(root, &["--all"]);
    assert_eq!(everything["open"].as_array().map(Vec::len), Some(13), "{everything}");
    assert_eq!(everything["closed"].as_array().map(Vec::len), Some(300), "{everything}");

    let (plain_bytes, all_bytes) = (bytes(&[]), bytes(&["--all"]));
    eprintln!("pending bytes: plain {plain_bytes}, --all {all_bytes}");
    assert!(plain_bytes * 10 < all_bytes, "the plain answer is a fraction of the whole list: {plain_bytes} against {all_bytes}");
}

/// `--term` sem `--remove` busca no título e no detalhe, nas abertas e nas
/// fechadas, e responde até 30 achados com quantos casaram ao todo.
#[test]
fn a_term_alone_searches_every_item_and_caps_the_hits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), b"{}").expect("config");
    a_ledger_of(root, 13, 300);
    let ids = |report: &Value| -> Vec<String> {
        report["found"].as_array().into_iter().flatten().filter_map(|i| i["id"].as_str().map(str::to_string)).collect()
    };

    let by_title = pending(root, &["--term", "painel"]);
    assert_eq!(by_title["ok"], true, "{by_title}");
    assert_eq!(by_title["found"].as_array().map(Vec::len), Some(30), "{by_title}");
    assert_eq!(by_title["found_line"], "Achadas pelo termo: 30 de 156.", "{by_title}");
    assert!(by_title.get("open").is_none() && by_title.get("closed").is_none(), "{by_title}");
    assert!(by_title["found"][0]["status"].is_string() && by_title["found"][0]["detail"].is_string(), "{by_title}");

    // A busca olha o título de uma fechada e o de uma aberta, e o detalhe.
    let closed_one = pending(root, &["--term", "zarabatana"]);
    assert_eq!(ids(&closed_one), ["P-7"], "a closed item is searched by its title: {closed_one}");
    assert_eq!(closed_one["found_line"], "Achadas pelo termo: 1 de 1.", "{closed_one}");
    let open_one = pending(root, &["--term", "xilofone"]);
    assert_eq!(ids(&open_one), ["P-303"], "an open item is searched by its title: {open_one}");
    let by_detail = pending(root, &["--term", "alabastro"]);
    assert_eq!(ids(&by_detail), ["P-9"], "the detail is searched too: {by_detail}");

    let none = pending(root, &["--term", "inexistente"]);
    assert_eq!(none["found"], serde_json::json!([]), "{none}");
    assert_eq!(none["found_line"], "Achadas pelo termo: 0 de 0.", "{none}");
}

/// `--term` com `--remove` segue sendo o seletor da remoção: só as abertas
/// entram na prévia, e a lista não muda sem a confirmação.
#[test]
fn a_term_with_remove_still_selects_the_items_to_take_out() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), b"{}").expect("config");
    a_ledger_of(root, 13, 300);

    let shown = pending(root, &["--remove", "--term", "xilofone", "--reason", "mudou"]);
    assert_eq!(shown["preview"], true, "{shown}");
    assert_eq!(shown["remove"], serde_json::json!([{ "id": "P-303", "title": "Pendência 303 sobre o tema gancho e o xilofone" }]), "{shown}");
    assert!(shown.get("found").is_none(), "{shown}");
    let token = shown["token"].as_str().expect("the preview code").to_string();
    let removed = pending(root, &["--remove", "--term", "xilofone", "--reason", "mudou", "--confirm", &token]);
    assert_eq!(removed["removed"], serde_json::json!(["P-303"]), "{removed}");
    assert_eq!(pending(root, &[])["open"].as_array().map(Vec::len), Some(12));

    // A remoção só pega as abertas: o termo de uma fechada não casa nada.
    let closed_only = rt(root, &["pending", "--remove", "--term", "zarabatana", "--reason", "mudou"]).output().expect("pending");
    assert_eq!(closed_only.status.code(), Some(1), "{}", String::from_utf8_lossy(&closed_only.stdout));
}

/// `--all` e `--term` sozinhos não se misturam com outra ação: a chamada é
/// recusada com saída 1 e nada é gravado.
#[test]
fn a_search_or_all_beside_another_action_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), b"{}").expect("config");
    a_ledger_of(root, 2, 2);
    let before = std::fs::read(root.join(".claude").join("pending").join("ledger.json")).expect("ledger");
    for args in [
        &["--add", "--title", "T", "--detail", "D", "--term", "x"][..],
        &["--drop", "P-3", "--reason", "r", "--term", "x"][..],
        &["--all", "--term", "x"][..],
        &["--all", "--stale"][..],
        &["--term", "   "][..],
    ] {
        let out = rt(root, &[&["pending"], args].concat()).output().expect("pending");
        assert_eq!(out.status.code(), Some(1), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
        let report: Value = serde_json::from_slice(&out.stdout).expect("JSON");
        assert_eq!(report["ok"], false, "{args:?}: {report}");
    }
    let after = std::fs::read(root.join(".claude").join("pending").join("ledger.json")).expect("ledger");
    assert_eq!(after, before, "a refusal writes nothing");
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
