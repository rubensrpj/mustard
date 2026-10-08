//! A syntactic graph cannot prove absence of runtime use. Only a native
//! compiler diagnostic from the current final checks can strengthen a
//! candidate into a refusal. Unknown formats remain warnings.

use std::path::Path;

use mustard_core::domain::project_map::{MapDecl, MapModule};
use serde_json::Value;

pub(super) fn proves(root: &Path, module: &MapModule, decl: &MapDecl, outputs: &[String]) -> bool {
    let Ok(current) = root.join(&module.path).canonicalize() else {
        return false;
    };
    let Ok(project) = root.canonicalize() else {
        return false;
    };
    if !current.starts_with(&project) {
        return false;
    }
    let Ok(source) = std::fs::read_to_string(&current) else {
        return false;
    };
    outputs.iter().flat_map(|output| output.lines()).any(|line| {
        let Ok(document) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        let diagnostic = if document["reason"] == "compiler-message" {
            &document["message"]
        } else if document["$message_type"] == "diagnostic" {
            &document
        } else {
            return false;
        };
        if diagnostic["code"]["code"] != "dead_code"
            || !matches!(diagnostic["level"].as_str(), Some("warning" | "error"))
            || !diagnostic["message"].as_str().is_some_and(|message| message.contains(&format!("`{}`", decl.name)))
        {
            return false;
        }
        diagnostic["spans"].as_array().into_iter().flatten().any(|span| {
            if span["is_primary"] != true {
                return false;
            }
            let (Some(file), Some(line)) = (span["file_name"].as_str(), span["line_start"].as_u64()) else {
                return false;
            };
            if line < decl.line || line > decl.end_line.max(decl.line) || root.join(file).canonicalize().ok().as_ref() != Some(&current) {
                return false;
            }
            let Some(index) = line.checked_sub(1).and_then(|n| usize::try_from(n).ok()) else {
                return false;
            };
            let Some(actual) = source.lines().nth(index) else {
                return false;
            };
            // Old diagnostics or a diagnostic for a different declaration
            // must not be promoted merely because the paths match.
            span["text"].as_array().into_iter().flatten().any(|text| text["text"].as_str() == Some(actual) && actual.contains(&decl.name))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_current_compiler_diagnostic_is_required_not_a_complete_syntax_parse() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("math.rs"), "fn unused_sum() {}\n").unwrap();
        let module: MapModule = serde_json::from_value(json!({"path":"math.rs","language":"rust",
            "analysis":{"parse_complete":true,"relations":"syntactic-candidates"}}))
        .unwrap();
        let decl: MapDecl = serde_json::from_value(json!({"name":"unused_sum","kind":"function","line":1,"end_line":1})).unwrap();
        assert!(!proves(dir.path(), &module, &decl, &[]));
        let diagnostic = json!({"$message_type":"diagnostic","level":"warning","code":{"code":"dead_code"},
            "message":"function `unused_sum` is never used","spans":[{"file_name":"math.rs","line_start":1,
            "is_primary":true,"text":[{"text":"fn unused_sum() {}"}]}]})
        .to_string();
        assert!(proves(dir.path(), &module, &decl, std::slice::from_ref(&diagnostic)));
        std::fs::write(dir.path().join("math.rs"), "fn unused_sum() { runtime_registration(); }\n").unwrap();
        assert!(!proves(dir.path(), &module, &decl, &[diagnostic]));
    }
    #[test]
    fn actual_compiler_evidence_distinguishes_private_dead_code_from_a_public_contract() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("math.rs"), "fn unused_sum() {}\npub fn public_contract() {}\n").unwrap();
        let ran = std::process::Command::new("rustc").args(["--crate-type", "lib", "--error-format=json", "math.rs"]).current_dir(root).output().unwrap();
        assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
        let output = String::from_utf8_lossy(&ran.stderr).into_owned();
        let module: MapModule = serde_json::from_value(json!({"path":"math.rs","language":"rust"})).unwrap();
        let private: MapDecl = serde_json::from_value(json!({"name":"unused_sum","kind":"function","line":1,"end_line":1})).unwrap();
        let public: MapDecl = serde_json::from_value(json!({"name":"public_contract","kind":"function","line":2,"end_line":2})).unwrap();
        assert!(proves(root, &module, &private, std::slice::from_ref(&output)));
        assert!(!proves(root, &module, &public, std::slice::from_ref(&output)));
        let after = serde_json::from_value(
            json!({"modules":[{"path":"math.rs","language":"rust","declarations":[{"name":"unused_sum","kind":"function","line":1,"end_line":1}]}]}),
        )
        .unwrap();
        let findings = super::super::removed_check::final_findings(
            root,
            &super::super::commit::AfterWave { base: Default::default(), after, changed: vec![(1, vec!["math.rs".into()])] },
            mustard_core::platform::i18n::Locale::PtBr,
            &[output],
        );
        assert!(findings.iter().any(|finding| finding.refuses && finding.text.contains("unused_sum")), "compiler proof must refuse in the final phase");
    }
}
