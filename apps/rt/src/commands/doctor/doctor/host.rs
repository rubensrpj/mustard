//! O que a máquina tem instalado: o programa `claude` no PATH, o servidor de
//! linguagem de cada pilha que o projeto usa (pela mesma tabela que
//! `mustard init` usa para instalar), a cópia certa de `rtk` no PATH e uma
//! Nerd Font para os temas da barra de status. Nenhuma dessas faltas trava:
//! cada uma vira aviso com o comando que instala.

use std::path::{Path, PathBuf};

use mustard_core::io::fs;

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

/// Look up `binary` in the directories listed in the `PATH` environment
/// variable. On Windows, also probes with the `.exe` suffix. Fail-open:
/// any lookup error returns `false`.
fn which(binary: &str) -> bool {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(target_os = "windows") { ';' } else { ':' };
    for dir in path_var.split(sep) {
        let candidate = std::path::Path::new(dir).join(binary);
        if candidate.exists() {
            return true;
        }
        // Windows: also try with .exe suffix.
        #[cfg(target_os = "windows")]
        {
            let exe = std::path::Path::new(dir).join(format!("{binary}.exe"));
            if exe.exists() {
                return true;
            }
        }
    }
    false
}

/// Check that each detected language's code-tool program is present on
/// `PATH` — the table `packages/core/src/platform/code_tools.rs` shares with
/// `mustard init`, so a language a plugin can drive here is the same one the
/// install tried to set up.
pub(super) fn lsp_check(project_dir: &Path) -> CheckResult {
    let model_path = mustard_core::io::project_map::model_path(project_dir);
    let languages = mustard_core::platform::code_tools::detect_code_languages(project_dir, &model_path);

    // Only languages the catalog maps to a program, deduplicated by binary
    // (typescript + javascript both map to the same server).
    let mapped: Vec<(&str, &str)> = languages
        .iter()
        .filter_map(|lang| {
            mustard_core::platform::code_tools::code_tool_for_language(lang)
                .map(|tool| (lang.as_str(), tool.program))
        })
        .collect();

    if mapped.is_empty() {
        return CheckResult::skip("lsp", "no mapped stacks detected");
    }

    let mut seen_bins: Vec<&str> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for (lang, bin) in &mapped {
        if seen_bins.contains(bin) {
            continue;
        }
        seen_bins.push(bin);
        if !which(bin) {
            let hint = mustard_core::platform::code_tools::code_tool_for_language(lang)
                .map(|tool| tool.install_cmd)
                .unwrap_or_default();
            missing.push(format!("missing: {bin} (install: {hint})"));
        }
    }

    if missing.is_empty() {
        CheckResult::ok("lsp")
    } else {
        CheckResult::warn("lsp", missing)
    }
}

/// A cópia de `rtk` que o Mustard instala ao lado do binário que está
/// rodando — `<pasta-do-binário>/rtk` (`<...>/rtk.exe` no Windows). `exe` já
/// deve chegar canonizado (o pacote `.deb` põe o binário em
/// `/usr/lib/mustard/bin`, e `/usr/bin/mustard-rt` é um atalho para lá).
/// `None` quando não há cópia ao lado — uma compilação de desenvolvimento,
/// por exemplo.
fn bundled_rtk_path(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let name = if cfg!(windows) { "rtk.exe" } else { "rtk" };
    let candidate = dir.join(name);
    candidate.is_file().then_some(candidate)
}

/// A primeira cópia de `rtk` (ou `rtk.exe` no Windows) achada em `path_var`,
/// na ordem do `PATH` — a que o gancho de fato roda.
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

/// O veredito puro do que [`check_rtk`] observou. Separado da leitura do
/// `PATH` e do `current_exe()` para que o teste possa afirmar as regras sem
/// depender do que esta máquina tem instalado — o mesmo motivo de
/// `append_missing` em `apps/rt/src/shared/proc.rs`.
///
/// `bundled` e `first_on_path` já chegam canonizados: a comparação é só
/// igualdade de caminho.
fn rtk_verdict(bundled: Option<&Path>, first_on_path: Option<&Path>, version_output: Option<&str>) -> CheckResult {
    let Some(first_on_path) = first_on_path else {
        return CheckResult::warn("rtk", vec!["rtk not found on PATH".to_string()]);
    };

    let mut warnings = Vec::new();

    if let Some(bundled) = bundled
        && bundled != first_on_path {
            warnings.push(format!(
                "the rtk PATH finds first is not Mustard's own copy: {} (Mustard's copy: {}) — \
                 put Mustard's copy ahead on PATH",
                first_on_path.display(),
                bundled.display()
            ));
        }

    match version_output {
        Some(out) if out.starts_with("rtk ") => {}
        Some(out) => warnings.push(format!(
            "`rtk --version` did not start with \"rtk \": {out:?} — another program named rtk is \
             on PATH ahead of Mustard's own copy"
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
/// npm chamado `rtk` ficou na frente e desligou o filtro sem aviso. Sem cópia
/// ao lado do binário (compilação de desenvolvimento), confere só a
/// resposta.
pub(super) fn check_rtk() -> CheckResult {
    let bundled = std::env::current_exe().ok().and_then(|exe| {
        let real = std::fs::canonicalize(&exe).unwrap_or(exe);
        bundled_rtk_path(&real)
    });
    let path_var = std::env::var("PATH").unwrap_or_default();
    let first = first_rtk_on_path(&path_var)
        .map(|p| std::fs::canonicalize(&p).unwrap_or(p));
    let version = first.as_deref().and_then(rtk_version_output);
    rtk_verdict(bundled.as_deref(), first.as_deref(), version.as_deref())
}

/// Probe OS font directories for *any* Nerd Font (filename containing both a
/// font-family-ish token and "nerd" or "nf-"). WARN when none is found, since
/// the powerline statusline themes need one.
///
/// Fail-open: read errors degrade to "not detected" (WARN) rather than
/// blocking the doctor run.
pub(super) fn check_nerd_font() -> CheckResult {
    let dirs = nerd_font_search_dirs();
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

fn nerd_font_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
        dirs.push(PathBuf::from("C:/Windows/Fonts"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library").join("Fonts"));
        }
        dirs.push(PathBuf::from("/Library/Fonts"));
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/fonts"));
        }
        dirs.push(PathBuf::from("/usr/share/fonts"));
    }
    dirs
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
    use tempfile::tempdir;

    use super::*;

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

    #[test]
    fn rtk_verdict_ok_when_the_first_on_path_is_mustards_own_copy() {
        let bundled = PathBuf::from("/usr/lib/mustard/bin/rtk");
        let result = rtk_verdict(Some(&bundled), Some(&bundled), Some("rtk 0.49.0"));
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    /// O caso de 19/09: um pacote de outro catálogo com o mesmo nome `rtk`
    /// ficou à frente do PATH e desligou o filtro sem aviso.
    #[test]
    fn rtk_verdict_warns_when_another_copy_is_ahead_on_path() {
        let bundled = PathBuf::from("/usr/lib/mustard/bin/rtk");
        let other = PathBuf::from("/home/dev/.npm-global/bin/rtk");
        let result = rtk_verdict(Some(&bundled), Some(&other), Some("rtk 0.49.0"));
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        assert!(
            result.details.iter().any(|d| d.contains("/usr/lib/mustard/bin/rtk")),
            "{:?}",
            result.details
        );
    }

    #[test]
    fn rtk_verdict_warns_when_the_version_reply_does_not_start_with_rtk_space() {
        let bundled = PathBuf::from("/usr/lib/mustard/bin/rtk");
        let result = rtk_verdict(Some(&bundled), Some(&bundled), Some("Rust Type Kit 1.0.0"));
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
    }

    /// Compilação de desenvolvimento: sem cópia ao lado do binário, o veredito
    /// confere só a resposta de `rtk --version` — não há caminho de cópia para
    /// comparar.
    #[test]
    fn rtk_verdict_without_a_bundled_copy_checks_only_the_response() {
        let path = PathBuf::from("/usr/bin/rtk");
        let ok = rtk_verdict(None, Some(&path), Some("rtk 0.49.0"));
        assert_eq!(ok.status, Status::Ok, "{:?}", ok.details);

        let warn = rtk_verdict(None, Some(&path), Some("not rtk at all"));
        assert_eq!(warn.status, Status::Warn);
    }

    #[test]
    fn rtk_verdict_warns_when_rtk_is_not_on_path_at_all() {
        let bundled = PathBuf::from("/usr/lib/mustard/bin/rtk");
        let result = rtk_verdict(Some(&bundled), None, None);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
    }
}
