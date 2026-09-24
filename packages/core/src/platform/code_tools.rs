//! O programa e o plugin de cada linguagem — a mesma ideia que a instalação já
//! aplica ao ripgrep, feita uma vez para cada linguagem que
//! `apps/scan/languages.toml` conhece. `mustard init` e `mustard-rt run
//! doctor` leem esta MESMA tabela: quando uma linguagem nova ganha um plugin
//! no catálogo, ela entra aqui, e só aqui — nem em `apps/cli`, nem no
//! diagnóstico.
//!
//! A etapa que instala o que a tabela pede também é uma só, em
//! [`ensure_code_tools`]: a instalação (`mustard init`) e a atualização do
//! projeto chamam a mesma função. Ela recebe quem roda os comandos
//! ([`ToolRunner`]) — a máquina, em [`MachineRunner`], ou um executor falso no
//! teste, que assim não instala nada de verdade — e devolve o que falhou como
//! aviso com o comando pronto, sem imprimir nada: quem chama decide onde
//! mostrar.
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
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

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

    // Dart: pubspec.yaml. Sem entrada no catálogo (o `code_tool_for_language`
    // continua sem ele), mas precisa entrar na lista para o aviso de
    // `ensure_code_tools` sair.
    if project_dir.join("pubspec.yaml").is_file() {
        stacks.push("dart");
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

/// O catálogo oficial de onde saem os plugins da tabela.
pub const PLUGIN_CATALOG: &str = "claude-plugins-official";

/// Quem roda os comandos da etapa das ferramentas de código. A máquina
/// responde em [`MachineRunner`]; o teste responde com um executor falso, que
/// anota o que lhe pediram sem instalar nada.
pub trait ToolRunner {
    /// `true` quando `program` está no `PATH` que este executor usa.
    fn on_path(&self, program: &str) -> bool;
    /// Roda `program` com `args`; `true` só quando o programa abriu e saiu com
    /// sucesso. Programa que nem abre vira `false`, nunca pânico.
    fn run(&self, program: &str, args: &[&str]) -> bool;
    /// Onde `program` foi parar fora do `PATH`, numa pasta de ferramentas do
    /// usuário — `rustup component add` põe o `rust-analyzer` em
    /// `~/.cargo/bin`, por exemplo. Serve só para o aviso dizer o que fazer.
    fn found_off_path(&self, program: &str) -> Option<PathBuf>;
}

/// O que a etapa não conseguiu fazer sozinha, com o comando pronto para a
/// pessoa rodar. A etapa segue depois de cada um: nenhum aviso para a
/// instalação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeToolWarning {
    /// O catálogo ainda não tem plugin para a linguagem — Dart, hoje.
    NoPlugin { language: String },
    /// O programa existe, mas numa pasta fora do `PATH`.
    OffPath { language: String, program: &'static str, found_at: PathBuf },
    /// O programa não está no `PATH`, e a instalação não o trouxe.
    ProgramMissing { language: String, program: &'static str, install_cmd: &'static str },
    /// `claude plugin install` não deu certo.
    PluginNotInstalled { language: String, plugin: String },
    /// `claude plugin enable` não deu certo.
    PluginNotEnabled { language: String, plugin: String },
}

impl fmt::Display for CodeToolWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPlugin { language } => write!(
                f,
                "{language}: no code-tool plugin in the catalog yet - install a language server manually if you want one"
            ),
            Self::OffPath { language, program, found_at } => write!(
                f,
                "{language}: {program} found at {} but not on PATH - add its folder to PATH",
                found_at.display()
            ),
            Self::ProgramMissing { language, program, install_cmd } => {
                write!(f, "{language}: {program} not found on PATH - install manually: {install_cmd}")
            }
            Self::PluginNotInstalled { language, plugin } => write!(
                f,
                "{language}: could not install the {plugin} plugin - run manually: claude plugin install {plugin}"
            ),
            Self::PluginNotEnabled { language, plugin } => write!(
                f,
                "{language}: could not enable the {plugin} plugin - run manually: claude plugin enable {plugin}"
            ),
        }
    }
}

/// A etapa das ferramentas de código: para cada linguagem que
/// [`detect_code_languages`] acha em `project_root`, confere se o programa da
/// tabela já está no `PATH`; se não está e o gerenciador de pacotes com que o
/// comando de instalação começa está, roda esse comando. Depois instala e liga
/// o plugin do catálogo — `claude plugin install`/`enable` não falham por já
/// estar instalado. Linguagem sem entrada na tabela ganha só o aviso de que
/// não há plugin.
///
/// O que falha vira [`CodeToolWarning`], com o comando pronto, e a etapa passa
/// à linguagem seguinte: nada aqui aborta a instalação nem imprime.
pub fn ensure_code_tools(
    project_root: &Path,
    model_path: &Path,
    runner: &impl ToolRunner,
) -> Vec<CodeToolWarning> {
    let mut warnings = Vec::new();
    for language in detect_code_languages(project_root, model_path) {
        let Some(tool) = code_tool_for_language(&language) else {
            warnings.push(CodeToolWarning::NoPlugin { language });
            continue;
        };

        if !runner.on_path(tool.program) {
            let mut words = tool.install_cmd.split_whitespace();
            if let Some(manager) = words.next()
                && runner.on_path(manager)
            {
                let args: Vec<&str> = words.collect();
                runner.run(manager, &args);
            }
        }

        if !runner.on_path(tool.program) {
            warnings.push(match runner.found_off_path(tool.program) {
                Some(found_at) => CodeToolWarning::OffPath {
                    language: language.clone(),
                    program: tool.program,
                    found_at,
                },
                None => CodeToolWarning::ProgramMissing {
                    language: language.clone(),
                    program: tool.program,
                    install_cmd: tool.install_cmd,
                },
            });
        }

        if let Some(plugin) = tool.plugin {
            let plugin = format!("{plugin}@{PLUGIN_CATALOG}");
            if !runner.run("claude", &["plugin", "install", &plugin]) {
                warnings.push(CodeToolWarning::PluginNotInstalled {
                    language: language.clone(),
                    plugin: plugin.clone(),
                });
            }
            if !runner.run("claude", &["plugin", "enable", &plugin]) {
                warnings.push(CodeToolWarning::PluginNotEnabled { language, plugin });
            }
        }
    }
    warnings
}

/// O executor da máquina: procura e roda os programas no `PATH` que recebe —
/// o do processo, na instalação de verdade; uma pasta de programas falsos, no
/// teste do binário. As pastas de ferramenta do usuário saem do `HOME` real.
pub struct MachineRunner {
    path_env: String,
    home: Option<PathBuf>,
}

impl MachineRunner {
    /// O executor sobre `path_env`, uma lista no formato do `PATH` do sistema.
    #[must_use]
    pub fn new(path_env: &str) -> Self {
        Self {
            path_env: path_env.to_string(),
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }
}

impl ToolRunner for MachineRunner {
    fn on_path(&self, program: &str) -> bool {
        if program.is_empty() {
            return false;
        }
        let sep = if cfg!(windows) { ';' } else { ':' };
        let names: Vec<String> = if cfg!(windows) {
            ["exe", "cmd", "bat"].iter().map(|ext| format!("{program}.{ext}")).collect()
        } else {
            vec![program.to_string()]
        };
        self.path_env
            .split(sep)
            .any(|dir| names.iter().any(|n| Path::new(dir).join(n).is_file()))
    }

    fn run(&self, program: &str, args: &[&str]) -> bool {
        Command::new(program)
            .args(args)
            .env("PATH", &self.path_env)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn found_off_path(&self, program: &str) -> Option<PathBuf> {
        let home = self.home.as_ref()?;
        [".cargo/bin", ".local/bin", ".dotnet/tools", "go/bin"]
            .iter()
            .map(|d| home.join(d).join(program))
            .find(|p| p.is_file())
    }
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

    /// Um projeto Dart sem mapa ainda (o caso comum: `mustard init` roda antes
    /// de qualquer scan) é achado pelo `pubspec.yaml`, como os outros
    /// manifestos já são — sem isso, "dart" nunca entra na lista, e o aviso de
    /// `ensure_code_tools` de que o catálogo ainda não cobre a linguagem nunca
    /// sai.
    #[test]
    fn detect_project_languages_finds_dart_by_its_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app\n").unwrap();
        assert!(detect_project_languages(dir.path()).contains(&"dart"), "{:?}", detect_project_languages(dir.path()));
    }

    /// Executor falso: anota cada comando pedido, sem rodar nada, e responde
    /// de memória. O comando de um gerenciador presente põe no `PATH` o
    /// programa que ele traz, como a máquina faria; um comando cuja linha
    /// contém um dos trechos de `failing` sai com erro.
    struct FakeRunner {
        on_path: std::cell::RefCell<BTreeSet<String>>,
        brings: Vec<(&'static str, &'static str)>,
        failing: Vec<&'static str>,
        off_path: Vec<(&'static str, &'static str)>,
        log: std::cell::RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn new(on_path: &[&str]) -> Self {
            Self {
                on_path: std::cell::RefCell::new(on_path.iter().map(|p| (*p).to_string()).collect()),
                brings: Vec::new(),
                failing: Vec::new(),
                off_path: Vec::new(),
                log: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl ToolRunner for FakeRunner {
        fn on_path(&self, program: &str) -> bool {
            self.on_path.borrow().contains(program)
        }

        fn run(&self, program: &str, args: &[&str]) -> bool {
            let line = std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
            self.log.borrow_mut().push(line.clone());
            if !self.on_path(program) || self.failing.iter().any(|f| line.contains(f)) {
                return false;
            }
            for (manager, brought) in &self.brings {
                if *manager == program {
                    self.on_path.borrow_mut().insert((*brought).to_string());
                }
            }
            true
        }

        fn found_off_path(&self, program: &str) -> Option<PathBuf> {
            self.off_path.iter().find(|(p, _)| *p == program).map(|(_, at)| PathBuf::from(at))
        }
    }

    /// Um projeto em C#, Rust e TypeScript, sem mapa ainda. Cada linguagem cai
    /// num lado da divisa: o `csharp-ls` falta e o `dotnet` também, então nada
    /// se instala e sai o aviso com o comando; o `rust-analyzer` falta e o
    /// `rustup` está, então o comando roda; o `typescript-language-server` já
    /// está, então o `npm` nem é chamado. O plugin de cada uma é instalado e
    /// ligado — o de C# falha na instalação, vira aviso com o comando pronto,
    /// e a etapa segue para Rust e TypeScript.
    #[test]
    fn a_etapa_das_ferramentas_instala_o_plugin_de_cada_linguagem() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::write(
            project.path().join("package.json"),
            r#"{"devDependencies":{"typescript":"5.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(project.path().join("App.csproj"), "<Project/>\n").unwrap();
        let model_path = project.path().join(".claude").join("grain.model.json");

        let mut runner = FakeRunner::new(&["rustup", "npm", "claude", "typescript-language-server"]);
        runner.brings.push(("rustup", "rust-analyzer"));
        runner.failing.push("claude plugin install csharp-lsp@claude-plugins-official");

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            *runner.log.borrow(),
            vec![
                "claude plugin install csharp-lsp@claude-plugins-official",
                "claude plugin enable csharp-lsp@claude-plugins-official",
                "rustup component add rust-analyzer",
                "claude plugin install rust-analyzer-lsp@claude-plugins-official",
                "claude plugin enable rust-analyzer-lsp@claude-plugins-official",
                "claude plugin install typescript-lsp@claude-plugins-official",
                "claude plugin enable typescript-lsp@claude-plugins-official",
            ]
        );
        assert_eq!(
            warnings,
            vec![
                CodeToolWarning::ProgramMissing {
                    language: "csharp".to_string(),
                    program: "csharp-ls",
                    install_cmd: "dotnet tool install --global csharp-ls",
                },
                CodeToolWarning::PluginNotInstalled {
                    language: "csharp".to_string(),
                    plugin: "csharp-lsp@claude-plugins-official".to_string(),
                },
            ]
        );
        let texts: Vec<String> = warnings.iter().map(ToString::to_string).collect();
        assert!(texts[0].ends_with("install manually: dotnet tool install --global csharp-ls"), "{texts:?}");
        assert!(
            texts[1].ends_with("run manually: claude plugin install csharp-lsp@claude-plugins-official"),
            "{texts:?}"
        );
    }

    /// Linguagem que o catálogo não cobre (Dart, pelo `pubspec.yaml`) ganha só
    /// o aviso, sem comando nenhum; programa instalado fora do `PATH` ganha o
    /// aviso de onde ele está, e o plugin segue sendo instalado.
    #[test]
    fn a_etapa_avisa_sem_plugin_e_fora_do_path_e_segue() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("pubspec.yaml"), "name: app\n").unwrap();
        std::fs::write(project.path().join("go.mod"), "module x\n").unwrap();
        let model_path = project.path().join(".claude").join("grain.model.json");

        let mut runner = FakeRunner::new(&["claude"]);
        runner.off_path.push(("gopls", "/home/u/go/bin/gopls"));

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            *runner.log.borrow(),
            vec![
                "claude plugin install gopls-lsp@claude-plugins-official",
                "claude plugin enable gopls-lsp@claude-plugins-official",
            ]
        );
        assert_eq!(
            warnings,
            vec![
                CodeToolWarning::NoPlugin { language: "dart".to_string() },
                CodeToolWarning::OffPath {
                    language: "go".to_string(),
                    program: "gopls",
                    found_at: PathBuf::from("/home/u/go/bin/gopls"),
                },
            ]
        );
    }

    #[test]
    fn o_executor_da_maquina_procura_no_path_que_recebe() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) { "toolx.cmd" } else { "toolx" };
        std::fs::write(dir.path().join(name), "").unwrap();
        let runner = MachineRunner::new(&dir.path().display().to_string());
        assert!(runner.on_path("toolx"));
        assert!(!runner.on_path("tooly"));
        assert!(!runner.on_path(""));
    }
}
