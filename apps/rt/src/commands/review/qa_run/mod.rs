//! The criteria runner: [`run_proof`], which runs one criterion's proof,
//! [`run_criteria_proofs`],
//! the loop that the close and the round both call to run a list of
//! criteria in order and stop at the first that does not pass,
//! [`run_command`], which runs a flow command that is not a proof,
//! [`run_server_command`], which the close calls for the lint and the whole
//! suite the server runs, and the section reader the page uses.
//!
//! As duas portas rodam o mesmo comando do mesmo jeito e se separam em duas
//! leituras, que valem só na prova de um critério, que promete rodar teste, e
//! nunca no lint nem em outro comando do fluxo: quantos testes a saída diz ter
//! rodado, e se cada nome de teste que o comando cita existe no projeto.

use std::path::Path;

use mustard_core::platform::git;

mod runner;

#[cfg(test)]
pub(crate) use runner::{ceiling_secs, with_timeout_variable, Ceiling};

/// One AC execution outcome.
pub(crate) struct AcResult {
    status: String,
    exit: Option<i64>,
    duration_ms: u128,
    stderr_excerpt: String,
    /// Quantos testes a saída do comando disse ter rodado, quando um executor
    /// que o projeto usa se reconheceu nela; `None` quando a saída não
    /// responde à pergunta ou quando o comando nem chegou a rodar. É leitura,
    /// e não veredito: quem julga esse número é a prova de um critério, em
    /// [`run_proof`], e nunca o lint nem outro comando do fluxo.
    tests_run: Option<u64>,
}

/// Uma prova de critério rodada uma vez, como o fechamento a grava.
pub(crate) struct ProofRun {
    /// `pass` quando a prova passou, `fail` em qualquer outro desfecho.
    pub result: &'static str,
    /// O código de saída; o que não chegou a rodar sai com o código de erro.
    pub exit: i64,
    /// Quanto demorou, em milissegundos.
    pub ms: u64,
    /// O começo do que a prova escreveu, quando ela não passou — a saída do
    /// executor, e não uma frase montada sobre ela.
    pub output: String,
    /// A prova de critério saiu verde sem rodar teste nenhum, e por isso não
    /// passou, com o número que a saída do executor disse — é ele que a
    /// recusa mostra. `None` quando a prova passou, quando ela falhou por
    /// outro motivo, e em todo comando que não é prova de critério.
    pub ran_no_test: Option<u64>,
    /// A prova de critério saiu verde, mas cita um nome de teste que não
    /// aparece em arquivo nenhum do projeto, e por isso não passou: o
    /// primeiro nome que faltou, que é o que a recusa mostra. `None` quando a
    /// prova passou, quando ela falhou por outro motivo, e em todo comando
    /// que não é prova de critério.
    pub missing_test: Option<String>,
}

/// Roda a prova de um critério uma vez, pelo mesmo executor do QA: o mesmo
/// shell, o mesmo teto de tempo e a mesma classificação. É por aqui que o
/// fechamento roda cada critério, para que as duas portas nunca discordem
/// sobre o que é uma prova que passou.
///
/// Só a prova de um critério promete rodar teste, e por isso só ela é lida
/// assim: verde sem rodar teste nenhum não passa, porque o nome do teste não
/// casou, e verde citando um teste que não existe também não. O comando do
/// fluxo que não é prova de critério — o lint do projeto — roda por
/// [`run_command`], que não faz essas leituras.
pub(crate) fn run_proof(command: &str, cwd: &Path) -> ProofRun {
    graded(runner::run_ac_command(command, None, cwd), Some((command, cwd)))
}

/// Roda um comando do fluxo que não é prova de critério — hoje, a
/// compilação que a rodada confere — pelo mesmo executor do QA, com o mesmo
/// shell e o mesmo teto de tempo.
///
/// A leitura de quantos testes o comando rodou não vale aqui: um lint verde
/// cuja saída cite "no tests" não é uma prova que deixou de provar, e quem
/// lesse assim recusaria um verde legítimo.
pub(crate) fn run_command(command: &str, cwd: &Path) -> ProofRun {
    graded(runner::run_ac_command(command, None, cwd), None)
}

/// Roda um dos dois comandos que o servidor roda — o `lintCommand` e o
/// `testCommand` do `mustard.json` —, como o fechamento os repete. Mesmo
/// executor e mesma leitura de [`run_command`], com um teto só deles, de uma
/// hora: a suíte inteira de um projeto não cabe no teto de uma prova de
/// critério, e a variável `MUSTARD_QA_AC_TIMEOUT_SECS` vale só para a prova.
pub(crate) fn run_server_command(command: &str, cwd: &Path) -> ProofRun {
    graded(runner::run_server_command(command, cwd), None)
}

/// Por que a prova de um critério não passou. A escolha entre os três
/// motivos é feita aqui, uma vez, em [`ProofRun::fault`]: o fechamento, a
/// rodada e o aviso da entrega leem o mesmo motivo, e cada um só escolhe a
/// própria frase.
#[derive(Debug)]
pub(crate) enum ProofFault {
    /// Saiu verde sem rodar teste nenhum, com o número que a saída disse.
    RanNoTest(u64),
    /// Saiu verde citando um nome de teste que não aparece em arquivo nenhum
    /// do projeto: o primeiro nome que faltou.
    MissingTest(String),
    /// Não executou ou não passou, com a saída de erro do comando.
    Failed(String),
}

impl ProofRun {
    /// O motivo de a prova não ter passado, ou `None` quando ela passou. A
    /// contagem de zero teste vem antes do nome ausente, porque só a prova
    /// que rodou teste tem o nome conferido ([`graded`]).
    pub(crate) fn fault(&self) -> Option<ProofFault> {
        if self.result == "pass" {
            return None;
        }
        Some(match (self.ran_no_test, &self.missing_test) {
            (Some(tests), _) => ProofFault::RanNoTest(tests),
            (None, Some(name)) => ProofFault::MissingTest(name.clone()),
            (None, None) => ProofFault::Failed(self.output.clone()),
        })
    }
}

/// A prova de um critério que não passou: o código dele, o comando inteiro
/// que tentou rodar e o motivo — o que a recusa do fechamento e da rodada
/// nomeiam.
pub(crate) struct FailedProof {
    pub code: String,
    pub command: String,
    pub fault: ProofFault,
}

/// Roda a prova de cada critério de `criteria` (id, código, comando), na
/// ordem em que a lista chega, uma de cada vez, e devolve a execução de cada
/// um junto do primeiro que não passou. É o mesmo laço que o fechamento roda
/// para os critérios da spec inteira, em `close.rs`, e que a rodada roda,
/// antes de comitar, só para os que as ondas do relatório cobrem: quem chama
/// decide o que grava com cada execução e como nomeia a recusa — aqui só se
/// roda e se lê o resultado.
pub(crate) fn run_criteria_proofs(
    root: &Path,
    criteria: &[(u64, String, String)],
) -> (Vec<(u64, String, ProofRun)>, Option<FailedProof>) {
    let mut runs = Vec::new();
    let mut failed = None;
    for (id, code, proof) in criteria {
        let out = run_proof(proof, root);
        if failed.is_none()
            && let Some(fault) = out.fault()
        {
            failed = Some(FailedProof { code: code.clone(), command: proof.clone(), fault });
        }
        runs.push((*id, code.clone(), out));
    }
    (runs, failed)
}

/// Uma execução classificada como o fechamento a grava. As duas portas
/// entram aqui, e a diferença entre elas é um lugar só: `proof`, o comando e
/// a raiz quando ele é a prova de um critério. Só nela o verde vira recusa,
/// por dois motivos. Um é a saída do executor dizer que não rodou teste
/// nenhum, e a recusa carrega o número lido — não há recusa sem contagem
/// lida. O outro, quando a contagem não recusou, é o comando citar um nome
/// de teste que não existe no projeto ([`missing_test_name`]).
///
/// O que a execução leva é sempre o que o comando escreveu, e nunca uma frase
/// montada aqui: quem lê o evento gravado precisa ver a saída do executor. O
/// número lido vai pelo `ran_no_test`, e o nome que faltou pelo
/// `missing_test`: é deles que [`ProofRun::fault`] tira o motivo que cada
/// recusa e cada aviso mostram.
fn graded(out: AcResult, proof: Option<(&str, &Path)>) -> ProofRun {
    let green = out.status == "pass";
    let ran_no_test = out.tests_run.filter(|count| *count == 0 && proof.is_some() && green);
    let missing_test = match proof {
        Some((command, root)) if green && ran_no_test.is_none() => missing_test_name(command, root),
        _ => None,
    };
    ProofRun {
        result: if green && ran_no_test.is_none() && missing_test.is_none() { "pass" } else { "fail" },
        exit: out.exit.unwrap_or(1),
        ms: u64::try_from(out.duration_ms).unwrap_or(u64::MAX),
        ran_no_test,
        missing_test,
        output: out.stderr_excerpt,
    }
}

/// O primeiro nome de teste que `command` cita ([`cited_test_names`]) e que
/// não aparece em arquivo nenhum do projeto em `root`, nem como pedaço do
/// texto de um arquivo, nem como nome de arquivo ou pasta. Basta o pedaço:
/// o filtro do executor de testes acha o teste por um pedaço do nome, como
/// `cargo test -- comeco_do_nome` roda `comeco_do_nome_inteiro`.
///
/// A busca olha os arquivos que o git guarda e os novos que ele não ignora:
/// a rodada roda a prova antes de comitar, quando o arquivo de teste que a
/// onda criou ainda não foi registrado. O que o git ignora fica de fora — a
/// pasta de compilação guarda os nomes dos testes que já existiram. A pasta
/// das specs também fica de fora, porque é nela que o próprio comando está
/// gravado. O nome de arquivo conta porque um alvo de teste inteiro, como o
/// de `cargo test --test nome_do_alvo`, é o nome do arquivo dele.
///
/// Onde o git não responde — raiz sem repositório, programa ausente —, nada
/// é recusado: sem a lista de arquivos não há como dizer que falta um teste.
fn missing_test_name(command: &str, root: &Path) -> Option<String> {
    let names = cited_test_names(command);
    if names.is_empty() {
        return None;
    }
    let mut files: Option<Option<Vec<String>>> = None;
    names.into_iter().find(|name| {
        let search = git::run(
            root,
            &["grep", "--untracked", "-F", "-q", "-e", name, "--", ".", ":(exclude).claude/spec"],
        );
        // Achou (saída zero) ou não pôde procurar (erro escrito): não falta.
        if search.ok || !search.stderr.trim().is_empty() {
            return false;
        }
        let listed = files.get_or_insert_with(|| {
            let run = git::run(root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]);
            run.ok.then(|| run.stdout.split('\0').map(str::to_string).collect())
        });
        listed.as_ref().is_some_and(|paths| !paths.iter().any(|path| names_file(path, name)))
    })
}

/// `path` tem um pedaço — pasta ou arquivo, sem o que vem depois do primeiro
/// ponto — igual a `name`.
fn names_file(path: &str, name: &str) -> bool {
    path.split('/').any(|piece| piece.split('.').next() == Some(name))
}

/// Os nomes de teste que `command` cita, na ordem em que aparecem e sem
/// repetir. O comando é lido como o shell o separa em palavras, com as aspas
/// tiradas. Tem jeito de nome de teste a palavra feita só de letras, dígitos
/// e sublinhado, com pelo menos um sublinhado; num caminho como `a::b`, vale
/// o último pedaço.
///
/// Não entram a opção (começa com `-`), a atribuição (tem `=`), o texto com
/// `/`, `$`, `~` ou espaço, o nome do programa de cada comando da linha —
/// o primeiro depois das atribuições, no começo e depois de `;`, `|`, `&` e
/// parênteses — e o arquivo de um redirecionamento (`>` ou `<`).
fn cited_test_names(command: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut program_next = true;
    let mut redirect_target = false;
    for token in shell_tokens(command) {
        let word = match token {
            ShellToken::Break => {
                program_next = true;
                redirect_target = false;
                continue;
            }
            ShellToken::Redirect => {
                redirect_target = true;
                continue;
            }
            ShellToken::Word(word) => word,
        };
        if std::mem::take(&mut redirect_target) || word.contains('=') {
            continue;
        }
        if std::mem::take(&mut program_next) {
            continue;
        }
        if let Some(name) = test_name(&word)
            && !names.contains(&name)
        {
            names.push(name);
        }
    }
    names
}

/// O nome de teste que `word` cita, pela regra de [`cited_test_names`].
fn test_name(word: &str) -> Option<String> {
    if word.starts_with('-') || word.contains(['=', '/', '$', '~']) || word.chars().any(char::is_whitespace) {
        return None;
    }
    let last = word.rsplit("::").next().unwrap_or(word);
    let shaped = last.contains('_')
        && last.chars().any(char::is_alphanumeric)
        && last.chars().all(|c| c.is_alphanumeric() || c == '_');
    shaped.then(|| last.to_string())
}

/// Um pedaço da linha de comando como o shell a separa.
enum ShellToken {
    /// Uma palavra, já sem as aspas.
    Word(String),
    /// O fim de um comando: `;`, `|`, `&`, parêntese ou quebra de linha.
    Break,
    /// Um redirecionamento: a palavra seguinte é um arquivo.
    Redirect,
}

/// `command` separado como o shell o separa: espaço fora de aspas separa
/// palavras, aspas simples e duplas juntam, a barra invertida protege o
/// caractere seguinte.
fn shell_tokens(command: &str) -> Vec<ShellToken> {
    let mut tokens = Vec::new();
    let mut word: Option<String> = None;
    let mut quote: Option<char> = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some('"') | None, '\\') => {
                if let Some(next) = chars.next() {
                    word.get_or_insert_with(String::new).push(next);
                }
            }
            (Some(_), c) => word.get_or_insert_with(String::new).push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                word.get_or_insert_with(String::new);
            }
            (None, c) if c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')' | '<' | '>') => {
                tokens.extend(word.take().map(ShellToken::Word));
                if matches!(c, '<' | '>') {
                    tokens.push(ShellToken::Redirect);
                } else if !c.is_whitespace() || c == '\n' {
                    tokens.push(ShellToken::Break);
                }
            }
            (None, c) => word.get_or_insert_with(String::new).push(c),
        }
    }
    tokens.extend(word.take().map(ShellToken::Word));
    tokens
}

/// Extract the `## Acceptance Criteria` section body (heading line stripped),
/// recognizing the EN and PT headings via [`crate::commands::spec::spec_sections`].
///
/// `pub(crate)`: the spec page (`spec_events::pages`) reads the section
/// through here.
pub(crate) fn extract_ac_section(markdown: &str) -> Option<String> {
    // Reuse the shared, i18n-aware section extractor so this QA reader and the
    // rewave producer (which carries this section verbatim into `wave-plan.md`)
    // parse the heading identically and cannot drift.
    let block =
        crate::commands::spec::spec_sections::section_block(markdown, "acceptanceCriteria")?;
    // Body only — drop the heading line itself.
    Some(block.split_once('\n').map_or("", |(_, body)| body).to_string())
}

/// Options for one run of the executor, carried on the thread-local it reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct QaRunOptions {
    /// `true` when invoked from a process that **could be** the binary some AC
    /// commands rebuild — this very `mustard-rt`.
    ///
    /// Setting this flag lets the executor ask the PATH question before
    /// spawning: when the file a `cargo build|test` would write IS the file
    /// this process is executing from, a `--workspace` command gets
    /// `--exclude <package>` appended and a direct `-p` command is refused
    /// outright with a reason naming that file, instead of failing with
    /// `failed to remove file mustard-rt.exe` (Windows os error 5). When the
    /// two paths differ — the shipped shape, an installed binary against the
    /// workspace `target/` — nothing is rewritten and nothing is refused. See
    /// [`targets_running_binary`].
    ///
    /// No production path sets this: the close and the round run their proofs
    /// with the default `false`, and only the executor's own tests turn it on.
    pub self_invoked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PT heading "Critérios de Aceitação globais" (suffix word after the
    /// canonical name) must still resolve — `is_heading` matches with a
    /// word-boundary tolerance after the variant. Regression guard for
    /// language-agnostic parsing.
    #[test]
    fn extracts_ac_section_pt_heading_with_suffix() {
        let md = "# Spec\n\n## Critérios de Aceitação globais\n- [ ] AC-G1: x — Command: `true`\n\n## Files\n- a.rs\n";
        let section = extract_ac_section(md).unwrap();
        assert!(section.contains("AC-G1"));
        assert!(!section.contains("Files"));
    }

    #[test]
    fn extracts_ac_section_body() {
        let md = "# Spec\n\n## Acceptance Criteria\n- [ ] AC-1: x — Command: `true`\n\n## Files\n- a.rs\n";
        let section = extract_ac_section(md).unwrap();
        assert!(section.contains("AC-1"));
        assert!(!section.contains("Files"));
    }

    /// A peça que roda a prova de um critério — [`run_proof`], a mesma que o
    /// fechamento e a rodada chamam — não se contenta com o código de saída:
    /// um comando real, de um executor real, cujo filtro não casa teste
    /// nenhum, sai verde e ainda assim não passa, porque a leitura da saída
    /// diz zero teste rodado. É a peça, e não a conversa entre close.rs e
    /// runner.rs, que promete essa leitura.
    #[test]
    fn a_verificacao_que_nao_roda_teste_nenhum_e_recusada() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma() { assert_eq!(1 + 1, 2); }\n}\n",
        )
        .unwrap();

        // Sanidade: o mesmo comando, com o nome certo, roda e passa — a
        // recusa abaixo é da leitura de zero testes, não de outro motivo.
        let matching = run_proof("cargo test --lib -- tests::soma --exact", root);
        assert_eq!(matching.result, "pass", "a prova com o nome certo passa");
        assert_eq!(matching.ran_no_test, None);

        let out = run_proof("cargo test --lib -- nome_que_nao_existe_em_lugar_nenhum", root);
        assert_eq!(out.result, "fail", "verde sem rodar teste não é prova aprovada");
        assert_eq!(out.ran_no_test, Some(0), "a recusa carrega o número que a saída disse");
    }

    /// Um repositório em `root` com o arquivo `path` comitado, contendo
    /// `body`.
    fn repo_with(root: &Path, files: &[(&str, &str)]) {
        for (path, body) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, body).unwrap();
        }
        for args in [&["init", "-q"][..], &["add", "-A"][..], &["commit", "-q", "-m", "semente"][..]] {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        }
    }

    /// Das palavras da prova, só as que têm jeito de nome de teste são
    /// buscadas: letras, dígitos e sublinhado, com pelo menos um sublinhado,
    /// e de um caminho `a::b` o último pedaço. A variável, o programa, as
    /// opções e o pacote com hífen ficam de fora, e a prova que não cita
    /// nome nenhum não tem o que buscar.
    #[test]
    fn a_proof_cites_only_the_words_shaped_like_a_test_name() {
        let names = |command: &str| cited_test_names(command);
        assert_eq!(
            names(r#"PATH="$HOME/.cargo/bin:$PATH" cargo test --locked -p pacote-x -- nome_presente"#),
            ["nome_presente"],
        );
        assert!(names("echo Tests: 3 total").is_empty());
        assert_eq!(names("cargo test --lib -- tests::soma_de_dois --exact"), ["soma_de_dois"]);
        assert!(names("cargo test --lib -- tests::soma --exact").is_empty(), "sem sublinhado não é nome");
        assert_eq!(names("cargo test -- um_teste um_teste outro_teste"), ["um_teste", "outro_teste"]);
        for refused_line in [
            "cargo test -- \"dois_nomes com_espaco\"",
            "cargo test -- $NOME_VAR",
            "cargo test -- ~/pasta_x",
            "pytest tests/test_soma.py::test_um",
            "dotnet test --filter Name=Soma_Dois",
            "dotnet test --filter FullyQualifiedName~Soma_Dois",
            "cargo test --no_run",
            "run_all_tests && cd pasta-x",
            "cargo test > saida_log",
            "FOO_BAR=1 run_tests",
        ] {
            assert!(names(refused_line).is_empty(), "{refused_line}: {:?}", names(refused_line));
        }
        assert_eq!(names("cd pasta-x && run_all -- um_teste | tail_it"), ["um_teste"], "cada comando tem seu programa");
    }

    /// A prova verde que cita um nome ausente de todo arquivo do projeto não
    /// passa, e leva o nome que faltou; a que cita só nomes presentes passa.
    /// Conta o arquivo que o git guarda, o novo que ele não ignora e o nome
    /// de arquivo, como o de um alvo de teste inteiro. Não conta o que o git
    /// ignora nem a pasta das specs, onde o próprio comando está gravado. O
    /// comando do fluxo que não é prova não faz essa busca, e sem
    /// repositório não há recusa.
    #[test]
    fn a_green_proof_citing_a_test_in_no_file_does_not_pass() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repo_with(
            root,
            &[
                ("src/lib.rs", "#[test]\nfn nome_presente() {}\n"),
                ("tests/alvo_inteiro.rs", "#[test]\nfn um() {}\n"),
                (".gitignore", "target/\n"),
            ],
        );
        std::fs::write(root.join("src/novo.rs"), "fn teste_novo() {}\n").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/velho.d"), "teste_velho\n").unwrap();
        std::fs::create_dir_all(root.join(".claude/spec/x")).unwrap();
        std::fs::write(root.join(".claude/spec/x/spec.ndjson"), "echo nome_so_na_spec\n").unwrap();

        let missing = run_proof("echo nome_presente nome_ausente_aqui", root);
        assert_eq!(missing.result, "fail", "verde citando teste que não existe não passa");
        assert_eq!(missing.missing_test.as_deref(), Some("nome_ausente_aqui"));
        assert_eq!(missing.ran_no_test, None);
        assert_eq!(missing.exit, 0, "a execução guarda o código com que o comando saiu");

        for present in ["echo nome_presente", "echo alvo_inteiro", "echo teste_novo", "echo Tests: 3 total"] {
            let out = run_proof(present, root);
            assert_eq!((out.result, out.missing_test), ("pass", None), "{present}");
        }
        for absent in ["echo teste_velho", "echo nome_so_na_spec"] {
            assert_eq!(run_proof(absent, root).result, "fail", "{absent}");
        }
        assert_eq!(
            missing_test_name(r#"PATH="$HOME/.cargo/bin:$PATH" cargo test --locked -p pacote-x -- nome_presente"#, root),
            None,
            "só o nome é buscado, e ele existe",
        );
        assert_eq!(run_command("echo nome_ausente_aqui", root).result, "pass", "o comando do fluxo não busca nome");

        let bare = tempfile::tempdir().unwrap();
        assert_eq!(run_proof("echo nome_ausente_aqui", bare.path()).result, "pass", "sem repositório não há recusa");
    }

    /// O nome citado vale pelo pedaço: o filtro do executor de testes acha o
    /// teste por um pedaço do nome, e a prova que cita só o começo do nome
    /// de um teste que existe passa. O pedaço que não está em arquivo
    /// nenhum continua recusado.
    #[test]
    fn a_proof_citing_the_start_of_a_test_name_passes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repo_with(root, &[("src/lib.rs", "#[test]\nfn comeco_do_nome_inteiro() {}\n")]);

        assert_eq!(missing_test_name("cargo test -p x -- comeco_do_nome", root), None, "o começo do nome existe");
        let out = run_proof("echo comeco_do_nome", root);
        assert_eq!((out.result, out.missing_test), ("pass", None), "a prova verde que cita o começo do nome passa");
        let absent = run_proof("echo comeco_de_outro", root);
        assert_eq!(absent.missing_test.as_deref(), Some("comeco_de_outro"), "o pedaço ausente continua recusado");
    }
}
