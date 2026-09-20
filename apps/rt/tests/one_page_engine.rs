//! Um motor de página só.
//!
//! A página avulsa escrita em markdown, a única que o binário ainda escreve
//! em HTML, sai deste motor: o mesmo estilo, as fontes do Google Fonts por
//! um link, e nenhuma fonte gravada dentro da página. A página da spec e a
//! do projeto saem hoje de um template mais banco de dados, fora deste
//! motor: o comando que as refazia saiu com esta obra.
//!
//! Fora de `apps/rt/src/report/`, nenhum arquivo de código do Mustard escreve
//! o começo de uma página HTML (`<!doctype html>`) ou uma folha de estilo
//! (`<style>`), nem converte markdown em HTML. A conversão aparece pelas
//! marcas que só ela produz: `<code>`, `<strong>` e `<em>`, que nascem das
//! crases e dos asteriscos. O código de teste fica de fora: ele confere
//! páginas, não as escreve.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// O que só o motor de página escreve.
const ENGINE_ONLY: &[&str] = &["<!doctype", "<style", "<code>", "<strong>", "<em>"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Os arquivos `.rs` de código sob `dir`, em ordem; pastas de teste, de
/// compilação e de dependências ficam de fora.
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !matches!(name.to_str(), Some("target" | "node_modules" | ".git" | "tests" | "benches")) {
                rust_sources(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// O código de produção de um arquivo: cada item marcado com `#[cfg(test)]`
/// sai, do atributo até o `;` ou até o fim do bloco de chaves dele.
fn production_code(source: &str) -> String {
    let mut out = String::new();
    let mut rest = source;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let item = &rest[at + "#[cfg(test)]".len()..];
        let semicolon = item.find(';');
        let brace = item.find('{');
        rest = match (brace, semicolon) {
            (Some(open), Some(end)) if end < open => &item[end + 1..],
            (Some(open), _) => {
                let mut depth = 0usize;
                let mut close = item.len();
                for (i, c) in item[open..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                close = open + i + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                &item[close..]
            }
            (None, Some(end)) => &item[end + 1..],
            (None, None) => "",
        };
    }
    out.push_str(rest);
    out
}

fn violations(path: &Path) -> Vec<&'static str> {
    let source = fs::read_to_string(path).unwrap_or_default();
    let code = production_code(&source).to_ascii_lowercase();
    ENGINE_ONLY.iter().copied().filter(|mark| code.contains(mark)).collect()
}

/// Roda `mustard-rt run page` com `args` no projeto `root` e devolve o
/// relatório.
fn page(root: &Path, args: &[&str]) -> Value {
    let (ok, report) = page_run(root, args);
    assert!(ok, "{report}");
    report
}

/// Como [`page`], dizendo se o comando saiu com sucesso em vez de exigir.
fn page_run(root: &Path, args: &[&str]) -> (bool, Value) {
    rt(root, "page", args)
}

/// Roda `mustard-rt run <command>` com `args` no projeto `root`: se saiu com
/// sucesso, e o relatório.
fn rt(root: &Path, command: &str, args: &[&str]) -> (bool, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", command])
        .args(args)
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .output()
        .expect("run the command");
    let report: Value = serde_json::from_slice(&out.stdout).expect("a JSON report");
    (out.status.success(), report)
}

/// A folha de estilo de uma página.
fn style(html: &str) -> &str {
    html.split_once("<style>")
        .and_then(|(_, tail)| tail.split_once("</style>"))
        .map_or("", |(css, _)| css)
}

/// A página avulsa que o binário ainda escreve em HTML.
fn the_page(root: &Path) -> String {
    fs::write(root.join("corpo.md"), "# Avulsa\n\n## Seção\n\nTexto com **negrito**.\n").expect("body");
    page(root, &["--body", "corpo.md", "--out", "avulsa.html"]);
    fs::read_to_string(root.join("avulsa.html")).unwrap_or_else(|_| panic!("avulsa.html"))
}

#[test]
fn only_the_page_engine_writes_html_pages_or_converts_markdown() {
    let project = tempfile::tempdir().expect("tempdir");
    let html = the_page(project.path());
    let fonts = "<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family=Geist";
    assert!(html.starts_with("<!doctype html>"), "{html}");
    assert!(!style(&html).is_empty(), "the page has no style");
    assert!(html.contains(fonts), "the page does not link the Geist fonts");
    assert!(!html.contains("@font-face") && !html.contains("data:font"), "the page carries a font");
    assert_eq!(html.matches("<style>").count(), 1, "the page carries a second style");
    assert!(html.contains("<strong>negrito</strong>"), "the loose page did not convert markdown");

    let root = repo_root();
    let mut files = Vec::new();
    for top in ["apps", "packages"] {
        rust_sources(&root.join(top), &mut files);
    }
    assert!(files.len() > 100, "the scan found only {} files", files.len());

    let engine = root.join("apps/rt/src/report");
    let mut found = Vec::new();
    for file in files.iter().filter(|f| !f.starts_with(&engine)) {
        for mark in violations(file) {
            found.push(format!("{} writes {mark}", file.strip_prefix(&root).unwrap_or(file).display()));
        }
    }
    assert!(found.is_empty(), "outside the page engine:\n{}", found.join("\n"));

    // A varredura enxerga o que procura: o próprio motor tem as marcas.
    let engine_marks: Vec<&str> = ["mod.rs", "markdown.rs", "render.rs"]
        .iter()
        .flat_map(|f| violations(&engine.join(f)))
        .collect();
    for mark in ["<!doctype", "<style", "<code>", "<strong>"] {
        assert!(engine_marks.contains(&mark), "the scan never sees {mark} even in the engine");
    }
}

/// Um rascunho vira arquivo de eventos item por item: cada regra, critério,
/// limite e caso de borda gravado pelo comando do seu tipo volta, na leitura
/// do bloco dele, com o tipo certo e o texto igual.
#[test]
fn each_draft_item_becomes_an_event_of_its_type_and_lands_in_its_block() {
    let project = tempfile::tempdir().expect("tempdir");
    let root = project.path();
    fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).expect("config");
    let spec = root.join(".claude").join("spec").join("demo");
    fs::create_dir_all(&spec).expect("spec dir");
    fs::write(
        spec.join("spec.ndjson"),
        concat!(
            r#"{"v":1,"id":1,"at":"2026-09-11T08:40:00-03:00","type":"state","author":"binary","phase":"survey"}"#,
            "\n",
            r#"{"v":1,"id":2,"at":"2026-09-11T08:41:00-03:00","type":"message","author":"user","text":"Converta o rascunho."}"#,
            "\n",
        ),
    )
    .expect("events");

    // (tipo, bloco da leitura, campos, os textos que ficam iguais)
    let items: [(&str, &str, Value, &[&str]); 4] = [
        ("rule", "agreed",
            serde_json::json!({"text": "A trava confere o programa, nunca o texto entre aspas.",
                "example": "rm -rf pasta é barrado.", "keys": ["trava"], "origin": 2}),
            &["text"]),
        ("criterion", "criteria",
            serde_json::json!({"when": "O pedido de uma onda passa de 500 linhas.",
                "then": "O binário recusa o despacho.", "proof": "cargo test", "origin": 2}),
            &["when", "then"]),
        ("limit", "agreed",
            serde_json::json!({"text": "Tamanho do pedido de cada onda.", "value": "500 linhas",
                "keys": ["pedido"], "origin": 2}),
            &["text"]),
        ("edge_case", "agreed",
            serde_json::json!({"text": "Duas sessões gravam a mesma spec ao mesmo tempo.",
                "expected": "A segunda espera a trava.", "keys": ["trava"], "origin": 2}),
            &["text"]),
    ];
    let mut written = Vec::new();
    for (kind, _, fields, _) in &items {
        let (ok, report) = rt(root, "write", &[*kind, "--spec", "demo", "--json", &fields.to_string()]);
        assert!(ok, "write {kind}: {report}");
        written.push(report["id"].as_u64().unwrap_or_else(|| panic!("write {kind} gives no number: {report}")));
    }

    for ((kind, block, fields, same), id) in items.iter().zip(written) {
        let (ok, read) = rt(root, "read", &[*block, "--spec", "demo"]);
        assert!(ok, "read {block}: {read}");
        let events = read["events"].as_array().cloned().unwrap_or_default();
        let event = events
            .iter()
            .find(|e| e["id"].as_u64() == Some(id))
            .unwrap_or_else(|| panic!("the {kind} written as {id} is not in the {block} block: {read}"));
        assert_eq!(event["type"], *kind, "{event}");
        for field in *same {
            assert_eq!(event[field], fields[field], "the {kind} {field} changed on the way: {event}");
        }
    }
}

#[test]
fn test_items_are_left_out_of_the_scan() {
    let source = "fn a() {}\n#[cfg(test)]\nuse x::y;\nfn b() {}\n#[cfg(test)]\nmod tests {\n    fn t() { let _ = \"<style>\"; }\n}\n";
    let code = production_code(source);
    assert!(code.contains("fn a()") && code.contains("fn b()"), "{code}");
    assert!(!code.contains("<style>") && !code.contains("use x::y"), "{code}");
}
