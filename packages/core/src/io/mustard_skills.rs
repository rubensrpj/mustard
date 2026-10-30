//! O registro das skills que o Mustard criou no projeto, em [`SKILLS_FILE`],
//! fora do git como o resto da pasta do Mustard no projeto. A spec guarda os
//! eventos `skill`; as contas moram aqui: o estado de cada skill, os arquivos
//! que ela cita, o grupo de falhas que a motivou, quantas vezes foi candidata
//! ao bloco de uma tarefa e quantas foi com uma, o último ganho medido, os
//! descartes seguidos e a tarefa de atualizá-la. A skill que só a pessoa
//! criou nunca entra.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::io::failures::SkillTask;
use crate::io::fs::lock::LockedFile;

/// O registro, relativo à raiz do projeto.
pub const SKILLS_FILE: &str = ".claude/mustard/skills.json";

/// A skill em uso.
pub const ACTIVE: &str = "active";
/// A skill cujo último teste não mostrou ganho.
pub const DISCARDED: &str = "discarded";
/// A skill que saiu de `.claude/skills/`.
pub const RETIRED: &str = "retired";

/// Em quantos envios a skill pode ser candidata sem nunca entrar no bloco de
/// uma tarefa antes de sair.
pub const IDLE_CANDIDACIES: u64 = 30;

/// Quantos descartes seguidos do teste da skill inteira, com e sem ela,
/// depois de criada, tiram a skill. A resposta da tarefa de atualizá-la não
/// entra na conta: ela testa a versão nova, e não a skill.
pub const DISCARDS_IN_ROW: u64 = 2;

/// Uma skill que o Mustard criou.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MustardSkill {
    /// [`ACTIVE`], [`DISCARDED`] ou [`RETIRED`].
    pub state: String,
    /// A spec que a criou.
    pub spec: String,
    /// A onda que a criou, quando se sabe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave: Option<u64>,
    /// A hora da última criação ou atualização.
    #[serde(default)]
    pub since: String,
    /// `true` depois de criada ou atualizada uma vez: a pasta dela existiu.
    #[serde(default)]
    pub made: bool,
    /// Os arquivos que ela cita.
    #[serde(default)]
    pub files: Vec<String>,
    /// Os grupos de falha que a motivaram ([`crate::io::failures::Failure::group_id`]).
    #[serde(default)]
    pub groups: Vec<String>,
    /// Em quantos envios ela foi candidata ao bloco de alguma tarefa.
    #[serde(default)]
    pub candidacies: u64,
    /// Quantas vezes ela foi inteira no bloco de uma tarefa.
    #[serde(default)]
    pub deliveries: u64,
    /// O último ganho medido, numa frase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain: Option<String>,
    /// Quantos descartes seguidos ela teve desde a última criação ou
    /// atualização.
    #[serde(default)]
    pub discards_in_row: u64,
    /// Quantos desses descartes vieram do teste da skill inteira, com e sem
    /// ela. O que respondeu à tarefa de atualizá-la não entra: testou a versão
    /// nova, e não a skill. O registro de antes desta conta traz zero, porque
    /// os descartes dele vieram da atualização.
    #[serde(default)]
    pub whole_discards: u64,
    /// A tarefa de atualizá-la, enquanto espera.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_task: Option<SkillTask>,
}

impl MustardSkill {
    /// O motivo de a skill sair agora, quando sai: candidata em
    /// [`IDLE_CANDIDACIES`] envios sem nunca ir com uma tarefa (`idle`), ou
    /// [`DISCARDS_IN_ROW`] descartes seguidos do teste da skill inteira
    /// depois de criada (`discarded`). A skill em dia, que a tarefa de
    /// atualizá-la manteve, não soma descarte. A skill que nunca teve pasta,
    /// ou que já saiu, não sai de novo.
    #[must_use]
    pub fn retire_reason(&self) -> Option<&'static str> {
        if !self.made || self.state == RETIRED {
            return None;
        }
        if self.whole_discards >= DISCARDS_IN_ROW {
            return Some("discarded");
        }
        self.idle().then_some("idle")
    }

    /// `true` quando a skill saiu só por descartes que respondiam à tarefa de
    /// atualizá-la, e não pelo teste da skill inteira nem por parada: ela
    /// merece voltar. Vale também para o registro de antes da conta dos
    /// descartes da skill inteira ([`Self::whole_discards`]), cujos descartes
    /// vieram da atualização.
    #[must_use]
    pub fn retired_by_updates(&self) -> bool {
        self.made && self.state == RETIRED && self.discards_in_row >= DISCARDS_IN_ROW && self.whole_discards < DISCARDS_IN_ROW && !self.idle()
    }

    /// `true` quando a skill foi candidata em [`IDLE_CANDIDACIES`] envios sem
    /// nunca ir com uma tarefa.
    fn idle(&self) -> bool {
        self.candidacies >= IDLE_CANDIDACIES && self.deliveries == 0
    }
}

/// O registro inteiro.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    /// Cada skill, pelo nome.
    #[serde(default)]
    pub skills: BTreeMap<String, MustardSkill>,
    /// O último evento de commit que a conferência das skills já leu, pelo
    /// nome da spec.
    #[serde(default)]
    pub commits_read: BTreeMap<String, u64>,
}

impl Registry {
    /// Grava o evento `skill` de ação `action` da skill `name`, feito na hora
    /// `at`, com o ganho ou o motivo `text` e os arquivos `files`, na spec
    /// `spec` e na onda `wave`, motivado pelos grupos `groups`. A criação e a
    /// atualização a deixam ativa, com o ganho, e zeram os descartes; a skill
    /// em dia (`kept`) segue ativa e também os zera; a volta (`restored`) a
    /// deixa ativa de novo. O descarte conta mais um seguido, e só o que não
    /// responde à tarefa de atualizá-la conta para tirar a skill; a saída a
    /// deixa retirada. Toda gravação encerra a tarefa de atualizá-la.
    pub fn record(&mut self, name: &str, action: &str, at: &str, text: &str, files: &[String], (spec, wave): (&str, Option<u64>), groups: &[String]) {
        let skill = self.skills.entry(name.to_string()).or_insert_with(|| MustardSkill { spec: spec.to_string(), wave, ..MustardSkill::default() });
        for group in groups {
            if !skill.groups.contains(group) {
                skill.groups.push(group.clone());
            }
        }
        let answers_update = skill.update_task.take().is_some();
        match action {
            "created" | "updated" => {
                if action == "created" || skill.spec.is_empty() {
                    (skill.spec, skill.wave) = (spec.to_string(), wave.or(skill.wave));
                }
                skill.state = ACTIVE.to_string();
                skill.since = at.to_string();
                skill.made = true;
                skill.discards_in_row = 0;
                skill.whole_discards = 0;
                if !text.trim().is_empty() {
                    skill.gain = Some(text.trim().to_string());
                }
                if !files.is_empty() {
                    skill.files = files.to_vec();
                }
            }
            "kept" => {
                if skill.state != RETIRED {
                    skill.state = ACTIVE.to_string();
                }
                skill.discards_in_row = 0;
                skill.whole_discards = 0;
            }
            "restored" => {
                skill.state = ACTIVE.to_string();
                skill.since = at.to_string();
                skill.discards_in_row = 0;
                skill.whole_discards = 0;
            }
            "discarded" => {
                skill.discards_in_row += 1;
                if !answers_update {
                    skill.whole_discards += 1;
                    if skill.state != RETIRED {
                        skill.state = DISCARDED.to_string();
                    }
                }
            }
            "retired" => skill.state = RETIRED.to_string(),
            _ => {}
        }
    }

    /// Conta um envio: cada skill registrada e não retirada que está em
    /// `candidates` ganha uma candidatura, e cada uma ganha uma entrega por
    /// bloco de tarefa de `delivered` que a levou.
    pub fn count_send(&mut self, candidates: &[String], delivered: &[Vec<String>]) {
        for (name, skill) in self.skills.iter_mut().filter(|(_, skill)| skill.state != RETIRED) {
            if candidates.contains(name) {
                skill.candidacies += 1;
            }
            skill.deliveries += delivered.iter().filter(|block| block.contains(name)).count() as u64;
        }
    }
}

/// O registro do projeto `root`.
#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(SKILLS_FILE)
}

/// O registro do projeto `root`. Sem o arquivo, ou ilegível, vazio.
#[must_use]
pub fn read(root: &Path) -> Registry {
    crate::io::fs::read_to_string(path(root)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// Troca, sob a trava do arquivo, o registro do projeto `root` pelo que
/// `change` faz com ele, e devolve o que `change` devolveu. Sem mudança, nada
/// se grava.
///
/// # Errors
///
/// A falha de leitura ou de gravação do arquivo.
pub fn change<T>(root: &Path, change: impl FnOnce(&mut Registry) -> T) -> crate::platform::error::Result<T> {
    let file = path(root);
    crate::io::fs::create_dir_all(file.parent().unwrap_or(root))?;
    let mut lock = LockedFile::exclusive(&file)?;
    let mut registry: Registry = serde_json::from_str(&lock.read_to_string()?).unwrap_or_default();
    let before = registry.clone();
    let out = change(&mut registry);
    if registry != before {
        let body = serde_json::to_string_pretty(&registry).map_err(|e| crate::platform::error::Error::Config(e.to_string()))?;
        lock.replace(format!("{body}\n").as_bytes())?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creation makes the skill active with its gain; each discard after it
    /// counts in a row until the next update; idle candidacies and discards
    /// in a row are the two ways out, and only for a skill that had a folder.
    #[test]
    fn the_registry_keeps_state_gain_counts_and_the_ways_out() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        change(root, |registry| registry.record("falta-teste", "created", "2026-10-20T10:00:00Z", "Ganho de 2 de 3.", &["src/a.rs".into()], ("x", Some(2)), &["x:1".into()]))
            .unwrap();
        let skill = read(root).skills["falta-teste"].clone();
        assert_eq!((skill.state.as_str(), skill.gain.as_deref(), skill.wave, skill.made), (ACTIVE, Some("Ganho de 2 de 3."), Some(2), true));
        assert_eq!((skill.files, skill.groups), (vec!["src/a.rs".to_string()], vec!["x:1".to_string()]));

        let mut registry = read(root);
        registry.record("falta-teste", "discarded", "t", "Sem ganho.", &[], ("x", None), &[]);
        assert_eq!((registry.skills["falta-teste"].state.as_str(), registry.skills["falta-teste"].retire_reason()), (DISCARDED, None));
        registry.record("falta-teste", "discarded", "t", "Sem ganho de novo.", &[], ("x", None), &[]);
        assert_eq!(registry.skills["falta-teste"].retire_reason(), Some("discarded"));
        assert_eq!(registry.skills["falta-teste"].gain.as_deref(), Some("Ganho de 2 de 3."), "the last measured gain stays");
        registry.record("falta-teste", "updated", "t", "Ganho de novo.", &[], ("x", None), &[]);
        assert_eq!(registry.skills["falta-teste"].retire_reason(), None, "an update starts the row again");

        for _ in 0..IDLE_CANDIDACIES {
            registry.count_send(&["falta-teste".into()], &[vec![]]);
        }
        assert_eq!(registry.skills["falta-teste"].retire_reason(), Some("idle"));
        registry.count_send(&["falta-teste".into()], &[vec!["falta-teste".into()]]);
        assert_eq!(registry.skills["falta-teste"].retire_reason(), None, "a skill that went with a task stays");

        registry.record("nunca-feita", "discarded", "t", "Sem ganho.", &[], ("x", None), &[]);
        registry.record("nunca-feita", "discarded", "t", "Sem ganho.", &[], ("x", None), &[]);
        assert_eq!(registry.skills["nunca-feita"].retire_reason(), None, "a skill that never had a folder has nothing to retire");
    }

    /// Marca a tarefa de atualizar a skill `name` como à espera de resposta.
    fn wait_for_update(registry: &mut Registry, name: &str) {
        registry.skills.get_mut(name).unwrap().update_task = Some(SkillTask { spec: "x".into(), task: Some(7) });
    }

    /// A skill criada com ganho, como a tarefa de criar a deixa.
    fn created(name: &str) -> Registry {
        let mut registry = Registry::default();
        registry.record(name, "created", "t0", "Ganho de 2 de 3.", &["src/a.rs".into()], ("x", Some(2)), &[]);
        registry
    }

    #[test]
    fn a_skill_the_update_task_finds_up_to_date_stays_active_and_never_counts_as_a_discard() {
        let mut registry = created("em-dia");
        for _ in 0..2 {
            wait_for_update(&mut registry, "em-dia");
            registry.record("em-dia", "kept", "t1", "A versão nova não traz ganho.", &[], ("x", None), &[]);
        }
        let skill = &registry.skills["em-dia"];
        assert_eq!((skill.state.as_str(), skill.retire_reason(), skill.discards_in_row, skill.update_task.is_none()), (ACTIVE, None, 0, true));
        assert_eq!((skill.gain.as_deref(), skill.since.as_str()), (Some("Ganho de 2 de 3."), "t0"), "being up to date measures no new gain and makes no new version");

        // O em dia zera a fila: o descarte de antes e o de depois não somam.
        registry.record("em-dia", "discarded", "t2", "Sem ganho.", &[], ("x", None), &[]);
        registry.record("em-dia", "kept", "t3", "Em dia.", &[], ("x", None), &[]);
        assert_eq!(registry.skills["em-dia"].state, ACTIVE, "up to date brings back a skill the last test discarded");
        registry.record("em-dia", "discarded", "t4", "Sem ganho.", &[], ("x", None), &[]);
        assert_eq!(registry.skills["em-dia"].retire_reason(), None, "up to date starts the row again");

        registry.record("saiu", "retired", "t5", "Saiu.", &[], ("x", None), &[]);
        registry.record("saiu", "kept", "t6", "Em dia.", &[], ("x", None), &[]);
        assert_eq!(registry.skills["saiu"].state, RETIRED, "up to date does not bring back a skill that left");
    }

    #[test]
    fn only_the_whole_skill_test_takes_the_skill_out_and_the_update_task_answer_does_not_count() {
        let mut registry = created("falta-teste");
        for _ in 0..3 {
            wait_for_update(&mut registry, "falta-teste");
            registry.record("falta-teste", "discarded", "t1", "A versão nova não traz ganho.", &[], ("x", None), &[]);
        }
        let skill = &registry.skills["falta-teste"];
        assert_eq!((skill.state.as_str(), skill.retire_reason(), skill.discards_in_row), (ACTIVE, None, 3), "the test of the new version is not a test of the skill");

        registry.record("falta-teste", "discarded", "t2", "Sem ganho com e sem a skill.", &[], ("x", None), &[]);
        assert_eq!(registry.skills["falta-teste"].retire_reason(), None, "one test of the whole skill is not enough");
        registry.record("falta-teste", "discarded", "t3", "Sem ganho com e sem a skill.", &[], ("x", None), &[]);
        assert_eq!(registry.skills["falta-teste"].retire_reason(), Some("discarded"), "two tests of the whole skill in a row take it out");
    }

    #[test]
    fn a_skill_retired_by_update_answers_is_told_apart_and_comes_back_active() {
        // O registro de antes da conta: dois descartes, nenhum da skill inteira.
        let old = MustardSkill { state: RETIRED.into(), made: true, candidacies: 38, deliveries: 30, discards_in_row: 2, ..MustardSkill::default() };
        assert!(old.retired_by_updates());
        assert!(!MustardSkill { whole_discards: 2, ..old.clone() }.retired_by_updates(), "the whole-skill test took it out");
        assert!(!MustardSkill { deliveries: 0, candidacies: IDLE_CANDIDACIES, ..old.clone() }.retired_by_updates(), "it stood idle");
        assert!(!MustardSkill { state: ACTIVE.into(), ..old.clone() }.retired_by_updates(), "it never left");
        assert!(!MustardSkill { made: false, ..old.clone() }.retired_by_updates(), "it never had a folder");

        let mut registry = Registry::default();
        registry.skills.insert("catalogo".into(), old);
        registry.record("catalogo", "restored", "t9", "Voltou.", &[], ("x", None), &[]);
        let skill = &registry.skills["catalogo"];
        assert_eq!((skill.state.as_str(), skill.since.as_str(), skill.discards_in_row, skill.whole_discards), (ACTIVE, "t9", 0, 0));
        assert_eq!((skill.retire_reason(), skill.retired_by_updates()), (None, false), "back, it does not leave again");
    }
}
