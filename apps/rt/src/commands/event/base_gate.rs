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
//! 2. Otherwise → `Open`.
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
//! can never wedge a project it cannot reason about.
//!
//! **Offline is not a verdict either.** Freshness needs the network; when the
//! fetch fails there is no evidence the base is behind, so the gate opens.
//! Refusing there would ground every offline session on a fact nobody measured.
//!
//! ## What it does not do
//!
//! It never touches the census. The map is updated by `mustard-rt run scan`,
//! which reads only what changed and never writes to git; nothing here mines
//! it and nothing here commits. What the opening door does after `Open` —
//! refreshing the base from `origin` — belongs to [`super::census_settlement`].

use std::path::Path;

use mustard_core::ProjectConfig;

use crate::commands::git_settle::git_out;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::event::census_settlement::{settle, CensusSettlement, CheckoutPosition};
    use crate::commands::event::work_branch::{checkout_work, CheckoutWork};
    use crate::commands::scan::default_model_path;
    use std::process::Command;

    /// The scan's second artifact, written beside the model on every run.
    const DICTIONARY: &str = "grain.dictionary.json";

    /// A pergunta inteira, feita como a porta de CORTE a faz.
    ///
    /// As fixtures deste módulo medem pelo MESMO ponto de entrada que o produto
    /// usa, e não por uma metade dele.
    fn settle_cut(
        root: &Path,
        current: Option<&str>,
        target: &str,
        base: Option<&str>,
        config: &ProjectConfig,
    ) -> CensusSettlement {
        settle(root, CheckoutPosition::at(current, Some(target), base), config)
    }

    /// …e como a porta EXPLÍCITA do `emit-pipeline` a faz: sem alvo, porque ali
    /// nada é checado out e portanto nada pode viajar.
    fn settle_open(
        root: &Path,
        current: Option<&str>,
        base: Option<&str>,
        config: &ProjectConfig,
    ) -> CensusSettlement {
        settle(root, CheckoutPosition::at(current, None, base), config)
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

    /// The refusal this test used to assert is GONE, and its absence is
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

    /// The compatibility half, and the reason `git.flow` was kept rather
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
    /// committed — the shape where a re-mined census shows up as a dirty tree
    /// at all. Returns the model path. The fixture tracks BOTH artifacts a scan
    /// writes, because the real miner writes both.
    fn repo_tracking_the_census(root: &Path) -> std::path::PathBuf {
        init_repo_on(root, "dev");
        let model = default_model_path(root);
        std::fs::create_dir_all(model.parent().expect("model parent")).unwrap();
        std::fs::write(&model, "{\"projects\":[]}\n").unwrap();
        std::fs::write(model.with_file_name(DICTIONARY), "{\"terms\":[]}\n").unwrap();
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
        std::fs::write(model.with_file_name(DICTIONARY), "{\"terms\":[\"wave\"]}\n").unwrap();
    }

    /// Deixa na árvore, e só na árvore, a saída da passagem de ENRIQUECIMENTO —
    /// o mapa de um subprojeto e um molde `{papel}-pattern`, que o mine
    /// determinístico não escreve.
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

    /// A abertura ORDINÁRIA: o operador parado NA base, a árvore suja só com o
    /// censo, e o corte da próxima unidade NÃO é recusado — e nenhum commit é
    /// criado. O censo não é trabalho de ninguém e não entra no git: ele segue
    /// sujo para a branch nova, sem ser gravado.
    ///
    /// Medido pela porta REAL (`cut_pending_work_branch`).
    #[test]
    fn a_census_only_dirty_tree_is_cut_without_a_commit() {
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

        let commits_before = git_out(root, &["rev-list", "--count", "HEAD"]).expect("count");

        // A porta real, e só ela: nada disso é trabalho de ninguém, então o
        // corte acontece de verdade.
        let sid = "sess-census-only-open";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = cut_pending_work_branch(root, sid);
        assert_eq!(
            outcome,
            CutOutcome::Cut("dev_second".to_string()),
            "a abertura ordinária não é recusada: {outcome:?}",
        );
        assert_eq!(
            git_out(root, &["rev-list", "--count", "HEAD"]).expect("count"),
            commits_before,
            "o corte não cria commit nenhum",
        );
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "e o censo segue sujo na branch nova, sem ser gravado",
        );
    }

    /// A base é ATUALIZADA a partir do `origin` com a árvore suja só com o
    /// censo, e nenhum commit é escrito nela: depois do corte a base local é
    /// exatamente a do `origin`. É a invariante que o Guard do `CLAUDE.md` da
    /// raiz enuncia: `--ff-only` só passa enquanto a base de integração não
    /// tem commit próprio, e o `git pull --ff-only origin {base}` que a recusa
    /// deste portão prescreve continua passando.
    ///
    /// O commit do `origin` é VAZIO de propósito: o avanço não depende da
    /// árvore suja. O caso em que o commit do `origin` TOCA o censo sujo é
    /// medido à parte, em
    /// `a_census_in_the_way_of_the_advance_is_set_aside_and_nothing_is_committed`.
    #[test]
    fn the_base_advances_under_a_census_only_tree_without_a_commit() {
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
        // é VAZIO de propósito: assim o fast-forward não depende da árvore suja.
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

        // A abertura ordinária: a árvore suja só com o censo.
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

        assert_eq!(
            git_out(root, &["rev-parse", "dev"]).expect("dev"),
            ahead,
            "a base é exatamente a do origin: avançou, e nada foi commitado nela",
        );
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "e o censo segue sujo, sem ser gravado",
        );
    }

    /// Um corte RECUSADO por base desconhecida não deixa nada para trás. Com
    /// vários candidatos declarados e nada dizendo de qual base a emergência
    /// saiu, o corte devolve `BaseUnknown` e não toca no git: nenhum commit,
    /// nenhuma branch, a árvore como estava.
    #[test]
    fn a_cut_denied_for_an_unknown_base_touches_nothing() {
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
            "e nenhum commit fica para trás de um corte que não houve",
        );
        assert_eq!(
            porcelain(root),
            dirty_before,
            "a árvore fica exatamente como estava, para o corte que vier de fato",
        );
        assert!(
            git_out(root, &["rev-parse", "--verify", "hotfix/urgente"]).is_none(),
            "e nenhuma branch foi criada",
        );
    }

    /// Numa base PROTEGIDA, com a árvore suja só com o censo, o corte segue e
    /// não cria commit: o censo não é trabalho de ninguém, então não há o que
    /// recusar nem onde gravá-lo. A posição NÃO MEDIDA (`HEAD` destacado, ou
    /// ilegível) responde igual.
    #[test]
    fn a_census_only_tree_on_a_protected_base_proceeds_without_a_commit() {
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
        for current in [Some("dev"), Some("HEAD"), None] {
            let settled = settle_cut(root, current, "dev_second", Some("dev"), &config);
            assert_eq!(
                settled,
                CensusSettlement::Proceed,
                "posição {current:?}: o censo sozinho não recusa nada",
            );
        }
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "nenhum commit foi criado",
        );
        assert_eq!(porcelain(root), dirty_before, "e a árvore fica exatamente como estava");
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
        git(root, &["commit", "-q", "-am", "another machine re-mined the census"]);
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

    /// O censo sujo no caminho do avanço — o modelo que o `origin` também
    /// reescreveu — é posto de lado, a base avança, e nada é commitado: a base
    /// local fica exatamente a do `origin`, e o `git pull --ff-only origin dev`
    /// que o portão prescreve continua passando. O modelo é o do `origin`, e o
    /// resto do censo segue sujo, sem ser gravado.
    #[test]
    fn a_census_in_the_way_of_the_advance_is_set_aside_and_nothing_is_committed() {
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
        assert_eq!(
            git_out(root, &["rev-parse", "dev"]).expect("dev"),
            ahead,
            "a base é exatamente a do origin — nenhum commit foi escrito nela",
        );
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "e o resto do censo segue sujo, sem ser gravado",
        );
    }

    /// …e quando o avanço NÃO tem como passar — a base local divergiu —, a
    /// resposta é RECUSAR, alto, com as palavras do git: nunca engolir e nunca
    /// cortar de uma base velha. E a recusa vem ANTES de qualquer ação: nada
    /// posto de lado, a árvore como estava.
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

    /// A porta EXPLÍCITA só move a base sobre a qual abre — e nenhuma outra.
    ///
    /// Um passo antigo avançava TODA base pré-selecionada do fluxo
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

    /// A árvore que TODAS as portas recebem neste módulo: o censo re-minerado
    /// e a saída da passagem de enriquecimento, e mais nada do operador.
    /// `stand_on` põe a árvore fora da base quando é `Some`.
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

    /// O que uma porta deixou observável: escreveu algum commit, e o censo
    /// continua sujo na árvore?
    #[derive(Debug, PartialEq, Eq)]
    struct DoorAnswerSeen {
        wrote_a_commit: bool,
        census_still_in_the_tree: bool,
    }

    /// Quantos commits o repositório inteiro tem — todas as refs, para que um
    /// commit escrito numa branch que não é a do checkout também apareça.
    fn commit_count(root: &Path) -> String {
        git_out(root, &["rev-list", "--count", "--all"]).expect("rev-list --count")
    }

    fn what_the_door_left(root: &Path, commits_before: &str) -> DoorAnswerSeen {
        DoorAnswerSeen {
            wrote_a_commit: commit_count(root) != commits_before,
            // Lido pela classificação do PRÓPRIO produto, e não por um
            // `git status --porcelain` cru: aquele COLAPSA um diretório
            // inteiramente não rastreado numa linha só.
            census_still_in_the_tree: matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
        }
    }

    /// NENHUMA porta cria commit.
    ///
    /// As duas portas que fazem a pergunta — a abertura explícita e o corte do
    /// `spec-draft` — recebem a MESMA árvore suja só com o censo, em duas
    /// posições, e deixam a mesma coisa: nenhum commit escrito,
    /// e o censo segue sujo na árvore. Uma porta nova que não passe pela
    /// resposta compartilhada diverge das outras aqui.
    #[test]
    fn no_door_writes_a_commit_for_a_census_only_tree() {
        type Door = fn(&Path, &str);
        let doors: [(&str, Door); 2] = [
            ("emit-pipeline (a porta explícita)", |root, _sid| {
                let _ = crate::commands::event::emit_pipeline::enforce_base_gate_at(
                    root,
                    None,
                    Some("dev"),
                );
            }),
            ("spec-draft (o corte da branch)", |root, sid| {
                let root_s = root.to_string_lossy().to_string();
                crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
                let _ = crate::commands::event::work_branch::cut_pending_work_branch(root, sid);
            }),
        ];
        let expected = DoorAnswerSeen {
            wrote_a_commit: false,
            census_still_in_the_tree: true,
        };

        for (position, stand_on) in [
            ("parado NA base", None),
            ("parado na branch de OUTRA unidade", Some("feature/outra-unidade")),
        ] {
            for (name, door) in doors {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path();
                a_tree_dirty_only_with_the_census(root, stand_on);
                let commits_before = commit_count(root);
                door(root, "sess-no-commit");
                assert_eq!(
                    what_the_door_left(root, &commits_before),
                    expected,
                    "{position}: '{name}' deixou algo diferente das outras portas",
                );
            }
        }
    }

    /// UMA porta, UMA varredura da árvore.
    ///
    /// `checkout_work` roda um `git status --porcelain --untracked-files=all` do
    /// repositório inteiro e abre cada `SKILL.md` sujo. Cada porta o roda UMA
    /// vez: a resposta compartilhada mede, e nada depois dela mede de novo.
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
            "a porta explícita mede a árvore uma vez",
        );

        TREE_PROBES.with(|n| n.set(0));
        crate::shared::context::set_pending_branch(&root_s, "sess-one-probe", "dev_second", None);
        let _ = crate::commands::event::work_branch::cut_pending_work_branch(root, "sess-one-probe");
        assert_eq!(
            TREE_PROBES.with(|n| n.get()),
            1,
            "e a porta de corte também",
        );
    }

    /// Um molde ADOTADO (`source: manual`) é escrita do OPERADOR, e o caminho
    /// dele é igualzinho ao de um molde gerado — o frontmatter é o que separa.
    ///
    /// Lê-lo como censo faria o corte parar de recusar por causa da edição à
    /// mão de alguém, e ela viajaria para a unidade nova. A recusa NOMEIA o
    /// molde adotado, e devolve antes de qualquer fetch.
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
            "e nada foi commitado",
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
        // …e a árvore parada na branch de OUTRA unidade, com trabalho dela não
        // commitado ao lado do censo: o corte é recusado porque esse trabalho
        // viajaria.
        git(root, &["checkout", "-q", "-b", "feature/outra-unidade"]);
        std::fs::write(root.join("theirs.txt"), "mine, not yours\n").unwrap();
        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        let settled = settle_cut(
            root,
            Some("feature/outra-unidade"),
            "dev_second",
            Some("dev"),
            &flow_config(),
        );
        let CensusSettlement::Refuse(busy) = settled else {
            panic!("a precondição é a recusa: {settled:?}");
        };
        assert_eq!(
            busy.cause,
            crate::commands::event::work_branch::RefusalCause::WorkWouldTravel,
            "a recusa é pelo trabalho que viajaria",
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
        assert_eq!(
            git_out(root, &["rev-parse", "dev"]).expect("dev"),
            ahead,
            "a base é exatamente a do origin — nenhum commit foi escrito nela",
        );
        assert!(
            matches!(checkout_work(root), CheckoutWork::CensusOnly(_)),
            "e o resto do censo segue sujo, sem ser gravado",
        );
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
        std::fs::write(model.with_file_name(DICTIONARY), "{\"terms\":[]}\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "track the census"]);
        let origin = root.parent().expect("tmp").join("origin.git");
        let origin_s = origin.to_string_lossy().to_string();
        git(root, &["init", "--bare", "-q", &origin_s]);
        git(root, &["remote", "add", "origin", &origin_s]);
        git(root, &["push", "-q", "origin", "main"]);
        const ORIGINS_CENSUS: &str = "{\"projects\":[{\"dir\":\"apps/rt\"},{\"dir\":\"apps/cli\"}]}\n";
        std::fs::write(&model, ORIGINS_CENSUS).unwrap();
        git(root, &["commit", "-q", "-am", "another machine re-mined the census"]);
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

    /// …e a outra metade da mesma regra: com trabalho do operador junto, a
    /// recusa continua valendo, nomeando SÓ o que é dele — e nada é commitado.
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
