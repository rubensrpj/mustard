//! O programa e o plugin de cada linguagem — a mesma ideia que a instalação já
//! aplica ao ripgrep, feita uma vez para cada linguagem que
//! `apps/scan/languages.toml` conhece. `mustard init` e `mustard-rt run
//! doctor` leem esta MESMA tabela: quando uma linguagem nova ganha um plugin
//! no catálogo, ela entra aqui, e só aqui — nem em `apps/cli`, nem no
//! diagnóstico.
//!
//! A detecção de linguagem também é uma só, em [`detect_code_languages`]:
//! quando o projeto já foi mapeado (`.claude/grain.model.json` existe), o
//! registro de pilhas atribui a cada subprojeto a linguagem do framework que
//! ele detectou ([`crate::domain::source_lang::detected_languages`]); sem
//! mapa — o caso comum, porque `mustard init` roda antes de qualquer scan —
//! uma sondagem best-effort dos arquivos de manifesto entra no lugar
//! ([`detect_project_languages`], a antiga `doctor::host::detect_stacks`,
//! movida para cá sem mudar a lógica).

use std::collections::BTreeSet;
use std::path::Path;

use crate::domain::scan::read_projects;
use crate::domain::source_lang::detected_languages;
use crate::io::fs;

/// O programa que um plugin de linguagem chama, e o comando que instala esse
/// programa. `plugin` é `None` para uma linguagem que o catálogo oficial
/// ainda não cobre — hoje, Dart.
pub struct CodeTool {
    pub plugin: Option<&'static str>,
    pub program: &'static str,
    pub install_cmd: &'static str,
}

/// `(linguagem, ferramenta)` — DADO, não lógica. As chaves são os nomes que
/// `apps/scan/languages.toml` e o registro de pilhas usam.
pub const CODE_TOOLS: &[(&str, CodeTool)] = &[
    (
        "rust",
        CodeTool {
            plugin: Some("rust-analyzer-lsp"),
            program: "rust-analyzer",
            install_cmd: "rustup component add rust-analyzer",
        },
    ),
    (
        "typescript",
        CodeTool {
            plugin: Some("typescript-lsp"),
            program: "typescript-language-server",
            install_cmd: "npm install -g typescript-language-server typescript",
        },
    ),
    (
        "javascript",
        CodeTool {
            plugin: Some("typescript-lsp"),
            program: "typescript-language-server",
            install_cmd: "npm install -g typescript-language-server typescript",
        },
    ),
    (
        "csharp",
        CodeTool {
            plugin: Some("csharp-lsp"),
            program: "csharp-ls",
            install_cmd: "dotnet tool install --global csharp-ls",
        },
    ),
    (
        "go",
        CodeTool {
            plugin: Some("gopls-lsp"),
            program: "gopls",
            install_cmd: "go install golang.org/x/tools/gopls@latest",
        },
    ),
    (
        "python",
        CodeTool {
            plugin: Some("pyright-lsp"),
            program: "pyright-langserver",
            install_cmd: "npm install -g pyright",
        },
    ),
    (
        "php",
        CodeTool {
            plugin: Some("php-lsp"),
            program: "intelephense",
            install_cmd: "npm install -g intelephense",
        },
    ),
];

/// A ferramenta de código de `language` (minúsculo), quando o catálogo a
/// cobre. `None` para uma linguagem sem entrada — Dart, ou qualquer outra que
/// a tabela ainda não liste.
#[must_use]
pub fn code_tool_for_language(language: &str) -> Option<&'static CodeTool> {
    CODE_TOOLS
        .iter()
        .find(|(name, _)| *name == language)
        .map(|(_, tool)| tool)
}

/// Sondagem best-effort das pilhas ativas em `project_dir`, pelos arquivos de
/// manifesto conhecidos — a `detect_code_languages` recorre a ela quando
/// ainda não há mapa do repositório. Falha aberta: erro de E/S vira lista
/// vazia.
#[must_use]
pub fn detect_project_languages(project_dir: &Path) -> Vec<&'static str> {
    let mut stacks: Vec<&'static str> = Vec::new();

    // Rust: Cargo.toml com [package]
    let cargo = project_dir.join("Cargo.toml");
    if cargo.is_file()
        && fs::read_to_string(&cargo)
            .unwrap_or_default()
            .contains("[package]")
    {
        stacks.push("rust");
    }

    // Go: go.mod
    if project_dir.join("go.mod").is_file() {
        stacks.push("go");
    }

    // Python: pyproject.toml ou requirements.txt
    if project_dir.join("pyproject.toml").is_file()
        || project_dir.join("requirements.txt").is_file()
    {
        stacks.push("python");
    }

    // TypeScript/JavaScript: package.json
    let pkg_path = project_dir.join("package.json");
    if pkg_path.is_file() {
        let content = fs::read_to_string(&pkg_path).unwrap_or_default();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            let deps_have_ts = ["dependencies", "devDependencies"].iter().any(|section| {
                json.get(*section)
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|obj| obj.contains_key("typescript"))
            });
            if deps_have_ts {
                stacks.push("typescript");
            } else {
                stacks.push("javascript");
            }
        } else {
            stacks.push("javascript");
        }
    }

    // C#: qualquer *.csproj presente
    if let Ok(entries) = fs::read_dir(project_dir) {
        let has_csproj = entries.iter().any(|e| e.file_name.ends_with(".csproj"));
        if has_csproj {
            stacks.push("csharp");
        }
    }

    // Java: pom.xml ou build.gradle
    if project_dir.join("pom.xml").is_file() || project_dir.join("build.gradle").is_file() {
        stacks.push("java");
    }

    stacks
}

/// As linguagens que `project_root` envolve, para a instalação e o
/// diagnóstico percorrerem. Lê o mapa do scan em `model_path` quando o
/// projeto já foi mapeado — [`detected_languages`] atribui a cada subprojeto
/// a linguagem que o registro de pilhas inferiu do framework, pegando por
/// exemplo um app PHP Laravel ou um app Dart Flutter que a sondagem de
/// manifesto sozinha não pegaria. Sem mapa ainda, ou quando o mapa não rende
/// nada, cai na sondagem de manifesto de [`detect_project_languages`]. Falha
/// aberta dos dois jeitos.
#[must_use]
pub fn detect_code_languages(project_root: &Path, model_path: &Path) -> BTreeSet<String> {
    if model_path.is_file() {
        let projects = read_projects(model_path);
        // `detected_languages` atribui cada caminho ao subprojeto cujo `dir` é
        // o prefixo mais específico; um caminho fictício dentro de cada
        // subprojeto basta para acionar essa atribuição sem precisar de uma
        // varredura de arquivos.
        let paths: Vec<String> = projects
            .iter()
            .filter(|p| !p.dir.is_empty())
            .map(|p| format!("{}/_", p.dir.trim_end_matches('/')))
            .collect();
        if !paths.is_empty() {
            let langs = detected_languages(&paths, &projects, project_root);
            if !langs.is_empty() {
                return langs;
            }
        }
    }
    detect_project_languages(project_root)
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_tool_for_language_covers_the_catalog() {
        for lang in ["rust", "typescript", "javascript", "csharp", "go", "python", "php"] {
            assert!(code_tool_for_language(lang).is_some(), "missing entry for {lang}");
        }
    }

    #[test]
    fn code_tool_for_language_dart_has_no_catalog_entry() {
        assert!(code_tool_for_language("dart").is_none());
    }

    #[test]
    fn detect_code_languages_falls_back_to_manifest_probe_without_a_model() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let model_path = dir.path().join(".claude").join("grain.model.json");
        let langs = detect_code_languages(dir.path(), &model_path);
        assert!(langs.contains("rust"), "{langs:?}");
    }
}
