//! `mustard-rt run close [--spec <nome>]` — o fechamento de uma spec.
//!
//! É a porta única do fechamento, e uma chamada só: grava o que voltou da
//! última rodada, confere se a obra terminou mesmo, roda cada critério uma vez
//! e grava a execução de cada um, e então fecha — grava a fase `closed`, que
//! arma a cobrança das pendências pela mesma porta, solta a spec da sessão e
//! refaz a página. A pasta de uma spec fechada fica com exatamente três
//! arquivos: o de eventos, o `.md` e a página.
//!
//! **O que trava.** Onda sem commit; onda cuja última revisão foi reprovada;
//! pedido do usuário que nenhuma onda entregou; e critério cuja prova não
//! passou. Cada recusa diz qual onda refazer — não basta os testes passarem.
//!
//! O fechamento não chama a função antiga de fechar, que grava arquivos do
//! formato velho: ela ficou onde estava, e a fase `closed` passa a sair só por
//! aqui.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::commands::spec_events::{self, read::checkout, write::record};
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// As opções de `mustard-rt run close`.
pub struct CloseOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec que fecha; sem ela, a spec atual.
    pub spec: Option<String>,
    /// O relatório da última rodada, em JSON, no mesmo formato da rodada.
    pub report: Option<String>,
}

/// Por que a spec não fechou.
enum CloseRefusal {
    /// Uma recusa do arquivo de eventos.
    Refused(Refusal),
    /// O relatório da última rodada não se entende.
    BadReport { detail: String },
    /// A spec não está em execução.
    NotRunning { phase: String },
    /// Uma onda que não tem commit nenhum.
    WaveWithoutCommit { wave: u64 },
    /// Uma onda cuja última revisão reprovou.
    WaveRejected { wave: u64 },
    /// Um pedido do usuário que nenhuma onda entregou.
    RequestNotDelivered { code: String },
    /// Um critério cuja prova não passou.
    CriterionFailed { code: String, output: String },
}

impl CloseRefusal {
    fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::BadReport { .. } => "close-bad-report".into(),
            Self::NotRunning { .. } => "close-not-running".into(),
            Self::WaveWithoutCommit { .. } => "wave-without-commit".into(),
            Self::WaveRejected { .. } => "wave-rejected".into(),
            Self::RequestNotDelivered { .. } => "request-not-delivered".into(),
            Self::CriterionFailed { .. } => "criterion-failed".into(),
        }
    }

    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::Refused(refusal) => refusal.message(lang),
            Self::BadReport { detail } => fill("close.bad_report", &[("{detail}", detail.clone())]),
            Self::NotRunning { phase } => fill("close.not_running", &[("{phase}", phase.clone())]),
            Self::WaveWithoutCommit { wave } => {
                fill("close.wave_without_commit", &[("{wave}", wave.to_string())])
            }
            Self::WaveRejected { wave } => fill("close.wave_rejected", &[("{wave}", wave.to_string())]),
            Self::RequestNotDelivered { code } => {
                fill("close.request_not_delivered", &[("{code}", code.clone())])
            }
            Self::CriterionFailed { code, output } => {
                fill("close.criterion_failed", &[("{code}", code.clone()), ("{output}", output.clone())])
            }
        }
    }

    fn to_value(&self, lang: Locale) -> Value {
        json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) })
    }
}

/// O núcleo testável de [`run_cmd`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn close_at(opts: &CloseOpts) -> Value {
    close_for(opts, session_from_env().as_deref())
}

/// [`close_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn close_for(opts: &CloseOpts, session: Option<&str>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    match run_close(opts, &project.root, lang, session) {
        Ok(report) => report,
        Err(refusal) => refusal.to_value(lang),
    }
}

fn run_close(
    opts: &CloseOpts,
    root: &Path,
    lang: Locale,
    session: Option<&str>,
) -> Result<Value, CloseRefusal> {
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root))
            .active(session)
            .ok_or(CloseRefusal::Refused(Refusal::NoCurrentSpec))?,
    };
    let path = store::spec_file(root, &spec).map_err(CloseRefusal::Refused)?;
    let read = |path: &Path| -> Result<SpecLog, CloseRefusal> {
        store::read(path)
            .map_err(CloseRefusal::Refused)?
            .ok_or_else(|| CloseRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))
    };
    let log = read(&path)?;

    let phase = State::from_log(&log).phase.unwrap_or_default().to_string();
    if phase != "running" {
        return Err(CloseRefusal::NotRunning { phase });
    }

    // O que voltou da última rodada entra antes das conferências: é ele que
    // fecha a última onda.
    let mut recorded: Vec<Value> = Vec::new();
    if let Some(raw) = opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        let reports = crate::commands::flow::round::parse_report(raw)
            .map_err(|detail| CloseRefusal::BadReport { detail })?;
        recorded = crate::commands::flow::round::record_reports(&opts.root, &spec, &reports)
            .map_err(CloseRefusal::Refused)?;
    }

    let log = read(&path)?;
    finished(&log)?;

    // Cada critério roda uma vez, e cada execução é gravada.
    let codes = log.codes();
    let criteria: Vec<(u64, String, String)> = log
        .block(BlockQuery::Block(Block::Criteria))
        .into_iter()
        .filter(|e| e.event_type == "criterion")
        .filter_map(|e| {
            let proof = e.str_field("proof")?.trim().to_string();
            Some((e.id, codes.get(&e.id).cloned().unwrap_or_else(|| e.id.to_string()), proof))
        })
        .collect();
    let mut runs: Vec<Value> = Vec::new();
    let mut failed: Option<CloseRefusal> = None;
    for (id, code, proof) in &criteria {
        let out = crate::commands::review::qa_run::run_proof(proof, root);
        let mut draft = Map::new();
        draft.insert("criterion".into(), json!(id));
        draft.insert("result".into(), json!(out.result));
        draft.insert("exit".into(), json!(out.exit));
        draft.insert("ms".into(), json!(out.ms));
        draft.insert("author".into(), json!("binary"));
        if !out.output.trim().is_empty() {
            draft.insert("output".into(), json!(out.output));
        }
        record(&opts.root, &spec, "criterion_run", draft, PhaseWriter::Binary)
            .map_err(CloseRefusal::Refused)?;
        runs.push(json!({ "criterion": code, "result": out.result, "exit": out.exit, "ms": out.ms }));
        if out.result != "pass" && failed.is_none() {
            failed = Some(CloseRefusal::CriterionFailed { code: code.clone(), output: out.output.clone() });
        }
    }
    if let Some(refusal) = failed {
        return Err(refusal);
    }

    // A fase `closed` sai só por aqui, e é a mesma porta que arma a cobrança
    // das pendências. A função antiga de fechar, que grava arquivos do formato
    // velho, não é chamada.
    crate::commands::spec_events::write::record_phase(&opts.root, &spec, "closed", session);
    if let Some(sid) = session {
        crate::shared::context::session::unbind_session_spec(&opts.root.to_string_lossy(), sid);
    }
    let pages = crate::commands::spec_events::pages::refresh(root, &spec, lang);

    // O pull request é o passo seguinte, e a linha dele sai pronta, com a base
    // e a branch tiradas do estado — pela mesma tabela que a retomada usa.
    let command = crate::commands::flow::resume::next_command("closed", &spec, &State::from_log(&read(&path)?));

    let mut out = json!({
        "ok": true,
        "spec": spec,
        "phase": "closed",
        "recorded": recorded,
        "criteria": runs,
    });
    if let Ok(pages) = &pages {
        out["md"] = json!(pages.md);
        out["html"] = json!(pages.html);
    }
    // O fechamento é um marco: manda publicar, menos com item retido ou com a
    // página que não pôde ser refeita, que esperam — e o pull request espera
    // a publicação.
    let then = match command.as_str() {
        Some(line) => translate("close.next", lang).replace("{command}", line),
        None => translate("resume.next.closed", lang).to_string(),
    };
    crate::commands::spec_events::pages::end_milestone(
        &mut out,
        pages.as_ref(),
        "close",
        &then,
        &crate::commands::spec_events::pages::after_purge("close", &then, lang),
        lang,
    );
    if !command.is_null() {
        out["command"] = command;
    }
    Ok(out)
}

/// A obra terminou? Recusa enquanto houver onda sem commit, onda cuja última
/// revisão foi reprovada ou pedido do usuário que nenhuma onda entregou. Não
/// basta os testes passarem.
fn finished(log: &SpecLog) -> Result<(), CloseRefusal> {
    let waves: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "wave")
        .filter_map(SpecEvent::wave)
        .collect();

    // A última revisão de cada onda: é a que vale.
    let mut last: BTreeMap<u64, &str> = BTreeMap::new();
    for verdict in log.block(BlockQuery::Block(Block::Review)).into_iter().filter(|e| e.event_type == "verdict") {
        if let (Some(n), Some(result)) = (verdict.wave(), verdict.str_field("result")) {
            last.insert(n, result);
        }
    }
    for (wave, result) in &last {
        if *result == "rejected" {
            return Err(CloseRefusal::WaveRejected { wave: *wave });
        }
    }

    let committed: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .filter(|e| e.event_type == "commit")
        .flat_map(|e| e.ints("waves"))
        .collect();
    for wave in &waves {
        if !committed.contains(wave) {
            return Err(CloseRefusal::WaveWithoutCommit { wave: *wave });
        }
    }

    // Um pedido do usuário que chegou depois da última entrega não foi
    // entregue por onda nenhuma.
    let last_delivered = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "delivered")
        .map(|e| e.id)
        .max()
        .unwrap_or(0);
    let codes = log.codes();
    for request in log.block(BlockQuery::Block(Block::Notes)).into_iter().filter(|e| e.event_type == "request") {
        if request.id > last_delivered {
            return Err(CloseRefusal::RequestNotDelivered {
                code: codes.get(&request.id).cloned().unwrap_or_else(|| request.id.to_string()),
            });
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::flow::round::{round_for, RoundOpts};
    use crate::commands::spec_events::write::{record_open, seed_at, WriteOpts};
    use std::process::Command;
    use tempfile::tempdir;

    fn write(root: &Path, spec: &str, event_type: &str, body: Value) -> Value {
        seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            event_type: event_type.into(),
            json: body.to_string(),
        })
    }

    fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {report}"))
    }

    fn git_at(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Uma spec de uma onda, já aprovada, despachada, entregue, revisada e
    /// comitada: pronta para fechar. Ela ganha um critério por comando de
    /// `proofs`, na ordem em que eles vêm.
    fn ready_to_close(root: &Path, spec: &str, proofs: &[&str]) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n").unwrap();
        git_at(root, &["init", "-q"]);
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "semente"]);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        let crits: Vec<u64> = proofs
            .iter()
            .map(|proof| {
                id_of(&write(root, spec, "criterion",
                    json!({"when": format!("a onda roda e prova com {proof}"), "then": "a suíte passa",
                           "proof": proof, "origin": said})))
            })
            .collect();
        write(root, spec, "wave", json!({"n": 1, "text": "Onda 1.", "criteria": crits,
            "done_when": "A suíte passa.", "origin": said}));
        write(root, spec, "task", json!({"wave": 1, "text": "Tarefa.", "files": [{"path": "src/a.rs"}], "origin": said}));
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));

        let round = |report: Option<String>| {
            round_for(
                &RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report },
                None,
            )
        };
        assert_eq!(round(None)["ok"], json!(true));
        std::fs::write(root.join("src/a.rs"), "fn um() {}\nfn dois() {}\n").unwrap();
        let checked: Vec<Value> =
            crits.iter().map(|id| json!({"criterion": id, "tests_rule": "confere a regra"})).collect();
        let report = json!({"waves": [{"wave": 1, "delivered": "Saiu.", "files": ["src/a.rs"],
            "verdict": {"result": "approved", "text": "passou", "criteria": checked}}],
            "commit": {"title": "feat(onda-1): a soma sai", "body": "A onda 1."}});
        assert_eq!(round(Some(report.to_string()))["ok"], json!(true));
    }

    fn close(root: &Path, spec: &str) -> Value {
        close_for(&CloseOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: None }, None)
    }

    /// O fechamento roda cada critério uma vez — os dois critérios da spec
    /// aparecem, cada um com uma execução —, grava cada execução, grava a fase
    /// fechada e deixa a pasta da spec com exatamente três arquivos.
    #[test]
    fn closing_runs_each_criterion_once_and_leaves_three_files_in_the_folder() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version", "git --help"]);

        let out = close(root, "x");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");
        assert_eq!(out["criteria"].as_array().map(Vec::len), Some(2), "os dois critérios rodaram: {out}");

        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let criteria: Vec<u64> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        assert_eq!(criteria.len(), 2, "a montagem tem dois critérios");
        let runs: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion_run").collect();
        let ran: Vec<u64> = runs.iter().filter_map(|e| e.int("criterion")).collect();
        assert_eq!(ran, criteria, "cada critério roda, e uma vez só");
        assert!(runs.iter().all(|e| e.str_field("result") == Some("pass")), "{runs:?}");
        assert_eq!(State::from_log(&log).phase, Some("closed"));

        let folder = root.join(".claude").join("spec").join("x");
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names, ["spec.html", "spec.md", "spec.ndjson"], "a pasta fechada tem 3 arquivos");
    }

    /// A resposta da rodada e a do fechamento mandam publicar a página da
    /// spec e a do projeto e dizem como gravar as duas publicações, e nenhuma
    /// delas traz o endereço da página para a conversa. A do plano prova o
    /// mesmo no teste dela. Um passo comum — o levantamento, a gravação de um
    /// item, a página refeita a pedido — não manda publicar.
    #[test]
    fn the_round_and_the_close_order_the_publish_and_never_carry_a_link() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);

        let rounded = crate::commands::flow::round::round_for(
            &crate::commands::flow::round::RoundOpts {
                root: root.to_path_buf(),
                spec: Some("x".into()),
                report: None,
            },
            None,
        );
        let closed = close(root, "x");

        for (report, milestone) in [(&rounded, "round"), (&closed, "close")] {
            assert_eq!(report["publish"], json!(["spec", "project"]), "{report}");
            let next = report["next"].as_str().unwrap_or_default();
            for page in ["spec", "project"] {
                let record = format!(r#"'{{"page":"{page}","milestone":"{milestone}","#);
                assert!(next.contains(&record), "{milestone} says how to record the {page} page: {next}");
            }
            let shown = report.to_string();
            assert!(!shown.contains("http"), "nenhum endereço na resposta: {shown}");
        }

        // Os passos comuns, numa spec em levantamento.
        let other = tempdir().unwrap();
        let root = other.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "y", "feature/y", "dev"), Ok(true));
        let said = write(root, "y", "message", json!({"author": "user", "text": "Quero a busca de lições."}));
        let context = write(root, "y", "context", json!({"text": "Quero a busca de lições.", "origin": id_of(&said)}));
        let grilled = crate::commands::flow::grill::grill_for(
            &crate::commands::flow::grill::GrillOpts {
                root: root.to_path_buf(),
                spec: Some("y".into()),
                kinds: Some("feature".into()),
                condensed: false,
            },
            None,
        );
        assert_eq!(grilled["ok"], json!(true), "{grilled}");
        let paged = crate::commands::spec::page::build(&crate::commands::spec::page::PageOpts {
            root: root.to_path_buf(),
            spec: Some("y".into()),
            body: None,
            out: None,
            title: None,
            subtitle: None,
            kind: None,
        });
        assert_eq!(paged["ok"], json!(true), "{paged}");
        for report in [&grilled, &context, &paged] {
            assert!(report.get("publish").is_none(), "um passo comum não manda publicar: {report}");
            let shown = report.to_string();
            assert!(!shown.contains("write publish"), "um passo comum não manda publicar: {shown}");
        }
    }

    /// O fim do `next` de cada marco, com o comando que a resposta devolve: a
    /// rodada com tudo entregue e aprovado manda fechar, e o fechamento manda
    /// abrir o pull request.
    fn then_of(report: &Value, key: &str) -> String {
        let command = report["command"].as_str().unwrap_or_else(|| panic!("sem comando: {report}"));
        translate(key, Locale::PtBr).replace("{command}", command)
    }

    /// Com um item de texto que parece senha, a rodada e o fechamento dizem o
    /// código do item a expurgar e não mandam publicar; a rodada continua
    /// dizendo o próximo passo, e o fechamento deixa o pull request para
    /// depois da publicação. O `.html` local sai sem o texto do item.
    #[test]
    fn a_withheld_item_holds_the_publish_of_the_round_and_the_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = write(root, "x", "message", json!({"author": "user", "text": "anota"}));
        let note = write(root, "x", "note",
            json!({"text": "GITHUB_TOKEN=a1b2c3d4e5f6g7h8i9j0", "keys": ["token"], "origin": id_of(&said)}));
        let code = note["code"].as_str().unwrap_or_default().to_string();

        let rounded = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let closed = close(root, "x");
        for (report, milestone, then) in
            [(&rounded, "round", then_of(&rounded, "round.close")), (&closed, "close", then_of(&closed, "close.next"))]
        {
            assert_eq!(report["ok"], json!(true), "{report}");
            assert!(report.get("publish").is_none(), "{milestone}: {report}");
            assert_eq!(report["withheld"], json!([code]), "{milestone}: {report}");
            let next = report["next"].as_str().unwrap_or_default();
            assert!(next.contains(&code) && next.contains("write purge"), "{milestone}: {next}");
            assert!(next.contains(&format!("`{milestone}`")) && next.ends_with(&then), "{milestone}: {next}");
            assert!(!next.contains("write publish"), "{milestone}: {next}");
            let warned = report["warnings"].as_array().cloned().unwrap_or_default();
            assert!(warned.iter().any(|w| w["hint"].as_str().unwrap_or_default().contains(&code)), "{report}");
        }
        let html = std::fs::read_to_string(root.join(".claude/spec/x/spec.html")).unwrap();
        assert!(!html.contains("a1b2c3d4e5f6g7h8i9j0"), "the local page keeps the secret out");
    }

    /// Quando a página não pode ser refeita, a rodada e o fechamento não
    /// mandam publicar a que ficou no disco: dizem o motivo nos avisos,
    /// mandam refazer a página antes de publicar e seguem com o próximo passo.
    #[test]
    fn a_page_that_could_not_be_rebuilt_is_never_ordered_to_be_published() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        // Uma pasta no lugar da página impede de gravá-la.
        let page = root.join(".claude/spec/x/spec.html");
        std::fs::remove_file(&page).unwrap();
        std::fs::create_dir(&page).unwrap();

        let rounded = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let closed = close(root, "x");
        for (report, milestone, then) in
            [(&rounded, "round", then_of(&rounded, "round.close")), (&closed, "close", then_of(&closed, "close.next"))]
        {
            assert_eq!(report["ok"], json!(true), "{report}");
            assert!(report.get("publish").is_none(), "{milestone}: {report}");
            let next = report["next"].as_str().unwrap_or_default();
            assert!(!next.contains("write publish"), "{milestone}: {next}");
            assert!(next.starts_with(translate("page.not_rebuilt", Locale::PtBr)), "{milestone}: {next}");
            assert!(next.contains("run page --spec") && next.contains(&format!("`{milestone}`")), "{milestone}: {next}");
            assert!(next.ends_with(&then), "{milestone}: {next}");
            let warned = report["warnings"].as_array().cloned().unwrap_or_default();
            assert!(warned.iter().any(|w| w["reason"] == json!("io-failed")), "{milestone} gives the reason: {report}");
        }
    }

    /// O fechamento devolve a linha inteira do pull request, com a base e a
    /// branch da spec, o binário a aceita, e o próximo passo em palavras a
    /// traz.
    #[test]
    fn closing_answers_the_whole_pull_request_line() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);

        let out = close(root, "x");
        assert_eq!(out["ok"], json!(true), "{out}");
        let line = "mustard-rt run pr-open --base dev --head feature/x --spec x";
        assert_eq!(out["command"], json!(line), "{out}");
        crate::commands::flow::resume::assert_parses(line);
        assert!(out["next"].as_str().unwrap_or_default().ends_with(&then_of(&out, "close.next")), "{out}");
    }

    /// Ao gravar a fase fechada, o fechamento arma a cobrança das pendências
    /// pela mesma porta que grava a fase.
    #[test]
    fn closing_arms_the_charge_of_the_pending_items() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        assert_eq!(close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None },
            Some("s-fecha"),
        )["ok"], json!(true));
        let armed = crate::commands::event::pending::armed_charges(root);
        assert!(armed.iter().any(|charge| charge.spec == "x"), "a cobrança ficou armada: {armed:?}");
    }

    /// O fechamento recusa enquanto houver onda sem commit, onda cuja última
    /// revisão foi reprovada ou pedido do usuário que nenhuma onda entregou, e
    /// diz qual onda refazer.
    #[test]
    fn closing_is_refused_while_the_work_is_not_finished() {
        // Onda sem commit.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "mais uma"})));
        let crit = id_of(&write(root, "x", "criterion",
            json!({"when": "a onda roda", "then": "passa", "proof": "git --version", "origin": said})));
        write(root, "x", "wave", json!({"n": 2, "text": "Onda 2.", "criteria": [crit],
            "done_when": "passa", "origin": said}));
        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("wave-without-commit"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains('2'), "{refused}");

        // Onda cuja última revisão reprovou.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        crate::shared::spec_state::seed_verdict(root, "x", 1, "rejected", crit);
        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains('1'), "{refused}");

        // Pedido do usuário que nenhuma onda entregou.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        crate::shared::spec_state::seed_request(root, "x", "Quero também a barra de status.");
        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("request-not-delivered"), "{refused}");
    }

    /// Um critério cuja prova não passa trava o fechamento, e a execução dele
    /// fica gravada assim mesmo: é o registro de que ele rodou.
    #[test]
    fn a_criterion_whose_proof_fails_blocks_the_close_and_stays_on_the_record() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --nao-existe-esta-opcao"]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-failed"), "{refused}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let runs: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "criterion_run").collect();
        assert_eq!(runs.len(), 1, "a execução fica gravada");
        assert_eq!(runs[0].str_field("result"), Some("fail"));
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }
}
