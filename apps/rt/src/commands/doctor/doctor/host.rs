//! O que a máquina tem instalado: o programa `claude` no PATH, o servidor de
//! linguagem de cada pilha que o projeto usa (pela mesma tabela que
//! `mustard init` usa para instalar), a cópia certa de `rtk` no PATH e uma
//! Nerd Font para os temas da barra de status. Nenhuma dessas faltas trava:
//! cada uma vira aviso com o comando que instala.

use std::path::{Path, PathBuf};

use mustard_core::domain::config::DEFAULT_AGENT_EFFORT;
use mustard_core::io::fs;
use mustard_core::io::sha256::Sha256;
use mustard_core::platform::code_tools::{
    code_tool_for_language, read_pinned_version, CodeTool, MachineRunner, PinState, ToolRunner,
};

use super::{CheckResult, Status};

/// Probe for the `claude` CLI binary and report its resolved path.
///
/// Searches `PATH` the same way the OS would, also probing `.cmd` / `.bat`
/// wrappers on Windows. Produces `OK` when found, `WARN` when absent (the
/// scan cold-path falls back to the agnostic floor without blocking).
pub(super) fn check_claude_cli() -> CheckResult {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ';' } else { ':' };

    for dir in path_var.split(sep) {
        let base = std::path::Path::new(dir).join("claude");
        if base.exists() {
            let p = base.to_string_lossy().into_owned();
            return CheckResult { name: "claude_cli", status: Status::Ok, details: vec![p] };
        }
        // Windows: try .cmd / .bat / .exe extensions.
        #[cfg(windows)]
        for ext in [".cmd", ".bat", ".exe"] {
            let candidate = std::path::Path::new(dir).join(format!("claude{ext}"));
            if candidate.exists() {
                let p = candidate.to_string_lossy().into_owned();
                return CheckResult { name: "claude_cli", status: Status::Ok, details: vec![p] };
            }
        }
    }

    CheckResult::warn(
        "claude_cli",
        vec![
            "claude CLI not found on PATH — scan cold-path will fall back to the agnostic floor."
                .to_string(),
            "fix: install Claude Code (https://claude.ai/code) and ensure the binary is on PATH"
                .to_string(),
        ],
    )
}

/// Check that each detected language's code-tool program is present on
/// `PATH` and answers — the table `packages/core/src/platform/code_tools.rs`
/// shares with `mustard init`, so a language a plugin can drive here is the
/// same one the install tried to set up, checked the same way, through the
/// machine's runner.
pub(super) fn lsp_check(project_dir: &Path) -> CheckResult {
    let path_var = std::env::var("PATH").unwrap_or_default();
    lsp_verdict(project_dir, &MachineRunner::new(&path_var))
}

/// O veredito de [`lsp_check`] com quem roda os comandos (`runner`): o
/// programa que falta e o que está no `PATH` sem responder à conferência da
/// tabela ([`CodeTool::answers`]) viram aviso, com o comando que instala; o
/// que está instalado em outra versão que não a fixada na tabela também, com o
/// comando que a atualiza. A versão é lida pela mesma função da instalação
/// ([`read_pinned_version`]), só lendo: o diagnóstico nunca atualiza nada.
fn lsp_verdict(project_dir: &Path, runner: &impl ToolRunner) -> CheckResult {
    let model_path = mustard_core::io::project_map::model_path(project_dir);
    let languages = mustard_core::platform::code_tools::detect_code_languages(project_dir, &model_path);

    // Only languages the catalog maps to a program, deduplicated by binary
    // (typescript + javascript both map to the same server).
    let mapped: Vec<(&str, &CodeTool)> = languages
        .iter()
        .filter_map(|lang| code_tool_for_language(lang).map(|tool| (lang.as_str(), tool)))
        .collect();

    if mapped.is_empty() {
        return CheckResult::skip("lsp", "no mapped stacks detected");
    }

    let mut seen_bins: Vec<&str> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for (_, tool) in &mapped {
        let (bin, hint) = (tool.program, tool.install_cmd);
        if seen_bins.contains(&bin) {
            continue;
        }
        seen_bins.push(bin);
        if !runner.on_path(bin) {
            missing.push(format!("missing: {bin} (install: {hint})"));
        } else if !tool.answers(runner) {
            missing.push(format!("not answering: {bin} (install: {hint})"));
        }
        if let (PinState::Wrong { installed }, Some(pin)) = (read_pinned_version(tool, runner), &tool.pin) {
            missing.push(format!(
                "wrong version: {bin} {installed}, {} is the one that works (update: {})",
                pin.version, pin.update_cmd
            ));
        }
    }

    if missing.is_empty() {
        CheckResult::ok("lsp")
    } else {
        CheckResult::warn("lsp", missing)
    }
}

/// A primeira cópia de `rtk` (ou `rtk.exe` no Windows) achada em `path_var`,
/// na ordem do `PATH` — a que o terminal do usuário roda. O agente não a usa:
/// o gancho chama a cópia do Mustard pelo caminho inteiro.
fn first_rtk_on_path(path_var: &str) -> Option<PathBuf> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let name = if cfg!(windows) { "rtk.exe" } else { "rtk" };
    for dir in path_var.split(sep) {
        let candidate = Path::new(dir).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// A saída de `rtk --version` de `path`, ou `None` quando o comando não roda.
fn rtk_version_output(path: &Path) -> Option<String> {
    let output = std::process::Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// A soma SHA-256 do conteúdo de `path`, ou `None` quando ele não se lê.
fn content_sum(path: &Path) -> Option<String> {
    let mut sum = Sha256::new();
    sum.update(&std::fs::read(path).ok()?);
    Some(sum.hex_digest())
}

/// Uma cópia de `rtk` como o aviso a mostra: o caminho, a linha de
/// `--version` e o começo da soma do conteúdo.
fn rtk_copy(path: &Path, version: Option<&str>, sum: Option<&str>) -> String {
    format!(
        "{} ({}, sha256 {})",
        path.display(),
        version.unwrap_or("no --version reply"),
        sum.map_or("unreadable", |sum| &sum[..sum.len().min(12)])
    )
}

/// O fim dos avisos sobre o `rtk` do `PATH`: o gancho troca o comando do
/// agente pela cópia do Mustard, chamada pelo caminho inteiro, e sem cópia não
/// troca nada; o `rtk` do `PATH` só roda no terminal do usuário.
fn terminal_only(bundled: Option<&Path>) -> &'static str {
    match bundled {
        Some(_) => "the agent's commands run Mustard's copy by its full path, never the rtk on PATH, \
                    so this only matters for rtk typed in your terminal",
        None => "the agent's commands never run the rtk on PATH, so this only matters for rtk typed \
                 in your terminal",
    }
}

/// O veredito do que [`check_rtk`] observou. Separado da leitura do `PATH` e
/// do `current_exe()` para que o teste possa afirmar as regras sem depender do
/// que esta máquina tem instalado — o mesmo motivo de `append_missing` em
/// `apps/rt/src/shared/proc.rs`. `version_of` dá a linha de `--version` de
/// uma cópia.
///
/// `bundled` e `first_on_path` já chegam canonizados. O `rtk` do PATH é a
/// cópia do Mustard quando é o mesmo caminho, o mesmo conteúdo byte a byte ou
/// responde a mesma versão: o do pacote instalado e o do plugin são o mesmo
/// programa em pastas diferentes. Só uma cópia de outro conteúdo e de outra
/// versão vira aviso, com as duas. Todo aviso sobre o `rtk` do PATH diz que
/// ele só vale para o terminal do usuário.
fn rtk_verdict(bundled: Option<&Path>, first_on_path: Option<&Path>, version_of: &dyn Fn(&Path) -> Option<String>) -> CheckResult {
    let Some(first_on_path) = first_on_path else {
        return CheckResult::warn("rtk", vec!["rtk not found on PATH".to_string()]);
    };
    let version_output = version_of(first_on_path);

    let mut warnings = Vec::new();

    if let Some(bundled) = bundled
        && bundled != first_on_path {
            let (first_sum, bundled_sum) = (content_sum(first_on_path), content_sum(bundled));
            let same_content = first_sum.is_some() && first_sum == bundled_sum;
            let bundled_version = if same_content { None } else { version_of(bundled) };
            let same_version = bundled_version.is_some() && bundled_version == version_output;
            if !same_content && !same_version {
                warnings.push(format!(
                    "the rtk PATH finds first is not Mustard's own copy: {} — Mustard's copy: {} — \
                     {}; put Mustard's copy ahead on PATH to use it there too",
                    rtk_copy(first_on_path, version_output.as_deref(), first_sum.as_deref()),
                    rtk_copy(bundled, bundled_version.as_deref(), bundled_sum.as_deref()),
                    terminal_only(Some(bundled))
                ));
            }
        }

    match version_output.as_deref() {
        Some(out) if out.starts_with("rtk ") => {}
        Some(out) => warnings.push(format!(
            "`rtk --version` did not start with \"rtk \": {out:?} — another program named rtk is \
             first on PATH; {}",
            terminal_only(bundled)
        )),
        None => warnings.push("could not run `rtk --version`".to_string()),
    }

    if warnings.is_empty() {
        CheckResult::ok("rtk")
    } else {
        CheckResult::warn("rtk", warnings)
    }
}

/// Confere que o `rtk` que o PATH acha primeiro é a cópia do Mustard, e que
/// `rtk --version` responde como o rtk de verdade — em 19/09, um pacote do
/// npm chamado `rtk` ficou na frente e desligou o filtro sem aviso. Hoje o
/// agente roda a cópia do Mustard pelo caminho inteiro, e o aviso só vale
/// para o terminal do usuário. A cópia é a que o gancho roda: ao lado do
/// binário ou, sem ela, a da pasta `bin` do plugin instalado; sem nenhuma das
/// duas, confere só a resposta.
pub(super) fn check_rtk() -> CheckResult {
    let exe = std::env::current_exe().ok();
    let plugin_dir = || mustard_core::platform::harness::installed_plugin().map(|plugin| plugin.dir);
    rtk_check(exe.as_deref(), plugin_dir, &std::env::var("PATH").unwrap_or_default(), &rtk_version_output)
}

/// O que [`check_rtk`] observa, com o binário em execução (`exe`), a pasta do
/// plugin instalado (`plugin_dir`), o `PATH` (`path_var`) e a resposta de
/// `--version` (`version_of`) dados por quem chama, para que o teste os
/// injete. A cópia do Mustard sai da mesma regra do gancho
/// ([`rtk_swap::bundled_at`](crate::hooks::bash::rtk_swap::bundled_at)); o
/// binário, a cópia e o `rtk` do PATH vão pelo caminho real (o pacote `.deb`
/// põe o binário em `/usr/lib/mustard/bin`, e `/usr/bin/mustard-rt` é um
/// atalho para lá).
fn rtk_check(
    exe: Option<&Path>,
    plugin_dir: impl FnOnce() -> Option<PathBuf>,
    path_var: &str,
    version_of: &dyn Fn(&Path) -> Option<String>,
) -> CheckResult {
    let real = |path: PathBuf| std::fs::canonicalize(&path).unwrap_or(path);
    let exe = exe.map(|exe| real(exe.to_path_buf()));
    let bundled = crate::hooks::bash::rtk_swap::bundled_at(exe.as_deref(), plugin_dir).map(real);
    let first = first_rtk_on_path(path_var).map(real);
    rtk_verdict(bundled.as_deref(), first.as_deref(), version_of)
}

/// Probe OS font directories for *any* Nerd Font (filename containing both a
/// font-family-ish token and "nerd" or "nf-"). WARN when none is found, since
/// the powerline statusline themes need one.
///
/// Fail-open: read errors degrade to "not detected" (WARN) rather than
/// blocking the doctor run.
pub(super) fn check_nerd_font() -> CheckResult {
    // A mesma lista de pastas que o instalador de fonte confere.
    let dirs = mustard_core::platform::fonts::font_dirs();
    if dirs.iter().any(|d| scan_for_any_nerd_font(d)) {
        return CheckResult::ok("nerd-font");
    }
    // Linux: fontconfig is authoritative if the binary is on PATH.
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = std::process::Command::new("fc-list").output()
            && output.status.success() {
                let listing = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
                if listing.contains("nerd") {
                    return CheckResult::ok("nerd-font");
                }
            }
    }
    CheckResult::warn(
        "nerd-font",
        vec![
            "no Nerd Font detected on this host — powerline statusline themes will render \
             tofu (□) instead of separator arrows."
                .to_string(),
            "fix: run `mustard install-nerd-font` (default JetBrainsMono)".to_string(),
            "or set MUSTARD_STATUSLINE_THEME=default (pipe-only, no Nerd Font needed)"
                .to_string(),
        ],
    )
}

/// The effort `CLAUDE_CODE_EFFORT_LEVEL` pins beats the one in the agent's
/// header and the one in the launch: set below the highest, every Mustard
/// agent runs at it. WARN then; unset or at the highest is OK.
pub(super) fn check_effort_env() -> CheckResult {
    effort_env_verdict(std::env::var("CLAUDE_CODE_EFFORT_LEVEL").ok().as_deref())
}

fn effort_env_verdict(value: Option<&str>) -> CheckResult {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if !value.eq_ignore_ascii_case(DEFAULT_AGENT_EFFORT) => CheckResult::warn(
            "effort-env",
            vec![
                format!(
                    "CLAUDE_CODE_EFFORT_LEVEL={value} overrides every agent's effort; \
                     the Mustard agents are meant to run at {DEFAULT_AGENT_EFFORT}."
                ),
                "fix: unset CLAUDE_CODE_EFFORT_LEVEL before starting Claude Code".to_string(),
            ],
        ),
        _ => CheckResult::ok("effort-env"),
    }
}

/// One level + immediate subdirectories. Match any file whose lowercased
/// name contains "nerd" or "nf-".
fn scan_for_any_nerd_font(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries {
        let name = entry.file_name.to_ascii_lowercase();
        if name.contains("nerd") || name.contains("nf-") {
            return true;
        }
        if entry.is_dir
            && let Ok(sub) = fs::read_dir(&entry.path) {
                for s in sub {
                    let sn = s.file_name.to_ascii_lowercase();
                    if sn.contains("nerd") || sn.contains("nf-") {
                        return true;
                    }
                }
            }
    }
    false
}

#[cfg(test)]
mod tests {
    use mustard_core::platform::code_tools::RunOutcome;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn an_effort_variable_below_the_highest_warns() {
        for quiet in [None, Some(""), Some("xhigh"), Some(" XHIGH ")] {
            assert_eq!(effort_env_verdict(quiet).status, Status::Ok, "{quiet:?}");
        }
        let warned = effort_env_verdict(Some("medium"));
        assert_eq!(warned.status, Status::Warn);
        assert!(warned.details.iter().any(|line| line.contains("medium") && line.contains("CLAUDE_CODE_EFFORT_LEVEL")), "{:?}", warned.details);
    }

    #[test]
    fn lsp_check_skips_with_no_mapped_stacks() {
        let dir = tempdir().unwrap();
        // Empty directory: no manifest files → no mapped stacks → Skip.
        let result = lsp_check(dir.path());
        assert_eq!(result.status, Status::Skip, "{:?}", result.details);
    }

    /// A tabela partilhada com `mustard init`
    /// (`packages/core/src/platform/code_tools.rs`) chega até aqui: um
    /// `Cargo.toml` faz o rust entrar no mapeamento, então o resultado nunca é
    /// `Skip` — a divisa entre "nada mapeado" e "rust mapeado".
    #[test]
    fn lsp_check_maps_rust_via_the_shared_code_tool_table() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let result = lsp_check(dir.path());
        assert_ne!(result.status, Status::Skip, "{:?}", result.details);
    }

    /// Executor falso do diagnóstico: responde que cada programa de
    /// `on_path` está no `PATH`, anota cada comando pedido e faz falhar o que
    /// começa por `failing`.
    struct Answering {
        on_path: Vec<&'static str>,
        failing: &'static str,
        ran: std::cell::RefCell<Vec<String>>,
        /// O que a listagem do gerenciador escreve, e as linhas de comando que
        /// pediram a listagem.
        listing: &'static str,
        listed: std::cell::RefCell<Vec<String>>,
    }

    impl ToolRunner for Answering {
        fn on_path(&self, program: &str) -> bool {
            self.on_path.contains(&program)
        }

        fn run(&self, program: &str, _: &[&str]) -> bool {
            self.ran.borrow_mut().push(program.to_string());
            self.on_path(program) && program != self.failing
        }

        fn output(&self, program: &str, args: &[&str], _: std::time::Duration) -> (RunOutcome, String) {
            self.listed.borrow_mut().push(std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" "));
            (RunOutcome::Succeeded, self.listing.to_string())
        }

        fn found_off_path(&self, _: &str) -> Option<PathBuf> {
            None
        }
    }

    /// O servidor de TypeScript no `PATH` cuja conferência falha — o
    /// TypeScript que ele acha não traz o `tsserver.js` — vira aviso no
    /// diagnóstico, com o comando que instala; com a conferência passando, o
    /// diagnóstico fica verde.
    #[test]
    fn lsp_check_warns_when_the_program_is_present_but_does_not_answer() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"devDependencies":{"typescript":"7.0.2"}}"#).unwrap();
        let tool = code_tool_for_language("typescript").unwrap();

        let silent = Answering {
            on_path: vec!["typescript-language-server", "node"],
            failing: "node",
            ran: Default::default(),
            listing: "",
            listed: Default::default(),
        };
        let result = lsp_verdict(dir.path(), &silent);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let expected = format!("not answering: typescript-language-server (install: {})", tool.install_cmd);
        assert_eq!(result.details, vec![expected]);
        assert_eq!(*silent.ran.borrow(), vec!["node"], "the check ran once");

        let answering = Answering {
            on_path: vec!["typescript-language-server", "node"],
            failing: "",
            ran: Default::default(),
            listing: "",
            listed: Default::default(),
        };
        let result = lsp_verdict(dir.path(), &answering);
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    /// O `csharp-ls` instalado em outra versão que não a 0.18.0 vira aviso no
    /// diagnóstico, com a linha que o atualiza, mesmo respondendo; na versão
    /// certa, o diagnóstico fica verde, e sem o dotnet nem chega a listar. O
    /// diagnóstico só lê: o único comando que ele pede é a listagem.
    #[test]
    fn lsp_check_warns_when_the_installed_version_is_not_the_pinned_one() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("App.csproj"), "<Project/>\n").unwrap();
        let update = "dotnet tool update --global csharp-ls --version 0.18.0";

        let wrong = Answering {
            on_path: vec!["csharp-ls", "dotnet"],
            failing: "",
            ran: Default::default(),
            listing: "Package Id  Version  Commands\n---\ncsharp-ls  0.19.2  csharp-ls\n",
            listed: Default::default(),
        };
        let result = lsp_verdict(dir.path(), &wrong);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        assert_eq!(
            result.details,
            vec![format!("wrong version: csharp-ls 0.19.2, 0.18.0 is the one that works (update: {update})")]
        );
        assert_eq!(*wrong.listed.borrow(), vec!["dotnet tool list --global"], "só a listagem foi pedida");
        assert!(wrong.ran.borrow().is_empty(), "nada mais rodou: {:?}", wrong.ran.borrow());

        let right = Answering {
            on_path: vec!["csharp-ls", "dotnet"],
            failing: "",
            ran: Default::default(),
            listing: "Package Id  Version  Commands\n---\ncsharp-ls  0.18.0  csharp-ls\n",
            listed: Default::default(),
        };
        assert_eq!(lsp_verdict(dir.path(), &right).status, Status::Ok);

        let without_dotnet = Answering {
            on_path: vec!["csharp-ls"],
            failing: "",
            ran: Default::default(),
            listing: "csharp-ls  0.19.2  csharp-ls\n",
            listed: Default::default(),
        };
        assert_eq!(lsp_verdict(dir.path(), &without_dotnet).status, Status::Ok);
        assert!(without_dotnet.listed.borrow().is_empty(), "sem dotnet, nada é listado");
    }

    /// A linha de `--version` que toda cópia responde nos testes.
    fn answers(version: &'static str) -> impl Fn(&Path) -> Option<String> {
        move |_| Some(version.to_string())
    }

    #[test]
    fn rtk_verdict_ok_when_the_first_on_path_is_mustards_own_copy() {
        let bundled = PathBuf::from("/usr/lib/mustard/bin/rtk");
        let result = rtk_verdict(Some(&bundled), Some(&bundled), &answers("rtk 0.49.0"));
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    /// O caso de 14/10: o PATH acha o rtk do pacote instalado e o Mustard que
    /// roda traz o dele em outra pasta, byte a byte igual. É a mesma cópia, e
    /// a do Mustard nem precisa responder.
    #[test]
    fn rtk_verdict_accepts_another_path_with_the_same_content() {
        let dir = tempdir().unwrap();
        let (bundled, packaged) = (dir.path().join("plugin-rtk"), dir.path().join("package-rtk"));
        std::fs::write(&bundled, "rtk 0.49.0 binary").unwrap();
        std::fs::write(&packaged, "rtk 0.49.0 binary").unwrap();
        let only_the_path_one = |path: &Path| (path == packaged).then(|| "rtk 0.49.0".to_string());
        let result = rtk_verdict(Some(&bundled), Some(&packaged), &only_the_path_one);
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    /// Conteúdos diferentes: a mesma versão ainda é a cópia do Mustard; outra
    /// versão — o caso de 19/09, um pacote de outro catálogo com o mesmo nome
    /// `rtk` à frente do PATH — vira aviso que diz as duas cópias.
    #[test]
    fn rtk_verdict_with_different_contents_warns_only_for_another_version() {
        let dir = tempdir().unwrap();
        let (bundled, other) = (dir.path().join("plugin-rtk"), dir.path().join("npm-rtk"));
        std::fs::write(&bundled, "rtk 0.49.0 binary").unwrap();
        std::fs::write(&other, "rtk 0.49.0 another build").unwrap();
        let same = rtk_verdict(Some(&bundled), Some(&other), &answers("rtk 0.49.0"));
        assert_eq!(same.status, Status::Ok, "{:?}", same.details);

        let versions = |path: &Path| Some(if path == bundled { "rtk 0.49.0" } else { "rtk 0.31.0" }.to_string());
        let result = rtk_verdict(Some(&bundled), Some(&other), &versions);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let warning = &result.details[0];
        for said in [
            other.display().to_string(),
            bundled.display().to_string(),
            "rtk 0.31.0".into(),
            "rtk 0.49.0".into(),
            "the agent's commands run Mustard's copy by its full path, never the rtk on PATH".into(),
            "only matters for rtk typed in your terminal".into(),
        ] {
            assert!(warning.contains(&said), "the warning does not say {said}: {warning}");
        }
    }

    /// Outro programa chamado `rtk` na frente do PATH não muda o que o agente
    /// roda: o aviso diz que só o terminal do usuário o acha.
    #[test]
    fn rtk_verdict_warns_when_the_version_reply_does_not_start_with_rtk_space() {
        let bundled = PathBuf::from("/usr/lib/mustard/bin/rtk");
        let result = rtk_verdict(Some(&bundled), Some(&bundled), &answers("Rust Type Kit 1.0.0"));
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let warning = &result.details[0];
        assert!(warning.contains("\"Rust Type Kit 1.0.0\""), "{warning}");
        assert!(warning.contains("the agent's commands run Mustard's copy by its full path, never the rtk on PATH"), "{warning}");
        assert!(warning.contains("only matters for rtk typed in your terminal"), "{warning}");
        assert!(!warning.contains("ahead of Mustard's own copy"), "{warning}");
    }

    /// Sem cópia do Mustard, nem ao lado do binário nem no plugin instalado,
    /// o veredito confere só a resposta de `rtk --version` — não há caminho de
    /// cópia para comparar.
    #[test]
    fn rtk_verdict_without_a_bundled_copy_checks_only_the_response() {
        let path = PathBuf::from("/usr/bin/rtk");
        let ok = rtk_verdict(None, Some(&path), &answers("rtk 0.49.0"));
        assert_eq!(ok.status, Status::Ok, "{:?}", ok.details);

        let warn = rtk_verdict(None, Some(&path), &answers("not rtk at all"));
        assert_eq!(warn.status, Status::Warn);
        let warning = &warn.details[0];
        assert!(warning.contains("the agent's commands never run the rtk on PATH, so this only matters for rtk typed in your terminal"), "{warning}");
        assert!(!warning.contains("Mustard's copy"), "without a copy the warning names none: {warning}");
    }

    /// Sem cópia ao lado do binário — a compilação de desenvolvimento —, o
    /// diagnóstico acha a cópia na pasta `bin` do plugin instalado, a mesma
    /// que o gancho roda, e compara o `rtk` do PATH com ela: outra versão vira
    /// aviso com as duas. Sem a do plugin, só confere a resposta. A cópia ao
    /// lado do binário vence, e então o plugin nem é procurado.
    #[test]
    fn rtk_verdict_compares_the_path_rtk_with_the_plugin_copy_when_none_sits_beside_the_binary() {
        let dir = tempdir().unwrap();
        let name = if cfg!(windows) { "rtk.exe" } else { "rtk" };
        let (build, plugin, path_dir) = (dir.path().join("target/debug"), dir.path().join("plugin"), dir.path().join("path"));
        for folder in [&build, &plugin.join("bin"), &path_dir] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let exe = build.join("mustard-rt");
        std::fs::write(&exe, "mustard-rt").unwrap();
        std::fs::write(plugin.join("bin").join(name), "rtk 0.49.0 binary").unwrap();
        std::fs::write(path_dir.join(name), "rtk 0.31.0 npm build").unwrap();
        let path_var = path_dir.to_string_lossy().into_owned();
        let versions = |path: &Path| Some(if path.starts_with(dir.path().canonicalize().unwrap().join("path")) { "rtk 0.31.0" } else { "rtk 0.49.0" }.to_string());

        let result = rtk_check(Some(&exe), || Some(plugin.clone()), &path_var, &versions);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let warning = &result.details[0];
        let plugin_copy = plugin.join("bin").join(name).canonicalize().unwrap();
        for said in [plugin_copy.display().to_string(), "rtk 0.31.0".into(), "rtk 0.49.0".into()] {
            assert!(warning.contains(&said), "the warning does not say {said}: {warning}");
        }

        let without_plugin = rtk_check(Some(&exe), || None, &path_var, &versions);
        assert_eq!(without_plugin.status, Status::Ok, "{:?}", without_plugin.details);

        std::fs::write(build.join(name), "rtk 0.31.0 npm build").unwrap();
        let beside = rtk_check(Some(&exe), || unreachable!("the plugin is looked up only without a copy beside the binary"), &path_var, &versions);
        assert_eq!(beside.status, Status::Ok, "{:?}", beside.details);
    }

    #[test]
    fn rtk_verdict_warns_when_rtk_is_not_on_path_at_all() {
        let bundled = PathBuf::from("/usr/lib/mustard/bin/rtk");
        let result = rtk_verdict(Some(&bundled), None, &|_| None);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
    }
}
