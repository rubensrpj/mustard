//! O arquivo de eventos da spec pelo binário de verdade.
//!
//! Duas gravações ao mesmo tempo, em dois processos, recebem números seguidos
//! e nenhuma linha sai estragada: a trava é a do sistema, a mesma no Linux, no
//! macOS e no Windows. Uma spec gravada pelo `write` é lida bloco a bloco pelo
//! `read`, e `read wave-2` devolve só a onda 2. O que só os ganchos e a rodada
//! gravam — a fala do usuário, a entrega oficial de uma onda — entra aqui pela
//! gravação do núcleo. O `write` recusa a fala, e aceita a entrega só como a
//! volta da onda com o envio dela aberto.

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
/// seu arquivo. As ondas saem com o autor do programa, como a rodada as
/// grava ao montar os lotes.
fn approved_with_waves(root: &Path, waves: u64) {
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let said = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    // A prova precisa passar de verdade: a rodada agora roda o critério
    // coberto antes de comitar.
    let crit = seed_binary(root, "criterion",
        &json!({"when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": said}));
    for n in 1..=waves {
        seed_binary(root, "wave", &json!({"author": "binary", "n": n, "text": format!("Onda {n}."), "criteria": [crit],
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
/// arquivo de eventos: com duas voltas gravadas e duas rodadas rodando ao
/// mesmo tempo, em dois processos, a cópia que ficou tem os dois itens, e
/// cada lote aponta só arquivos que estão lá, sem sobra de outra rodada.
#[test]
fn two_processes_closing_a_wave_at_once_leave_both_items_in_the_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let spec = root.join(".claude").join("spec").join("teste");
    let rounds: u64 = 5;
    approved_with_waves(root, 2 * rounds);
    // Cada onda grava a volta com o próprio arquivo, e as duas rodadas
    // disputam o commit: a primeira a pegar a trava assume as duas voltas.
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
        let dispatch = rt(root, &["round", "--spec", "teste"]).output().expect("dispatch");
        assert!(dispatch.status.success(), "{}", String::from_utf8_lossy(&dispatch.stdout));
        let texts: Vec<String> = (0..2).map(|w| format!("rodada {round} escrita {w}")).collect();
        for (text, w) in texts.iter().zip(0u64..) {
            let wave = 2 * round + w + 1;
            let file = format!("a{wave}.rs");
            std::fs::write(root.join(&file), format!("fn um() {{}}\n// {text}\n")).expect("the wave's change");
            write(root, "delivered", &json!({"wave": wave, "text": text, "files": [file], "commit": format!("a onda {wave} sai")}));
        }
        let writers: Vec<_> = (0..2)
            .map(|_| rt(root, &["round", "--spec", "teste"]).stdout(Stdio::piped()).spawn().expect("spawn round"))
            .collect();
        for writer in writers {
            let out = writer.wait_with_output().expect("wait write");
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
            // Sem a linha de consumo, que só quem despacha escreve, a rodada
            // avisa; é de outro assunto, e aqui se olha o resto.
            let warned = stdout_json(&out)["warnings"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|w| w["reason"] != json!("usage-missing"))
                .count();
            assert_eq!(warned, 0, "{}", String::from_utf8_lossy(&out.stdout));
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
    let c1 = write(root, "criterion",
        &json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": msg}));
    let c2 = write(root, "criterion",
        &json!({"when": "c", "then": "d", "proof": "q", "form": "ubiquitous", "origin": msg}));
    // A onda nasce do backlog: as duas ondas e a tarefa da onda 2 entram como o
    // programa as grava ao montar os lotes, e a tarefa sem onda, pelo `write`.
    seed_binary(root, "wave", &json!({"author": "binary", "n": 1, "text": "Um.", "criteria": [c1], "done_when": "x", "origin": msg}));
    write(root, "task", &json!({"title": "Entregar o T1", "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": msg}));
    seed_binary(root, "wave", &json!({"author": "binary", "n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "origin": msg}));
    seed_binary(root, "task", &json!({"author": "binary", "wave": 2, "text": "T2.", "files": [{"path": "b.rs"}], "depends_on": [], "origin": msg}));
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
    // O veredito entra só como a volta do revisor com o pedido de revisão
    // aberto: nenhum foi pedido, e a gravação é recusada.
    let verdict = json!({"author": "review", "wave": 2, "result": "rejected", "text": "t", "criteria": [{"criterion": c2, "tests_rule": false}]});
    let unasked =
        rt(root, &["write", "verdict", "--spec", "teste", "--json", &verdict.to_string()]).output().expect("run");
    assert_eq!(unasked.status.code(), Some(1));
    assert_eq!(stdout_json(&unasked)["reason"], json!("no-open-review"));
    // A entrega entra só como a volta da onda com o envio dela aberto: a onda
    // 1 nunca saiu, e a gravação é recusada sem gravar nada.
    let delivered = json!({"wave": 1, "text": "Pronta.", "files": ["a.rs"]}).to_string();
    let unsent = rt(root, &["write", "delivered", "--spec", "teste", "--json", &delivered]).output().expect("run");
    assert_eq!(unsent.status.code(), Some(1));
    assert_eq!(stdout_json(&unsent)["reason"], json!("no-open-send"));
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
    // A prova precisa passar de verdade: a rodada agora roda o critério
    // coberto antes de comitar.
    let crit = seed_binary(root, "criterion",
        &json!({"when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": said}));
    for (n, files) in [(1u64, ["a1.rs"].as_slice()), (2u64, ["Makefile"].as_slice())] {
        seed_binary(root, "wave", &json!({"author": "binary", "n": n, "text": format!("Onda {n}."), "criteria": [crit],
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
    // Quem comita a rodada é o binário, não o `git` deste teste: sem
    // identidade gravada no repositório temporário ele cai no nome do
    // sistema, que numa máquina de integração vem vazio e faz o git recusar.
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
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
    write(root, "delivered", &json!({"wave": 1, "text": "Saiu.", "files": ["a1.rs"], "commit": "a1 sai"}));
    let out = rt(root, &["round", "--spec", "teste"]).output().expect("round 1");
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
    let warned: Vec<serde_json::Value> = body["warnings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w["reason"] != json!("usage-missing"))
        .collect();
    assert_eq!(json!(warned), json!([{"reason": "files-diverged", "wave": 1, "hint": hint}]), "{body}");

    // Onda 2: a cópia dela deixa o comando de compilação quebrado.
    let before = head();
    std::fs::write(copy(2).join("Makefile"), "default:\n\texit 1\n").expect("Makefile quebrado");
    write(root, "delivered", &json!({"wave": 2, "text": "Saiu.", "files": ["Makefile"], "commit": "makefile sai"}));
    let out = rt(root, &["round", "--spec", "teste"]).output().expect("round 2");
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

/// O número de linhas do arquivo de eventos da spec `teste`.
fn event_lines(root: &Path) -> usize {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    std::fs::read_to_string(path).expect("the event file").lines().count()
}

/// Uma gravação pela linha de comando, devolvendo a saída e o JSON dela.
fn write_out(root: &Path, event_type: &str, fields: &Value) -> (Option<i32>, Value) {
    let out = rt(root, &["write", event_type, "--spec", "teste", "--json", &fields.to_string()])
        .output()
        .expect("run write");
    (out.status.code(), stdout_json(&out))
}

/// A onda nasce do backlog, e só o programa a grava. Pela linha de comando, a
/// onda é recusada sem gravar nada, com o texto que manda gravar só a
/// tarefa; a tarefa que traz um número de onda novo também. A versão nova de
/// uma tarefa que repete a onda da versão que ela substitui passa, e a
/// remoção de uma onda também. O item combinado sem dono não manda mais
/// dizer a onda: manda dar o dono pelos arquivos, e o dono pelos arquivos
/// passa.
#[test]
fn gravar_onda_a_mao_e_recusado_e_manda_gravar_so_a_tarefa() {
    use mustard_core::platform::i18n::{translate, Locale};
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = write(root, "criterion",
        &json!({"when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": msg}));
    let refusal_text = [Locale::PtBr, Locale::EnUs].map(|lang| translate("spec_events.wave_by_backlog", lang));

    // A onda gravada pelo modelo: recusada, e nada foi gravado.
    let before = event_lines(root);
    let (code, out) = write_out(root, "wave",
        &json!({"n": 1, "text": "Um.", "criteria": [crit], "done_when": "x", "origin": msg}));
    assert_eq!((code, &out["reason"]), (Some(1), &json!("wave-by-backlog")), "{out}");
    let hint = out["hint"].as_str().unwrap_or_default();
    assert!(refusal_text.contains(&hint), "{out}");
    assert!(hint.contains("backlog"), "{hint}");
    assert!(hint.contains("sem `wave`") || hint.contains("without `wave`"), "{hint}");
    assert_eq!(event_lines(root), before, "nada foi gravado");

    // A tarefa com um número de onda que nenhuma versão dela tinha: recusada.
    let (code, out) = write_out(root, "task",
        &json!({"wave": 1, "title": "Entregar o T1", "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": msg}));
    assert_eq!((code, &out["reason"]), (Some(1), &json!("wave-by-backlog")), "{out}");
    assert_eq!(event_lines(root), before, "nada foi gravado");

    // A onda e a tarefa dela como a rodada as grava ao montar o lote.
    let wave = seed_binary(root, "wave", &json!({"author": "binary", "n": 1, "text": "Um.", "criteria": [crit],
        "done_when": "x", "origin": msg}));
    let task = seed_binary(root, "task", &json!({"author": "binary", "wave": 1, "text": "T1.",
        "files": [{"path": "a.rs"}], "depends_on": [], "covers": [crit], "origin": msg}));

    // A versão nova da tarefa que repete a onda da versão revista passa; a
    // que troca a onda é recusada.
    let revised = write(root, "task", &json!({"wave": 1, "title": "Entregar o T1", "text": "T1, revista.", "files": [{"path": "a.rs"}],
        "depends_on": [], "covers": [crit], "origin": msg, "replaces": task}));
    let before = event_lines(root);
    let (code, out) = write_out(root, "task", &json!({"wave": 2, "title": "Entregar o T1", "text": "T1, noutra onda.",
        "files": [{"path": "a.rs"}], "depends_on": [], "origin": msg, "replaces": revised}));
    assert_eq!((code, &out["reason"]), (Some(1), &json!("wave-by-backlog")), "{out}");
    assert_eq!(event_lines(root), before, "nada foi gravado");

    // O item combinado novo, depois da aprovação, sem dono: a recusa manda dar
    // o dono pelos arquivos, e não mais dizer a onda.
    seed_state(root, &json!({"author": "user", "phase": "approved",
        "witness": {"question": "Aprovar esta spec?", "answer": "Aprovar"}}));
    let decision = |extra: Value| {
        let mut body = json!({"text": "Decidido.", "keys": ["d"], "why": "o usuário disse", "origin": msg});
        body.as_object_mut().expect("object").extend(extra.as_object().cloned().unwrap_or_default());
        body
    };
    let (code, out) = write_out(root, "decision", &decision(json!({})));
    assert_eq!((code, &out["reason"]), (Some(1), &json!("owner-missing")), "{out}");
    let hint = out["hint"].as_str().unwrap_or_default();
    assert!(!hint.contains("waves"), "{hint}");
    for lang in [Locale::PtBr, Locale::EnUs] {
        let text = translate("plan.owner_missing", lang);
        assert!(!text.contains("waves") && text.contains("applies_to"), "{text}");
    }
    let (code, out) = write_out(root, "decision", &decision(json!({"applies_to": {"files": ["a.rs"]}})));
    assert_eq!((code, &out["ok"]), (Some(0), &json!(true)), "o dono pelos arquivos da tarefa passa: {out}");

    // A remoção da onda continua valendo.
    write(root, "remove", &json!({"targets": [wave], "reason": "sai"}));
    let waves = read(root, "waves");
    let events = waves["events"].as_array().expect("events");
    assert!(events.iter().all(|e| e["type"] != json!("wave")), "{waves}");
}

/// A tarefa gravada pelo modelo traz um título curto, que diz o que ela
/// entrega. Sem ele, a gravação é recusada sem gravar nada, com o texto que
/// pede o título de até 70 caracteres; com 71 caracteres também. Com 70 (e
/// letras acentuadas, que contam uma cada), a tarefa é gravada. A versão
/// nova que o modelo grava sem título também é recusada.
#[test]
fn tarefa_sem_titulo_e_recusada_e_com_titulo_e_gravada() {
    use mustard_core::platform::i18n::{translate, Locale};
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let task = |title: Option<&str>, extra: Value| {
        let mut body = json!({"text": "Conferir cada critério no fechamento.", "files": [{"path": "a.rs"}],
            "depends_on": [], "origin": msg});
        if let Some(title) = title {
            body["title"] = json!(title);
        }
        body.as_object_mut().expect("object").extend(extra.as_object().cloned().unwrap_or_default());
        body
    };
    let labels = [Locale::PtBr, Locale::EnUs].map(|lang| translate("spec_events.task_declaration_title", lang));
    for lang in [Locale::PtBr, Locale::EnUs] {
        let label = translate("spec_events.task_declaration_title", lang);
        assert!(label.contains("70"), "o texto diz o tamanho: {label}");
    }
    let refused = |body: &Value, why: &str| {
        let before = event_lines(root);
        let (code, out) = write_out(root, "task", body);
        assert_eq!((code, &out["reason"]), (Some(1), &json!("task-declaration-missing")), "{why}: {out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(labels.iter().any(|label| hint.contains(label)), "{why}: a recusa pede o título: {hint}");
        assert_eq!(event_lines(root), before, "{why}: nada foi gravado");
    };

    refused(&task(None, json!({})), "sem título");
    refused(&task(Some("   "), json!({})), "título em branco");
    let long: String = "ç".repeat(71);
    refused(&task(Some(&long), json!({})), "71 caracteres");

    let exact: String = "ç".repeat(70);
    let (code, out) = write_out(root, "task", &task(Some(&exact), json!({})));
    assert_eq!((code, &out["ok"]), (Some(0), &json!(true)), "70 caracteres passam: {out}");
    let first = out["id"].as_u64().expect("o número da tarefa");
    let written = read(root, "waves");
    let saved = written["events"].as_array().expect("events").iter().find(|e| e["id"] == json!(first)).cloned();
    assert_eq!(saved.map(|e| e["title"].clone()), Some(json!(exact)), "o título fica gravado: {written}");

    // A versão nova pelo modelo, sem título: recusada.
    refused(&task(None, json!({"replaces": first})), "versão nova sem título");
    let (code, out) = write_out(root, "task", &task(Some("Fechamento confere cada critério"), json!({"replaces": first})));
    assert_eq!((code, &out["ok"]), (Some(0), &json!(true)), "a versão nova com título passa: {out}");
}

/// O item combinado com dono pelos arquivos, em `applies_to`, vai no pedido
/// da onda cuja tarefa toca um desses arquivos, sem passar pela análise antes
/// do envio, e fica fora do pedido da onda que não toca.
#[test]
fn o_item_com_dono_pelos_arquivos_vai_no_pedido_da_onda_que_toca_neles() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    approved_with_waves(root, 2);
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "e mais isto"}));
    let (code, out) = write_out(root, "decision", &json!({"text": "O arquivo a1 guarda só a conta.", "keys": ["conta"],
        "why": "o usuário disse", "origin": msg, "applies_to": {"files": ["a1.rs"]}}));
    assert_eq!((code, &out["ok"]), (Some(0), &json!(true)), "{out}");
    let decision = out["code"].as_str().expect("o código da decisão").to_string();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    std::fs::write(root.join("mustard.json"), b"{}").expect("mustard.json");
    std::fs::write(root.join("a1.rs"), "fn um() {}\n").expect("a1.rs");
    std::fs::write(root.join("a2.rs"), "fn dois() {}\n").expect("a2.rs");
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\n").expect("exclude");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "semente"]);

    let out = rt(root, &["round", "--spec", "teste"]).output().expect("dispatch");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let body = stdout_json(&out);
    assert!(body.get("analysis").is_none(), "o item com dono não pede a análise: {body}");
    let prompt = |wave: u64| {
        body["dispatch"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["wave"] == json!(wave))
            .and_then(|d| d["prompt"].as_str())
            .unwrap_or_else(|| panic!("a onda {wave} sai: {body}"))
            .to_string()
    };
    assert!(prompt(1).contains(&decision), "a onda que toca a1.rs leva a decisão: {}", prompt(1));
    assert!(!prompt(2).contains(&decision), "a onda que não toca fica sem ela: {}", prompt(2));
}
