//! A conferência da prova de um critério, na gravação e no plano, e dos
//! comandos que a linha `Prova:` de uma tarefa nomeia, na gravação: a prova é
//! o comando que a rodada, o fechamento e a onda rodam, e quatro defeitos a
//! fazem passar ou falhar sem provar nada.
//!
//! O primeiro termo tem de ser um programa que roda: a frase escrita no lugar
//! do comando, ou o nome solto de um teste, vira um comando que o shell não
//! acha. O programa é procurado do jeito que o executor das provas o procura
//! ([`crate::shared::proc::resolves`]), com os diretórios de ferramenta do
//! usuário além do `PATH`, e não só pelo `PATH`. Comandos ligados por `;`
//! deixam só o último decidir. E a busca sozinha, sem `!` na frente, sai com
//! sucesso quando acha texto: a que devia sair vazia passa achando o resto.
//! E o `cargo test -p <pacote> --test <alvo>` com um alvo que o pacote não
//! tem é recusado pelo cargo antes de rodar teste nenhum: os alvos do pacote
//! saem dos manifestos do Cargo do projeto.
//!
//! Um critério já gravado não é conferido de novo por quem o lê ou o roda; só
//! a gravação nova e o plano passam por aqui.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::Refusal;

use crate::shared::proc::resolves;

/// As palavras que abrem uma construção do shell ou que o próprio shell
/// executa, sem programa no disco.
const SHELL_WORDS: &[&str] = &[
    "cd", "test", "[", "[[", "env", "exit", "export", "set", "source", ".", ":", "eval", "exec", "command", "time",
    "true", "false", "if", "for", "while", "until", "case",
];

/// As construções em que o ponto e vírgula faz parte da sintaxe.
const COMPOUND: &[&str] = &["if", "for", "while", "until", "case"];

/// Os programas de busca que saem com sucesso quando acham texto.
const SEARCH_PROGRAMS: &[&str] = &["grep", "egrep", "fgrep", "rg", "ag", "ack"];

/// O subcomando do `cargo` que roda os testes, e o apelido que o próprio
/// cargo traz para ele.
const CARGO_TEST: &[&str] = &["test", "t"];

/// As opções do `cargo` antes do subcomando que levam o valor na palavra
/// seguinte.
const CARGO_VALUED: &[&str] = &["--config", "-Z", "--color"];

/// Um pedaço da prova: uma palavra, já sem aspas, ou o operador que separa
/// comandos.
#[derive(Debug, PartialEq)]
enum Piece {
    Word(String),
    Op(String),
}

/// Divide a prova em palavras e operadores (`;`, `&&`, `||`, `|`, `&` e
/// quebra de linha). Aspas, barra invertida e o que está entre parênteses ou
/// crases não separam nada: `find . -exec x {} \;` e `node -e "a; b"` são um
/// comando só.
fn pieces(proof: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut open = false;
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut chars = proof.chars().peekable();
    let flush = |out: &mut Vec<Piece>, word: &mut String, open: &mut bool| {
        if *open {
            out.push(Piece::Word(std::mem::take(word)));
            *open = false;
        }
    };
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if c == '\\' && q == '"' {
                word.extend(chars.next());
            } else {
                word.push(c);
            }
            continue;
        }
        if depth > 0 {
            word.push(c);
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            continue;
        }
        match c {
            '\\' => {
                word.extend(chars.next());
                open = true;
            }
            '\'' | '"' | '`' => {
                quote = Some(c);
                open = true;
            }
            '(' => {
                word.push(c);
                open = true;
                depth = 1;
            }
            ' ' | '\t' | '\r' => flush(&mut out, &mut word, &mut open),
            '\n' | ';' => {
                flush(&mut out, &mut word, &mut open);
                out.push(Piece::Op(c.to_string()));
            }
            '&' if word.ends_with('>') || word.ends_with('<') => word.push(c),
            '&' | '|' => {
                flush(&mut out, &mut word, &mut open);
                let doubled = chars.next_if_eq(&c).is_some();
                out.push(Piece::Op(if doubled { format!("{c}{c}") } else { c.to_string() }));
            }
            _ => {
                word.push(c);
                open = true;
            }
        }
    }
    flush(&mut out, &mut word, &mut open);
    out
}

/// `true` quando `word` é uma atribuição de variável de ambiente à frente do
/// comando, como `PATH=...` ou `CARGO_TARGET_DIR=/tmp/x`.
fn env_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else { return false };
    !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `true` quando o primeiro termo do comando roda: é um caminho, uma
/// construção do shell, uma expansão que só o shell resolve, ou um programa
/// que `finds` acha.
fn runs(program: &str, finds: &impl Fn(&str) -> bool) -> bool {
    program.contains('/')
        || program.contains('\\')
        || program.starts_with(['(', '$'])
        || SHELL_WORDS.contains(&program)
        || finds(program)
}

/// `true` quando os argumentos de uma busca a deixam silenciosa: com `-q`, só
/// o código de saída conta, e quem escreve deixa claro que espera achar.
fn quiet(args: &[&str]) -> bool {
    args.iter().any(|arg| {
        matches!(*arg, "--quiet" | "--silent")
            || (arg.starts_with('-') && !arg.starts_with("--") && arg.len() > 1 && arg[1..].contains('q'))
    })
}

/// O subcomando do `git` nos argumentos `args`: o primeiro que não é opção,
/// pulando o valor de `-C` e de `-c`.
fn git_subcommand<'a>(args: &[&'a str]) -> Option<&'a str> {
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if matches!(*arg, "-C" | "-c") {
            rest.next();
        } else if !arg.starts_with('-') {
            return Some(arg);
        }
    }
    None
}

/// Os comandos simples da prova, cada um com as palavras dele, na ordem em
/// que os operadores os separam.
fn commands(all: &[Piece]) -> Vec<Vec<&str>> {
    all.split(|piece| matches!(piece, Piece::Op(_)))
        .map(|words| {
            words
                .iter()
                .filter_map(|piece| match piece {
                    Piece::Word(word) => Some(word.as_str()),
                    Piece::Op(_) => None,
                })
                .collect()
        })
        .collect()
}

/// O pacote e os alvos de teste que um `cargo test` cita, de `args`, as
/// palavras depois de `cargo`. `None` quando o comando não é um `cargo test`
/// com um pacote só e algum `--test`, ou quando ele aponta outra pasta
/// (`-C`, `--manifest-path`). O que vem depois de `--` é do programa de
/// teste, e não do cargo.
fn cited_test_targets<'a>(args: &[&'a str]) -> Option<(&'a str, Vec<&'a str>)> {
    let mut words = args.iter().copied();
    loop {
        let word = words.next()?;
        if CARGO_VALUED.contains(&word) {
            words.next();
        } else if word == "-C" {
            return None;
        } else if !word.starts_with(['-', '+']) {
            if !CARGO_TEST.contains(&word) {
                return None;
            }
            break;
        }
    }
    let mut packages = Vec::new();
    let mut targets = Vec::new();
    while let Some(word) = words.next() {
        let (flag, inline) = match word.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (word, None),
        };
        match flag {
            "--" => break,
            "--manifest-path" => return None,
            "-p" | "--package" => packages.push(inline.or_else(|| words.next())?),
            "--test" => targets.push(inline.or_else(|| words.next())?),
            _ if flag.len() > 2 && flag.starts_with("-p") && !flag.starts_with("--") => packages.push(&flag[2..]),
            _ => {}
        }
    }
    (packages.len() == 1 && !targets.is_empty()).then(|| (packages[0], targets))
}

/// O manifesto do Cargo da pasta `dir`, ou `None` quando ela não tem um que
/// se leia.
fn manifest(dir: &Path) -> Option<toml::Table> {
    toml::from_str(&std::fs::read_to_string(dir.join("Cargo.toml")).ok()?).ok()
}

/// `true` quando `name` casa com `pattern`, em que `*` vale qualquer trecho.
fn wildcard(pattern: &str, name: &str) -> bool {
    let mut parts: Vec<&str> = pattern.split('*').collect();
    let last = if parts.len() > 1 { parts.pop() } else { None };
    let Some(mut rest) = name.strip_prefix(parts[0]) else { return false };
    for part in &parts[1..] {
        let Some(at) = rest.find(part) else { return false };
        rest = &rest[at + part.len()..];
    }
    last.map_or(rest.is_empty(), |last| rest.ends_with(last))
}

/// As pastas que o membro `pattern` de um workspace aponta, a partir de
/// `root`: o caminho como veio ou, com `*` num trecho, cada pasta que o
/// trecho casa.
fn member_dirs(root: &Path, pattern: &str) -> Vec<PathBuf> {
    pattern.split('/').filter(|part| !part.is_empty() && *part != ".").fold(vec![root.to_path_buf()], |dirs, part| {
        if !part.contains('*') {
            return dirs.into_iter().map(|dir| dir.join(part)).collect();
        }
        dirs.iter()
            .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path()))
            .filter(|path| path.is_dir() && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| wildcard(part, name)))
            .collect()
    })
}

/// Os alvos de teste do pacote da pasta `dir`, com o manifesto `manifest`, em
/// ordem: os `[[test]]` declarados e, sem `autotests = false`, os que o cargo
/// acha sozinho, cada `tests/<alvo>.rs` e cada `tests/<alvo>/main.rs`.
fn test_targets(dir: &Path, manifest: &toml::Table) -> Vec<String> {
    let declared = manifest.get("test").and_then(toml::Value::as_array).into_iter().flatten();
    let mut targets: BTreeSet<String> =
        declared.filter_map(|target| target.get("name").and_then(toml::Value::as_str)).map(str::to_string).collect();
    let package = manifest.get("package");
    if package.and_then(|package| package.get("autotests")).and_then(toml::Value::as_bool) != Some(false) {
        for path in std::fs::read_dir(dir.join("tests")).into_iter().flatten().flatten().map(|entry| entry.path()) {
            let found = if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                path.file_stem()
            } else if path.join("main.rs").is_file() {
                path.file_name()
            } else {
                None
            };
            targets.extend(found.and_then(|name| name.to_str()).map(str::to_string));
        }
    }
    targets.into_iter().collect()
}

/// Os alvos de teste do pacote `package`, achado pelo nome no manifesto do
/// Cargo de `root` ou nos dos membros do workspace dele. `None` quando a raiz
/// não tem manifesto do Cargo ou nenhum deles é desse pacote.
fn package_test_targets(root: &Path, package: &str) -> Option<Vec<String>> {
    let top = manifest(root)?;
    let named = |manifest: &toml::Table| {
        manifest.get("package").and_then(|package| package.get("name")).and_then(toml::Value::as_str) == Some(package)
    };
    if named(&top) {
        return Some(test_targets(root, &top));
    }
    let members = top.get("workspace").and_then(|workspace| workspace.get("members")).and_then(toml::Value::as_array);
    members
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .flat_map(|member| member_dirs(root, member))
        .find_map(|dir| manifest(&dir).filter(named).map(|found| test_targets(&dir, &found)))
}

/// A recusa do primeiro `cargo test -p <pacote> --test <alvo>` da prova cujo
/// alvo o pacote não tem, com os alvos que ele tem; o alvo escrito como
/// padrão (`*`, `?`, `[`) e o pacote que os manifestos de `root` não acham
/// ficam sem conferência.
fn missing_test_target(root: &Path, all: &[Piece]) -> Option<Refusal> {
    for words in commands(all) {
        let mut at = words.iter().take_while(|word| env_assignment(word)).count();
        if words.get(at) == Some(&"!") {
            at += 1;
        }
        if words.get(at) != Some(&"cargo") {
            continue;
        }
        let Some((package, cited)) = cited_test_targets(&words[at + 1..]) else { continue };
        let Some(targets) = package_test_targets(root, package) else { continue };
        let missing = cited.into_iter().find(|target| !target.contains(['*', '?', '[']) && !targets.iter().any(|known| known == target));
        if let Some(target) = missing {
            return Some(Refusal::ProofTestTargetMissing { package: package.to_string(), target: target.to_string(), targets });
        }
    }
    None
}

/// O defeito da prova `proof`, com os manifestos do projeto em `root`, ou
/// `None` quando ela se sustenta. A prova em branco fica para a conferência
/// do campo obrigatório.
#[must_use]
pub(crate) fn proof_defect(root: &Path, proof: &str) -> Option<Refusal> {
    proof_defect_finding(root, proof, resolves)
}

/// O mesmo que [`proof_defect`], com `finds` dizendo quais programas existem:
/// a conferência não olha a máquina de quem a roda, e o teste dela escolhe os
/// programas que existem em vez de depender dos que estão instalados.
fn proof_defect_finding(root: &Path, proof: &str, finds: impl Fn(&str) -> bool) -> Option<Refusal> {
    let proof = proof.trim();
    if proof.is_empty() {
        return None;
    }
    let all = pieces(proof);
    let first: Vec<&str> = all
        .iter()
        .map_while(|piece| match piece {
            Piece::Word(word) => Some(word.as_str()),
            Piece::Op(_) => None,
        })
        .collect();
    let mut at = first.iter().take_while(|word| env_assignment(word)).count();
    let negated = first.get(at) == Some(&"!");
    if negated {
        at += 1;
    }
    let program = first.get(at).copied().unwrap_or_default();
    if !runs(program, &finds) {
        let term = if program.is_empty() { proof } else { program };
        return Some(Refusal::ProofProgramUnknown { term: term.to_string() });
    }
    if COMPOUND.contains(&program) {
        return None;
    }
    if all.contains(&Piece::Op(";".into())) {
        return Some(Refusal::ProofChainedBySemicolon { found: proof.to_string() });
    }
    let args = &first[at + 1..];
    let git_grep = program == "git" && git_subcommand(args) == Some("grep");
    let lone = all.iter().all(|piece| matches!(piece, Piece::Word(_)));
    if (git_grep || SEARCH_PROGRAMS.contains(&program)) && lone && !negated && !quiet(args) {
        return Some(Refusal::ProofSearchNotNegated { found: proof.to_string() });
    }
    missing_test_target(root, &all)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os programas que estes testes dão como instalados: a resposta não
    /// depende do que a máquina tem, como o `rg`, que o servidor não traz.
    const INSTALLED: [&str; 9] = ["cargo", "git", "grep", "rg", "node", "find", "echo", "npx", "npm"];

    fn reason(proof: &str) -> Option<&'static str> {
        proof_defect_finding(Path::new("."), proof, |program| INSTALLED.contains(&program)).map(|refusal| refusal.reason())
    }

    /// A prova escrita em prosa, ou com o nome solto de um teste, não tem
    /// programa para rodar; o programa que só está nos diretórios de
    /// ferramenta do usuário, e o que o shell entende sozinho, rodam.
    #[test]
    fn the_first_term_has_to_be_a_program_that_runs() {
        assert_eq!(reason("sai vazio"), Some("proof-program-unknown"));
        assert_eq!(reason("sum_comes_out_right"), Some("proof-program-unknown"));
        assert_eq!(reason("PATH=x sai vazio"), Some("proof-program-unknown"));
        assert_eq!(reason("! sai vazio"), Some("proof-program-unknown"));
        let named = proof_defect(Path::new("."), "sai vazio").unwrap().message(mustard_core::platform::i18n::Locale::PtBr);
        assert!(named.contains("sai"), "a recusa diz o termo que falhou: {named}");
        // A conferência de verdade acha o programa pelo executor das provas: o
        // que a máquina tem passa, e o que ela não tem é recusado.
        let shell = if cfg!(windows) { "cmd /c ver" } else { "sh -c true" };
        assert!(proof_defect(Path::new("."), shell).is_none(), "{shell}");
        assert!(proof_defect(Path::new("."), "mustard-definitely-not-a-real-program-xyz --help").is_some());
        for fine in [
            "cargo test",
            "git --version",
            "./target/debug/x --help",
            "/usr/bin/env true",
            "PATH=\"$HOME/.cargo/bin:$PATH\" cargo test --locked",
            "FOO=\"a b\" git --version",
            "! git --version",
            "cd . && git --version",
            "test -f Cargo.toml",
            "[ -f Cargo.toml ]",
            "env FOO=1 git --version",
            "(cd . && git --version)",
        ] {
            assert_eq!(reason(fine), None, "{fine}");
        }
    }

    /// Comandos ligados por `;` são recusados, e `&&` pede o mesmo; o ponto
    /// e vírgula entre aspas, escapado ou dentro de parênteses não liga nada,
    /// e a construção que o exige (`for`, `if`) passa.
    #[test]
    fn a_semicolon_between_commands_is_refused() {
        assert_eq!(reason("git --version ; git --help"), Some("proof-chained-by-semicolon"));
        assert_eq!(reason("cd . ; git --version"), Some("proof-chained-by-semicolon"));
        assert_eq!(reason("git --version;git --help"), Some("proof-chained-by-semicolon"));
        for fine in [
            "git --version && git --help",
            "git --version || git --help",
            "node -e \"a; b\"",
            "git grep -n 'a;b' -- src && git --version",
            "find . -name x -exec git --version \\;",
            "echo $(git --version; git --help)",
            "for f in a b; do git --version; done",
            "if git --version; then git --help; fi",
        ] {
            assert_eq!(reason(fine), None, "{fine}");
        }
    }

    /// A busca sozinha, sem `!` na frente e sem o modo silencioso, é
    /// recusada; com `!`, com `-q`, ligada a outro comando ou passada por um
    /// pipe, passa.
    #[test]
    fn a_lone_search_without_the_negation_is_refused() {
        assert_eq!(reason("git grep -n x"), Some("proof-search-not-negated"));
        assert_eq!(reason("grep -rn x src"), Some("proof-search-not-negated"));
        assert_eq!(reason("rg x"), Some("proof-search-not-negated"));
        assert_eq!(reason("git -C . grep x"), Some("proof-search-not-negated"));
        assert_eq!(reason("PATH=x git grep -n x"), Some("proof-search-not-negated"));
        for fine in [
            "! git grep -n x",
            "! grep -rn x src",
            "git grep -q x",
            "grep -rnq x src",
            "rg --quiet x",
            "git grep -n x | git --version",
            "git grep -n x && cargo test",
            "git --version && ! git grep -n x",
            "git log --grep x",
            "cargo test",
        ] {
            assert_eq!(reason(fine), None, "{fine}");
        }
    }

    /// O `cargo test` que cita, com `-p`, um alvo de teste que o pacote não
    /// tem é recusado com o alvo e os alvos do pacote: os `[[test]]`
    /// declarados e, sem `autotests = false`, os `tests/<alvo>.rs` e
    /// `tests/<alvo>/main.rs`. O pacote vem dos membros do workspace, também
    /// por `*`. O alvo que existe passa; o comando que o manifesto da raiz não
    /// decide, e a prova de outra ferramenta num projeto sem Cargo, passam.
    #[test]
    fn a_test_target_the_package_lacks_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let put = |path: &str, text: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        put("Cargo.toml", "[workspace]\nmembers = [\"apps/*\", \"packages/core\"]\n");
        put("apps/rt/Cargo.toml", "[package]\nname = \"mustard-rt\"\nautotests = false\n\n[[test]]\nname = \"it\"\npath = \"tests/it.rs\"\n");
        put("apps/rt/tests/it.rs", "mod agents_find_code_through_the_map;\n");
        put("apps/rt/tests/agents_find_code_through_the_map.rs", "");
        put("apps/scan/Cargo.toml", "[package]\nname = \"scan\"\n");
        put("apps/scan/tests/graph.rs", "");
        put("apps/scan/tests/e2e/main.rs", "");
        put("apps/scan/tests/fixtures/data.txt", "");
        put("packages/core/Cargo.toml", "[package]\nname = \"mustard-core\"\n");
        let defect = |proof: &str| proof_defect_finding(root, proof, |program| INSTALLED.contains(&program));

        let refused = defect("cargo test -p mustard-rt --test agents_find_code_through_the_map").unwrap();
        assert_eq!(
            refused,
            Refusal::ProofTestTargetMissing {
                package: "mustard-rt".into(),
                target: "agents_find_code_through_the_map".into(),
                targets: vec!["it".into()],
            }
        );
        let message = refused.message(mustard_core::platform::i18n::Locale::PtBr);
        assert!(message.contains("agents_find_code_through_the_map") && message.contains(": it."), "{message}");
        let none = defect("cargo test -p mustard-core --test it").unwrap().message(mustard_core::platform::i18n::Locale::EnUs);
        assert!(none.contains("are: none."), "{none}");
        for (proof, target, targets) in [
            ("cargo test -p scan --test nope", "nope", vec!["e2e", "graph"]),
            ("cargo build -j 2 && CARGO_TARGET_DIR=x cargo test --package=scan --test=gone", "gone", vec!["e2e", "graph"]),
            ("cargo +nightly t -pscan --test graph --test gone x", "gone", vec!["e2e", "graph"]),
        ] {
            let found = defect(proof);
            let expected = Refusal::ProofTestTargetMissing {
                package: "scan".into(),
                target: target.into(),
                targets: targets.into_iter().map(str::to_string).collect(),
            };
            assert_eq!(found, Some(expected), "{proof}");
        }
        for fine in [
            "cargo test -p mustard-rt --test it agents_find_code_through_the_map",
            "cargo test -j 2 -p mustard-rt --lib -- spec_events::proof_check",
            "cargo test -p scan --test graph && cargo test -p scan --test e2e",
            "cargo test -p mustard-rt agents_find_code_through_the_map",
            "cargo test -p mustard-rt -- --test gone",
            "cargo test -p mustard-rt --test 'agents*'",
            "cargo test -p mustard-rt -p scan --test graph",
            "cargo test -p not-in-the-workspace --test gone",
            "cargo test -p mustard-rt --manifest-path other/Cargo.toml --test gone",
            "cargo test --test gone",
        ] {
            assert_eq!(defect(fine), None, "{fine}");
        }

        // Um projeto em TypeScript, sem manifesto do Cargo: a prova de outra
        // ferramenta, e até um `cargo test` com alvo, não têm o que conferir.
        let web = tempfile::tempdir().unwrap();
        std::fs::write(web.path().join("package.json"), r#"{"name":"web","scripts":{"test":"vitest"}}"#).unwrap();
        for fine in ["npx vitest run src/sum.test.ts", "npm test --workspace web -- --test sum", "cargo test -p web --test sum"] {
            assert_eq!(proof_defect_finding(web.path(), fine, |program| INSTALLED.contains(&program)), None, "{fine}");
        }
    }

    /// A prova em branco fica para a conferência do campo obrigatório.
    #[test]
    fn a_blank_proof_is_left_to_the_required_field_check() {
        assert_eq!(reason(""), None);
        assert_eq!(reason("   "), None);
    }
}
