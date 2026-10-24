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

use super::queue::{backlog_ready, max_parallel, open_sends, orphaned_waves, outgoing, unanswered_sends, waves_in_progress};
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
    /// As ondas do plano que voltaram e esperam a revisão, a resposta ou o
    /// commit: guardam a cópia, sem ocupar vaga. A que saiu do plano por
    /// ficar sem tarefa viva não conta.
    pub(crate) waiting: BTreeSet<u64>,
    /// Quantas cópias ainda cabem sob o teto: as pastas de vaga que nenhuma
    /// onda nem a revisão final segura.
    pub(crate) copies_free: usize,
    /// As pastas de vaga presas ([`super::slots::held_slots`]), cada uma pelo
    /// caminho como o envio a grava.
    held: BTreeSet<String>,
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

    /// `true` quando a pasta `copy` estava livre antes da rodada: a onda que a
    /// recebe toma uma das cópias que ainda cabiam sob o teto. A onda que volta
    /// à vaga que o envio dela gravou, a replanejada ou a de cópia apagada,
    /// reaproveita uma pasta presa e não toma cópia nenhuma.
    pub(crate) fn takes_free_copy(&self, copy: &str) -> bool {
        !self.held.iter().any(|place| same_place(place, copy))
    }
}

/// As vagas da obra `spec`, lida em `log`, com as vagas do projeto em `root`.
pub(crate) fn vacancies(root: &Path, spec: &str, log: &SpecLog) -> Vacancies {
    vacancies_with(root, spec, log, max_parallel(root))
}

/// [`vacancies`] com `capacity` vagas, no lugar das do projeto.
pub(crate) fn vacancies_with(root: &Path, spec: &str, log: &SpecLog, capacity: usize) -> Vacancies {
    // A onda que perdeu a última tarefa depois de voltar saiu do plano: a volta
    // dela segue pendente no arquivo, mas a cópia dela já não é sua
    // ([`super::slots::held_slots`] só conta as do plano) e o código que ela
    // tinha fica guardado quando a vaga passa a outra onda. Sem tarefa viva,
    // ela não espera nada, não conserta nada e não segura o despacho: a vaga
    // conta só as voltas que esperam a rodada.
    let returned: BTreeSet<u64> = log.returns_awaiting_round().into_iter().filter_map(SpecEvent::wave).collect();
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
    Vacancies { capacity, copies, open: open_sends(log), working, holding, waiting, copies_free, held }
}

/// O aviso da resposta da rodada quando o teto de cópias segura o despacho
/// ([`Vacancies::held_by_copies`]): as ondas que saem de fato tomam as
/// últimas cópias livres e ficou o que sairia sem o teto — mais ondas do
/// plano, ou a tarefa pronta do backlog, com vaga de agente sobrando. A conta
/// é a do que sai depois dos outros filtros do despacho: `allowed` é quantas
/// ondas o teto deixou sair, e `leaving` são as pastas das cópias que as
/// ondas que saem de fato recebem, a da reenviada em cópia nova inclusive. A
/// onda que a escolha dos itens, a criação da cópia ou a escolha das skills
/// seguram não toma cópia, e a que sobra depois do despacho diz que o que
/// ficou espera outra coisa, como o Jev ou a vez de sair sozinha, e não há
/// aviso. Diz quais ondas seguram e o que as libera.
pub(super) fn copies_full_warning(
    log: &SpecLog,
    vacancies: &Vacancies,
    (allowed, leaving): (usize, &[&str]),
    stuck: &BTreeMap<u64, Vec<&SpecEvent>>,
    lang: Locale,
) -> Option<Value> {
    let held = vacancies.held_by_copies();
    let taken = leaving.iter().filter(|copy| vacancies.takes_free_copy(copy)).count();
    if held.is_empty() || vacancies.copies_free > taken {
        return None;
    }
    let uncapped = outgoing(log, &Vacancies { copies_free: usize::MAX, ..vacancies.clone() }, stuck, &BTreeSet::new()).ok()?;
    let left = uncapped.go.len() > allowed || (uncapped.free > 0 && !backlog_ready(log).is_empty());
    if !left {
        return None;
    }
    // Com o despacho, nenhuma cópia sob o teto fica livre: as presas são
    // todas as do teto, a da onda que sai agora inclusive.
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

    /// O aviso de cópias cheias da resposta `out`, quando saiu.
    fn copies_full(out: &Value) -> Option<&Value> {
        out["warnings"].as_array().into_iter().flatten().find(|warning| warning["reason"] == json!("copies-full"))
    }

    /// Com duas vagas, o teto é de três cópias. As ondas 1 e 2 voltam e
    /// esperam a revisão, guardando duas; a onda 3 sai na última, e a 4 fica
    /// sem cópia. O aviso conta as três cópias presas, a da onda que saiu
    /// agora inclusive, e diz quais ondas seguram.
    #[test]
    fn the_copy_cap_holds_dispatch_and_the_answer_names_the_waves_keeping_copies() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[]), (4, &["src/d.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        back_for_review(root, 1, "src/a.rs");
        back_for_review(root, 2, "src/b.rs");

        let out = unreviewed_round(root);
        assert_eq!(waves_in(&out, "dispatch"), vec![3], "the last free copy goes to wave 3, and the cap holds wave 4: {out}");
        let full = copies_full(&out).unwrap_or_else(|| panic!("no copies-full warning: {out}"));
        assert_eq!(full["waves"], json!([1, 2]), "{out}");
        let expected = translate("round.copies_full", Locale::PtBr).replace("{copies}", "3").replace("{waves}", "1, 2");
        assert_eq!(full["hint"], json!(expected), "{out}");
        assert!(out["next"].as_str().is_some_and(|next| next.contains(&expected)), "{out}");
    }

    /// Três ondas prontas, três vagas de agente e quatro cópias, duas presas:
    /// o teto deixou uma onda sair, e o aviso de cópias cheias conta só as
    /// cópias livres que as ondas que saem de fato tomam. A onda nova e a
    /// reenviada em cópia nova tomam uma cada, e as duas esgotam as livres; a
    /// onda que volta à pasta presa dela não toma nenhuma, e a que não saiu
    /// também não.
    #[test]
    fn the_copies_full_warning_counts_the_free_copies_the_leaving_waves_take() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[])]);
        let log = spec_x(root);
        let held: BTreeSet<String> = ["/copies/0", "/copies/1"].map(String::from).into();
        let vacancies = Vacancies { capacity: 3, copies: 4, waiting: BTreeSet::from([9]), copies_free: 2, held, ..Vacancies::default() };
        let warn = |leaving: &[&str]| copies_full_warning(&log, &vacancies, (1, leaving), &BTreeMap::new(), Locale::PtBr);

        let both = warn(&["/copies/2", "/copies/3"]).expect("the new wave and the resent one take the two free copies");
        assert_eq!(both["waves"], json!([9]), "{both}");
        assert_eq!(warn(&["/copies/2"]), None, "only the new wave takes a copy, and one is still free");
        assert_eq!(warn(&["/copies/2", "/copies/0"]), None, "the wave back to its own held folder takes no copy");
        assert_eq!(warn(&[]), None, "no wave leaves, so no copy is taken");
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
