//! A busca da skill que serve para o texto de uma tarefa. Um lugar só: o
//! plano usa para achar a skill que já existe e serve (`best_skill`), e a
//! rodada usa, antes do envio, para sugerir ao orquestrador todas as que
//! casam (`matching_skills`) e para mostrar o "quando usar" de todas as
//! skills da área da tarefa (`skills_on_disk`).
//!
//! Sem disco nenhum aqui além da leitura das próprias skills: a regra da
//! busca por palavras mora em `domain::search`.

use std::path::{Path, PathBuf};

use mustard_core::domain::search;
use mustard_core::domain::spec_events::{search_field, SpecEvent};

use super::plan::declared_files;

/// Quantos arquivos parecidos o mapa sugere, para a recusa da tarefa sem
/// arquivo e para a escolha antes do envio.
pub(crate) const MAP_SUGGESTIONS: usize = 3;

/// As skills que existem no disco, pelo nome e pelo "quando usar" delas, tal
/// como a descrição escreve — pronto para mostrar ao orquestrador e para a
/// busca reduzir na hora. Procuradas onde o pedido da onda as procura: nas
/// pastas dos arquivos que as tarefas declaram, subindo até a raiz, e na raiz
/// do projeto. A skill sem descrição fica de fora, porque é a descrição que
/// diz se ela serve para a tarefa.
pub(crate) fn skills_on_disk(root: &Path, tasks: &[&SpecEvent]) -> Vec<(String, String)> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for task in tasks {
        for (file, _) in declared_files(task) {
            let mut folder = root.join(file);
            while folder.pop() && folder.starts_with(root) {
                if !folders.contains(&folder) {
                    folders.push(folder.clone());
                }
            }
        }
    }
    if !folders.contains(&root.to_path_buf()) {
        folders.push(root.to_path_buf());
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(folder.join(".claude").join("skills")) else { continue };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if out.iter().any(|(had, _)| *had == name) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path().join("SKILL.md")) else { continue };
            let Ok(front) = mustard_core::domain::skill::frontmatter::parse(&text) else { continue };
            let when = front.description.split_whitespace().collect::<Vec<_>>().join(" ");
            if !when.is_empty() {
                out.push((name, when));
            }
        }
    }
    out.sort();
    out
}

/// A skill que serve mais forte para o texto de uma tarefa, entre as skills
/// no disco (`on_disk`): a que casa mais forte, pela mesma busca do recorte
/// dos itens. `None` quando nenhuma casa.
pub(crate) fn best_skill(on_disk: &[(String, String)], text: &str) -> Option<String> {
    matching_skills(on_disk, text).into_iter().next()
}

/// Todas as skills, entre as skills no disco (`on_disk`), cujo "quando usar"
/// casa com o texto de uma tarefa, da mais forte para a mais fraca. Vazio
/// quando nenhuma casa.
pub(crate) fn matching_skills(on_disk: &[(String, String)], text: &str) -> Vec<String> {
    let reduced: Vec<String> = on_disk.iter().map(|(_, when)| search_field(Some(when), &[])).collect();
    let docs = reduced.iter().enumerate().map(|(i, when)| (i as u64, when.as_str()));
    search::search(docs, text).into_iter().filter_map(|hit| on_disk.get(hit.id as usize)).map(|(name, _)| name.clone()).collect()
}
