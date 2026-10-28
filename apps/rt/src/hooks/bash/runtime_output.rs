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
//! A recusa diz o caminho do comando barrado ([`Way`]): o resumo do bloco
//! grande, a leitura por termo ou o comando sozinho. Ela nunca manda gravar a
//! resposta num arquivo. A conferência lê os comandos que
//! [`super::lex::segments`] achou, nunca o texto cru: `echo mustard-rt | wc -c`
//! cita o nome sem rodar o Mustard e passa.

use mustard_core::SupportedLocale;
use mustard_core::domain::model::contract::Verdict;

use super::lex::{Segment, truncate};
use crate::shared::say::say;

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

/// Os comandos do Mustard cuja resposta já vem curta: cortá-la não poupa
/// leitura, e a recusa manda rodar o comando sozinho.
const SHORT_ANSWER: &[&str] = &["round", "close", "map"];

/// O caminho que a recusa aponta para o comando do Mustard barrado. Nenhum
/// deles passa por arquivo: a resposta se lê inteira, na conversa.
#[derive(Debug)]
enum Way {
    /// `run read` de um bloco grande de [`SUMMARY_OF`]: o bloco e o resumo
    /// pronto dele.
    Summary(&'static str, &'static str),
    /// Outro `run read`: a leitura por termo.
    Term,
    /// Um comando de [`SHORT_ANSWER`]: rodar sozinho, que a resposta já vem
    /// curta.
    ShortAlone,
    /// Qualquer outro comando do Mustard: rodar sozinho e ler a resposta
    /// inteira.
    Alone,
}

impl Way {
    /// O caminho do comando do Mustard `segment`, pelas palavras depois do
    /// `run`.
    fn of(segment: &Segment) -> Self {
        let mut words = segment.args.iter().map(|arg| arg.text.as_str()).skip_while(|word| *word != "run").skip(1);
        match words.next() {
            Some("read") => {
                let block = words.next();
                SUMMARY_OF.iter().find(|(big, _)| Some(*big) == block).map_or(Way::Term, |&(big, summary)| Way::Summary(big, summary))
            }
            Some(command) if SHORT_ANSWER.contains(&command) => Way::ShortAlone,
            _ => Way::Alone,
        }
    }

    /// A frase do caminho, no idioma `lang`.
    fn said(&self, lang: SupportedLocale) -> String {
        match *self {
            Way::Summary(block, summary) => say("runtime_output.summary", lang, &[("{block}", block), ("{summary}", summary)]),
            Way::Term => say("runtime_output.term", lang, &[]),
            Way::ShortAlone => say("runtime_output.alone_short", lang, &[]),
            Way::Alone => say("runtime_output.alone", lang, &[]),
        }
    }
}

/// Se `segment` roda o Mustard pedindo uma resposta que se lê inteira: tudo,
/// menos a ajuda e a versão ([`asks_a_fixed_answer`]).
fn reads_whole(segment: &Segment) -> bool {
    runs_the_runtime(segment) && !asks_a_fixed_answer(segment)
}

/// O comando do Mustard cuja resposta passa por script, filtro ou corte: o
/// que a manda, pelo encadeamento de `|`, a um programa de [`SCRIPTS`], ou o
/// que um programa de [`SCRIPTS`] recebe por substituição. `None` quando
/// nenhum passa.
fn barred(segments: &[Segment]) -> Option<&Segment> {
    (0..segments.len()).find_map(|at| {
        if reads_whole(&segments[at]) && feeds_a_script(segments, at) {
            return Some(&segments[at]);
        }
        if !runs_a_script(&segments[at]) {
            return None;
        }
        inside(segments, at).iter().find(|inner| reads_whole(inner))
    })
}

/// A conferência: recusa o comando `cmd`, já lido em `segments`, quando a
/// resposta do Mustard passa por script, filtro ou corte. A recusa, no
/// idioma `lang`, mostra o comando e diz o caminho do comando barrado
/// ([`Way`]); ela nunca manda gravar a resposta num arquivo.
///
/// A ajuda e a versão passam: a resposta delas é curta, fixa e não traz nada
/// da obra. O gancho pegava até `mustard-rt run read --help | head`.
pub(super) fn bash_runtime_output(segments: &[Segment], cmd: &str, lang: SupportedLocale) -> Option<Verdict> {
    let way = Way::of(barred(segments)?);
    let mut reason = say("runtime_output.scripted", lang, &[("{command}", truncate(cmd, 120))]);
    reason.push('\n');
    reason.push_str(&way.said(lang));
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
    /// nos agentes. A recusa mostra o comando e termina no caminho do comando
    /// barrado: o resumo do bloco grande, a leitura por termo nos outros
    /// blocos, ou o comando sozinho, com a razão de resposta curta na rodada,
    /// no fechamento e no mapa.
    #[test]
    fn a_runtime_answer_piped_into_a_script_is_refused() {
        let lang = Ctx::for_test(".", None).config.language().text_or_default();
        for (cmd, way) in [
            (r#"mustard-rt run round --spec falha-fora-da-obra 2>&1 | python3 -c "import json,sys; print(json.load(sys.stdin)['next'])""#, Way::ShortAlone),
            (r#"mustard-rt run panel | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('waves'))""#, Way::Alone),
            (r#"mustard-rt run read metrics --spec falha-fora-da-obra | python3 -c "import sys; print(sys.stdin.read()[:4000])""#, Way::Summary("metrics", "calls")),
            (r#"mustard-rt run write step --spec falha-fora-da-obra --json '{"wave":3}' | grep -E '"(ok|id|code)"'"#, Way::Alone),
            ("mustard-rt run resume | head -80", Way::Alone),
            ("mustard-rt run read item-2384 --spec falha-fora-da-obra | cut -c1-900", Way::Term),
            ("mustard-rt run read request-57 --spec falha-fora-da-obra | sed -n '1,30p'", Way::Term),
            ("mustard-rt run read conversation --spec falha-fora-da-obra | head -c 6000", Way::Term),
            ("mustard-rt run read waves --spec x | head -40", Way::Summary("waves", "wave-list")),
            ("mustard-rt run close --spec x | tail -3", Way::ShortAlone),
            ("mustard-rt run map summary --file src/main.rs | head", Way::ShortAlone),
            (r#"jq '.waves' <<< "$(mustard-rt run panel)""#, Way::Alone),
            ("python3 summarize.py <(mustard-rt run read metrics)", Way::Summary("metrics", "calls")),
            ("python3 - <<EOF\nprint(len('''$(mustard-rt run panel)'''))\nEOF", Way::Alone),
            ("mustard-rt run read wave-list | xargs -n1 echo", Way::Term),
            (r#"mustard-rt run panel | node -e "process.stdin.on('data', d => console.log(String(d).length))""#, Way::Alone),
            ("rtk mustard-rt run resume | rtk tail -5", Way::Alone),
            ("~/.local/bin/mustard-boot run read request-1 | fold -w 3000 | head", Way::Term),
            (r#"mustard-rt run write delivered --json "$(cat /tmp/entrega.json)" | grep ok"#, Way::Alone),
            (r#"bash -c "mustard-rt run panel | jq .""#, Way::Alone),
        ] {
            for agent in [false, true] {
                match guard(cmd, agent) {
                    Verdict::Deny { reason } => {
                        assert!(reason.contains(truncate(cmd, 120)), "the refusal shows the command: {reason}");
                        assert!(reason.ends_with(&way.said(lang)), "{cmd} (agent: {agent}) ends on {way:?}: {reason}");
                    }
                    other => panic!("{cmd} (agent: {agent}) must be refused, got {other:?}"),
                }
            }
        }
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
    /// linhas com `fold` não passam a resposta por script, filtro nem corte:
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
