//! A conferência dos restos depois da onda: o que a onda tirou do projeto
//! sai completo. Tirado é a declaração ou o arquivo que está no mapa da base
//! da rodada e não está no mapa de depois da junção.
//!
//! - Resto pelo nome: o nome tirado, como palavra inteira, nos arquivos que
//!   o git rastreia e nos que a onda criou (código, comentário, teste,
//!   documento, molde), com o arquivo e a linha. O nome que continua declarado noutro lugar não conta pelo texto:
//!   ali o texto pode citar a outra declaração, e o mapa de depois já liga
//!   cada chamada a ela. A spec (`.claude/spec/`), o registro de mudanças do
//!   projeto e o histórico do git ficam de fora: são registro do que
//!   aconteceu. Só se procura o nome que não se confunde com uma palavra da
//!   prosa ([`distinctive`]).
//! - Órfão: a declaração que tinha uso fora de teste no mapa da base, e cujo
//!   último uso a onda tirou, e ficou sem nenhum. Teste chamando não conta
//!   como uso: nem o arquivo de teste, nem o trecho de teste de um arquivo
//!   do programa, que o scan reconhece e o mapa guarda pelas linhas. A
//!   declaração escrita dentro desse trecho é do teste e não entra. A
//!   declaração já sem uso na base não entra, e o ponto de entrada nunca é
//!   órfão: a função principal, a que atende uma rota do
//!   mapa, o método que cumpre um contrato (chamado por quem registra o
//!   tipo) e a declarada no arquivo de entrada da pasta, que exporta o
//!   pacote. O órfão cujo nome ainda aparece como palavra inteira fora de
//!   teste e fora das linhas dele, pela mesma busca do resto pelo nome, vai
//!   só como aviso: o texto pode ser um uso que o mapa não ligou.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::ast::{is_entry_file, is_test_path};
use mustard_core::domain::project_map::{MapDecl, MapModule, ProjectMap, UseSite};
use mustard_core::platform::git as git_exec;
use mustard_core::platform::i18n::{translate, Locale};

use super::commit::{AfterWave, Finding};

/// Os nomes de arquivo, sem a extensão e sem diferença de caixa, do registro
/// de mudanças de um projeto.
const CHANGE_LOGS: &[&str] = &["changelog", "changes", "history", "news", "releases", "release_notes", "release-notes"];

/// As extensões de texto que o registro de mudanças usa; sem extensão
/// também vale.
const CHANGE_LOG_EXTENSIONS: &[&str] = &["md", "txt", "rst", "adoc"];

/// Os achados dos restos e dos órfãos de cada onda de `maps`, procurando o
/// texto em `root`, já com a junção no disco.
pub(super) fn findings(root: &Path, maps: &AfterWave, lang: Locale) -> Vec<Finding> {
    let declared: BTreeSet<&str> =
        maps.after.modules.iter().flat_map(|m| &m.declarations).map(|d| d.name.as_str()).collect();
    let file_names: BTreeSet<&str> = maps.after.modules.iter().map(|m| file_name(&m.path)).collect();
    let created = created(root, maps);
    let mut searched: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for (wave, files) in &maps.changed {
        for file in files {
            let Some(before) = maps.base.module(file) else { continue };
            let now = maps.after.module(file);
            let mut gone: Vec<&str> = before
                .declarations
                .iter()
                .map(|d| d.name.as_str())
                .filter(|name| !now.is_some_and(|m| m.declarations.iter().any(|d| d.name == *name)))
                .filter(|name| !declared.contains(name) && distinctive(name))
                .collect();
            if now.is_none() && !file_names.contains(file_name(file)) {
                gone.push(file_name(file));
            }
            for name in gone.into_iter().filter(|name| searched.insert((*name).to_string())) {
                for (site, line) in cited(root, name, &created) {
                    let text = translate("round.after_wave.leftover", lang)
                        .replace("{file}", &site)
                        .replace("{line}", &line.to_string())
                        .replace("{name}", name)
                        .replace("{from}", file);
                    out.push(Finding { wave: *wave, refuses: true, text });
                }
            }
        }
    }
    out.extend(orphans(root, maps, &created, lang));
    out
}

/// As declarações que ficaram sem uso fora de teste porque uma onda tirou o
/// último, cada uma com a onda que tirou. A que ainda tem o nome escrito
/// fora de teste ([`cited_outside_tests`]) vai só como aviso: o texto pode
/// ser um uso que o mapa não ligou.
fn orphans(root: &Path, maps: &AfterWave, created: &[String], lang: Locale) -> Vec<Finding> {
    let mut out = Vec::new();
    for module in maps.after.modules.iter().filter(|m| !is_test_path(&m.path) && !is_entry_file(&m.path, &m.language)) {
        let Some(before) = maps.base.module(&module.path) else { continue };
        for decl in module.declarations.iter().filter(|decl| !in_test_lines(module, decl.line)) {
            let routed = module.routes.iter().chain(&before.routes).any(|route| route.handler == decl.name);
            let entry = decl.name == "main" || routed || !decl.implements.is_empty();
            if entry || decl.used_by.iter().any(|site| from_program(&maps.after, site)) {
                continue;
            }
            let Some(old) = before.declarations.iter().find(|d| d.name == decl.name && d.kind == decl.kind) else { continue };
            let callers: Vec<&str> =
                old.used_by.iter().filter(|site| from_program(&maps.base, site)).map(|site| site.file.as_str()).collect();
            let wave = maps.changed.iter().find(|(_, files)| files.iter().any(|f| callers.contains(&f.as_str())));
            let Some((wave, _)) = wave else { continue };
            let text = translate("round.after_wave.orphan", lang)
                .replace("{name}", &decl.name)
                .replace("{file}", &module.path)
                .replace("{line}", &decl.line.to_string());
            let refuses = !cited_outside_tests(root, maps, created, module, decl);
            out.push(Finding { wave: *wave, refuses, text });
        }
    }
    out
}

/// O uso `site` é do programa, pelo mapa `map` em que ele está: fora de
/// arquivo de teste e fora dos trechos de teste do arquivo dele.
fn from_program(map: &ProjectMap, site: &UseSite) -> bool {
    let line = u64::try_from(site.line).unwrap_or(u64::MAX);
    !is_test_path(&site.file) && !map.module(&site.file).is_some_and(|module| in_test_lines(module, line))
}

/// A linha `line` cai dentro de um trecho de teste do arquivo `module`, pelas
/// linhas que o mapa guarda de cada um.
fn in_test_lines(module: &MapModule, line: u64) -> bool {
    module.test_lines.iter().any(|&(first, last)| (first..=last).contains(&line))
}

/// O nome de `decl`, declarada em `module`, aparece como palavra inteira
/// num arquivo que o git rastreia ou que a onda criou ([`cited`]), fora de
/// arquivo de teste, fora dos trechos de teste de cada arquivo no mapa de
/// depois e fora das linhas da própria declaração.
fn cited_outside_tests(root: &Path, maps: &AfterWave, created: &[String], module: &MapModule, decl: &MapDecl) -> bool {
    let own = decl.line..=decl.end_line.max(decl.line);
    cited(root, &decl.name, created).iter().any(|(file, line)| {
        let line = u64::try_from(*line).unwrap_or(u64::MAX);
        let own_lines = *file == module.path && own.contains(&line);
        let test_block = maps.after.module(file).is_some_and(|site| in_test_lines(site, line));
        let skipped = is_test_path(file) || own_lines || test_block;
        !skipped
    })
}

/// O nome de arquivo de `path`, sem as pastas.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// O nome que se procura como texto: o que não se confunde com uma palavra
/// comum da prosa — com `_`, `.`, `-` ou dígito, ou com maiúscula depois da
/// primeira letra (`camelCase`, `PascalCase` de duas partes, `CONSTANTE`). A
/// palavra solta (`total`, `Report`) aparece em todo comentário, e o uso
/// dela no código a compilação já recusa.
fn distinctive(name: &str) -> bool {
    name.chars().count() >= 3
        && (name.contains(['_', '.', '-'])
            || name.chars().any(|c| c.is_ascii_digit())
            || name.chars().skip(1).any(char::is_uppercase))
}

/// Os arquivos que as ondas de `maps` criaram e o git ainda não rastreia,
/// fora os que ele ignora.
fn created(root: &Path, maps: &AfterWave) -> Vec<String> {
    let mut args = vec!["-c", "core.quotePath=false", "ls-files", "-z", "--others", "--exclude-standard", "--"];
    args.extend(maps.changed.iter().flat_map(|(_, files)| files).map(String::as_str));
    let out = git_exec::run(root, &args);
    out.stdout.split('\0').filter(|file| !file.is_empty()).map(str::to_string).collect()
}

/// Cada lugar que cita `name` como palavra inteira, com o arquivo e a linha:
/// nos arquivos que o git rastreia em `root` e nos criados (`created`). A
/// spec e o registro de mudanças ficam de fora.
fn cited(root: &Path, name: &str, created: &[String]) -> Vec<(String, usize)> {
    fn grep<'a>(name: &'a str, untracked: &[&'a str], paths: &[&'a str]) -> Vec<&'a str> {
        let head = ["-c", "core.quotePath=false", "grep"];
        [&head[..], untracked, &["-I", "-n", "-z", "-w", "-F", "-e", name, "--"], paths].concat()
    }
    let mut searches = vec![grep(name, &[], &[".", ":(exclude).claude/spec"])];
    if !created.is_empty() {
        let files: Vec<&str> = created.iter().map(String::as_str).collect();
        searches.push(grep(name, &["--untracked"], &files));
    }
    let mut found = BTreeSet::new();
    for args in searches {
        for line in git_exec::run(root, &args).stdout.lines() {
            let mut parts = line.splitn(3, '\0');
            let (Some(file), Some(Ok(at))) = (parts.next(), parts.next().map(str::parse)) else { continue };
            if !is_change_log(file) {
                found.insert((file.to_string(), at));
            }
        }
    }
    found.into_iter().collect()
}

/// `path` é o registro de mudanças do projeto (`CHANGELOG.md`, `NEWS`…)?
fn is_change_log(path: &str) -> bool {
    let name = file_name(path).to_lowercase();
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name.as_str(), ""));
    CHANGE_LOGS.contains(&stem) && (extension.is_empty() || CHANGE_LOG_EXTENSIONS.contains(&extension))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::super::imports_check::tests::mine_giving;
    use super::super::tests::{approved, delivered, git_at, git_text, round, round_with_mine, write};
    use super::*;

    /// Um arquivo do mapa, com as declarações `decls` (nome, linha e quem a
    /// usa, como o scan grava: `arquivo:linha:quem`).
    fn module(path: &str, decls: &[(&str, u64, &[&str])]) -> Value {
        let declarations: Vec<Value> = decls
            .iter()
            .map(|(name, line, used)| json!({"kind": "function", "name": name, "line": line, "end_line": line + 2, "used_by": used}))
            .collect();
        json!({"path": path, "language": "rust", "declarations": declarations})
    }

    /// Uma spec aprovada com a onda 1, que muda `src/a.rs`; os arquivos
    /// `files` comitados no projeto; a onda já enviada; e o mapa da base
    /// `base`.
    fn project(root: &Path, files: &[(&str, &str)], base: &Value) {
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        git_at(root, &["add", "-A", "--", ".", ":(exclude).claude"]);
        git_at(root, &["commit", "-q", "-m", "arquivos"]);
        round(root, "x", None);
        // O mapa da base entra depois do envio, como na conferência das
        // importações.
        mustard_core::io::project_map::write_text(root, &base.to_string()).unwrap();
    }

    /// A volta da onda 1 mudando `src/a.rs`, com o mapa de depois `after`.
    fn back(root: &Path, after: Value) -> Value {
        let report = delivered(root, 1, "A soma mudou.", &["src/a.rs"]);
        round_with_mine(root, "x", Some(&report), &mine_giving(after))
    }

    /// A resposta não traz texto nenhum da conferência depois da onda.
    fn silent(out: &Value) {
        assert_eq!(out["ok"], json!(true), "{out}");
        let text = out.to_string();
        for said in ["round-after-wave", "conferência depois da onda", "ainda cita", "sem uso fora de teste"] {
            assert!(!text.contains(said), "{said}: {out}");
        }
    }

    #[test]
    fn removing_a_function_but_leaving_a_comment_that_cites_it_gets_the_fix() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("src/b.rs", "fn outra() {}\n// soma pelo old_total antes de gravar\n")];
        let base = json!({"modules": [module("src/a.rs", &[("old_total", 1, &[]), ("keep_sum", 5, &[])]), module("src/b.rs", &[("outra", 1, &[])])]});
        project(root, &files, &base);
        let head = git_text(root, &["rev-parse", "HEAD"]);
        let after = json!({"modules": [module("src/a.rs", &[("keep_sum", 1, &[])]), module("src/b.rs", &[("outra", 1, &[])])]});
        let out = back(root, after);
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`src/b.rs` linha 2 ainda cita `old_total`, que a onda tirou de `src/a.rs`"), "{hint}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "nothing committed: {out}");
    }

    #[test]
    fn removing_the_only_caller_of_a_tested_function_makes_it_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let before = json!({"modules": [
            module("src/a.rs", &[("run_sum", 1, &[])]),
            module("src/lib_sum.rs", &[("compute_total", 5, &["src/a.rs:2:run_sum", "tests/lib_sum_test.rs:3:checks"])]),
        ]});
        project(root, &[("src/lib_sum.rs", "\n\n\n\nfn compute_total() {}\n")], &before);
        let after = json!({"modules": [
            module("src/a.rs", &[("run_sum", 1, &[])]),
            module("src/lib_sum.rs", &[("compute_total", 5, &["tests/lib_sum_test.rs:3:checks"])]),
        ]});
        let out = back(root, after);
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/lib_sum.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
    }

    #[test]
    fn a_function_already_unused_before_the_wave_is_not_listed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let map = json!({"modules": [
            module("src/a.rs", &[("run_sum", 1, &[])]),
            module("src/lib_sum.rs", &[("never_called", 5, &["tests/lib_sum_test.rs:3:checks"])]),
        ]});
        project(root, &[("src/lib_sum.rs", "fn never_called() {}\n")], &map);
        silent(&back(root, map));
    }

    #[test]
    fn a_removed_name_still_declared_elsewhere_gives_no_text_leftover() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("src/b.rs", "// chama o shared_total da outra pasta\n"), ("src/c.rs", "fn shared_total() {}\n")];
        let base = json!({"modules": [module("src/a.rs", &[("shared_total", 1, &[])]), module("src/c.rs", &[("shared_total", 1, &[])])]});
        project(root, &files, &base);
        let after = json!({"modules": [module("src/a.rs", &[]), module("src/c.rs", &[("shared_total", 1, &[])])]});
        silent(&back(root, after));
    }

    #[test]
    fn the_spec_the_change_log_and_the_history_do_not_count() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("CHANGELOG.md", "- tira o old_total\n"), ("docs/NEWS", "old_total saiu\n")];
        project(root, &files, &json!({"modules": [module("src/a.rs", &[("old_total", 1, &[])])]}));
        // O nome fica também na spec e numa mensagem de commit.
        let said = write(root, "x", "message", json!({"author": "user", "text": "O old_total sai nesta onda."}));
        assert!(said["id"].is_u64(), "{said}");
        git_at(root, &["commit", "-q", "--allow-empty", "-m", "prepara a saída do old_total"]);
        silent(&back(root, json!({"modules": [module("src/a.rs", &[])]})));
    }

    #[test]
    fn a_registered_route_with_no_caller_is_not_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let routed = |used: &[&str]| {
            let mut routes = module("src/routes.rs", &[("list_orders", 4, used)]);
            routes["routes"] = json!([{"method": "GET", "path": "/orders", "handler": "list_orders", "line": 4, "called_by": []}]);
            json!({"modules": [module("src/a.rs", &[("run_sum", 1, &[])]), routes]})
        };
        project(root, &[("src/routes.rs", "fn list_orders() {}\n")], &routed(&["src/a.rs:2:run_sum"]));
        silent(&back(root, routed(&[])));
    }

    /// O mapa de `src/a.rs`, com o trecho de teste nas linhas 10 a 30: a
    /// função `compute_total`, do programa, na linha 5, e a ajudante
    /// `sample_rows`, do teste, na linha 12, cada uma com quem a usa.
    fn with_test_block(total_used: &[&str], rows_used: &[&str]) -> Value {
        let mut file = module("src/a.rs", &[
            ("run_sum", 1, &[]),
            ("compute_total", 5, total_used),
            ("sample_rows", 12, rows_used),
            ("checks_total", 15, &[]),
        ]);
        file["test_lines"] = json!([[10, 30]]);
        json!({"modules": [file]})
    }

    #[test]
    fn a_helper_inside_the_test_block_that_loses_its_use_is_not_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = with_test_block(&["src/a.rs:2:run_sum"], &["src/a.rs:16:checks_total"]);
        project(root, &[("src/a.rs", "fn run_sum() {}\n")], &base);
        silent(&back(root, with_test_block(&["src/a.rs:2:run_sum"], &[])));
    }

    #[test]
    fn a_program_function_beside_the_test_block_that_loses_its_use_is_still_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = with_test_block(&["src/a.rs:2:run_sum"], &["src/a.rs:16:checks_total"]);
        project(root, &[("src/a.rs", "fn run_sum() {}\n")], &base);
        let out = back(root, with_test_block(&[], &[]));
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
        assert!(!hint.contains("sample_rows"), "the test helper is not listed: {hint}");
    }

    #[test]
    fn a_program_function_left_called_only_from_the_test_block_is_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = with_test_block(&["src/a.rs:2:run_sum", "src/a.rs:16:checks_total"], &[]);
        project(root, &[("src/a.rs", "fn run_sum() {}\n")], &base);
        let out = back(root, with_test_block(&["src/a.rs:16:checks_total"], &[]));
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
    }

    #[test]
    fn a_program_function_used_only_from_the_test_block_before_the_wave_is_not_listed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = with_test_block(&["src/a.rs:16:checks_total"], &[]);
        project(root, &[("src/a.rs", "fn run_sum() {}\n")], &base);
        silent(&back(root, with_test_block(&[], &[])));
    }

    #[test]
    fn a_program_function_still_cited_in_another_file_outside_tests_only_warns() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("src/a.rs", "fn run_sum() {}\n"), ("src/b.rs", "use crate::a as calc;\n\nfn go() {\n    calc::compute_total();\n}\n")];
        project(root, &files, &with_test_block(&["src/a.rs:2:run_sum"], &["src/a.rs:16:checks_total"]));
        let out = back(root, with_test_block(&[], &[]));
        assert_eq!(out["ok"], json!(true), "{out}");
        let warned = out["warnings"].as_array().cloned().unwrap_or_default();
        let hint = warned.iter().find(|w| w["reason"] == json!("round-after-wave-warnings")).map(|w| w["hint"].to_string());
        let hint = hint.unwrap_or_else(|| panic!("the warning: {out}"));
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
    }

    #[test]
    fn a_program_function_cited_only_by_tests_or_its_own_lines_still_refuses() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // O nome aparece na linha da declaração, dentro do corpo dela, no
        // trecho de teste do arquivo (linhas 10 a 30) e num arquivo de teste.
        let mut lines = vec![""; 30];
        lines[0] = "fn run_sum() {}";
        lines[4] = "fn compute_total() {";
        lines[5] = "    compute_total();";
        lines[6] = "}";
        lines[9] = "#[cfg(test)]";
        lines[10] = "mod tests {";
        lines[11] = "    fn sample_rows() { super::compute_total(); }";
        lines[29] = "}";
        let text = lines.join("\n") + "\n";
        let files = [("src/a.rs", text.as_str()), ("tests/total_test.rs", "fn checks() { compute_total(); }\n")];
        project(root, &files, &with_test_block(&["src/a.rs:2:run_sum"], &["src/a.rs:16:checks_total"]));
        let out = back(root, with_test_block(&[], &[]));
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
    }
}
