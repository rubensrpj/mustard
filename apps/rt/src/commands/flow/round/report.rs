//! O relatório da rodada e as linhas dos agentes: o que cada agente devolve,
//! lido como veio, conferido antes de qualquer gravação e gravado pela mesma
//! porta das outras gravações, com o commit no meio.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::spec_events::{Refusal, SpecLog, DELIVERED_MAX_CHARS};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::answer::RoundRefusal;
use super::commit::{
    commit_draft, commit_message, format_round_files, make_commit, record_commit, unknown_file, UNMADE_SHA,
};
use super::queue::{task_files, waves_in_progress};
use super::stops::{change_accepted, replan_code};
use crate::commands::spec_events::write::{record, RecordCheck};

/// A linha da entrega, como o agente de onda a devolve.
const DELIVERED_LINE: &str = "DELIVERED";
/// A linha do veredito, como o revisor a devolve.
const VERDICT_LINE: &str = "VERDICT";

/// O que a linha `DELIVERED` de uma onda trouxe.
pub(crate) struct WaveReport {
    pub wave: u64,
    pub delivered: String,
    pub files: Vec<String>,
    /// O resumo do commit, de onde a mensagem é montada.
    pub commit: Option<String>,
    /// As provas novas: o critério, pelo código ou pelo número, e o comando.
    pub proofs: Vec<(Value, String)>,
    /// As ondas que este conserto fecha.
    pub fixes: Vec<u64>,
    pub replan: Option<String>,
}

/// O que a linha `VERDICT` de uma onda trouxe: os campos do veredito, com a
/// onda à parte.
pub(crate) struct VerdictReport {
    pub wave: u64,
    pub fields: Map<String, Value>,
}

/// O relatório de uma rodada: as entregas e os vereditos que ele traz. O
/// fechamento lê o relatório da última rodada pela mesma porta.
pub(crate) struct Report {
    pub waves: Vec<WaveReport>,
    pub verdicts: Vec<VerdictReport>,
}

/// O que a rodada fez com um relatório.
pub(crate) struct Taken {
    /// O que foi gravado, na ordem.
    pub recorded: Vec<Value>,
    /// Os arquivos formatados.
    pub formatted: Vec<String>,
    /// Os avisos: formatador não achado e prova nova que não roda teste.
    pub warnings: Vec<Value>,
    /// O commit feito, quando houve arquivo entregue.
    pub commit: Option<Value>,
}

/// Fecha o que voltou de uma rodada, a partir do texto `raw` com as linhas
/// dos agentes: confere tudo, formata os arquivos da rodada e faz o commit, e
/// só então grava os vereditos, as entregas, a versão nova de cada critério
/// com prova nova e o commit. O git, que pode recusar, roda antes da primeira
/// gravação: a chamada corrigida depois de uma recusa grava tudo uma vez só.
/// A rodada e o fechamento fecham o relatório por aqui.
pub(crate) fn take_report(
    start: &Path,
    root: &Path,
    spec: &str,
    raw: &str,
    log: &SpecLog,
    lang: Locale,
) -> Result<Taken, RoundRefusal> {
    let report = parse_report(raw)?;
    // O agente que diz que o plano da onda não funciona para a rodada: a
    // mudança proposta é mostrada, e só o clique do usuário em "Aceitar",
    // gravado pela testemunha, a deixa seguir.
    for wave in &report.waves {
        if let Some(change) = &wave.replan {
            let code = replan_code(wave.wave, change);
            if !change_accepted(log, wave.wave, &code) {
                return Err(RoundRefusal::Replan { wave: wave.wave, change: change.clone(), code });
            }
        }
    }
    for wave in &report.waves {
        let chars = wave.delivered.chars().count();
        if chars > DELIVERED_MAX_CHARS {
            return Err(RoundRefusal::DeliveredTooLong { wave: wave.wave, chars });
        }
    }
    reserved_elsewhere(log, &report.waves)?;
    unknown_file(root, &report.waves)?;
    // A mensagem do commit é montada e conferida junto das outras travas,
    // antes de qualquer gravação: recusá-la depois de gravar o entregou e o
    // veredito faria a chamada seguinte, com a mensagem corrigida, duplicar os
    // dois.
    let message = commit_message(&report.waves, lang)?;
    let mut files: Vec<String> = Vec::new();
    for file in report.waves.iter().flat_map(|w| w.files.iter()) {
        if !files.contains(file) {
            files.push(file.clone());
        }
    }
    let mut waves: Vec<u64> = Vec::new();
    for n in report.waves.iter().flat_map(|w| std::iter::once(w.wave).chain(w.fixes.iter().copied())) {
        if !waves.contains(&n) {
            waves.push(n);
        }
    }
    // Toda gravação que vem depois do git passa antes pela mesma conferência
    // da gravação, contra a spec: a recusa que viesse depois do commit
    // deixaria o commit feito e a chamada corrigida sem nada a comitar.
    let planned = message.as_ref().map(|(title, _)| commit_draft(root, UNMADE_SHA, title, &waves, &files));
    let checked = check_reports(start, spec, &report, planned).map_err(RoundRefusal::Refused)?;

    let mut warnings: Vec<Value> = Vec::new();
    // A formatação roda uma vez por rodada, só nos arquivos da rodada.
    let outcome = format_round_files(root, &files);
    for name in outcome.missing {
        warnings.push(json!({
            "reason": "formatter-not-found",
            "hint": translate("round.formatter_missing", lang).replace("{name}", &name),
        }));
    }
    let made = match message {
        Some((title, body)) => Some((make_commit(root, &title, &body, &files)?, title)),
        None => None,
    };
    let (recorded, proofs) = record_reports(start, spec, checked).map_err(RoundRefusal::Refused)?;
    let commit = match made {
        Some((sha, title)) => Some(record_commit(start, root, spec, &sha, &title, &waves, &files)?),
        None => None,
    };
    // A prova nova roda uma vez: a que sai verde sem rodar teste nenhum é
    // avisada agora, antes de o fechamento recusá-la.
    for (code, proof) in proofs {
        if crate::commands::review::qa_run::run_proof(&proof, root).ran_no_test {
            warnings.push(json!({
                "reason": "proof-ran-no-test",
                "hint": translate("round.proof_ran_no_test", lang).replace("{code}", &code),
            }));
        }
    }
    Ok(Taken { recorded, formatted: outcome.formatted, warnings, commit })
}

/// Os trechos entre `<tag>` e `</tag>` de `raw`, na ordem.
fn tagged<'a>(raw: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        let Some(end) = after.find(&close) else { break };
        out.push(after[..end].trim());
        rest = &after[end + close.len()..];
    }
    out
}

/// O objeto JSON de uma linha, com a onda dela.
fn line_object(body: &str, line: &'static str) -> Result<(u64, Map<String, Value>), RoundRefusal> {
    let parsed: Value =
        serde_json::from_str(body).map_err(|e| RoundRefusal::BadReport { detail: format!("{line}: {e}") })?;
    let Value::Object(fields) = parsed else {
        return Err(RoundRefusal::BadReport { detail: format!("{line}: {body}") });
    };
    let wave = fields.get("wave").and_then(Value::as_u64).ok_or(RoundRefusal::LineField { line, field: "wave" })?;
    Ok((wave, fields))
}

/// O relatório da rodada anterior: as linhas `DELIVERED` e `VERDICT` que o
/// texto recebido traz, como os agentes as devolvem. O resto do texto não é
/// lido.
pub(crate) fn parse_report(raw: &str) -> Result<Report, RoundRefusal> {
    let text = |fields: &Map<String, Value>, key: &str| {
        fields.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
    };
    let mut waves = Vec::new();
    for body in tagged(raw, DELIVERED_LINE) {
        let (wave, fields) = line_object(body, DELIVERED_LINE)?;
        let field = |field| RoundRefusal::LineField { line: DELIVERED_LINE, field };
        let delivered = text(&fields, "text").ok_or_else(|| field("text"))?;
        let files: Vec<String> = fields
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| field("files"))?
            .iter()
            .filter_map(Value::as_str)
            .map(|f| f.trim().replace('\\', "/"))
            .filter(|f| !f.is_empty())
            .collect();
        let replan = text(&fields, "replan");
        let commit = text(&fields, "commit");
        // Arquivo entregue pede commit, e o commit sai do resumo.
        if commit.is_none() && !files.is_empty() && replan.is_none() {
            return Err(field("commit"));
        }
        let mut proofs = Vec::new();
        for proof in fields.get("proofs").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
            let criterion = proof.get("criterion").filter(|c| !c.is_null()).cloned().ok_or_else(|| field("proofs"))?;
            let command = proof.get("proof").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
            proofs.push((criterion, command.ok_or_else(|| field("proofs"))?.to_string()));
        }
        let fixes = fields
            .get("fixes")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_u64).filter(|n| *n != wave).collect())
            .unwrap_or_default();
        waves.push(WaveReport { wave, delivered, files, commit, proofs, fixes, replan });
    }
    let mut verdicts = Vec::new();
    for body in tagged(raw, VERDICT_LINE) {
        let (wave, mut fields) = line_object(body, VERDICT_LINE)?;
        fields.remove("wave");
        verdicts.push(VerdictReport { wave, fields });
    }
    if waves.is_empty() && verdicts.is_empty() {
        return Err(RoundRefusal::LineMissing);
    }
    Ok(Report { waves, verdicts })
}

/// Um arquivo entregue que está reservado para outra onda em andamento: duas
/// ondas no mesmo arquivo seriam dois agentes editando o mesmo arquivo ao mesmo
/// tempo. As ondas que entregam neste relatório já não estão em andamento.
fn reserved_elsewhere(log: &SpecLog, waves: &[WaveReport]) -> Result<(), RoundRefusal> {
    let reporting: BTreeSet<u64> = waves.iter().flat_map(|w| std::iter::once(w.wave).chain(w.fixes.clone())).collect();
    let running: Vec<u64> =
        waves_in_progress(log).into_keys().filter(|n| !reporting.contains(n)).collect();
    let reserved = task_files(log);
    for wave in waves {
        for file in &wave.files {
            if let Some(other) = running.iter().find(|n| reserved.get(n).is_some_and(|files| files.contains(file))) {
                return Err(RoundRefusal::FileReserved { file: file.clone(), wave: wave.wave, other: *other });
            }
        }
    }
    Ok(())
}

/// O número do critério `reference`, dado pelo código que a página mostra ou
/// pelo número, na versão mais nova. Um critério que a spec não tem é
/// recusado.
fn criterion_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    let unknown = || Refusal::UnknownTarget {
        target: mustard_core::domain::spec_events::EventRef::from_value(reference)
            .unwrap_or(mustard_core::domain::spec_events::EventRef::Code(reference.to_string())),
    };
    let codes = log.codes();
    let id = match reference {
        Value::Number(n) => n.as_u64().ok_or_else(unknown)?,
        Value::String(code) => {
            let code = code.trim();
            match code.parse::<u64>() {
                Ok(n) => n,
                Err(_) => codes.iter().filter(|(_, c)| c.as_str() == code).map(|(id, _)| *id).max().ok_or_else(unknown)?,
            }
        }
        _ => return Err(unknown()),
    };
    let current = log.current(id).filter(|e| e.event_type == "criterion").ok_or_else(unknown)?;
    Ok(current.id)
}

/// O que [`record_reports`] devolve: o que foi gravado e, de cada prova nova,
/// o código do critério e o comando.
type RecordedReport = (Vec<Value>, Vec<(String, String)>);

/// O que [`check_reports`] conferiu e [`record_reports`] grava: cada veredito
/// e cada entregou já montado, com a onda, e o número de cada critério com
/// prova nova, com o comando.
struct CheckedReport {
    verdicts: Vec<(u64, Map<String, Value>)>,
    deliveries: Vec<(u64, Map<String, Value>)>,
    proofs: Vec<(u64, String)>,
}

/// Monta o que voltou e passa cada gravação que virá — cada veredito, cada
/// entregou, a versão nova de cada critério com prova nova e o commit
/// `commit`, na ordem em que serão gravados — pela conferência inteira da
/// gravação, contra a spec, sem gravar nada: a linha sem campo obrigatório
/// nunca deixa gravada a que veio antes dela, e nada é recusado depois do
/// commit. O entregou vai também em cada onda que o conserto fecha.
fn check_reports(
    start: &Path,
    spec: &str,
    report: &Report,
    commit: Option<Map<String, Value>>,
) -> Result<CheckedReport, Refusal> {
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary)?;
    // Os critérios citados existem, antes de qualquer gravação.
    let mut verdicts = Vec::new();
    for verdict in &report.verdicts {
        let mut draft = verdict.fields.clone();
        if let Some(Value::Array(criteria)) = draft.get_mut("criteria") {
            for item in criteria.iter_mut() {
                if let Some(reference) = item.get("criterion").cloned() {
                    item["criterion"] = json!(criterion_id(check.log(), &reference)?);
                }
            }
        }
        draft.insert("wave".into(), json!(verdict.wave));
        draft.insert("author".into(), json!("review"));
        check.record("verdict", draft.clone())?;
        verdicts.push((verdict.wave, draft));
    }
    let mut deliveries = Vec::new();
    for report in &report.waves {
        for wave in std::iter::once(report.wave).chain(report.fixes.iter().copied()) {
            let mut draft = Map::new();
            draft.insert("wave".into(), json!(wave));
            draft.insert("text".into(), json!(report.delivered));
            draft.insert("files".into(), json!(report.files));
            draft.insert("author".into(), json!("wave"));
            check.record("delivered", draft.clone())?;
            deliveries.push((wave, draft));
        }
    }
    let mut proofs = Vec::new();
    for wave in &report.waves {
        for (reference, proof) in &wave.proofs {
            proofs.push((criterion_id(check.log(), reference)?, proof.clone()));
        }
    }
    for (id, proof) in &proofs {
        if let Some(version) = criterion_version(check.log(), *id, proof) {
            check.record("criterion", version.draft)?;
        }
    }
    if let Some(draft) = commit {
        check.record("commit", draft)?;
    }
    Ok(CheckedReport { verdicts, deliveries, proofs })
}

/// A versão nova de um critério com a prova nova.
struct CriterionVersion {
    /// O código do critério que a página mostra.
    code: String,
    /// O número da versão que a nova substitui.
    replaces: u64,
    draft: Map<String, Value>,
}

/// A versão nova do critério `id`, com a prova `proof` e o mesmo resto da
/// versão mais nova dele no arquivo `log`. `None` quando o critério saiu da
/// leitura.
fn criterion_version(log: &SpecLog, id: u64, proof: &str) -> Option<CriterionVersion> {
    let criterion = log.current(id)?;
    let mut draft: Map<String, Value> = criterion
        .fields
        .iter()
        .filter(|(key, _)| !["v", "id", "code", "at", "type", "search", "author", "replaces"].contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    draft.insert("proof".into(), json!(proof));
    draft.insert("replaces".into(), json!(criterion.id));
    draft.insert("author".into(), json!("wave"));
    let code = log.codes().get(&criterion.id).cloned().unwrap_or_else(|| criterion.id.to_string());
    Some(CriterionVersion { code, replaces: criterion.id, draft })
}

/// Grava o que [`check_reports`] conferiu, pela mesma porta de gravação das
/// outras: primeiro os vereditos, que julgam entregas já gravadas; depois o
/// entregou de cada onda e a versão nova de cada critério com prova nova.
/// Devolve o que foi gravado e, de cada prova nova, o código do critério e o
/// comando.
fn record_reports(start: &Path, spec: &str, checked: CheckedReport) -> Result<RecordedReport, Refusal> {
    let CheckedReport { verdicts, deliveries, proofs } = checked;
    let path = store::spec_file(&crate::commands::spec_events::project(start).root, spec)?;
    let read = || store::read(&path)?.ok_or_else(|| Refusal::NoSpecFile { spec: spec.to_string() });
    let mut recorded = Vec::new();
    for (wave, draft) in verdicts {
        let written = record(start, spec, "verdict", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "wave": wave, "type": "verdict", "id": written.written.id }));
    }
    for (wave, draft) in deliveries {
        let written = record(start, spec, "delivered", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "wave": wave, "type": "delivered", "id": written.written.id }));
    }
    let mut ran = Vec::new();
    for (id, proof) in proofs {
        let Some(CriterionVersion { code, replaces, draft }) = criterion_version(&read()?, id, &proof) else {
            continue;
        };
        let written = record(start, spec, "criterion", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "type": "criterion", "id": written.written.id, "replaces": replaces }));
        ran.push((code, proof));
    }
    Ok((recorded, ran))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use mustard_core::domain::spec_events::SpecEvent;
    use tempfile::tempdir;

    use crate::commands::flow::round::queue::waves_to_redo;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A rodada grava o que cada onda entregou e o veredito da revisão dela, e
    /// passa a pedir a revisão do que entregou depois do último veredito.
    #[test]
    fn what_came_back_becomes_the_delivered_and_the_verdict_of_the_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let out = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        let reviews = out["reviews"].as_array().cloned().unwrap_or_default();
        assert_eq!(reviews.len(), 1, "a onda entregue sem veredito pede revisão: {out}");
        assert_eq!(reviews[0]["wave"], json!(1), "{out}");

        let out = round(root, "x", Some(&verdict(1, "approved", "passou")));
        assert_eq!(out["reviews"], json!([]), "o veredito é mais novo que a entrega dele: {out}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 1);
        let judged: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "verdict").collect();
        assert_eq!(judged.len(), 1);
        let criterion = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        assert_eq!(judged[0].fields["criteria"][0]["criterion"], json!(criterion), "the code became the number");
    }

    /// O que uma onda entregou acima do teto de caracteres é recusado, e nada
    /// é gravado.
    #[test]
    fn a_delivered_report_over_the_character_cap_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let long = "a".repeat(DELIVERED_MAX_CHARS + 1);
        let refused = round(root, "x", Some(&delivered(root, 1, &long, &["src/a.rs"])));
        assert_eq!(refused["reason"], json!("delivered-too-long"), "{refused}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 0);
    }

    /// A rodada aceita as linhas do fim exatamente como os textos dos agentes
    /// as ensinam, nos dois idiomas, dentro da resposta inteira do agente:
    /// grava a entrega e o veredito, e o commit sai com o título e o corpo
    /// montados do resumo. A resposta da rodada ensina o mesmo formato.
    #[test]
    fn the_round_takes_the_closing_lines_exactly_as_the_agent_texts_teach_and_commits() {
        for (lang, answer) in [(Locale::PtBr, "Entreguei a soma."), (Locale::EnUs, "I delivered the sum.")] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[])]);
            let config = format!(r#"{{"language":{{"text":"{}"}}}}"#, lang.as_str());
            std::fs::write(root.join("mustard.json"), config).unwrap();
            let first = round(root, "x", None);
            let taught = translate("round.report", lang);
            assert!(first["next"].as_str().unwrap_or_default().contains(taught), "{first}");
            assert!(taught.contains("<DELIVERED>") && taught.contains("<VERDICT>") && taught.contains("\"commit\""));

            std::fs::write(root.join("src/a.rs"), "fn um() {}\nfn soma() {}\n").unwrap();
            let (wave_text, review_text) = mustard_core::agent_texts(lang)
                .iter()
                .fold((String::new(), String::new()), |(w, r), (name, body)| match *name {
                    "wave" => ((*body).to_string(), r),
                    "review" => (w, (*body).to_string()),
                    _ => (w, r),
                });
            let example = if lang == Locale::PtBr {
                [("<a entrega>", "A soma saiu, com o teste."), ("caminho/do/arquivo.rs", "src/a.rs"), ("<o resumo do commit>", "a soma sai")]
            } else {
                [("<the delivery>", "The sum is out, with its test."), ("path/to/file.rs", "src/a.rs"), ("<the commit summary>", "the sum ships")]
            };
            let line = taught_line(&wave_text, "DELIVERED", &example);
            let whole = format!("{answer}\n\nOs arquivos mudaram.\n\n{line}\n");
            let back = round(root, "x", Some(&whole));
            assert_eq!(back["ok"], json!(true), "{lang:?}: {back}");
            let (subject, body) = last_commit(root);
            let (title, summary) = if lang == Locale::PtBr {
                ("feat(onda-1): a soma sai", "- onda 1: a soma sai")
            } else {
                ("feat(wave-1): the sum ships", "- wave 1: the sum ships")
            };
            assert_eq!(subject, title, "{lang:?}");
            assert_eq!(body, summary, "{lang:?}");
            assert_eq!(back["commit"]["title"], json!(title), "{back}");
            let shown = Command::new("git").args(["show", "--name-only", "--format=", "HEAD"]).current_dir(root).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&shown.stdout).trim(), "src/a.rs");

            let judged = round(root, "x", Some(&taught_line(&review_text, "VERDICT", &[])));
            assert_eq!(judged["ok"], json!(true), "{lang:?}: {judged}");
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let visible = log.visible();
            let delivered = visible.iter().find(|e| e.event_type == "delivered").expect("the delivery");
            assert_eq!(delivered.str_field("text"), Some(example[0].1));
            assert_eq!(delivered.wave(), Some(1));
            let commit = visible.iter().find(|e| e.event_type == "commit").expect("the commit");
            assert_eq!(commit.str_field("title"), Some(title));
            let verdict = visible.iter().find(|e| e.event_type == "verdict").expect("the verdict");
            assert_eq!(verdict.str_field("result"), Some("approved"));
            let criterion = visible.iter().find(|e| e.event_type == "criterion").map(|e| e.id);
            assert_eq!(verdict.fields["criteria"][0]["criterion"].as_u64(), criterion);
            assert_eq!(judged["command"], json!("mustard-rt run close --spec x"), "{judged}");
        }
    }

    /// Sem a linha do fim, ou com a linha sem o resumo do commit, a rodada
    /// recusa e não grava nada; um critério que a spec não tem também.
    #[test]
    fn a_report_without_the_closing_line_or_its_fields_is_refused_and_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let lines_before = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();

        let missing = round(root, "x", Some("Entreguei a soma, e os arquivos mudaram."));
        assert_eq!(missing["reason"], json!("round-line-missing"), "{missing}");
        assert_eq!(missing["hint"], json!(translate("round.line_missing", Locale::PtBr)), "{missing}");

        let without = line("DELIVERED", json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"]}));
        let refused = round(root, "x", Some(&without));
        assert_eq!(refused["reason"], json!("round-line-field-missing"), "{refused}");
        let expected = translate("round.line_field", Locale::PtBr).replace("{line}", "DELIVERED").replace("{field}", "commit");
        assert_eq!(refused["hint"], json!(expected), "{refused}");

        let no_wave = line("VERDICT", json!({"result": "approved", "text": "passou", "criteria": []}));
        assert_eq!(round(root, "x", Some(&no_wave))["reason"], json!("round-line-field-missing"));

        let unknown = line("VERDICT", json!({"wave": 1, "result": "approved", "text": "passou",
            "criteria": [{"criterion": "MSTD-CRIT-0099", "tests_rule": true}]}));
        let refused = round(root, "x", Some(&unknown));
        assert_eq!(refused["ok"], json!(false), "{refused}");

        let lines_after = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        assert_eq!(lines_after, lines_before, "nothing was recorded");
    }

    /// Tudo é conferido antes da primeira gravação. O caminho que não está no
    /// disco nem no git é recusado sozinho e junto de um caminho certo, e a
    /// chamada seguinte, com a linha corrigida, grava a entrega uma vez só. O
    /// veredito sem resultado depois de um válido também é recusado sem
    /// deixar o primeiro gravado.
    #[test]
    fn a_wrong_path_or_a_line_missing_a_field_is_refused_before_anything_is_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let spec_lines = || std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        let before = spec_lines();

        for files in [&["src/nao_existe.rs"][..], &["src/a.rs", "src/nao_existe.rs"][..]] {
            let wrong = delivered(root, 1, "Saiu.", files);
            let refused = round(root, "x", Some(&wrong));
            assert_eq!(refused["reason"], json!("round-file-unknown"), "{files:?}: {refused}");
            let expected = translate("round.file_unknown", Locale::PtBr)
                .replace("{file}", "src/nao_existe.rs")
                .replace("{wave}", "1");
            assert_eq!(refused["hint"], json!(expected), "{refused}");
            assert_eq!(spec_lines(), before, "{files:?}: nothing was recorded");
        }

        let valid = verdict(1, "approved", "passou");
        let no_result = line("VERDICT", json!({"wave": 1, "text": "sem resultado",
            "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]}));
        let refused = round(root, "x", Some(&format!("{valid}\n{no_result}")));
        assert_eq!(refused["reason"], json!("missing-field"), "{refused}");
        assert_eq!(spec_lines(), before, "the valid verdict was not recorded either");

        let went = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "the corrected line records the delivery once");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(log.visible().iter().all(|e| e.event_type != "verdict"), "no verdict was left behind");
    }

    /// A lista de arquivos entregue é conferida contra os arquivos
    /// reservados: o arquivo de outra onda ainda em andamento é recusado, e o
    /// de uma onda que já não está em andamento passa.
    #[test]
    fn a_delivered_file_reserved_for_another_wave_in_flight_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);

        let refused = round(root, "x", Some(&delivered(root, 1, "Mexi na b também.", &["src/a.rs", "src/b.rs"])));
        assert_eq!(refused["reason"], json!("round-file-reserved"), "{refused}");
        let expected = translate("round.file_reserved", Locale::PtBr)
            .replace("{file}", "src/b.rs")
            .replace("{wave}", "1")
            .replace("{other}", "2");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        assert_eq!(delivered_count(root), 0);

        // As duas voltam juntas: nenhuma está mais em andamento.
        let both = format!("{}\n{}", delivered(root, 1, "A.", &["src/a.rs"]), delivered(root, 2, "B.", &["src/b.rs"]));
        let went = round(root, "x", Some(&both));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(last_commit(root).0, "feat(ondas-1-2): a onda 1 saiu");
    }

    /// O conserto que diz as ondas que fecha grava a entrega também nelas, o
    /// que pede a revisão de cada uma de novo sem mandá-las refazer; o commit
    /// é de conserto e leva as ondas consertadas.
    #[test]
    fn a_fix_records_the_delivery_on_the_waves_it_closes_and_asks_their_review_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let back = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&back, "dispatch"), vec![2], "{back}");
        // A onda 1 é reprovada com a única vaga ocupada pela 2: ela espera na
        // fila, e quem entrega o conserto dela é a 2.
        let rejected = round(root, "x", Some(&verdict(1, "rejected", "faltou o commit")));
        assert_eq!(waves_in(&rejected, "dispatch"), Vec::<u64>::new(), "{rejected}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(waves_to_redo(&log).contains(&1), "the rejected wave waits in the queue");

        std::fs::write(root.join("src/b.rs"), "fn um() {}\nfn conserto() {}\n").unwrap();
        let fix = line("DELIVERED", json!({"wave": 2, "text": "Consertei a onda 1.", "files": ["src/b.rs"],
            "commit": "o commit sai do resumo", "fixes": [1]}));
        let out = round(root, "x", Some(&fix));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "reviews"), vec![1, 2], "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "the fixed wave is not redone: {out}");
        let (subject, body) = last_commit(root);
        assert_eq!(subject, "fix(onda-2): o commit sai do resumo");
        assert_eq!(body, "- onda 2: o commit sai do resumo (conserta: onda 1)");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let visible = log.visible();
        let fixed_delivery = visible.iter().rfind(|e| e.event_type == "delivered" && e.wave() == Some(1)).unwrap();
        assert_eq!(fixed_delivery.str_field("text"), Some("Consertei a onda 1."));
        let commit = visible.iter().rfind(|e| e.event_type == "commit").unwrap();
        assert_eq!(commit.ints("waves"), vec![2, 1]);
    }

    /// A prova nova de um critério cujo teste mudou de nome vira a versão nova
    /// do critério, com o mesmo resto; a prova nova que sai verde sem rodar
    /// teste nenhum é avisada pelo código do critério.
    #[test]
    fn a_new_proof_becomes_the_criterions_new_version_and_one_that_runs_no_test_is_warned() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/lib.rs"], &[])]);
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        round(root, "x", None);
        std::fs::write(
            root.join("src/lib.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma_nova() { assert_eq!(1 + 1, 2); }\n}\n",
        )
        .unwrap();
        let proof = |name: &str| format!("cargo test --lib -- tests::{name} --exact");
        let report = |name: &str, summary: &str| {
            line("DELIVERED", json!({"wave": 1, "text": "O teste mudou de nome.", "files": ["src/lib.rs"],
                "commit": summary, "proofs": [{"criterion": "MSTD-CRIT-0001", "proof": proof(name)}]}))
        };
        let out = round(root, "x", Some(&report("soma_nova", "o teste muda de nome")));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("warnings").is_none(), "the right name runs a test: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let visible = log.visible();
        let criteria: Vec<&&SpecEvent> = visible.iter().filter(|e| e.event_type == "criterion").collect();
        assert_eq!(criteria.len(), 1, "the new version replaces the old one");
        assert_eq!(criteria[0].str_field("proof"), Some(proof("soma_nova").as_str()));
        assert_eq!(criteria[0].str_field("when"), Some("a onda roda"));
        assert!(criteria[0].int("replaces").is_some());
        assert_eq!(log.codes()[&criteria[0].id], "MSTD-CRIT-0001");

        std::fs::write(root.join("src/lib.rs"), "#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma_nova() {}\n}\n").unwrap();
        let out = round(root, "x", Some(&report("soma", "a prova errada")));
        assert_eq!(out["ok"], json!(true), "{out}");
        let expected = translate("round.proof_ran_no_test", Locale::PtBr).replace("{code}", "MSTD-CRIT-0001");
        assert_eq!(out["warnings"], json!([{"reason": "proof-ran-no-test", "hint": expected}]), "{out}");
    }

    /// A conferência antes do git é a da gravação inteira, contra a spec, e
    /// não só a da forma de cada linha: o veredito que aponta uma origem que a
    /// spec não tem é recusado antes do commit, sem commit e sem nada gravado,
    /// e a chamada corrigida faz o commit e grava a entrega uma vez só.
    #[test]
    fn a_line_the_spec_would_refuse_is_refused_before_the_commit() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let head = || {
            let out = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let spec_lines = || std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        let (seed, before) = (head(), spec_lines());

        let delivery = delivered(root, 1, "A soma saiu.", &["src/a.rs"]);
        let unknown_origin = line("VERDICT", json!({"wave": 1, "result": "approved", "text": "passou",
            "origin": 99_999, "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]}));
        let refused = round(root, "x", Some(&format!("{delivery}\n{unknown_origin}")));
        assert_eq!(refused["reason"], json!("unknown-target"), "{refused}");
        assert_eq!(head(), seed, "nothing was committed: {refused}");
        assert_eq!(spec_lines(), before, "nothing was recorded: {refused}");

        let went = round(root, "x", Some(&delivery));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_ne!(head(), seed, "the corrected call commits: {went}");
        assert_eq!(delivered_count(root), 1, "the corrected call records the delivery once");
    }
}
