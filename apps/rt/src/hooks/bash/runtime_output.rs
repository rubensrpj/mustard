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

/// Se `segment` roda o Mustard.
fn runs_the_runtime(segment: &Segment) -> bool {
    RUNTIME.contains(&segment.name())
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

/// A conferência: recusa o comando `cmd`, já lido em `segments`, quando a
/// resposta do Mustard passa por script, filtro ou corte. A recusa, no
/// idioma `lang`, mostra o comando e diz como ler a resposta inteira.
pub(super) fn bash_runtime_output(segments: &[Segment], cmd: &str, lang: SupportedLocale) -> Option<Verdict> {
    let scripted = (0..segments.len()).any(|at| {
        (runs_the_runtime(&segments[at]) && feeds_a_script(segments, at))
            || (runs_a_script(&segments[at]) && inside(segments, at).iter().any(runs_the_runtime))
    });
    scripted.then(|| Verdict::Deny {
        reason: crate::shared::say::say("runtime_output.scripted", lang, &[("{command}", truncate(cmd, 120))]),
    })
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
