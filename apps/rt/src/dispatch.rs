//! O despachante — transforma uma chamada do Claude Code num [`Outcome`].
//!
//! É o único lugar em que mora a regra de nunca falhar: um gancho não se
//! defende de entrada ruim nem dos próprios erros, o despachante os absorve.
//! A cada chamada:
//!
//! 1. Pega no [`Registry`] os ganchos do evento e da ferramenta.
//! 2. Roda o `Observer` de cada um, sem esperar nada dele.
//! 3. Roda o `Check` de cada um e junta o veredito no [`Outcome`]. Um `Check`
//!    que devolve `Err` conta como `Allow`.
//!
//! ## O que se grava
//!
//! Num projeto cujo `mustard.json` traz `enabled: false`, nada disso roda: a
//! chamada termina em `Allow` antes do primeiro gancho.
//!
//! O gancho só grava quando age, e quem grava é o despachante: um gancho que
//! barra ou avisa vira um evento `hook`, e um que coloca texto vira um evento
//! `injection`, com o tamanho. O que deixou passar não grava nada. No fim da
//! resposta, a resposta do assistente vai para a conversa, também a que a
//! conferência do fim da resposta barrou: ela já apareceu na tela, e o
//! complemento que o bloqueio pede vem depois dela, na próxima. Tudo na spec
//! atual; sem ela, nada é gravado.

use std::path::{Path, PathBuf};

use crate::commands::spec_events::conversation::{record_hook, record_injection, record_response, HookAction};
use crate::registry::{Module, Registry};
use mustard_core::domain::model::contract::{Ctx, HookInput, Outcome, Trigger, Verdict};
use mustard_core::io::workspace::workspace_root;

/// Roda os ganchos de um evento inteiro (`mustard-rt on <evento>`).
///
/// `trigger` é `None` quando o nome do evento não é conhecido: nenhum gancho
/// casa, e o resultado é um `Allow`.
#[must_use]
pub fn run_event(trigger: Option<Trigger>, input: &HookInput) -> Outcome {
    let Some(trigger) = trigger else {
        return Outcome::allow();
    };
    let ctx = build_ctx(trigger, input);
    // O projeto que desligou o Mustard no `mustard.json` não tem gancho que
    // aja: nem barra, nem avisa, nem coloca texto, nem grava.
    if !ctx.config.enabled() {
        return Outcome::allow();
    }
    let registry = Registry::new();
    let tool = input.tool_name.as_deref();
    let root = PathBuf::from(&ctx.project_dir);
    let mut outcome = Outcome::allow();
    for module in registry.applicable(trigger, tool) {
        run_module(module, input, &ctx, &root, &mut outcome);
    }
    // A resposta barrada também é gravada, antes do complemento que o
    // bloqueio pede: o usuário a leu na tela.
    if trigger == Trigger::Stop && !input.is_subagent() {
        let _ = record_response(&root, input.session_id.as_deref(), input.last_assistant_message().unwrap_or_default());
    }
    outcome
}

/// Monta o [`Ctx`] de uma chamada a partir da entrada do Claude Code.
///
/// Resolve a raiz do projeto uma vez, por [`workspace_root`]. Quando ela não se
/// acha, o `Ctx` fica com a pasta da chamada e sem raiz, e um aviso vai ao
/// stderr: o gancho nunca barra por isso.
///
/// Uma pasta `"."` ou vazia vira a pasta do processo antes da busca, senão a
/// busca não sobe pelas pastas acima e o estado dos ganchos iria parar dentro
/// de `apps/rt/` nos testes.
fn build_ctx(trigger: Trigger, input: &HookInput) -> Ctx {
    let raw_cwd = input.cwd.clone().unwrap_or_default();
    let resolved_cwd = if raw_cwd.is_empty() || raw_cwd == "." {
        std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| raw_cwd.clone())
    } else {
        raw_cwd.clone()
    };
    let workspace_root = resolve_workspace_root_fail_open(&resolved_cwd);
    let project_dir = workspace_root
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(resolved_cwd);
    let config = crate::shared::context::config::project_config_cached(Path::new(&project_dir));
    Ctx { project_dir, trigger: Some(trigger), workspace_root, config }
}

/// A raiz do projeto, ou `None` com um aviso de uma linha no stderr.
fn resolve_workspace_root_fail_open(project_dir: &str) -> Option<PathBuf> {
    let start = PathBuf::from(project_dir);
    match workspace_root(&start) {
        Ok(root) => Some(root),
        Err(err) => {
            let _ = serde_json::to_string(&serde_json::json!({
                "level": "warn",
                "module": "dispatch",
                "event": "workspace_root.unresolved",
                "project_dir": project_dir,
                "error": err.to_string(),
            }))
            .map(|s| eprintln!("{s}"));
            None
        }
    }
}

/// Roda um gancho: o `Observer`, depois o `Check`, cujo veredito é gravado
/// quando o gancho agiu e entra no resultado.
fn run_module(module: &Module, input: &HookInput, ctx: &Ctx, root: &Path, outcome: &mut Outcome) {
    if let Some(observer) = &module.observer {
        observer.observe(input, ctx);
    }
    let Some(check) = &module.check else {
        return;
    };
    let verdict = check.evaluate(input, ctx).unwrap_or(Verdict::Allow);
    record_action(module.id, &verdict, input, ctx, root);
    outcome.fold(verdict);
}

/// Grava o que o gancho `hook` fez, quando ele agiu: barrar ou avisar vira
/// `hook`, colocar texto vira `injection`. Deixar passar não grava nada.
fn record_action(hook: &str, verdict: &Verdict, input: &HookInput, ctx: &Ctx, root: &Path) {
    let session = input.session_id.as_deref();
    let tool = input
        .tool_name
        .as_deref()
        .or_else(|| ctx.trigger.map(Trigger::as_event_name))
        .unwrap_or_default();
    let _ = match verdict {
        Verdict::Deny { reason } => record_hook(root, session, hook, HookAction::Block, tool, reason),
        Verdict::Warn { message } => record_hook(root, session, hook, HookAction::Warn, tool, message),
        Verdict::Inject { context } if !context.is_empty() => record_injection(root, session, hook, context),
        _ => None,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_open;
    use crate::shared::spec_state::{stand_on_spec_branch, DiskSpecState};
    use mustard_core::domain::model::contract::{Check, Observer};
    use mustard_core::domain::spec_state::SpecState;
    use mustard_core::platform::error::Error;
    use serde_json::{json, Map, Value};

    /// A entrada de Bash traz uma pasta temporária como `cwd`: sem ela, o
    /// despachante usaria a pasta do processo, que fica dentro do checkout, e
    /// os ganchos gravariam o estado deles no projeto de verdade.
    fn bash_input(dir: &Path, command: &str, event: &str) -> HookInput {
        HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": command }),
            hook_event_name: Some(event.to_string()),
            cwd: Some(dir.to_string_lossy().into_owned()),
            ..HookInput::default()
        }
    }

    #[test]
    fn unknown_event_fails_open_to_allow() {
        let outcome = run_event(None, &HookInput::default());
        assert_eq!(outcome.verdict, Verdict::Allow);
    }

    #[test]
    fn dispatch_runs_bash_guard_for_bash_pretooluse() {
        let dir = tempfile::tempdir().unwrap();
        let input = bash_input(dir.path(), "rm -rf /", "PreToolUse");
        let outcome = run_event(Some(Trigger::PreToolUse), &input);
        assert!(outcome.is_blocking());
    }

    /// Com o Mustard desligado no `mustard.json`, o comando que a trava barra
    /// passa, e nada é gravado; religado, a trava volta a barrar.
    #[test]
    fn a_project_with_mustard_off_has_no_hook_acting() {
        let dir = project_on("desligado");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"enabled":false}"#).unwrap();
        let input = HookInput { session_id: Some("s1".to_string()), ..bash_input(root, "rm -rf /", "PreToolUse") };
        let outcome = run_event(Some(Trigger::PreToolUse), &input);
        assert_eq!(outcome.verdict, Verdict::Allow, "{outcome:?}");
        assert!(outcome.warnings.is_empty(), "{outcome:?}");
        assert!(recorded(root, "desligado").is_empty(), "nothing is recorded while Mustard is off");

        let on = tempfile::tempdir().unwrap();
        std::fs::write(on.path().join("mustard.json"), r#"{"enabled":true}"#).unwrap();
        assert!(run_event(Some(Trigger::PreToolUse), &bash_input(on.path(), "rm -rf /", "PreToolUse")).is_blocking());
    }

    /// Um comando comum de leitura passa pelo despachante.
    #[test]
    fn dispatch_allows_bare_ls_for_bash_pretooluse() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = run_event(Some(Trigger::PreToolUse), &bash_input(dir.path(), "ls", "PreToolUse"));
        assert_eq!(outcome.verdict, Verdict::Allow, "warnings {:?}", outcome.warnings);
    }

    /// O contexto dos ganchos traz o `mustard.json` do projeto; sem o
    /// arquivo, traz a configuração padrão.
    #[test]
    fn the_context_carries_the_project_config() {
        let with_flow = tempfile::tempdir().unwrap();
        std::fs::write(with_flow.path().join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .unwrap();
        let ctx = build_ctx(Trigger::PreToolUse, &bash_input(with_flow.path(), "ls", "PreToolUse"));
        let bases: Vec<String> = ctx.config.git.declared_bases().into_iter().collect();
        assert_eq!(bases, ["dev", "main"]);

        let without = tempfile::tempdir().unwrap();
        let ctx = build_ctx(Trigger::PreToolUse, &bash_input(without.path(), "ls", "PreToolUse"));
        assert!(ctx.config.git.flow.is_empty(), "{:?}", ctx.config.git);
    }

    /// Um gancho de teste que devolve sempre o mesmo veredito.
    struct Says(Verdict);

    impl Check for Says {
        fn evaluate(&self, _: &HookInput, _: &Ctx) -> Result<Verdict, Error> {
            Ok(self.0.clone())
        }
    }

    /// Um observador de teste, que nunca decide nada.
    struct Watches;

    impl Observer for Watches {
        fn observe(&self, _: &HookInput, _: &Ctx) {}
    }

    fn module(id: &'static str, check: Option<Verdict>) -> Module {
        use crate::registry::ToolMatch;
        Module {
            id,
            applies_to: &[(Trigger::PreToolUse, ToolMatch::Any)],
            check: check.map(|verdict| Box::new(Says(verdict)) as Box<dyn Check>),
            observer: Some(Box::new(Watches)),
        }
    }

    /// Um projeto com a spec `spec` aberta e o checkout na branch dela.
    fn project_on(spec: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").expect("config");
        stand_on_spec_branch(root, spec);
        record_open(root, spec, &format!("feature/{spec}"), "dev").expect("open");
        dir
    }

    /// Os eventos da conversa gravados na spec, sem a mensagem e a resposta.
    fn recorded(root: &Path, spec: &str) -> Vec<Map<String, Value>> {
        DiskSpecState::new(root)
            .log(spec)
            .map(|log| {
                log.visible()
                    .into_iter()
                    .filter(|e| ["hook", "injection", "call"].contains(&e.event_type.as_str()))
                    .map(|e| {
                        let mut fields = e.fields.clone();
                        fields.insert("type".to_string(), json!(e.event_type));
                        fields
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Um gancho que deixa passar não grava nada; o que barra grava `hook`;
    /// o que avisa grava `hook` com a ação de aviso; o que coloca texto grava
    /// `injection` com o tamanho; e um passo do fluxo, rodado pelo mesmo
    /// despacho que o `mustard-rt run` usa, grava `call`.
    #[test]
    fn the_dispatcher_records_only_what_a_hook_did() {
        let dir = project_on("despacho");
        let root = dir.path();
        let ctx = Ctx::for_test(root.to_string_lossy().to_string(), Some(Trigger::PreToolUse));
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            session_id: Some("s1".to_string()),
            ..HookInput::default()
        };
        let mut outcome = Outcome::allow();
        for m in [
            module("passa", Some(Verdict::Allow)),
            module("so_observa", None),
            module("avisa", Some(Verdict::Warn { message: "cuidado".to_string() })),
            module("coloca", Some(Verdict::Inject { context: "linha de texto".to_string() })),
            module("barra", Some(Verdict::Deny { reason: "apaga trabalho".to_string() })),
        ] {
            run_module(&m, &input, &ctx, root, &mut outcome);
        }
        assert!(outcome.is_blocking());

        crate::commands::flow::cli::dispatch(crate::commands::flow::cli::FlowCmd::Resume {
            spec: Some("despacho".to_string()),
            root: root.to_path_buf(),
        });

        let events = recorded(root, "despacho");
        let hooks: Vec<&str> = events.iter().filter_map(|e| e.get("hook").and_then(Value::as_str)).collect();
        assert_eq!(hooks, ["avisa", "coloca", "barra"], "nothing for the hook that let it pass: {events:?}");
        assert_eq!(events[0]["type"], json!("hook"));
        assert_eq!((events[0]["action"].as_str(), events[0]["tool"].as_str()), (Some("warn"), Some("Bash")));
        assert_eq!(events[0]["reason"], json!("cuidado"));
        assert_eq!(events[1]["type"], json!("injection"));
        assert_eq!((events[1]["chars"].as_u64(), events[1]["text"].as_str()), (Some(14), Some("linha de texto")));
        assert_eq!(events[2]["type"], json!("hook"));
        assert_eq!((events[2]["action"].as_str(), events[2]["reason"].as_str()), (Some("block"), Some("apaga trabalho")));
        assert_eq!(events[3]["type"], json!("call"));
        assert_eq!((events[3]["command"].as_str(), events[3]["result"].as_str()), (Some("resume"), Some("ok")));
        assert_eq!(events.len(), 4, "{events:?}");
    }

    /// No fim de uma resposta que passa, a resposta vai para a conversa,
    /// ligada à última mensagem.
    #[test]
    fn the_end_of_an_answer_records_the_response() {
        let dir = project_on("resposta");
        let root = dir.path();
        let asked =
            crate::commands::spec_events::conversation::record_message(root, Some("s1"), "e agora?").expect("message");
        let stop = |text: &str| HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw: json!({ "last_assistant_message": text }),
            ..HookInput::default()
        };
        let outcome = run_event(Some(Trigger::Stop), &stop("Pronto."));
        assert!(!outcome.is_blocking(), "{outcome:?}");
        let log = DiskSpecState::new(root).log("resposta").expect("log");
        let responses: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "response").collect();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].str_field("text"), Some("Pronto."));
        assert_eq!(responses[0].int("reply_to"), Some(asked));
    }

    /// O caminho de verdade, com três mensagens do usuário: a spec nasce e o
    /// assistente sugere um objetivo; o usuário pede outro; o assistente
    /// sugere o novo numa resposta com uma frase de mais de 25 palavras, que a
    /// conferência de escrita não barra mais, e a segunda resposta, que um
    /// bloqueio do fim da resposta pede, chega com `stop_hook_active`; o
    /// usuário responde "pode usar essa" pelo gancho da mensagem; o assistente
    /// responde de novo e o usuário manda a terceira mensagem. A primeira
    /// resposta fica gravada inteira, antes da segunda, e as duas antes do
    /// sim. O `run write` do objetivo grava a frase sugerida com `origin` no
    /// sim, a mensagem do usuário que o define. Apontando uma resposta do
    /// assistente, e não a mensagem, o objetivo é recusado e nada é gravado.
    #[test]
    fn each_response_is_recorded_in_order_and_the_goal_points_at_the_users_message() {
        let dir = project_on("barrada");
        let root = dir.path();
        let hook_call = |event: &str, raw: Value| HookInput {
            hook_event_name: Some(event.to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw,
            ..HookInput::default()
        };
        let stop = |text: &str| {
            run_event(Some(Trigger::Stop), &hook_call("Stop", json!({ "last_assistant_message": text })))
        };
        let say = |text: &str| {
            run_event(Some(Trigger::UserPromptSubmit), &hook_call("UserPromptSubmit", json!({ "prompt": text })))
        };
        let opening_suggestion = "Travar o envio com pendência aberta.";
        let opening = format!("A spec nasceu. Sugiro: \"{opening_suggestion}\" Serve?");
        assert!(!stop(&opening).is_blocking());
        assert!(!say("Não, outra.").is_blocking());
        let suggestion = "Um sim aprova o objetivo sugerido.";
        let barred = format!(
            "Então sugiro: \"{suggestion}\" Depois de ler todos os arquivos do projeto e \
             conferir cada teste que ainda falhava na máquina do usuário, eu ajustei a leitura do \
             idioma e a contagem das linhas para que a resposta final saia bem curta e clara."
        );
        let first = stop(&barred);
        assert!(!first.is_blocking(), "the writing check no longer bars the long sentence: {first:?}");
        let complement = "Resumo: ajustei a leitura do idioma.";
        let retry = json!({ "last_assistant_message": complement, "stop_hook_active": true });
        let second = run_event(Some(Trigger::Stop), &hook_call("Stop", retry));
        assert!(!second.is_blocking(), "{second:?}");
        assert!(!say("pode usar essa").is_blocking());
        let after_yes = "Gravo: \"Travar tudo, sempre.\"";
        assert!(!stop(after_yes).is_blocking());
        assert!(!say("grave").is_blocking());

        let log = DiskSpecState::new(root).log("barrada").expect("log");
        let talk: Vec<(&str, Option<&str>)> = log
            .visible()
            .into_iter()
            .filter(|e| matches!(e.event_type.as_str(), "message" | "response"))
            .map(|e| (e.event_type.as_str(), e.str_field("text")))
            .collect();
        assert_eq!(
            talk,
            [
                ("response", Some(opening.as_str())),
                ("message", Some("Não, outra.")),
                ("response", Some(barred.as_str())),
                ("response", Some(complement)),
                ("message", Some("pode usar essa")),
                ("response", Some(after_yes)),
                ("message", Some("grave")),
            ],
            "the barred answer comes whole, before the complement"
        );
        let messages: Vec<u64> =
            log.visible().into_iter().filter(|e| e.event_type == "message").map(|e| e.id).collect();
        let yes = messages[1];

        let goal = |text: &str, origin: u64| {
            crate::commands::spec_events::write::write_at(&crate::commands::spec_events::write::WriteOpts {
                root: root.to_path_buf(),
                spec: Some("barrada".to_string()),
                event_type: "context".to_string(),
                json: json!({ "text": text, "origin": origin }).to_string(),
            })
        };
        let spec_file = || std::fs::read_to_string(root.join(".claude/spec/barrada/spec.ndjson")).expect("spec file");
        let before = spec_file();
        let replied = log.visible().into_iter().find(|e| e.event_type == "response").map(|e| e.id).expect("a response");
        let refused = goal(suggestion, replied);
        assert_eq!(refused["reason"], json!("goal-origin-not-user"), "the goal points at a reply: {refused}");
        assert_eq!(spec_file(), before, "a refusal writes nothing");
        let written = goal(suggestion, yes);
        assert_eq!(written["ok"], json!(true), "{written}");
        let log = DiskSpecState::new(root).log("barrada").expect("log");
        let recorded = mustard_core::domain::survey::goal(&log).expect("the goal was recorded");
        assert_eq!((recorded.str_field("text"), recorded.int("origin")), (Some(suggestion), Some(yes)));
    }
}
