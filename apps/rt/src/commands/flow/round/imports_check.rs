//! A conferência das importações depois da onda: cada importação nova de um
//! arquivo que a onda mudou — presente no mapa de depois da junção e
//! ausente no da base da rodada — é conferida pelas regras do padrão do
//! projeto, aprendidas do mapa da base. A que vai contra uma regra forte
//! recusa a volta, com o arquivo, a linha e o papel por onde a chamada
//! deveria passar; a que vai contra um costume, e a que fecha um ciclo novo
//! de importações, só avisam. A importação que já existia na base nunca
//! conta: o projeto segue como está no código. A tarefa da onda que leva o
//! par de papéis (`role_pair`) libera a importação entre os dois, só nos
//! arquivos dela.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use mustard_core::domain::ast::is_test_path;
use mustard_core::domain::pattern::{learn, Direction};
use mustard_core::domain::project_map::ProjectMap;
use mustard_core::domain::spec_events::SpecLog;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::Value;

use super::commit::{AfterWave, Finding};

/// Os achados das importações novas de cada onda de `maps`, em `root`, com o
/// que as tarefas de cada onda no plano `log` liberam.
pub(super) fn findings(root: &Path, maps: &AfterWave, log: &SpecLog, lang: Locale) -> Vec<Finding> {
    let base = learn(&maps.base);
    let after = learn(&maps.after);
    let role = |path: &str| base.roles.get(path).or_else(|| after.roles.get(path)).map(String::as_str);
    let (before, now) = (graph(&maps.base), graph(&maps.after));
    let mut out = Vec::new();
    for (wave, files) in &maps.changed {
        let released = released_pairs(log, *wave);
        for file in files.iter().filter(|file| !is_test_path(file)) {
            let Some(imports) = now.get(file.as_str()) else { continue };
            let old = before.get(file.as_str());
            for &target in imports.iter().filter(|t| **t != file.as_str() && !old.is_some_and(|o| o.contains(*t))) {
                let (Some(from), Some(to)) = (role(file), role(target)) else { continue };
                let at = |key: &str, rule: Option<&Direction>| {
                    let mut text = translate(key, lang)
                        .replace("{file}", file)
                        .replace("{line}", &import_line(root, &maps.after, file, target).to_string())
                        .replace("{target}", target)
                        .replace("{from}", from)
                        .replace("{to}", to);
                    if let Some(rule) = rule {
                        text = text
                            .replace("{rule_from}", &rule.from)
                            .replace("{rule_to}", &rule.to)
                            .replace("{along}", &rule.along.to_string())
                            .replace("{total}", &(rule.along + rule.against).to_string());
                    }
                    text
                };
                let free = released.iter().any(|(pair, own)| own.contains(file.as_str()) && *pair == BTreeSet::from([from, to]));
                if let Some(rule) = base.strong_against(from, to) {
                    if !free {
                        out.push(Finding { wave: *wave, refuses: true, text: at("round.after_wave.import", Some(rule)) });
                    }
                } else if let Some(rule) = base.info_against(from, to) {
                    out.push(Finding { wave: *wave, refuses: false, text: at("round.after_wave.weak", Some(rule)) });
                }
                if reaches(&now, target, file) && !(reaches(&before, file, target) && reaches(&before, target, file)) {
                    out.push(Finding { wave: *wave, refuses: false, text: at("round.after_wave.cycle", None) });
                }
            }
        }
    }
    out
}

/// As importações de cada arquivo de `map` fora de teste, só para arquivo do
/// projeto fora de teste.
fn graph(map: &ProjectMap) -> BTreeMap<&str, BTreeSet<&str>> {
    map.modules
        .iter()
        .filter(|m| !is_test_path(&m.path))
        .map(|m| (m.path.as_str(), m.deps.iter().map(String::as_str).filter(|d| !is_test_path(d)).collect()))
        .collect()
}

/// `from` chega a `to` seguindo as importações de `graph`?
fn reaches(graph: &BTreeMap<&str, BTreeSet<&str>>, from: &str, to: &str) -> bool {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<&str> = VecDeque::from([from]);
    while let Some(at) = queue.pop_front() {
        for next in graph.get(at).into_iter().flatten() {
            if *next == to {
                return true;
            }
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    false
}

/// O par de papéis que cada tarefa vigente da onda `wave` libera, com os
/// arquivos dela: o campo `role_pair`, com dois papéis.
fn released_pairs(log: &SpecLog, wave: u64) -> Vec<(BTreeSet<&str>, BTreeSet<&str>)> {
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "task" && e.wave() == Some(wave))
        .filter_map(|task| {
            let pair: BTreeSet<&str> =
                task.fields.get("role_pair")?.as_array()?.iter().filter_map(Value::as_str).collect();
            let files = task.fields.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            let own = files.iter().filter_map(|f| f.get("path").and_then(Value::as_str)).collect();
            (pair.len() == 2).then_some((pair, own))
        })
        .collect()
}

/// A linha de `file` que importa `target`: a primeira chamada que o mapa de
/// depois liga a uma declaração de `target`; sem ela, a primeira linha do
/// arquivo que cita o nome de `target` sem a extensão; senão, a primeira.
fn import_line(root: &Path, after: &ProjectMap, file: &str, target: &str) -> usize {
    let linked = after.module(target).into_iter().flat_map(|m| &m.declarations).flat_map(|d| &d.used_by);
    if let Some(line) = linked.filter(|site| site.file == file).map(|site| site.line).min() {
        return line;
    }
    let name = target.rsplit('/').next().unwrap_or(target);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let text = std::fs::read_to_string(root.join(file)).unwrap_or_default();
    let cites = |line: &str| {
        line.match_indices(stem).any(|(at, _)| {
            let word = |c: char| c.is_alphanumeric() || c == '_';
            !line[..at].chars().next_back().is_some_and(word) && !line[at + stem.len()..].chars().next().is_some_and(word)
        })
    };
    text.lines().position(cites).map_or(1, |index| index + 1)
}

#[cfg(test)]
pub(super) mod tests {
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::super::tests::{approved_with, delivered, round, round_with_mine, write};
    use super::*;

    /// Um mapa com cinco controllers e cinco services em `src/`, os
    /// controllers importando os services em `along` importações e os
    /// services importando os controllers em `against`, cada importação de
    /// um par de arquivos diferente, mais os `extra` pares (arquivo e o que
    /// ele importa).
    fn two_roles(along: usize, against: usize, extra: &[(&str, &str)]) -> Value {
        let named = |role: &str, n: usize| format!("src/{role}/{role}{n}.{role}.ts");
        let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let pairs: Vec<(String, String)> =
            (0..5).flat_map(|c| (0..5).map(move |s| (named("controller", c), named("service", s)))).collect();
        for (from, to) in pairs.iter().take(along) {
            deps.entry(from.clone()).or_default().push(to.clone());
        }
        for (to, from) in pairs.iter().skip(along).take(against) {
            deps.entry(from.clone()).or_default().push(to.clone());
        }
        for (from, to) in extra {
            deps.entry((*from).to_string()).or_default().push((*to).to_string());
        }
        let paths: BTreeSet<String> = (0..5).flat_map(|n| [named("controller", n), named("service", n)]).chain(deps.keys().cloned()).collect();
        let modules: Vec<Value> = paths
            .iter()
            .map(|path| json!({"path": path, "language": "typescript", "deps": deps.get(path).cloned().unwrap_or_default()}))
            .collect();
        json!({"modules": modules})
    }

    /// Quem relê o mapa depois da onda num teste: grava `after` como o mapa
    /// de depois, no lugar que a rodada pede.
    pub(in super::super) fn mine_giving(
        after: Value,
    ) -> impl Fn(&Path, &Path) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport> {
        move |_root, out| {
            mustard_core::io::project_map::write_text_at(out, &after.to_string()).unwrap();
            Ok(mustard_core::domain::scan::ScanReport::default())
        }
    }

    /// Uma spec aprovada com a onda 1, cuja tarefa muda `file`, com o par
    /// liberado `pair` quando há; a onda já enviada; e o mapa da base `base`.
    fn project(root: &Path, file: &str, pair: Option<[&str; 2]>, base: &Value) {
        approved_with(root, "x", &[], |said| {
            let log = mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root, "x").unwrap())
                .unwrap()
                .unwrap();
            let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
            write(root, "x", "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "A suíte passa.", "origin": said}));
            let mut task = json!({"wave": 1, "text": "Tarefa da onda 1.", "files": [{"path": file}], "depends_on": [], "origin": said});
            if let Some(pair) = pair {
                task["role_pair"] = json!(pair);
            }
            write(root, "x", "task", task);
        });
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "import { Controller4 } from '../controller/controller4.controller';\n").unwrap();
        round(root, "x", None);
        // O mapa da base entra depois do envio: a rodada relê o mapa velho
        // antes de enviar, e o scan de verdade trocaria este pelo do disco.
        mustard_core::io::project_map::write_text(root, &base.to_string()).unwrap();
    }

    const SERVICE: &str = "src/service/service0.service.ts";
    const CONTROLLER: &str = "src/controller/controller4.controller.ts";

    /// A volta da onda 1 mudando o service, com o mapa de depois `after`.
    fn back(root: &Path, after: Value) -> Value {
        let report = delivered(root, 1, "O service mudou.", &[SERVICE]);
        round_with_mine(root, "x", Some(&report), &mine_giving(after))
    }

    #[test]
    fn a_new_import_against_a_strong_rule_is_refused_with_the_fix() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, SERVICE, None, &two_roles(24, 1, &[]));
        let head = super::super::tests::git_text(root, &["rev-parse", "HEAD"]);
        let out = back(root, two_roles(24, 1, &[(SERVICE, CONTROLLER)]));
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&format!("`{SERVICE}` linha 1 importa `{CONTROLLER}`")), "{hint}");
        assert!(hint.contains("regra controller importa service, seguida em 24 de 25 importações"), "{hint}");
        assert!(hint.contains("Leve essa chamada para um arquivo de controller"), "{hint}");
        assert!(hint.contains("Onda 1, rodada de conserto 1 de 2"), "{hint}");
        assert_eq!(super::super::tests::git_text(root, &["rev-parse", "HEAD"]), head, "nothing committed: {out}");
    }

    #[test]
    fn at_the_fix_limit_the_refused_import_goes_to_the_user() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, SERVICE, None, &two_roles(24, 1, &[]));
        let after = two_roles(24, 1, &[(SERVICE, CONTROLLER)]);
        // A entrega e as duas voltas de conserto, todas com a mesma
        // importação contra a regra.
        let rounds: Vec<Value> = (0..3).map(|_| back(root, after.clone())).collect();
        assert_eq!(rounds[1]["reason"], json!("round-after-wave"), "{}", rounds[1]);
        assert!(rounds[1]["hint"].as_str().unwrap_or_default().contains("Onda 1, rodada de conserto 2 de 2"), "{}", rounds[1]);
        assert_eq!(rounds[1].get("question"), None, "{}", rounds[1]);
        let last = &rounds[2];
        assert_eq!(last["reason"], json!("round-after-wave-limit"), "{last}");
        let question = last["question"].as_str().unwrap_or_default();
        assert!(question.contains("A onda 1 ainda tem o que consertar depois de 2 rodadas de conserto"), "{last}");
        let hint = last["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("regra controller importa service, seguida em 24 de 25 importações"), "{hint}");
        assert!(hint.contains(&format!("`{SERVICE}` linha 1 importa `{CONTROLLER}`")), "{hint}");
    }

    #[test]
    fn the_same_import_passes_when_the_task_releases_the_pair() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, SERVICE, Some(["service", "controller"]), &two_roles(24, 1, &[]));
        let out = back(root, two_roles(24, 1, &[(SERVICE, CONTROLLER)]));
        assert_eq!(out["ok"], json!(true), "{out}");
        // O ciclo novo que a importação fecha ainda avisa; a regra, não.
        assert!(!out.to_string().contains("Leve essa chamada"), "{out}");
    }

    #[test]
    fn an_import_against_a_weak_rule_only_warns() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, SERVICE, None, &two_roles(17, 3, &[]));
        let out = back(root, two_roles(17, 3, &[(SERVICE, CONTROLLER)]));
        assert_eq!(out["ok"], json!(true), "{out}");
        let warned = out["warnings"].as_array().cloned().unwrap_or_default();
        let hint = warned.iter().find(|w| w["reason"] == json!("round-after-wave-warnings")).map(|w| w["hint"].to_string());
        let hint = hint.unwrap_or_else(|| panic!("the warning: {out}"));
        assert!(hint.contains("costume controller importa service, seguido em 17 de 20 importações"), "{hint}");
        assert!(hint.contains("só um aviso"), "{hint}");
    }

    #[test]
    fn an_old_import_against_a_strong_rule_is_not_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // O service já importava o controller antes da onda: a importação
        // contra a regra é do projeto como ele está.
        let old = two_roles(24, 0, &[(SERVICE, CONTROLLER)]);
        project(root, SERVICE, None, &old);
        let out = back(root, old);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(!out.to_string().contains("round-after-wave"), "{out}");
    }

    #[test]
    fn a_wave_with_no_new_import_gives_no_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let same = two_roles(24, 1, &[]);
        project(root, SERVICE, None, &same);
        let out = back(root, same);
        assert_eq!(out["ok"], json!(true), "{out}");
        let text = out.to_string();
        for said in ["round-after-wave", "conferência depois da onda", "importa"] {
            assert!(!text.contains(said), "{said}: {out}");
        }
    }
}
