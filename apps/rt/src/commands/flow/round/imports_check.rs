//! A conferência das importações depois da onda: cada importação nova de um
//! arquivo que a onda mudou — presente no mapa de depois da junção e
//! ausente no da base da rodada — é conferida pelas regras do padrão do
//! projeto, aprendidas do mapa da base. A que vai contra uma regra forte
//! recusa a volta, com o arquivo, a linha e o papel por onde a chamada
//! deveria passar; a que vai contra um costume, e a que fecha um ciclo novo
//! de importações, só avisam. O pai que declara o módulo filho dentro de si
//! (`mod filho;`) e o filho que usa o que é do pai (`use super::Coisa`) não
//! fecham ciclo: é o arranjo normal da língua, e o ciclo entre dois irmãos
//! continua avisado. A importação que já existia na base nunca conta: o
//! projeto segue como está no código. A tarefa da onda que leva o
//! par de papéis (`role_pair`) libera a importação entre os dois, só nos
//! arquivos dela.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use mustard_core::domain::ast::{is_declared_child, is_test_path};
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
    let (before_cycles, now_cycles) = (cycle_graph(&maps.base), cycle_graph(&maps.after));
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
                let declared = maps.after.module(file).is_some_and(|m| is_declared_child(file, target, &m.language));
                if !declared
                    && reaches(&now_cycles, target, file)
                    && !(reaches(&before_cycles, file, target) && reaches(&before_cycles, target, file))
                {
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

/// O grafo dos ciclos: o de [`graph`] sem a aresta do arquivo para o módulo
/// filho que ele declara dentro de si. O pai só diz que o filho existe, e o
/// filho que usa o pai é o arranjo normal da língua; contadas, as duas
/// arestas formariam um ciclo em todo módulo com filho.
fn cycle_graph(map: &ProjectMap) -> BTreeMap<&str, BTreeSet<&str>> {
    map.modules
        .iter()
        .filter(|m| !is_test_path(&m.path))
        .map(|m| {
            let deps = m.deps.iter().map(String::as_str);
            let counted = deps.filter(|d| !is_test_path(d) && !is_declared_child(&m.path, d, &m.language));
            (m.path.as_str(), counted.collect())
        })
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
        project_of(root, &[file], pair, base);
    }

    /// [`project`] com a tarefa da onda mudando cada arquivo de `files`.
    fn project_of(root: &Path, files: &[&str], pair: Option<[&str; 2]>, base: &Value) {
        approved_with(root, "x", &[], |said| {
            let log = mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root, "x").unwrap())
                .unwrap()
                .unwrap();
            let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
            write(root, "x", "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "A suíte passa.", "origin": said}));
            let listed: Vec<Value> = files.iter().map(|file| json!({"path": file})).collect();
            let mut task = json!({"wave": 1, "text": "Tarefa da onda 1.", "files": listed, "depends_on": [], "origin": said});
            if let Some(pair) = pair {
                task["role_pair"] = json!(pair);
            }
            write(root, "x", "task", task);
        });
        for file in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "import { Controller4 } from '../controller/controller4.controller';\n").unwrap();
        }
        round(root, "x", None);
        // O mapa da base entra depois do envio: a rodada relê o mapa velho
        // antes de enviar, e o scan de verdade trocaria este pelo do disco.
        mustard_core::io::project_map::write_text(root, &base.to_string()).unwrap();
    }

    const SERVICE: &str = "src/service/service0.service.ts";
    const CONTROLLER: &str = "src/controller/controller4.controller.ts";

    /// A volta da onda 1 mudando o service, com o mapa de depois `after`.
    fn back(root: &Path, after: Value) -> Value {
        back_of(root, &[SERVICE], after)
    }

    /// A volta da onda 1 mudando cada arquivo de `files`, com o mapa de depois
    /// `after`.
    fn back_of(root: &Path, files: &[&str], after: Value) -> Value {
        let report = delivered(root, 1, "A onda mudou.", files);
        round_with_mine(root, "x", Some(&report), &mine_giving(after))
    }

    /// Um mapa de `language` com as importações de cada arquivo (`deps`), o
    /// que a passada do scan grava de um projeto pequeno.
    fn small_map(language: &str, deps: &[(&str, &[&str])]) -> Value {
        let modules: Vec<Value> =
            deps.iter().map(|(path, deps)| json!({"path": path, "language": language, "deps": deps})).collect();
        json!({"modules": modules})
    }

    /// Os avisos da conferência de importações que a volta `out` trouxe,
    /// juntos num texto só; vazio quando não veio nenhum.
    fn warned(out: &Value) -> String {
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        let hints = warnings.iter().filter(|w| w["reason"] == json!("round-after-wave-warnings"));
        hints.map(|w| w["hint"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>().join("\n")
    }

    const CYCLE: &str = "fecha um ciclo novo de importações";
    const PENDING: &str = "src/comandos/evento/pending.rs";
    const CARRIED: &str = "src/comandos/evento/pending/carried.rs";
    const PR_DOOR: &str = "src/comandos/revisao/pr_door.rs";

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

    /// O pai e o filho que ele declara (`mod carried;`) chegam um ao outro e
    /// isso não é ciclo: o filho ganha o `use super::` do pai que já o
    /// declarava; o pai ganha o `mod carried;` de um filho que já o usava; a
    /// onda traz de uma vez a porta que importa o filho, o filho que usa o pai
    /// e o pai que o declara.
    #[test]
    fn a_parent_and_the_child_it_declares_reaching_each_other_close_no_cycle() {
        let door = [PR_DOOR, CARRIED, PENDING];
        let cases: [(&str, &[&str], Value, Value); 3] = [
            (
                "the child uses its declaring parent",
                &[CARRIED],
                small_map("rust", &[(PENDING, &[CARRIED]), (CARRIED, &[])]),
                small_map("rust", &[(PENDING, &[CARRIED]), (CARRIED, &[PENDING])]),
            ),
            (
                "the parent declares a child that already used it",
                &[PENDING],
                small_map("rust", &[(PENDING, &[]), (CARRIED, &[PENDING])]),
                small_map("rust", &[(PENDING, &[CARRIED]), (CARRIED, &[PENDING])]),
            ),
            (
                "a door, a child and its parent come together",
                &door,
                small_map("rust", &[(PR_DOOR, &[PENDING]), (PENDING, &[]), (CARRIED, &[])]),
                small_map("rust", &[(PR_DOOR, &[PENDING, CARRIED]), (PENDING, &[CARRIED]), (CARRIED, &[PENDING])]),
            ),
        ];
        for (case, files, base, after) in cases {
            let dir = tempdir().unwrap();
            let root = dir.path();
            project_of(root, files, None, &base);
            let out = back_of(root, files, after);
            assert_eq!(out["ok"], json!(true), "{case}: {out}");
            assert!(!warned(&out).contains(CYCLE), "{case}: {out}");
        }
    }

    /// Dois arquivos que passam a se importar fecham um ciclo de verdade, e o
    /// aviso continua: dois irmãos filhos do mesmo pai (com a linha do arquivo
    /// no aviso); um pai que não declara o filho, que é só um arquivo que
    /// importa outro; no TypeScript, o `index` que reexporta o `./x` de onde o
    /// `./x` importa o `index`.
    #[test]
    fn two_files_that_start_importing_each_other_still_close_a_cycle() {
        let other = "src/comandos/evento/pending/other.rs";
        let (one, two) = ("src/comandos/evento/um.rs", "src/comandos/evento/dois.rs");
        let (index, x) = ("src/pasta/index.ts", "src/pasta/x.ts");
        type Case<'a> = (&'a str, &'a [&'a str], Value, Value, Option<&'a str>);
        let cases: [Case; 3] = [
            (
                "two siblings of the same parent",
                &[other],
                small_map("rust", &[(PENDING, &[CARRIED, other]), (CARRIED, &[other]), (other, &[])]),
                small_map("rust", &[(PENDING, &[CARRIED, other]), (CARRIED, &[other]), (other, &[CARRIED])]),
                Some(other),
            ),
            (
                "a parent that does not declare the other",
                &[two],
                small_map("rust", &[(one, &[two]), (two, &[])]),
                small_map("rust", &[(one, &[two]), (two, &[one])]),
                None,
            ),
            (
                "an index and the file it reexports in typescript",
                &[x],
                small_map("typescript", &[(index, &[x]), (x, &[])]),
                small_map("typescript", &[(index, &[x]), (x, &[index])]),
                None,
            ),
        ];
        for (case, files, base, after, named) in cases {
            let dir = tempdir().unwrap();
            let root = dir.path();
            project_of(root, files, None, &base);
            let out = back_of(root, files, after);
            assert_eq!(out["ok"], json!(true), "{case}: {out}");
            assert!(warned(&out).contains(CYCLE), "{case}: {out}");
            if let Some(file) = named {
                assert!(warned(&out).contains(&format!("`{file}` linha")), "{case}: {out}");
            }
        }
    }
}
