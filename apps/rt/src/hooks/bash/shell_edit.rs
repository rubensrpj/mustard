//! `shell_edit` — num agente de onda, o arquivo do projeto muda só pela
//! ferramenta de edição do hospedeiro, nunca pelo terminal.
//!
//! A regra é de sintaxe e não adivinha o que o programa faz. No agente de
//! onda, ela recusa:
//!
//! - o interpretador (`python`, `node`, `perl`, `ruby`, `bun`) com o programa
//!   na própria linha (`-c`, `-e`, heredoc ou `<<<`), quando o texto dele cita
//!   um caminho que existe no projeto ou na pasta das cópias;
//! - o interpretador que roda um programa guardado fora do projeto, como o da
//!   pasta temporária;
//! - a edição no lugar (`sed -i`, `perl -i`, `ruby -i`);
//! - `cat`, `echo`, `printf` e `tee` gravando no projeto ou numa cópia (`>`,
//!   `>>` ou o arquivo do `tee`).
//!
//! Passam o programa na linha sem caminho do projeto, o script do projeto
//! (`python scripts/x.py`, `python -m pytest`), o gerador com a saída
//! redirecionada (`cargo run --bin gen > src/gen.rs`), a leitura e a gravação
//! fora do projeto. A conversa principal não passa por aqui.

use super::lex::{Redirect, Segment, Word};
use mustard_core::domain::model::contract::{Ctx, HookInput, Verdict};
use mustard_core::io::wave_prompt;
use mustard_core::platform::i18n::Locale;
use std::path::{Path, PathBuf};

/// Como um interpretador lê as opções da linha de comando.
struct Grammar {
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

const BUN: Grammar = Grammar { runner: Some("run"), ..NODE };

const PERL: Grammar = Grammar {
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

/// A gramática do interpretador `name`, ou `None` para outro programa.
fn grammar(name: &str) -> Option<&'static Grammar> {
    let name = name.strip_suffix(".exe").unwrap_or(name);
    match name {
        "python" | "python3" => Some(&PYTHON),
        _ if name.starts_with("python3.") => Some(&PYTHON),
        "node" | "nodejs" => Some(&NODE),
        "bun" => Some(&BUN),
        "perl" => Some(&PERL),
        "ruby" => Some(&RUBY),
        _ => None,
    }
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
    name == "sed" || WRITERS.contains(&name) || grammar(name).is_some()
}

/// As pastas do projeto: o checkout principal e a pasta das cópias.
struct Project {
    folders: Vec<PathBuf>,
    home: Option<PathBuf>,
}

impl Project {
    fn new(root: &Path, home: Option<PathBuf>) -> Self {
        let real = |path: PathBuf| path.canonicalize().unwrap_or(path);
        let folders = [root.to_path_buf(), wave_prompt::copies_dir(root)]
            .into_iter()
            .filter(|folder| !folder.as_os_str().is_empty())
            .map(real)
            .collect();
        Self { folders, home }
    }

    /// `path` fica no projeto ou numa cópia, pelo caminho real da pasta mais
    /// funda que já existe.
    fn holds(&self, path: &Path) -> bool {
        let real = path.ancestors().find(|folder| folder.exists()).and_then(|folder| folder.canonicalize().ok());
        real.is_some_and(|real| self.folders.iter().any(|folder| real.starts_with(folder)))
    }

    /// O caminho que `text` nomeia a partir de `here`, ou `None` quando ele
    /// depende do que só o terminal sabe (`$(…)`, variável sem valor aqui).
    fn resolved(&self, here: &Path, text: &str) -> Option<PathBuf> {
        if text.is_empty() {
            return None;
        }
        let path = if text == "~" {
            self.home.clone()?
        } else if let Some(rest) = text.strip_prefix("~/") {
            self.home.as_ref()?.join(rest)
        } else if let Some(variable) = text.strip_prefix('$') {
            let braced = variable.strip_prefix('{').and_then(|inner| inner.split_once('}'));
            let (name, rest) = braced.unwrap_or_else(|| {
                let end = variable.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(variable.len());
                variable.split_at(end)
            });
            let value = match name {
                "" => return None,
                "PWD" => here.to_path_buf(),
                "HOME" => self.home.clone()?,
                _ => PathBuf::from(std::env::var_os(name)?),
            };
            PathBuf::from(format!("{}{rest}", value.to_string_lossy()))
        } else {
            PathBuf::from(text)
        };
        Some(crate::hooks::write::wave_copy::lexical(&if path.is_absolute() { path } else { here.join(path) }))
    }

    /// O texto de um programa cita um caminho que existe no projeto: a palavra
    /// com `/` ou `.`, lida a partir de `here` ou, quando começa em `/`, também
    /// como o resto de um caminho montado (`f"{root}/src/x.rs"`).
    fn cited(&self, text: &str, here: &Path) -> bool {
        text.split(|c: char| !(c.is_alphanumeric() || "_-./~+@".contains(c)))
            .map(|token| token.trim_end_matches('.'))
            .filter(|token| token.contains(['/', '.']) && token.chars().any(char::is_alphanumeric))
            .any(|token| {
                let joined = here.join(token.trim_start_matches('/'));
                self.resolved(here, token).into_iter().chain([joined]).any(|path| path.exists() && self.holds(&path))
            })
    }

    /// O interpretador mexe em arquivo do projeto por uma das três formas
    /// dele: edição no lugar, programa na linha que cita caminho do projeto
    /// ou programa guardado fora do projeto.
    fn interprets(&self, grammar: &Grammar, segment: &Segment, here: &Path) -> bool {
        let outside = |text: &str| self.resolved(here, text).is_some_and(|path| !self.holds(&path));
        match program(grammar, &segment.args) {
            Program::InPlace => true,
            Program::Module => false,
            Program::Embedded(codes) => codes.iter().any(|code| self.cited(code, here)),
            Program::Script(script) => outside(&script),
            Program::Stdin => segment.redirects.iter().any(|redirect| {
                match (redirect.op.trim_start_matches(|c: char| c.is_ascii_digit()), &redirect.body) {
                    (_, Some(body)) => self.cited(body, here),
                    ("<<<", None) => self.cited(&redirect.target.text, here),
                    ("<", None) => outside(&redirect.target.text),
                    _ => false,
                }
            }),
        }
    }

    /// `cat`, `echo`, `printf` ou `tee` grava num arquivo do projeto, pelo
    /// redirecionamento ou pelo arquivo do `tee`.
    fn written(&self, segment: &Segment, here: &Path) -> bool {
        let teed = segment.args.iter().filter(|_| segment.name() == "tee").filter(|arg| !arg.text.starts_with('-'));
        let redirected = segment.redirects.iter().filter(|redirect| writes(redirect)).map(|redirect| &redirect.target);
        teed.chain(redirected)
            .filter(|target| !target.text.starts_with('&'))
            .any(|target| self.resolved(here, &target.text).is_some_and(|path| self.holds(&path)))
    }

    fn edits(&self, segment: &Segment, here: &Path) -> bool {
        match segment.name() {
            "sed" => sed_in_place(&segment.args),
            name if WRITERS.contains(&name) => self.written(segment, here),
            name => grammar(name).is_some_and(|grammar| self.interprets(grammar, segment, here)),
        }
    }
}

/// A recusa do comando do agente de onda que mexe em arquivo do projeto
/// pelo terminal, com a ferramenta de edição do hospedeiro que o chamou.
pub(crate) fn bash_shell_edit(segments: &[Segment], input: &HookInput, ctx: &Ctx, lang: Locale) -> Option<Verdict> {
    if !segments.iter().any(suspect) {
        return None;
    }
    let root = Path::new(&ctx.project_dir);
    let mut here = crate::hooks::write::wave_copy::wave_shell_folder(root, input, ctx)?;
    let project = Project::new(root, mustard_core::platform::harness::home_dir());
    for segment in segments {
        if segment.name() == "cd" {
            let target = match segment.args.iter().find(|arg| !arg.text.starts_with('-')) {
                Some(arg) => project.resolved(&here, &arg.text),
                None => project.home.clone(),
            };
            here = target.unwrap_or(here);
            continue;
        }
        if project.edits(segment, &here) {
            let edit = if input.raw["mustard_host"] == "codex" { "`apply_patch`" } else { "`Edit`/`Write`" };
            return Some(Verdict::Deny { reason: crate::shared::say::say("command_guard.shell_edit", lang, &[("{edit}", edit)]) });
        }
    }
    None
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
    /// lugar e o texto gravado por `cat`, `echo`, `printf` ou `tee`. A recusa
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
    /// um `cd` para fora dele.
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
        ];
        for command in &free {
            let verdict = guard(&scene, command, true, "claude");
            assert!(!matches!(verdict, Verdict::Deny { .. }), "{command}: {verdict:?}");
        }
    }
}
