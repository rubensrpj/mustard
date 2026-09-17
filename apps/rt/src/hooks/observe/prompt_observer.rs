//! `prompt_observer` — o observador do `UserPromptSubmit`.
//!
//! O gravador velho de eventos saiu nesta onda, e com ele o `user.prompt` que
//! este observador era o único a escrever: não sobrou trabalho nenhum aqui. O
//! gancho em si sai com os ganchos, na onda deles; enquanto isso ele fica
//! registrado e não faz nada — nunca bloqueia o prompt e nunca lê o conteúdo
//! dele.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};

/// The `UserPromptSubmit` lifecycle observer.
pub struct PromptObserver;

impl Observer for PromptObserver {
    fn observe(&self, _input: &HookInput, _ctx: &Ctx) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use mustard_core::domain::model::contract::Trigger;
    use mustard_core::ClaudePaths;
    use tempfile::tempdir;

    fn input_with_prompt(prompt: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some("s-prompt".to_string()),
            raw: json!({ "prompt": prompt }),
            ..HookInput::default()
        }
    }

    fn ctx(dir: &str) -> Ctx {
        Ctx::for_test(dir.to_string(), Some(Trigger::UserPromptSubmit))
    }

    /// The session-sink `.events/` dir for a spec-less event (mirrors
    /// `writer_ndjson::event_dir` with `spec = None`).
    fn session_events_dir(project: &std::path::Path, session: &str) -> std::path::PathBuf {
        ClaudePaths::for_project(project)
            .unwrap()
            .claude_dir()
            .join(".session")
            .join(session)
            .join(".events")
    }


    #[test]
    fn empty_prompt_emits_nothing() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();

        PromptObserver.observe(&input_with_prompt(""), &ctx(project));

        let events_dir = session_events_dir(dir.path(), "s-prompt");
        assert!(!events_dir.exists(), "empty prompt must not write any event");
    }

    #[test]
    fn observe_is_failopen_with_no_project() {
        // Missing `prompt` key entirely — observe must not panic / propagate.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some("s-prompt".to_string()),
            raw: json!({ "other": "x" }),
            ..HookInput::default()
        };
        PromptObserver.observe(&input, &ctx(project));
        // Survival is the contract — no event, no panic.
    }
}
