//! New test candidates are observations, not a count gate or coverage proof.
use mustard_core::domain::project_map::ProjectMap;
use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn candidates(root: &Path, log: &SpecLog, before: &ProjectMap, after: &ProjectMap, changed: &[String], lang: Locale) -> Vec<Value> {
    let proofs = log.block(BlockQuery::Block(Block::Criteria)).into_iter().filter_map(|e| e.str_field("proof")).collect::<Vec<_>>().join("\n");
    let mut out = Vec::new();
    for file in changed {
        let Some(module) = after.module(file) else {
            continue;
        };
        let text = std::fs::read_to_string(root.join(file)).ok();
        let lines: Vec<_> = text.as_deref().into_iter().flat_map(str::lines).collect();
        for decl in &module.declarations {
            if !matches!(decl.kind.as_str(), "function" | "method" | "test")
                || before.module(file).is_some_and(|m| m.declarations.iter().any(|d| d.name == decl.name && d.kind == decl.kind))
            {
                continue;
            }
            let at = usize::try_from(decl.line.saturating_sub(1)).unwrap_or(usize::MAX);
            let annotated = lines
                .get(at.saturating_sub(3)..at.min(lines.len()))
                .is_some_and(|prior| prior.iter().any(|line| line.contains("#[test]") || line.contains("@Test")));
            let named = decl.name.starts_with("Test") || decl.name.starts_with("test_") || decl.kind == "test";
            let in_tests = mustard_core::domain::ast::is_test_path(file) || module.test_lines.iter().any(|(from, to)| (*from..=*to).contains(&decl.line));
            if !(annotated || (named && in_tests)) || proofs.contains(&decl.name) {
                continue;
            }
            let hint = match lang {
                Locale::EnUs => format!(
                    "{file}:{}: new test `{}` has no direct criterion proof reference. Verify its behavioral contribution in the review; this candidate is not coverage evidence.",
                    decl.line, decl.name
                ),
                Locale::PtBr => format!(
                    "{file}:{}: teste novo `{}` sem referência direta na prova de um critério. Confira sua contribuição de comportamento na revisão; este candidato não prova cobertura.",
                    decl.line, decl.name
                ),
            };
            out.push(json!({"reason":"new-test-contribution-unknown","file":file,"line":decl.line,"test":decl.name,"evidence":"syntax-candidate","hint":hint}));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_new_python_test_without_a_criterion_reference_warns_with_its_location() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("tests/test_api.py"), "def test_empty_input():\n    assert reject('')\n").unwrap();
        let after: ProjectMap = serde_json::from_value(
            json!({"modules":[{"path":"tests/test_api.py","language":"python","declarations":[{"name":"test_empty_input","kind":"function","line":1}]}]}),
        )
        .unwrap();
        let empty = mustard_core::domain::spec_events::parse_log("");
        let found = candidates(dir.path(), &empty, &ProjectMap::default(), &after, &["tests/test_api.py".into()], Locale::EnUs);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0]["file"].clone(), found[0]["line"].clone()), (json!("tests/test_api.py"), json!(1)));
        assert_eq!(found[0]["evidence"], "syntax-candidate");
        assert!(candidates(dir.path(), &empty, &after, &after, &["tests/test_api.py".into()], Locale::PtBr).is_empty());
    }
}
