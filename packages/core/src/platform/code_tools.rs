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
//! mostrar. Cada comando da máquina tem um prazo ([`COMMAND_DEADLINE`]): o que
//! o passa vira o aviso de que passou do prazo, nunca uma instalação parada.
//!
//! A detecção de linguagem também é uma só, em [`detect_code_languages`]:
//! quando o projeto já foi mapeado (`.claude/grain.db` existe), o
//! registro de pilhas atribui a cada subprojeto a linguagem do framework que
//! ele detectou ([`crate::domain::source_lang::detected_languages`]); sem
//! mapa — o caso comum, porque `mustard init` roda antes de qualquer scan —
//! uma sondagem best-effort dos arquivos de manifesto entra no lugar
//! ([`detect_project_languages`], a antiga `doctor::host::detect_stacks`,
//! movida para cá sem mudar a lógica).

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::domain::scan::read_projects;
use crate::domain::source_lang::detected_languages;
use crate::io::fs;
use crate::platform::harness::home_dir;
use crate::platform::process::{program_file, program_file_names};

/// O programa que um plugin de linguagem chama, e o comando que instala esse
/// programa. `plugin` é `None` para uma linguagem que o catálogo oficial
/// ainda não cobre — hoje, Dart.
///
/// `install_cmd` pode ter mais de um passo, separados por `&&`: a etapa roda
/// um depois do outro, sem shell, e a pessoa pode colar a linha inteira no
/// terminal. `check`, quando há, é o comando — o programa e os argumentos —
/// que sai com sucesso só quando o programa responde: estar no `PATH` não
/// basta para ele. `start_hint`, quando há, é a frase que acompanha o aviso do
/// programa que foi instalado mas não roda, com o comando pronto.
pub struct CodeTool {
    pub plugin: Option<&'static str>,
    pub program: &'static str,
    pub install_cmd: &'static str,
    pub check: Option<&'static [&'static str]>,
    pub start_hint: Option<&'static str>,
}

impl CodeTool {
    /// `true` quando o programa responde, pela conferência da tabela; sem
    /// conferência, estar no `PATH` basta, e nada roda.
    pub fn answers(&self, runner: &impl ToolRunner) -> bool {
        match self.check {
            Some([program, args @ ..]) => runner.run(program, args),
            _ => true,
        }
    }
}

/// A instalação do servidor de TypeScript. O servidor vai global, e o
/// TypeScript 6 vai para dentro da pasta dele (`lib/node_modules`), onde ele
/// o acha antes do global: o TypeScript 7 não traz o `tsserver.js` de que o
/// servidor precisa, e com ele o servidor abre e responde vazio. O `tsc`
/// global da pessoa fica como está. `npm explore` roda o segundo passo já na
/// pasta do servidor, sem depender do shell para achar a pasta global.
const TYPESCRIPT_INSTALL: &str = "npm install -g typescript-language-server && npm explore -g \
    typescript-language-server -- npm install --global=false --prefix lib --no-save typescript@6";

/// A conferência do servidor de TypeScript: o TypeScript que ele acha a
/// partir do próprio `lib/cli.mjs`, como ele mesmo procura, traz o
/// `tsserver.js` ao lado.
const TYPESCRIPT_CHECK: &[&str] = &[
    "node",
    "-e",
    "const{execSync:x}=require('child_process'),p=require('path'),f=require('fs'),m=require('module');\
     const s=p.join(x('npm root -g').toString().trim(),'typescript-language-server','lib','cli.mjs');\
     const t=m.createRequire(s).resolve('typescript');\
     process.exit(f.existsSync(p.join(p.dirname(t),'tsserver.js'))?0:1)",
];

/// A instalação do servidor de C#, na versão 0.18.0: a mais nova falha no SDK
/// 9 do dotnet (a instalação acaba sem o `DotnetToolSettings.xml`), e a 0.18.0
/// instala.
const CSHARP_INSTALL: &str = "dotnet tool install --global csharp-ls --version 0.18.0";

/// O que fazer quando o `csharp-ls` instalado não abre: o dotnet do sistema é
/// mais velho que o da pasta pessoal, e o programa só roda apontando para ela.
const CSHARP_START_HINT: &str = "if csharp-ls will not start because the system dotnet is older than the one in \
     ~/.dotnet, run it with DOTNET_ROOT=~/.dotnet - set it once: export DOTNET_ROOT=\"$HOME/.dotnet\"";

/// `(linguagem, ferramenta)` — DADO, não lógica. As chaves são os nomes que
/// `apps/scan/languages.toml` e o registro de pilhas usam.
pub const CODE_TOOLS: &[(&str, CodeTool)] = &[
    (
        "rust",
        CodeTool {
            plugin: Some("rust-analyzer-lsp"),
            program: "rust-analyzer",
            install_cmd: "rustup component add rust-analyzer",
            check: None,
            start_hint: None,
        },
    ),
    (
        "typescript",
        CodeTool {
            plugin: Some("typescript-lsp"),
            program: "typescript-language-server",
            install_cmd: TYPESCRIPT_INSTALL,
            check: Some(TYPESCRIPT_CHECK),
            start_hint: None,
        },
    ),
    (
        "javascript",
        CodeTool {
            plugin: Some("typescript-lsp"),
            program: "typescript-language-server",
            install_cmd: TYPESCRIPT_INSTALL,
            check: Some(TYPESCRIPT_CHECK),
            start_hint: None,
        },
    ),
    (
        "csharp",
        CodeTool {
            plugin: Some("csharp-lsp"),
            program: "csharp-ls",
            install_cmd: CSHARP_INSTALL,
            check: None,
            start_hint: Some(CSHARP_START_HINT),
        },
    ),
    (
        "go",
        CodeTool {
            plugin: Some("gopls-lsp"),
            program: "gopls",
            install_cmd: "go install golang.org/x/tools/gopls@latest",
            check: None,
            start_hint: None,
        },
    ),
    (
        "python",
        CodeTool {
            plugin: Some("pyright-lsp"),
            program: "pyright-langserver",
            install_cmd: "npm install -g pyright",
            check: None,
            start_hint: None,
        },
    ),
    (
        "php",
        CodeTool {
            plugin: Some("php-lsp"),
            program: "intelephense",
            install_cmd: "npm install -g intelephense",
            check: None,
            start_hint: None,
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
    if crate::io::project_map::exists_at(model_path) {
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

/// O prazo de cada comando que a máquina roda na etapa: a instalação de um
/// servidor de linguagem ou de um plugin que passa dele é cortada, e o que era
/// uma espera sem fim vira um aviso. O Claude Code também corta o comando que
/// dura demais, e sem o aviso quem instala fica sem saber o que faltou.
pub const COMMAND_DEADLINE: Duration = Duration::from_secs(60);

/// Como um comando da etapa terminou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// Abriu e saiu com sucesso.
    Succeeded,
    /// Não abriu, ou saiu com erro.
    Failed,
    /// Passou do prazo `after` e foi cortado.
    TimedOut { after: Duration },
}

/// Quem roda os comandos da etapa das ferramentas de código. A máquina
/// responde em [`MachineRunner`]; o teste responde com um executor falso, que
/// anota o que lhe pediram sem instalar nada.
pub trait ToolRunner {
    /// `true` quando `program` está no `PATH` que este executor usa.
    fn on_path(&self, program: &str) -> bool;
    /// Roda `program` com `args`; `true` só quando o programa abriu e saiu com
    /// sucesso. Programa que nem abre vira `false`, nunca pânico.
    fn run(&self, program: &str, args: &[&str]) -> bool;
    /// Como [`ToolRunner::run`], dizendo também quando o comando foi cortado
    /// por passar do prazo. Um executor sem prazo não precisa escrevê-lo: o
    /// comando ou deu certo, ou falhou.
    fn run_outcome(&self, program: &str, args: &[&str]) -> RunOutcome {
        if self.run(program, args) {
            RunOutcome::Succeeded
        } else {
            RunOutcome::Failed
        }
    }
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
    /// O programa está no `PATH`, mas a conferência dele falha, mesmo depois
    /// de a etapa rodar a instalação.
    NotReady { language: String, program: &'static str, install_cmd: &'static str },
    /// `claude plugin install` não deu certo.
    PluginNotInstalled { language: String, plugin: String },
    /// `claude plugin enable` não deu certo.
    PluginNotEnabled { language: String, plugin: String },
    /// Um comando passou do prazo e foi cortado; `command` é a linha inteira
    /// que a pessoa roda para terminar o que ele fazia.
    TimedOut { language: String, command: String, seconds: u64 },
    /// O programa instalado pode não abrir sem um ajuste da pessoa; `hint` é
    /// a frase da tabela, com o comando pronto.
    StartHint { language: String, hint: &'static str },
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
            Self::NotReady { language, program, install_cmd } => {
                write!(f, "{language}: {program} is on PATH but does not answer - install manually: {install_cmd}")
            }
            Self::PluginNotInstalled { language, plugin } => write!(
                f,
                "{language}: could not install the {plugin} plugin - run manually: claude plugin install {plugin}"
            ),
            Self::PluginNotEnabled { language, plugin } => write!(
                f,
                "{language}: could not enable the {plugin} plugin - run manually: claude plugin enable {plugin}"
            ),
            Self::TimedOut { language, command, seconds } => {
                write!(f, "{language}: timed out after {seconds}s - run manually: {command}")
            }
            Self::StartHint { language, hint } => write!(f, "{language}: {hint}"),
        }
    }
}

/// A etapa das ferramentas de código: para cada linguagem que
/// [`detect_code_languages`] acha em `project_root`, confere se o programa da
/// tabela já está no `PATH` e responde ([`CodeTool::answers`]); se não, roda
/// o comando de instalação ([`install`]). O programa que segue no `PATH` sem
/// responder vira [`CodeToolWarning::NotReady`]. Depois instala e liga o
/// plugin do catálogo — `claude plugin install`/`enable` não falham por já
/// estar instalado. Linguagem sem entrada na tabela ganha só o aviso de que
/// não há plugin.
///
/// O que falha vira [`CodeToolWarning`], com o comando pronto, e a etapa passa
/// à linguagem seguinte: nada aqui aborta a instalação nem imprime. O comando
/// que passa do prazo do executor vira [`CodeToolWarning::TimedOut`] no lugar
/// do aviso de falha do passo dele.
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

        if !(runner.on_path(tool.program) && tool.answers(runner)) {
            if let Some(after) = install(tool.install_cmd, runner) {
                warnings.push(CodeToolWarning::TimedOut {
                    language: language.clone(),
                    command: tool.install_cmd.to_string(),
                    seconds: after.as_secs(),
                });
            } else if !runner.on_path(tool.program) {
                let found_off_path = runner.found_off_path(tool.program);
                let off_path = found_off_path.is_some();
                warnings.push(match found_off_path {
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
                // O programa que a instalação deixou fora do `PATH` é o que
                // pode não abrir; o que nem foi instalado não tem o que ajustar.
                push_start_hint(&mut warnings, &language, tool, off_path);
            } else if !tool.answers(runner) {
                warnings.push(CodeToolWarning::NotReady {
                    language: language.clone(),
                    program: tool.program,
                    install_cmd: tool.install_cmd,
                });
                push_start_hint(&mut warnings, &language, tool, true);
            }
        }

        if let Some(plugin) = tool.plugin {
            let plugin = format!("{plugin}@{PLUGIN_CATALOG}");
            for verb in ["install", "enable"] {
                match runner.run_outcome("claude", &["plugin", verb, &plugin]) {
                    RunOutcome::Succeeded => {}
                    RunOutcome::Failed => warnings.push(if verb == "install" {
                        CodeToolWarning::PluginNotInstalled { language: language.clone(), plugin: plugin.clone() }
                    } else {
                        CodeToolWarning::PluginNotEnabled { language: language.clone(), plugin: plugin.clone() }
                    }),
                    RunOutcome::TimedOut { after } => warnings.push(CodeToolWarning::TimedOut {
                        language: language.clone(),
                        command: format!("claude plugin {verb} {plugin}"),
                        seconds: after.as_secs(),
                    }),
                }
            }
        }
    }
    warnings
}

/// Acrescenta a frase de partida da tabela, quando a ferramenta tem uma e o
/// programa chegou à máquina (`installed`).
fn push_start_hint(warnings: &mut Vec<CodeToolWarning>, language: &str, tool: &CodeTool, installed: bool) {
    if let Some(hint) = tool.start_hint.filter(|_| installed) {
        warnings.push(CodeToolWarning::StartHint { language: language.to_string(), hint });
    }
}

/// Roda o comando de instalação `install_cmd` passo a passo: os passos se
/// separam por `&&`, e cada um roda só quando o gerenciador com que ele
/// começa está no `PATH` e o passo anterior deu certo. Nada aqui passa por
/// shell: cada passo é o programa e as palavras que o seguem. Devolve o prazo
/// que um passo estourou — e os passos seguintes não rodam —, ou `None`.
fn install(install_cmd: &str, runner: &impl ToolRunner) -> Option<Duration> {
    for step in install_cmd.split("&&") {
        let mut words = step.split_whitespace();
        let manager = words.next()?;
        let args: Vec<&str> = words.collect();
        if !runner.on_path(manager) {
            return None;
        }
        match runner.run_outcome(manager, &args) {
            RunOutcome::Succeeded => {}
            RunOutcome::Failed => return None,
            RunOutcome::TimedOut { after } => return Some(after),
        }
    }
    None
}

/// As pastas de ferramenta do usuário, sob a pasta pessoal, onde o executor
/// da máquina procura o programa que não está no `PATH`.
const USER_TOOL_DIRS: [&str; 4] = [".cargo/bin", ".local/bin", ".dotnet/tools", "go/bin"];

/// De quanto em quanto tempo o executor da máquina olha se o comando que
/// espera já acabou.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// O executor da máquina: procura e roda os programas no `PATH` que recebe —
/// o do processo, na instalação de verdade; uma pasta de programas falsos, no
/// teste do binário. As pastas de ferramenta do usuário saem da pasta pessoal
/// real, lida por [`home_dir`] como no resto do programa: `HOME`, ou
/// `USERPROFILE` no Windows. Cada comando tem o prazo `deadline`
/// ([`COMMAND_DEADLINE`], salvo o que [`MachineRunner::with_deadline`] troca).
pub struct MachineRunner {
    path_env: String,
    home: Option<PathBuf>,
    /// Se o sistema é o Windows: decide o separador do `PATH` e o nome com
    /// que o programa aparece numa pasta. Vem da compilação; o teste o troca
    /// para conferir o Windows fora dele.
    windows: bool,
    /// O tempo que cada comando tem para acabar antes de ser cortado.
    deadline: Duration,
}

impl MachineRunner {
    /// O executor sobre `path_env`, uma lista no formato do `PATH` do sistema.
    #[must_use]
    pub fn new(path_env: &str) -> Self {
        Self {
            path_env: path_env.to_string(),
            home: home_dir(),
            windows: cfg!(windows),
            deadline: COMMAND_DEADLINE,
        }
    }

    /// O mesmo executor com outro prazo por comando.
    #[must_use]
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// As pastas de ferramenta do usuário, sob a pasta pessoal; nenhuma
    /// quando não há pasta pessoal.
    pub(crate) fn user_tool_dirs(&self) -> Vec<PathBuf> {
        self.home
            .iter()
            .flat_map(|home| USER_TOOL_DIRS.iter().map(move |dir| home.join(dir)))
            .collect()
    }
}

impl ToolRunner for MachineRunner {
    fn on_path(&self, program: &str) -> bool {
        program_file(program, self.windows, &self.path_env).is_some_and(|file| file.is_file())
    }

    fn run(&self, program: &str, args: &[&str]) -> bool {
        self.run_outcome(program, args) == RunOutcome::Succeeded
    }

    /// Roda o arquivo que o `PATH` do executor tem para `program` (no
    /// Windows, o `npm.cmd` do `npm`); sem ele, o nome puro, que falha como
    /// antes. A saída do comando não é lida, então nada a prende: o comando
    /// que passa do prazo é morto e a espera acaba na hora.
    fn run_outcome(&self, program: &str, args: &[&str]) -> RunOutcome {
        let file = program_file(program, self.windows, &self.path_env).unwrap_or_else(|| PathBuf::from(program));
        let Ok(mut child) = Command::new(file)
            .args(args)
            .env("PATH", &self.path_env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return RunOutcome::Failed;
        };
        let limit = Instant::now() + self.deadline;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return RunOutcome::Succeeded,
                Ok(Some(_)) => return RunOutcome::Failed,
                Ok(None) if Instant::now() < limit => std::thread::sleep(POLL_INTERVAL),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return RunOutcome::TimedOut { after: self.deadline };
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return RunOutcome::Failed;
                }
            }
        }
    }

    fn found_off_path(&self, program: &str) -> Option<PathBuf> {
        let names = program_file_names(program, self.windows);
        self.user_tool_dirs()
            .into_iter()
            .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
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
        let model_path = crate::io::project_map::model_path(dir.path());
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
        timing_out: Vec<&'static str>,
        off_path: Vec<(&'static str, &'static str)>,
        log: std::cell::RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn new(on_path: &[&str]) -> Self {
            Self {
                on_path: std::cell::RefCell::new(on_path.iter().map(|p| (*p).to_string()).collect()),
                brings: Vec::new(),
                failing: Vec::new(),
                timing_out: Vec::new(),
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

        /// Um comando cuja linha contém um dos trechos de `timing_out` passa
        /// do prazo de 60 s, sem esperar de verdade.
        fn run_outcome(&self, program: &str, args: &[&str]) -> RunOutcome {
            let line = std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
            if self.timing_out.iter().any(|t| line.contains(t)) {
                self.log.borrow_mut().push(line);
                return RunOutcome::TimedOut { after: COMMAND_DEADLINE };
            }
            if self.run(program, args) {
                RunOutcome::Succeeded
            } else {
                RunOutcome::Failed
            }
        }

        fn found_off_path(&self, program: &str) -> Option<PathBuf> {
            self.off_path.iter().find(|(p, _)| *p == program).map(|(_, at)| PathBuf::from(at))
        }
    }

    /// Um projeto em C#, Rust e TypeScript, sem mapa ainda. Cada linguagem cai
    /// num lado da divisa: o `csharp-ls` falta e o `dotnet` também, então nada
    /// se instala e sai o aviso com o comando; o `rust-analyzer` falta e o
    /// `rustup` está, então o comando roda; o `typescript-language-server` já
    /// está e a conferência dele passa, então o `npm` nem é chamado. O plugin de cada uma é instalado e
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
        let model_path = crate::io::project_map::model_path(project.path());

        let mut runner = FakeRunner::new(&["rustup", "npm", "node", "claude", "typescript-language-server"]);
        runner.brings.push(("rustup", "rust-analyzer"));
        runner.failing.push("claude plugin install csharp-lsp@claude-plugins-official");

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            *runner.log.borrow(),
            vec![
                "claude plugin install csharp-lsp@claude-plugins-official".to_string(),
                "claude plugin enable csharp-lsp@claude-plugins-official".to_string(),
                "rustup component add rust-analyzer".to_string(),
                "claude plugin install rust-analyzer-lsp@claude-plugins-official".to_string(),
                "claude plugin enable rust-analyzer-lsp@claude-plugins-official".to_string(),
                TYPESCRIPT_CHECK.join(" "),
                "claude plugin install typescript-lsp@claude-plugins-official".to_string(),
                "claude plugin enable typescript-lsp@claude-plugins-official".to_string(),
            ]
        );
        assert_eq!(
            warnings,
            vec![
                CodeToolWarning::ProgramMissing {
                    language: "csharp".to_string(),
                    program: "csharp-ls",
                    install_cmd: CSHARP_INSTALL,
                },
                CodeToolWarning::PluginNotInstalled {
                    language: "csharp".to_string(),
                    plugin: "csharp-lsp@claude-plugins-official".to_string(),
                },
            ]
        );
        let texts: Vec<String> = warnings.iter().map(ToString::to_string).collect();
        assert!(
            texts[0].ends_with("install manually: dotnet tool install --global csharp-ls --version 0.18.0"),
            "{texts:?}"
        );
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
        let model_path = crate::io::project_map::model_path(project.path());

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

    /// Um projeto TypeScript, sem mapa ainda: o `package.json` com o
    /// `typescript` entre as dependências.
    fn typescript_project() -> tempfile::TempDir {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("package.json"), r#"{"devDependencies":{"typescript":"7.0.2"}}"#).unwrap();
        project
    }

    /// O servidor de TypeScript que falta é instalado global, e o TypeScript 6
    /// vai para a pasta dele; nenhum comando da tabela instala o `typescript`
    /// global, o que trocaria o `tsc` que a pessoa usa no próprio trabalho.
    #[test]
    fn o_servidor_de_typescript_ausente_ganha_o_typescript_6_na_pasta_dele() {
        let project = typescript_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["npm", "node", "claude"]);
        runner.brings.push(("npm", "typescript-language-server"));

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            *runner.log.borrow(),
            vec![
                "npm install -g typescript-language-server".to_string(),
                "npm explore -g typescript-language-server -- npm install --global=false --prefix lib --no-save \
                 typescript@6"
                    .to_string(),
                TYPESCRIPT_CHECK.join(" "),
                "claude plugin install typescript-lsp@claude-plugins-official".to_string(),
                "claude plugin enable typescript-lsp@claude-plugins-official".to_string(),
            ]
        );
        assert_eq!(warnings, Vec::new());
        for (language, tool) in CODE_TOOLS {
            for step in tool.install_cmd.split("&&") {
                let words: Vec<&str> = step.split_whitespace().collect();
                let global_install = words.starts_with(&["npm", "install"]) && words.contains(&"-g");
                let typescript = words.iter().any(|w| *w == "typescript" || w.starts_with("typescript@"));
                assert!(!(global_install && typescript), "{language} installs typescript globally: {step}");
            }
        }
    }

    /// O servidor que está no `PATH` mas não responde — o TypeScript que ele
    /// acha não traz o `tsserver.js` — conta como faltando: a etapa roda a
    /// instalação, e, com a conferência falhando de novo, avisa com o comando
    /// pronto. O plugin segue sendo instalado.
    #[test]
    fn o_servidor_que_nao_responde_e_reinstalado_e_avisa() {
        let project = typescript_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["npm", "node", "claude", "typescript-language-server"]);
        runner.failing.push("node -e");

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        let lines = runner.log.borrow().clone();
        let check = TYPESCRIPT_CHECK.join(" ");
        assert_eq!(lines.iter().filter(|l| **l == check).count(), 2, "checked before and after: {lines:?}");
        assert_eq!(lines[1], "npm install -g typescript-language-server", "{lines:?}");
        assert!(lines[2].starts_with("npm explore -g typescript-language-server -- "), "{lines:?}");
        assert_eq!(
            warnings,
            vec![CodeToolWarning::NotReady {
                language: "typescript".to_string(),
                program: "typescript-language-server",
                install_cmd: TYPESCRIPT_INSTALL,
            }]
        );
        let text = warnings[0].to_string();
        assert!(text.ends_with(&format!("install manually: {TYPESCRIPT_INSTALL}")), "{text}");
        assert!(lines.ends_with(&["claude plugin install typescript-lsp@claude-plugins-official".to_string(),
            "claude plugin enable typescript-lsp@claude-plugins-official".to_string()]), "{lines:?}");
    }

    /// A linguagem sem conferência na tabela, como Rust, não roda conferência
    /// nenhuma: o programa no `PATH` basta, como sempre bastou.
    #[test]
    fn a_linguagem_sem_conferencia_nao_roda_conferencia() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let model_path = crate::io::project_map::model_path(project.path());
        let runner = FakeRunner::new(&["rustup", "claude", "rust-analyzer"]);

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            *runner.log.borrow(),
            vec![
                "claude plugin install rust-analyzer-lsp@claude-plugins-official",
                "claude plugin enable rust-analyzer-lsp@claude-plugins-official",
            ]
        );
        assert_eq!(warnings, Vec::new());
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

    /// Ferramenta instalada na pasta de ferramentas do usuário, fora do
    /// `PATH`, é achada pelo nome que o sistema dá ao arquivo: no Windows, com
    /// a extensão de executável; nos outros sistemas, pelo nome puro. Sem
    /// isso, no Windows ela sairia como ausente, com o comando de instalar.
    #[test]
    fn o_executor_acha_fora_do_path_o_programa_pelo_nome_do_sistema() {
        let windows_home = tempfile::tempdir().unwrap();
        let windows_bin = windows_home.path().join(".cargo").join("bin");
        std::fs::create_dir_all(&windows_bin).unwrap();
        std::fs::write(windows_bin.join("rg.exe"), "").unwrap();
        let windows = MachineRunner {
            path_env: String::new(),
            home: Some(windows_home.path().to_path_buf()),
            windows: true,
            deadline: COMMAND_DEADLINE,
        };
        assert_eq!(windows.found_off_path("rg"), Some(windows_bin.join("rg.exe")));
        assert!(!windows.on_path("rg"));

        let linux_home = tempfile::tempdir().unwrap();
        let linux_bin = linux_home.path().join(".cargo").join("bin");
        std::fs::create_dir_all(&linux_bin).unwrap();
        std::fs::write(linux_bin.join("rg"), "").unwrap();
        let linux = MachineRunner {
            path_env: String::new(),
            home: Some(linux_home.path().to_path_buf()),
            windows: false,
            deadline: COMMAND_DEADLINE,
        };
        assert_eq!(linux.found_off_path("rg"), Some(linux_bin.join("rg")));
    }

    /// Um projeto em C#, sem mapa ainda: o `.csproj` basta.
    fn csharp_project() -> tempfile::TempDir {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("App.csproj"), "<Project/>\n").unwrap();
        project
    }

    /// O servidor de C# que falta é instalado na versão 0.18.0: a mais nova
    /// falha no SDK 9 do dotnet, então a linha da tabela a fixa e a etapa roda
    /// exatamente essa linha.
    #[test]
    fn o_servidor_de_csharp_e_instalado_na_versao_que_funciona() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["dotnet", "claude"]);
        runner.brings.push(("dotnet", "csharp-ls"));

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            *runner.log.borrow(),
            vec![
                "dotnet tool install --global csharp-ls --version 0.18.0",
                "claude plugin install csharp-lsp@claude-plugins-official",
                "claude plugin enable csharp-lsp@claude-plugins-official",
            ]
        );
        assert_eq!(warnings, Vec::new());
    }

    /// O `csharp-ls` que a instalação deixou fora do `PATH` pode não abrir
    /// quando o dotnet do sistema é mais velho que o da pasta pessoal: o
    /// aviso do `PATH` vem seguido da frase do `DOTNET_ROOT`, com o comando
    /// pronto. Sem o programa instalado (nada a ajustar) e em outra
    /// linguagem, a frase não sai.
    #[test]
    fn o_csharp_ls_instalado_fora_do_path_traz_o_dotnet_root_no_aviso() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["claude"]);
        runner.off_path.push(("csharp-ls", "/home/u/.dotnet/tools/csharp-ls"));

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            warnings,
            vec![
                CodeToolWarning::OffPath {
                    language: "csharp".to_string(),
                    program: "csharp-ls",
                    found_at: PathBuf::from("/home/u/.dotnet/tools/csharp-ls"),
                },
                CodeToolWarning::StartHint { language: "csharp".to_string(), hint: CSHARP_START_HINT },
            ]
        );
        let text = warnings[1].to_string();
        assert!(text.contains("DOTNET_ROOT=~/.dotnet"), "{text}");
        assert!(text.ends_with(r#"export DOTNET_ROOT="$HOME/.dotnet""#), "{text}");

        let absent = FakeRunner::new(&["claude"]);
        let warnings = ensure_code_tools(project.path(), &model_path, &absent);
        assert!(
            matches!(warnings.as_slice(), [CodeToolWarning::ProgramMissing { .. }]),
            "sem programa instalado não há o que ajustar: {warnings:?}"
        );
    }

    /// O comando de instalação que passa do prazo vira o aviso de que passou,
    /// com a linha inteira para a pessoa rodar, e não o de programa
    /// ausente. Num comando de dois passos, o passo que estoura é o último:
    /// o seguinte não roda.
    #[test]
    fn o_comando_de_instalacao_que_passa_do_prazo_vira_aviso() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["dotnet", "claude"]);
        runner.timing_out.push("dotnet tool install");

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            warnings,
            vec![CodeToolWarning::TimedOut {
                language: "csharp".to_string(),
                command: CSHARP_INSTALL.to_string(),
                seconds: 60,
            }]
        );
        assert_eq!(
            warnings[0].to_string(),
            "csharp: timed out after 60s - run manually: dotnet tool install --global csharp-ls --version 0.18.0"
        );

        let project = typescript_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["npm", "node", "claude"]);
        runner.timing_out.push("npm install -g typescript-language-server");
        let warnings = ensure_code_tools(project.path(), &model_path, &runner);
        assert_eq!(
            *runner.log.borrow(),
            vec![
                "npm install -g typescript-language-server",
                "claude plugin install typescript-lsp@claude-plugins-official",
                "claude plugin enable typescript-lsp@claude-plugins-official",
            ],
            "o passo seguinte não roda depois do que estourou o prazo"
        );
        assert_eq!(
            warnings,
            vec![CodeToolWarning::TimedOut {
                language: "typescript".to_string(),
                command: TYPESCRIPT_INSTALL.to_string(),
                seconds: 60,
            }]
        );
    }

    /// O `claude plugin install` e o `claude plugin enable` que passam do
    /// prazo viram o aviso de que passou, cada um com o próprio comando; o
    /// outro passo segue.
    #[test]
    fn o_plugin_que_passa_do_prazo_vira_aviso() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let model_path = crate::io::project_map::model_path(project.path());
        let plugin = "rust-analyzer-lsp@claude-plugins-official";

        for verb in ["install", "enable"] {
            let mut runner = FakeRunner::new(&["claude", "rust-analyzer"]);
            runner.timing_out.push(if verb == "install" {
                "claude plugin install"
            } else {
                "claude plugin enable"
            });

            let warnings = ensure_code_tools(project.path(), &model_path, &runner);

            assert_eq!(
                warnings,
                vec![CodeToolWarning::TimedOut {
                    language: "rust".to_string(),
                    command: format!("claude plugin {verb} {plugin}"),
                    seconds: 60,
                }],
                "{verb}"
            );
            assert_eq!(runner.log.borrow().len(), 2, "o outro passo do plugin segue: {:?}", runner.log.borrow());
        }
    }

    /// O executor da máquina corta o comando que passa do prazo e volta na
    /// hora, em vez de esperar o fim dele; o que acaba antes disso volta com o
    /// resultado de sempre.
    #[test]
    #[cfg(unix)]
    fn o_executor_da_maquina_corta_o_comando_que_passa_do_prazo() {
        let path = std::env::var("PATH").unwrap_or_default();
        let runner = MachineRunner::new(&path).with_deadline(Duration::from_millis(300));

        let started = Instant::now();
        let outcome = runner.run_outcome("sh", &["-c", "sleep 8"]);
        assert_eq!(outcome, RunOutcome::TimedOut { after: Duration::from_millis(300) });
        assert!(started.elapsed() < Duration::from_secs(4), "voltou depois de {:?}", started.elapsed());
        assert!(!runner.run("sh", &["-c", "sleep 8"]), "passar do prazo não é dar certo");

        assert_eq!(runner.run_outcome("sh", &["-c", "exit 0"]), RunOutcome::Succeeded);
        assert_eq!(runner.run_outcome("sh", &["-c", "exit 3"]), RunOutcome::Failed);
        assert_eq!(runner.run_outcome("no-such-program-here", &[]), RunOutcome::Failed);
    }

    /// O executor que a instalação e a atualização criam já nasce com o
    /// prazo de 60 s por comando.
    #[test]
    fn o_executor_da_maquina_nasce_com_o_prazo_de_60_segundos() {
        assert_eq!(COMMAND_DEADLINE, Duration::from_secs(60));
        assert_eq!(MachineRunner::new("").deadline, COMMAND_DEADLINE);
    }
}
