//! O que a máquina tem instalado: o programa `claude` no PATH, o servidor de
//! linguagem de cada pilha que o projeto usa e uma Nerd Font para os temas da
//! barra de status. Nenhuma dessas faltas trava: cada uma vira aviso com o
//! comando que instala.

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

/// Map a stack name to the canonical language-server binary name (and an
/// install hint). The table is best-effort; unmapped stacks are silently ignored.
fn lsp_server_for_stack(stack: &str) -> Option<(&'static str, &'static str)> {
    match stack {
        "rust" => Some(("rust-analyzer", "rustup component add rust-analyzer")),
        "typescript" | "javascript" => {
            Some(("typescript-language-server", "npm install -g typescript-language-server typescript"))
        }
        "python" => Some(("pyright", "pip install pyright")),
        "go" => Some(("gopls", "go install golang.org/x/tools/gopls@latest")),
        "java" => Some(("jdtls", "install Eclipse JDT Language Server")),
        "csharp" => Some(("omnisharp", "install OmniSharp via .NET or VS extension")),
        _ => None,
    }
}

/// Detect which language stacks are active in `project_dir` by probing for
/// well-known manifest files, reduced to stack-name strings. Fail-open: IO
/// errors → empty list.
fn detect_stacks(project_dir: &Path) -> Vec<&'static str> {
    let mut stacks: Vec<&'static str> = Vec::new();

    // Rust: Cargo.toml with [package]
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

    // Python: pyproject.toml or requirements.txt
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

    // C#: any *.csproj present
    if let Ok(entries) = fs::read_dir(project_dir) {
        let has_csproj = entries
            .iter()
            .any(|e| e.file_name.ends_with(".csproj"));
        if has_csproj {
            stacks.push("csharp");
        }
    }

    // Java: pom.xml or build.gradle
    if project_dir.join("pom.xml").is_file() || project_dir.join("build.gradle").is_file() {
        stacks.push("java");
    }

    stacks
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

/// Check that each detected stack's language server is present on `PATH`.
pub(super) fn lsp_check(project_dir: &Path) -> CheckResult {
    let stacks = detect_stacks(project_dir);

    // Collect mapped (stack → server) entries, ignoring unmapped stacks.
    let mapped: Vec<(&str, &str, &str)> = stacks
        .iter()
        .filter_map(|s| lsp_server_for_stack(s).map(|(bin, hint)| (*s, bin, hint)))
        .collect();

    if mapped.is_empty() {
        return CheckResult::skip("lsp", "no mapped stacks detected");
    }

    // Deduplicate by binary (typescript + javascript both map to the same server).
    let mut seen_bins: Vec<&str> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for (_stack, bin, hint) in &mapped {
        if seen_bins.contains(bin) {
            continue;
        }
        seen_bins.push(bin);
        if !which(bin) {
            missing.push(format!("missing: {bin} (install: {hint})"));
        }
    }

    if missing.is_empty() {
        CheckResult::ok("lsp")
    } else {
        CheckResult::warn("lsp", missing)
    }
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
}
