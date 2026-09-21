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
//! constants, and no installed-layout `templates/` directory is required for
//! these seeds. The CLI's `MUSTARD_TEMPLATES_DIR` / `resolve_templates_dir`
//! machinery remains only for the payloads that stay CLI-side (`.github/`
//! scaffolding, `.artifacts.json`).
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

/// Os nomes dos quatro agentes do Mustard: os dois moldes de onda (um para
/// lote de uma tarefa só, outro para lote de várias), o que revisa e o que
/// escreve uma skill. O nome do arquivo é o nome com `.md`.
pub const AGENT_NAMES: [&str; 4] = ["wave", "review", "skill", "wave-solo"];

const AGENTS_PT_BR: [&str; 4] = [
    include_str!("../../templates/agents/pt-BR/wave.md"),
    include_str!("../../templates/agents/pt-BR/review.md"),
    include_str!("../../templates/agents/pt-BR/skill.md"),
    include_str!("../../templates/agents/pt-BR/wave-solo.md"),
];
const AGENTS_EN_US: [&str; 4] = [
    include_str!("../../templates/agents/en-US/wave.md"),
    include_str!("../../templates/agents/en-US/review.md"),
    include_str!("../../templates/agents/en-US/skill.md"),
    include_str!("../../templates/agents/en-US/wave-solo.md"),
];

/// O texto de cada agente no idioma `text`, na ordem de [`AGENT_NAMES`]:
/// `(nome, corpo)`. Os dois idiomas são molde do produto; o projeto recebe
/// só os quatro do `language.text`. `wave` fica no índice 0 e `review` no
/// índice 1, como o resto do código já assume.
#[must_use]
pub fn agent_texts(text: Locale) -> [(&'static str, &'static str); 4] {
    let bodies = match text {
        Locale::PtBr => AGENTS_PT_BR,
        Locale::EnUs => AGENTS_EN_US,
    };
    [
        (AGENT_NAMES[0], bodies[0]),
        (AGENT_NAMES[1], bodies[1]),
        (AGENT_NAMES[2], bodies[2]),
        (AGENT_NAMES[3], bodies[3]),
    ]
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
}
