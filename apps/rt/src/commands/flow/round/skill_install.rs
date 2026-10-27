//! A versão nova de uma skill que a onda entrega. A pasta das skills do
//! projeto fica fora da cópia de cada onda, e a onda da tarefa de criar ou de
//! atualizar uma skill, presa à cópia como toda onda, escreve a versão nova no
//! caminho reservado da cópia dela ([`RESERVED_SKILLS`]).
//! O caminho reservado nunca entra no commit: sai da lista que a volta cita
//! ([`drop_reserved`]) e da lista do que a cópia mudou ([`super::commit`]). Depois
//! de gravar a entrega da onda de uma tarefa de skill, a rodada grava a versão
//! nova no lugar da skill quando a onda gravou a criação ou a atualização, com
//! a cópia de segurança junto, e esvazia o caminho reservado
//! ([`install_delivered`]).

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::wave_prompt::{RESERVED_SKILLS, reserved_skill};
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

use super::commit::copy_of;
use super::report::WaveReport;
use crate::shared::failure_skills::{copy_folder, skill_dir};
use crate::shared::skill_cycle::in_reserved_skills;

/// Tira da lista de cada volta de `waves` o caminho reservado às versões novas
/// das skills: a rodada o grava no lugar da skill, e nunca no commit.
pub(super) fn drop_reserved(waves: &mut [WaveReport]) {
    for wave in waves {
        wave.files.retain(|file| !in_reserved_skills(file));
    }
}

/// `true` quando a onda `wave` de `log` levou uma tarefa de skill: a que traz
/// o skill-creator no bloco.
fn skill_wave(log: &SpecLog, wave: u64) -> bool {
    super::sent_tasks::sent_tasks(log, wave).iter().any(|task| task.str_field("skill") == Some(mustard_core::io::skill_creator::NAME))
}

/// `true` quando a última gravação da skill `name` em `log` depois do evento
/// `sent` é uma criação ou uma atualização.
fn written_after(log: &SpecLog, sent: u64, name: &str) -> bool {
    log.visible()
        .into_iter()
        .rfind(|e| e.event_type == "skill" && e.id > sent && e.str_field("name").map(str::trim) == Some(name))
        .is_some_and(|e| matches!(e.str_field("action").map(str::trim), Some("created" | "updated")))
}

/// Grava o `SKILL.md` `file` no lugar da skill `name` do projeto `root`, com a
/// pasta criada quando a skill é nova, e a pasta da skill, já com ele, na
/// cópia de segurança.
fn install(root: &Path, name: &str, file: &Path) -> std::io::Result<()> {
    let target = skill_dir(root, name);
    std::fs::create_dir_all(&target)?;
    std::fs::copy(file, target.join("SKILL.md"))?;
    copy_folder(&target, &mustard_core::io::wave_prompt::skills_backup_dir(root).join(name))
}

/// Depois de gravadas as entregas `waves`, lidas em `log` do projeto `root`:
/// a onda de uma tarefa de skill que gravou, depois do envio dela, a criação
/// ou a atualização de uma skill cuja versão nova está no caminho reservado da
/// cópia tem essa versão gravada no lugar da skill ([`install`]). O caminho
/// reservado sai da cópia; com uma gravação que falhou, ele fica, e o aviso,
/// em `lang`, diz onde.
pub(super) fn install_delivered(root: &Path, log: &SpecLog, waves: &[WaveReport], lang: Locale) -> Vec<Value> {
    let mut warnings: Vec<Value> = Vec::new();
    let dispatched = log.last_dispatch_by_wave();
    for wave in waves {
        let Some(copy) = copy_of(log, wave.wave) else { continue };
        let folder = copy.join(RESERVED_SKILLS);
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        let sent = dispatched.get(&wave.wave).copied().unwrap_or_default();
        let skill_task = skill_wave(log, wave.wave);
        let mut kept = false;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let file = entry.path().join("SKILL.md");
            if !skill_task || !file.is_file() || !written_after(log, sent, &name) {
                continue;
            }
            if let Err(error) = install(root, &name, &file) {
                kept = true;
                let hint = translate("skills.not_installed", lang)
                    .replace("{name}", &name)
                    .replace("{detail}", &error.to_string())
                    .replace("{reserved}", &mustard_core::io::wave_prompt::shown(&copy.join(reserved_skill(&name))));
                warnings.push(json!({ "reason": "skill-not-installed", "wave": wave.wave, "hint": hint }));
            }
        }
        if !kept {
            let _ = std::fs::remove_dir_all(&folder);
        }
    }
    warnings
}
