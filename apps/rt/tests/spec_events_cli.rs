//! O arquivo de eventos da spec pelo binário de verdade.
//!
//! Duas gravações ao mesmo tempo, em dois processos, recebem números seguidos
//! e nenhuma linha sai estragada: a trava é a do sistema, a mesma no Linux, no
//! macOS e no Windows. Uma spec gravada pelo `write` é lida bloco a bloco pelo
//! `read`, `read wave-2` devolve só a onda 2 e `read dispatch-2`, tudo o que o
//! pedido da onda 2 lista. O que só os ganchos e a rodada
//! gravam — a fala do usuário, a entrega oficial de uma onda — entra aqui pela
//! gravação do núcleo. O `write` recusa a fala, e aceita a entrega só como a
//! volta da onda com o envio dela aberto.

#[path = "support/binary.rs"]
mod binary;

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

#[path = "support/mod.rs"]
mod support;
#[path = "support/review.rs"]
mod wave_review;

fn rt(root: &Path, args: &[&str]) -> Command {
    let mut owned: Vec<String> = args.iter().map(|arg| (*arg).into()).collect();
    if matches!(args.first(), Some(&"round" | &"close")) {
        let spec = args.windows(2).find(|pair| pair[0] == "--spec").map(|pair| pair[1]).unwrap_or("teste");
        let previous = args.windows(2).find(|pair| pair[0] == "--report").map(|pair| pair[1]).unwrap_or_default();
        let report = wave_review::review_report(root, spec, previous, |wave| {
            let out = rt(root, &["review-wave", "--wave", &wave.to_string(), "--spec", spec]).output().expect("view diff");
            stdout_json(&out)
        });
        if !report.is_empty() {
            if let Some(at) = owned.iter().position(|arg| arg == "--report") { owned[at + 1] = report }
            else { owned.extend(["--report".into(), report]); }
        }
    }
    let mut cmd = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"));
    // O ambiente de quem roda a suíte não chega ao serviço do Jev: a rodada o
    // chamaria de verdade.
    cmd.arg("run")
        .args(&owned)
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("MUSTARD_JEV_URL");
    cmd
}

/// O pedido de um item que a rodada despachou, lido como o agente o lê: pelo
/// comando que a resposta traz no lugar do pedido, rodado pelo binário, com a
/// saída crua. O comando já traz a raiz, então não passa pelo `rt`.
fn request_by_command(root: &Path, entry: &Value) -> String {
    let command = entry["read"].as_str().unwrap_or_else(|| panic!("o item não traz o comando: {entry}"));
    assert!(entry.get("prompt").is_none(), "o pedido não vem inteiro na resposta: {entry}");
    let words: Vec<&str> = command.split_whitespace().collect();
    assert_eq!(words.first(), Some(&"mustard-rt"), "{command}");
    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(&words[1..])
        .current_dir(root)
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .output()
        .expect("o binário roda");
    assert!(out.status.success(), "{command}: {}", String::from_utf8_lossy(&out.stdout));
    String::from_utf8(out.stdout).expect("o pedido é texto")
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn repeated_options_return_one_json_error_before_any_item_is_written() {
    let dir=tempfile::tempdir().expect("tempdir");
    open_spec(dir.path());
    for args in [
        vec!["write","point","--spec","teste","--json","{}","--spec","teste"],
        vec!["write","point","--spec=teste","--spec=teste","--json","{}"],
    ] {
        let output=rt(dir.path(),&args).output().expect("CLI");
        assert_eq!(output.status.code(),Some(2));
        let error=stdout_json(&output);
        assert_eq!(error["reason"],"duplicate-option");
        assert!(error["hint"].as_str().expect("clear error").contains("--spec"));
    }
    let log=mustard_core::io::spec_events::read(&dir.path().join(".claude/spec/teste/spec.ndjson")).expect("read").expect("spec");
    assert!(log.events.is_empty(),"refusal measurements never change the canonical spec");
    let panel=stdout_json(&rt(dir.path(),&["panel","--spec","teste"]).output().expect("panel"));
    assert_eq!(panel["specs"][0]["operations"]["calls"],2);
    assert_eq!(panel["specs"][0]["operations"]["refusals"],2);
}

fn write(root: &Path, event_type: &str, fields: &Value) -> u64 {
    let out = rt(root, &["write", event_type, "--spec", "teste", "--json", &fields.to_string()])
        .output()
        .expect("run write");
    assert!(out.status.success(), "write {event_type}: {}", String::from_utf8_lossy(&out.stdout));
    stdout_json(&out)["id"].as_u64().expect("the write reports its number")
}

/// O agente da onda `wave` lê, de dentro da cópia e pelo comando que o pedido
/// ensina, cada item que o envio dela manda ler: sem isso a entrega é
/// recusada.
fn read_request(root: &Path, wave: u64) {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
    let sent = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "send" && e.wave() == Some(wave) && e.str_field("role") != Some("review"))
        .unwrap_or_else(|| panic!("no send for wave {wave}"));
    let copy = std::path::PathBuf::from(sent.str_field("copy").expect("the copy"));
    let listed = sent.fields.get("read_items").and_then(Value::as_array).cloned().unwrap_or_default();
    for item in listed.iter().filter_map(Value::as_str) {
        match item.strip_prefix("lesson-") {
            Some(number) => read_from(root, &copy, "lessons", Some(number)),
            None => read_from(root, &copy, &format!("item-{item}"), None),
        };
    }
}

/// O agente da onda `wave` grava, pelo comando que o pedido ensina, o passo de
/// término de cada tarefa dela: sem ele, a entrega que dá a tarefa como feita
/// é recusada.
fn finish_tasks(root: &Path, wave: u64) {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
    let codes = log.codes();
    for task in log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(wave)) {
        let item = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
        write(root, "step", &json!({"wave": wave, "item": item, "text": "A tarefa ficou pronta."}));
    }
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
/// grava ao montar os lotes. As cópias que a rodada criar saem no fim do
/// teste.
fn approved_with_waves(root: &Path, waves: u64) {
    support::copies_leave_with_the_test(root);
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let said = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
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

/// Uma gravação que esbarra no limite de tamanho de arquivo do processo (o
/// `ulimit -f`), com 40 bytes de folga, grava esses 40 e falha: o arquivo da
/// spec termina com os mesmos bytes de antes, o comando recusa, e a gravação
/// seguinte, sem o limite, leva o número que a falhada teria levado e deixa
/// todas as linhas inteiras.
#[cfg(target_os = "linux")]
#[test]
fn a_write_cut_by_the_size_limit_leaves_the_spec_file_as_it_was() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    write(root, "message", &json!({"text": "primeira"}));
    let path = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
    // O limite é de 64 KiB; a linha de enchimento deixa o arquivo 40 bytes abaixo dele.
    let limit = 64 * 1024;
    let used = std::fs::metadata(&path).expect("the spec file exists").len() as usize;
    let padding = format!("{}\n", "x".repeat(limit - 40 - used - 1));
    let mut file = std::fs::OpenOptions::new().append(true).open(&path).expect("open the spec file");
    std::io::Write::write_all(&mut file, padding.as_bytes()).expect("pad the spec file");
    drop(file);
    let before = std::fs::read(&path).expect("read the spec file");
    assert_eq!(before.len(), limit - 40);

    let fields = json!({"text": "segunda ".repeat(20)});
    let out = binary::isolated("bash")
        .args(["-c", "trap '' XFSZ; ulimit -f 64 && exec \"$0\" \"$@\""])
        .arg(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "write", "message", "--spec", "teste", "--json", &fields.to_string()])
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .output()
        .expect("run write under the size limit");
    assert!(!out.status.success(), "the write was refused: {}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(std::fs::read(&path).expect("read the spec file"), before, "same size, same bytes");

    let id = write(root, "message", &fields);
    assert_eq!(id, 2, "the next write takes the number the cut one would have taken");
    let raw = std::fs::read_to_string(&path).expect("read the spec file");
    let parsed = raw.lines().filter(|line| serde_json::from_str::<Value>(line).is_ok()).count();
    assert_eq!(parsed, 2, "both events are whole lines; the padding is the only other line");
    assert_eq!(raw.lines().count(), 3);
}

/// A cópia para o banco da página é preparada inteira dentro da trava do
/// arquivo de eventos: com duas voltas gravadas e duas rodadas rodando ao
/// mesmo tempo, em dois processos, a cópia que ficou tem os dois itens, e
/// cada lote aponta só arquivos que estão lá, sem sobra de outra rodada.
#[test]
fn two_processes_closing_a_wave_preserve_both_deliveries_without_automatic_publication() {
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
    // O checkout fica no ramo da spec, como a abertura da obra o deixa:
    // a rodada só comita e só abre cópia nele.
    git(&["init", "-q", "-b", "feature/teste"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\n").expect("exclude");
    for n in 1..=2 * rounds {
        std::fs::write(root.join(format!("a{n}.rs")), "fn one() {}\n").expect("seed file");
    }
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "semente"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    git(&["config", "commit.gpgsign", "false"]);
    for round in 0..rounds {
        let dispatch = rt(root, &["round", "--dispatch", "--spec", "teste"]).output().expect("dispatch");
        assert!(dispatch.status.success(), "{}", String::from_utf8_lossy(&dispatch.stdout));
        let texts: Vec<String> = (0..2).map(|w| format!("rodada {round} escrita {w}")).collect();
        for (text, w) in texts.iter().zip(0u64..) {
            let wave = 2 * round + w + 1;
            let file = format!("a{wave}.rs");
            std::fs::write(root.join(&file), format!("fn one() {{}}\n// {text}\n")).expect("the wave's change");
            read_request(root, wave);
            finish_tasks(root, wave);
            write(root, "delivered", &json!({"wave": wave, "text": text, "files": [file], "commit": format!("a entrega {wave} sai")}));
        }
        let writers: Vec<_> = (0..2)
            .map(|_| rt(root, &["round", "--dispatch", "--spec", "teste"]).stdout(Stdio::piped()).spawn().expect("spawn round"))
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
                .filter(|w| w["reason"] != "usage-missing")
                .count();
            assert_eq!(warned, 0, "{}", String::from_utf8_lossy(&out.stdout));
        }
        let history = std::fs::read_to_string(spec.join("spec.ndjson")).expect("canonical history");
        let items: Vec<Value> = history.lines().map(|line| serde_json::from_str(line).expect("canonical event")).collect();
        for text in &texts {
            assert!(items.iter().any(|item| item["type"] == "delivered" && item["text"] == json!(text)), "round {round}: canonical history lacks {text}");
        }
        assert!(!spec.join("copy").exists(), "concurrent round closures do not create external database batches");
        assert!(!root.join(".claude/mustard/publications").exists());
        for page in ["spec.md", "spec.html"] {
            assert!(!spec.join(page).exists(), "round {round}: no {page} is written");
        }
    }
}

/// A cópia que a rodada, pelo binário, cria para um projeto de teste sai no
/// fim do teste, também quando ele falha: a pasta das cópias do projeto não
/// sobra no disco.
#[test]
fn the_copies_a_test_makes_leave_when_it_ends_even_when_it_fails() {
    for fails in [false, true] {
        let (sent, made) = std::sync::mpsc::channel();
        let test = std::thread::spawn(move || {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path();
            approved_with_waves(root, 1);
            let git = |args: &[&str]| {
                let out = Command::new("git")
                    .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                    .args(args)
                    .current_dir(root)
                    .output()
                    .expect("git");
                assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            };
            // O checkout fica no ramo da spec, como a abertura da obra o deixa:
            // a rodada só comita e só abre cópia nele.
            git(&["init", "-q", "-b", "feature/teste"]);
            std::fs::write(root.join(".git/info/exclude"), ".claude/\n").expect("exclude");
            std::fs::write(root.join("a1.rs"), "fn one() {}\n").expect("seed file");
            git(&["add", "-A"]);
            git(&["commit", "-q", "-m", "semente"]);
            let dispatch = rt(root, &["round", "--dispatch", "--spec", "teste"]).output().expect("dispatch");
            assert!(dispatch.status.success(), "{}", String::from_utf8_lossy(&dispatch.stdout));
            let copy = mustard_core::io::wave_prompt::slot_path(root, "teste", 0);
            assert!(copy.join("a1.rs").is_file(), "the round made the copy: {}", String::from_utf8_lossy(&dispatch.stdout));
            sent.send(mustard_core::io::wave_prompt::copies_dir(root)).expect("send the copies folder");
            assert!(!fails, "the test fails on purpose");
        });
        assert_eq!(test.join().is_err(), fails);
        let copies = made.try_recv().expect("the copies folder");
        assert!(!copies.exists(), "fails={fails}: the copies folder stayed at {}", copies.display());
    }
}

#[test]
fn a_spec_written_by_the_cli_is_read_block_by_block_and_wave_2_is_only_wave_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "survey", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "Revise tudo"}));
    write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "Revise tudo", "origin": msg}));
    let c1 = write(root, "criterion",
        &json!({"title": "Combinar o item", "when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": msg}));
    let c2 = write(root, "criterion",
        &json!({"title": "Combinar o item", "when": "c", "then": "d", "proof": "echo q", "form": "ubiquitous", "origin": msg}));
    // A onda nasce do backlog: as duas ondas e a tarefa da onda 2 entram como o
    // programa as grava ao montar os lotes, e a tarefa sem onda, pelo `write`.
    seed_binary(root, "wave", &json!({"author": "binary", "n": 1, "text": "Um.", "criteria": [c1], "done_when": "x", "origin": msg}));
    write(root, "task", &json!({"agent": "- conferir pelo teste", "title": "Entregar o T1", "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "covers": [c1], "origin": msg}));
    seed_binary(root, "wave", &json!({"author": "binary", "n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "origin": msg}));
    seed_binary(root, "task", &json!({"author": "binary", "wave": 2, "text": "T2.", "files": [{"path": "b.rs"}], "depends_on": [], "origin": msg}));
    seed_binary(root, "delivered", &json!({"author": "wave", "wave": 2, "text": "Feito.", "files": ["b.rs"]}));
    seed_binary(root, "verdict", &json!({"author": "review", "wave": 2, "result": "approved", "text": "Sem achados.", "criteria": [{"criterion": c2}]}));

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
    let verdict = json!({"author": "review", "wave": 2, "result": "rejected", "text": "t", "criteria": [{"criterion": c2}]});
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
    let fields = json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "t", "keys": ["k"], "origin": msg}).to_string();
    let missing = rt(root, &["write", "rule", "--spec", "teste", "--json", &fields]).output().expect("run");
    assert_eq!(missing.status.code(), Some(1));
    let refusal = stdout_json(&missing);
    assert_eq!(refusal["reason"], json!("missing-field"));
    assert!(refusal["hint"].as_str().unwrap_or_default().contains("example"), "{refusal}");
    let block = rt(root, &["read", "everything", "--spec", "teste"]).output().expect("run");
    assert_eq!(block.status.code(), Some(1));
    assert_eq!(stdout_json(&block)["reason"], json!("unknown-block"));
}

/// Os códigos de item que um texto cita (`MSTD-<sigla>-<NNNN>`).
fn codes_in(text: &str) -> std::collections::BTreeSet<String> {
    text.match_indices("MSTD-")
        .map(|(at, _)| {
            let rest = &text[at..];
            let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(rest.len());
            rest[..end].to_string()
        })
        .filter(|code| code.rsplit('-').next().is_some_and(|n| n.len() == 4 && n.chars().all(|c| c.is_ascii_digit())))
        .collect()
}

/// O pedido da onda `wave` como a rodada o monta, e o que ele lista: os
/// códigos dos itens e os números das lições.
fn request_of(root: &Path, wave: u64) -> (String, std::collections::BTreeSet<String>, std::collections::BTreeSet<u64>) {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
    let built = mustard_core::io::wave_prompt::prompts(
        root,
        "teste",
        &log,
        mustard_core::platform::i18n::Locale::PtBr,
        &mustard_core::io::wave_prompt::Flight::default(),
        &std::collections::BTreeSet::from([wave]),
    );
    let text = built.into_iter().find(|p| p.wave == wave).expect("the request of the wave").text;
    let lessons = text
        .lines()
        .filter_map(|line| line.strip_prefix("- Lição "))
        .filter_map(|line| line.split_whitespace().next())
        .map(|n| n.parse().expect("a lesson number"))
        .collect();
    (text.clone(), codes_in(&text), lessons)
}

/// O que a leitura `dispatch-<n>` traz: os códigos dos itens da spec e os
/// números das lições, que não têm código.
fn dispatch_of(root: &Path, wave: u64, term: Option<&str>) -> (Value, std::collections::BTreeSet<String>, std::collections::BTreeSet<u64>) {
    let block = format!("dispatch-{wave}");
    let mut args = vec!["read", block.as_str(), "--spec", "teste"];
    if let Some(term) = term {
        args.extend(["--term", term]);
    }
    let out = rt(root, &args).output().expect("run read");
    assert!(out.status.success(), "read {block}: {}", String::from_utf8_lossy(&out.stdout));
    let report = stdout_json(&out);
    let events = report["events"].as_array().cloned().unwrap_or_default();
    let codes = events.iter().filter_map(|e| e["code"].as_str().map(str::to_string)).collect();
    let lessons = events.iter().filter(|e| e.get("code").is_none()).map(|e| e["id"].as_u64().expect("a lesson number")).collect();
    (report, codes, lessons)
}

/// O agente lê numa leitura só tudo o que o pedido da onda lista, e o
/// contexto que o pedido não lista: a onda, a tarefa, o critério dela, o que
/// ela atende, o combinado do arquivo que ela toca, a entrega da onda de que
/// ela depende e as lições, menos a que a escolha antes do envio tirou. Nada
/// do envio, da entrega nem dos passos da própria onda. No conserto, a
/// reprovação e a entrega que ela julgou vêm junto. Um código de item acha só
/// aquele item, mesmo que uma lição tenha o mesmo número dele no banco.
#[test]
fn the_dispatch_reading_returns_every_item_the_wave_request_lists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "Somar a fatura"}));
    let context = write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "A fatura soma centavos.", "origin": msg}));
    let c1 = write(root, "criterion", &json!({"title": "Combinar o item", "when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": msg}));
    let c2 = write(root, "criterion", &json!({"title": "Combinar o item", "when": "c", "then": "d", "proof": "echo q", "form": "ubiquitous", "origin": msg}));
    let mine = write(root, "decision", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "A soma arredonda no fim.", "why": "Centavos.", "keys": ["soma"],
        "applies_to": {"files": ["src/soma.rs"]}, "origin": msg}));
    let other = write(root, "decision", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "O outro arquivo guarda o histórico.", "why": "Auditoria.", "keys": ["outro"],
        "applies_to": {"files": ["src/outro.rs"]}, "origin": msg}));
    let w1 = seed_binary(root, "wave", &json!({"author": "binary", "n": 1, "text": "Um.", "criteria": [c1], "done_when": "x", "origin": msg}));
    seed_binary(root, "task", &json!({"author": "binary", "wave": 1, "text": "Guardar o histórico.",
        "files": [{"path": "src/outro.rs"}], "depends_on": [], "origin": msg}));
    let w2 = seed_binary(root, "wave", &json!({"author": "binary", "n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y",
        "depends_on": [1], "origin": msg}));
    let task = seed_binary(root, "task", &json!({"author": "binary", "wave": 2, "text": "Somar a fatura no total.",
        "files": [{"path": "src/soma.rs"}], "depends_on": [], "origin": msg}));
    let before = seed_binary(root, "delivered", &json!({"author": "wave", "wave": 1, "text": "Histórico guardado.", "files": ["src/outro.rs"]}));

    // As lições do banco: a do arquivo da onda, a que a escolha antes do
    // envio tira e a do arquivo da outra onda.
    let lesson = |keys: &[&str], file: &str, text: &str| -> u64 {
        let body = json!({"class": "environment_trap", "text": text, "keys": keys,
            "applies_to": {"files": [file]}, "found_in": {"spec": "teste"}});
        write(root, "lesson", &body)
    };
    let elsewhere = lesson(&["fatura"], "src/outro.rs", "A fatura antiga fica no histórico.");
    let removed = lesson(&["total"], "src/soma.rs", "O total passa pelo arredondamento.");
    let kept = lesson(&["fatura"], "src/soma.rs", "A fatura chega em centavos.");
    assert_eq!(kept, context, "the lesson that reaches the wave shares the number of the context item");

    let analysis = json!({"judged": [], "removed": [], "added": [], "judged_lessons": [kept, removed],
        "removed_lessons": [{"lesson": removed, "why": "A tarefa não arredonda."}], "tasks": []});
    let sent = seed_binary(root, "send", &json!({"author": "binary", "wave": 2, "role": "wave", "agent": "wave",
        "text": "o pedido", "lines": 1, "chars": 8, "mustard": "0", "analysis": analysis}));
    let step = seed_binary(root, "step", &json!({"author": "wave", "wave": 2, "item": task, "text": "Tarefa feita."}));
    let own = seed_binary(root, "delivered", &json!({"author": "wave", "wave": 2, "text": "Somado.", "files": ["src/soma.rs"]}));

    let code = |id: u64| -> String {
        let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
        let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
        log.codes().get(&id).cloned().unwrap_or_else(|| panic!("no code for {id}"))
    };

    let (request, listed, listed_lessons) = request_of(root, 2);
    let (report, read, lessons) = dispatch_of(root, 2, None);
    assert_eq!(report["block"], json!("dispatch-2"), "{report}");
    let expected: std::collections::BTreeSet<String> =
        [w2, task, c2, context, mine, before, msg].into_iter().map(code).collect();
    assert_eq!(read, expected, "{report}");
    assert!(listed.is_subset(&read), "the reading brings every item the request lists:\n{request}\n{report}");
    assert_eq!(
        listed,
        [task, mine, msg].into_iter().map(code).collect(),
        "the request lists the task, what it attends and the rule that reaches the file:\n{request}"
    );
    assert_eq!(lessons, [kept].into_iter().collect(), "{report}");
    assert_eq!(lessons, listed_lessons, "the reading and the request carry the same lessons:\n{request}\n{report}");
    for (what, id) in [("send", sent), ("step", step), ("own delivery", own), ("wave 1", w1), ("criterion of wave 1", c1), ("other file", other)] {
        assert!(!read.contains(&code(id)), "the {what} is not part of the request: {report}");
    }
    assert!(!lessons.contains(&removed) && !lessons.contains(&elsewhere), "{report}");
    for event in report["events"].as_array().expect("events") {
        assert!(event.get("search").is_none(), "the search field is never shown: {event}");
    }

    // Um código de item acha só aquele item, e nunca a lição que tem o
    // mesmo número dele no banco.
    for id in [context, task] {
        let (by_code, found, found_lessons) = dispatch_of(root, 2, Some(&code(id)));
        assert_eq!(found, [code(id)].into_iter().collect(), "{by_code}");
        assert!(found_lessons.is_empty(), "{by_code}");
    }
    // Uma palavra filtra dentro do que o pedido lista: a lição que a tem vem,
    // e o item que não a tem fica fora.
    let (by_word, found, found_lessons) = dispatch_of(root, 2, Some("fatura"));
    assert_eq!(found_lessons, [kept].into_iter().collect(), "{by_word}");
    assert!(!found.contains(&code(w2)), "{by_word}");
    // A onda que o plano não tem não tem pedido.
    assert_eq!(dispatch_of(root, 9, None).0["count"], json!(0));
    let bad = rt(root, &["read", "dispatch-x", "--spec", "teste"]).output().expect("run");
    assert_eq!(bad.status.code(), Some(1));
    assert_eq!(stdout_json(&bad)["reason"], json!("unknown-block"));

    // O conserto: a revisão reprovou a entrega, e o pedido novo cita a
    // reprovação e a entrega que ela julgou; a leitura as traz junto.
    let rejected = seed_binary(root, "verdict", &json!({"author": "review", "wave": 2, "result": "rejected", "text": "Falta o arredondamento.",
        "criteria": [{"criterion": c2}]}));
    seed_binary(root, "send", &json!({"author": "binary", "wave": 2, "role": "wave", "agent": "wave",
        "text": "o conserto", "lines": 1, "chars": 10, "mustard": "0", "analysis": analysis}));
    let (request, listed, _) = request_of(root, 2);
    let (report, read, _) = dispatch_of(root, 2, None);
    assert!(read.contains(&code(rejected)) && read.contains(&code(own)), "{report}");
    assert_eq!(
        listed,
        [task, mine, msg, rejected, own].into_iter().map(code).collect(),
        "the request of the fix lists the rejection and the delivery it judged:\n{request}"
    );
    assert!(listed.is_subset(&read), "the reading brings every item the request of the fix lists:\n{request}\n{report}");
}

/// O commit da rodada nunca depende da lista de arquivos que a onda
/// declara: ele lê da cópia dela o que mudou de fato. A onda 1 muda o
/// arquivo que declarou e outro que não citou, e o commit leva os dois,
/// com um aviso da divergência.
#[test]
fn the_round_commits_the_undeclared_file_the_wave_changed() {
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
    support::copies_leave_with_the_test(root);

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
    std::fs::write(root.join("a1.rs"), "fn one() {}\n").expect("a1.rs");
    std::fs::write(root.join("Makefile"), "default:\n\t@true\n").expect("Makefile");
    // O checkout fica no ramo da spec, como a abertura da obra o deixa:
    // a rodada só comita e só abre cópia nele.
    git(&["init", "-q", "-b", "feature/teste"]);
    // Quem comita a rodada é o binário, não o `git` deste teste: sem
    // identidade gravada no repositório temporário ele cai no nome do
    // sistema, que numa máquina de integração vem vazio e faz o git recusar.
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\n").expect("exclude");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "semente"]);

    // Despacha as duas ondas: cria a cópia separada de cada uma.
    let dispatch = rt(root, &["round", "--dispatch", "--spec", "teste"]).output().expect("dispatch");
    assert!(dispatch.status.success(), "{}", String::from_utf8_lossy(&dispatch.stdout));

    let copy = |wave: u64| mustard_core::io::wave_prompt::slot_path(root, "teste", usize::try_from(wave).expect("a wave number fits") - 1);

    // Onda 1: muda o arquivo declarado e um outro que a entrega não cita.
    std::fs::write(copy(1).join("a1.rs"), "fn one() {}\n// muda\n").expect("a1 muda");
    std::fs::write(copy(1).join("extra.rs"), "fn main() {}\n").expect("extra");
    read_request(root, 1);
    finish_tasks(root, 1);
    write(root, "delivered", &json!({"wave": 1, "text": "Saiu.", "files": ["a1.rs"], "commit": "a1 sai"}));
    let out = rt(root, &["round", "--dispatch", "--spec", "teste"]).output().expect("round 1");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let body = stdout_json(&out);
    assert_eq!(body["ok"], json!(true), "{body}");
    assert!(body["commit"]["sha"].as_str().is_some(), "{body}");
    assert_eq!(
        std::fs::read_to_string(root.join("extra.rs")).expect("o arquivo nao citado"),
        "fn main() {}\n",
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
        .filter(|w| w["reason"] != "usage-missing")
        .collect();
    assert_eq!(warned.len(), 2, "{body}");
    assert_eq!(warned[0], json!({"reason": "files-diverged", "wave": 1, "hint": hint}), "{body}");
    assert_eq!(warned[1]["reason"], "scope-expanded", "{body}");
    assert_eq!(warned[1]["files"], json!(["extra.rs"]), "{body}");
    assert_eq!(warned[1]["classification"], "necessity-unverified", "{body}");
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
    write(root, "context", &json!({"title": "Deixar o índice certo", "agent": "- conferir pelo teste", "text": "Deixar o índice certo. Depois o resto.", "origin": msg}));
    write(root, "rule", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "**Uma linha por spec.** Com o objetivo.", "keys": ["índice"], "example": "e", "origin": msg}));
    let written = std::fs::read(index_file(root)).expect("the write left the index");
    let text = String::from_utf8_lossy(&written);
    assert!(text.contains("\"name\":\"teste\"") && text.contains("\"goal\":\"Deixar o índice certo\""), "{text}");

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
fn writing_a_wave_by_hand_is_refused_and_says_to_write_only_the_task() {
    use mustard_core::platform::i18n::{Locale, translate};
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = write(root, "criterion",
        &json!({"title": "Combinar o item", "when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": msg}));
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
        &json!({"agent": "- conferir pelo teste", "wave": 1, "title": "Entregar o T1", "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "covers": [crit], "origin": msg}));
    assert_eq!((code, &out["reason"]), (Some(1), &json!("wave-by-backlog")), "{out}");
    assert_eq!(event_lines(root), before, "nada foi gravado");

    // A onda e a tarefa dela como a rodada as grava ao montar o lote.
    let wave = seed_binary(root, "wave", &json!({"author": "binary", "n": 1, "text": "Um.", "criteria": [crit],
        "done_when": "x", "origin": msg}));
    let task = seed_binary(root, "task", &json!({"author": "binary", "wave": 1, "text": "T1.",
        "files": [{"path": "a.rs"}], "depends_on": [], "covers": [crit], "origin": msg}));

    // A versão nova da tarefa que repete a onda da versão revista passa; a
    // que troca a onda é recusada.
    let revised = write(root, "task", &json!({"agent": "- conferir pelo teste", "wave": 1, "title": "Entregar o T1", "text": "T1, revista.", "files": [{"path": "a.rs"}],
        "depends_on": [], "covers": [crit], "origin": msg, "replaces": task}));
    let before = event_lines(root);
    let (code, out) = write_out(root, "task", &json!({"agent": "- conferir pelo teste", "wave": 2, "title": "Entregar o T1", "text": "T1, noutra onda.",
        "files": [{"path": "a.rs"}], "depends_on": [], "covers": [crit], "origin": msg, "replaces": revised}));
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
    let (code, out) = write_out(root, "decision", &decision(json!({"title": "Combinar o item", "agent": "- conferir pelo teste"})));
    assert_eq!((code, &out["reason"]), (Some(1), &json!("owner-missing")), "{out}");
    let hint = out["hint"].as_str().unwrap_or_default();
    assert!(!hint.contains("waves"), "{hint}");
    for lang in [Locale::PtBr, Locale::EnUs] {
        let text = translate("plan.owner_missing", lang);
        assert!(!text.contains("waves") && text.contains("applies_to"), "{text}");
    }
    let (code, out) = write_out(root, "decision", &decision(json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "applies_to": {"files": ["a.rs"]}})));
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
/// nova que o modelo grava sem título herda o da versão que substitui.
#[test]
fn task_without_a_title_is_refused_and_with_a_title_is_written() {
    use mustard_core::platform::i18n::{Locale, translate};
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = write(root, "criterion",
        &json!({"title": "Combinar o item", "when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": msg}));
    let task = |title: Option<&str>, extra: Value| {
        let mut body = json!({"text": "Conferir cada critério no fechamento.", "files": [{"path": "a.rs"}],
            "depends_on": [], "covers": [crit], "origin": msg, "agent": "- conferir cada critério"});
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
    let saved = written["events"].as_array().expect("events").iter().find(|e| e["code"] == out["code"]).cloned();
    assert_eq!(saved.map(|e| e["title"].clone()), Some(json!(exact)), "o título fica gravado: {written}");

    // A versão nova pelo modelo, sem título: herda o da versão que substitui.
    let (code, out) = write_out(root, "task", &task(None, json!({"replaces": first})));
    assert_eq!((code, &out["ok"]), (Some(0), &json!(true)), "a versão nova sem título herda o título: {out}");
    let second = out["id"].as_u64().expect("o número da versão");
    let revised = read(root, "waves");
    let saved = revised["events"].as_array().expect("events").iter().find(|e| e["code"] == out["code"]).cloned();
    assert_eq!(saved.map(|e| e["title"].clone()), Some(json!(exact)), "o título veio da versão anterior: {revised}");
    let (code, out) = write_out(root, "task", &task(Some("Fechamento confere cada critério"), json!({"replaces": second})));
    assert_eq!((code, &out["ok"]), (Some(0), &json!(true)), "a versão nova com título passa: {out}");
}

/// Duas tarefas, a segunda depende da primeira: o `remove` da primeira é
/// recusado nomeando a dependente, e nada é gravado. Remover as duas no mesmo
/// `targets` passa, porque nenhuma dependente fica viva.
#[test]
fn removing_a_task_with_a_live_dependent_is_refused_naming_the_dependent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = write(root, "criterion",
        &json!({"title": "Combinar o item", "when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": msg}));
    let task = |title: &str, depends_on: Value| json!({"agent": "- conferir pelo teste", "title": title, "text": "Tarefa.", "files": [{"path": "a.rs"}],
        "depends_on": depends_on, "covers": [crit], "origin": msg});
    let (_, first) = write_out(root, "task", &task("Entregar a primeira", json!([])));
    let (_, second) = write_out(root, "task", &task("Entregar a segunda", json!([first["id"]])));

    let before = event_lines(root);
    let (code, refused) = write_out(root, "remove", &json!({"targets": [first["id"]], "reason": "Já foi feito fora."}));
    assert_eq!((code, &refused["reason"]), (Some(1), &json!("task-has-dependents")), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    let (first_code, second_code) = (first["code"].as_str().unwrap_or_default(), second["code"].as_str().unwrap_or_default());
    assert!(hint.contains(second_code) && hint.contains(first_code), "a recusa nomeia a dependente e a tarefa que sairia: {hint}");
    assert_eq!(event_lines(root), before, "nada foi gravado");

    let (code, removed) = write_out(root, "remove", &json!({"targets": [first["id"], second["id"]], "reason": "As duas saem."}));
    assert_eq!((code, &removed["ok"]), (Some(0), &json!(true)), "{removed}");
}

/// A tarefa cuja dependência foi removida (a remoção que o programa de antes
/// aceitava, semeada no arquivo) recebe a versão nova com a dependência que
/// já trazia: a gravação julga só a que acrescenta. A versão que acrescenta
/// uma dependência que não existe é recusada, nomeando só essa, e nada é
/// gravado.
#[test]
fn a_revision_keeps_the_dependency_already_broken_and_is_refused_only_for_the_one_it_adds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = write(root, "criterion",
        &json!({"title": "Combinar o item", "when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": msg}));
    let task = |title: &str, depends_on: Value, extra: Value| {
        let mut body = json!({"agent": "- conferir pelo teste", "title": title, "text": "Tarefa.", "files": [{"path": "a.rs"}],
            "depends_on": depends_on, "covers": [crit], "origin": msg});
        for (key, value) in extra.as_object().into_iter().flatten() {
            body[key] = value.clone();
        }
        body
    };
    let (_, first) = write_out(root, "task", &task("Entregar a primeira", json!([]), json!({})));
    let (_, second) = write_out(root, "task", &task("Entregar a segunda", json!([first["id"]]), json!({})));
    seed_binary(root, "remove", &json!({"targets": [first["id"]], "reason": "Já foi feito fora."}));

    let before = event_lines(root);
    let added = task("Entregar a segunda", json!([first["id"], "MSTD-TASK-0099"]), json!({"replaces": second["id"]}));
    let (code, refused) = write_out(root, "task", &added);
    assert_eq!((code, &refused["reason"]), (Some(1), &json!("task-depends-on-unknown")), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("MSTD-TASK-0099"), "a recusa nomeia a dependência que a versão acrescenta: {hint}");
    assert!(!hint.contains(first["code"].as_str().unwrap_or_default()), "a que a versão anterior já trazia fica fora da recusa: {hint}");
    assert_eq!(event_lines(root), before, "nada foi gravado");

    let kept = task("Entregar a segunda", json!([first["id"]]), json!({"replaces": second["id"]}));
    let (code, revised) = write_out(root, "task", &kept);
    assert_eq!((code, &revised["ok"]), (Some(0), &json!(true)), "a versão com só a dependência herdada grava: {revised}");
    assert_eq!(event_lines(root), before + 1);
}

/// O item combinado com dono pelos arquivos, em `applies_to`, vai no pedido
/// da onda cuja tarefa toca um desses arquivos, sem passar pela análise antes
/// do envio, e fica fora do pedido da onda que não toca.
#[test]
fn item_owned_by_its_files_goes_in_the_request_of_the_wave_that_touches_them() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    approved_with_waves(root, 2);
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "e mais isto"}));
    let (code, out) = write_out(root, "decision", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "O arquivo a1 guarda só a conta.", "keys": ["conta"],
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
    std::fs::write(root.join("a1.rs"), "fn one() {}\n").expect("a1.rs");
    std::fs::write(root.join("a2.rs"), "fn dois() {}\n").expect("a2.rs");
    // O checkout fica no ramo da spec, como a abertura da obra o deixa:
    // a rodada só comita e só abre cópia nele.
    git(&["init", "-q", "-b", "feature/teste"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\n").expect("exclude");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "semente"]);

    let out = rt(root, &["round", "--dispatch", "--spec", "teste"]).output().expect("dispatch");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let body = stdout_json(&out);
    assert!(body.get("analysis").is_none(), "o item com dono não pede a análise: {body}");
    let prompt = |wave: u64| {
        let entry = body["dispatch"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["wave"] == json!(wave))
            .unwrap_or_else(|| panic!("a entrega {wave} sai: {body}"));
        request_by_command(root, entry)
    };
    assert!(prompt(1).contains(&decision), "a onda que toca a1.rs leva a decisão: {}", prompt(1));
    assert!(!prompt(2).contains(&decision), "a onda que não toca fica sem ela: {}", prompt(2));
}

/// O pedido de uma onda sai, pela leitura do comando, igual byte a byte ao
/// texto gravado no envio: acentos, crases, linha em branco e a quebra do fim,
/// sem uma quebra a mais. Um segundo envio da mesma onda vence o primeiro, e
/// a onda que nunca saiu é recusada: não tem pedido para ler.
#[test]
fn the_request_reading_prints_the_sent_text_byte_for_byte() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "running", "branch": "feature/teste", "base": "dev"}));
    let first = "# teste — onda 2\n\nO primeiro.\n";
    let text = "# teste — onda 2\n\nLeia o `spec.ndjson` só pelo binário: ação, acentuação.\n\n- uma linha\n  - recuada\n";
    for body in [first, text] {
        seed_binary(root, "send", &json!({"author": "binary", "wave": 2, "role": "wave", "agent": "wave",
            "text": body, "lines": body.lines().count(), "chars": body.chars().count(), "mustard": "0"}));
    }

    let out = rt(root, &["read", "request-2", "--spec", "teste"]).output().expect("read");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(out.stdout, text.as_bytes(), "{}", String::from_utf8_lossy(&out.stdout));

    let none = rt(root, &["read", "request-3", "--spec", "teste"]).output().expect("read");
    assert!(!none.status.success(), "{}", String::from_utf8_lossy(&none.stdout));
    assert_eq!(stdout_json(&none)["reason"], json!("no-request"), "{}", String::from_utf8_lossy(&none.stdout));
}

/// O pedido do revisor final, que não tem onda, sai pela leitura
/// `request-review` igual byte a byte ao texto do último envio de revisão: o
/// envio de uma onda não entra, o envio de revisão mais novo vence o antigo, e
/// a spec sem envio de revisão é recusada. O pedido de uma onda segue
/// saindo por `request-<n>`, sem trocar de lugar com o da revisão.
#[test]
fn the_final_review_request_reading_prints_the_last_review_text_byte_for_byte() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "running", "branch": "feature/teste", "base": "dev"}));
    let wave = "# teste — onda 1\n\nO pedido da onda.\n";
    seed_binary(root, "send", &json!({"author": "binary", "wave": 1, "role": "wave", "agent": "wave",
        "text": wave, "lines": wave.lines().count(), "chars": wave.chars().count(), "mustard": "0"}));

    let none = rt(root, &["read", "request-review", "--spec", "teste"]).output().expect("read");
    assert!(!none.status.success(), "o envio de onda não é o da revisão: {}", String::from_utf8_lossy(&none.stdout));
    assert_eq!(stdout_json(&none)["reason"], json!("no-request"), "{}", String::from_utf8_lossy(&none.stdout));

    let first = "# teste — revisão final\n\nO primeiro.\n";
    let last = "# teste — revisão final\n\nLeia a `spec` só pelo binário: ação, acentuação.\n\n- uma linha\n  - recuada\n";
    for body in [first, last] {
        seed_binary(root, "send", &json!({"author": "binary", "role": "review",
            "text": body, "lines": body.lines().count(), "chars": body.chars().count(), "mustard": "0"}));
    }
    let out = rt(root, &["read", "request-review", "--spec", "teste"]).output().expect("read");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(out.stdout, last.as_bytes(), "{}", String::from_utf8_lossy(&out.stdout));

    let of_wave = rt(root, &["read", "request-1", "--spec", "teste"]).output().expect("read");
    assert_eq!(of_wave.stdout, wave.as_bytes(), "{}", String::from_utf8_lossy(&of_wave.stdout));
}

/// Cada item forma uma fila de versões, e a versão nova só entra no fim dela.
/// Com a versão 2 no lugar da 1, gravar outra versão sobre a 1 é recusado sem
/// gravar nada, e a recusa diz a 2 pelo código e pelo número; sobre a 2
/// passa. Pelo código, a gravação vai à vigente. Uma lista com um alvo já
/// substituído é recusada. O arquivo antigo que já tem duas pontas no mesmo
/// item continua sendo lido, com as duas.
#[test]
fn a_new_version_over_a_replaced_version_is_refused_and_names_the_current_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let decision = |text: &str, replaces: Option<Value>| {
        let mut body = json!({"title": "Guardar a conta", "agent": "- conferir pelo teste", "text": text,
            "keys": ["conta"], "why": "o usuário disse", "origin": msg});
        if let Some(replaces) = replaces {
            body["replaces"] = replaces;
        }
        body
    };
    let written = |body: &Value| {
        let (code, out) = write_out(root, "decision", body);
        assert_eq!((code, &out["ok"]), (Some(0), &json!(true)), "{out}");
        (out["id"].as_u64().expect("o número"), out["code"].as_str().expect("o código").to_string())
    };
    let refused = |body: &Value, current: &str, why: &str| {
        let before = event_lines(root);
        let (code, out) = write_out(root, "decision", body);
        assert_eq!((code, &out["reason"]), (Some(1), &json!("replaces-superseded")), "{why}: {out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(current), "{why}: a recusa diz a vigente {current}: {hint}");
        assert_eq!(event_lines(root), before, "{why}: nada foi gravado");
    };

    let (v1, item) = written(&decision("A conta fica no arquivo.", None));
    let (v2, same) = written(&decision("A conta fica no arquivo, revista.", Some(json!(v1))));
    assert_eq!(same, item, "a versão nova guarda o código do item");
    refused(&decision("A conta, pela versão velha.", Some(json!(v1))), &format!("{item} ({v2})"), "sobre a 1");
    let (v3, _) = written(&decision("A conta, sobre a vigente.", Some(json!(v2))));

    let (v4, _) = written(&decision("A conta, pelo código.", Some(json!(item))));
    let saved = read(root, &format!("item-{v4}"));
    assert_eq!(saved["events"][0]["replaces"], json!(v3), "pelo código vai à vigente: {saved}");

    let (other, _) = written(&decision("Outra conta.", None));
    refused(&decision("As duas contas.", Some(json!([other, v2]))), &format!("{item} ({v4})"), "lista com a 2");
    written(&decision("As duas contas.", Some(json!([other, v4]))));

    // O arquivo gravado antes da recusa, com duas versões sobre a mesma: a
    // leitura mostra as duas pontas, sem recusar nada.
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let mut content = std::fs::read_to_string(&path).expect("the event file");
    let last = content.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()?["id"].as_u64()).max();
    let base = last.expect("o último número") + 1;
    for (id, text, replaces) in [(base, "Base.", None), (base + 1, "Ponta um.", Some(base)), (base + 2, "Ponta dois.", Some(base))] {
        let mut line = json!({"v": 1, "id": id, "code": "MSTD-DEC-0099", "at": "2026-09-26T08:00:00-03:00",
            "type": "decision", "author": "assistant", "title": "Duas pontas", "agent": "- ler", "text": text,
            "keys": ["k"], "why": "w", "origin": msg});
        if let Some(old) = replaces {
            line["replaces"] = json!(old);
        }
        content.push_str(&line.to_string());
        content.push('\n');
    }
    std::fs::write(&path, content).expect("the old event file");
    let agreed = read(root, "agreed");
    let tips: Vec<&Value> = agreed["events"]
        .as_array()
        .expect("events")
        .iter()
        .filter(|e| e["code"] == json!("MSTD-DEC-0099"))
        .map(|e| &e["text"])
        .collect();
    assert_eq!(tips, [&json!("Ponta dois."), &json!("Ponta um.")], "as duas pontas seguem na leitura, a mais nova primeiro: {agreed}");
}

/// O comando de ler, rodado pelo binário de dentro de `from`, que é a pasta
/// de onde o agente o roda.
fn read_from(root: &Path, from: &Path, block: &str, term: Option<&str>) -> Value {
    let mut args = vec!["read", block, "--spec", "teste"];
    if let Some(term) = term {
        args.extend(["--term", term]);
    }
    let out = rt(root, &args).current_dir(from).output().expect("run read");
    assert!(out.status.success(), "read {block}: {}", String::from_utf8_lossy(&out.stdout));
    stdout_json(&out)
}

/// As leituras que o binário registrou, na ordem: o pedido e cada item dos
/// recibos.
fn reads_recorded(root: &Path) -> Vec<(String, String)> {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "call" && e.str_field("command") == Some("read"))
        .flat_map(|e| {
            let request = e.str_field("request").unwrap_or_default().to_string();
            let items: Vec<String> = e.fields["items"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            items.into_iter().map(move |item| (request.clone(), item))
        })
        .collect()
}

/// O agente que roda `run read` de dentro da cópia do pedido aberto lê a
/// mensagem do usuário inteira, e cada item ou lição que ele acha fica
/// registrado como lido para aquele pedido; o mesmo comando rodado de outra
/// pasta lê igual e não registra nada.
#[test]
fn reading_from_inside_the_copy_of_an_open_request_shows_the_message_and_records_the_reading() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "Pode liberar mais espaço, é voce que está lotando o disco"}));
    let crit = write(root, "criterion", &json!({"title": "Uma prova", "when": "a", "then": "b", "proof": "echo p", "form": "ubiquitous", "origin": msg}));
    seed_binary(root, "wave", &json!({"author": "binary", "n": 1, "text": "Um.", "criteria": [crit], "done_when": "x", "origin": msg}));
    seed_binary(root, "task", &json!({"author": "binary", "wave": 1, "text": "Guardar.", "files": [{"path": "src/a.rs"}],
        "depends_on": [], "origin": msg, "covers": [msg]}));
    let copy = root.join("copias").join("a");
    std::fs::create_dir_all(copy.join("src")).expect("the copy folder");
    seed_binary(root, "send", &json!({"author": "binary", "wave": 1, "role": "wave", "agent": "wave", "text": "o pedido",
        "lines": 1, "chars": 8, "mustard": "0", "copy": mustard_core::io::wave_prompt::shown(&copy)}));
    let lesson = write(root, "lesson", &json!({"class": "environment_trap", "text": "O disco enche.", "keys": ["disco"],
        "applies_to": {"files": ["**"]}, "found_in": {"spec": "teste"}}));

    let message = read_from(root, &copy.join("src"), "item-MSTD-MSG-0001", None);
    assert_eq!(message["events"][0]["text"], json!("Pode liberar mais espaço, é voce que está lotando o disco"), "{message}");
    assert_eq!(message["events"][0]["code"], json!("MSTD-MSG-0001"), "{message}");
    read_from(root, &copy, "lessons", Some(&lesson.to_string()));
    assert_eq!(
        reads_recorded(root),
        [("request-1".to_string(), "MSTD-MSG-0001".to_string()), ("request-1".to_string(), format!("lesson-{lesson}"))]
    );

    let outside = read_from(root, root, "item-MSTD-MSG-0001", None);
    assert_eq!(outside["events"][0]["text"], message["events"][0]["text"], "{outside}");
    assert_eq!(reads_recorded(root).len(), 2, "a leitura de fora da cópia não é do pedido");
}

/// Quantas chamadas `read` o binário observou na spec `teste`: as do arquivo
/// de operações dela, onde a chamada mora.
fn read_calls(root: &Path) -> usize {
    let path = mustard_core::io::spec_events::operations_file(root, "teste").expect("the operations file");
    let log = mustard_core::io::spec_events::read(&path).ok().flatten().unwrap_or_default();
    log.visible().into_iter().filter(|e| e.event_type == "call" && e.str_field("command") == Some("read")).count()
}

/// O agente de uma conversa sem ligação parado na cópia da própria spec, com a
/// pasta das cópias escrita de outro jeito pelo sistema: no Linux e no macOS,
/// a base das cópias atrás de um link — a pasta pessoal num link —, e o
/// sistema dá a pasta do terminal já resolvida; no Windows, sem o link, o
/// lugar resolvido traz o prefixo de caminho literal. O `run read` dele fica
/// gravado, uma vez só; o mesmo comando da mesma conversa, rodado da pasta
/// principal, não fica.
#[test]
fn a_read_from_a_spec_copy_behind_a_link_is_recorded_for_an_unbound_conversation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("projeto");
    std::fs::create_dir_all(&root).expect("the project");
    seed_state(&root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    seed_binary(&root, "message", &json!({"author": "user", "text": "O agente lê de dentro da cópia."}));
    let real = dir.path().join("real");
    std::fs::create_dir_all(&real).expect("the real base");
    #[cfg(unix)]
    let base = {
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("the link");
        link
    };
    #[cfg(not(unix))]
    let base = real.clone();
    let project_folder = mustard_core::io::wave_prompt::copies_dir(&root).file_name().expect("the project folder").to_os_string();
    let copy = real.join(project_folder).join("teste").join("a");
    std::fs::create_dir_all(&copy).expect("the copy");
    let read_unbound_from = |from: &Path| {
        let out = rt(&root, &["read", "conversation", "--spec", "teste"])
            .current_dir(from)
            .env("MUSTARD_COPIES_DIR", &base)
            .env("MUSTARD_SESSION_ID", "s-paralela")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .output()
            .expect("run read");
        assert!(out.status.success(), "read from {}: {}", from.display(), String::from_utf8_lossy(&out.stdout));
    };

    read_unbound_from(&root);
    assert_eq!(read_calls(&root), 0, "a conversa sem ligação lendo da pasta principal não grava a chamada");
    read_unbound_from(&copy);
    assert_eq!(read_calls(&root), 1, "lendo de dentro da cópia da spec, a chamada fica uma vez");
}

/// O campo `every_wave` é aceito na gravação de uma regra e aparece na leitura
/// do item, pelo código; o valor que não é verdadeiro ou falso é recusado sem
/// gravar nada, e o item com a marca tem dono sem onda nem arquivo, mesmo numa
/// spec já aprovada.
#[test]
fn the_every_wave_mark_is_accepted_on_write_and_shown_on_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    approved_with_waves(root, 1);
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "vale para toda onda"}));
    let rule = |extra: Value| {
        let mut body = json!({"title": "Vale em toda onda", "agent": "- conferir pelo teste", "text": "A regra vale em toda onda.",
            "keys": ["onda"], "example": "um exemplo", "origin": msg});
        body.as_object_mut().expect("an object").extend(extra.as_object().cloned().unwrap_or_default());
        body
    };

    let (code, refused) = write_out(root, "rule", &rule(json!({"every_wave": "sim"})));
    assert_ne!(code, Some(0), "{refused}");
    assert_eq!(refused["ok"], json!(false), "{refused}");

    let (code, plain) = write_out(root, "rule", &rule(json!({})));
    assert_ne!(code, Some(0), "sem dono nem marca, a regra é recusada: {plain}");

    let (code, written) = write_out(root, "rule", &rule(json!({"every_wave": true})));
    assert_eq!((code, &written["ok"]), (Some(0), &json!(true)), "{written}");
    let item = read(root, &format!("item-{}", written["code"].as_str().expect("the rule code")));
    assert_eq!(item["events"][0]["every_wave"], json!(true), "{item}");
}

/// A leitura de um bloco, como quem conduz a faz, traz os mais novos
/// primeiro, até o teto, com a linha que diz quantos de quantos; `--all` traz
/// todos, sem a linha.
#[test]
fn a_conversation_read_brings_the_newest_up_to_the_cap_and_all_brings_every_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let ids: Vec<String> = (1..=35)
        .map(|n| seed_binary(root, "message", &json!({"author": "user", "text": format!("mensagem {n}")})))
        .map(|id| format!("MSTD-MSG-{id:04}"))
        .collect();
    let numbers = |report: &Value| -> Vec<String> {
        report["events"].as_array().expect("events").iter().map(|e| e["code"].as_str().expect("code").to_string()).collect()
    };

    let capped = read(root, "conversation");
    let newest: Vec<String> = ids.iter().rev().take(30).cloned().collect();
    assert_eq!(numbers(&capped), newest, "{capped}");
    assert_eq!(capped["count"], json!(30), "{capped}");
    let more = capped["more"].as_str().unwrap_or_default();
    assert!(more.contains("30") && more.contains("35") && more.contains("--all"), "{capped}");

    let out = rt(root, &["read", "conversation", "--spec", "teste", "--all"]).output().expect("run read");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let all = stdout_json(&out);
    assert_eq!(numbers(&all), ids.iter().rev().cloned().collect::<Vec<_>>(), "{all}");
    assert!(all.get("more").is_none(), "{all}");
}

/// A lista das ondas mostra só as abertas, com a linha que diz quantas de
/// quantas; `--all` traz também a onda já entregue.
#[test]
fn the_wave_list_shows_only_the_open_waves_unless_all_is_asked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let said = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = seed_binary(root, "criterion",
        &json!({"when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": said}));
    for n in 1..=3 {
        seed_binary(root, "wave", &json!({"author": "binary", "n": n, "text": format!("Onda {n}."), "criteria": [crit],
            "done_when": "x", "origin": said}));
        seed_binary(root, "task", &json!({"wave": n, "text": format!("Tarefa {n}."),
            "files": [{"path": format!("a{n}.rs")}], "origin": said}));
    }
    seed_binary(root, "delivered", &json!({"author": "binary", "wave": 1, "text": "Pronta.", "files": ["a1.rs"]}));
    let waves = |report: &Value| -> Vec<u64> {
        report["events"].as_array().expect("events").iter().map(|e| e["n"].as_u64().expect("n")).collect()
    };

    let open = read(root, "wave-list");
    assert_eq!(waves(&open), [2, 3], "{open}");
    let more = open["more"].as_str().unwrap_or_default();
    assert!(more.contains('2') && more.contains('3') && more.contains("--all"), "{open}");

    let out = rt(root, &["read", "wave-list", "--spec", "teste", "--all"]).output().expect("run read");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let all = stdout_json(&out);
    assert_eq!(waves(&all), [1, 2, 3], "{all}");
    assert!(all.get("more").is_none(), "{all}");
}

/// A busca com termo no painel vem resumida, uma linha por tipo, com quantos
/// e os códigos dos mais novos; `--all` traz os eventos inteiros.
#[test]
fn a_metrics_search_comes_as_a_summary_and_all_brings_the_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let call = |command: &str, ms: u64| {
        seed_binary(root, "call", &json!({"author": "binary", "command": command, "result": "ok", "ms": ms}))
    };
    call("map search", 10);
    call("round", 3);
    call("map search", 20);
    call("map search", 30);

    let out = rt(root, &["read", "metrics", "--spec", "teste", "--term", "map search"]).output().expect("run read");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let summary = stdout_json(&out);
    let lines = summary["events"].as_array().expect("events");
    assert_eq!(lines.len(), 1, "{summary}");
    assert_eq!((&lines[0]["type"], &lines[0]["count"]), (&json!("call"), &json!(3)), "{summary}");
    assert!(lines[0].get("command").is_none(), "the summary does not bring the events: {summary}");
    assert!(summary["more"].as_str().unwrap_or_default().contains("--all"), "{summary}");

    let out = rt(root, &["read", "metrics", "--spec", "teste", "--term", "map search", "--all"]).output().expect("run read");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let all = stdout_json(&out);
    let commands: Vec<&Value> = all["events"].as_array().expect("events").iter().map(|e| &e["command"]).collect();
    assert_eq!(commands, [&json!("map search"); 3], "{all}");
}

/// As marcas de conferência de um fato, que só a leitura do item traz.
const FACT_MARKS: [&str; 5] = ["source_version", "current_source_version", "classification", "verification", "source_status"];

/// O campo `name` de uma linha do tipo `kind` aponta outro item, e a leitura
/// de um bloco o mostra pelo código: o tipo o declara como referência, ou ele
/// é uma ligação que qualquer linha traz sem o tipo declarar (a origem, a
/// versão que ela substitui e a mensagem a que a resposta responde).
fn links_an_item(kind: &str, name: &str) -> bool {
    use mustard_core::domain::spec_events::{Kind, type_spec};
    ["origin", "replaces", "reply_to"].contains(&name)
        || type_spec(kind).is_some_and(|spec| spec.fields.iter().any(|field| field.name == name && matches!(field.kind, Kind::Ref | Kind::Refs)))
}

/// O número de um evento, ou a lista deles, trocado pelo código do item.
fn by_code(codes: &std::collections::BTreeMap<u64, String>, value: &Value) -> Value {
    match value {
        Value::Array(list) => Value::Array(list.iter().map(|one| by_code(codes, one)).collect()),
        one => one.as_u64().and_then(|id| codes.get(&id)).map_or_else(|| one.clone(), |code| json!(code)),
    }
}

/// Uma spec com um item de cada tipo, cada linha com as marcas que o binário
/// carimba (versão do formato, número, hora em nanossegundos, autor e
/// sessão); dois pontos com os mesmos fatos; e as ligações: a origem, a
/// resposta a uma mensagem, a versão nova de uma decisão, o ponto que fecha
/// outro com o resultado e o ponto que reaproveita resultados de antes; e
/// cada referência que o tipo declara, apontando a primeira mensagem.
/// Devolve os códigos de cada número.
fn spec_with_every_item_type(root: &Path) -> std::collections::BTreeMap<u64, String> {
    open_spec(root);
    let fact = |source: &str, text: &str| {
        json!({"source": source, "text": text, "classification": "syntax-candidate", "verification": "requires-source-confirmation",
            "source_version": "c21d75f84991d3ef1951c00770f9ad5939451299ec672ff0da55c364c45ddbba"})
    };
    let shared = vec![fact("src/soma.rs:1", "A soma mora em src/soma.rs."), fact("src/total.rs:2", "O total mora em src/total.rs.")];
    let mut lines = vec![json!({"type": "message", "author": "user", "text": "Somar a fatura."})];
    let mut first_of: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for spec in mustard_core::domain::spec_events::TYPES.iter().filter(|spec| spec.name != "message") {
        first_of.insert(spec.name, lines.len() as u64 + 1);
        let mut line = json!({"type": spec.name, "author": "assistant", "title": format!("Título {}", spec.name),
            "text": format!("Texto {}.", spec.name), "agent": "- a parte do agente", "keys": ["chave"], "example": "um exemplo",
            "why": "um porquê", "expected": "o esperado", "value": "um valor", "effect": "new_waves", "origin": 1});
        // Os alvos da remoção e do expurgo somem da leitura: eles ficam fora.
        for field in spec.fields.iter().filter(|field| field.name != "targets") {
            match field.kind {
                mustard_core::domain::spec_events::Kind::Ref => line[field.name] = json!(1),
                mustard_core::domain::spec_events::Kind::Refs => line[field.name] = json!([1]),
                _ => {}
            }
        }
        lines.push(line);
    }
    let point = |facts: Vec<Value>| json!({"type": "point", "author": "binary", "block": "defect", "gap": "Onde a fatura soma",
        "from": "gap", "status": "open", "origin": 1, "facts": facts});
    lines.push(point(shared.clone()));
    let mut more = shared;
    more.push(fact("src/fatura.rs:3", "A fatura mora em src/fatura.rs."));
    lines.push(point(more));
    lines.push(json!({"type": "response", "author": "assistant", "text": "Feito.", "reply_to": 1}));
    lines.push(json!({"type": "decision", "author": "assistant", "title": "Título decision", "text": "Texto decision, revisto.",
        "agent": "- a parte do agente", "keys": ["chave"], "why": "um porquê", "origin": 1, "replaces": first_of["decision"]}));
    lines.push(json!({"type": "point", "author": "assistant", "block": "defect", "gap": "Onde a fatura soma", "from": "gap",
        "status": "closed", "origin": 1, "closes": first_of["point"], "result": [lines.len()]}));
    lines.push(json!({"type": "point", "author": "assistant", "block": "defect", "gap": "Onde o total soma", "from": "gap",
        "status": "closed", "origin": 1, "result": [first_of["decision"]], "reused_results": [first_of["decision"], first_of["task"]]}));
    let text: String = lines
        .into_iter()
        .enumerate()
        .fold(String::new(), |text, (at, mut line)| {
            line["v"] = json!(1);
            line["id"] = json!(at + 1);
            line["at"] = json!("2026-10-22T14:38:48.128232016-03:00");
            line["session"] = json!("sessao-da-obra");
            text + &line.to_string() + "\n"
        });
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    std::fs::write(&path, text).expect("the event file");
    mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file").codes()
}

/// A leitura de um bloco traz o conteúdo de cada item sem as marcas internas.
/// Saem a versão do formato, a sessão e o número, porque o código fica. A
/// hora sai fora da conversa e, nela, fica até o minuto. O autor fica só na
/// conversa, e cada fato sai sem as marcas de conferência. As ligações saem
/// pelo código do item, e todo o resto continua igual ao que a leitura do
/// item traz. O fato repetido sai uma vez, e o ponto seguinte aponta para
/// ele. A leitura do item pelo código segue com o evento inteiro.
#[test]
fn a_block_read_shows_each_item_without_the_internal_marks_and_links_by_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let codes = spec_with_every_item_type(root);
    let conversation = |kind: &str| {
        mustard_core::domain::spec_events::type_spec(kind)
            .is_some_and(|spec| spec.block == mustard_core::domain::spec_events::Block::Conversation)
    };
    // Cada versão, lida pelo número como hoje: o ponto que fecha outro leva o
    // código dele, e as duas linhas se separam pelo texto e pela situação.
    let wholes: Vec<Value> = codes.keys().map(|id| read(root, &format!("item-{id}"))["events"][0].clone()).collect();
    let mut seen_types = std::collections::BTreeSet::new();
    for block in ["conversation", "state", "agreed", "specification", "criteria", "waves", "review", "progress", "notes"] {
        let report = read(root, block);
        let mut facts_seen: std::collections::BTreeMap<(String, String), (Value, usize)> = std::collections::BTreeMap::new();
        for lean in report["events"].as_array().expect("events") {
            let code = lean["code"].as_str().unwrap_or_else(|| panic!("each item comes with its code: {lean}"));
            let same: Vec<&Value> = wholes.iter().filter(|whole| whole["code"] == json!(code)).collect();
            let whole = match same[..] {
                [one] => one.clone(),
                _ => (*same.iter().find(|whole| whole.get("text") == lean.get("text") && whole.get("status") == lean.get("status")).expect("the version shown")).clone(),
            };
            let kind = whole["type"].as_str().expect("the type").to_string();
            if whole.get("facts").is_some() && same.len() == 1 {
                let by_code = read(root, &format!("item-{code}"))["events"][0].clone();
                assert_eq!(by_code, whole, "the item read by code keeps the whole event");
            }
            seen_types.insert(kind.clone());
            for mark in ["v", "id", "session", "search"] {
                assert!(lean.get(mark).is_none(), "{block}: {kind} without {mark}: {lean}");
                assert!(mark == "search" || whole.get(mark).is_some(), "the item read keeps {mark}: {whole}");
            }
            let at = whole["at"].as_str().expect("the item read keeps the whole time");
            assert_eq!(at, "2026-10-22T14:38:48.128232016-03:00", "{whole}");
            if conversation(&kind) {
                assert_eq!(lean["at"], json!("2026-10-22T14:38"), "{block}: in the conversation, the time to the minute: {lean}");
                assert_eq!(lean["author"], whole["author"], "{block}: in the conversation, who spoke: {lean}");
            } else {
                assert!(lean.get("at").is_none() && lean.get("author").is_none(), "{block}: {kind} without time or author: {lean}");
            }
            for (key, value) in whole.as_object().expect("an object") {
                match key.as_str() {
                    "v" | "id" | "session" | "at" | "author" | "changed" | "replaced_by" | "removed" => {}
                    "text" | "tasks" if kind == "send" => {}
                    link if links_an_item(&kind, link) => {
                        assert_eq!(lean[link], by_code(&codes, value), "{block}: {kind} links {link} by code: {lean}");
                        assert!(lean[link].to_string().contains("MSTD-"), "{block}: {lean}");
                    }
                    "facts" => {
                        let shown = lean["facts"].as_array().expect("the facts stay");
                        for (n, (fact, kept)) in value.as_array().expect("facts").iter().zip(shown).enumerate() {
                            for mark in FACT_MARKS {
                                assert!(fact.get(mark).is_some(), "the item read keeps {mark}: {fact}");
                            }
                            let key = (fact["source"].to_string(), fact["text"].to_string());
                            match facts_seen.get(&key) {
                                Some((first, at)) => assert_eq!(kept, &json!({"same_as": first, "fact": at}), "{block}: {lean}"),
                                None => {
                                    let mut bare = fact.clone();
                                    for mark in FACT_MARKS {
                                        bare.as_object_mut().expect("a fact").remove(mark);
                                    }
                                    assert_eq!(kept, &bare, "{block}: the fact without its marks: {lean}");
                                    facts_seen.insert(key, (json!(code), n + 1));
                                }
                            }
                        }
                        assert_eq!(shown.len(), value.as_array().map_or(0, Vec::len), "{lean}");
                    }
                    other => assert_eq!(lean.get(other), Some(value), "{block}: {kind} keeps {other}: {lean}"),
                }
            }
        }
        if block == "agreed" {
            let text = report.to_string();
            for said in ["A soma mora em src/soma.rs.", "O total mora em src/total.rs.", "A fatura mora em src/fatura.rs."] {
                assert_eq!(text.matches(said).count(), 1, "the repeated fact comes once: {report}");
            }
            assert!(!text.contains("c21d75f8"), "{report}");
        }
    }
    for kind in ["message", "response", "point", "rule", "limit", "edge_case", "decision", "context", "criterion", "task", "wave", "request"] {
        assert!(seen_types.contains(kind), "the {kind} was read: {seen_types:?}");
    }
}

/// O nome parecido `messages` lê a conversa, e a resposta diz o nome certo.
#[test]
fn reading_messages_brings_the_conversation_and_says_its_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_binary(root, "message", &json!({"author": "user", "text": "Somar a fatura."}));
    let read = read(root, "messages");
    assert_eq!((&read["block"], &read["count"]), (&json!("conversation"), &json!(1)), "{read}");
    let warned = read["warnings"].to_string();
    assert!(warned.contains("messages") && warned.contains("conversation"), "{read}");
}

/// A leitura de um bloco mostra a origem pelo código da mensagem, e a gravação
/// aceita esse mesmo código em `origin`: o arquivo guarda o número da
/// mensagem. O código que a spec não tem e o texto fora do formato do código
/// são recusados pelo nome, sem gravar nada, e o número segue valendo.
#[test]
fn the_origin_of_a_write_accepts_the_item_code_and_stores_its_number() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = &dir.path().canonicalize().expect("the folder");
    let said = opened_by_a_session(root, "o pedido");
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let stored = |id: u64| {
        let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
        log.get(id).and_then(|event| event.fields.get("origin").cloned())
    };
    let code = mustard_core::io::spec_events::read(&path)
        .expect("read")
        .expect("the spec file")
        .codes()
        .get(&said)
        .cloned()
        .expect("the message has its code");
    let context = |origin: &Value, text: &str| json!({"title": "Combinar o item", "agent": "- conferir pelo teste",
        "text": text, "origin": origin});

    let by_code = write(root, "context", &context(&json!(code), "A fatura soma centavos."));
    assert_eq!(stored(by_code), Some(json!(said)), "the file keeps the message number, not its code");
    let by_number = write(root, "context", &context(&json!(said), "O total soma a fatura."));
    assert_eq!(stored(by_number), Some(json!(said)), "the number keeps working");
    let shown = read(root, "specification")["events"].to_string();
    assert!(shown.contains(&format!("\"origin\":\"{code}\"")), "the block read links the origin by code: {shown}");

    let (stem, _) = code.rsplit_once('-').expect("a code has its number");
    let before = event_lines(root);
    let (status, unknown) = write_out(root, "context", &context(&json!(format!("{stem}-9999")), "Outro texto."));
    assert_ne!(status, Some(0), "{unknown}");
    assert_eq!(unknown["reason"], json!("unknown-target"), "{unknown}");
    let (status, malformed) = write_out(root, "context", &context(&json!("a mensagem do pedido"), "Mais um texto."));
    assert_ne!(status, Some(0), "{malformed}");
    assert_eq!(malformed["reason"], json!("invalid-value"), "{malformed}");
    assert_eq!(event_lines(root), before, "a refused origin writes nothing");
}

/// A spec de antes desta versão do Mustard só se lê. A gravação, a rodada, a
/// retomada, o plano, a reabertura, o fechamento e a abertura do pull request
/// recusam com o código próprio, e o arquivo dela fica com os mesmos bytes; a
/// leitura passa. O comando que mexia nos registros das
/// specs antigas saiu.
#[test]
fn a_legacy_spec_is_read_only_and_move_records_is_gone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), "{}").expect("config");
    approved_with_waves(root, 1);
    mustard_core::io::legacy_specs::mark_once(root, "0.2.24").expect("the list");
    let spec = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let before = std::fs::read(&spec).expect("spec");

    for args in [
        vec!["write", "decision", "--spec", "teste", "--json", r#"{"text":"Fica assim.","origin":2}"#],
        vec!["round", "--spec", "teste", "--dispatch"],
        vec!["resume", "--spec", "teste"],
        vec!["plan", "--spec", "teste"],
        vec!["reopen", "--spec", "teste", "--reason", "mais um ajuste"],
        vec!["close", "--spec", "teste"],
    ] {
        let out = rt(root, &args).output().expect("run");
        assert_eq!(out.status.code(), Some(1), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
        let answer = stdout_json(&out);
        assert_eq!(answer["reason"], json!("legacy-spec"), "{args:?}: {answer}");
        assert!(answer["hint"].as_str().unwrap_or_default().contains("teste"), "{answer}");
        assert_eq!(std::fs::read(&spec).expect("spec"), before, "{args:?} left the legacy spec as it was");
    }

    let pr = rt(root, &["pr-open", "--base", "dev", "--head", "feature/teste", "--spec", "teste"]).output().expect("run pr-open");
    let answer = stdout_json(&pr);
    assert_eq!(answer["ok"], json!(false), "{answer}");
    assert!(answer["error"].as_str().unwrap_or_default().starts_with("legacy-spec"), "{answer}");
    assert_eq!(std::fs::read(&spec).expect("spec"), before, "pr-open left the legacy spec as it was");

    let read = rt(root, &["read", "state", "--spec", "teste"]).output().expect("run read");
    assert!(read.status.success(), "{}", String::from_utf8_lossy(&read.stdout));

    let gone = rt(root, &["move-records", "--spec", "teste"]).output().expect("run move-records");
    assert!(!gone.status.success());
    let gone = stdout_json(&gone);
    assert_eq!(gone["kind"], json!("InvalidSubcommand"), "{gone}");
}

/// A spec `teste` aberta pelo `run open`, num repositório parado em `dev`, e
/// a fala `said` do usuário gravada pelo gancho da entrada, como numa sessão
/// de verdade. Devolve o número da fala.
fn opened_by_a_session(root: &Path, said: &str) -> u64 {
    let git = |args: &[&str]| {
        let out = Command::new("git").args(args).current_dir(root).output().expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "t"]);
    git(&["config", "commit.gpgsign", "false"]);
    git(&["checkout", "-q", "-b", "main"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\n").expect("exclude");
    std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"},"git":{"flow":{"*":"dev","dev":"main"}}}"#).expect("config");
    std::fs::write(root.join("README.md"), "oi\n").expect("code");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "init"]);
    git(&["checkout", "-q", "-b", "dev"]);
    // A conversa que abre a spec é a mesma que fala depois: o gancho do
    // terminal passa a sessão ao comando, e só a spec ligada a ela grava a fala.
    let opening = rt(root, &["open", "--kind", "feature", "--name", "teste", "--base", "dev"])
        .env("MUSTARD_SESSION_ID", "s-teste")
        .output()
        .expect("run open");
    let opened = stdout_json(&opening);
    assert_eq!(opened["ok"], json!(true), "{opened}");

    let prompt = json!({"hook_event_name": "UserPromptSubmit", "prompt": said, "session_id": "s-teste", "cwd": root.to_string_lossy()});
    let mut hook = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["on", "UserPromptSubmit"])
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the hook runs");
    if let Some(mut pipe) = hook.stdin.take() {
        std::io::Write::write_all(&mut pipe, prompt.to_string().as_bytes()).expect("the hook reads the prompt");
    }
    assert!(hook.wait().expect("the hook finishes").success(), "a hook always exits 0");
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    std::fs::read_to_string(path)
        .expect("the event file")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .rfind(|event| event["type"] == json!("message") && event["author"] == json!("user") && event["text"] == json!(said))
        .and_then(|event| event["id"].as_u64())
        .expect("the entry hook recorded the message")
}

/// A sigla explicada num item da spec vale nas gravações seguintes: o
/// objetivo diz "Planejamento Integrado (PI)", e a regra que cita só "PI"
/// passa. A sigla nunca explicada é recusada, a recusa cita a chave
/// `acronyms`, e a chamada recusada guarda o motivo. A spec nasce do `run
/// open` e a fala do usuário, do gancho da entrada.
#[test]
fn an_acronym_explained_in_the_spec_counts_for_the_whole_spec() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = &dir.path().canonicalize().expect("the folder");
    let msg = opened_by_a_session(root, "o pedido");
    write(root, "context", &json!({"title": "Planejamento da carga", "agent": "- conferir a carga",
        "text": "Ligar o Planejamento Integrado (PI) ao estoque.", "origin": msg}));
    let rule = |text: &str| json!({"title": "Carga do dia", "agent": "- conferir a carga", "text": text,
        "keys": ["carga"], "example": "A carga sai às oito.", "origin": msg});

    let (code, explained) = write_out(root, "rule", &rule("O PI fecha a carga do dia."));
    assert_eq!(code, Some(0), "the acronym explained in the goal counts: {explained}");

    let (code, refused) = write_out(root, "rule", &rule("O MRP fecha a carga do dia."));
    assert_ne!(code, Some(0), "{refused}");
    assert_eq!(refused["reason"], json!("item-unclear"), "{refused}");
    assert!(refused["hint"].as_str().is_some_and(|hint| hint.contains("acronyms")), "the refusal names the key: {refused}");
    let operations = mustard_core::io::spec_events::operations_file(root, "teste").expect("the operations file");
    let calls = std::fs::read_to_string(operations).expect("the calls");
    let call: Value = serde_json::from_str(calls.lines().last().expect("the refused call")).expect("a call");
    assert_eq!(call["refusal"], json!("item-unclear"), "{call}");
    assert!(call["refusal_detail"].as_str().is_some_and(|detail| detail.contains("MRP")), "the call keeps the reason: {call}");
}

/// A recusa por texto confuso cita o começo da frase longa; se a frase abre
/// com uma chave, a chamada recusada guarda o motivo sem a chave: o arquivo
/// de operações da spec nunca recebe o segredo, e o resto do motivo fica.
#[test]
fn a_refusal_detail_quoting_a_key_never_reaches_the_operations_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = &dir.path().canonicalize().expect("the folder");
    let msg = opened_by_a_session(root, "o pedido");
    let key = "AKIAIOSFODNN7EXAMPLE";
    let long = format!("{key} {}", vec!["palavra"; 30].join(" "));

    let (code, refused) = write_out(root, "rule", &json!({"title": "Carga do dia", "agent": "- conferir a carga",
        "text": long, "keys": ["carga"], "example": "A carga sai às oito.", "origin": msg}));
    assert_ne!(code, Some(0), "{refused}");
    assert_eq!(refused["reason"], json!("item-unclear"), "{refused}");

    let operations = mustard_core::io::spec_events::operations_file(root, "teste").expect("the operations file");
    let calls = std::fs::read_to_string(operations).expect("the calls");
    assert!(!calls.contains(key), "the key reached the operations file: {calls}");
    let call: Value = serde_json::from_str(calls.lines().last().expect("the refused call")).expect("a call");
    assert_eq!(call["refusal"], json!("item-unclear"), "{call}");
    let detail = call["refusal_detail"].as_str().expect("the call keeps the reason");
    assert!(detail.contains("…") && detail.contains("palavra"), "the reason keeps the rest of the sentence opening: {call}");
}

/// O objetivo que o painel mostra da spec `teste`, nas três leituras dele: a
/// visão geral, a tela da spec e a lista das specs.
fn panel_goals(root: &Path) -> Vec<Value> {
    let screen = |args: &[&str]| {
        let out = rt(root, args).output().expect("run panel");
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
        stdout_json(&out)
    };
    let overview = screen(&["panel", "--spec", "teste", "--section", "overview"]);
    let spec = screen(&["panel", "--spec", "teste", "--section", "spec"]);
    let listed = screen(&["panel", "--spec", "teste"]);
    let row = listed["specs"].as_array().and_then(|specs| specs.iter().find(|spec| spec["name"] == json!("teste")).cloned());
    vec![overview["spec"]["goal"].clone(), spec["spec"]["goal"].clone(), row.map_or(Value::Null, |row| row["goal"].clone())]
}

/// O `title` do objetivo é o título dele em toda tela: com um título de 30
/// caracteres e uma primeira frase de 80, a gravação passa e o painel mostra
/// o título. O teto do título do pull request vale para o `title`: a versão
/// com um título de 61 caracteres é recusada, a recusa traz o título medido e
/// nada é gravado. A spec nasce do `run open` e a fala do usuário, do gancho
/// da entrada.
#[test]
fn the_goal_title_is_measured_and_shown_instead_of_the_first_sentence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = &dir.path().canonicalize().expect("the folder");
    let msg = opened_by_a_session(root, "o pedido");
    let title = "Título do objetivo vale sempre";
    let sentence = "O título gravado no objetivo vira o nome do pull request, do painel e do resumo.";
    assert_eq!((title.chars().count(), sentence.chars().count()), (30, 80));
    let goal = |title: &str| {
        json!({"title": title, "agent": "- conferir o título", "text": format!("{sentence} O resto fica no texto."), "origin": msg})
    };

    let (code, written) = write_out(root, "context", &goal(title));
    assert_eq!(code, Some(0), "a short title passes with a long first sentence: {written}");
    assert_eq!(panel_goals(root), vec![json!(title); 3], "every panel reading shows the title");

    let long = "O título do objetivo passa do teto e a gravação o recusa logo";
    assert_eq!(long.chars().count(), 61);
    let mut revised = goal(long);
    revised["replaces"] = written["id"].clone();
    let before = event_lines(root);
    let (code, refused) = write_out(root, "context", &revised);
    assert_eq!((code, &refused["reason"]), (Some(1), &json!("goal-title-too-long")), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains(long) && hint.contains("61") && hint.contains("60"), "the refusal names the measured title: {hint}");
    assert_eq!(event_lines(root), before, "nothing was written");
}

/// O `title` do objetivo escrito em negrito sai sem o negrito, como já sai a
/// primeira frase: o painel mostra o título limpo, e o teto do título do pull
/// request mede o título limpo. Um título de 60 caracteres entre asteriscos
/// passa, porque os asteriscos não chegam ao pull request.
#[test]
fn a_bold_goal_title_is_shown_and_measured_without_the_bold() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = &dir.path().canonicalize().expect("the folder");
    let msg = opened_by_a_session(root, "o pedido");
    let plain = "Travar o merge enquanto houver pendência aberta em toda spec";
    assert_eq!(plain.chars().count(), 60, "the plain title is at the cap");
    let goal = json!({"title": format!("**{plain}**"), "agent": "- conferir o título", "text": "O merge espera a pendência fechar.", "origin": msg});

    let (code, written) = write_out(root, "context", &goal);
    assert_eq!(code, Some(0), "the asterisks are not measured: {written}");
    assert_eq!(panel_goals(root), vec![json!(plain); 3], "every panel reading shows the title without the bold");
}

/// O objetivo de antes do `title`, como as specs de antes da forma fixa o
/// têm, segue medido e mostrado pela primeira frase: a versão nova dele, sem
/// título, com a primeira frase acima de 60 caracteres é recusada com a frase
/// na recusa; com a frase curta, passa e o painel mostra a frase. O objetivo
/// antigo entra pela gravação do núcleo, que é como ele chegou ao arquivo.
#[test]
fn a_goal_without_a_title_is_still_measured_and_shown_by_its_first_sentence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = &dir.path().canonicalize().expect("the folder");
    let msg = opened_by_a_session(root, "o pedido");
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let untitled = json!({"author": "assistant", "text": "Medir o objetivo antigo pela frase.", "origin": msg});
    let old = mustard_core::io::spec_events::write(&path, "context", untitled.as_object().cloned().expect("an object"), &[])
        .expect("the old goal")
        .id;

    let sentence = "O objetivo de antes do título segue medido pela primeira frase dele como sempre.";
    assert_eq!(sentence.chars().count(), 80);
    let version = |text: &str| json!({"replaces": old, "text": format!("{text} O resto fica no texto."), "origin": msg});
    let before = event_lines(root);
    let (code, refused) = write_out(root, "context", &version(sentence));
    assert_eq!((code, &refused["reason"]), (Some(1), &json!("goal-title-too-long")), "{refused}");
    assert!(refused["hint"].as_str().is_some_and(|hint| hint.contains(sentence)), "the refusal names the sentence: {refused}");
    assert_eq!(event_lines(root), before, "nothing was written");

    let short = "Medir o objetivo revisto pela frase.";
    let (code, written) = write_out(root, "context", &version(short));
    assert_eq!(code, Some(0), "{written}");
    assert_eq!(panel_goals(root), vec![json!(short); 3], "without a title the panel shows the first sentence");
}

/// Um JSON com acento, aspas, crase, parênteses e dois-pontos vai direto e
/// por `@arquivo`, gravado pela ferramenta de escrita, ao `run write context`
/// e ao `--json` e ao `--facts` do `run answer`: o arquivo grava o mesmo que
/// o texto direto. O arquivo que falta recusa, com o motivo e o caminho, sem
/// gravar nada, no `run write` e no `run answer`.
#[test]
fn a_json_argument_read_from_a_file_writes_the_same_as_the_direct_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let said = write(root, "message", &json!({"text": "Trocar a saudação."}));
    for gap in ["Onde a saudação nasce", "Onde a saudação aparece"] {
        let seen = json!([{"text": "A saudação mora na página.", "source": "mensagem 1"}]);
        seed_binary(root, "point", &json!({"block": "defect", "gap": gap, "from": "gap", "status": "open", "origin": said, "facts": seen}));
    }
    let text = "Ação “citada” (parênteses): dois-pontos; 'simples' e \"duplas\".";
    let agent = "- conferir: pelo `teste`";
    let fields = json!({"title": "Saudação nova", "text": text, "agent": agent, "origin": said});
    let decision = json!({"title": "Saudação nova", "text": text, "agent": agent, "keys": ["saudação"], "why": "pedido: \"trocar\"", "origin": said});
    let facts = json!([{"text": text, "source": "mensagem 1"}]);
    let in_file = |name: &str, value: &Value| {
        let file = dir.path().join(name);
        std::fs::write(&file, value.to_string()).expect("the file");
        format!("@{}", file.display())
    };
    let recorded = |args: &[&str]| {
        let out = rt(root, args).output().expect("the command runs");
        let report = stdout_json(&out);
        assert!(out.status.success(), "{args:?}: {report}");
        report
    };
    let context = |json: &str| recorded(&["write", "context", "--spec", "teste", "--json", json])["id"].clone();
    let (direct, from_file) = (context(&fields.to_string()), context(&in_file("contexto.json", &fields)));
    let answer = |json: &str, facts: &str| {
        let report = recorded(&["answer", "--spec", "teste", "--type", "decision", "--json", json, "--facts", facts]);
        (report["id"].clone(), report["closed"]["id"].clone())
    };
    let answered = answer(&decision.to_string(), &facts.to_string());
    let answered_from_file = answer(&in_file("decisao.json", &decision), &in_file("fatos.json", &facts));

    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let log = mustard_core::io::spec_events::read(&path).expect("readable").expect("the spec");
    let said_in = |id: &Value| {
        let event = id.as_u64().and_then(|id| log.get(id)).unwrap_or_else(|| panic!("no item {id}"));
        ["title", "text", "agent", "why", "facts"].map(|field| event.fields.get(field).cloned())
    };
    assert_eq!(said_in(&from_file), said_in(&direct), "the file writes the same context as the direct text");
    assert_eq!(said_in(&direct)[1], Some(json!(text)));
    assert_eq!(said_in(&answered_from_file.0), said_in(&answered.0), "the file writes the same answer as the direct text");
    assert_eq!(said_in(&answered.0)[3], Some(decision["why"].clone()));
    assert_eq!(said_in(&answered_from_file.1)[4], Some(facts.clone()), "the facts read from the file close the point");
    assert_eq!(said_in(&answered_from_file.1), said_in(&answered.1));

    // O `\` no nome faz no Linux o que o separador faz no Windows: o caminho
    // do arquivo chega à recusa com barras invertidas.
    let missing = dir.path().join("fal\\ta.json");
    let before = event_lines(root);
    for args in [
        vec!["write", "context", "--spec", "teste", "--json", &format!("@{}", missing.display())],
        vec!["answer", "--spec", "teste", "--type", "decision", "--json", "{}", "--facts", &format!("@{}", missing.display())],
    ] {
        let out = rt(root, &args).output().expect("the command runs");
        let refused = stdout_json(&out);
        assert_eq!((out.status.code(), refused["reason"].clone()), (Some(1), json!("io-failed")), "{args:?}: {refused}");
        // O texto do campo, e não o JSON impresso, que escapa a barra invertida.
        let names_the_file = refused["hint"].as_str().is_some_and(|hint| hint.contains(&missing.display().to_string()));
        assert!(names_the_file, "the refusal names the file: {refused}");
    }
    assert_eq!(event_lines(root), before, "nothing is written");
}

/// O código de um item gravado na spec, como o pedido da onda e a leitura
/// o mostram.
fn code_of(root: &Path, id: u64) -> String {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
    log.codes().get(&id).cloned().unwrap_or_else(|| panic!("no code for {id}"))
}

/// A resposta aponta a mensagem do usuário pelo código que a leitura mostra,
/// e a linha gravada guarda o número da mensagem, como faz com a origem. A
/// resposta é do binário, que a grava pela gravação do núcleo: o `run write`
/// a recusa.
#[test]
fn a_response_replying_by_the_message_code_keeps_the_message_number() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let said = seed_binary(root, "message", &json!({"author": "user", "text": "Trocar o tema do site."}));
    let answered =
        seed_binary(root, "response", &json!({"author": "assistant", "text": "Troquei o tema.", "reply_to": code_of(root, said)}));
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
    assert_eq!(log.get(answered).and_then(|answer| answer.int("reply_to")), Some(said));
}

/// O pedido que aponta pelo código a mensagem do usuário de onde veio leva a
/// fala dela à pergunta sobre o lugar do pedido. Com o Jev ligado e fora do
/// ar, a decisão vai à reserva, cujo material é o mesmo pedido ao Jev.
#[test]
fn a_request_from_the_message_code_takes_what_the_user_said_to_the_jev() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let config = json!({"ai": {"fallback": true}, "judgement": {"wave-planning": {"filter": "jev"}}});
    std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
    let goal = seed_binary(root, "message", &json!({"author": "user", "text": "Somar dois números na calculadora."}));
    write(root, "context", &json!({"title": "Somar na calculadora", "text": "Somar dois números na calculadora.",
        "agent": "- conferir pelo teste", "origin": goal}));
    let user_said = "Quero a tela inteira no modo escuro, por favor.";
    let said = seed_binary(root, "message", &json!({"author": "user", "text": user_said}));
    let request = json!({"title": "Tema escuro", "text": "Trocar o tema do site.", "keys": ["tema"], "effect": "new_waves",
        "agent": "- conferir pelo teste", "origin": code_of(root, said)});
    // Um endereço que ninguém atende: a pergunta ao Jev falha na hora.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let url = format!("http://{}/v1/systemone", closed.local_addr().expect("address"));
    drop(closed);
    let out = rt(root, &["write", "request", "--spec", "teste", "--json", &request.to_string()])
        .env("TYPESAFE_API_KEY", "fake-key")
        .env("MUSTARD_JEV_URL", &url)
        .output()
        .expect("run write");
    let reserved = stdout_json(&out);
    assert_eq!(reserved["reason"], json!("jev-reserve"), "{reserved}");
    let prompt = reserved["reserve"]["delegation"]["request"]["prompt"].as_str().expect("the reserve prompt");
    assert!(prompt.contains(user_said), "the user's words go to the Jev: {prompt}");
}

/// A busca na spec que precisa do Jev e fica sem ele diz por quê, pelo
/// `mustard.json` de verdade: o Jev desligado no projeto, a busca na spec sem
/// a escolha própria — mesmo com a chave e as outras finalidades ligadas —, a
/// chave que falta e a do `mustard.json` que o git guarda. O desligado nomeia
/// o ajuste que o liga.
#[test]
fn a_spec_search_without_the_jev_says_why_and_which_setting_turns_it_on() {
    let others = json!({"context": {"filter": "jev"}, "search": {"filter": "jev"}, "wave-planning": {"filter": "jev"}});
    let own = json!({"context": {"filter": "jev"}, "spec-search": {"filter": "jev"}});
    // (mustard.json, chave no ambiente, git guarda o mustard.json, a última linha)
    let cases = [
        (json!({"ai": {"fallback": false}, "judgement": own}), Some("fake-key"), false, "ai.fallback"),
        (json!({"ai": {"fallback": true}, "judgement": others, "jev": {"key": "from-file"}}), None, false, "judgement.spec-search.filter"),
        (json!({"ai": {"fallback": true}, "judgement": own}), None, false, "Jev fora: sem chave."),
        (json!({"ai": {"fallback": true}, "judgement": own, "jev": {"key": "from-file"}}), None, true, "git"),
    ];
    let mut lines = std::collections::BTreeSet::new();
    for (mut config, env, tracked, says) in cases {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        config["language"] = json!({"text": "pt-BR"});
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        if tracked {
            assert!(mustard_core::platform::git::run(root, &["init", "-q"]).ok);
            assert!(mustard_core::platform::git::run(root, &["add", "mustard.json"]).ok);
        }
        open_spec(root);
        let said = seed_binary(root, "message", &json!({"author": "user", "text": "nada"}));
        for (title, text) in [("Commit da onda", "A página mostra o commit."), ("Rodada formata", "A rodada formata os arquivos antes do commit.")] {
            write(root, "context", &json!({"title": title, "text": text, "agent": "- conferir pelo teste", "origin": said}));
        }
        let mut find = rt(root, &["find", "rodada formata arquivos commit onda", "--spec", "teste"]);
        if let Some(key) = env {
            find.env("TYPESAFE_API_KEY", key);
        }
        let out = find.output().expect("run find");
        let shown = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(out.status.success(), "{config}: {shown}");
        let last = shown.lines().last().unwrap_or_default().to_string();
        assert!(last.starts_with("Jev fora: ") && last.contains(says), "{config}: {shown}");
        lines.insert(last);
    }
    assert_eq!(lines.len(), 4, "each case has its own reason: {lines:?}");
}

/// O comando `cmd` com `input` na entrada padrão, como um heredoc o entrega.
fn fed(mut cmd: Command, input: &str) -> Output {
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("the command runs");
    let mut pipe = child.stdin.take().expect("the standard input");
    std::io::Write::write_all(&mut pipe, input.as_bytes()).expect("the command reads its input");
    drop(pipe);
    child.wait_with_output().expect("the command ends")
}

/// O item gravado com o código `code`, como a leitura o devolve.
fn item_read(root: &Path, code: &str) -> Value {
    let out = rt(root, &["read", &format!("item-{code}"), "--spec", "teste"]).output().expect("run read");
    assert!(out.status.success(), "read {code}: {}", String::from_utf8_lossy(&out.stdout));
    let read = stdout_json(&out);
    read["events"].as_array().and_then(|events| events.first()).cloned().unwrap_or(read)
}

/// O texto longo, com título, lista, crase e aspas, como um heredoc o manda.
const LONG_TEXT: &str = "## O que muda\n\n- A trava fica `presa` até o fim.\n- O lote \"grava\" as boas: e devolve as outras.\n\n```text\nmustard-rt run read state\n```";

/// Um lote de quatro tarefas: a segunda vem sem os arquivos, a terceira
/// depende dela pelo rótulo e a quarta depende da primeira pelo rótulo. A
/// primeira e a quarta gravam, e a quarta aponta o número da primeira. A
/// segunda volta com o motivo, e a terceira volta apontando a linha da
/// segunda. A resposta tem uma linha curta por item, na ordem.
#[test]
fn a_batch_records_the_good_lines_and_returns_the_refused_ones() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    seed_state(root, &json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
    let msg = seed_binary(root, "message", &json!({"author": "user", "text": "o plano"}));
    let crit = write(root, "criterion",
        &json!({"title": "Combinar o item", "when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": msg}));
    let task = |label: &str, depends_on: Value| json!({"type": "task", "label": label, "agent": "- conferir pelo teste",
        "title": format!("Entregar a {label}"), "text": "Tarefa.", "files": [{"path": "a.rs"}], "depends_on": depends_on,
        "covers": [crit], "origin": msg});
    let mut second = task("segunda", json!([]));
    second.as_object_mut().expect("an object").remove("files");
    let input = [task("primeira", json!([])), second, task("terceira", json!(["segunda"])), task("quarta", json!(["primeira"]))]
        .map(|line| line.to_string())
        .join("\n");
    let before = event_lines(root);

    let out = fed(rt(root, &["write", "--batch", "--spec", "teste"]), &format!("{input}\n"));
    let shown = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(1), "a refused line makes the batch fail: {shown}");
    let lines: Vec<Value> = shown.lines().map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}"))).collect();
    assert_eq!(lines.iter().map(|line| line["line"].clone()).collect::<Vec<_>>(), vec![json!(1), json!(2), json!(3), json!(4)], "{shown}");
    let first = lines[0]["code"].as_str().unwrap_or_else(|| panic!("the first line is written: {shown}"));
    assert!(lines[1]["reason"].is_string() && lines[1]["hint"].is_string() && lines[1].get("code").is_none(), "{shown}");
    assert_eq!(lines[2]["reason"], json!("batch-line-refused"), "{shown}");
    let hint = lines[2]["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("segunda") && hint.contains(" 2 "), "the refusal points at the line it cites: {hint}");
    let fourth = lines[3]["code"].as_str().unwrap_or_else(|| panic!("the fourth line is written: {shown}"));

    assert_eq!(event_lines(root), before + 2, "only the good lines are written");
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
    let codes = log.codes();
    let id_of = |code: &str| codes.iter().find(|(_, c)| c.as_str() == code).map(|(id, _)| *id).expect("the code is in the spec");
    let depends_on = log.get(id_of(fourth)).and_then(|event| event.fields.get("depends_on").cloned());
    assert_eq!(depends_on, Some(json!([id_of(first)])), "the label becomes the number of the line it names");
}

/// O texto longo, com título, lista, crase e aspas, entra cru pela entrada
/// padrão no `--agent -`, com o `--text` dado direto e os outros campos no
/// `--json`, e a leitura do item devolve os dois como foram escritos, sem a
/// quebra que fecha a última linha do heredoc.
#[test]
fn raw_text_from_standard_input_is_recorded_as_written() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let said = write(root, "message", &json!({"text": "Trocar a saudação."}));
    let text = "Ação “citada” (parênteses): dois-pontos; 'simples' e \"duplas\".";
    let fields = json!({"title": "Saudação nova", "origin": said}).to_string();
    let out = fed(rt(root, &["write", "context", "--spec", "teste", "--json", &fields, "--text", text, "--agent", "-"]), &format!("{LONG_TEXT}\n"));
    let report = stdout_json(&out);
    assert!(out.status.success(), "{report}");
    let item = item_read(root, report["code"].as_str().unwrap_or_else(|| panic!("no code: {report}")));
    assert_eq!(item["agent"], json!(LONG_TEXT), "{item}");
    assert_eq!(item["text"], json!(text), "{item}");
}

/// A resposta de um ponto leva o texto longo cru pela entrada padrão no
/// `--agent -`, e a leitura do item da resposta o devolve como foi escrito.
#[test]
fn raw_text_from_standard_input_answers_a_point_as_written() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let said = write(root, "message", &json!({"text": "Trocar a saudação."}));
    let seen = json!([{"text": "A saudação mora na página.", "source": "mensagem 1"}]);
    seed_binary(root, "point", &json!({"block": "defect", "gap": "Onde a saudação nasce", "from": "gap", "status": "open", "origin": said, "facts": seen}));
    let fields = json!({"title": "Saudação nova", "keys": ["saudação"], "why": "O usuário pediu.", "origin": said}).to_string();
    let args = ["answer", "--spec", "teste", "--type", "decision", "--json", &fields, "--text", "Trocar a saudação da página.", "--agent", "-"];
    let out = fed(rt(root, &args), &format!("{LONG_TEXT}\n"));
    let report = stdout_json(&out);
    assert!(out.status.success(), "{report}");
    assert_eq!(report["closed"]["status"], json!("closed"), "{report}");
    let item = item_read(root, report["code"].as_str().unwrap_or_else(|| panic!("no code: {report}")));
    assert_eq!(item["agent"], json!(LONG_TEXT), "{item}");
}

/// O JSON com uma quebra de linha crua dentro do texto é recusado no `run
/// write` e no `run answer`, sem gravar nada, e a dica ensina o texto cru pela
/// entrada padrão e o lote.
#[test]
fn an_invalid_json_refusal_points_to_raw_input_and_batch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let broken = "{\"title\": \"Nota\", \"text\": \"primeira linha\nsegunda linha\"}";
    let before = event_lines(root);
    for args in [
        vec!["write", "note", "--spec", "teste", "--json", broken],
        vec!["answer", "--spec", "teste", "--type", "decision", "--json", broken],
    ] {
        let out = rt(root, &args).output().expect("the command runs");
        let refused = stdout_json(&out);
        assert_eq!((out.status.code(), refused["reason"].clone()), (Some(1), json!("not-an-object")), "{args:?}: {refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`--text -`") && hint.contains("`--agent -`"), "the hint shows the raw input: {hint}");
        assert!(hint.contains("--batch"), "the hint shows the batch: {hint}");
    }
    assert_eq!(event_lines(root), before, "nothing is written");
}

/// A recusa impressa pela gravação `args`, que sai com 1 sem gravar nada.
fn refused_write(root: &Path, args: &[&str]) -> Value {
    let before = event_lines(root);
    let out = rt(root, args).output().expect("the command runs");
    let refused = stdout_json(&out);
    assert_eq!(out.status.code(), Some(1), "{args:?}: {refused}");
    assert_eq!(event_lines(root), before, "nothing is written: {args:?}");
    refused
}

/// O `--text -` e o `--agent -` juntos pedem a entrada padrão, que é uma só:
/// a gravação recusa antes de ler, e nada se grava.
#[test]
fn both_raw_fields_from_standard_input_are_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let fields = json!({"title": "Nota"}).to_string();
    let refused = refused_write(root, &["write", "note", "--spec", "teste", "--json", &fields, "--text", "-", "--agent", "-"]);
    assert_eq!(refused["reason"], json!("raw-input-twice"), "{refused}");
}

/// O campo que vem no `--json` e também na opção própria é recusado, com a
/// dica nomeando o campo, e nada se grava.
#[test]
fn a_raw_field_also_given_in_the_json_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let fields = json!({"title": "Nota", "agent": "no json"}).to_string();
    let refused = refused_write(root, &["write", "note", "--spec", "teste", "--json", &fields, "--agent", "na opção"]);
    assert_eq!(refused["reason"], json!("raw-field-twice"), "{refused}");
    assert!(refused["hint"].as_str().unwrap_or_default().contains("agent"), "the hint names the field: {refused}");
}

/// O `--json` que não é objeto, com o `--text` dado direto, segue como veio,
/// e a gravação o recusa pelo motivo de sempre, sem gravar nada.
#[test]
fn a_json_that_is_not_an_object_with_a_direct_text_is_refused_as_usual() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let refused = refused_write(root, &["write", "note", "--spec", "teste", "--json", "[\"Nota\"]", "--text", "direto"]);
    assert_eq!(refused["reason"], json!("not-an-object"), "{refused}");
}

/// As linhas curtas que o lote `input` imprime, com o código de saída.
fn batch_out(root: &Path, input: &str) -> (Option<i32>, Vec<Value>) {
    let out = fed(rt(root, &["write", "--batch", "--spec", "teste"]), input);
    let shown = String::from_utf8_lossy(&out.stdout).to_string();
    let lines = shown.lines().map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}"))).collect();
    (out.status.code(), lines)
}

/// O lote só com linhas em branco é recusado numa linha só, e nada se grava.
#[test]
fn an_empty_batch_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let before = event_lines(root);
    let (code, lines) = batch_out(root, "\n  \n");
    assert_eq!(code, Some(1), "{lines:?}");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["reason"], json!("batch-empty"), "{lines:?}");
    assert_eq!(event_lines(root), before, "nothing is written");
}

/// A linha do lote sem o tipo, ou com o tipo em branco, é recusada pelo
/// motivo próprio, cada uma na sua linha, e nada se grava.
#[test]
fn a_batch_line_without_its_type_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    open_spec(root);
    let before = event_lines(root);
    let input = format!("{}\n{}\n", json!({"title": "Nota"}), json!({"type": " ", "title": "Nota"}));
    let (code, lines) = batch_out(root, &input);
    assert_eq!(code, Some(1), "{lines:?}");
    let refusals: Vec<(Value, Value)> = lines.iter().map(|line| (line["line"].clone(), line["reason"].clone())).collect();
    assert_eq!(refusals, vec![(json!(1), json!("batch-type-missing")), (json!(2), json!("batch-type-missing"))], "{lines:?}");
    assert_eq!(event_lines(root), before, "nothing is written");
}
