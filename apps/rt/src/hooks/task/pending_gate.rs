//! `pending_gate` — a regra das pendências do fim da resposta ([`PendingRule`],
//! uma das regras do `end_of_turn_check`): depois que uma spec fecha, ou entra
//! no merge, a resposta não termina com uma mensagem que omite uma pendência
//! aberta nascida nela.
//!
//! ## O caso que a fez nascer
//!
//! Em 09/09/2026 três trabalhos foram combinados na ordem 2 → 3 → 1. Os dois
//! primeiros viraram pull requests e, no mesmo turno do último merge, o resumo
//! "O dia fechou assim" listou duas pendências e omitiu o terceiro trabalho. O
//! operador só descobriu no dia seguinte, perguntando. Nenhum gancho conferia o
//! texto final do assistente.
//!
//! ## Quem arma a cobrança
//!
//! A porta do fechamento e da entrega (`record_phase`) grava o `state` e, no
//! mesmo passo, arma um contador por spec e por número do fechamento, no
//! checkout principal, ao lado da lista de pendências
//! (`.claude/pending/charges.json`). A regra lê os contadores armados sem
//! perguntar qual é a spec atual: a arrumação do merge, que volta para a base,
//! a sessão desligada no fechamento e o worktree, em que o degrau da branch
//! pode não responder, não apagam a cobrança.
//!
//! Chamam essa porta o `close`, o `pr-merge` e o início da sessão, quando
//! encontra o merge feito por outra pessoa: a spec atual em "pull request
//! aberto" e o provedor dizendo que ele entrou.
//!
//! ## Quando cobra — todos os fatos precisam valer
//!
//! 1. É o `Stop` da sessão principal (nunca o de um subagente) — o
//!    `end_of_turn_check` só chama as regras nele.
//! 2. Há um fechamento armado que ainda não se encerrou, a spec continua
//!    fechada por ele (uma spec reaberta tira o contador), e ele é desta
//!    sessão: o contador guarda a sessão de quem fechou, quando ela era
//!    conhecida, e uma sessão paralela noutro worktree não gasta os bloqueios
//!    dele. Sem sessão conhecida, qualquer sessão principal é cobrada. O
//!    arquivo é lido e gravado com a trava presa.
//! 3. O `Stop` trouxe `last_assistant_message` (o texto final do turno, campo
//!    documentado do evento). Sem ele não há o que conferir.
//! 4. Alguma pendência aberta nascida na spec daquele fechamento — a que um
//!    evento `deferred` dela cita — não é citada nesse texto, pelo título ou
//!    pelo id (`P-3`), sem diferenciar maiúsculas. Id e título contam só
//!    inteiros: `P-1` não cita dentro de `P-10`, e o título `um` não cita
//!    dentro de `algum`. O bloqueio pede o título: a regra de clareza aponta
//!    o código interno como defeito na conversa, sem barrar.
//! 5. A regra ainda não bloqueou [`MAX_BLOCKS`] vezes por este fechamento, no
//!    total, em qualquer sessão.
//!
//! Qualquer fato que falte libera. Uma pendência que não nasceu na spec nunca
//! é cobrada aqui: a lista inteira fica na listagem do `run pending`.
//!
//! ## O contador
//!
//! - **Encerra** o fechamento, e o tira do arquivo, quando nada nascido na
//!   spec está aberto, quando a mensagem cita cada pendência, quando não há
//!   texto final, ou quando já bloqueou [`MAX_BLOCKS`] vezes.
//! - **Bloqueia e conta** nos demais casos.
//!
//! Um fechamento encerrado não volta: uma sessão nova depois do `/clear`, parada
//! na branch da spec, não é cobrada de novo. Um fechamento novo, ou um merge,
//! arma outro número. Bloquear sem conseguir gravar o contador bloquearia sem
//! limite; nesse caso a regra libera.
//!
//! ## `stop_hook_active` não libera
//!
//! Ele não diz QUEM bloqueou. Se esta regra barra o primeiro `Stop`, por um
//! fechamento anterior, e nesse meio-tempo a reescrita fecha a spec de novo, o
//! `Stop` seguinte chega com `stop_hook_active` e um fechamento novo — e é a
//! mensagem que encerra o fechamento novo. Liberá-lo pelo
//! campo deixaria essa mensagem sem conferência (a perda original). O limite é
//! o contador: no máximo [`MAX_BLOCKS`] bloqueios por fechamento, longe do teto
//! de 8 bloqueios seguidos do Claude Code.
//!
//! ## Sem modo `MUSTARD_*_MODE`
//!
//! A regra não ganha porta de configuração. Ela já se restringe sozinha aos
//! fechamentos armados, e desligá-la devolveria exatamente a perda que ela
//! existe para impedir.

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{SpecState, State};
use mustard_core::platform::i18n::Locale;

use crate::commands::event::pending::{armed_charges, format_pending_items, open_born_in, update_charges, OpenPending};
use crate::hooks::task::end_of_turn_check::{Finding, Turn, TurnRule};
use crate::shared::spec_state::DiskSpecState;

/// Quantas vezes, no máximo, a regra bloqueia por fechamento, no total. Dois:
/// um para a primeira mensagem, outro para a reescrita que ainda omite — e
/// então libera, sem laço.
const MAX_BLOCKS: u32 = 2;

/// A regra das pendências do fim da resposta.
pub struct PendingRule;

impl TurnRule for PendingRule {
    fn check(&self, turn: &Turn<'_>) -> Option<Finding> {
        // `stop_hook_active` NÃO libera aqui (ver "`stop_hook_active` não
        // libera"): o contador é o limite.
        let project = Path::new(turn.project_dir);
        if armed_charges(project).is_empty() {
            return None;
        }
        let session = turn.session.map(str::trim).filter(|s| !s.is_empty());
        let disk = DiskSpecState::new(project);
        let mut reasons = Vec::new();
        // Ler, contar e gravar com a trava do arquivo presa: um fechamento
        // armado por outra sessão no meio não some.
        let written = update_charges(project, |armed| {
            let mut still_armed = Vec::new();
            for mut charge in armed {
                // O fechamento armado por uma sessão conhecida é só dela: outra
                // sessão, noutro worktree, não gasta os bloqueios dele.
                if charge.session.as_deref().is_some_and(|owner| session != Some(owner)) {
                    still_armed.push(charge);
                    continue;
                }
                // A spec reaberta, ou sem arquivo, não cobra por este
                // fechamento: o contador sai.
                let Some(log) = disk.log(&charge.spec) else {
                    continue;
                };
                if State::from_log(&log).closing() != Some(charge.closure) {
                    continue;
                }
                // Encerrar tira o fechamento do arquivo; bloquear conta.
                let omitted = omitted_items(turn.message, project, &log);
                if omitted.is_empty() || charge.blocks >= MAX_BLOCKS {
                    continue;
                }
                charge.blocks += 1;
                reasons.push(block_reason(&charge.spec, &omitted, turn.lang));
                still_armed.push(charge);
            }
            still_armed
        });
        // Sem contador gravado, libera — nunca um bloqueio sem limite.
        if !written || reasons.is_empty() {
            return None;
        }
        Some(Finding::Block(reasons.join("\n\n")))
    }
}

/// As pendências abertas nascidas na spec que o texto final do turno não
/// cita. Vazio quando o `Stop` não trouxe texto final: sem texto, nada a
/// conferir.
fn omitted_items(message: &str, project: &Path, log: &SpecLog) -> Vec<OpenPending> {
    if message.trim().is_empty() {
        return Vec::new();
    }
    open_born_in(project, log).into_iter().filter(|item| !cites(message, item)).collect()
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

/// O motivo do bloqueio: nomeia a spec que fechou e CADA pendência omitida —
/// sem corte, porque o próximo passo é citá-las todas — e diz as duas saídas
/// honestas. O texto sai do catálogo, no idioma do projeto.
fn block_reason(spec: &str, omitted: &[OpenPending], lang: Locale) -> String {
    mustard_core::translate("pending.gate.block", lang)
        .replace("{spec}", spec)
        .replace("{count}", &omitted.len().to_string())
        .replace("{items}", &format_pending_items(omitted, omitted.len()))
}

/// Semeia a spec `spec` do projeto em `root` em andamento, com um evento
/// `deferred` para cada pendência de `born`, e liga a sessão `session` a ela
/// (um `session` vazio não liga nada). Fechar é com a porta do binário,
/// `record_phase`.
#[cfg(test)]
pub(crate) fn seed_spec(root: &Path, spec: &str, born: &[u64], session: &str) {
    use mustard_core::io::spec_events as store;
    use serde_json::json;
    let path = store::spec_file(root, spec).expect("a valid spec name");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("spec folder");
    let draft = |value: serde_json::Value| value.as_object().cloned().expect("an object");
    store::write(&path, "state", draft(json!({ "phase": "running" })), &[]).expect("state");
    for n in born {
        let deferred = json!({ "text": format!("pedido {n}"), "keys": ["pedido"], "pending": n, "origin": 1 });
        store::write(&path, "deferred", draft(deferred), &[]).expect("deferred");
    }
    crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, spec);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::event::pending::{pending_at, Charge, PendingOpts};
    use crate::commands::spec_events::write::record_phase;
    use crate::hooks::task::end_of_turn_check::run_rules;
    use mustard_core::domain::model::contract::{Ctx, HookInput, Trigger, Verdict};
    use serde_json::json;
    use tempfile::tempdir;

    /// A spec dos testes.
    const SPEC: &str = "trava";

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

    const PT: &str = r#"{"language":{"text":"pt-BR"}}"#;

    /// Um projeto instalado em `root` com as pendências abertas `titles`,
    /// numeradas na ordem (`P-1`, `P-2`, …).
    fn add_items(root: &Path, titles: &[&str]) {
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
    }

    fn project_with_open_items(titles: &[&str]) -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("mustard.json"), PT).expect("cfg");
        add_items(dir.path(), titles);
        dir
    }

    /// Um projeto instalado com duas pendências abertas: `P-1` "Humanize" e
    /// `P-2` "HTML padrao da spec".
    fn project_with_two_open_items() -> tempfile::TempDir {
        project_with_open_items(&["Humanize", "HTML padrao da spec"])
    }

    /// A spec [`SPEC`] em que as pendências `born` nasceram, ligada à sessão
    /// `session` e fechada pela porta do binário.
    fn closed_spec(root: &Path, born: &[u64], session: &str) {
        seed_spec(root, SPEC, born, session);
        // Sem sessão: a do processo de teste, vinda do ambiente, não entra.
        assert!(record_phase(root, SPEC, "closed", None), "the binary door records the close");
    }

    /// A regra das pendências sozinha, como a conferência do fim da resposta
    /// a roda.
    fn verdict(root: &Path, input: &HookInput) -> Verdict {
        run_rules(&[&PendingRule], input, &ctx(root))
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

    /// Um repositório com um commit, parado na branch `branch`.
    fn repo_on(dir: &Path, branch: &str) {
        git(dir, &["init", "-q"]);
        git(dir, &["checkout", "-q", "-b", branch]);
        git(
            dir,
            &["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "root"],
        );
    }

    /// O turno depois do fechamento cuja mensagem final omite uma pendência
    /// aberta nascida na spec é bloqueado, e o motivo NOMEIA a omitida (e só
    /// ela). A reescrita que a cita passa e encerra o fechamento.
    #[test]
    fn pending_gate_blocks_closing_turn_that_omits_open_item() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        closed_spec(root, &[1, 2], "s-close");

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
        assert_eq!(armed_charges(root), vec![], "that Stop settled the closure");
        assert_eq!(verdict(root, &stop("s-close", summary)), Verdict::Allow, "once per closure");
    }

    /// Sem fechamento armado, a resposta passa mesmo omitindo todas as
    /// pendências abertas.
    #[test]
    fn pending_gate_ignores_turn_without_closure() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        seed_spec(root, SPEC, &[1, 2], "s-quiet");
        assert_eq!(
            verdict(root, &stop("s-quiet", "Pronto, ajustei o teste.")),
            Verdict::Allow,
            "an ordinary turn never recites the list",
        );
    }

    /// O contador vale para o fechamento, em qualquer sessão: bloqueia no
    /// máximo [`MAX_BLOCKS`] vezes no total e depois libera e encerra, sem
    /// laço.
    #[test]
    fn a_closure_is_charged_at_most_twice_then_released() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        closed_spec(root, &[1, 2], "s-twice");
        let omits = "Fechei a spec; segue o P-2.";
        let charge = |blocks| vec![Charge { spec: SPEC.to_string(), closure: 4, blocks, session: None }];

        assert!(verdict(root, &stop("s-twice", omits)).is_blocking(), "first Stop blocks");
        assert_eq!(armed_charges(root), charge(1), "a block is counted");

        let mut again = stop("s-outra", omits);
        again.raw["stop_hook_active"] = json!(true);
        assert!(verdict(root, &again).is_blocking(), "still omitted: checked and blocked again");
        assert_eq!(armed_charges(root), charge(2), "the count is the closure's, not the session's");

        assert_eq!(verdict(root, &again), Verdict::Allow, "the third Stop is released");
        assert_eq!(armed_charges(root), vec![], "and the closure settled");
        assert_eq!(
            verdict(root, &stop("s-twice", "Pronto, ajustei o teste.")),
            Verdict::Allow,
            "the next ordinary turn never inherits the closure",
        );
    }

    /// A mensagem que cita cada pendência nascida na spec libera o `Stop` e
    /// encerra o fechamento ali.
    #[test]
    fn a_cited_closure_settles_the_charge() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        closed_spec(root, &[1, 2], "s-cited");
        assert_eq!(armed_charges(root), vec![Charge { spec: SPEC.to_string(), closure: 4, blocks: 0, session: None }]);

        assert_eq!(verdict(root, &stop("s-cited", "Seguem P-1 e P-2.")), Verdict::Allow);
        assert_eq!(armed_charges(root), vec![], "the allowing Stop settled it");
    }

    /// Um título curto conta só como palavra inteira: `um` não é citado dentro
    /// de `algum`, e é citado quando aparece solto.
    #[test]
    fn a_short_title_is_not_cited_by_a_longer_word() {
        let dir = project_with_open_items(&["um"]);
        let root = dir.path();
        closed_spec(root, &[1], "s-short");

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
    /// linha; um subagente nunca é cobrado nem gasta o contador; e sem o texto
    /// final não há o que conferir.
    #[test]
    fn a_title_counts_as_a_citation_and_the_gate_self_restricts() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        closed_spec(root, &[1, 2], "s-title");

        let mut sub = stop("s-sub", "nada");
        sub.agent_id = Some("closure-1".to_string());
        assert_eq!(verdict(root, &sub), Verdict::Allow, "a subagent stop is never gated");
        assert_eq!(armed_charges(root).len(), 1, "nor does it spend the charge");

        let both = "Seguem abertos: HUMANIZE e o html padrao\nda spec.";
        assert_eq!(verdict(root, &stop("s-title", both)), Verdict::Allow, "titles cite");
        assert_eq!(armed_charges(root), vec![]);

        let bare_dir = project_with_two_open_items();
        closed_spec(bare_dir.path(), &[1, 2], "s-bare");
        let bare = HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s-bare".to_string()),
            ..HookInput::default()
        };
        assert_eq!(verdict(bare_dir.path(), &bare), Verdict::Allow, "no final text, nothing to check");
    }

    /// O fechamento que acontece na continuação de um bloqueio de OUTRA regra
    /// (a clareza barrou o primeiro `Stop`, a reescrita fechou a spec) chega
    /// com `stop_hook_active`. Essa mensagem é conferida — é a que encerra o
    /// fechamento — e o fechamento se encerra no `Stop` que libera: o próximo
    /// turno comum nunca o herda.
    #[test]
    fn a_closure_in_a_blocked_continuation_is_checked_and_never_leaks() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        let continuation = |message: &str| {
            let mut input = stop("s-cont", message);
            input.raw["stop_hook_active"] = json!(true);
            input
        };

        closed_spec(root, &[1, 2], "s-cont");
        match verdict(root, &continuation("Fechei a spec; segue o P-2.")) {
            Verdict::Deny { reason } => assert!(reason.contains("P-1"), "names the omitted: {reason}"),
            other => panic!("the closing message of a continuation must be checked, got {other:?}"),
        }

        assert_eq!(verdict(root, &continuation("Seguem P-1 e P-2.")), Verdict::Allow);
        assert_eq!(
            verdict(root, &stop("s-cont", "Pronto, ajustei o teste.")),
            Verdict::Allow,
            "the next ordinary turn never inherits the closure",
        );
    }

    /// Depois do merge, a arrumação volta o checkout para a base e a sessão
    /// não está ligada à spec: a escada não acha spec nenhuma, e a cobrança
    /// dispara do mesmo jeito.
    #[test]
    fn a_merge_that_switches_back_to_the_base_is_still_charged() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"},"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .expect("cfg");
        repo_on(root, "feature/trava");
        add_items(root, &["Humanize"]);
        seed_spec(root, SPEC, &[1], "");
        assert!(record_phase(root, SPEC, "delivered", None), "the merge is recorded");
        git(root, &["checkout", "-q", "-b", "dev"]);
        assert_eq!(crate::shared::spec_state::active_spec(&root.to_string_lossy(), Some("s-nova")), None);

        match verdict(root, &stop("s-nova", "PR mergeado, voltei para a dev.")) {
            Verdict::Deny { reason } => assert!(reason.contains(SPEC) && reason.contains("Humanize"), "{reason}"),
            other => panic!("the merge is charged after the switch to the base, got {other:?}"),
        }
    }

    /// O fechamento desliga a sessão da spec antes do `Stop`: a cobrança não
    /// depende da ligação.
    #[test]
    fn a_closure_with_the_session_unbound_is_still_charged() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        seed_spec(root, SPEC, &[1], "s-solta");
        crate::shared::context::session::unbind_session_spec(&root.to_string_lossy(), "s-solta");
        assert!(record_phase(root, SPEC, "closed", None));

        match verdict(root, &stop("s-solta", "Spec fechada.")) {
            Verdict::Deny { reason } => assert!(reason.contains("Humanize"), "{reason}"),
            other => panic!("an unbound session is still charged, got {other:?}"),
        }
    }

    /// Um fechamento gravado de dentro de um worktree arma a cobrança no
    /// checkout principal, e o contador é um só: o `Stop` do checkout
    /// principal e o do worktree contam no mesmo fechamento.
    #[test]
    fn a_closure_recorded_in_a_worktree_arms_the_main_checkout() {
        let dir = tempdir().expect("tempdir");
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).expect("main");
        repo_on(&main, "dev");
        std::fs::write(main.join("mustard.json"), PT).expect("cfg");
        let tree = dir.path().join("unit");
        git(&main, &["worktree", "add", "-q", "-b", "feature/unit", &tree.to_string_lossy()]);
        std::fs::write(tree.join("mustard.json"), PT).expect("cfg in the worktree");
        add_items(&main, &["Humanize"]);
        seed_spec(&main, SPEC, &[1], "");

        assert!(record_phase(&tree, SPEC, "closed", None), "the close is recorded from the worktree");
        let omits = "Spec fechada.";
        match verdict(&main, &stop("s-tree", omits)) {
            Verdict::Deny { reason } => assert!(reason.contains("Humanize"), "{reason}"),
            other => panic!("a worktree closure must arm the main checkout's Stop, got {other:?}"),
        }
        assert!(verdict(&tree, &stop("s-tree", omits)).is_blocking(), "the worktree's Stop reads the same charge");
        assert_eq!(armed_charges(&tree), armed_charges(&main));
        assert_eq!(verdict(&main, &stop("s-tree", omits)), Verdict::Allow, "two blocks in all, then released");
    }

    /// Um fechamento encerrado não volta: uma sessão nova depois do `/clear`,
    /// parada na branch da spec fechada, não é cobrada.
    #[test]
    fn a_released_closure_is_never_charged_in_a_new_session() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        closed_spec(root, &[1, 2], "s-antes");
        assert_eq!(verdict(root, &stop("s-antes", "Seguem P-1 e P-2.")), Verdict::Allow);

        crate::shared::spec_state::stand_on_spec_branch(root, SPEC);
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), "s-depois", SPEC);
        assert_eq!(verdict(root, &stop("s-depois", "Oi, vamos continuar.")), Verdict::Allow);
    }

    /// A spec reaberta antes do `Stop` não é cobrada pelo fechamento de antes:
    /// o contador sai.
    #[test]
    fn a_spec_reopened_before_the_stop_is_not_charged() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        closed_spec(root, &[1, 2], "s-reaberta");
        let path = mustard_core::io::spec_events::spec_file(root, SPEC).expect("spec file");
        let running = json!({ "phase": "running" }).as_object().cloned().expect("object");
        mustard_core::io::spec_events::write(&path, "state", running, &[]).expect("reopen");

        assert_eq!(verdict(root, &stop("s-reaberta", "Voltei a mexer na spec.")), Verdict::Allow);
        assert_eq!(armed_charges(root), vec![], "the charge of a reopened spec is dropped");
    }

    /// O fechamento armado por uma sessão conhecida só cobra essa sessão: uma
    /// sessão paralela não é cobrada nem gasta os bloqueios dele. Sem sessão
    /// conhecida, qualquer sessão é cobrada.
    #[test]
    fn a_closure_armed_by_a_session_charges_only_that_session() {
        let omits = "Spec fechada.";
        let dir = project_with_two_open_items();
        let root = dir.path();
        seed_spec(root, SPEC, &[1], "");
        assert!(record_phase(root, SPEC, "closed", Some("s-dona")), "the close is recorded");
        assert_eq!(armed_charges(root).first().and_then(|c| c.session.clone()).as_deref(), Some("s-dona"));
        assert_eq!(verdict(root, &stop("s-paralela", omits)), Verdict::Allow, "another session is not charged");
        assert_eq!(armed_charges(root).first().map(|c| c.blocks), Some(0), "nor does it spend a block");
        assert!(verdict(root, &stop("s-dona", omits)).is_blocking(), "the session that closed is charged");

        let open = project_with_two_open_items();
        closed_spec(open.path(), &[1], "");
        assert_eq!(armed_charges(open.path()).first().and_then(|c| c.session.clone()), None);
        assert!(verdict(open.path(), &stop("s-qualquer", omits)).is_blocking(), "without a known session, any one");
    }

    /// Um id conta inteiro: `P-10` não cita `P-1`.
    #[test]
    fn an_id_is_matched_whole() {
        assert!(mentions_id("segue o p-1.", "p-1"));
        assert!(mentions_id("(p-1)", "p-1"));
        assert!(!mentions_id("segue o p-10", "p-1"));
        assert!(!mentions_id("xp-1", "p-1"));
    }

    /// O fechamento cobra só as pendências nascidas na spec que fechou: as
    /// outras abertas, mesmo omitidas, não entram no bloqueio.
    #[test]
    fn the_end_of_turn_check_charges_only_the_items_born_in_the_spec_that_closed() {
        let dir = project_with_open_items(&["Humanize", "HTML padrao da spec", "Painel novo"]);
        let root = dir.path();
        closed_spec(root, &[2], "s-born");

        match verdict(root, &stop("s-born", "Fechei a spec.")) {
            Verdict::Deny { reason } => {
                assert!(reason.contains("P-2") && reason.contains("HTML padrao da spec"), "{reason}");
                assert!(!reason.contains("Humanize") && !reason.contains("Painel novo"), "{reason}");
            }
            other => panic!("the item born in the spec is charged, got {other:?}"),
        }
        assert_eq!(
            verdict(root, &stop("s-born", "Fechei a spec; segue o html padrao da spec.")),
            Verdict::Allow,
            "citing what was born in the spec is enough",
        );
    }

    /// Cada fechamento e cada merge armam a cobrança de novo: depois de um
    /// fechamento encerrado, reabrir e fechar a spec cobra outra vez, e o
    /// merge também.
    #[test]
    fn every_closure_and_every_merge_charge_again() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        closed_spec(root, &[1], "s-again");
        let omits = "Fechei a spec.";
        let cites = "Fechei a spec; segue o Humanize.";

        assert!(verdict(root, &stop("s-again", omits)).is_blocking(), "the close charges");
        assert_eq!(verdict(root, &stop("s-again", cites)), Verdict::Allow);
        assert_eq!(verdict(root, &stop("s-again", omits)), Verdict::Allow, "that close is settled");

        let path = mustard_core::io::spec_events::spec_file(root, SPEC).expect("spec file");
        let running = json!({ "phase": "running" }).as_object().cloned().expect("object");
        mustard_core::io::spec_events::write(&path, "state", running, &[]).expect("reopen");
        assert_eq!(verdict(root, &stop("s-again", omits)), Verdict::Allow, "a reopened spec charges nothing");
        assert!(record_phase(root, SPEC, "closed", None), "closed again");
        assert!(verdict(root, &stop("s-again", omits)).is_blocking(), "a second close charges again");
        assert_eq!(verdict(root, &stop("s-again", cites)), Verdict::Allow);

        assert!(record_phase(root, SPEC, "delivered", None), "the merge");
        assert!(verdict(root, &stop("s-again", omits)).is_blocking(), "the merge charges again");
    }

}
