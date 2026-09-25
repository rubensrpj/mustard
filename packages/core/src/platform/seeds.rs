//! `seeds` — the bundled project-seed payload, compiled into the binary.
//!
//! ## Why these live in the core
//!
//! The files Mustard lays down in a project (`.claude/settings.json`, the
//! session map under `.claude/mustard/`, the three agents under
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

/// O mapa do início da sessão no idioma `text`: o que o Mustard faz, quando
/// uma spec abre e onde cada coisa mora. Os dois idiomas são molde do
/// produto; o projeto recebe só o do `language.text`.
#[must_use]
pub fn session_map(text: Locale) -> &'static str {
    match text {
        Locale::PtBr => SESSION_MAP_PT_BR,
        Locale::EnUs => SESSION_MAP_EN_US,
    }
}

/// Os nomes dos três agentes do Mustard: o de onda, que recebe toda onda,
/// de uma tarefa ou de várias, o que revisa e o que escreve uma skill. O nome
/// do arquivo é o nome com `.md`.
pub const AGENT_NAMES: [&str; 3] = ["wave", "review", "skill"];

const AGENTS_PT_BR: [&str; 3] = [
    include_str!("../../templates/agents/pt-BR/wave.md"),
    include_str!("../../templates/agents/pt-BR/review.md"),
    include_str!("../../templates/agents/pt-BR/skill.md"),
];
const AGENTS_EN_US: [&str; 3] = [
    include_str!("../../templates/agents/en-US/wave.md"),
    include_str!("../../templates/agents/en-US/review.md"),
    include_str!("../../templates/agents/en-US/skill.md"),
];

/// O texto de cada agente no idioma `text`, na ordem de [`AGENT_NAMES`]:
/// `(nome, corpo)`. Os dois idiomas são molde do produto; o projeto recebe
/// só os três do `language.text`. `wave` fica no índice 0 e `review` no
/// índice 1, como o resto do código já assume.
#[must_use]
pub fn agent_texts(text: Locale) -> [(&'static str, &'static str); 3] {
    let bodies = match text {
        Locale::PtBr => AGENTS_PT_BR,
        Locale::EnUs => AGENTS_EN_US,
    };
    [(AGENT_NAMES[0], bodies[0]), (AGENT_NAMES[1], bodies[1]), (AGENT_NAMES[2], bodies[2])]
}

/// The `.claude/.gitignore` seed covering the ephemeral harness state
/// (caches, pipeline states, per-spec event logs, worktrees).
pub const CLAUDE_GITIGNORE: &str = include_str!("../../templates/.gitignore");

#[cfg(test)]
mod tests {
    use super::*;

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
            lines_fixing_a_language("agents/en-US/skill.md", "Text in the project's text language; code and names in English."),
            vec!["agents/en-US/skill.md:1: Text in the project's text language; code and names in English.".to_string()],
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
            (Locale::PtBr, "toda execução de código", "a investigação que abre muitos arquivos"),
            (Locale::EnUs, "every code run", "any investigation that opens many files"),
        ] {
            let map = session_map(text);
            assert!(!map.contains(code_run), "the {text} map still hands code execution to an agent: {map}");
            assert!(map.contains(investigation), "the {text} map lost the investigation delegation: {map}");
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
    /// tamanho: a entrega vai gravada na spec pela ferramenta, a última
    /// mensagem só diz que gravou, e todo o detalhe do trabalho vai no campo
    /// de texto da entrega. Ele não ensina mais a linha colada.
    #[test]
    fn o_molde_da_onda_grava_a_entrega_sem_relatorio_pelo_tamanho() {
        for (text, size_report, recorded, last_message) in [
            (Locale::PtBr, "entre mil e dois mil tokens", "`run write delivered --json", "a última mensagem só diz que gravou"),
            (Locale::EnUs, "between one and two thousand tokens", "`run write delivered --json", "the last message only says it did"),
        ] {
            for (name, body) in agent_texts(text) {
                if name != "wave" {
                    continue;
                }
                let lower = body.to_lowercase();
                assert!(!lower.contains(size_report), "the {text} `{name}` agent still asks for a report by size: {body}");
                assert!(body.contains(recorded), "the {text} `{name}` agent does not record the delivery: {body}");
                assert!(lower.contains(last_message), "the {text} `{name}` agent does not say what the last message holds: {body}");
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
    /// `/clear` só depois de gravado o objetivo, o primeiro `context`: antes
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
            assert!(before.contains("`context`"), "the {text} map suggests `/clear` before the first `context` is recorded: {line}");
            assert!(before.contains(goal), "the {text} map does not name the goal as what is recorded first: {line}");
            assert!(line[clear..].contains(resume_line), "the {text} map does not say the resume line shows where the spec stands: {line}");

            let resume = &map[map.find(resume_heading).unwrap_or_else(|| panic!("the {text} map lost its resume section: {map}"))..];
            assert!(!resume.contains("/clear"), "the {text} resume section still speaks of `/clear`: {resume}");
            let next_goal = crate::platform::i18n::translate("open.next_goal", text);
            assert!(!next_goal.contains("/clear"), "the {text} answer of `run open` repeats the `/clear` suggestion: {next_goal}");
        }
    }

    /// O mapa pede a todo agente chamado que grave o resultado na spec, pelo
    /// `mustard-rt run write`, e volte com duas linhas, na linha que delega
    /// ao agente a investigação que abre muitos arquivos.
    #[test]
    fn the_session_map_asks_every_agent_to_record_in_the_spec_and_come_back_in_two_lines() {
        for (text, investigation, every_agent, in_the_spec, two_lines) in [
            (Locale::PtBr, "a investigação que abre muitos arquivos", "todo agente", "na spec", "duas linhas"),
            (Locale::EnUs, "any investigation that opens many files", "every agent", "in the spec", "two lines"),
        ] {
            let map = session_map(text);
            let line = map
                .lines()
                .find(|line| line.contains(investigation))
                .unwrap_or_else(|| panic!("the {text} map lost the investigation delegation: {map}"));
            let write = line
                .find("`mustard-rt run write`")
                .unwrap_or_else(|| panic!("the {text} map does not ask the agent to record through `run write`: {line}"));
            assert!(line[..write].contains(every_agent), "the {text} map does not ask it of every agent: {line}");
            assert!(line[..write].contains(in_the_spec), "the {text} map does not say the result goes to the spec: {line}");
            assert!(line[write..].contains(two_lines), "the {text} map does not ask the agent to come back in two lines: {line}");
        }
    }
}
