//! `runtime_output` — a conferência da trava de comandos que guarda a
//! resposta do Mustard inteira.
//!
//! A resposta do `mustard-rt` (e do `mustard-boot`, que o chama) se lê
//! inteira: quem corta, filtra ou passa a resposta por um script perde o que
//! ela diz. A conferência recusa o comando quando:
//!
//! - o Mustard manda a resposta, pelo encadeamento de `|` ou `|&`, a um
//!   programa de [`SCRIPTS`]; o programa que vem depois do `rtk`, do `sudo` e
//!   dos outros invólucros conta pelo nome dele, e o `xargs` conta como
//!   programa da lista;
//! - um programa de [`SCRIPTS`] recebe a resposta por substituição:
//!   `$(mustard-rt …)`, crases, `<(mustard-rt …)`, ou o corpo de um heredoc
//!   que roda o Mustard.
//!
//! A leitura mascarada da configuração (`run config-read`), que a trava de
//! leitura devolve pronta, não é resposta do Mustard: o filtro depois dela
//! passa.
//!
//! Gravar a resposta num arquivo (`>` ou `>>`), juntar os erros (`2>&1`) e
//! dobrar as linhas (`| fold`) continuam liberados: é assim que se lê por
//! partes uma resposta grande. A conferência lê os comandos que
//! [`super::lex::segments`] achou, nunca o texto cru: `echo mustard-rt | wc -c`
//! cita o nome sem rodar o Mustard e passa.

use mustard_core::SupportedLocale;
use mustard_core::domain::model::contract::Verdict;

use super::lex::{Segment, truncate};

/// Os programas do Mustard cuja resposta se lê inteira.
const RUNTIME: &[&str] = &["mustard-rt", "mustard-rt.exe", "mustard-boot", "mustard-boot.cmd"];

/// Os programas que, ao receber a resposta, a passam por script, filtro ou
/// corte. A lista é fechada: o `fold` e o `cat` ficam de fora.
const SCRIPTS: &[&str] = &[
    "python", "python3", "jq", "node", "perl", "ruby", "grep", "rg", "sed", "awk", "head", "tail", "cut", "tr", "sort",
    "uniq", "wc", "xargs", "tee",
];

/// Se `segment` roda o Mustard. A leitura mascarada da configuração
/// (`run config-read`) fica de fora: o que sai dela é o arquivo da pessoa,
/// com a chave escondida, e não uma resposta do Mustard.
fn runs_the_runtime(segment: &Segment) -> bool {
    RUNTIME.contains(&segment.name())
        && !matches!(segment.args.as_slice(), [run, read, ..] if run.text == "run" && read.text == "config-read")
}

/// Se `segment` roda um programa de [`SCRIPTS`], pelo nome ou pelo `xargs`
/// que o chama.
fn runs_a_script(segment: &Segment) -> bool {
    SCRIPTS.contains(&segment.name()) || segment.wrappers.iter().any(|wrapper| SCRIPTS.contains(&wrapper.as_str()))
}

/// Os comandos que estão dentro de `segments[at]`: os que vêm logo depois
/// dele, mais fundos que ele.
fn inside(segments: &[Segment], at: usize) -> &[Segment] {
    let depth = segments[at].depth;
    let rest = &segments[at + 1..];
    let end = rest.iter().position(|segment| segment.depth <= depth).unwrap_or(rest.len());
    &rest[..end]
}

/// Se a saída de `segments[at]` chega, pelo encadeamento de `|`, a um
/// programa de [`SCRIPTS`]. Os comandos de dentro de cada elo não são elos;
/// o encadeamento acaba no primeiro elo sem `|` ou quando a lista que o
/// guarda fecha.
fn feeds_a_script(segments: &[Segment], at: usize) -> bool {
    let depth = segments[at].depth;
    let mut piped = segments[at].piped;
    for segment in &segments[at + 1..] {
        if segment.depth < depth || !piped {
            return false;
        }
        if segment.depth > depth {
            continue;
        }
        if runs_a_script(segment) {
            return true;
        }
        piped = segment.piped;
    }
    false
}

/// As opções cuja resposta é curta, fixa e sem nada da obra: cortá-la não
/// perde o que o Mustard diz, porque o que ele diz ali não muda.
const FIXED_ANSWER: &[&str] = &["--help", "-h", "--version", "-V"];

/// `true` quando o comando do Mustard em `segment` pede uma resposta de
/// tamanho fixo ([`FIXED_ANSWER`]): a ajuda e a versão. Qualquer outra pode
/// trazer uma recusa, que se lê inteira.
fn asks_a_fixed_answer(segment: &Segment) -> bool {
    segment.args.iter().any(|arg| FIXED_ANSWER.contains(&arg.text.as_str()))
}

/// O bloco de resumo de cada bloco grande do `read`: quem corta um bloco
/// grande quase sempre quer o resumo dele, e o Mustard já o monta inteiro.
const SUMMARY_OF: &[(&str, &str)] = &[("metrics", "calls"), ("waves", "wave-list")];

/// O bloco grande que algum comando do Mustard em `segments` lê e o resumo
/// dele, quando é um `run read <bloco>` de [`SUMMARY_OF`].
fn summary_asked(segments: &[Segment]) -> Option<(&'static str, &'static str)> {
    segments.iter().filter(|segment| runs_the_runtime(segment)).find_map(|segment| {
        let mut words = segment.args.iter().map(|arg| arg.text.as_str()).skip_while(|word| *word != "read").skip(1);
        let asked = words.next()?;
        SUMMARY_OF.iter().copied().find(|(block, _)| *block == asked)
    })
}

/// O mesmo comando `cmd` com a resposta inteira num arquivo: a etapa que
/// corta a resposta do Mustard sai, com o encadeamento dela, e cada etapa do
/// Mustard grava num arquivo próprio ([`answer_file`]). A etapa de ajuda ou
/// de versão, que passa sozinha, fica como foi escrita, com o corte dela.
/// `None` quando a reescrita não acha as duas etapas — a resposta chega ao
/// script por substituição, e não por encadeamento.
fn with_the_answer_in_a_file(segments: &[Segment], cmd: &str) -> Option<String> {
    if !segments.iter().any(runs_a_script) {
        return None;
    }
    // A etapa anterior foi uma do Mustard reescrita: só o corte que vem logo
    // depois dela sai. O corte da ajuda, ou de outro programa, fica.
    let cutting = std::cell::Cell::new(false);
    let names = std::cell::RefCell::new(Vec::<String>::new());
    let out = super::rewrite::rewritten_steps(cmd, |step| {
        // A etapa se lê pelo mesmo analisador do comando inteiro: o programa
        // dela vem depois dos invólucros, como `rtk head -5`.
        let read = super::lex::segments(step);
        let first = read.first()?;
        if runs_a_script(first) {
            return cutting.get().then_some(super::rewrite::Step::Out);
        }
        cutting.set(runs_the_runtime(first) && !asks_a_fixed_answer(first));
        if !cutting.get() {
            return None;
        }
        // O `2>&1` no fim vai depois do arquivo: antes dele, o erro seguiria
        // para a saída de antes, e não para o arquivo.
        let (body, errors) = match step.strip_suffix("2>&1") {
            Some(body) => (body.trim_end(), " 2>&1"),
            None => (step, ""),
        };
        let file = answer_file(first, &mut names.borrow_mut());
        Some(super::rewrite::Step::Put(format!("{body} > {file}{errors}")))
    })?;
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// O arquivo em que a etapa do Mustard `segment` grava a resposta: na pasta
/// temporária, fora do repositório, com o nome da etapa — `read-backlog`,
/// `resume`. Um arquivo dentro do projeto entrava no commit de quem roda
/// `git add -A`, e duas etapas no mesmo arquivo perdiam a primeira resposta.
/// O nome que já saiu neste comando (`taken`) ganha um número.
fn answer_file(segment: &Segment, taken: &mut Vec<String>) -> String {
    let words: Vec<String> = segment
        .args
        .iter()
        .map(|arg| arg.text.as_str())
        .skip_while(|word| *word != "run")
        .skip(1)
        .take_while(|word| !word.starts_with('-'))
        .take(2)
        .map(|word| word.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect())
        .collect();
    let stem = if words.is_empty() { "mustard".to_string() } else { format!("mustard-{}", words.join("-")) };
    let mut name = stem.clone();
    let mut n = 1;
    while taken.contains(&name) {
        n += 1;
        name = format!("{stem}-{n}");
    }
    taken.push(name.clone());
    let path = std::env::temp_dir().join(format!("{name}.txt")).display().to_string();
    if path.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '\\' | '.' | '_' | '-' | ':')) { path } else { format!("'{path}'") }
}

/// A conferência: recusa o comando `cmd`, já lido em `segments`, quando a
/// resposta do Mustard passa por script, filtro ou corte. A recusa, no
/// idioma `lang`, mostra o comando, devolve o comando pronto com a resposta
/// num arquivo e, quando o corte é de um bloco grande, diz o bloco de resumo
/// que cabe inteiro.
///
/// A ajuda e a versão passam: a resposta delas é curta, fixa e não traz nada
/// da obra. O gancho pegava até `mustard-rt run read --help | head`.
pub(super) fn bash_runtime_output(segments: &[Segment], cmd: &str, lang: SupportedLocale) -> Option<Verdict> {
    let scripted = (0..segments.len()).any(|at| {
        let runtime = runs_the_runtime(&segments[at]) && !asks_a_fixed_answer(&segments[at]);
        (runtime && feeds_a_script(segments, at))
            || (runs_a_script(&segments[at]) && inside(segments, at).iter().any(|inner| runs_the_runtime(inner) && !asks_a_fixed_answer(inner)))
    });
    if !scripted {
        return None;
    }
    let mut reason = crate::shared::say::say("runtime_output.scripted", lang, &[("{command}", truncate(cmd, 120))]);
    if let Some(ready) = with_the_answer_in_a_file(segments, cmd) {
        reason.push('\n');
        reason.push_str(&crate::shared::say::say("runtime_output.ready", lang, &[("{ready}", truncate(&ready, 200))]));
    }
    if let Some((block, summary)) = summary_asked(segments) {
        reason.push('\n');
        reason.push_str(&crate::shared::say::say("runtime_output.summary", lang, &[("{block}", block), ("{summary}", summary)]));
    }
    Some(Verdict::Deny { reason })
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger};
    use serde_json::json;

    use super::*;
    use crate::hooks::bash::command_guard::CommandGuard;

    /// O veredito da trava de comandos inteira para `cmd`, na conversa
    /// principal ou num agente.
    fn guard(cmd: &str, agent: bool) -> Verdict {
        let dir = tempfile::tempdir().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": cmd }),
            hook_event_name: Some("PreToolUse".to_string()),
            agent_id: agent.then(|| "wave-agent".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(dir.path().to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        CommandGuard.evaluate(&input, &ctx).unwrap()
    }

    /// Os comandos de 14/10 que cortaram ou filtraram a resposta do Mustard,
    /// e as mesmas formas por substituição, pelo `xargs`, depois do `rtk`, por
    /// um caminho e dentro de `bash -c`, são recusados na conversa principal e
    /// nos agentes. A recusa mostra o comando e o jeito de ler por partes.
    #[test]
    fn a_runtime_answer_piped_into_a_script_is_refused() {
        for cmd in [
            r#"mustard-rt run round --spec falha-fora-da-obra 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin)['next'])""#,
            r#"mustard-rt run panel | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('waves'))""#,
            r#"mustard-rt run read metrics --spec falha-fora-da-obra | python3 -c "import sys; print(sys.stdin.read()[:4000])""#,
            r#"mustard-rt run write step --spec falha-fora-da-obra --json '{"wave":3}' | grep -E '"(ok|id|code)"'"#,
            "mustard-rt run resume | head -80",
            "mustard-rt run read item-2384 --spec falha-fora-da-obra | cut -c1-900",
            "mustard-rt run read request-57 --spec falha-fora-da-obra | sed -n '1,30p'",
            "mustard-rt run read conversation --spec falha-fora-da-obra | head -c 6000",
            r#"jq '.waves' <<< "$(mustard-rt run panel)""#,
            "python3 summarize.py <(mustard-rt run read metrics)",
            "python3 - <<EOF\nprint(len('''$(mustard-rt run panel)'''))\nEOF",
            "mustard-rt run read wave-list | xargs -n1 echo",
            r#"mustard-rt run panel | node -e "process.stdin.on('data', d => console.log(String(d).length))""#,
            "rtk mustard-rt run resume | rtk tail -5",
            "~/.local/bin/mustard-boot run read request-1 | fold -w 3000 | head",
            r#"mustard-rt run write delivered --json "$(cat /tmp/entrega.json)" | grep ok"#,
            r#"bash -c "mustard-rt run panel | jq .""#,
        ] {
            for agent in [false, true] {
                match guard(cmd, agent) {
                    Verdict::Deny { reason } => {
                        assert!(reason.contains(truncate(cmd, 120)), "the refusal shows the command: {reason}");
                        assert!(reason.contains("run read") && reason.contains("--term"), "the refusal shows the right read: {reason}");
                    }
                    other => panic!("{cmd} (agent: {agent}) must be refused, got {other:?}"),
                }
            }
        }
    }

    /// A recusa devolve o comando pronto: a etapa que corta sai, com o
    /// encadeamento dela, a do Mustard grava num arquivo com o nome da etapa
    /// na pasta temporária, e as etapas sem relação ficam como foram escritas.
    /// O `2>&1` vai depois do arquivo. Quando o corte é de um bloco grande, a
    /// recusa diz o bloco de resumo.
    ///
    /// Em campo (21/10/2026) o gancho barrava o comando todo e só dizia o que
    /// fazer: eram 93 bloqueios, e o agente remontava à mão as etapas que não
    /// tinham relação com o motivo.
    #[test]
    fn the_refusal_gives_the_command_back_with_the_answer_in_a_file() {
        let ready = |cmd: &str| match guard(cmd, false) {
            Verdict::Deny { reason } => reason,
            other => panic!("{cmd} must be refused, got {other:?}"),
        };

        let file = |name: &str| std::env::temp_dir().join(format!("{name}.txt")).display().to_string();
        let said = ready("mustard-rt run resume | head -80");
        assert!(said.contains(&format!("mustard-rt run resume > {}", file("mustard-resume"))), "{said}");
        // A etapa sem relação com o motivo fica como foi escrita.
        let said = ready("mustard-rt run read request-1 | head -5 && git log --oneline -3");
        assert!(said.contains(&format!("mustard-rt run read request-1 > {} && git log --oneline -3", file("mustard-read-request-1"))), "{said}");
        // O `2>&1` vai depois do arquivo, senão o erro não entra nele.
        let said = ready("mustard-rt run round --spec x 2>&1 | python3 -c 'x'");
        assert!(said.contains(&format!("mustard-rt run round --spec x > {} 2>&1", file("mustard-round"))), "{said}");
        // O bloco grande tem resumo, e a recusa o diz.
        let said = ready("mustard-rt run read metrics --spec x | head -40");
        assert!(said.contains("run read calls"), "{said}");
        let said = ready("mustard-rt run read waves --spec x | head -40");
        assert!(said.contains("run read wave-list"), "{said}");
        // O bloco sem resumo não ganha sugestão nenhuma.
        let said = ready("mustard-rt run read conversation --spec x | head -40");
        assert!(!said.contains("run read calls") && !said.contains("run read wave-list"), "{said}");
    }

    /// A ajuda e a versão passam, com corte e tudo: a resposta delas é curta,
    /// fixa e não traz nada da obra. O gancho pegava até a ajuda dos comandos.
    #[test]
    fn the_cut_of_the_help_and_of_the_version_passes() {
        for cmd in [
            "mustard-rt run read --help | head -20",
            "mustard-rt --help | grep -n write",
            "mustard-rt --version | head -1",
            "mustard-rt run write note -h | sed -n '1,5p'",
        ] {
            let verdict = guard(cmd, false);
            assert!(!matches!(verdict, Verdict::Deny { .. }), "{cmd} must pass, got {verdict:?}");
        }
    }

    /// A resposta do Mustard que sai de um grupo, entre parênteses, entre
    /// chaves ou de um laço, e vai por `|` a um filtro é recusada na conversa
    /// principal e nos agentes. Com `||` depois do grupo, o filtro não recebe
    /// a resposta, e o comando passa.
    #[test]
    fn a_runtime_answer_piped_out_of_a_group_is_refused() {
        for cmd in [
            "(mustard-rt run panel) | grep x",
            "(mustard-rt run resume; mustard-rt run panel) 2>&1 | head -5",
            "{ mustard-rt run panel; } | grep x",
            "for s in 1 2; do mustard-rt run read request-$s; done | tail -1",
        ] {
            for agent in [false, true] {
                let verdict = guard(cmd, agent);
                assert!(matches!(verdict, Verdict::Deny { .. }), "{cmd} (agent: {agent}) must be refused, got {verdict:?}");
            }
        }
        let verdict = guard("(mustard-rt run panel) || grep x notas.txt", false);
        assert!(!matches!(verdict, Verdict::Deny { .. }), "the `||` must pass, got {verdict:?}");
    }

    /// Gravar a resposta num arquivo, com ou sem os erros juntos, e dobrar as
    /// linhas com `fold` são o jeito de ler por partes uma resposta grande:
    /// passam.
    #[test]
    fn a_runtime_answer_to_a_file_or_fold_passes() {
        for cmd in [
            "mustard-rt run read request-57 --spec falha-fora-da-obra > /tmp/pedido.txt",
            "mustard-rt run panel >> /tmp/painel.txt 2>&1",
            "mustard-rt run read item-2384 --spec falha-fora-da-obra | fold -w 3000",
        ] {
            let verdict = guard(cmd, false);
            assert!(!matches!(verdict, Verdict::Deny { .. }), "{cmd} must pass, got {verdict:?}");
        }
    }

    /// A chamada do Mustard sem encadeamento, a que junta os erros, a que só
    /// recebe a saída de outro programa, o comando que só cita o nome dele e
    /// o corte da saída das ferramentas do projeto, em Rust, TypeScript ou
    /// C#, passam.
    #[test]
    fn a_plain_runtime_call_and_other_pipes_pass() {
        for cmd in [
            "mustard-rt run resume",
            "mustard-rt run round --spec falha-fora-da-obra 2>&1",
            "cargo test -p mustard-rt --lib runtime_output 2>&1 | head -30",
            "npx tsc --noEmit -p tsconfig.json | head -20",
            "dotnet test Api.Tests.csproj | tail -15",
            "cat /tmp/entrega.json | mustard-rt run write delivered --spec falha-fora-da-obra",
            "mustard-rt run read wave-list && grep -n onda notas.txt",
            "echo mustard-rt | wc -c",
        ] {
            let verdict = guard(cmd, false);
            assert!(!matches!(verdict, Verdict::Deny { .. }), "{cmd} must pass, got {verdict:?}");
        }
    }
}
