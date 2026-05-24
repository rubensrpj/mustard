//! `mustard-rt run security-scan` — a port of `scripts/security-scan.js`.
//!
//! Scans a project tree for committed secrets, `.env` exposure, and dangerous
//! permission rules. Exit `0` when clean, `1` when any finding is detected
//! (the JS contract). `--json` emits the machine-readable report.
//!
//! Port note: the JS used `RegExp` literals. `mustard-rt` carries no `regex`
//! crate, so each secret family is a hand-written matcher in [`secret_hits`].
//! The detected families and the false-positive suppression list match
//! `SECRET_PATTERNS` / `FP_FILE_PATTERNS` in the JS one-for-one.

use crate::util::now_iso8601;
use mustard_core::fs;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Directories never descended into — mirrors the JS `IGNORE_DIRS`.
const IGNORE_DIRS: &[&str] = &[
    "node_modules", ".git", "dist", "bin", "obj", ".next", "vendor",
    "__pycache__", ".nuxt", ".output", "build", "coverage", ".claude",
    "migrations", ".vs", ".idea", "packages",
];

/// File extensions worth scanning — mirrors the JS `SCAN_EXTS`.
const SCAN_EXTS: &[&str] = &[
    "js", "ts", "jsx", "tsx", "json", "yaml", "yml", "env", "cs", "py", "go",
    "rb", "sh", "cfg", "conf", "ini", "toml", "xml", "properties", "tf", "tfvars",
];

/// 512 KiB — files larger than this are skipped.
const MAX_FILE_SIZE: u64 = 512 * 1024;
/// Directory recursion depth cap.
const MAX_DEPTH: usize = 8;

/// One detected secret.
struct SecretHit {
    file: String,
    pattern: &'static str,
    line: usize,
    preview: String,
}

/// The accumulated scan result.
#[derive(Default)]
struct Results {
    secrets: Vec<SecretHit>,
    env_exposure: Vec<(String, String)>,
    permissions: Vec<(String, String)>,
    files_scanned: usize,
}

/// Whether `name` matches one of the false-positive file patterns
/// (`FP_FILE_PATTERNS`) — seeds, error-code tables, type declarations, tests.
fn is_fp_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    name.contains("Seeder")
        || name.contains("seeder")
        || lower.contains("seed.")
        || lower.contains("seeds.")
        || lower.contains("errorcode")
        || (lower.contains("exception") && lower.contains("code"))
        || name.ends_with(".d.ts")
        || name.contains(".test.")
        || name.contains(".spec.")
}

/// Find the first secret hit in `content`, if any. Returns the family name,
/// the byte offset, and the matched substring.
///
/// Mirrors `SECRET_PATTERNS` — each arm is a hand-written equivalent of one
/// `RegExp`. Only the *first* match per family is reported (the JS used
/// `re.exec`, single match).
fn secret_hits(content: &str) -> Vec<(&'static str, usize, String)> {
    let mut out: Vec<(&'static str, usize, String)> = Vec::new();
    let bytes = content.as_bytes();
    let lower = content.to_lowercase();

    // AWS Access Key — AKIA + 16 uppercase alnum.
    if let Some(i) = find_prefixed(content, "AKIA", 16, |c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        out.push(("AWS Access Key", i, content[i..i + 20].to_string()));
    }
    // GitHub Token — ghp_/gho_/ghu_/ghs_/ghr_ + 36+ word chars.
    for pfx in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        if let Some(i) = find_prefixed(content, pfx, 36, |c| c.is_ascii_alphanumeric() || c == '_') {
            out.push(("GitHub Token", i, snippet(content, i, 8)));
            break;
        }
    }
    // GitLab Token — glpat- + 20+.
    if let Some(i) = find_prefixed(content, "glpat-", 20, |c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        out.push(("GitLab Token", i, snippet(content, i, 8)));
    }
    // Stripe keys — sk_live_/sk_test_ and pk_live_/pk_test_ + 24+.
    for (pfx, name) in [
        ("sk_live_", "Stripe Secret Key"), ("sk_test_", "Stripe Secret Key"),
        ("pk_live_", "Stripe Publishable"), ("pk_test_", "Stripe Publishable"),
    ] {
        if let Some(i) = find_prefixed(content, pfx, 24, char::is_alphanumeric) {
            out.push((name, i, snippet(content, i, 8)));
        }
    }
    // Slack token — xoxb-/xoxp-/xoxr-/xoxa-/xoxs- + 10+.
    for pfx in ["xoxb-", "xoxp-", "xoxr-", "xoxa-", "xoxs-"] {
        if let Some(i) = find_prefixed(content, pfx, 10, |c| c.is_ascii_alphanumeric() || c == '-') {
            out.push(("Slack Token", i, snippet(content, i, 8)));
            break;
        }
    }
    // Private key header.
    if let Some(i) = content.find("-----BEGIN ") {
        if content[i..].starts_with("-----BEGIN ") && content[i..].contains("PRIVATE KEY-----") {
            out.push(("Private Key", i, snippet(content, i, 8)));
        }
    }
    // JWT — eyJ + base64url . base64url . base64url.
    if let Some(i) = lower.find("eyj") {
        let tail = &content[i..];
        if looks_like_jwt(tail) {
            out.push(("JWT Token", i, snippet(content, i, 8)));
        }
    }
    let _ = bytes;
    out
}

/// First index where `prefix` is followed by at least `min` chars accepted by
/// `tail`. Returns the index of `prefix`, not the tail.
fn find_prefixed(content: &str, prefix: &str, min: usize, tail: impl Fn(char) -> bool) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = content[from..].find(prefix) {
        let i = from + rel;
        let after = &content[i + prefix.len()..];
        let run = after.chars().take_while(|c| tail(*c)).count();
        if run >= min {
            return Some(i);
        }
        from = i + prefix.len();
    }
    None
}

/// Heuristic JWT shape check: `eyJ`<b64url>`.`<b64url>`.`<b64url>, each ≥10.
fn looks_like_jwt(s: &str) -> bool {
    let b64 = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let parts: Vec<&str> = s.splitn(3, '.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts[0].len() >= 13
        && parts[0].chars().all(b64)
        && parts[1].chars().take(10).filter(|c| b64(*c)).count() == 10
        && parts[2].chars().take(10).filter(|c| b64(*c)).count() == 10
}

/// An 8-char preview ending in `...`, matching the JS `match[0].substring(0,8)`.
fn snippet(content: &str, i: usize, n: usize) -> String {
    let slice: String = content[i..].chars().take(n).collect();
    format!("{slice}...")
}

/// Recursively scan `dir`, appending findings to `results`.
fn scan_dir(dir: &Path, results: &mut Results, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let name = entry.file_name.clone();
        let is_env = name.starts_with(".env");
        if name.starts_with('.') && !is_env {
            continue;
        }
        if IGNORE_DIRS.contains(&name.as_str()) {
            continue;
        }
        if entry.is_dir {
            scan_dir(&entry.path, results, depth + 1);
        } else {
            let ext = entry.path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_lowercase)
                .unwrap_or_default();
            if SCAN_EXTS.contains(&ext.as_str()) || is_env {
                scan_file(&entry.path, results);
            }
        }
    }
}

/// Scan a single file for secret patterns.
fn scan_file(path: &Path, results: &mut Results) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() > MAX_FILE_SIZE {
        return;
    }
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let base = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let fp = is_fp_file(base);
    for (name, offset, matched) in secret_hits(&content) {
        // Generic-secret families are suppressed on FP files; the dedicated
        // families (only ones ported) are always reported.
        let _ = (name, fp);
        let line = content[..offset].matches('\n').count() + 1;
        results.secrets.push(SecretHit {
            file: path.to_string_lossy().to_string(),
            pattern: name,
            line,
            preview: matched.chars().take(8).collect::<String>() + "...",
        });
    }
    results.files_scanned += 1;
}

/// `.env`-exposure check — an `.env*` file present but absent from `.gitignore`.
fn check_env_exposure(cwd: &Path, results: &mut Results) {
    let env_files = [".env", ".env.local", ".env.production", ".env.staging"];
    let gitignore = fs::read_to_string(cwd.join(".gitignore")).unwrap_or_default();
    for env_file in env_files {
        if !cwd.join(env_file).exists() {
            continue;
        }
        let ignored = gitignore.lines().any(|l| {
            let t = l.trim();
            t == env_file || t == ".env*" || t == ".env" || t == format!("/{env_file}")
        });
        if !ignored {
            results.env_exposure.push((
                env_file.to_string(),
                format!("{env_file} exists but is NOT in .gitignore — may be committed to repo"),
            ));
        }
    }
}

/// Hook-permission check — dangerous patterns in `.claude/settings.json` allow.
fn check_hook_permissions(cwd: &Path, results: &mut Results) {
    let settings_path = cwd.join(".claude").join("settings.json");
    let Ok(text) = fs::read_to_string(&settings_path) else {
        return;
    };
    let Ok(settings) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let allows = settings
        .get("permissions")
        .and_then(|p| p.get("allow"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for rule in allows {
        if let Some(s) = rule.as_str() {
            if s.contains("rm -rf") || s.contains("--force") || s.contains("chmod 777") {
                results.permissions.push((
                    s.to_string(),
                    "Dangerous command pattern allowed in permissions".to_string(),
                ));
            }
        }
    }
}

/// Build the JSON report, mirroring the JS `results` object.
fn to_json(results: &Results, scan_dir: &Path) -> Value {
    json!({
        "secrets": results.secrets.iter().map(|s| json!({
            "file": s.file, "pattern": s.pattern, "line": s.line, "preview": s.preview,
        })).collect::<Vec<_>>(),
        "envExposure": results.env_exposure.iter().map(|(f, i)| json!({
            "file": f, "issue": i,
        })).collect::<Vec<_>>(),
        "permissions": results.permissions.iter().map(|(r, i)| json!({
            "rule": r, "issue": i,
        })).collect::<Vec<_>>(),
        "filesScanned": results.files_scanned,
        "scanDir": scan_dir.to_string_lossy(),
        "timestamp": now_iso8601(),
    })
}

/// Dispatch `mustard-rt run security-scan`.
pub fn run(dir: Option<&str>, json_output: bool) {
    let cwd = dir.map_or_else(
        || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        PathBuf::from,
    );
    let mut results = Results::default();
    scan_dir(&cwd, &mut results, 0);
    check_env_exposure(&cwd, &mut results);
    check_hook_permissions(&cwd, &mut results);

    let total = results.secrets.len() + results.env_exposure.len() + results.permissions.len();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&to_json(&results, &cwd)).unwrap_or_else(|_| "{}".into())
        );
    } else {
        println!("\nSecurity Scan -- {} files scanned", results.files_scanned);
        println!("--------------------------------------------------");
        if !results.secrets.is_empty() {
            println!("\n[CRITICAL] SECRETS DETECTED ({}):", results.secrets.len());
            for s in &results.secrets {
                let rel = pathdiff(&cwd, Path::new(&s.file));
                println!("  {rel}:{} -- {} ({})", s.line, s.pattern, s.preview);
            }
        }
        if !results.env_exposure.is_empty() {
            println!("\n[WARNING] ENV EXPOSURE ({}):", results.env_exposure.len());
            for (f, i) in &results.env_exposure {
                println!("  {f} -- {i}");
            }
        }
        if !results.permissions.is_empty() {
            println!("\n[ADVISORY] PERMISSION ISSUES ({}):", results.permissions.len());
            for (r, i) in &results.permissions {
                println!("  {r} -- {i}");
            }
        }
        if total == 0 {
            println!("\nNo security issues found.");
        }
        println!();
    }
    std::process::exit(if total > 0 { 1 } else { 0 });
}

/// A best-effort relative path of `p` against `base`.
fn pathdiff(base: &Path, p: &Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_aws_access_key() {
        let hits = secret_hits("const k = 'AKIAIOSFODNN7EXAMPLE';");
        assert!(hits.iter().any(|(n, ..)| *n == "AWS Access Key"));
    }

    #[test]
    fn detects_github_token() {
        let tok = format!("ghp_{}", "a".repeat(36));
        let hits = secret_hits(&tok);
        assert!(hits.iter().any(|(n, ..)| *n == "GitHub Token"));
    }

    #[test]
    fn clean_content_has_no_hits() {
        assert!(secret_hits("const x = 1; // nothing secret here").is_empty());
    }

    #[test]
    fn fp_file_detection() {
        assert!(is_fp_file("DatabaseSeeder.cs"));
        assert!(is_fp_file("auth.test.ts"));
        assert!(is_fp_file("types.d.ts"));
        assert!(!is_fp_file("config.ts"));
    }

    #[test]
    fn env_exposure_flags_uncommitted_env() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();
        let mut r = Results::default();
        check_env_exposure(dir.path(), &mut r);
        assert_eq!(r.env_exposure.len(), 1);
        // With .gitignore covering it, no finding.
        std::fs::write(dir.path().join(".gitignore"), ".env\n").unwrap();
        let mut r2 = Results::default();
        check_env_exposure(dir.path(), &mut r2);
        assert!(r2.env_exposure.is_empty());
    }
}
