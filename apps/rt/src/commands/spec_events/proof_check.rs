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
/// acha sozinho, cada `tests/<alvo>.rs` e cada `tests/<alvo>/main.rs` — no
/// disco ou entre os arquivos que o trabalho declara criar, `created`,
/// relativos a `root`.
fn test_targets(root: &Path, dir: &Path, manifest: &toml::Table, created: &BTreeSet<String>) -> Vec<String> {
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
        let tests = dir.strip_prefix(root).unwrap_or(dir).join("tests");
        let tests = tests.to_string_lossy().replace('\\', "/");
        let tests = tests.trim_start_matches("./");
        for path in created {
            let Some(rest) = path.strip_prefix(tests).and_then(|rest| rest.strip_prefix('/')) else { continue };
            let found = match rest.split_once('/') {
                Some((name, "main.rs")) => Some(name),
                None => rest.strip_suffix(".rs"),
                Some(_) => None,
            };
            targets.extend(found.filter(|name| !name.is_empty()).map(str::to_string));
        }
    }
    targets.into_iter().collect()
}

/// Os alvos de teste do pacote `package`, achado pelo nome no manifesto do
/// Cargo de `root` ou nos dos membros do workspace dele, com os que o
/// trabalho declara criar em `created`. `None` quando a raiz não tem
/// manifesto do Cargo ou nenhum deles é desse pacote.
fn package_test_targets(root: &Path, package: &str, created: &BTreeSet<String>) -> Option<Vec<String>> {
    let top = manifest(root)?;
    let named = |manifest: &toml::Table| {
        manifest.get("package").and_then(|package| package.get("name")).and_then(toml::Value::as_str) == Some(package)
    };
    if named(&top) {
        return Some(test_targets(root, root, &top, created));
    }
    let members = top.get("workspace").and_then(|workspace| workspace.get("members")).and_then(toml::Value::as_array);
    members
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .flat_map(|member| member_dirs(root, member))
        .find_map(|dir| manifest(&dir).filter(named).map(|found| test_targets(root, &dir, &found, created)))
}

/// O começo de um comando simples: quantas palavras vêm antes do programa
/// (as atribuições de ambiente e o `!`) e se o `!` está lá.
fn program_at(words: &[&str]) -> (usize, bool) {
    let at = words.iter().take_while(|word| env_assignment(word)).count();
    if words.get(at) == Some(&"!") { (at + 1, true) } else { (at, false) }
}

/// A recusa do primeiro `cargo test -p <pacote> --test <alvo>` da prova cujo
/// alvo o pacote não tem, nem o trabalho declara criar (`created`), com os
/// alvos que ele tem; o alvo escrito como padrão (`*`, `?`, `[`) e o pacote
/// que os manifestos de `root` não acham ficam sem conferência.
fn missing_test_target(root: &Path, all: &[Piece], created: &BTreeSet<String>) -> Option<Refusal> {
    for words in commands(all) {
        let (at, _) = program_at(&words);
        if words.get(at) != Some(&"cargo") {
            continue;
        }
        let Some((package, cited)) = cited_test_targets(&words[at + 1..]) else { continue };
        let Some(targets) = package_test_targets(root, package, created) else { continue };
        let missing = cited.into_iter().find(|target| !target.contains(['*', '?', '[']) && !targets.iter().any(|known| known == target));
        if let Some(target) = missing {
            return Some(Refusal::ProofTestTargetMissing { package: package.to_string(), target: target.to_string(), targets });
        }
    }
    None
}

/// Os programas cujo primeiro argumento solto é um padrão ou um roteiro, e
/// não um arquivo.
const PATTERN_FIRST: &[&str] = &["grep", "egrep", "fgrep", "rg", "ag", "ack", "sed", "awk", "jq", "perl"];

/// Os programas que criam, movem ou apagam os caminhos que recebem, e as
/// construções que conferem se um caminho existe: o caminho que eles citam é
/// o assunto do comando, e não algo que precisa existir antes dele.
const PATH_SUBJECT: &[&str] = &["test", "[", "[[", "mkdir", "touch", "tee", "cp", "mv", "ln", "rm", "install", "rsync"];

/// Os programas que só escrevem o texto que recebem: a palavra com barra que
/// eles citam é texto, e não caminho.
const TEXT_ONLY: &[&str] = &["echo", "printf"];

/// As opções que mandam a ferramenta trabalhar noutra pasta: o caminho
/// relativo do comando que as traz deixa de ser relativo à raiz.
const ELSEWHERE: &[&str] = &[
    "-C", "--prefix", "--cwd", "--dir", "--directory", "--filter", "--workspace", "--project", "--manifest-path", "--rootdir", "--root-dir",
];

/// O caminho relativo que a palavra `word` de um comando cita, já sem o `./`
/// da frente, a barra do fim, o `/...` com que o `go` pede os pacotes de
/// baixo e o `::` com que o teste é escolhido dentro do arquivo; `None`
/// quando ela não tem cara de caminho — opção, expansão, padrão,
/// redirecionamento, endereço, caminho absoluto, ou palavra sem barra que
/// não termina em extensão de código nem segue a convenção de testes do
/// mapa.
fn cited_path(word: &str) -> Option<String> {
    let word = word.split_once("::").map_or(word, |(path, _)| path);
    let path = relative_dir(word)?;
    let named = word.contains('/')
        || mustard_core::domain::source_lang::language_of_path(&path).is_some()
        || mustard_core::domain::ast::is_test_path(&path) && path.contains('.');
    named.then_some(path)
}

/// A palavra `word` como caminho relativo, sem o `./` da frente, a barra do
/// fim e o `/...` do `go`; `None` quando ela não é um caminho relativo
/// escrito só com letras, números e `_-./+` — o pacote com escopo
/// (`@escopo/nome`), o caminho absoluto e a opção também ficam de fora.
fn relative_dir(word: &str) -> Option<String> {
    let plain = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '+');
    if word.is_empty() || word.starts_with(['-', '/']) || word.contains("//") || !word.chars().all(plain) {
        return None;
    }
    let path = word.strip_suffix("/...").unwrap_or(word).trim_end_matches('/');
    let path = path.strip_prefix("./").unwrap_or(path);
    (!path.is_empty()).then(|| path.to_string())
}

/// `true` quando o trabalho declara criar o caminho `path` ou algo dentro
/// dele (`created`).
fn declared(path: &str, created: &BTreeSet<String>) -> bool {
    let inside = format!("{path}/");
    created.iter().any(|file| file == path || file.starts_with(&inside))
}

/// A recusa do primeiro caminho relativo que um comando da prova cita e que
/// não existe no projeto em `root`, nem o trabalho declara criar
/// (`created`), em qualquer ferramenta: o comando que roda com um arquivo de
/// teste que não existe falha só quando a onda o roda, ou passa sem provar
/// nada. O `cd` muda a pasta de que os caminhos seguintes partem; o comando
/// que trabalha noutra pasta ([`ELSEWHERE`]), o que tem o caminho como
/// assunto ([`PATH_SUBJECT`]) e o que só escreve texto ([`TEXT_ONLY`]) ficam
/// sem conferência; o `git` só tem os
/// caminhos depois de `--`; e o primeiro argumento de uma busca é o padrão.
/// Depois de um `cd` que não se segue (`cd -`, `cd ~`), nada mais é conferido.
fn missing_path(root: &Path, all: &[Piece], created: &BTreeSet<String>) -> Option<Refusal> {
    let mut base = Some(PathBuf::new());
    for words in commands(all) {
        let Some(here) = base.clone() else { break };
        let (at, _) = program_at(&words);
        let Some(&program) = words.get(at) else { continue };
        let args = &words[at + 1..];
        if PATH_SUBJECT.contains(&program) || TEXT_ONLY.contains(&program) || args.iter().any(|arg| ELSEWHERE.contains(&arg.split('=').next().unwrap_or(arg))) {
            continue;
        }
        let mut cited: Vec<&str> = Vec::new();
        let mut redirected = false;
        let mut skip_pattern = PATTERN_FIRST.contains(&program) || (program == "git" && git_subcommand(args) == Some("grep"));
        let mut after_dashes = false;
        for arg in args {
            if redirected {
                redirected = false;
                continue;
            }
            if arg.contains(['>', '<']) {
                redirected = arg.ends_with(['>', '<']);
                continue;
            }
            if *arg == "--" {
                after_dashes = true;
                continue;
            }
            if arg.starts_with('-') {
                continue;
            }
            if skip_pattern {
                skip_pattern = false;
                continue;
            }
            if program != "git" || after_dashes {
                cited.push(arg);
            }
        }
        let paths: Vec<String> = if program == "cd" {
            let dir = match (cited.as_slice(), args.len()) {
                ([dir], 1) => relative_dir(dir),
                _ => None,
            };
            base = dir.as_ref().map(|dir| if dir == "." { here.clone() } else { here.join(dir) });
            dir.into_iter().filter(|dir| dir != ".").collect()
        } else {
            cited.into_iter().filter_map(cited_path).collect()
        };
        for path in paths {
            let relative = here.join(&path);
            let shown = relative.to_string_lossy().replace('\\', "/");
            if !root.join(&relative).exists() && !declared(&shown, created) {
                return Some(Refusal::ProofPathMissing { path: shown });
            }
        }
    }
    None
}

/// O caminho `path` como os arquivos declarados se comparam: com `/`, sem
/// espaços nas pontas e sem o `./` da frente.
pub(crate) fn relative_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    path.strip_prefix("./").map_or_else(|| path.clone(), str::to_string)
}

/// Os arquivos que a tarefa de campos `fields` declara criar: os de `files`
/// marcados com `new: true` ([`relative_path`]).
pub(crate) fn created_files(fields: &serde_json::Map<String, serde_json::Value>) -> BTreeSet<String> {
    let files = fields.get("files").and_then(serde_json::Value::as_array).into_iter().flatten();
    files
        .filter(|file| file.get("new").and_then(serde_json::Value::as_bool) == Some(true))
        .filter_map(|file| file.get("path").and_then(serde_json::Value::as_str))
        .map(relative_path)
        .filter(|path| !path.is_empty())
        .collect()
}

/// O defeito da prova `proof`, com o projeto em `root` e os arquivos que o
/// trabalho declara criar em `created` (relativos à raiz, com `/`), ou
/// `None` quando ela se sustenta. A prova em branco fica para a conferência
/// do campo obrigatório.
#[must_use]
pub(crate) fn proof_defect(root: &Path, proof: &str, created: &BTreeSet<String>) -> Option<Refusal> {
    proof_defect_finding(root, proof, created, resolves)
}

/// O mesmo que [`proof_defect`], com `finds` dizendo quais programas existem:
/// a conferência não olha a máquina de quem a roda, e o teste dela escolhe os
/// programas que existem em vez de depender dos que estão instalados.
fn proof_defect_finding(root: &Path, proof: &str, created: &BTreeSet<String>, finds: impl Fn(&str) -> bool) -> Option<Refusal> {
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
    missing_test_target(root, &all, created).or_else(|| missing_path(root, &all, created))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os programas que estes testes dão como instalados: a resposta não
    /// depende do que a máquina tem, como o `rg`, que o servidor não traz.
    const INSTALLED: [&str; 9] = ["cargo", "git", "grep", "rg", "node", "find", "echo", "npx", "npm"];

    fn reason(proof: &str) -> Option<&'static str> {
        proof_defect_finding(Path::new("."), proof, &BTreeSet::new(), |program| INSTALLED.contains(&program)).map(|refusal| refusal.reason())
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
        let named = proof_defect(Path::new("."), "sai vazio", &BTreeSet::new()).unwrap().message(mustard_core::platform::i18n::Locale::PtBr);
        assert!(named.contains("sai"), "a recusa diz o termo que falhou: {named}");
        // A conferência de verdade acha o programa pelo executor das provas: o
        // que a máquina tem passa, e o que ela não tem é recusado.
        let shell = if cfg!(windows) { "cmd /c ver" } else { "sh -c true" };
        assert!(proof_defect(Path::new("."), shell, &BTreeSet::new()).is_none(), "{shell}");
        assert!(proof_defect(Path::new("."), "mustard-definitely-not-a-real-program-xyz --help", &BTreeSet::new()).is_some());
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
        let defect = |proof: &str| proof_defect_finding(root, proof, &BTreeSet::new(), |program| INSTALLED.contains(&program));

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
        // O pacote sem alvo nenhum tem frase própria, sem lista vazia.
        let none = defect("cargo test -p mustard-core --test it").unwrap().message(mustard_core::platform::i18n::Locale::EnUs);
        assert!(none.contains("has no test target at all") && !none.contains(": ."), "{none}");
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

        // O alvo que a tarefa declara criar, marcado como novo na lista de
        // arquivos, conta como alvo do pacote quando o cargo acha os alvos
        // sozinho; com `autotests = false`, o arquivo novo não vira alvo.
        let created: BTreeSet<String> =
            ["packages/core/tests/novo.rs", "apps/scan/tests/fresh/main.rs", "apps/rt/tests/novo.rs"].map(str::to_string).into();
        let with_new = |proof: &str| proof_defect_finding(root, proof, &created, |program| INSTALLED.contains(&program));
        assert_eq!(with_new("cargo test -p mustard-core --test novo"), None);
        assert_eq!(with_new("cargo test -p scan --test fresh"), None);
        assert_eq!(with_new("cargo test -p mustard-rt --test novo").map(|refusal| refusal.reason()), Some("proof-test-target-missing"));
        assert_eq!(defect("cargo test -p mustard-core --test novo").map(|refusal| refusal.reason()), Some("proof-test-target-missing"));

        // Um projeto em TypeScript, sem manifesto do Cargo: o `cargo test`
        // com alvo não tem o que conferir.
        let web = tempfile::tempdir().unwrap();
        std::fs::write(web.path().join("package.json"), r#"{"name":"web","scripts":{"test":"vitest"}}"#).unwrap();
        for fine in ["npm test --workspace web -- --test sum", "cargo test -p web --test sum"] {
            assert_eq!(proof_defect_finding(web.path(), fine, &BTreeSet::new(), |program| INSTALLED.contains(&program)), None, "{fine}");
        }
    }

    /// Em qualquer ferramenta, o caminho relativo que a prova cita e que o
    /// projeto não tem é recusado, dizendo o caminho: o arquivo de teste do
    /// vitest, do pytest, a pasta do `go test` e a do `dotnet test`, também
    /// depois de um `cd`. O caminho que existe passa, e o que a tarefa
    /// declara criar também. O padrão de uma busca, o argumento do `git`
    /// antes de `--`, o pacote com escopo, o comando que trabalha noutra
    /// pasta, o que tem o caminho como assunto, o texto que o `echo` e o
    /// `printf` escrevem e o redirecionamento ficam sem conferência.
    #[test]
    fn a_path_the_project_lacks_is_refused_in_any_tool() {
        let web = tempfile::tempdir().unwrap();
        let root = web.path();
        for (path, text) in [
            ("package.json", r#"{"name":"web","scripts":{"test":"vitest"}}"#),
            ("src/sum.ts", "export const sum = (a: number, b: number) => a + b;\n"),
            ("src/sum.test.ts", ""),
            ("apps/site/src/page.test.ts", ""),
            ("docs/here.md", ""),
        ] {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        let created: BTreeSet<String> = ["src/novo.test.ts"].map(str::to_string).into();
        // Toda ferramenta conta como instalada: o caso aqui é só o caminho.
        let defect = |proof: &str| proof_defect_finding(root, proof, &created, |_| true);

        for (proof, path) in [
            ("npx vitest run src/nao_existe.test.ts", "src/nao_existe.test.ts"),
            ("npx vitest run sum.test.ts", "sum.test.ts"),
            ("pytest tests/test_nao_existe.py::test_soma", "tests/test_nao_existe.py"),
            ("go test ./pkg/gone/...", "pkg/gone"),
            ("dotnet test tests/App.Tests", "tests/App.Tests"),
            ("cd apps/site && npx vitest run src/gone.test.ts", "apps/site/src/gone.test.ts"),
            ("cd apps/gone && npx vitest run", "apps/gone"),
            ("! rg -n 'a/b' docs/gone.md", "docs/gone.md"),
            ("npm test -- src/nao_existe.test.ts", "src/nao_existe.test.ts"),
        ] {
            assert_eq!(defect(proof), Some(Refusal::ProofPathMissing { path: path.into() }), "{proof}");
        }
        let message = defect("npx vitest run src/nao_existe.test.ts").unwrap().message(mustard_core::platform::i18n::Locale::PtBr);
        assert!(message.contains("src/nao_existe.test.ts"), "a recusa diz o caminho: {message}");

        for fine in [
            "npx vitest run src/sum.test.ts",
            "npx vitest run ./src/sum.test.ts",
            "npx vitest run src/novo.test.ts",
            "cd apps/site && npx vitest run src/page.test.ts",
            "go test ./...",
            "! rg -n 'x/y' docs/here.md",
            "grep -q 'src/gone.ts' docs/here.md",
            "git log origin/main",
            "git diff --exit-code -- src/sum.ts",
            "npx @biomejs/biome check src",
            "npm test --workspace web -- src/gone.test.ts",
            "test -f src/gone.ts",
            "! test -e src/gone.ts",
            "node -e \"require('./src/gone.js')\"",
            "npx vitest run > out/log.txt",
            "cargo test -j 2 -p web --lib -- flow::plan",
            "cd - && npx vitest run src/gone.test.ts",
            "echo ok x/pkg 0.002s && echo ok x/outro",
            "printf 'src/gone.ts\\n'",
        ] {
            assert_eq!(defect(fine), None, "{fine}");
        }
    }

    /// A prova em branco fica para a conferência do campo obrigatório.
    #[test]
    fn a_blank_proof_is_left_to_the_required_field_check() {
        assert_eq!(reason(""), None);
        assert_eq!(reason("   "), None);
    }
}
