//! `git` — the one place this project runs git.
//!
//! ## Why this exists
//!
//! Every module that needed a fact from the repository used to build the
//! invocation by hand: `Command::new("git").args(…).current_dir(…).output()`,
//! then its own reading of the exit status, its own `from_utf8_lossy`, its own
//! `trim`. The call was the same everywhere; the ERROR HANDLING was not. One
//! module folded empty stdout into "could not measure", another kept the two
//! apart; one read stderr, most threw it away; one passed the directory as
//! `-C <dir>` and the rest as the working directory. Each of those differences
//! is a decision, and a decision repeated by hand is a decision that drifts.
//!
//! So there is exactly one function that spawns git, [`run`], and exactly one
//! type that reads what it left behind, [`GitRun`]. A caller says where and
//! with which arguments; everything after that — WHICH PROGRAM, spawning, the
//! exit status, decoding the bytes, trimming — happens here, once.
//!
//! ## Which program
//!
//! The name of the binary is a project's answer, not a constant: `mustard.json`
//! carries it under `vcs`, absent meaning `git` and empty meaning "this project
//! controls no versions". That answer used to be read by each caller and then
//! threaded through every helper it passed on the way down, which is the same
//! decision spelled out by hand ten times over. It is read HERE now, so a
//! caller asks the repository a question and never has to know what answers it.
//!
//! It is read from the PROJECT that owns the directory, never from the
//! directory itself. A caller runs git wherever it needs to — the folder of the
//! file it is about to write, a subdirectory, `.claude/` — and only the project
//! root carries `mustard.json`; reading beside the working directory would
//! answer `git` for every call made one folder down, which is the silent wrong
//! answer this whole task exists to remove. The project is found by
//! [`crate::io::workspace::anchor_of`], the workspace resolver's own ancestor
//! walk, so this is not a second opinion about where a project begins.
//!
//! The separate copy of a wave lives outside its project and carries no
//! `mustard.json`, so the walk from inside it finds nothing. It is a linked
//! worktree, though, and its `.git` file points back at the main checkout:
//! when the walk finds no project, the owner is looked for there
//! ([`crate::io::workspace::worktree_main_from_files`]), by reading files and
//! never by running git — this function cannot call itself to find out how to
//! call itself. When neither finds a project, nobody declared anything, and the
//! program is the default one. A `mustard.json` lying beside the directory
//! without its `.claude/` is not a project, and it is not read: reading it
//! would be a second rule for where a project begins.
//!
//! ## What it deliberately is not
//!
//! **No trait, and no test double.** An injectable seam existed to let a test
//! pretend to be git; the tests of this project create a real repository in a
//! temporary directory instead, which measures the thing itself rather than a
//! description of it. A layer in front of a double nobody uses is machinery,
//! and this project is removing machinery.
//!
//! **No policy.** This module decides nothing about branches, bases,
//! protection or remotes — it runs a command and hands back what happened.
//! Whether a failure means "degrade" or "refuse" belongs to the caller, which
//! is the only layer that knows what the answer is for.
//!
//! **Nothing here panics and nothing here blocks.** git missing, the directory
//! not a repository, a non-zero exit, a project that opted out: each comes back
//! as a [`GitRun`] whose [`ok`](GitRun::ok) is `false`, never as an error to
//! propagate.

use crate::domain::config::ProjectConfig;
use std::path::Path;
use std::process::Command;

/// What a call reports when the project declares no version control at all.
///
/// It is a refusal to spawn, not a failed spawn: nothing ran, so nothing can be
/// blamed on the machine. The text names the file and the key so an operator
/// reading a degraded answer can find the line that produced it.
const OPTED_OUT: &str = "mustard.json: vcs \"\" — this project controls no versions";

/// What one git invocation left behind.
///
/// `stdout` and `stderr` are the raw decoded streams — untrimmed, because a
/// caller that parses columns or counts lines needs them as git wrote them.
/// The readings that DO trim are the two methods, so "trimmed stdout when git
/// succeeded" is spelled once for the whole project instead of at every call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRun {
    /// `true` only when git ran AND exited zero. A binary that is absent and a
    /// command that failed are the same answer to every caller here: the fact
    /// could not be obtained.
    pub ok: bool,
    /// Standard output, decoded lossily and left exactly as git wrote it.
    pub stdout: String,
    /// Standard error, decoded lossily. When git could not be spawned at all
    /// this carries the spawn failure, so the reason is never lost.
    pub stderr: String,
}

impl GitRun {
    /// Trimmed stdout when git succeeded, `None` for every failure.
    ///
    /// The reading almost every caller wants, and the reason they no longer
    /// write it themselves. Note what it does NOT do: an empty answer from a
    /// successful command comes back as `Some("")`, never as `None` — "git
    /// said nothing" and "git could not be asked" are different facts, and
    /// folding them together is what turns an offline machine into a
    /// repository with no branches.
    #[must_use]
    pub fn out(&self) -> Option<String> {
        self.ok.then(|| self.stdout.trim().to_string())
    }

    /// Trimmed stdout on success, trimmed stderr on failure — for the callers
    /// whose refusal text the operator must actually see.
    ///
    /// # Errors
    ///
    /// The trimmed standard error whenever git failed or could not be spawned.
    pub fn result(self) -> Result<String, String> {
        if self.ok {
            Ok(self.stdout.trim().to_string())
        } else {
            Err(self.stderr.trim().to_string())
        }
    }
}

/// Run the project's version-control program in `root` with `args`.
///
/// The one function in this project that spawns it. The directory always
/// travels as the working directory — never as a `-C <dir>` argument — so a
/// caller can never put it in the wrong place, and `args` is passed through
/// verbatim, in order.
///
/// Which program it is comes from the `mustard.json` of the project that owns
/// `root`, a wave copy outside it included — see the module note above. A project that pinned the key to an
/// empty string gets no spawn at all: the answer is a [`GitRun`] carrying
/// [`OPTED_OUT`], which every caller already degrades from the same way it
/// degrades from an absent binary.
///
/// Nothing here may stop to ask the operator anything. Every call is a probe
/// made on their behalf while they wait on something else, and a question
/// nobody is watching for hangs the whole invocation. Standard input is
/// closed, and that is not enough on its own: git asks for a credential on the
/// TERMINAL, not on standard input, so the terminal question is turned off
/// too. A credential that is already stored still answers; a missing one
/// makes the call fail, and a failure is something every caller already knows
/// how to degrade from.
#[must_use]
pub fn run(root: &Path, args: &[&str]) -> GitRun {
    let config = owner_of(root).map(|owner| ProjectConfig::load(&owner)).unwrap_or_default();
    let Some(binary) = config.vcs() else {
        return GitRun { ok: false, stdout: String::new(), stderr: OPTED_OUT.to_string() };
    };
    match Command::new(binary)
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
    {
        Ok(out) => GitRun {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        },
        Err(err) => GitRun { ok: false, stdout: String::new(), stderr: err.to_string() },
    }
}

/// The project that owns `root`: the ancestor walk first; for a wave copy that
/// lives outside its project, the main checkout its `.git` file points at.
/// Both by reading files — asking git here would be asking git how to ask git.
fn owner_of(root: &Path) -> Option<std::path::PathBuf> {
    crate::io::workspace::anchor_of(root).or_else(|| {
        crate::io::workspace::worktree_main_from_files(root)
            .and_then(|main| crate::io::workspace::anchor_of(&main))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um lugar onde o git não consegue responder não devolve erro nem
    /// derruba nada: devolve uma corrida que não deu certo, e a leitura
    /// trimada vira "não medido".
    #[test]
    fn um_lugar_sem_repositorio_devolve_corrida_sem_sucesso() {
        let run = run(Path::new("/no/such/place/at/all"), &["status"]);
        assert!(!run.ok, "nem o diretório existe: {run:?}");
        assert_eq!(run.out(), None, "não medido nunca vira resposta vazia");
        assert!(run.result().is_err(), "o motivo chega a quem precisa dele");
    }

    /// Um repositório de verdade, em pasta temporária: sem dublê, sem
    /// interface. A resposta vazia de um comando que deu certo continua sendo
    /// uma resposta, e não "não medido".
    #[test]
    fn um_repositorio_de_verdade_responde_e_o_vazio_continua_resposta() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        if !run(root, &["init", "-q", "-b", "trunk", "."]).ok {
            return; // sem git utilizável aqui
        }
        let _ = run(root, &["config", "user.email", "t@t.t"]);
        let _ = run(root, &["config", "user.name", "t"]);
        assert!(run(root, &["commit", "-q", "--allow-empty", "-m", "semente"]).ok);

        assert_eq!(
            run(root, &["rev-parse", "--abbrev-ref", "HEAD"]).out().as_deref(),
            Some("trunk"),
            "a saída chega trimada",
        );
        assert_eq!(
            run(root, &["status", "--porcelain"]).out().as_deref(),
            Some(""),
            "árvore limpa responde vazio, e vazio não é ausência de resposta",
        );
    }

    /// Quando o git recusa, o texto da recusa não se perde: ele chega por
    /// `result`, que é o que um comando mostra ao operador.
    #[test]
    fn a_recusa_do_git_chega_com_o_texto_dela() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        if !run(root, &["init", "-q", "."]).ok {
            return;
        }
        let refused = run(root, &["rev-parse", "--verify", "naoexiste"]);
        assert!(!refused.ok);
        let message = refused.result().unwrap_err();
        assert!(!message.is_empty(), "o motivo da recusa é passado adiante");
    }

    /// A marca que só o programa falso imprime: o git nunca a responderia.
    const MARK: &str = "quem-respondeu-foi-o-escolhido";

    /// Não prova comportamento nenhum: é o programa falso que os testes abaixo
    /// declaram como controle de versão. Rodado pela suíte, só passa. Chamado
    /// pelo executor, imprime a marca e o valor que recebeu para o pedido de
    /// credencial por terminal.
    #[test]
    fn programa_falso() {
        let prompt = std::env::var("GIT_TERMINAL_PROMPT").unwrap_or_default();
        println!("{MARK} pedido-de-credencial={prompt}");
    }

    /// Declara o próprio executável destes testes como o programa de controle
    /// de versão no `mustard.json` de `dir`.
    ///
    /// O executável de testes existe nos três sistemas e roda de verdade
    /// quando chamado, então o programa falso não depende de script de shell.
    /// Quando `project` é verdadeiro, `dir` ganha também a `.claude/`, e só
    /// então passa a ser a raiz de um projeto.
    fn declare_fake_program(dir: &Path, project: bool) {
        let exe = std::env::current_exe().expect("o executável dos testes");
        std::fs::write(dir.join("mustard.json"), serde_json::json!({ "vcs": exe }).to_string())
            .unwrap();
        if project {
            std::fs::create_dir_all(dir.join(".claude")).unwrap();
        }
    }

    /// Chama o executor com os argumentos que fazem o executável de testes
    /// rodar só o [`programa_falso`]. Os nomes de teste não levam o nome do
    /// pacote, que vem na frente do caminho do módulo.
    fn run_fake(dir: &Path) -> GitRun {
        let module = module_path!();
        let module = module.split_once("::").map_or(module, |(_, rest)| rest);
        let test = format!("{module}::programa_falso");
        run(dir, &[&test, "--exact", "--nocapture"])
    }

    /// Quem escolhe o programa é o `mustard.json`, não o executor: com outro
    /// programa nomeado ali, é ele que roda, e é a resposta dele que volta.
    ///
    /// O programa falso responde uma marca que o git nunca responderia, então
    /// esta asserção só passa se a configuração tiver sido lida de verdade.
    #[test]
    fn o_programa_que_roda_e_o_que_o_mustard_json_nomeia() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        declare_fake_program(root, true);

        let answer = run_fake(root);
        assert!(
            answer.out().is_some_and(|out| out.contains(MARK)),
            "o programa nomeado no mustard.json é quem responde: {answer:?}",
        );
    }

    /// A configuração é do PROJETO, não da pasta em que o comando roda. Quem
    /// chama roda o git onde precisa — a pasta do arquivo que vai escrever,
    /// uma subpasta, o `.claude/` — e só a raiz carrega o `mustard.json`.
    ///
    /// A chamada é feita de uma subpasta, onde não há `mustard.json` nenhum:
    /// sem a subida até a raiz, o programa seria o padrão e a marca não
    /// apareceria.
    #[test]
    fn a_configuracao_lida_e_a_do_projeto_dono_da_pasta() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        declare_fake_program(root, true);
        let deep = root.join("src").join("dentro");
        std::fs::create_dir_all(&deep).unwrap();

        let answer = run_fake(&deep);
        assert!(
            answer.out().is_some_and(|out| out.contains(MARK)),
            "a pergunta é feita de dentro, e quem responde é o programa do projeto: {answer:?}",
        );
        // E a pasta `.claude`, que nunca é raiz de projeto, chega à mesma
        // resposta em vez de derrubar a leitura.
        let answer = run_fake(&root.join(".claude"));
        assert!(answer.out().is_some_and(|out| out.contains(MARK)), "{answer:?}");
    }

    /// Um `mustard.json` numa pasta que não é projeto — sem a `.claude/` ao
    /// lado — não escolhe o programa: sem projeto dono, vale o padrão, e a
    /// marca do programa falso não aparece.
    #[test]
    fn um_mustard_json_fora_de_projeto_nao_escolhe_o_programa() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        declare_fake_program(root, false);

        let answer = run_fake(root);
        assert!(
            !answer.stdout.contains(MARK),
            "a pasta sem projeto não é lida, e o programa falso não é chamado: {answer:?}",
        );
    }

    /// O programa chamado recebe desligado o pedido de credencial por
    /// terminal. Fechar a entrada não basta: o git pede a credencial no
    /// terminal, e uma sondagem parada ali trava a chamada inteira sem
    /// ninguém para responder.
    #[test]
    fn o_programa_chamado_nao_pede_credencial_no_terminal() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        declare_fake_program(root, true);

        let answer = run_fake(root);
        assert!(
            answer
                .out()
                .is_some_and(|out| out.contains(&format!("{MARK} pedido-de-credencial=0"))),
            "o pedido de credencial chega desligado ao programa: {answer:?}",
        );
    }

    /// Projeto que declarou não controlar versões não faz o executor chamar
    /// programa nenhum: um `init` que tivesse rodado deixaria um `.git` para
    /// trás, e é a ausência dele que prova que nada foi chamado.
    #[test]
    fn o_projeto_que_dispensa_controle_de_versao_nao_chama_programa_nenhum() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("mustard.json"), b"{\"vcs\": \"\"}\n").unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        let refused = run(root, &["init", "-q", "."]);
        assert!(!refused.ok, "sem programa declarado não há corrida que dê certo");
        assert!(!root.join(".git").exists(), "nada foi chamado, então nada foi criado");
        assert!(
            refused.result().unwrap_err().contains("vcs"),
            "o motivo aponta a chave que produziu a recusa",
        );
    }

    /// Monta à mão, sem rodar git, o que o `git worktree add` deixa: no
    /// projeto, a pasta `.git/worktrees/<nome>` com o `commondir`; na cópia,
    /// o arquivo `.git` apontando para ela, pelo caminho que `pointer` dá.
    ///
    /// Montada assim, a cópia não é um worktree que o git aceite: só quem a
    /// lê como arquivo chega ao projeto por ela.
    fn link_copy(project: &Path, copy: &Path, pointer: &str) {
        let admin = project.join(".git").join("worktrees").join("copia");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::create_dir_all(copy).unwrap();
        std::fs::write(copy.join(".git"), format!("gitdir: {pointer}\n")).unwrap();
    }

    /// A cópia de uma onda mora fora da pasta do projeto e não traz o
    /// `mustard.json`: o programa é o que o projeto dono declarou, achado pelo
    /// arquivo `.git` da cópia; e o projeto que desligou o controle de versão
    /// não roda nada, nem de dentro da cópia.
    #[test]
    fn a_git_run_inside_a_wave_copy_uses_the_vcs_its_owner_project_declared() {
        let tmp = tempfile::tempdir().unwrap();
        let copies = tmp.path().join("copias");

        // O projeto que nomeou outro programa; a cópia aponta para ele por um
        // caminho relativo, como o git grava quando pedem caminhos relativos.
        let named = tmp.path().join("nomeou");
        std::fs::create_dir_all(&named).unwrap();
        declare_fake_program(&named, true);
        let copy = copies.join("nomeou-1");
        link_copy(&named, &copy, "../../nomeou/.git/worktrees/copia");
        let deep = copy.join("src");
        std::fs::create_dir_all(&deep).unwrap();
        let answer = run_fake(&deep);
        assert!(
            answer.out().is_some_and(|out| out.contains(MARK)),
            "de dentro da cópia, quem responde é o programa do projeto dono: {answer:?}",
        );

        // O projeto que desligou o controle de versão; o caminho é absoluto.
        let off = tmp.path().join("desligou");
        std::fs::create_dir_all(off.join(".claude")).unwrap();
        std::fs::write(off.join("mustard.json"), b"{\"vcs\": \"\"}\n").unwrap();
        let copy = copies.join("desligou-1");
        let admin = off.join(".git").join("worktrees").join("copia");
        link_copy(&off, &copy, &admin.to_string_lossy());
        let inner = copy.join("dentro");
        std::fs::create_dir_all(&inner).unwrap();
        let refused = run(&inner, &["init", "-q", "."]);
        assert_eq!(refused.stderr, OPTED_OUT, "a cópia recusa como o projeto dono: {refused:?}");
        assert!(!inner.join(".git").exists(), "nada foi chamado, então nada foi criado");

        // Um worktree de verdade, quando há git aqui: o arquivo que o git grava
        // leva ao mesmo projeto.
        let real = tmp.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        if !run(&real, &["init", "-q", "."]).ok {
            return; // sem git utilizável aqui
        }
        let _ = run(&real, &["config", "user.email", "t@t.t"]);
        let _ = run(&real, &["config", "user.name", "t"]);
        assert!(run(&real, &["commit", "-q", "--allow-empty", "-m", "semente"]).ok);
        let copy = copies.join("real-1");
        let added = run(&real, &["worktree", "add", "-q", "-b", "onda-1", &copy.to_string_lossy()]);
        assert!(added.ok, "{added:?}");
        declare_fake_program(&real, true);
        let answer = run_fake(&copy);
        assert!(
            answer.out().is_some_and(|out| out.contains(MARK)),
            "o arquivo que o git grava leva ao projeto dono: {answer:?}",
        );
    }
}
