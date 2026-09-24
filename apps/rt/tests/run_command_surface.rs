//! O retrato da superfície publicada de `mustard-rt run`.
//!
//! Os nomes de `run <nome>` são chamados pelos ganchos, pelo `settings.json`,
//! pelos moldes e pela prosa do produto: um renome ou um registro perdido não
//! quebra a compilação — faz o comando SUMIR em tempo de execução. Este arquivo
//! transforma isso numa falha de teste.
//!
//! O retrato, e não uma lista escrita à mão: a superfície vive no arquivo
//! `tests/fixtures/run-surface.txt`, uma linha por nome em ordem alfabética, e
//! o teste compara a árvore do clap com ele. Quem acrescenta ou tira um comando regrava o arquivo
//! com o texto que a falha imprime — nada de manter a mesma lista em dois
//! lugares.
//!
//! O arquivo também lê as superfícies de instrução ENTREGUES (`plugin/**`): a
//! árvore do clap sozinha não pega um texto que promete um comando que o leitor
//! não vai achar, e uma instrução errada falha tão em silêncio quanto um
//! registro perdido.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Command, Subcommand};
use mustard_rt::commands::RunCmd;

/// O retrato da superfície, relativo à raiz do repositório.
const SURFACE_SNAPSHOT: &str = "apps/rt/tests/fixtures/run-surface.txt";

/// Instruction surfaces SHIPPED to the reader, relative to the repo root, with
/// the file extension each one is scanned through (`None` = every file).
///
/// `plugin/**/*.md` is what the agent loads at runtime (commands, refs, agent
/// prompts). Each entry is ASSERTED to exist and to yield files — a surface that
/// silently disappears would turn this guard into a green no-op.
const DOC_SURFACES: &[(&str, Option<&str>)] = &[("plugin", Some("md"))];

/// The repo root, resolved from this crate (`apps/rt`) so the scan does not
/// depend on the directory the test runner happens to start in.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Os nomes gravados no retrato, em ordem alfabética.
fn snapshot_names() -> Vec<String> {
    let path = repo_root().join(SURFACE_SNAPSHOT);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("o retrato da superfície não abriu: {}", path.display()));
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// Recursively collect files under `dir` in a deterministic (sorted) order,
/// keeping only `ext` when it is set. An unreadable directory yields nothing.
fn collect_files(dir: &Path, ext: Option<&str>, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name == "node_modules" || name == "target" || name == ".git" {
                continue;
            }
            collect_files(&path, ext, out);
        } else if ext
            .is_none_or(|want| path.extension().and_then(|e| e.to_str()).is_some_and(|e| e == want))
        {
            out.push(path);
        }
    }
}

/// Every `mustard-rt run <name>` token in `text`, in order of appearance.
///
/// All three invocation spellings are recognised — the same set
/// `template_parity`'s `CALLER_PREFIXES` feeds to its `extract_run_names`.
/// Recognising only the bare one
/// would let a Windows-flavoured hint (`mustard-rt.exe run …`) or a packaging
/// script (`$RtExe run …`) name a dead command and still pass.
///
/// A token runs to the first byte outside `[a-z0-9-]`, which drops the trailing
/// backtick / period / paren that normally closes the instruction in prose.
/// Placeholders are SKIPPED rather than reported: an explicit `<`, `{`, `$` or
/// backtick start teaches a shape, not a command, and any other non-token byte
/// (the `…` elision in `scan.md`) yields an empty token.
fn documented_run_tokens(text: &str) -> Vec<String> {
    const PREFIXES: &[&str] = &["mustard-rt run ", "mustard-rt.exe run ", "$RtExe run "];
    const PLACEHOLDER_STARTS: &[u8] = b"<{$`";

    let bytes = text.as_bytes();
    let mut out = Vec::new();
    // One cursor per spelling: each advances independently through the text, and
    // the run with the smallest next hit is consumed, so the tokens stay in order
    // of appearance no matter which spelling produced them.
    let mut cursors = vec![0usize; PREFIXES.len()];
    while let Some((which, pos)) = PREFIXES
        .iter()
        .enumerate()
        .filter_map(|(i, p)| text[cursors[i]..].find(p).map(|off| (i, cursors[i] + off)))
        .min_by_key(|(_, pos)| *pos)
    {
        let start = pos + PREFIXES[which].len();
        cursors[which] = start;
        // Every other cursor must clear this hit too, else the same region is
        // re-scanned forever by the spellings that did not match here.
        for (i, c) in cursors.iter_mut().enumerate() {
            if i != which && *c <= pos {
                *c = start;
            }
        }
        if start >= bytes.len() || PLACEHOLDER_STARTS.contains(&bytes[start]) {
            continue;
        }
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_lowercase() || bytes[end].is_ascii_digit() || bytes[end] == b'-')
        {
            end += 1;
        }
        if end > start {
            out.push(text[start..end].to_string());
        }
    }
    out
}

/// The `run` subcommand tree as clap materialises it.
fn run_command_tree() -> Command {
    let mut cmd = RunCmd::augment_subcommands(Command::new("run"));
    // `build()` materialises what the parser/help actually expose (it is what
    // adds the auto-generated `help` subcommand).
    cmd.build();
    cmd
}

/// A árvore do clap é igual ao retrato gravado, nome por nome.
#[test]
fn a_superficie_publicada_e_igual_ao_retrato() {
    let cmd = run_command_tree();
    let mut atual: Vec<String> =
        cmd.get_subcommands().map(|c| c.get_name().to_string()).collect();
    atual.sort();

    assert_eq!(
        atual,
        snapshot_names(),
        "a superfície de `run` mudou. Se a mudança é a pretendida, regrave \
         {SURFACE_SNAPSHOT} com estes nomes, um por linha:\n{}",
        atual.join("\n")
    );
}

/// Dois comandos no mesmo lugar da lista fariam o `run --help` embaralhar
/// sozinho: o clap ordena por `(display_order, name)`.
#[test]
fn nenhum_comando_divide_o_lugar_de_outro_na_ajuda() {
    let cmd = run_command_tree();
    let mut slots: Vec<usize> = cmd
        .get_subcommands()
        .filter(|c| c.get_name() != "help")
        .map(clap::Command::get_display_order)
        .collect();
    slots.sort_unstable();
    let mut unicos = slots.clone();
    unicos.dedup();
    assert_eq!(slots, unicos, "dois comandos declaram o mesmo `display_order`");
}

/// Every `mustard-rt run <name>` a SHIPPED instruction surface tells the reader
/// (or an agent) to type must be a name the CLI actually publishes.
///
/// Field defect: `wave-scaffold` was absorbed into
/// `plan-materialize`, but a shipped hint still told the reader to run it.
/// Nothing broke at build time — the command simply does not exist, so an
/// obedient agent burns a call on a clap error. `template_parity` runs the same
/// forward check over the template/plugin/packaging corpus; this one walks the
/// plugin tree as the reader's own instruction surface.
#[test]
fn every_documented_run_command_exists() {
    let root = repo_root();
    let publicados = snapshot_names();
    let mut offenders = Vec::new();

    for (rel, ext) in DOC_SURFACES {
        let dir = root.join(rel);
        // Assert the surface instead of skipping it (the idiom `template_parity`
        // already uses): a moved or renamed directory would otherwise make this
        // guard pass while scanning nothing — a dead guard reads exactly like a
        // clean one, which is the failure mode the whole test exists to prevent.
        assert!(dir.is_dir(), "declared instruction surface `{rel}` is missing — update DOC_SURFACES");
        let mut files = Vec::new();
        collect_files(&dir, *ext, &mut files);
        assert!(
            !files.is_empty(),
            "instruction surface `{rel}` yielded 0 files — the guard would pass vacuously"
        );
        for file in files {
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            for name in documented_run_tokens(&text) {
                if !publicados.contains(&name) {
                    let shown = file.strip_prefix(&root).unwrap_or(&file);
                    offenders.push(format!("{} -> `mustard-rt run {name}`", shown.display()));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "shipped instructions name `mustard-rt run` commands the CLI does not \
         publish — the call dies on a clap error at runtime, and the reader has \
         no way to tell. Fix the surface or register the command:\n{}",
        offenders.join("\n")
    );
}

/// The guard above is only as good as its tokenizer, and a tokenizer that
/// silently stops matching turns the whole test green-and-blind. Pin the three
/// spellings it must catch and the placeholder shapes it must ignore.
#[test]
fn documented_run_tokens_catches_every_spelling_and_skips_placeholders() {
    let found = documented_run_tokens(
        "run `mustard-rt run resume` first.\n\
         On Windows: `mustard-rt.exe run doctor`.\n\
         Packaging uses `$RtExe run upsert`.\n\
         Shapes teach nothing: `mustard-rt run <name>`, `mustard-rt run {kind}`, \
         `mustard-rt run $Cmd`.\n",
    );
    assert_eq!(
        found,
        vec!["resume", "doctor", "upsert"],
        "all three invocation spellings must be caught, in order, and every \
         placeholder skipped",
    );
    // Every name it caught here is real — the guard flags exactly the ones that
    // are not.
    let publicados = snapshot_names();
    for name in &found {
        assert!(publicados.contains(name), "{name} should be a real command");
    }
    assert_eq!(
        documented_run_tokens("`mustard-rt run wave-scaffold` (the shipped defect)"),
        vec!["wave-scaffold"],
        "the absorbed command must still be recognised as a name — that is what \
         makes the guard fail when a surface names it",
    );
    assert!(!publicados.contains(&"wave-scaffold".to_string()));
}

/// Quem vai mexer numa função pergunta ao mapa quem a usa, pelo comando que a
/// pessoa roda: `run map users --name <declaração>`. A resposta cita onde a
/// declaração mora e cada uso, como `arquivo:linha:quem chama`; um nome que o
/// mapa não declara é recusado com o texto de declaração desconhecida.
#[test]
fn o_mapa_devolve_quem_usa_uma_declaracao_pelo_nome() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".claude")).unwrap();
    // O mapa como o scan o grava: `total` em src/preco.rs, usada duas vezes
    // por `fechar`, em src/pedido.rs.
    fs::write(
        root.join(".claude/grain.model.json"),
        r#"{"modules": [
             {"path": "src/preco.rs", "loc": 5, "declarations": [
               {"kind": "function", "name": "total", "line": 1, "end_line": 3,
                "used_by": ["src/pedido.rs:5:fechar", "src/pedido.rs:6:fechar"]}]},
             {"path": "src/pedido.rs", "loc": 8, "declarations": [
               {"kind": "function", "name": "fechar", "line": 4, "end_line": 7}]}
           ]}"#,
    )
    .unwrap();
    let ask = |name: &str| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "map", "users", "--name", name, "--root"])
            .arg(root)
            .current_dir(root)
            .output()
            .expect("run map users");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
        (out.status.success(), report)
    };

    let (ok, report) = ask("total");
    assert!(ok, "{report}");
    let declarations = report["declarations"].as_array().unwrap();
    assert_eq!(declarations.len(), 1, "{report}");
    assert_eq!(declarations[0]["file"], "src/preco.rs", "{report}");
    assert_eq!(
        declarations[0]["used_by"],
        serde_json::json!(["src/pedido.rs:5:fechar", "src/pedido.rs:6:fechar"]),
        "os dois usos, com o arquivo, a linha e quem chama: {report}"
    );

    let (ok, report) = ask("nao_existe");
    assert!(!ok, "{report}");
    assert_eq!(report["reason"], "unknown-declaration", "{report}");
    let hint = report["hint"].as_str().unwrap();
    assert!(hint.contains("nao_existe"), "a recusa diz o nome: {report}");
    assert!(hint.contains("Confira o nome"), "o texto de declaração desconhecida: {report}");
}

/// A ajuda do `run pending`, como o usuário a pede, escreve o número de uma
/// pendência como `P-N`, inteiro na linha da opção: nenhum `P-` fica partido
/// por uma quebra no lugar do número.
#[test]
fn the_pending_help_shows_the_item_id_as_p_n() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "pending", "--help"])
        .output()
        .expect("mustard-rt run pending --help");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let help = String::from_utf8_lossy(&out.stdout);

    let close = help.lines().find(|line| line.trim_start().starts_with("--close")).expect("the --close line");
    assert!(close.contains("`P-N` as DELIVERED"), "the id stays on the option's line: {close}");
    let spelled: Vec<String> = help.match_indices("P-").map(|(at, _)| help[at..].chars().take(3).collect()).collect();
    assert!(spelled.len() >= 6, "the summary and the five options name the id: {help}");
    assert!(spelled.iter().all(|id| *id == "P-N"), "every id is spelled P-N: {spelled:?}\n{help}");
}
