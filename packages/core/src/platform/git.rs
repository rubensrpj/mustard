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
/// `root` — see the module note above. A project that pinned the key to an
/// empty string gets no spawn at all: the answer is a [`GitRun`] carrying
/// [`OPTED_OUT`], which every caller already degrades from the same way it
/// degrades from an absent binary.
///
/// Standard input is closed. Every call here is a probe made on the operator's
/// behalf while they are waiting on something else, and a git that decides to
/// ask for a credential would hang the whole invocation with nobody watching
/// for the question. With no input to read, it fails instead, and a failure is
/// something every caller already knows how to degrade from.
#[must_use]
pub fn run(root: &Path, args: &[&str]) -> GitRun {
    let owner = crate::io::workspace::anchor_of(root);
    let Some(binary) = ProjectConfig::load(owner.as_deref().unwrap_or(root)).vcs() else {
        return GitRun { ok: false, stdout: String::new(), stderr: OPTED_OUT.to_string() };
    };
    match Command::new(binary)
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
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

    /// Quem escolhe o programa é o `mustard.json`, não o executor: com outro
    /// programa nomeado ali, é ele que roda, e é a resposta dele que volta.
    ///
    /// O programa falso responde uma marca que o git nunca responderia, e
    /// responde a qualquer argumento — então esta asserção só passa se a
    /// configuração tiver sido lida de verdade.
    #[cfg(unix)]
    #[test]
    fn o_programa_que_roda_e_o_que_o_mustard_json_nomeia() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let fake = root.join("controle-de-versao-falso");
        std::fs::write(&fake, b"#!/bin/sh\nprintf 'quem-respondeu-foi-o-escolhido\\n'\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            root.join("mustard.json"),
            format!("{{\"vcs\": \"{}\"}}\n", fake.display()),
        )
        .unwrap();

        assert_eq!(
            run(root, &["rev-parse", "--abbrev-ref", "HEAD"]).out().as_deref(),
            Some("quem-respondeu-foi-o-escolhido"),
            "o programa nomeado no mustard.json é quem responde",
        );
    }

    /// A configuração é do PROJETO, não da pasta em que o comando roda. Quem
    /// chama roda o git onde precisa — a pasta do arquivo que vai escrever,
    /// uma subpasta, o `.claude/` — e só a raiz carrega o `mustard.json`.
    ///
    /// O programa falso responde uma marca que o git nunca responderia, e a
    /// chamada é feita de uma subpasta: ler ao lado do diretório de trabalho
    /// devolveria "git" e a marca não apareceria.
    #[cfg(unix)]
    #[test]
    fn a_configuracao_lida_e_a_do_projeto_dono_da_pasta() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let fake = root.join("controle-de-versao-falso");
        std::fs::write(&fake, b"#!/bin/sh\nprintf 'quem-respondeu-foi-o-escolhido\\n'\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            root.join("mustard.json"),
            format!("{{\"vcs\": \"{}\"}}\n", fake.display()),
        )
        .unwrap();
        // A raiz de um projeto é `mustard.json` mais `.claude/`, e é essa a
        // âncora que a subida procura.
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        let deep = root.join("src").join("dentro");
        std::fs::create_dir_all(&deep).unwrap();

        assert_eq!(
            run(&deep, &["status"]).out().as_deref(),
            Some("quem-respondeu-foi-o-escolhido"),
            "a pergunta é feita de dentro, e quem responde é o programa do projeto",
        );
        // E a pasta `.claude`, que nunca é raiz de projeto, chega à mesma
        // resposta em vez de derrubar a leitura.
        assert_eq!(
            run(&root.join(".claude"), &["status"]).out().as_deref(),
            Some("quem-respondeu-foi-o-escolhido"),
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

        let refused = run(root, &["init", "-q", "."]);
        assert!(!refused.ok, "sem programa declarado não há corrida que dê certo");
        assert!(!root.join(".git").exists(), "nada foi chamado, então nada foi criado");
        assert!(
            refused.result().unwrap_err().contains("vcs"),
            "o motivo aponta a chave que produziu a recusa",
        );
    }
}
