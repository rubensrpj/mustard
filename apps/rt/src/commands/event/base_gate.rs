//! `base_gate` — the check that runs BEFORE ANALYZE, at the single
//! pipeline-opening door (`emit-pipeline --kind pipeline.kind`).
//!
//! ## What it guards
//!
//! A work unit is the branch plus everything the work produces, so the unit is
//! only coherent if the branch is cut from a base the project actually promotes
//! through, at that base's LATEST commit. Both facts are cheap to establish
//! exactly once — before ANALYZE reads a single file — and expensive to
//! discover later: a unit cut off another unit cannot be reviewed apart, and a
//! unit cut off a stale base re-does work that is already merged.
//!
//! Two answers, never more ([`BaseVerdict`]):
//!
//! 1. **Behind its remote** → `Refuse`, naming the exact pull to run.
//! 2. Otherwise → `Open`, and the census refresh fires when it is due.
//!
//! There is no membership answer any more. "Not an integration base" used to be
//! the first of three, tested against `git.flow`'s declared set — which refused
//! a branch cut last Tuesday with a sentence about a configuration file, in a
//! repository whose branch convention the operator does not own. What a base IS
//! is now measured where it can be: the cut point is every branch git has
//! ([`mustard_core::branch_catalog`]) and the branches that refuse a direct
//! commit are [`mustard_core::protected_branches`]. See the `evaluate` body for
//! what that deliberately gives up.
//!
//! ## Abstention is not a pass
//!
//! `Abstain` is a fourth state kept deliberately apart from `Open`: an explicit
//! `vcs: ""` opt-out, a directory that is not a repository, a git that would
//! not answer. The gate did not run — it did not approve, and the caller must
//! not read it as one. Only a POSITIVE observation ever refuses, so the gate
//! can never wedge a project it cannot reason about (the same invariant
//! [`crate::hooks::write::scan_clean_gate`] states for itself).
//!
//! **Offline is not a verdict either.** Freshness needs the network; when the
//! fetch fails there is no evidence the base is behind, so the gate opens.
//! Refusing there would ground every offline session on a fact nobody measured.
//!
//! ## Why the census refresh lives here
//!
//! In a SHARED install `/scan` rewrites VERSIONED artifacts — the grain model,
//! its dictionary — so it needs a clean tree to stay its own reviewable commit;
//! that is precisely what `scan_clean_gate` refuses to let happen on a dirty
//! one. A freshly updated base, before the first edit, is the one moment in the
//! flow where a clean tree holds by construction, which is why the refresh is
//! triggered from this gate instead of from a door the user has to remember. It
//! is best-effort throughout: a stale census is a worse map, never a blocker.
//!
//! And it FINISHES that commit rather than announcing it. This module no longer
//! decides WHEN or WHERE, though: the re-mine writes files and stops
//! ([`mine_census_if_stale`]), and the commit is made by
//! [`super::census_settlement`], the one place that weighs what is dirty,
//! where the checkout stands and what is about to happen. Leaving the write
//! dirty made the next unit's branch cut refuse, attributing the write to
//! another unit of the operator's work — one manual commit per pipeline opened;
//! deciding it HERE, in a second spelling, is how the recording and the cut came
//! to disagree about the same tree.
//!
//! In a PRIVATE install the census is invisible to the host repository's git,
//! so there is no commit to keep apart and the tree's state decides nothing —
//! staleness alone is the whole question. Both readings come from the ONE
//! predicate [`crate::hooks::write::scan_clean_gate::scan_output_is_versioned`],
//! shared with the door that refuses, so the automatic path can never start
//! mining exactly where the user-invoked one is turned away.

use std::path::Path;

use mustard_core::{record_written_path, RecordOutcome, ProjectConfig, Scan};

use super::work_branch::CheckoutWork;
use crate::commands::git_settle::git_out;
use crate::commands::scan::{default_model_path, hollow_submodules};
use crate::commands::spec::active_specs::{active_spec_names, without_spec_date_prefix};
use crate::commands::spec::spec_slug::canonical_for_project;
use crate::util::format_gate_message;

/// The closed set of answers the base gate can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BaseVerdict {
    /// The checkout IS an integration base and carries no commit its remote
    /// has already published. The pipeline may open; the named base is the one
    /// the unit will be cut from.
    Open(String),
    /// Nothing to judge — VCS opt-out, not a repository, or a branch probe that
    /// did not answer. Never an approval: the gate simply did not run.
    Abstain,
    /// The pipeline must NOT open here. Carries the didactic refusal, which
    /// always names the command that resolves it.
    Refuse(String),
}

/// Judge the current checkout as a base to cut a unit from.
///
/// One question survives: is it up to date with its remote? A unit cut from a
/// stale base re-does merged work and conflicts on the way back, and NO branch
/// convention protects against that — which is why this is the check that
/// stayed when the membership test went.
///
/// `project` is the state root (where `mustard.json` and `.claude/` live) and
/// also the tree the branch is read from — opening a NEW pipeline from inside
/// a work unit's own worktree is exactly the case this gate exists to refuse,
/// so there is no local-tree redirect here.
pub(crate) fn evaluate(project: &Path, config: &ProjectConfig) -> BaseVerdict {
    // An explicit `vcs: ""` opt-out means the project declined branch
    // management altogether; there is no base to be on.
    if config.vcs().is_none() {
        return BaseVerdict::Abstain;
    }
    let Some(current) = git_out(project, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty())
    else {
        // Not a repository, no git on PATH, an unborn HEAD — unmeasured.
        return BaseVerdict::Abstain;
    };

    // NO membership test any more. It used to read the declared base set and
    // refuse everything outside it, which meant a branch cut last Tuesday was
    // told it "is not an integration base of this project" — a sentence about a
    // configuration file, delivered as if it were a sentence about the
    // repository. In a client repository, where
    // the operator does not own the branch convention, the only offered way out
    // was to edit that file per project.
    //
    // What is DELIBERATELY given up: the gate no longer distinguishes a base
    // from another unit's work branch, so it can no longer refuse a unit cut
    // off another unit. That refusal was only ever possible because the base
    // set was closed, and a closed set is exactly what made the common case
    // wrong. Stacking a unit on another branch is legitimate in the flows this
    // opens up for; the picker shows what each candidate IS, and the choice is
    // the operator's. The safety that survives is the one no convention can
    // supply for itself — see below.
    match commits_behind_remote(project, &current) {
        Some(behind) if behind > 0 => BaseVerdict::Refuse(behind_reason(&current, behind)),
        // `None` = unmeasured (offline, no remote-tracking ref): open.
        _ => BaseVerdict::Open(current),
    }
}

/// Gate title every refusal carries — the `[Base Gate]` prefix
/// [`format_gate_message`] renders.
const GATE: &str = "Base Gate";

/// The refusal for a base that trails its remote, naming the exact pull.
fn behind_reason(base: &str, behind: u64) -> String {
    let plural = if behind == 1 { "commit" } else { "commits" };
    format_gate_message(
        GATE,
        &format!("the integration base '{base}' is {behind} {plural} behind origin/{base}"),
        "a unit cut from a stale base re-does work that is already merged and conflicts \
         on the way back",
        &format!("git pull --ff-only origin {base}"),
    )
}

/// How many commits `origin/<base>` carries that the checkout does not.
///
/// `None` whenever the question could not be answered — the fetch failed
/// (offline, no remote), or there is no `origin/<base>` ref to compare with.
/// The caller reads that as "unmeasured" and opens; see the module doc.
fn commits_behind_remote(project: &Path, base: &str) -> Option<u64> {
    // Refresh the remote-tracking refs first: without it the count is measured
    // against whatever the last fetch left behind, which is exactly the stale
    // reading this check exists to catch.
    git_out(project, &["fetch", "origin"])?;
    let range = format!("HEAD..origin/{base}");
    git_out(project, &["rev-list", "--count", &range])?.trim().parse::<u64>().ok()
}

/// `true` when the deterministic census is worth re-mining AND re-mining it
/// can still be a commit of its own — the conjunction the gate acts on.
///
/// Split out of [`mine_census_if_stale`] so the DECISION is testable without
/// the grain sidecar binary: the effect needs it, the judgement does not.
///
/// `work` is the tree as [`super::census_settlement`] measured it — HANDED IN,
/// never measured again here. This function used to run its own
/// `git status --porcelain --untracked-files=all`, which made one pipeline
/// opening walk the whole tree twice for one answer.
pub(crate) fn census_refresh_due(project: &Path, model: &Path, work: &CheckoutWork) -> bool {
    if !census_is_stale(project, model) {
        return false;
    }
    // A census git cannot see never fuses with the user's work, so staleness is
    // the whole question there. Without this, a client repository — dirty
    // nearly always — would carry a census that silently never refreshed.
    //
    // The question is asked of the FILES, not of the install mode. The mode
    // predicate reads "private install ⇒ invisible", and this very repository
    // falsifies it: it carries both private marks in `info/exclude` AND a
    // tracked census. Under the coarse answer the gate re-mined on a dirty tree
    // and then had to leave the versioned result uncommitted — the debt-
    // admission this whole unit exists to delete. `record_written_path` already
    // judges per path; this now asks the same fact of the same paths, so the
    // two halves of one decision can no longer disagree.
    if !census_is_visible_to_git(project, model) {
        return true;
    }
    // Shared install: only a POSITIVE clean tree qualifies. `None` (no git,
    // unreadable status) is unmeasured, and a refresh mined over unknown dirt
    // is exactly what `scan_clean_gate` refuses for the user-invoked door.
    //
    // A saída do PRÓPRIO censo é descontada, e é o que torna UM commit possível.
    // Enquanto a passagem de enriquecimento suja a árvore, o mine se considerava
    // impedido por ela — então o portão tinha de gravar ANTES para se
    // desimpedir, e gravava o modelo VELHO sob o assunto do censo; o mine então
    // gravava o modelo novo sob o MESMO assunto, e sobravam dois commits com o
    // mesmo título, o primeiro registrando conteúdo que a própria ferramenta
    // acabara de superar. Descontando a saída da ferramenta, o mine roda
    // primeiro e a gravação acontece uma vez só, no fim — e não aqui, e sim em
    // [`super::census_settlement::settle`], que é quem decide onde ela pode
    // cair.
    //
    // O que NÃO é descontado continua sendo tudo: uma linha do operador junto
    // devolve `Holds` e o mine segue impedido, que é a regra de sempre.
    // `Unproven` (sem git, status ilegível) não autoriza nada, exatamente como
    // o `None` de antes.
    matches!(work, CheckoutWork::ProvenClean | CheckoutWork::CensusOnly(_))
}

/// `true` when the census on disk describes an older tree than the one checked
/// out: it is absent, or HEAD's commit is newer than the model file.
///
/// The commit date is the honest clock here. A working-tree mtime sweep would
/// re-mine after every checkout touch, and a content hash costs a full walk —
/// the thing the refresh itself is trying to earn. Unreadable either side ⇒
/// `false`: with no evidence the tree moved, a full workspace walk is not
/// something to spend on a guess.
fn census_is_stale(project: &Path, model: &Path) -> bool {
    if !model.is_file() {
        return true;
    }
    let Some(head_committed_at) = git_out(project, &["log", "-1", "--format=%ct", "HEAD"])
        .and_then(|s| s.trim().parse::<u64>().ok())
    else {
        return false;
    };
    let Some(model_written_at) = model
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
    else {
        return false;
    };
    head_committed_at > model_written_at
}

/// Re-mine `<project>/.claude/grain.model.json` when [`census_refresh_due`]
/// says so. Deterministic census only — the `--full` pass that rewrites every
/// `scan-map.md` and each subproject's `## Guards` stays with the FLOW, which
/// dispatches it as a work unit of its own once this gate reports the gap
/// ([`super::enrichment_gap`]). It is not an explicit door the user types: that
/// one was sealed, and rewriting versioned files needs a clean tree and a commit
/// apart, which is a unit, not a side effect of opening another one.
///
/// Fail-open at every step, and loud on stderr rather than on stdout: this runs
/// inside `emit-pipeline`, whose one JSON line is byte-compared by gates.
///
/// **It writes FILES and records NOTHING.** The commit that finishes the job is
/// [`super::census_settlement::settle`]'s, and that is the whole point of the
/// split: this function used to record on its own terms — a `worktree_is_clean`
/// sample of its own, with no idea where the checkout was standing — which made
/// it the one writer of the census commit that no positional guard ever
/// covered. It could commit the census onto another unit's branch while the two
/// cutting doors, three call sites away, were refusing to do exactly that.
///
/// `work` is the tree as the settlement measured it, and `on_the_base` is that
/// settlement's positional answer. The second one gates the WRITE, not just the
/// commit: mining a VISIBLE census where it could not be recorded only dirties
/// the tree for the next cut to refuse — the tool blocking itself on its own
/// output, one step upstream. A census git cannot see is exempt, because it
/// fuses with nobody's work and lands in no commit, so no position is wrong for
/// it (a private install is dirty nearly always, and gating it on position
/// would mean its census never refreshed at all).
///
/// `true` when the miner was actually invoked — the only observable this
/// function has, and what the postponement above is tested through.
pub(crate) fn mine_census_if_stale(
    project: &Path,
    work: &CheckoutWork,
    on_the_base: bool,
) -> bool {
    let model = default_model_path(project);
    if !census_refresh_due(project, &model, work) {
        return false;
    }
    if census_is_visible_to_git(project, &model) && !on_the_base {
        eprintln!(
            "base-gate: census refresh postponed — the checkout is not the base this open cuts \
             from, so a re-mined census could not be recorded here and would be left for the \
             next branch cut to refuse"
        );
        return false;
    }
    // The same preflight `scan` runs: an unpopulated submodule is
    // indistinguishable from an absent subtree once the walk starts, and the
    // previous complete model is strictly better than a hollow replacement.
    let hollow = hollow_submodules(project);
    if !hollow.is_empty() {
        eprintln!(
            "base-gate: census refresh skipped — empty submodule(s) {}; the model would \
             silently omit them. Run: git submodule update --init --recursive",
            hollow.join(", ")
        );
        return false;
    }
    match Scan::locate().scan(project, &model) {
        Ok(()) => eprintln!(
            "base-gate: census refreshed ({}) — the recording that follows is the settlement's",
            model.display()
        ),
        Err(e) => eprintln!("base-gate: census refresh failed ({e}); the previous model stands"),
    }
    true
}

/// Os caminhos que o mine determinístico ESCREVE, relativos à raiz — o modelo e
/// o dicionário ao lado dele —, e só os que existem mesmo em disco.
///
/// DERIVADOS, nunca medidos de novo: um `git status` a mais por causa de dois
/// caminhos conhecidos é exatamente a segunda varredura que este trabalho
/// removeu. Um pathspec para um arquivo que não mudou é inócuo para o gravador
/// (ele compara o porcelain dos caminhos dados e devolve `Nothing` se não houver
/// nada), mas um pathspec para um arquivo AUSENTE aborta o `git add` inteiro
/// (`fatal: pathspec did not match any files`) e levaria o outro junto — o
/// resultado que esta filtragem existe para evitar, alcançado por excesso de
/// zelo.
pub(crate) fn mined_census_paths(project: &Path) -> Vec<String> {
    let model = default_model_path(project);
    [model.clone(), model.with_file_name(GRAIN_DICTIONARY)]
        .into_iter()
        .filter(|written| written.is_file())
        .filter_map(|written| {
            written.strip_prefix(project).ok().map(|rel| rel.to_string_lossy().replace('\\', "/"))
        })
        .collect()
}

/// Grava `paths` como UM commit do censo, pela única máquina que o produto usa
/// para tudo que escreve numa árvore que o repositório versiona
/// ([`record_written_path`]), sob o assunto do censo
/// ([`CENSUS_COMMIT_SUBJECT`]).
///
/// SEM DECIDIR NADA. Quem decidiu que havia um commit a fazer, e onde ele podia
/// cair, foi [`super::census_settlement::settle`] — esta função é o efeito, e a
/// separação é o que impede a terceira leitura da mesma pergunta. Por isso o
/// `found_clean` é `Some(true)` sem hesitação: o chamador só chega aqui depois
/// de ter medido que a árvore e o índice não têm uma linha do operador para o
/// commit varrer junto, que é exatamente o fato que `record_written_path` pede.
///
/// `true` quando o commit foi mesmo escrito. Ignorado, invisível para o git ou
/// recusado por ele: `false`, e a escrita fica onde caiu — fail-open, como o
/// mine determinístico já degrada. Mas NUNCA em silêncio: cada
/// [`RecordOutcome`] imprime a sua linha no stderr, do catálogo, no idioma do
/// projeto. Uma gravação que falhava calada deixava o censo sujo, e o corte
/// seguinte o recusava nomeando `grain.model.json` como trabalho não commitado
/// do operador — sem aviso prévio de que fora a ferramenta que o deixou ali.
/// "Prosseguir depois de uma gravação que falhou" e "prosseguir sem dever
/// nada" são fatos diferentes, e é esta linha que diz qual dos dois foi.
pub(crate) fn commit_census(project: &Path, paths: &[String]) -> bool {
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let outcome = record_written_path(project, &refs, CENSUS_COMMIT_SUBJECT, Some(true));
    let key = match outcome {
        RecordOutcome::Recorded => "basegate.census.recorded",
        RecordOutcome::Nothing => "basegate.census.nothing",
        RecordOutcome::TreeNotClean => "basegate.census.not_clean",
        RecordOutcome::Unavailable => "basegate.census.unavailable",
    };
    let lang = ProjectConfig::load(project).i18n().lang;
    eprintln!(
        "{}",
        mustard_core::translate(key, lang).replace("{paths}", &paths.join(", "))
    );
    outcome == RecordOutcome::Recorded
}

/// Quantos tokens significativos duas unidades precisam compartilhar para uma
/// virar suspeita da outra. Dois, porque um só ("harness", "spec") é o
/// vocabulário do projeto inteiro e apontaria todas as unidades abertas.
const OVERLAP_MIN_TOKENS: usize = 2;

/// Comprimento mínimo de um token para ele contar. Abaixo disso sobra a cola
/// que o slug não removeu, não o assunto.
const OVERLAP_MIN_TOKEN_LEN: usize = 3;

/// Os tokens de um slug que dizem sobre O QUÊ ele é: sem o prefixo de data que
/// alguns diretórios de spec carregam (puro dígito) e sem as partículas curtas.
fn significant_tokens(slug: &str) -> std::collections::BTreeSet<String> {
    slug.split('-')
        .filter(|t| t.len() >= OVERLAP_MIN_TOKEN_LEN && !t.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// As specs ATIVAS que o `--intent` desta abertura parece repetir — suspeitas,
/// nunca um veredito: o retorno é relatado (`overlappingSpecs`) e não bloqueia
/// nada. Duas unidades abertas sobre o mesmo assunto é uma decisão do operador,
/// e o portão que a tomasse por ele erraria justamente nos casos legítimos
/// (a segunda onda de um assunto, um fix adjacente).
///
/// A comparação roda na MESMA derivação que nomeia a unidade
/// ([`canonical_for_project`]), então o intent e o diretório da spec chegam
/// aqui na mesma grafia, já sem stopwords e no idioma que o projeto declara. As
/// specs vêm do MESMO localizador que o `active-specs` usa
/// ([`active_spec_names`]) — um segundo enumerador é como o portão e o picker
/// passariam a discordar sobre o que está aberto.
///
/// A unidade que está sendo aberta NÃO é suspeita de si mesma. Isso é inócuo na
/// primeira abertura, quando o diretório da spec ainda não existe, e errado em
/// todo RE-despacho de uma unidade já aberta — que é justamente o que o
/// `dispatch.md` manda fazer depois de uma recusa do portão. O nome descartado
/// vem da MESMA derivação que nomeia a unidade, então os dois lados não têm como
/// discordar sobre qual é ele.
///
/// A comparação roda SEM o prefixo de data: um diretório pode se chamar
/// `2026-05-23-harness-enxerga-toda-branch` e `canonical_for_project` nunca
/// produz a data, então a exclusão exata não casava e a unidade se acusava de
/// sobrepor a si mesma. A remoção vai pelo MESMO helper que o picker usa
/// ([`without_spec_date_prefix`]) — [`significant_tokens`] já descarta os
/// tokens puramente numéricos porque sabe que o prefixo existe; a exclusão
/// passa a saber também.
///
/// Determinístico: a ordem é a do localizador (ordenada), e nada de timestamp
/// ou caminho volátil entra no resultado.
pub(crate) fn overlapping_active_specs(project: &Path, intent: &str) -> Vec<String> {
    let intent = intent.trim();
    if intent.is_empty() {
        return Vec::new();
    }
    let own = canonical_for_project(intent, project);
    let wanted = significant_tokens(&own);
    if wanted.len() < OVERLAP_MIN_TOKENS {
        return Vec::new();
    }
    active_spec_names(project)
        .into_iter()
        .filter(|name| without_spec_date_prefix(name) != without_spec_date_prefix(&own))
        .filter(|name| {
            significant_tokens(name).intersection(&wanted).count() >= OVERLAP_MIN_TOKENS
        })
        .collect()
}

/// The commit subject the gate writes when it records a census it re-mined.
///
/// Deliberately plain: it describes the file that changed and names no tool.
/// The commit lands in the OPERATOR's history, next to their own work.
pub(crate) const CENSUS_COMMIT_SUBJECT: &str = "chore: refresh the deterministic project census";

/// The scan's second versioned artifact, written beside the model on every run.
/// Named here because the recording has to cover everything the miner wrote:
/// leaving it out left the tree dirty under a message claiming it was clean.
const GRAIN_DICTIONARY: &str = "grain.dictionary.json";

/// Whether git would SEE the census — asked of the files, not of the install
/// mode.
///
/// Visible means "no ignore rule hides it", which is the same fact
/// [`record_written_path`] judges when it decides whether a write is worth
/// recording. Tracked would be the wrong question: a first mine is untracked by
/// definition and still shows up as `??`, so answering "invisible" there would
/// re-mine onto a dirty tree and leave exactly the dirt this unit removes.
///
/// Not the install mode either. That predicate reads "private install ⇒
/// invisible", and this very repository falsifies it: it carries private marks
/// in `info/exclude` AND a tracked census. Under the coarse answer the gate
/// re-mined on a dirty tree and then had to leave the versioned result
/// uncommitted — the debt-admission this unit exists to delete.
///
/// Unmeasured (no git, no repository) reads as INVISIBLE, the direction the
/// mode predicate also took: a census nobody can see is one staleness alone
/// should decide.
fn census_is_visible_to_git(project: &Path, model: &Path) -> bool {
    [model.to_path_buf(), model.with_file_name(GRAIN_DICTIONARY)]
        .iter()
        .filter_map(|p| p.strip_prefix(project).ok())
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .any(|rel| !path_is_ignored(project, &rel))
}

/// `true` when git would ignore `rel` — an ignore rule or the clone-local
/// exclude file a private install writes into; `check-ignore` reads both.
/// `false` when git could not answer, so an unmeasured path counts as visible
/// and the stricter clean-tree requirement applies.
pub(crate) fn path_is_ignored(project: &Path, rel: &str) -> bool {
    std::process::Command::new("git")
        .args(["check-ignore", "-q", "--", rel])
        .current_dir(project)
        .output()
        .is_ok_and(|out| out.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::event::census_settlement::{
        settle, CensusDoor, CensusSettlement, CheckoutPosition,
    };
    use crate::commands::event::work_branch::checkout_work;
    use std::process::Command;

    /// A pergunta inteira, feita como a porta de CORTE a faz.
    ///
    /// As fixtures deste módulo medem pelo MESMO ponto de entrada que o produto
    /// usa, e não por uma metade dele: enquanto mediam a decisão de um lado e a
    /// gravação do outro, as duas ficaram verdes enquanto o par entre elas
    /// estava quebrado — quatro rodadas seguidas.
    fn settle_cut(
        root: &Path,
        current: Option<&str>,
        target: &str,
        base: Option<&str>,
        config: &ProjectConfig,
    ) -> CensusSettlement {
        settle(
            root,
            CheckoutPosition::at(current, Some(target), base),
            config,
            CensusDoor::BranchCut,
        )
    }

    /// …e como a porta EXPLÍCITA do `emit-pipeline` a faz: sem alvo, porque ali
    /// nada é checado out e portanto nada pode viajar.
    fn settle_open(
        root: &Path,
        current: Option<&str>,
        base: Option<&str>,
        config: &ProjectConfig,
    ) -> CensusSettlement {
        settle(
            root,
            CheckoutPosition::at(current, None, base),
            config,
            CensusDoor::ExplicitOpen,
        )
    }

    /// Run a git command in `root`, asserting success — test scaffolding only.
    fn git(root: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed");
    }

    /// A `dev`/`main` project config — the base set is derived, never hardcoded.
    fn flow_config() -> ProjectConfig {
        let mut config = ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "dev".to_string());
        config.git.flow.insert("dev".to_string(), "main".to_string());
        config
    }

    /// Init a repo whose single commit lives on `base`.
    ///
    /// The line-ending config is not cosmetic. These fixtures assert the BYTES
    /// git puts back on disk after a `reset --hard`, a fast-forward or a stash
    /// pop, and the Windows runner carries `core.autocrlf=true` globally — so
    /// the same commit checks out with CRLF there and every byte comparison
    /// fails while the content is identical. Pinning both keys makes the
    /// fixture answer the same on every platform. Writing git config is
    /// confined to `#[cfg(test)]` by the root `CLAUDE.md` guard; this is that
    /// carve-out, not an exception to it.
    fn init_repo_on(root: &Path, base: &str) {
        git(root, &["init"]);
        git(root, &["config", "core.autocrlf", "false"]);
        git(root, &["config", "core.eol", "lf"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", base]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);
    }

    /// AC-1 — the refusal this test used to assert is GONE, and its absence is
    /// the feature. A branch the project never declared is an ordinary base:
    /// `release/2026-Q3` is cut on a Tuesday and works the same afternoon,
    /// where before it was told it "is not an integration base of this
    /// project" — a sentence about a configuration file dressed up as a
    /// sentence about the repository.
    #[test]
    fn accepts_any_real_branch_as_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "release/2026-Q3");

        assert_eq!(
            evaluate(root, &flow_config()),
            BaseVerdict::Open("release/2026-Q3".to_string()),
            "a branch git really has is a base, declared or not",
        );
    }

    /// AC-6 — the compatibility half, and the reason `git.flow` was kept rather
    /// than deleted: a project that still declares one is not restricted BY it.
    /// The declaration survives as a hint for where a picker opens; it decides
    /// nothing here.
    #[test]
    fn a_declared_flow_preselects_without_refusing_others() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "squad-b/integration");

        let config = flow_config(); // declares dev and main, and neither is this
        assert_eq!(
            evaluate(root, &config),
            BaseVerdict::Open("squad-b/integration".to_string()),
            "an undeclared branch opens exactly like a declared one",
        );

        let declared = config.git.preselected_bases();
        assert!(
            declared.contains("dev") && !declared.contains("squad-b/integration"),
            "the flow still says what it always said — it just no longer refuses: {declared:?}",
        );
        assert_eq!(config.git.primary_base(), "dev", "and it still seeds the cursor");
    }

    /// Agnostic: a `develop`/`master` project judges against ITS bases — being
    /// on `develop` opens, and no `dev`/`main` literal is involved.
    #[test]
    fn opens_on_an_integration_base_of_any_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "develop");

        let mut config = ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "develop".to_string());
        config.git.flow.insert("develop".to_string(), "master".to_string());

        // No `origin` remote ⇒ freshness is unmeasured, which opens (offline is
        // not a verdict).
        assert_eq!(
            evaluate(root, &config),
            BaseVerdict::Open("develop".to_string()),
            "a bare integration base with no measurable remote opens",
        );
    }

    /// A base whose remote has moved ahead refuses, and the refusal spells the
    /// pull out — the whole point of measuring instead of warning.
    #[test]
    fn refuses_when_the_base_is_behind_origin_and_names_the_pull() {
        let tmp = tempfile::tempdir().unwrap();

        // A bare "remote" whose HEAD is `dev` (set explicitly — do not depend
        // on the git version's default-branch flag).
        let remote = tmp.path().join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        let remote_s = remote.to_str().unwrap();
        git(&remote, &["init", "--bare"]);
        git(&remote, &["symbolic-ref", "HEAD", "refs/heads/dev"]);

        // A seed clone publishes the first `dev` commit.
        let seed = tmp.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        init_repo_on(&seed, "dev");
        git(&seed, &["remote", "add", "origin", remote_s]);
        git(&seed, &["push", "origin", "dev"]);

        // The project clone starts level with origin/dev...
        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        git(&proj, &["clone", remote_s, "."]);
        assert_eq!(
            evaluate(&proj, &flow_config()),
            BaseVerdict::Open("dev".to_string()),
            "level with its remote, the base opens",
        );

        // ...then origin/dev gains a commit this clone has never seen.
        std::fs::write(seed.join("f.txt"), "two").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-m", "two"]);
        git(&seed, &["push", "origin", "dev"]);

        let BaseVerdict::Refuse(reason) = evaluate(&proj, &flow_config()) else {
            panic!("a base behind its remote must refuse before ANALYZE");
        };
        assert!(reason.contains("behind origin/dev"), "says what it measured: {reason}");
        assert!(
            reason.contains("git pull --ff-only origin dev"),
            "names the pull command: {reason}",
        );
    }

    /// A directory that is not a repository, and an explicit `vcs: ""` opt-out,
    /// both ABSTAIN — the gate never blocks what it could not measure.
    #[test]
    fn abstains_without_a_repository_or_with_vcs_opted_out() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            evaluate(dir.path(), &flow_config()),
            BaseVerdict::Abstain,
            "not a repository — unmeasured, never refused",
        );

        let repo = tempfile::tempdir().unwrap();
        init_repo_on(repo.path(), "dev_unit");
        let mut opted_out = flow_config();
        opted_out.vcs = Some(String::new());
        assert_eq!(
            evaluate(repo.path(), &opted_out),
            BaseVerdict::Abstain,
            "an explicit vcs opt-out has no base to be on",
        );
    }

    /// The hidden-census reading of the same decision: a census no git can see
    /// has no commit of its own to keep apart from the dirt, so a dirty tree
    /// disqualifies nothing and staleness alone decides. Without this the
    /// census on a client repository silently never refreshed — the tree there
    /// is dirty nearly always.
    ///
    /// The fixture excludes the CENSUS, not merely the two marks that DETECT a
    /// private install (`settings.local.json`, `CLAUDE.local.md`). A real
    /// private install excludes both census artifacts, and writing only the
    /// marks modelled an install that does not exist: the census stayed plainly
    /// visible while the test asserted it was hidden. That gap is why the
    /// decision now asks whether git can SEE these files instead of which mode
    /// the install is in — this repository carries the marks AND a tracked
    /// census, and the coarse answer sent it down the wrong branch.
    #[test]
    fn a_hidden_census_refreshes_on_a_dirty_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "dev");
        let model = default_model_path(root);

        let info = root.join(".git").join("info");
        std::fs::create_dir_all(&info).unwrap();
        let mut rules: Vec<String> =
            mustard_core::PRIVATE_MARKS.iter().map(|m| (*m).to_string()).collect();
        rules.push(".claude/grain.model.json".to_string());
        rules.push(".claude/grain.dictionary.json".to_string());
        std::fs::write(info.join("exclude"), rules.join("\n") + "\n").unwrap();

        std::fs::write(root.join("stray.txt"), "x").unwrap();
        assert!(
            census_refresh_due(root, &model, &checkout_work(root)),
            "a census git cannot see has no commit of its own to keep apart from the dirt",
        );
    }

    /// …and the counter-case the old mode predicate got wrong. Private marks
    /// present, census NOT excluded — this repository's own shape. The census
    /// is visible, so a dirty tree must postpone the re-mine rather than mine
    /// into it and leave a versioned file uncommitted.
    #[test]
    fn a_visible_census_postpones_the_refresh_on_a_dirty_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "dev");
        let model = default_model_path(root);

        let info = root.join(".git").join("info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("exclude"), mustard_core::PRIVATE_MARKS.join("\n") + "\n")
            .unwrap();

        std::fs::write(root.join("stray.txt"), "x").unwrap();
        assert!(
            !census_refresh_due(root, &model, &checkout_work(root)),
            "the marks say `private` but nothing hides the census: mining here would leave \
             a versioned file dirty, which is the debt this unit removes",
        );
    }

    /// The refresh decision is the CONJUNCTION: an absent model on a clean tree
    /// is due; the same absent model on a dirty tree is not, because the refresh
    /// could no longer be committed apart from the user's work.
    #[test]
    fn census_refresh_needs_both_staleness_and_a_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "dev");
        let model = default_model_path(root);

        assert!(
            census_refresh_due(root, &model, &checkout_work(root)),
            "no model at all on a clean tree is the clearest possible staleness",
        );

        // Dirty the tree with a file `git add -A` would stage.
        std::fs::write(root.join("stray.txt"), "x").unwrap();
        assert!(
            !census_refresh_due(root, &model, &checkout_work(root)),
            "a dirty tree fuses the refresh with the user's work — never mine there",
        );

        // Clean again, with the model written AFTER the last commit: the census
        // already describes this tree, so there is nothing to re-mine. The
        // commit lands FIRST on purpose — `%ct` has one-second resolution, so
        // writing the model afterwards is what makes the comparison decidable
        // instead of a race with the clock.
        std::fs::remove_file(root.join("stray.txt")).unwrap();
        std::fs::write(root.join(".gitignore"), ".claude/\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "ignore claude"]);
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, "{}").unwrap();
        assert!(
            !census_refresh_due(root, &model, &checkout_work(root)),
            "a model newer than HEAD is not stale: {}",
            model.display(),
        );
    }

    /// `git status --porcelain` for `root` — the tree as the NEXT command's
    /// clean-tree guard will read it.
    fn porcelain(root: &Path) -> String {
        let out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root)
            .output()
            .expect("git status");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repo on `dev` whose `.claude/grain.model.json` is TRACKED and
    /// committed — the shape this repository has, and the only one where a
    /// census refresh can dirty anything at all. Returns the model path.
    /// The fixture tracks BOTH artifacts a scan writes, because the real miner
    /// writes both. Tracking only the model made the AC-2 test a false
    /// positive: it passed while the field run left the dictionary sidecar
    /// modified and the tree dirty.
    fn repo_tracking_the_census(root: &Path) -> std::path::PathBuf {
        init_repo_on(root, "dev");
        let model = default_model_path(root);
        std::fs::create_dir_all(model.parent().expect("model parent")).unwrap();
        std::fs::write(&model, "{\"projects\":[]}\n").unwrap();
        std::fs::write(model.with_file_name(GRAIN_DICTIONARY), "{\"terms\":[]}\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "track the census"]);
        assert_eq!(porcelain(root), "", "the fixture must start clean");
        model
    }

    /// O caminho do modelo desta árvore, para quem só tem a raiz em mãos.
    fn model_of(root: &Path) -> std::path::PathBuf {
        default_model_path(root)
    }

    /// Everything a scan writes, as the miner would — model AND sidecar.
    fn remine(model: &Path) {
        std::fs::write(model, "{\"projects\":[{\"dir\":\"apps/rt\"}]}\n").unwrap();
        std::fs::write(model.with_file_name(GRAIN_DICTIONARY), "{\"terms\":[\"wave\"]}\n").unwrap();
    }

    /// AC-2 — the refresh finishes its own job. Re-mining a VERSIONED census on
    /// a tree the gate found clean leaves the tree clean again, with no manual
    /// commit in between.
    ///
    /// This is the defect the installer's version stamp had, with another file:
    /// the write landed, the gate announced it as work the operator could
    /// "commit apart", and the next unit's branch cut refused — five times in
    /// one session.
    #[test]
    fn census_refresh_leaves_the_tree_clean() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = repo_tracking_the_census(root);
        assert_eq!(porcelain(root), "", "the fixture tree is clean");

        // The miner's effect, without the grain binary — BOTH artifacts move.
        remine(&model);
        assert_ne!(porcelain(root), "", "the re-mined census really did dirty the tree");

        // Through the real door. The recording used to be reachable on its own,
        // with a clean-tree sample of its own — which is precisely how it became
        // the one writer no positional guard ever covered.
        assert!(
            matches!(
                settle_open(root, Some("dev"), Some("dev"), &flow_config()),
                CensusSettlement::Recorded(_)
            ),
            "a census the gate itself wrote on a clean base is the gate's to record",
        );
        assert_eq!(
            porcelain(root),
            "",
            "the next unit's branch cut must find nothing to blame on the operator",
        );
    }

    /// AC-3 — and it never finishes SOMEONE ELSE's. A tree that already carried
    /// the operator's work is left entirely alone: nothing is committed, and
    /// their change is neither swept into a commit of ours nor staged.
    #[test]
    fn census_refresh_never_commits_over_the_operators_work() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = repo_tracking_the_census(root);

        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        // The operator's work, present BEFORE the gate looks.
        std::fs::write(root.join("theirs.txt"), "mine, not yours\n").unwrap();
        assert!(porcelain(root).contains("theirs.txt"), "the tree already carried their work");

        std::fs::write(&model, "{\"projects\":[{\"dir\":\"apps/rt\"}]}\n").unwrap();

        assert_eq!(
            settle_open(root, Some("dev"), Some("dev"), &flow_config()),
            CensusSettlement::Proceed,
            "with the operator's work in the tree the gate records nothing",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "no commit was written at all",
        );
        let status = porcelain(root);
        assert!(
            status.contains("theirs.txt"),
            "their file is untouched and still theirs to commit: {status}",
        );
        assert_eq!(
            std::fs::read_to_string(root.join("theirs.txt")).unwrap(),
            "mine, not yours\n",
            "and its bytes were never rewritten",
        );
    }

    /// Deixa na árvore, e só na árvore, a saída da passagem de ENRIQUECIMENTO —
    /// o mapa de um subprojeto e um molde `{papel}-pattern`, que o mine
    /// determinístico não escreve e por isso não grava.
    fn leftover_enrichment(root: &Path) {
        let claude = root.join("apps").join("rt").join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("scan-map.md"), "Tipo: cargo · 307 arquivos\n").unwrap();
        let mold = claude.join("skills").join("rt-gate-pattern");
        std::fs::create_dir_all(&mold).unwrap();
        // `source: scan` é o ÚNICO marcador que declara o molde como saída da
        // ferramenta — a regra canônica de `scan_patterns::origin`, que é
        // também a que a passagem de enriquecimento carimba em tudo que escreve.
        std::fs::write(
            mold.join("SKILL.md"),
            "---\nname: rt-gate-pattern\nsource: scan\n---\n",
        )
        .unwrap();
    }

    /// AC-7 — a abertura ORDINÁRIA: o operador parado NA base, a árvore suja só
    /// com o censo, e o corte da próxima unidade NÃO é recusado — o portão fecha
    /// a conta ele mesmo, em vez de deixá-la para o operador.
    ///
    /// Era a ferramenta se barrando nas próprias saídas: a passagem de
    /// enriquecimento reescreve arquivos versionados que ninguém pediu ao
    /// operador, o corte seguinte os lia como trabalho dele e recusava,
    /// mandando commitar ou guardar a saída do próprio Mustard.
    ///
    /// O par inteiro, pela porta REAL (`cut_pending_work_branch`): a decisão
    /// libera E a gravação acontece. Medir só a decisão foi como a metade
    /// anterior desta correção ficou verde enquanto o censo viajava para dentro
    /// da branch nova — as duas metades têm de ser medidas na mesma corrida.
    #[test]
    fn a_census_only_dirty_tree_does_not_refuse_the_cut() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        // Escrito ANTES do `git init` da fixture, para entrar no commit inicial:
        // um `mustard.json` solto seria trabalho do operador na árvore.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        // A árvore fica em `dev`, que é a base de onde `dev_second` sai.
        let model = repo_tracking_the_census(root);

        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");

        // A porta real, e só ela: nada disso é trabalho de ninguém, então o
        // corte acontece de verdade e a árvore volta limpa porque a gravação
        // mora dentro da mesma resposta que liberou o corte.
        //
        // A asserção que vinha antes desta media a metade DECISÃO por um
        // predicado à parte (`busy_checkout`). Esse predicado não existe mais:
        // decidir e gravar são o mesmo passo agora, e chamá-lo aqui gravaria o
        // censo e deixaria a asserção de árvore limpa abaixo trivialmente
        // verdadeira. A cobertura "as quatro portas respondem igual" mudou de
        // lugar, para `every_writer_answers_the_same_for_the_same_tree`.
        let sid = "sess-census-only-open";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(
            outcome,
            CutOutcome::Cut("dev_second".to_string()),
            "a abertura ordinária não é recusada: {outcome:?}",
        );
        assert_eq!(
            porcelain(root),
            "",
            "o portão gravou o que ele mesmo escreveu, sem commit manual no meio",
        );
    }

    /// O PAR que a ordem inversa quebrava: a base é ATUALIZADA a partir do
    /// `origin` ANTES de o commit do censo cair nela, e o censo é gravado assim
    /// mesmo. As duas metades, na mesma corrida.
    ///
    /// A regressão que isto tranca: a gravação do censo vinha primeiro e o
    /// avanço da base logo depois, com o resultado descartado. Um commit do
    /// censo na base local a faz divergir de `origin/{base}` — o passo é
    /// `merge --ff-only` —, o avanço é recusado, ninguém é avisado, e a
    /// unidade sai de uma base velha. É também a invariante que o Guard do
    /// `CLAUDE.md` da raiz enuncia: `--ff-only` só passa quando a base de
    /// integração não tem commit próprio; depois disso o
    /// `git pull --ff-only origin {base}` que a recusa deste portão prescreve
    /// falha também para o operador.
    ///
    /// O commit do `origin` é VAZIO de propósito: o avanço não depende da
    /// árvore suja, e o único motivo para ele falhar seria a ordem errada. O
    /// caso em que o commit do `origin` TOCA o censo sujo é medido à parte, em
    /// `a_census_in_the_way_of_the_advance_is_set_aside_not_committed_stale`.
    #[test]
    fn the_base_is_refreshed_before_the_census_commit_lands_on_it() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome};

        let tmp = tempfile::tempdir().unwrap();
        // A árvore e o `origin` vivem LADO A LADO: um repositório DENTRO da
        // árvore seria trabalho não versionado do operador, e o corte seria
        // recusado por isso em vez de medir o que este teste mede.
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let root_s = root.to_string_lossy().to_string();
        let origin = tmp.path().join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        let model = repo_tracking_the_census(root);

        // Um `origin` cuja `dev` está UM commit à frente da base local. O commit
        // é VAZIO de propósito: assim o fast-forward não depende da árvore suja,
        // e o único motivo para ele falhar é a divergência que a ordem errada
        // cria.
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev"]);
        git(root, &["commit", "-q", "--allow-empty", "-m", "origin moved"]);
        git(root, &["push", "-q", "origin", "dev"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a fixture tem de começar com a base ATRÁS do origin",
        );

        // A abertura ordinária do AC-7: a árvore suja só com o censo.
        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");

        let sid = "sess-stale-base";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(
            outcome,
            CutOutcome::Cut("dev_second".to_string()),
            "o corte tem de acontecer: {outcome:?}",
        );

        // Metade 1: a base avançou até o `origin`.
        assert!(
            git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base ficou velha: o commit do censo a fez divergir e o \
             `merge --ff-only` foi recusado em silêncio",
        );
        // Metade 2: e o censo foi gravado assim mesmo — nada sobrou para o
        // operador. Medir só uma das duas é como esta ordem entrou.
        assert_eq!(porcelain(root), "", "o censo não foi gravado");
    }

    /// A REGRESSÃO que este teste tranca, e o PAR que as quatro rodadas
    /// anteriores nunca mediram junto: fora da base, uma árvore suja só com o
    /// censo NÃO libera o corte.
    ///
    /// A decisão liberava `CensusOnly` em QUALQUER posição, dizendo no próprio
    /// comentário que "o portão base já grava esses arquivos antes do corte"; a
    /// gravação, corrigida à parte, passou a declinar fora da base. As duas
    /// metades verdes, o par quebrado: parado em `feature/outra-unidade` o corte
    /// era liberado, nada era gravado, e o `git checkout -b` levava
    /// `.claude/scan-map.md` e os moldes gerados para dentro da branch da unidade
    /// nova — pior do que o código que esta unidade substituiu, que ali RECUSAVA.
    ///
    /// Medido pela porta REAL, com as duas asserções que a quebra exige: o censo
    /// não viajou, e o operador foi informado do quê.
    #[test]
    fn an_off_base_census_refuses_the_cut_instead_of_riding_into_the_new_branch() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        let model = repo_tracking_the_census(root);
        // A posição do defeito: a branch de OUTRA unidade. Não é protegida, não
        // é a base do corte, e não é o alvo.
        git(root, &["checkout", "-b", "feature/outra-unidade"]);

        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let sid = "sess-census-off-base";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
        let dirty_before = porcelain(root);
        let outcome = cut_pending_work_branch(root, sid);

        let CutOutcome::Refused(busy) = outcome else {
            panic!("fora da base o censo não tem onde ser gravado, então o corte recusa: {outcome:?}");
        };
        assert_eq!(busy.current, "feature/outra-unidade");
        assert_eq!(busy.target, "dev_second");

        // 1. O censo NÃO viajou: nenhuma branch nova, nenhum commit, nada movido.
        assert!(
            git_out(root, &["rev-parse", "--verify", "dev_second"]).is_none(),
            "um corte recusado não cria branch — e é dentro dela que o censo entraria",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "e nada foi commitado na cabeça da outra unidade",
        );
        assert_eq!(porcelain(root), dirty_before, "a árvore fica exatamente como estava");

        // 2. E o operador foi informado do QUÊ: a frase nomeia as duas branches e
        //    os caminhos que estão no caminho do corte.
        let reason = busy.reason(mustard_core::platform::i18n::Locale::EnUs);
        assert!(
            reason.contains("feature/outra-unidade") && reason.contains("dev_second"),
            "a recusa nomeia de onde e para onde: {reason}",
        );
        assert!(
            reason.contains("scan-map.md"),
            "e NOMEIA o que precisa sair da frente, em vez de dizer que não pôde medir: {reason}",
        );
    }

    /// …e o corte que a decisão liberou GRAVA o censo antes de cortar, em vez de
    /// levá-lo embora dentro da branch da nova unidade.
    ///
    /// A regressão que este teste tranca: `CensusOnly` passava como limpo nas
    /// três portas, mas só a do portão base gravava o censo antes. Nas outras
    /// duas o `git checkout -b` carregava `.claude/scan-map.md` e os moldes
    /// gerados para dentro da branch da unidade, onde eles entram no diff dela e
    /// no pull request dela — a atribuição que o assunto de commit do censo
    /// existe para evitar. A porta medida aqui é a do `spec-draft`
    /// (`cut_pending_work_branch`); a do hook toma a MESMA chamada.
    #[test]
    fn the_cut_records_the_census_on_the_base_before_taking_the_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        // Escrito ANTES do `git init` da fixture, para entrar no commit inicial
        // dela: um `mustard.json` solto seria trabalho do operador na árvore e a
        // recusa de hoje — correta — abortaria o corte antes da medição.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        // A árvore fica na BASE, que é onde o portão a encontra: é dela que a
        // próxima unidade é cortada, e é nela que o censo tem de aterrissar.
        let model = repo_tracking_the_census(root);

        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
        let base_head = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let sid = "sess-cut-records-census";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = crate::commands::event::work_branch::cut_pending_work_branch(root, sid);
        assert_eq!(
            outcome,
            crate::commands::event::work_branch::CutOutcome::Cut("dev_second".to_string()),
            "a árvore só com censo não recusa o corte: {outcome:?}",
        );

        let status = porcelain(root);
        for artefact in ["scan-map.md", "grain.model.json", "grain.dictionary.json"] {
            assert!(
                !status.contains(artefact),
                "o censo não viajou sujo para dentro da branch nova ({artefact}): {status}",
            );
        }
        // O commit do censo ficou na BASE de onde o corte saiu, com o assunto do
        // censo. `dev_second` foi cortada depois, então o herda como ancestral e
        // o diff da unidade contra a base dela não carrega o censo.
        let census_commit = git_out(root, &["rev-parse", "dev"]).expect("dev");
        assert_ne!(census_commit, base_head, "o portão gravou um commit do censo");
        let subject = git_out(root, &["log", "-1", "--format=%s", "dev"]).unwrap_or_default();
        assert_eq!(
            subject.trim(),
            CENSUS_COMMIT_SUBJECT,
            "e o assunto é o do censo, não o da unidade",
        );
        let carried =
            git_out(root, &["diff", "--name-only", "dev", "dev_second"]).unwrap_or_default();
        assert_eq!(carried.trim(), "", "e a branch nova nasce sem nada do censo no diff dela");
    }

    /// A REGRESSÃO que este teste tranca: um corte RECUSADO por base
    /// desconhecida não pode deixar para trás o commit do censo de um corte que
    /// nunca aconteceu.
    ///
    /// A gravação morava dentro da decisão de "checkout ocupado", que roda ANTES
    /// da resolução da base. Com vários candidatos declarados e nada dizendo de
    /// qual base a emergência saiu, o corte devolve `BaseUnknown` e não toca no
    /// git — mas o censo já tinha sido commitado.
    #[test]
    fn a_cut_denied_for_an_unknown_base_leaves_no_census_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        // Dois candidatos declarados e nenhum registro: `hotfix/…` não tem base
        // derivável, então a resolução responde `Ambiguous`.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        let model = repo_tracking_the_census(root);

        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let sid = "sess-base-unknown";
        crate::shared::context::set_pending_branch(&root_s, sid, "hotfix/urgente", None);
        // Amostrado DEPOIS do marcador, que também escreve na árvore: o que
        // este teste mede é o que o corte faz, não o que o marcador fez.
        let dirty_before = porcelain(root);
        let outcome = crate::commands::event::work_branch::cut_pending_work_branch(root, sid);
        assert!(
            matches!(
                outcome,
                crate::commands::event::work_branch::CutOutcome::BaseUnknown { .. }
            ),
            "a base não foi estabelecida, então nada é cortado: {outcome:?}",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "e nenhum commit do censo fica para trás de um corte que não houve",
        );
        assert_eq!(
            porcelain(root),
            dirty_before,
            "a árvore fica exatamente como estava, para o corte que vier de fato",
        );
        assert!(
            git_out(root, &["rev-parse", "--verify", "hotfix/urgente"]).is_none(),
            "e nenhuma branch foi criada — é esta metade que autoriza a resposta \
             compartilhada a não falar do censo quando a base não é um fato",
        );
    }

    /// …e a outra metade: numa posição PROTEGIDA o corte não grava nada. A
    /// árvore é medida (uma vez, como sempre), e a resposta é que o commit do
    /// censo não pertence ali: um hook não cria commit numa base protegida atrás
    /// do operador. É a ÚNICA divergência legítima entre as portas, e ela é um
    /// insumo nomeado da decisão ([`CensusDoor`]) — a porta explícita do
    /// `emit-pipeline`, onde o operador digitou o comando, continua gravando lá.
    #[test]
    fn the_cut_does_not_commit_the_census_onto_a_protected_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // `git.protected` nomeia a branch em que a árvore está parada.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev"},"protected":["dev"]}}"#,
        )
        .unwrap();
        let model = repo_tracking_the_census(root);

        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let config = ProjectConfig::load(root);
        assert!(
            crate::commands::event::work_branch::is_protected(root, "dev", &config),
            "a fixture precisa de uma posição realmente protegida",
        );
        let dirty_before = porcelain(root);
        let settled = settle_cut(root, Some("dev"), "dev_second", Some("dev"), &config);
        assert!(
            matches!(settled, CensusSettlement::Refuse(_)),
            "não grava E não libera: o censo não tem como aterrissar aqui por esta porta: {settled:?}",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "nada é commitado numa base protegida pelo caminho do corte",
        );
        assert_eq!(porcelain(root), dirty_before, "e a árvore fica exatamente como estava");

        // E a posição NÃO MEDIDA (`HEAD` destacado, ou ilegível) idem — e
        // também recusa, pelo mesmo motivo: o censo não pode viajar de onde
        // não pode ser gravado.
        for current in [Some("HEAD"), None] {
            let settled = settle_cut(root, current, "dev_second", Some("dev"), &config);
            assert!(
                matches!(settled, CensusSettlement::Refuse(_)),
                "posição {current:?}: o censo não viaja de uma posição não medida: {settled:?}",
            );
        }
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "uma posição que não foi medida não autoriza commit nenhum",
        );
    }

    /// A LINHA que faltava na tabela: base PROTEGIDA, árvore suja só com o
    /// censo, e uma porta que não pode gravar ali (o corte, o hook).
    ///
    /// Era o buraco entre duas linhas certas. `holds_other_work` isenta a
    /// posição protegida (o trabalho do operador viajar da base para a primeira
    /// unidade é de propósito), e `may_record_on_a_protected_base` nega a
    /// gravação ao hook — então o censo não era recusado E não era gravado, e o
    /// `git checkout -b` o levava para dentro da branch da unidade nova. Num
    /// projeto de branch única (`flow *: main`) é o caso ORDINÁRIO, não a
    /// exceção: `main` é protegida, e toda passagem de enriquecimento deixava
    /// o censo pronto para viajar.
    ///
    /// As três metades na mesma corrida: a porta de corte RECUSA nomeando os
    /// caminhos e a porta que consegue; a árvore fica como estava; e a porta
    /// explícita, na mesma árvore, GRAVA — que é o que a recusa manda fazer.
    #[test]
    fn a_census_on_a_protected_base_is_refused_at_the_cut_and_recorded_at_the_open() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome, RefusalCause};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        // Branch única: `main` é a base de tudo E é protegida.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"main"},"protected":["main"]}}"#,
        )
        .unwrap();
        init_repo_on(root, "main");
        let model = default_model_path(root);
        std::fs::create_dir_all(model.parent().expect("model parent")).unwrap();
        std::fs::write(&model, "{\"projects\":[]}\n").unwrap();
        std::fs::write(model.with_file_name(GRAIN_DICTIONARY), "{\"terms\":[]}\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "track the census"]);
        let config = ProjectConfig::load(root);
        assert!(
            crate::commands::event::work_branch::is_protected(root, "main", &config),
            "a fixture precisa de uma base realmente protegida",
        );

        // O censo sujo DEPOIS da abertura explícita: a passagem de enriquecimento.
        remine(&model);
        leftover_enrichment(root);
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "precondição: só o censo está sujo",
        );

        // 1. A porta REAL de corte recusa — e diz por quê e o que fazer.
        let sid = "sess-census-protected-base";
        crate::shared::context::set_pending_branch(&root_s, sid, "feature/segunda", None);
        // Amostrado DEPOIS do marcador, que também escreve na árvore.
        let dirty_before = porcelain(root);
        let outcome = cut_pending_work_branch(root, sid);
        let CutOutcome::Refused(busy) = outcome else {
            panic!("o censo não pode viajar para dentro da unidade nova: {outcome:?}");
        };
        assert_eq!(busy.cause, RefusalCause::CensusOnProtectedBase);
        let reason = busy.reason(mustard_core::platform::i18n::Locale::EnUs);
        assert!(
            reason.contains("scan-map.md") && reason.contains("grain.model.json"),
            "a recusa NOMEIA os caminhos do censo: {reason}",
        );
        assert!(
            reason.contains("emit-pipeline"),
            "e nomeia a porta que consegue gravá-los ali: {reason}",
        );
        assert!(
            git_out(root, &["rev-parse", "--verify", "feature/segunda"]).is_none(),
            "nenhuma branch foi criada — é dentro dela que o censo entraria",
        );
        assert_eq!(git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"), head_before);
        assert_eq!(porcelain(root), dirty_before, "a árvore fica exatamente como estava");

        // 2. E a porta EXPLÍCITA, na mesma árvore, grava: é a saída que a
        //    recusa apontou.
        assert!(
            matches!(
                settle_open(root, Some("main"), Some("main"), &config),
                CensusSettlement::Recorded(_)
            ),
            "a porta explícita grava numa base protegida",
        );
        // Lido pela classificação do PRÓPRIO produto: o marcador pendente
        // (`.claude/.session/`) é rascunho do harness, e não entra em commit.
        assert_eq!(
            checkout_work(root),
            CheckoutWork::ProvenClean,
            "e nada do censo sobra sujo",
        );
        assert_eq!(
            git_out(root, &["log", "-1", "--format=%s"]).unwrap_or_default().trim(),
            CENSUS_COMMIT_SUBJECT,
        );
        // …depois do que o corte passa.
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(outcome, CutOutcome::Cut("feature/segunda".to_string()));
    }

    /// Monta a árvore e um `origin` LADO A LADO, com a `dev` local UM commit
    /// atrás do `origin` — e o commit do `origin` TOCANDO o modelo do censo,
    /// que é o caso que o commit vazio da fixture irmã contorna. Devolve o
    /// commit à frente e o conteúdo que o `origin` tem para o modelo.
    fn origin_ahead_touching_the_census(root: &Path) -> (String, &'static str) {
        let origin = root.parent().expect("tmp").join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        let model = repo_tracking_the_census(root);
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev"]);
        // A máquina A re-minerou e publicou.
        const ORIGINS_CENSUS: &str = "{\"projects\":[{\"dir\":\"apps/rt\"},{\"dir\":\"apps/cli\"}]}\n";
        std::fs::write(&model, ORIGINS_CENSUS).unwrap();
        git(root, &["commit", "-q", "-am", "chore: refresh the deterministic project census"]);
        git(root, &["push", "-q", "origin", "dev"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        // A máquina B ainda não puxou.
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a fixture tem de começar com a base ATRÁS do origin",
        );
        (ahead, ORIGINS_CENSUS)
    }

    /// A REGRESSÃO que este teste tranca: o `merge --ff-only` corria com o
    /// censo sujo, falhava em "local changes would be overwritten", ninguém
    /// lia o resultado, e a gravação commitava o censo na `dev` local VELHA —
    /// que passava a ter commit próprio E a estar atrás. A unidade saía de uma
    /// base velha e o `git pull --ff-only origin dev` que o portão prescreve em
    /// seguida não tinha mais como passar.
    ///
    /// O censo é saída regenerável da ferramenta: o que está no caminho do
    /// avanço é posto de lado, a base avança, e o que sobrou do censo é gravado
    /// em cima da base NOVA. As metades, na mesma corrida: a base avançou; o
    /// modelo é o do `origin`, não o local velho; e nada sobrou sujo.
    #[test]
    fn a_census_in_the_way_of_the_advance_is_set_aside_not_committed_stale() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let root_s = root.to_string_lossy().to_string();
        let (ahead, origins_census) = origin_ahead_touching_the_census(root);

        // A máquina B com o censo sujo — o modelo INCLUSIVE, que é o arquivo
        // que o avanço sobrescreve.
        remine(&model_of(root));
        leftover_enrichment(root);
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "precondição: só o censo está sujo",
        );

        let sid = "sess-census-in-the-way";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(outcome, CutOutcome::Cut("dev_second".to_string()), "{outcome:?}");

        assert!(
            git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base avançou até o origin: o censo no caminho não a prendeu",
        );
        assert_eq!(
            std::fs::read_to_string(model_of(root)).unwrap(),
            origins_census,
            "o modelo é o do origin — o local velho foi posto de lado, não gravado por cima",
        );
        assert_eq!(porcelain(root), "", "e o resto do censo foi gravado, nada sobrou sujo");
        // A `dev` não divergiu: o origin é ancestral dela, então o próximo
        // `git pull --ff-only origin dev` continua passando.
        assert!(
            git_out(root, &["merge-base", "--is-ancestor", "origin/dev", "dev"]).is_some(),
            "a base local contém o origin — nenhum commit foi escrito numa base velha",
        );
    }

    /// …e quando o avanço NÃO tem como passar — a base local divergiu —, a
    /// resposta é RECUSAR, alto, com as palavras do git: nunca engolir e nunca
    /// gravar numa base velha. E a recusa vem ANTES de qualquer ação: nada
    /// posto de lado, nada commitado, a árvore como estava.
    #[test]
    fn a_base_that_cannot_advance_refuses_loudly_instead_of_cutting_stale() {
        use crate::commands::event::work_branch::RefusalCause;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let (ahead, _) = origin_ahead_touching_the_census(root);
        // A `dev` local com um commit PRÓPRIO: divergiu do origin.
        git(root, &["commit", "-q", "--allow-empty", "-m", "a commit of its own"]);

        remine(&model_of(root));
        leftover_enrichment(root);
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        let dirty_before = porcelain(root);

        let settled = settle_cut(root, Some("dev"), "dev_second", Some("dev"), &flow_config());
        let CensusSettlement::Refuse(busy) = settled else {
            panic!("uma base que não avança não recebe corte nem commit: {settled:?}");
        };
        let RefusalCause::BaseStale { base, error } = &busy.cause else {
            panic!("a causa é a base, não a árvore: {:?}", busy.cause);
        };
        assert_eq!(base, "dev");
        assert!(!error.is_empty(), "as palavras do git viajam na recusa");
        let reason = busy.reason(mustard_core::platform::i18n::Locale::EnUs);
        assert!(reason.contains("origin/dev"), "a frase nomeia o remoto: {reason}");

        assert_eq!(git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"), head_before);
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base não foi rebobinada nem mesclada",
        );
        assert_eq!(porcelain(root), dirty_before, "nada foi posto de lado antes de recusar");
    }

    /// Uma gravação que o git RECUSA (aqui, um `pre-commit` que nega) responde
    /// `Proceed`, não `Recorded`, e deixa o censo onde caiu — e diz isso no
    /// stderr, do catálogo (`basegate.census.unavailable`), em vez de calar:
    /// rode com `--nocapture` para ver a linha. O corte seguinte vai nomear
    /// esses caminhos, e sem a linha o operador não teria aviso prévio de que
    /// foi a ferramenta que os deixou ali.
    #[test]
    fn a_declined_recording_proceeds_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = repo_tracking_the_census(root);
        remine(&model);
        let hooks = root.join(".git").join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        assert_eq!(
            settle_open(root, Some("dev"), Some("dev"), &flow_config()),
            CensusSettlement::Proceed,
            "o git recusou o commit: a resposta é seguir, não fingir que gravou",
        );
        assert_eq!(git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"), head_before);
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "o censo fica onde caiu, e o índice volta ao que era",
        );
    }

    /// A porta EXPLÍCITA só move a base sobre a qual abre — e nenhuma outra.
    ///
    /// Antes do colapso, `BaseVerdict::Open` refrescava o censo e não movia ref
    /// nenhuma. Depois, o passo 3 avançava TODA base pré-selecionada do fluxo
    /// (`fetch origin main:main`, `release/*`…), atrás do operador. Mover outras
    /// refs locais nunca foi trabalho desta decisão.
    #[test]
    fn the_explicit_open_advances_the_base_it_opens_on_and_no_other_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let origin = tmp.path().join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        repo_tracking_the_census(root);
        // `main` local, parada no commit inicial.
        git(root, &["branch", "main"]);
        let main_before = git_out(root, &["rev-parse", "main"]).expect("main");
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev", "main"]);
        // O origin avança AS DUAS; a local só a `dev` vai puxar.
        git(root, &["commit", "-q", "--allow-empty", "-m", "moved"]);
        git(root, &["push", "-q", "origin", "dev", "dev:main"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);

        let config = ProjectConfig::load(root);
        assert!(
            config.git.preselected_bases().contains("main"),
            "a fixture precisa de uma base pré-selecionada que NÃO é a desta abertura",
        );
        let _ = settle_open(root, Some("dev"), Some("dev"), &config);
        assert!(
            git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base desta abertura avançou",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "main"]).expect("main"),
            main_before,
            "e a `main` local, que o operador não mencionou, não se mexeu",
        );
    }

    /// A REGRESSÃO que este teste tranca, e a terceira iteração da MESMA
    /// família: o commit do censo pertence à BASE e a mais nada.
    ///
    /// As exclusões anteriores — protegida, `HEAD`, não medida — não nomeiam a
    /// posição que faltava: uma OUTRA branch de unidade. Ela não é protegida,
    /// não é a base e não é o alvo, então passava por todas as checagens, e com
    /// só o censo sujo o `git commit` caía na cabeça dela — o censo entrava no
    /// diff e no pull request daquela unidade, que é exatamente a
    /// mis-atribuição que `CENSUS_COMMIT_SUBJECT` existe para evitar.
    #[test]
    fn the_census_is_not_committed_onto_another_units_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        let model = repo_tracking_the_census(root);
        // A posição do defeito: a branch de OUTRA unidade. Não é protegida, não
        // é a base do corte, e não é o alvo.
        git(root, &["checkout", "-b", "feature/outra-unidade"]);

        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        let dirty_before = porcelain(root);

        let config = ProjectConfig::load(root);
        assert!(
            !crate::commands::event::work_branch::is_protected(
                root,
                "feature/outra-unidade",
                &config
            ),
            "a fixture precisa de uma posição NÃO protegida, senão mede a exclusão antiga",
        );
        settle_cut(root, Some("feature/outra-unidade"), "dev_second", Some("dev"), &config);
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "o censo não é commitado dentro da branch de outra unidade",
        );
        assert_eq!(
            porcelain(root),
            dirty_before,
            "e a árvore fica como estava, para o corte que sair mesmo da base",
        );

        // …e a outra metade, que não pode ser apertada junto: PARADO NA BASE, o
        // corte ordinário continua gravando e deixando a árvore limpa (AC-7).
        git(root, &["checkout", "dev"]);
        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a fixture precisa da árvore suja de novo");
        settle_cut(root, Some("dev"), "dev_second", Some("dev"), &config);
        assert_eq!(
            porcelain(root),
            "",
            "parado na base, o portão grava o que ele mesmo escreveu",
        );
    }

    /// A REGRESSÃO que este teste tranca, na PORTA AO LADO: o `emit-pipeline`
    /// gravava o censo em qualquer posição.
    ///
    /// `evaluate` devolve `Open(current)` para QUALQUER nome de branch — a
    /// checagem de pertencimento foi removida de propósito —, então a porta que
    /// ABRE a unidade commitava `.claude/scan-map.md` e os moldes gerados na
    /// cabeça de `feature/outra-unidade`, sob o assunto do censo: exatamente a
    /// mis-atribuição que a porta de CORTE já recusava. Uma condição posicional
    /// só, lida pelas duas ([`crate::commands::event::census_settlement`]).
    ///
    /// As duas metades na mesma corrida: fora da base a cabeça não se mexe, e
    /// PARADO na base a gravação continua acontecendo.
    #[test]
    fn the_open_door_records_the_census_only_where_it_belongs() {
        use crate::commands::event::emit_pipeline::enforce_base_gate_at;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        let model = repo_tracking_the_census(root);
        // A posição do defeito: a branch de OUTRA unidade.
        git(root, &["checkout", "-b", "feature/outra-unidade"]);

        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        let dirty_before = porcelain(root);

        // A porta real, com a base que esta abertura cortaria (`dev`).
        let _ = enforce_base_gate_at(root, None, Some("dev"));
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "a cabeça da outra unidade não recebe o commit do censo",
        );
        assert_eq!(
            porcelain(root),
            dirty_before,
            "e nada foi varrido para dentro de um commit dela",
        );

        // A outra metade, que não pode ser apertada junto: PARADO na base, a
        // porta explícita grava e a árvore volta limpa.
        git(root, &["checkout", "dev"]);
        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a fixture precisa da árvore suja de novo");
        let _ = enforce_base_gate_at(root, None, Some("dev"));
        assert_eq!(
            porcelain(root),
            "",
            "parado na base, a porta que abre a unidade grava o que a ferramenta escreveu",
        );
    }

    /// A árvore que TODAS as escritoras recebem neste módulo: o censo
    /// re-minerado e a saída da passagem de enriquecimento, e mais nada do
    /// operador. `stand_on` põe a árvore fora da base quando é `Some`.
    fn a_tree_dirty_only_with_the_census(root: &Path, stand_on: Option<&str>) {
        // Escrito ANTES do `git init` da fixture, para entrar no commit inicial:
        // um `mustard.json` solto seria trabalho do operador na árvore.
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        let model = repo_tracking_the_census(root);
        if let Some(branch) = stand_on {
            git(root, &["checkout", "-b", branch]);
        }
        remine(&model);
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a passagem de enriquecimento sujou a árvore");
    }

    /// O que uma escritora deixou observável: o commit do censo caiu na BASE, e
    /// o censo saiu da frente do próximo corte?
    ///
    /// As DUAS metades juntas, sempre. Foi medindo uma de cada vez que quatro
    /// rodadas seguidas ficaram verdes com o par quebrado: a decisão liberava e
    /// a gravação declinava, cada metade correta sozinha.
    #[derive(Debug, PartialEq, Eq)]
    struct CensusAnswerSeen {
        base_carries_the_census_commit: bool,
        census_still_in_the_way: bool,
    }

    fn what_the_writer_answered(root: &Path) -> CensusAnswerSeen {
        let subject = git_out(root, &["log", "-1", "--format=%s", "dev"]).unwrap_or_default();
        CensusAnswerSeen {
            base_carries_the_census_commit: subject.trim() == CENSUS_COMMIT_SUBJECT,
            // Lido pela classificação do PRÓPRIO produto, e não por um
            // `git status --porcelain` cru: aquele COLAPSA um diretório
            // inteiramente não rastreado numa linha só, então procurar
            // "scan-map.md" nele responde sobre o formato da saída do git em vez
            // de sobre o censo.
            census_still_in_the_way: matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
        }
    }

    /// A entrada do hook de escrita, montada aqui porque este é o único teste
    /// que precisa das QUATRO escritoras lado a lado.
    fn write_hook_verdict(root: &Path, sid: &str) {
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger};
        let root_s = root.to_string_lossy().to_string();
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: serde_json::json!({ "file_path": "f.txt", "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(root_s.clone()),
            session_id: Some(sid.to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(root_s, Some(Trigger::PreToolUse));
        let _ = crate::hooks::write::work_branch_gate::WorkBranchGate.evaluate(&input, &ctx);
    }

    /// A PROVA DO COLAPSO — e o teste que nenhuma rodada anterior podia ter
    /// escrito.
    ///
    /// As QUATRO escritoras da decisão do censo recebem a MESMA árvore na MESMA
    /// posição, e respondem a mesma coisa. Eram quatro condições, escritas em
    /// três arquivos, e cada uma das seis rodadas de revisão consertou as que
    /// enxergava: as três primeiras nunca souberam da gravação do próprio mine,
    /// e a sétima corrigiu a ORDEM em duas portas de três. Nenhuma delas tinha
    /// uma pergunta única para fazer às quatro — e por isso nenhuma delas podia
    /// medir isto.
    ///
    /// É este teste que torna impossível a próxima chamadora esquecida: uma
    /// escritora nova que não passe pela resposta compartilhada diverge das
    /// outras três aqui, na linha que compara as quatro respostas entre si.
    #[test]
    fn every_writer_answers_the_same_for_the_same_tree() {
        // As quatro, nomeadas como o relatório de revisão as nomeou. A segunda é
        // o caminho que a gravação do PRÓPRIO mine ocupava: a mesma porta
        // explícita, com o censo VENCIDO (o modelo apagado é a forma mais clara
        // de "mais velho que a árvore"), que era exatamente quando aquele
        // escritor não-guardado disparava.
        type Writer = fn(&Path, &str);
        let writers: [(&str, Writer, bool); 4] = [
            (
                "emit-pipeline (a porta explícita)",
                |root, _sid| {
                    let _ = crate::commands::event::emit_pipeline::enforce_base_gate_at(
                        root,
                        None,
                        Some("dev"),
                    );
                },
                false,
            ),
            (
                "emit-pipeline (o caminho da gravação do próprio mine)",
                |root, _sid| {
                    let _ = crate::commands::event::emit_pipeline::enforce_base_gate_at(
                        root,
                        None,
                        Some("dev"),
                    );
                },
                true,
            ),
            (
                "spec-draft (o corte da branch)",
                |root, sid| {
                    let root_s = root.to_string_lossy().to_string();
                    crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
                    let _ =
                        crate::commands::event::work_branch::cut_pending_work_branch(root, sid);
                },
                false,
            ),
            (
                "o hook de escrita",
                |root, sid| {
                    let root_s = root.to_string_lossy().to_string();
                    crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
                    write_hook_verdict(root, sid);
                },
                false,
            ),
        ];

        // Duas posições, e a divergência histórica mora na segunda.
        for (position, stand_on, expected) in [
            (
                "parado NA base",
                None,
                CensusAnswerSeen {
                    base_carries_the_census_commit: true,
                    census_still_in_the_way: false,
                },
            ),
            (
                "parado na branch de OUTRA unidade",
                Some("feature/outra-unidade"),
                CensusAnswerSeen {
                    base_carries_the_census_commit: false,
                    census_still_in_the_way: true,
                },
            ),
        ] {
            let mut answers: Vec<(&str, CensusAnswerSeen)> = Vec::with_capacity(writers.len());
            for (name, writer, stale_census) in writers {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path();
                a_tree_dirty_only_with_the_census(root, stand_on);
                if stale_census {
                    std::fs::remove_file(default_model_path(root)).unwrap();
                }
                writer(root, "sess-collapse");
                answers.push((name, what_the_writer_answered(root)));
            }
            for (name, answer) in &answers {
                assert_eq!(
                    answer, &expected,
                    "{position}: '{name}' respondeu diferente do contrato — as quatro \
                     escritoras leem a MESMA resposta ou não colapsaram",
                );
            }
            // E entre si, explicitamente: é a divergência ENTRE portas que as
            // seis rodadas produziram, não o desvio de uma porta do contrato.
            let (first_name, first) = &answers[0];
            for (name, answer) in &answers[1..] {
                assert_eq!(
                    answer, first,
                    "{position}: '{name}' e '{first_name}' discordam sobre a mesma árvore",
                );
            }
        }
    }

    /// UMA porta, UMA varredura da árvore.
    ///
    /// `checkout_work` roda um `git status --porcelain --untracked-files=all` do
    /// repositório inteiro e abre cada `SKILL.md` sujo. Antes do colapso ele
    /// rodava DUAS vezes por porta — a decisão de recusa media, e a gravação
    /// media de novo, cada uma com sua ideia do que "sujo" queria dizer — e três
    /// vezes numa abertura de pipeline com o corte que a segue.
    ///
    /// O contador é por THREAD, e o harness do cargo dá uma thread a cada teste,
    /// então a contagem de um vizinho rodando em paralelo não vaza para cá.
    #[test]
    fn each_door_walks_the_tree_exactly_once() {
        use crate::commands::event::work_branch::TREE_PROBES;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        a_tree_dirty_only_with_the_census(root, None);

        TREE_PROBES.with(|n| n.set(0));
        let _ = crate::commands::event::emit_pipeline::enforce_base_gate_at(root, None, Some("dev"));
        assert_eq!(
            TREE_PROBES.with(|n| n.get()),
            1,
            "a porta explícita mede a árvore uma vez: o mine e a gravação leem a MESMA medição",
        );

        // …e com o censo VENCIDO, que é quando a gravação do próprio mine
        // disparava: ali eram DUAS varreduras, uma para autorizar o mine e outra
        // para achar o que gravar.
        remine(&model_of(root));
        leftover_enrichment(root);
        std::fs::remove_file(default_model_path(root)).unwrap();
        TREE_PROBES.with(|n| n.set(0));
        let _ = crate::commands::event::emit_pipeline::enforce_base_gate_at(root, None, Some("dev"));
        assert_eq!(
            TREE_PROBES.with(|n| n.get()),
            1,
            "o censo vencido não compra uma segunda varredura: o mine lê a medição da decisão",
        );

        TREE_PROBES.with(|n| n.set(0));
        crate::shared::context::set_pending_branch(&root_s, "sess-one-probe", "dev_second", None);
        let _ = crate::commands::event::work_branch::cut_pending_work_branch(root, "sess-one-probe");
        assert_eq!(
            TREE_PROBES.with(|n| n.get()),
            1,
            "e a porta de corte também: a recusa e a gravação são a mesma resposta",
        );
    }

    /// Um molde ADOTADO (`source: manual`) é escrita do OPERADOR, e o caminho
    /// dele é igualzinho ao de um molde gerado — o frontmatter é o que separa.
    ///
    /// Lê-lo como censo faz o corte parar de recusar por causa da edição à mão
    /// de alguém e a gravação varrê-la para dentro de um commit da ferramenta,
    /// que é exatamente a troca que a categoria existe para impedir.
    ///
    /// As duas metades numa resposta só, que é a forma nova: a recusa NOMEIA o
    /// molde adotado, e uma recusa não grava nada por construção — ela devolve
    /// antes de qualquer fetch, mine ou commit.
    #[test]
    fn an_adopted_mold_is_the_operators_writing_not_the_census() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = repo_tracking_the_census(root);
        git(root, &["checkout", "-b", "dev_first"]);

        remine(&model);
        leftover_enrichment(root);
        // O molde curado, adotado: a partir do `source: manual` quem escreve
        // ali é o operador, e o próprio molde documenta isso.
        let adopted = root
            .join("apps")
            .join("rt")
            .join(".claude")
            .join("skills")
            .join("rt-verdict-pattern");
        std::fs::create_dir_all(&adopted).unwrap();
        std::fs::write(
            adopted.join("SKILL.md"),
            "---\nname: rt-verdict-pattern\nsource: manual\n---\n\n## Purpose\n",
        )
        .unwrap();
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let CensusSettlement::Refuse(busy) =
            settle_cut(root, Some("dev_first"), "dev_second", Some("dev"), &flow_config())
        else {
            panic!("a edição à mão do operador recusa o corte");
        };
        let CheckoutWork::Holds { theirs: dirty, .. } = &busy.work else {
            panic!("os caminhos foram observados, veio {:?}", busy.work);
        };
        assert_eq!(
            dirty,
            &vec!["apps/rt/.claude/skills/rt-verdict-pattern/SKILL.md".to_string()],
            "a recusa nomeia o molde adotado e só ele: {dirty:?}",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "e o portão não varre a escrita do operador para um commit dele",
        );
    }

    /// A REGRESSÃO que este teste tranca: o mine se considerava impedido pela
    /// saída da PRÓPRIA ferramenta, e era isso que produzia dois commits de
    /// mesmo título.
    ///
    /// Com o censo contando como sujeira, o portão tinha de gravar ANTES do mine
    /// só para se desimpedir — gravando o modelo VELHO sob o assunto do censo —
    /// e o mine em seguida gravava o novo sob o MESMO assunto. Descontada a
    /// saída da ferramenta, o mine roda primeiro e a gravação acontece uma vez
    /// só, no fim.
    #[test]
    fn a_census_only_dirty_tree_does_not_block_the_mine() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = repo_tracking_the_census(root);
        // O censo precisa estar VENCIDO para a pergunta ter conteúdo: o modelo
        // é apagado, que é a forma mais clara de "mais velho que a árvore".
        std::fs::remove_file(&model).unwrap();
        assert!(census_refresh_due(root, &model, &checkout_work(root)), "precondição: o mine está vencido");

        // A saída da passagem de enriquecimento, e só ela.
        leftover_enrichment(root);
        assert_ne!(porcelain(root), "", "a árvore está suja — só de censo");
        assert!(
            census_refresh_due(root, &model, &checkout_work(root)),
            "a ferramenta não pode se impedir com a própria saída: enquanto isso for \
             `false`, o portão precisa gravar antes do mine e sobram dois commits de \
             mesmo título",
        );

        // A outra metade: UMA linha do operador junto e o mine volta a ser
        // impedido — o desconto é do censo, não da sujeira em geral.
        std::fs::write(root.join("theirs.txt"), "mine, not yours\n").unwrap();
        assert!(
            !census_refresh_due(root, &model, &checkout_work(root)),
            "com trabalho do operador na árvore o mine continua impedido",
        );
    }

    /// Uma RECUSA não deixa nada para trás — nem um fetch, nem um avanço da
    /// base, nem um commit.
    ///
    /// A resposta `Refuse` promete isso na própria documentação dela, e uma
    /// promessa sobre o que outra parte do código faz é exatamente o tipo de
    /// comentário que este trabalho encontrou desatualizado em três arquivos. A
    /// ordem que a sustenta — recusar ANTES de agir — não tem como ser lida do
    /// resultado: sem o `origin` adiantado desta fixture, agir primeiro e
    /// recusar depois passa despercebido.
    #[test]
    fn a_refusal_leaves_the_repository_exactly_as_it_found_it() {
        let tmp = tempfile::tempdir().unwrap();
        // Árvore e `origin` LADO A LADO: um repositório DENTRO da árvore seria
        // trabalho não versionado do operador.
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let origin = tmp.path().join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        a_tree_dirty_only_with_the_census(root, None);

        // Um `origin` cuja `dev` está um commit VAZIO à frente da base local.
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev"]);
        git(root, &["commit", "-q", "--allow-empty", "-m", "origin moved"]);
        git(root, &["push", "-q", "origin", "dev"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        // …e a árvore parada na branch de OUTRA unidade, que é onde o censo não
        // tem para onde ir e o corte é recusado.
        git(root, &["checkout", "-q", "-b", "feature/outra-unidade"]);
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let settled = settle_cut(
            root,
            Some("feature/outra-unidade"),
            "dev_second",
            Some("dev"),
            &flow_config(),
        );
        assert!(
            matches!(settled, CensusSettlement::Refuse(_)),
            "a precondição é a recusa: {settled:?}",
        );
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base NÃO foi avançada: quem recusa não age antes de recusar",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "e nada foi commitado onde a árvore estava parada",
        );

        // A linha NA BASE da mesma promessa: o censo já foi posto de lado para
        // o avanço, e o avanço falha mesmo assim — aqui, num rascunho do
        // harness que o `origin` passou a versionar (rascunho não entra na
        // medição, então nada o pôs de lado). A recusa devolve o censo exatamente
        // como estava, ÍNDICE incluído, e não deixa entrada nenhuma no stash.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let (ahead, _) = origin_ahead_touching_the_census(root);
        // O `origin` também passou a versionar um rascunho do harness (em cima
        // do commit à frente, e a máquina B volta dois)…
        git(root, &["reset", "-q", "--hard", &ahead]);
        std::fs::write(root.join(".claude").join("feature-digest.json"), "{}\n").unwrap();
        git(root, &["add", "-f", ".claude/feature-digest.json"]);
        git(root, &["commit", "-q", "-m", "a scratch file, versioned by mistake"]);
        git(root, &["push", "-q", "origin", "dev"]);
        git(root, &["reset", "-q", "--hard", "HEAD~2"]);
        // …que nesta máquina existe, não rastreado, e vai barrar o avanço.
        std::fs::write(root.join(".claude").join("feature-digest.json"), "{\"local\":1}\n")
            .unwrap();
        remine(&model_of(root));
        leftover_enrichment(root);
        // O modelo ENCENADO no índice: o estado que o descarte antigo não via.
        git(root, &["add", ".claude/grain.model.json"]);
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "precondição: só o censo (e um rascunho) está sujo",
        );
        let ours = std::fs::read_to_string(model_of(root)).unwrap();
        let dirty_before = porcelain(root);
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let settled = settle_cut(root, Some("dev"), "dev_second", Some("dev"), &flow_config());
        let CensusSettlement::Refuse(busy) = settled else {
            panic!("o avanço barrado pelo rascunho recusa: {settled:?}");
        };
        assert!(
            matches!(busy.cause, crate::commands::event::work_branch::RefusalCause::BaseStale { .. }),
            "a causa é a base: {:?}",
            busy.cause
        );
        assert!(
            !git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base não avançou",
        );
        assert_eq!(git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"), head_before);
        assert_eq!(
            porcelain(root),
            dirty_before,
            "o censo posto de lado voltou exatamente como estava — o modelo encenado inclusive",
        );
        assert_eq!(
            std::fs::read_to_string(model_of(root)).unwrap(),
            ours,
            "e com o conteúdo local, não o do origin",
        );
        assert!(
            git_out(root, &["rev-parse", "--verify", "--quiet", "refs/stash"]).is_none(),
            "nenhuma entrada de stash ficou para trás",
        );
    }

    /// Um censo ENCENADO no índice (`M `) é posto de lado do mesmo jeito que um
    /// só modificado (` M`): o avanço passa e o modelo é o do origin.
    ///
    /// O descarte antigo restaurava do ÍNDICE, então uma mudança encenada
    /// continuava na frente do fast-forward — recusa com um remédio que falhava
    /// do mesmo jeito. O stash guarda os dois estados.
    #[test]
    fn a_staged_census_change_is_set_aside_and_the_base_advances() {
        use crate::commands::event::work_branch::{cut_pending_work_branch, CutOutcome};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let root_s = root.to_string_lossy().to_string();
        let (ahead, origins_census) = origin_ahead_touching_the_census(root);

        remine(&model_of(root));
        leftover_enrichment(root);
        git(root, &["add", ".claude/grain.model.json"]);
        assert!(
            porcelain(root).lines().any(|l| l.starts_with("M ")),
            "precondição: o modelo está ENCENADO: {}",
            porcelain(root)
        );

        let sid = "sess-staged-census";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(outcome, CutOutcome::Cut("dev_second".to_string()), "{outcome:?}");
        assert!(
            git_out(root, &["rev-list", "dev"]).expect("rev-list").contains(&ahead),
            "a base avançou apesar do censo encenado",
        );
        assert_eq!(std::fs::read_to_string(model_of(root)).unwrap(), origins_census);
        assert_eq!(porcelain(root), "", "o resto do censo foi gravado, nada sobrou");
        assert!(
            git_out(root, &["rev-parse", "--verify", "--quiet", "refs/stash"]).is_none(),
            "a entrada de stash foi consumida",
        );
    }

    /// Um molde AUTORADO (`source: scan`, escrito pela passagem de
    /// enriquecimento e não regenerado pelo mine) que o `origin` também
    /// reescreveu: os DOIS textos sobrevivem — o do origin no lugar dele, o
    /// local ao lado — e o stderr diz onde. O descarte antigo apagava o local
    /// em silêncio.
    #[test]
    fn an_authored_mold_rewritten_on_origin_too_is_kept_beside_origins() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        repo_tracking_the_census(root);
        // O molde, rastreado, escrito pela passagem de enriquecimento.
        let mold = root.join("apps").join("rt").join(".claude").join("skills").join("rt-gate-pattern");
        std::fs::create_dir_all(&mold).unwrap();
        let mold_rel = "apps/rt/.claude/skills/rt-gate-pattern/SKILL.md";
        std::fs::write(mold.join("SKILL.md"), "---\nname: rt-gate-pattern\nsource: scan\n---\n\nA\n")
            .unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "the mold"]);
        let origin = tmp.path().join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "dev"]);
        // A máquina A re-autorou o molde e publicou.
        const THEIRS: &str = "---\nname: rt-gate-pattern\nsource: scan\n---\n\nB (origin)\n";
        std::fs::write(mold.join("SKILL.md"), THEIRS).unwrap();
        git(root, &["commit", "-q", "-am", "re-authored on origin"]);
        git(root, &["push", "-q", "origin", "dev"]);
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        // A máquina B também, sem ter puxado.
        const OURS: &str = "---\nname: rt-gate-pattern\nsource: scan\n---\n\nC (local)\n";
        std::fs::write(mold.join("SKILL.md"), OURS).unwrap();
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "precondição: o molde `source: scan` é censo",
        );

        let settled = settle_open(root, Some("dev"), Some("dev"), &flow_config());
        assert!(
            !matches!(settled, CensusSettlement::Refuse(_)),
            "o molde no caminho não prende a base: {settled:?}",
        );
        assert_eq!(
            std::fs::read_to_string(mold.join("SKILL.md")).unwrap(),
            THEIRS,
            "o texto do origin está no lugar dele",
        );
        assert_eq!(
            std::fs::read_to_string(mold.join("SKILL.set-aside.md")).unwrap(),
            OURS,
            "e o texto local foi mantido AO LADO, não apagado",
        );
        assert!(
            git_out(root, &["rev-parse", "--verify", "--quiet", "refs/stash"]).is_none(),
            "com os dois textos em casa, a entrada de stash foi consumida",
        );
        let CheckoutWork::Holds { theirs, .. } = checkout_work(root) else {
            panic!("o texto mantido ao lado é do operador reconciliar");
        };
        assert_eq!(theirs, vec![mold_rel.replace("SKILL.md", "SKILL.set-aside.md")]);
    }

    /// Uma base PROTEGIDA com o censo re-minerado E uma edição do operador —
    /// a primeira unidade cortando no lugar, por desenho — cujo `origin` tocou
    /// o censo. Devolve o commit à frente e o conteúdo do origin para o modelo.
    fn protected_main_behind_origin(root: &Path) -> (String, &'static str) {
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"main"},"protected":["main"]}}"#,
        )
        .unwrap();
        init_repo_on(root, "main");
        let model = default_model_path(root);
        std::fs::create_dir_all(model.parent().expect("model parent")).unwrap();
        std::fs::write(&model, "{\"projects\":[]}\n").unwrap();
        std::fs::write(model.with_file_name(GRAIN_DICTIONARY), "{\"terms\":[]}\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "track the census"]);
        let origin = root.parent().expect("tmp").join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "main"]);
        const ORIGINS_CENSUS: &str = "{\"projects\":[{\"dir\":\"apps/rt\"},{\"dir\":\"apps/cli\"}]}\n";
        std::fs::write(&model, ORIGINS_CENSUS).unwrap();
        git(root, &["commit", "-q", "-am", "chore: refresh the deterministic project census"]);
        git(root, &["push", "-q", "origin", "main"]);
        let ahead = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");
        git(root, &["reset", "-q", "--hard", "HEAD~1"]);
        (ahead, ORIGINS_CENSUS)
    }

    /// `Holds` numa base protegida: o trabalho do operador segue para a
    /// primeira unidade por desenho, mas o CENSO ao lado dele continua sendo da
    /// ferramenta — e é posto de lado para a base avançar, exatamente como
    /// numa árvore só de censo. Antes, a leitura `Holds` descartava os caminhos
    /// do censo, o fast-forward abortava neles e toda escrita da sessão era
    /// negada prescrevendo um `git pull` que abortava do mesmo jeito.
    #[test]
    fn a_holds_tree_on_a_protected_base_sets_its_census_aside_and_advances() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let (ahead, origins_census) = protected_main_behind_origin(root);
        let config = ProjectConfig::load(root);
        remine(&model_of(root));
        std::fs::write(root.join("theirs.txt"), "mine, not yours\n").unwrap();
        let CheckoutWork::Holds { theirs, census } = checkout_work(root) else {
            panic!("precondição: trabalho do operador E censo");
        };
        assert_eq!(theirs, vec!["theirs.txt".to_string()]);
        assert!(census.iter().any(|p| p.ends_with("grain.model.json")), "{census:?}");

        let settled = settle_cut(root, Some("main"), "feature/first", Some("main"), &config);
        assert!(
            !matches!(settled, CensusSettlement::Refuse(_)),
            "o censo ao lado do trabalho deles não prende a base: {settled:?}",
        );
        assert!(
            git_out(root, &["rev-list", "main"]).expect("rev-list").contains(&ahead),
            "a base avançou",
        );
        assert_eq!(std::fs::read_to_string(model_of(root)).unwrap(), origins_census);
        assert_eq!(
            std::fs::read_to_string(root.join("theirs.txt")).unwrap(),
            "mine, not yours\n",
            "e o arquivo deles não foi tocado",
        );
    }

    /// …e quando é o arquivo DELES que o avanço sobrescreveria, a recusa nomeia
    /// esse arquivo e prescreve o stash — não um `git pull` que falha nele do
    /// mesmo jeito — e não toca em nada: nem o censo é posto de lado.
    #[test]
    fn their_file_in_the_way_of_the_advance_is_named_and_the_stash_prescribed() {
        use crate::commands::event::work_branch::RefusalCause;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.as_path();
        let (ahead, _) = protected_main_behind_origin(root);
        // O origin também tocou `f.txt` (em cima do commit à frente; a máquina
        // B volta dois)…
        git(root, &["reset", "-q", "--hard", &ahead]);
        std::fs::write(root.join("f.txt"), "theirs on origin").unwrap();
        git(root, &["commit", "-q", "-am", "f on origin"]);
        git(root, &["push", "-q", "origin", "main"]);
        git(root, &["reset", "-q", "--hard", "HEAD~2"]);
        // …que o operador editou aqui, sem commitar.
        std::fs::write(root.join("f.txt"), "edited here").unwrap();
        remine(&model_of(root));
        let config = ProjectConfig::load(root);
        let dirty_before = porcelain(root);
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let settled = settle_cut(root, Some("main"), "feature/first", Some("main"), &config);
        let CensusSettlement::Refuse(busy) = settled else {
            panic!("o arquivo deles no caminho recusa: {settled:?}");
        };
        let RefusalCause::BaseBlockedByWork { base, paths } = &busy.cause else {
            panic!("a causa nomeia o trabalho deles: {:?}", busy.cause);
        };
        assert_eq!(base, "main");
        assert_eq!(paths, &vec!["f.txt".to_string()], "só o arquivo no caminho, não todo o sujo");
        let reason = busy.reason(mustard_core::platform::i18n::Locale::EnUs);
        assert!(reason.contains("f.txt"), "a frase nomeia o arquivo: {reason}");
        assert!(reason.contains("stash"), "e prescreve o stash: {reason}");
        assert!(
            !git_out(root, &["rev-list", "main"]).expect("rev-list").contains(&ahead),
            "a base não avançou",
        );
        assert_eq!(git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"), head_before);
        assert_eq!(porcelain(root), dirty_before, "nada foi tocado, nem o censo");
        assert!(
            git_out(root, &["rev-parse", "--verify", "--quiet", "refs/stash"]).is_none(),
            "e nada foi posto de lado",
        );
    }

    /// O mine não escreve onde a gravação não poderia cair.
    ///
    /// Um censo VISÍVEL re-minerado fora da base suja a árvore com arquivos
    /// versionados que a resposta compartilhada não vai gravar ali — e o corte
    /// seguinte recusa por causa deles. É a ferramenta se barrando na própria
    /// saída, um passo acima de onde este trabalho a encontrou.
    ///
    /// A outra metade, que não pode ser apertada junto: um censo que o git NÃO
    /// vê não entra em commit nenhum, então nenhuma posição está errada para ele
    /// — e um install privado, que está sujo quase sempre, nunca re-mineraria se
    /// a posição valesse também ali.
    #[test]
    fn a_visible_census_is_not_mined_where_it_could_not_be_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = repo_tracking_the_census(root);
        std::fs::remove_file(&model).unwrap();
        let work = checkout_work(root);
        assert!(
            census_refresh_due(root, &model, &work),
            "precondição: o mine está vencido e a árvore só tem censo",
        );

        assert!(
            !mine_census_if_stale(root, &work, false),
            "fora da base o mine é adiado: o que ele escrevesse ficaria sujo para o \
             próximo corte recusar",
        );

        // E o censo INVISÍVEL, na MESMA posição, é minerado. Repositório
        // próprio porque a fixture acima RASTREIA o censo, e um caminho já
        // rastreado não é "ignorado" para o git por mais regras de exclude que
        // se escreva — é a mesma razão pela qual a pergunta é feita dos
        // ARQUIVOS e não do modo de instalação.
        let hidden = tempfile::tempdir().unwrap();
        let hidden = hidden.path();
        init_repo_on(hidden, "dev");
        let info = hidden.join(".git").join("info");
        std::fs::create_dir_all(&info).unwrap();
        let mut rules: Vec<String> =
            mustard_core::PRIVATE_MARKS.iter().map(|m| (*m).to_string()).collect();
        rules.push(".claude/grain.model.json".to_string());
        rules.push(".claude/grain.dictionary.json".to_string());
        std::fs::write(info.join("exclude"), rules.join("\n") + "\n").unwrap();
        assert!(
            mine_census_if_stale(hidden, &checkout_work(hidden), false),
            "um censo que o git não vê não depende de posição nenhuma",
        );
    }

    /// …e a outra metade da mesma regra: com trabalho do operador junto, a
    /// recusa de hoje continua valendo, nomeando SÓ o que é dele — e o portão
    /// não grava nada, porque um commit ali varreria a mudança do operador para
    /// dentro de um commit da ferramenta.
    #[test]
    fn operator_work_beside_the_census_still_refuses_and_names_only_theirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = repo_tracking_the_census(root);
        git(root, &["checkout", "-b", "dev_first"]);

        remine(&model);
        leftover_enrichment(root);
        std::fs::write(root.join("theirs.txt"), "mine, not yours\n").unwrap();
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let CensusSettlement::Refuse(busy) =
            settle_cut(root, Some("dev_first"), "dev_second", Some("dev"), &flow_config())
        else {
            panic!("o trabalho do operador ainda recusa o corte");
        };
        let CheckoutWork::Holds { theirs: dirty, .. } = &busy.work else {
            panic!("os caminhos foram observados, veio {:?}", busy.work);
        };
        assert_eq!(
            dirty,
            &vec!["theirs.txt".to_string()],
            "a recusa nomeia só o que é do operador: {dirty:?}",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "com trabalho do operador na árvore o portão não commita nada",
        );
        assert!(
            porcelain(root).contains("theirs.txt"),
            "e o arquivo dele segue sendo dele para commitar",
        );
    }
}
