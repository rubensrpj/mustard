//! A troca do rtk feita por dentro do gancho do Mustard.
//!
//! O rtk encurta a saída dos comandos trocando o comando antes de ele rodar.
//! O gancho do próprio rtk (`rtk hook claude`) fazia essa troca ao lado do
//! gancho do Mustard, e o host ficava com uma troca só: ora o comando perdia a
//! cópia da onda, a sessão e o teto do cargo, ora perdia o rtk. Aqui a troca
//! do rtk entra antes do embrulho da cópia, da sessão e do condutor, e o
//! `rtk` que o comando trocado chama é a cópia que acompanha o Mustard, pelo
//! caminho inteiro, nunca a que o `PATH` acha primeiro.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mustard_core::domain::model::contract::{HookInput, Verdict};
use mustard_core::platform::code_tools::{RunOutcome, ToolRunner};
use serde_json::{Value, json};

use super::lex;

/// O nome do programa do rtk nesta plataforma.
const RTK_FILE: &str = if cfg!(windows) { "rtk.exe" } else { "rtk" };

/// O prazo da troca: o `rtk rewrite` só devolve texto, nunca roda o comando.
const REWRITE_LIMIT: Duration = Duration::from_secs(1);

/// A cópia do rtk que acompanha o Mustard: ao lado do binário em execução
/// (pelo caminho real, já que `/usr/bin/mustard-rt` é atalho para a pasta do
/// pacote) ou, sem ela, na pasta `bin` do plugin instalado. `None` quando
/// nenhuma das duas existe.
pub(crate) fn bundled() -> Option<PathBuf> {
    // Os testes da biblioteca nunca acham o rtk instalado na máquina: a troca
    // entra neles pelo executor de mentira, e o resto dos ganchos fica igual
    // em qualquer máquina.
    if cfg!(test) {
        return None;
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.canonicalize().ok())
        .and_then(|exe| exe.parent().map(|dir| dir.join(RTK_FILE)))
        .filter(|path| path.is_file());
    beside.or_else(|| {
        mustard_core::platform::harness::installed_plugin()
            .map(|plugin| plugin.dir.join("bin").join(RTK_FILE))
            .filter(|path| path.is_file())
    })
}

/// `path` entre aspas simples, do jeito que o shell lê um caminho com espaço.
pub(crate) fn quoted(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// O veredito com a troca do rtk aplicada ao comando: `rtk rewrite` da cópia
/// `rtk` decide a troca, e cada `rtk` solto na posição de programa vira o
/// caminho inteiro da cópia. A recusa de quem veio antes fica como está; o
/// comando do próprio Mustard não é trocado, porque ele cuida da própria
/// saída. Sem troca válida, ou com um `rtk` solto que a troca não alcança, o
/// veredito volta igual. A saída 2 do rtk é a recusa pelas regras de permissão
/// dele, e vira recusa sem rodar nada.
pub(crate) fn swap(input: &HookInput, verdict: Verdict, rtk: &Path, runner: &impl ToolRunner) -> Verdict {
    let (mut tool_input, note) = match &verdict {
        Verdict::Allow => (input.tool_input.clone(), None),
        Verdict::Rewrite { tool_input, note } => (tool_input.clone(), note.clone()),
        _ => return verdict,
    };
    let Some(command) = tool_input.get("command").and_then(Value::as_str) else {
        return verdict;
    };
    if command.len() > 65_536
        || command.contains('\0')
        || lex::segments(command).iter().any(|seg| matches!(seg.name(), "mustard-rt" | "mustard-rt.exe" | "mustard-boot" | "mustard-boot.cmd"))
    {
        return verdict;
    }
    let program = rtk.to_string_lossy();
    let (status, exit_code, text) = runner.output_with_exit(&program, &["rewrite", "--", command], REWRITE_LIMIT);
    if status == RunOutcome::Failed && exit_code == Some(2) {
        return Verdict::Deny { reason: "[Mustard] RTK rejected this command through its configured permission rules. Nothing was executed.".into() };
    }
    // O rtk 0.49 sai 3 numa troca válida sem regra de liberação do host, e 0
    // com ela; qualquer outra saída deixa o comando como veio.
    let rewritten = text.trim();
    let valid = (status == RunOutcome::Succeeded && exit_code == Some(0) || status == RunOutcome::Failed && exit_code == Some(3))
        && !rewritten.is_empty()
        && rewritten.len() <= 131_072
        && !rewritten.contains('\0');
    let base = if valid { rewritten } else { command };
    let Some(swapped) = pin_rtk(base, &quoted(rtk)) else {
        return verdict;
    };
    if swapped == command {
        return verdict;
    }
    tool_input["command"] = json!(swapped);
    Verdict::Rewrite { tool_input, note }
}

/// `command` com cada `rtk` solto, na posição de programa de um comando
/// simples de fora (depois das atribuições `NOME=valor`), trocado por
/// `pinned`. `None` quando sobra a palavra `rtk` em qualquer outro lugar, como
/// dentro de um `$(…)` que essa leitura não alcança: melhor o comando
/// original que um `rtk` achado pelo `PATH`.
fn pin_rtk(command: &str, pinned: &str) -> Option<String> {
    let mut out = command.to_string();
    for range in lex::command_ranges(command).into_iter().rev() {
        if let Some(at) = rtk_word(&command[range.clone()]) {
            let start = range.start + at;
            out.replace_range(start..start + "rtk".len(), pinned);
        }
    }
    (!out.split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | '`')).any(|word| word == "rtk")).then_some(out)
}

/// Onde começa o programa `rtk` solto de um comando simples, depois dos
/// espaços e das atribuições `NOME=valor`; `None` quando o programa é outro.
fn rtk_word(part: &str) -> Option<usize> {
    let mut at = 0;
    loop {
        let rest = &part[at..];
        let blank = rest.len() - rest.trim_start().len();
        at += blank;
        let word_end = word_end(&part[at..]);
        let word = &part[at..at + word_end];
        if word.is_empty() {
            return None;
        }
        if is_assignment(word) {
            at += word_end;
            continue;
        }
        return (word == "rtk").then_some(at);
    }
}

/// O fim da palavra que começa `text`, com as aspas simples e duplas
/// guardando os espaços de dentro.
fn word_end(text: &str) -> usize {
    let mut quote = None;
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, c) if c.is_whitespace() => return at,
            _ => {}
        }
    }
    text.len()
}

/// `true` quando `word` é uma atribuição `NOME=valor` antes do programa.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        let mut chars = name.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O rtk de mentira: confere o pedido e devolve a saída e o código dados.
    struct Rewriter {
        exit: Option<i32>,
        status: RunOutcome,
        text: &'static str,
    }
    impl ToolRunner for Rewriter {
        fn on_path(&self, _: &str) -> bool {
            panic!("no PATH probe")
        }
        fn run(&self, _: &str, _: &[&str]) -> bool {
            panic!("never execute the user's command")
        }
        fn found_off_path(&self, _: &str) -> Option<PathBuf> {
            None
        }
        fn output_with_exit(&self, program: &str, args: &[&str], limit: Duration) -> (RunOutcome, Option<i32>, String) {
            assert_eq!(program, "/opt/mustard bin/rtk", "the bundled copy, never the PATH one");
            assert_eq!(&args[..2], ["rewrite", "--"]);
            assert_eq!(limit, REWRITE_LIMIT);
            (self.status, self.exit, self.text.into())
        }
    }
    const RTK: &str = "/opt/mustard bin/rtk";
    const PINNED: &str = "'/opt/mustard bin/rtk'";

    fn bash(command: &str) -> HookInput {
        serde_json::from_value(json!({"tool_name":"Bash","tool_input":{"command":command,"timeout":600_000,"description":"d"}})).unwrap()
    }
    fn rewrite(exit: i32, text: &'static str) -> Rewriter {
        Rewriter { exit: Some(exit), status: if exit == 0 { RunOutcome::Succeeded } else { RunOutcome::Failed }, text }
    }
    fn command_of(verdict: &Verdict) -> &str {
        match verdict {
            Verdict::Rewrite { tool_input, .. } => tool_input["command"].as_str().unwrap(),
            other => panic!("expected a rewrite, got {other:?}"),
        }
    }

    /// Cada `rtk` que a troca põe na frente de um comando simples, depois de
    /// atribuições e de `cd`, chama a cópia pelo caminho inteiro; os outros
    /// campos da chamada ficam.
    #[test]
    fn every_rtk_the_rewrite_adds_runs_the_bundled_copy() {
        let input = bash("FOO=1 cargo test; cd /tmp && git diff");
        let verdict = swap(&input, Verdict::Allow, Path::new(RTK), &rewrite(3, "FOO=1 rtk cargo test; cd /tmp && rtk git diff\n"));
        assert_eq!(command_of(&verdict), format!("FOO=1 {PINNED} cargo test; cd /tmp && {PINNED} git diff"));
        let Verdict::Rewrite { tool_input, .. } = verdict else { unreachable!() };
        assert_eq!(tool_input["timeout"], 600_000);
        assert_eq!(tool_input["description"], "d");
    }

    /// O `rtk` que o próprio agente escreveu também vai para a cópia, mesmo
    /// quando a troca não tem nada a acrescentar.
    #[test]
    fn an_rtk_the_agent_wrote_also_runs_the_bundled_copy() {
        let verdict = swap(&bash("rtk git status"), Verdict::Allow, Path::new(RTK), &rewrite(1, ""));
        assert_eq!(command_of(&verdict), format!("{PINNED} git status"));
    }

    /// A recusa de antes fica; o comando do Mustard e o comando sem troca
    /// passam como vieram; a saída 2 do rtk recusa; uma saída inválida ou
    /// vazia não troca nada.
    #[test]
    fn denials_own_commands_and_invalid_rewrites_keep_the_command() {
        let runner = rewrite(3, "rtk git status");
        let denied = swap(&bash("git status"), Verdict::Deny { reason: "protected".into() }, Path::new(RTK), &runner);
        assert!(matches!(denied, Verdict::Deny { reason } if reason == "protected"));
        let own = swap(&bash("mustard-rt run search --request '{}'"), Verdict::Allow, Path::new(RTK), &runner);
        assert!(matches!(own, Verdict::Allow));
        assert!(matches!(swap(&bash("git status"), Verdict::Allow, Path::new(RTK), &rewrite(2, "")), Verdict::Deny { .. }));
        for code in [1, 4, 126, 127] {
            assert!(matches!(swap(&bash("git status"), Verdict::Allow, Path::new(RTK), &rewrite(code, "rtk git status")), Verdict::Allow), "exit {code}");
        }
        let timed_out = Rewriter { exit: None, status: RunOutcome::TimedOut { after: REWRITE_LIMIT }, text: "rtk git status" };
        assert!(matches!(swap(&bash("git status"), Verdict::Allow, Path::new(RTK), &timed_out), Verdict::Allow));
        assert!(matches!(swap(&bash("git status"), Verdict::Allow, Path::new(RTK), &rewrite(0, "")), Verdict::Allow));
    }

    /// Um `rtk` solto que a leitura dos comandos simples não alcança deixa o
    /// comando original, em vez de um `rtk` achado pelo `PATH`.
    #[test]
    fn a_loose_rtk_out_of_reach_keeps_the_original_command() {
        let verdict = swap(&bash("echo $(git log -1)"), Verdict::Allow, Path::new(RTK), &rewrite(3, "echo $(rtk git log -1)"));
        assert!(matches!(verdict, Verdict::Allow));
    }

    /// A nota de quem trocou antes segue junto com a troca do rtk.
    #[test]
    fn an_earlier_rewrite_keeps_its_note() {
        let earlier = Verdict::Rewrite { tool_input: json!({"command":"cargo test","timeout":600_000}), note: Some("antes".into()) };
        let verdict = swap(&bash("cargo test &"), earlier, Path::new(RTK), &rewrite(3, "rtk cargo test"));
        assert!(matches!(&verdict, Verdict::Rewrite { note: Some(note), .. } if note == "antes"));
        assert_eq!(command_of(&verdict), format!("{PINNED} cargo test"));
    }
}
