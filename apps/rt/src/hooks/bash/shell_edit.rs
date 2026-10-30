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
//! - a edição no lugar (`sed -i`, `perl -i`, `ruby -i`);
//! - `cat`, `echo`, `printf` e `tee` gravando no projeto ou numa cópia (`>`,
//!   `>>` ou o arquivo do `tee`).
//!
//! Passam o programa na linha sem caminho do projeto, o script do projeto
//! (`python scripts/x.py`, `python -m pytest`), o gerador com a saída
//! redirecionada (`cargo run --bin gen > src/gen.rs`), a leitura e a gravação
//! fora do projeto. A conversa principal não passa pela recusa; a medição do
//! uso real conta, pela mesma regra ([`edited_by_shell`]), as edições por
//! script de toda conversa do projeto, e reconhece o interpretador, o
//! `python` e o programa escrito na linha pela mesma gramática
//! ([`is_interpreter`], [`is_python`], [`embedded_programs`]), sem lista
//! própria de nomes.

use super::lex::{Redirect, Segment, Word};
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
};

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
/// programa (`uv run --with x python -c …`).
const LAUNCHERS: &[&str] = &["uv", "poetry", "pipenv", "pdm", "hatch", "rye", "conda"];

/// O interpretador que `segment` roda, com os argumentos dele: o próprio
/// programa ou, num lançador, o primeiro interpretador depois do `run`.
fn interpreter(segment: &Segment) -> Option<(&'static Grammar, &[Word])> {
    if let Some(grammar) = grammar(segment.name()) {
        return Some((grammar, &segment.args));
    }
    let (run, rest) = segment.args.split_first().filter(|_| LAUNCHERS.contains(&segment.name()))?;
    if run.text != "run" {
        return None;
    }
    rest.iter().enumerate().find_map(|(at, word)| Some((grammar(&word.text)?, &rest[at + 1..])))
}

/// O comando que passa a saída pelo cano a `line[at]`: o anterior no mesmo
/// nível, quando encadeado a ele. Os comandos de dentro dele (`$(…)`) vêm
/// logo depois dele, mais fundos, e ficam de fora.
fn feeder(line: &[Segment], at: usize) -> Option<&Segment> {
    let depth = line[at].depth;
    line[..at].iter().rev().find(|segment| segment.depth <= depth).filter(|segment| segment.depth == depth && segment.piped)
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
/// redirecionamento ou, no `tee`, pelo arquivo dado.
const WRITERS: &[&str] = &["cat", "echo", "printf", "tee"];

/// O comando pode mexer em arquivo por uma das quatro formas da regra.
fn suspect(segment: &Segment) -> bool {
    let name = segment.name();
    name == "sed" || WRITERS.contains(&name) || interpreter(segment).is_some()
}

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
        text.split(|c: char| !(c.is_alphanumeric() || "_-./~+@".contains(c)))
            .map(|token| token.trim_end_matches('.'))
            .filter(|token| token.contains(['/', '.']) && token.chars().any(char::is_alphanumeric))
            .any(|token| {
                let joined = shell.here().join(token.trim_start_matches('/'));
                shell.path(token).into_iter().chain([joined]).any(|path| self.exists(&path) && self.holds(&path))
            })
    }

    /// O interpretador de gramática `grammar`, com os argumentos `args`,
    /// rodado por `line[at]`, mexe em arquivo do projeto por uma das três
    /// formas dele: edição no lugar, programa na linha que cita caminho do
    /// projeto ou programa guardado fora do projeto. O programa da entrada
    /// padrão vem do redirecionamento ou do comando que o passa pelo cano.
    fn interprets(&self, grammar: &Grammar, args: &[Word], line: &[Segment], at: usize, shell: &Shell) -> bool {
        match program(grammar, args) {
            Program::InPlace => true,
            Program::Module => false,
            Program::Embedded(codes) => codes.iter().any(|code| self.cited(code, shell)),
            Program::Script(script) => self.outside(&script, shell),
            Program::Stdin => {
                self.fed(&line[at].redirects, shell) || feeder(line, at).is_some_and(|feeder| self.piped(feeder, shell))
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

    /// `cat`, `echo`, `printf` ou `tee` grava num arquivo do projeto, pelo
    /// redirecionamento ou pelo arquivo do `tee`.
    fn written(&self, segment: &Segment, shell: &Shell) -> bool {
        let teed = segment.args.iter().filter(|_| segment.name() == "tee").filter(|arg| !arg.text.starts_with('-'));
        let redirected = segment.redirects.iter().filter(|redirect| writes(redirect)).map(|redirect| &redirect.target);
        teed.chain(redirected)
            .filter(|target| !target.text.starts_with('&'))
            .any(|target| shell.path(&target.text).is_some_and(|path| self.holds(&path)))
    }

    /// O comando `line[at]`, rodado na pasta atual de `shell`, mexe em
    /// arquivo do projeto.
    fn edits(&self, line: &[Segment], at: usize, shell: &Shell) -> bool {
        let segment = &line[at];
        match segment.name() {
            "sed" => sed_in_place(&segment.args),
            name if WRITERS.contains(&name) => self.written(segment, shell),
            _ => interpreter(segment).is_some_and(|(grammar, args)| self.interprets(grammar, args, line, at, shell)),
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

    /// As quatro formas de mexer em arquivo do projeto pelo terminal são
    /// recusadas no agente de onda: o programa na linha que cita caminho do
    /// projeto ou da cópia, o programa guardado fora do projeto, a edição no
    /// lugar e o texto gravado por `cat`, `echo`, `printf` ou `tee`. Valem
    /// também o programa passado pelo cano, o interpretador rodado por um
    /// lançador e a gravação depois do `cd -` que volta à cópia. A recusa
    /// manda ler o trecho e editar pela ferramenta do hospedeiro que pediu. A
    /// conversa principal segue sem essa recusa.
    #[test]
    fn a_wave_agent_writing_files_from_the_shell_is_refused() {
        let scene = scene();
        let (root, copy, outside) = (scene.root.display(), scene.copy.display(), scene.outside.display());
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
    /// script do projeto, o módulo instalado, o gerador com a saída
    /// redirecionada, a leitura e a gravação fora do projeto, também depois de
    /// um `cd` para fora dele. O programa passado pelo cano e o rodado por um
    /// lançador passam pelas mesmas regras.
    #[test]
    fn a_wave_agent_one_line_check_and_reads_pass() {
        let scene = scene();
        let outside = scene.outside.display();
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
        ];
        for command in &free {
            let verdict = guard(&scene, command, true, "claude");
            assert!(!matches!(verdict, Verdict::Deny { .. }), "{command}: {verdict:?}");
        }
    }
}
