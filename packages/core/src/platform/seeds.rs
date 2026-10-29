//! `seeds` — the bundled project-seed payload, compiled into the binary.
//!
//! ## Why these live in the core
//!
//! The files Mustard lays down in a project (`.claude/settings.json`, the
//! session map under `.claude/mustard/`, the two agents under
//! `.claude/agents/mustard/` and the `.claude/.gitignore`) used to ship only as loose files under
//! `apps/cli/templates/`, reachable solely by the `mustard` CLI through a
//! `templates/` directory lookup. That made the CLI the only possible
//! installer: `mustard-rt` (the plugin's binary) had no way to seed a project.
//!
//! Moving the files to `packages/core/templates/` and embedding them with
//! `include_str!` makes the core the single source of truth: both the CLI
//! (`mustard init`) and the runtime (`mustard-rt run upsert`) consume the same
//! constants, and no installed-layout `templates/` directory is required:
//! the installer looks up no folder of molds at all.
//!
//! The seeding logic that consumes these constants lives in
//! [`crate::platform::project_seed`].

use std::fmt::Write as _;
use std::sync::LazyLock;

use crate::domain::config::AgentSettings;
use crate::platform::i18n::Locale;

/// The reduced `.claude/settings.json` seed: env / permissions / statusLine /
/// plansDirectory. Plugin enablement is deliberately absent (a user-scope
/// choice — see `project_seed::retire_planted_plugin_enablement`).
pub const SETTINGS_SEED: &str = include_str!("../../templates/settings.json");

/// O nome do mapa do início da sessão, em `.claude/mustard/`. É o único texto
/// que o início da sessão coloca, e o nome não muda com o idioma: a
/// declaração do `mustard.json` segue valendo quando o `language.text` muda.
/// O nome é em inglês, como o dos outros arquivos do Mustard; o nome antigo,
/// em português, é trocado na atualização (veja `project_seed::files`).
pub const SESSION_MAP_NAME: &str = "session-map.md";

const SESSION_MAP_PT_BR: &str = include_str!("../../templates/mustard/pt-BR/session-map.md");
const SESSION_MAP_EN_US: &str = include_str!("../../templates/mustard/en-US/session-map.md");

/// A linha do molde do mapa que dá lugar aos princípios do condutor.
const PRINCIPLES_SLOT: &str = "{principles}";

/// O mapa de cada idioma, montado uma vez: o molde com os princípios do
/// condutor no lugar de [`PRINCIPLES_SLOT`].
static SESSION_MAPS: LazyLock<[String; 2]> = LazyLock::new(|| {
    [Locale::PtBr, Locale::EnUs].map(|text| {
        let template = match text {
            Locale::PtBr => SESSION_MAP_PT_BR,
            Locale::EnUs => SESSION_MAP_EN_US,
        };
        template.replacen(PRINCIPLES_SLOT, principles_for(text, Reader::Conductor, 2).trim_end(), 1)
    })
});

/// O mapa do início da sessão no idioma `text`: o que o Mustard faz, quando
/// uma spec abre, onde cada coisa mora e os princípios do condutor. Os dois
/// idiomas são molde do produto; o projeto recebe só o do `language.text`.
#[must_use]
pub fn session_map(text: Locale) -> &'static str {
    match text {
        Locale::PtBr => &SESSION_MAPS[0],
        Locale::EnUs => &SESSION_MAPS[1],
    }
}

const PRINCIPLES_PT_BR: &str = include_str!("../../templates/mustard/pt-BR/principles.md");
const PRINCIPLES_EN_US: &str = include_str!("../../templates/mustard/en-US/principles.md");

/// Quem recebe um princípio do produto: quem conduz a sessão, o agente de
/// onda ou o revisor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reader {
    Conductor,
    Wave,
    Review,
}

/// Um princípio do produto, como o `principles.md` do idioma o escreve: o
/// título, o texto de uma frase, quem o recebe e o teste que o cobra, ou
/// `None` no princípio que é só texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principle {
    pub title: &'static str,
    pub text: &'static str,
    pub readers: Vec<Reader>,
    pub test: Option<&'static str>,
}

/// As palavras do `principles.md` de um idioma: os rótulos dos campos, o
/// nome de cada destinatário e o valor da cobrança que é só texto.
struct Labels {
    readers: &'static str,
    enforcement: &'static str,
    test: &'static str,
    text_only: &'static str,
    names: [(&'static str, Reader); 3],
}

fn labels(text: Locale) -> Labels {
    match text {
        Locale::PtBr => Labels {
            readers: "Para",
            enforcement: "Cobrança",
            test: "Teste",
            text_only: "só texto",
            names: [("condutor", Reader::Conductor), ("onda", Reader::Wave), ("revisão", Reader::Review)],
        },
        Locale::EnUs => Labels {
            readers: "For",
            enforcement: "Enforcement",
            test: "Test",
            text_only: "text only",
            names: [("conductor", Reader::Conductor), ("wave", Reader::Wave), ("review", Reader::Review)],
        },
    }
}

/// O `principles.md` embutido do idioma `text`.
fn principles_source(text: Locale) -> &'static str {
    match text {
        Locale::PtBr => PRINCIPLES_PT_BR,
        Locale::EnUs => PRINCIPLES_EN_US,
    }
}

/// Os princípios de `body`, escrito no idioma `text`: cada `## ` abre um, o
/// primeiro parágrafo é o texto, e as linhas `- Rótulo: valor` dizem quem o
/// recebe e como ele é cobrado. Falta de campo, destinatário desconhecido,
/// cobrança sem teste ou teste no princípio só de texto é erro, com o título.
fn parse_principles(body: &'static str, text: Locale) -> Result<Vec<Principle>, String> {
    let words = labels(text);
    let mut out = Vec::new();
    for section in body.split("\n## ").skip(1) {
        let (title, rest) = section.split_once('\n').unwrap_or((section, ""));
        let title = title.trim();
        let mut paragraph: Vec<&'static str> = Vec::new();
        let (mut readers, mut enforcement, mut test) = (None, None, None);
        for line in rest.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let Some((label, value)) = line.strip_prefix("- ").and_then(|field| field.split_once(": ")) else {
                paragraph.push(line);
                continue;
            };
            let value = value.trim();
            if label == words.readers {
                let mut named = Vec::new();
                for name in value.split(',').map(str::trim) {
                    let reader = words.names.iter().find(|(word, _)| *word == name).map(|(_, reader)| *reader);
                    named.push(reader.ok_or_else(|| format!("`{title}`: unknown reader `{name}`"))?);
                }
                readers = Some(named);
            } else if label == words.enforcement {
                enforcement = Some(value);
            } else if label == words.test {
                test = Some(value.trim_matches('`'));
            } else {
                return Err(format!("`{title}`: unknown field `{label}`"));
            }
        }
        let readers = readers.ok_or_else(|| format!("`{title}`: no `{}` line", words.readers))?;
        let enforcement = enforcement.ok_or_else(|| format!("`{title}`: no `{}` line", words.enforcement))?;
        let text_only = enforcement == words.text_only;
        if text_only == test.is_some() {
            return Err(format!("`{title}`: `{}: {enforcement}` and the test {test:?} disagree", words.enforcement));
        }
        let [text] = paragraph[..] else {
            return Err(format!("`{title}`: the text is not one paragraph line: {paragraph:?}"));
        };
        out.push(Principle { title, text, readers, test });
    }
    Ok(out)
}

/// Os princípios de cada idioma, lidos uma vez. O arquivo embutido que não
/// se lê não tira a sessão do ar: os testes o recusam antes de ele sair.
static PRINCIPLES: LazyLock<[Vec<Principle>; 2]> = LazyLock::new(|| {
    [Locale::PtBr, Locale::EnUs].map(|text| parse_principles(principles_source(text), text).unwrap_or_default())
});

/// Os princípios do produto no idioma `text`, na ordem do arquivo.
#[must_use]
pub fn principles(text: Locale) -> &'static [Principle] {
    match text {
        Locale::PtBr => &PRINCIPLES[0],
        Locale::EnUs => &PRINCIPLES[1],
    }
}

/// O bloco dos princípios que `reader` recebe, no idioma `text`: o título do
/// arquivo num cabeçalho de `level` cerquilhas e um item por princípio. O
/// condutor recebe o título e a frase; o agente de onda e o revisor recebem
/// também o teste que cobra o princípio.
#[must_use]
pub fn principles_for(text: Locale, reader: Reader, level: usize) -> String {
    let heading = principles_source(text).lines().next().and_then(|line| line.strip_prefix("# ")).unwrap_or_default();
    let mut out = format!("{} {heading}\n\n", "#".repeat(level));
    for principle in principles(text).iter().filter(|principle| principle.readers.contains(&reader)) {
        let _ = write!(out, "- **{}.** {}", principle.title, principle.text);
        if let Some(test) = principle.test.filter(|_| reader != Reader::Conductor) {
            let _ = write!(out, " {}: `{test}`.", labels(text).test);
        }
        out.push('\n');
    }
    out
}

/// Os nomes dos dois agentes do Mustard: o de onda, que recebe toda onda,
/// de uma tarefa ou de várias, e o que revisa. O nome do arquivo é o nome
/// com `.md`.
pub const AGENT_NAMES: [&str; 2] = ["wave", "review"];

const AGENTS_PT_BR: [&str; 2] = [
    include_str!("../../templates/agents/pt-BR/wave.md"),
    include_str!("../../templates/agents/pt-BR/review.md"),
];
const AGENTS_EN_US: [&str; 2] = [
    include_str!("../../templates/agents/en-US/wave.md"),
    include_str!("../../templates/agents/en-US/review.md"),
];

/// O texto de cada agente no idioma `text`, na ordem de [`AGENT_NAMES`]:
/// `(nome, corpo)`. Os dois idiomas são molde do produto; o projeto recebe
/// só os dois do `language.text`. `wave` fica no índice 0 e `review` no
/// índice 1, como o resto do código já assume.
#[must_use]
pub fn agent_texts(text: Locale) -> [(&'static str, &'static str); 2] {
    let bodies = match text {
        Locale::PtBr => AGENTS_PT_BR,
        Locale::EnUs => AGENTS_EN_US,
    };
    [(AGENT_NAMES[0], bodies[0]), (AGENT_NAMES[1], bodies[1])]
}

pub fn analysis_agent_text(text: Locale) -> &'static str {
    match text {
        Locale::PtBr => include_str!("../../templates/agents/pt-BR/analysis.md"),
        Locale::EnUs => include_str!("../../templates/agents/en-US/analysis.md"),
    }
}

/// O texto de um agente com as linhas `model:` e `effort:` do cabeçalho
/// trocadas pelas de `settings`: o molde traz os padrões, e a instalação
/// escreve no projeto o que o `mustard.json` declara em `agents`. Só o
/// cabeçalho, entre os dois `---` do começo, é lido; o corpo passa como está.
/// A linha que falta no cabeçalho entra antes do `---` que o fecha. Um texto
/// sem cabeçalho volta igual.
#[must_use]
pub fn with_agent_settings(body: &str, settings: AgentSettings<'_>) -> String {
    let wanted = [("model", settings.model), ("effort", settings.effort)];
    let mut out = String::with_capacity(body.len() + 32);
    let mut lines = body.split_inclusive('\n');
    match lines.next() {
        Some(first) if first.trim_end() == "---" => out.push_str(first),
        _ => return body.to_string(),
    }
    let mut placed = [false; 2];
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            for (at, (key, value)) in wanted.iter().enumerate() {
                if !placed[at] {
                    push_setting(&mut out, key, value);
                }
            }
            out.push_str(line);
            closed = true;
            break;
        }
        match wanted.iter().position(|(key, _)| line.strip_prefix(key).is_some_and(|rest| rest.starts_with(':'))) {
            Some(at) if !placed[at] => {
                push_setting(&mut out, wanted[at].0, wanted[at].1);
                placed[at] = true;
            }
            _ => out.push_str(line),
        }
    }
    if !closed {
        return body.to_string();
    }
    out.extend(lines);
    out
}

/// Acrescenta a linha `key: value` ao cabeçalho que `out` monta.
fn push_setting(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

/// The `.claude/.gitignore` seed covering the ephemeral harness state
/// (caches, pipeline states, per-spec event logs, worktrees).
pub const CLAUDE_GITIGNORE: &str = include_str!("../../templates/.gitignore");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::{DEFAULT_AGENT_EFFORT, DEFAULT_AGENT_MODEL, FINAL_REVIEW_MODEL, REVIEW_AGENT};

    /// Os moldes embutidos não estão vazios e cada um abre como deve: um
    /// caminho de `include_str!` quebrado falha a compilação, mas um molde
    /// esvaziado ou trocado de lugar semearia silêncio.
    #[test]
    fn seeds_carry_their_identifying_content() {
        let settings: serde_json::Value =
            serde_json::from_str(SETTINGS_SEED).expect("settings seed is valid JSON");
        assert!(settings.get("permissions").is_some(), "settings seed has permissions");
        assert!(settings.get("statusLine").is_some(), "settings seed has statusLine");

        for text in [Locale::PtBr, Locale::EnUs] {
            assert!(session_map(text).starts_with("# "), "the {text} session map opens with its title");
            for (name, body) in agent_texts(text) {
                assert!(
                    body.starts_with(&format!("---\nname: mustard-{name}\n")),
                    "the {text} `{name}` agent does not open with its own name",
                );
            }
        }
        assert_ne!(session_map(Locale::PtBr), session_map(Locale::EnUs), "each language has its own map");

        assert!(CLAUDE_GITIGNORE.contains(".events/"), "gitignore covers the event logs");
    }

    /// As palavras que, num molde, só aparecem quando ele fixa o idioma de
    /// alguma coisa: o idioma dos nomes e do texto vem do cabeçalho de cada
    /// pedido, lido da configuração do projeto, nunca do molde.
    const LANGUAGE_WORDS: [&str; 6] = ["inglês", "ingles", "english", "português", "portugues", "portuguese"];

    /// Cada linha de `body` que cita um idioma pelo nome, como
    /// `caminho:linha: texto`, com a linha contada a partir de 1.
    fn lines_fixing_a_language(path: &str, body: &str) -> Vec<String> {
        body.lines()
            .enumerate()
            .filter(|(_, line)| {
                let lower = line.to_lowercase();
                LANGUAGE_WORDS.iter().any(|word| lower.contains(word))
            })
            .map(|(index, line)| format!("{path}:{}: {}", index + 1, line.trim()))
            .collect()
    }

    /// Todo arquivo sob `dir`, recursivo, com o caminho relativo a `root`
    /// escrito com `/`.
    fn files_under(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("reading {}: {err}", dir.display()))
            .map(|entry| entry.expect("a readable folder entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                files_under(root, &path, out);
                continue;
            }
            let relative = path.strip_prefix(root).expect("the file lives under the templates folder");
            let label = relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
            out.push((label, String::from_utf8_lossy(&bytes).into_owned()));
        }
    }

    /// Nenhum molde que o produto grava no projeto fixa o idioma dos nomes
    /// ou do texto: quem diz os dois idiomas é o cabeçalho do pedido, lido
    /// da configuração. O teste percorre todos os moldes, sem exceção, e
    /// aponta cada linha que cita um idioma pelo nome, com arquivo e linha.
    #[test]
    fn no_template_fixes_the_language_of_names() {
        let old = "- Comentários seguem o idioma do projeto e, como o nome de teste, descrevem o comportamento sem citar \
                   código de item, onda, spec, pendência ou Mustard; nomes, comandos e chaves ficam em inglês.";
        assert_eq!(
            lines_fixing_a_language("agents/pt-BR/wave.md", &format!("---\n{old}\n")),
            vec![format!("agents/pt-BR/wave.md:2: {old}")],
            "the sentence that keeps names in English is not pointed out",
        );
        assert_eq!(
            lines_fixing_a_language("agents/en-US/review.md", "Text in the project's text language; code and names in English."),
            vec!["agents/en-US/review.md:1: Text in the project's text language; code and names in English.".to_string()],
            "the English sentence that fixes the names is not pointed out",
        );

        let templates = crate::manifest_dir::manifest_dir().join("templates");
        let mut files = Vec::new();
        files_under(&templates, &templates, &mut files);
        assert!(
            files.iter().any(|(label, _)| label == "agents/pt-BR/wave.md"),
            "the walk did not reach the agent templates: {:?}",
            files.iter().map(|(label, _)| label).collect::<Vec<_>>(),
        );
        let fixed: Vec<String> = files.iter().flat_map(|(label, body)| lines_fixing_a_language(label, body)).collect();
        assert!(fixed.is_empty(), "templates that fix a language instead of following the request's header:\n{}", fixed.join("\n"));
    }

    /// O mapa não manda mais passar toda execução de código a um agente: quem
    /// diz quem executa é a resposta do plano, pela soma das notas. A
    /// delegação da investigação que abre muitos arquivos continua.
    #[test]
    fn the_session_map_no_longer_delegates_every_code_run() {
        for (text, code_run, investigation) in [
            (Locale::PtBr, "toda execução de código", "Delegue só se compensar"),
            (Locale::EnUs, "every code run", "only useful bounded investigation"),
        ] {
            let map = session_map(text);
            assert!(!map.contains(code_run), "the {text} map still hands code execution to an agent: {map}");
            assert!(map.contains(investigation), "the {text} map lost the investigation delegation: {map}");
        }
    }

    /// A revisão de onda chega como resumo, com o diff a pedido, e o aceite
    /// vai na rodada sem copiar a impressão nem escrever a linha de uso.
    #[test]
    fn the_session_map_accepts_the_review_without_fingerprint_or_usage() {
        for text in [Locale::PtBr, Locale::EnUs] {
            let map = session_map(text);
            let line = map.lines().find(|line| line.contains("review-wave")).unwrap_or_else(|| panic!("the {text} map lost the review: {map}"));
            assert!(line.contains("--diff") && line.contains(r#"<REVIEWED>{"wave":<n>,"note":"…"}</REVIEWED>"#), "{text}: {line}");
            assert!(!line.contains("fingerprint") && !line.contains("USAGE"), "{text}: {line}");
        }
    }

    /// O pedido novo numa spec fechada, ou com o pull request aberto, passa
    /// antes pela reabertura, e só depois entra pelo `write request`: sem
    /// ela, a gravação é recusada e o assistente propõe uma spec nova em
    /// outra branch. O pull request que o servidor reprovou vai à porta de
    /// conserto, pedida pelo nome; o pedido novo nunca vai a ela. Os dois
    /// mapas dizem as duas coisas.
    #[test]
    fn the_session_map_sends_a_request_on_a_closed_spec_through_the_reopen() {
        for (text, closed, pr_open, first, server_failed) in [
            (Locale::PtBr, "spec fechada", "pull request aberto", "vem antes", "reprovado pelo servidor"),
            (Locale::EnUs, "closed spec", "pull request open", "comes first", "the server failed"),
        ] {
            let map = session_map(text);
            let sentence_with = |needle: &str| {
                map.lines().flat_map(|line| line.split(". ")).find(|sentence| sentence.contains(needle))
            };

            let request = sentence_with(closed)
                .unwrap_or_else(|| panic!("the {text} map says nothing of a request on a closed spec: {map}"));
            assert!(request.contains("`write request`"), "the {text} map does not say where the request is recorded: {request}");
            assert!(request.contains(pr_open), "the {text} map leaves out the spec with the pull request open: {request}");
            let reopen = request
                .find("`mustard-rt run reopen --reason")
                .unwrap_or_else(|| panic!("the {text} map does not send the request through the reopen: {request}"));
            assert!(request[reopen..].contains(first), "the {text} map does not put the reopen before the request: {request}");
            assert!(!request.contains("--fix"), "the {text} map sends a new request to the fix door: {request}");

            let fix = sentence_with(server_failed)
                .unwrap_or_else(|| panic!("the {text} map says nothing of a pull request the server failed: {map}"));
            assert!(fix.contains("`mustard-rt run reopen --fix"), "the {text} map does not send the failed pull request to the fix: {fix}");
        }
    }

    /// O molde de onda, nos dois idiomas, não pede mais um relatório pelo
    /// tamanho: a entrega vai gravada na spec pela ferramenta, e todo o
    /// detalhe do trabalho vai no campo de texto da entrega. A ordem de a
    /// última mensagem só dizer que gravou mora no pedido. Ele não ensina mais
    /// a linha colada.
    #[test]
    fn the_wave_template_records_the_delivery_without_a_report_by_size() {
        for (text, size_report, recorded) in [
            (Locale::PtBr, "entre mil e dois mil tokens", "`run write delivered --json"),
            (Locale::EnUs, "between one and two thousand tokens", "`run write delivered --json"),
        ] {
            for (name, body) in agent_texts(text) {
                if name != "wave" {
                    continue;
                }
                let lower = body.to_lowercase();
                assert!(!lower.contains(size_report), "the {text} `{name}` agent still asks for a report by size: {body}");
                assert!(body.contains(recorded), "the {text} `{name}` agent does not record the delivery: {body}");
                assert!(body.contains("`text`"), "the {text} `{name}` agent does not send the work's detail to the delivery's text: {body}");
                assert!(!body.contains("<DELIVERED>"), "the {text} `{name}` agent still teaches the pasted line: {body}");
            }
        }
    }

    /// O pedido de outro assunto, no meio de uma spec, continua virando
    /// pendência; na mesma linha, depois da porta da pendência, o mapa diz
    /// que, se o usuário quiser fazer já, o assistente sugere abrir outra
    /// conversa para esse pedido, em vez de fazê-lo na conversa da spec.
    #[test]
    fn the_session_map_sends_another_subject_to_its_own_conversation() {
        for (text, now, other_conversation) in [
            (Locale::PtBr, "quiser fazer já", "outra conversa"),
            (Locale::EnUs, "wants it done now", "another conversation"),
        ] {
            let map = session_map(text);
            let door = "`mustard-rt run pending --add`";
            let line = map
                .lines()
                .find(|line| line.contains(door))
                .unwrap_or_else(|| panic!("the {text} map no longer records a different subject as a pending item: {map}"));
            let after = &line[line.find(door).unwrap_or_default() + door.len()..];
            let suggestion = after
                .find(other_conversation)
                .unwrap_or_else(|| panic!("the {text} map does not suggest another conversation after the pending item: {line}"));
            assert!(after[..suggestion].contains(now), "the {text} map does not tie the other conversation to doing it now: {line}");
        }
    }

    /// Aberta a spec, o mapa manda sugerir ao usuário limpar a conversa com
    /// `/clear` só depois de gravado o objetivo, o primeiro `context`, pelo
    /// `run write context`: antes
    /// disso a conversa ainda tem a mensagem do usuário, que o objetivo aponta
    /// como origem. A mesma frase diz que a linha de retomada mostra onde a
    /// spec está. A sugestão mora num lugar só: a seção da retomada não fala
    /// mais em `/clear`, e a resposta do `run open` também não.
    #[test]
    fn the_session_map_suggests_a_clean_conversation_once_the_goal_is_recorded() {
        for (text, goal, resume_line, resume_heading) in [
            (Locale::PtBr, "objetivo", "linha de retomada", "## Retomar"),
            (Locale::EnUs, "goal", "resume line", "## Resuming"),
        ] {
            let map = session_map(text);
            assert_eq!(map.matches("/clear").count(), 1, "the {text} map should suggest `/clear` in one place only: {map}");
            let line = map.lines().find(|line| line.contains("`/clear`")).unwrap_or_default();
            let open = line
                .find("`mustard-rt run open`")
                .unwrap_or_else(|| panic!("the {text} map does not suggest `/clear` beside the spec opening: {map}"));
            let clear = line.find("`/clear`").unwrap_or_default();
            let before = line.get(open..clear).unwrap_or_default();
            assert!(before.contains("`mustard-rt run write context`"), "the {text} map suggests `/clear` before the first `context` is recorded: {line}");
            assert!(before.contains(goal), "the {text} map does not name the goal as what is recorded first: {line}");
            assert!(line[clear..].contains(resume_line), "the {text} map does not say the resume line shows where the spec stands: {line}");

            let resume = &map[map.find(resume_heading).unwrap_or_else(|| panic!("the {text} map lost its resume section: {map}"))..];
            assert!(!resume.contains("/clear"), "the {text} resume section still speaks of `/clear`: {resume}");
            let next_goal = crate::platform::i18n::translate("open.next_goal", text);
            assert!(!next_goal.contains("/clear"), "the {text} answer of `run open` repeats the `/clear` suggestion: {next_goal}");
        }
    }

    /// Na linha que delega ao agente a investigação que abre muitos arquivos,
    /// o mapa diz que o agente de onda grava a entrega pelo `write delivered`,
    /// põe nela o erro e a sobra, em `leftovers`, e volta com duas linhas,
    /// nessa ordem.
    #[test]
    fn the_session_map_asks_the_wave_agent_to_record_its_delivery_and_come_back_in_two_lines() {
        for (text, investigation, two_lines) in [
            (Locale::PtBr, "Delegue só se compensar", "duas linhas"),
            (Locale::EnUs, "only useful bounded investigation", "two lines"),
        ] {
            let map = session_map(text);
            let line = map
                .lines()
                .find(|line| line.contains(investigation))
                .unwrap_or_else(|| panic!("the {text} map lost the investigation delegation: {map}"));
            let delivered = line
                .find("`write delivered`")
                .unwrap_or_else(|| panic!("the {text} map does not ask the wave agent to record its delivery: {line}"));
            let leftovers = line[delivered..]
                .find("`leftovers`")
                .map(|at| delivered + at)
                .unwrap_or_else(|| panic!("the {text} map does not send errors and leftovers to the delivery: {line}"));
            assert!(line[leftovers..].contains(two_lines), "the {text} map does not ask the agent to come back in two lines: {line}");
        }
    }

    /// O JSON longo vai ao binário por arquivo: na mesma linha, os dois mapas
    /// mandam gravar um arquivo temporário com a ferramenta de escrita e
    /// passá-lo por `--json @arquivo`, que vale também no `--facts`, sem
    /// script ao lado.
    #[test]
    fn the_session_map_sends_a_long_json_through_a_temporary_file() {
        for (text, by_file, temporary, no_script) in [
            (Locale::PtBr, "`--json @arquivo`", "temporário", "sem script"),
            (Locale::EnUs, "`--json @file`", "temp file", "no script"),
        ] {
            let map = session_map(text);
            let line = map
                .lines()
                .find(|line| line.contains(by_file))
                .unwrap_or_else(|| panic!("the {text} map never passes a JSON by file: {map}"));
            for said in ["Write", temporary, "`--facts`", no_script] {
                assert!(line.contains(said), "the {text} map does not say `{said}` beside {by_file}: {line}");
            }
        }
    }

    /// Os dois idiomas trazem os mesmos princípios, na mesma ordem: cada um
    /// com título e texto de uma frase, os mesmos destinatários e o mesmo
    /// teste de cobrança. O condutor recebe todos, e o mapa do início da
    /// sessão leva cada um com o título e a frase.
    #[test]
    fn both_languages_carry_the_same_principles() {
        let [pt, en] = [Locale::PtBr, Locale::EnUs].map(|text| {
            parse_principles(principles_source(text), text).unwrap_or_else(|err| panic!("the {text} principles do not read: {err}"))
        });
        assert_eq!(pt.len(), en.len(), "the two languages carry different counts of principles");
        assert!(pt.len() >= 18, "the principles file lost principles: {pt:?}");
        for (left, right) in pt.iter().zip(&en) {
            assert_eq!((&left.readers, left.test), (&right.readers, right.test), "`{}` and `{}` differ", left.title, right.title);
            for principle in [left, right] {
                assert!(!principle.title.is_empty() && principle.text.ends_with('.'), "{principle:?}");
                assert!(!principle.text.contains(". "), "the text of `{}` is more than one sentence", principle.title);
                assert!(principle.readers.contains(&Reader::Conductor), "`{}` does not reach the conductor", principle.title);
            }
        }
        for text in [Locale::PtBr, Locale::EnUs] {
            let map = session_map(text);
            assert!(!map.contains(PRINCIPLES_SLOT), "the {text} map kept the slot: {map}");
            for principle in principles(text) {
                assert!(map.contains(&format!("- **{}.** {}\n", principle.title, principle.text)), "the {text} map lacks `{}`", principle.title);
            }
        }
    }

    /// Todo `fn` de teste dos fontes do repositório, pelo nome: o que um
    /// `#[test]` abre nas linhas logo acima.
    fn workspace_tests() -> std::collections::BTreeSet<String> {
        let root = crate::manifest_dir::manifest_dir().join("../..");
        let mut files = Vec::new();
        for dir in ["apps", "packages"] {
            files_under(&root, &root.join(dir), &mut files);
        }
        let mut names = std::collections::BTreeSet::new();
        for (_, body) in files.iter().filter(|(label, _)| label.ends_with(".rs") && !label.contains("/target/")) {
            let lines: Vec<&str> = body.lines().collect();
            for (at, line) in lines.iter().enumerate() {
                let Some(name) = line.trim_start().strip_prefix("fn ").and_then(|rest| rest.split('(').next()) else { continue };
                if lines[at.saturating_sub(3)..at].iter().any(|above| above.trim() == "#[test]") {
                    names.insert(name.to_string());
                }
            }
        }
        names
    }

    /// O princípio que o programa cobra nomeia um teste que existe nos fontes;
    /// o que não é cobrado diz que é só texto. A leitura recusa o princípio
    /// sem a linha da cobrança, o cobrado sem teste e o só de texto com teste.
    #[test]
    fn an_enforced_principle_names_a_test_that_exists() {
        let tests = workspace_tests();
        assert!(tests.contains("an_enforced_principle_names_a_test_that_exists"), "the walk did not find the tests");
        assert!(!tests.contains("a_test_nobody_wrote"), "the walk finds names that are not tests");
        for text in [Locale::PtBr, Locale::EnUs] {
            for principle in principles(text) {
                if let Some(test) = principle.test {
                    assert!(tests.contains(test), "the {text} principle `{}` names `{test}`, which is no test", principle.title);
                }
            }
        }

        let head = "# P\n\n## T\n\nUma frase.\n\n- Para: onda\n";
        assert!(parse_principles(Box::leak(format!("{head}- Cobrança: só texto\n").into_boxed_str()), Locale::PtBr).is_ok_and(|all| all[0].test.is_none()));
        for broken in [
            head.to_string(),
            format!("{head}- Cobrança: o programa recusa\n"),
            format!("{head}- Cobrança: só texto\n- Teste: `x`\n"),
            "# P\n\n## T\n\nUma frase.\n\n- Para: ninguém\n- Cobrança: só texto\n".to_string(),
        ] {
            let body: &'static str = Box::leak(broken.into_boxed_str());
            assert!(parse_principles(body, Locale::PtBr).is_err(), "a broken principle was read: {body}");
        }
    }

    /// A troca mexe só nas linhas `model:` e `effort:` do cabeçalho de cada
    /// agente: o resto do cabeçalho e o corpo inteiro passam como estão, e o
    /// modelo e o esforço do molde são os padrões da configuração — o do
    /// revisor final é o Opus —, então pedir os padrões devolve o molde igual.
    #[test]
    fn the_agent_settings_replace_only_the_model_and_effort_lines_of_the_header() {
        let custom = AgentSettings { model: "haiku", effort: "medium" };
        for text in [Locale::PtBr, Locale::EnUs] {
            for (name, body) in agent_texts(text) {
                let model = if name == REVIEW_AGENT { FINAL_REVIEW_MODEL } else { DEFAULT_AGENT_MODEL };
                for (key, default) in [("model", model), ("effort", DEFAULT_AGENT_EFFORT)] {
                    assert!(
                        body.contains(&format!("\n{key}: {default}\n")),
                        "the {text} `{name}` mold does not carry the default {key}",
                    );
                }
                assert_eq!(
                    with_agent_settings(body, AgentSettings { model, effort: DEFAULT_AGENT_EFFORT }),
                    body,
                    "the default settings must give the mold back untouched",
                );

                let changed = with_agent_settings(body, custom);
                let expected = body
                    .replacen(&format!("\nmodel: {model}\n"), "\nmodel: haiku\n", 1)
                    .replacen(&format!("\neffort: {DEFAULT_AGENT_EFFORT}\n"), "\neffort: medium\n", 1);
                assert_eq!(changed, expected, "the {text} `{name}` agent changed more than the two lines");
                assert_eq!(changed.matches("model:").count(), body.matches("model:").count(), "{text} {name}");
                assert_eq!(changed.matches("effort:").count(), body.matches("effort:").count(), "{text} {name}");
            }
        }
    }

    /// Só o cabeçalho é lido: uma linha `model:` ou `effort:` no corpo passa
    /// como está; a linha que falta no cabeçalho entra antes do `---` que o
    /// fecha; sem cabeçalho ou com ele aberto e nunca fechado, o texto volta
    /// igual.
    #[test]
    fn the_agent_settings_are_written_in_the_header_only() {
        let custom = AgentSettings { model: "opus", effort: "low" };
        let in_the_body = "---\nname: a\nmodel: sonnet\neffort: xhigh\n---\nmodel: sonnet\neffort: xhigh\n";
        assert_eq!(
            with_agent_settings(in_the_body, custom),
            "---\nname: a\nmodel: opus\neffort: low\n---\nmodel: sonnet\neffort: xhigh\n"
        );

        let only_effort = "---\nname: a\neffort: xhigh\n---\ncorpo\n";
        assert_eq!(with_agent_settings(only_effort, custom), "---\nname: a\neffort: low\nmodel: opus\n---\ncorpo\n");

        let only_model = "---\nname: a\nmodel: sonnet\n---\ncorpo\n";
        assert_eq!(with_agent_settings(only_model, custom), "---\nname: a\nmodel: opus\neffort: low\n---\ncorpo\n");

        for untouched in ["sem cabeçalho\nmodel: sonnet\n", "---\nname: a\nmodel: sonnet\neffort: xhigh\n", ""] {
            assert_eq!(with_agent_settings(untouched, custom), untouched);
        }
    }
}
