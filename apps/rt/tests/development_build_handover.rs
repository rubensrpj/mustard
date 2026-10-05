// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]

//! A passagem da chamada ao programa compilado da branch, pelo binário de
//! verdade: um `mustard-rt` instalado — uma cópia do programa, fora de
//! qualquer repositório — roda de dentro de um repositório de mentira que
//! declara o pacote `mustard-rt`, e o programa compilado é um script que diz
//! o que recebeu.
//!
//! - O instalado, de dentro do repositório, de uma subpasta ou de uma cópia de
//!   onda, passa tudo ao compilado: argumentos, entrada e a guarda de um salto.
//! - Fora do repositório, o instalado responde por si.
//! - O que o próprio repositório compilou — a pasta `target`, uma cópia de
//!   onda, outra pasta de saída do cargo — responde por si.
//! - O compilado nunca passa de novo, nem a um plugin mais novo do registro.
//! - Sem compilado, ou com um que não abre, o instalado responde por si.

#[path = "support/executable.rs"]
mod executable;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;

/// O que o compilado de mentira imprime antes de repetir a entrada: os
/// argumentos que recebeu e a guarda de um salto.
const COMPILED: &str = "#!/bin/sh\nprintf 'compiled:%s:%s:' \"$*\" \"$MUSTARD_RT_DELEGATED\"\ncat\n";

/// O que o plugin mais novo do registro, o chamariz, imprime.
const DECOY: &str = "#!/bin/sh\necho decoy\n";

/// A máquina de mentira: o repositório do Mustard, o programa instalado fora
/// dele, a pasta onde o compilado nasce e a pasta pessoal.
struct Machine {
    _dir: TempDir,
    base: PathBuf,
    repo: PathBuf,
}

/// Uma cópia do programa deste pacote em `to`, feita por outro processo para
/// que o arquivo nunca fique aberto para escrita aqui enquanto um vizinho o
/// roda.
fn copy_the_program(to: &Path) {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    let copied = Command::new("cp").arg(env!("CARGO_BIN_EXE_mustard-rt")).arg(to).status().expect("cp runs");
    assert!(copied.success(), "{} was not copied", to.display());
}

fn git(at: &Path, args: &[&str]) {
    let out = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(at).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

impl Machine {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().canonicalize().expect("the canonical temp folder");
        let repo = base.join("qualquer-nome");
        std::fs::create_dir_all(repo.join("apps/rt")).unwrap();
        std::fs::write(repo.join("apps/rt/Cargo.toml"), "[package]\nname = \"mustard-rt\"\nversion = \"0.1.0\"\n").unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "o programa"]);
        Self { _dir: dir, base, repo }
    }

    /// A pasta do compilado do projeto `repo` nesta máquina: a base e a chave
    /// do projeto.
    fn build_folder_of(&self, repo: &Path) -> PathBuf {
        let key = mustard_core::io::wave_prompt::development_build_dir(repo);
        self.base.join("build").join(key.file_name().expect("the project key"))
    }

    /// A pasta do compilado do repositório do Mustard desta máquina.
    fn build_folder(&self) -> PathBuf {
        self.build_folder_of(&self.repo)
    }

    /// O compilado de mentira do repositório do Mustard, pronto para rodar.
    fn compiled(&self) -> PathBuf {
        self.compiled_for(&self.repo)
    }

    /// O compilado de mentira do projeto `repo`, pronto para rodar.
    fn compiled_for(&self, repo: &Path) -> PathBuf {
        let program = self.build_folder_of(repo).join("release").join("mustard-rt");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        executable::write_executable(&program, COMPILED);
        program
    }

    /// O programa instalado, fora de qualquer repositório.
    fn installed(&self) -> PathBuf {
        let program = self.base.join("plugin").join("bin").join("mustard-rt");
        copy_the_program(&program);
        program
    }

    /// Uma cópia de onda do repositório: um `worktree` na pasta das cópias.
    fn wave_copy(&self) -> PathBuf {
        let key = self.build_folder().file_name().unwrap().to_os_string();
        let copy = self.base.join("copias").join(key).join("spec").join("a");
        git(&self.repo, &["worktree", "add", "-q", "--detach", &copy.to_string_lossy(), "HEAD"]);
        copy
    }

    /// O registro de plugins com um plugin mais novo que o programa, cujo
    /// programa é o chamariz.
    fn newer_plugin_in_the_registry(&self) {
        let plugin = self.base.join("plugin-novo");
        std::fs::create_dir_all(plugin.join("bin")).unwrap();
        executable::write_executable(&plugin.join("bin").join("mustard-rt"), DECOY);
        let registry = self.base.join("claude").join("plugins");
        std::fs::create_dir_all(&registry).unwrap();
        let record = serde_json::json!({"version": 2, "plugins": {"mustard@mustard-local": [
            {"scope": "user", "version": "99.0.0", "installPath": plugin.to_string_lossy()}]}});
        std::fs::write(registry.join("installed_plugins.json"), record.to_string()).unwrap();
    }

    /// `program` com `args` e `stdin`, de dentro de `cwd`, e o que imprimiu.
    fn run(&self, program: &Path, cwd: &Path, args: &[&str], stdin: &str, delegated: bool) -> String {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .env("HOME", &self.base)
            .env("USERPROFILE", &self.base)
            .env("CLAUDE_CONFIG_DIR", self.base.join("claude"))
            .env("MUSTARD_BUILD_DIR", self.base.join("build"))
            .env("MUSTARD_COPIES_DIR", self.base.join("copias"))
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("MUSTARD_RT_DELEGATED")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if delegated {
            command.env("MUSTARD_RT_DELEGATED", "1");
        }
        let mut child = command.spawn().expect("the program runs");
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        let out = child.wait_with_output().expect("the program finishes");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// O `--version` de `program` de dentro de `cwd`.
    fn version(&self, program: &Path, cwd: &Path) -> String {
        self.run(program, cwd, &["--version"], "", false)
    }
}

/// A resposta do programa que rodou ele mesmo: a linha de versão.
fn assert_answers_itself(said: &str, case: &str) {
    assert!(said.starts_with("mustard-rt "), "{case}: o programa responde por si, e disse {said:?}");
}

/// O programa instalado, de dentro do repositório do Mustard — da raiz, de uma
/// subpasta ou de uma cópia de onda —, passa a chamada inteira ao compilado:
/// os argumentos, a entrada e a guarda de um salto, que o compilado recebe
/// ligada.
#[test]
fn an_installed_program_in_the_mustard_repository_hands_the_whole_call_to_the_compiled_one() {
    let machine = Machine::new();
    machine.compiled();
    let installed = machine.installed();
    let copy = machine.wave_copy();
    let cases = [
        ("the root", machine.repo.clone()),
        ("a subfolder", machine.repo.join("apps").join("rt")),
        ("a wave copy", copy.clone()),
        ("a subfolder of a wave copy", copy.join("apps").join("rt")),
    ];
    for (case, cwd) in cases {
        let said = machine.run(&installed, &cwd, &["on", "SessionStart"], "o evento", false);
        assert_eq!(said, "compiled:on SessionStart:1:o evento", "{case}");
    }
}

/// Fora do repositório do Mustard — uma pasta qualquer, ou um repositório de
/// outro projeto, ou um cujo `apps/rt` é de outro pacote — o instalado
/// responde por si, mesmo com o compilado em disco.
#[test]
fn outside_the_mustard_repository_the_installed_program_answers_itself() {
    let machine = Machine::new();
    machine.compiled();
    let installed = machine.installed();

    let loose = machine.base.join("solta");
    std::fs::create_dir_all(&loose).unwrap();
    machine.compiled_for(&loose);
    assert_answers_itself(&machine.version(&installed, &loose), "a folder outside git");

    let other = machine.base.join("outro");
    std::fs::create_dir_all(other.join("apps/rt")).unwrap();
    std::fs::write(other.join("apps/rt/Cargo.toml"), "[package]\nname = \"outro-app\"\n").unwrap();
    git(&other, &["init", "-q"]);
    machine.compiled_for(&other);
    assert_answers_itself(&machine.version(&installed, &other), "another project");
}

/// O que o próprio repositório compilou nunca passa a chamada: a pasta
/// `target` do checkout, uma pasta de `target` numa cópia de onda e outra
/// pasta de saída do cargo, qualquer que seja o lugar dela. O programa da
/// pasta do compilado também não: ele é o destino, não a origem.
#[test]
fn a_program_the_repository_built_itself_never_hands_over() {
    let machine = Machine::new();
    machine.compiled();
    let copy = machine.wave_copy();
    let elsewhere = machine.base.join("saida-do-cargo");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join(".rustc_info.json"), "{}").unwrap();
    let cases = [
        ("target of the checkout", machine.repo.join("target").join("debug").join("mustard-rt")),
        ("target of a wave copy", copy.join("target").join("debug").join("mustard-rt")),
        ("another cargo output folder", elsewhere.join("debug").join("mustard-rt")),
    ];
    for (case, program) in cases {
        copy_the_program(&program);
        assert_answers_itself(&machine.version(&program, &machine.repo), case);
    }
}

/// O compilado, que recebe a chamada do instalado, nunca passa de novo: nem a
/// um plugin mais novo do registro, que um programa que roda sozinho acharia
/// por si. O controle: sem o compilado, o instalado vai ao plugin mais novo.
#[test]
fn the_compiled_program_never_hands_over_again() {
    let machine = Machine::new();
    machine.newer_plugin_in_the_registry();
    let installed = machine.installed();
    let said = machine.version(&installed, &machine.repo);
    assert_eq!(said.trim(), "decoy", "without a compiled program the newer plugin of the registry answers");

    let compiled = machine.build_folder().join("release").join("mustard-rt");
    copy_the_program(&compiled);
    let said = machine.version(&installed, &machine.repo);
    assert_answers_itself(&said, "the compiled program is the real program, one hop away");
    assert!(!said.contains("decoy"), "the compiled program handed the call over again: {said}");
}

/// A guarda de um salto vale para o instalado também: com ela ligada, nada
/// passa, nem em cima do compilado.
#[test]
fn a_call_that_was_already_handed_over_is_never_handed_over_again() {
    let machine = Machine::new();
    machine.compiled();
    let installed = machine.installed();
    let said = machine.run(&installed, &machine.repo, &["--version"], "", true);
    assert_answers_itself(&said, "the guard is on");
}

/// Sem compilado em disco, ou com um que não abre, o instalado responde por
/// si, como respondia antes de a passagem existir.
#[test]
fn without_a_compiled_program_that_opens_the_installed_one_answers_itself() {
    use std::os::unix::fs::PermissionsExt as _;

    let machine = Machine::new();
    let installed = machine.installed();
    assert_answers_itself(&machine.version(&installed, &machine.repo), "no compiled program");

    let compiled = machine.compiled();
    std::fs::set_permissions(&compiled, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_answers_itself(&machine.version(&installed, &machine.repo), "a compiled program with no permission to run");
}
