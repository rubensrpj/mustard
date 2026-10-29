//! O programa e o plugin de cada linguagem — a mesma ideia que a instalação já
//! aplica ao ripgrep, feita uma vez para cada linguagem que
//! `apps/scan/languages.toml` conhece. `mustard init` e `mustard-rt run
//! doctor` leem esta MESMA tabela: quando uma linguagem nova ganha um plugin
//! no catálogo, ela entra aqui, e só aqui — nem em `apps/cli`, nem no
//! diagnóstico.
//!
//! O Mustard nunca instala o que a tabela pede: nada roda com `-g`,
//! `--global` ou `@latest` na máquina de quem programa. A instalação e o
//! diagnóstico leem a mesma lista do que falta ([`code_tool_hints`]), cada
//! item com o comando pronto, e quem decide rodá-lo é a pessoa. Quem roda as
//! conferências é o [`ToolRunner`] — a máquina, em [`MachineRunner`], ou um
//! executor falso no teste.
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
    // continua sem ele), mas é uma linguagem do projeto.
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

/// O que falta, nas linguagens de `languages` que a tabela conhece, para o
/// programa de cada uma responder, com o comando pronto que a pessoa roda: o
/// programa fora do `PATH` e o que está nele sem responder à conferência
/// ([`CodeTool::answers`]). O programa de duas linguagens conta uma vez. Só
/// lê: nada é instalado, e a instalação e o diagnóstico mostram a mesma lista.
#[must_use]
pub fn code_tool_hints(languages: &BTreeSet<String>, runner: &impl ToolRunner) -> Vec<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut hints = Vec::new();
    for tool in languages.iter().filter_map(|language| code_tool_for_language(language)) {
        if seen.contains(&tool.program) {
            continue;
        }
        seen.push(tool.program);
        if !runner.on_path(tool.program) {
            hints.push(format!("missing: {} (install: {})", tool.program, tool.install_cmd));
        } else if !tool.answers(runner) {
            hints.push(format!("not answering: {} (install: {})", tool.program, tool.install_cmd));
        }
    }
    hints
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
    /// Preserve protocol exit codes without treating an arbitrary failed
    /// process as successful. Missing/terminated processes have no exit code.
    fn output_with_exit(&self, program: &str, args: &[&str], limit: Duration) -> (RunOutcome, Option<i32>, String) {
        let (outcome,text)=self.output(program,args,limit);
        (outcome,(outcome==RunOutcome::Succeeded).then_some(0),text)
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

    /// Roda o comando dentro do que resta do orçamento, devolvendo também a
    /// saída dele. Com o orçamento esgotado o comando nem roda, e o resultado é
    /// o de quem passou do prazo.
    fn output(&self, program: &str, args: &[&str]) -> (RunOutcome, String) {
        self.within(self.runner.command_deadline(), |limit| self.runner.output(program, args, limit))
            .unwrap_or((RunOutcome::TimedOut { after: self.total }, String::new()))
    }

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
    cwd: Option<PathBuf>,
    native_runtime: bool,
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
            cwd: None,
            native_runtime: false,
            home: home_dir(),
            windows: cfg!(windows),
        }
    }

    /// Set the subprocess directory without changing the caller's directory.
    #[must_use]
    pub fn at(mut self, cwd: &Path) -> Self {
        self.cwd = Some(cwd.to_path_buf());
        self
    }

    /// Inspect the actual companion, without handing over to a development
    /// build or another installed runtime. Does not change the caller's env.
    #[must_use]
    pub fn without_runtime_delegation(mut self) -> Self {
        self.native_runtime = true;
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

    /// Roda o arquivo que o `PATH` do executor tem para `program` (no
    /// Windows, o `npm.cmd` do `npm`); sem ele, o nome puro, que falha como
    /// antes. O comando que passa de `limit` é morto e a espera acaba na hora.
    /// A saída só é lida quando `capture` pede: sem isso, nada a prende.
    fn spawn_and_wait(&self, program: &str, args: &[&str], limit: Duration, capture: bool) -> (RunOutcome, Option<i32>, String) {
        let file = program_file(program, self.windows, &self.path_env).unwrap_or_else(|| PathBuf::from(program));
        let mut command = Command::new(file);
        if let Some(cwd) = &self.cwd { command.current_dir(cwd); }
        if self.native_runtime { command.env("MUSTARD_RT_DELEGATED", "1"); }
        let Ok(mut child) = command.args(args)
            .env("PATH", &self.path_env)
            .stdin(Stdio::null())
            .stdout(if capture { Stdio::piped() } else { Stdio::null() })
            .stderr(Stdio::null())
            .spawn()
        else {
            return (RunOutcome::Failed, None, String::new());
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
        let (outcome,exit_code) = loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break (RunOutcome::Succeeded,status.code()),
                Ok(Some(status)) => break (RunOutcome::Failed,status.code()),
                Ok(None) if Instant::now() < limit_at => std::thread::sleep(POLL_INTERVAL),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (RunOutcome::TimedOut { after: limit }, None, String::new());
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (RunOutcome::Failed, None, String::new());
                }
            }
        };
        let text = output.and_then(|receiver| receiver.recv_timeout(OUTPUT_GRACE).ok()).unwrap_or_default();
        (outcome, exit_code, text)
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
        let (outcome,_,text)=self.spawn_and_wait(program, args, limit, true);
        (outcome,text)
    }

    fn output_with_exit(&self, program: &str, args: &[&str], limit: Duration) -> (RunOutcome, Option<i32>, String) {
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
    /// manifestos já são.
    #[test]
    fn detect_project_languages_finds_dart_by_its_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: app\n").unwrap();
        assert!(detect_project_languages(dir.path()).contains(&"dart"), "{:?}", detect_project_languages(dir.path()));
    }

    /// A lista do que falta traz o comando de instalação de cada programa
    /// fora do `PATH`, uma vez por programa, e não roda comando nenhum: o
    /// servidor de TypeScript serve ao JavaScript também.
    #[test]
    fn the_hints_name_the_install_command_without_running_it() {
        let languages: BTreeSet<String> = ["rust", "typescript", "javascript"].map(String::from).into();
        let runner = FakeRunner::new(&[]);
        let hints = code_tool_hints(&languages, &runner);
        let program = |language: &str| code_tool_for_language(language).expect("in the table");
        let expected: Vec<String> = ["javascript", "rust"]
            .iter()
            .map(|language| format!("missing: {} (install: {})", program(language).program, program(language).install_cmd))
            .collect();
        assert_eq!(hints, expected);
        assert!(runner.log.borrow().is_empty(), "nothing ran: {:?}", runner.log.borrow());
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
        plugins: &'static str,
        off_path: Vec<(&'static str, &'static str)>,
        log: std::cell::RefCell<Vec<String>>,
        budget: Duration,
        command_deadline: Duration,
        limits: std::cell::RefCell<Vec<Duration>>,
        clock: crate::fake_clock::FakeClock,
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
                plugins: "",
                off_path: Vec::new(),
                log: std::cell::RefCell::new(Vec::new()),
                budget: STEP_BUDGET,
                command_deadline: COMMAND_DEADLINE,
                limits: std::cell::RefCell::new(Vec::new()),
                clock: crate::fake_clock::FakeClock::default(),
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
                self.clock.advance(limit);
                self.log.borrow_mut().push(line);
                return RunOutcome::TimedOut { after: limit };
            }
            self.clock.advance(cost);
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
            let text = if outcome != RunOutcome::Succeeded { String::new() }
                else if listed { self.listing.to_string() }
                else if program == "claude" && args == ["plugin","list","--json"] { self.plugins.to_string() }
                else { String::new() };
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
            self.clock.now()
        }
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

    #[test]
    fn a_project_query_sets_only_the_child_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Command::new("git").args(["init", "-q"]).current_dir(dir.path()).status().unwrap().success());
        let parent = std::env::current_dir().unwrap();
        let runner = MachineRunner::new(&std::env::var("PATH").unwrap_or_default()).at(dir.path());
        let (outcome, output) = runner.output("git", &["rev-parse", "--show-toplevel"], Duration::from_secs(3));
        assert_eq!(outcome, RunOutcome::Succeeded);
        assert_eq!(Path::new(output.trim()).canonicalize().unwrap(), dir.path().canonicalize().unwrap());
        assert_eq!(std::env::current_dir().unwrap(), parent);
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
            cwd: None,
            native_runtime: false,
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
            cwd: None,
            native_runtime: false,
            home: Some(linux_home.path().to_path_buf()),
            windows: false,
        };
        assert_eq!(linux.found_off_path("rg"), Some(linux_bin.join("rg")));
    }

    /// O comando de shell que fica preso até `release` existir — ou até o
    /// limite de segurança das esperas dos testes, para nunca sobrar — e
    /// então cria `ended`.
    #[cfg(unix)]
    fn held_until(release: &Path, ended: &Path) -> String {
        format!("{}; touch '{}'", crate::held_command::held_until_released(release), ended.display())
    }

    /// O executor da máquina corta o comando que passa do prazo e volta
    /// enquanto ele ainda está preso, em vez de esperar o fim dele; o que
    /// acaba antes disso volta com o resultado de sempre.
    #[test]
    #[cfg(unix)]
    fn the_machine_runner_cuts_the_command_that_passes_its_deadline() {
        let path = std::env::var("PATH").unwrap_or_default();
        let runner = MachineRunner::new(&path);
        let dir = tempfile::tempdir().unwrap();
        let (release, ended) = (dir.path().join("release"), dir.path().join("ended"));

        let limit = Duration::from_millis(300);
        let outcome = runner.run_outcome("sh", &["-c", &held_until(&release, &ended)], limit);
        assert_eq!(outcome, RunOutcome::TimedOut { after: Duration::from_millis(300) });
        assert!(!ended.exists(), "the call came back while the command was still held");
        std::fs::write(&release, "").unwrap();

        let ample = Duration::from_secs(60);
        assert_eq!(runner.run_outcome("sh", &["-c", "exit 0"], ample), RunOutcome::Succeeded);
        assert_eq!(runner.run_outcome("sh", &["-c", "exit 3"], ample), RunOutcome::Failed);
        assert_eq!(runner.run_outcome("no-such-program-here", &[], ample), RunOutcome::Failed);
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

        // Quem segura a saída é um processo que o comando abriu, e que fica
        // vivo depois de o comando ser morto.
        let dir = tempfile::tempdir().unwrap();
        let (release, ended) = (dir.path().join("release"), dir.path().join("ended"));
        let held = format!("echo partial; ({})", held_until(&release, &ended));
        let (outcome, text) = runner.output("sh", &["-c", &held], Duration::from_millis(300));
        assert_eq!(outcome, RunOutcome::TimedOut { after: Duration::from_millis(300) });
        assert_eq!(text, "");
        assert!(!ended.exists(), "the call came back while the output was still held");
        std::fs::write(&release, "").unwrap();
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

    /// O orçamento devolve `None` sem rodar nada quando acabou, e a fatia dele
    /// nunca passa do que resta nem do teto.
    #[test]
    fn the_budget_does_not_run_the_work_when_it_runs_out_and_the_slice_respects_what_is_left() {
        let runner = FakeRunner::new(&[]);
        let budget = Budget::start(&runner, Duration::from_secs(10));
        assert_eq!(budget.within(Duration::from_secs(45), |limit| limit), Some(Duration::from_secs(10)));
        runner.clock.advance(Duration::from_secs(7));
        assert_eq!(budget.within(Duration::from_secs(2), |limit| limit), Some(Duration::from_secs(2)));
        assert_eq!(budget.slice(Duration::from_secs(60)).total(), Duration::from_secs(3));
        assert_eq!(budget.slice(Duration::from_secs(1)).total(), Duration::from_secs(1));
        runner.clock.advance(Duration::from_secs(3));
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
