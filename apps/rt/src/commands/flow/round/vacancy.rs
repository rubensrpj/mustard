//! A conta única das vagas da obra: quantas ondas novas saem agora. A vaga é
//! o agente que trabalha, e não a pasta presa: a onda que voltou e espera a
//! revisão, a resposta da rodada ou o commit guarda a cópia dela até o
//! commit, mas não segura o despacho da próxima. As cópias podem ser mais que
//! as vagas, até um teto por obra ([`copy_cap`]); só com o teto atingido a
//! onda que voltou segura o despacho, e a resposta da rodada diz qual. O
//! despacho, a montagem do backlog, a formação, a retomada, a fila declarada
//! e o painel contam por aqui, e só por aqui.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::io::wave_prompt::{shown, slot_path};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

use super::queue::{backlog_ready, max_parallel, next_waves, open_sends, orphaned_waves, unanswered_sends, waves_in_progress, waves_returned};
use super::stops::waves_replanned;
use crate::shared::paths::same_place;

/// Quantas cópias a obra guarda além das vagas (`maxCompilingWaves`). Cada
/// cópia tem a sua pasta de compilação: no repositório do Mustard, de 23 a
/// 31 GB cada uma, com 28 GB livres no disco em 18/10. Cabe uma a mais, e é
/// ela que deixa a onda que voltou esperar a revisão sem parar o despacho.
const EXTRA_COPIES: usize = 1;

/// O teto de cópias de uma obra com `capacity` vagas.
pub(crate) fn copy_cap(capacity: usize) -> usize {
    capacity + EXTRA_COPIES
}

/// As vagas da obra, numa conta só ([`vacancies`]).
#[derive(Debug, Clone, Default)]
pub(crate) struct Vacancies {
    /// Quantas ondas o projeto deixa compilar juntas (`maxCompilingWaves`).
    pub(crate) capacity: usize,
    /// O teto de cópias da obra ([`copy_cap`]).
    pub(crate) copies: usize,
    /// As ondas com envio aberto, cada uma com o número do envio
    /// ([`open_sends`]): em andamento, órfãs ou de volta.
    pub(crate) open: BTreeMap<u64, u64>,
    /// As ondas que ocupam vaga: em andamento, com o agente trabalhando ou
    /// por lançar, a órfã, que sai de novo com agente novo, e a que voltou
    /// com um conserto por fazer, que um agente trabalha na mesma cópia.
    pub(crate) working: BTreeSet<u64>,
    /// A onda replanejada com o envio aberto e sem volta: dona da vaga que o
    /// envio dela gravou, e sai de novo por ela.
    pub(crate) holding: BTreeSet<u64>,
    /// As ondas que voltaram e esperam a revisão, a resposta ou o commit:
    /// guardam a cópia, sem ocupar vaga.
    pub(crate) waiting: BTreeSet<u64>,
    /// Quantas cópias ainda cabem sob o teto: as pastas de vaga que nenhuma
    /// onda nem a revisão final segura.
    pub(crate) copies_free: usize,
}

impl Vacancies {
    /// As vagas sem agente: as do projeto menos as que trabalham e as que a
    /// onda replanejada segura.
    pub(crate) fn agents_free(&self) -> usize {
        self.capacity.saturating_sub(self.working.len() + self.holding.len())
    }

    /// Quantas ondas novas saem agora: as vagas sem agente que ainda têm
    /// cópia livre.
    pub(crate) fn free(&self) -> usize {
        self.agents_free().min(self.copies_free)
    }

    /// As ondas que voltaram e seguram o despacho: só quando o teto de cópias
    /// para o despacho antes das vagas, com cópia guardada por uma onda que
    /// espera. Fora disso, nenhuma.
    pub(crate) fn held_by_copies(&self) -> BTreeSet<u64> {
        if self.copies_free < self.agents_free() { self.waiting.clone() } else { BTreeSet::new() }
    }
}

/// As vagas da obra `spec`, lida em `log`, com as vagas do projeto em `root`.
pub(crate) fn vacancies(root: &Path, spec: &str, log: &SpecLog) -> Vacancies {
    vacancies_with(root, spec, log, max_parallel(root))
}

/// [`vacancies`] com `capacity` vagas, no lugar das do projeto.
pub(crate) fn vacancies_with(root: &Path, spec: &str, log: &SpecLog, capacity: usize) -> Vacancies {
    let returned = waves_returned(log);
    // A onda que voltou com o trecho de conserto gravado tem agente: o que a
    // fez ou o novo, na mesma cópia.
    let repairing: BTreeSet<u64> =
        returned.iter().copied().filter(|wave| super::fixes::fix_file(root, spec, log, *wave).is_some_and(|file| file.is_file())).collect();
    // A órfã sai de novo nesta mesma rodada, com agente novo: ocupa a vaga.
    let working: BTreeSet<u64> =
        waves_in_progress(log).into_keys().chain(orphaned_waves(log).into_keys()).chain(repairing.iter().copied()).collect();
    let replanned = waves_replanned(log);
    let holding: BTreeSet<u64> = unanswered_sends(log).into_keys().filter(|n| replanned.contains(n) && !returned.contains(n)).collect();
    let waiting: BTreeSet<u64> = returned.difference(&repairing).copied().collect();
    let copies = copy_cap(capacity);
    let held = super::slots::held_slots(root, spec, log);
    let copies_free = (0..copies).filter(|slot| !held.iter().any(|place| same_place(place, &shown(&slot_path(root, spec, *slot))))).count();
    Vacancies { capacity, copies, open: open_sends(log), working, holding, waiting, copies_free }
}

/// O aviso da resposta da rodada quando o teto de cópias segura o despacho
/// ([`Vacancies::held_by_copies`]) e ficou onda pronta sem sair — sem o teto
/// sairiam mais que as `sent` — ou tarefa pronta no backlog: quais ondas
/// seguram e o que as libera.
pub(super) fn copies_full_warning(log: &SpecLog, vacancies: &Vacancies, sent: usize, stuck: &BTreeMap<u64, Vec<&SpecEvent>>, lang: Locale) -> Option<Value> {
    let held = vacancies.held_by_copies();
    if held.is_empty() {
        return None;
    }
    let uncapped = Vacancies { copies_free: usize::MAX, ..vacancies.clone() };
    let left = next_waves(log, &uncapped, stuck).is_ok_and(|all| all.len() > sent) || !backlog_ready(log).is_empty();
    if !left {
        return None;
    }
    let waves: Vec<String> = held.iter().map(u64::to_string).collect();
    let hint = translate("round.copies_full", lang).replace("{copies}", &vacancies.copies.to_string()).replace("{waves}", &waves.join(", "));
    Some(json!({ "reason": "copies-full", "waves": held, "hint": hint }))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::domain::spec_state::State;
    use mustard_core::io::spec_events as store;
    use mustard_core::io::wave_prompt::recorded_copy;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::tests::{approved, launched, returned, round, warning_of, waves_in};
    use super::super::{RoundOpts, round_for};
    use super::*;

    /// A spec `x` como está gravada agora.
    fn spec_x(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// A rodada da spec `x` sem a revisão do condutor: a volta que chegou
    /// fica esperando a revisão do diff, fora do commit.
    fn unreviewed_round(root: &Path) -> Value {
        launched(root, "x", round_for(&RoundOpts { root: root.into(), spec: Some("x".into()), report: None }, None))
    }

    /// A onda `wave` volta com `file` mudado na cópia dela. Devolve a cópia.
    fn back_for_review(root: &Path, wave: u64, file: &str) -> String {
        let copy = recorded_copy(&spec_x(root), wave).expect("the copy of the wave").path;
        std::fs::write(Path::new(&copy).join(file), format!("fn changed_{wave}() {{}}\n")).unwrap();
        let out = returned(root, json!({"wave": wave, "text": "Concluída.", "files": [file], "commit": "Muda o comportamento"}));
        assert_eq!(out["ok"], true, "{out}");
        copy
    }

    /// Com uma vaga só, a onda 1 volta e espera a revisão do diff. A rodada
    /// seguinte despacha a onda 2 numa cópia própria, e a cópia da onda 1
    /// guarda o código dela até o commit.
    #[test]
    fn a_wave_back_for_review_frees_its_slot_and_keeps_its_copy() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let copy = back_for_review(root, 1, "src/a.rs");

        let out = unreviewed_round(root);
        warning_of(&out, "wave-review-required");
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "the wave back for review holds no slot: {out}");
        let second = recorded_copy(&spec_x(root), 2).expect("the copy of wave 2").path;
        assert_ne!(second, copy, "wave 2 never takes the copy of the wave back for review");
        assert_eq!(std::fs::read_to_string(Path::new(&copy).join("src/a.rs")).unwrap(), "fn changed_1() {}\n");
    }

    /// Com uma vaga só, o teto é de duas cópias. As ondas 1 e 2 voltam e
    /// esperam a revisão: nenhum agente trabalha, mas as duas cópias estão
    /// guardadas, e a onda 3 espera. A resposta diz quais ondas seguram.
    #[test]
    fn the_copy_cap_holds_dispatch_and_the_answer_names_the_waves_keeping_copies() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        back_for_review(root, 1, "src/a.rs");
        assert_eq!(waves_in(&unreviewed_round(root), "dispatch"), vec![2]);
        back_for_review(root, 2, "src/b.rs");

        let out = unreviewed_round(root);
        assert!(waves_in(&out, "dispatch").is_empty(), "the copy cap holds wave 3: {out}");
        let full = warning_of(&out, "copies-full");
        assert_eq!(full["waves"], json!([1, 2]), "{out}");
        let hint = full["hint"].as_str().unwrap_or_default();
        assert!(hint.contains('2') && out["next"].as_str().is_some_and(|next| next.contains(hint)), "{out}");
    }

    /// O despacho, a retomada, a fila declarada e o painel dizem a mesma vaga
    /// livre: com duas vagas, a onda 1 trabalhando e a 2 de volta, sobra uma,
    /// e é a onda 3 que sai.
    #[test]
    fn dispatch_resume_queue_and_panel_count_the_same_free_slots() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        back_for_review(root, 2, "src/b.rs");

        let log = spec_x(root);
        assert_eq!(vacancies(root, "x", &log).free(), 1, "the round");
        assert_eq!(crate::commands::panel::progress::slots(root, "x", &log), (2, 1), "the panel");
        assert_eq!(super::super::scheduling::snapshot(root, "x", &log)["free_wave_slots"], json!(1), "the declared queue");
        let step = crate::commands::flow::resume::next_step(root, "running", "x", &State::from_log(&log), &log, Locale::EnUs);
        assert_eq!(step["text"], json!(translate("resume.step.dispatch_one", Locale::EnUs).replace("{waves}", "3")), "the resume: {step}");
        assert_eq!(waves_in(&unreviewed_round(root), "dispatch"), vec![3], "the dispatch");
    }
}
