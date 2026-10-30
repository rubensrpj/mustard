//! `shell_edit` — num agente de onda, o arquivo do projeto muda só pela
//! ferramenta de edição do hospedeiro, nunca pelo terminal.
//!
//! A regra é de sintaxe e não adivinha o que o programa faz. No agente de
//! onda, ela recusa:
//!
//! - o interpretador (`python`, `py`, `node`, `perl`, `ruby`, `bun`, `php`), também
//!   o rodado por um lançador (`uv run`, `poetry run`), com o programa na
//!   própria linha (`-c`, `-e`, heredoc, `<<<` ou o texto que `echo`,
//!   `printf` ou `cat` lhe passa pelo cano), quando o texto dele cita um
//!   caminho que existe no projeto ou na pasta das cópias;
//! - o interpretador que roda um programa guardado fora do projeto, como o da
//!   pasta temporária, pelo nome, pelo `<` ou pelo `cat` que o passa pelo
//!   cano;
//! - o interpretador com o programa na linha ou pela entrada cuja saída chega
//!   a um arquivo do projeto, mesmo que o texto dele não cite caminho nenhum:
//!   pelo redirecionamento dele (`python3 -c "print(1)" > src/lib.rs`), pelo
//!   de um comando que a recebe pelo cano (`| head > src/lib.rs`) ou pelo do
//!   grupo que termina nele (`( … ) > src/lib.rs`);
//! - o lançador que roda, sem nome de interpretador, um arquivo de programa
//!   guardado fora do projeto (`uv run /tmp/edit.py`);
//! - a edição no lugar (`sed -i`, `perl -i`, `ruby -i`);
//! - `cat`, `echo`, `printf`, `tee`, `sed` e `awk` gravando no projeto ou numa
//!   cópia (`>`, `>>`, o arquivo do `tee` ou o redirecionamento do grupo);
//! - `mv` e `cp` que trazem para dentro do projeto um arquivo guardado fora
//!   dele ou de lugar que só o terminal sabe, como a troca pela pasta
//!   temporária (`sed … > /tmp/x.rs && mv /tmp/x.rs src/lib.rs`, `mv "$tmp"
//!   src/lib.rs`).
//!
//! Passam o programa na linha sem caminho do projeto, o script do projeto
//! pelo nome ou pela entrada (`python scripts/x.py`, `python3 <
//! scripts/x.py`, `python -m pytest`), a ferramenta instalada rodada pelo
//! lançador, também pelo caminho inteiro (`uv run /usr/bin/env pytest`), o
//! gerador com a saída redirecionada (`cargo run --bin gen > src/gen.rs`), a
//! leitura, a gravação fora do projeto e o `mv`/`cp` entre arquivos do
//! projeto ou para fora dele.
//! A conversa principal não passa pela recusa; a medição do
//! uso real conta, pela mesma regra ([`edited_by_shell`]), as edições por
//! script de toda conversa do projeto, e reconhece o interpretador, o
//! `python` e o programa escrito na linha pela mesma gramática
//! ([`is_interpreter`], [`is_python`], [`embedded_programs`]), sem lista
//! própria de nomes.

use super::lex::{command_start, Redirect, Segment, Word};
use crate::hooks::write::wave_copy::Shell;
use mustard_core::domain::model::contract::{Ctx, HookInput, Verdict};
use mustard_core::io::wave_prompt;
use mustard_core::platform::i18n::Locale;
use std::path::{Path, PathBuf};

/// Como um interpretador lê as opções da linha de comando.
struct Grammar {
    /// O nome da linguagem: o que `python`, `python3`, `python3.12` e `py`
    /// têm em comum.
    name: &'static str,
    /// Letras curtas que trazem o programa, na palavra seguinte ou, com
    /// `glued_code`, no resto do grupo.
    code: &'static str,
    /// O resto do grupo depois de uma letra de `code` é o programa
    /// (`perl -e'print 1'`); sem isso, o resto são outras letras
    /// (`node -pe "1"`).
    glued_code: bool,
    /// Letras curtas com valor no resto do grupo ou na palavra seguinte.
    value: &'static str,
    /// Letras curtas cujo valor é só o resto do grupo, colado a elas.
    attached: &'static str,
    /// Letra curta que roda um módulo instalado, e não um arquivo.
    module: &'static str,
    /// Letra curta que edita os arquivos no lugar.
    in_place: &'static str,
    /// Opções longas que trazem o programa (`--eval`).
    long_code: &'static [&'static str],
    /// Opções longas com valor na palavra seguinte, quando não vem com `=`.
    long_value: &'static [&'static str],
    /// A palavra que, antes do arquivo, só pede para rodá-lo (`bun run x.ts`).
    runner: Option<&'static str>,
    /// As extensões do arquivo de programa da linguagem, sem o ponto.
    extensions: &'static [&'static str],
}

const PYTHON: Grammar = Grammar {
    name: "python",
    code: "c",
    glued_code: true,
    value: "WX",
    attached: "",
    module: "m",
    in_place: "",
    long_code: &[],
    long_value: &["--check-hash-based-pycs"],
    runner: None,
    extensions: &["py", "pyw"],
};

const NODE: Grammar = Grammar {
    name: "node",
    code: "ep",
    glued_code: false,
    value: "rC",
    attached: "",
    module: "",
    in_place: "",
    long_code: &["--eval", "--print"],
    long_value: &["--require", "--import", "--loader", "--experimental-loader", "--conditions", "--input-type"],
    runner: None,
    extensions: &["js", "mjs", "cjs", "ts", "mts", "cts"],
};

const BUN: Grammar = Grammar { name: "bun", runner: Some("run"), ..NODE };

const PERL: Grammar = Grammar {
    name: "perl",
    code: "eE",
    glued_code: true,
    value: "",
    attached: "IMmFxdDC",
    module: "",
    in_place: "i",
    long_code: &[],
    long_value: &[],
    runner: None,
    extensions: &["pl", "pm"],
};

const RUBY: Grammar = Grammar {
    name: "ruby",
    code: "e",
    glued_code: true,
    value: "IrCE",
    attached: "FxKTW",
    module: "",
    in_place: "i",
    long_code: &[],
    long_value: &[],
    runner: None,
    extensions: &["rb"],
};

/// O `-r` traz o programa; o `-B`, o `-R` e o `-E`, o trecho de antes, de
/// cada linha e de depois da entrada. O arquivo do `-f` e do `-F` é o primeiro
/// nome fora das opções.
const PHP: Grammar = Grammar {
    name: "php",
    code: "rBRE",
    glued_code: true,
    value: "cdtSz",
    attached: "",
    module: "",
    in_place: "",
    long_code: &["--run", "--process-begin", "--process-code", "--process-end"],
    long_value: &["--php-ini", "--define", "--docroot", "--server", "--zend-extension"],
    runner: None,
    extensions: &["php"],
};

/// As gramáticas de todos os interpretadores.
const GRAMMARS: &[&Grammar] = &[&PYTHON, &NODE, &BUN, &PERL, &RUBY, &PHP];

/// A gramática do interpretador `name`, com ou sem a pasta, ou `None` para
/// outro programa. `py` é o lançador do Python no Windows, com as mesmas
/// opções (`py -3 -c …`).
fn grammar(name: &str) -> Option<&'static Grammar> {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    match name {
        "python" | "python3" | "py" => Some(&PYTHON),
        _ if name.starts_with("python3.") => Some(&PYTHON),
        "node" | "nodejs" => Some(&NODE),
        "bun" => Some(&BUN),
        "perl" => Some(&PERL),
        "ruby" => Some(&RUBY),
        "php" => Some(&PHP),
        _ => None,
    }
}

/// Os lançadores que rodam, depois de `run` e das opções deles, outro
/// programa (`uv run --with x python -c …`), cada um com as opções que levam o
/// valor na palavra seguinte (`conda run -n py python`): o valor delas não é o
/// programa.
const LAUNCHERS: &[(&str, &[&str])] = &[
    (
        "uv",
        &[
            "--with",
            "--with-editable",
            "--with-requirements",
            "--python",
            "-p",
            "--project",
            "--directory",
            "--package",
            "--group",
            "--extra",
            "--env-file",
            "--config-file",
            "--cache-dir",
            "--index",
            "--index-url",
            "--extra-index-url",
            "--find-links",
            "-f",
        ],
    ),
    ("poetry", &["--directory", "-C", "--project", "-P"]),
    ("pipenv", &["--python"]),
    ("pdm", &["--project"]),
    ("hatch", &["--env", "-e", "--include", "-i", "--exclude", "-x"]),
    ("rye", &["--pyproject"]),
    ("conda", &["-n", "--name", "-p", "--prefix", "--cwd"]),
];

/// Os programas que o lançador de `segment` pode rodar, cada um com os
/// argumentos dele: a primeira palavra depois do `run`, pulando as opções do
/// lançador, o valor das que o levam e os invólucros (`uv run env python`).
/// Essa palavra, logo depois de uma opção fora da lista, pode ser o valor da
/// opção (`uv run --color never python`): quando ela não é um interpretador,
/// a seguinte também pode ser o programa. Vazio para o comando que não é um
/// lançador com `run`.
fn launched(segment: &Segment) -> Vec<(&Word, &[Word])> {
    let Some((_, valued)) = LAUNCHERS.iter().find(|(name, _)| *name == segment.name()) else {
        return Vec::new();
    };
    let Some((_, rest)) = segment.args.split_first().filter(|(run, _)| run.text == "run") else {
        return Vec::new();
    };
    let program = move |at: usize| {
        let start = at + command_start(rest.get(at..).unwrap_or_default()).0;
        rest.get(start).map(|word| (word, &rest[start + 1..]))
    };
    let mut unknown = false;
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        match word.text.as_str() {
            "--" => return program(at + 1).into_iter().collect(),
            option if option.len() > 1 && option.starts_with('-') => {
                let takes = valued.contains(&option);
                unknown = !takes && !option.contains('=');
                at += 1 + usize::from(takes);
            }
            _ => {
                let first = program(at);
                let hidden = unknown && !first.is_some_and(|(word, _)| grammar(&word.text).is_some());
                return first.into_iter().chain(program(at + 1).filter(|_| hidden)).collect();
            }
        }
    }
    Vec::new()
}

/// O interpretador que `segment` roda, com os argumentos dele: o próprio
/// programa ou, num lançador, o primeiro dos programas que ele pode rodar que
/// é um interpretador.
fn interpreter(segment: &Segment) -> Option<(&'static Grammar, &[Word])> {
    if let Some(grammar) = grammar(segment.name()) {
        return Some((grammar, &segment.args));
    }
    launched(segment).into_iter().find_map(|(program, args)| Some((grammar(&program.text)?, args)))
}

/// O comando que passa a saída pelo cano a `line[at]`: o anterior no mesmo
/// nível, quando encadeado a ele. Os comandos de dentro dele (`$(…)`) vêm
/// logo depois dele, mais fundos, e ficam de fora.
fn feeder(line: &[Segment], at: usize) -> Option<&Segment> {
    let depth = line[at].depth;
    line[..at].iter().rev().find(|segment| segment.depth <= depth).filter(|segment| segment.depth == depth && segment.piped)
}

/// O programa que `line[at]` lê da entrada padrão vem de um arquivo: o do `<`
/// ou um dos que o `cat` lhe passa pelo cano. Esse arquivo é um script, como
/// o dado pelo nome.
fn from_file(line: &[Segment], at: usize) -> bool {
    line[at].redirects.iter().any(|redirect| redirect.op.trim_start_matches(|c: char| c.is_ascii_digit()) == "<")
        || feeder(line, at)
            .is_some_and(|feeder| feeder.name() == "cat" && feeder.args.iter().any(|arg| !arg.text.starts_with('-')))
}

/// O programa que o interpretador vai rodar, lido das opções.
enum Program {
    /// O programa vem na própria linha, um texto por opção.
    Embedded(Vec<String>),
    /// O programa é o arquivo dado.
    Script(String),
    /// O programa é um módulo instalado (`python -m pytest`).
    Module,
    /// O programa vem pela entrada padrão.
    Stdin,
    /// O interpretador edita os arquivos no lugar.
    InPlace,
}

/// O programa que `args` pedem ao interpretador de gramática `grammar`.
fn program(grammar: &Grammar, args: &[Word]) -> Program {
    let mut codes = Vec::new();
    let mut runner_seen = false;
    let mut words = args.iter().map(|word| word.text.as_str());
    while let Some(word) = words.next() {
        if word == "--" {
            return match words.next() {
                _ if !codes.is_empty() => Program::Embedded(codes),
                Some("-") | None => Program::Stdin,
                Some(script) => Program::Script(script.to_string()),
            };
        }
        if word.starts_with("--") {
            let (name, value) = word.split_once('=').map_or((word, None), |(name, value)| (name, Some(value)));
            if grammar.long_code.contains(&name) {
                codes.extend(value.or_else(|| words.next()).map(str::to_string));
            } else if value.is_none() && grammar.long_value.contains(&name) {
                words.next();
            }
            continue;
        }
        if let Some(cluster) = word.strip_prefix('-').filter(|cluster| !cluster.is_empty()) {
            for (at, letter) in cluster.char_indices() {
                let rest = &cluster[at + letter.len_utf8()..];
                if grammar.in_place.contains(letter) {
                    return Program::InPlace;
                }
                if grammar.module.contains(letter) {
                    return Program::Module;
                }
                if grammar.code.contains(letter) {
                    if rest.is_empty() {
                        codes.extend(words.next().map(str::to_string));
                        break;
                    }
                    if grammar.glued_code {
                        codes.push(rest.to_string());
                        break;
                    }
                    continue;
                }
                if grammar.value.contains(letter) {
                    if rest.is_empty() {
                        words.next();
                    }
                    break;
                }
                if grammar.attached.contains(letter) {
                    break;
                }
            }
            continue;
        }
        // A primeira palavra fora das opções: o arquivo do programa, ou o
        // primeiro argumento do programa que veio na linha.
        if !codes.is_empty() {
            break;
        }
        if word == "-" {
            return Program::Stdin;
        }
        if grammar.runner == Some(word) && !runner_seen {
            runner_seen = true;
            continue;
        }
        return Program::Script(word.to_string());
    }
    if codes.is_empty() { Program::Stdin } else { Program::Embedded(codes) }
}

/// O programa que `segment` roda: o que o interpretador pede nas opções ou,
/// no lançador sem nome de interpretador, o arquivo de programa dado depois
/// do `run` (`uv run /tmp/edit.py`) ou a entrada padrão (`uv run -`). A
/// ferramenta instalada que o lançador roda (`uv run pytest`, `uv run
/// /usr/bin/env pytest`) não é um programa da regra.
fn program_of(segment: &Segment) -> Option<Program> {
    if let Some((grammar, args)) = interpreter(segment) {
        return Some(program(grammar, args));
    }
    launched(segment).into_iter().find_map(|(word, _)| match word.text.as_str() {
        "-" => Some(Program::Stdin),
        path if is_script(path) => Some(Program::Script(path.to_string())),
        _ => None,
    })
}

/// `path` é o arquivo de programa de um dos interpretadores, pela extensão
/// (`edit.py`, `edit.ts`). A ferramenta instalada, como `/usr/bin/env` ou
/// `~/.local/bin/black`, não tem nenhuma.
fn is_script(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| GRAMMARS.iter().any(|grammar| grammar.extensions.contains(&extension)))
}

/// O programa que o redirecionamento `redirect` traz na própria linha: o
/// corpo do heredoc ou o texto do `<<<`.
fn inline_text(redirect: &Redirect) -> Option<&str> {
    match (redirect.op.trim_start_matches(|c: char| c.is_ascii_digit()), &redirect.body) {
        (_, Some(body)) => Some(body),
        ("<<<", None) => Some(&redirect.target.text),
        _ => None,
    }
}

/// O comando é um interpretador (`python`, `py`, `node`, `bun`, `perl`,
/// `ruby`, `php`), com ou sem a pasta e o `.exe`, também o rodado por um
/// lançador (`uv run python`).
pub(crate) fn is_interpreter(segment: &Segment) -> bool {
    interpreter(segment).is_some()
}

/// O comando roda o `python` (`python3`, `python3.12`, `py`), com ou sem a
/// pasta, também pelo lançador (`uv run python`).
pub(crate) fn is_python(segment: &Segment) -> bool {
    interpreter(segment).is_some_and(|(grammar, _)| grammar.name == PYTHON.name)
}

/// Os programas que o interpretador do comando traz escritos na própria
/// linha: o texto de cada `-c`/`-e` ou, quando o programa vem pela entrada
/// padrão, o corpo de cada heredoc e o texto do `<<<`. O script dado pelo
/// nome, o módulo instalado e a edição no lugar não trazem nenhum.
pub(crate) fn embedded_programs(segment: &Segment) -> Vec<String> {
    let Some((grammar, args)) = interpreter(segment) else { return Vec::new() };
    match program(grammar, args) {
        Program::Embedded(codes) => codes,
        Program::Stdin => segment.redirects.iter().filter_map(inline_text).map(str::to_string).collect(),
        Program::Script(_) | Program::Module | Program::InPlace => Vec::new(),
    }
}

/// `sed` com `-i` ou `--in-place`, também no meio de um grupo (`-ni`).
fn sed_in_place(args: &[Word]) -> bool {
    let mut words = args.iter().map(|word| word.text.as_str());
    while let Some(word) = words.next() {
        if word == "--" {
            return false;
        }
        if word == "--in-place" || word.starts_with("--in-place=") {
            return true;
        }
        let Some(cluster) = word.strip_prefix('-').filter(|cluster| !cluster.is_empty() && !cluster.starts_with('-')) else {
            continue;
        };
        for (at, letter) in cluster.char_indices() {
            match letter {
                'i' => return true,
                'e' | 'f' | 'l' => {
                    if cluster[at + letter.len_utf8()..].is_empty() {
                        words.next();
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    false
}

/// O redirecionamento grava no alvo (`>`, `>>`, `>|`, `2>`, `&>`, `&>>`),
/// e não só junta saídas (`2>&1`).
fn writes(redirect: &Redirect) -> bool {
    matches!(redirect.op.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&'), ">" | ">>" | ">|")
}

/// Os programas que passam texto adiante e o gravam num arquivo pelo
/// redirecionamento ou, no `tee`, pelo arquivo dado: os que só o copiam
/// (`cat`, `echo`, `printf`, `tee`) e os que o trocam no caminho (`sed`,
/// `awk`).
const WRITERS: &[&str] = &["cat", "echo", "printf", "tee", "sed", "awk"];

/// Os programas que levam um arquivo de um lugar a outro: a troca pela pasta
/// temporária termina num deles.
const TRANSFERS: &[&str] = &["mv", "cp"];

/// As origens e o destino de `mv` ou `cp`: o destino é a pasta do `-t` ou,
/// sem ele, a última palavra fora das opções. `None` quando faltam as origens
/// ou o destino.
fn transfer(args: &[Word]) -> Option<(Vec<&str>, &str)> {
    let mut operands = Vec::new();
    let mut target = None;
    let mut words = args.iter().map(|word| word.text.as_str());
    while let Some(word) = words.next() {
        if word == "--" {
            operands.extend(words);
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            match long.split_once('=') {
                Some(("target-directory", value)) => target = Some(value),
                None if long == "target-directory" => target = words.next(),
                None if long == "suffix" => {
                    words.next();
                }
                _ => {}
            }
        } else if let Some(cluster) = word.strip_prefix('-').filter(|cluster| !cluster.is_empty()) {
            if let Some((at, letter)) = cluster.char_indices().find(|(_, letter)| matches!(letter, 't' | 'S')) {
                let rest = &cluster[at + letter.len_utf8()..];
                let value = if rest.is_empty() { words.next() } else { Some(rest) };
                if letter == 't' {
                    target = value;
                }
            }
        } else {
            operands.push(word);
        }
    }
    match target {
        Some(target) => Some((operands, target)),
        None => {
            let destination = operands.pop()?;
            (!operands.is_empty()).then_some((operands, destination))
        }
    }
}

/// O comando pode mexer em arquivo por uma das formas da regra.
fn suspect(segment: &Segment) -> bool {
    let name = segment.name();
    WRITERS.contains(&name) || TRANSFERS.contains(&name) || grammar(name).is_some() || !launched(segment).is_empty()
}

/// Os sinais que, além de letra e número, cabem num caminho citado no texto
/// de um programa. No Windows entram a barra invertida e os dois-pontos da
/// unidade: sem eles, `C:\proj\src\lib.rs` vira pedaços que nunca batem com um
/// arquivo do projeto.
const PATH_CHARS: &str = if cfg!(windows) { "_-./~+@\\:" } else { "_-./~+@" };

/// As pastas do projeto: o checkout principal e a pasta das cópias.
struct Project {
    folders: Vec<PathBuf>,
    /// Na leitura de uma conversa que já passou, o checkout principal e a
    /// pasta das cópias: o arquivo de uma cópia que já saiu do disco existe
    /// quando o checkout tem o mesmo caminho, contado a partir da cópia.
    gone_copies: Option<(PathBuf, PathBuf)>,
}

impl Project {
    fn new(root: &Path) -> Self {
        let real = |path: PathBuf| path.canonicalize().unwrap_or(path);
        let folders = [root.to_path_buf(), wave_prompt::copies_dir(root)]
            .into_iter()
            .filter(|folder| !folder.as_os_str().is_empty())
            .map(real)
            .collect();
        Self { folders, gone_copies: None }
    }

    /// O mesmo projeto, lido numa conversa que já passou, quando a cópia da
    /// onda pode já ter saído do disco.
    fn past(self, root: &Path) -> Self {
        Self { gone_copies: Some((root.to_path_buf(), wave_prompt::copies_dir(root))), ..self }
    }

    /// `path` existe no disco ou, na conversa que já passou, no checkout
    /// principal, quando fica numa cópia (`<cópias>/<obra>/<vaga>/…`).
    fn exists(&self, path: &Path) -> bool {
        path.exists()
            || self.gone_copies.as_ref().is_some_and(|(root, copies)| {
                let inside: PathBuf = path.strip_prefix(copies).map(|rest| rest.components().skip(2).collect()).unwrap_or_default();
                !inside.as_os_str().is_empty() && root.join(inside).exists()
            })
    }

    /// `path` fica no projeto ou numa cópia, pelo caminho real da pasta mais
    /// funda que já existe.
    fn holds(&self, path: &Path) -> bool {
        let real = path.ancestors().find(|folder| folder.exists()).and_then(|folder| folder.canonicalize().ok());
        real.is_some_and(|real| self.folders.iter().any(|folder| real.starts_with(folder)))
    }

    /// `text` nomeia, a partir da pasta atual de `shell`, um caminho fora do
    /// projeto.
    fn outside(&self, text: &str, shell: &Shell) -> bool {
        shell.path(text).is_some_and(|path| !self.holds(&path))
    }

    /// O texto de um programa cita um caminho que existe no projeto: a palavra
    /// com `/` ou `.`, lida a partir da pasta atual de `shell` ou, quando
    /// começa em `/`, também como o resto de um caminho montado
    /// (`f"{root}/src/x.rs"`).
    fn cited(&self, text: &str, shell: &Shell) -> bool {
        text.split(|c: char| !(c.is_alphanumeric() || PATH_CHARS.contains(c)))
            .map(|token| token.trim_end_matches('.'))
            .filter(|token| token.contains(['/', '.', '\\']) && token.chars().any(char::is_alphanumeric))
            .any(|token| {
                let joined = shell.here().join(token.trim_start_matches('/'));
                shell.path(token).into_iter().chain([joined]).any(|path| self.exists(&path) && self.holds(&path))
            })
    }

    /// O programa `program`, rodado por `line[at]`, mexe em arquivo do projeto
    /// por uma das formas dele: edição no lugar, programa na linha que cita
    /// caminho do projeto ou programa guardado fora do projeto. O programa da
    /// entrada padrão vem do redirecionamento ou do comando que o passa pelo
    /// cano. O programa na linha ou pela entrada cuja saída chega a um
    /// arquivo do projeto também mexe nele, cite ou não um caminho. O script
    /// do projeto, pelo nome ou pela entrada (`python3 < scripts/x.py`), com a
    /// saída redirecionada passa, como o gerador.
    fn interprets(&self, program: Program, line: &[Segment], at: usize, shell: &Shell) -> bool {
        let segment = &line[at];
        match program {
            Program::InPlace => true,
            Program::Module => false,
            Program::Embedded(codes) => codes.iter().any(|code| self.cited(code, shell)) || self.lands(line, at, shell),
            Program::Script(script) => self.outside(&script, shell),
            Program::Stdin => {
                self.fed(&segment.redirects, shell)
                    || feeder(line, at).is_some_and(|feeder| self.piped(feeder, shell))
                    || (!from_file(line, at) && self.lands(line, at, shell))
            }
        }
    }

    /// A entrada `redirects` traz um programa que mexe em arquivo do projeto:
    /// o heredoc ou o `<<<` que cita caminho dele, ou o arquivo do `<`
    /// guardado fora dele.
    fn fed(&self, redirects: &[Redirect], shell: &Shell) -> bool {
        redirects.iter().any(|redirect| match inline_text(redirect) {
            Some(text) => self.cited(text, shell),
            None => {
                redirect.op.trim_start_matches(|c: char| c.is_ascii_digit()) == "<" && self.outside(&redirect.target.text, shell)
            }
        })
    }

    /// O comando `feeder`, que passa pelo cano o programa ao interpretador,
    /// traz um programa que mexe em arquivo do projeto: o texto de `echo` ou
    /// `printf` que cita caminho dele, ou o que o `cat` passa adiante (a
    /// entrada dele, ou um arquivo guardado fora do projeto).
    fn piped(&self, feeder: &Segment, shell: &Shell) -> bool {
        match feeder.name() {
            "echo" | "printf" => feeder.args.iter().any(|arg| self.cited(&arg.text, shell)),
            "cat" => {
                self.fed(&feeder.redirects, shell)
                    || feeder.args.iter().filter(|arg| !arg.text.starts_with('-')).any(|file| self.outside(&file.text, shell))
            }
            _ => false,
        }
    }

    /// `target`, o alvo de uma gravação (o arquivo do `tee` ou o de um
    /// redirecionamento), é um arquivo do projeto. O `&` do começo é o
    /// descritor de `2>&1`, não um arquivo.
    fn writes_to(&self, target: &Word, shell: &Shell) -> bool {
        !target.text.starts_with('&') && shell.path(&target.text).is_some_and(|path| self.holds(&path))
    }

    /// A saída de `line[at]` é gravada num arquivo do projeto por um
    /// redirecionamento: o dele ou o do grupo que termina nele (`( … ) > f`,
    /// `{ …; } > f`), que vem no comando sem programa logo depois do grupo.
    fn redirected(&self, line: &[Segment], at: usize, shell: &Shell) -> bool {
        let depth = line[at].depth;
        let group = line[at + 1..]
            .iter()
            .find(|segment| segment.depth <= depth)
            .filter(|segment| segment.depth == depth && segment.program.text.is_empty());
        std::iter::once(&line[at])
            .chain(group)
            .flat_map(|segment| &segment.redirects)
            .filter(|redirect| writes(redirect))
            .any(|redirect| self.writes_to(&redirect.target, shell))
    }

    /// A saída de `line[at]` chega a um arquivo do projeto: pelo
    /// redirecionamento dele ou pelo de um comando que a recebe pelo cano
    /// (`| head > src/lib.rs`), também o do grupo que termina num deles.
    fn lands(&self, line: &[Segment], at: usize, shell: &Shell) -> bool {
        std::iter::once(at).chain(Segment::fed(line, at)).any(|link| self.redirected(line, link, shell))
    }

    /// Um programa de [`WRITERS`] grava num arquivo do projeto, pelo
    /// redirecionamento ou pelo arquivo do `tee`.
    fn written(&self, line: &[Segment], at: usize, shell: &Shell) -> bool {
        let segment = &line[at];
        let teed = segment.args.iter().filter(|_| segment.name() == "tee").filter(|arg| !arg.text.starts_with('-'));
        teed.into_iter().any(|target| self.writes_to(target, shell)) || self.redirected(line, at, shell)
    }

    /// `mv` ou `cp` traz para o projeto um arquivo guardado fora dele: o
    /// destino fica no projeto e alguma origem fora dele ou num lugar que só
    /// o terminal sabe (`"$tmp"`, `$(mktemp)`), como a troca pela pasta
    /// temporária. Mover ou copiar entre arquivos do projeto, ou do projeto
    /// para fora, não troca o conteúdo por um que veio de fora e passa.
    fn transferred(&self, segment: &Segment, shell: &Shell) -> bool {
        transfer(&segment.args).is_some_and(|(sources, destination)| {
            shell.path(destination).is_some_and(|path| self.holds(&path))
                && sources.iter().any(|source| shell.path(source).is_none_or(|path| !self.holds(&path)))
        })
    }

    /// O comando `line[at]`, rodado na pasta atual de `shell`, mexe em
    /// arquivo do projeto.
    fn edits(&self, line: &[Segment], at: usize, shell: &Shell) -> bool {
        let segment = &line[at];
        match segment.name() {
            "sed" if sed_in_place(&segment.args) => true,
            name if WRITERS.contains(&name) => self.written(line, at, shell),
            name if TRANSFERS.contains(&name) => self.transferred(segment, shell),
            _ => program_of(segment).is_some_and(|program| self.interprets(program, line, at, shell)),
        }
    }

    /// Algum comando de `segments`, rodados a partir de `shell` e seguindo os
    /// `cd` da linha, mexe em arquivo do projeto.
    fn edited(&self, segments: &[Segment], mut shell: Shell) -> bool {
        for (at, segment) in segments.iter().enumerate() {
            if segment.name() == "cd" {
                shell.cd(segment);
                continue;
            }
            if self.edits(segments, at, &shell) {
                return true;
            }
        }
        false
    }
}

/// A recusa do comando do agente de onda que mexe em arquivo do projeto
/// pelo terminal, com a ferramenta de edição do hospedeiro que o chamou.
pub(crate) fn bash_shell_edit(segments: &[Segment], input: &HookInput, ctx: &Ctx, lang: Locale) -> Option<Verdict> {
    if !segments.iter().any(suspect) {
        return None;
    }
    let root = Path::new(&ctx.project_dir);
    let here = crate::hooks::write::wave_copy::wave_shell_folder(root, input, ctx)?;
    // A pasta anterior, destino de `cd -`, é a de onde o comando parte, também
    // quando a troca do fim o leva à cópia.
    let cwd = Path::new(input.cwd.as_deref().unwrap_or(&ctx.project_dir)).to_path_buf();
    let shell = Shell::new(here, cwd, mustard_core::platform::harness::home_dir());
    if !Project::new(root).edited(segments, shell) {
        return None;
    }
    let edit = if input.raw["mustard_host"] == "codex" { "`apply_patch`" } else { "`Edit`/`Write`" };
    Some(Verdict::Deny { reason: crate::shared::say::say("command_guard.shell_edit", lang, &[("{edit}", edit)]) })
}

/// A linha `segments`, que rodou em `here` numa conversa do projeto do
/// checkout principal `root`, mexeu em arquivo do projeto por uma das formas
/// que o agente de onda tem recusadas, em qualquer conversa. A cópia de onda
/// que já saiu do disco vale pelo checkout principal.
pub(crate) fn edited_by_shell(segments: &[Segment], root: &Path, here: &Path) -> bool {
    let shell = || Shell::new(here.to_path_buf(), here.to_path_buf(), mustard_core::platform::harness::home_dir());
    segments.iter().any(suspect) && Project::new(root).past(root).edited(segments, shell())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::bash::command_guard::CommandGuard;
    use mustard_core::domain::model::contract::{Check, Trigger};
    use serde_json::json;

    /// O projeto com `src/lib.rs`, a cópia de uma onda dele com o mesmo
    /// arquivo, e uma pasta fora dos dois, no papel da pasta temporária. A
    /// cópia mora na pasta das cópias do projeto, que sai junto com o teste.
    struct Scene {
        _dir: tempfile::TempDir,
        root: PathBuf,
        copy: PathBuf,
        outside: PathBuf,
    }

    impl Drop for Scene {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(wave_prompt::copies_dir(&self.root));
        }
    }

    fn scene() -> Scene {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let outside = dir.path().join("scratch");
        // O endereço da cópia sai do caminho real do projeto, que só existe
        // depois que a pasta nasce: com um atalho na pasta temporária, como no
        // macOS, o endereço montado antes não bate com o que a trava calcula.
        std::fs::create_dir_all(&root).unwrap();
        let copy = wave_prompt::slot_path(&root, "work", 0);
        for folder in [&root, &copy] {
            std::fs::create_dir_all(folder.join("scripts")).unwrap();
            std::fs::create_dir_all(folder.join("src")).unwrap();
            std::fs::write(folder.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
            std::fs::write(folder.join("scripts/check.py"), "print('ok')\n").unwrap();
        }
        std::fs::create_dir_all(&outside).unwrap();
        Scene { _dir: dir, root, copy, outside }
    }

    /// `path` como ele vai num comando de terminal: com a barra comum, porque
    /// a barra invertida do Windows fora de aspas é escape e some na leitura.
    fn in_shell(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    /// O veredito da trava de comandos inteira para `command`, pedido pelo
    /// agente de onda de dentro da cópia (`wave`) ou pela conversa principal
    /// no checkout, no hospedeiro `host`.
    fn guard(scene: &Scene, command: &str, wave: bool, host: &str) -> Verdict {
        let cwd = if wave { &scene.copy } else { &scene.root };
        let mut input: HookInput = serde_json::from_value(json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "hook_event_name": "PreToolUse",
            "session_id": "session",
            "cwd": cwd,
        }))
        .unwrap();
        if wave {
            input.agent_id = Some("worker".into());
            input.agent_type = Some("mustard-wave".into());
        }
        input.raw = json!({"mustard_host": host});
        let ctx = Ctx::for_test(scene.root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        CommandGuard.evaluate(&input, &ctx).unwrap()
    }

    /// As formas de mexer em arquivo do projeto pelo terminal são recusadas
    /// no agente de onda: o programa na linha que cita caminho do projeto ou
    /// da cópia, o programa na linha ou pela entrada cuja saída chega ao
    /// projeto (pelo redirecionamento dele, pelo cano ou pelo grupo), o
    /// programa guardado fora do projeto, a edição no lugar, o texto gravado
    /// por `cat`, `echo`, `printf`, `tee`, `sed` ou `awk`, também pelo grupo,
    /// e o arquivo de fora ou de lugar desconhecido trazido por `mv` ou `cp`.
    /// Valem também o programa passado pelo cano, o interpretador rodado por
    /// um lançador (que pula o valor das opções dele e os invólucros, confere
    /// a palavra seguinte ao valor de uma opção que ele não conhece e roda
    /// também o script de fora sem nome de interpretador) e a gravação depois
    /// do `cd -` que volta à cópia. A recusa manda ler o trecho e editar pela
    /// ferramenta do hospedeiro que pediu. A conversa principal segue sem
    /// essa recusa.
    #[test]
    fn a_wave_agent_writing_files_from_the_shell_is_refused() {
        let scene = scene();
        let (root, copy, outside) = (in_shell(&scene.root), in_shell(&scene.copy), in_shell(&scene.outside));
        let refused = [
            "python3 - <<'EOF'\nfrom pathlib import Path\np = Path('src/lib.rs')\np.write_text(p.read_text().replace('a', 'b'))\nEOF".to_string(),
            format!("python3 -c \"open('{copy}/src/lib.rs', 'w').write('')\""),
            format!("node -e \"require('fs').writeFileSync('{root}/src/lib.rs', '')\""),
            "ruby -e 'File.write(\"src/lib.rs\", \"\")'".to_string(),
            "python3 <<< \"open('src/lib.rs', 'a').write('x')\"".to_string(),
            format!("python3 {outside}/edit.py src/lib.rs"),
            format!("python3 < {outside}/edit.py"),
            format!("bun run {outside}/edit.ts"),
            "sed -i 's/a/b/' src/lib.rs".to_string(),
            "sed -E -i.bak -e 's/a/b/' src/lib.rs".to_string(),
            "find src -name '*.rs' | xargs sed --in-place 's/a/b/'".to_string(),
            "perl -pi -e 's/a/b/' src/lib.rs".to_string(),
            "cat > src/new.rs <<'EOF'\npub fn b() {}\nEOF".to_string(),
            "echo 'pub mod new;' >> src/lib.rs".to_string(),
            format!("printf 'x\\n' > {root}/src/lib.rs"),
            "printf 'x\\n' | tee -a src/lib.rs".to_string(),
            format!("cd {outside} && echo x > {copy}/src/lib.rs"),
            format!("cd {outside} && cd - && echo x > src/lib.rs"),
            "echo \"open('src/lib.rs', 'w').write('')\" | python3".to_string(),
            "cat <<'EOF' | python3 -\nopen('src/lib.rs', 'w').write('')\nEOF".to_string(),
            format!("cat {outside}/edit.py | python3"),
            "uv run --with requests python -c \"open('src/lib.rs', 'w').write('')\"".to_string(),
            "poetry run python - <<'EOF'\nopen('src/lib.rs', 'w').write('')\nEOF".to_string(),
            "py -3 -c \"open('src/lib.rs', 'w').write('')\"".to_string(),
            "php -r 'file_put_contents(\"src/lib.rs\", \"\");'".to_string(),
            "python3 -c \"print(1)\" > src/lib.rs".to_string(),
            "python3 - > src/lib.rs <<'EOF'\nprint(1)\nEOF".to_string(),
            format!("node -e \"console.log(1)\" >> {root}/src/lib.rs"),
            format!("echo 'print(1)' | python3 > {copy}/src/lib.rs"),
            format!("sed 's/a/b/' src/lib.rs > {outside}/x.rs && mv {outside}/x.rs src/lib.rs"),
            format!("cp {outside}/x.rs {copy}/src/lib.rs"),
            format!("mv -f -t src {outside}/x.rs"),
            "conda run -n py python -c \"open('src/lib.rs', 'w').write('')\"".to_string(),
            "conda run --name py python - <<'EOF'\nopen('src/lib.rs', 'w').write('')\nEOF".to_string(),
            format!("uv run {outside}/edit.py"),
            format!("poetry run -- {outside}/edit.py"),
            "pdm run -p . python -c \"open('src/lib.rs', 'w').write('')\"".to_string(),
            "uv run --color never python -c \"open('src/lib.rs', 'w').write('')\"".to_string(),
            "uv run --exclude-newer 2024-01-01 python -c \"open('src/lib.rs', 'w').write('')\"".to_string(),
            "uv run --only-group dev python -c \"open('src/lib.rs', 'w').write('')\"".to_string(),
            "uv run /usr/bin/env python -c \"open('src/lib.rs', 'w').write('')\"".to_string(),
            format!("uv run --color never {outside}/edit.py"),
            "python3 -c \"print(1)\" | head -n 100 > src/lib.rs".to_string(),
            "python3 -c \"print(1)\" | sed 's/x/x/' > src/lib.rs".to_string(),
            "(python3 -c \"print(1)\") > src/lib.rs".to_string(),
            "{ echo 'pub fn b() {}'; } > src/lib.rs".to_string(),
            "tmp=$(mktemp) && sed 's/a/b/' src/lib.rs > \"$tmp\" && mv \"$tmp\" src/lib.rs".to_string(),
            "sed 's/a/b/' src/lib.rs > src/lib.rs.new && mv src/lib.rs.new src/lib.rs".to_string(),
            "awk '{print}' src/lib.rs > src/lib.rs.new".to_string(),
        ];
        for command in &refused {
            let Verdict::Deny { reason } = guard(&scene, command, true, "claude") else {
                panic!("not refused: {command}");
            };
            assert!(reason.contains("`offset`") && reason.contains("`Edit`/`Write`"), "{command}: {reason}");
            let main = guard(&scene, command, false, "claude");
            assert!(!matches!(&main, Verdict::Deny { reason: other } if *other == reason), "{command}: {main:?}");
        }
        let Verdict::Deny { reason } = guard(&scene, &refused[0], true, "codex") else {
            panic!("not refused under the other host");
        };
        assert!(reason.contains("`apply_patch`") && !reason.contains("`Edit`"), "{reason}");
    }

    /// No agente de onda passam o programa na linha sem caminho do projeto, o
    /// script do projeto, pelo nome ou pela entrada, o módulo instalado, a
    /// ferramenta instalada rodada pelo lançador, também pelo caminho inteiro,
    /// o gerador com a saída redirecionada, a leitura e a gravação fora do
    /// projeto, também depois de um `cd` para fora dele. O programa passado
    /// pelo cano e o rodado por um lançador passam pelas mesmas regras.
    #[test]
    fn a_wave_agent_one_line_check_and_reads_pass() {
        let scene = scene();
        let outside = in_shell(&scene.outside);
        let free = [
            "python3 -c \"print(sum(range(10)))\"".to_string(),
            "python3 - <<'EOF'\nimport json\nprint(json.dumps({'a': 1}))\nEOF".to_string(),
            format!("cat {outside}/out.json | python3 -c \"import json, sys; print(len(json.load(sys.stdin)))\""),
            "node -e \"console.log(process.version)\"".to_string(),
            "python3 scripts/check.py".to_string(),
            "python3 -m pytest -q".to_string(),
            "cargo run --bin gen > src/gen.rs".to_string(),
            "cat src/lib.rs".to_string(),
            "sed -n '1,20p' src/lib.rs".to_string(),
            "echo checked 2>/dev/null".to_string(),
            format!("echo done > {outside}/note.txt"),
            format!("printf 'x\\n' | tee {outside}/log.txt"),
            format!("cat src/lib.rs > {outside}/lib.rs"),
            format!("cd {outside} && echo x > note.txt"),
            "echo 'print(sum(range(10)))' | python3".to_string(),
            "cat scripts/check.py | python3".to_string(),
            "uv run python -m pytest -q".to_string(),
            "poetry run python scripts/check.py".to_string(),
            format!("python3 -c \"print(1)\" > {outside}/out.txt"),
            "python3 -c \"print(1)\" 2>&1".to_string(),
            "python3 -c \"print(1)\" > /dev/null".to_string(),
            "python3 scripts/check.py > src/gen.rs".to_string(),
            format!("sed 's/a/b/' src/lib.rs > {outside}/x.rs"),
            "mv src/lib.rs src/old.rs".to_string(),
            "cp src/lib.rs src/copy.rs".to_string(),
            format!("cp src/lib.rs {outside}/lib.rs"),
            format!("mv {outside}/a.txt {outside}/b.txt"),
            "uv run scripts/check.py".to_string(),
            "conda run -n py python -m pytest -q".to_string(),
            "conda run -n py python -c \"print(1)\"".to_string(),
            "uv run /usr/bin/env pytest -q".to_string(),
            "poetry run ~/.local/bin/black src/".to_string(),
            "python3 < scripts/check.py > src/gen.rs".to_string(),
            "cat scripts/check.py | python3 > src/gen.rs".to_string(),
            "uv run pytest -k python".to_string(),
            format!("sed -n 1,20p src/lib.rs > {outside}/x"),
        ];
        for command in &free {
            let verdict = guard(&scene, command, true, "claude");
            assert!(!matches!(verdict, Verdict::Deny { .. }), "{command}: {verdict:?}");
        }
    }

    /// O lançador roda a primeira palavra depois do `run`, pulado o valor das
    /// opções dele e os invólucros: `conda run -n py python` roda o `python`,
    /// e não o `py` que é o valor do `-n`; `uv run pytest -k python` não roda
    /// interpretador. Depois de uma opção que ele não conhece, a palavra que
    /// não é interpretador pode ser o valor dela, e a seguinte também conta:
    /// `uv run --color never python` roda o `python`.
    #[test]
    fn a_launcher_runs_the_first_word_after_its_options() {
        let runs = |command: &str| {
            let line = super::super::lex::segments(command);
            (is_interpreter(&line[0]), is_python(&line[0]))
        };
        assert_eq!(runs("conda run -n py python -c 'print(1)'"), (true, true));
        assert_eq!(runs("conda run -n py node -e '1'"), (true, false));
        assert_eq!(runs("uv run --with requests --python 3.12 python3 x.py"), (true, true));
        assert_eq!(runs("uv run -- python x.py"), (true, true));
        assert_eq!(runs("uv run pytest -k python"), (false, false));
        assert_eq!(runs("conda run -n py pytest"), (false, false));
        assert_eq!(runs("uv sync python"), (false, false));
        assert_eq!(runs("uv run --color never python3 x.py"), (true, true));
        assert_eq!(runs("pdm run -p . node x.js"), (true, false));
        assert_eq!(runs("uv run /usr/bin/env python x.py"), (true, true));
        assert_eq!(runs("uv run --frozen pytest -k python"), (false, false));
        assert_eq!(runs("uv run /usr/bin/env pytest -q"), (false, false));
    }
}
