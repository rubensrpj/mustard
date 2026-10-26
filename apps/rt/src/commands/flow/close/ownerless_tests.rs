//! Os testes sem dono: os que a obra criou e que nenhum critério cita na
//! prova dele. A rodada os aponta ao juntar as ondas, com o mapa de antes e o
//! de depois delas; o fechamento, com o retrato da base da obra e o mapa de
//! agora. É a mesma leitura nos dois, e só aviso, nunca recusa. O teste que
//! já existia na base fica de fora, mesmo mudado: a obra não o criou.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::project_map::{MapDecl, MapModule, ProjectMap};
use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use mustard_core::platform::i18n::{Locale, translate};

/// O motivo do aviso de teste sem dono, o mesmo na rodada e no fechamento.
pub(crate) const REASON: &str = "unowned-test";

/// O aviso de cada teste que `after` declara num arquivo de `files` e que
/// `before` não tinha no mesmo arquivo, quando nenhuma prova de critério do
/// `log` cita o nome dele. Sai na ordem dos arquivos e, em cada um, na das
/// declarações. O texto de cada arquivo, lido de `root`, diz se a declaração
/// leva a marca de teste logo acima dela.
pub(crate) fn hints(root: &Path, log: &SpecLog, before: &ProjectMap, after: &ProjectMap, files: &[String], lang: Locale) -> Vec<String> {
    let proofs: Vec<&str> =
        log.block(BlockQuery::Block(Block::Criteria)).into_iter().filter(|e| e.event_type == "criterion").filter_map(|e| e.str_field("proof")).collect();
    let files: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for file in files {
        let Some(module) = after.module(file) else {
            continue;
        };
        let text = std::fs::read_to_string(root.join(file)).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let old = before.module(file);
        for decl in &module.declarations {
            let existed = old.is_some_and(|m| m.declarations.iter().any(|d| d.name == decl.name && d.kind == decl.kind));
            if existed || !is_test(file, module, decl, &lines) || proofs.iter().any(|proof| proof.contains(&decl.name)) {
                continue;
            }
            let hint = translate("close.unowned_test", lang).replace("{name}", &decl.name).replace("{file}", file).replace("{line}", &decl.line.to_string());
            out.push(hint);
        }
    }
    out
}

/// [`hints`] da obra `spec` inteira, no fechamento: o retrato da base da obra
/// contra o mapa de agora, nos arquivos que os commits dela levaram. Sem um
/// dos dois mapas, nenhum aviso: a análise final de declarações sem uso já
/// diz que não houve com que comparar.
pub(super) fn at_close(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Vec<String> {
    let Some((before, after)) = crate::commands::flow::round::work_maps(root, spec) else {
        return Vec::new();
    };
    let files: Vec<String> = log.delivered_files().into_iter().collect();
    hints(root, log, &before, &after, &files, lang)
}

/// Se a declaração `decl` do arquivo `file` é um teste: uma função com a marca
/// de teste numa das três linhas de cima (`lines` é o texto do arquivo), ou
/// com nome de teste dentro de arquivo ou trecho de teste. A leitura é de
/// sintaxe: o aviso que ela dá não prova cobertura nenhuma.
fn is_test(file: &str, module: &MapModule, decl: &MapDecl, lines: &[&str]) -> bool {
    if !matches!(decl.kind.as_str(), "function" | "method" | "test") {
        return false;
    }
    let at = usize::try_from(decl.line.saturating_sub(1)).unwrap_or(usize::MAX);
    let marked = lines
        .get(at.saturating_sub(3)..at.min(lines.len()))
        .is_some_and(|prior| prior.iter().any(|line| line.contains("#[test]") || line.contains("@Test")));
    let named = decl.name.starts_with("Test") || decl.name.starts_with("test_") || decl.kind == "test";
    let in_tests = mustard_core::domain::ast::is_test_path(file) || module.test_lines.iter().any(|(from, to)| (*from..=*to).contains(&decl.line));
    marked || (named && in_tests)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::tests::{close, id_of, returned, round_for, write};
    use super::REASON;
    use crate::commands::flow::round::{RoundOpts, copies_leave_with_the_test, mine_giving, round_with_mine};
    use crate::commands::spec_events::write::record_open;
    use mustard_core::platform::i18n::translate;

    /// O arquivo da onda na base da obra: um teste que já existia.
    const BEFORE: &str = "fn one() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn old_check() { assert_eq!(1, 1); }\n}\n";

    /// O arquivo depois da onda: o teste antigo mudou, e nasceram um teste que
    /// a prova do critério cita e outro que nenhuma prova cita.
    const AFTER: &str = concat!(
        "fn one() {}\n#[cfg(test)]\nmod tests {\n",
        "    #[test]\n    fn old_check() { assert_eq!(2, 2); }\n",
        "    #[test]\n    fn cited_check() { assert_eq!(3, 3); }\n",
        "    #[test]\n    fn fresh_check() { assert_eq!(4, 4); }\n}\n",
    );

    /// O mapa do arquivo da onda com as funções `names`, cada uma na linha
    /// dela, e o trecho de teste de `tests`.
    fn map_of(names: &[(&str, u64)], tests: (u64, u64)) -> Value {
        let declarations: Vec<Value> =
            names.iter().map(|(name, line)| json!({"kind": "function", "name": name, "line": line, "end_line": line, "used_by": []})).collect();
        json!({"modules": [{"path": "src/w1.rs", "language": "rust", "test_lines": [[tests.0, tests.1]], "declarations": declarations}]})
    }

    /// Os textos dos avisos de teste sem dono de uma resposta.
    fn ownerless(answer: &Value) -> Vec<String> {
        let warnings = answer["warnings"].as_array().into_iter().flatten();
        warnings.filter(|w| w["reason"] == json!(REASON)).filter_map(|w| w["hint"].as_str().map(str::to_string)).collect()
    }

    /// A onda muda um teste que a base já tinha e cria dois: um que a prova
    /// do critério cita e um que nenhuma prova cita. A rodada que junta a
    /// onda e o fechamento da obra dão a mesma lista, com um aviso só: o do
    /// teste novo sem critério, com o arquivo e a linha dele.
    #[test]
    fn only_a_test_the_work_created_can_be_ownerless() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = "x";
        crate::shared::test_fixture::seeded_repo_on(root, &format!("feature/{spec}"), &[("mustard.json", "{}"), ("src/w1.rs", BEFORE)]);
        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        let gate = id_of(&write(
            root,
            spec,
            "criterion",
            json!({"when": "a onda roda", "then": "o teste citado passa", "proof": "echo running 1 test cited_check",
                "form": "ubiquitous", "origin": said}),
        ));
        write(root, spec, "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [gate], "done_when": "A suíte passa.", "origin": said}));
        write(root, spec, "task", json!({"wave": 1, "text": "Tarefa da onda 1.", "files": [{"path": "src/w1.rs"}], "origin": said}));
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        copies_leave_with_the_test(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();
        let dispatch = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: None }, None);
        assert_eq!(dispatch["ok"], json!(true), "{dispatch}");

        // O mapa da base entra depois do envio, que relê o mapa do disco.
        let before = map_of(&[("one", 1), ("old_check", 5)], (2, 6));
        let after = map_of(&[("one", 1), ("old_check", 5), ("cited_check", 7), ("fresh_check", 9)], (2, 10));
        mustard_core::io::project_map::write_text(root, &before.to_string()).unwrap();
        std::fs::write(root.join("src/w1.rs"), AFTER).unwrap();
        returned(root, spec, json!({"wave": 1, "text": "Saiu.", "files": ["src/w1.rs"], "commit": "testes novos saem"}));
        let back = round_with_mine(root, spec, None, &mine_giving(after.clone()));
        assert_eq!(back["ok"], json!(true), "{back}");
        let in_round = ownerless(&back);

        // A passada do mapa depois do commit deixa o mapa do projeto no de
        // depois da onda, e é ele que o fechamento lê.
        mustard_core::io::project_map::write_text(root, &after.to_string()).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        let closed = close(root, spec);
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let at_close = ownerless(&closed);

        let lang = crate::commands::spec_events::project(root).lang;
        let expected = translate("close.unowned_test", lang).replace("{name}", "fresh_check").replace("{file}", "src/w1.rs").replace("{line}", "9");
        assert_eq!(in_round, vec![expected.clone()], "{back}");
        assert_eq!(at_close, vec![expected], "{closed}");
    }
}
