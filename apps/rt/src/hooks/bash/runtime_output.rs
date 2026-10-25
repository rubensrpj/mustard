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
use mustard_core::domain::model::contract::{HookInput, Verdict};

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
/// Volta com o comando os arquivos, escritos como estão nele. `None` quando a
/// reescrita não acha as duas etapas — a resposta chega ao script por
/// substituição, e não por encadeamento.
fn with_the_answer_in_a_file(segments: &[Segment], cmd: &str, owner: Option<&str>) -> Option<(String, Vec<String>)> {
    if !segments.iter().any(runs_a_script) {
        return None;
    }
    // A etapa anterior foi uma do Mustard reescrita: só o corte que vem logo
    // depois dela sai. O corte da ajuda, ou de outro programa, fica.
    let cutting = std::cell::Cell::new(false);
    let names = std::cell::RefCell::new(Vec::<String>::new());
    let files = std::cell::RefCell::new(Vec::<String>::new());
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
        let file = answer_file(first, &mut names.borrow_mut(), owner);
        let put = format!("{body} > {file}{errors}");
        files.borrow_mut().push(file);
        Some(super::rewrite::Step::Put(put))
    })?;
    let out = out.trim().to_string();
    (!out.is_empty()).then(|| (out, files.into_inner()))
}

/// O tamanho máximo, em bytes, do comando pronto na recusa. Até ele, o
/// comando volta inteiro e roda como está; acima, o comando de quem pediu já
/// é longo — um JSON grande, o corpo de um heredoc — e repeti-lo inteiro na
/// recusa só gasta a leitura.
const READY_MAX: usize = 600;

/// Os bytes que cada trecho encurtado do comando pronto guarda de cada ponta:
/// o começo diz o comando, e o fim chega ao arquivo.
const READY_EDGE: usize = 40;

/// A marca do pedaço tirado do comando pronto.
const ELLIPSIS: &str = "…";

/// O comando pronto `ready` sem o meio dos trechos longos, ou `None` quando ele
/// já cabe em [`READY_MAX`] bytes. O que passa sai do meio dos trechos entre os
/// arquivos `files`, o mais longo primeiro, cada trecho guardando
/// [`READY_EDGE`] bytes de cada ponta, e cada pedaço tirado vira [`ELLIPSIS`].
/// Os arquivos voltam sempre inteiros, com as aspas: um corte no meio do
/// caminho tirava a aspa do fim e deixava um comando que o terminal não fecha.
///
/// O limite só se alcança quando os trechos entre os arquivos dão o que tirar.
/// Se não bastam, o comando volta mais curto, mas ainda acima de
/// [`READY_MAX`]; se nenhum trecho passa das duas pontas e da marca, volta
/// `None` e o comando segue inteiro, como foi montado.
fn shortened(ready: &str, files: &[String]) -> Option<String> {
    let mut over = ready.len().checked_sub(READY_MAX).filter(|over| *over > 0)?;
    let mut kept: Vec<std::ops::Range<usize>> =
        files.iter().flat_map(|file| ready.match_indices(file.as_str()).map(|(at, found)| at..at + found.len())).collect();
    kept.sort_by_key(|file| file.start);
    let mut stretches = Vec::new();
    let mut at = 0;
    for file in kept {
        if file.start > at {
            stretches.push(at..file.start);
        }
        at = at.max(file.end);
    }
    if ready.len() > at {
        stretches.push(at..ready.len());
    }
    stretches.sort_by_key(|stretch| std::cmp::Reverse(stretch.len()));
    let mut cuts = Vec::new();
    for stretch in stretches {
        let take = (over + ELLIPSIS.len()).min(stretch.len().saturating_sub(2 * READY_EDGE));
        // Os trechos vêm do mais longo: o seguinte tem ainda menos a dar.
        if take <= ELLIPSIS.len() {
            break;
        }
        let mut start = stretch.start + (stretch.len() - take) / 2;
        while !ready.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = start + take;
        while !ready.is_char_boundary(end) {
            end += 1;
        }
        over = over.saturating_sub(end - start - ELLIPSIS.len());
        cuts.push(start..end);
        if over == 0 {
            break;
        }
    }
    if cuts.is_empty() {
        return None;
    }
    cuts.sort_by_key(|cut| cut.start);
    let mut out = String::with_capacity(READY_MAX + cuts.len() * ELLIPSIS.len());
    let mut at = 0;
    for cut in cuts {
        out.push_str(&ready[at..cut.start]);
        out.push_str(ELLIPSIS);
        at = cut.end;
    }
    out.push_str(&ready[at..]);
    Some(out)
}

/// O arquivo em que a etapa do Mustard `segment` grava a resposta: na pasta
/// temporária, fora do repositório, com o nome da etapa — `read-backlog`,
/// `resume` — e quem pediu o comando (`owner`, de [`owner_tag`]). Um arquivo
/// dentro do projeto entrava no commit de quem roda `git add -A`; duas etapas
/// no mesmo arquivo perdiam a primeira resposta; e duas conversas, ou dois
/// agentes da mesma conversa, com a mesma etapa na mesma pasta temporária
/// gravavam uma por cima da outra. O nome que já saiu neste comando (`taken`)
/// ganha um número.
fn answer_file(segment: &Segment, taken: &mut Vec<String>, owner: Option<&str>) -> String {
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
    if let Some(owner) = owner {
        name = format!("{name}-{owner}");
    }
    shell_path(&std::env::temp_dir(), &format!("{name}.txt"))
}

/// Quem pediu o comando, como cabe num nome de arquivo: o id da sessão do host
/// e, dentro de um agente, o id do agente depois dele, separados por `.`. O
/// ponto não aparece em nenhum dos dois ids depois de [`id_tag`], e por isso a
/// junção não confunde a sessão `a` com o agente `b-c` e a sessão `a-b` com o
/// agente `c`. `None` quando o host não mandou nenhum dos dois.
fn owner_tag(input: &HookInput) -> Option<String> {
    let tags: Vec<String> = [input.session_id.as_deref(), input.subagent_id()].into_iter().filter_map(id_tag).collect();
    (!tags.is_empty()).then(|| tags.join("."))
}

/// O id do host como cabe num nome de arquivo: letras, números, `-` e `_`
/// ficam, o resto vira `-`, e o nome para em [`ID_TAG_MAX`] caracteres. `None`
/// quando não há id.
fn id_tag(id: Option<&str>) -> Option<String> {
    let tag: String = id?
        .chars()
        .take(ID_TAG_MAX)
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '-' })
        .collect();
    (!tag.is_empty()).then_some(tag)
}

/// O tamanho máximo de cada id no nome do arquivo.
const ID_TAG_MAX: usize = 64;

/// O caminho de `file` em `dir`, escrito como o terminal o lê: solto quando só
/// tem letras, números, `/`, `.`, `_` e `-`; do contrário, entre aspas simples.
/// O espaço, a letra de unidade (`C:`) e a barra invertida do Windows quebram ou
/// são comidos numa palavra solta, e a aspa simples do nome vira `'\''`.
fn shell_path(dir: &std::path::Path, file: &str) -> String {
    let path = dir.join(file).display().to_string();
    if path.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-')) {
        path
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

/// A conferência: recusa o comando `cmd`, já lido em `segments`, quando a
/// resposta do Mustard passa por script, filtro ou corte. A recusa, no
/// idioma `lang`, mostra o comando, devolve o comando pronto com a resposta
/// num arquivo — inteiro até [`READY_MAX`] bytes, e acima deles sem o meio dos
/// trechos longos, nunca cortando o caminho de um arquivo ([`shortened`]) — e,
/// quando o corte é de um bloco grande, diz o bloco de resumo que cabe
/// inteiro. O arquivo leva o nome da sessão e do agente de `input`.
///
/// A ajuda e a versão passam: a resposta delas é curta, fixa e não traz nada
/// da obra. O gancho pegava até `mustard-rt run read --help | head`.
pub(super) fn bash_runtime_output(segments: &[Segment], cmd: &str, input: &HookInput, lang: SupportedLocale) -> Option<Verdict> {
    let scripted = (0..segments.len()).any(|at| {
        let runtime = runs_the_runtime(&segments[at]) && !asks_a_fixed_answer(&segments[at]);
        (runtime && feeds_a_script(segments, at))
            || (runs_a_script(&segments[at]) && inside(segments, at).iter().any(|inner| runs_the_runtime(inner) && !asks_a_fixed_answer(inner)))
    });
    if !scripted {
        return None;
    }
    let mut reason = crate::shared::say::say("runtime_output.scripted", lang, &[("{command}", truncate(cmd, 120))]);
    if let Some((ready, files)) = with_the_answer_in_a_file(segments, cmd, owner_tag(input).as_deref()) {
        reason.push('\n');
        // O comando encurtado não roda como está, e a frase diz isso.
        let said = match shortened(&ready, &files) {
            Some(short) => crate::shared::say::say("runtime_output.ready_shortened", lang, &[("{ready}", &short)]),
            None => crate::shared::say::say("runtime_output.ready", lang, &[("{ready}", &ready)]),
        };
        reason.push_str(&said);
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
        guard_in(cmd, agent, None)
    }

    /// O mesmo veredito, com a sessão do host que pediu o comando.
    fn guard_in(cmd: &str, agent: bool, session: Option<&str>) -> Verdict {
        let dir = tempfile::tempdir().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": cmd }),
            hook_event_name: Some("PreToolUse".to_string()),
            agent_id: agent.then(|| "wave-agent".to_string()),
            session_id: session.map(str::to_string),
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

        let file = |name: &str| shell_path(&std::env::temp_dir(), &format!("{name}.txt"));
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

    /// O arquivo da resposta volta entre aspas quando o caminho pede: espaço
    /// no nome da pasta, barra invertida e letra de unidade do Windows, aspa
    /// simples dentro do nome. O caminho sem nada disso volta como está.
    #[test]
    fn the_answer_file_is_quoted_when_the_path_needs_it() {
        use std::path::Path;

        // A junção usa o separador do sistema: o esperado se monta pelo mesmo
        // `Path::join`, e não com uma barra fixa.
        let joined = |dir: &str| Path::new(dir).join("mustard-resume.txt").display().to_string();
        // Só o sistema cujo separador é a barra deixa o caminho solto.
        #[cfg(unix)]
        assert_eq!(shell_path(Path::new("/tmp"), "mustard-resume.txt"), "/tmp/mustard-resume.txt");
        assert_eq!(shell_path(Path::new("/tmp/with space"), "mustard-resume.txt"), format!("'{}'", joined("/tmp/with space")));
        let windows_spaced = r"C:\Users\Ana Maria\AppData\Local\Temp";
        assert_eq!(shell_path(Path::new(windows_spaced), "mustard-resume.txt"), format!("'{}'", joined(windows_spaced)));
        // Sem espaço, o caminho do Windows também vai entre aspas: o terminal
        // comeria as barras invertidas de uma palavra solta.
        let windows = shell_path(Path::new(r"C:\Users\ana\AppData\Local\Temp"), "mustard-resume.txt");
        assert!(windows.starts_with('\'') && windows.ends_with('\''), "{windows}");
        assert_eq!(
            shell_path(Path::new("/tmp/o'brien"), "a.txt"),
            format!("'{}'", Path::new("/tmp/o'brien").join("a.txt").display().to_string().replace('\'', r"'\''"))
        );
    }

    /// Duas conversas que cortam o mesmo comando do Mustard ao mesmo tempo
    /// não gravam no mesmo arquivo: o id da sessão que pediu o comando entra
    /// no nome.
    #[test]
    fn the_answer_file_is_named_by_conversation() {
        let ready = |cmd: &str, session: Option<&str>| match guard_in(cmd, false, session) {
            Verdict::Deny { reason } => reason,
            other => panic!("{cmd} must be refused, got {other:?}"),
        };
        let file = |name: &str| shell_path(&std::env::temp_dir(), &format!("{name}.txt"));

        let cmd = "mustard-rt run resume | head -80";
        let first = ready(cmd, Some("sessao-a"));
        let second = ready(cmd, Some("sessao-b"));
        assert!(first.contains(&format!("mustard-rt run resume > {}", file("mustard-resume-sessao-a"))), "{first}");
        assert!(second.contains(&format!("mustard-rt run resume > {}", file("mustard-resume-sessao-b"))), "{second}");
        assert!(!first.contains("sessao-b") && !second.contains("sessao-a"), "each refusal names only its own conversation");

        // O id vira nome de arquivo: nada que o terminal leia como outra coisa.
        let said = ready(cmd, Some("../x y;rm"));
        assert!(said.contains(&format!("mustard-rt run resume > {}", file("mustard-resume----x-y-rm"))), "{said}");
        // Sem id, o nome é o da etapa, como antes.
        let said = ready(cmd, Some(""));
        assert!(said.contains(&format!("mustard-rt run resume > {}", file("mustard-resume"))), "{said}");
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

    /// O arquivo da resposta como o comando pronto o escreve.
    fn answer(name: &str) -> String {
        format!("/tmp/mustard-{name}.txt")
    }

    /// O comando pronto de exatamente `len` bytes: um `echo` com o enchimento e
    /// a resposta do Mustard gravada em `file`.
    fn ready_of_len(len: usize, file: &str) -> String {
        let fixed = "echo  && mustard-rt run resume > ".len() + file.len();
        format!("echo {} && mustard-rt run resume > {file}", "x".repeat(len - fixed))
    }

    /// O comando pronto que cabe em 600 bytes volta como está, e o que passa
    /// um byte só do limite perde o necessário para chegar nele.
    #[test]
    fn a_ready_command_is_shortened_only_above_the_maximum() {
        let files = [answer("resume")];
        assert_eq!(shortened(&ready_of_len(READY_MAX - 1, &files[0]), &files), None);
        assert_eq!(shortened(&ready_of_len(READY_MAX, &files[0]), &files), None);

        let ready = ready_of_len(READY_MAX + 1, &files[0]);
        let short = shortened(&ready, &files).expect("one byte above the maximum is shortened");
        assert_eq!(short.len(), READY_MAX, "{short}");
        assert_eq!(short.matches(ELLIPSIS).count(), 1, "{short}");
        assert!(short.ends_with(&format!("> {}", files[0])), "the file stays whole: {short}");
    }

    /// O texto acentuado — letras de dois, três e quatro bytes — acima de 600
    /// bytes é encurtado entre as letras, nunca no meio de uma, onde quer que o
    /// corte caia no trecho: o começo e o fim do trecho ficam, o arquivo volta
    /// inteiro e o resultado cabe no limite.
    #[test]
    fn an_accented_ready_command_is_shortened_between_letters() {
        let files = [answer("resume")];
        for lead in 0..10 {
            for trail in 0..10 {
                let note = format!("{}{}{}", "x".repeat(lead), "aç€😀".repeat(70), "y".repeat(trail));
                let ready = format!("echo '{note}' && mustard-rt run resume > {} && echo ação", files[0]);
                assert!(ready.len() > READY_MAX, "the command must need the cut: {} bytes", ready.len());

                let short = shortened(&ready, &files).unwrap_or_else(|| panic!("lead {lead}, trail {trail}: {ready}"));
                assert!(short.len() <= READY_MAX, "lead {lead}, trail {trail}: {} bytes: {short}", short.len());
                let (head, tail) = short.split_once(ELLIPSIS).unwrap_or_else(|| panic!("the cut is marked: {short}"));
                assert!(!tail.contains(ELLIPSIS), "one cut only: {short}");
                assert!(ready.starts_with(head) && ready.ends_with(tail), "only the middle goes: {short}");
                assert!(head.len() >= READY_EDGE - 3 && head.starts_with("echo '"), "the start stays: {short}");
                assert!(tail.contains(&format!("> {} && echo ação", files[0])), "the file and the rest stay whole: {short}");
            }
        }
    }

    /// Com dois arquivos no comando, os dois voltam inteiros e na ordem em que
    /// estavam; o corte sai do meio dos dois trechos longos, e o começo e o
    /// fim do comando ficam.
    #[test]
    fn a_ready_command_with_two_files_keeps_both_whole() {
        let files = [format!("/tmp/{}-resume.txt", "a".repeat(80)), format!("/tmp/{}-panel.txt", "b".repeat(80))];
        let ready = format!(
            "echo {} && mustard-rt run resume > {} && echo {} && mustard-rt run panel > {}",
            "x".repeat(350),
            files[0],
            "y".repeat(350),
            files[1]
        );

        let short = shortened(&ready, &files).expect("the command is over the maximum");
        assert!(short.len() <= READY_MAX, "{} bytes: {short}", short.len());
        assert_eq!(short.matches(ELLIPSIS).count(), 2, "one cut in each long stretch: {short}");
        let (first, second) = (short.find(&files[0]).expect("first file whole"), short.find(&files[1]).expect("second file whole"));
        assert!(first < second, "the files keep their order: {short}");
        assert!(short.starts_with("echo xxx") && short.ends_with(&files[1]), "the ends stay: {short}");
    }

    /// Trechos curtos demais entre os arquivos não dão o que tirar: sem
    /// nenhum trecho além das duas pontas e da marca, o comando fica como foi
    /// montado; com um trecho que dá menos do que falta, ele volta mais curto,
    /// mas ainda acima do limite, e os arquivos seguem inteiros.
    #[test]
    fn short_stretches_between_files_do_not_reach_the_maximum() {
        // Quatro arquivos de 200 bytes, separados por quatro bytes cada.
        let files: Vec<String> = ["a", "b", "c", "d"].iter().map(|c| c.repeat(200)).collect();
        assert_eq!(shortened(&files.join(" && "), &files), None, "stretches of four bytes give nothing");

        // O trecho de 83 bytes é as duas pontas e a marca: não tem meio.
        let edges_only = ["f".repeat(520)];
        let ready = format!("{}{}", "x".repeat(2 * READY_EDGE + ELLIPSIS.len()), edges_only[0]);
        assert_eq!(shortened(&ready, &edges_only), None);

        // O comando passa 120 bytes do limite, e o trecho de 120 bytes dá 40 do
        // meio, que valem 37 depois da marca: ainda passam 83 bytes do limite.
        let long_file = ["f".repeat(600)];
        let ready = format!("{}{}", "x".repeat(120), long_file[0]);
        let short = shortened(&ready, &long_file).expect("the stretch gives something");
        assert_eq!(short.len(), ready.len() - 37, "{short}");
        assert!(short.len() > READY_MAX && short.ends_with(&long_file[0]), "above the maximum, with the file whole: {short}");
    }
}
