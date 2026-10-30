//! As ferramentas de que o `mustard init` precisa na máquina: o portão do rtk
//! e o aviso do ripgrep. As duas olham a MÁQUINA, por isso rodam de
//! `cli::dispatch`, nunca da metade de biblioteca do `init`. Nada é instalado
//! aqui: o ripgrep que falta e as ferramentas de código de cada linguagem
//! saem como comando pronto, e quem os roda é a pessoa.
//!
//! rtk itself comes with the installers, in a fixed version checked against
//! `checksums.txt`; `mustard init` only refuses to go on without it. Nothing
//! here writes rtk's own configuration: its hook lives in the project's
//! `.claude/settings.local.json`, written by the seed.
//!
//! [`complete_installation`] finishes through the native update door, sharing
//! its index, tool, plugin and version checks. Library seeding stays private
//! to the project; only the CLI performs machine preparation.

use std::path::Path;
use std::process::Command;

use mustard_core::platform::process;

/// Whether `rtk --version` succeeds (RTK reachable on PATH).
fn rtk_on_path() -> bool {
    Command::new("rtk")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Probe `rtk --version` and exit hard with install instructions when it
/// fails. RTK is a mandatory dependency: the harness prefixes Bash commands
/// with `rtk`, so a Mustard install without `rtk` on `PATH` would produce a
/// `.claude/` that cannot run. We abort before touching disk rather than
/// failing later in a confusing way.
///
/// This is **not** fail-open. The exit code is `1` so a script driving the
/// binary can detect the failure and surface it to the user. NOT library
/// callers: they never reach this function, which is the whole point of it
/// living in `cli::dispatch`. `pub(crate)` makes the compiler enforce that
/// rather than leaving it to this comment.
pub(crate) fn probe_rtk() {
    // Skip the hard gate under unit tests: a clean CI runner has no `rtk`, and a
    // `process::exit` here would kill the whole test process.
    //
    // That guard is narrower than it reads, which is why this function may only
    // be called from the BINARY's dispatch and never from the library: `cfg!(test)`
    // is true while this crate compiles its own unit tests and false everywhere
    // else — an integration test in another crate compiles this as an ordinary
    // dependency and gets the `exit(1)`. That is exactly how it died on CI's
    // first run of that crate.
    if cfg!(test) || rtk_on_path() {
        return;
    }
    eprintln!(
        "\nMustard requires RTK (Rust Token Killer) on PATH.\n\
         Could not run `rtk --version` — RTK is a mandatory dependency.\n\
         The Mustard installers bring it. Otherwise, download the release your system\n\
         needs from https://github.com/rtk-ai/rtk/releases, put `rtk` on PATH and\n\
         re-run `mustard init`.\n"
    );
    std::process::exit(1);
}

/// Diz o comando que instala o ripgrep (`rg`) quando ele falta, sem rodá-lo:
/// quem instala é a pessoa, e o `rg` que falta nunca trava o `init`.
///
/// Por quê: os filtros `grep`/`find` do RTK usam o `rg`. Sem ele, o RTK
/// imprime um aviso de troca em toda chamada, uns 50 tokens a mais em cada
/// saída do Bash.
pub(crate) fn ripgrep_hint() {
    // Nos testes de unidade, nenhum efeito de ferramenta externa.
    if cfg!(test) || rg_on_path() {
        return;
    }
    println!("  ripgrep not found - install it to silence the RTK `rg` fallback warning:");
    if cfg!(windows) {
        println!("    Windows: scoop install ripgrep");
        println!("         or: cargo install ripgrep");
    } else if cfg!(target_os = "macos") {
        println!("    macOS:   brew install ripgrep");
    } else {
        println!("    Linux:   apt install ripgrep | pacman -S ripgrep | dnf install ripgrep");
    }
}

/// Whether `rg --version` succeeds (ripgrep reachable on PATH).
fn rg_on_path() -> bool {
    process::command("rg")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The binary install and plugin update finish through the same native door.
/// Library callers keep the pure project-seeding contract; cancellation and
/// dry-run never reach this step.
pub(crate) fn complete_installation(root: &Path) -> anyhow::Result<()> {
    let file = if cfg!(windows) { "mustard-rt.exe" } else { "mustard-rt" };
    let sibling = std::env::current_exe()?.parent().map(|parent|parent.join(file));
    let Some(program) = sibling.filter(|path|path.is_file()) else {
        return incomplete(root,"native companion missing");
    };
    let version=Command::new(&program).arg("--version").env("MUSTARD_RT_DELEGATED","1").output()?;
    if !version.status.success() || !String::from_utf8_lossy(&version.stdout).split_whitespace().any(|part|part==mustard_core::harness_version()) {
        return incomplete(root,"native companion version differs from this CLI");
    }
    let output = Command::new(program).args(["run","upsert"])
        .env("MUSTARD_RT_DELEGATED","1").current_dir(root).output()?;
    anyhow::ensure!(output.status.success(), "native installation failed; run mustard-rt run upsert");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let receipt = report.get("installation").ok_or_else(||anyhow::anyhow!("native installation returned no verification; run mustard-rt run upsert"))?;
    println!("{}",serde_json::to_string_pretty(receipt)?);
    if let Some(warnings)=report["codeToolWarnings"].as_array() {
        for warning in warnings { if let Some(text)=warning.as_str() { println!("  {text}"); } }
    }
    Ok(())
}

fn incomplete(root:&Path,reason:&str)->anyhow::Result<()> {
    let receipt=serde_json::json!({"schema_version":1,"version":mustard_core::harness_version(),
        "complete":false,"project_ready":false,"host_ready":false,"provider_verified":false,
        "checks":[{"component":"native-installer","ready":false,"reason":reason,"remedy":"reinstall the complete Mustard bundle, then run mustard-rt run upsert"}]});
    let text=serde_json::to_string_pretty(&receipt)?;
    mustard_core::io::fs::write_atomic(root.join(".claude/mustard/installation.json"),text.as_bytes())?;
    println!("{text}");
    Ok(())
}
