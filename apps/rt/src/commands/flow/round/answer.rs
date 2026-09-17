//! A resposta da rodada e o próximo passo: a recusa, com a mensagem no idioma
//! do projeto, e o caminho de uma chamada — conferir a fase, fechar o que
//! voltou, despachar as ondas prontas, pedir as revisões e dizer o que fazer
//! em seguida.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::spec_events::{Refusal, DELIVERED_MAX_CHARS};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State};
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{prompts, Flight};
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::commit::git_lock;
use super::queue::{first_unfinished, max_parallel, next_waves, open_copies, reviews_due, sent_items, waves_in_progress};
use super::report::{take_report, Taken};
use super::stops::{change_question, stopped_waves, waves_stuck};
use super::{can_run, RoundOpts, DONE_STEP};
use crate::commands::spec_events::{read::checkout, write::record};
use crate::shared::spec_state::DiskSpecState;

/// Por que a rodada não correu.
pub(crate) enum RoundRefusal {
    /// Uma recusa do arquivo de eventos.
    Refused(Refusal),
    /// Uma linha do relatório não se entende.
    BadReport { detail: String },
    /// O relatório não traz nenhuma linha de entrega nem de veredito.
    LineMissing,
    /// Uma linha do relatório não traz um campo obrigatório.
    LineField { line: &'static str, field: &'static str },
    /// A entrega de uma onda conflita com o repositório principal: os
    /// trechos, a cópia em que se resolve e o commit atual.
    MergeConflict { wave: u64, copy: String, conflicts: Vec<String>, head: String },
    /// Um arquivo entregue não está no disco nem é conhecido do git.
    FileUnknown { file: String, wave: u64 },
    /// A spec ainda não foi aprovada.
    NotApproved { phase: String },
    /// O que uma onda entregou passa do teto de caracteres.
    DeliveredTooLong { wave: u64, chars: usize },
    /// A mensagem do commit não cabe no modelo.
    CommitTooLong { part: String, chars: usize, max: usize },
    /// A mensagem do commit traz o que ela nunca leva.
    CommitForbidden { found: String },
    /// Um agente disse que o plano da onda não funciona: a rodada para e
    /// mostra a mudança proposta, com a pergunta que decide.
    Replan { wave: u64, change: String, code: String },
    /// O git recusou o commit.
    Git { detail: String },
}

impl RoundRefusal {
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::Refused(refusal) => refusal.reason().to_string(),
            Self::BadReport { .. } => "round-bad-report".into(),
            Self::LineMissing => "round-line-missing".into(),
            Self::LineField { .. } => "round-line-field-missing".into(),
            Self::MergeConflict { .. } => "round-merge-conflict".into(),
            Self::FileUnknown { .. } => "round-file-unknown".into(),
            Self::NotApproved { .. } => "round-not-approved".into(),
            Self::DeliveredTooLong { .. } => "delivered-too-long".into(),
            Self::CommitTooLong { .. } => "commit-too-long".into(),
            Self::CommitForbidden { .. } => "commit-forbidden-text".into(),
            Self::Replan { .. } => "wave-plan-does-not-work".into(),
            Self::Git { .. } => "git-refused".into(),
        }
    }

    pub(crate) fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::Refused(refusal) => refusal.message(lang),
            Self::BadReport { detail } => fill("round.bad_report", &[("{detail}", detail.clone())]),
            Self::LineMissing => fill("round.line_missing", &[]),
            Self::LineField { line, field } => {
                fill("round.line_field", &[("{line}", (*line).to_string()), ("{field}", (*field).to_string())])
            }
            Self::MergeConflict { wave, copy, conflicts, head } => fill(
                "round.merge_conflict",
                &[
                    ("{wave}", wave.to_string()),
                    ("{conflicts}", conflicts.join(", ")),
                    ("{copy}", copy.clone()),
                    ("{head}", head.clone()),
                ],
            ),
            Self::FileUnknown { file, wave } => {
                fill("round.file_unknown", &[("{file}", file.clone()), ("{wave}", wave.to_string())])
            }
            Self::NotApproved { phase } => fill("round.not_approved", &[("{phase}", phase.clone())]),
            Self::DeliveredTooLong { wave, chars } => fill(
                "round.delivered_too_long",
                &[
                    ("{wave}", wave.to_string()),
                    ("{chars}", chars.to_string()),
                    ("{max}", DELIVERED_MAX_CHARS.to_string()),
                ],
            ),
            Self::CommitTooLong { part, chars, max } => fill(
                "round.commit_too_long",
                &[("{part}", part.clone()), ("{chars}", chars.to_string()), ("{max}", max.to_string())],
            ),
            Self::CommitForbidden { found } => {
                fill("round.commit_forbidden", &[("{found}", found.clone())])
            }
            Self::Replan { wave, change, code } => fill(
                "round.replan",
                &[
                    ("{wave}", wave.to_string()),
                    ("{change}", change.clone()),
                    ("{question}", change_question(code, lang)),
                    ("{yes}", translate("change.accept", lang).to_string()),
                    ("{no}", translate("change.decline", lang).to_string()),
                ],
            ),
            Self::Git { detail } => fill("round.git_refused", &[("{detail}", detail.clone())]),
        }
    }

    pub(crate) fn to_value(&self, lang: Locale) -> Value {
        let mut out = json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) });
        // A pergunta da mudança vai pronta, com as opções, como a revisão de
        // um bloco do levantamento: é ela, e só ela, que a testemunha lê.
        if let Self::Replan { code, .. } = self {
            out["question"] = json!(change_question(code, lang));
            out["options"] = json!([translate("change.accept", lang), translate("change.decline", lang)]);
        }
        out
    }
}

pub(super) fn run_round(
    opts: &RoundOpts,
    root: &Path,
    lang: Locale,
    session: Option<&str>,
) -> Result<Value, RoundRefusal> {
    let refuse = RoundRefusal::Refused;
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root))
            .active(session)
            .ok_or_else(|| refuse(Refusal::NoCurrentSpec))?,
    };
    let path = store::spec_file(root, &spec).map_err(RoundRefusal::Refused)?;
    let log = store::read(&path)
        .map_err(RoundRefusal::Refused)?
        .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))?;

    // Só uma spec aprovada roda. Antes disso a rodada não tem o que despachar.
    let phase = State::from_log(&log).phase.unwrap_or_default().to_string();
    if !can_run(&phase) {
        return Err(RoundRefusal::NotApproved { phase });
    }

    let Taken { mut recorded, formatted, mut warnings, commit } =
        match opts.report.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            Some(raw) => take_report(&opts.root, root, &spec, raw, &log, lang)?,
            None => Taken { recorded: Vec::new(), formatted: Vec::new(), warnings: Vec::new(), commit: None },
        };

    // O despacho — a entrada na execução, a leitura da spec, a escolha das
    // ondas, a criação das cópias e a gravação dos envios — roda inteiro com a
    // trava do passo do git presa: a rodada que chega ao mesmo tempo só lê a
    // spec depois dos envios desta, e nunca solta a mesma onda de novo.
    let held_lock = git_lock(root)?;
    // A primeira rodada leva a spec para a execução.
    let entering = phase == "approved"
        && crate::commands::spec_events::write::record_phase(&opts.root, &spec, "running", session);

    let log = store::read(&path)
        .map_err(RoundRefusal::Refused)?
        .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.clone() }))?;
    let codes = log.codes();

    // O despacho da rodada seguinte: as ondas prontas, no máximo o que o
    // projeto deixa compilar ao mesmo tempo contando as que já estão em
    // andamento, e nenhuma que a onda parada pelo limite de consertos segura.
    // Cada uma sai com a sua cópia e a sua pasta de compilação.
    let running = waves_in_progress(&log);
    let stuck = waves_stuck(&log);
    let ready = next_waves(&log, max_parallel(root), &running, &stuck);
    let (copies, not_copied) = open_copies(root, &spec, &log, &held_lock, &ready, &running, lang);
    warnings.extend(not_copied);
    let next: Vec<u64> = ready.into_iter().filter(|wave| copies.contains_key(wave)).collect();
    // O pedido de cada onda lista as outras em andamento, contando as que
    // saem junto com ela nesta rodada, e traz a cópia dela.
    let flight = Flight { running: running.keys().chain(&next).copied().collect(), copies };
    let built = prompts(root, &spec, &log, lang, &flight);
    let mut dispatched: Vec<Value> = Vec::new();
    let mut in_flight: BTreeMap<u64, String> = running
        .iter()
        .map(|(wave, sent)| (*wave, codes.get(sent).cloned().unwrap_or_else(|| sent.to_string())))
        .collect();
    for wave in &next {
        let Some(prompt) = built.iter().find(|p| p.wave == *wave) else { continue };
        let mut draft = Map::new();
        draft.insert("wave".into(), json!(wave));
        draft.insert("role".into(), json!("wave"));
        draft.insert("text".into(), json!(prompt.text));
        draft.insert("lines".into(), json!(prompt.lines));
        draft.insert("chars".into(), json!(prompt.text.chars().count()));
        draft.insert("items".into(), json!(sent_items(&log, *wave)));
        draft.insert("mustard".into(), json!(env!("CARGO_PKG_VERSION")));
        if let Some(copy) = flight.copies.get(wave) {
            draft.insert("copy".into(), json!(copy.path));
            if let Some(dir) = &copy.build_dir {
                draft.insert("build_dir".into(), json!(dir));
            }
        }
        draft.insert("author".into(), json!("binary"));
        let written = record(&opts.root, &spec, "send", draft, PhaseWriter::Binary)
            .map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
        dispatched.push(json!({ "wave": wave, "lines": prompt.lines, "prompt": prompt.text }));
        let code = written.written.code.clone().unwrap_or_else(|| written.written.id.to_string());
        in_flight.insert(*wave, code);
    }
    drop(held_lock);
    let reviews = reviews_due(&log, &built);

    // O próximo passo: despachar o que saiu agora; esperar as que estão em
    // andamento; fechar, com tudo entregue e aprovado; ou dizer qual onda
    // falta, quando nada se move.
    let report_back = translate("round.report", lang);
    let mut command: Option<String> = None;
    let then = if !dispatched.is_empty() || !reviews.is_empty() {
        format!("{} {report_back}", translate("round.next", lang))
    } else if !running.is_empty() {
        let waves: Vec<String> = running.keys().map(u64::to_string).collect();
        format!("{} {report_back}", translate("round.waiting", lang).replace("{waves}", &waves.join(", ")))
    } else if !stuck.is_empty() {
        String::new()
    } else if let Some(wave) = first_unfinished(&log, &running) {
        translate("round.missing", lang).replace("{wave}", &wave.to_string())
    } else {
        let state = State::from_log(&log);
        let close = crate::commands::flow::resume::step_command(DONE_STEP, &spec, &state);
        let text = translate("round.close", lang).replace("{command}", close.as_deref().unwrap_or_default());
        command = close;
        text
    };
    // A pergunta da onda parada vem antes do resto, que segue sem ela.
    let (stopped, asked) = stopped_waves(&stuck, &codes, lang);
    let then = asked.into_iter().chain(Some(then).filter(|t| !t.is_empty())).collect::<Vec<_>>().join(" ");

    // A página sai no fim do passo, uma vez, e a rodada manda publicá-la,
    // menos quando ela não pôde ser refeita.
    let pages = crate::commands::spec_events::pages::refresh(root, &spec, lang);

    // Com o pull request aberto, o corpo dele é refeito aqui: ele é montado do
    // mesmo arquivo de eventos que acabou de mudar, e um corpo que descreve a
    // rodada anterior é pior do que nenhum — foi por isso que existiu um portão
    // só para reparar que ele tinha envelhecido.
    let rewritten = rewrite_open_pr(root, &spec);

    let running: Vec<Value> =
        in_flight.iter().map(|(wave, send)| json!({ "wave": wave, "send": send })).collect();
    let mut out = json!({
        "ok": true,
        "spec": spec,
        "recorded": recorded,
        "formatted": formatted,
        "dispatch": dispatched,
        "reviews": reviews,
        "running": running,
    });
    if entering {
        out["phase"] = json!("running");
    }
    if !stopped.is_empty() {
        out["stopped"] = json!(stopped);
    }
    if let Some(commit) = commit {
        out["commit"] = commit;
    }
    if let Ok(pages) = &pages {
        out["md"] = json!(pages.md);
        out["html"] = json!(pages.html);
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    crate::commands::spec_events::pages::end_milestone(&mut out, pages.as_ref(), "round", &then, lang);
    if let Some(command) = command {
        out["command"] = json!(command);
    }
    if let Some(number) = rewritten {
        out["pr"] = json!({ "number": number, "body": "rewritten" });
    }
    Ok(out)
}

/// Refaz o corpo do pull request desta spec, quando há um aberto. Devolve o
/// número do pull request reescrito, `None` quando não há nenhum ou quando o
/// provedor não respondeu — a rodada nunca para por causa disso.
///
/// O pull request é o da branch DESTA spec, não o da branch em que o checkout
/// está. Fora da branch da spec não há o que refazer aqui, e perguntar pelo
/// checkout reescreveria o corpo do pull request de outra unidade.
fn rewrite_open_pr(root: &Path, spec: &str) -> Option<u64> {
    let branch = crate::commands::spec_events::write::branch_of_spec(root, spec)?;
    let (_, body) = crate::commands::review::pr_publish::message_of(root, spec).ok()?;
    let provider = crate::shared::pr_provider::provider_for(root);
    crate::commands::review::pr_publish::rewrite_body(provider.as_ref(), &branch, &body)
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::SpecEvent;
    use tempfile::tempdir;

    use crate::commands::spec_events::write::record_open;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A spec que ainda não foi aprovada não roda onda nenhuma.
    #[test]
    fn a_spec_that_is_not_approved_yet_dispatches_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("round-not-approved"), "{refused}");
    }

    /// A primeira rodada leva a spec para a execução, grava o envio de cada
    /// onda com o pedido exato e devolve o pedido pronto para injetar.
    #[test]
    fn the_first_round_records_what_it_injected_and_marks_the_spec_running() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["phase"], json!("running"), "{out}");
        let dispatched = out["dispatch"].as_array().cloned().unwrap_or_default();
        assert_eq!(dispatched.len(), 1, "{out}");
        let prompt = dispatched[0]["prompt"].as_str().unwrap_or_default().to_string();
        assert!(prompt.contains("--term MSTD-TASK-0001"), "{prompt}");

        // O envio gravado guarda o pedido exato, letra por letra.
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let sent: Vec<&SpecEvent> =
            log.visible().into_iter().filter(|e| e.event_type == "send").collect();
        assert_eq!(sent.len(), 1, "um envio por onda despachada");
        assert_eq!(sent[0].str_field("text"), Some(prompt.as_str()));
        assert_eq!(sent[0].wave(), Some(1));
    }

    /// O pedido da onda nova traz os comandos do projeto e a outra onda que
    /// sai junto, com o arquivo dela. O do conserto traz também o veredito,
    /// a entrega anterior e a decisão gravada depois do envio; a revisão do
    /// conserto, as mesmas linhas, a entrega dele e a cópia no commit dele.
    #[test]
    fn the_round_assembles_the_new_request_the_fix_request_and_its_review() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"buildCommand":"make","testCommand":"make test"}"#).unwrap();
        let text = |out: &Value, field: &str, wave: u64| -> String {
            let found = out[field].as_array().into_iter().flatten().find(|d| d["wave"] == json!(wave));
            found.and_then(|d| d["prompt"].as_str()).unwrap_or_default().to_string()
        };
        let first = text(&round(root, "x", None), "dispatch", 1);
        for line in ["- Compile com `make`.", "- Teste com `make test`.", "  - Onda 2: `src/b.rs`"] {
            assert!(first.contains(line), "{line}: {first}");
        }
        assert!(!first.contains("## Conserto"), "{first}");

        round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        write(root, "x", "decision", json!({"author": "user", "text": "A soma aceita negativos.", "keys": ["soma"],
            "why": "o usuário pediu", "waves": [1]}));
        let fix = text(&round(root, "x", Some(&verdict(1, "rejected", "faltou o teste"))), "dispatch", 1);
        let heading = format!("## Conserto\n\n{}", translate("prompt.fix.wave", Locale::PtBr));
        assert!(fix.contains(&heading), "{fix}");
        for code in ["--term MSTD-VERD-0001`", "--term MSTD-DELIV-0001`", "--term MSTD-DEC-0001`"] {
            assert!(fix.split("\n## ").nth(1).unwrap_or_default().contains(code), "{code}: {fix}");
        }

        let back = round(root, "x", Some(&delivered(root, 1, "Teste acrescentado.", &["src/a.rs"])));
        let review = text(&back, "reviews", 1);
        let sha = back["commit"]["sha"].as_str().unwrap_or_default();
        assert!(review.contains(&format!("## Conserto\n\n{}", translate("prompt.fix.review", Locale::PtBr))), "{review}");
        let fix_part = review.split("\n## ").nth(1).unwrap_or_default();
        for line in ["--term MSTD-VERD-0001`", "--term MSTD-DELIV-0001`", "--term MSTD-DEC-0001`"] {
            assert!(fix_part.contains(line), "{line}: {review}");
        }
        assert!(review.contains("## O que esta onda entregou\n\n- MSTD-DELIV-0002 (entregou) — `mustard-rt run read waves --spec x --term MSTD-DELIV-0002`\n\n"), "{review}");
        let review_copy = mustard_core::io::wave_prompt::shown(&mustard_core::io::wave_prompt::copy_path(root, "x", 1, true));
        assert!(!sha.is_empty() && review.contains(&format!("--detach {review_copy} {sha}`")), "{sha}: {review}");
    }

    /// A rodada diz o próximo passo de cada situação: despachar o que saiu;
    /// esperar as ondas em andamento; dizer qual onda falta quando nada se
    /// move; e, com todas as ondas entregues e aprovadas, fechar — com a
    /// linha do fechamento pronta, que o binário aceita.
    #[test]
    fn the_round_answers_close_when_every_wave_is_approved_and_names_the_missing_one() {
        let report_back = translate("round.report", Locale::PtBr);

        // A cópia da única onda não pode ser criada: nada sai, o aviso diz por
        // quê, e a resposta diz que falta a onda 1. Desfeito o impedimento, a
        // onda sai na rodada seguinte.
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let blocked = mustard_core::io::wave_prompt::copy_path(root, "x", 1, false);
        std::fs::create_dir_all(blocked.parent().unwrap()).unwrap();
        std::fs::write(&blocked, b"no caminho da copia").unwrap();
        let held = round(root, "x", None);
        assert_eq!(held["ok"], json!(true), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), Vec::<u64>::new(), "{held}");
        assert_eq!(held["running"], json!([]), "{held}");
        let warnings = held["warnings"].as_array().cloned().unwrap_or_default();
        assert_eq!(warnings.len(), 1, "{held}");
        assert_eq!((&warnings[0]["reason"], &warnings[0]["wave"]), (&json!("copy-not-created"), &json!(1)), "{held}");
        let failed = translate("round.copy_failed", Locale::PtBr).replace("{wave}", "1");
        let (before, after) = failed.split_once("{detail}").unwrap();
        let hint = warnings[0]["hint"].as_str().unwrap_or_default();
        assert!(hint.starts_with(before) && hint.ends_with(after) && hint.len() > failed.len(), "{hint}");
        let missing = translate("round.missing", Locale::PtBr).replace("{wave}", "1");
        assert!(held["next"].as_str().unwrap_or_default().ends_with(&missing), "{held}");
        assert!(held.get("command").is_none(), "{held}");
        std::fs::remove_file(&blocked).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);

        let first = round(root, "x", None);
        let next = first["next"].as_str().unwrap_or_default();
        assert!(next.ends_with(&format!("{} {report_back}", translate("round.next", Locale::PtBr))), "{first}");
        assert!(first.get("command").is_none(), "{first}");

        let waiting = round(root, "x", None);
        let next = waiting["next"].as_str().unwrap_or_default();
        let expected = translate("round.waiting", Locale::PtBr).replace("{waves}", "1, 2");
        assert!(next.ends_with(&format!("{expected} {report_back}")), "{waiting}");
        assert!(waiting.get("command").is_none(), "{waiting}");

        // As duas voltam; a 1 aprovada, a 2 reprovada. O conserto da 2 sai,
        // e o plano dela muda depois: o conserto sai de novo, com o plano
        // atual, e a rodada manda despachá-lo.
        let both = format!("{}\n{}", delivered(root, 1, "Saiu.", &["src/a.rs"]), delivered(root, 2, "Saiu.", &["src/b.rs"]));
        let back = round(root, "x", Some(&both));
        assert_eq!(waves_in(&back, "reviews"), vec![1, 2], "{back}");
        let judged = format!("{}\n{}", verdict(1, "approved", "passou"), verdict(2, "rejected", "faltou"));
        let fix = round(root, "x", Some(&judged));
        assert_eq!(waves_in(&fix, "dispatch"), vec![2], "{fix}");
        replan(root, 2);
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![2], "{again}");
        let next = again["next"].as_str().unwrap_or_default();
        assert!(next.ends_with(&format!("{} {report_back}", translate("round.next", Locale::PtBr))), "{again}");
        assert!(again.get("command").is_none(), "{again}");

        // O conserto volta aprovado: tudo entregue e aprovado, a rodada manda
        // fechar.
        round(root, "x", Some(&delivered(root, 2, "Consertou.", &["src/b.rs"])));
        let done = round(root, "x", Some(&verdict(2, "approved", "passou")));
        assert_eq!(done["ok"], json!(true), "{done}");
        let command = done["command"].as_str().unwrap_or_default();
        assert_eq!(command, "mustard-rt run close --spec x", "{done}");
        crate::commands::flow::resume::assert_parses(command);
        let close = translate("round.close", Locale::PtBr).replace("{command}", command);
        assert!(done["next"].as_str().unwrap_or_default().ends_with(&close), "{done}");
    }

    /// Duas rodadas ao mesmo tempo, sem relatório, com uma onda pronta. As
    /// duas chegam ao despacho enquanto outro passo do git segura a trava;
    /// solta a trava, uma despacha a onda, e a outra lê a spec depois do envio
    /// dela: vê a onda em andamento e não a solta de novo. A onda tem um envio
    /// só, as duas respostas a mostram em andamento com esse envio, e a spec
    /// entra na execução uma vez.
    #[test]
    fn two_rounds_at_the_same_time_dispatch_a_wave_only_once() {
        use std::time::Duration;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let outs: Vec<Value> = std::thread::scope(|scope| {
            let Ok(lock) = super::git_lock(root) else { panic!("the git lock") };
            let rounds = [scope.spawn(|| round(root, "x", None)), scope.spawn(|| round(root, "x", None))];
            // O tempo de as duas lerem a spec e chegarem à trava.
            std::thread::sleep(Duration::from_millis(1000));
            drop(lock);
            rounds.into_iter().map(|r| r.join().unwrap()).collect()
        });
        for out in &outs {
            assert_eq!(out["ok"], json!(true), "{outs:?}");
        }
        let dispatched: Vec<u64> = outs.iter().flat_map(|out| waves_in(out, "dispatch")).collect();
        assert_eq!(dispatched, vec![1], "only one round sends the wave out: {outs:?}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let visible = log.visible();
        let sends: Vec<String> = visible.iter().filter(|e| e.event_type == "send").map(|e| codes[&e.id].clone()).collect();
        assert_eq!(sends.len(), 1, "the wave has one send: {outs:?}");
        for out in &outs {
            assert_eq!(out["running"], json!([{"wave": 1, "send": sends[0]}]), "{outs:?}");
        }
        let entered = visible.iter().filter(|e| e.event_type == "state" && e.str_field("phase") == Some("running"));
        assert_eq!(entered.count(), 1, "the spec enters the run once: {outs:?}");
    }
}
