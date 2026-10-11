//! Detect stale generated declarations before blaming an implementation wave.
//! Preparation only runs an existing generator script declared by the project.
use super::answer::RoundRefusal;
use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Models = BTreeMap<String, BTreeMap<String, String>>;

fn models(text: &str) -> Option<Models> {
    let mut models = Models::new();
    let mut current = None;
    for line in text.lines().map(|line| line.split("//").next().unwrap_or_default().trim()) {
        if line.is_empty() {
            continue;
        }
        let words: Vec<_> = line.split_whitespace().collect();
        if current.is_none() {
            if words.len() == 3 && matches!(words[0], "model" | "type" | "enum") && words[2] == "{" {
                let name = format!("{} {}", words[0], words[1]);
                models.insert(name.clone(), BTreeMap::new());
                current = Some(name);
            }
        } else if line == "}" {
            current = None;
        } else if let Some(name) = &current {
            let identifier = |word: &str| !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
            if identifier(words[0]) {
                let kind = if name.starts_with("enum ") { "" } else { *words.get(1)? };
                models.get_mut(name)?.insert(words[0].into(), kind.into());
            }
        }
    }
    (current.is_none() && !models.is_empty()).then_some(models)
}

fn schema_files(path: &Path) -> Option<Vec<PathBuf>> {
    if path.is_file() {
        return (path.extension()? == "prisma").then(|| vec![path.to_path_buf()]);
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        if entry.file_type().ok()?.is_dir() {
            files.extend(schema_files(&entry.path())?);
        } else if entry.path().extension().is_some_and(|ext| ext == "prisma") {
            files.push(entry.path());
        }
    }
    Some(files)
}

struct Stale {
    models: Vec<String>,
    command: Option<String>,
}

fn stale(root: &Path) -> Option<Stale> {
    let package: Value = serde_json::from_str(&std::fs::read_to_string(root.join("package.json")).ok()?).ok()?;
    let client = root.join("node_modules/@prisma/client").canonicalize().ok()?;
    let generated = client.ancestors().nth(2)?.join(".prisma/client/schema.prisma");
    let generated = models(&std::fs::read_to_string(generated).ok()?)?;
    let schema = package
        .pointer("/prisma/schema")
        .and_then(Value::as_str)
        .map(|p| root.join(p))
        .unwrap_or_else(|| {
            if root.join("prisma/schema.prisma").is_file() {
                root.join("prisma/schema.prisma")
            } else {
                root.join("prisma/schema")
            }
        });
    let mut current = Models::new();
    for file in schema_files(&schema)? {
        let text = std::fs::read_to_string(file).ok()?;
        if let Some(part) = models(&text) {
            current.extend(part);
        }
    }
    if current.is_empty() {
        return None;
    }
    let changed: Vec<_> = current
        .keys()
        .chain(generated.keys())
        .filter(|name| current.get(*name) != generated.get(*name))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let runner = package["packageManager"]
        .as_str()
        .and_then(|s| s.split('@').next())
        .filter(|s| matches!(*s, "npm" | "pnpm" | "yarn" | "bun"))
        .unwrap_or_else(|| {
            if root.join("pnpm-lock.yaml").is_file() {
                "pnpm"
            } else if root.join("yarn.lock").is_file() {
                "yarn"
            } else {
                "npm"
            }
        });
    let script = ["prisma:generate", "generate:prisma", "db:generate"]
        .into_iter()
        .find(|name| package["scripts"][*name].as_str().is_some_and(|s| s.contains("prisma generate")));
    Some(Stale {
        models: changed,
        command: script.map(|script| format!("{runner} run {script}")),
    })
}

pub(super) fn prepare(root: &Path, spec: &str, lang: Locale) -> Result<(), RoundRefusal> {
    let Some(stale_client) = stale(root).filter(|client| !client.models.is_empty()) else {
        return Ok(());
    };
    let mut attempt = Value::Null;
    if let Some(command) = &stale_client.command {
        let result = crate::commands::review::qa_run::run_build_command(command, root);
        super::super::validation::record_run(root, spec, "delivery", "prepare-generated-client", command, &result).map_err(RoundRefusal::Refused)?;
        if result.result == "pass" && stale(root).is_some_and(|client| client.models.is_empty()) {
            return Ok(());
        }
        attempt = json!({"result":result.result,"exit":result.exit,"output":result.output});
    }
    Err(RoundRefusal::PreparationRequired(
        json!({"ok":false,"reason":"round-generated-client-stale","generator":"prisma",
        "models":stale_client.models,"prepare_command":stale_client.command,"preparation":attempt,"agent_rework_required":false,
        "hint":match lang {
            Locale::PtBr=>"O cliente gerado do Prisma difere do schema atual. O preparo não resolveu ou não há script de geração declarado. Corrija o preparo no repositório principal e repita a rodada. As entregas foram preservadas; não devolva as ondas aos agentes por esta falha.",
            Locale::EnUs=>"The generated Prisma client differs from the current schema. Preparation did not resolve it or no generator script is declared. Fix preparation in the main repository and repeat the round. Deliveries are preserved; do not send the waves back to their agents for this failure."
        }}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_relations_are_detected_without_treating_layout_comments_or_indexes_as_client_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("node_modules/@prisma/client")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/.prisma/client")).unwrap();
        std::fs::create_dir_all(root.join("prisma/schema")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"packageManager":"pnpm@9.0.0","scripts":{"prisma:generate":"prisma generate --schema ./prisma/schema"}}"#,
        )
        .unwrap();
        let old = "model Plan {\n id String @id\n}\n";
        std::fs::write(root.join("node_modules/.prisma/client/schema.prisma"), old).unwrap();
        std::fs::write(
            root.join("prisma/schema/plan.prisma"),
            "// comment\nmodel Plan {\n  id   String @id\n @@index([id])\n}\n",
        )
        .unwrap();
        assert!(stale(root).unwrap().models.is_empty());
        let new = "model Plan {\n id String @id\n menus Menu[]\n}\n";
        std::fs::write(root.join("prisma/schema/plan.prisma"), new).unwrap();
        let diagnostic = stale(root).unwrap();
        assert_eq!(diagnostic.models, vec!["model Plan"]);
        assert_eq!(diagnostic.command.as_deref(), Some("pnpm run prisma:generate"));
        std::fs::write(root.join("node_modules/.prisma/client/schema.prisma"), new).unwrap();
        assert!(stale(root).unwrap().models.is_empty());
    }
    #[test]
    fn unrelated_projects_are_not_given_a_prisma_preparation() {
        let dir = tempfile::tempdir().unwrap();
        assert!(prepare(dir.path(), "demo", Locale::PtBr).is_ok());
    }
    #[test]
    fn a_stale_client_keeps_the_delivery_without_sending_unrelated_build_repairs_to_the_agent() {
        use crate::commands::flow::round::tests::*;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), r#"{"buildCommand":"echo build"}"#).unwrap();
        assert_eq!(round(root, "x", None)["ok"], true);
        std::fs::create_dir_all(root.join("node_modules/@prisma/client")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/.prisma/client")).unwrap();
        std::fs::create_dir_all(root.join("prisma")).unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();
        std::fs::write(root.join("node_modules/.prisma/client/schema.prisma"), "model Plan {\n id String\n}\n").unwrap();
        std::fs::write(root.join("prisma/schema.prisma"), "model Plan {\n id String\n menus Menu[]\n}\n").unwrap();
        let copy = mustard_core::io::wave_prompt::slot_path(root, "x", 0);
        std::fs::write(copy.join("src/a.rs"), "fn one() {}\n// Delivered.\n").unwrap();
        assert_eq!(
            returned(root, json!({"wave":1,"text":"Done.","files":["src/a.rs"],"commit":"the task is done"}))["ok"],
            true
        );
        let result = round(root, "x", None);
        assert_eq!(result["reason"], "round-generated-client-stale", "{result}");
        assert_eq!(result["agent_rework_required"], false);
        assert!(!result["hint"].as_str().unwrap().contains("Mande cada onda"));
        assert!(std::fs::read_to_string(copy.join("src/a.rs")).unwrap().contains("Delivered"));
        assert_eq!(delivered_count(root), 0);
        let log = mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root, "x").unwrap())
            .unwrap()
            .unwrap();
        assert!(!super::super::fix_file(root, "x", &log, 1).is_some_and(|file| file.exists()));
    }
}
