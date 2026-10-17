//! O que a linha de resumo de um executor de teste ou de compilação diz sobre
//! como ele acabou. O host pode não mandar o código de saída, e um pipe só
//! entrega o código do último programa da linha: o resumo do próprio executor
//! é então o único rastro do resultado. O filtro que tira essa linha da saída
//! deixa a execução sem veredito.
use crate::hooks::bash::lex::{self, Segment};
use serde_json::Value;

/// Como o executor de um comando acabou, pelo resumo dele na saída.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Summary {
    /// O resumo diz que passou, e nenhuma linha diz que falhou.
    Passed,
    /// Uma linha da saída diz que o executor falhou.
    Failed,
    /// O executor mandou a saída a um pipe, e o resumo dele não chegou.
    Hidden,
}

impl Summary {
    /// O `result_status` do recibo que o resumo decide.
    pub(crate) fn status(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Hidden => "summary-hidden",
        }
    }
}

/// Um executor e as linhas, já aparadas, com que o resumo dele começa.
struct Runner {
    program: &'static str,
    /// O primeiro argumento que não é opção, para o programa que roda muitas
    /// coisas; vazio quando o programa é o próprio executor.
    subcommands: &'static [&'static str],
    passed: &'static [&'static str],
    failed: &'static [&'static str],
}

/// Os executores cujo resumo se lê. O `error:` do cargo cobre a compilação que
/// caiu e o `error: test failed` que ele escreve no erro padrão, fora do pipe.
const RUNNERS: &[Runner] = &[
    Runner { program: "cargo", subcommands: &["test"], passed: &["test result: ok."], failed: &["test result: FAILED.", "error:", "error["] },
    Runner { program: "cargo", subcommands: &["build", "check", "clippy"], passed: &["Finished "], failed: &["error:", "error["] },
    Runner {
        program: "dotnet",
        subcommands: &["test"],
        passed: &["Passed!", "Test Run Successful."],
        failed: &["Failed!", "Test Run Failed.", "Build FAILED."],
    },
    Runner { program: "go", subcommands: &["test"], passed: &["ok "], failed: &["FAIL", "--- FAIL"] },
];

/// O executor de [`RUNNERS`] que `segment` roda.
fn runner_of(segment: &Segment) -> Option<&'static Runner> {
    let subcommand = segment.args.iter().map(|arg| arg.text.as_str()).find(|arg| !arg.starts_with(['-', '+']));
    RUNNERS.iter().find(|runner| {
        runner.program == segment.name()
            && (runner.subcommands.is_empty() || subcommand.is_some_and(|word| runner.subcommands.contains(&word)))
    })
}

/// Os executores que `command` roda, cada um com o seu comando: a falta do
/// resumo de um executor encadeado num pipe se resolve rodando-o sozinho.
fn runners(command: &str) -> Vec<(&'static Runner, Segment)> {
    lex::segments(command).into_iter().filter_map(|segment| runner_of(&segment).map(|runner| (runner, segment))).collect()
}

/// O resumo dos executores que `command` roda, lido na saída de `response`.
/// Uma falha vence; o executor encadeado num pipe sem o resumo dele na saída
/// deixa a execução escondida; sem pipe e sem resumo, nada se diz. `None`
/// também quando o comando não roda executor nenhum de [`RUNNERS`].
pub(crate) fn read(command: &str, response: &Value) -> Option<Summary> {
    let runners = runners(command);
    if runners.is_empty() {
        return None;
    }
    let lines: Vec<&str> = ["stdout", "stderr", "output"]
        .iter()
        .filter_map(|key| response.get(key).and_then(Value::as_str))
        .flat_map(str::lines)
        .map(str::trim)
        .collect();
    let says = |markers: &[&str]| lines.iter().any(|line| markers.iter().any(|marker| line.starts_with(marker)));
    if runners.iter().any(|(runner, _)| says(runner.failed)) {
        return Some(Summary::Failed);
    }
    if runners.iter().any(|(runner, segment)| segment.piped && !says(runner.passed)) {
        return Some(Summary::Hidden);
    }
    runners.iter().any(|(runner, _)| says(runner.passed)).then_some(Summary::Passed)
}

/// Os executores encadeados num pipe que `command` roda, escritos sem o pipe:
/// o que se roda para o resumo chegar inteiro à saída.
pub(crate) fn without_pipe(command: &str) -> String {
    runners(command)
        .into_iter()
        .filter(|(_, segment)| segment.piped)
        .map(|(_, segment)| {
            std::iter::once(segment.program.raw.as_str())
                .chain(segment.args.iter().map(|arg| arg.raw.as_str()))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// O resumo do executor decide, em Rust e em C#: a falha vence, o pipe que
    /// tira o resumo esconde a execução, e o comando sem pipe nem resumo, ou
    /// que não roda executor, fica sem veredito.
    #[test]
    fn the_runner_summary_decides_passed_failed_or_hidden() {
        let out = |stdout: &str, stderr: &str| json!({"stdout": stdout, "stderr": stderr, "interrupted": false});
        let piped = "cargo test -j 1 -p mustard-rt --lib -- tool_observations 2>&1 | grep -E \"test result|FAILED\"";
        assert_eq!(read(piped, &out("test result: ok. 30 passed; 0 failed; 0 ignored", "")), Some(Summary::Passed));
        assert_eq!(read(piped, &out("test result: FAILED. 29 passed; 1 failed", "")), Some(Summary::Failed));
        let grep_ok = "cargo test -p x | grep ok";
        assert_eq!(read(grep_ok, &out("test a ... ok\ntest b ... ok\n", "")), Some(Summary::Hidden));
        assert_eq!(read(grep_ok, &out("test a ... ok\n", "error: test failed, to rerun pass `-p x --lib`")), Some(Summary::Failed));
        assert_eq!(read("cargo build -q", &out("", "")), None, "no pipe, no summary: nothing to say");
        assert_eq!(read("git diff | grep ok", &out("ok", "")), None, "no runner");
        let dotnet = "dotnet test App.Tests/App.Tests.csproj | tail -5";
        assert_eq!(read(dotnet, &out("Passed!  - Failed:     0, Passed:     3, Skipped:     0, Total:     3", "")), Some(Summary::Passed));
        assert_eq!(read(dotnet, &out("Failed!  - Failed:     1, Passed:     2, Skipped:     0, Total:     3", "")), Some(Summary::Failed));
        assert_eq!(read("dotnet test App.Tests | grep Passed", &out("", "")), Some(Summary::Hidden));
    }
}
