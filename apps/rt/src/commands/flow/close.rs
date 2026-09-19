//! `mustard-rt run close [--spec <nome>]` — o fechamento de uma spec.
//!
//! É a porta única do fechamento: grava o que voltou da última rodada,
//! confere se a obra terminou mesmo, roda o lint do projeto inteiro (o
//! `lintCommand` do `mustard.json`), roda cada critério uma vez e grava a
//! execução de cada um, e então fecha — grava a fase `closed`, que arma a
//! cobrança das pendências pela mesma porta, solta a spec da sessão e prepara
//! a cópia para o banco de dados da página. A pasta de uma spec fechada fica
//! com o arquivo de eventos e a pasta da cópia (`copy/`), e nenhuma página.
//!
//! **O agente de teste dedicado.** Nenhuma spec fecha sem ele — nem a de uma
//! onda só —, e a rodada não pede a revisão de onda nenhuma: com a máquina
//! verde, o fechamento devolve o pedido dele, com as entregas da obra, os
//! critérios, as mudanças da branch, as emendas gravadas entre as ondas e o
//! que cada onda deixou aberto, e só fecha — e só então devolve o pull
//! request — quando a linha dele volta aprovada, pelo mesmo relatório.
//! Enquanto nada muda depois da máquina verde, a volta não roda o lint nem os
//! critérios de novo. A reprovação fica na onda que ele apontou, que volta
//! como conserto pela rodada; entregue o conserto, o fechamento pede o mesmo
//! agente de novo, mas só para conferir o conserto, não a obra inteira. Em
//! até duas voltas de conserto sem aprovar, a onda para e a decisão passa a
//! ser do usuário — o mesmo limite de qualquer conserto.
//!
//! **O que trava.** Onda sem commit; onda cuja última revisão reprovou e
//! ainda não recebeu o conserto; pedido do usuário que nenhuma onda entregou;
//! o lint que falha, com a saída dele; critério cuja prova não passou; e
//! critério cuja prova saiu verde sem rodar teste nenhum — a saída do
//! executor diz zero teste, que é o que um nome de teste errado dá, e a
//! recusa traz o comando e o número que ela leu. Cada recusa diz qual onda
//! refazer — não basta os testes passarem.
//!
//! O fechamento não chama a função antiga de fechar, que grava arquivos do
//! formato velho: ela ficou onde estava, e a fase `closed` passa a sair só por
//! aqui.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecLog};
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
    /// O relatório da última rodada foi recusado pela mesma porta da rodada.
    Report(crate::commands::flow::round::RoundRefusal),
    /// A spec não está em execução.
    NotRunning { phase: String },
    /// Uma onda que não tem commit nenhum.
    WaveWithoutCommit { wave: u64 },
    /// Uma onda cuja última revisão reprovou.
    WaveRejected { wave: u64 },
    /// Um pedido do usuário que nenhuma onda entregou.
    RequestNotDelivered { code: String },
    /// O lint do projeto falhou.
    LintFailed { command: String, output: String },
    /// Um critério cuja prova não passou.
    CriterionFailed { code: String, output: String },
    /// Um critério cuja prova saiu verde sem rodar teste nenhum, com o
    /// comando dela e o número de testes que a saída dele disse.
    CriterionRanNoTest { code: String, command: String, tests: u64 },
}

impl CloseRefusal {
    fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::Report(refusal) => refusal.reason(),
            Self::NotRunning { .. } => "close-not-running".into(),
            Self::WaveWithoutCommit { .. } => "wave-without-commit".into(),
            Self::WaveRejected { .. } => "wave-rejected".into(),
            Self::RequestNotDelivered { .. } => "request-not-delivered".into(),
            Self::LintFailed { .. } => "lint-failed".into(),
            Self::CriterionFailed { .. } => "criterion-failed".into(),
            Self::CriterionRanNoTest { .. } => "criterion-ran-no-test".into(),
        }
    }

    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::Refused(refusal) => refusal.message(lang),
            Self::Report(refusal) => refusal.message(lang),
            Self::NotRunning { phase } => fill("close.not_running", &[("{phase}", phase.clone())]),
            Self::WaveWithoutCommit { wave } => {
                fill("close.wave_without_commit", &[("{wave}", wave.to_string())])
            }
            Self::WaveRejected { wave } => fill("close.wave_rejected", &[("{wave}", wave.to_string())]),
            Self::RequestNotDelivered { code } => {
                fill("close.request_not_delivered", &[("{code}", code.clone())])
            }
            Self::LintFailed { command, output } => {
                fill("close.lint_failed", &[("{command}", command.clone()), ("{output}", output.clone())])
            }
            Self::CriterionFailed { code, output } => {
                fill("close.criterion_failed", &[("{code}", code.clone()), ("{output}", output.clone())])
            }
            Self::CriterionRanNoTest { code, command, tests } => fill(
                "close.criterion_ran_no_test",
                &[("{code}", code.clone()), ("{command}", command.clone()), ("{count}", tests.to_string())],
            ),
        }
    }

    fn to_value(&self, lang: Locale) -> Value {
        if let Self::Report(refusal) = self {
            return refusal.to_value(lang);
        }
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

    // O que voltou da última rodada entra antes das conferências, pela mesma
    // porta da rodada, com o commit: é ele que fecha a última onda.
    let mut recorded: Vec<Value> = Vec::new();
    if let Some(raw) = opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        let taken = crate::commands::flow::round::take_report(&opts.root, root, &spec, raw, &log, lang)
            .map_err(CloseRefusal::Report)?;
        recorded = taken.recorded;
    }

    let log = read(&path)?;
    finished(&log)?;

    // A máquina antes do agente de teste dedicado: o lint do projeto inteiro
    // e cada critério, uma vez por fechamento. A volta que só confere a
    // aprovação, sem nada mudado desde a máquina verde, não roda nada de novo.
    let runs = if proved_since_last_change(&log) { Vec::new() } else { machine(opts, root, &spec, &log)? };

    let log = read(&path)?;
    if !final_approved(&log) {
        let prompt = mustard_core::io::wave_prompt::final_review(root, &spec, &log, lang);
        let next = translate("close.final_review", lang).replace("{spec}", &spec);
        return Ok(json!({
            "ok": true,
            "spec": spec,
            "phase": "running",
            "recorded": recorded,
            "criteria": runs,
            "review": { "final": true, "prompt": prompt },
            "next": next,
        }));
    }

    // A fase `closed` sai só por aqui, e é a mesma porta que arma a cobrança
    // das pendências. A função antiga de fechar, que grava arquivos do formato
    // velho, não é chamada.
    crate::commands::spec_events::write::record_phase(&opts.root, &spec, "closed", session);
    if let Some(sid) = session {
        crate::shared::context::session::unbind_session_spec(&opts.root.to_string_lossy(), sid);
    }
    // O fechamento é um marco: a cópia para o banco da página sai aqui, com a
    // fase fechada na linha da spec da página do projeto.
    let prepared = crate::commands::spec_events::pages::copy::prepare_milestone(root, &spec, lang);

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
    // O fechamento manda copiar, menos com a cópia que não pôde ser
    // preparada, que fica para a próxima — e o pull request vem depois.
    let then = match command.as_str() {
        Some(line) => translate("close.next", lang).replace("{command}", line),
        None => translate("resume.next.closed", lang).to_string(),
    };
    crate::commands::spec_events::pages::end_milestone(&mut out, prepared.as_ref(), &spec, "close", &then, lang);
    if !command.is_null() {
        out["command"] = command;
    }
    Ok(out)
}

/// A máquina do fechamento: o lint do projeto inteiro, quando o
/// `mustard.json` declara um, e depois cada critério, uma vez, com a execução
/// de cada um gravada. O lint que falha recusa antes de qualquer critério
/// rodar; o critério que falha recusa depois de todos rodarem.
fn machine(opts: &CloseOpts, root: &Path, spec: &str, log: &SpecLog) -> Result<Vec<Value>, CloseRefusal> {
    if let Some(lint) = mustard_core::ProjectConfig::load(root).commands().lint {
        // O lint não é prova de critério: ele não promete rodar teste nenhum,
        // e a leitura de quantos testes a saída diz fica fora do caminho dele.
        let out = crate::commands::review::qa_run::run_command(&lint, root);
        if out.result != "pass" {
            return Err(CloseRefusal::LintFailed { command: lint, output: out.output });
        }
    }

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
        record(&opts.root, spec, "criterion_run", draft, PhaseWriter::Binary)
            .map_err(CloseRefusal::Refused)?;
        runs.push(json!({ "criterion": code, "result": out.result, "exit": out.exit, "ms": out.ms }));
        if out.result != "pass" && failed.is_none() {
            failed = Some(match out.ran_no_test {
                Some(tests) => CloseRefusal::CriterionRanNoTest {
                    code: code.clone(),
                    command: proof.clone(),
                    tests,
                },
                None => CloseRefusal::CriterionFailed { code: code.clone(), output: out.output.clone() },
            });
        }
    }
    match failed {
        Some(refusal) => Err(refusal),
        None => Ok(runs),
    }
}

/// O número da última mudança da obra: a entrega ou o commit mais novo.
fn last_change(log: &SpecLog) -> u64 {
    log.events.iter().filter(|e| matches!(e.event_type.as_str(), "delivered" | "commit")).map(|e| e.id).max().unwrap_or(0)
}

/// A máquina já passou depois da última mudança: cada critério vigente tem,
/// depois dela, uma execução, e a mais nova passou. O critério só roda com o
/// lint verde, então a mesma leitura diz que o lint passou.
fn proved_since_last_change(log: &SpecLog) -> bool {
    let since = last_change(log);
    let visible = log.block(BlockQuery::Block(Block::Criteria));
    let criteria: Vec<u64> = visible.iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
    !criteria.is_empty()
        && criteria.iter().all(|id| {
            visible
                .iter()
                .rev()
                .find(|e| e.event_type == "criterion_run" && e.id > since && e.int("criterion") == Some(*id))
                .is_some_and(|run| run.str_field("result") == Some("pass"))
        })
}

/// O agente de teste dedicado voltou aprovado depois da última mudança da
/// obra.
fn final_approved(log: &SpecLog) -> bool {
    let since = last_change(log);
    log.block(BlockQuery::Block(Block::Review)).into_iter().any(|e| {
        e.event_type == "verdict"
            && e.id > since
            && e.fields.get("final") == Some(&Value::Bool(true))
            && e.str_field("result") == Some("approved")
    })
}

/// A obra terminou? Recusa enquanto houver onda sem commit, onda com o
/// conserto pendente ([`crate::commands::flow::round::waves_pending_fix`]:
/// a última revisão dela reprovou e nenhuma entrega chegou depois), ou
/// pedido do usuário que nenhuma onda entregou. Não basta os testes
/// passarem. As ondas e os vereditos são lidos como a rodada os lê: a onda
/// que saiu do plano não é cobrada.
///
/// A leitura é a mesma da fila e do estado da página, não a de
/// `waves_to_redo`: aquela deixa de listar a onda assim que a rodada a
/// despacha de novo, antes de o conserto chegar, e um fechamento nesse meio
/// tempo fecharia com o conserto ainda em andamento — o commit da entrega
/// original da onda, anterior à reprovação, já satisfaz a exigência de commit
/// acima, e nada mais a travaria.
///
/// A onda reprovada que já entregou o conserto não trava mais: falta o
/// agente de teste dedicado conferir esse conserto, e é o fechamento —
/// pedindo o agente de novo, mais adiante — que cobra isso, não esta função.
/// Sem essa saída, o conserto nunca chegaria ao agente de teste: nada mais
/// grava um veredito comum enquanto a rodada está no ar.
fn finished(log: &SpecLog) -> Result<(), CloseRefusal> {
    if let Some(wave) = crate::commands::flow::round::waves_pending_fix(log).into_keys().next() {
        return Err(CloseRefusal::WaveRejected { wave });
    }

    let committed: BTreeSet<u64> = log
        .block(BlockQuery::Block(Block::Progress))
        .into_iter()
        .filter(|e| e.event_type == "commit")
        .flat_map(|e| e.ints("waves"))
        .collect();
    for wave in log.planned_waves() {
        if !committed.contains(&wave) {
            return Err(CloseRefusal::WaveWithoutCommit { wave });
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
    use mustard_core::domain::spec_events::SpecEvent;
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
        ready_with_waves(root, spec, proofs, 1);
    }

    /// O arquivo da tarefa da onda `n`.
    fn wave_file(n: u64) -> String {
        format!("src/w{n}.rs")
    }

    /// [`ready_to_close`] com `waves` ondas soltas, cada uma com a tarefa num
    /// arquivo dela, despachadas e entregues juntas. A rodada não pede
    /// revisão de onda nenhuma: a entrega já basta, e falta só o fechamento
    /// pedir o agente de teste dedicado.
    fn ready_with_waves(root: &Path, spec: &str, proofs: &[&str], waves: u64) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        for n in 1..=waves {
            std::fs::write(root.join(wave_file(n)), "fn um() {}\n").unwrap();
        }
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
        for n in 1..=waves {
            write(root, spec, "wave", json!({"n": n, "text": format!("Onda {n}."), "criteria": crits,
                "done_when": "A suíte passa.", "origin": said}));
            write(root, spec, "task", json!({"wave": n, "text": format!("Tarefa da onda {n}."),
                "files": [{"path": wave_file(n)}], "origin": said}));
        }
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();

        let round = |report: Option<String>| {
            round_for(&RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report }, None)
        };
        assert_eq!(round(None)["ok"], json!(true));
        let mut delivered = String::new();
        for n in 1..=waves {
            std::fs::write(root.join(wave_file(n)), "fn um() {}\nfn dois() {}\n").unwrap();
            let line = json!({"wave": n, "text": "Saiu.", "files": [wave_file(n)], "commit": "a soma sai"});
            delivered.push_str(&format!("<DELIVERED>{line}</DELIVERED>\n"));
        }
        let back = round(Some(delivered));
        assert_eq!(back["ok"], json!(true), "{back}");
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
    }

    /// A linha `<VERDICT>` do agente de teste dedicado aprovando a obra
    /// inteira.
    fn final_approval() -> String {
        let approved = json!({"final": true, "result": "approved", "text": "A obra está pronta."});
        format!("<VERDICT>{approved}</VERDICT>")
    }

    /// O fechamento, com a volta transparente do agente de teste dedicado: a
    /// obra pronta para fechar sempre passa por ele agora, mesmo a de uma
    /// onda só, e quem só quer o resultado final não precisa cuidar disso.
    /// Os testes que conferem o pedido dele por dentro chamam `close_for`
    /// direto.
    fn close(root: &Path, spec: &str) -> Value {
        let out = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: None }, None);
        if out["review"]["final"] == json!(true) {
            return close_for(
                &CloseOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: Some(final_approval()) },
                None,
            );
        }
        out
    }

    /// O fechamento roda cada critério uma vez — os dois critérios da spec
    /// aparecem, cada um com uma execução —, grava cada execução, pede o
    /// agente de teste dedicado mesmo com uma onda só, e, aprovado ele, grava
    /// a fase fechada e deixa a pasta da spec com exatamente três arquivos.
    #[test]
    fn closing_runs_each_criterion_once_and_leaves_three_files_in_the_folder() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version", "git --help"]);

        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(asked["phase"], json!("running"), "{asked}");
        assert_eq!(asked["criteria"].as_array().map(Vec::len), Some(2), "os dois critérios rodaram: {asked}");
        assert_eq!(asked["review"]["final"], json!(true), "a de uma onda só também pede o agente de teste: {asked}");

        let out = close(root, "x");
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("closed"), "{out}");
        assert!(out.get("review").is_none(), "{out}");

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
        assert_eq!(names, ["copy", "spec.ndjson"], "a pasta fechada tem o arquivo de eventos e a cópia, e nenhuma página");
        assert!(!root.join(".claude/spec/project.html").exists(), "nem a página do projeto");
    }

    /// O fechamento com um `mustard.json` que declara o lint.
    fn close_with_lint(root: &Path, lint: &str, report: Option<String>) -> Value {
        std::fs::write(root.join("mustard.json"), json!({ "lintCommand": lint }).to_string()).unwrap();
        close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None)
    }

    /// Quantas execuções de critério a spec tem.
    fn criterion_runs(root: &Path) -> usize {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.visible().iter().filter(|e| e.event_type == "criterion_run").count()
    }

    /// O fechamento roda o lint do projeto inteiro. O lint que falha recusa
    /// com a saída dele, antes de critério nenhum rodar. O que passa roda no
    /// projeto e, com a máquina verde, a resposta é o pedido do agente de
    /// teste dedicado — mesmo a de uma onda só —, e não fecha nem devolve o
    /// pull request; a reprovação dele volta como conserto da onda que ele
    /// aponta, e o fechamento recusa enquanto ela não sai; entregue o
    /// conserto, o fechamento pede o mesmo agente de novo, mas só para
    /// conferir o conserto; e a spec só fecha e só devolve o pull request com
    /// ele aprovado depois da última mudança, sem rodar a máquina de novo.
    ///
    /// As duas linhas do agente de teste vêm sem critério nenhum, que é como
    /// ele as devolve: ele confere o encaixe das ondas, e não critério. As
    /// duas são gravadas assim mesmo.
    #[test]
    fn closing_runs_the_project_lint_and_opens_the_pull_request_only_after_the_dedicated_test_agent() {
        let lint = "git init -q lint-rodou";
        let ran = |root: &Path| root.join("lint-rodou").is_dir();

        // O lint que falha recusa com a saída dele, antes de critério nenhum
        // rodar.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        let refused = close_with_lint(root, "git lint-que-nao-existe", None);
        assert_eq!(refused["reason"], json!("lint-failed"), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("git lint-que-nao-existe") && hint.contains("lint-que-nao-existe' is not a git command"), "{hint}");
        assert_eq!(criterion_runs(root), 0, "nenhum critério roda com o lint vermelho");

        // O lint que passa roda no projeto, e a máquina verde pede o agente
        // de teste dedicado.
        let asked = close_with_lint(root, lint, None);
        assert_eq!(asked["ok"], json!(true), "{asked}");
        assert_eq!(asked["phase"], json!("running"), "{asked}");
        assert!(ran(root), "o lint rodou antes do agente");
        assert_eq!(asked["criteria"].as_array().map(Vec::len), Some(1), "{asked}");
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let prompt = asked["review"]["prompt"].as_str().unwrap_or_default();
        assert!(prompt.contains(translate("prompt.final.fixed", Locale::PtBr)), "{prompt}");
        assert!(prompt.contains("código repetido entre ondas") && prompt.contains("prova que uma apagou da outra"), "{prompt}");
        for n in [1, 2] {
            assert!(prompt.contains(&format!("MSTD-WAVE-000{n}")), "a onda {n} está no pedido: {prompt}");
        }
        assert!(asked.get("command").is_none(), "sem aprovação, nada de pull request: {asked}");
        let expected = translate("close.final_review", Locale::PtBr).replace("{spec}", "x");
        assert_eq!(asked["next"], json!(expected), "{asked}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");

        // O agente reprova apontando a onda 2: a onda 2 volta como conserto,
        // e o fechamento recusa enquanto ela não sai.
        let rejected = json!({"final": true, "wave": 2, "result": "rejected", "text": "A onda 2 repete a 1."});
        let out = close_with_lint(root, lint, Some(format!("<VERDICT>{rejected}</VERDICT>")));
        assert_eq!(out["reason"], json!("wave-rejected"), "{out}");
        assert!(out["hint"].as_str().unwrap_or_default().contains('2'), "{out}");
        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None);
        let fix = round(None);
        let sent: Vec<u64> = fix["dispatch"].as_array().unwrap().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(sent, vec![2], "{fix}");
        std::fs::write(root.join(wave_file(2)), "fn um() {}\nfn tres() {}\n").unwrap();
        let line = json!({"wave": 2, "text": "Sem repetir a 1.", "files": [wave_file(2)], "commit": "a onda 2 sem repetição"});
        let back = round(Some(format!("<DELIVERED>{line}</DELIVERED>")));
        assert_eq!(back["ok"], json!(true), "{back}");
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");

        // Depois do conserto, a máquina roda de novo, e o pedido volta —
        // agora só com o conserto, não a obra inteira de novo.
        std::fs::remove_dir_all(root.join("lint-rodou")).unwrap();
        let again = close_with_lint(root, lint, None);
        assert_eq!(again["review"]["final"], json!(true), "{again}");
        assert!(ran(root), "{again}");
        let fix_prompt = again["review"]["prompt"].as_str().unwrap_or_default();
        assert!(fix_prompt.contains(translate("prompt.fix.final", Locale::PtBr)), "{fix_prompt}");
        assert!(fix_prompt.contains("MSTD-WAVE-0002") && !fix_prompt.contains("MSTD-WAVE-0001"), "só o conserto: {fix_prompt}");

        // Aprovado, a spec fecha sem rodar a máquina outra vez.
        std::fs::remove_dir_all(root.join("lint-rodou")).unwrap();
        let before = criterion_runs(root);
        let approved = json!({"final": true, "result": "approved", "text": "O conserto ficou certo."});
        let closed = close_with_lint(root, lint, Some(format!("<VERDICT>{approved}</VERDICT>")));
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        assert!(closed.get("review").is_none(), "{closed}");
        assert_eq!(closed["command"], json!("mustard-rt run pr-open --base dev --head feature/x --spec x"), "{closed}");
        assert_eq!(criterion_runs(root), before, "a volta do agente de teste não roda os critérios de novo");
        assert!(!ran(root), "nem o lint");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let last = log.visible().into_iter().rfind(|e| e.event_type == "verdict").unwrap();
        assert_eq!((last.wave(), last.fields.get("final")), (Some(2), Some(&json!(true))), "a aprovação fica na última onda");
        let finals: Vec<&SpecEvent> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "verdict" && e.fields.get("final") == Some(&json!(true)))
            .collect();
        let results: Vec<Option<&str>> = finals.iter().map(|e| e.str_field("result")).collect();
        assert_eq!(results, [Some("rejected"), Some("approved")], "as duas revisões finais ficaram gravadas");
        assert!(finals.iter().all(|e| e.fields.get("criteria").is_none()), "e nenhuma delas confere critério");
    }

    /// O agente de teste dedicado fecha toda obra: a de duas ondas, que
    /// entrega as duas, e a de uma onda só, que entrega a dela. A rodada
    /// nunca pede revisão de onda nenhuma, o fechamento pede o agente
    /// dedicado com um pedido que leva as entregas da obra, e quando ele
    /// aponta um problema numa onda, o conserto sai pela rodada e o
    /// fechamento pede o mesmo agente de novo, só para conferir o conserto —
    /// em até duas voltas; na terceira reprovação seguida, a onda para e a
    /// decisão passa a ser do usuário.
    #[test]
    fn the_dedicated_test_agent_closes_every_work() {
        let waves_in = |out: &Value, field: &str| -> Vec<u64> {
            out[field].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect()
        };

        // A obra de duas ondas: as duas entregam, sem revisão nenhuma da
        // rodada, e o fechamento pede o agente dedicado, com as duas
        // entregas no pedido.
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        let asked = close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let prompt = asked["review"]["prompt"].as_str().unwrap_or_default();
        for n in [1, 2] {
            assert!(prompt.contains(&format!("MSTD-DELIV-000{n}")), "a entrega da onda {n} está no pedido: {prompt}");
        }

        // A obra de uma onda só entrega a dela e passa pela mesma exigência.
        let solo = tempdir().unwrap();
        let solo_root = solo.path();
        ready_to_close(solo_root, "x", &["git --version"]);
        let asked_solo = close_for(&CloseOpts { root: solo_root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(asked_solo["review"]["final"], json!(true), "uma onda só também pede o agente: {asked_solo}");
        let round_solo =
            round_for(&RoundOpts { root: solo_root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert!(round_solo.get("reviews").is_none(), "a rodada não pede revisão da onda: {round_solo}");

        // De volta à obra de duas ondas: o agente aponta um problema na onda
        // 2. O conserto sai pela rodada, sem revisão nenhuma pedida por ela.
        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None);
        let close = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None);
        let reject = |text: &str| {
            let body = json!({"final": true, "wave": 2, "result": "rejected", "text": text});
            format!("<VERDICT>{body}</VERDICT>")
        };
        let refused = close(Some(reject("Falta o teste da onda 2.")));
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");
        let fix = round(None);
        assert_eq!(waves_in(&fix, "dispatch"), vec![2], "{fix}");
        assert!(fix.get("reviews").is_none(), "{fix}");
        std::fs::write(root.join(wave_file(2)), "fn um() {}\nfn tres() {}\n").unwrap();
        let line = json!({"wave": 2, "text": "Consertou.", "files": [wave_file(2)], "commit": "conserta a onda 2"});
        let back = round(Some(format!("<DELIVERED>{line}</DELIVERED>")));
        assert_eq!(back["ok"], json!(true), "{back}");
        assert!(back.get("reviews").is_none(), "a rodada não pede revisão do conserto: {back}");

        // Entregue o conserto, o fechamento pede o mesmo agente de novo, só
        // para conferir o conserto — não a obra inteira de novo.
        let rechecked = close(None);
        assert_eq!(rechecked["review"]["final"], json!(true), "{rechecked}");
        let fix_prompt = rechecked["review"]["prompt"].as_str().unwrap_or_default();
        assert!(fix_prompt.contains(translate("prompt.fix.final", Locale::PtBr)), "{fix_prompt}");
        assert!(fix_prompt.contains("MSTD-WAVE-0002") && !fix_prompt.contains("MSTD-WAVE-0001"), "só o conserto: {fix_prompt}");

        // A segunda volta de conserto: reprovado de novo, o conserto sai mais
        // uma vez.
        let refused_again = close(Some(reject("Ainda falta.")));
        assert_eq!(refused_again["reason"], json!("wave-rejected"), "{refused_again}");
        let fix_again = round(None);
        assert_eq!(waves_in(&fix_again, "dispatch"), vec![2], "{fix_again}");
        std::fs::write(root.join(wave_file(2)), "fn um() {}\nfn quatro() {}\n").unwrap();
        let line = json!({"wave": 2, "text": "Consertou de novo.", "files": [wave_file(2)], "commit": "conserta de novo"});
        assert_eq!(round(Some(format!("<DELIVERED>{line}</DELIVERED>")))["ok"], json!(true));

        // A terceira reprovação seguida para a onda: a rodada deixa de
        // despachá-la, e a decisão passa a ser do usuário.
        close(Some(reject("Ainda não.")));
        let stopped = round(None);
        assert_eq!(stopped["stopped"][0]["wave"], json!(2), "{stopped}");
        assert_eq!(waves_in(&stopped, "dispatch"), Vec::<u64>::new(), "a rodada para de despachar: {stopped}");
        let after = close(None);
        assert_eq!(after["reason"], json!("wave-rejected"), "o fechamento segue recusando: {after}");
    }

    /// O conserto de uma onda que não é a última do plano: o agente de teste
    /// dedicado reprova a onda 1 de duas, e a aprovação final, sem onda, fica
    /// gravada na onda 2, a última — nunca ganha um veredito próprio a onda 1.
    /// Três coisas têm de continuar certas mesmo assim: (1) o fechamento
    /// recusa enquanto o conserto está só despachado, sem entrega ainda; (2)
    /// entregue o conserto, a rodada solta a onda e manda fechar; e (3) depois
    /// da aprovação final, nenhuma onda fica com o estado de reprovada.
    #[test]
    fn a_fix_of_a_non_final_wave_releases_the_queue_and_leaves_no_wave_rejected() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_with_waves(root, "x", &["git --version"], 2);
        let round = |report: Option<String>| round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None);
        let close = |report: Option<String>| close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report }, None);

        // O agente de teste dedicado aponta a onda 1, não a última.
        let reject = json!({"final": true, "wave": 1, "result": "rejected", "text": "Falta o teste da onda 1."});
        let refused = close(Some(format!("<VERDICT>{reject}</VERDICT>")));
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");

        // A rodada despacha o conserto da onda 1.
        let fix = round(None);
        assert_eq!(fix["dispatch"].as_array().unwrap().iter().filter_map(|d| d["wave"].as_u64()).collect::<Vec<_>>(), vec![1], "{fix}");

        // (1) Só despachado, sem entrega: o fechamento continua recusando —
        // o commit da entrega original da onda 1, anterior à reprovação, não
        // pode bastar.
        let still_pending = close(None);
        assert_eq!(still_pending["reason"], json!("wave-rejected"), "{still_pending}");
        assert!(still_pending["hint"].as_str().unwrap_or_default().contains('1'), "{still_pending}");

        // A onda 1 entrega o conserto, com um código diferente do que já
        // estava no disco.
        std::fs::write(root.join(wave_file(1)), "fn um() {}\nfn tres() {}\n").unwrap();
        let line = json!({"wave": 1, "text": "Sem faltar o teste.", "files": [wave_file(1)], "commit": "conserta a onda 1"});
        let back = round(Some(format!("<DELIVERED>{line}</DELIVERED>")));
        assert_eq!(back["ok"], json!(true), "{back}");

        // (2) Entregue o conserto, a fila solta a onda: a rodada não tem mais
        // nada a despachar nem a esperar, e manda fechar.
        let released = round(None);
        assert_eq!(released["command"], json!("mustard-rt run close --spec x"), "{released}");

        // O agente de teste dedicado confere só o conserto, e aprova a obra
        // inteira: a aprovação sem onda fica gravada na última onda do plano
        // (a 2), não na 1, que foi a reprovada.
        let fix_prompt = close(None)["review"]["prompt"].as_str().unwrap_or_default().to_string();
        assert!(fix_prompt.contains("MSTD-WAVE-0001") && !fix_prompt.contains("MSTD-WAVE-0002"), "{fix_prompt}");
        let approved = json!({"final": true, "result": "approved", "text": "O conserto ficou certo."});
        let closed = close(Some(format!("<VERDICT>{approved}</VERDICT>")));
        assert_eq!(closed["phase"], json!("closed"), "{closed}");

        // (3) Nenhuma onda fica com o estado de reprovada, nem a 1, cujo
        // último veredito próprio continua sendo a reprovação: a aprovação
        // que fechou a obra não é dela.
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let states = crate::commands::flow::round::wave_states(&log);
        let rejected: Vec<u64> = states
            .into_iter()
            .filter(|(_, s)| *s == mustard_core::view::document::WaveState::Rejected)
            .map(|(n, _)| n)
            .collect();
        assert_eq!(rejected, Vec::<u64>::new(), "{closed}");
    }

    /// A resposta da rodada e a do fechamento, numa spec ainda sem páginas
    /// publicadas, mandam publicar a página da spec e a do projeto, dizem como
    /// gravar as duas publicações e mandam copiar os lotes, e nenhuma delas
    /// traz o endereço de uma página para a conversa. A do plano prova o mesmo
    /// no teste dela. Um passo comum — o levantamento, a gravação de um item —
    /// não manda publicar nem copiar.
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
            assert!(next.contains("write copy"), "{milestone}: {next}");
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
        for report in [&grilled, &context] {
            assert!(report.get("publish").is_none(), "um passo comum não manda publicar: {report}");
            assert!(report.get("copy").is_none(), "nem copiar: {report}");
            let shown = report.to_string();
            assert!(!shown.contains("write publish"), "um passo comum não manda publicar: {shown}");
            assert!(!shown.contains("write copy"), "nem copiar: {shown}");
        }
    }

    /// O fim do `next` de cada marco, com o comando que a resposta devolve: a
    /// rodada com tudo entregue e aprovado manda fechar, e o fechamento manda
    /// abrir o pull request.
    fn then_of(report: &Value, key: &str) -> String {
        let command = report["command"].as_str().unwrap_or_else(|| panic!("sem comando: {report}"));
        translate(key, Locale::PtBr).replace("{command}", command)
    }

    /// Com um item de texto que parece senha, a rodada e o fechamento mandam
    /// publicar e copiar assim mesmo e dizem o código do item a expurgar; o
    /// item fica fora da cópia.
    #[test]
    fn a_withheld_item_is_named_and_no_longer_holds_the_publish_of_the_round_and_the_close() {
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
            assert_eq!(report["publish"], json!(["spec", "project"]), "{milestone}: {report}");
            assert_eq!(report["withheld"], json!([code]), "{milestone}: {report}");
            let next = report["next"].as_str().unwrap_or_default();
            assert!(next.contains(&code) && next.contains("write purge"), "{milestone}: {next}");
            assert!(next.contains("write publish") && next.ends_with(&then), "{milestone}: {next}");
            let warned = report["warnings"].as_array().cloned().unwrap_or_default();
            assert!(warned.iter().any(|w| w["hint"].as_str().unwrap_or_default().contains(&code)), "{report}");
            let items = crate::commands::spec_events::pages::copy::sent_items(root, report);
            assert!(!items.contains(&id_of(&note)), "{milestone}: the item stays out of the copy");
        }
    }

    /// Quando a cópia para o banco não pode ser preparada, a rodada e o
    /// fechamento não mandam publicar nem copiar: dizem nos avisos por quê,
    /// dizem que a próxima cópia leva os mesmos itens e seguem com o próximo
    /// passo.
    #[test]
    fn a_copy_that_could_not_be_prepared_is_never_ordered() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        // Um arquivo no lugar da pasta da cópia impede de prepará-la.
        let folder = root.join(".claude/spec/x/copy");
        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::write(&folder, "não é pasta").unwrap();

        let rounded = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        let closed = close(root, "x");
        let failed = translate("page.copy.failed", Locale::PtBr);
        for (report, milestone, then) in
            [(&rounded, "round", then_of(&rounded, "round.close")), (&closed, "close", then_of(&closed, "close.next"))]
        {
            assert_eq!(report["ok"], json!(true), "{milestone}: {report}");
            assert!(report.get("publish").is_none() && report.get("copy").is_none(), "{milestone}: {report}");
            let next = report["next"].as_str().unwrap_or_default();
            assert!(!next.contains("write publish") && !next.contains("write copy"), "{milestone}: {next}");
            assert_eq!(next, format!("{failed} {then}"), "{milestone}");
            let warned = report["warnings"].as_array().cloned().unwrap_or_default();
            assert!(warned.iter().any(|w| w["reason"] == json!("io-failed")), "{milestone} says why: {report}");
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
        )["review"]["final"], json!(true));
        let closed = close_for(
            &CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report: Some(final_approval()) },
            Some("s-fecha"),
        );
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
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

    /// A onda parada pelo limite de consertos trava o fechamento enquanto está
    /// no plano, e a rodada faz a pergunta dela. Tirada do plano, com a
    /// tarefa, ela deixa de contar nos dois: a rodada manda fechar e o
    /// fechamento passa, sem cobrar dela veredito nem commit.
    #[test]
    fn a_stuck_wave_taken_out_of_the_plan_no_longer_counts_in_the_round_or_the_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        ready_to_close(root, "x", &["git --version"]);
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": "mais uma"})));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        let wave = id_of(&write(root, "x", "wave", json!({"n": 2, "text": "Onda 2.", "criteria": [crit],
            "done_when": "passa", "origin": said})));
        let task = id_of(&write(root, "x", "task", json!({"wave": 2, "text": "Tarefa.",
            "files": [{"path": "src/a.rs"}], "origin": said})));
        for attempt in 0..3 {
            crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave": 2, "role": "wave",
                "text": "pedido", "lines": 1, "chars": 6, "items": [wave], "mustard": "0", "author": "binary"}));
            write(root, "x", "delivered", json!({"wave": 2, "text": format!("Tentativa {attempt}."), "files": ["src/a.rs"]}));
            crate::shared::spec_state::seed_verdict(root, "x", 2, "rejected", crit);
        }
        let round = || round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);

        let stopped = round();
        assert_eq!(stopped["stopped"][0]["wave"], json!(2), "{stopped}");
        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("wave-rejected"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains('2'), "{refused}");

        write(root, "x", "remove", json!({"targets": [wave, task], "reason": "o usuário tirou a onda do plano"}));
        let rounded = round();
        assert!(rounded.get("stopped").is_none(), "{rounded}");
        assert_eq!(rounded["command"], json!("mustard-rt run close --spec x"), "{rounded}");
        let closed = close(root, "x");
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
    }

    /// Uma prova do cargo com o nome do teste errado e `--exact` sai verde sem
    /// rodar teste nenhum: o fechamento recusa, diz qual critério, o comando
    /// dela e o número de testes que a saída dele disse, e grava a execução
    /// como reprovada. A prova com o nome certo passa.
    #[test]
    fn a_proof_that_ran_zero_tests_blocks_the_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn soma(a: u32, b: u32) -> u32 { a + b }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma_de_dois() { assert_eq!(super::soma(1, 1), 2); }\n}\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        ready_to_close(
            root,
            "x",
            &["cargo test --lib -- tests::soma_de_dois --exact", "cargo test --lib -- tests::soma --exact"],
        );

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-ran-no-test"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        let wrong_name = "cargo test --lib -- tests::soma --exact";
        let expected = translate("close.criterion_ran_no_test", Locale::PtBr)
            .replace("{code}", &codes[&criteria[1]])
            .replace("{command}", wrong_name)
            .replace("{count}", "0");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(wrong_name) && hint.contains('0'), "a recusa diz o comando e o número: {hint}");
        let runs: Vec<(Option<u64>, Option<&str>)> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "criterion_run")
            .map(|e| (e.int("criterion"), e.str_field("result")))
            .collect();
        assert_eq!(runs, vec![(Some(criteria[0]), Some("pass")), (Some(criteria[1]), Some("fail"))]);
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }

    /// A leitura do número de testes não é só do cargo: a prova que roda outro
    /// executor e sai verde dizendo zero teste trava o fechamento do mesmo
    /// jeito, com o comando e o número na recusa; a que roda pelo menos um
    /// teste passa.
    #[test]
    fn a_proof_that_ran_zero_tests_outside_cargo_blocks_the_close_too() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let zero = "echo Tests: 0 total";
        ready_to_close(root, "x", &["echo Tests: 3 total", zero]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-ran-no-test"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        let expected = translate("close.criterion_ran_no_test", Locale::PtBr)
            .replace("{code}", &codes[&criteria[1]])
            .replace("{command}", zero)
            .replace("{count}", "0");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let runs: Vec<(Option<u64>, Option<&str>)> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "criterion_run")
            .map(|e| (e.int("criterion"), e.str_field("result")))
            .collect();
        assert_eq!(runs, vec![(Some(criteria[0]), Some("pass")), (Some(criteria[1]), Some("fail"))]);
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }

    /// Os dois executores que dizem zero sem escrever número são lidos pela
    /// linha de resumo de cada um, e não por uma frase qualquer. O vitest sai
    /// com código 0 quando o filtro por nome não casa teste nenhum e escreve
    /// `Tests  no tests`: essa prova é recusada. O go escreve a marca dele por
    /// pacote, então a prova em que um pacote não rodou teste ao lado de outro
    /// que rodou passa — a corrida rodou teste. E a execução recusada guarda o
    /// que o executor escreveu, e não uma frase montada sobre ela.
    #[test]
    fn a_proof_that_ran_zero_tests_is_read_by_the_summary_line_of_each_runner() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let go_mixed = "echo ok x/pkg 0.002s [no tests to run] && echo ok x/outro 0.02s";
        let vitest_zero = "echo Tests no tests";
        ready_to_close(root, "x", &[go_mixed, vitest_zero]);

        let refused = close(root, "x");
        assert_eq!(refused["reason"], json!("criterion-ran-no-test"), "{refused}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let criteria: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "criterion").map(|e| e.id).collect();
        let expected = translate("close.criterion_ran_no_test", Locale::PtBr)
            .replace("{code}", &codes[&criteria[1]])
            .replace("{command}", vitest_zero)
            .replace("{count}", "0");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let runs: Vec<(Option<&str>, Option<&str>)> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "criterion_run")
            .map(|e| (e.str_field("result"), e.str_field("output")))
            .collect();
        assert_eq!(runs.len(), 2, "os dois critérios rodaram: {runs:?}");
        assert_eq!(runs[0], (Some("pass"), None), "o go com um pacote sem teste e outro com teste passa");
        assert_eq!(runs[1].0, Some("fail"));
        assert_eq!(
            runs[1].1,
            Some("Tests no tests"),
            "a execução recusada guarda o que o executor escreveu: {runs:?}"
        );
        assert_eq!(State::from_log(&log).phase, Some("running"), "a spec não fechou");
    }

    /// A leitura de quantos testes o comando rodou vale só na prova de um
    /// critério. O lint do projeto não passa por ela: um lint verde que
    /// escreve `Tests: 0 total` fecha a spec do mesmo jeito. E a prova de
    /// critério que não é comando de teste nenhum, cuja saída verde só cita
    /// "no tests" sem contagem de executor, passa: frase não é contagem.
    #[test]
    fn a_proof_that_ran_zero_tests_is_read_only_in_the_proof_of_a_criterion() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let not_a_test = "echo src/msg.rs: no tests found here";
        ready_to_close(root, "x", &[not_a_test]);

        let asked = close_with_lint(root, "echo Tests: 0 total", None);
        assert_eq!(asked["ok"], json!(true), "o lint verde não é lido como prova: {asked}");
        assert_eq!(asked["review"]["final"], json!(true), "{asked}");
        let closed = close_with_lint(root, "echo Tests: 0 total", Some(final_approval()));
        assert_eq!(closed["ok"], json!(true), "{closed}");
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let runs: Vec<Option<&str>> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "criterion_run")
            .map(|e| e.str_field("result"))
            .collect();
        assert_eq!(runs, vec![Some("pass")], "a prova que não é teste passou: {runs:?}");
        assert_eq!(State::from_log(&log).phase, Some("closed"));
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
