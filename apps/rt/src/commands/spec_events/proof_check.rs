//! A conferência da prova de um critério, na gravação e no plano: a prova é o
//! comando que a rodada e o fechamento rodam, e três defeitos a fazem passar
//! ou falhar sem provar nada.
//!
//! O primeiro termo tem de ser um programa que roda: a frase escrita no lugar
//! do comando, ou o nome solto de um teste, vira um comando que o shell não
//! acha. O programa é procurado do jeito que o executor das provas o procura
//! ([`crate::shared::proc::resolves`]), com os diretórios de ferramenta do
//! usuário além do `PATH`, e não só pelo `PATH`. Comandos ligados por `;`
//! deixam só o último decidir. E a busca sozinha, sem `!` na frente, sai com
//! sucesso quando acha texto: a que devia sair vazia passa achando o resto.
//!
//! Um critério já gravado não é conferido de novo por quem o lê ou o roda; só
//! a gravação nova e o plano passam por aqui.

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
/// que o executor das provas acha.
fn runs(program: &str) -> bool {
    program.contains('/')
        || program.contains('\\')
        || program.starts_with(['(', '$'])
        || SHELL_WORDS.contains(&program)
        || resolves(program)
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

/// O defeito da prova `proof`, ou `None` quando ela se sustenta. A prova em
/// branco fica para a conferência do campo obrigatório.
#[must_use]
pub(crate) fn proof_defect(proof: &str) -> Option<Refusal> {
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
    if !runs(program) {
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
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(proof: &str) -> Option<&'static str> {
        proof_defect(proof).map(|refusal| refusal.reason())
    }

    /// A prova escrita em prosa, ou com o nome solto de um teste, não tem
    /// programa para rodar; o programa que só está nos diretórios de
    /// ferramenta do usuário, e o que o shell entende sozinho, rodam.
    #[test]
    fn the_first_term_has_to_be_a_program_that_runs() {
        assert_eq!(reason("sai vazio"), Some("proof-program-unknown"));
        assert_eq!(reason("a_soma_sai_certa"), Some("proof-program-unknown"));
        assert_eq!(reason("PATH=x sai vazio"), Some("proof-program-unknown"));
        assert_eq!(reason("! sai vazio"), Some("proof-program-unknown"));
        let named = proof_defect("sai vazio").unwrap().message(mustard_core::platform::i18n::Locale::PtBr);
        assert!(named.contains("sai"), "a recusa diz o termo que falhou: {named}");
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

    /// A prova em branco fica para a conferência do campo obrigatório.
    #[test]
    fn a_blank_proof_is_left_to_the_required_field_check() {
        assert_eq!(reason(""), None);
        assert_eq!(reason("   "), None);
    }
}
