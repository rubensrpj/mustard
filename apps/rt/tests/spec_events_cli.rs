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

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

#[path = "support/mod.rs"]
mod support;

fn rt(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
    // O ambiente de quem roda a suíte não chega ao serviço do Jev: a rodada o
    // chamaria de verdade.
    cmd.arg("run")
        .args(args)
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
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
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
    let out = Command::new("bash")
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
        std::fs::write(root.join(format!("a{n}.rs")), "fn one() {}\n").expect("seed file");
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
            std::fs::write(root.join(&file), format!("fn one() {{}}\n// {text}\n")).expect("the wave's change");
            read_request(root, wave);
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
                .filter(|w| !matches!(w["reason"].as_str(), Some("usage-missing" | "wave-size")))
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
            git(&["init", "-q"]);
            std::fs::write(root.join(".git/info/exclude"), ".claude/\n").expect("exclude");
            std::fs::write(root.join("a1.rs"), "fn one() {}\n").expect("seed file");
            git(&["add", "-A"]);
            git(&["commit", "-q", "-m", "semente"]);
            let dispatch = rt(root, &["round", "--spec", "teste"]).output().expect("dispatch");
            assert!(dispatch.status.success(), "{}", String::from_utf8_lossy(&dispatch.stdout));
            let copy = mustard_core::io::wave_prompt::slot_path(root, "teste", 0);
            assert!(copy.join("a1.rs").is_file(), "the round made the copy: {}", String::from_utf8_lossy(&dispatch.stdout));
            sent.send(mustard_core::io::wave_prompt::copies_dir(root)).expect("send the copies folder");
            assert!(!fails, "the test fails on purpose");
        });
        assert_eq!(test.join().is_err(), fails);
        let copies = made.recv().expect("the copies folder");
        assert!(!copies.exists(), "fails={fails}: the copies folder stayed at {}", copies.display());
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
        "criteria": [{"criterion": c2, "tests_rule": false}]}));
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

    let copy = |wave: u64| mustard_core::io::wave_prompt::slot_path(root, "teste", usize::try_from(wave).expect("a wave number fits") - 1);

    // Onda 1: muda o arquivo declarado e um outro que a entrega não cita; o
    // repositório continua compilando com o Makefile que já está lá.
    std::fs::write(copy(1).join("a1.rs"), "fn one() {}\n// muda\n").expect("a1 muda");
    std::fs::write(copy(1).join("extra.rs"), "fn main() {}\n").expect("extra");
    read_request(root, 1);
    write(root, "delivered", &json!({"wave": 1, "text": "Saiu.", "files": ["a1.rs"], "commit": "a1 sai"}));
    let out = rt(root, &["round", "--spec", "teste"]).output().expect("round 1");
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
        .filter(|w| !matches!(w["reason"].as_str(), Some("usage-missing" | "wave-size")))
        .collect();
    assert_eq!(json!(warned), json!([{"reason": "files-diverged", "wave": 1, "hint": hint}]), "{body}");

    // Onda 2: a cópia dela deixa o comando de compilação quebrado.
    let before = head();
    std::fs::write(copy(2).join("Makefile"), "default:\n\texit 1\n").expect("Makefile quebrado");
    read_request(root, 2);
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
    write(root, "context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "Deixar o índice certo. Depois o resto.", "origin": msg}));
    write(root, "rule", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": "**Uma linha por spec.** Com o objetivo.", "keys": ["índice"], "example": "e", "origin": msg}));
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
fn writing_a_wave_by_hand_is_refused_and_says_to_write_only_the_task() {
    use mustard_core::platform::i18n::{translate, Locale};
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
/// nova que o modelo grava sem título também é recusada.
#[test]
fn task_without_a_title_is_refused_and_with_a_title_is_written() {
    use mustard_core::platform::i18n::{translate, Locale};
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
        let entry = body["dispatch"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["wave"] == json!(wave))
            .unwrap_or_else(|| panic!("a onda {wave} sai: {body}"));
        request_by_command(root, entry)
    };
    assert!(prompt(1).contains(&decision), "a onda que toca a1.rs leva a decisão: {}", prompt(1));
    assert!(!prompt(2).contains(&decision), "a onda que não toca fica sem ela: {}", prompt(2));
}

/// O pedido de uma onda sai, pela leitura do comando, igual byte a byte ao
/// texto gravado no envio: acentos, crases, linha em branco e a quebra do fim,
/// sem uma quebra a mais. Um segundo envio da mesma onda vence o primeiro, e
/// a onda que nunca saiu devolve texto vazio.
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
    assert!(none.status.success(), "{}", String::from_utf8_lossy(&none.stdout));
    assert!(none.stdout.is_empty(), "{}", String::from_utf8_lossy(&none.stdout));
}

/// O pedido do revisor final, que não tem onda, sai pela leitura
/// `request-review` igual byte a byte ao texto do último envio de revisão: o
/// envio de uma onda não entra, o envio de revisão mais novo vence o antigo, e
/// a spec sem envio de revisão devolve texto vazio. O pedido de uma onda segue
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
    assert!(none.status.success(), "{}", String::from_utf8_lossy(&none.stdout));
    assert!(none.stdout.is_empty(), "o envio de onda não é o da revisão: {}", String::from_utf8_lossy(&none.stdout));

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
    let agreed = read(root, "agreed");
    let saved = agreed["events"].as_array().expect("events").iter().find(|e| e["id"] == json!(v4)).cloned();
    assert_eq!(saved.map(|e| e["replaces"].clone()), Some(json!(v3)), "pelo código vai à vigente: {agreed}");

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
    let tips: Vec<u64> = agreed["events"]
        .as_array()
        .expect("events")
        .iter()
        .filter(|e| e["code"] == json!("MSTD-DEC-0099"))
        .filter_map(|e| e["id"].as_u64())
        .collect();
    assert_eq!(tips, vec![base + 1, base + 2], "as duas pontas seguem na leitura: {agreed}");
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

/// As leituras que o binário registrou, na ordem: o pedido e o item.
fn reads_recorded(root: &Path) -> Vec<(String, String)> {
    let path = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
    let log = mustard_core::io::spec_events::read(&path).expect("read").expect("the spec file");
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "call" && e.str_field("command") == Some("read"))
        .map(|e| (e.str_field("request").unwrap_or_default().to_string(), e.str_field("item").unwrap_or_default().to_string()))
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
