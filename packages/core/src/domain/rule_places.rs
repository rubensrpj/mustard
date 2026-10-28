//! `rule_places` — os lugares da regra que uma tarefa faz valer, e quais a
//! entrega deixou de fora.
//!
//! Um lugar é o que o achado ou a tarefa cita em `places`: um arquivo com
//! linha (`apps/a.rs:12`), um arquivo (`apps/a.rs`) ou um nome de código
//! (`check_return`, `a::b`), que o mapa diz onde é declarado. O lugar que não
//! leva a arquivo nenhum do projeto — um comando, um nome que o mapa não
//! conhece — não tem arquivo a mudar e fica fora da conta. O lugar que a
//! entrega marca como conferido sem mudança, com o motivo, também.
//!
//! A regra mora aqui uma vez, sem disco: quem responde onde o nome é
//! declarado é o [`CitationWorld`], o mesmo da conferência das citações.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::domain::citation::{file_citation, CitationWorld};
use crate::domain::clarity::is_file_path;
use crate::domain::project_map::clean_path;

/// Os arquivos a que o lugar `place` leva, com barras normais: o da citação
/// com linha; o caminho de arquivo; ou, no nome de código, cada arquivo onde
/// o mapa o declara — o qualificado (`a::b`) e o chamado (`run()`) valem pelo
/// último pedaço. Vazio no lugar que não leva a arquivo nenhum.
#[must_use]
pub fn place_files(world: &impl CitationWorld, place: &str) -> Vec<String> {
    let place = place.trim();
    if let Some((path, _)) = file_citation(place) {
        return vec![clean_path(&path)];
    }
    if is_file_path(place) {
        return vec![clean_path(&place.replace('\\', "/"))];
    }
    let name = place.strip_suffix("()").unwrap_or(place);
    let name = name.rsplit("::").next().unwrap_or(name);
    let mut files: Vec<String> = world.declared(name).into_iter().map(|(path, _)| clean_path(&path)).collect();
    files.dedup();
    files
}

/// Os lugares que a entrega marca como conferidos sem mudança, das marcas
/// `marks` (`place`, `reason`), sem os espaços das pontas. Só a marca com o
/// lugar e o motivo conta: a sem motivo deixa o lugar na conta.
#[must_use]
pub fn checked_places(marks: &[Value]) -> BTreeSet<String> {
    let filled = |mark: &Value, key: &str| mark.get(key).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
    marks.iter().filter(|mark| filled(mark, "reason").is_some()).filter_map(|mark| filled(mark, "place")).collect()
}

/// Os lugares de `places` que a entrega deixou de fora: os que levam a algum
/// arquivo ([`place_files`]), cujo arquivo nenhum está entre os mudados
/// (`touched`, relativos à raiz, com barras normais) e que a entrega não marca
/// como conferidos sem mudança (`checked`, de [`checked_places`]), pelo texto
/// do lugar. O nome declarado em mais de um arquivo conta como mudado quando
/// um deles mudou. Na ordem dada.
#[must_use]
pub fn untouched_places<'a>(
    world: &impl CitationWorld,
    places: &'a [String],
    touched: &BTreeSet<String>,
    checked: &BTreeSet<String>,
) -> Vec<&'a str> {
    let touched: BTreeSet<String> = touched.iter().map(|path| clean_path(&path.replace('\\', "/"))).collect();
    places
        .iter()
        .map(String::as_str)
        .filter(|place| !checked.contains(place.trim()))
        .filter(|place| {
            let files = place_files(world, place);
            !files.is_empty() && !files.iter().any(|file| touched.contains(file))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O mapa de memória: as declarações, nome a nome, cada uma no arquivo
    /// dela.
    struct Map(Vec<(&'static str, &'static str)>);

    impl CitationWorld for Map {
        fn file_lines(&self, _path: &str) -> Option<u64> {
            None
        }
        fn declared(&self, name: &str) -> Vec<(String, u64)> {
            self.0.iter().filter(|(_, declared)| *declared == name).map(|(path, _)| ((*path).to_string(), 1)).collect()
        }
        fn has_map(&self) -> bool {
            true
        }
        fn files_named(&self, _name: &str) -> Vec<String> {
            Vec::new()
        }
    }

    /// O lugar leva ao arquivo pela linha, pelo caminho e pelo nome que o mapa
    /// declara; o lugar sem arquivo — um comando, um nome desconhecido — fica
    /// fora, e o nome de dois arquivos vale por qualquer um deles.
    #[test]
    fn a_place_is_touched_through_its_file_or_the_file_the_map_declares_it_in() {
        let map = Map(vec![("src/a.rs", "check_return"), ("src/b.rs", "run"), ("src/c.rs", "run")]);
        let places: Vec<String> =
            ["src/a.rs:12", "./src/d.rs", "check_return", "Report::run()", "cargo test", "unknown_name"].map(String::from).to_vec();
        let touched = BTreeSet::from(["src/c.rs".to_string()]);
        let none = BTreeSet::new();
        assert_eq!(untouched_places(&map, &places, &touched, &none), ["src/a.rs:12", "./src/d.rs", "check_return"]);
        let all = BTreeSet::from(["src/a.rs".to_string(), "src/b.rs".to_string(), "src/d.rs".to_string()]);
        assert_eq!(untouched_places(&map, &places, &all, &none), Vec::<&str>::new());
    }
}
