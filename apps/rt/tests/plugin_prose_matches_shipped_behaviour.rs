// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! A mechanism nobody is told about is a mechanism nobody takes.
//!
//! Mecanismos já chegaram entregues e sem leitor: o binário os emitia enquanto
//! a prosa do operador seguia descrevendo o mundo sem eles — o mesmo defeito
//! que esta spec existe para tirar, uma camada acima: o harness afirmando uma
//! completude que não conferiu. O contrário também já aconteceu, e é o mesmo
//! defeito virado do avesso: a prosa prometendo um passo que o binário nunca
//! entrega.
//!
//! Every test here reads BOTH halves and requires them to agree:
//!
//! 1. the shipped prose under `plugin/` names the mechanism, at the place a
//!    reader actually arrives at it — not merely somewhere in the file; and
//! 2. the CODE still emits or takes what that prose promises.
//!
//! Half 2 is what makes these more than spell-checks. A prose assertion alone
//! passes forever once the sentence is written, even after the mechanism is
//! deleted; asserting the emitter too means the pair can only be broken
//! together, deliberately.
//!
//! They live in `tests/` rather than in-file because each acceptance criterion
//! runs `cargo test -p mustard-rt <fn>` and libtest matches the FULL test path —
//! which equals the bare function name only at the root of an integration-test
//! binary.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_rt::commands::flow::resume::NEXT_BY_PHASE;


/// The repository root — two levels up from this crate's manifest.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Read a repo-relative file, failing with the path when it is missing.
fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{rel} unreadable at {}: {e}", path.display()))
}

/// The first line of `body` containing `needle`, or `None`.
fn line_with<'a>(body: &'a str, needle: &str) -> Option<&'a str> {
    body.lines().find(|l| l.contains(needle))
}

/// The PRODUCTION half of a Rust source file — everything before its
/// `#[cfg(test)]` module.
///
/// A negative assertion ("this sentence is gone from the shipped code") must
/// not be answered by the test that quotes the sentence in order to forbid it,
/// nor by the doc comment explaining what was removed. Whole-file `contains`
/// is right for the POSITIVE half and wrong for this one.
fn production_half(rel: &str) -> String {
    let body = read(rel);
    match body.find("#[cfg(test)]") {
        Some(at) => body[..at].to_string(),
        None => body,
    }
}

/// Where that line SITS — the only way to assert an order between two rows.
/// `line_with` answers whether a row exists, which is what let a re-ordering
/// of the unit's question pass every ratchet it had.
fn line_index(body: &str, needle: &str) -> Option<usize> {
    body.lines().position(|l| l.contains(needle))
}

/// The options a `  label:` row of the unit's question OFFERS, in order, with
/// the label stripped — columns are separated by three or more spaces, so a
/// single-space phrase like `…ou o seu` stays ONE entry instead of three.
fn offered_options<'a>(row: &'a str, label: &str) -> Vec<&'a str> {
    let (_, rest) = row.split_once(label).unwrap_or(("", row));
    rest.split("   ").map(str::trim).filter(|s| !s.is_empty()).collect()
}





/// Os nomes que a prosa entregue diz que o campo do próximo passo pode
/// entregar são exatamente os que a tabela do próximo passo entrega.
///
/// As duas metades, como todo teste deste arquivo. A prosa: a porta da retomada
/// lista, para o leitor, os comandos que a resposta pode mandar rodar. O
/// código: a tabela do próximo passo, que é de onde o campo `command` sai. Uma
/// lista de prosa sozinha passa para sempre depois de escrita, inclusive depois
/// de o passo que ela promete deixar de existir.
///
/// Sobrar de um lado e faltar do outro são dois defeitos diferentes, e o teste
/// nomeia cada um. Prometido e não entregue: quem lê a porta espera um passo
/// que nunca chega, e fica esperando ou o escolhe sozinho — que é exatamente o
/// que a porta proíbe. Entregue e não prometido: a resposta manda rodar um
/// comando que o leitor não foi ensinado a reconhecer.
#[test]
fn a_porta_promete_os_mesmos_comandos_que_o_proximo_passo_entrega() {
    // --- 1. A prosa entregue: os nomes que a porta lista ------------------
    let porta = read("plugin/commands/continue.md");
    let linha = line_with(&porta, "The names it can hand you")
        .expect("a porta da retomada lista os comandos que o campo pode entregar");
    let prometidos: BTreeSet<String> = linha
        .split("mustard-rt run ")
        .skip(1)
        .filter_map(|rest| {
            let nome: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                .collect();
            (!nome.is_empty()).then_some(nome)
        })
        .collect();
    assert!(!prometidos.is_empty(), "a porta não nomeia comando nenhum: {linha}");

    // --- 2. O código: os nomes que a tabela do próximo passo entrega ------
    let entregues: BTreeSet<String> =
        NEXT_BY_PHASE.iter().map(|(_, comando)| (*comando).to_string()).collect();
    assert!(!entregues.is_empty(), "a tabela do próximo passo está vazia");

    let so_na_prosa: Vec<&String> = prometidos.difference(&entregues).collect();
    assert!(
        so_na_prosa.is_empty(),
        "a porta promete passos que o campo nunca entrega: {so_na_prosa:?} - \
         quem obedecer a porta espera um comando que nenhuma fase manda rodar"
    );
    let so_no_codigo: Vec<&String> = entregues.difference(&prometidos).collect();
    assert!(
        so_no_codigo.is_empty(),
        "o campo entrega passos que a porta não ensina: {so_no_codigo:?} - \
         a resposta manda rodar um comando que o leitor não foi apresentado"
    );
}

/// The orchestrator's Verdict rule names a MEASUREMENT an agent claims
/// as the second thing never relayed on a briefing alone.
///
/// The rule used to cover one claim only: a runtime symptom the user reported.
/// So an orchestrator following it to the letter relayed "13 of 13 passed"
/// because an agent said so — which happened, and was false. The second half
/// says a measurement is not evidence until the orchestrator takes it itself.
///
/// The counterweight is asserted too, and deliberately: a rule that only added
/// "verify more" would license re-deriving the whole briefing and spending a
/// subagent to double-check one's own work. Both halves must survive together
/// or the sentence teaches the opposite failure.
#[test]
fn orchestrator_prose_teaches_the_measurement_half_of_the_verdict_rule() {
    // --- 1. The shipped seed states both claims, measurement second -------
    // The compiled-in seed is what `upsert` lays down in every project, so
    // this reads the text that actually ships — not a stray copy on disk.
    let seed = mustard_core::ORCHESTRATOR_MD;
    let verdict =
        line_with(seed, "Verdict rule").expect("the orchestrator seed states no Verdict rule");

    let symptom_at = verdict
        .find("runtime symptom")
        .expect("the Verdict rule dropped its first half — the user-reported symptom");
    let measurement_at = verdict.find("MEASUREMENT").unwrap_or_else(|| {
        panic!("the Verdict rule never names a measurement an agent claims: {verdict}")
    });
    assert!(
        measurement_at > symptom_at,
        "the measurement claim must be the SECOND thing the rule refuses to relay, \
         after the reported symptom (symptom at {symptom_at}, measurement at {measurement_at})",
    );

    // Naming it is not teaching it: the line must say what turns the claim
    // into evidence, which is taking the measurement again.
    assert!(
        verdict.contains("take it yourself"),
        "the rule names a claimed measurement without saying who has to take it: {verdict}",
    );
    assert!(
        verdict.contains("re-run the command"),
        "the rule must name the act that settles it — re-running the command: {verdict}",
    );

    // The counterweight, so the rule cannot be read as "verify everything".
    assert!(
        verdict.contains("double-checking your own work"),
        "the rule adds verification without its limit — the rest of a briefing IS \
         the answer, and no subagent re-checks your own work: {verdict}",
    );

    // --- 2. The seed is really the file a session reads -------------------
    // Without this half the sentence is a template nobody is served.
    let project_seed = read("packages/core/src/platform/project_seed.rs");
    assert!(
        project_seed.contains("(\"orchestrator.md\", ORCHESTRATOR_MD)"),
        "nothing seeds orchestrator.md any more, so the rule reaches no window",
    );
    let config = read("packages/core/src/domain/config.rs");
    assert!(
        config.contains(".claude/mustard/orchestrator.md"),
        "the default inject no longer declares the orchestrator injectable",
    );

    // --- 3. This repository's own delivered copy has not drifted -----------
    // `seed_injectable_files` rewrites the file, but only when it RUNS: editing
    // the template alone leaves this repository's committed copy behind until
    // an install or an update lays the new body down. Silent drift is the whole
    // failure mode: the rule would ship to new projects while the one that
    // wrote it kept reading the old text.
    let delivered = read(".claude/mustard/orchestrator.md");
    let delivered_verdict = line_with(&delivered, "Verdict rule")
        .expect("the delivered injectable states no Verdict rule");
    assert_eq!(
        delivered_verdict, verdict,
        "the delivered .claude/mustard/orchestrator.md drifted from the seed — \
         re-seed it, or this project never reads the rule it just wrote",
    );
}





/// The question asks WHERE the unit starts before WHAT it is called.
///
/// The ratchet above demands both rows EXIST and says nothing about their
/// order, so shipping `tipo` above `sai de` broke no test — and a type read
/// first makes the base look like its consequence, which is the implication
/// this product removed the day the base began being chosen against a real
/// catalogue.
///
/// Prose-only, deliberately: the row order is a RENDERING decision and no
/// emitter can be asked whether the block was drawn in it. The mechanism half
/// — that the base is MEASURED rather than derived from the type — is already
/// ratcheted by `router_prose_teaches_the_kind_named_branch_and_its_one_question`,
/// and duplicating it here would assert the wrong thing twice.
#[test]
fn router_asks_the_base_before_the_type() {
    let seed = mustard_core::DISPATCH_MD;
    let delivered = read(".claude/mustard/dispatch.md");

    for (label, body) in [("the seed", seed), ("the delivered copy", delivered.as_str())] {
        let base = line_index(body, "  sai de:")
            .unwrap_or_else(|| panic!("{label} shows no `sai de` row — the base is never asked"));
        let kind = line_index(body, "  tipo:")
            .unwrap_or_else(|| panic!("{label} shows no `tipo` row — the type is never asked"));
        assert!(
            base < kind,
            "{label} shows `tipo` above `sai de`, so the base reads as a consequence \
             of the type — the implication a real catalogue removed",
        );
    }

    // Both rows still open on a pre-marked answer: an Enter accepts, and the
    // re-order must not cost the operator a decision it never used to cost.
    for (row, marked) in [("  sai de:", "[dev]"), ("  tipo:", "[fix]")] {
        let line = line_with(seed, row).unwrap_or_else(|| panic!("no `{row}` row"));
        assert!(
            line.contains(marked),
            "`{row}` lists its options without PRE-MARKING one, so the re-ordered \
             question costs two decisions instead of two Enters: {line}",
        );
    }

    // Say WHY, or the order is a coincidence the next editor tidies away.
    let why = line_with(seed, "`sai de` FIRST")
        .expect("the router never says the base is asked first — nothing stops a re-order");
    assert!(
        why.contains("before what it is CALLED"),
        "the order is stated without its reason: the operator settles where the unit \
         STARTS before what it is called: {why}",
    );
}

/// The rows are independent fields, the surface has a ceiling, and `hotfix`
/// survives it.
///
/// Two silences in the router produced one defect. "Ask both together" never
/// said the fields are INDEPENDENT, so the question came back as pre-paired
/// options (`fix saindo de dev` / `hotfix saindo de main`) — the cartesian
/// product of two choices, which has no row at all for a `hotfix` cut from the
/// ordinary base. And the prose never named the surface's ceiling of four
/// options, so the renderer dropped a suggestion to fit — and the one it
/// dropped was `hotfix`, the row's whole reason for existing.
#[test]
fn router_forbids_pairing_and_pins_hotfix() {
    let seed = mustard_core::DISPATCH_MD;

    let rule = line_with(seed, "INDEPENDENT fields")
        .expect("the router never says the rows are independent fields");
    assert!(
        rule.contains("cartesian product"),
        "independence is asserted without naming what pairing actually hands back — \
         the product of two choices, in which one combination has no row: {rule}",
    );
    assert!(
        rule.contains("4 options"),
        "the prose never names the ceiling of the question surface, so the reader \
         discovers it by getting it wrong in front of the operator: {rule}",
    );
    assert!(
        rule.contains("PINNED"),
        "nothing forbids dropping `hotfix` to fit the ceiling — the exact suggestion \
         that fell out last time: {rule}",
    );

    // The block obeys its own rule: four options plus the free field, `hotfix`
    // among them, and no row spelling a pair.
    let kind_row = line_with(seed, "  tipo:").expect("the router seed shows no `tipo` row");
    let offered = offered_options(kind_row, "tipo:");
    let (free, options) = offered
        .split_last()
        .expect("the `tipo` row offers nothing at all");
    assert!(
        free.starts_with('…'),
        "the `tipo` row does not end in the free field, so a type the list omits \
         cannot be typed: {kind_row}",
    );
    assert!(
        options.len() <= 4,
        "the `tipo` row offers {} options over a surface that takes 4 — the renderer \
         will drop one, and the prose does not get to choose which: {kind_row}",
        options.len(),
    );
    assert!(
        options.contains(&"hotfix"),
        "`hotfix` is not among the offered types, so an emergency cannot be named \
         from the question: {kind_row}",
    );
    let base_row = line_with(seed, "  sai de:").expect("the router seed shows no `sai de` row");
    let base_offered = offered_options(base_row, "sai de:");
    let (base_free, base_options) = base_offered
        .split_last()
        .expect("the `sai de` row offers nothing at all");
    assert!(
        base_free.starts_with('…'),
        "the `sai de` row has no free field, so a catalogue longer than the ceiling \
         hides the branches that did not fit: {base_row}",
    );
    assert!(
        base_options.len() <= 4,
        "the `sai de` row offers {} options over a surface that takes 4: {base_row}",
        base_options.len(),
    );
    for row in [kind_row, base_row] {
        assert!(
            !row.contains("saindo de"),
            "a row of the question spells a PAIR, which is the defect itself — the \
             operator who wants `hotfix` off the ordinary base finds no line: {row}",
        );
    }

    // The code half: the chooser's suggestions are where a renderer takes its
    // four from, so `hotfix` has to survive that truncation there too.
    let kinds = read("apps/rt/src/shared/work_kind.rs");
    assert!(
        kinds.contains("[\"feature\", \"fix\", \"hotfix\", \"chore\""),
        "`hotfix` fell past the fourth SUGGESTED token, so anything taking the first \
         four to fit the surface drops it — exactly the pin the prose promises",
    );
}







/// Nothing refuses an operation because a branch is absent from the
/// PRE-SELECTED list.
///
/// `git.flow` used to answer two questions at once — "where may a unit be cut
/// from?" and "where is a direct commit forbidden?" — and six places read it to
/// REFUSE. The installer writes no flow at all, so every one of those refusals
/// fired on a correct installation, telling the operator that a branch their
/// project really integrates through "is not an integration base of this
/// project" and offering an edit to a configuration file as the way out.
///
/// This ratchet holds the three command doors to the measured questions: is
/// this branch somebody's WORK UNIT, and is it PROTECTED. Both are facts the
/// repository can answer; membership in a list nobody maintains is not.
#[test]
fn nothing_refuses_for_absence_from_the_preselected_list() {
    // --- 1. The sentence, and the exit it offered, are gone -----------------
    // Read the PRODUCTION half: the tests below each file quote the removed
    // sentence in order to forbid it, and a negative assertion answered by its
    // own guard proves nothing.
    let open = production_half("apps/rt/src/commands/work_unit_open.rs");
    let pr = production_half("apps/rt/src/commands/review/pr_door.rs");
    let delete = production_half("apps/rt/src/commands/git_delete.rs");
    for (name, body) in
        [("work_unit_open", &open), ("pr_door", &pr), ("git_delete", &delete)]
    {
        assert!(
            !body.contains("is not an integration base of this"),
            "{name} still pronounces a verdict about a configuration file over a \
             repository nobody asked about",
        );
        assert!(
            !body.contains("Declare it in mustard.json"),
            "{name} still offers editing the configuration as the way past a refusal \
             — the one exit this design removed",
        );
    }

    // --- 2. Neither door may decide by the pre-selected list -----------------
    //
    // This block used to assert the PRESENCE of the exact expressions each door
    // was written with. That is not a criterion: it certifies code presence, not
    // effectiveness, and it was measured GREEN while reporting "git delete
    // decides what it may remove by a declared list again" — with the shipped
    // code deleting a real `release/2026-Q3` from the remote. A presence
    // assertion can only ever pin the line it was written against, defect
    // included.
    //
    // What is asserted here now is an ABSENCE, which cannot certify a defect:
    // the pre-selected list must not appear in either door at all. The positive
    // half — that the right branches are refused and the right ones deleted — is
    // proven by driving the doors and reading the refs afterwards, in
    // `git_delete::tests::a_slashed_integration_base_is_never_deleted_and_never_refused`.
    for (name, body) in [("pr_door", &pr), ("git_delete", &delete)] {
        // The CALL form, never the bare name: both files legitimately explain in
        // prose why they do not use it, and a check that cannot tell a call from
        // a comment forbids writing down the reason.
        for forbidden in ["preselected_bases()", "flow.bases()"] {
            assert!(
                !body.contains(forbidden),
                "{name} consults `{forbidden}` again. With no `git.flow` written — what \
                 the installer produces today — that set is the hardcoded {{main, master}}, \
                 so the door would decide by two literals and nothing else",
            );
        }
        assert!(
            body.contains("has_unit_record"),
            "{name} no longer asks the project for its RECORD of the unit, so it is back \
             to deciding by the shape of a name — and `release/2026-Q3` has the shape of \
             a unit",
        );
    }
    // O alvo que a recusa nomeia não pode acabar num nome que ninguém mediu.
    //
    // A primeira forma desta conferência proibia o `primary_base()` de uma vez,
    // e isso era cego demais: com um fluxo declarado ele devolve a base que o
    // próprio projeto escreveu, e proibi-lo mandou para outra branch uma
    // unidade que integra na `dev`. A segunda forma exigia uma guarda escrita à
    // mão — "só consulte o declarado quando houver fluxo" — e essa guarda agora
    // mora no tipo: sem fluxo declarado o acessor devolve ausência, e não um
    // nome. O que continua exigido é o último degrau: quando o projeto não
    // declarou nada, a recusa nomeia a branch padrão do próprio remoto.
    for (name, body) in [("pr_door", &pr), ("git_delete", &delete)] {
        assert!(
            body.contains("mustard_core::default_branch("),
            "{name}'s refusal no longer falls back to origin/HEAD, so the base it \
             names can be a literal nobody measured",
        );
    }

    // --- 3. The legacy shape is resolved by the CATALOGUE -------------------
    let work_kind = read("apps/rt/src/shared/work_kind.rs");
    assert!(
        work_kind.contains("fn legacy_base_of") && work_kind.contains("with_remote_names(project"),
        "the `{{base}}_{{slug}}` shape is read against the declaration alone again, \
         so a unit whose base the flow never named is orphaned",
    );
}

/// The doctor does not ask for a `git.flow` the installer no longer writes, and
/// reports what is REALLY protected.
///
/// The check warned that `git.flow` was empty and prescribed declaring one.
/// `project_seed` seeds an EMPTY flow on purpose — the project decides later —
/// so the warning fired on every correct installation, and the claim it made
/// (`only main/master are protected`) was false in front of
/// `protected_branches`. A diagnostic that fires on a healthy install teaches
/// the operator to ignore diagnostics.
#[test]
fn doctor_does_not_ask_for_a_flow_that_the_installer_no_longer_writes() {
    let doctor = production_half("apps/rt/src/commands/doctor/doctor.rs");

    // --- 1. The prescription is gone ----------------------------------------
    assert!(
        !doctor.contains("git.flow is empty"),
        "the doctor still reports the shape a correct install has as a finding",
    );
    assert!(
        !doctor.contains("fix: declare the flow in mustard.json"),
        "the doctor still prescribes declaring a flow to get protection it does not \
         grant",
    );

    // --- 2. O que entrou no lugar é a medição, RODADA e não lida -------------
    //
    // Esta metade já grepou o `doctor.rs` atrás do nome da função e da chamada
    // que ela faz — a prática que o próprio critério proíbe pelo nome, e pelo
    // motivo que cinco rodadas de revisão mostraram: uma busca no texto do
    // código prova que a linha existe, nunca que o comportamento vale. Então a
    // conferência é EXECUTADA contra um projeto de verdade na forma que o
    // instalador deixa (sem `git.flow` escrito) e o que se afirma é a SAÍDA
    // dela.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git")
    };
    // Um origin de verdade: o projeto precisa estar na forma que o instalador
    // deixa, com remoto e sem `git.flow`.
    let upstream = dir.path().join("upstream.git");
    std::process::Command::new("git")
        .args(["init", "-q", "--bare"])
        .arg(&upstream)
        .output()
        .expect("bare origin");
    git(&["init", "."]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    git(&["checkout", "-b", "producao"]);
    std::fs::write(root.join("mustard.json"), r#"{"git":{"provider":"github"}}"#).expect("cfg");
    git(&["add", "-A"]);
    git(&["commit", "-m", "seed"]);
    git(&["remote", "add", "origin", &upstream.to_string_lossy()]);
    git(&["push", "-q", "-u", "origin", "producao"]);
    git(&["remote", "set-head", "origin", "producao"]);

    // `doctor` has no `--root`: it reads the project from the working directory,
    // so the test must STAND in the temp project rather than name it.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "doctor", "--check", "branch-protection"])
        .current_dir(root)
        .output()
        .expect("doctor runs");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "`doctor --check branch-protection` does not run — the name the operator is \
         told to type is not the name the binary answers to: {said}",
    );
    // Sem `git.flow`, nada fica protegido — nem aqui nem no servidor — e o
    // diagnóstico precisa dizer isso. Ficar calado deixaria o operador
    // acreditando numa proteção que não existe: a instalação não escreve fluxo
    // nenhum, e a proteção passou a sair só do que o projeto declara.
    assert!(
        said.contains("git.flow"),
        "o diagnóstico não diz que a falta do `git.flow` deixa tudo desprotegido: {said}",
    );
    assert!(
        said.contains("mustard init"),
        "e não diz como declarar as bases: {said}",
    );

    // --- 3. The installer really writes no flow -----------------------------
    // Without this half the assertions above outlive their reason: they are
    // right only while an empty flow is the INSTALLED shape.
    let seed = read("packages/core/src/platform/project_seed.rs");
    assert!(
        seed.contains("git.flow starts empty — the project decides"),
        "the installer seeds a flow again, which would make the removed warning \
         meaningful and this whole check wrong",
    );
}



// ---------------------------------------------------------------------------
// The two bootstrap twins under `plugin/bin/`.
//
// `mustard-boot.cmd` runs only under cmd.exe, and this block used to claim no
// runner here could execute it. That claim was FALSE, and it cost six releases:
// `.github/workflows/ci.yml`'s `test` job matrix carries `windows-latest`, and a
// `#[cfg(windows)]` test can hand the script to cmd.exe and read the exit code.
// So the subject itself is asked, once, in
// `windows_boot_really_parses_under_cmd` — and the models below are what stays
// checkable on the other two operating systems, pinned to the literals they
// rest on so a model and its subject can only drift together.
//
// The stakes are why they exist at all. This pair has already shipped one
// Windows-only defect that survived six releases in total silence (the trailing
// backslash of 0.1.52, documented at the top of the `.cmd`), and both properties
// below fail the same way: nothing on screen, `bin/` never populated, the whole
// harness dormant.
// ---------------------------------------------------------------------------

/// One field of a cmd `for /f` line after `%~` expansion: surrounding double
/// quotes are dropped, and ONLY when the field carries both of them — a field
/// like `"https` (what the manifest's `$schema` line yields once `:` is a
/// delimiter) is left exactly as it is.
fn cmd_unquote(field: &str) -> &str {
    match field.strip_prefix('"').and_then(|f| f.strip_suffix('"')) {
        Some(inner) if field.len() >= 2 => inner,
        _ => field,
    }
}

/// A model of cmd's `for /f "usebackq tokens=1,2 delims=:, "` over one line:
/// cut on colon, comma or space, treat a run of them as ONE cut, ignore the
/// leading ones, and hand back the first two fields as `%~a` / `%~b`.
fn cmd_for_f_tokens(line: &str) -> (Option<&str>, Option<&str>) {
    let mut fields = line
        .split([':', ',', ' '])
        .filter(|f| !f.is_empty())
        .map(cmd_unquote);
    (fields.next(), fields.next())
}

/// The whole of what `mustard-boot.cmd` computes into `VER`: walk the manifest
/// line by line, skip what cmd's default `eol=;` would skip, and take `%~b` off
/// the FIRST line whose `%~a` is `version` (`if not defined VER` — first match
/// wins).
fn cmd_boot_version(manifest: &str) -> Option<&str> {
    manifest
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.starts_with(';'))
        .find_map(|l| {
            let (key, value) = cmd_for_f_tokens(l);
            key.filter(|k| k.eq_ignore_ascii_case("version"))
                .and(value)
        })
}

/// The version `mustard-boot.cmd` will read out of the committed manifest is the
/// version the manifest actually declares.
///
/// The failure this guards is silent and total. An empty `VER` sends the Windows
/// boot to `:noversion` and leaves `bin/` unpopulated, so after a plugin version
/// bump every hook on every Windows machine goes dormant and stays dormant. The
/// parse has no other alarm: no exit code, no missing file, nothing in the
/// session but hooks that quietly do nothing.
///
/// Two halves, so the model cannot rot away from its subject:
///
/// 1. the `.cmd` still carries the exact `for /f` line modelled above; and
/// 2. the model, run over the real `plugin/.claude-plugin/plugin.json`, agrees
///    with `serde_json` — which parses that file by a completely different road.
///
/// What it does NOT prove is that cmd.exe tokenises the way this models it; only
/// a Windows box can answer that, and the one-line command that asks lives in
/// the doc comment of `every_batch_file_carries_no_percent_sequence_cmd_will_refuse`
/// below. It may NOT live in the `.cmd`: a percent sequence in a comment there
/// aborts the whole file, which is the defect that test exists to keep shut.
/// What it does prove is the half that actually moves: the manifest's shape.
/// That shape is what a careless reformat, a new key, or a minifier in the
/// release path would change.
#[test]
fn windows_boot_reads_the_version_the_manifest_actually_ships() {
    let boot_cmd = read("plugin/bin/mustard-boot.cmd");
    let shipped_line = r#"for /f "usebackq tokens=1,2 delims=:, " %%a in ("%MANIFEST%") do if not defined VER if /i "%%~a"=="version" set "VER=%%~b""#;
    assert!(
        boot_cmd.contains(shipped_line),
        "mustard-boot.cmd no longer carries the `for /f` line this test models, \
         so the model below proves nothing about the shipped parser",
    );

    let manifest = read(MANIFEST_PATH);
    let declared: serde_json::Value =
        serde_json::from_str(&manifest).expect("plugin.json is not valid JSON");
    let declared = declared
        .get("version")
        .and_then(serde_json::Value::as_str)
        .expect("plugin.json declares no `version`");

    let parsed = cmd_boot_version(&manifest).unwrap_or_else(|| {
        panic!(
            "mustard-boot.cmd would read an EMPTY version out of plugin.json — \
             every Windows machine goes dormant after the next version bump. \
             The manifest must keep `version` alone on its own line, as \
             `  \"version\": \"{declared}\",`"
        )
    });
    assert_eq!(
        parsed, declared,
        "mustard-boot.cmd would stamp bin/ with a version the manifest does not \
         declare, so the download URL points at a release that does not exist",
    );

    // The PROMPT form of that same line lives in this file's own doc comment,
    // because a percent sequence in the `.cmd` aborts it. Nothing else ties the
    // two together: in the old layout they sat three lines apart, and the
    // adjacency WAS the anchor (found in review). Pin the half that drifts —
    // the `for /f` options — so the line an operator copy-pastes on Windows
    // cannot go on reporting a PASS for a parse the script no longer performs.
    let options = shipped_line
        .split('"')
        .nth(1)
        .unwrap_or_else(|| panic!("the shipped `for /f` line carries no quoted options"));
    // The needle is the DOC-COMMENT prefix, not the one-liner's own text: a
    // plain substring search matched the source line that performs the search.
    // `file!()` rather than the path spelled out — a literal path panics with
    // "unreadable" instead of the assertion the moment the file is renamed.
    let this_file = read(file!());
    let prompt: Vec<&str> = this_file
        .lines()
        .filter(|l| l.trim_start().starts_with("/// for /f"))
        .collect();
    assert_eq!(
        prompt.len(),
        1,
        "exactly one doc comment may carry the copy-pasteable one-liner, or this \
         anchor pins whichever came first and the real one drifts unchecked",
    );
    let prompt_line = prompt[0];
    // BOTH halves that can rot: the options decide the tokenisation, the path
    // decides whether the operator parses a file that still exists.
    for pinned in [options, &manifest_in_cmd_backslashes()] {
        assert!(
            prompt_line.contains(pinned),
            "the one-liner an operator copy-pastes on a Windows box no longer \
             carries `{pinned}`, so it would report a PASS for a parse the \
             script does not perform:\n{prompt_line}",
        );
    }
}

/// The manifest, in the ONE spelling the whole file reads it by.
const MANIFEST_PATH: &str = "plugin/.claude-plugin/plugin.json";

/// The same path as the copy-pasteable one-liner spells it, in cmd's own
/// backslashes — DERIVED, so moving the manifest cannot leave a third spelling
/// pinned to the old place (found in review).
fn manifest_in_cmd_backslashes() -> String {
    MANIFEST_PATH.replace('/', "\\")
}

/// `defaultEnabled` and `displayName` stay in the manifest, and this records WHY.
///
/// The manifest's own `$schema` points at
/// `https://json.schemastore.org/claude-code-plugin-manifest.json`, and that
/// schema defines **neither** key at the root (`displayName` appears there only
/// inside `channels`). So an editor validating this file against the schema it
/// declares flags both. Checked against the source outside this repository on
/// 2026-09-01: the official Claude Code plugin reference
/// (`code.claude.com/docs/en/plugins-reference`) documents BOTH in its metadata
/// table — `displayName` as the human-readable name shown in the `/plugin`
/// picker, `defaultEnabled` as "whether the plugin starts in an enabled state
/// when the user has not set one", defaulting to `true`. The published schema is
/// therefore behind the product; the keys are real.
///
/// That divergence is worth pinning rather than "cleaning up", because the
/// cleanup is not cosmetic: dropping `"defaultEnabled": false` does not remove a
/// field, it flips the documented default, and Mustard would auto-enable itself
/// on every marketplace installation that has never expressed a preference. A
/// schema that lags the product is not a reason to change what the product does.
#[test]
fn the_manifest_keeps_the_two_keys_its_schema_does_not_define() {
    let manifest: serde_json::Value =
        serde_json::from_str(&read(MANIFEST_PATH)).expect("plugin.json is not valid JSON");
    assert_eq!(
        manifest.get("defaultEnabled").and_then(serde_json::Value::as_bool),
        Some(false),
        "`defaultEnabled: false` is gone — the plugin now auto-enables on every \
         marketplace install that never chose. It is absent from the declared \
         $schema but documented by the product; see this test's doc comment",
    );
    assert!(
        manifest
            .get("displayName")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.is_empty()),
        "`displayName` is gone — the /plugin picker falls back to the bare \
         `name`. Also absent from the declared $schema, also documented by the \
         product",
    );
}

/// Every prose pointer to the batch-file guard names a test that EXISTS.
///
/// This class already bit once, inside this very unit: the guard was renamed,
/// two doc comments and the pull request's own validation step kept the old
/// name, and the published "run this to see it fail" filter matched zero tests
/// and exited 0 — a proof that proved nothing, on the defect whose whole story
/// is a false claim nobody could check. Cheap to pin, so pin it.
#[test]
fn the_prose_names_a_guard_that_exists() {
    const GUARD: &str = "every_batch_file_carries_no_percent_sequence_cmd_will_refuse";
    let this_file = read(file!());
    assert!(
        this_file.contains(&format!("fn {GUARD}()")),
        "the guard named all over this repository is not defined here",
    );
    for (file, body) in [
        (file!(), &this_file),
        ("plugin/bin/mustard-boot.cmd", &read("plugin/bin/mustard-boot.cmd")),
    ] {
        assert!(
            body.contains(GUARD),
            "{file} points readers at the batch-file guard without naming it, \
             so the pointer cannot be followed and cannot be checked",
        );
    }
}

/// The runner that makes the subject askable at all.
///
/// Everything this unit added rests on one line of `ci.yml`, and nothing pinned
/// it. Trim the matrix to cut runner minutes and `windows_boot_really_parses_under_cmd`
/// stops running, both changed files go back to claiming something false, and CI
/// stays green — which is the exact silence this unit exists to end.
#[test]
fn the_ci_matrix_still_carries_the_windows_runner() {
    let ci = read(".github/workflows/ci.yml");
    assert!(
        ci.contains("windows-latest"),
        "no `windows-latest` in .github/workflows/ci.yml, so nothing asks \
         cmd.exe anything and the Windows guards are decoration",
    );
}

/// Every `%~` in `body` that cmd.exe would REFUSE, as `(line, excerpt)`.
///
/// An illegal one does not misbehave — it ABORTS the file. cmd expands percent
/// sequences BEFORE it notices a line is a `rem`, so a percent-tilde written to
/// ILLUSTRATE the parser is evaluated as a reference to an argument by that
/// name. There is none; cmd prints "The following usage of the path operator in
/// batch parameter substitution is invalid", stops reading and exits 255.
/// Measured on a real Windows box, 2026-08-31: the illustrative one-liner sat in
/// the `.cmd` as a comment and killed the boot ABOVE the version read — every
/// Windows install dormant from 0.1.59 to 0.1.61, nothing on screen saying why.
///
/// Read LEFT TO RIGHT, the way cmd reads, because looking backwards cannot
/// work. The first draft asked "is the previous character a percent?", then "is
/// the run of previous percents odd?" — and both answers are wrong for
/// `%VAR%%~a`, where the percent in front of the tilde is the one CLOSING an
/// expansion, not the one escaping the tilde. The heuristic called that legal;
/// cmd aborts on it. The mirror image, `%DIR%~about`, it called illegal; cmd
/// accepts it. One left-to-right pass answers both, and it is not a heuristic:
/// it consumes the same four things cmd consumes.
///
/// - `%%` — an escaped percent. Consume both and read on.
/// - `%~…` — an argument reference, the ONLY shape that can abort. The
///   modifiers are `f d p n x s a t z`, read CASE-INSENSITIVELY (`%~DP0` is
///   legal), and a digit or `*` must close them. `%~$VAR:n`, the documented
///   PATH-search form, closes on its own digit. Nothing closes `%~b`.
/// - `%VAR%` — an expansion. Consume it WHOLE, so its closing percent can never
///   be read as the opening of the next sequence.
/// - a lone `%` — cmd leaves it alone, and so does this.
fn illegal_percent_tilde(body: &str) -> Vec<(usize, String)> {
    body.lines()
        .enumerate()
        .flat_map(|(idx, line)| {
            illegal_in_line(line).into_iter().map(move |at| {
                // The excerpt cannot outrun the line: a blind character window
                // bled three CRLF lines into one message (found in review).
                (idx + 1, line[at..].chars().take(24).collect::<String>())
            })
        })
        .collect()
}

/// Byte offsets of the percent sequences in ONE line that cmd.exe would refuse.
fn illegal_in_line(line: &str) -> Vec<usize> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        match bytes.get(i + 1) {
            Some(b'%') => i += 2,
            Some(b'~') => {
                if !closes_on_an_argument(&line[i + 2..]) {
                    out.push(i);
                }
                i += 2;
            }
            // `%1`…`%9`, `%0` and `%*` are argument references, and cmd consumes
            // them in TWO characters — they have no closing percent to look for.
            // Reading one as the opening of an expansion swallows everything up
            // to the next percent, `%~b` included (found in review).
            Some(c) if c.is_ascii_digit() || *c == b'*' => i += 2,
            _ => {
                i = match line[i + 1..].find('%') {
                    Some(rel) => i + 1 + rel + 1,
                    None => i + 1,
                };
            }
        }
    }
    out
}

/// Does the text after a `%~` name an argument cmd can actually resolve?
fn closes_on_an_argument(rest: &str) -> bool {
    const MODIFIERS: &str = "fdpnxsatz";
    let closes = |c: char| c.is_ascii_digit() || c == '*';
    if let Some(search) = rest.strip_prefix('$') {
        // The variable NAME, not "everything up to the first colon anywhere on
        // the line" — that read `%~$FOO bar:1` as a legal search form, spaces
        // and all (found in review). cmd documents this one closing on a digit.
        let name: String = search
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        return !name.is_empty()
            && matches!(
                search[name.len()..].strip_prefix(':'),
                Some(tail) if tail.starts_with(|c: char| c.is_ascii_digit())
            );
    }
    matches!(
        rest.chars().find(|c| !MODIFIERS.contains(c.to_ascii_lowercase())),
        Some(c) if closes(c)
    )
}

/// One truth table, TWO judges — and that is the whole design.
///
/// Every entry is a batch line and whether cmd.exe refuses it. The model
/// ([`illegal_percent_tilde`]) is checked against this table on every operating
/// system; on Windows, `windows_boot_really_parses_under_cmd` feeds the SAME
/// table to a real cmd.exe. Two review rounds shipped a hole each because the
/// model was only ever asserted against itself: every "cmd accepts this" label
/// was a belief. Now a belief that drifts from cmd turns the Windows leg red.
///
/// Each line here was a hole in some draft of the scanner, and a guard with
/// holes is worse than none: it says the class is closed while the exact shape
/// that cost six releases walks through.
const PERCENT_CASES: &[(&str, bool)] = &[
    // Accepted by cmd.
    (r#"set "DIR=%~dp0""#, false),
    // A COMPLETE line, and the fix is a measurement: as the bare fragment
    // `if /i "%~1"=="on"` this entry made cmd.exe refuse it — an `if` with no
    // command is a syntax error for reasons that have nothing to do with
    // percent sequences. The table's job is to name shapes, so every entry has
    // to stand alone as a batch line. Caught by the Windows leg on its first
    // run, after three review rounds had not.
    (r#"if /i "%~1"=="on" echo matched"#, false),
    (r#"set "DIR=%~DP0""#, false), // modifiers are case-insensitive
    ("echo %~$PATH:1", false),     // the documented search form
    ("for %%a in (x) do echo %%~b", false), // a FOR variable: percents doubled
    ("echo %%%%~b", false),        // two escaped pairs, then a literal tilde
    ("rem see %DIR%~about the dir", false), // an expansion, then literal text
    (r#"set "DEST=%DIR:~0,-1%""#, false), // a substring expansion
    // Refused by cmd — each one aborts the whole file.
    ("rem echo %~b", true),   // the defect this block exists for
    ("rem %%%~b", true),      // an escaped pair, then a bare `%~`
    ("echo %VAR%%~a", true),  // the CLOSING percent of an expansion
    ("echo %1 %~b", true),    // an argument reference, consumed in two chars
    ("rem %* and %~b", true), // likewise, and it swallowed the rest
    (r#"echo %~a""#, true),   // modifiers that close on no argument
    ("echo %~$PATH", true),   // the search form without its colon
    ("echo %~$PATH:x", true), // the search form closing on no digit
    ("echo %~", true),
];

/// The model agrees with the table. On Windows, so does cmd.exe.
#[test]
fn the_percent_tilde_scanner_knows_which_shapes_cmd_refuses() {
    for (line, refused) in PERCENT_CASES {
        assert_eq!(
            !illegal_percent_tilde(line).is_empty(),
            *refused,
            "the model disagrees with the table on: {line}",
        );
    }

    // The excerpt stops at the line end. A blind character window bled three
    // CRLF lines into one message, at the exact moment an operator needs it.
    assert_eq!(
        illegal_percent_tilde("rem head\r\nrem %~b tail\r\nrem next\r\n"),
        vec![(2, "%~b tail".to_string())],
    );
}

/// Every `.cmd`/`.bat` this repository TRACKS, asked of git rather than walked.
///
/// A second batch file — an installer step, a dev helper — must not arrive with
/// zero coverage of a defect class that already cost six releases. The first
/// draft hand-rolled the walk and bought three defects with it (found in
/// review): a four-name skip list that would red the suite over a developer's
/// gitignored scratch `.bat`, `is_dir()` following a symlink into a cycle, and
/// filesystem ordering that makes the same failure read differently per machine.
/// `git ls-files` has none of them — it is sorted, it never leaves the index,
/// and "what the repository ships" is precisely the question it answers.
fn tracked_batch_files() -> Vec<String> {
    let out = std::process::Command::new("git")
        .args(["ls-files", "-z", "*.cmd", "*.bat"])
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|e| panic!("could not ask git for the batch files: {e}"));
    assert!(
        out.status.success(),
        "git ls-files refused: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// No batch file in this repository carries a percent sequence cmd.exe refuses.
///
/// The scope is the FILE CLASS, not one path: `.gitattributes` already reasons
/// this way (`*.cmd text eol=crlf`), and the failure mode is identical wherever
/// it lands — silent abort, `bin/` empty, the whole harness dormant.
///
/// The one-liner that confirms the tokenisation on a real Windows box lives
/// HERE, where a percent sign is inert prose. From the repository root, at a
/// `cmd` prompt:
///
/// ```text
/// for /f "usebackq tokens=1,2 delims=:, " %a in ("plugin\.claude-plugin\plugin.json") do @if /i "%~a"=="version" @echo %~b
/// ```
///
/// It must print the manifest version and nothing else. (One percent at the
/// prompt, two inside a file — that difference is cmd, not a typo.)
#[test]
fn every_batch_file_carries_no_percent_sequence_cmd_will_refuse() {
    let files = tracked_batch_files();
    assert!(
        files.iter().any(|p| p.ends_with("plugin/bin/mustard-boot.cmd")),
        "git listed no mustard-boot.cmd, so this test is guarding nothing: {files:?}",
    );

    let mut offenders: Vec<String> = Vec::new();
    for rel in &files {
        // LOSSY, never skipped. `let Ok(body) = read_to_string(…) else
        // { continue }` turned a file re-saved in a non-UTF-8 encoding into a
        // silent pass — a guard that scans nothing while announcing the class is
        // closed (found in review). This file carries 21 em-dashes, so that is
        // one editor away.
        let Ok(bytes) = std::fs::read(repo_root().join(rel)) else {
            // A tracked path missing from the worktree is a sparse checkout or a
            // staged deletion, not a percent-sequence regression — reporting it
            // as one sends the reader to the wrong file (found in review).
            continue;
        };
        // The encodings a Windows editor actually produces, which the lossy read
        // does NOT catch: UTF-16 decodes to `%\0~\0b` with no replacement
        // character at all, and three BOM bytes make cmd choke on line 1
        // (`'\u{feff}@echo' is not recognized`). Both leave the scanner reading
        // inert prose and reporting a clean file (found in review).
        assert!(
            !bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
            "{rel} starts with a UTF-8 byte-order mark — cmd.exe refuses its \
             first line, and this scanner cannot see why",
        );
        assert!(
            !bytes.contains(&0),
            "{rel} carries NUL bytes, so it was re-saved as UTF-16 — cmd.exe \
             cannot read it, and this scanner would call it clean",
        );
        for (line, excerpt) in illegal_percent_tilde(&String::from_utf8_lossy(&bytes)) {
            offenders.push(format!("  {rel}:{line}: {excerpt}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "a batch file carries a percent sequence cmd.exe refuses, and the refusal \
         aborts the WHOLE file — every Windows machine goes dormant, in silence. \
         Write a FOR variable as `%%~x`, an argument as `%~1`, and keep \
         illustrative percent sequences out of batch files entirely:\n{}",
        offenders.join("\n"),
    );
}

/// The scanner above is a MODEL; on Windows, ask the subject.
///
/// It closes the class the scanner cannot — an unbalanced parenthesis, a bad
/// label, a line-ending regression — each of which fails the same silent way.
///
/// Three things this test does deliberately, each one a review finding on the
/// draft before it:
///
/// 1. It runs a COPY, in a temp tree carrying only the script and the manifest
///    beside it. Against `plugin/bin` the script would find a real
///    `mustard-rt.exe` (a gitignored build artifact) at `:run` and fire the live
///    `PreToolUse` hooks against this very checkout, mid-suite.
/// 2. It invokes the script DIRECTLY, not through `call` — the field
///    reproduction did, and whether an abort's 255 survives the extra hop is one
///    more thing nobody has measured.
/// 3. It MEASURES the truth table. Every `PERCENT_CASES` line goes to the same
///    cmd.exe, and its verdict has to match the one the model is checked
///    against everywhere else. Two rounds shipped a scanner hole because the
///    table was a belief; here it becomes an observation, and a wrong belief
///    turns this leg red.
/// 4. `if !cfg!(windows) { return }`, not `#[cfg(windows)]` on the item. The
///    body uses nothing Windows-specific, and gated OUT it is not even
///    type-checked on Linux or macOS — a compile error would reach only the
///    third CI leg, after the other two had gone green.
#[test]
fn windows_boot_really_parses_under_cmd() {
    if !cfg!(windows) {
        return;
    }
    // The temp tree is removed on EVERY path, failure included: the draft
    // cleaned up after the assertions, so the runs that matter — the failing
    // ones — were the runs that leaked (found in review).
    let tmp = std::env::temp_dir().join(format!(
        "mustard-boot-parse-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    ));
    let outcome = std::panic::catch_unwind(|| ask_a_real_cmd(&tmp));
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// The Windows half of the test above, so the cleanup can wrap it.
fn ask_a_real_cmd(tmp: &Path) {
    let bin = tmp.join("bin");
    for dir in [&bin, &tmp.join(".claude-plugin")] {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    }
    let copy = |from: &str, to: PathBuf| {
        std::fs::copy(repo_root().join(from), &to)
            .unwrap_or_else(|e| panic!("copying {from}: {e}"));
        to
    };
    let script = copy("plugin/bin/mustard-boot.cmd", bin.join("mustard-boot.cmd"));
    copy(
        "plugin/.claude-plugin/plugin.json",
        tmp.join(".claude-plugin").join("plugin.json"),
    );

    // `on PreToolUse` is LOAD-BEARING, not decoration: it is the argument pair
    // that trips the fetch gate (`NEED=0` for every trigger but SessionStart),
    // so the copy walks to `:run` and stops. Drop it, or pass `on
    // SessionStart`, and this test downloads a release archive on every CI run.
    let ask_cmd = |path: &Path| {
        std::process::Command::new("cmd")
            .args(["/c", &path.display().to_string(), "on", "PreToolUse"])
            .output()
            .unwrap_or_else(|e| panic!("could not spawn cmd.exe: {e}"))
    };

    // The whole truth table, measured rather than believed.
    for (idx, (line, refused)) in PERCENT_CASES.iter().enumerate() {
        let probe = bin.join(format!("case-{idx}.cmd"));
        std::fs::write(
            &probe,
            format!("@echo off\r\necho reached\r\n{line}\r\necho done\r\n"),
        )
        .unwrap_or_else(|e| panic!("writing probe {idx}: {e}"));
        assert_eq!(
            !ask_cmd(&probe).status.success(),
            *refused,
            "cmd.exe disagrees with PERCENT_CASES on: {line}",
        );
    }

    // The copy has no mustard-rt.exe beside it, so a script that parsed all the
    // way to `:run` says the binary is missing and exits 1 — the loud failure.
    // A parse abort exits 255 and never reaches that line, so the
    // message is the proof the whole file was read.
    let out = ask_cmd(&script);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.code() == Some(1) && stderr.contains("[mustard-boot]") && stderr.contains("mustard-rt"),
        "cmd.exe refused mustard-boot.cmd (exit {:?}) — the script aborts, so \
         every Mustard hook on every Windows machine does nothing, in silence.\n{stderr}",
        out.status.code(),
    );
}

/// With the binary missing, the launcher fails LOUDLY: exit 1 (a
/// non-blocking hook error the harness shows; never 2, which would block the
/// tool call) and one clear line on stderr, in the project's language, naming
/// the command that downloads the binary. It used to exit 0 in silence.
#[test]
fn boot_fails_loudly_when_the_binary_is_missing() {
    if cfg!(windows) {
        return;
    }
    let plugin = tempfile::tempdir().expect("tempdir");
    let bin = plugin.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::create_dir_all(plugin.path().join(".claude-plugin")).expect("manifest dir");
    std::fs::copy(repo_root().join("plugin/bin/mustard-boot"), bin.join("mustard-boot")).expect("copy boot");
    std::fs::copy(
        repo_root().join("plugin/.claude-plugin/plugin.json"),
        plugin.path().join(".claude-plugin").join("plugin.json"),
    )
    .expect("copy manifest");

    for (config, message) in [
        (
            r#"{"specLang":"pt-BR"}"#,
            ": os ganchos do Mustard não rodam nesta sessão. Para baixá-lo, rode: ",
        ),
        (
            r#"{"specLang":"en-US"}"#,
            ": Mustard hooks do not run in this session. To download it, run: ",
        ),
    ] {
        let project = tempfile::tempdir().expect("tempdir");
        std::fs::write(project.path().join("mustard.json"), config).expect("config");
        // `on PreToolUse`, never `on SessionStart`: only the session start may
        // download, and this test must not reach the network.
        let out = std::process::Command::new("sh")
            .arg(bin.join("mustard-boot"))
            .args(["on", "PreToolUse"])
            .env("CLAUDE_PROJECT_DIR", project.path())
            .output()
            .expect("spawn sh");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{config}: a missing binary fails, loudly: {stderr}");
        assert!(stderr.starts_with("[mustard-boot] "), "{config}: {stderr}");
        assert!(stderr.contains(message), "{config}: {stderr}");
        assert!(stderr.trim_end().ends_with("mustard-boot\" --version"), "{config}: {stderr}");
    }
}

/// Both twins say the missing binary with the same words, in both languages.
#[test]
fn both_boot_twins_name_the_missing_binary_the_same_way() {
    let posix = read("plugin/bin/mustard-boot");
    let windows = read("plugin/bin/mustard-boot.cmd");
    for fragment in [
        "[mustard-boot] o mustard-rt não está em ",
        ": os ganchos do Mustard não rodam nesta sessão. Para baixá-lo, rode: ",
        "[mustard-boot] mustard-rt is missing from ",
        ": Mustard hooks do not run in this session. To download it, run: ",
        "en-US",
    ] {
        assert!(posix.contains(fragment), "plugin/bin/mustard-boot lost {fragment:?}");
        assert!(windows.contains(fragment), "plugin/bin/mustard-boot.cmd lost {fragment:?}");
    }
    // A COMMAND that exits 2, not the digits in prose: the `.cmd` explains the
    // parse abort's `exit 255` in a comment.
    for (file, body) in [("mustard-boot", &posix), ("mustard-boot.cmd", &windows)] {
        let blocks = body.lines().map(str::trim).any(|line| {
            ["exit 2", "exit /b 2"]
                .iter()
                .any(|code| line.strip_prefix(code).is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_digit())))
        });
        assert!(!blocks, "{file} must never block a hook");
    }
}

/// Both twins cap the download at the SAME number of seconds.
///
/// The ceiling is not a free parameter: the tightest hook budget that can still
/// reach the fetch is 15 s (`plugin/hooks/hooks.json`, the
/// `clear|compact|resume|fork` arm of `SessionStart`), and unpacking plus the
/// hand-off still has to fit under it. Both files say in prose "keep this in
/// step with the twin", and prose is not a lock: a bump applied to one body only
/// is invisible, and shows up as `Hook cancelled` on exactly one operating
/// system. Another test asserts each twin has SOME deadline; this asserts they are the
/// same one, and that it fits.
#[test]
fn both_boot_twins_carry_the_same_download_deadline() {
    fn max_time(body: &str, file: &str) -> u32 {
        // Read the deadline off the COMMAND, never off the prose that explains
        // it. Both twins name `--max-time 10` in a comment several lines ABOVE
        // the `curl` that carries it, so splitting the whole body on the first
        // occurrence reads the comment — and a divergence introduced in the
        // command alone stays invisible. Found in review, 2026-08-29: with the
        // POSIX `curl` bumped to 90 and its comment left at 10, this test was
        // still green. A test that locks the prose is the exact failure its own
        // doc comment above warns about.
        let after = body
            .lines()
            .filter(|line| line.contains("curl "))
            .find_map(|line| line.split("--max-time ").nth(1))
            .unwrap_or_else(|| panic!("{file} has no `--max-time` on its curl"));
        after
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or_else(|e| panic!("{file}'s `--max-time` is not a number: {e}"))
    }

    let posix = max_time(&read("plugin/bin/mustard-boot"), "plugin/bin/mustard-boot");
    let windows = max_time(
        &read("plugin/bin/mustard-boot.cmd"),
        "plugin/bin/mustard-boot.cmd",
    );
    assert_eq!(
        posix, windows,
        "the two mustard-boot twins cap the download at different deadlines — \
         one operating system gets `Hook cancelled` and the other does not",
    );
    assert!(
        posix < 15,
        "a {posix}s download ceiling does not fit the 15s budget of the \
         tightest hook that can reach it (hooks.json, SessionStart on \
         clear|compact|resume|fork) — the unpack and the hand-off come after it",
    );
}

/// **A door does what it names — and the rule ships to every project, not just
/// to the one where it was learned.**
///
/// Measured in the field, 2026-08-31, on a repository that is not this one: a
/// bare `/mustard:pr open` produced a full test-suite run, a 328-commit drift
/// analysis, a dry-run merge in a throwaway worktree and a proposal to merge an
/// integration base into the operator's own unit — and no pull request. The
/// operator had asked for a pull request and nothing else.
///
/// Where the rule lives is the whole point of this ratchet. The door's own
/// prose (`plugin/commands/pr.md`) is read only once the door OPENS, and that
/// session never got that far — it went wide before it went anywhere. The
/// ORCHESTRATOR is the file injected at the start of every session of every
/// project, so that is the copy that has to carry it; the door carries the
/// operational half. Both, or the rule only reaches the sessions that were
/// already going to be fine.
///
/// This holds all three claims: the general rule, its two operational halves,
/// and the measurement that justifies them — a reader can check the date rather
/// than take the rule on faith.
#[test]
fn every_project_learns_that_a_door_does_what_it_names() {
    let orchestrator = mustard_core::ORCHESTRATOR_MD;

    // The general rule, in the file every session reads. It is stated ABOUT
    // doors, not about `pr open`, because the next over-reach will be at some
    // other door.
    assert!(
        orchestrator.contains("A door does what it NAMES and stops there"),
        "the injected router never says a door is bounded by its own name",
    );

    // Half one: a measurement taken for a report is not a gate. A red suite is
    // `merge`'s business; `open` states the number and publishes.
    assert!(
        orchestrator.contains("REPORTED, never investigated"),
        "the router lets a measurement taken for the body become a gate",
    );

    // Half two: the environment refusing is news, not a problem to route
    // around. Carrying an integration base into the operator's unit to make
    // someone else's failure go green is the specific move that was measured.
    assert!(
        orchestrator.contains("Never propose carrying an integration base into the operator's unit"),
        "the router leaves the 328-commit merge proposal available as an answer",
    );

    // The measurement itself, with its date — the claim above is checkable.
    assert!(
        orchestrator.contains("2026-08-31"),
        "the router asserts the door-scope rule without the field case behind it",
    );

    // The door repeats the operational half where the work actually happens, so
    // a session that DID reach the door is told there too.
    let pr_door = read("plugin/commands/pr.md");
    for claim in [
        "It does not judge, and it does not gate",
        "A red suite is REPORTED, never investigated here",
        "A push refused by the repository's own tooling is REPORTED, not routed around",
    ] {
        assert!(
            pr_door.contains(claim),
            "the `pr` door dropped `{claim}` — the operational half of the rule",
        );
    }
}

// ---------------------------------------------------------------------------
// The always-rewritten contract, and the set it is stated about
// ---------------------------------------------------------------------------

/// Where the always-rewrite contract is stated, and the sentences that state
/// it: `(file, claims)`, with `claims[0]` the always-rewrite sentence itself.
///
/// Five surfaces describe what `/mustard:upsert` does to the instruction files
/// the harness seeds — three doc comments, one door, one command reference, in
/// two languages — and until now NOTHING read any of them. The regression they
/// ratchet against is not hypothetical: a round of this work replaced the
/// always-rewrite sentence with a merge-mode one and cost two turns of
/// correction, with a green build both times, because every criterion pinned
/// the BEHAVIOUR and none pinned the prose that describes it.
///
/// `project_seed.rs` is here because that is where the ENGINE states the
/// contract, and two of the four original offenders lived in it. A ratchet that
/// reads the doors and not the engine leaves the sentence closest to the code
/// unguarded.
///
/// The file NAMES are not listed here. They come from the seed, so a new
/// injectable makes every one of these surfaces owe it a mention rather than
/// being discovered missing one surface at a time.
const REWRITE_CONTRACT_SURFACES: &[(&str, &[&str])] = &[
    (
        "MUSTARD-COMMANDS.md",
        &["toda execução regrava o texto embarcado", "`updated`", "`preserved`"],
    ),
    (
        "apps/rt/src/commands/maint/cli.rs",
        &["ALWAYS rewritten", "`updated`", "`preserved`"],
    ),
    (
        "apps/rt/src/commands/maint/upsert.rs",
        &["ALWAYS rewritten", "`Updated`", "`Preserved`"],
    ),
    (
        "packages/core/src/platform/project_seed.rs",
        &["Always rewritten", "`SeedOutcome::Updated`", "`SeedOutcome::Preserved`"],
    ),
    (
        "plugin/commands/upsert.md",
        &["every run lays the shipped text down again", "`updated`", "`preserved`"],
    ),
];

/// Sentences that state the OPPOSITE of the always-rewrite contract, forbidden
/// anywhere in a surface that describes it.
///
/// This half exists because the rest of this ratchet catches a SUBSTITUTION and
/// not an ADDITION. Measured by a reviewer: a contradicting sentence INSERTED
/// beside the surviving contract sentence left every assertion here green — the
/// required claims were still present, the names were still all named, and the
/// line rule only fires on a line that files an injectable under `merge`. A
/// reader who meets both sentences believes the one that flatters their hope,
/// which for an operator with a local edit is always the wrong one.
///
/// Matched against the surface with its whitespace collapsed, so a claim that
/// is merely hard-wrapped across two doc-comment lines is caught like any
/// other. Each phrase here is one that is NEVER true of these files — the
/// legitimate `preserved` sentences of the other seeds say `an existing file`
/// or `o que já existe`, never the injectables.
///
/// ## Why this is still a list of phrases, measured
///
/// A reviewer escaped it with a paraphrase that names no file — *"Uma cópia que
/// você editou à mão é mantida como está"* — and the obvious repair is to derive
/// the rule from a PRESERVATION VERB plus a ROUTER SUBJECT instead of listing
/// sentences. That repair is now made, for the subjects where it MEASURES
/// CLEAN: [`PRESERVATION_VERBS`] × ([`injectables`] ∪ [`PRESERVATION_SUBJECTS`]) flags
/// zero sentences across all five surfaces today and catches five paraphrases
/// the list misses (`kept as it is`, `left untouched`, `fica intacto`,
/// `são mantidos como estão`, `never overwritten`).
///
/// What it does NOT reach is the reviewer's own sentence, because its subject is
/// the generic noun `cópia`. Adding `copy`/`cópia` to [`INJECTABLE_NOUNS`] was
/// measured before being rejected: it catches that paraphrase and flags THREE
/// true sentences on a clean tree — `MUSTARD-COMMANDS.md`'s *"uma idêntica volta
/// em `preserved`, porque não havia o que escrever"*, which is the convergence
/// case and is simply true, and two dated cadavers in `project_seed.rs` (*"The
/// pair used to carry a catalog of fingerprints…"*, *"The guess errs on the
/// wrong side: a copy the catalog does not recognise is PRESERVED…"*) that
/// record the bug this contract fixed. Those three would have to be deleted or
/// exempted to keep the build green, and deleting a dated measurement to please
/// a test is the one thing this project may not do. So the generic-noun
/// paraphrase is a DECLARED LIMIT, not an oversight: detecting contradiction by
/// meaning is not text matching, and the list below stays as the cheap half.
const CONTRACT_CONTRADICTIONS: &[&str] = &[
    "an existing user file is preserved",
    "injectable is preserved",
    "injectables are preserved",
    "injetáveis são preservados",
    "never overwritten",
    "nunca sobrescrito",
    "os injetáveis são preservados",
    "user file is preserved",
    "user files are preserved",
];

/// Words that file a file under the OPERATOR's ownership — the category the
/// injectables are the exception to.
const OWNERSHIP_MARKERS: &[&str] = &["merge", "preserv", "clobber", "yours", "you own"];

/// Ways of saying a file SURVIVES an install untouched.
///
/// A verb list rather than a sentence list, because a verb list is smaller and
/// far more stable than the phrases it appears in — the point of deriving. It
/// is deliberately narrower than [`OWNERSHIP_MARKERS`]: `merge` is a word about
/// WHO OWNS a file, not about what happens to it, and it stands legitimately in
/// three sentences of `project_seed.rs` that name the injectables while
/// explaining the exception in other words (measured).
const PRESERVATION_VERBS: &[&str] = &[
    "as-is",
    "como está",
    "intact",
    "keep as",
    "kept as",
    "left alone",
    "mantid",
    "mantém",
    "never overwritten",
    "nunca sobrescrit",
    "preserv",
    "unchanged",
    "untouched",
];

/// Generic nouns that mean *a file the router seeds*, for the sentences that
/// state the contract without naming a file. The subject half of the derived
/// rule.
///
/// Narrower than the cardinality half's [`INJECTABLE_NOUNS`], and measured that
/// way: reusing that list put `sibling hook` in the subject set, which reddened
/// on `project_seed.rs`'s dated 2026-08-25 note that two siblings' context
/// *"both arrived intact"* — a measurement about hook DELIVERY, with nothing to
/// say about what an install writes. `copy`/`cópia` is deliberately absent for
/// the same kind of reason — see [`CONTRACT_CONTRADICTIONS`] for that
/// measurement.
const PRESERVATION_SUBJECTS: &[&str] = &["injectable", "injetáv", "injetav"];

/// Ways of saying the file is written again — the exception stated in other
/// words than the surface's own headline claim.
///
/// Wider than [`EXCEPTION_MARKERS`] and used ONLY by the generic-noun half,
/// because that half's subject matches identifiers like `seed_injectable_files`
/// in a doc comment: the sentences that carry them say `is replaced`,
/// `is rewritten`, `never as Preserved`, and each is the contract stated
/// correctly rather than contradicted.
const REWRITE_MARKERS: &[&str] = &[
    "always rewritten",
    "always-rewrite",
    "lays the shipped text down again",
    "never preserved",
    "not preserved",
    "nunca preservad",
    "reescrit",
    "regrava",
    "replaced",
    "replaces",
    "rewrite",
    "rewritten",
];

/// Ways of naming the exception, beyond each surface's own headline claim.
///
/// A sentence that says `Preserved` about an injectable is fine when it is the
/// sentence explaining that the always-rewrite contract is what forbids it —
/// which is how the ENGINE documents the bug it fixed. What is not fine is the
/// same sentence with no exception named at all.
const EXCEPTION_MARKERS: &[&str] =
    &["always rewritten", "always-rewrite", "regrava o texto embarcado"];

/// The prose of a surface: a markdown file whole, a Rust file's `//` comment
/// lines only.
///
/// Code is not prose. A fixture that writes `"orchestrator.md"` into a JSON
/// literal makes no claim about the contract, and reading it as one would
/// bury the sentences that do.
fn contract_prose(rel: &str, body: &str) -> String {
    if !rel.ends_with(".rs") {
        return body.to_string();
    }
    body.lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("//"))
        .map(|l| l.trim_start_matches('/').trim_start_matches('!'))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A body with every whitespace run collapsed to one space, lowercased — the
/// shape a claim takes once hard wrapping stops mattering.
fn flattened(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The basenames the seed carries, asked of the seed.
fn injectables() -> Vec<&'static str> {
    let names = mustard_core::injectable_names();
    assert!(!names.is_empty(), "the seed carries no injectable — every check below would measure nothing");
    names
}

/// Every surface that describes the install states the contract, names every
/// file it holds for, and never files them under what the operator owns.
///
/// Both halves, as everywhere in this file. The PROSE half is every surface in
/// [`REWRITE_CONTRACT_SURFACES`]; the CODE half drives `seed_injectable_files`
/// over a diverged copy and requires the outcome the prose promises — so the
/// pair can only be broken together, deliberately.
#[test]
fn every_surface_that_describes_upsert_states_the_always_rewritten_contract() {
    let names = injectables();

    for (rel, claims) in REWRITE_CONTRACT_SURFACES {
        let body = read(rel);
        for claim in *claims {
            assert!(
                body.contains(claim),
                "{rel} no longer says `{claim}` — the always-rewrite contract is stated \
                 nowhere a reader of this file arrives at, and the merge-mode regression \
                 this ratchet exists for landed here twice",
            );
        }
        for name in &names {
            assert!(
                body.contains(name),
                "{rel} describes what an install does to the harness's own instruction \
                 files and never names `{name}`, which the seed carries. A reader is told \
                 the contract for some of them and left to guess for the rest",
            );
        }
        // A line that names one of these files and talks about MERGING must be
        // the line that also states the exception. This is the exact shape the
        // regression took: the names kept, the verb swapped.
        for (n, line) in body.lines().enumerate() {
            if !names.iter().any(|name| line.contains(name)) {
                continue;
            }
            if !line.to_ascii_lowercase().contains("merge") {
                continue;
            }
            assert!(
                line.contains(claims[0]),
                "{rel}:{} files an injectable under MERGE without stating the exception \
                 on the same line: {}",
                n + 1,
                line.trim(),
            );
        }

        // The ADDITION half. Everything above passes with the contract sentence
        // intact and a contradicting one sitting next to it, which is exactly
        // what a reviewer measured. Two checks close that.
        //
        // First: a phrase that is never true of these files, anywhere in the
        // surface, wrapping ignored.
        let prose = contract_prose(rel, &body);
        let flat = flattened(&prose);
        for forbidden in CONTRACT_CONTRADICTIONS {
            assert!(
                !flat.contains(forbidden),
                "{rel} says `{forbidden}`. These files are ALWAYS rewritten, and a \
                 surface that says both is worse than one that says neither: the \
                 reader believes whichever sentence suits them, and the operator \
                 with a local edit believes the wrong one",
            );
        }

        // Second, and derived rather than listed: a SENTENCE that names an
        // injectable and files it under the operator's ownership must be the
        // sentence that also states the exception. The line rule above sees one
        // physical line; this sees the statement, however it is wrapped, and it
        // widens `merge` to every word that puts a file on the operator's side.
        for sentence in prose.split(". ") {
            let lowered = sentence.to_lowercase();
            let headline = sentence.contains(claims[0]);

            // Half one: the sentence NAMES a file the seed carries.
            if names.iter().any(|name| sentence.contains(name)) {
                let marker = OWNERSHIP_MARKERS
                    .iter()
                    .chain(PRESERVATION_VERBS)
                    .find(|m| lowered.contains(**m));
                if let Some(marker) = marker {
                    let states_exception =
                        headline || EXCEPTION_MARKERS.iter().any(|m| lowered.contains(*m));
                    assert!(
                        states_exception,
                        "{rel} names an injectable in a sentence that files it under the \
                         operator's own files (`{marker}`) and never states the exception \
                         (`{}`) in that same sentence: {}",
                        claims[0],
                        sentence.split_whitespace().collect::<Vec<_>>().join(" "),
                    );
                }
            }

            // Half two, DERIVED and file-name-free: a preservation VERB standing
            // next to a generic name for these files. This is the half the
            // phrase list could not have — a paraphrase invents its own wording,
            // and a list of sentences only ever knows the wordings already seen.
            if PRESERVATION_SUBJECTS.iter().any(|noun| lowered.contains(noun)) {
                let Some(verb) = PRESERVATION_VERBS.iter().find(|v| lowered.contains(**v)) else {
                    continue;
                };
                let states_exception =
                    headline || REWRITE_MARKERS.iter().any(|m| lowered.contains(*m));
                assert!(
                    states_exception,
                    "{rel} says an injectable is `{verb}` and never says, in that same \
                     sentence, that it is written again. These files are ALWAYS \
                     rewritten — a reader who meets this sentence keeps a local edit \
                     that the next install will silently take: {}",
                    sentence.split_whitespace().collect::<Vec<_>>().join(" "),
                );
            }
        }
    }

    // The code half. A copy that diverged is REPLACED and reported as Updated;
    // a copy already identical is Preserved because there was nothing to write.
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    let created = mustard_core::seed_injectable_files(&claude).unwrap();
    assert_eq!(created.len(), names.len(), "the seeder wrote a different set than the seed carries");
    for (name, outcome) in &created {
        assert_eq!(*outcome, mustard_core::SeedOutcome::Created, "{name} on a fresh project");
    }

    for name in &names {
        std::fs::write(claude.join("mustard").join(name), "AN OPERATOR EDIT").unwrap();
    }
    let rewritten = mustard_core::seed_injectable_files(&claude).unwrap();
    for (name, outcome) in &rewritten {
        assert_eq!(
            *outcome,
            mustard_core::SeedOutcome::Updated,
            "{name} survived an install as the operator's edit — the prose above promises \
             it is replaced, so a corrected rule now fails to reach installed projects",
        );
    }

    let settled = mustard_core::seed_injectable_files(&claude).unwrap();
    for (name, outcome) in &settled {
        assert_eq!(
            *outcome,
            mustard_core::SeedOutcome::Preserved,
            "{name} is reported as written when the shipped text was already on disk — \
             the operation must converge after one run",
        );
    }
}

// ---------------------------------------------------------------------------
// No document names SOME of the injectables
// ---------------------------------------------------------------------------

/// Where a document that enumerates the injectables can live.
///
/// `.claude/spec/` is deliberately absent: a spec is a FROZEN record of a unit
/// that shipped, and a record naming what existed then is not drift. Only the
/// delivered copies under `.claude/mustard/` are read from that tree.
const PROSE_SCAN_ROOTS: &[&str] = &[".claude/mustard", "apps", "packages", "plugin"];

/// Blocks that name SOME injectables and not all, kept deliberately.
///
/// `(file, anchor, why)`. The bar is not "it is minor": it is that the block is
/// a DATED MEASUREMENT of specific files, taken when the set was smaller.
/// Adding a name a measurement never covered would falsify it, and a measured
/// number is the one thing this project may not edit to keep a test green.
/// The sibling assertion drops a row that stops being needed.
/// A repo-relative path spelled with forward slashes on every platform.
///
/// Every exemption table in this file is written with `/`. `Path::display` uses
/// the platform separator, so comparing the two directly is a bug that can only
/// appear where the separator differs — which is exactly the platform the author
/// is not running.
fn normalise_separators(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

const SUBSET_EXEMPT_BLOCKS: &[(&str, &str, &str)] = &[
    (
        "apps/cli/tests/template_budget.rs",
        "held nothing but rules",
        "the measurement that set INJECTABLE_CHAR_CAP, taken on the rewrite that \
         introduced it and before the third channel was split off. It names the two \
         files that were measured",
    ),
    (
        "packages/core/src/platform/project_seed.rs",
        "on the theory that siblings share one ceiling",
        "the 2026-08-25 experiment, reported as it ran: two sibling hooks on one \
         event, 6,000 characters each, both intact. Re-typing the number as the \
         set grows would claim an experiment that was never performed",
    ),
    (
        "plugin/refs/mustard/router-rationale.md",
        "both arrived intact, in separate blocks",
        "the same 2026-08-25 experiment, in the ref that carries it in full. The \
         count belongs to the hooks that were REGISTERED that day, not to the \
         set of injectables the seed carries now",
    ),
    (
        "plugin/refs/mustard/router-rationale.md",
        "with zero operational tokens lost",
        "the 2026-08-20 character counts, file by file, from before the split — a \
         record of what two specific documents measured then",
    ),
];

/// Recursively collect `.rs` and `.md` files under `dir`, sorted.
fn collect_documents(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if matches!(name.to_str(), Some("target" | "node_modules" | "dist" | "build" | ".git")) {
                continue;
            }
            collect_documents(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "md")
        ) {
            out.push(path);
        }
    }
}

/// Every document the scan reads: the roots above, plus the repo-root markdown.
fn scanned_documents() -> Vec<PathBuf> {
    let root = repo_root();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    for rel in PROSE_SCAN_ROOTS {
        collect_documents(&root.join(rel), &mut out);
    }
    out
}

/// The PROSE blocks of a document: runs of `//`-comment lines in Rust, runs of
/// non-blank lines in markdown.
///
/// Code is not read. A fixture may legitimately model a project that declares
/// two of three — that is the state the doctor exists to FIND — while a comment
/// or a paragraph makes a claim about the harness, and a claim about some of
/// the injectables is a claim that goes stale the day another is seeded.
/// In markdown, the HEADING a block sits under travels with it.
///
/// A heading is a claim, and it is usually the shortest one on the page: `##
/// Why two files rather than one` stands alone between blank lines, names no
/// file and carries no subject word, so read on its own it says nothing this
/// scan can weigh — and that is exactly how a section title survived counting
/// the injectables wrong. Read with the paragraph it opens, it is a sentence of
/// that paragraph, which is how a reader takes it.
fn prose_blocks(path: &Path, body: &str) -> Vec<(usize, String)> {
    let rust = path.extension().and_then(|e| e.to_str()) == Some("rs");
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut start = 0usize;
    let mut acc: Vec<&str> = Vec::new();
    let mut heading: Option<&str> = None;
    for (n, line) in body.lines().enumerate() {
        let keep = if rust { line.trim_start().starts_with("//") } else { !line.trim().is_empty() };
        if keep {
            if acc.is_empty() {
                start = n + 1;
            }
            acc.push(line);
        } else if !acc.is_empty() {
            let block = acc.join("\n");
            let is_heading = !rust && acc.len() == 1 && acc[0].trim_start().starts_with('#');
            let carried = match heading {
                Some(h) if !rust && !is_heading => format!("{h}\n{block}"),
                _ => block,
            };
            if is_heading {
                heading = Some(acc[0]);
            }
            out.push((start, carried));
            acc.clear();
        }
    }
    if !acc.is_empty() {
        let block = acc.join("\n");
        let carried = match heading {
            Some(h) if !rust => format!("{h}\n{block}"),
            _ => block,
        };
        out.push((start, carried));
    }
    out
}

/// Brace alternation expanded: `templates/mustard/{orchestrator,dispatch,material}.md`
/// becomes the paths it stands for.
///
/// A shorthand is an enumeration. One of these named a proper subset in a
/// shipped ref and in a budget test and read as naming NONE, because the scan
/// matched basenames and a braced group spells no basename at all. A group with
/// no comma (`{slug}`, `{kind}/{slug}`) is a placeholder and is left alone, and
/// so is one carrying spaces — that is a JSON or Rust literal, not a path.
fn expand_braces(block: &str) -> String {
    let mut out = String::with_capacity(block.len());
    let mut rest = block;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inner = &after[..close];
        let tail = &after[close + 1..];
        if !inner.contains(',') || inner.contains(' ') {
            out.push('{');
            out.push_str(inner);
            out.push('}');
            rest = tail;
            continue;
        }
        let end = tail
            .find(|c: char| !(c.is_alphanumeric() || c == '.' || c == '_' || c == '-'))
            .unwrap_or(tail.len());
        for alt in inner.split(',') {
            out.push_str(alt.trim());
            out.push_str(&tail[..end]);
            out.push(' ');
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The injectables a block names, minus the document's own basename — a file
/// that calls itself "this file" has named the whole set.
fn named_in(block: &str, own: &str, names: &[&'static str]) -> Vec<&'static str> {
    let expanded = expand_braces(block);
    names
        .iter()
        .filter(|name| **name != own && expanded.contains(*name))
        .copied()
        .collect()
}

/// Count words a cardinality claim can be spelled with, in both languages this
/// repository writes in. `both`/`ambas` are `two` said without the digit.
const COUNT_WORDS: &[(&str, usize)] = &[
    ("ambas", 2),
    ("ambos", 2),
    ("both", 2),
    ("cinco", 5),
    ("dois", 2),
    ("duas", 2),
    ("five", 5),
    ("four", 4),
    ("quatro", 4),
    ("three", 3),
    ("tres", 3),
    ("três", 3),
    ("two", 2),
];

/// Nouns whose count IS the number of injectables, wherever they are counted.
const INJECTABLE_NOUNS: &[&str] = &[
    "injectable",
    "injectables",
    "injetáveis",
    "injetável",
    "sibling hook",
    "sibling hooks",
];

/// Nouns that count the injectables only inside a block that is ABOUT them.
///
/// A count of `halves` is no evidence on its own: this repository uses that
/// idiom for the two sides of one assertion roughly eighty times, and a ratchet
/// that reddened on all of them would be deleted within a week. So these are
/// read only next to a subject word — see [`SUBJECT_WINDOW`].
const ROUTER_NOUNS: &[&str] = &[
    "arquivo",
    "arquivos",
    "document",
    "documents",
    "file",
    "files",
    "half",
    "halves",
    "metade",
    "metades",
    "part",
    "parte",
    "partes",
    "parts",
];

/// Words that make a COUNT be a count of the router's injectables. Matched as
/// whole tokens, so `router-rationale` and `seed_injectable_files` both carry
/// one and `.claude/mustard/` — whose tokens are the two most common words in
/// this repository — carries none.
const ROUTER_SUBJECT: &[&str] = &[
    "injectable",
    "injectables",
    "injetáveis",
    "injetável",
    "roteador",
    "router",
    "sibling",
];

/// How far a [`ROUTER_SUBJECT`] word may sit from a generic count and still be
/// what that count is counting.
///
/// Measured, not chosen: this crate says "both halves" about the two sides of
/// one assertion roughly eighty times, and several of those paragraphs mention
/// the router somewhere else in the same doc comment. At eight tokens every one
/// of them falls out and the four live claims stay in — the nearest subject to
/// the count that ships is four tokens away.
///
/// **Widening it was tried and rejected on the measurement.** A reviewer put
/// `## Why two files rather than one` over a sentence spelling the subject NINE
/// tokens from the count and watched the suite stay green — the exact title
/// this ratchet was built for, escaping by one token. Raising the window until
/// it reached is what turns a hole into noise: at 12 it reddens a pair of true
/// sentences — one in `apps/rt/src/shared/paths.rs`, counting `dispatch.md`
/// against `dispatch.md.bak`, whose nearest subject word sits exactly 12 tokens
/// away, and one in this file's own doc comments — at 24 it reddens five, and
/// with no window at all thirteen. So the window stays where the measurement
/// put it and the HEADING is what changed — see [`stale_counts`].
const SUBJECT_WINDOW: usize = 8;

/// The stale cardinality claims a block makes: a count word qualifying a noun
/// that counts the injectables, where the count is not the number the seed
/// carries.
///
/// The subset scan reads NAMES, and skips any block naming fewer than two. That
/// is how the sharpest finding of this whole unit survived a green build: the
/// opening paragraph of the seeded rules asked why there were TWO of them while
/// naming exactly one, so nothing ever looked at it. A number is an enumeration
/// too.
///
/// `injectable` and `sibling hook` count the set wherever they appear. The
/// generic nouns — files, halves, parts — count it only with a subject word
/// inside [`SUBJECT_WINDOW`] tokens, or anywhere in the SECTION when a markdown
/// heading is one of the two.
///
/// A heading is not a sentence that happens to sit above a paragraph: it is the
/// subject line of everything under it, which is why it can name the subject
/// once and never repeat it — and why `## Why two files rather than one` reads
/// as a claim about the router even when the word *router* is only spelled four
/// sentences down. Token distance is the wrong ruler for that relation, so a
/// count in a heading is weighed against the whole section, and a count in the
/// body can take its subject from the heading. Everything else keeps the
/// measured window; see [`SUBJECT_WINDOW`] for what widening it costs.
fn stale_counts(block: &str, total: usize) -> Vec<String> {
    let lowered = expand_braces(block).to_lowercase();
    let words: Vec<&str> = lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    // `prose_blocks` prepends the heading a markdown block sits under, so it is
    // the first line when there is one. Its word span is the heading.
    let heading_len = lowered
        .lines()
        .next()
        .filter(|line| line.trim_start().starts_with('#'))
        .map(|line| {
            line.split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .count()
        })
        .unwrap_or(0);
    let has_subject = |slice: &[&str]| slice.iter().any(|w| ROUTER_SUBJECT.contains(w));
    let subject_near = |i: usize| {
        if i < heading_len {
            // The count is IN the title. What the title is about is the section.
            return has_subject(&words);
        }
        let lo = i.saturating_sub(SUBJECT_WINDOW);
        let hi = (i + SUBJECT_WINDOW + 1).min(words.len());
        has_subject(&words[lo..hi]) || has_subject(&words[..heading_len])
    };

    let mut out = Vec::new();
    for (i, word) in words.iter().enumerate() {
        let Some((_, count)) = COUNT_WORDS.iter().find(|(w, _)| w == word) else {
            continue;
        };
        if *count == total {
            continue;
        }
        for span in 1..=2usize {
            if i + span >= words.len() {
                break;
            }
            let noun = words[i + 1..=i + span].join(" ");
            let counts_injectables = INJECTABLE_NOUNS.contains(&noun.as_str())
                || (ROUTER_NOUNS.contains(&noun.as_str()) && subject_near(i));
            if counts_injectables {
                out.push(format!("{word} {noun}"));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// No prose block names — or COUNTS — some of the injectables the seed carries.
///
/// The class, not a list of files. Three places named two of three at once —
/// two doc blocks in the seeding engine and the ratchet that proves each
/// injectable rides its own hook — and each was found by a person reading, one
/// at a time, months apart. Deriving the set from the seed means the fourth
/// injectable reddens every place left behind on the day it is added, instead
/// of being discovered the same way.
///
/// The NUMBER half was added after the name half shipped and missed the
/// sharpest instance of all. A block naming fewer than two files was skipped,
/// so the first paragraph a router reader ever meets — which named one file and
/// counted them as two — passed a green build inside the very unit that closed
/// twenty-seven divergences of exactly that kind. The scan now reads the count
/// as well as the names, expands a braced shorthand into the paths it stands
/// for, and reads a markdown heading as part of the section it opens.
#[test]
fn no_prose_block_names_a_proper_subset_of_the_injectables() {
    let names = injectables();
    let root = repo_root();

    let mut offenders = Vec::new();
    for path in scanned_documents() {
        let Ok(body) = std::fs::read_to_string(&path) else { continue };
        let own = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
        // Forward slashes ALWAYS: `SUBSET_EXEMPT_BLOCKS` is written with them, and
        // `Display` for a Windows path yields `\\`, so the `ends_with` below matched
        // nothing there and every exemption silently stopped excusing its block.
        // Measured on CI 2026-09-02: green on ubuntu and macos, red on windows with
        // five offenders that are all exempted rows — this ratchet was itself the
        // "green where it is written, red where nobody is looking" it exists to catch.
        let rel = normalise_separators(path.strip_prefix(&root).unwrap_or(&path));
        let required = names.iter().filter(|n| **n != own).count();
        for (line, block) in prose_blocks(&path, &body) {
            if SUBSET_EXEMPT_BLOCKS
                .iter()
                .any(|(file, anchor, _)| rel.ends_with(file) && block.contains(anchor))
            {
                continue;
            }
            // Half one: the NAMES. A block naming some and not all.
            let named = named_in(&block, own, &names);
            if named.len() >= 2 && named.len() != required {
                let missing: Vec<&str> = names
                    .iter()
                    .filter(|n| **n != own && !named.contains(n))
                    .copied()
                    .collect();
                offenders.push(format!(
                    "{rel}:{line} names {named:?} and not {missing:?}. Every injectable the \
                     seed carries takes the same rule, so a document that lists some of them \
                     tells a reader the set is smaller than it is — say it about the SET, or \
                     name them all"
                ));
            }
            // Half two: the NUMBER. A block that counts them, whether it names
            // two of them, one, or none at all.
            for claim in stale_counts(&block, names.len()) {
                offenders.push(format!(
                    "{rel}:{line} claims `{claim}` and the seed carries {}. A count is an \
                     enumeration: it goes stale the day another injectable is seeded, and \
                     the reader it misleads is the one who never opens the seed. Say it \
                     about the SET, or carry the real number",
                    names.len(),
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "prose that enumerates a proper subset of the injectables:\n{}",
        offenders.join("\n"),
    );
}

/// Every subset exemption is still there, still partial, and still sorted.
#[test]
fn subset_exemptions_stay_sorted_present_and_necessary() {
    let names = injectables();
    let root = repo_root();

    for pair in SUBSET_EXEMPT_BLOCKS.windows(2) {
        assert!(
            (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1),
            "SUBSET_EXEMPT_BLOCKS must stay sorted: {} before {}",
            pair[0].0,
            pair[1].0,
        );
    }
    for (file, anchor, why) in SUBSET_EXEMPT_BLOCKS {
        assert!(!why.trim().is_empty(), "SUBSET_EXEMPT_BLOCKS entry {file} carries no justification");
        let path = root.join(file);
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("exempted document {file} is unreadable: {e}"));
        let own = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
        let required = names.iter().filter(|n| **n != own).count();
        // Still an offender of EITHER kind — a dated measurement usually names
        // the files it measured AND counts them, and a row that stops earning
        // its place on both counts is a row that hides nothing any more.
        let still_partial = prose_blocks(&path, &body).into_iter().any(|(_, block)| {
            if !block.contains(*anchor) {
                return false;
            }
            let named = named_in(&block, own, &names);
            (named.len() >= 2 && named.len() != required)
                || !stale_counts(&block, names.len()).is_empty()
        });
        assert!(
            still_partial,
            "SUBSET_EXEMPT_BLOCKS names {file} at `{anchor}`, and no block there names or \
             counts a proper subset any more — drop the row, there is nothing left to excuse",
        );
    }
}

/// A path comparison that depends on the platform separator is green where it is
/// written and red where nobody is looking. This pins the normaliser rather than
/// the call site, so a new exemption table gets the same guarantee for free.
#[test]
fn exemption_paths_are_matched_without_a_platform_separator() {
    let windows = Path::new(r"apps\cli\tests\template_budget.rs");
    let unix = Path::new("apps/cli/tests/template_budget.rs");
    assert_eq!(normalise_separators(windows), normalise_separators(unix));
    let spelled = SUBSET_EXEMPT_BLOCKS[0].0;
    assert!(
        normalise_separators(windows).ends_with(spelled),
        "the exemption table is written with `/` and the lookup must reach it from either platform",
    );
}

