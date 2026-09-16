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
//! with which arguments; everything after that — spawning, the exit status,
//! decoding the bytes, trimming — happens here, once.
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
//! not a repository, a non-zero exit: each comes back as a [`GitRun`] whose
//! [`ok`](GitRun::ok) is `false`, never as an error to propagate.

use std::path::Path;
use std::process::Command;

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

/// Run git in `root` with `args`.
///
/// The one function in this project that spawns git. The directory always
/// travels as the working directory — never as a `-C <dir>` argument — so a
/// caller can never put it in the wrong place, and `args` is passed through
/// verbatim, in order.
///
/// Standard input is closed. Every call here is a probe made on the operator's
/// behalf while they are waiting on something else, and a git that decides to
/// ask for a credential would hang the whole invocation with nobody watching
/// for the question. With no input to read, it fails instead, and a failure is
/// something every caller already knows how to degrade from.
#[must_use]
pub fn run(root: &Path, args: &[&str]) -> GitRun {
    match Command::new("git")
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
}
