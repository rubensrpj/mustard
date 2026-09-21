//! O arquivo de eventos da spec pelo binário de verdade.
//!
//! Duas gravações ao mesmo tempo, em dois processos, recebem números seguidos
//! e nenhuma linha sai estragada: a trava é a do sistema, a mesma no Linux, no
//! macOS e no Windows. Uma spec gravada pelo `write` é lida bloco a bloco pelo
//! `read`, e `read wave-2` devolve só a onda 2. O que só os ganchos e a rodada
//! gravam — a fala do usuário, o que uma onda entregou — entra aqui pela
//! gravação do núcleo, e o `write` recusa os dois.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

fn rt(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
    cmd.arg("run").args(args).arg("--root").arg(root).current_dir(root);
    cmd
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

fn write(root: &Path, event_type: &str, fields: &Value) -> u64 {
    let out = rt(root, &["write", event_type, "--spec", "teste", "--json", &fields.to_string()])
        .output()
        .expect("run write");
    assert!(out.status.success(), "write {event_type}: {}", String::from_utf8_lossy(&out.stdout));
    stdout_json(&out)["id"].as_u64().expect("the write reports its number")
}

/// A spec já aberta: o arquivo de eventos existe antes de qualquer gravação,
/// como o comando que abre a spec o deixa. Sem ele, o `write` recusa e manda
/// abrir a spec.
fn open_spec(root: &Path) {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
    if !path.exists() {
        std::fs::File::create(&path).expect("the event file");
    }
}

/// O `state` da spec, pela gravação do núcleo: o `run write` não grava o
/// estado, que é dos comandos do fluxo e da testemunha.
fn seed_state(root: &Path, fields: &Value) {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
    let draft = fields.as_object().cloned().expect("an object");
    mustard_core::io::spec_events::write(&path, "state", draft, &[]).expect("state");
}

/// Um evento que só o binário grava, pela gravação do núcleo: o `run write`
/// recusa a execução dos critérios, o veredito, o que uma onda entregou e a
/// fala do usuário. Devolve o número dele.
fn seed_binary(root: &Path, event_type: &str, fields: &Value) -> u64 {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
    let draft = fields.as_object().cloned().expect("an object");
    mustard_core::io::spec_events::write(&path, event_type, draft, &[]).expect(event_type).id
}

/// A spec aprovada com `waves` ondas soltas, cada uma com a sua tarefa e o
/// seu arquivo.
fn approved_with_waves(root: &Path, waves: u64) {
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let said = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = seed_binary(root, "criterion", &json!({"when": "a", "then": "b", "proof": "p", "origin": said}));
    for n in 1..=waves {
        seed_binary(root, "wave", &json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit],
            "done_when": "x", "origin": said}));
        seed_binary(root, "task", &json!({"wave": n, "text": format!("Tarefa {n}."),
            "files": [{"path": format!("a{n}.rs")}], "origin": said}));
    }
    seed_state(root, &json!({"author": "user", "phase": "approved",
        "witness": {"question": "Aprovar esta spec?", "answer": "Aprovar"}}));
}

fn read(root: &Path, block: &str) -> Value {
    let out = rt(root, &["read", block, "--spec", "teste"]).output().expect("run read");
    assert!(out.status.success(), "read {block}: {}", String::from_utf8_lossy(&out.stdout));
    stdout_json(&out)
}

#[test]
fn two_processes_writing_at_once_get_consecutive_numbers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let rounds = 10;
    for round in 0..rounds {
        let writers: Vec<_> = (0..2)
            .map(|w| {
                let fields = json!({"text": format!("rodada {round}, gravação {w}")});
                rt(root, &["write", "message", "--spec", "teste", "--json", &fields.to_string()])
                    .stdout(Stdio::piped())
                    .spawn()
                    .expect("spawn write")
            })
            .collect();
        for writer in writers {
            let out = writer.wait_with_output().expect("wait write");
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        }
    }

    let raw = std::fs::read_to_string(root.join(".claude").join("spec").join("teste").join("spec.ndjson"))
        .expect("the spec file exists");
    let ids: Vec<u64> = raw
        .lines()
        .map(|line| {
            let event: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("torn line ({e}): {line}"));
            event["id"].as_u64().expect("id")
        })
        .collect();
    assert_eq!(ids, (1..=2 * rounds).collect::<Vec<u64>>(), "consecutive, in file order, none repeated");
}

/// A cópia para o banco da página é preparada inteira dentro da trava do
/// arquivo de eventos: depois de dois fins de onda gravados ao mesmo tempo,
/// por duas rodadas em dois processos, a cópia que ficou tem os dois itens, e
/// cada lote aponta só arquivos que estão lá, sem sobra de outra rodada.
#[test]
fn two_processes_closing_a_wave_at_once_leave_both_items_in_the_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let spec = root.join(".claude").join("spec").join("teste");
    let rounds: u64 = 5;
    approved_with_waves(root, 2 * rounds);
    // Cada onda entrega o próprio arquivo, e cada rodada faz o commit dele.
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\n").expect("exclude");
    for n in 1..=2 * rounds {
        std::fs::write(root.join(format!("a{n}.rs")), "fn um() {}\n").expect("seed file");
    }
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "semente"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    git(&["config", "commit.gpgsign", "false"]);
    for round in 0..rounds {
        let texts: Vec<String> = (0..2).map(|w| format!("rodada {round} escrita {w}")).collect();
        let writers: Vec<_> = texts
            .iter()
            .zip(0u64..)
            .map(|(text, w)| {
                let wave = 2 * round + w + 1;
                let file = format!("a{wave}.rs");
                std::fs::write(root.join(&file), format!("fn um() {{}}\n// {text}\n")).expect("the wave's change");
                let line = json!({"wave": wave, "text": text, "files": [file], "commit": format!("a onda {wave} sai")});
                let report = format!("<DELIVERED>{line}</DELIVERED>");
                rt(root, &["round", "--spec", "teste", "--report", &report])
                    .stdout(Stdio::piped())
                    .spawn()
                    .expect("spawn round")
            })
            .collect();
        for writer in writers {
            let out = writer.wait_with_output().expect("wait write");
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
            assert!(stdout_json(&out).get("warnings").is_none(), "{}", String::from_utf8_lossy(&out.stdout));
        }
        let copied = copied_items(root, &spec.join("copy"));
        for text in &texts {
            assert!(copied.iter().any(|item| item["text"] == json!(text)), "round {round}: the copy lacks {text}");
        }
        for page in ["spec.md", "spec.html"] {
            assert!(!spec.join(page).exists(), "round {round}: no {page} is written");
        }
    }
}

/// Os itens que a cópia em `folder` manda para o banco, lidos como a
/// ferramenta do banco os lê: de cada lote `spec-<n>.json`, cada escrita da
/// coleção das faixas pelo arquivo dela, com os itens dela abertos. Cada
/// arquivo apontado existe, e cada arquivo de faixa da pasta é apontado por
/// um lote: a pasta é de uma cópia só.
fn copied_items(root: &std::path::Path, folder: &std::path::Path) -> Vec<Value> {
    let mut batches: Vec<std::path::PathBuf> = std::fs::read_dir(folder)
        .expect("the copy folder")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("spec-")))
        .collect();
    batches.sort();
    let mut pointed: Vec<std::path::PathBuf> = Vec::new();
    let mut items = Vec::new();
    for (n, batch) in batches.iter().enumerate() {
        let name = batch.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        assert_eq!(name, format!("spec-{}.json", n + 1), "{batches:?}");
        let writes: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(batch).expect("batch")).expect("json");
        for write in writes.iter().filter(|w| w["op"] == json!("set")) {
            let file = root.join(write["file_path"].as_str().expect("file_path"));
            let body = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            if write["collection"] == json!("ranges") {
                let range: Value = serde_json::from_str(&body).expect("range json");
                items.extend(range["items"].as_array().cloned().unwrap_or_default());
            }
            pointed.push(file);
        }
    }
    for entry in std::fs::read_dir(folder.join("ranges")).expect("ranges").flatten() {
        assert!(pointed.contains(&entry.path()), "{} is left over from another copy", entry.path().display());
    }
    items
}

#[test]
fn a_spec_written_by_the_cli_is_read_block_by_block_and_wave_2_is_only_wave_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "survey", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "Revise tudo"}));
    write(root, "context", &json!({"text": "Revise tudo", "origin": msg}));
    let c1 = write(root, "criterion", &json!({"when": "a", "then": "b", "proof": "p", "origin": msg}));
    let c2 = write(root, "criterion", &json!({"when": "c", "then": "d", "proof": "q", "origin": msg}));
    write(root, "wave", &json!({"n": 1, "text": "Um.", "criteria": [c1], "done_when": "x", "origin": msg}));
    write(root, "task", &json!({"wave": 1, "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": msg}));
    write(root, "wave", &json!({"n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "depends_on": [1], "origin": msg}));
    write(root, "task", &json!({"wave": 2, "text": "T2.", "files": [{"path": "b.rs"}], "depends_on": [], "origin": msg}));
    seed_binary(root, "delivered", &json!({"author": "wave", "wave": 2, "text": "Feito.", "files": ["b.rs"]}));
    seed_binary(root, "verdict", &json!({"author": "review", "wave": 2, "result": "approved", "text": "Sem achados.", "criteria": [{"criterion": c2, "tests_rule": true}]}));

    let wave2 = read(root, "wave-2");
    let events = wave2["events"].as_array().expect("events");
    assert_eq!(wave2["count"], json!(3), "{wave2}");
    for event in events {
        let n = event.get("n").or_else(|| event.get("wave")).and_then(Value::as_u64);
        assert_eq!(n, Some(2), "{event}");
        assert!(event.get("search").is_none(), "the search field is never shown: {event}");
    }
    assert_eq!(read(root, "state")["count"], json!(1));
    assert_eq!(read(root, "criteria")["count"], json!(2));
    assert_eq!(read(root, "review")["count"], json!(1));
    assert_eq!(read(root, "conversation")["count"], json!(1));

    // Refusals leave with exit 1 and say what is wrong.
    let unknown = rt(root, &["write", "licao", "--spec", "teste", "--json", "{}"]).output().expect("run");
    assert_eq!(unknown.status.code(), Some(1));
    assert_eq!(stdout_json(&unknown)["reason"], json!("unknown-type"));
    let verdict = json!({"author": "review", "wave": 2, "result": "rejected", "text": "t", "criteria": [{"criterion": c2, "tests_rule": false}]});
    let binary_only =
        rt(root, &["write", "verdict", "--spec", "teste", "--json", &verdict.to_string()]).output().expect("run");
    assert_eq!(binary_only.status.code(), Some(1));
    assert_eq!(stdout_json(&binary_only)["reason"], json!("binary-only-type"));
    let delivered = json!({"wave": 1, "text": "Pronta.", "files": ["a.rs"]}).to_string();
    let by_hand = rt(root, &["write", "delivered", "--spec", "teste", "--json", &delivered]).output().expect("run");
    assert_eq!(by_hand.status.code(), Some(1));
    assert_eq!(stdout_json(&by_hand)["reason"], json!("binary-only-type"));
    let click = json!({"author": "user", "text": "Aceitar", "witness": {"question": "Seguir?", "answer": "Aceitar"}});
    let forged = rt(root, &["write", "message", "--spec", "teste", "--json", &click.to_string()]).output().expect("run");
    assert_eq!(forged.status.code(), Some(1));
    assert_eq!(stdout_json(&forged)["reason"], json!("user-message-by-hook"));
    // A fala digitada do usuário chega só pelo gancho da entrada: o `write`
    // recusa gravá-la, revê-la e tirá-la.
    for (event_type, body) in [
        ("message", json!({"author": "user", "text": "pode seguir"})),
        ("message", json!({"author": "user", "text": "outra fala", "replaces": msg})),
        ("remove", json!({"targets": [msg], "reason": "engano"})),
    ] {
        let typed = rt(root, &["write", event_type, "--spec", "teste", "--json", &body.to_string()]).output().expect("run");
        assert_eq!(typed.status.code(), Some(1), "{body}");
        assert_eq!(stdout_json(&typed)["reason"], json!("user-message-by-hook"), "{body}");
    }
    assert_eq!(read(root, "conversation")["count"], json!(1));
    let fields = json!({"text": "t", "keys": ["k"], "origin": msg}).to_string();
    let missing = rt(root, &["write", "rule", "--spec", "teste", "--json", &fields]).output().expect("run");
    assert_eq!(missing.status.code(), Some(1));
    let refusal = stdout_json(&missing);
    assert_eq!(refusal["reason"], json!("missing-field"));
    assert!(refusal["hint"].as_str().unwrap_or_default().contains("example"), "{refusal}");
    let block = rt(root, &["read", "everything", "--spec", "teste"]).output().expect("run");
    assert_eq!(block.status.code(), Some(1));
    assert_eq!(stdout_json(&block)["reason"], json!("unknown-block"));
}

/// O commit da rodada nunca depende da lista de arquivos que a onda
/// declara: ele lê da cópia dela o que mudou de fato. A onda 1 muda o
/// arquivo que declarou e outro que não citou, o repositório continua
/// compilando, e o commit leva os dois, com um aviso da divergência. A onda
/// 2 deixa o comando de compilação quebrado, e a rodada não comita nada
/// dela: o repositório volta ao que era, e a recusa mostra o erro.
#[test]
fn a_wave_that_still_builds_commits_the_undeclared_file_and_one_that_breaks_the_build_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    let head = || {
        let out = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().expect("git rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let said = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = seed_binary(root, "criterion", &json!({"when": "a", "then": "b", "proof": "p", "origin": said}));
    for (n, files) in [(1u64, ["a1.rs"].as_slice()), (2u64, ["Makefile"].as_slice())] {
        seed_binary(root, "wave", &json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit],
            "done_when": "x", "origin": said}));
        let declared: Vec<Value> = files.iter().map(|f| json!({"path": f})).collect();
        seed_binary(root, "task", &json!({"wave": n, "text": format!("Tarefa {n}."), "files": declared, "origin": said}));
    }
    seed_state(root, &json!({"author": "user", "phase": "approved",
        "witness": {"question": "Aprovar esta spec?", "answer": "Aprovar"}}));

    std::fs::write(root.join("mustard.json"), br#"{"buildCommand":"make"}"#).expect("mustard.json");
    std::fs::write(root.join("a1.rs"), "fn um() {}\n").expect("a1.rs");
    std::fs::write(root.join("Makefile"), "default:\n\t@true\n").expect("Makefile");
    git(&["init", "-q"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\n").expect("exclude");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "semente"]);

    // Despacha as duas ondas: cria a cópia separada de cada uma.
    let dispatch = rt(root, &["round", "--spec", "teste"]).output().expect("dispatch");
    assert!(dispatch.status.success(), "{}", String::from_utf8_lossy(&dispatch.stdout));

    let copy = |wave: u64| mustard_core::io::wave_prompt::copy_path(root, "teste", wave, false);

    // Onda 1: muda o arquivo declarado e um outro que a entrega não cita; o
    // repositório continua compilando com o Makefile que já está lá.
    std::fs::write(copy(1).join("a1.rs"), "fn um() {}\n// muda\n").expect("a1 muda");
    std::fs::write(copy(1).join("extra.rs"), "fn extra() {}\n").expect("extra");
    let one = json!({"wave": 1, "text": "Saiu.", "files": ["a1.rs"], "commit": "a1 sai"});
    let report_one = format!("<DELIVERED>{one}</DELIVERED>");
    let out = rt(root, &["round", "--spec", "teste", "--report", &report_one]).output().expect("round 1");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let body = stdout_json(&out);
    assert_eq!(body["ok"], json!(true), "{body}");
    assert!(body["commit"]["sha"].as_str().is_some(), "{body}");
    assert_eq!(
        std::fs::read_to_string(root.join("extra.rs")).expect("o arquivo nao citado"),
        "fn extra() {}\n",
        "o arquivo que a onda não citou entra no commit quando o repositório compila"
    );
    let hint = mustard_core::platform::i18n::translate("round.files_diverged", mustard_core::platform::i18n::Locale::PtBr)
        .replace("{wave}", "1")
        .replace("{changed}", "2")
        .replace("{declared}", "1")
        .replace("{missing}", "extra.rs");
    assert_eq!(body["warnings"], json!([{"reason": "files-diverged", "wave": 1, "hint": hint}]), "{body}");

    // Onda 2: a cópia dela deixa o comando de compilação quebrado.
    let before = head();
    std::fs::write(copy(2).join("Makefile"), "default:\n\texit 1\n").expect("Makefile quebrado");
    let two = json!({"wave": 2, "text": "Saiu.", "files": ["Makefile"], "commit": "makefile sai"});
    let report_two = format!("<DELIVERED>{two}</DELIVERED>");
    let out = rt(root, &["round", "--spec", "teste", "--report", &report_two]).output().expect("round 2");
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stdout));
    let body = stdout_json(&out);
    assert_eq!(body["reason"], json!("round-build-failed"), "{body}");
    assert_eq!(head(), before, "nada foi comitado com o repositório quebrado");
    assert_eq!(
        std::fs::read_to_string(root.join("Makefile")).expect("o Makefile do principal"),
        "default:\n\t@true\n",
        "o repositório principal volta ao que era: nada da onda 2 entrou"
    );
}

fn index_file(root: &Path) -> std::path::PathBuf {
    root.join(".claude").join("spec").join("index.ndjson")
}

/// Cada gravação deixa a linha da spec no índice; apagado o índice, o
/// `index` pelo binário devolve o arquivo com os mesmos bytes.
#[test]
fn the_index_command_rebuilds_the_same_bytes_after_the_file_is_deleted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "survey", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "Deixar o índice certo. Depois o resto."}));
    write(root, "context", &json!({"text": "Deixar o índice certo. Depois o resto.", "origin": msg}));
    write(root, "rule", &json!({"text": "**Uma linha por spec.** Com o objetivo.", "keys": ["índice"], "example": "e", "origin": msg}));
    let written = std::fs::read(index_file(root)).expect("the write left the index");
    let text = String::from_utf8_lossy(&written);
    assert!(text.contains("\"name\":\"teste\"") && text.contains("\"goal\":\"Deixar o índice certo.\""), "{text}");

    std::fs::remove_file(index_file(root)).expect("delete the index");
    let out = rt(root, &["index"]).output().expect("run index");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let report = stdout_json(&out);
    assert_eq!(report["index"], json!(".claude/spec/index.ndjson"), "{report}");
    assert_eq!(report["specs"], json!(1), "{report}");
    assert_eq!(std::fs::read(index_file(root)).expect("the index is back"), written);
}

/// Com o índice no lugar de uma pasta, o `index` recusa com exit 1, e o
/// `write` grava o evento mesmo assim, com o aviso.
#[test]
fn an_index_that_cannot_be_written_is_refused_with_exit_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    std::fs::create_dir_all(index_file(root)).expect("an index that is a folder");
    let fields = json!({"text": "fica gravado"}).to_string();
    let written = rt(root, &["write", "message", "--spec", "teste", "--json", &fields]).output().expect("run write");
    assert!(written.status.success(), "{}", String::from_utf8_lossy(&written.stdout));
    let warnings = stdout_json(&written)["warnings"].to_string();
    assert!(warnings.contains("mustard-rt run index"), "{warnings}");

    let out = rt(root, &["index"]).output().expect("run index");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_json(&out)["reason"], json!("io-failed"));
}
