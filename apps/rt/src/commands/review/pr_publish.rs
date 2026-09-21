//! `mustard-rt run pr-open` / `pr-edit` / `pr-ready` — the pull-request
//! PUBLISH actions, behind the provider port.
//!
//! Until this module, the `/mustard:pr` and `/git` prose told the model to run
//! `rtk gh pr create/edit/ready` directly — `github` fixed in text no test
//! covers. Each command here resolves the provider IN FORCE through
//! [`provider_for`] (`git.provider` declared wins, then the `origin` remote,
//! then the fallback) and speaks only the [`PrProvider`] port, so WHICH
//! provider answers is an internal detail: the prose names this command, never
//! a CLI.
//!
//! ## The contract of every answer
//!
//! One JSON document, always exit 0: [`PrPublishReport`] with `ok`, the
//! `action` taken, the `provider` that was asked, and — on success — the
//! `number`/`url` the action itself proved. Failure degrades into the `error`
//! FIELD (the port's stable tokens, or the CLI's own stderr), never a panic
//! and never a non-zero exit: `clippy::unwrap_used` is `deny` crate-wide and
//! the caller is prose that must keep reading JSON whatever the network did.
//!
//! ## Nobody writes the title or the body
//!
//! `pr-open` takes no `--title` and no `--body-file`. The message is BUILT
//! from the spec's own event file — the goal for the title, the recorded
//! summary and what each wave delivered for the body — by
//! [`mustard_core::domain::spec_events::pr_message`], the one place the
//! template and its limits live. A hand-written `pr-body.md` was a fourth file
//! in the spec folder, committed with it, and stale the moment the next round
//! landed; a gate existed only to notice it had gone stale.
//!
//! A repository with no spec of its own has no such file to
//! read, and that is what `--fill` is for: the title and body come from the
//! commits the branch carries.
//!
//! ## An open pull request is EDITED, never opened twice
//!
//! When the head branch already has a pull request, `pr-open` rewrites its
//! body instead of asking the provider to create a second one. That is also
//! how each round refreshes the body: the same call, from the same door.
//!
//! ## Os submódulos primeiro
//!
//! Com submódulo mexido pela spec — o ponteiro dele muda na branch da spec —,
//! o `pr-open` envia a branch de mesmo nome de cada submódulo e abre, primeiro,
//! o pull request dela contra a base daquele repositório; só então abre o do
//! principal, como rascunho enquanto algum deles não entrou. A resposta traz o
//! endereço de todos. O principal fica pronto pela conferência dos pull
//! requests dos submódulos, no `pr-merge` e no início da sessão.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use mustard_core::domain::spec_events::pr_message;
use mustard_core::platform::i18n::translate;

use crate::commands::git_settle::{bump_pointers, submodule_base, submodule_tip_landed, unit_submodules};
use crate::commands::review::pr_door::project_root;
use crate::shared::branch_state::PrStatus;
use crate::shared::pr_provider::{provider_for, provider_in, PrProvider, PrRef, PrToOpen, PrView};
use mustard_core::domain::spec_state::SpecState;

use crate::shared::spec_state::DiskSpecState;

/// The `action` vocabulary — closed, `&'static str` so the report stays
/// byte-stable and no caller re-parses a free string.
const ACTION_OPEN: &str = "open";
const ACTION_EDIT: &str = "edit";
/// O pull request do submódulo que já entrou: nada foi enviado nem reescrito.
const ACTION_MERGED: &str = "merged";

// ---------------------------------------------------------------------------
// The report — the one JSON document each command answers
// ---------------------------------------------------------------------------

/// What one publish action answered.
///
/// `ok: false` means the provider did not perform the action — the reason is
/// in `error`, verbatim (the port's stable tokens such as
/// `provider-unsupported` / `gh-not-found`, or the provider CLI's own stderr).
/// Optional fields are skipped when absent so the emitted JSON stays
/// byte-identical run over run.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct PrPublishReport {
    /// False only when the action was NOT performed.
    pub ok: bool,
    /// Which action was asked: `open`, `edit` or `ready`.
    pub action: &'static str,
    /// The provider token that was asked (`resolve_provider`'s vocabulary) —
    /// named even on failure, so the operator knows WHO refused.
    pub provider: String,
    /// The PR number the action proved (`open`) or acted on (`edit`/`ready`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// The PR's web URL — only `open` learns it, from the create itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Why the action did not happen. Absent on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What the operator should know even though the action HAPPENED — today,
    /// that the spec's criteria have not all passed.
    ///
    /// It used to be a Bash-tool stage that watched for a typed `gh pr create`
    /// and warned there. That only ever reached the person who typed the
    /// provider's command line by hand; the door that really opens the pull
    /// request said nothing. The warning belongs where the action is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl PrPublishReport {
    /// The failure shape every builder degrades to: the action and provider
    /// still named, the reason in the field, `number` echoing what the caller
    /// pointed at (when it pointed at one).
    fn failed(action: &'static str, provider: String, number: Option<u64>, error: String) -> Self {
        Self {
            ok: false,
            action,
            provider,
            number,
            url: None,
            error: Some(error),
            warning: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Builders — pure over the port, so a table of fakes proves every shape
// ---------------------------------------------------------------------------

/// Ask `provider` to open `pr` and fold the answer into the report.
///
/// On success the report carries exactly what the create itself proved
/// (number + URL, per [`crate::shared::pr_provider::PrOpened`]); on failure
/// there is no number to echo — nothing was created to point at.
#[must_use]
pub(crate) fn open_report(provider: &dyn PrProvider, pr: &PrToOpen) -> PrPublishReport {
    let name = provider.provider().to_string();
    match provider.open(pr) {
        Ok(opened) => PrPublishReport {
            ok: true,
            action: ACTION_OPEN,
            provider: name,
            number: Some(opened.number),
            url: Some(opened.url),
            error: None,
            warning: None,
        },
        Err(error) => PrPublishReport::failed(ACTION_OPEN, name, None, error),
    }
}

/// Ask `provider` to replace the body of PR `number`.
///
/// The number is echoed on BOTH outcomes — it identifies which PR the edit
/// was for, and the caller handed it in.
#[must_use]
pub(crate) fn edit_report(provider: &dyn PrProvider, number: u64, body: &str) -> PrPublishReport {
    let name = provider.provider().to_string();
    match provider.edit_body(number, body) {
        Ok(()) => PrPublishReport {
            ok: true,
            action: ACTION_EDIT,
            provider: name,
            number: Some(number),
            url: None,
            error: None,
            warning: None,
        },
        Err(error) => PrPublishReport::failed(ACTION_EDIT, name, Some(number), error),
    }
}

/// Abre o pull request de `pr`, ou reescreve o corpo daquele que a branch
/// dele já carrega — desde que ele ainda esteja ABERTO.
///
/// A pergunta é sempre pela BRANCH que se vai abrir, nunca pela do checkout:
/// a branch chega por opção, e as duas não são a mesma coisa. Perguntando pelo
/// checkout, a porta reescrevia o corpo do pull request de outra unidade e
/// relatava ter editado aquele número.
///
/// A busca pela branch pode devolver um pull request já juntado ou fechado —
/// a branch continua existindo, e o provedor não esquece o histórico dela. Só
/// o estado aberto é editado; qualquer outro (juntado, fechado, desconhecido)
/// segue para `open_report`, que abre um pedido novo em vez de mexer no
/// antigo.
#[must_use]
pub(crate) fn open_or_edit(provider: &dyn PrProvider, pr: &PrToOpen) -> PrPublishReport {
    match provider.view(PrRef::Head(&pr.head)) {
        // O endereço vem da consulta: é o que a fase de pull request aberto
        // grava junto com o número. Só o estado Open é reescrito — juntado
        // ou fechado seguem para open_report, como se a busca não tivesse
        // achado nada.
        Ok(view) if view.status == PrStatus::Open => PrPublishReport {
            url: Some(view.url).filter(|url| !url.trim().is_empty()),
            ..edit_report(provider, view.number, &pr.body)
        },
        Ok(_) | Err(_) => open_report(provider, pr),
    }
}

/// Refaz o corpo do pull request que a branch `head` carrega. Devolve o número
/// reescrito, `None` quando não há pull request aberto para ela ou quando o
/// provedor não respondeu.
///
/// A mesma pergunta de [`open_or_edit`], feita de uma vez só para as duas
/// portas: a branch em jogo é quem aponta o pull request, e só o estado ABERTO
/// é reescrito. A busca pela branch devolve também o pull request já juntado
/// ou fechado — o provedor não esquece o histórico dela —, e reescrever o
/// corpo dele mexia num pedido que já saiu. Aqui não há o que abrir no lugar:
/// a rodada só refaz o que está aberto, e sem isso ela não para.
pub(crate) fn rewrite_body(provider: &dyn PrProvider, head: &str, body: &str) -> Option<u64> {
    let view = provider.view(PrRef::Head(head)).ok().filter(|view| view.status == PrStatus::Open)?;
    provider.edit_body(view.number, body).ok().map(|()| view.number)
}

// ---------------------------------------------------------------------------
// CLI faces — resolve the root, pick the adapter, build the report, print it
// ---------------------------------------------------------------------------

/// Print one report as the single JSON document the command answers with.
fn emit<T: Serialize>(report: &T) {
    println!("{}", serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string()));
}

/// The title and body `--fill` derives from the commits `base..head` carries —
/// the shape a repository with no spec needs:
/// title = the newest commit's subject (a small unit usually has one), body
/// = the whole `git log --oneline` of the range. Answers `Err` with git's own
/// words when the range cannot be read, so the report names the reason.
fn fill_from_commits(repo: &Path, base: &str, head: &str) -> Result<(String, String), String> {
    let log = mustard_core::platform::git::run(repo, &["log", "--format=%s", &format!("{base}..{head}")])
        .result()
        .map_err(|err| if err.is_empty() { "git-log-failed".to_string() } else { err })?;
    let subjects: Vec<String> = log
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let Some(title) = subjects.first().cloned() else {
        return Err(format!("nothing-to-fill: {base}..{head} carries no commits"));
    };
    let body = subjects.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n");
    Ok((title, body))
}

/// What the operator must be told even when the pull request DOES open: the
/// spec's criteria have not all passed.
///
/// `None` only when every criterion of the spec has a passing last run — the
/// same reading the close doors refuse on, so the warning and the refusal can
/// never disagree about whether the unit is verified. A spec whose event file
/// cannot be read, or which states no criterion at all, is NOT verified, and
/// saying nothing there would be the one silence that matters.
pub(crate) fn qa_warning(repo: &Path, spec: &str) -> Option<String> {
    let lang = mustard_core::ProjectConfig::load(repo).language().text_or_default();
    let qa = DiskSpecState::new(repo)
        .log(spec)
        .map(|log| mustard_core::domain::spec_state::qa(&log))
        .unwrap_or_default();
    if qa.passed_all() {
        return None;
    }
    Some(
        mustard_core::platform::i18n::translate("pr.qa_pending", lang)
            .replace("{spec}", spec)
            .replace("{passed}", &qa.passed.to_string())
            .replace("{criteria}", &qa.criteria.to_string()),
    )
}

/// The pull request's title and body for `spec`, built from its event file.
///
/// The refusal travels as the stable reason plus the sentence the operator
/// reads, because both halves matter here: prose branches on the reason and a
/// person has to be told WHICH excerpt is the problem.
pub(crate) fn message_of(repo: &Path, spec: &str) -> Result<(String, String), String> {
    let lang = mustard_core::ProjectConfig::load(repo).language().text_or_default();
    let Some(log) = DiskSpecState::new(repo).log(spec) else {
        return Err(format!("spec-events-unreadable: {spec}"));
    };
    pr_message(&log).map_err(|refusal| format!("{}: {}", refusal.reason(), refusal.message(lang)))
}

/// O pull request de um submódulo da spec, como a abertura o deixou.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct SubmodulePr {
    /// O caminho do submódulo no principal.
    pub path: String,
    #[serde(flatten)]
    pub report: PrPublishReport,
}

/// A resposta do `pr-open`: a do principal, os pull requests dos submódulos,
/// abertos antes dele, e o aviso de que ele segue como rascunho.
#[derive(Debug, Serialize)]
struct OpenAnswer {
    #[serde(flatten)]
    principal: PrPublishReport,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    submodules: Vec<SubmodulePr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

/// Envia a branch `head` do submódulo `sub` do principal `repo` e abre, ou
/// reescreve, o pull request dela contra a base daquele repositório, com o
/// título e o corpo da spec. O pull request que já entrou fica como está —
/// desde que a ponta local seja a que entrou: um commit que o merge não levou
/// para com o motivo, antes de o principal abrir. Devolve a resposta e se ele
/// já entrou.
fn open_submodule(repo: &Path, sub: &str, head: &str, (title, body): (&str, &str)) -> (SubmodulePr, bool) {
    let dir = repo.join(sub);
    let provider = provider_in(repo, &dir);
    let name = provider.provider().to_string();
    let entry = |report: PrPublishReport| SubmodulePr { path: sub.to_string(), report };
    let failed = |error: String| (entry(PrPublishReport::failed(ACTION_OPEN, name.clone(), None, error)), false);
    let Some(base) = submodule_base(&dir, head) else {
        return failed(format!("submodule-base-unknown: {sub}"));
    };
    if let Ok(view) = provider.view(PrRef::Head(head))
        && view.status == PrStatus::Merged
    {
        if let Err(reason) = submodule_tip_landed(repo, sub, head, &base) {
            return failed(reason);
        }
        let report = PrPublishReport {
            ok: true,
            action: ACTION_MERGED,
            provider: name.clone(),
            number: Some(view.number),
            url: Some(view.url).filter(|url| !url.trim().is_empty()),
            error: None,
            warning: None,
        };
        return (entry(report), true);
    }
    if let Err(error) = mustard_core::platform::git::run(&dir, &["push", "-q", "origin", head]).result() {
        return failed(format!("push {sub}: {error}"));
    }
    let pr = PrToOpen {
        title: title.to_string(),
        body: body.to_string(),
        head: head.to_string(),
        base,
        draft: false,
    };
    (entry(open_or_edit(provider.as_ref(), &pr)), false)
}

/// Os pull requests dos submódulos de uma spec com o pull request do
/// principal aberto, e o que a conferência fez com eles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SubmodulePrs {
    /// O pull request do principal.
    pub pr: u64,
    /// Os submódulos cujo pull request entrou.
    pub landed: Vec<String>,
    /// Os submódulos cujo ponteiro foi levado ao principal e enviado agora.
    pub bumped: Vec<String>,
    /// Os submódulos cujo pull request ainda não entrou: o principal segue
    /// como rascunho.
    pub waiting: Vec<String>,
    /// O pull request do principal ficou pronto agora.
    pub ready: bool,
    /// Por que a pergunta, o ponteiro, o envio ou o pronto não aconteceram.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

impl SubmodulePrs {
    /// A frase do estado: o que travou, o que falta ou o principal pronto.
    /// `None` quando não há nada a dizer.
    pub(crate) fn text(&self, lang: mustard_core::platform::i18n::Locale) -> Option<String> {
        let pr = self.pr.to_string();
        if let Some(reason) = &self.problem {
            return Some(translate("pr.submodules.stuck", lang).replace("{pr}", &pr).replace("{reason}", reason));
        }
        if !self.waiting.is_empty() {
            let paths = self.waiting.join(", ");
            return Some(translate("pr.submodules.waiting", lang).replace("{pr}", &pr).replace("{paths}", &paths));
        }
        self.ready.then(|| {
            let paths = self.landed.join(", ");
            translate("pr.submodules.ready", lang).replace("{pr}", &pr).replace("{paths}", &paths)
        })
    }
}

/// Com o pull request `principal` da spec `spec` aberto, pergunta ao provedor
/// de cada submódulo que a spec mexe pelo pull request da branch dela. Cada um
/// que entrou vai para a conferência do ponteiro, que decide pelo fato — o
/// ponteiro gravado no principal já está na base do submódulo, depois de
/// buscar? — e não pela branch ainda estar na máquina, que não diz nada sobre
/// o ponteiro. O que se mover é comitado no principal, que envia a branch; sem
/// nenhum faltando e sem nada travado, o pull request do principal que ainda é
/// rascunho fica pronto. Enquanto falta algum, ele segue como rascunho, e a
/// resposta diz qual falta. `None` quando a spec não mexe em submódulo.
pub(crate) fn submodules_landed(repo: &Path, spec: &str, principal: &PrView) -> Option<SubmodulePrs> {
    let log = DiskSpecState::new(repo).log(spec)?;
    let state = mustard_core::domain::spec_state::State::from_log(&log);
    let (branch, base) = (state.branch?, state.base?);
    let subs = unit_submodules(repo, &base, &branch);
    if subs.is_empty() {
        return None;
    }
    let mut found = SubmodulePrs {
        pr: principal.number,
        landed: Vec::new(),
        bumped: Vec::new(),
        waiting: Vec::new(),
        ready: false,
        problem: None,
    };
    for sub in subs {
        let dir = repo.join(&sub);
        match provider_in(repo, &dir).view(PrRef::Head(&branch)) {
            Ok(view) if view.status == PrStatus::Merged => found.landed.push(sub),
            Ok(_) => found.waiting.push(sub),
            Err(reason) => {
                found.problem.get_or_insert(format!("{sub}: {reason}"));
                found.waiting.push(sub);
            }
        }
    }
    if !found.landed.is_empty() {
        let cfg = mustard_core::ProjectConfig::load(repo);
        let title = translate("pr.pointer_commit", cfg.language().text_or_default());
        match bump_pointers(repo, &branch, &found.landed, title, cfg.git.delete_remote_branch) {
            Ok(moved) => found.bumped = moved,
            Err(reason) => found.problem = Some(reason),
        }
    }
    if found.waiting.is_empty() && found.problem.is_none() && principal.draft {
        match provider_for(repo).ready(principal.number) {
            Ok(()) => found.ready = true,
            Err(reason) => found.problem = Some(reason),
        }
    }
    Some(found)
}

/// Como o pull request do principal de uma spec é apontado.
pub(crate) enum SpecPr {
    Number(u64),
    Head(String),
}

impl SpecPr {
    pub(crate) fn as_ref(&self) -> PrRef<'_> {
        match self {
            Self::Number(number) => PrRef::Number(*number),
            Self::Head(branch) => PrRef::Head(branch),
        }
    }
}

/// O pull request do principal da spec `spec`: o número gravado no "pull
/// request aberto" mais novo e, sem ele, a branch da spec.
pub(crate) fn spec_pr(repo: &Path, spec: &str) -> Option<SpecPr> {
    use mustard_core::domain::spec_events::{Block, BlockQuery};

    let log = DiskSpecState::new(repo).log(spec)?;
    let number = log
        .block(BlockQuery::Block(Block::State))
        .into_iter()
        .filter(|e| e.event_type == "state" && e.str_field("phase") == Some("pr_open"))
        .max_by_key(|e| e.id)
        .and_then(|e| e.fields.get("pr").and_then(|pr| pr.get("number")).and_then(Value::as_u64));
    match (number, mustard_core::domain::spec_state::State::from_log(&log).branch) {
        (Some(number), _) => Some(SpecPr::Number(number)),
        (None, Some(branch)) => Some(SpecPr::Head(branch)),
        (None, None) => None,
    }
}

/// Dispatch `mustard-rt run pr-open`.
///
/// The body comes from the spec's event file, or from the commits with
/// `--fill` (a repository with no spec of its own). A head branch that
/// already carries a pull request has its body REWRITTEN — the door never asks
/// for a second one. Com submódulo mexido pela spec, os pull requests deles
/// abrem antes, e o do principal abre como rascunho enquanto algum não
/// entrou; o que um submódulo recusa para a abertura antes do principal.
pub fn run_open(root: &Path, base: &str, head: &str, spec: Option<&str>, fill: bool, draft: bool) {
    let started = std::time::Instant::now();
    let repo = project_root(root);
    let provider = provider_for(&repo);
    let sourced = if fill {
        fill_from_commits(&repo, base, head)
    } else {
        match spec.map(str::trim).filter(|s| !s.is_empty()) {
            Some(slug) => message_of(&repo, slug),
            None => Err("spec-missing: pass --spec or --fill".to_string()),
        }
    };
    let slug = spec.map(str::trim).filter(|s| !s.is_empty());
    let warning = slug.and_then(|slug| qa_warning(&repo, slug));
    let subs = if slug.is_some() && !fill { unit_submodules(&repo, base, head) } else { Vec::new() };
    let mut submodules: Vec<SubmodulePr> = Vec::new();
    let mut waiting: Vec<String> = Vec::new();
    let mut refused: Option<String> = None;
    if let Ok((title, body)) = &sourced {
        for sub in &subs {
            let (entry, merged) = open_submodule(&repo, sub, head, (title, body));
            if !entry.report.ok {
                refused = Some(format!("{sub}: {}", entry.report.error.clone().unwrap_or_default()));
                submodules.push(entry);
                break;
            }
            if !merged {
                waiting.push(sub.clone());
            }
            submodules.push(entry);
        }
    }
    let name = provider.provider().to_string();
    let mut report = match (sourced, refused) {
        // Já existe pull request para a branch que se ia abrir: o corpo é
        // reescrito, e nenhum segundo pull request nasce.
        (Ok((title, body)), None) => {
            let pr = PrToOpen {
                title,
                body,
                head: head.to_string(),
                base: base.to_string(),
                draft: draft || !waiting.is_empty(),
            };
            open_or_edit(provider.as_ref(), &pr)
        }
        (Ok(_), Some(error)) | (Err(error), _) => PrPublishReport::failed(ACTION_OPEN, name, None, error),
    };
    report.warning = warning;
    let lang = mustard_core::ProjectConfig::load(&repo).language().text_or_default();
    let hint = (report.ok && !waiting.is_empty()).then(|| {
        translate("pr.submodules.waiting", lang)
            .replace("{pr}", &report.number.map(|n| n.to_string()).unwrap_or_default())
            .replace("{paths}", &waiting.join(", "))
    });
    // Com o pull request aberto ou reescrito, a spec passa à fase de pull
    // request aberto, com o número e o endereço: é por ela que o início da
    // sessão percebe o merge feito por outra pessoa.
    if let (true, Some(slug), Some(number)) = (report.ok, slug, report.number) {
        crate::commands::spec_events::write::record_pr_open(&repo, slug, number, report.url.as_deref());
    }
    let answer = OpenAnswer { principal: report, submodules, hint };
    let shown = serde_json::to_value(&answer).unwrap_or_default();
    let _ = crate::commands::spec_events::conversation::record_call(&repo, "pr-open", spec, started, &shown);
    emit(&shown);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use crate::shared::pr_provider::{PrChecks, PrOpened, PrView};

    /// A [`PrProvider`] answering from a table, like `branch_state`'s
    /// `FakePr`: each operation's answer is set up front, and every call is
    /// recorded so a test can assert WHAT the builder handed the port.
    struct FakePub {
        name: &'static str,
        open: Result<PrOpened, String>,
        edit: Result<(), String>,
        ready: Result<(), String>,
        /// O pull request que este provedor já tem aberto, com a branch que ele
        /// leva. `None` = nenhum, e a consulta responde erro.
        opened_for: Option<(String, u64)>,
        /// O estado que a consulta devolve para `opened_for` — `Open` por
        /// padrão. `with_landed_pr` o troca para provar que um pull request
        /// juntado ou fechado nunca é reescrito.
        status: PrStatus,
        seen: RefCell<Vec<String>>,
    }

    impl FakePub {
        /// A provider on which every operation succeeds.
        fn green(name: &'static str) -> Self {
            Self {
                name,
                open: Ok(PrOpened { number: 7, url: "https://example.test/pr/7".into() }),
                edit: Ok(()),
                ready: Ok(()),
                opened_for: None,
                status: PrStatus::Open,
                seen: RefCell::new(Vec::new()),
            }
        }

        /// O mesmo provedor, já com um pull request aberto para a branch
        /// `head`.
        fn with_open_pr(name: &'static str, head: &str, number: u64) -> Self {
            Self { opened_for: Some((head.to_string(), number)), ..Self::green(name) }
        }

        /// O mesmo provedor, com um pull request JÁ JUNTADO (ou fechado) para
        /// a branch `head` — o caso que `open_or_edit` deve tratar como se a
        /// busca não tivesse achado nada.
        fn with_landed_pr(name: &'static str, head: &str, number: u64, status: PrStatus) -> Self {
            Self { opened_for: Some((head.to_string(), number)), status, ..Self::green(name) }
        }

        /// A provider on which every operation answers `token` — the shape of
        /// an absent CLI, a refused create, or an unadapted provider.
        fn red(name: &'static str, token: &str) -> Self {
            Self {
                name,
                open: Err(token.to_string()),
                edit: Err(token.to_string()),
                ready: Err(token.to_string()),
                opened_for: None,
                status: PrStatus::Open,
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl PrProvider for FakePub {
        fn provider(&self) -> &str {
            self.name
        }

        fn open(&self, pr: &PrToOpen) -> Result<PrOpened, String> {
            self.seen.borrow_mut().push(format!(
                "open title={} head={} base={} draft={}",
                pr.title, pr.head, pr.base, pr.draft
            ));
            self.open.clone()
        }

        fn edit_body(&self, number: u64, body: &str) -> Result<(), String> {
            self.seen.borrow_mut().push(format!("edit {number} body={body}"));
            self.edit.clone()
        }

        fn ready(&self, number: u64) -> Result<(), String> {
            self.seen.borrow_mut().push(format!("ready {number}"));
            self.ready.clone()
        }

        fn view(&self, which: PrRef<'_>) -> Result<PrView, String> {
            // O que foi perguntado fica registrado: é o ponto do critério —
            // a pergunta é pela branch em jogo, nunca pela do checkout.
            self.seen.borrow_mut().push(match which {
                PrRef::Number(n) => format!("view number={n}"),
                PrRef::Head(head) => format!("view head={head}"),
                PrRef::Checkout => "view checkout".to_string(),
            });
            let PrRef::Head(asked) = which else {
                return Err("view-not-under-test".to_string());
            };
            let Some((head, number)) = self.opened_for.as_ref().filter(|(h, _)| h == asked) else {
                return Err("no-pr-for-branch".to_string());
            };
            Ok(PrView {
                number: *number,
                title: "the unit".into(),
                head: head.clone(),
                base: "dev".into(),
                status: self.status,
                merge_status: None,
                draft: true,
                url: format!("https://example.test/pr/{number}"),
            })
        }

        fn checks(&self, _number: u64) -> Result<PrChecks, String> {
            // Publishing never asks the provider's checks — the merge door
            // does. Answering an `Err` here keeps that visible: a version that
            // started asking would fail loudly instead of reading a green.
            Err("checks-not-under-test".to_string())
        }

        fn branch_protection(&self, _branch: &str) -> Result<bool, String> {
            // Publicar não pergunta o que o servidor protege — o diagnóstico
            // pergunta. Responder erro aqui mantém isso visível.
            Err("protection-not-under-test".to_string())
        }
    }

    fn to_open() -> PrToOpen {
        PrToOpen {
            title: "the unit".into(),
            body: "b".into(),
            head: "feature/my-unit".into(),
            base: "dev".into(),
            draft: true,
        }
    }

    /// A successful open answers exactly what the create proved — number and
    /// URL — names the provider, and carries no error field at all.
    #[test]
    fn pr_open_reports_through_the_port() {
        let fake = FakePub::green("github");
        let report = open_report(&fake, &to_open());
        assert_eq!(
            report,
            PrPublishReport {
                ok: true,
                action: "open",
                provider: "github".into(),
                number: Some(7),
                url: Some("https://example.test/pr/7".into()),
                error: None,
                warning: None,
            }
        );
        assert_eq!(
            fake.seen.borrow().as_slice(),
            ["open title=the unit head=feature/my-unit base=dev draft=true"],
            "the port receives the request verbatim",
        );
        let json = serde_json::to_string(&report).unwrap_or_default();
        assert!(!json.contains("error"), "absent fields are skipped, byte-stably: {json}");
    }

    /// A porta pergunta ao provedor pelo pull request da BRANCH que ela vai
    /// abrir, nunca pelo da branch em que o checkout está.
    ///
    /// Com a branch chegando por opção as duas são diferentes: perguntando
    /// pelo checkout, a porta reescrevia o corpo do pull request de outra
    /// unidade e relatava ter editado aquele número. A reescrita que a rodada
    /// faz passa pela mesma pergunta, pela mesma porta.
    #[test]
    fn a_abertura_pergunta_pelo_pull_request_da_branch_que_vai_abrir() {
        let pr = to_open();

        // Sem pull request para essa branch, um novo nasce.
        let fake = FakePub::green("github");
        let report = open_or_edit(&fake, &pr);
        assert_eq!(report.action, ACTION_OPEN, "{report:?}");
        assert_eq!(
            fake.seen.borrow().first().map(String::as_str),
            Some("view head=feature/my-unit"),
            "a pergunta não foi pela branch que se ia abrir: {:?}",
            fake.seen.borrow(),
        );

        // Com um pull request aberto para ela, o corpo dele é reescrito, e o
        // número relatado é o dele.
        let fake = FakePub::with_open_pr("github", "feature/my-unit", 42);
        let report = open_or_edit(&fake, &pr);
        assert_eq!(report.action, ACTION_EDIT, "{report:?}");
        assert_eq!(report.number, Some(42), "{report:?}");

        // E o pull request de OUTRA branch nunca é tocado.
        let fake = FakePub::with_open_pr("github", "dev_outra-unidade", 13);
        let report = open_or_edit(&fake, &pr);
        assert_eq!(
            report.action, ACTION_OPEN,
            "o pull request de outra unidade foi reescrito: {report:?}",
        );
        assert!(
            !fake.seen.borrow().iter().any(|call| call.starts_with("edit 13")),
            "o corpo de outra unidade foi reescrito: {:?}",
            fake.seen.borrow(),
        );

        // A reescrita da rodada faz a mesma pergunta.
        let fake = FakePub::with_open_pr("github", "feature/my-unit", 42);
        assert_eq!(rewrite_body(&fake, "feature/my-unit", "outro corpo"), Some(42));
        assert_eq!(
            rewrite_body(&fake, "dev_outra-unidade", "outro corpo"),
            None,
            "a rodada reescreveu o corpo de um pull request que não é o da spec",
        );
    }

    /// Um pull request já juntado nunca é editado de novo, nas DUAS portas —
    /// a da abertura e a da rodada, lado a lado: a busca pela branch pode
    /// devolvê-lo mesmo depois de fechado, e nenhuma delas pode tratar isso
    /// como "a branch tem um pull request aberto". A porta da abertura abre um
    /// pedido novo no lugar; a da rodada, que não abre nada, responde `None` e
    /// não reescreve nada. Aberto, as duas reescrevem, e o número que volta é
    /// o dele.
    ///
    /// Em 18/09 a porta da abertura reescreveu o texto do pedido 278, já
    /// juntado; a porta da rodada ficou com o mesmo defeito de pé. O mesmo
    /// vale para um pull request fechado sem juntar.
    #[test]
    fn a_merged_pull_request_is_never_edited() {
        let pr = to_open();

        for status in [PrStatus::Merged, PrStatus::Closed] {
            let fake = FakePub::with_landed_pr("github", "feature/my-unit", 278, status);
            let report = open_or_edit(&fake, &pr);
            assert_eq!(
                report.action, ACTION_OPEN,
                "um pull request {status:?} foi editado em vez de abrir um novo: {report:?}",
            );
            assert_eq!(report.number, Some(7), "{report:?}");
            assert!(
                !fake.seen.borrow().iter().any(|call| call.starts_with("edit 278")),
                "o pedido já juntado foi reescrito: {:?}",
                fake.seen.borrow(),
            );

            // A porta da rodada, pela mesma branch e com o mesmo estado.
            let fake = FakePub::with_landed_pr("github", "feature/my-unit", 278, status);
            assert_eq!(
                rewrite_body(&fake, "feature/my-unit", "outro corpo"),
                None,
                "a rodada reescreveu o corpo de um pull request {status:?}",
            );
            assert!(
                !fake.seen.borrow().iter().any(|call| call.starts_with("edit 278")),
                "o pedido {status:?} foi reescrito pela rodada: {:?}",
                fake.seen.borrow(),
            );
        }

        // Aberto, a rodada reescreve: é a divisa entre o que ela toca e o que
        // não toca.
        let fake = FakePub::with_open_pr("github", "feature/my-unit", 278);
        assert_eq!(rewrite_body(&fake, "feature/my-unit", "outro corpo"), Some(278));
        assert!(fake.seen.borrow().iter().any(|call| call == "edit 278 body=outro corpo"), "{:?}", fake.seen.borrow());
    }

    /// Every failure — table-driven over the two actions — degrades into the
    /// `error` field with the provider still named: `ok:false`, exit stays 0,
    /// and `open` echoes NO number because nothing was created to point at.
    #[test]
    fn failures_degrade_into_the_error_field_never_a_panic() {
        let fake = FakePub::red("azure", "provider-unsupported");
        let cases: Vec<(PrPublishReport, &str, Option<u64>)> = vec![
            (open_report(&fake, &to_open()), "open", None),
            (edit_report(&fake, 12, "new body"), "edit", Some(12)),
        ];
        for (report, action, number) in cases {
            assert_eq!(
                report,
                PrPublishReport {
                    ok: false,
                    action: if action == "open" { ACTION_OPEN } else { ACTION_EDIT },
                    provider: "azure".into(),
                    number,
                    url: None,
                    error: Some("provider-unsupported".into()),
                    warning: None,
                },
                "{action} must fail honestly",
            );
        }
    }

    /// `edit` echoes the number the caller pointed at and hands the port the
    /// body verbatim — no re-reading, no rewriting.
    #[test]
    fn edit_echoes_the_number_and_passes_the_body_through() {
        let fake = FakePub::green("github");
        let edited = edit_report(&fake, 42, "line one\nline two");
        assert_eq!((edited.ok, edited.number, edited.url), (true, Some(42), None));
        assert_eq!(fake.seen.borrow().as_slice(), ["edit 42 body=line one\nline two"]);
    }

    /// The title comes out of the body document itself: the first heading
    /// sheds its markers, a plain first line serves as-is, and a body with no
    /// usable line falls back to the head branch so the create never starves.
    #[test]
    fn fill_reads_title_and_body_from_the_commit_range() {
        // A real repo: two commits on a branch off the base.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .expect("spawn git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "dev", "."]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["commit", "-q", "--allow-empty", "-m", "seed"]);
        git(&["checkout", "-q", "-b", "fix/x"]);
        git(&["commit", "-q", "--allow-empty", "-m", "first change"]);
        git(&["commit", "-q", "--allow-empty", "-m", "the newest subject"]);

        let (title, body) = fill_from_commits(root, "dev", "fix/x").expect("range readable");
        assert_eq!(title, "the newest subject", "title = newest commit subject");
        assert!(body.contains("- first change") && body.contains("- the newest subject"));

        // An empty range refuses with the range named, never an empty PR.
        let err = fill_from_commits(root, "fix/x", "fix/x").expect_err("nothing to fill");
        assert!(err.contains("nothing-to-fill"), "{err}");
    }

    /// O título e o corpo saem do arquivo de eventos da spec, e a recusa do
    /// montador chega inteira a quem chamou: o motivo estável e a frase.
    ///
    /// Uma spec sem arquivo de eventos não abre pull request nenhum — a porta
    /// não tem de onde tirar o texto e diz isso, em vez de inventar um corpo.
    #[test]
    fn a_mensagem_sai_do_arquivo_de_eventos_da_spec() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").expect("cfg");

        let sem_spec = message_of(root, "naoexiste").expect_err("nada a ler");
        assert!(sem_spec.contains("spec-events-unreadable"), "{sem_spec}");

        let path = mustard_core::io::spec_events::spec_file(root, "trava").expect("caminho");
        std::fs::create_dir_all(path.parent().expect("pasta")).expect("pasta da spec");
        let draft = |value: serde_json::Value| {
            value.as_object().cloned().expect("um objeto")
        };
        let said = mustard_core::io::spec_events::write(
            &path,
            "message",
            draft(serde_json::json!({"text": "preciso barrar o comando"})),
            &[],
        )
        .expect("mensagem");
        mustard_core::io::spec_events::write(
            &path,
            "context",
            draft(serde_json::json!({
                "text": "Barrar comando que apaga trabalho. E mais.",
                "origin": said.id,
            })),
            &[],
        )
        .expect("contexto");
        mustard_core::io::spec_events::write(
            &path,
            "pr_summary",
            draft(serde_json::json!({"text": "O portão lê o estado."})),
            &[],
        )
        .expect("resumo");

        let (title, body) = message_of(root, "trava").expect("a spec tem objetivo");
        assert_eq!(title, "Barrar comando que apaga trabalho.");
        assert!(body.contains("O portão lê o estado."), "{body}");
    }
}
