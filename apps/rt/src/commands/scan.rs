//! `scan` — mine the workspace into `grain.model.json` via the bundled grain
//! tool. This is THE scan now: it replaces the old in-tree scan engine
//! (miner / ast / vocabulary / cluster discovery / skill+agent generation),
//! which is removed. grain is deterministic and fully
//! language-agnostic; Mustard never reads project source to understand a repo.
//!
//! The model lands at `<root>/.claude/grain.model.json` (the durable product,
//! re-run when the codebase changes). Downstream commands consume it through the
//! [`mustard_core::Scan`] client (`digest --query`, `spec`), never by reading
//! source. No skills or agents are produced; with `--full`, the one file
//! written per subproject is its `.claude/scan-map.md`.
//!
//! A cada vez que roda, o scan também lê o banco de lições e aponta o que
//! enxugar nele, sem mudar o banco: os grupos de lições parecidas, a juntar
//! numa lição só, e as lições que citam um caminho que o projeto já não tem,
//! a retirar. A lista sai em `lessons` e o que fazer com ela, em `next`; quem
//! junta e retira é o assistente, pelo `run write lesson`. O caminho que
//! falta só é apontado com o mapa desta vez: quando a ferramenta do scan
//! falha, só as lições parecidas saem.

use std::path::{Path, PathBuf};

use mustard_core::Scan;
use mustard_core::domain::lessons::{self, MissingPaths};
use mustard_core::domain::project_map::{self, ProjectMap};
use mustard_core::domain::scan::{mark_own_git_roots, read_projects, ScanReport};
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::ClaudePaths;
use serde_json::{json, Value};

use super::scan_claude;

/// Default model location under the project's `.claude/` directory.
pub(crate) fn default_model_path(root: &Path) -> PathBuf {
    root.join(".claude").join("grain.model.json")
}

/// Run `grain scan <root> --out <model>`; print a small JSON result. Fail-open:
/// a spawn/exit error is reported, never panics (matches the other handlers).
///
/// With a model of this project already on disk, only the files that changed
/// since are read again; the result says how many (`read`, a count, never
/// the list of names) and whether every file was (`full`). Nothing is
/// written to git and nothing runs this on its own.
///
/// When `full` is `true`, (re)generates the mustard-owned
/// `.claude/scan-map.md` per subproject after the model is written; no
/// `CLAUDE.md` is ever written. The hard cap guards the map against a runaway
/// generator.
pub fn run(root: &Path, out: Option<&Path>, full: bool) {
    let result = scan_at(root, out, full, |root, model| Scan::locate().scan(root, model));
    println!("{}", serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into()));
}

/// O núcleo testável de [`run`]: o relatório que o comando imprime. `mine`
/// é a leitura do projeto pela ferramenta do scan, que grava o mapa em
/// `model`; o comando passa a ferramenta instalada, e o teste, uma que
/// grava o mapa dos arquivos do disco ou que falha.
pub(crate) fn scan_at(
    root: &Path,
    out: Option<&Path>,
    full: bool,
    mine: impl FnOnce(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>,
) -> Value {
    let model_path = out.map_or_else(|| default_model_path(root), Path::to_path_buf);

    // Preflight BEFORE the miner: an unpopulated submodule is indistinguishable
    // from an absent subtree once the walk runs — it visits the directory, finds
    // nothing, and mines a model missing that whole subproject with no error and
    // no coverage entry. Refusing here keeps the PREVIOUS (complete) model on
    // disk, which is strictly better than replacing it with a hollow one.
    let hollow = hollow_submodules(root);
    if !hollow.is_empty() {
        for path in &hollow {
            eprintln!(
                "scan: submodule `{path}` is declared in .gitmodules but its directory is EMPTY — \
                 the model would silently omit that entire subproject."
            );
        }
        eprintln!(
            "scan: refusing to mine a hollow model (the existing one is left untouched). \
             Populate with `git submodule update --init --recursive`, then re-run."
        );
        return json!({
            "ok": false,
            "reason": "hollow-submodules",
            "empty_submodules": hollow,
        });
    }

    let scan_result = mine(root, &model_path);

    let mut result: Value = match &scan_result {
        Ok(report) => json!({
            "ok": true,
            "model": model_path.to_string_lossy(),
            "full": report.full,
            // Só quantos arquivos foram lidos, como a abertura de spec
            // responde: a lista fica no relatório da ferramenta.
            "read": report.read.len(),
            "files": report.files,
        }),
        Err(err) => {
            eprintln!("scan: grain failed: {err}");
            json!({ "ok": false, "error": err.to_string() })
        }
    };

    // Only run the map pass when grain succeeded (model file is valid).
    if scan_result.is_ok() {
        let mut projects = read_projects(&model_path);
        // The grain miner is git-blind; stamp the git-boundary FACT onto the
        // census here (a `.git` dir/file at each subproject's dir) so the
        // subproject list carries "this is its own repo" for every downstream
        // consumer (dispatch / prompt render / branch gate re-derive it the
        // same way from the same helper). See `mark_own_git_roots`.
        mark_own_git_roots(root, &mut projects);
        let pass = scan_claude::run_pass(root, &projects, full);

        if full {
            result["regenerated"] = json!(pass.regenerated);
            if !pass.over_cap.is_empty() {
                for entry in &pass.over_cap {
                    eprintln!(
                        "scan: scan-map over hard cap ({} bytes > {} ceiling): {} — not written; runaway machine map",
                        entry.bytes,
                        scan_claude::SCAN_MAP_HARD_CAP_BYTES,
                        entry.path,
                    );
                }
                let over_cap_json: Vec<Value> = pass.over_cap.iter().map(|e| {
                    json!({ "path": e.path, "bytes": e.bytes })
                }).collect();
                result["over_cap"] = json!(over_cap_json);
                result["ok"] = json!(false);
            }
        }
    }

    // O mapa só vale quando o scan desta vez deu certo: o de uma volta
    // anterior ainda acha o arquivo que já saiu.
    review_lessons(root, scan_result.is_ok().then_some(model_path.as_path()), &mut result);
    result
}

/// Lê o banco de lições e põe no relatório o que enxugar nele: em `lessons`,
/// os grupos de lições parecidas (`similar`) e as lições que citam caminhos
/// que o projeto já não tem (`missing_paths`); em `next`, o que o assistente
/// faz com eles. Só lê: o banco fica com os mesmos bytes. Sem banco, ou com
/// nada a enxugar, o relatório fica como estava. `model_path` é o mapa que o
/// scan acabou de gravar. Sem ele, nenhum caminho é dado como faltando: o
/// disco sozinho não acha o caminho que a lição cita só pelo fim, e a lição
/// cujo arquivo existe seria mandada embora.
fn review_lessons(root: &Path, model_path: Option<&Path>, result: &mut Value) {
    let home = mustard_core::io::spec_events::spec_root(root);
    let Some(path) = ClaudePaths::for_project(&home).ok().map(|paths| paths.lessons_path()) else { return };
    let Ok(Some(bank)) = mustard_core::io::lessons::read(&path) else { return };
    let map: Option<ProjectMap> =
        model_path.and_then(|model| std::fs::read_to_string(model).ok()).and_then(|text| serde_json::from_str(&text).ok());
    let missing = map.as_ref().map_or_else(Vec::new, |map| {
        lessons::citing_missing_paths(&bank, |cited: &str, inside: Option<&str>| path_found(root, map, cited, inside))
    });
    let leaving: Vec<u64> = missing.iter().map(|m| m.id).collect();
    let similar = lessons::similar(&bank, &leaving);
    if similar.is_empty() && missing.is_empty() {
        return;
    }
    let lang = mustard_core::ProjectConfig::load(&home).language().text_or_default();
    result["lessons"] = json!({
        "similar": similar,
        "missing_paths": missing.iter().map(|m| json!({ "id": m.id, "paths": m.paths })).collect::<Vec<_>>(),
    });
    result["next"] = json!(next_step(&similar, &missing, lang));
}

/// O passo seguinte do scan: juntar cada grupo e retirar as lições que já
/// não valem, pelo `run write lesson`.
fn next_step(similar: &[Vec<u64>], missing: &[MissingPaths], lang: Locale) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !similar.is_empty() {
        let groups: Vec<String> = similar
            .iter()
            .map(|group| format!("[{}]", group.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")))
            .collect();
        parts.push(translate("lessons.scan_merge", lang).replace("{groups}", &groups.join(", ")));
    }
    if !missing.is_empty() {
        let shown: Vec<String> = missing.iter().map(|m| format!("{} ({})", m.id, m.paths.join(", "))).collect();
        parts.push(translate("lessons.scan_retire", lang).replace("{lessons}", &shown.join(", ")));
    }
    parts.push(translate("lessons.scan_untouched", lang).to_string());
    parts.join(" ")
}

/// O caminho que uma lição cita existe no projeto: no disco, a partir da raiz
/// ou de uma das pastas do subprojeto da lição (`inside`) até ela, ou no mapa
/// que o scan acabou de gravar, que aceita só o fim do caminho.
fn path_found(root: &Path, map: &ProjectMap, cited: &str, inside: Option<&str>) -> bool {
    if root.join(cited).exists() {
        return true;
    }
    let mut folder = inside.map(|sub| root.join(sub));
    while let Some(dir) = folder {
        if dir.join(cited).exists() {
            return true;
        }
        folder = dir.parent().filter(|up| up.starts_with(root) && *up != root).map(Path::to_path_buf);
    }
    project_map::map_knows(map, cited)
}

/// Submodule paths declared in `.gitmodules` whose working directory holds no
/// files — checked out out of band or never populated.
///
/// The declaration is the evidence: `.gitmodules` says a subtree belongs here,
/// so an empty directory is a hole, not an absence. Nothing else can tell the
/// difference — git metadata is invisible to the miner, and the walk only sees
/// files. Parsing is deliberately dumb (the `path =` entries, nothing else): a
/// `.gitmodules` we cannot read yields nothing to complain about, which is the
/// fail-open default for a repo that has no submodules at all.
fn hollow_submodules(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(".gitmodules")) else {
        return Vec::new(); // no submodules declared — nothing to check.
    };
    let mut out: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("path")?.trim_start().strip_prefix('='))
        .map(|p| p.trim().replace('\\', "/"))
        .filter(|p| !p.is_empty())
        .filter(|p| {
            // Absent or empty — both mean "not checked out here".
            std::fs::read_dir(root.join(p)).map_or(true, |mut it| it.next().is_none())
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, body).expect("write");
    }

    /// A ferramenta do scan no teste: grava em `model` o mapa com os arquivos
    /// que o disco tem em `root`, fora a pasta `.claude`, como a ferramenta
    /// instalada faz, e devolve o relatório de uma leitura inteira. O teste
    /// não depende da ferramenta instalada na máquina.
    fn mine_disk(root: &Path, model: &Path) -> mustard_core::platform::error::Result<ScanReport> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<Value>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
            entries.sort();
            for path in entries {
                if path.file_name().is_some_and(|name| name == ".claude") {
                    continue;
                }
                if path.is_dir() {
                    walk(root, &path, out);
                } else if let Ok(rel) = path.strip_prefix(root) {
                    out.push(json!({ "path": rel.to_string_lossy().replace('\\', "/") }));
                }
            }
        }
        let mut modules = Vec::new();
        walk(root, root, &mut modules);
        let files = modules.len();
        write(model, &json!({ "modules": modules }).to_string());
        Ok(ScanReport { full: true, files, ..ScanReport::default() })
    }

    /// A ferramenta do scan que não roda, como quando ela não está no caminho
    /// do shell.
    fn mine_fails(_: &Path, _: &Path) -> mustard_core::platform::error::Result<ScanReport> {
        Err(mustard_core::platform::error::Error::check_failed("scan: No such file or directory"))
    }

    /// Uma regra do projeto como o importador das instruções a deixa no
    /// banco, gravada pelo mesmo gravador do comando de gravar lição.
    fn rule(bank: &Path, subproject: &str, text: &str, keys: &[&str]) -> u64 {
        let draft = serde_json::json!({"class": "project_rule", "text": text, "keys": keys,
            "applies_to": {"subproject": subproject}, "found_in": {"source": format!("{subproject}/CLAUDE.md")}});
        let Value::Object(draft) = draft else { unreachable!() };
        mustard_core::io::lessons::write(bank, draft, None).expect("a lição entra no banco").id
    }

    /// O scan roda num projeto cujo banco tem duas regras reais com o mesmo
    /// assunto em palavras diferentes, vizinhas do mesmo subprojeto que só
    /// dividem com elas a palavra dele, e uma lição que cita um arquivo. Com o
    /// arquivo no lugar, o scan não aponta a lição; depois de o arquivo ser
    /// apagado, aponta as duas parecidas como um grupo a juntar e a outra como
    /// candidata a sair, e o passo seguinte manda juntar e retirar pelo
    /// comando de gravar lição. O banco fica com os mesmos bytes.
    #[test]
    fn the_scan_points_out_similar_lessons_and_the_one_citing_a_deleted_file_without_touching_the_bank() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(&root.join("apps/rt/src/main.rs"), "fn main() {}\n");
        let cited = root.join("packages/core/src/domain/economy/estimator.rs");
        write(&cited, "pub fn estimate() -> usize { 0 }\n");
        let bank = ClaudePaths::for_project(root).expect("paths").lessons_path();
        let sub = "apps/rt";
        rule(&bank, sub, "Hook nunca pode entrar em pânico nem barrar a sessão por erro próprio.", &["rt", "nunca", "entrar", "pânico", "barrar", "sessão", "próprio"]);
        let first = rule(&bank, sub, "Subcomando novo de `run` exige QUATRO registros; esquecer qualquer um compila mas quebra algo em silêncio.", &["rt", "subcomando", "exige", "quatro", "registros", "esquecer", "qualquer"]);
        rule(&bank, sub, "A face `run` NÃO lê o stdin do harness.", &["rt", "stdin", "harness", "despachada", "antes", "leitura", "check"]);
        let second = rule(&bank, sub, "Subcomando novo de `run` exige QUATRO registros (variante no enum, braço no `dispatch()`, entrada na lista trancada e um chamador).", &["rt", "subcomando", "exige", "quatro", "registros", "variante", "família"]);
        let stale = rule(&bank, "packages/core", "Trate a contagem de tokens (`domain/economy/estimator.rs`) como aproximação.", &["core", "contagem", "tokens"]);

        let before = scan_at(root, None, false, mine_disk);
        assert_eq!(before["lessons"]["similar"], serde_json::json!([[first, second]]), "{before}");
        assert_eq!(before["lessons"]["missing_paths"], serde_json::json!([]), "o arquivo ainda existe: {before}");

        std::fs::remove_file(&cited).expect("apaga o arquivo citado");
        let bytes = std::fs::read(&bank).expect("o banco");
        let result = scan_at(root, None, false, mine_disk);
        assert_eq!(result["lessons"]["similar"], serde_json::json!([[first, second]]), "{result}");
        assert_eq!(
            result["lessons"]["missing_paths"],
            serde_json::json!([{"id": stale, "paths": ["domain/economy/estimator.rs"]}]),
            "{result}"
        );
        let next = result["next"].as_str().expect("o passo seguinte");
        assert!(next.contains(&format!("[{first}, {second}]")), "{next}");
        assert!(next.contains(&format!("{stale} (domain/economy/estimator.rs)")), "{next}");
        assert!(next.contains("mustard-rt run write lesson") && next.contains("\"replaces\"") && next.contains("\"targets\""), "{next}");
        assert_eq!(std::fs::read(&bank).expect("o banco"), bytes, "o scan não muda o banco");
    }

    /// Quando a ferramenta do scan falha, não há mapa desta vez, e o disco
    /// sozinho não acha o arquivo que a lição cita só pelo fim do caminho: o
    /// scan não aponta caminho nenhum como faltando, nem a lição cujo arquivo
    /// saiu, porque sem o mapa não dá para saber. As lições parecidas, que não
    /// dependem do mapa, continuam apontadas. Com a ferramenta rodando, o
    /// arquivo que existe é achado pelo mapa.
    #[test]
    fn a_failed_scan_points_out_no_missing_path_and_keeps_the_similar_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let cited = root.join("packages/core/src/domain/economy/estimator.rs");
        write(&cited, "pub fn estimate() -> usize { 0 }\n");
        write(&root.join("apps/rt/src/lib.rs"), "pub fn run() {}\n");
        let bank = ClaudePaths::for_project(root).expect("paths").lessons_path();
        let sub = "apps/rt";
        let first = rule(&bank, sub, "Subcomando novo de `run` exige QUATRO registros; esquecer qualquer um compila mas quebra algo em silêncio.", &["rt", "subcomando", "exige", "quatro", "registros", "esquecer", "qualquer"]);
        let second = rule(&bank, sub, "Subcomando novo de `run` exige QUATRO registros (variante no enum, braço no `dispatch()`, entrada na lista trancada e um chamador).", &["rt", "subcomando", "exige", "quatro", "registros", "variante", "família"]);
        rule(&bank, "packages/core", "Trate a contagem de tokens (`domain/economy/estimator.rs`) como aproximação.", &["core", "contagem", "tokens"]);

        let mined = scan_at(root, None, false, mine_disk);
        assert_eq!(mined["lessons"]["missing_paths"], serde_json::json!([]), "o mapa acha o arquivo: {mined}");

        let failed = scan_at(root, None, false, mine_fails);
        assert_eq!(failed["ok"], serde_json::json!(false), "{failed}");
        assert_eq!(failed["lessons"]["missing_paths"], serde_json::json!([]), "o arquivo existe: {failed}");
        assert_eq!(failed["lessons"]["similar"], serde_json::json!([[first, second]]), "{failed}");
        let next = failed["next"].as_str().expect("o passo seguinte");
        assert!(!next.contains("\"targets\""), "nada a retirar: {next}");

        std::fs::remove_file(&cited).expect("apaga o arquivo citado");
        let failed = scan_at(root, None, false, mine_fails);
        assert_eq!(failed["lessons"]["missing_paths"], serde_json::json!([]), "sem o mapa, nada falta: {failed}");
    }

    /// Sem banco de lições, ou sem nada a enxugar, o relatório do scan não
    /// ganha a lista nem o passo seguinte.
    #[test]
    fn a_scan_with_nothing_to_trim_in_the_bank_adds_no_next_step() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(&root.join("src/main.rs"), "fn main() {}\n");
        let result = scan_at(root, None, false, mine_disk);
        assert!(result.get("lessons").is_none() && result.get("next").is_none(), "{result}");
        let bank = ClaudePaths::for_project(root).expect("paths").lessons_path();
        rule(&bank, "src", "O `main.rs` só chama a biblioteca.", &["main"]);
        let result = scan_at(root, None, false, mine_disk);
        assert!(result.get("lessons").is_none() && result.get("next").is_none(), "{result}");
    }

    /// A ferramenta do scan que grava o mapa dos arquivos do disco e devolve
    /// o relatório escrito na última linha, no formato que a ferramenta
    /// instalada imprime.
    fn mine_reporting(line: String) -> impl FnOnce(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport> {
        move |root, model| {
            mine_disk(root, model)?;
            Ok(serde_json::from_str(&line).expect("o relatório da ferramenta"))
        }
    }

    /// O comando de mapeamento lê o projeto e responde. A resposta diz
    /// quantos arquivos a ferramenta leu desta vez, e nenhum nome deles. Na
    /// primeira leitura são os dois arquivos do projeto; depois de mudar um,
    /// só esse é lido de novo, e a resposta diz 1, não os 2 que o mapa tem.
    /// Com os 1349 arquivos que a leitura da Suzano trouxe, a resposta diz
    /// 1349 e continua sem a lista.
    #[test]
    fn the_scan_answer_carries_only_the_count_of_files_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(&root.join("src/main.rs"), "fn main() {}\n");
        write(&root.join("src/lib.rs"), "pub fn a() {}\n");
        let head = "345b9361a952fba302c9f811626e26bea7fac2f2";

        let whole = format!(r#"{{"ok":true,"full":true,"read":["src/lib.rs","src/main.rs"],"files":2,"head":"{head}"}}"#);
        let answer = scan_at(root, None, false, mine_reporting(whole));
        assert_eq!(answer["ok"], json!(true), "{answer}");
        assert_eq!(answer["full"], json!(true), "{answer}");
        assert_eq!(answer["read"], json!(2), "{answer}");
        assert_eq!(answer["files"], json!(2), "{answer}");

        let changed = format!(r#"{{"ok":true,"full":false,"read":["src/lib.rs"],"files":2,"head":"{head}"}}"#);
        let answer = scan_at(root, None, false, mine_reporting(changed));
        assert_eq!(answer["full"], json!(false), "{answer}");
        assert_eq!(answer["read"], json!(1), "{answer}");
        let text = answer.to_string();
        assert!(!text.contains("src/lib.rs") && !text.contains("src/main.rs"), "a resposta não leva os nomes: {text}");

        let read: Vec<String> = (0..1349).map(|n| format!("src/modulo_{n}.ts")).collect();
        let suzano = json!({"ok": true, "full": true, "read": read, "files": 1349, "head": head}).to_string();
        let answer = scan_at(root, None, false, mine_reporting(suzano));
        assert_eq!(answer["read"], json!(1349), "{answer}");
        let text = answer.to_string();
        assert!(!text.contains("modulo_"), "a resposta não leva os nomes: {text}");
    }

    #[test]
    fn a_repo_without_submodules_has_nothing_to_report() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(hollow_submodules(dir.path()).is_empty(), "no .gitmodules → fail-open");
    }

    #[test]
    fn a_declared_but_unpopulated_submodule_is_reported() {
        // The shape that costs a scan run: `.gitmodules` promises the subtree,
        // the directory is there, and it is empty. Only the declaration and the
        // emptiness matter — never what the subtree is written in.
        let dir = tempfile::tempdir().expect("tempdir");
        write(&dir.path().join(".gitmodules"), "[submodule \"sub\"]\n\tpath = sub\n\turl = u\n");
        std::fs::create_dir_all(dir.path().join("sub")).expect("empty dir");
        assert_eq!(hollow_submodules(dir.path()), vec!["sub".to_string()]);

        // A missing directory is the same hole.
        let gone = tempfile::tempdir().expect("tempdir");
        write(&gone.path().join(".gitmodules"), "[submodule \"sub\"]\n\tpath = sub\n");
        assert_eq!(hollow_submodules(gone.path()), vec!["sub".to_string()]);
    }

    #[test]
    fn any_file_at_all_counts_as_populated() {
        // The check is emptiness, not content: the miner decides what is source,
        // and this preflight stays blind to language, extension and layout.
        let dir = tempfile::tempdir().expect("tempdir");
        write(&dir.path().join(".gitmodules"), "[submodule \"sub\"]\n\tpath = sub\n\turl = u\n");
        write(&dir.path().join("sub").join("anything"), "x");
        assert!(hollow_submodules(dir.path()).is_empty(), "checked out → silent");
    }

    #[test]
    fn every_declared_path_is_checked_not_just_the_first() {
        // A superproject with several submodules must not hide the second hole
        // behind the first populated one.
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            &dir.path().join(".gitmodules"),
            "[submodule \"a\"]\n\tpath = vendor/a\n[submodule \"b\"]\n\tpath = vendor/b\n",
        );
        write(&dir.path().join("vendor").join("a").join("anything"), "x");
        std::fs::create_dir_all(dir.path().join("vendor").join("b")).expect("empty dir");
        assert_eq!(hollow_submodules(dir.path()), vec!["vendor/b".to_string()]);
    }
}
