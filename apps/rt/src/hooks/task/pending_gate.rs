//! `pending_gate` — a regra das pendências do fim da resposta ([`PendingRule`],
//! uma das regras do `end_of_turn_check`): o turno em que a spec fechou, ou
//! entrou no merge, não termina com uma mensagem que omite uma pendência
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
//! ## Quando cobra — todos os fatos precisam valer
//!
//! 1. É o `Stop` da sessão principal (nunca o de um subagente) — o
//!    `end_of_turn_check` só chama as regras nele — e ele traz o id da sessão.
//! 2. A spec atual da sessão, pela escada única, está fechada: o estado dela
//!    tem um fechamento ou uma entrega ([`State::closing`]) que esta sessão
//!    ainda não encerrou. Quem grava esse estado é a ponte do fechamento (o
//!    `complete-spec`) e a do merge (o `pr-merge`).
//! 3. O `Stop` trouxe `last_assistant_message` (o texto final do turno, campo
//!    documentado do evento). Sem ele não há o que conferir.
//! 4. Alguma pendência aberta nascida na spec — a que um evento `deferred`
//!    dela cita — não é citada nesse texto, pelo título ou pelo id (`P-3`), sem
//!    diferenciar maiúsculas. Id e título contam só inteiros: `P-1` não cita
//!    dentro de `P-10`, e o título `um` não cita dentro de `algum`. O bloqueio
//!    pede o título: a regra de clareza barra o código na conversa.
//! 5. A regra ainda não bloqueou [`MAX_BLOCKS`] vezes por este fechamento.
//!
//! Qualquer fato que falte libera o turno. Uma pendência que não nasceu na
//! spec nunca é cobrada aqui: a lista inteira fica na listagem do
//! `run pending`, e cobrá-la a cada fechamento faria cada resumo recitar tudo.
//!
//! ## Por que só no turno do fechamento
//!
//! É o momento exato da perda original. Cobrar em todo turno obrigaria cada
//! resposta curta a recitar a lista — e um aviso que sempre dispara aprende-se a
//! ignorar.
//!
//! ## O contador da cobrança
//!
//! Cada fechamento e cada merge gravam um `state` com número novo, e é esse
//! número que a regra cobra. O contador mora num arquivo da sessão, no checkout
//! principal (`.claude/.session/<sessão>/pending-charge`), que só a regra
//! grava: a spec, o número do fechamento, quantas vezes ela bloqueou e se o
//! fechamento já se encerrou.
//!
//! - **Encerra** quando nada nascido na spec está aberto, quando a mensagem
//!   cita cada pendência, quando não há texto final, ou quando já bloqueou
//!   [`MAX_BLOCKS`] vezes por este fechamento.
//! - **Bloqueia e conta** nos demais casos.
//!
//! Um fechamento novo, ou outra spec, começa a conta de novo. Bloquear sem
//! conseguir gravar o contador bloquearia sem limite; nesse caso a regra
//! libera.
//!
//! ## `stop_hook_active` não libera
//!
//! Ele não diz QUEM bloqueou. Se a clareza barra o primeiro `Stop` e a
//! reescrita fecha a spec, o `Stop` seguinte chega com `stop_hook_active` e
//! um fechamento novo — e é a mensagem que encerra o fechamento. Liberá-lo pelo
//! campo deixaria essa mensagem sem conferência (a perda original). O limite é
//! o contador: no máximo [`MAX_BLOCKS`] bloqueios por fechamento, longe do teto
//! de 8 bloqueios seguidos do Claude Code.
//!
//! ## Sem modo `MUSTARD_*_MODE`
//!
//! A regra não ganha porta de configuração. Ela já se restringe sozinha ao
//! turno do fechamento, e desligá-la devolveria exatamente a perda que ela
//! existe para impedir.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{SpecState, State};
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs;
use mustard_core::platform::i18n::Locale;
use serde::{Deserialize, Serialize};

use crate::commands::event::pending::{format_pending_items, open_born_in, OpenPending};
use crate::hooks::task::end_of_turn_check::{Finding, Turn, TurnRule};
use crate::shared::spec_state::DiskSpecState;

/// Quantas vezes, no máximo, a regra bloqueia por fechamento. Dois: um para a
/// primeira mensagem, outro para a reescrita que ainda omite — e então libera,
/// sem laço.
const MAX_BLOCKS: u32 = 2;

/// O nome do arquivo do contador, na pasta da sessão.
const CHARGE_FILE: &str = "pending-charge";

/// A regra das pendências do fim da resposta.
pub struct PendingRule;

impl TurnRule for PendingRule {
    fn check(&self, turn: &Turn<'_>) -> Option<Finding> {
        // `stop_hook_active` NÃO libera aqui (ver "`stop_hook_active` não
        // libera"): o contador é o limite.
        let session = turn.session.map(str::trim).filter(|s| !s.is_empty())?;
        let project = Path::new(turn.project_dir);

        // Fato 2 — a spec atual fechou, por um fechamento que esta sessão ainda
        // não encerrou.
        let disk = DiskSpecState::new(project);
        let spec = disk.active(Some(session))?;
        let log = disk.log(&spec)?;
        let closure = State::from_log(&log).closing()?;
        let blocks = match Charge::read(project, session) {
            Some(charge) if charge.spec == spec && charge.closure == closure => {
                if charge.settled {
                    return None;
                }
                charge.blocks
            }
            _ => 0,
        };

        // Fatos 3, 4 e 5 — liberar encerra o fechamento.
        let omitted = omitted_items(turn.message, project, &log);
        let mut charge = Charge { spec, closure, blocks, settled: false };
        if omitted.is_empty() || blocks >= MAX_BLOCKS {
            charge.settled = true;
            charge.write(project, session);
            return None;
        }

        // Bloquear conta. Sem contador gravado, libera — nunca um bloqueio sem
        // limite.
        charge.blocks = blocks + 1;
        if !charge.write(project, session) {
            return None;
        }
        Some(Finding::Block(block_reason(&omitted, turn.lang)))
    }
}

/// O contador da cobrança de um fechamento, gravado só pela regra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Charge {
    /// A spec que fechou.
    spec: String,
    /// O número do `state` de fechamento ou de entrega que está sendo cobrado.
    closure: u64,
    /// Quantas vezes a regra já bloqueou por ele.
    blocks: u32,
    /// O fechamento se encerrou: a regra liberou o `Stop` dele.
    settled: bool,
}

impl Charge {
    /// `<principal>/.claude/.session/<sessão>/pending-charge`. O checkout
    /// principal, porque a spec mora nele e um worktree cobra o mesmo
    /// fechamento. `None` para um id de sessão que sairia da pasta.
    fn path(project: &Path, session: &str) -> Option<PathBuf> {
        if session.contains(['/', '\\']) || session.starts_with('.') {
            return None;
        }
        let main = mustard_core::io::spec_events::spec_root(project);
        Some(ClaudePaths::for_project(&main).ok()?.claude_dir().join(".session").join(session).join(CHARGE_FILE))
    }

    fn read(project: &Path, session: &str) -> Option<Self> {
        let body = fs::read_to_string(&Self::path(project, session)?).ok()?;
        serde_json::from_str(&body).ok()
    }

    /// `true` quando gravou.
    fn write(&self, project: &Path, session: &str) -> bool {
        let Some(path) = Self::path(project, session) else {
            return false;
        };
        let Some(parent) = path.parent() else {
            return false;
        };
        let _ = fs::create_dir_all(parent);
        serde_json::to_vec(self).is_ok_and(|body| fs::write_atomic(&path, &body).is_ok())
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

/// O motivo do bloqueio: nomeia CADA pendência omitida — sem corte, porque o
/// próximo passo é citá-las todas — e diz as duas saídas honestas. O texto sai
/// do catálogo, no idioma do projeto.
fn block_reason(omitted: &[OpenPending], lang: Locale) -> String {
    mustard_core::translate("pending.gate.block", lang)
        .replace("{count}", &omitted.len().to_string())
        .replace("{items}", &format_pending_items(omitted, omitted.len()))
}

/// Semeia a spec `spec` do projeto em `root` em andamento, com um evento
/// `deferred` para cada pendência de `born`, e liga a sessão `session` a ela.
/// Fechar é com a ponte, `record_phase`.
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
    crate::shared::context::bind_session_spec(&root.to_string_lossy(), session, spec);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::event::pending::{open_pending, pending_at, PendingOpts};
    use crate::commands::spec_events::write::record_phase;
    use crate::hooks::task::end_of_turn_check::run_rules;
    use crate::shared::context::{mark_unit_closed, record_unit_closed_block, take_unit_closed, unit_closed_blocks};
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

    /// Um projeto instalado com as pendências abertas `titles`, numeradas na
    /// ordem (`P-1`, `P-2`, …).
    fn project_with_open_items(titles: &[&str]) -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).expect("cfg");
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

    /// A spec [`SPEC`] em que as pendências `born` nasceram, ligada à sessão
    /// `session` e fechada pela ponte.
    fn closed_spec(root: &Path, born: &[u64], session: &str) {
        seed_spec(root, SPEC, born, session);
        assert!(record_phase(root, SPEC, "closed"), "the bridge records the close");
    }

    /// A regra das pendências sozinha, como a conferência do fim da resposta
    /// a roda.
    fn verdict(root: &Path, input: &HookInput) -> Verdict {
        run_rules(&[&PendingRule], input, &ctx(root))
    }

    /// O contador da sessão, como a regra o deixou.
    fn charge(root: &Path, session: &str) -> Option<Charge> {
        Charge::read(root, session)
    }

    /// O turno em que a spec fechou e cuja mensagem final omite uma pendência
    /// aberta nascida nela é bloqueado, e o motivo NOMEIA a omitida (e só ela).
    /// A reescrita que a cita passa e encerra o fechamento.
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
        assert!(charge(root, "s-close").is_some_and(|c| c.settled), "that Stop settled the closure");
        assert_eq!(verdict(root, &stop("s-close", summary)), Verdict::Allow, "once per closure");
    }

    /// Sem fechamento, a resposta passa mesmo omitindo todas as pendências
    /// abertas; e o fechamento de uma spec de OUTRA sessão não conta.
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

        let other = project_with_two_open_items();
        closed_spec(other.path(), &[1, 2], "s-other");
        assert_eq!(
            verdict(other.path(), &stop("s-quiet", "Pronto, ajustei o teste.")),
            Verdict::Allow,
            "a spec another session is on is not this turn's",
        );
    }

    /// O contador fica no bloqueio, então o `Stop` seguinte — a reescrita, com
    /// `stop_hook_active` — confere de novo, até [`MAX_BLOCKS`] vezes; depois
    /// libera e encerra, sem laço.
    #[test]
    fn a_closure_is_charged_at_most_twice_then_released() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        closed_spec(root, &[1, 2], "s-twice");
        let omits = "Fechei a spec; segue o P-2.";

        assert!(verdict(root, &stop("s-twice", omits)).is_blocking(), "first Stop blocks");
        assert_eq!(charge(root, "s-twice").map(|c| c.blocks), Some(1), "a block is counted");

        let mut again = stop("s-twice", omits);
        again.raw["stop_hook_active"] = json!(true);
        assert!(verdict(root, &again).is_blocking(), "still omitted: checked and blocked again");
        assert_eq!(charge(root, "s-twice").map(|c| c.blocks), Some(2));

        assert_eq!(verdict(root, &again), Verdict::Allow, "the third Stop is released");
        assert!(charge(root, "s-twice").is_some_and(|c| c.settled), "and the closure settled");
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

        assert_eq!(verdict(root, &stop("s-cited", "Seguem P-1 e P-2.")), Verdict::Allow);
        assert_eq!(
            charge(root, "s-cited"),
            Some(Charge { spec: SPEC.to_string(), closure: 4, blocks: 0, settled: true }),
            "the allowing Stop settled it",
        );
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
    /// linha; um subagente nunca é cobrado; e sem o texto final não há o que
    /// conferir.
    #[test]
    fn a_title_counts_as_a_citation_and_the_gate_self_restricts() {
        let dir = project_with_two_open_items();
        let root = dir.path();
        closed_spec(root, &[1, 2], "s-title");
        for session in ["s-sub", "s-bare"] {
            crate::shared::context::bind_session_spec(&root.to_string_lossy(), session, SPEC);
        }

        let both = "Seguem abertos: HUMANIZE e o html padrao\nda spec.";
        assert_eq!(verdict(root, &stop("s-title", both)), Verdict::Allow, "titles cite");

        let mut sub = stop("s-sub", "nada");
        sub.agent_id = Some("closure-1".to_string());
        assert_eq!(verdict(root, &sub), Verdict::Allow, "a subagent stop is never gated");

        let bare = HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s-bare".to_string()),
            ..HookInput::default()
        };
        assert_eq!(verdict(root, &bare), Verdict::Allow, "no final text, nothing to check");
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

        // Omite P-1: cobrada mesmo com `stop_hook_active`.
        closed_spec(root, &[1, 2], "s-cont");
        match verdict(root, &continuation("Fechei a spec; segue o P-2.")) {
            Verdict::Deny { reason } => assert!(reason.contains("P-1"), "names the omitted: {reason}"),
            other => panic!("the closing message of a continuation must be checked, got {other:?}"),
        }

        // A reescrita cita todas: passa, e o fechamento NÃO sobrevive ao turno.
        assert_eq!(verdict(root, &continuation("Seguem P-1 e P-2.")), Verdict::Allow);
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
    /// `Stop` faz no checkout principal: a spec mora no checkout principal, e
    /// a ponte grava lá.
    #[test]
    fn a_closure_recorded_in_a_worktree_arms_the_main_checkout() {
        let dir = tempdir().expect("tempdir");
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).expect("main");
        std::fs::write(main.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).expect("cfg");
        for args in [
            &["init"][..],
            &[
                "-c",
                "user.email=t@example.com",
                "-c",
                "user.name=t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "root",
            ],
        ] {
            git(&main, args);
        }
        let tree = dir.path().join("unit");
        git(&main, &["worktree", "add", "-b", "feature/unit", &tree.to_string_lossy()]);
        let out = pending_at(&PendingOpts {
            root: main.clone(),
            add: true,
            title: Some("Humanize".into()),
            detail: Some("terceiro trabalho".into()),
            ..PendingOpts::default()
        });
        assert_eq!(out["ok"], json!(true), "seed: {out}");
        seed_spec(&main, SPEC, &[1], "s-tree");

        assert!(record_phase(&tree, SPEC, "closed"), "the close is recorded from the worktree");
        match verdict(&main, &stop("s-tree", "Spec fechada.")) {
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
        assert!(record_phase(root, SPEC, "closed"), "closed again");
        assert!(verdict(root, &stop("s-again", omits)).is_blocking(), "a second close charges again");
        assert_eq!(verdict(root, &stop("s-again", cites)), Verdict::Allow);

        assert!(record_phase(root, SPEC, "delivered"), "the merge");
        assert!(verdict(root, &stop("s-again", omits)).is_blocking(), "the merge charges again");
    }

    /// A regra de antes: armada pela marca que o gravador de eventos deixava
    /// na sessão, cobrando toda pendência aberta. Existe só para o teste lado
    /// a lado.
    struct MarkerRule;

    impl TurnRule for MarkerRule {
        fn check(&self, turn: &Turn<'_>) -> Option<Finding> {
            let project_dir = turn.project_dir;
            let session = turn.session.unwrap_or_default();
            let blocks = unit_closed_blocks(project_dir, session)?;
            let omitted: Vec<OpenPending> = if turn.message.trim().is_empty() {
                Vec::new()
            } else {
                open_pending(Path::new(project_dir)).into_iter().filter(|i| !cites(turn.message, i)).collect()
            };
            if omitted.is_empty() || blocks >= MAX_BLOCKS {
                take_unit_closed(project_dir, session);
                return None;
            }
            if !record_unit_closed_block(project_dir, session, blocks + 1) && !take_unit_closed(project_dir, session) {
                return None;
            }
            Some(Finding::Block(block_reason(&omitted, turn.lang)))
        }
    }

    /// Lado a lado — o mesmo fechamento, armado pela marca da sessão e pelo
    /// estado da spec, dá o mesmo bloqueio, com o mesmo texto, e libera do
    /// mesmo jeito.
    #[test]
    fn the_state_charges_what_the_session_mark_charged() {
        let (by_mark, by_state) = (project_with_two_open_items(), project_with_two_open_items());
        mark_unit_closed(&by_mark.path().to_string_lossy(), "s-lado");
        closed_spec(by_state.path(), &[1, 2], "s-lado");

        for message in ["Fechei; segue o p-2.", "Fechei; segue o p-2 e o Humanize."] {
            let old = run_rules(&[&MarkerRule], &stop("s-lado", message), &ctx(by_mark.path()));
            let new = verdict(by_state.path(), &stop("s-lado", message));
            assert_eq!(new, old, "one closure, one answer: {message}");
        }
    }

    /// O escritor de eventos é quem armava a cobrança de antes: gravar
    /// `pipeline.complete` ou `pr.merged` marca a sessão que o gravou, e só
    /// ela. Um evento comum não marca nada.
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
