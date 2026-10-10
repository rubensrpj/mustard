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
//! mostrar. A etapa inteira tem um orçamento de tempo ([`STEP_BUDGET`]), somando
//! todas as linguagens, e cada comando da máquina roda com o menor entre o prazo
//! dele ([`COMMAND_DEADLINE`]) e o que resta do orçamento: o comando que passa
//! do prazo vira o aviso de que passou, nunca uma instalação parada, e com o
//! orçamento esgotado os passos seguintes nem rodam — cada um vira o mesmo
//! aviso, com a linha inteira da tabela.
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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::domain::scan::read_projects;
use crate::domain::source_lang::detected_languages;
use crate::io::fs;
use crate::platform::harness::home_dir;
use crate::platform::process::{program_file, program_file_names};

/// Startup configuration for native queries against an existing language server.
pub struct LanguageServer {
    pub args: &'static [&'static str],
    pub initialization_options: Option<&'static str>,
}

/// O programa que um plugin de linguagem chama, e o comando que instala esse
/// programa. `plugin` é `None` para uma linguagem que o catálogo oficial
/// ainda não cobre — hoje, Dart.
///
/// `install_cmd` pode ter mais de um passo, separados por `&&`: a etapa roda
/// um depois do outro, sem shell, e a pessoa pode colar a linha inteira no
/// terminal. `check`, quando há, é o comando — o programa e os argumentos —
/// que sai com sucesso só quando o programa responde: estar no `PATH` não
/// basta para ele. `start_hint`, quando há, é a frase que acompanha o aviso do
/// programa que foi instalado mas não roda, com o comando pronto. `pin`, quando
/// há, é a versão em que o programa precisa ficar, e como a etapa a confere.
pub struct CodeTool {
    pub lsp: LanguageServer,
    pub plugin: Option<&'static str>,
    pub program: &'static str,
    pub install_cmd: &'static str,
    pub check: Option<&'static [&'static str]>,
    pub start_hint: Option<&'static str>,
    pub pin: Option<VersionPin>,
}

/// A versão em que um programa precisa ficar: uma versão mais nova ou mais
/// velha instala e não funciona. A etapa lê a listagem do gerenciador e, se o
/// pacote aparece em outra versão, roda `update_cmd`.
pub struct VersionPin {
    /// O comando — o programa e os argumentos — que lista o que o gerenciador
    /// instalou. O primeiro item é o gerenciador: fora do `PATH`, nada se
    /// confere e nada muda.
    pub list: &'static [&'static str],
    /// O nome com que o pacote aparece na primeira coluna da listagem.
    pub package: &'static str,
    /// A versão certa, como a segunda coluna da listagem a escreve.
    pub version: &'static str,
    /// A linha que leva o pacote a `version`; a pessoa a cola no terminal.
    pub update_cmd: &'static str,
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

/// A conferência da versão do servidor de C#: o `csharp-ls` instalado em outra
/// versão que não a 0.18.0 volta para ela. A listagem do dotnet traz uma linha
/// por ferramenta global (`Package Id  Version  Commands`), e o nome do pacote
/// é o mesmo do programa.
const CSHARP_PIN: VersionPin = VersionPin {
    list: &["dotnet", "tool", "list", "--global"],
    package: "csharp-ls",
    version: "0.18.0",
    update_cmd: "dotnet tool update --global csharp-ls --version 0.18.0",
};

/// `(linguagem, ferramenta)` — DADO, não lógica. As chaves são os nomes que
/// `apps/scan/languages.toml` e o registro de pilhas usam.
pub const CODE_TOOLS: &[(&str, CodeTool)] = &[
    (
        "rust",
        CodeTool {
            lsp: LanguageServer {
                args: &[],
                initialization_options: Some(
                    r#"{"check":{"enable":false},"cargo":{"buildScripts":{"enable":false}},"procMacro":{"enable":false}}"#,
                ),
            },
            plugin: Some("rust-analyzer-lsp"),
            program: "rust-analyzer",
            install_cmd: "rustup component add rust-analyzer",
            check: None,
            start_hint: None,
            pin: None,
        },
    ),
    (
        "typescript",
        CodeTool {
            lsp: LanguageServer {
                args: &["--stdio"],
                initialization_options: Some(
                    r#"{"disableAutomaticTypingAcquisition":true,"tsserver":{"useSyntaxServer":"never"}}"#,
                ),
            },
            plugin: Some("typescript-lsp"),
            program: "typescript-language-server",
            install_cmd: TYPESCRIPT_INSTALL,
            check: Some(TYPESCRIPT_CHECK),
            start_hint: None,
            pin: None,
        },
    ),
    (
        "javascript",
        CodeTool {
            lsp: LanguageServer {
                args: &["--stdio"],
                initialization_options: Some(
                    r#"{"disableAutomaticTypingAcquisition":true,"tsserver":{"useSyntaxServer":"never"}}"#,
                ),
            },
            plugin: Some("typescript-lsp"),
            program: "typescript-language-server",
            install_cmd: TYPESCRIPT_INSTALL,
            check: Some(TYPESCRIPT_CHECK),
            start_hint: None,
            pin: None,
        },
    ),
    (
        "csharp",
        CodeTool {
            lsp: LanguageServer {
                args: &[],
                initialization_options: None,
            },
            plugin: Some("csharp-lsp"),
            program: "csharp-ls",
            install_cmd: CSHARP_INSTALL,
            check: None,
            start_hint: Some(CSHARP_START_HINT),
            pin: Some(CSHARP_PIN),
        },
    ),
    (
        "go",
        CodeTool {
            lsp: LanguageServer {
                args: &[],
                initialization_options: None,
            },
            plugin: Some("gopls-lsp"),
            program: "gopls",
            install_cmd: "go install golang.org/x/tools/gopls@latest",
            check: None,
            start_hint: None,
            pin: None,
        },
    ),
    (
        "python",
        CodeTool {
            lsp: LanguageServer {
                args: &["--stdio"],
                initialization_options: None,
            },
            plugin: Some("pyright-lsp"),
            program: "pyright-langserver",
            install_cmd: "npm install -g pyright",
            check: None,
            start_hint: None,
            pin: None,
        },
    ),
    (
        "php",
        CodeTool {
            lsp: LanguageServer {
                args: &["--stdio"],
                initialization_options: None,
            },
            plugin: Some("php-lsp"),
            program: "intelephense",
            install_cmd: "npm install -g intelephense",
            check: None,
            start_hint: None,
            pin: None,
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

/// O orçamento de tempo da etapa inteira, somando todas as linguagens: cada
/// comando roda com o menor entre [`COMMAND_DEADLINE`] e o que resta dele, e
/// com o orçamento esgotado os passos seguintes não rodam. Sem ele, o prazo de
/// cada comando se soma por linguagem e por passo, e a etapa passa dos 2
/// minutos que o Bash do Claude Code dá ao comando que a chama.
pub const STEP_BUDGET: Duration = Duration::from_secs(60);

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
    /// Como [`ToolRunner::run`], cortando o comando quando ele passa de `limit`
    /// e dizendo isso. Um executor sem prazo não precisa escrevê-lo: o comando
    /// ou deu certo, ou falhou.
    fn run_outcome(&self, program: &str, args: &[&str], _limit: Duration) -> RunOutcome {
        if self.run(program, args) {
            RunOutcome::Succeeded
        } else {
            RunOutcome::Failed
        }
    }
    /// Como [`ToolRunner::run_outcome`], devolvendo também o que o comando
    /// escreveu na saída. Um executor que não lê a saída devolve o texto vazio.
    fn output(&self, program: &str, args: &[&str], limit: Duration) -> (RunOutcome, String) {
        (self.run_outcome(program, args, limit), String::new())
    }
    /// Onde `program` foi parar fora do `PATH`, numa pasta de ferramentas do
    /// usuário — `rustup component add` põe o `rust-analyzer` em
    /// `~/.cargo/bin`, por exemplo. Serve só para o aviso dizer o que fazer.
    fn found_off_path(&self, program: &str) -> Option<PathBuf>;
    /// O tempo que a etapa inteira tem para acabar.
    fn budget(&self) -> Duration {
        STEP_BUDGET
    }
    /// O prazo de cada comando, antes de descontar o que já se gastou do
    /// orçamento.
    fn command_deadline(&self) -> Duration {
        COMMAND_DEADLINE
    }
    /// O relógio com que a etapa mede o que gastou; o teste o troca por um que
    /// anda só quando um comando falso gasta tempo.
    fn now(&self) -> Instant {
        Instant::now()
    }
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
    /// O programa está instalado em outra versão que não a que funciona, e o
    /// comando que o leva a ela não deu certo.
    WrongVersion {
        language: String,
        program: &'static str,
        installed: String,
        wanted: &'static str,
        update_cmd: &'static str,
    },
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
            Self::WrongVersion { language, program, installed, wanted, update_cmd } => write!(
                f,
                "{language}: {program} {installed} is installed, but {wanted} is the version that works - run manually: {update_cmd}"
            ),
        }
    }
}

/// A etapa das ferramentas de código: para cada linguagem que
/// [`detect_code_languages`] acha em `project_root`, confere se o programa da
/// tabela já está no `PATH` e responde ([`CodeTool::answers`]); se não, roda
/// o comando de instalação ([`run_steps`]). O programa que segue no `PATH` sem
/// responder vira [`CodeToolWarning::NotReady`]. Antes disso, o programa com a
/// versão fixada na tabela ([`VersionPin`]) tem a versão instalada lida na
/// listagem do gerenciador ([`read_pinned_version`]) e volta para a certa se
/// estiver em outra — responda ele ou não, e esteja no `PATH` ou não, porque o
/// pacote que já existe faria a instalação falhar e deixaria a versão errada.
/// Depois instala e liga o plugin do catálogo — `claude plugin install`/`enable`
/// não falham por já estar instalado. Linguagem sem entrada na tabela ganha só
/// o aviso de que não há plugin.
///
/// O que falha vira [`CodeToolWarning`], com o comando pronto, e a etapa passa
/// à linguagem seguinte: nada aqui aborta a instalação nem imprime. A etapa
/// inteira gasta no máximo [`ToolRunner::budget`]: cada comando roda com o
/// menor entre o prazo dele ([`ToolRunner::command_deadline`]) e o que resta,
/// e com o orçamento esgotado o passo seguinte não roda. O comando que passa
/// do prazo, ou que já não tem tempo para rodar, vira
/// [`CodeToolWarning::TimedOut`] no lugar do aviso de falha do passo dele.
pub fn ensure_code_tools(
    project_root: &Path,
    model_path: &Path,
    runner: &impl ToolRunner,
) -> Vec<CodeToolWarning> {
    ensure_code_tools_in(project_root, model_path, &Budget::start(runner, runner.budget()))
}

/// Como [`ensure_code_tools`], dentro de um orçamento que quem chama já
/// dividiu com outros passos: a etapa gasta no máximo o menor entre o
/// orçamento dela ([`ToolRunner::budget`]) e o que resta de `within`, e o que
/// gasta sai do orçamento de quem chama, porque o relógio é o mesmo.
pub fn ensure_code_tools_in<R: ToolRunner>(
    project_root: &Path,
    model_path: &Path,
    within: &Budget<'_, R>,
) -> Vec<CodeToolWarning> {
    let budget = within.slice(within.runner.budget());
    let mut warnings = Vec::new();
    for language in detect_code_languages(project_root, model_path) {
        let Some(tool) = code_tool_for_language(&language) else {
            warnings.push(CodeToolWarning::NoPlugin { language });
            continue;
        };

        let package_present = settle_pinned_version(&language, tool, pin_state(tool, &budget), &budget, &mut warnings);
        settle_program(&language, tool, package_present, &budget, &mut warnings);

        if let Some(plugin) = tool.plugin {
            let plugin = format!("{plugin}@{PLUGIN_CATALOG}");
            for verb in ["install", "enable"] {
                match budget.run("claude", &["plugin", verb, &plugin]) {
                    RunOutcome::Succeeded => {}
                    RunOutcome::Failed => warnings.push(if verb == "install" {
                        CodeToolWarning::PluginNotInstalled { language: language.clone(), plugin: plugin.clone() }
                    } else {
                        CodeToolWarning::PluginNotEnabled { language: language.clone(), plugin: plugin.clone() }
                    }),
                    RunOutcome::TimedOut { after } => warnings.push(CodeToolWarning::TimedOut {
                        language: language.clone(),
                        command: format!("claude plugin {verb} {plugin}"),
                        seconds: whole_seconds(after),
                    }),
                }
            }
        }
    }
    warnings
}

/// Põe o programa da tabela no `PATH` e respondendo: se ele já está e
/// responde, não faz nada; senão roda a instalação — salvo se o gerenciador já
/// lista o pacote (`package_present`), caso em que instalar falharia — e avisa
/// o que ainda faltar.
fn settle_program<R: ToolRunner>(
    language: &str,
    tool: &CodeTool,
    package_present: bool,
    budget: &Budget<'_, R>,
    warnings: &mut Vec<CodeToolWarning>,
) {
    let runner = budget.runner;
    if runner.on_path(tool.program) && budget.answers(tool) == RunOutcome::Succeeded {
        return;
    }
    if !package_present && let RunOutcome::TimedOut { after } = run_steps(tool.install_cmd, budget) {
        warnings.push(CodeToolWarning::TimedOut {
            language: language.to_string(),
            command: tool.install_cmd.to_string(),
            seconds: whole_seconds(after),
        });
    } else if !runner.on_path(tool.program) {
        let found_off_path = runner.found_off_path(tool.program);
        let off_path = found_off_path.is_some();
        warnings.push(match found_off_path {
            Some(found_at) => CodeToolWarning::OffPath {
                language: language.to_string(),
                program: tool.program,
                found_at,
            },
            None => CodeToolWarning::ProgramMissing {
                language: language.to_string(),
                program: tool.program,
                install_cmd: tool.install_cmd,
            },
        });
        // O programa que a instalação deixou fora do `PATH` é o que
        // pode não abrir; o que nem foi instalado não tem o que ajustar.
        push_start_hint(warnings, language, tool, off_path);
    } else {
        match budget.answers(tool) {
            RunOutcome::Succeeded => {}
            RunOutcome::Failed => {
                warnings.push(CodeToolWarning::NotReady {
                    language: language.to_string(),
                    program: tool.program,
                    install_cmd: tool.install_cmd,
                });
                push_start_hint(warnings, language, tool, true);
            }
            // A conferência que não coube no orçamento não diz que o programa
            // não responde: diz que a etapa acabou antes de saber, e a
            // pessoa termina com a linha de instalação.
            RunOutcome::TimedOut { after } => warnings.push(CodeToolWarning::TimedOut {
                language: language.to_string(),
                command: tool.install_cmd.to_string(),
                seconds: whole_seconds(after),
            }),
        }
    }
}

/// Acrescenta a frase de partida da tabela, quando a ferramenta tem uma e o
/// programa chegou à máquina (`installed`).
fn push_start_hint(warnings: &mut Vec<CodeToolWarning>, language: &str, tool: &CodeTool, installed: bool) {
    if let Some(hint) = tool.start_hint.filter(|_| installed) {
        warnings.push(CodeToolWarning::StartHint { language: language.to_string(), hint });
    }
}

/// O que a listagem do gerenciador diz da versão fixada de uma ferramenta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinState {
    /// Nada a comparar: a ferramenta não fixa versão, o gerenciador não está
    /// no `PATH`, ou a listagem não respondeu.
    NotChecked,
    /// O gerenciador não lista o pacote — não foi instalado por ele.
    NotListed,
    /// O pacote está listado na versão certa.
    Right,
    /// O pacote está listado em `installed`, outra versão que não a certa.
    Wrong { installed: String },
    /// A listagem passou do prazo, ou não coube no orçamento.
    TimedOut { after: Duration },
}

/// Lê, só lendo, a versão que o gerenciador tem instalada da ferramenta e a
/// compara com a fixada na tabela ([`VersionPin`]): é a mesma leitura que a
/// etapa de instalação faz antes de atualizar, e a que o diagnóstico usa para
/// avisar. Nada é instalado nem atualizado aqui.
#[must_use]
pub fn read_pinned_version(tool: &CodeTool, runner: &impl ToolRunner) -> PinState {
    pin_state(tool, &Budget::start(runner, runner.command_deadline()))
}

fn pin_state<R: ToolRunner>(tool: &CodeTool, budget: &Budget<'_, R>) -> PinState {
    let Some(pin) = &tool.pin else { return PinState::NotChecked };
    let [manager, args @ ..] = pin.list else { return PinState::NotChecked };
    if !budget.runner.on_path(manager) {
        return PinState::NotChecked;
    }
    match budget.output(manager, args) {
        (RunOutcome::Succeeded, listing) => match installed_version(&listing, pin.package) {
            None => PinState::NotListed,
            Some(installed) if installed == pin.version => PinState::Right,
            Some(installed) => PinState::Wrong { installed: installed.to_string() },
        },
        (RunOutcome::Failed, _) => PinState::NotChecked,
        (RunOutcome::TimedOut { after }, _) => PinState::TimedOut { after },
    }
}

/// Leva o pacote à versão fixada quando `state` diz que ele está em outra, e
/// põe no aviso o que não deu certo, com o comando pronto. Devolve `true`
/// quando o gerenciador já tem o pacote — ou está parado demais para dizer —,
/// e então instalar de novo só falharia.
fn settle_pinned_version<R: ToolRunner>(
    language: &str,
    tool: &CodeTool,
    state: PinState,
    budget: &Budget<'_, R>,
    warnings: &mut Vec<CodeToolWarning>,
) -> bool {
    let Some(pin) = &tool.pin else { return false };
    let timed_out = |after: Duration| CodeToolWarning::TimedOut {
        language: language.to_string(),
        command: pin.update_cmd.to_string(),
        seconds: whole_seconds(after),
    };
    match state {
        PinState::NotChecked | PinState::NotListed => false,
        PinState::Right => true,
        PinState::TimedOut { after } => {
            warnings.push(timed_out(after));
            true
        }
        PinState::Wrong { installed } => {
            match run_steps(pin.update_cmd, budget) {
                RunOutcome::Succeeded => {}
                RunOutcome::Failed => warnings.push(CodeToolWarning::WrongVersion {
                    language: language.to_string(),
                    program: tool.program,
                    installed,
                    wanted: pin.version,
                    update_cmd: pin.update_cmd,
                }),
                RunOutcome::TimedOut { after } => warnings.push(timed_out(after)),
            }
            true
        }
    }
}

/// A versão que `listing` — a saída de um gerenciador de pacotes — dá a
/// `package`: a primeira linha cuja primeira coluna é o nome do pacote (sem
/// distinguir maiúsculas) traz a versão na segunda. O cabeçalho e o texto de
/// primeira execução do gerenciador não têm o pacote na primeira coluna e não
/// contam; a língua deles também não importa.
fn installed_version<'a>(listing: &'a str, package: &str) -> Option<&'a str> {
    listing.lines().find_map(|line| {
        let mut columns = line.split_whitespace();
        let name = columns.next()?;
        let version = columns.next()?;
        name.eq_ignore_ascii_case(package).then_some(version)
    })
}

/// O prazo em segundos inteiros, arredondado para o mais próximo: o primeiro
/// comando recebe o orçamento menos os milissegundos que a etapa levou para
/// chegar até ele, e o aviso de 59,99 s diz 60 s.
#[must_use]
pub fn whole_seconds(after: Duration) -> u64 {
    after.as_secs() + u64::from(after.subsec_millis() >= 500)
}

/// O orçamento de tempo de uma sequência de comandos: quando ela começou,
/// quanto pode gastar e quem roda os comandos. O tempo gasto sai do relógio do
/// executor ([`ToolRunner::now`]), o mesmo para o orçamento e para as fatias
/// dele, então o que uma fatia gasta também sai do que resta ao todo.
pub struct Budget<'a, R: ToolRunner> {
    runner: &'a R,
    started: Instant,
    total: Duration,
}

impl<'a, R: ToolRunner> Budget<'a, R> {
    /// O orçamento de `total`, que começa a contar agora.
    pub fn start(runner: &'a R, total: Duration) -> Self {
        Self { runner, started: runner.now(), total }
    }

    /// O tempo que este orçamento tinha ao começar.
    #[must_use]
    pub fn total(&self) -> Duration {
        self.total
    }

    /// O que ainda resta.
    #[must_use]
    pub fn remaining(&self) -> Duration {
        self.total.saturating_sub(self.runner.now().saturating_duration_since(self.started))
    }

    /// Uma fatia deste orçamento, com o teto `cap`: nunca mais que o que
    /// resta agora. Começa a contar já.
    #[must_use]
    pub fn slice(&self, cap: Duration) -> Self {
        Self { runner: self.runner, started: self.runner.now(), total: cap.min(self.remaining()) }
    }

    /// Roda `work` com o prazo do próximo comando — o menor entre `deadline` e
    /// o que resta do orçamento —, ou devolve `None` sem rodar nada quando o
    /// orçamento está esgotado.
    pub fn within<T>(&self, deadline: Duration, work: impl FnOnce(Duration) -> T) -> Option<T> {
        let left = self.remaining();
        (!left.is_zero()).then(|| work(left.min(deadline)))
    }

    /// Roda o comando dentro do que resta do orçamento. Com o orçamento
    /// esgotado o comando nem roda, e o resultado é o de quem passou do prazo.
    fn run(&self, program: &str, args: &[&str]) -> RunOutcome {
        self.within(self.runner.command_deadline(), |limit| self.runner.run_outcome(program, args, limit))
            .unwrap_or(RunOutcome::TimedOut { after: self.total })
    }

    /// Como [`Budget::run`], devolvendo também a saída do comando.
    fn output(&self, program: &str, args: &[&str]) -> (RunOutcome, String) {
        self.within(self.runner.command_deadline(), |limit| self.runner.output(program, args, limit))
            .unwrap_or((RunOutcome::TimedOut { after: self.total }, String::new()))
    }

    /// A conferência do programa pela tabela, dentro do orçamento; sem
    /// conferência, o programa no `PATH` basta e nada roda.
    fn answers(&self, tool: &CodeTool) -> RunOutcome {
        match tool.check {
            Some([program, args @ ..]) => self.run(program, args),
            _ => RunOutcome::Succeeded,
        }
    }
}

/// Roda o comando `line` passo a passo: os passos se separam por `&&`, e cada
/// um roda só quando o gerenciador com que ele começa está no `PATH` e o passo
/// anterior deu certo. Nada aqui passa por shell: cada passo é o programa e as
/// palavras que o seguem. O passo que estoura o prazo — ou que já não cabe no
/// orçamento — devolve [`RunOutcome::TimedOut`], e os seguintes não rodam.
fn run_steps<R: ToolRunner>(line: &str, budget: &Budget<'_, R>) -> RunOutcome {
    for step in line.split("&&") {
        let mut words = step.split_whitespace();
        let Some(manager) = words.next() else { return RunOutcome::Failed };
        let args: Vec<&str> = words.collect();
        if !budget.runner.on_path(manager) {
            return RunOutcome::Failed;
        }
        match budget.run(manager, &args) {
            RunOutcome::Succeeded => {}
            other => return other,
        }
    }
    RunOutcome::Succeeded
}

/// As pastas de ferramenta do usuário, sob a pasta pessoal, onde o executor
/// da máquina procura o programa que não está no `PATH`.
const USER_TOOL_DIRS: [&str; 4] = [".cargo/bin", ".local/bin", ".dotnet/tools", "go/bin"];

/// De quanto em quanto tempo o executor da máquina olha se o comando que
/// espera já acabou.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Quanto o executor espera pela saída de um comando que já terminou: o
/// comando que deixou um processo filho segurando a saída não prende a etapa.
const OUTPUT_GRACE: Duration = Duration::from_millis(500);

/// O executor da máquina: procura e roda os programas no `PATH` que recebe —
/// o do processo, na instalação de verdade; uma pasta de programas falsos, no
/// teste do binário. As pastas de ferramenta do usuário saem da pasta pessoal
/// real, lida por [`home_dir`] como no resto do programa: `HOME`, ou
/// `USERPROFILE` no Windows. Cada comando tem o prazo [`COMMAND_DEADLINE`], e
/// a etapa inteira, o orçamento [`STEP_BUDGET`].
pub struct MachineRunner {
    path_env: String,
    home: Option<PathBuf>,
    /// Se o sistema é o Windows: decide o separador do `PATH` e o nome com
    /// que o programa aparece numa pasta. Vem da compilação; o teste o troca
    /// para conferir o Windows fora dele.
    windows: bool,
}

impl MachineRunner {
    /// O executor sobre `path_env`, uma lista no formato do `PATH` do sistema.
    #[must_use]
    pub fn new(path_env: &str) -> Self {
        Self {
            path_env: path_env.to_string(),
            home: home_dir(),
            windows: cfg!(windows),
        }
    }

    /// As pastas de ferramenta do usuário, sob a pasta pessoal; nenhuma
    /// quando não há pasta pessoal.
    pub(crate) fn user_tool_dirs(&self) -> Vec<PathBuf> {
        self.home
            .iter()
            .flat_map(|home| USER_TOOL_DIRS.iter().map(move |dir| home.join(dir)))
            .collect()
    }

    /// Roda o arquivo que o `PATH` do executor tem para `program` (no
    /// Windows, o `npm.cmd` do `npm`); sem ele, o nome puro, que falha como
    /// antes. O comando que passa de `limit` é morto e a espera acaba na hora.
    /// A saída só é lida quando `capture` pede: sem isso, nada a prende.
    fn spawn_and_wait(&self, program: &str, args: &[&str], limit: Duration, capture: bool) -> (RunOutcome, String) {
        let file = program_file(program, self.windows, &self.path_env).unwrap_or_else(|| PathBuf::from(program));
        let Ok(mut child) = Command::new(file)
            .args(args)
            .env("PATH", &self.path_env)
            .stdin(Stdio::null())
            .stdout(if capture { Stdio::piped() } else { Stdio::null() })
            .stderr(Stdio::null())
            .spawn()
        else {
            return (RunOutcome::Failed, String::new());
        };
        // A saída é lida à parte, para o comando que escreve muito não parar
        // no pipe cheio; a leitura acaba quando o comando (e quem ele abriu)
        // fecha a saída.
        let output = child.stdout.take().map(|mut out| {
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = out.read_to_end(&mut bytes);
                let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
            });
            receiver
        });
        let limit_at = Instant::now() + limit;
        let outcome = loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break RunOutcome::Succeeded,
                Ok(Some(_)) => break RunOutcome::Failed,
                Ok(None) if Instant::now() < limit_at => std::thread::sleep(POLL_INTERVAL),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (RunOutcome::TimedOut { after: limit }, String::new());
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (RunOutcome::Failed, String::new());
                }
            }
        };
        let text = output.and_then(|receiver| receiver.recv_timeout(OUTPUT_GRACE).ok()).unwrap_or_default();
        (outcome, text)
    }
}

impl ToolRunner for MachineRunner {
    fn on_path(&self, program: &str) -> bool {
        program_file(program, self.windows, &self.path_env).is_some_and(|file| file.is_file())
    }

    fn run(&self, program: &str, args: &[&str]) -> bool {
        self.run_outcome(program, args, COMMAND_DEADLINE) == RunOutcome::Succeeded
    }

    fn run_outcome(&self, program: &str, args: &[&str], limit: Duration) -> RunOutcome {
        self.spawn_and_wait(program, args, limit, false).0
    }

    fn output(&self, program: &str, args: &[&str], limit: Duration) -> (RunOutcome, String) {
        self.spawn_and_wait(program, args, limit, true)
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
    /// contém um dos trechos de `failing` sai com erro. O tempo também é de
    /// mentira: o relógio só anda quando um comando de `taking` (ou de
    /// `timing_out`, que passa de qualquer prazo) gasta o dele, então o
    /// orçamento se esgota sem ninguém esperar.
    struct FakeRunner {
        on_path: std::cell::RefCell<BTreeSet<String>>,
        brings: Vec<(&'static str, &'static str)>,
        failing: Vec<&'static str>,
        timing_out: Vec<&'static str>,
        taking: Vec<(&'static str, Duration)>,
        listing: &'static str,
        off_path: Vec<(&'static str, &'static str)>,
        log: std::cell::RefCell<Vec<String>>,
        budget: Duration,
        command_deadline: Duration,
        limits: std::cell::RefCell<Vec<Duration>>,
        started: Instant,
        spent: std::cell::Cell<Duration>,
    }

    impl FakeRunner {
        fn new(on_path: &[&str]) -> Self {
            Self {
                on_path: std::cell::RefCell::new(on_path.iter().map(|p| (*p).to_string()).collect()),
                brings: Vec::new(),
                failing: Vec::new(),
                timing_out: Vec::new(),
                taking: Vec::new(),
                listing: "",
                off_path: Vec::new(),
                log: std::cell::RefCell::new(Vec::new()),
                budget: STEP_BUDGET,
                command_deadline: COMMAND_DEADLINE,
                limits: std::cell::RefCell::new(Vec::new()),
                started: Instant::now(),
                spent: std::cell::Cell::new(Duration::ZERO),
            }
        }

        /// O mesmo executor com um orçamento largo, para que o prazo de cada
        /// comando seja o que corta, e não o fim do orçamento.
        fn with_wide_budget(mut self) -> Self {
            self.budget = COMMAND_DEADLINE * 10;
            self
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
            // Ler a listagem não instala nada: só o comando que muda a máquina
            // põe no `PATH` o programa que o gerenciador traz.
            if !args.contains(&"list") {
                for (manager, brought) in &self.brings {
                    if *manager == program {
                        self.on_path.borrow_mut().insert((*brought).to_string());
                    }
                }
            }
            true
        }

        /// Um comando de `timing_out` passa de qualquer prazo, sem esperar de
        /// verdade: gasta o prazo que recebeu e é cortado. Um de `taking` gasta
        /// o tempo dele, ou é cortado no prazo, se o dele for maior.
        fn run_outcome(&self, program: &str, args: &[&str], limit: Duration) -> RunOutcome {
            let line = std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
            self.limits.borrow_mut().push(limit);
            let cost = if self.timing_out.iter().any(|t| line.contains(t)) {
                Duration::MAX
            } else {
                self.taking.iter().find(|(t, _)| line.contains(t)).map_or(Duration::ZERO, |(_, cost)| *cost)
            };
            if cost > limit {
                self.spent.set(self.spent.get() + limit);
                self.log.borrow_mut().push(line);
                return RunOutcome::TimedOut { after: limit };
            }
            self.spent.set(self.spent.get() + cost);
            if self.run(program, args) {
                RunOutcome::Succeeded
            } else {
                RunOutcome::Failed
            }
        }

        /// A saída da listagem do dotnet é a de `listing`; de qualquer outro
        /// comando, vazia.
        fn output(&self, program: &str, args: &[&str], limit: Duration) -> (RunOutcome, String) {
            let outcome = self.run_outcome(program, args, limit);
            let listed = program == "dotnet" && args.starts_with(&["tool", "list"]);
            let text = if listed && outcome == RunOutcome::Succeeded { self.listing.to_string() } else { String::new() };
            (outcome, text)
        }

        fn found_off_path(&self, program: &str) -> Option<PathBuf> {
            self.off_path.iter().find(|(p, _)| *p == program).map(|(_, at)| PathBuf::from(at))
        }

        fn budget(&self) -> Duration {
            self.budget
        }

        fn command_deadline(&self) -> Duration {
            self.command_deadline
        }

        fn now(&self) -> Instant {
            self.started + self.spent.get()
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
    fn the_tools_step_installs_the_plugin_of_each_language() {
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
    fn the_step_warns_without_a_plugin_and_off_the_path_and_goes_on() {
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
    fn a_missing_typescript_server_gets_typescript_6_in_its_own_folder() {
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
    fn a_server_that_does_not_respond_is_reinstalled_and_warns() {
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
    fn a_language_without_a_check_runs_no_check() {
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
    fn the_machine_runner_looks_in_the_path_it_receives() {
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
    fn the_runner_finds_the_program_off_the_path_by_its_system_name() {
        let windows_home = tempfile::tempdir().unwrap();
        let windows_bin = windows_home.path().join(".cargo").join("bin");
        std::fs::create_dir_all(&windows_bin).unwrap();
        std::fs::write(windows_bin.join("rg.exe"), "").unwrap();
        let windows = MachineRunner {
            path_env: String::new(),
            home: Some(windows_home.path().to_path_buf()),
            windows: true,
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
    /// exatamente essa linha, depois de ler a listagem do dotnet e ver que o
    /// pacote não está nela.
    #[test]
    fn the_csharp_server_is_installed_in_the_version_that_works() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["dotnet", "claude"]);
        runner.brings.push(("dotnet", "csharp-ls"));

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            *runner.log.borrow(),
            vec![
                "dotnet tool list --global",
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
    fn a_csharp_ls_installed_off_the_path_brings_the_dotnet_root_in_the_warning() {
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
    fn an_install_command_that_passes_its_deadline_becomes_a_warning() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["dotnet", "claude"]).with_wide_budget();
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
        let mut runner = FakeRunner::new(&["npm", "node", "claude"]).with_wide_budget();
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
    fn a_plugin_that_passes_its_deadline_becomes_a_warning() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let model_path = crate::io::project_map::model_path(project.path());
        let plugin = "rust-analyzer-lsp@claude-plugins-official";

        for verb in ["install", "enable"] {
            let mut runner = FakeRunner::new(&["claude", "rust-analyzer"]).with_wide_budget();
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
    fn the_machine_runner_cuts_the_command_that_passes_its_deadline() {
        let path = std::env::var("PATH").unwrap_or_default();
        let runner = MachineRunner::new(&path);

        let limit = Duration::from_millis(300);
        let started = Instant::now();
        let outcome = runner.run_outcome("sh", &["-c", "sleep 8"], limit);
        assert_eq!(outcome, RunOutcome::TimedOut { after: Duration::from_millis(300) });
        assert!(started.elapsed() < Duration::from_secs(4), "voltou depois de {:?}", started.elapsed());

        assert_eq!(runner.run_outcome("sh", &["-c", "exit 0"], limit), RunOutcome::Succeeded);
        assert_eq!(runner.run_outcome("sh", &["-c", "exit 3"], limit), RunOutcome::Failed);
        assert_eq!(runner.run_outcome("no-such-program-here", &[], limit), RunOutcome::Failed);
    }

    /// O executor que a instalação e a atualização criam já nasce com o
    /// prazo de 60 s por comando e o orçamento de 60 s para a etapa inteira.
    #[test]
    fn the_machine_runner_starts_with_a_60_second_deadline() {
        assert_eq!(COMMAND_DEADLINE, Duration::from_secs(60));
        assert_eq!(STEP_BUDGET, Duration::from_secs(60));
        let runner = MachineRunner::new("");
        assert_eq!(runner.command_deadline(), COMMAND_DEADLINE);
        assert_eq!(runner.budget(), STEP_BUDGET);
    }

    /// Um projeto em Go e Rust, sem mapa ainda: `go` vem antes de `rust` na
    /// ordem das linguagens.
    fn go_and_rust_project() -> tempfile::TempDir {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("go.mod"), "module x\n").unwrap();
        std::fs::write(project.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        project
    }

    /// A etapa inteira gasta o orçamento, somando todas as linguagens: os
    /// comandos do Go gastam os 10 s do orçamento, e a linguagem seguinte não
    /// roda nada — cada passo dela vira o aviso de prazo, com a linha inteira
    /// da tabela. Com tempo de sobra, a mesma etapa roda as duas linguagens
    /// e não avisa nada.
    #[test]
    fn the_whole_step_has_a_budget_and_the_next_language_becomes_a_warning_without_running() {
        let project = go_and_rust_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let plugin = "gopls-lsp@claude-plugins-official";

        let mut runner = FakeRunner::new(&["gopls", "rustup", "claude"]);
        runner.budget = Duration::from_secs(10);
        runner.taking.push(("claude plugin install gopls-lsp", Duration::from_secs(6)));
        runner.taking.push(("claude plugin enable gopls-lsp", Duration::from_secs(4)));
        runner.brings.push(("rustup", "rust-analyzer"));

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            *runner.log.borrow(),
            vec![format!("claude plugin install {plugin}"), format!("claude plugin enable {plugin}")],
            "a segunda linguagem não roda nada com o orçamento esgotado"
        );
        let rust_plugin = "rust-analyzer-lsp@claude-plugins-official";
        assert_eq!(
            warnings,
            vec![
                CodeToolWarning::TimedOut {
                    language: "rust".to_string(),
                    command: "rustup component add rust-analyzer".to_string(),
                    seconds: 10,
                },
                CodeToolWarning::TimedOut {
                    language: "rust".to_string(),
                    command: format!("claude plugin install {rust_plugin}"),
                    seconds: 10,
                },
                CodeToolWarning::TimedOut {
                    language: "rust".to_string(),
                    command: format!("claude plugin enable {rust_plugin}"),
                    seconds: 10,
                },
            ]
        );
        assert_eq!(
            warnings[0].to_string(),
            "rust: timed out after 10s - run manually: rustup component add rust-analyzer"
        );

        let mut roomy = FakeRunner::new(&["gopls", "rustup", "claude"]);
        roomy.budget = Duration::from_secs(11);
        roomy.taking.push(("claude plugin install gopls-lsp", Duration::from_secs(6)));
        roomy.taking.push(("claude plugin enable gopls-lsp", Duration::from_secs(4)));
        roomy.brings.push(("rustup", "rust-analyzer"));
        let warnings = ensure_code_tools(project.path(), &model_path, &roomy);
        assert_eq!(warnings, Vec::new());
        assert_eq!(roomy.log.borrow().len(), 5, "as duas linguagens rodam: {:?}", roomy.log.borrow());
    }

    /// Cada comando roda com o menor entre o prazo dele e o que resta do
    /// orçamento: o primeiro recebe o prazo, o segundo, o que sobrou do
    /// orçamento depois de o primeiro gastar 30 s; e, com o prazo maior que o
    /// orçamento, o primeiro recebe o orçamento inteiro.
    #[test]
    fn each_command_runs_with_the_smaller_of_its_deadline_and_what_is_left() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("go.mod"), "module x\n").unwrap();
        let model_path = crate::io::project_map::model_path(project.path());

        let mut runner = FakeRunner::new(&["gopls", "claude"]);
        runner.command_deadline = Duration::from_secs(40);
        runner.taking.push(("claude plugin install", Duration::from_secs(30)));
        ensure_code_tools(project.path(), &model_path, &runner);
        assert_eq!(*runner.limits.borrow(), vec![Duration::from_secs(40), Duration::from_secs(30)]);

        let mut runner = FakeRunner::new(&["gopls", "claude"]);
        runner.command_deadline = Duration::from_secs(100);
        ensure_code_tools(project.path(), &model_path, &runner);
        assert_eq!(*runner.limits.borrow(), vec![STEP_BUDGET, STEP_BUDGET]);
    }

    /// O comando de conferência do servidor também gasta do orçamento: com o
    /// orçamento acabado antes dele, ele nem roda, e o servidor que ele
    /// conferiria vira o aviso de prazo com a linha de instalação.
    #[test]
    fn the_server_check_also_spends_from_the_budget() {
        let project = typescript_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["npm", "node", "claude", "typescript-language-server"]);
        runner.budget = Duration::from_secs(5);
        runner.timing_out.push("node -e");

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        let check = TYPESCRIPT_CHECK.join(" ");
        assert_eq!(*runner.log.borrow(), vec![check], "só a conferência roda; o resto não cabe no orçamento");
        assert_eq!(warnings[0].to_string(), format!("typescript: timed out after 5s - run manually: {TYPESCRIPT_INSTALL}"));
        assert_eq!(warnings.len(), 3, "a instalação e os dois passos do plugin: {warnings:?}");
    }

    /// O aviso diz o prazo em segundos inteiros, arredondado: 0,99 s é 1 s.
    #[test]
    fn the_warning_deadline_is_rounded_to_the_nearest_second() {
        assert_eq!(whole_seconds(Duration::from_millis(59_990)), 60);
        assert_eq!(whole_seconds(Duration::from_millis(990)), 1);
        assert_eq!(whole_seconds(Duration::from_millis(1_499)), 1);
        assert_eq!(whole_seconds(Duration::from_secs(60)), 60);
        assert_eq!(whole_seconds(Duration::ZERO), 0);
    }

    /// A listagem do dotnet como esta máquina a escreve: o cabeçalho, o
    /// traço e uma linha por ferramenta, em colunas separadas por espaços.
    const DOTNET_LISTING: &str = "Package Id      Version      Commands \n--------------------------------------\ncsharp-ls       0.18.0       csharp-ls\n";

    /// A versão sai da segunda coluna da linha do pacote; o cabeçalho, o traço
    /// e o texto solto do gerenciador não contam, e o nome se compara sem
    /// distinguir maiúsculas.
    #[test]
    fn the_installed_version_comes_from_the_dotnet_listing() {
        assert_eq!(installed_version(DOTNET_LISTING, "csharp-ls"), Some("0.18.0"));
        let with_banner = format!("Welcome to .NET 9.0!\n\n{DOTNET_LISTING}");
        assert_eq!(installed_version(&with_banner, "csharp-ls"), Some("0.18.0"));
        let newer = "Package Id  Version  Commands\n---\nCSharp-LS  0.19.1  csharp-ls\ndotnet-ef  9.0.0  dotnet-ef\n";
        assert_eq!(installed_version(newer, "csharp-ls"), Some("0.19.1"));
        assert_eq!(installed_version(newer, "dotnet-ef"), Some("9.0.0"));
        let empty = "Package Id      Version      Commands \n----------------------------------\n";
        assert_eq!(installed_version(empty, "csharp-ls"), None);
        assert_eq!(installed_version("", "csharp-ls"), None);
        assert_eq!(installed_version("csharp-ls\n", "csharp-ls"), None, "sem a segunda coluna não há versão");
    }

    /// A linha de instalação e a de atualização do C# trazem a mesma versão
    /// que a conferência espera.
    #[test]
    fn the_csharp_install_and_update_carry_the_version_of_the_check() {
        let (_, tool) = CODE_TOOLS.iter().find(|(language, _)| *language == "csharp").unwrap();
        let pin = tool.pin.as_ref().expect("o C# fixa a versão");
        assert_eq!(pin.version, "0.18.0");
        let wanted = format!("--version {}", pin.version);
        assert!(tool.install_cmd.ends_with(&wanted), "{}", tool.install_cmd);
        assert!(pin.update_cmd.ends_with(&wanted), "{}", pin.update_cmd);
        assert!(pin.update_cmd.starts_with("dotnet tool update --global csharp-ls"), "{}", pin.update_cmd);
        assert_eq!(pin.list, ["dotnet", "tool", "list", "--global"]);
        assert!(CODE_TOOLS.iter().filter(|(_, tool)| tool.pin.is_some()).count() == 1, "só o C# fixa versão");
    }

    /// O `csharp-ls` instalado numa versão que não a 0.18.0 volta para ela, pela
    /// linha da tabela; o plugin segue sendo instalado.
    #[test]
    fn csharp_ls_in_another_version_runs_the_update_with_the_version() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["dotnet", "csharp-ls", "claude"]);
        runner.listing = "Package Id      Version      Commands \n--------------------------------------\ncsharp-ls       0.19.2       csharp-ls\n";

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert_eq!(
            *runner.log.borrow(),
            vec![
                "dotnet tool list --global",
                "dotnet tool update --global csharp-ls --version 0.18.0",
                "claude plugin install csharp-lsp@claude-plugins-official",
                "claude plugin enable csharp-lsp@claude-plugins-official",
            ]
        );
        assert_eq!(warnings, Vec::new());
    }

    /// O `csharp-ls` na 0.18.0 não roda o update; o pacote que a listagem não
    /// traz (instalado por outro caminho) também não, e o dotnet que não
    /// está no `PATH` nem chega a listar: nada muda.
    #[test]
    fn csharp_ls_in_the_right_version_or_without_dotnet_does_not_run_the_update() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let plugin_commands = vec![
            "claude plugin install csharp-lsp@claude-plugins-official".to_string(),
            "claude plugin enable csharp-lsp@claude-plugins-official".to_string(),
        ];

        let mut right = FakeRunner::new(&["dotnet", "csharp-ls", "claude"]);
        right.listing = DOTNET_LISTING;
        assert_eq!(ensure_code_tools(project.path(), &model_path, &right), Vec::new());
        let expected: Vec<String> = std::iter::once("dotnet tool list --global".to_string()).chain(plugin_commands.clone()).collect();
        assert_eq!(*right.log.borrow(), expected, "a 0.18.0 listada não pede update");

        let mut unlisted = FakeRunner::new(&["dotnet", "csharp-ls", "claude"]);
        unlisted.listing = "Package Id      Version      Commands \n----------------------------------\nother-tool  1.0.0  other\n";
        assert_eq!(ensure_code_tools(project.path(), &model_path, &unlisted), Vec::new());
        assert!(!unlisted.log.borrow().iter().any(|l| l.contains("update")), "{:?}", unlisted.log.borrow());

        let without_dotnet = FakeRunner::new(&["csharp-ls", "claude"]);
        assert_eq!(ensure_code_tools(project.path(), &model_path, &without_dotnet), Vec::new());
        assert_eq!(*without_dotnet.log.borrow(), plugin_commands, "sem dotnet, nada é listado nem atualizado");
    }

    /// O update que falha vira o aviso da versão errada, com a linha pronta;
    /// o que passa do prazo vira o aviso de prazo com a mesma linha.
    #[test]
    fn a_csharp_update_that_fails_or_passes_its_deadline_becomes_a_warning() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());

        let mut failing = FakeRunner::new(&["dotnet", "csharp-ls", "claude"]);
        failing.listing = "csharp-ls  0.19.2  csharp-ls\n";
        failing.failing.push("dotnet tool update");
        let warnings = ensure_code_tools(project.path(), &model_path, &failing);
        assert_eq!(
            warnings,
            vec![CodeToolWarning::WrongVersion {
                language: "csharp".to_string(),
                program: "csharp-ls",
                installed: "0.19.2".to_string(),
                wanted: "0.18.0",
                update_cmd: "dotnet tool update --global csharp-ls --version 0.18.0",
            }]
        );
        assert_eq!(
            warnings[0].to_string(),
            "csharp: csharp-ls 0.19.2 is installed, but 0.18.0 is the version that works - run manually: \
             dotnet tool update --global csharp-ls --version 0.18.0"
        );

        let mut stalled = FakeRunner::new(&["dotnet", "csharp-ls", "claude"]).with_wide_budget();
        stalled.listing = "csharp-ls  0.19.2  csharp-ls\n";
        stalled.timing_out.push("dotnet tool update");
        let warnings = ensure_code_tools(project.path(), &model_path, &stalled);
        assert_eq!(
            warnings,
            vec![CodeToolWarning::TimedOut {
                language: "csharp".to_string(),
                command: "dotnet tool update --global csharp-ls --version 0.18.0".to_string(),
                seconds: 60,
            }]
        );
    }

    /// O executor da máquina devolve o que o comando escreveu na saída, sem
    /// travar num comando que escreve mais do que cabe no pipe, e volta na
    /// hora, sem texto, com o comando que passa do prazo.
    #[test]
    #[cfg(unix)]
    fn the_machine_runner_reads_the_command_output() {
        let path = std::env::var("PATH").unwrap_or_default();
        let runner = MachineRunner::new(&path);
        let limit = Duration::from_secs(20);

        let (outcome, text) = runner.output("sh", &["-c", "printf 'csharp-ls 0.18.0 csharp-ls\\n'"], limit);
        assert_eq!(outcome, RunOutcome::Succeeded);
        assert_eq!(text, "csharp-ls 0.18.0 csharp-ls\n");

        let (outcome, text) = runner.output("sh", &["-c", "head -c 300000 /dev/zero | tr '\\0' a; exit 3"], limit);
        assert_eq!(outcome, RunOutcome::Failed);
        assert_eq!(text.len(), 300_000, "a saída grande foi lida inteira");

        let started = Instant::now();
        let (outcome, text) = runner.output("sh", &["-c", "echo partial; sleep 8"], Duration::from_millis(300));
        assert_eq!(outcome, RunOutcome::TimedOut { after: Duration::from_millis(300) });
        assert_eq!(text, "");
        assert!(started.elapsed() < Duration::from_secs(4), "voltou depois de {:?}", started.elapsed());
    }

    /// O `csharp-ls` instalado em outra versão e fora do `PATH` não vai para a
    /// instalação — o pacote já existe, e o `dotnet tool install` falharia,
    /// deixando a versão errada —: a listagem do dotnet decide, e o update leva
    /// o pacote à 0.18.0. Com o pacote já na versão certa, a instalação também
    /// não roda, e o aviso do `PATH` sai do mesmo jeito.
    #[test]
    fn csharp_ls_off_the_path_in_another_version_gets_the_update_and_not_the_install() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let plugin_commands = [
            "claude plugin install csharp-lsp@claude-plugins-official".to_string(),
            "claude plugin enable csharp-lsp@claude-plugins-official".to_string(),
        ];
        let off_path = |runner: &mut FakeRunner| {
            runner.off_path.push(("csharp-ls", "/home/u/.dotnet/tools/csharp-ls"));
            runner.failing.push("dotnet tool install");
        };
        let expected_warnings = vec![
            CodeToolWarning::OffPath {
                language: "csharp".to_string(),
                program: "csharp-ls",
                found_at: PathBuf::from("/home/u/.dotnet/tools/csharp-ls"),
            },
            CodeToolWarning::StartHint { language: "csharp".to_string(), hint: CSHARP_START_HINT },
        ];

        let mut broken = FakeRunner::new(&["dotnet", "claude"]);
        off_path(&mut broken);
        broken.listing = "csharp-ls  0.19.2  csharp-ls\n";
        let warnings = ensure_code_tools(project.path(), &model_path, &broken);
        let expected_log: Vec<String> = [
            "dotnet tool list --global".to_string(),
            "dotnet tool update --global csharp-ls --version 0.18.0".to_string(),
        ]
        .into_iter()
        .chain(plugin_commands.clone())
        .collect();
        assert_eq!(*broken.log.borrow(), expected_log, "atualiza, e não tenta instalar o que já existe");
        assert_eq!(warnings, expected_warnings);

        let mut right = FakeRunner::new(&["dotnet", "claude"]);
        off_path(&mut right);
        right.listing = DOTNET_LISTING;
        let warnings = ensure_code_tools(project.path(), &model_path, &right);
        let expected_log: Vec<String> =
            std::iter::once("dotnet tool list --global".to_string()).chain(plugin_commands).collect();
        assert_eq!(*right.log.borrow(), expected_log, "a 0.18.0 listada não pede update nem instalação");
        assert_eq!(warnings, expected_warnings);
    }

    /// A listagem do dotnet que passa do prazo vira o aviso de prazo com a
    /// linha do update, e a instalação não roda por cima de um gerenciador
    /// parado.
    #[test]
    fn a_listing_that_passes_its_deadline_becomes_a_warning_and_does_not_install() {
        let project = csharp_project();
        let model_path = crate::io::project_map::model_path(project.path());
        let mut runner = FakeRunner::new(&["dotnet", "claude"]).with_wide_budget();
        runner.timing_out.push("dotnet tool list");

        let warnings = ensure_code_tools(project.path(), &model_path, &runner);

        assert!(
            !runner.log.borrow().iter().any(|line| line.contains("install --global")),
            "{:?}",
            runner.log.borrow()
        );
        assert_eq!(
            warnings[0],
            CodeToolWarning::TimedOut {
                language: "csharp".to_string(),
                command: "dotnet tool update --global csharp-ls --version 0.18.0".to_string(),
                seconds: 60,
            }
        );
    }

    /// A leitura só lê: a versão certa, a errada, o pacote fora da listagem,
    /// o gerenciador ausente e a ferramenta que não fixa versão dão cada uma o
    /// seu estado, e nenhuma roda outro comando que não a listagem.
    #[test]
    fn reading_the_pinned_version_only_reads_and_gives_a_state_for_each_case() {
        let (_, csharp) = CODE_TOOLS.iter().find(|(language, _)| *language == "csharp").unwrap();
        let (_, rust) = CODE_TOOLS.iter().find(|(language, _)| *language == "rust").unwrap();

        let mut runner = FakeRunner::new(&["dotnet"]);
        runner.listing = DOTNET_LISTING;
        assert_eq!(read_pinned_version(csharp, &runner), PinState::Right);
        runner.listing = "csharp-ls  0.19.2  csharp-ls\n";
        assert_eq!(read_pinned_version(csharp, &runner), PinState::Wrong { installed: "0.19.2".to_string() });
        runner.listing = "Package Id  Version  Commands\n---\nother  1.0.0  other\n";
        assert_eq!(read_pinned_version(csharp, &runner), PinState::NotListed);
        assert_eq!(*runner.log.borrow(), vec!["dotnet tool list --global"; 3], "só a listagem roda");

        assert_eq!(read_pinned_version(rust, &runner), PinState::NotChecked, "rust não fixa versão");
        let without_dotnet = FakeRunner::new(&[]);
        assert_eq!(read_pinned_version(csharp, &without_dotnet), PinState::NotChecked);
        assert!(without_dotnet.log.borrow().is_empty());

        let mut stalled = FakeRunner::new(&["dotnet"]);
        stalled.timing_out.push("dotnet tool list");
        assert_eq!(
            read_pinned_version(csharp, &stalled),
            PinState::TimedOut { after: COMMAND_DEADLINE },
            "o prazo de cada comando vale também para a leitura"
        );
    }

    /// A etapa que roda dentro de um orçamento maior gasta no máximo o menor
    /// entre o teto dela (60 s) e o que resta dele, e o que gasta sai do que
    /// resta a quem a chamou. Com 90 s dos 100 já gastos, o primeiro comando
    /// recebe 10 s e o segundo, os 4 s que sobram depois de o primeiro gastar
    /// 6; com o orçamento intacto, recebe o teto de 60 s.
    #[test]
    fn a_step_inside_a_bigger_budget_spends_the_smaller_of_the_cap_and_what_is_left() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("go.mod"), "module x\n").unwrap();
        let model_path = crate::io::project_map::model_path(project.path());
        let whole = Duration::from_secs(100);

        let mut spent = FakeRunner::new(&["gopls", "claude"]);
        spent.taking.push(("claude plugin install", Duration::from_secs(6)));
        let outer = Budget::start(&spent, whole);
        spent.spent.set(Duration::from_secs(90));
        let warnings = ensure_code_tools_in(project.path(), &model_path, &outer);
        assert_eq!(*spent.limits.borrow(), vec![Duration::from_secs(10), Duration::from_secs(4)]);
        assert_eq!(warnings, Vec::new());
        assert_eq!(outer.remaining(), Duration::from_secs(4), "o que a etapa gastou saiu do orçamento de fora");

        let fresh = FakeRunner::new(&["gopls", "claude"]);
        let outer = Budget::start(&fresh, whole);
        ensure_code_tools_in(project.path(), &model_path, &outer);
        assert_eq!(*fresh.limits.borrow(), vec![STEP_BUDGET, STEP_BUDGET], "o teto da etapa continua em 60 s");
    }

    /// O orçamento devolve `None` sem rodar nada quando acabou, e a fatia dele
    /// nunca passa do que resta nem do teto.
    #[test]
    fn the_budget_does_not_run_the_work_when_it_runs_out_and_the_slice_respects_what_is_left() {
        let runner = FakeRunner::new(&[]);
        let budget = Budget::start(&runner, Duration::from_secs(10));
        assert_eq!(budget.within(Duration::from_secs(45), |limit| limit), Some(Duration::from_secs(10)));
        runner.spent.set(Duration::from_secs(7));
        assert_eq!(budget.within(Duration::from_secs(2), |limit| limit), Some(Duration::from_secs(2)));
        assert_eq!(budget.slice(Duration::from_secs(60)).total(), Duration::from_secs(3));
        assert_eq!(budget.slice(Duration::from_secs(1)).total(), Duration::from_secs(1));
        runner.spent.set(Duration::from_secs(10));
        let mut ran = false;
        assert_eq!(
            budget.within(Duration::from_secs(45), |_| {
                ran = true;
            }),
            None
        );
        assert!(!ran, "com o orçamento esgotado o trabalho nem começa");
        assert_eq!(budget.slice(Duration::from_secs(5)).total(), Duration::ZERO);
    }
}
