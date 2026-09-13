//! `pending_gate` — a regra das pendências do fim da resposta ([`PendingRule`],
//! uma das regras do `end_of_turn_check`): o turno em que uma unidade fechou
//! não termina com uma mensagem que omite uma pendência aberta.
//!
//! ## O caso que a fez nascer
//!
//! Em 09/09/2026 três trabalhos foram combinados na ordem 2 → 3 → 1. Os dois
//! primeiros viraram pull requests e, no mesmo turno do último merge, o resumo
//! "O dia fechou assim" listou duas pendências e omitiu o terceiro trabalho. O
//! operador só descobriu no dia seguinte, perguntando. Nenhum gancho conferia o
//! texto final do assistente.
//!
//! ## Quando cobra — todos os fatos precisam valer
//!
//! 1. É o `Stop` da sessão principal (nunca o de um subagente) — o
//!    `end_of_turn_check` só chama as regras nele.
//! 2. Uma unidade FECHOU e o fechamento ainda não se encerrou: a sessão carrega
//!    a marca que o escritor de eventos grava ao registrar `pipeline.complete`
//!    ou `pr.merged` ([`unit_closed_blocks`]).
//! 3. O `Stop` trouxe `last_assistant_message` (o texto final do turno, campo
//!    documentado do evento). Sem ele não há o que conferir.
//! 4. Alguma pendência aberta não é citada nesse texto, pelo título ou pelo id
//!    (`P-3`), sem diferenciar maiúsculas. Id e título contam só inteiros: `P-1`
//!    não cita dentro de `P-10`, e o título `um` não cita dentro de `algum`. O
//!    bloqueio pede o título: a regra de clareza barra o código na conversa.
//! 5. A regra ainda não bloqueou [`MAX_BLOCKS`] vezes por este fechamento.
//!
//! Qualquer fato que falte libera o turno.
//!
//! ## Por que só no turno do fechamento
//!
//! É o momento exato da perda original. Cobrar em todo turno obrigaria cada
//! resposta curta a recitar a lista — e um aviso que sempre dispara aprende-se a
//! ignorar.
//!
//! ## A marca só some quando a regra libera
//!
//! O bloqueio pede uma reescrita, e é ela que precisa ser conferida: por isso
//! a marca fica enquanto a regra bloqueia.
//!
//! - **Libera e consome** quando nada está aberto, quando a mensagem cita cada
//!   pendência aberta, quando não há texto final, ou quando já bloqueou
//!   [`MAX_BLOCKS`] vezes por este fechamento.
//! - **Bloqueia e conta** nos demais casos: a marca fica, com `blocks: N+1`.
//!
//! Um turno só TERMINA num `Stop` que a conferência liberou — e nele esta regra
//! consumiu a marca, então o próximo turno nunca herda o fechamento. Antes, com
//! um gancho por regra, um bloqueio de outro gancho engolia o desta; agora os
//! achados dividem um bloqueio só, e o texto dela sempre chega.
//!
//! Bloquear sem conseguir gravar o contador bloquearia sem limite; nesse caso a
//! regra consome a marca no lugar (um bloqueio só) e, sem conseguir nem isso,
//! libera.
//!
//! ## `stop_hook_active` não libera
//!
//! Ele não diz QUEM bloqueou. Se a clareza barra o primeiro `Stop` e a
//! reescrita fecha a unidade, o `Stop` seguinte chega com `stop_hook_active` e
//! uma marca nova — e é a mensagem que encerra o fechamento. Liberá-lo pelo
//! campo deixaria essa mensagem sem conferência (a perda original). O limite é
//! o contador: no máximo [`MAX_BLOCKS`] bloqueios por fechamento, longe do teto
//! de 8 bloqueios seguidos do Claude Code.
//!
//! ## Sem modo `MUSTARD_*_MODE`
//!
//! A regra não ganha porta de configuração. Ela já se restringe sozinha ao
//! turno do fechamento, e desligá-la devolveria exatamente a perda que ela
//! existe para impedir.

use crate::commands::event::pending::{format_pending_items, open_pending, OpenPending};
use crate::hooks::task::end_of_turn_check::{Finding, Turn, TurnRule};
use crate::shared::context::{record_unit_closed_block, take_unit_closed, unit_closed_blocks};
use mustard_core::platform::i18n::Locale;
use std::path::Path;

/// Quantas vezes, no máximo, a regra bloqueia por fechamento. Dois: um para a
/// primeira mensagem, outro para a reescrita que ainda omite — e então libera,
/// sem laço.
const MAX_BLOCKS: u32 = 2;

/// A regra das pendências do fim da resposta.
pub struct PendingRule;

impl TurnRule for PendingRule {
    fn check(&self, turn: &Turn<'_>) -> Option<Finding> {
        // `stop_hook_active` NÃO libera aqui (ver "`stop_hook_active` não
        // libera"): o contador é o limite.
        let project_dir = turn.project_dir;
        let session = turn.session.unwrap_or_default();

        // Fato 2 — uma unidade fechou e o fechamento não se encerrou. Só lê: a
        // marca some apenas quando a regra libera (ver "A marca só some…").
        let blocks = unit_closed_blocks(project_dir, session)?;

        // Fatos 3, 4 e 5 — liberar encerra o fechamento: consome a marca.
        let omitted = omitted_items(turn.message, project_dir);
        if omitted.is_empty() || blocks >= MAX_BLOCKS {
            take_unit_closed(project_dir, session);
            return None;
        }

        // Bloquear conta. Sem contador gravado, consome no lugar; sem nem isso,
        // libera — nunca um bloqueio sem limite.
        if !record_unit_closed_block(project_dir, session, blocks + 1)
            && !take_unit_closed(project_dir, session)
        {
            return None;
        }
        Some(Finding::Block(block_reason(&omitted, turn.lang)))
    }
}

/// As pendências abertas que o texto final do turno não cita. Vazio quando o
/// `Stop` não trouxe texto final: sem texto, nada a conferir.
fn omitted_items(message: &str, project_dir: &str) -> Vec<OpenPending> {
    if message.trim().is_empty() {
        return Vec::new();
    }
    open_pending(Path::new(project_dir))
        .into_iter()
        .filter(|item| !cites(message, item))
        .collect()
}

/// `true` quando `message` cita `item` pelo id ou pelo título, sem diferenciar
/// maiúsculas e sem ligar para quebras de linha no meio do título.
fn cites(message: &str, item: &OpenPending) -> bool {
    let text = normalize(message);
    mentions_id(&text, &item.id.to_lowercase()) || mentions_title(&text, &normalize(&item.title))
}

/// Minúsculas, espaços colapsados — a forma em que texto e título se comparam.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// `id` aparece em `text` como um id inteiro: `p-1` não conta dentro de `p-10`
/// nem de `xp-1`, senão citar uma pendência bastaria para cobrir outra.
fn mentions_id(text: &str, id: &str) -> bool {
    occurs_whole(text, id, |c| c.is_ascii_digit())
}

/// `title` aparece em `text` delimitado por não-alfanuméricos ou pelas bordas:
/// o título `um` não conta dentro de `algum`.
fn mentions_title(text: &str, title: &str) -> bool {
    occurs_whole(text, title, char::is_alphanumeric)
}

/// `needle` aparece em `text` sem colar em outra palavra: o caractere antes não
/// é alfanumérico e o depois não satisfaz `joins_after` (ou são as bordas).
/// Testa cada posição, não só as ocorrências sem sobreposição — uma ocorrência
/// inválida não esconde uma válida que começa dentro dela.
fn occurs_whole(text: &str, needle: &str, joins_after: fn(char) -> bool) -> bool {
    !needle.is_empty()
        && text.char_indices().any(|(at, _)| {
            text[at..].starts_with(needle) && {
                let before = text[..at].chars().next_back();
                let after = text[at + needle.len()..].chars().next();
                !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(joins_after)
            }
        })
}

/// O motivo do bloqueio: nomeia CADA pendência omitida — sem corte, porque o
/// próximo passo é citá-las todas — e diz as duas saídas honestas. O texto sai
/// do catálogo, no idioma do projeto.
fn block_reason(omitted: &[OpenPending], lang: Locale) -> String {
    mustard_core::translate("pending.gate.block", lang)
        .replace("{count}", &omitted.len().to_string())
        .replace("{items}", &format_pending_items(omitted, omitted.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::event::pending::{pending_at, PendingOpts};
    use crate::hooks::task::end_of_turn_check::run_rules;
    use crate::shared::context::mark_unit_closed;
    use mustard_core::domain::model::contract::{Ctx, HookInput, Trigger, Verdict};
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(dir: &Path) -> Ctx {
        Ctx::for_test(dir.to_string_lossy().into_owned(), Some(Trigger::Stop))
    }

    /// Um `Stop` da sessão principal com o texto final do turno.
    fn stop(session: &str, message: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some(session.to_string()),
            raw: json!({ "last_assistant_message": message }),
            ..HookInput::default()
        }
    }

    /// Um projeto instalado com as pendências abertas `titles`, numeradas na
    /// ordem (`P-1`, `P-2`, …).
    fn project_with_open_items(titles: &[&str]) -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"lang":"pt-BR"}"#).expect("cfg");
        for title in titles {
            let out = pending_at(&PendingOpts {
                root: root.to_path_buf(),
                add: true,
                title: Some((*title).into()),
                detail: Some("combinado".into()),
                ..PendingOpts::default()
            });
            assert_eq!(out["ok"], json!(true), "seed: {out}");
        }
        dir
    }

    /// Um projeto instalado com duas pendências abertas: P-1 "Humanize" e
    /// P-2 "HTML padrao da spec".
    fn project_with_two_open_items() -> tempfile::TempDir {
        project_with_open_items(&["Humanize", "HTML padrao da spec"])
    }

    /// A regra das pendências sozinha, como a conferência do fim da resposta
    /// a roda.
    fn verdict(root: &Path, input: &HookInput) -> Verdict {
        run_rules(&[&PendingRule], input, &ctx(root))
    }

    /// O turno em que uma unidade fechou e cuja mensagem final omite uma
    /// pendência aberta é bloqueado, e o motivo NOMEIA a omitida (e só ela). A
    /// reescrita que a cita passa e encerra o fechamento.
    #[test]
    fn pending_gate_blocks_closing_turn_that_omits_open_item() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        let project = root.to_string_lossy().into_owned();
        mark_unit_closed(&project, "s-close");

        // O resumo do caso real: cita o segundo trabalho e esquece o terceiro.
        let summary = "O dia fechou assim: PRs 267 a 270 mergeados; segue o p-2.";
        match verdict(root, &stop("s-close", summary)) {
            Verdict::Deny { reason } => {
                assert!(reason.contains("P-1"), "the omitted item's id is named: {reason}");
                assert!(reason.contains("Humanize"), "the omitted item's title is named: {reason}");
                assert!(!reason.contains("P-2"), "a cited item is not demanded again: {reason}");
            }
            other => panic!("a closing turn that omits an open item must block, got {other:?}"),
        }

        // A reescrita que o bloqueio provoca chega com `stop_hook_active` e cita
        // a omitida: passa, e o fechamento se encerra nela.
        let mut rewrite = stop("s-close", "O dia fechou assim; seguem o P-1 (Humanize) e o p-2.");
        rewrite.raw["stop_hook_active"] = json!(true);
        assert_eq!(verdict(root, &rewrite), Verdict::Allow, "the rewrite that cites passes");
        assert_eq!(unit_closed_blocks(&project, "s-close"), None, "that Stop consumed the marker");
        assert_eq!(verdict(root, &stop("s-close", summary)), Verdict::Allow, "once per closure");
    }

    /// Sem fechamento neste turno, a resposta passa mesmo omitindo todas
    /// as pendências abertas; e o fechamento de OUTRA sessão não conta.
    #[test]
    fn pending_gate_ignores_turn_without_closure() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        assert_eq!(
            verdict(root, &stop("s-quiet", "Pronto, ajustei o teste.")),
            Verdict::Allow,
            "an ordinary turn never recites the list",
        );

        mark_unit_closed(&root.to_string_lossy(), "s-other");
        assert_eq!(
            verdict(root, &stop("s-quiet", "Pronto, ajustei o teste.")),
            Verdict::Allow,
            "a closure recorded by another session is not this turn's",
        );
    }

    /// A marca fica no bloqueio, então o `Stop` seguinte — a reescrita, com
    /// `stop_hook_active` — confere de novo, até [`MAX_BLOCKS`] vezes; depois
    /// libera e consome, sem laço. O nome vem de quando um gancho irmão podia
    /// engolir este bloqueio.
    #[test]
    fn a_pending_block_swallowed_by_another_gate_is_checked_again() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        let project = root.to_string_lossy().into_owned();
        mark_unit_closed(&project, "s-swallow");
        let omits = "Fechei a unidade; segue o P-2.";

        assert!(verdict(root, &stop("s-swallow", omits)).is_blocking(), "first Stop blocks");
        assert_eq!(unit_closed_blocks(&project, "s-swallow"), Some(1), "a block keeps the marker");

        let mut again = stop("s-swallow", omits);
        again.raw["stop_hook_active"] = json!(true);
        assert!(verdict(root, &again).is_blocking(), "still omitted: checked and blocked again");
        assert_eq!(unit_closed_blocks(&project, "s-swallow"), Some(2));

        assert_eq!(verdict(root, &again), Verdict::Allow, "the third Stop is released");
        assert_eq!(unit_closed_blocks(&project, "s-swallow"), None, "and the marker consumed");
        assert_eq!(
            verdict(root, &stop("s-swallow", "Pronto, ajustei o teste.")),
            Verdict::Allow,
            "the next ordinary turn never inherits the closure",
        );
    }

    /// A mensagem que cita cada pendência aberta libera o `Stop` e encerra o
    /// fechamento: a marca some ali.
    #[test]
    fn a_cited_closure_consumes_the_marker() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        let project = root.to_string_lossy().into_owned();
        mark_unit_closed(&project, "s-cited");

        assert_eq!(verdict(root, &stop("s-cited", "Seguem P-1 e P-2.")), Verdict::Allow);
        assert_eq!(unit_closed_blocks(&project, "s-cited"), None, "the allowing Stop consumed it");
        assert!(!take_unit_closed(&project, "s-cited"), "nothing left to consume");
    }

    /// Um título curto conta só como palavra inteira: `um` não é citado dentro
    /// de `algum`, e é citado quando aparece solto.
    #[test]
    fn a_short_title_is_not_cited_by_a_longer_word() {
        let dir = project_with_open_items(&["um"]);
        let root = dir.path();
        let project = root.to_string_lossy().into_owned();

        mark_unit_closed(&project, "s-short");
        match verdict(root, &stop("s-short", "Fechei algum trabalho hoje.")) {
            Verdict::Deny { reason } => assert!(reason.contains("P-1"), "{reason}"),
            other => panic!("`algum` must not cite the title `um`, got {other:?}"),
        }
        assert_eq!(
            verdict(root, &stop("s-short", "Segue aberto o \"UM\".")),
            Verdict::Allow,
            "the whole word cites",
        );

        assert!(!mentions_title("algum trabalho", "um"));
        assert!(!mentions_title("umbigo", "um"));
        assert!(mentions_title("segue o um.", "um"));
        assert!(mentions_title("um", "um"));
        // Uma ocorrência inválida não esconde a válida que começa dentro dela.
        assert!(mentions_title("xa b a b a", "a b a"));
    }

    /// Citar pelo título também vale, sem diferenciar maiúsculas nem quebras de
    /// linha; um subagente nunca é cobrado; e sem o texto final não há o que
    /// conferir.
    #[test]
    fn a_title_counts_as_a_citation_and_the_gate_self_restricts() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        let session = root.to_string_lossy().into_owned();

        mark_unit_closed(&session, "s-title");
        let both = "Seguem abertos: HUMANIZE e o html padrao\nda spec.";
        assert_eq!(verdict(root, &stop("s-title", both)), Verdict::Allow, "titles cite");

        mark_unit_closed(&session, "s-sub");
        let mut sub = stop("s-sub", "nada");
        sub.agent_id = Some("closure-1".to_string());
        assert_eq!(verdict(root, &sub), Verdict::Allow, "a subagent stop is never gated");

        mark_unit_closed(&session, "s-bare");
        let bare = HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s-bare".to_string()),
            ..HookInput::default()
        };
        assert_eq!(verdict(root, &bare), Verdict::Allow, "no final text, nothing to check");
    }

    /// O fechamento que acontece na continuação de um bloqueio de OUTRA regra
    /// (a clareza barrou o primeiro `Stop`, a reescrita fechou a unidade)
    /// chega com `stop_hook_active` e uma marca nova. Essa mensagem é conferida
    /// — é a que encerra o fechamento — e a marca some no `Stop` que libera: o
    /// próximo turno comum nunca herda o fechamento.
    #[test]
    fn a_closure_in_a_blocked_continuation_is_checked_and_never_leaks() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        let project = root.to_string_lossy().into_owned();
        let continuation = |message: &str| {
            let mut input = stop("s-cont", message);
            input.raw["stop_hook_active"] = json!(true);
            input
        };

        // Omite P-1: cobrada mesmo com `stop_hook_active`.
        mark_unit_closed(&project, "s-cont");
        match verdict(root, &continuation("Fechei a unidade; segue o P-2.")) {
            Verdict::Deny { reason } => assert!(reason.contains("P-1"), "names the omitted: {reason}"),
            other => panic!("the closing message of a continuation must be checked, got {other:?}"),
        }

        // A reescrita cita todas: passa, e a marca NÃO sobrevive ao turno.
        assert_eq!(verdict(root, &continuation("Seguem P-1 e P-2.")), Verdict::Allow);
        assert!(!take_unit_closed(&project, "s-cont"), "the marker was consumed by that Stop");
        assert_eq!(
            verdict(root, &stop("s-cont", "Pronto, ajustei o teste.")),
            Verdict::Allow,
            "the next ordinary turn never inherits the closure",
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed in {}", dir.display());
    }

    /// Um fechamento gravado de dentro de um worktree arma a cobrança que o
    /// `Stop` faz no checkout principal: quem grava e quem lê resolvem a marca
    /// pelo mesmo checkout principal.
    #[test]
    fn a_closure_recorded_in_a_worktree_arms_the_main_checkout() {
        let dir = tempdir().expect("tempdir");
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).expect("main");
        std::fs::write(main.join("mustard.json"), r#"{"lang":"pt-BR"}"#).expect("cfg");
        for args in [
            &["init"][..],
            &["config", "user.email", "t@example.com"],
            &["config", "user.name", "t"],
            &["config", "commit.gpgsign", "false"],
            &["commit", "--allow-empty", "-m", "root"],
        ] {
            git(&main, args);
        }
        let tree = dir.path().join("unit");
        let tree_arg = tree.to_string_lossy().into_owned();
        git(&main, &["worktree", "add", "-b", "feature/unit", &tree_arg]);
        let out = pending_at(&PendingOpts {
            root: main.clone(),
            add: true,
            title: Some("Humanize".into()),
            detail: Some("terceiro trabalho".into()),
            ..PendingOpts::default()
        });
        assert_eq!(out["ok"], json!(true), "seed: {out}");

        mark_unit_closed(&tree_arg, "s-tree");
        match verdict(&main, &stop("s-tree", "Unidade fechada.")) {
            Verdict::Deny { reason } => assert!(reason.contains("Humanize"), "{reason}"),
            other => panic!("a worktree closure must arm the main checkout's Stop, got {other:?}"),
        }
    }

    /// Um id conta inteiro: `P-10` não cita `P-1`.
    #[test]
    fn an_id_is_matched_whole() {
        assert!(mentions_id("segue o p-1.", "p-1"));
        assert!(mentions_id("(p-1)", "p-1"));
        assert!(!mentions_id("segue o p-10", "p-1"));
        assert!(!mentions_id("xp-1", "p-1"));
    }

    /// O escritor de eventos é quem arma a cobrança: gravar `pipeline.complete`
    /// ou `pr.merged` marca a sessão que o gravou, e só ela. Um evento comum não
    /// marca nada.
    #[test]
    fn a_recorded_closure_arms_the_gate_for_its_session() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        let project = root.to_string_lossy().into_owned();
        let event = |name: &str, session: &str| mustard_core::domain::model::event::HarnessEvent {
            v: mustard_core::domain::model::event::SCHEMA_VERSION,
            ts: "2026-09-10T12:00:00.000Z".to_string(),
            session_id: session.to_string(),
            wave: 0,
            actor: mustard_core::domain::model::event::Actor {
                kind: mustard_core::domain::model::event::ActorKind::Orchestrator,
                id: Some("test".to_string()),
                actor_type: None,
            },
            event: name.to_string(),
            payload: json!({}),
            spec: Some("uma-unidade".to_string()),
        };

        crate::shared::events::route::emit(&project, &event("tool.use", "s-w"));
        assert!(!take_unit_closed(&project, "s-w"), "an ordinary event is not a closure");

        for closure in ["pipeline.complete", "pr.merged"] {
            crate::shared::events::route::emit(&project, &event(closure, "s-w"));
            assert!(!take_unit_closed(&project, "s-other"), "{closure}: only its own session");
            assert_eq!(unit_closed_blocks(&project, "s-w"), Some(0), "{closure}: a fresh closure");
            assert!(take_unit_closed(&project, "s-w"), "{closure} must arm the gate");
            assert!(!take_unit_closed(&project, "s-w"), "{closure}: consumed once");
        }
    }
}
