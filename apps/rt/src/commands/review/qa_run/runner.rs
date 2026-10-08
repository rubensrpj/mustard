//! O executor das provas: roda o comando de um critério, ou o lint e a suíte
//! do servidor, com o teto de tempo de cada um, e devolve o veredito, a
//! contagem de testes que a saída diz e o trecho da saída que explica uma
//! falha.

use super::AcResult;
use crate::shared::proc::{ShellOutcome, run_shell_with_deadline};
use std::path::Path;
use std::time::{Duration, Instant};

/// O prazo comum de uma prova (2 minutos) para o comando que não compila.
const AC_TIMEOUT_SECS: u64 = 120;

/// O código de saída de "comando não encontrado" do shell POSIX.
///
/// Mora numa constante compartilhada, e não num número escrito por quem lê o
/// registro: o executor o nomeia na razão da falha e tenta uma vez de novo
/// quando o programa existe no `PATH`, mas o veredito continua `fail`.
pub(crate) const EXIT_COMMAND_NOT_FOUND: i64 = 127;

/// Per-AC timeout ceiling (10 min) for commands invoking `cargo `: a
/// `cargo build`/`cargo test` AC that runs right after an edit must recompile,
/// and a cold compile routinely exceeds the 120 s default (real case:
/// `cargo test -p mustard-rt` hit 120 s mid-recompile and degraded to a
/// silent `skip`). Mirrors `TIMEOUT_RUST_SECS` in the pipeline verifier.
const AC_TIMEOUT_CARGO_SECS: u64 = 600;

/// Teto (uma hora) dos dois comandos que o servidor roda e que a rodada e o
/// fechamento repetem — o `lintCommand` e o `testCommand` do `mustard.json`. Não é o teto
/// de uma prova de critério: uma suíte inteira leva o tempo que o projeto
/// precisa, e o `pnpm test` de um projeto Node passa com folga dos 2 minutos
/// de um critério. Ele só existe para a máquina não ficar presa num processo
/// travado, e a variável `MUSTARD_QA_AC_TIMEOUT_SECS` não o alcança.
const SERVER_COMMAND_TIMEOUT_SECS: u64 = 3600;

/// Que teto de tempo vale para um comando: o de uma prova de critério ou o
/// de um comando do servidor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ceiling {
    /// A prova de um critério: 2 minutos, 10 para o que compila, ou o valor
    /// da variável `MUSTARD_QA_AC_TIMEOUT_SECS`.
    Criterion,
    /// Declared build, irrespective of language/tool.
    Build,
    /// O lint ou a suíte que o servidor roda: [`SERVER_COMMAND_TIMEOUT_SECS`],
    /// sempre.
    ServerCommand,
}

/// Onde cada executor de teste diz quantos testes rodaram. A linha, em
/// minúsculas, tem de trazer `needs`; o número vem depois de `before` (vazio,
/// no começo da linha) e a palavra seguinte a ele começa com `after` (vazia,
/// qualquer palavra serve). O primeiro executor que casa na linha responde por
/// ela.
const TEST_COUNTS: &[(&str, &str, &str)] = &[
    // cargo: `running 3 tests`, uma linha por alvo, somadas.
    ("running ", "running ", "test"),
    // jest: `Tests:       2 failed, 3 passed, 5 total`.
    ("tests:", "tests:", "total"),
    // vitest: `Tests  3 passed (3)`.
    ("tests ", "tests ", "passed"),
    // pytest: `collected 3 items`.
    ("collected ", "collected ", "item"),
    // unittest: `Ran 3 tests in 0.001s`.
    ("ran ", "ran ", "test"),
    // dotnet: `Failed: 0, Passed: 3, Skipped: 0, Total: 3`.
    ("passed:", "total: ", ""),
    // mocha: `3 passing (12ms)`.
    ("passing", "", "passing"),
];

/// Dois executores da tabela dizem zero sem escrever número, e cada um tem a
/// linha dele. A frase solta continua de fora, e o motivo é medido: `no tests
/// to skip` na saída verde de um lint e `no tests found here` numa prova que
/// não é teste diziam zero e recusavam um verde legítimo. Quem não escreve
/// contagem nem a linha de resumo do próprio executor não respondeu à
/// pergunta.
///
/// A linha de resumo do vitest quando o filtro por nome não casou teste
/// nenhum: `Tests  no tests`. É contagem de executor — o rótulo dele abre a
/// linha —, e não prosa. Sem ela o vitest escapava por inteiro: ele sai com
/// código 0 nesse caso, e a linha não traz número para a tabela ler.
///
/// A linha já chega aqui aparada e em minúsculas.
fn vitest_said_no_test(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("tests") else {
        return false;
    };
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    rest.starts_with(char::is_whitespace) && rest.trim_start().starts_with("no tests")
}

/// A marca do go no pacote cujo filtro por nome não casou teste nenhum:
/// `ok  x/pkg  0.002s [no tests to run]`, com os colchetes que são dele.
const GO_NO_TESTS_TO_RUN: &str = "[no tests to run]";

/// O que a linha de resultado de um pacote do go diz sobre ter rodado teste:
/// `Some(true)` quando aquele pacote rodou, `Some(false)` quando ele mesmo diz
/// que não rodou nenhum, `None` quando a linha não é resultado de pacote.
///
/// A leitura é por pacote porque a saída é por pacote, e o go não escreve
/// contagem em linha nenhuma: num `go test -run X ./...` a marca sai ao lado
/// de pacotes que rodaram teste de verdade, e tomá-la por resposta da corrida
/// inteira recusava uma prova legítima. Só quando nenhum pacote rodou é que a
/// corrida rodou zero.
fn go_package_ran_tests(line: &str) -> Option<bool> {
    let rest = line.strip_prefix("ok")?;
    rest.starts_with(char::is_whitespace).then(|| !(rest.contains(GO_NO_TESTS_TO_RUN) || rest.contains("[no test files]")))
}

/// O número que uma linha de saída, já em minúsculas, diz ter rodado, pelo
/// executor que casar com ela.
fn counted_in_line(line: &str) -> Option<u64> {
    for (needs, before, after) in TEST_COUNTS {
        if !line.contains(needs) {
            continue;
        }
        let rest = match line.find(before) {
            _ if before.is_empty() => line,
            Some(at) => &line[at + before.len()..],
            None => continue,
        };
        let words: Vec<&str> = rest.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
        for (i, word) in words.iter().enumerate() {
            let Ok(count) = word.parse::<u64>() else {
                continue;
            };
            let next = words.get(i + 1).copied().unwrap_or_default();
            if after.is_empty() || next.starts_with(after) {
                return Some(count);
            }
        }
    }
    None
}

/// Quantos testes a saída de um comando diz ter rodado, quando um executor
/// que o projeto usa se reconhece nela: o cargo escreve uma linha por alvo e
/// as contas se somam, os outros dizem o total numa linha de resumo, e os dois
/// que dizem zero sem número — o vitest na linha de resumo dele e o go na
/// marca do pacote — contam zero. `None` quando nenhuma linha responde à
/// pergunta: verde sem contagem não é verde sem teste.
///
/// Isto é leitura, e não veredito: quem decide o que fazer com o número é
/// quem pediu o comando — só a prova de um critério recusa o zero.
fn tests_run(output: &str) -> Option<u64> {
    let mut counts: Vec<u64> = Vec::new();
    let mut said_none = false;
    let (mut go_said_none, mut go_ran) = (false, false);
    for line in output.split(['\n', '\r']) {
        let line = line.trim().to_ascii_lowercase();
        if vitest_said_no_test(&line) {
            said_none = true;
        }
        match go_package_ran_tests(&line) {
            Some(true) => go_ran = true,
            Some(false) => go_said_none |= line.contains(GO_NO_TESTS_TO_RUN),
            None => {}
        }
        counts.extend(counted_in_line(&line));
    }
    let total: u64 = counts.iter().sum();
    if total > 0 {
        return Some(total);
    }
    (!counts.is_empty() || said_none || (go_said_none && !go_ran)).then_some(0)
}

/// Timeout for `command` under `ceiling`, env-aware: the environment is read
/// here, and the choice itself is [`ceiling_secs`], a pure function.
///
/// `MUSTARD_QA_AC_TIMEOUT_SECS` (whole seconds, `u64`) overrides BOTH defaults
/// when set to a parseable value; an invalid value is ignored. Without the
/// override, commands containing `cargo ` get [`AC_TIMEOUT_CARGO_SECS`]
/// (compilation-bound) and everything else keeps [`AC_TIMEOUT_SECS`].
fn ac_timeout_secs(command: &str, ceiling: Ceiling, cwd: &Path) -> u64 {
    let env = timeout_variable();
    // The project's OWN build and type-check commands, so the compile-bound
    // ceiling is not a list of tool names this file happens to know. Both are
    // already declared in `mustard.json`; fail-open to none.
    let declared = mustard_core::ProjectConfig::load(cwd).commands();
    let compiling: Vec<String> = [declared.build, declared.type_check].into_iter().flatten().collect();
    ceiling_secs(ceiling, command, env.as_deref(), &compiling)
}

/// O valor da variável `MUSTARD_QA_AC_TIMEOUT_SECS`. Nos testes, o valor
/// injetado por [`with_timeout_variable`] vence o do ambiente, para que o
/// caminho inteiro do fechamento seja testado com a variável ligada sem
/// mexer no ambiente do processo.
fn timeout_variable() -> Option<String> {
    #[cfg(test)]
    if let Some(value) = TIMEOUT_VARIABLE_FOR_TEST.with(|cell| cell.borrow().clone()) {
        return Some(value);
    }
    std::env::var("MUSTARD_QA_AC_TIMEOUT_SECS").ok()
}

#[cfg(test)]
thread_local! {
    /// O valor da variável de teto que um teste injeta nesta thread.
    static TIMEOUT_VARIABLE_FOR_TEST: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Roda `f` como se a variável `MUSTARD_QA_AC_TIMEOUT_SECS` valesse `value`
/// nesta thread, e a desliga depois.
#[cfg(test)]
pub(crate) fn with_timeout_variable<T>(value: &str, f: impl FnOnce() -> T) -> T {
    TIMEOUT_VARIABLE_FOR_TEST.with(|cell| *cell.borrow_mut() = Some(value.to_string()));
    let out = f();
    TIMEOUT_VARIABLE_FOR_TEST.with(|cell| *cell.borrow_mut() = None);
    out
}

/// A escolha do teto, função pura das entradas: sem relógio e sem ambiente
/// lidos aqui dentro. O comando do servidor tem sempre o teto dele, com ou
/// sem a variável; a prova de critério segue o teto de sempre.
pub(crate) fn ceiling_secs(ceiling: Ceiling, command: &str, env_override: Option<&str>, compiling: &[String]) -> u64 {
    match ceiling {
        Ceiling::ServerCommand => SERVER_COMMAND_TIMEOUT_SECS,
        Ceiling::Build => env_override.and_then(|s| s.trim().parse().ok()).unwrap_or(AC_TIMEOUT_CARGO_SECS),
        Ceiling::Criterion => ac_timeout_secs_with_override(command, env_override, compiling),
    }
}

/// Deterministic core of [`ac_timeout_secs`]: the env value is injected as a
/// parameter so the decision is a pure function of its inputs (no wall-clock,
/// no globals) and unit-testable without mutating process env (which would
/// need `unsafe` under Rust 2024 — forbidden in this crate).
fn ac_timeout_secs_with_override(command: &str, env_override: Option<&str>, compiling: &[String]) -> u64 {
    if let Some(secs) = env_override.and_then(|s| s.trim().parse::<u64>().ok()) {
        return secs;
    }
    if is_compile_bound(command, compiling) { AC_TIMEOUT_CARGO_SECS } else { AC_TIMEOUT_SECS }
}

/// `true` when `command` has to COMPILE before it can answer.
///
/// **Why this is not just `cargo`.** A compile-bound criterion's runtime is
/// bimodal: seconds on a warm cache, minutes on a cold one. It is the same
/// criterion either way. Measured in the field: `pnpm type-check` answered in
/// 1 s on one pass and was killed at 148 s on the next, purely because the build
/// cache had gone cold — and closing refused the spec for it. The spec
/// was not wrong; the clock was.
///
/// Three signals, in order of how much they are worth:
///
/// 1. `cargo ` — this workspace's own case, unchanged.
/// 2. The project's DECLARED commands (`mustard.json#buildCommand` and
///    `#typeCheckCommand`, passed in as `compiling`): the one place a project
///    already says how it compiles, so nothing here has to guess.
/// 3. A small set of type-check drivers, for a project that declared neither —
///    a type check compiles without being called "build".
fn is_compile_bound(command: &str, compiling: &[String]) -> bool {
    let lower = command.to_ascii_lowercase();
    if lower.contains("cargo ") {
        return true;
    }
    if compiling.iter().map(|c| c.trim().to_ascii_lowercase()).filter(|c| !c.is_empty()).any(|c| lower.contains(&c)) {
        return true;
    }
    ["type-check", "typecheck", "tsc "].iter().any(|t| lower.contains(t))
}

/// The verdict of evaluating an AC's optional `Expect:` evidence regex against
/// a passing command's captured output. Pure and panic-free (SRP: no process,
/// no I/O) so the matcher is unit-testable in isolation.
enum ExpectVerdict {
    /// No `Expect:` declared ⇒ the caller keeps the legacy exit-code verdict.
    NoExpectation,
    /// The pattern compiled and matched the output.
    Matched,
    /// The pattern compiled but did NOT match the output.
    Missed,
    /// The pattern is not a valid regex ⇒ fail-open to `skip`, never a panic.
    InvalidPattern,
}

/// Evaluate an optional `Expect:` regex against a command's combined output.
/// Total + pure: an absent expectation is [`ExpectVerdict::NoExpectation`], an
/// uncompilable pattern is [`ExpectVerdict::InvalidPattern`] (never a panic),
/// otherwise match/miss. A regex é compilada aqui, uma vez por critério: uma
/// prova roda poucos critérios, e não há laço quente que peça cache.
fn evaluate_expect(expect: Option<&str>, output: &str) -> ExpectVerdict {
    let Some(pattern) = expect else {
        return ExpectVerdict::NoExpectation;
    };
    match regex::Regex::new(pattern) {
        Ok(re) if re.is_match(output) => ExpectVerdict::Matched,
        Ok(_) => ExpectVerdict::Missed,
        Err(_) => ExpectVerdict::InvalidPattern,
    }
}

/// Os primeiros 100 caracteres de `s`: o trecho que o verde de zero teste leva
/// em `stderr_excerpt`, porque ali o começo do que o executor escreveu é a
/// única evidência.
fn excerpt(s: &str) -> String {
    s.chars().take(100).collect()
}

/// Quantas linhas do fim da saída uma falha guarda.
const FAILURE_TAIL_LINES: usize = 40;

/// Quantos caracteres do fim da saída uma falha guarda.
const FAILURE_TAIL_CHARS: usize = 2000;

/// O fim de `s`: as últimas [`FAILURE_TAIL_LINES`] linhas, cortadas ainda aos
/// últimos [`FAILURE_TAIL_CHARS`] caracteres, o que restringir mais.
///
/// É o trecho que uma falha leva em `stderr_excerpt`: quem roda a prova de um
/// critério, o lint ou a suíte escreve o motivo da falha no fim — o teste que
/// quebrou, o erro que interrompeu a compilação —, e o começo da saída é só o
/// andamento do que ainda passava.
fn tail_excerpt(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let kept = lines[lines.len().saturating_sub(FAILURE_TAIL_LINES)..].join("\n");
    let skipped = kept.chars().count().saturating_sub(FAILURE_TAIL_CHARS);
    kept.chars().skip(skipped).collect()
}

/// Roda o comando de um critério, como o fechamento e a rodada pedem a prova,
/// sob o prazo que [`ac_timeout_secs`] escolhe para ele.
///
/// Classificação: `pass` (saída zero), `fail` (saída diferente de zero),
/// `timeout` (morto pelo prazo) e `skip` (o critério nem pôde ser tentado).
///
/// `expect` é a evidência opcional, uma regex. Com ela e saída zero, a regex
/// tem de casar com a saída junta de stdout e stderr, senão o comando verde
/// cai para `fail` (não escreveu a evidência esperada); uma regex que não
/// compila vira `skip` (falha aberta). Sem ela, o veredito é só a saída.
pub(super) fn run_ac_command(command: &str, expect: Option<&str>, cwd: &Path) -> AcResult {
    run_with_ceiling(command, expect, cwd, Ceiling::Criterion)
}

/// Roda um dos dois comandos do servidor com o teto deles
/// ([`SERVER_COMMAND_TIMEOUT_SECS`]), pelo mesmo executor e com a mesma
/// classificação da prova de um critério.
pub(super) fn run_server_command(command: &str, cwd: &Path) -> AcResult {
    run_with_ceiling(command, None, cwd, Ceiling::ServerCommand)
}

pub(super) fn run_build_command(command: &str, cwd: &Path) -> AcResult {
    run_with_ceiling(command, None, cwd, Ceiling::Build)
}

/// O executor com o teto que `ceiling` escolhe.
fn run_with_ceiling(command: &str, expect: Option<&str>, cwd: &Path, ceiling: Ceiling) -> AcResult {
    let timeout = Duration::from_secs(ac_timeout_secs(command, ceiling, cwd));
    run_ac_command_with_timeout(command, expect, cwd, timeout)
}

/// O núcleo determinístico de [`run_ac_command`]: o prazo chega por
/// parâmetro, e não lido do ambiente aqui dentro, para que o ramo do prazo
/// estourado seja testável sem mexer no ambiente do processo (mexer nele pede
/// `unsafe` no Rust 2024, proibido neste crate). É a mesma costura de prazo
/// injetado que [`ac_timeout_secs_with_override`] já usa.
fn run_ac_command_with_timeout(command: &str, expect: Option<&str>, cwd: &Path, timeout: Duration) -> AcResult {
    run_ac_command_inner(command, expect, cwd, timeout, false)
}

/// Is the first word of `command` a program the shell can find?
///
/// Answers the ONE question that separates "this criterion names a program that
/// does not exist" from "the environment could not run it just now": the former
/// is a real defect and must stay `fail`, the latter is worth one retry.
///
/// `false` whenever the question cannot be answered — an empty command, a shell
/// builtin, an unreadable `PATH`. That direction keeps the historical behaviour
/// (no retry, verdict unchanged), which is the safe one.
fn first_program_is_on_path(command: &str) -> bool {
    let Some(word) = command.split_whitespace().next() else {
        return false;
    };
    // An absolute or relative path is checked directly; a bare name is looked
    // up the way the shell would. `\` counts as a separator too — on Windows
    // that is THE separator, so testing only `/` sent every absolute path there
    // through the PATH scan below, where it never matches.
    if word.contains('/') || word.contains('\\') {
        return Path::new(word).is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        // The bare name FIRST — that is the whole answer on Unix.
        if dir.join(word).is_file() {
            return true;
        }
        // On Windows the file on disk carries an extension the command line
        // does not: `cargo` is `cargo.exe`. Without this the function answered
        // "not on PATH" for EVERY program there, so the retry this module
        // exists for could never fire on Windows — a product defect, found by
        // CI when a test finally asked the question on that platform.
        cfg!(windows) && executable_extensions().iter().any(|ext| dir.join(format!("{word}{ext}")).is_file())
    })
}

/// The extensions Windows appends to a bare command name, from `PATHEXT`, each
/// lowercased and dot-prefixed. Empty off Windows, where the name IS the file.
///
/// Falls back to the documented default set when `PATHEXT` is unset — an
/// absent variable must not mean "no program is executable".
fn executable_extensions() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let raw = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    raw.split(';')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| {
            let e = e.to_ascii_lowercase();
            if e.starts_with('.') { e } else { format!(".{e}") }
        })
        .collect()
}

/// [`run_ac_command_with_timeout`], with the retry flag the public face hides.
///
/// `is_retry` exists to bound the recursion at exactly one extra attempt: a
/// flaky environment gets a second chance, a genuinely missing program does not
/// spin.
fn run_ac_command_inner(command: &str, expect: Option<&str>, cwd: &Path, timeout: Duration, is_retry: bool) -> AcResult {
    let t0 = Instant::now();
    // O comando roda como veio, num shell POSIX: no Windows o executor
    // compartilhado acha o que vem ao lado do `git`, e as aspas simples de
    // `rg 'token' caminho` chegam ao programa como aspas, e não como
    // caracteres soltos de um `cmd.exe`. A criação do processo, o esvaziamento
    // dos canais e a espera pelo prazo moram num lugar só
    // ([`crate::shared::proc::run_shell_with_deadline`]). O esvaziamento é
    // necessário: um comando que escreve mais que o buffer de ~64 KB do canal
    // travava na escrita e gastava o prazo inteiro, já tendo terminado o
    // trabalho.
    let (status, stdout, stderr) = match run_shell_with_deadline(command, cwd, timeout) {
        ShellOutcome::Exited { status, stdout, stderr } => (status, stdout, stderr),
        // Killed by its deadline: a class of its own, NEVER `skip`. The
        // criterion WAS attempted and simply never finished, so it verified
        // nothing — `skip` (warn-and-allow) would let the run read green.
        ShellOutcome::TimedOut { after } => {
            return AcResult {
                status: "timeout".to_string(),
                exit: None,
                duration_ms: t0.elapsed().as_millis(),
                stderr_excerpt: format!("timeout after {}ms", after.as_millis()),
                tests_run: None,
            };
        }
        // Never ran ⇒ the criterion could not be attempted at all ⇒ `skip`.
        // Never attempted, or attempted and no status can ever arrive. Carry the
        // OS error instead of asserting "command not found": that reason is a
        // guess, and a criterion reported `skip` for the wrong cause is exactly
        // the misleading verdict this spec exists to remove.
        ShellOutcome::SpawnFailed { error } => {
            return AcResult {
                status: "skip".to_string(),
                exit: None,
                duration_ms: t0.elapsed().as_millis(),
                stderr_excerpt: format!("could not run the command: {error}"),
                tests_run: None,
            };
        }
    };

    let duration_ms = t0.elapsed().as_millis();
    // Full combined output (stderr first, then stdout): the haystack the
    // optional `Expect:` regex matches against AND the source of the bounded
    // excerpt shown on failure.
    let combined_full = [stderr.trim(), stdout.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    // O que o executor disse sobre quantos testes rodaram viaja no resultado,
    // como número. O julgamento não mora aqui: este executor responde ao lint
    // do projeto e à prova de um critério, e só a segunda promete rodar teste
    // (ver `super::run_proof`).
    let counted = tests_run(&combined_full);
    if status.success() {
        // Optional `Expect:` evidence gate. Absent ⇒ the legacy exit-0 pass
        // (byte-for-byte). Present ⇒ the regex must match the command's own
        // output, else the "green" command proved nothing (fail); an
        // uncompilable pattern degrades to skip, never a panic (fail-open).
        let pattern = expect.unwrap_or_default();
        return match evaluate_expect(expect, &combined_full) {
            ExpectVerdict::NoExpectation | ExpectVerdict::Matched => AcResult {
                status: "pass".to_string(),
                exit: Some(0),
                duration_ms,
                // O verde que diz ter rodado zero teste leva o começo do que
                // escreveu: é a única evidência do que aconteceu, e quem
                // recusa por zero teste grava esse trecho em vez de uma frase
                // montada. O verde comum segue com o excerto vazio, como
                // sempre.
                stderr_excerpt: match counted {
                    Some(0) => excerpt(&combined_full),
                    _ => String::new(),
                },
                tests_run: counted,
            },
            ExpectVerdict::Missed => AcResult {
                status: "fail".to_string(),
                exit: Some(0),
                duration_ms,
                stderr_excerpt: format!("Expect `{pattern}` not found in command output: {}", tail_excerpt(&combined_full)),
                tests_run: counted,
            },
            ExpectVerdict::InvalidPattern => AcResult {
                status: "skip".to_string(),
                exit: Some(0),
                duration_ms,
                stderr_excerpt: format!("Expect `{pattern}` is not a valid regex; skipped (fail-open)"),
                tests_run: counted,
            },
        };
    }
    // 127 é o "comando não encontrado" do próprio shell POSIX. Ele é NOMEADO
    // aqui e continua `fail`, e as duas metades são de propósito.
    //
    // Nomeado, porque a saída crua nem sempre se lê como causa.
    //
    // Continua `fail`, porque um critério que ninguém conseguiu rodar nunca
    // pode deixar o fechamento verde: graduá-lo `skip` já deixou uma spec
    // fechar sobre um critério que ninguém rodou. Quem precisa separar o 127
    // (a regra do vermelho é saída diferente de zero, e um comando que não
    // roda não é prova vermelha) lê o `exit` deste registro e decide por
    // conta própria — a distinção mora no único chamador que a quer, e nunca
    // no veredito compartilhado, que cada chamador lê de um jeito.
    //
    // **Um 127 cujo programa EXISTE quer dizer "não deu para rodar agora", e
    // não "não existe" — e isso vale uma nova tentativa, nunca um veredito
    // mais brando.** Medido quatro vezes numa sessão: um segundo `cargo`
    // compilando em paralelo fazia o primeiro perder a trava da compilação, o
    // shell respondia 127 e os catorze critérios falhavam de uma vez, todos
    // passando de novo menos de um minuto depois. O fechamento da spec então
    // apontava um critério sadio e pedia conserto do que não estava quebrado.
    //
    // Se a nova tentativa também falha, o veredito segue `fail`. Uma tentativa
    // separa um ambiente instável de um defeito real sem enfraquecer o que o
    // registro afirma.
    if status.code().map(i64::from) == Some(EXIT_COMMAND_NOT_FOUND) && !is_retry && first_program_is_on_path(command) {
        return run_ac_command_inner(command, expect, cwd, timeout, true);
    }
    if status.code().map(i64::from) == Some(EXIT_COMMAND_NOT_FOUND) {
        return AcResult {
            status: "fail".to_string(),
            exit: Some(EXIT_COMMAND_NOT_FOUND),
            duration_ms,
            stderr_excerpt: format!("the shell could not find the command (exit {EXIT_COMMAND_NOT_FOUND}): {}", tail_excerpt(&combined_full)),
            tests_run: counted,
        };
    }
    AcResult {
        status: "fail".to_string(),
        exit: Some(status.code().map_or(1, i64::from)),
        duration_ms,
        stderr_excerpt: tail_excerpt(&combined_full),
        tests_run: counted,
    }
}

#[cfg(test)]
mod tests {

    /// A program that genuinely does not exist still FAILS — no retry softens
    /// it, and no retry spins on it.
    ///
    /// The retry exists only for the other 127: a program the shell CAN find
    /// that could not run just now (a second `cargo` holding the build lock).
    /// Measured four times in one session — fourteen criteria failing together
    /// with 127, all passing again under a minute later.
    #[test]
    fn a_missing_program_still_fails_and_one_that_exists_is_retried() {
        let dir = tempfile::tempdir().unwrap();

        // Not on PATH: no retry, verdict unchanged. Grading this `skip` is the
        // regression documented above — it once let a spec CLOSE on a criterion
        // nobody had run.
        assert!(!first_program_is_on_path("programa-que-nao-existe-xyz --flag"));
        let res = run_ac_command_with_timeout("programa-que-nao-existe-xyz", None, dir.path(), Duration::from_secs(10));
        assert_eq!(res.status, "fail", "a missing program must stay a failure");
        assert_eq!(res.exit, Some(EXIT_COMMAND_NOT_FOUND));

        // On PATH: the retry is eligible. Twice this assertion named a program
        // that does not exist on every runner — first `sh` (absent on Windows),
        // then `cargo` — and the second failure was not the test's fault: the
        // lookup itself never matched anything on Windows, because the file on
        // disk is `cargo.exe` and the name asked for is `cargo`. So this line
        // now guards a real product behaviour, not a convenience.
        assert!(
            first_program_is_on_path("cargo --version"),
            "cargo invoked these tests, so it IS on PATH — a false answer here means \
             the lookup cannot see programs on this platform",
        );
        // An absolute path is answered by the filesystem, without consulting
        // PATH at all — so the executable named here must exist on every
        // platform. `current_exe` is this very test binary, and on Windows its
        // path uses `\`, which the lookup must accept as a separator too.
        if let Ok(me) = std::env::current_exe() {
            assert!(first_program_is_on_path(&me.to_string_lossy()), "an absolute path must be answered by the filesystem: {}", me.display());
        }
        assert!(!first_program_is_on_path("/nao/existe/em/lugar/nenhum"));
        assert!(!first_program_is_on_path("programa-que-nao-existe-xyz"));
        assert!(!first_program_is_on_path(""));
    }

    use super::*;
    use tempfile::tempdir;

    /// Quantos testes a saída diz ter rodado, em cada executor que o projeto
    /// usa: o cargo soma os alvos e basta um alvo com teste para o verde
    /// valer; o jest, o vitest, o pytest, o unittest, o dotnet e o mocha dizem
    /// o total numa linha de resumo.
    ///
    /// Os dois que dizem zero sem escrever número têm cada um a linha dele. O
    /// vitest, que sai verde quando o filtro por nome não casa nada, diz `Tests
    /// no tests` na linha de resumo — é contagem, e sem ela o executor inteiro
    /// escapava. O go diz a marca dele entre colchetes, mas por pacote: num
    /// `go test -run X ./...` ela sai ao lado de pacotes que rodaram teste de
    /// verdade, e aí a corrida não rodou zero — só quando nenhum pacote rodou.
    ///
    /// A saída que não responde à pergunta não vira contagem nenhuma: a frase
    /// solta que cita "no tests" num lint verde ou numa prova que não é teste
    /// não é contagem, e quem a lia recusava um verde legítimo.
    ///
    /// O número lido viaja no resultado, e o executor não julga: o comando
    /// que sai verde sem rodar teste sai daqui como `pass`, com o zero que a
    /// saída disse e com o começo do que ele escreveu, e quem recusa é a prova
    /// de um critério.
    #[test]
    fn every_runner_the_project_uses_says_how_many_tests_it_ran() {
        let none = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed\n\n     Running tests/a.rs\n\nrunning 0 tests\n";
        assert_eq!(tests_run(none), Some(0));
        assert_eq!(tests_run("running 0 tests\n\nrunning 1 test\ntest tests::sum ... ok\n"), Some(1));
        assert_eq!(tests_run("running 12 tests\n"), Some(12));
        assert_eq!(tests_run("Tests:       2 failed, 3 passed, 5 total\n"), Some(5));
        assert_eq!(tests_run("Tests:       0 total\n"), Some(0));
        assert_eq!(tests_run(" Tests  3 passed (3)\n"), Some(3));
        assert_eq!(tests_run("collected 3 items\n"), Some(3));
        assert_eq!(tests_run("collected 0 items\n\nno tests ran in 0.01s\n"), Some(0));
        assert_eq!(tests_run("Ran 3 tests in 0.001s\n\nOK\n"), Some(3));
        assert_eq!(tests_run("Failed: 0, Passed: 3, Skipped: 0, Total: 3\n"), Some(3));
        assert_eq!(tests_run("Failed: 0, Passed: 0, Skipped: 0, Total: 0\n"), Some(0));
        assert_eq!(tests_run("  3 passing (12ms)\n"), Some(3));
        assert_eq!(tests_run("testing: warning: no tests to run\nok\tx/pkg\t0.002s [no tests to run]\n"), Some(0));
        assert_eq!(tests_run("?   x/pkg\t[no test files]\nok  \tx/outro\t0.02s\n"), None, "go por pacote não conclui");
        assert_eq!(
            tests_run("ok  \tx/pkg\t0.002s [no tests to run]\nok  \tx/outro\t0.02s\n"),
            None,
            "um pacote sem teste ao lado de um que rodou não é corrida sem teste"
        );
        assert_eq!(tests_run("ok  \tx/pkg\t0.002s [no tests to run]\n?   \tx/vazio\t[no test files]\n"), Some(0), "nenhum pacote rodou");
        assert_eq!(tests_run("cargo test: 6 passed (1 suite)"), None, "sem contagem, sem veredito");
        assert_eq!(tests_run("lint ok: no tests to skip\n"), None, "frase num lint verde não é contagem");
        assert_eq!(tests_run("src/msg.rs: no tests found here\n"), None, "nem numa prova que não é teste");
        assert_eq!(tests_run(" Test Files  1 passed (1)\n      Tests  no tests\n"), Some(0), "a linha de resumo do vitest é contagem, mesmo sem número");
        assert_eq!(tests_run("testing: no tests here\n"), None, "prosa que começa parecido não é resumo");

        // O número lido chega no resultado, e o verde sem teste sai daqui
        // verde: o veredito é de quem pediu a prova. O verde que diz zero leva
        // o começo do que escreveu; o verde comum não leva nada.
        let dir = tempdir().unwrap();
        let three = run_ac_command("echo running 3 tests", None, dir.path());
        assert_eq!((three.status.as_str(), three.tests_run), ("pass", Some(3)), "{}", three.stderr_excerpt);
        assert!(three.stderr_excerpt.is_empty(), "o verde comum não leva excerto");
        let zero = run_ac_command("echo running 0 tests", None, dir.path());
        assert_eq!((zero.status.as_str(), zero.tests_run), ("pass", Some(0)), "{}", zero.stderr_excerpt);
        assert!(zero.stderr_excerpt.contains("running 0 tests"), "o zero leva a saída real: {}", zero.stderr_excerpt);
        let quiet = run_ac_command("echo lint ok: no tests to skip", None, dir.path());
        assert_eq!((quiet.status.as_str(), quiet.tests_run), ("pass", None), "{}", quiet.stderr_excerpt);
    }

    /// An AC-style command with quotes AND parentheses must survive intact to
    /// the shell. Under the old `cmd.arg("/C").arg(command)` path, `std`'s
    /// `CommandLineToArgvW`-style quoting corrupts the line (`node` sees a
    /// split string → "Unterminated string constant"); the `raw_arg`-based
    /// `build_shell_command` passes it verbatim, so this exits 0.
    #[cfg(windows)]
    #[test]
    fn ac_command_with_quotes_and_parens_runs_verbatim() {
        let dir = tempdir().unwrap();
        // node one-liner: a regex test inside parentheses, double-quoted -e arg.
        let cmd = r#"node -e "process.exit(/^(foo|bar)$/.test('bar') ? 0 : 1)""#;
        let res = run_ac_command(cmd, None, dir.path());
        assert_eq!(res.status, "pass", "quoted+parenthesized AC command must run verbatim (exit {:?}, stderr: {})", res.exit, res.stderr_excerpt);
        assert_eq!(res.exit, Some(0));
    }

    /// A `cmd.exe`-native command echoing a parenthesized, quoted string — the
    /// simplest case proving the outer quote pair is stripped and the inner
    /// `()` reach the program unmangled.
    #[cfg(windows)]
    #[test]
    fn ac_command_echoes_parenthesized_string() {
        let dir = tempdir().unwrap();
        let cmd = r#"node -e "console.log('(ok)')""#;
        let res = run_ac_command(cmd, None, dir.path());
        assert_eq!(res.status, "pass", "stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, Some(0));
    }

    /// THE regression this fix exists for, in both senses. A criterion written
    /// POSIX-style — the shape the flow's own prose teaches — must be able to go
    /// GREEN when its evidence is there and RED when it is not. Under `cmd.exe`
    /// the apostrophes reached `echo` as literal characters, so the output was
    /// `'a b'`, the `Expect:` never matched, and BOTH senses came back red: a
    /// criterion that could not pass in any tree state, stamped `proven: red` by
    /// the negative test and blamed on the implementer at QA.
    #[test]
    fn an_ac_written_with_single_quotes_can_go_both_ways() {
        let dir = tempdir().unwrap();
        let green = run_ac_command("echo 'a b'", Some("^a b$"), dir.path());
        assert_eq!(green.status, "pass", "single-quoted AC must be able to pass, stderr: {}", green.stderr_excerpt);
        let red = run_ac_command("echo 'a b'", Some("^zzz$"), dir.path());
        assert_eq!(red.status, "fail", "and must still fail on absent evidence, stderr: {}", red.stderr_excerpt);
    }

    /// Um comando que o shell não acha é graduado `fail`, e NOMEADO.
    ///
    /// Chegou a sair como `skip`, para que ninguém o tomasse por prova
    /// vermelha. Isso consertou um leitor e quebrou o outro: um critério cujo
    /// programa não existia deixou de travar o fechamento e passou como verde.
    /// A distinção foi para o chamador que a quer — o código de saída a leva —,
    /// e o veredito aqui voltou a `fail`.
    #[test]
    fn a_command_the_shell_cannot_find_fails_and_names_the_cause() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("mustard-no-such-program-9f3c --version", None, dir.path());
        assert_eq!(res.status, "fail", "an unrunnable criterion must still block QA, stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, Some(EXIT_COMMAND_NOT_FOUND), "the shell's own not-found code");
        assert!(res.stderr_excerpt.contains("could not find the command"), "the cause is named, not left to the raw output: {}", res.stderr_excerpt);
    }

    /// Commands invoking `cargo ` get the compile-aware ceiling (600 s): a
    /// build/test AC may need a full recompile, and the 120 s default turned
    /// such ACs into silent skips (the regression behind this fix).
    #[test]
    fn qa_timeout_cargo_command_gets_big_ceiling() {
        assert_eq!(ac_timeout_secs_with_override("cargo test -p mustard-rt", None, &[]), AC_TIMEOUT_CARGO_SECS);
        assert_eq!(ac_timeout_secs_with_override("cargo build --workspace", None, &[]), AC_TIMEOUT_CARGO_SECS);
        // Wrapped/chained invocations still contain `cargo ` → big ceiling.
        assert_eq!(ac_timeout_secs_with_override("rtk cargo test && echo ok", None, &[]), AC_TIMEOUT_CARGO_SECS);
    }

    /// Non-cargo commands keep the historical 120 s default.
    #[test]
    fn qa_timeout_non_cargo_keeps_default() {
        assert_eq!(ac_timeout_secs_with_override(r#"node -e "process.exit(0)""#, None, &[]), AC_TIMEOUT_SECS);
        assert_eq!(ac_timeout_secs_with_override("grep -q Modelo SKILL.md", None, &[]), AC_TIMEOUT_SECS);
    }

    /// `MUSTARD_QA_AC_TIMEOUT_SECS` overrides BOTH defaults when it parses as
    /// `u64`; an invalid value is ignored and the command-sensitive default
    /// applies. Exercised through the injected-override core (env mutation
    /// needs `unsafe` under Rust 2024, forbidden in this crate).
    #[test]
    fn qa_timeout_env_override_wins() {
        assert_eq!(ac_timeout_secs_with_override("cargo test -p mustard-rt", Some("300"), &[]), 300);
        assert_eq!(ac_timeout_secs_with_override("echo ok", Some("300"), &[]), 300);
        // Surrounding whitespace is tolerated.
        assert_eq!(ac_timeout_secs_with_override("cargo build", Some(" 42 "), &[]), 42);
        // Invalid values fall back to the command-sensitive defaults.
        assert_eq!(ac_timeout_secs_with_override("cargo build", Some("not-a-number"), &[]), AC_TIMEOUT_CARGO_SECS);
        assert_eq!(ac_timeout_secs_with_override("echo ok", Some(""), &[]), AC_TIMEOUT_SECS);
    }

    /// The pure `Expect:` matcher: absent ⇒ NoExpectation, a compiling pattern
    /// that matches ⇒ Matched, that misses ⇒ Missed, an uncompilable pattern ⇒
    /// InvalidPattern (never a panic). This is the SRP surface the exit-0 gate
    /// in `run_ac_command` delegates to.
    #[test]
    fn expect_regex_matcher_verdicts() {
        assert!(matches!(evaluate_expect(None, "anything"), ExpectVerdict::NoExpectation));
        assert!(matches!(evaluate_expect(Some("test result: ok"), "running 3 tests\ntest result: ok. 3 passed"), ExpectVerdict::Matched));
        assert!(matches!(evaluate_expect(Some("0 passed"), "test result: ok. 3 passed"), ExpectVerdict::Missed));
        // Unclosed character class ⇒ not a valid regex ⇒ fail-open, no panic.
        assert!(matches!(evaluate_expect(Some("[unterminated"), "x"), ExpectVerdict::InvalidPattern));
    }

    /// End-to-end: an exit-0 command whose output MATCHES the `Expect:` regex
    /// passes. `echo` is a builtin in both `cmd.exe` and `sh`, so this is
    /// cross-platform.
    #[test]
    fn expect_regex_exit0_match_passes() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("echo evidence-token", Some("evidence-token"), dir.path());
        assert_eq!(res.status, "pass", "stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, Some(0));
        assert!(res.stderr_excerpt.is_empty());
    }

    /// End-to-end: an exit-0 command whose output does NOT match the `Expect:`
    /// regex is downgraded to `fail` — a green command that printed no expected
    /// evidence proved nothing. The excerpt names the pattern and the output.
    #[test]
    fn expect_regex_exit0_no_match_fails() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("echo evidence-token", Some("MISSING-TOKEN"), dir.path());
        assert_eq!(res.status, "fail", "stderr: {}", res.stderr_excerpt);
        // The command genuinely exited 0; the fail is the evidence gate.
        assert_eq!(res.exit, Some(0));
        assert!(res.stderr_excerpt.contains("MISSING-TOKEN"), "names the pattern: {}", res.stderr_excerpt);
        assert!(res.stderr_excerpt.contains("evidence-token"), "shows the output excerpt: {}", res.stderr_excerpt);
    }

    /// End-to-end: an exit-0 command with NO `Expect:` keeps the legacy pass,
    /// byte-for-byte (empty excerpt) — the unchanged-behaviour guarantee.
    #[test]
    fn expect_regex_absent_keeps_legacy_pass() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("echo whatever", None, dir.path());
        assert_eq!(res.status, "pass", "stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, Some(0));
        assert!(res.stderr_excerpt.is_empty(), "legacy pass carries an empty excerpt");
    }

    /// A shell command that prints ~90 KB — far past the ~64 KB OS pipe buffer
    /// — and then exits 3. The POSIX form is what runs whenever a POSIX shell
    /// was resolved, which on Windows is now the normal case; the `cmd.exe`
    /// form is kept for the fallback so the test still drives the REAL shell on
    /// a machine carrying no `git` install beside one.
    const BIG_OUTPUT_EXIT_3_POSIX: &str = "s=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA; i=0; \
         while [ $i -lt 12 ]; do s=\"$s$s\"; i=$((i+1)); done; echo \"$s\"; exit 3";
    #[cfg(windows)]
    const BIG_OUTPUT_EXIT_3_CMD: &str = "(for /L %i in (1,1,3000) do @echo AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA) & exit 3";

    /// The form matching the shell this process will actually spawn.
    fn big_output_exit_3() -> &'static str {
        #[cfg(windows)]
        if crate::util::platform::posix_shell().is_none() {
            return BIG_OUTPUT_EXIT_3_CMD;
        }
        BIG_OUTPUT_EXIT_3_POSIX
    }

    /// A command that stays alive ~3 s, so a 1 s deadline always fires first.
    #[cfg(windows)]
    const SLEEPS_SECONDS: &str = "powershell -NoProfile -Command \"while (-not (Test-Path mustard-deadline-test.release)) { Start-Sleep -Milliseconds 25 }\"";
    #[cfg(not(windows))]
    const SLEEPS_SECONDS: &str = "sleep 3";

    /// THE regression that must never come back: an AC printing far more than
    /// the OS pipe buffer still completes and is judged by its EXIT CODE. Under
    /// the old `wait_with_output`-after-`try_wait` shape the child blocked
    /// writing into a full pipe, never exited, and the AC degraded to a bogus
    /// timeout — a green-looking `skip` for a criterion that had already failed.
    #[test]
    fn ac_command_past_the_pipe_buffer_is_judged_by_exit_code() {
        let dir = tempdir().unwrap();
        let res = run_ac_command(big_output_exit_3(), None, dir.path());
        assert_eq!(res.status, "fail", "verdict comes from the exit code, stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, Some(3), "the command's own code, not a timeout");
    }

    /// Um comando que escreve 200 linhas numeradas, `linha 1` a `linha 200`, e
    /// sai com o código 3: a falha aparece na última.
    const PRINTS_LINES_THEN_FAILS: &str = "i=1; while [ $i -le 200 ]; do echo \"linha $i\"; i=$((i+1)); done; exit 3";

    /// A falha mostra o FIM da saída, onde o motivo aparece: numa saída de 200
    /// linhas que falha na última, o trecho tem a última linha, guarda só as
    /// 40 últimas e deixa a primeira de fora. O verde que diz rodar zero teste
    /// segue com o começo.
    #[test]
    fn a_failure_keeps_the_end_of_the_output_and_a_zero_test_green_keeps_the_start() {
        let dir = tempdir().unwrap();
        let res = run_ac_command(PRINTS_LINES_THEN_FAILS, None, dir.path());
        assert_eq!((res.status.as_str(), res.exit), ("fail", Some(3)), "{}", res.stderr_excerpt);
        assert!(res.stderr_excerpt.ends_with("linha 200"), "o fim da saída: {}", res.stderr_excerpt);
        assert_eq!(res.stderr_excerpt.lines().count(), 40, "as últimas 40 linhas: {}", res.stderr_excerpt);
        assert!(res.stderr_excerpt.starts_with("linha 161\n"), "{}", res.stderr_excerpt);
        assert!(!res.stderr_excerpt.contains("linha 1\n"), "o começo fica de fora: {}", res.stderr_excerpt);

        // A evidência que falhou por não achar o que esperava também mostra o fim.
        let missed = run_ac_command("i=1; while [ $i -le 200 ]; do echo \"linha $i\"; i=$((i+1)); done", Some("nunca-escrito"), dir.path());
        assert_eq!(missed.status, "fail", "{}", missed.stderr_excerpt);
        assert!(missed.stderr_excerpt.ends_with("linha 200"), "{}", missed.stderr_excerpt);

        // O verde de zero teste continua com o começo da saída, em 100 caracteres.
        let zero = run_ac_command("echo running 0 tests; i=1; while [ $i -le 30 ]; do echo \"linha $i\"; i=$((i+1)); done", None, dir.path());
        assert_eq!((zero.status.as_str(), zero.tests_run), ("pass", Some(0)), "{}", zero.stderr_excerpt);
        assert!(zero.stderr_excerpt.starts_with("running 0 tests"), "{}", zero.stderr_excerpt);
        assert_eq!(zero.stderr_excerpt.chars().count(), 100, "{}", zero.stderr_excerpt);
    }

    /// O teto de caracteres vale junto do de linhas: 100 linhas de 100
    /// caracteres cabem nas 40 linhas mas passam de 2.000 caracteres, e sobram
    /// os 2.000 do fim; uma saída curta sai inteira.
    #[test]
    fn the_end_of_the_output_is_cut_at_forty_lines_or_two_thousand_characters() {
        let long: String = (1..=100).flat_map(|n| [format!("{n:03}{}", "x".repeat(97)), "\n".to_string()]).collect();
        let tail = tail_excerpt(long.trim_end());
        assert_eq!(tail.chars().count(), 2000, "{tail}");
        assert!(tail.ends_with(&format!("100{}", "x".repeat(97))), "{tail}");
        assert!(tail.contains("082"), "a primeira linha inteira que cabe nos 2.000 caracteres: {tail}");
        assert!(!tail.contains("081"), "o número de uma linha cortada fica de fora: {tail}");

        let short: String = (1..=12).flat_map(|n| [format!("linha {n}"), "\n".to_string()]).collect();
        assert_eq!(tail_excerpt(short.trim_end()), short.trim_end());
        assert_eq!(tail_excerpt(""), "");
    }

    /// An AC killed by its deadline reports `timeout` — its OWN class, never
    /// `skip`. `skip` keeps meaning "could not be attempted at all"; a timed-out
    /// AC WAS attempted and simply verified nothing.
    #[test]
    fn ac_command_killed_by_deadline_reports_timeout_not_skip() {
        let dir = tempdir().unwrap();
        let res = run_ac_command_with_timeout(SLEEPS_SECONDS, None, dir.path(), Duration::from_secs(1));
        assert_eq!(res.status, "timeout", "stderr: {}", res.stderr_excerpt);
        assert_eq!(res.exit, None, "a killed command has no exit code");
        assert_eq!(res.stderr_excerpt, "timeout after 1000ms");
    }

    /// End-to-end: an exit-0 command whose `Expect:` is an INVALID regex skips
    /// (fail-open) with a reason — never a panic, never a false pass/fail.
    #[test]
    fn expect_regex_invalid_pattern_skips() {
        let dir = tempdir().unwrap();
        let res = run_ac_command("echo whatever", Some("[unterminated"), dir.path());
        assert_eq!(res.status, "skip", "stderr: {}", res.stderr_excerpt);
        assert!(res.stderr_excerpt.contains("not a valid regex"), "skip reason states the invalid pattern: {}", res.stderr_excerpt);
    }
}
