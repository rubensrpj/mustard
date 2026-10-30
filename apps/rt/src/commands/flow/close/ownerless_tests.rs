//! Os testes sem dono da obra inteira, no fechamento, pela leitura comum
//! da rodada e do fechamento ([`hints`]).

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::platform::i18n::Locale;

use crate::commands::flow::common::ownerless::hints;

/// [`hints`] da obra `spec` inteira, no fechamento: o retrato da base da obra
/// contra o mapa de agora, nos arquivos que os commits dela levaram. Sem um
/// dos dois mapas, nenhum aviso: a análise final de declarações sem uso já
/// diz que não houve com que comparar.
pub(super) fn at_close(root: &Path, spec: &str, log: &SpecLog, lang: Locale) -> Vec<String> {
    let Some((before, after)) = crate::commands::flow::round::work_maps(root, spec) else {
        return Vec::new();
    };
    let files: Vec<String> = log.delivered_files().into_iter().collect();
    hints(log, &before, &after, &files, lang)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::tests::{close, id_of, returned, round_for, write};
    use crate::commands::flow::common::ownerless::REASON;
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

    /// A declaração `name` de tipo `kind` na linha `line`, com a marca de teste
    /// do mapa quando `test`.
    fn decl(kind: &str, name: &str, line: u64, test: bool) -> Value {
        let syntax = if test { json!({"test": true}) } else { Value::Null };
        json!({"kind": kind, "name": name, "line": line, "end_line": line, "syntax": syntax, "used_by": []})
    }

    /// O módulo `path` da língua `language` no mapa, com o trecho de teste
    /// `tests` e as declarações `declarations`.
    fn module(path: &str, language: &str, tests: (u64, u64), declarations: Vec<Value>) -> Value {
        json!({"path": path, "language": language, "test_lines": [[tests.0, tests.1]], "declarations": declarations})
    }

    /// Os textos dos avisos de teste sem dono de uma resposta.
    fn ownerless(answer: &Value) -> Vec<String> {
        let warnings = answer["warnings"].as_array().into_iter().flatten();
        warnings.filter(|w| w["reason"] == json!(REASON)).filter_map(|w| w["hint"].as_str().map(str::to_string)).collect()
    }

    /// Uma obra de uma onda que muda os arquivos `base` para `after` e cria
    /// os arquivos novos de `after`, com um critério cuja prova é `proof`, e
    /// os mapas `before` e `later` de antes e de depois dela. Devolve os
    /// avisos de teste sem dono da rodada que junta a onda e os do
    /// fechamento da obra, e o texto que cada aviso de `expected` (o nome, o
    /// arquivo e a linha) tem na língua do projeto.
    fn warned(
        base: &[(&str, &str)],
        after: &[(&str, &str)],
        proof: &str,
        (before, later): (Vec<Value>, Vec<Value>),
        expected: &[(&str, &str, u64)],
    ) -> (Vec<String>, Vec<String>, Vec<String>) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec = "x";
        let mut seed = vec![("mustard.json", "{}")];
        seed.extend_from_slice(base);
        crate::shared::test_fixture::seeded_repo_on(root, &format!("feature/{spec}"), &seed);
        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        let gate = id_of(&write(
            root,
            spec,
            "criterion",
            json!({"when": "a onda roda", "then": "o teste citado passa", "proof": proof, "form": "ubiquitous", "origin": said}),
        ));
        let files: Vec<&str> = after.iter().map(|(path, _)| *path).collect();
        let task_files: Vec<Value> = files.iter().map(|path| json!({"path": path})).collect();
        write(root, spec, "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [gate], "done_when": "A suíte passa.", "origin": said}));
        write(root, spec, "task", json!({"wave": 1, "text": "Tarefa da onda 1.", "files": task_files, "origin": said}));
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        copies_leave_with_the_test(root);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":4}"#).unwrap();
        let dispatch = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: None }, None);
        assert_eq!(dispatch["ok"], json!(true), "{dispatch}");

        // O mapa da base entra depois do envio, que relê o mapa do disco.
        mustard_core::io::project_map::write_text(root, &json!({"modules": before}).to_string()).unwrap();
        for (path, body) in after {
            std::fs::write(root.join(path), body).unwrap();
        }
        returned(root, spec, json!({"wave": 1, "text": "Saiu.", "files": files, "commit": "testes novos saem"}));
        let later = json!({"modules": later});
        let back = round_with_mine(root, spec, None, &mine_giving(later.clone()));
        assert_eq!(back["ok"], json!(true), "{back}");
        let in_round = ownerless(&back);

        // A passada do mapa depois do commit deixa o mapa do projeto no de
        // depois da onda, e é ele que o fechamento lê.
        mustard_core::io::project_map::write_text(root, &later.to_string()).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        let closed = close(root, spec);
        assert_eq!(closed["phase"], json!("closed"), "{closed}");
        let at_close = ownerless(&closed);

        let lang = crate::commands::spec_events::project(root).lang;
        let text = |(name, file, line): &(&str, &str, u64)| {
            translate("close.unowned_test", lang).replace("{name}", name).replace("{file}", file).replace("{line}", &line.to_string())
        };
        (in_round, at_close, expected.iter().map(text).collect())
    }

    /// A onda muda um teste que a base já tinha e cria dois: um que a prova
    /// do critério cita e um que nenhuma prova cita. A rodada que junta a
    /// onda e o fechamento da obra dão a mesma lista, com um aviso só: o do
    /// teste novo sem critério, com o arquivo e a linha dele. A função nova
    /// sem a marca de teste do mapa não é teste.
    #[test]
    fn only_a_test_the_work_created_can_be_ownerless() {
        let before = vec![module("src/w1.rs", "rust", (2, 6), vec![decl("function", "one", 1, false), decl("function", "old_check", 5, true)])];
        let later = vec![module(
            "src/w1.rs",
            "rust",
            (2, 10),
            vec![
                decl("function", "one", 1, false),
                decl("function", "old_check", 5, true),
                decl("function", "cited_check", 7, true),
                decl("function", "fresh_check", 9, true),
                decl("function", "test_helper", 10, false),
            ],
        )];
        let (in_round, at_close, expected) =
            warned(&[("src/w1.rs", BEFORE)], &[("src/w1.rs", AFTER)], "echo running 1 test cited_check", (before, later), &[("fresh_check", "src/w1.rs", 9)]);
        assert_eq!(in_round, expected);
        assert_eq!(at_close, expected);
    }

    /// O teste novo de qualquer forma que a língua e os frameworks dela usam,
    /// sem critério que o cite, ganha o aviso na rodada e no fechamento: o do
    /// Rust por `#[tokio::test]`, sem o `#[test]` em cima nem nome começado
    /// por `test_`, e o do TypeScript por `it(...)`, que o mapa guarda como
    /// declaração de teste com o texto do nome. A prova do critério cita um
    /// teste que a obra criou: citar o que a base já tinha passaria também no
    /// começo da obra, e o fechamento a recusaria.
    #[test]
    fn a_new_test_of_any_form_without_a_criterion_is_warned() {
        let rust_after = concat!(
            "fn one() {}\n#[cfg(test)]\nmod tests {\n",
            "    #[tokio::test]\n    async fn tokio_check() { assert_eq!(2, 2); }\n",
            "    #[test]\n    fn old_check() { assert_eq!(1, 1); }\n",
            "    #[test]\n    fn cited_check() { assert_eq!(3, 3); }\n}\n",
        );
        let typescript = "import { sum } from \"./sum\";\n\nit(\"adds two numbers\", () => {\n  expect(sum(1, 2)).toBe(3);\n});\n";
        let before = vec![module("src/w1.rs", "rust", (2, 6), vec![decl("function", "one", 1, false), decl("function", "old_check", 5, true)])];
        let later = vec![
            module(
                "src/w1.rs",
                "rust",
                (2, 10),
                vec![
                    decl("function", "one", 1, false),
                    decl("function", "tokio_check", 4, true),
                    decl("function", "old_check", 6, true),
                    decl("function", "cited_check", 8, true),
                ],
            ),
            module("src/sum.test.ts", "typescript", (3, 5), vec![decl("test", "adds two numbers", 3, true)]),
        ];
        let (in_round, at_close, expected) = warned(
            &[("src/w1.rs", BEFORE)],
            &[("src/w1.rs", rust_after), ("src/sum.test.ts", typescript)],
            "echo running 1 test cited_check",
            (before, later),
            &[("adds two numbers", "src/sum.test.ts", 3), ("tokio_check", "src/w1.rs", 4)],
        );
        assert_eq!(in_round, expected);
        assert_eq!(at_close, expected);
    }
}
