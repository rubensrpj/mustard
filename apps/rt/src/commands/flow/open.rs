//! `mustard-rt run open --kind <tipo> --name <nome> --base <base>` — abre uma
//! spec: a branch `<tipo>/<nome>`, o `spec.ndjson` da spec `<nome>` e o
//! nascimento dela em levantamento, com a branch e a base.
//!
//! O nome é usado como o usuário escreveu. Só o que o git recusa é ajustado
//! (acento, espaço, barra, pontos nas pontas, um `.lock` no fim), sem juntar
//! hífens nem mudar maiúsculas, e o ajuste volta para o usuário confirmar
//! antes de qualquer coisa ser criada. A branch e a spec saem do mesmo nome.
//!
//! O que falta é perguntado de volta, com `ok: true` e o passo em `step`:
//! o tipo (`choose_kind`, com as sugestões), o nome (`choose_name`), o nome
//! ajustado (`confirm_name`) e a base (`choose_base`, com as candidatas).
//! As candidatas são as bases que o `git.flow` declara, na ordem do fluxo;
//! sem `git.flow`, as branches do repositório, com o aviso de que nenhuma
//! fica protegida. Nada é criado antes de os três serem conhecidos.
//!
//! Depois vêm as conferências, todas antes de mexer no git: a base existe no
//! repositório (quando o git não responde, nenhuma existe), nenhuma branch e
//! nenhuma pasta de spec têm o nome, o checkout não carrega trabalho de
//! outra spec, pela mesma pergunta do corte da branch, e está num commit para
//! onde voltar se a spec não puder ser gravada. A branch nasce no
//! checkout de `--root`, que num worktree é o próprio worktree; a spec mora
//! no checkout principal. O mapa do projeto é atualizado só no que mudou, e a
//! falha dele só avisa.
//!
//! Com `--pending P-12`, a spec vem de uma pendência da lista: ela precisa
//! existir e estar aberta, conferido antes de qualquer coisa ser criada, e
//! depois do nascimento ganha a nota "virou a spec `<nome>`", que o merge da
//! spec lê para fechá-la. A nota que não se grava vira aviso, e a spec fica.
//!
//! A resposta termina na pergunta do objetivo, que o assistente faz ao
//! usuário. Chamar o `open` de novo na branch que ele criou devolve o mesmo
//! relatório, com `already_open`, e não grava nada além da nota da pendência
//! que ainda faltar.
//!
//! Recusa sai com exit 1 e `ok: false`, com a razão curta em `reason` e a
//! mensagem no idioma do projeto em `hint`.

use std::path::{Path, PathBuf};

use mustard_core::domain::config::GitConfig;
use mustard_core::domain::scan::ScanReport;
use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::{birth_event, SpecState, State};
use mustard_core::domain::text::fold_accents;
use mustard_core::platform::git;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::{ClaudePaths, ProjectConfig, Scan};
use serde_json::{json, Value};

use crate::commands::event::census_settlement::{settle, CensusSettlement, CheckoutPosition};
use crate::commands::event::pending::{mark_became, pending_id, pending_is_open};
use crate::commands::event::work_branch::{
    checkout_work_branch, local_branch_exists, name_dirty_paths, remote_branch_exists, BusyCheckout,
    CheckoutWork, RefusalCause,
};
use crate::commands::scan::default_model_path;
use crate::commands::spec_events::{self, write::record_open};
use crate::shared::spec_state::DiskSpecState;
use crate::shared::work_kind::WorkKind;

/// Options for `mustard-rt run open`.
pub struct OpenOpts {
    /// Qualquer pasta do checkout em que a branch nasce.
    pub root: PathBuf,
    /// O tipo da branch, como `feature` ou `fix`.
    pub kind: Option<String>,
    /// O nome da spec, como o usuário escreveu.
    pub name: Option<String>,
    /// A branch de que a spec sai.
    pub base: Option<String>,
    /// A pendência aberta de onde a spec vem, como `P-12`.
    pub pending: Option<String>,
}

/// As recusas do `open`: as dele e as da gravação da spec.
enum OpenRefusal {
    KindInvalid { kind: String },
    NameEmpty { asked: String },
    BaseNotFound { base: String, candidates: Vec<String> },
    BranchTaken { branch: String },
    SpecTaken { spec: String },
    Busy(BusyCheckout),
    GitFailed { branch: String, detail: String },
    UnbornBranch { base: String },
    PendingUnknown { pending: String },
    PendingClosed { pending: String },
    Spec(Refusal),
}

impl OpenRefusal {
    /// A razão curta, estável, para quem lê a saída por máquina.
    fn reason(&self) -> &'static str {
        match self {
            Self::KindInvalid { .. } => "kind-invalid",
            Self::NameEmpty { .. } => "name-empty",
            Self::BaseNotFound { .. } => "base-not-found",
            Self::BranchTaken { .. } => "branch-taken",
            Self::SpecTaken { .. } => "spec-taken",
            Self::Busy(busy) if matches!(busy.cause, RefusalCause::BaseStale { .. }) => "base-stale",
            Self::Busy(_) => "tree-holds-work",
            Self::GitFailed { .. } => "git-failed",
            Self::UnbornBranch { .. } => "unborn-branch",
            Self::PendingUnknown { .. } => "pending-unknown",
            Self::PendingClosed { .. } => "pending-closed",
            Self::Spec(refusal) => refusal.reason(),
        }
    }

    /// A mensagem exata, no idioma pedido. O checkout ocupado diz os caminhos
    /// quando eles foram medidos; senão, a frase do corte da branch.
    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, &str)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::KindInvalid { kind } => fill("open.kind_invalid", &[("{kind}", kind)]),
            Self::NameEmpty { asked } => fill("open.name_empty", &[("{asked}", asked)]),
            Self::BaseNotFound { base, candidates } => {
                let listed = if candidates.is_empty() { "-".to_string() } else { candidates.join(", ") };
                fill("open.base_not_found", &[("{base}", base), ("{candidates}", &listed)])
            }
            Self::BranchTaken { branch } => fill("open.branch_taken", &[("{branch}", branch)]),
            Self::SpecTaken { spec } => fill("open.spec_taken", &[("{spec}", spec)]),
            Self::Busy(busy) => match named_paths(busy) {
                Some(paths) => fill("open.tree_busy", &[("{paths}", &paths)]),
                None => busy.reason(lang),
            },
            Self::GitFailed { branch, detail } => fill("open.git_failed", &[("{branch}", branch), ("{detail}", detail)]),
            Self::UnbornBranch { base } => fill("open.unborn_branch", &[("{base}", base)]),
            Self::PendingUnknown { pending } => fill("open.pending_unknown", &[("{pending}", pending)]),
            Self::PendingClosed { pending } => fill("open.pending_closed", &[("{pending}", pending)]),
            Self::Spec(refusal) => refusal.message(lang),
        }
    }

    /// A recusa como o comando imprime; a base que não existe leva as
    /// candidatas.
    fn report(&self, lang: Locale) -> Value {
        let mut report = json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) });
        if let Self::BaseNotFound { candidates, .. } = self {
            report["candidates"] = json!(candidates);
        }
        report
    }
}

/// Os caminhos medidos no checkout ocupado, já com o corte dos que passam do
/// limite; `None` quando nada foi medido ou quando a base é que ficou para
/// trás.
fn named_paths(busy: &BusyCheckout) -> Option<String> {
    let paths = match (&busy.cause, &busy.work) {
        (RefusalCause::BaseStale { .. }, _) => return None,
        (RefusalCause::BaseBlockedByWork { paths, .. }, _) => paths,
        (_, CheckoutWork::Holds { theirs, .. }) => theirs,
        (_, CheckoutWork::CensusOnly(dirty)) => dirty,
        (_, CheckoutWork::ProvenClean | CheckoutWork::Unproven) => return None,
    };
    if paths.is_empty() {
        return None;
    }
    let (shown, more) = name_dirty_paths(paths);
    Some(format!("{shown}{more}"))
}

/// O nome que o git aceita como parte de uma branch e que serve de pasta de
/// spec, a partir do que o usuário escreveu, e o que mudou.
///
/// Tira o acento, troca espaço e todo caractere fora de letras, números, `.`,
/// `_` e `-` por `-` (a barra também, porque a pasta da spec não a aceita),
/// troca `..` por `-`, tira `-` e `.` do começo, `.` e um `.lock` do fim. Não
/// junta hífens nem muda maiúsculas. O resultado é ponto fixo: ajustar de novo
/// não muda nada. Vazio quando não sobra letra nem número.
pub(crate) fn adjust_name(raw: &str) -> (String, Vec<&'static str>) {
    let asked = raw.trim();
    let mut changes = Vec::new();
    let folded = fold_accents(asked);
    if folded != asked {
        changes.push("accents");
    }
    let (mut spaces, mut others) = (false, false);
    let mut name: String = folded
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' => c,
            c if c.is_whitespace() => {
                spaces = true;
                '-'
            }
            _ => {
                others = true;
                '-'
            }
        })
        .collect();
    if spaces {
        changes.push("spaces");
    }
    if others {
        changes.push("characters");
    }
    let shaped = name.clone();
    loop {
        let before = name.clone();
        while name.contains("..") {
            name = name.replace("..", "-");
        }
        name = name.trim_start_matches(['-', '.']).trim_end_matches('.').to_string();
        if let Some(stem) = name.strip_suffix(".lock") {
            name = stem.to_string();
        }
        if name == before {
            break;
        }
    }
    if name != shaped {
        changes.push("edges");
    }
    (name, changes)
}

/// O tipo que o usuário escreveu, quando ele serve de começo de branch como
/// está: minúsculo, com letras, números, `-` ou `_`, começando por letra ou
/// número. Nada é trocado sem avisar, nem a maiúscula.
fn kind_of(raw: &str) -> Option<WorkKind> {
    let raw = raw.trim();
    WorkKind::parse(raw).filter(|kind| kind.token() == raw && raw.starts_with(|c: char| c.is_ascii_alphanumeric()))
}

/// O nome completo de uma branch, `<tipo>/<nome>`, dividido nos dois quando o
/// começo é um tipo.
fn split_full_name(raw: &str) -> Option<(WorkKind, String)> {
    let (head, tail) = raw.split_once('/')?;
    let kind = kind_of(head)?;
    let tail = tail.trim();
    (!tail.is_empty()).then(|| (kind, tail.to_string()))
}

/// `false` quando o programa de versões não aceita `branch` como nome de
/// branch — e também quando ele não responde, porque abrir uma unidade corta
/// uma branch e não tem como seguir sem quem a corte. Antes essas duas
/// respostas eram separadas e a segunda deixava passar, só para o corte falhar
/// mais adiante, depois de a pasta da spec já ter sido decidida.
fn git_accepts(root: &Path, branch: &str) -> bool {
    git::run(root, &["check-ref-format", "--branch", branch]).ok
}

/// A branch existe no repositório, local ou no `origin`. Sem resposta do git,
/// não existe.
fn branch_exists(root: &Path, branch: &str) -> bool {
    local_branch_exists(root, branch) || remote_branch_exists(root, branch)
}

/// As bases que o `git.flow` declara, na ordem do fluxo: a do trabalho comum
/// (`*`) e, a partir dela, cada base para onde ela sobe; as que sobrarem, em
/// ordem alfabética.
fn flow_order(git: &GitConfig) -> Vec<String> {
    let step = |from: &str| git.flow.get(from).map(|to| to.trim().to_string()).filter(|to| !to.is_empty());
    let mut order: Vec<String> = Vec::new();
    let mut next = step("*");
    while let Some(base) = next.take() {
        if order.contains(&base) {
            break;
        }
        next = step(&base);
        order.push(base);
    }
    for base in git.declared_bases() {
        if !order.contains(&base) {
            order.push(base);
        }
    }
    order
}

/// Todas as branches do repositório, locais e do `origin`, cada uma uma vez,
/// a do commit mais novo primeiro. Vazia quando o git não responde.
fn repository_branches(root: &Path) -> Vec<String> {
    let listing = git::run(
        root,
        &["for-each-ref", "--sort=-committerdate", "--format=%(refname)", "refs/heads", "refs/remotes/origin"],
    );
    if !listing.ok {
        return Vec::new();
    }
    let mut names: Vec<String> = Vec::new();
    for line in listing.stdout.lines().map(str::trim) {
        let name = line.strip_prefix("refs/heads/").or_else(|| line.strip_prefix("refs/remotes/origin/"));
        if let Some(name) = name.filter(|name| !name.is_empty() && *name != "HEAD")
            && !names.iter().any(|known| known == name)
        {
            names.push(name.to_string());
        }
    }
    names
}

/// A pasta da spec `name` já existe com algum arquivo dentro.
fn spec_folder_taken(project: &Path, name: &str) -> bool {
    ClaudePaths::for_project(project)
        .and_then(|paths| paths.for_spec(name))
        .ok()
        .and_then(|spec| std::fs::read_dir(spec.dir()).ok())
        .is_some_and(|mut entries| entries.next().is_some())
}

/// O commit de que a branch nova sai, pela mesma ordem do corte
/// ([`checkout_work_branch`]): a base local, a do `origin` e, sem nenhuma
/// das duas, o commit atual.
fn cut_start(root: &Path, base: &str) -> Option<String> {
    let from = if local_branch_exists(root, base) {
        base.to_string()
    } else if remote_branch_exists(root, base) {
        format!("origin/{base}")
    } else {
        "HEAD".to_string()
    };
    git::run(root, &["rev-parse", "--verify", "--quiet", &format!("{from}^{{commit}}")]).out()
}

/// Desfaz a branch `target` que o `open` acabou de criar, quando a spec não
/// pôde ser gravada: o checkout volta para `back_to`, onde estava, e a branch
/// nova é apagada. Sem isso, o checkout ficaria numa branch de trabalho sem
/// spec, onde o portão de escrita deixa editar, e um novo `open` só diria que
/// a branch já existe.
///
/// Só apaga a branch que não tem commit além de `start`, o commit de que ela
/// saiu; com commit próprio, ou sem resposta do git, nada é desfeito.
/// `true` quando desfez.
fn undo_branch(root: &Path, back_to: &str, start: Option<&str>, target: &str) -> bool {
    let Some(start) = start else {
        return false;
    };
    if git::run(root, &["rev-list", "--count", &format!("{start}..{target}")]).out().as_deref() != Some("0") {
        return false;
    }
    git::run(root, &["checkout", "-q", back_to]).ok && git::run(root, &["branch", "-D", target]).ok
}

/// Por que a pendência dita não serve: a lista não a tem, ou ela já fechou.
enum PendingMiss {
    Unknown(String),
    Closed(String),
}

impl From<PendingMiss> for OpenRefusal {
    fn from(miss: PendingMiss) -> Self {
        match miss {
            PendingMiss::Unknown(pending) => Self::PendingUnknown { pending },
            PendingMiss::Closed(pending) => Self::PendingClosed { pending },
        }
    }
}

/// A pendência `raw` de onde a spec vem, na grafia da lista, quando ela
/// existe e está aberta; senão, por que não serve. `root` é a raiz do projeto
/// em que a spec mora, o checkout principal.
fn pending_to_note(root: &Path, raw: &str) -> Result<String, PendingMiss> {
    let Some(id) = pending_id(raw) else {
        return Err(PendingMiss::Unknown(raw.to_string()));
    };
    match pending_is_open(root, &id) {
        Some(true) => Ok(id),
        Some(false) => Err(PendingMiss::Closed(id)),
        None => Err(PendingMiss::Unknown(id)),
    }
}

/// Grava na pendência `pending` a nota "virou a spec `spec`", pela mesma
/// gravação que o merge lê. Devolve o aviso quando a nota não se grava.
fn note_pending(root: &Path, pending: Option<&str>, spec: &str, lang: Locale) -> Option<String> {
    let id = pending?;
    (!mark_became(root, id, spec))
        .then(|| translate("open.pending_note_failed", lang).replace("{spec}", spec).replace("{pending}", id))
}

/// O relatório da spec aberta, que termina na pergunta do objetivo.
fn opened(spec: &str, branch: &str, base: &str, kind: &WorkKind, map: Option<Value>, warnings: &[String], lang: Locale) -> Value {
    let mut report = json!({
        "ok": true,
        "step": "ask_goal",
        "spec": spec,
        "branch": branch,
        "base": base,
        "kind": kind.token(),
        "name": spec,
        "question": translate("open.ask_goal", lang),
        "hint": translate("open.next_goal", lang).replace("{spec}", spec).replace("{branch}", branch),
    });
    if let Some(map) = map {
        report["map"] = map;
    }
    if !warnings.is_empty() {
        report["warnings"] = json!(warnings);
    }
    report
}

/// O núcleo testável de [`run`], com o mapa do projeto atualizado de verdade.
/// Nunca entra em pânico.
pub(crate) fn open_at(opts: &OpenOpts) -> Value {
    open_with(opts, |root| Scan::locate().scan(root, &default_model_path(root)).map_err(|e| e.to_string()))
}

/// O `open`, com quem atualiza o mapa do projeto dado por quem chama.
fn open_with(opts: &OpenOpts, refresh: impl FnOnce(&Path) -> Result<ScanReport, String>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    let refuse = |refusal: OpenRefusal| refusal.report(lang);
    let root = std::path::absolute(&opts.root).unwrap_or_else(|_| opts.root.clone());
    let config = ProjectConfig::load(&project.root);
    let given = |value: &Option<String>| value.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    // A pendência de onde a spec vem: só uma aberta vira spec. A lista é a do
    // projeto em que a spec mora, também vista de um worktree.
    let pending = || given(&opts.pending).map(|raw| pending_to_note(&project.root, &raw)).transpose();

    // O tipo, dito ou no começo do nome completo da branch.
    let (kind, asked) = match given(&opts.kind) {
        Some(raw) => match kind_of(&raw) {
            Some(kind) => (kind, given(&opts.name)),
            None => return refuse(OpenRefusal::KindInvalid { kind: raw }),
        },
        None => match given(&opts.name).as_deref().and_then(split_full_name) {
            Some((kind, name)) => (kind, Some(name)),
            None => {
                let mut report = json!({
                    "ok": true,
                    "step": "choose_kind",
                    "suggested": WorkKind::SUGGESTED,
                    "hint": translate("open.choose_kind", lang),
                });
                if let Some(name) = given(&opts.name) {
                    report["name"] = json!(name);
                }
                return report;
            }
        },
    };
    let Some(asked) = asked else {
        return json!({
            "ok": true,
            "step": "choose_name",
            "kind": kind.token(),
            "hint": translate("open.choose_name", lang).replace("{kind}", kind.token()),
        });
    };

    // O nome: o que o git recusa volta ajustado, para o sim do usuário.
    let (name, changes) = adjust_name(&asked);
    if name.is_empty() {
        return refuse(OpenRefusal::NameEmpty { asked });
    }
    if name != asked {
        return json!({
            "ok": true,
            "step": "confirm_name",
            "kind": kind.token(),
            "asked": asked,
            "adjusted": name,
            "changes": changes,
            "hint": translate("open.confirm_name", lang).replace("{asked}", &asked).replace("{adjusted}", &name),
        });
    }
    let target = kind.branch_name(&name);
    if config.vcs().is_none() {
        return refuse(OpenRefusal::GitFailed { branch: target, detail: "mustard.json: vcs \"\"".to_string() });
    }
    if !git_accepts(&root, &target) {
        return refuse(OpenRefusal::GitFailed { branch: target, detail: "git check-ref-format --branch".to_string() });
    }

    // A mesma chamada de novo, na branch que ela criou: o mesmo relatório.
    let current = mustard_core::current_branch(&root);
    if current.as_deref() == Some(target.as_str())
        && let Some(state) = DiskSpecState::new(&root)
            .log(&name)
            .filter(|log| birth_event(log).is_some())
            .map(|log| State::from_log(&log))
            .filter(|state| state.branch.as_deref() == Some(target.as_str()))
    {
        // Com a pendência, a mesma conferência, e a nota que ainda faltar.
        let pending = match pending() {
            Ok(pending) => pending,
            Err(miss) => return refuse(miss.into()),
        };
        let warnings: Vec<String> =
            note_pending(&project.root, pending.as_deref(), &name, lang).into_iter().collect();
        let base = state.base.unwrap_or_default();
        let mut report = opened(&name, &target, &base, &kind, None, &warnings, lang);
        report["already_open"] = json!(true);
        if let Some(id) = pending {
            report["pending"] = json!(id);
        }
        return report;
    }

    // A base: as candidatas quando falta; a que não existe é recusada.
    let declared = !config.git.declared_bases().is_empty();
    let candidates = || -> Vec<String> {
        if declared {
            flow_order(&config.git).into_iter().filter(|base| branch_exists(&root, base)).collect()
        } else {
            repository_branches(&root)
        }
    };
    let Some(base) = given(&opts.base) else {
        let mut report = json!({
            "ok": true,
            "step": "choose_base",
            "kind": kind.token(),
            "name": name,
            "candidates": candidates(),
            "hint": translate("open.choose_base", lang),
        });
        if !declared {
            report["warnings"] = json!([translate("open.no_flow", lang)]);
        }
        return report;
    };
    if !branch_exists(&root, &base) {
        return refuse(OpenRefusal::BaseNotFound { base, candidates: candidates() });
    }
    let pending = match pending() {
        Ok(pending) => pending,
        Err(miss) => return refuse(miss.into()),
    };

    // O nome livre, na branch e na pasta da spec.
    if branch_exists(&root, &target) {
        return refuse(OpenRefusal::BranchTaken { branch: target });
    }
    if spec_folder_taken(&project.root, &name) {
        return refuse(OpenRefusal::SpecTaken { spec: name });
    }

    // O checkout: a mesma pergunta do corte da branch, que também atualiza a
    // base pelo `origin`.
    let position = CheckoutPosition::at(current.as_deref(), Some(target.as_str()), Some(base.as_str()));
    if let CensusSettlement::Refuse(busy) = settle(&root, position, &config) {
        return refuse(OpenRefusal::Busy(busy));
    }
    // Onde o checkout estava e de que commit a branch sai: se a spec não
    // puder ser gravada, a branch nova é desfeita a partir daqui. Com o
    // checkout solto, a volta é para o commit em que ele estava.
    let back_to = current.clone().or_else(|| git::run(&root, &["rev-parse", "HEAD"]).out());
    // Numa branch sem nenhum commit, o git não dá nem nome nem commit para
    // voltar, e o desfazer não teria o que fazer: a recusa vem antes de a
    // branch nova nascer.
    let Some(back_to) = back_to else {
        return refuse(OpenRefusal::UnbornBranch { base });
    };
    let start = cut_start(&root, &base);
    if let Err(detail) = checkout_work_branch(&root, &target, &base) {
        return refuse(OpenRefusal::GitFailed { branch: target, detail });
    }
    if let Err(refusal) = record_open(&root, &name, &target, &base) {
        undo_branch(&root, &back_to, start.as_deref(), &target);
        return refuse(OpenRefusal::Spec(refusal));
    }

    // A nota da pendência, e o mapa, só no que mudou; a falha de cada um só
    // avisa.
    let mut warnings: Vec<String> = note_pending(&project.root, pending.as_deref(), &name, lang).into_iter().collect();
    let map = match refresh(&project.root) {
        Ok(report) => Some(json!({ "full": report.full, "read": report.read.len() })),
        Err(detail) => {
            warnings.push(translate("open.map_warning", lang).replace("{detail}", &detail));
            None
        }
    };
    let mut report = opened(&name, &target, &base, &kind, map, &warnings, lang);
    if let Some(id) = pending {
        report["pending"] = json!(id);
    }
    report
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::event::work_branch::{
        cut_pending_work_branch, slug_of_work_branch, CutOutcome,
    };
    use crate::commands::spec_events::write::{seed_at, WriteOpts};
    use crate::hooks::write::write_gate::WriteGate;
    use crate::shared::context::checkout::spec_of_checkout_branch;
use crate::shared::context::pending_branch::set_pending_branch;
    use crate::shared::spec_state::active_spec;
    use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
    use tempfile::tempdir;

    /// O fluxo deste repositório: `dev` e `main` são bases.
    const DEV_MAIN: &str = r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#;
    const EN: &str = r#"{"language":{"text":"en-US"},"git":{"flow":{"*":"dev","dev":"main"}}}"#;

    fn git(root: &Path, args: &[&str]) -> String {
        let out = git::run(root, args);
        assert!(out.ok, "git {args:?}: {}", out.stderr);
        out.stdout.trim().to_string()
    }

    /// Um repositório com `main` e `dev`, o `mustard.json` dado e um arquivo
    /// de código, parado em `dev`. O Mustard fica fora do git, como num
    /// projeto de verdade.
    fn repo(config: &str) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-q", "-b", "main"]);
        std::fs::write(root.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").unwrap();
        std::fs::write(root.join("mustard.json"), config).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("main.rs"), "fn main() {}\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "init"]);
        git(root, &["checkout", "-q", "-b", "dev"]);
        dir
    }

    fn mapped(_: &Path) -> Result<ScanReport, String> {
        Ok(ScanReport { full: false, read: vec!["src/main.rs".into()], files: 1, head: String::new(), dictionary: false })
    }

    fn opts(root: &Path, kind: Option<&str>, name: Option<&str>, base: Option<&str>) -> OpenOpts {
        OpenOpts {
            root: root.to_path_buf(),
            kind: kind.map(str::to_string),
            name: name.map(str::to_string),
            base: base.map(str::to_string),
            pending: None,
        }
    }

    fn open(root: &Path, kind: Option<&str>, name: Option<&str>, base: Option<&str>) -> Value {
        open_with(&opts(root, kind, name, base), mapped)
    }

    /// A branch em que o checkout está, pela leitura da biblioteca — a mesma
    /// que o código de produção usa, em vez de uma segunda escrita da pergunta.
    fn head(root: &Path) -> String {
        mustard_core::current_branch(root).unwrap_or_default()
    }

    fn branches(root: &Path) -> Vec<String> {
        git(root, &["for-each-ref", "--format=%(refname:short)", "refs/heads"]).lines().map(str::to_string).collect()
    }

    fn spec_dir(root: &Path, name: &str) -> PathBuf {
        root.join(".claude").join("spec").join(name)
    }

    fn state(root: &Path, name: &str) -> State {
        DiskSpecState::new(root).state(name).expect("the spec has an event file")
    }

    /// A abertura recebe "feature/trava-de-pendencias", dita como tipo e
    /// nome: a branch e a spec nascem com esse nome exato, sem cortar o "de".
    #[test]
    fn open_names_the_branch_and_the_spec_exactly_as_written() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let report = open(root, Some("feature"), Some("trava-de-pendencias"), Some("dev"));
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["branch"], json!("feature/trava-de-pendencias"));
        assert_eq!(report["spec"], json!("trava-de-pendencias"));
        assert_eq!(head(root), "feature/trava-de-pendencias");
        assert!(spec_dir(root, "trava-de-pendencias").join("spec.ndjson").is_file());
    }

    /// O `open` não escreve página: a spec nasce só com o arquivo de
    /// eventos, e a mesma chamada de novo também não escreve.
    #[test]
    fn the_open_writes_no_page() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        assert_eq!(open(root, Some("feature"), Some("trava"), Some("dev"))["ok"], json!(true));
        let spec = spec_dir(root, "trava");
        let again = open(root, Some("feature"), Some("trava"), Some("dev"));
        assert_eq!(again["already_open"], json!(true), "{again}");
        for page in ["spec.html", "spec.md"] {
            assert!(!spec.join(page).exists(), "{page}");
        }
        assert!(!root.join(".claude/spec/project.html").exists(), "nem a página do projeto");
    }

    /// O nome completo da branch, sem o tipo à parte, é dividido em tipo e
    /// nome, e dá a mesma branch e a mesma spec.
    #[test]
    fn a_full_branch_name_is_split_into_kind_and_name() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let report = open(root, None, Some("feature/trava-de-pendencias"), Some("dev"));
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!((report["kind"].clone(), report["name"].clone()), (json!("feature"), json!("trava-de-pendencias")));
        assert_eq!(head(root), "feature/trava-de-pendencias");
        assert!(spec_dir(root, "trava-de-pendencias").join("spec.ndjson").is_file());
    }

    /// O nome com espaço e acento volta ajustado, para o usuário ver antes, e
    /// nada é criado: nem a branch, nem a pasta da spec.
    #[test]
    fn a_name_git_refuses_is_shown_adjusted_and_nothing_is_created() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let report = open(root, Some("feature"), Some("trava de pendências"), Some("dev"));
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["step"], json!("confirm_name"));
        assert_eq!(report["asked"], json!("trava de pendências"));
        assert_eq!(report["adjusted"], json!("trava-de-pendencias"));
        let hint = report["hint"].as_str().unwrap();
        assert!(hint.contains("trava de pendências") && hint.contains("trava-de-pendencias"), "{hint}");
        assert_eq!(branches(root), vec!["dev".to_string(), "main".to_string()]);
        assert!(!root.join(".claude").join("spec").exists());
    }

    /// Com o sim do usuário, o nome ajustado passa sem mudança na segunda
    /// chamada e abre a spec.
    #[test]
    fn the_adjusted_name_opens_on_the_second_call() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let first = open(root, Some("feature"), Some("trava de pendências"), Some("dev"));
        let adjusted = first["adjusted"].as_str().unwrap().to_string();
        let second = open(root, Some("feature"), Some(&adjusted), Some("dev"));
        assert_eq!(second["step"], json!("ask_goal"), "{second}");
        assert_eq!(second["branch"], json!("feature/trava-de-pendencias"));
        assert_eq!(head(root), "feature/trava-de-pendencias");
    }

    /// Barra, pontos nas pontas, `..`, um `.lock` no fim e letra fora do
    /// alfabeto viram o que a pasta da spec e o git aceitam; maiúscula, ponto
    /// no meio e `_` ficam.
    #[test]
    fn a_name_with_a_slash_or_dots_is_adjusted_for_the_spec_folder() {
        for (asked, adjusted) in [
            ("fase/1", "fase-1"),
            ("..x..y.", "x-y"),
            ("nome.lock", "nome"),
            ("Ação_Rápida.v2", "Acao_Rapida.v2"),
            ("straße", "stra-e"),
            ("日本-x", "x"),
            ("-a--b-", "a--b-"),
        ] {
            assert_eq!(adjust_name(asked).0, adjusted, "{asked}");
            let tmp = tempdir().unwrap();
            let paths = ClaudePaths::for_project(tmp.path()).unwrap();
            assert!(paths.for_spec(adjusted).is_ok(), "{adjusted} is a spec folder");
        }
        assert_eq!(adjust_name("trava-de-pendencias"), ("trava-de-pendencias".to_string(), Vec::new()));
    }

    /// O nome que o ajuste esvazia é recusado, nos dois idiomas, e nada é
    /// criado.
    #[test]
    fn a_name_left_empty_by_the_adjustment_is_refused_in_both_languages() {
        for (config, words) in [(DEV_MAIN, "fica vazio"), (EN, "is empty")] {
            let dir = repo(config);
            let root = dir.path();
            let report = open(root, Some("feature"), Some("..."), Some("dev"));
            assert_eq!(report["ok"], json!(false));
            assert_eq!(report["reason"], json!("name-empty"));
            let hint = report["hint"].as_str().unwrap();
            assert!(hint.contains("\"...\"") && hint.contains(words), "{hint}");
            assert_eq!(branches(root), vec!["dev".to_string(), "main".to_string()]);
        }
    }

    /// O nome ajustado não muda quando passa de novo pelo ajuste.
    #[test]
    fn an_adjusted_name_is_a_fixed_point_of_the_adjustment() {
        for asked in ["trava de pendências", "..x..y.", "nome.lock", "Ação_Rápida.v2", "a/b/c", "x.lock.lock", "@{u}"] {
            let (name, _) = adjust_name(asked);
            assert_eq!(adjust_name(&name), (name.clone(), Vec::new()), "{asked}");
        }
    }

    /// A branch que o `open` cria leva de volta à spec, com maiúscula, ponto
    /// e `_` no nome: pelo leitor do nome da branch e pela escada da spec
    /// atual.
    #[test]
    fn the_branch_open_creates_leads_back_to_its_spec() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let config = ProjectConfig::load(root);
        for name in ["Trava.De_Pendencias", "UPPER", "a_b.c"] {
            let report = open(root, Some("fix"), Some(name), Some("dev"));
            assert_eq!(report["ok"], json!(true), "{report}");
            let branch = head(root);
            assert_eq!(branch, format!("fix/{name}"));
            assert_eq!(slug_of_work_branch(&branch, &config).as_deref(), Some(name));
            assert_eq!(spec_of_checkout_branch(&root.to_string_lossy()).as_deref(), Some(name));
            assert_eq!(active_spec(&root.to_string_lossy(), None).as_deref(), Some(name));
        }
    }

    /// Sem a base, o `open` devolve as bases que o `git.flow` declara, na
    /// ordem do fluxo, e não cria nada.
    #[test]
    fn without_a_base_open_lists_the_declared_bases_and_creates_nothing() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let report = open(root, Some("feature"), Some("x"), None);
        assert_eq!(report["step"], json!("choose_base"), "{report}");
        assert_eq!(report["candidates"], json!(["dev", "main"]));
        assert!(report.get("warnings").is_none());
        assert_eq!(branches(root), vec!["dev".to_string(), "main".to_string()]);
        assert!(!root.join(".claude").join("spec").exists());
    }

    /// Sem `git.flow`, as candidatas são as branches do repositório, com o
    /// aviso de que nenhuma fica protegida; a base escolhida entre elas abre a
    /// spec.
    #[test]
    fn without_git_flow_open_lists_the_repository_branches_and_warns() {
        let dir = repo("{}");
        let root = dir.path();
        git(root, &["branch", "release/2026-Q3"]);
        let report = open(root, Some("feature"), Some("x"), None);
        assert_eq!(report["step"], json!("choose_base"), "{report}");
        let mut candidates: Vec<String> =
            serde_json::from_value(report["candidates"].clone()).unwrap();
        candidates.sort();
        assert_eq!(candidates, vec!["dev", "main", "release/2026-Q3"]);
        assert_eq!(report["warnings"], json!([translate("open.no_flow", Locale::PtBr)]));
        assert!(!root.join(".claude").join("spec").exists());
        let opened = open(root, Some("feature"), Some("x"), Some("release/2026-Q3"));
        assert_eq!(opened["base"], json!("release/2026-Q3"), "{opened}");
    }

    /// A base que não existe é recusada, com as candidatas; fora de um
    /// repositório, onde o git não responde, nenhuma base existe.
    #[test]
    fn a_base_that_does_not_exist_is_refused_with_the_candidates() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let report = open(root, Some("feature"), Some("x"), Some("nao-existe"));
        assert_eq!(report["reason"], json!("base-not-found"), "{report}");
        assert_eq!(report["candidates"], json!(["dev", "main"]));
        let hint = report["hint"].as_str().unwrap();
        assert!(hint.contains("nao-existe") && hint.contains("dev, main"), "{hint}");
        assert_eq!(branches(root), vec!["dev".to_string(), "main".to_string()]);

        let bare = tempdir().unwrap();
        std::fs::write(bare.path().join("mustard.json"), "{}").unwrap();
        let unmeasured = open(bare.path(), Some("feature"), Some("x"), Some("dev"));
        assert_eq!(unmeasured["reason"], json!("base-not-found"), "{unmeasured}");
        assert!(!bare.path().join(".claude").join("spec").exists());
    }

    /// Uma branch com o mesmo nome recusa a abertura, e nada é gravado.
    #[test]
    fn a_name_taken_by_a_branch_is_refused() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        git(root, &["branch", "feature/x"]);
        let report = open(root, Some("feature"), Some("x"), Some("dev"));
        assert_eq!(report["reason"], json!("branch-taken"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("feature/x"));
        assert_eq!(head(root), "dev");
        assert!(!spec_dir(root, "x").exists());
    }

    /// Uma pasta de spec com o nome, com qualquer arquivo, recusa a abertura,
    /// e a branch não nasce.
    #[test]
    fn a_name_taken_by_a_spec_folder_is_refused() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        std::fs::create_dir_all(spec_dir(root, "x")).unwrap();
        std::fs::write(spec_dir(root, "x").join("notas.txt"), "oi").unwrap();
        let report = open(root, Some("feature"), Some("x"), Some("dev"));
        assert_eq!(report["reason"], json!("spec-taken"), "{report}");
        assert_eq!(branches(root), vec!["dev".to_string(), "main".to_string()]);
    }

    /// A mesma chamada de novo, na branch que ela criou, devolve o mesmo
    /// relatório e não grava nada, com ou sem a base.
    #[test]
    fn running_open_again_on_its_own_branch_answers_the_same_and_writes_nothing() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let first = open(root, Some("feature"), Some("x"), Some("dev"));
        assert_eq!(first["ok"], json!(true), "{first}");
        let events = spec_dir(root, "x").join("spec.ndjson");
        let bytes = std::fs::read(&events).unwrap();
        for base in [Some("dev"), None] {
            let again = open(root, Some("feature"), Some("x"), base);
            assert_eq!(again["already_open"], json!(true), "{again}");
            for field in ["spec", "branch", "base", "step", "question", "hint"] {
                assert_eq!(again[field], first[field], "{field}");
            }
            assert_eq!(std::fs::read(&events).unwrap(), bytes);
        }
    }

    /// Um checkout parado na branch de outra spec, com mudança sem commit, é
    /// recusado com os caminhos, e nada muda.
    #[test]
    fn open_refuses_a_checkout_holding_another_specs_changes() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        git(root, &["checkout", "-q", "-b", "feature/outra"]);
        std::fs::write(root.join("src").join("main.rs"), "fn main() { todo!() }\n").unwrap();
        let report = open(root, Some("feature"), Some("x"), Some("dev"));
        assert_eq!(report["reason"], json!("tree-holds-work"), "{report}");
        assert!(report["hint"].as_str().unwrap().contains("src/main.rs"));
        assert_eq!(head(root), "feature/outra");
        assert!(!spec_dir(root, "x").exists());
        assert_eq!(branches(root), vec!["dev".to_string(), "feature/outra".to_string(), "main".to_string()]);
    }

    /// O `open` e o corte antigo da branch recusam o mesmo checkout ocupado,
    /// pelos mesmos caminhos: nenhuma proteção do corte se perde.
    #[test]
    fn open_and_the_old_cut_refuse_the_same_busy_checkout() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        git(root, &["checkout", "-q", "-b", "feature/outra"]);
        std::fs::write(root.join("src").join("main.rs"), "fn main() { todo!() }\n").unwrap();
        std::fs::write(root.join("src").join("novo.rs"), "fn novo() {}\n").unwrap();

        set_pending_branch(&root.to_string_lossy(), "sess-lado", "feature/x", Some("dev"));
        let CutOutcome::Refused(busy) = cut_pending_work_branch(root, "sess-lado") else {
            panic!("the old cut refuses the busy checkout");
        };
        let report = open(root, Some("feature"), Some("x"), Some("dev"));
        assert_eq!(report["reason"], json!("tree-holds-work"), "{report}");
        let paths = named_paths(&busy).expect("the cut measured the paths");
        assert!(report["hint"].as_str().unwrap().contains(&paths), "{report} vs {paths}");
        assert_eq!(head(root), "feature/outra");
    }

    /// Aberta de um worktree, a branch nasce no worktree, e a spec, no
    /// checkout principal, com a branch nova no `state`.
    #[test]
    fn open_from_a_linked_worktree_writes_the_spec_in_the_main_checkout() {
        let dir = tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["config", "user.email", "t@example.com"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(main.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").unwrap();
        std::fs::write(main.join("mustard.json"), DEV_MAIN).unwrap();
        std::fs::write(main.join("README.md"), "oi\n").unwrap();
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-q", "-m", "init"]);
        let worktree = dir.path().join("wt");
        git(&main, &["worktree", "add", "-q", "-b", "scratch", &worktree.to_string_lossy()]);

        let report = open(&worktree, Some("feature"), Some("x"), Some("dev"));
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(head(&worktree), "feature/x");
        assert_eq!(head(&main), "dev");
        assert!(spec_dir(&main, "x").join("spec.ndjson").is_file());
        assert!(!spec_dir(&worktree, "x").exists());
        assert_eq!(state(&main, "x").branch.as_deref(), Some("feature/x"));
    }

    /// A lista de pendências do projeto em `root`.
    fn ledger(root: &Path) -> PathBuf {
        ClaudePaths::for_project(root).unwrap().pending_ledger_path()
    }

    /// Acrescenta à lista a pendência `title`; a primeira ganha o número
    /// `P-1`.
    fn add_pending(root: &Path, title: &str) {
        let added = crate::commands::event::pending::pending_at(&crate::commands::event::pending::PendingOpts {
            root: root.to_path_buf(),
            add: true,
            title: Some(title.to_string()),
            detail: Some("nasceu na conversa".to_string()),
            ..crate::commands::event::pending::PendingOpts::default()
        });
        assert_eq!(added["ok"], json!(true), "{added}");
    }

    /// O `open` da spec `cadastro` a partir de `dev`, vindo da pendência
    /// `pending`.
    fn open_from(root: &Path, pending: &str) -> Value {
        let mut with = opts(root, Some("feature"), Some("cadastro"), Some("dev"));
        with.pending = Some(pending.to_string());
        open_with(&with, mapped)
    }

    /// A pendência que virou a spec `spec`, pela leitura do merge.
    fn became(root: &Path, spec: &str) -> Option<String> {
        crate::commands::event::pending::became_of(root, spec)
    }

    /// Nada foi criado: nem branch nova, nem pasta de spec, e a lista de
    /// pendências tem os mesmos bytes.
    fn nothing_created(root: &Path, list: Option<&Vec<u8>>) {
        assert_eq!(head(root), "dev");
        assert_eq!(branches(root), vec!["dev".to_string(), "main".to_string()]);
        assert!(!spec_dir(root, "cadastro").exists());
        assert_eq!(std::fs::read(ledger(root)).ok().as_ref(), list, "the list stays the same");
    }

    /// Aberta a partir de uma pendência aberta, a spec nasce, o relatório
    /// traz o número, e a pendência ganha a nota de que virou a spec.
    #[test]
    fn open_with_a_pending_item_records_that_it_became_the_spec() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        add_pending(root, "Cadastro de clientes");
        let report = open_from(root, "P-1");
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["pending"], json!("P-1"), "{report}");
        assert!(report.get("warnings").is_none(), "{report}");
        assert_eq!(state(root, "cadastro").phase, Some("survey"));
        assert_eq!(became(root, "cadastro").as_deref(), Some("P-1"));
    }

    /// A nota que o `open` grava é a que o merge lê: a leitura do merge acha
    /// a pendência pela spec, e o fechamento dela passa.
    #[test]
    fn a_spec_opened_from_a_pending_is_found_by_the_merge_reader() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        add_pending(root, "Cadastro de clientes");
        assert_eq!(open_from(root, "P-1")["ok"], json!(true));
        let found = became(root, "cadastro").expect("the merge reader finds the item");
        assert_eq!(found, "P-1");
        assert!(crate::commands::event::pending::close_pending(root, &found, "PR #7 mergeado"));
        assert_eq!(pending_is_open(root, "P-1"), Some(false));
    }

    /// O número da pendência vale como `P-1`, `p-1` ou `1`, com espaço nas
    /// pontas, e o relatório o traz na grafia da lista.
    #[test]
    fn a_pending_number_is_accepted_as_p_n_or_n() {
        for written in ["P-1", "p-1", "1", " P-1 "] {
            let dir = repo(DEV_MAIN);
            let root = dir.path();
            add_pending(root, "Cadastro de clientes");
            let report = open_from(root, written);
            assert_eq!(report["pending"], json!("P-1"), "{written}: {report}");
            assert_eq!(became(root, "cadastro").as_deref(), Some("P-1"), "{written}");
        }
    }

    /// Uma pendência que a lista não tem é recusada, nos dois idiomas, antes
    /// de qualquer coisa ser criada; um texto sem número também.
    #[test]
    fn open_with_an_unknown_pending_is_refused_and_creates_nothing() {
        for (config, lang) in [(DEV_MAIN, Locale::PtBr), (EN, Locale::EnUs)] {
            let dir = repo(config);
            let root = dir.path();
            add_pending(root, "Cadastro de clientes");
            let list = std::fs::read(ledger(root)).ok();
            for (written, named) in [("P-9", "P-9"), ("abc", "abc")] {
                let report = open_from(root, written);
                assert_eq!(report["ok"], json!(false), "{report}");
                assert_eq!(report["reason"], json!("pending-unknown"), "{report}");
                let expected = translate("open.pending_unknown", lang).replace("{pending}", named);
                assert_eq!(report["hint"], json!(expected), "{report}");
                nothing_created(root, list.as_ref());
            }
        }
    }

    /// Uma pendência fechada é recusada, nos dois idiomas, antes de qualquer
    /// coisa ser criada: só uma pendência aberta vira spec.
    #[test]
    fn open_with_a_closed_pending_is_refused_and_creates_nothing() {
        for (config, lang) in [(DEV_MAIN, Locale::PtBr), (EN, Locale::EnUs)] {
            let dir = repo(config);
            let root = dir.path();
            add_pending(root, "Cadastro de clientes");
            assert!(crate::commands::event::pending::close_pending(root, "P-1", "feito"));
            let list = std::fs::read(ledger(root)).ok();
            let report = open_from(root, "P-1");
            assert_eq!(report["reason"], json!("pending-closed"), "{report}");
            let expected = translate("open.pending_closed", lang).replace("{pending}", "P-1");
            assert_eq!(report["hint"], json!(expected), "{report}");
            nothing_created(root, list.as_ref());
        }
    }

    /// Sem `--pending`, a lista de pendências fica com os mesmos bytes.
    #[test]
    fn without_a_pending_the_list_is_not_touched() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        add_pending(root, "Cadastro de clientes");
        let list = std::fs::read(ledger(root)).unwrap();
        let report = open(root, Some("feature"), Some("cadastro"), Some("dev"));
        assert_eq!(report["ok"], json!(true), "{report}");
        assert!(report.get("pending").is_none(), "{report}");
        assert_eq!(std::fs::read(ledger(root)).unwrap(), list);
        assert_eq!(became(root, "cadastro"), None);
    }

    /// Chamar o `open` de novo, com a mesma pendência, devolve o mesmo
    /// relatório e não grava a nota outra vez: a lista fica com os mesmos
    /// bytes.
    #[test]
    fn running_open_again_with_its_pending_records_the_note_once() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        add_pending(root, "Cadastro de clientes");
        assert_eq!(open_from(root, "P-1")["ok"], json!(true));
        let list = std::fs::read(ledger(root)).unwrap();
        let again = open_from(root, "P-1");
        assert_eq!(again["already_open"], json!(true), "{again}");
        assert_eq!(again["pending"], json!("P-1"), "{again}");
        assert_eq!(std::fs::read(ledger(root)).unwrap(), list, "the note is written once");
        assert_eq!(became(root, "cadastro").as_deref(), Some("P-1"));
    }

    /// A nota que não se grava vira aviso, e a spec fica aberta; chamar o
    /// `open` de novo, com a lista gravável, grava a nota que faltou.
    #[cfg(unix)]
    #[test]
    fn a_note_that_cannot_be_written_warns_and_keeps_the_spec() {
        use std::os::unix::fs::PermissionsExt;
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        add_pending(root, "Cadastro de clientes");
        let list = ledger(root);
        let folder = list.parent().unwrap().to_path_buf();
        let modes = |file: u32, folder_mode: u32| {
            std::fs::set_permissions(&list, std::fs::Permissions::from_mode(file)).unwrap();
            std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(folder_mode)).unwrap();
        };
        modes(0o444, 0o555);
        // Quem roda com poder total escreve mesmo assim: aí não há o que
        // provar.
        if std::fs::write(folder.join("probe"), "x").is_ok() {
            let _ = std::fs::remove_file(folder.join("probe"));
            modes(0o644, 0o755);
            return;
        }
        let report = open_from(root, "P-1");
        modes(0o644, 0o755);
        assert_eq!(report["ok"], json!(true), "{report}");
        let expected =
            translate("open.pending_note_failed", Locale::PtBr).replace("{spec}", "cadastro").replace("{pending}", "P-1");
        assert_eq!(report["warnings"], json!([expected]), "{report}");
        assert_eq!(state(root, "cadastro").phase, Some("survey"), "the spec stays");
        assert_eq!(became(root, "cadastro"), None);
        let again = open_from(root, "P-1");
        assert!(again.get("warnings").is_none(), "{again}");
        assert_eq!(became(root, "cadastro").as_deref(), Some("P-1"), "the repeated call records the note");
    }

    /// Uma pendência que já virou outra spec passa a apontar a nova.
    #[test]
    fn a_pending_that_became_another_spec_now_points_to_the_new_one() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        add_pending(root, "Cadastro de clientes");
        assert!(mark_became(root, "P-1", "outra"));
        assert_eq!(open_from(root, "P-1")["ok"], json!(true));
        assert_eq!(became(root, "cadastro").as_deref(), Some("P-1"));
        assert_eq!(became(root, "outra"), None);
    }

    /// Aberta de um worktree, a spec vem da pendência da lista do checkout
    /// principal, e a nota entra lá.
    #[test]
    fn open_from_a_linked_worktree_notes_the_pending_of_the_main_checkout() {
        let dir = tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["config", "user.email", "t@example.com"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(main.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").unwrap();
        std::fs::write(main.join("mustard.json"), DEV_MAIN).unwrap();
        std::fs::write(main.join("README.md"), "oi\n").unwrap();
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-q", "-m", "init"]);
        let worktree = dir.path().join("wt");
        git(&main, &["worktree", "add", "-q", "-b", "scratch", &worktree.to_string_lossy()]);
        add_pending(&main, "Cadastro de clientes");

        let report = open_from(&worktree, "P-1");
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["pending"], json!("P-1"), "{report}");
        assert_eq!(became(&main, "cadastro").as_deref(), Some("P-1"));
        assert!(!ledger(&worktree).exists(), "the worktree has no list of its own");
    }

    /// A spec nasce em levantamento, com a branch e a base, gravada pelo
    /// binário.
    #[test]
    fn the_spec_is_born_in_survey_with_its_branch_and_base() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        open(root, Some("feature"), Some("x"), Some("dev"));
        let now = state(root, "x");
        assert_eq!(now.phase, Some("survey"));
        assert_eq!((now.branch.as_deref(), now.base.as_deref()), (Some("feature/x"), Some("dev")));
        let log = DiskSpecState::new(root).log("x").unwrap();
        let birth = birth_event(&log).unwrap();
        assert_eq!(birth.str_field("author"), Some("binary"));
        assert_eq!(log.events.len(), 1, "the birth is the only event");
    }

    /// O portão de escrita barra o código de uma spec aberta pelo `open`: ela
    /// está em levantamento, e não aprovada.
    #[test]
    fn a_spec_opened_by_open_blocks_production_edits_until_approved() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        open(root, Some("feature"), Some("x"), Some("dev"));
        let project = root.to_string_lossy().into_owned();
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": root.join("src").join("main.rs").to_string_lossy(), "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(project.clone()),
            ..HookInput::default()
        };
        let mut ctx = Ctx::for_test(project, Some(Trigger::PreToolUse));
        ctx.config = ProjectConfig::load(root);
        let verdict = WriteGate.evaluate(&input, &ctx).expect("the gate never errors");
        assert!(matches!(verdict, Verdict::Deny { .. }), "{verdict:?}");
    }

    /// O mapa que não atualiza só avisa: a spec é aberta.
    #[test]
    fn a_failed_map_refresh_only_warns() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let report = open_with(&opts(root, Some("feature"), Some("x"), Some("dev")), |_| Err("sem o scan".to_string()));
        assert_eq!(report["ok"], json!(true), "{report}");
        assert!(report.get("map").is_none());
        let warning = report["warnings"][0].as_str().unwrap();
        assert!(warning.contains("sem o scan") && warning.contains("mustard-rt run scan"), "{warning}");
        assert_eq!(state(root, "x").phase, Some("survey"));

        let mapped = open(root, Some("feature"), Some("y"), Some("dev"));
        assert_eq!(mapped["map"], json!({ "full": false, "read": 1 }));
    }

    /// A abertura termina na pergunta do objetivo, no idioma do projeto: o
    /// objetivo numa frase e, junto, o card, os critérios de aceite e os
    /// documentos antigos. A dica diz que o objetivo é uma frase inteira do
    /// usuário ou a sugestão que ele aprovou, e que o card, os critérios e os
    /// documentos vão logo depois dele.
    #[test]
    fn open_asks_for_the_goal_in_both_languages() {
        for (config, question, sentence, approved, along) in [
            (
                DEV_MAIN,
                "Qual o objetivo, numa frase? Pode ser a sua ou a que eu sugerir, se você aprovar. Junto \
                 dele, mande o card, os critérios de aceite e os documentos antigos, se tiver.",
                "uma frase que diz o que ele pediu",
                "ou a que você sugeriu e ele aprovou",
                "O card, os critérios de aceite e os documentos antigos que vierem junto vão logo depois",
            ),
            (
                EN,
                "What is the goal, in one sentence? It can be yours, or the one I suggest, if you approve \
                 it. Along with it, send the card, the acceptance criteria and the old documents, if you \
                 have them.",
                "one sentence saying what they asked for",
                "or the one you suggested and they approved",
                "The card, the acceptance criteria and the old documents that come along go right after it",
            ),
        ] {
            let dir = repo(config);
            let report = open(dir.path(), Some("feature"), Some("x"), Some("dev"));
            assert_eq!(report["step"], json!("ask_goal"), "{report}");
            assert_eq!(report["question"], json!(question));
            let hint = report["hint"].as_str().unwrap();
            assert!(hint.contains("feature/x") && !hint.contains("{spec}"), "{hint}");
            assert!(hint.contains(sentence), "the goal is one whole sentence of the user's: {hint}");
            assert!(hint.contains(approved), "the suggestion the user approved counts too: {hint}");
            assert!(hint.contains(along), "the card comes right after the goal: {hint}");
        }
    }

    /// O caminho de verdade da abertura: a spec nasce pelo `open`, e o
    /// usuário responde, pelo gancho da mensagem, com o objetivo na primeira
    /// frase e, junto, o card, os critérios de aceite e os documentos
    /// antigos. O `run write` grava como objetivo a primeira frase, com
    /// `origin` na mensagem, e o card como `context` logo depois; o índice
    /// mostra a primeira frase. O objetivo que não aponta uma mensagem do
    /// usuário é recusado, e nada é gravado.
    #[test]
    fn the_goal_is_the_first_sentence_of_an_answer_that_brings_the_card() {
        use crate::commands::spec_events::write::write_at;
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        assert_eq!(open(root, Some("feature"), Some("x"), Some("dev"))["step"], json!("ask_goal"));
        let goal = "Travar o merge enquanto houver pendência aberta.";
        let card = "Card MUS-12: o merge espera a revisão.\nCritérios de aceite: o merge barrado mostra a \
                    pendência.\nDocumentos antigos: docs/merge.md.";
        let prompt = HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw: json!({ "prompt": format!("{goal} {card}") }),
            ..HookInput::default()
        };
        assert!(!crate::dispatch::run_event(Some(Trigger::UserPromptSubmit), &prompt).is_blocking());
        let log = DiskSpecState::new(root).log("x").expect("the spec has an event file");
        let said = log.visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        let said = said.expect("the hook records the user's answer");
        let context = |text: &str| {
            write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                event_type: "context".into(),
                json: json!({ "text": text, "origin": said }).to_string(),
            })
        };
        let events = || std::fs::read_to_string(spec_dir(root, "x").join("spec.ndjson")).unwrap().lines().count();
        let before = events();
        let state = log.visible().into_iter().find(|e| e.event_type == "state").map(|e| e.id);
        let state = state.expect("the spec was born with its state");
        let refused = write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            event_type: "context".into(),
            json: json!({ "text": goal, "origin": state }).to_string(),
        });
        assert_eq!(refused["reason"], json!("goal-origin-not-user"), "{refused}");
        assert_eq!(events(), before, "a refusal writes nothing");
        assert_eq!(context(goal)["ok"], json!(true));
        assert_eq!(context(card)["ok"], json!(true));

        let log = DiskSpecState::new(root).log("x").expect("the spec has an event file");
        let recorded = mustard_core::domain::survey::goal(&log).expect("the goal was recorded");
        assert_eq!((recorded.str_field("text"), recorded.int("origin")), (Some(goal), Some(said)));
        let index = std::fs::read_to_string(root.join(".claude").join("spec").join("index.ndjson")).unwrap();
        let line: Value = index
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|line| line["name"] == json!("x"))
            .expect("the spec has its index line");
        assert_eq!(line["goal"], json!(goal));
    }

    /// Sem o tipo, o `open` pergunta por ele, com as sugestões; o tipo que não
    /// serve como está é recusado; sem o nome, pergunta pelo nome. Nada é
    /// criado.
    #[test]
    fn a_missing_kind_or_name_is_asked_and_a_kind_git_cannot_carry_is_refused() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let kind = open(root, None, Some("x"), Some("dev"));
        assert_eq!(kind["step"], json!("choose_kind"), "{kind}");
        assert_eq!(kind["suggested"][0], json!("feature"));
        assert_eq!(kind["name"], json!("x"));
        let invalid = open(root, Some("Feature"), Some("x"), Some("dev"));
        assert_eq!(invalid["reason"], json!("kind-invalid"), "{invalid}");
        let name = open(root, Some("fix"), None, Some("dev"));
        assert_eq!(name["step"], json!("choose_name"), "{name}");
        assert!(name["hint"].as_str().unwrap().contains("fix/"));
        assert_eq!(branches(root), vec!["dev".to_string(), "main".to_string()]);
        assert!(!root.join(".claude").join("spec").exists());
    }

    /// Depois da resposta do usuário, gravada palavra por palavra como o
    /// primeiro `context`, a linha da spec no índice mostra o objetivo.
    #[test]
    fn the_index_shows_the_goal_after_the_answer() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        open(root, Some("feature"), Some("x"), Some("dev"));
        let write = |event_type: &str, json: Value| {
            seed_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                event_type: event_type.into(),
                json: json.to_string(),
            })
        };
        let answer = "Travar o merge enquanto houver pendência aberta.";
        let message = write("message", json!({ "text": answer, "author": "user" }));
        let goal = write("context", json!({ "text": answer, "origin": message["id"] }));
        assert_eq!(goal["ok"], json!(true), "{goal}");
        let index = std::fs::read_to_string(root.join(".claude").join("spec").join("index.ndjson")).unwrap();
        let line: Value = index
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|line| line["name"] == json!("x"))
            .expect("the spec has its index line");
        assert_eq!(line["goal"], json!(answer));
    }

    /// Quando a spec não pode ser gravada depois de a branch nascer, o
    /// checkout volta para onde estava, e a branch nova, sem commit, é
    /// apagada: nada fica numa branch de trabalho sem spec, onde o portão
    /// deixaria editar. Vale saindo da base e de outra branch de trabalho, e
    /// o `open` seguinte abre a spec.
    #[test]
    fn a_spec_that_cannot_be_written_undoes_the_new_branch() {
        for from in ["dev", "feature/outra"] {
            let dir = repo(DEV_MAIN);
            let root = dir.path();
            if from != "dev" {
                git(root, &["checkout", "-q", "-b", from]);
            }
            // Um arquivo no lugar da pasta da spec faz a gravação falhar.
            std::fs::create_dir_all(root.join(".claude").join("spec")).unwrap();
            std::fs::write(spec_dir(root, "x"), "não é pasta").unwrap();
            let report = open(root, Some("feature"), Some("x"), Some("dev"));
            assert_eq!(report["ok"], json!(false), "{report}");
            assert_eq!(report["reason"], json!("io-failed"), "{report}");
            assert_eq!(head(root), from);
            assert!(!branches(root).contains(&"feature/x".to_string()), "{from}");

            std::fs::remove_file(spec_dir(root, "x")).unwrap();
            let again = open(root, Some("feature"), Some("x"), Some("dev"));
            assert_eq!(again["ok"], json!(true), "{again}");
            assert_eq!(head(root), "feature/x");
        }
    }

    /// Partindo de um checkout fora de qualquer branch, a falha ao gravar a
    /// spec também desfaz a branch nova: o checkout volta, solto, ao commit
    /// de onde saiu, e a branch nova some.
    #[test]
    fn a_spec_that_cannot_be_written_from_a_detached_head_goes_back_to_its_commit() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        git(root, &["checkout", "-q", "--detach", "dev"]);
        let from = git(root, &["rev-parse", "HEAD"]);
        std::fs::create_dir_all(root.join(".claude").join("spec")).unwrap();
        std::fs::write(spec_dir(root, "x"), "não é pasta").unwrap();
        let report = open(root, Some("feature"), Some("x"), Some("dev"));
        assert_eq!(report["ok"], json!(false), "{report}");
        assert_eq!(report["reason"], json!("io-failed"), "{report}");
        // A leitura crua aqui, e não a `head`: só o `HEAD` literal separa
        // "solto" de "não consegui ler", e é o solto que este teste afirma.
        // A leitura da biblioteca devolve ausência nos dois casos, e uma
        // asserção que aceitasse ausência passaria também com um repositório
        // ilegível.
        assert_eq!(
            git(root, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "HEAD",
            "the checkout is detached again",
        );
        assert_eq!(git(root, &["rev-parse", "HEAD"]), from);
        assert!(!branches(root).contains(&"feature/x".to_string()));
    }

    /// O desfazer nunca apaga uma branch que já tem commit próprio.
    #[test]
    fn the_undo_keeps_a_branch_with_its_own_commit() {
        let dir = repo(DEV_MAIN);
        let root = dir.path();
        let start = cut_start(root, "dev").expect("the base has a commit");
        git(root, &["checkout", "-q", "-b", "feature/y"]);
        std::fs::write(root.join("src").join("y.rs"), "fn y() {}\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "y"]);
        assert!(!undo_branch(root, "dev", Some(&start), "feature/y"));
        assert_eq!(head(root), "feature/y");
        assert!(branches(root).contains(&"feature/y".to_string()));
    }

    /// Numa branch sem nenhum commit, o `open` não teria para onde voltar se
    /// a gravação da spec falhasse: ele recusa antes de criar a branch, nos
    /// dois idiomas, dizendo o que fazer, e nada nasce, nem a branch nova nem
    /// a pasta da spec.
    #[test]
    fn a_branch_without_any_commit_is_refused_before_the_new_branch_is_created() {
        for (config, words) in [(DEV_MAIN, "primeiro commit"), (EN, "first commit")] {
            let dir = repo(config);
            let root = dir.path();
            git(root, &["checkout", "-q", "--orphan", "vazia"]);
            let report = open(root, Some("feature"), Some("x"), Some("dev"));
            assert_eq!(report["ok"], json!(false), "{report}");
            assert_eq!(report["reason"], json!("unborn-branch"), "{report}");
            let hint = report["hint"].as_str().unwrap();
            assert!(hint.contains(words) && hint.contains("dev"), "{hint}");
            assert_eq!(branches(root), vec!["dev".to_string(), "main".to_string()]);
            assert_eq!(git(root, &["symbolic-ref", "--short", "HEAD"]), "vazia");
            assert!(!spec_dir(root, "x").exists());
        }
    }
}
