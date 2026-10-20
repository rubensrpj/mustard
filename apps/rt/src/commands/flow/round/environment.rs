//! Detect stale generated declarations before blaming an implementation wave.
//! Preparation only runs an existing generator script declared by the project.
use super::answer::RoundRefusal;
use super::report::WaveReport;
use super::stops::{HeldReturn, hold_refused};
use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
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
    let runner = mustard_core::domain::command_detect::detect_js_package_manager(root)
        .filter(|runner| matches!(runner.as_str(), "npm" | "pnpm" | "yarn" | "bun"));
    let scripts: Vec<_> = package["scripts"].as_object().into_iter().flatten().filter_map(|(name, body)| {
        body.as_str().filter(|body| crate::hooks::bash::lex::segments(body).iter().any(|segment| {
            let prisma=|name:&str| matches!(name.rsplit(['/', '\\']).next().unwrap_or(name),"prisma"|"prisma.cmd"|"prisma.exe");
            let args:Vec<_>=segment.args.iter().map(|arg|arg.text.as_str()).collect();
            let direct=|args:&[&str]| args.first().is_some_and(|name|prisma(name)) && args.get(1)==Some(&"generate");
            prisma(segment.name()) && args.first()==Some(&"generate")
                || match segment.name() {
                    "npx"|"bunx" => direct(&args),
                    "pnpm"|"yarn"|"bun" => direct(&args) || args.first()==Some(&"exec") && direct(&args[1..]),
                    "npm" => args.first()==Some(&"exec") && direct(&args[1..]),
                    _ => false,
                }
        })).map(|_|name)
    }).collect();
    // Script names belong to the project. Multiple possible generators need
    // an explicit preparation decision, rather than an arbitrary alias win.
    let command = match (runner, scripts.as_slice()) {
        (Some(runner), [script]) => Some(format!("{runner} run {}",crate::shared::search_gateway::quote(script))),
        _ => None,
    };
    Some(Stale {
        models: changed,
        command,
    })
}

/// O preparo do cliente gerado ([`prepare`]) com a recusa atribuída às ondas
/// de `waves` que mudaram um schema do Prisma, fora as de `committed`, cujo
/// código já está num commit: cada uma sai de `waves` com a recusa dela
/// ([`HeldReturn`]), e só ela fica fora do commit. Sem nenhuma, o cliente já
/// estava atrás antes da rodada, e a recusa não é de onda nenhuma.
pub(super) fn prepare_or_hold(
    root: &Path,
    spec: &str,
    waves: &mut Vec<WaveReport>,
    committed: &BTreeSet<u64>,
    lang: Locale,
) -> Result<Vec<HeldReturn>, RoundRefusal> {
    let detail = match prepare(root, spec, lang) {
        Err(RoundRefusal::PreparationRequired(detail)) => detail,
        other => return other.map(|()| Vec::new()),
    };
    let schema = |wave: &WaveReport| !committed.contains(&wave.wave) && wave.files.iter().any(|file| file.ends_with(".prisma"));
    if !waves.iter().any(schema) {
        return Err(RoundRefusal::PreparationRequired(detail));
    }
    Ok(hold_refused(waves, |wave| if schema(wave) { Err(RoundRefusal::PreparationRequired(detail.clone())) } else { Ok(()) }))
}

fn prepare(root: &Path, spec: &str, lang: Locale) -> Result<(), RoundRefusal> {
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
            Locale::PtBr=>"O cliente gerado do Prisma difere do schema atual. O preparo não resolveu ou não foi identificado um único script de geração com gerenciador declarado. Corrija o preparo no repositório principal e repita a rodada. As entregas foram preservadas; não devolva as ondas aos agentes por esta falha.",
            Locale::EnUs=>"The generated Prisma client differs from the current schema. Preparation did not resolve it or no unique generator script with a declared package manager was identified. Fix preparation in the main repository and repeat the round. Deliveries are preserved; do not send the waves back to their agents for this failure."
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
        assert_eq!(diagnostic.command.as_deref(), Some("pnpm run 'prisma:generate'"));
        std::fs::write(root.join("package.json"), r#"{"packageManager":"bun@1.3.0","scripts":{"refresh generated types":"custom-runner prisma generate","notes":"echo prisma generate"}}"#).unwrap();
        assert!(stale(root).unwrap().command.is_none(),"unknown wrappers must not become invented preparation commands");
        std::fs::write(root.join("package.json"), r#"{"packageManager":"bun@1.3.0","scripts":{"refresh generated types":"prisma generate","notes":"echo prisma generate"}}"#).unwrap();
        assert_eq!(stale(root).unwrap().command.as_deref(),Some("bun run 'refresh generated types'"));
        std::fs::write(root.join("package.json"), r#"{"packageManager":"bun@1.3.0","scripts":{"first":"prisma generate","second":"bunx prisma generate"}}"#).unwrap();
        assert!(stale(root).unwrap().command.is_none(),"multiple generators need explicit preparation");
        std::fs::write(root.join("package.json"), r#"{"scripts":{"generate":"prisma generate"}}"#).unwrap();
        assert!(stale(root).unwrap().command.is_none(),"no package manager may be guessed");
        std::fs::write(root.join("bun.lock"),"{}").unwrap();
        assert_eq!(stale(root).unwrap().command.as_deref(),Some("bun run 'generate'"));
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
