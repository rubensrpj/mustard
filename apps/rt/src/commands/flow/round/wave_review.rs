//! O revisor de cada onda. A volta que passa nas conferências automáticas
//! espera, antes do commit, o veredito de um revisor de contexto limpo: a
//! rodada grava o pedido dele — o diff da onda contra a base dela, as tarefas,
//! o texto inteiro do que elas cobrem, as regras e decisões da onda, as regras
//! do projeto pelos arquivos tocados, as skills e as provas, com teto de
//! tamanho — num envio de papel próprio (`wave-review`), preso à onda e à
//! impressão do diff, e diz a quem conduz como lançá-lo. O veredito curto
//! volta pela gravação: aprovado vale como o aceite da revisão; reprovado
//! devolve a onda a um agente novo com o motivo, pelo caminho da reprovação de
//! quem conduz. Quem conduz lê só o veredito e os arquivos apontados.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use mustard_core::domain::spec_events::{BlockQuery, Hidden, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::domain::wave_prompt::{WAVE_REQUEST_TOKEN_CAP, project_rules_section, wave_files};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::RoundRefusal;
use super::commit::copy_of;
use super::report::WaveReport;
use super::review::Snapshot;
use crate::commands::spec_events::write::record;

/// O papel do envio que pede a revisão de uma onda.
pub(crate) const ROLE: &str = "wave-review";

/// O veredito do revisor de uma onda: a gravação o marca, e a revisão final
/// nunca o toma como dela.
pub(crate) fn is_wave_verdict(event: &SpecEvent) -> bool {
    event.event_type == "verdict" && event.fields.get("wave_review") == Some(&Value::Bool(true))
}

/// O começo do nome com que se lê o pedido do revisor de uma onda.
const REQUEST_PREFIX: &str = "review-request-";

/// O nome com que se lê o pedido do revisor da onda `wave`.
pub(crate) fn request_name(wave: u64) -> String {
    format!("{REQUEST_PREFIX}{wave}")
}

/// Se o despacho `prompt` manda ler o pedido do revisor de uma onda.
pub(crate) fn is_dispatch(prompt: &str) -> bool {
    prompt.contains(&format!("run read {REQUEST_PREFIX}"))
}

/// O título do despacho do revisor da onda `wave`, que nunca se lê como o de
/// um pedido de onda.
pub(crate) fn title(spec: &str, wave: u64, lang: Locale) -> String {
    format!("# {}", translate("round.wave_review.title", lang).replace("{spec}", spec).replace("{wave}", &wave.to_string()))
}

/// Os pedidos de revisão de onda abertos: de cada onda, o pedido mais novo,
/// enquanto nenhum veredito oficial do revisor de onda veio depois dele.
pub(crate) fn open_requests(log: &SpecLog) -> BTreeMap<u64, u64> {
    let visible = log.visible();
    let mut latest: BTreeMap<u64, u64> = BTreeMap::new();
    for sent in visible.iter().filter(|e| e.event_type == "send" && e.str_field("role") == Some(ROLE)) {
        if let Some(wave) = sent.int("reviews") {
            let newest = latest.entry(wave).or_insert(sent.id);
            *newest = (*newest).max(sent.id);
        }
    }
    latest.retain(|wave, sent| !visible.iter().any(|e| is_wave_verdict(e) && e.wave() == Some(*wave) && e.id > *sent));
    latest
}

/// O pedido gravado do revisor da onda `wave`, igual byte a byte.
pub(crate) fn wave_review_text(log: &SpecLog, wave: u64) -> Option<String> {
    let sent = log.visible().into_iter().filter(|e| e.event_type == "send" && e.str_field("role") == Some(ROLE) && e.int("reviews") == Some(wave))
        .max_by_key(|e| e.id)?;
    log.send_text(sent.id).map(str::to_string)
}

/// O veredito oficial aprovado do revisor da onda `wave` para o diff de
/// impressão `fingerprint`: o mais novo dos que responderam a um pedido com
/// essa impressão, quando aprovou.
pub(super) fn approved<'a>(log: &'a SpecLog, wave: u64, fingerprint: &str) -> Option<&'a SpecEvent> {
    let mut asked: Option<&SpecEvent> = None;
    let mut last = None;
    for event in log.visible() {
        if event.event_type == "send" && event.str_field("role") == Some(ROLE) && event.int("reviews") == Some(wave) {
            asked = Some(event);
        } else if is_wave_verdict(event) && event.wave() == Some(wave) && asked.is_some_and(|sent| sent.str_field("fingerprint") == Some(fingerprint)) {
            last = Some(event);
        }
    }
    last.filter(|verdict| verdict.str_field("result") == Some("approved"))
}

/// Como lançar o revisor de uma onda: o título, o comando que lê o pedido e
/// o recado a quem conduz.
pub(super) struct Asked {
    pub(super) title: String,
    pub(super) read: String,
    pub(super) hint: String,
}

/// O pedido do revisor da onda `wave`, gravado uma vez por diff: o pedido
/// aberto da mesma impressão vale, e o de outra impressão dá lugar a um novo.
pub(super) fn ask(root: &Path, spec: &str, log: &SpecLog, wave: &WaveReport, seen: &Snapshot, lang: Locale) -> Result<Asked, Refusal> {
    let title = title(spec, wave.wave, lang);
    let read = super::read_command(root, spec, &request_name(wave.wave));
    let open = open_requests(log).get(&wave.wave).and_then(|id| log.get(*id)).is_some_and(|sent| sent.str_field("fingerprint") == Some(seen.fingerprint.as_str()));
    if !open {
        let text = request(root, spec, log, wave, seen, &title, lang);
        let settings = mustard_core::ProjectConfig::load(root);
        let mut draft = Map::new();
        draft.insert("role".into(), json!(ROLE));
        draft.insert("reviews".into(), json!(wave.wave));
        draft.insert("fingerprint".into(), json!(seen.fingerprint));
        draft.insert("lines".into(), json!(text.lines().count()));
        draft.insert("chars".into(), json!(text.chars().count()));
        draft.insert("text".into(), json!(text));
        draft.insert("model".into(), json!(settings.wave_review_settings().model));
        draft.insert("effort".into(), json!(settings.wave_review_settings().effort));
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        draft.insert("author".into(), json!("binary"));
        record(root, spec, "send", draft, PhaseWriter::Binary)?;
    }
    let hint = translate("round.wave_review.required", lang).replace("{wave}", &wave.wave.to_string()).replace("{title}", &title).replace("{read}", &read);
    Ok(Asked { title, read, hint })
}

/// O texto do pedido: o que a onda devia fazer, o que valia para ela e o diff
/// dela, que corta por arquivo para o pedido caber no teto, com a lista do
/// que ficou de fora.
fn request(root: &Path, spec: &str, log: &SpecLog, wave: &WaveReport, seen: &Snapshot, title: &str, lang: Locale) -> String {
    let n = wave.wave;
    let codes = log.codes();
    let code = |event: &SpecEvent| codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
    // O item inteiro: o título e o texto ou, sem texto (o critério), cada
    // campo dele, sem os de registro.
    let line = |event: &SpecEvent| {
        let head = event.str_field("title").map(|title| format!(" {title}:")).unwrap_or_default();
        let body = event.str_field("text").map_or_else(|| {
            let skipped = ["v", "id", "code", "at", "type", "author", "origin", "search", "title", "replaces"];
            let fields = event.fields.iter().filter(|(key, _)| !skipped.contains(&key.as_str()));
            fields.map(|(key, value)| format!("{key}: {}", value.as_str().map_or_else(|| value.to_string(), str::to_string))).collect::<Vec<_>>().join("; ")
        }, |text| text.trim().to_string());
        format!("- {} ({}){head} {body}", code(event), event.event_type)
    };
    let section = |out: &mut String, key: &str, lines: Vec<String>| {
        if !lines.is_empty() {
            let _ = write!(out, "\n## {}\n\n{}\n", translate(key, lang), lines.join("\n"));
        }
    };
    let copy = copy_of(log, n).unwrap_or_else(|| root.to_path_buf());
    let command = format!("mustard-rt run write verdict --root {} --spec {spec} --json '<…>'", mustard_core::io::wave_prompt::shown(root));
    let mut out = format!("{title}\n\n{}\n", translate("round.wave_review.request", lang).replace("{wave}", &n.to_string())
        .replace("{copy}", &mustard_core::io::wave_prompt::shown(&copy)).replace("{command}", &command));
    let block = log.block(BlockQuery::Wave(n));
    let tasks: Vec<&SpecEvent> = block.iter().copied().filter(|e| e.event_type == "task").collect();
    section(&mut out, "round.wave_review.tasks", tasks.iter().map(|task| line(task)).collect());
    let planned_criteria = block.iter().filter(|e| e.event_type == "wave").flat_map(|wave| wave.ints("criteria"));
    let mut covered: Vec<u64> = tasks.iter().flat_map(|task| task.ints("covers")).chain(planned_criteria).collect();
    covered.sort_unstable();
    covered.dedup();
    section(&mut out, "round.wave_review.covered", covered.iter().filter_map(|id| log.get(*id)).map(line).collect());
    let sent = log.last_by_wave("send").get(&n).copied();
    let items = sent.and_then(|sent| log.send_field(sent, "items")).and_then(Value::as_array).cloned().unwrap_or_default();
    let rules = items.iter().filter_map(Value::as_u64).filter_map(|id| log.get(id)).filter(|e| matches!(e.event_type.as_str(), "rule" | "decision"));
    section(&mut out, "round.wave_review.rules", rules.map(line).collect());
    let files: Vec<String> = seen.changes.iter().map(|(file, _, _)| file.clone()).collect();
    if let Some(rules) = project_rules_section(&mustard_core::io::wave_prompt::project_rules(root, &files), lang) {
        let _ = write!(out, "\n{}\n", rules.trim_end());
    }
    let skills = sent.and_then(|sent| log.send_field(sent, "skills")).and_then(Value::as_array).cloned().unwrap_or_default();
    let skill = |skill: &Value| format!("- {} {}", skill["name"].as_str().unwrap_or_default(), skill["path"].as_str().map(|path| format!("(`{path}`)")).unwrap_or_default());
    section(&mut out, "round.wave_review.skills", skills.iter().map(skill).collect());
    let (steps, _) = super::finish_check::review_steps(log, n);
    let proofs = wave.proofs.iter().map(|(criterion, proof)| format!("- {criterion}: `{proof}`"));
    let steps = steps.iter().map(|step| format!("- {}: {}", step["item"].as_str().unwrap_or_default(), step["text"].as_str().unwrap_or_default()));
    section(&mut out, "round.wave_review.proofs", proofs.chain(steps).collect());
    let planned = wave_files(log, n);
    let _ = write!(out, "\n## {}\n", translate("round.wave_review.diff", lang));
    let room = (WAVE_REQUEST_TOKEN_CAP as usize * 4).saturating_sub(out.chars().count() + 2_000);
    let (mut used, mut left) = (0, Vec::new());
    for ((file, piece), (_, added, removed)) in seen.pieces.iter().zip(&seen.changes) {
        let outside = if planned.iter().any(|scope| crate::shared::dag::files_cross(scope, file)) { "" } else { " *" };
        let size = piece.chars().count();
        if used + size > room {
            left.push(format!("`{file}` (+{added} −{removed}){outside}"));
            continue;
        }
        used += size;
        out.push_str(piece);
    }
    if !left.is_empty() {
        let _ = write!(out, "\n{}\n", translate("round.wave_review.left_out", lang).replace("{files}", &left.join(", ")));
    }
    out
}

/// O que [`take_verdicts`] gravou, com o resultado, o motivo e os arquivos de
/// cada veredito, e cada reprovação, com a onda e o motivo.
type Taken = (Vec<Value>, Vec<(u64, String)>);

/// Assume o veredito que o revisor de cada onda gravou: o oficial, com
/// `replaces` para as voltas dele, fecha o pedido. O aprovado deixa a onda ir
/// ao commit pelo aceite que ele dá; o reprovado volta como a reprovação de
/// quem conduz, com o motivo e os arquivos que ele apontou. Devolve o que foi
/// gravado, com o resultado, o motivo e os arquivos, e as reprovações; com
/// algo gravado, `log` é lido de novo.
pub(super) fn take_verdicts(start: &Path, spec: &str, path: &Path, log: &mut SpecLog) -> Result<Taken, RoundRefusal> {
    let hidden = log.hidden();
    let mut recorded = Vec::new();
    let mut rejected = Vec::new();
    for (wave, asked) in open_requests(log) {
        let returns: Vec<&SpecEvent> = log.events.iter()
            .filter(|e| is_wave_verdict(e) && e.id > asked && e.wave() == Some(wave) && hidden.get(&e.id) == Some(&Hidden::Returned)).collect();
        let Some(last) = returns.last() else { continue };
        let skipped = ["v", "id", "code", "at", "type", "search", "author", "returned"];
        let mut draft: Map<String, Value> = last.fields.iter().filter(|(key, _)| !skipped.contains(&key.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        super::report::settle_verdict(log, &mut draft).map_err(RoundRefusal::Refused)?;
        let ids: Vec<u64> = returns.iter().map(|e| e.id).collect();
        draft.insert("replaces".into(), if ids.len() == 1 { json!(ids[0]) } else { json!(ids) });
        draft.insert("author".into(), json!("review"));
        let written = record(start, spec, "verdict", draft.clone(), PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
        let text = draft.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_string();
        let files: Vec<String> = draft.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
        let result = draft.get("result").cloned().unwrap_or_default();
        recorded.push(json!({"wave": wave, "type": "verdict", "id": written.written.id, "result": result, "text": text, "files": files}));
        if result == "rejected" && super::rejection::pending_return(log, wave).is_some() {
            let reason = if files.is_empty() { text } else { format!("{text} ({})", files.join(", ")) };
            rejected.push((wave, reason));
        }
    }
    if !recorded.is_empty() && let Some(fresh) = store::read(path).map_err(RoundRefusal::Refused)? {
        *log = fresh;
    }
    Ok((recorded, rejected))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::commit::copy_of;
    use super::super::tests::{approved, git_at, judged, returned, round, seed_review};
    use super::super::{RoundOpts, round_for};

    fn bare_round(root: &Path) -> Value {
        round_for(&RoundOpts { root: root.into(), spec: Some("x".into()), report: Some("<USAGE>{\"wave\":1}</USAGE>".into()) }, None)
    }

    fn log_of(root: &Path) -> mustard_core::domain::spec_events::SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// The wave-review requests recorded in the spec of `root`.
    fn review_sends(root: &Path) -> Vec<mustard_core::domain::spec_events::SpecEvent> {
        log_of(root).visible().into_iter().filter(|e| e.event_type == "send" && e.str_field("role") == Some("wave-review")).cloned().collect()
    }

    fn waiting(out: &Value) -> Value {
        out["warnings"].as_array().into_iter().flatten().find(|w| w["reason"] == "wave-review-required" && w["wave"] == 1).cloned()
            .unwrap_or_else(|| panic!("the wave does not wait for its review: {out}"))
    }

    /// Wave 1 changed `src/a.rs` in its copy and recorded its return; the
    /// project keeps `config` in `mustard.json` when given.
    fn returned_wave(root: &Path, config: Option<&str>) {
        returned_wave_with(root, config, &[]);
    }

    /// [`returned_wave`] with `extra` files written in the copy and returned.
    fn returned_wave_with(root: &Path, config: Option<&str>, extra: &[(&str, String)]) {
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        if let Some(config) = config {
            std::fs::write(root.join("mustard.json"), config).unwrap();
            git_at(root, &["commit", "-qam", "config"]);
        }
        round(root, "x", None);
        let copy = copy_of(&log_of(root), 1).unwrap();
        std::fs::write(copy.join("src/a.rs"), "fn changed() {}\n").unwrap();
        let mut files = vec!["src/a.rs"];
        for (file, text) in extra {
            std::fs::write(copy.join(file), text).unwrap();
            files.push(file);
        }
        let out = returned(root, json!({"wave":1,"text":"Concluída.","files":files,"commit":"Altera comportamento"}));
        assert_eq!(out["ok"], true, "{out}");
    }

    #[test]
    fn a_returned_wave_that_passes_the_checks_asks_for_the_wave_reviewer_and_not_for_reviewed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        returned_wave(root, None);
        let held = waiting(&bare_round(root));
        assert_eq!(held["review"]["agent"], "mustard-review", "{held}");
        assert_eq!(held["review"]["title"], "# x — revisão da onda 1", "{held}");
        let read = crate::commands::flow::round::read_command(root, "x", "review-request-1");
        assert_eq!(held["review"]["read"], json!(read), "{held}");
        let hint = held["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("mustard-review") && !hint.contains("REVIEWED"), "{hint}");
        let sent = review_sends(root);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!((sent[0].int("reviews"), sent[0].wave()), (Some(1), None), "the request is not the wave's send");
        assert_eq!((sent[0].str_field("model"), sent[0].str_field("effort")), (Some("sonnet"), Some("xhigh")));
        waiting(&bare_round(root));
        assert_eq!(review_sends(root).len(), 1, "the same diff is not asked twice");
        assert!(log_of(root).last_by_wave("delivered").is_empty());
    }

    #[test]
    fn the_wave_reviewer_reads_the_diff_the_tasks_the_whole_criteria_and_the_rules_within_a_cap() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        returned_wave_with(root, None, &[("src/big.rs", "// filler line for the cap\n".repeat(6_000))]);
        std::fs::write(root.join("CLAUDE.md"), "Use shared schemas; never duplicate types.\n").unwrap();
        waiting(&bare_round(root));
        let opts = crate::commands::spec_events::read::ReadOpts { root: root.into(), spec: Some("x".into()), block: "review-request-1".into(), term: None , session: None };
        let text = crate::commands::spec_events::read::read_at(&opts).unwrap_or_else(|refusal| panic!("{refusal}"));
        assert!(text.starts_with("# x — revisão da onda 1"), "{text}");
        let log = log_of(root);
        let criterion = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap();
        let task = log.visible().into_iter().find(|e| e.event_type == "task").unwrap();
        for expected in ["fn changed()", "MSTD-TASK-0001", task.str_field("text").unwrap(), "MSTD-CRIT-0001", criterion.str_field("then").unwrap(),
            "never duplicate types", "src/big.rs", "run write verdict"] {
            assert!(text.contains(expected), "{expected} missing: {text}");
        }
        assert!(!text.contains(&"// filler line for the cap\n".repeat(10)), "the big file stays out of the diff");
        let cap = mustard_core::domain::wave_prompt::WAVE_REQUEST_TOKEN_CAP;
        assert!(mustard_core::domain::wave_prompt::estimate_tokens(&text) <= cap, "{} chars", text.len());
    }

    #[test]
    fn an_approved_wave_review_verdict_lets_the_round_commit() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        returned_wave(root, None);
        waiting(&bare_round(root));
        let wrote = judged(root, json!({"wave":1,"result":"approved","text":"Conferi o diff e as regras."}));
        assert_eq!(wrote["ok"], true, "{wrote}");
        let out = bare_round(root);
        assert_eq!(out["ok"], true, "{out}");
        assert!(out["commit"].is_object(), "{out}");
        assert!(std::fs::read_to_string(root.join("src/a.rs")).unwrap().contains("changed"));
        let verdict = out["recorded"].as_array().into_iter().flatten().find(|r| r["type"] == "verdict").cloned().unwrap_or_else(|| panic!("{out}"));
        assert_eq!((verdict["wave"].clone(), verdict["result"].clone(), verdict["text"].clone()),
            (json!(1), json!("approved"), json!("Conferi o diff e as regras.")), "{out}");
        assert_eq!(crate::commands::flow::round::open_review(&log_of(root)), None);
    }

    #[test]
    fn a_rejected_wave_review_verdict_sends_the_wave_back_with_its_reason_and_one_model_up() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        returned_wave(root, None);
        waiting(&bare_round(root));
        let wrote = judged(root, json!({"wave":1,"result":"rejected","text":"Falta o teste do caso vazio.","files":["src/a.rs"]}));
        assert_eq!(wrote["ok"], true, "{wrote}");
        let out = bare_round(root);
        let back = out["warnings"].as_array().into_iter().flatten().find(|w| w["reason"] == "round-wave-rejected" && w["wave"] == 1);
        assert!(back.is_some(), "{out}");
        let verdict = out["recorded"].as_array().into_iter().flatten().find(|r| r["type"] == "verdict").cloned().unwrap_or_else(|| panic!("{out}"));
        assert_eq!((verdict["result"].clone(), verdict["files"].clone()), (json!("rejected"), json!(["src/a.rs"])), "{out}");
        let log = log_of(root);
        assert!(log.last_by_wave("delivered").is_empty(), "nothing was committed");
        let (_, reason) = super::super::rejection::rejection_of(&log, 1).expect("the wave went back");
        assert!(reason.contains("Falta o teste do caso vazio.") && reason.contains("src/a.rs"), "{reason}");
        assert_eq!(log.verdicts_by_wave()[&1].iter().filter(|v| v.str_field("result") == Some("rejected")).count(), 1);
        let up = super::super::queue::new_agent_escalation(root, &log, 1);
        assert!(up.contains_key("model") && up.contains_key("escalated_from"), "{up:?}");
    }

    #[test]
    fn a_wave_review_verdict_never_closes_the_final_review_nor_the_reverse() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        returned_wave(root, None);
        waiting(&bare_round(root));
        seed_review(root);
        crate::shared::spec_state::seed_event(root, "x", "verdict", json!({"author":"review","final":true,"result":"approved","text":"Obra conferida."}));
        let log = log_of(root);
        assert_eq!(crate::commands::flow::round::open_review(&log), None, "the final verdict closes the final review");
        assert!(super::open_requests(&log).contains_key(&1), "the final verdict leaves the wave review open");
        let asked = seed_review(root);
        let wave = judged(root, json!({"wave":1,"result":"approved","text":"Onda conferida."}));
        assert_eq!(wave["ok"], true, "{wave}");
        bare_round(root);
        let log = log_of(root);
        assert_eq!(crate::commands::flow::round::open_review(&log), Some(asked), "the wave verdict leaves the final review open");
        let official = log.visible().into_iter().rfind(|e| e.event_type == "verdict").unwrap();
        assert_eq!(official.fields.get("wave_review"), Some(&json!(true)), "{:?}", official.fields);
        let open = super::open_requests(&log);
        assert!(open.is_empty(), "{open:?}");
    }

    #[test]
    fn with_the_wave_reviewer_off_the_round_asks_for_reviewed_as_before() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        returned_wave(root, Some(r#"{"agents":{"wave_review":{"enabled":false}}}"#));
        let held = waiting(&bare_round(root));
        assert!(held["hint"].as_str().unwrap_or_default().contains("REVIEWED"), "{held}");
        assert!(held.get("review").is_none(), "{held}");
        assert!(review_sends(root).is_empty());
    }

    /// Wave 1 of `root` returned, was sent to its reviewer and came back
    /// rejected for `reason` on `src/a.rs`, and the round took the verdict.
    fn rejected_by_the_reviewer(root: &Path, reason: &str) {
        returned_wave(root, None);
        waiting(&bare_round(root));
        let wrote = judged(root, json!({"wave":1,"result":"rejected","text":reason,"files":["src/a.rs"]}));
        assert_eq!(wrote["ok"], true, "{wrote}");
        bare_round(root);
    }

    /// An earlier failure of another spec on the same file as wave 1.
    fn earlier_failure(root: &Path) {
        let earlier = mustard_core::io::failures::Failure {
            id: "antiga:9".into(),
            spec: "antiga".into(),
            wave: 2,
            at: "2026-10-01T10:00:00Z".into(),
            source: "conductor".into(),
            reason: "Esqueceu o teste do caso vazio.".into(),
            files: vec!["src/a.rs".into()],
            group: Some("antiga:9".into()),
            ..Default::default()
        };
        mustard_core::io::failures::append(root, &[earlier]).unwrap();
    }

    /// The skill-creation tasks recorded in spec `x` of `root`.
    fn skill_tasks(root: &Path) -> Vec<mustard_core::domain::spec_events::SpecEvent> {
        log_of(root).visible().into_iter().filter(|e| e.event_type == "task" && e.str_field("skill") == Some("skill-creator")).cloned().collect()
    }

    #[test]
    fn each_rejection_becomes_one_line_in_the_project_failures() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        rejected_by_the_reviewer(root, "Falta o teste do caso vazio.");
        bare_round(root);
        let rows = mustard_core::io::failures::read(root);
        assert_eq!(rows.len(), 1, "the same rejection is recorded once: {rows:?}");
        assert_eq!((rows[0].spec.as_str(), rows[0].wave, rows[0].source.as_str()), ("x", 1, "wave_review"));
        assert_eq!((rows[0].reason.as_str(), rows[0].files.clone()), ("Falta o teste do caso vazio.", vec!["src/a.rs".to_string()]));
        assert!(rows[0].id.starts_with("x:") && !rows[0].at.is_empty(), "{rows:?}");
        assert_eq!(rows[0].group.as_deref(), Some(rows[0].id.as_str()), "without the Jev the failure is its own group");

        let other = tempdir().unwrap();
        let root = other.path();
        returned_wave(root, None);
        let report = r#"<REJECTED>{"wave":1,"reason":"O teste não cobre o vazio."}</REJECTED>"#;
        round_for(&RoundOpts { root: root.into(), spec: Some("x".into()), report: Some(report.into()) }, None);
        let rows = mustard_core::io::failures::read(root);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((rows[0].source.as_str(), rows[0].reason.as_str()), ("conductor", "O teste não cobre o vazio."));
        assert_eq!(rows[0].files, ["src/a.rs"], "the files of the rejected return");
    }

    #[test]
    fn a_second_occurrence_of_the_same_failure_creates_the_skill_task_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        earlier_failure(root);
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let seen = std::rc::Rc::clone(&asked);
        crate::shared::jev::yes_no::ask_in_test(Some(std::rc::Rc::new(move |payloads: &[String]| {
            seen.borrow_mut().extend(payloads.iter().cloned());
            let answer = |payload: &String| {
                let body: Value = serde_json::from_str(payload).unwrap();
                let keys: Vec<String> = body["questions"].as_object().unwrap().keys().cloned().collect();
                json!({"answers": keys.iter().map(|key| (key.clone(), json!({"type":"noul","noul":0.92}))).collect::<serde_json::Map<_, _>>()})
            };
            Some(payloads.iter().map(answer).collect())
        })));
        rejected_by_the_reviewer(root, "Falta o teste do caso vazio.");
        bare_round(root);
        crate::shared::jev::yes_no::ask_in_test(None);

        let asked = asked.borrow();
        assert_eq!(asked.len(), 1, "one question for the new failure: {asked:?}");
        let body: Value = serde_json::from_str(&asked[0]).unwrap();
        assert_eq!(body["questions"]["f1"]["instructions"]["question"], "Is `failures.new` the same problem as `failures.f1`?");
        assert_eq!(body["state"]["failures"]["new"]["reason"], "Falta o teste do caso vazio.");
        assert_eq!(body["state"]["failures"]["f1"]["reason"], "Esqueceu o teste do caso vazio.");

        let rows = mustard_core::io::failures::read(root);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[1].group.as_deref(), Some("antiga:9"), "the yes joins the earlier group");
        let tasks = skill_tasks(root);
        assert_eq!(tasks.len(), 1, "one task for the group, never two: {tasks:?}");
        let title = tasks[0].str_field("title").unwrap_or_default();
        assert!(title.starts_with("Criar a skill: Esqueceu o teste do caso vazio") && title.chars().count() <= 70, "{title}");
        let agent = tasks[0].str_field("agent").unwrap_or_default();
        for expected in ["antiga", "Esqueceu o teste do caso vazio.", "Falta o teste do caso vazio.", "`src/a.rs`"] {
            assert!(agent.contains(expected), "{expected} missing: {agent}");
        }
        assert_eq!(tasks[0].fields["files"], json!([{"path": "src/a.rs"}]));
        let marked = rows[1].skill_task.clone().expect("the group records its task");
        assert_eq!((marked.spec.as_str(), marked.task), ("x", Some(tasks[0].id)));
    }

    #[test]
    fn a_rejection_in_the_group_that_motivated_a_skill_the_wave_got_creates_the_update_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        super::super::tests::approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let criterion = log_of(root).visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
            let task = json!({"wave": 1, "title": "Testar o vazio", "text": "Cobrir o caso vazio.", "files": [{"path": "src/a.rs"}],
                "depends_on": [], "covers": [criterion], "origin": said});
            assert_eq!(super::super::tests::write(root, "x", "task", task)["ok"], json!(true));
        });
        let motivated = mustard_core::io::failures::SkillTask { spec: "antiga".into(), task: Some(5) };
        let earlier = mustard_core::io::failures::Failure {
            id: "antiga:9".into(),
            spec: "antiga".into(),
            wave: 2,
            at: "2026-10-01T10:00:00Z".into(),
            source: "conductor".into(),
            reason: "Esqueceu o teste do caso vazio.".into(),
            files: vec!["src/a.rs".into()],
            group: Some("antiga:9".into()),
            skill_task: Some(motivated),
        };
        mustard_core::io::failures::append(root, &[earlier]).unwrap();
        let skill = root.join(".claude/skills/falta-teste");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: falta-teste\ndescription: Lembra o teste do caso vazio.\npaths:\n  - src/**\n---\n\nTeste o vazio.\n").unwrap();
        mustard_core::io::mustard_skills::change(root, |held| {
            held.record("falta-teste", "created", "2026-10-02T10:00:00Z", "Ganho de 2 de 3.", &["src/a.rs".into()], ("antiga", Some(3)), &["antiga:9".into()]);
        })
        .unwrap();
        round(root, "x", None);
        let copy = copy_of(&log_of(root), 1).unwrap();
        std::fs::write(copy.join("src/a.rs"), "fn changed() {}\n").unwrap();
        assert_eq!(returned(root, json!({"wave":1,"text":"Concluída.","files":["src/a.rs"],"commit":"Altera comportamento"}))["ok"], true);
        waiting(&bare_round(root));
        crate::shared::jev::yes_no::ask_in_test(Some(std::rc::Rc::new(|payloads: &[String]| {
            let answer = |payload: &String| {
                let body: Value = serde_json::from_str(payload).unwrap();
                let keys: Vec<String> = body["questions"].as_object().unwrap().keys().cloned().collect();
                json!({"answers": keys.iter().map(|key| (key.clone(), json!({"type":"noul","noul":0.92}))).collect::<serde_json::Map<_, _>>()})
            };
            Some(payloads.iter().map(answer).collect())
        })));
        let wrote = judged(root, json!({"wave":1,"result":"rejected","text":"Falta o teste do caso vazio.","files":["src/a.rs"]}));
        assert_eq!(wrote["ok"], true, "{wrote}");
        bare_round(root);
        crate::shared::jev::yes_no::ask_in_test(None);

        let rows = mustard_core::io::failures::read(root);
        assert_eq!(rows.last().and_then(|row| row.group.as_deref()), Some("antiga:9"), "{rows:?}");
        let updates: Vec<_> = skill_tasks(root).into_iter().filter(|task| task.str_field("title") == Some("Atualizar a skill falta-teste")).collect();
        assert_eq!(updates.len(), 1, "{:?}", skill_tasks(root));
        let agent = updates[0].str_field("agent").unwrap_or_default();
        assert!(agent.contains("Falta o teste do caso vazio.") && agent.contains("mesma falha"), "{agent}");
        let criterion = log_of(root).visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        assert_eq!(updates[0].ints("covers"), vec![criterion], "the update covers what the rejected wave's tasks covered: {:?}", updates[0].fields);
        assert_eq!(skill_tasks(root).len(), 1, "the group that already had its skill gets no second creation task");
    }

    #[test]
    fn with_the_jev_off_a_repeated_failure_gets_no_group_and_no_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        earlier_failure(root);
        rejected_by_the_reviewer(root, "Falta o teste do caso vazio.");
        let rows = mustard_core::io::failures::read(root);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[1].group.as_deref(), Some(rows[1].id.as_str()), "{rows:?}");
        assert!(skill_tasks(root).is_empty());
        assert!(rows.iter().all(|row| row.skill_task.is_none()), "{rows:?}");
    }
}
