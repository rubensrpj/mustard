// SPEC LANG: pt-allowed — this module's PT_MARKER_WORDS array contains the diacritic seed words.
//! `mustard-rt run language-audit` — list files in mustard's own repo that
//! contain PT-BR text where they should be EN.
//!
//! Policy ([[2026-05-26-template-agnostic-audit]]): specs follow
//! `mustard.json#specLang`, and so does every comment written in the source —
//! `//`, `///`, `//!`, `/* */`. The audit therefore looks only at what sits
//! OUTSIDE a comment. Everything else stays EN-only: identifiers, file paths,
//! shell commands, log/error messages, API string constants, plus the prose of
//! templates, refs and ADRs. Markdown is scanned whole — there the prose *is*
//! the content, not a comment. This subcommand surfaces drift as a soft
//! warning — exit 0 always, even when hits are found, unless `--strict` is
//! passed.
//!
//! ## Heuristic
//!
//! For each `.md`/`.rs`/`.ts`/`.tsx` under the audit targets, count distinct
//! Portuguese diacritic words (nao, esta, tambem, funcao, ...). In `.rs`,
//! `.ts` and `.tsx` the comments are removed before the count, so only code
//! outside them is scored; `.md` is counted as-is. When the distinct count
//! reaches the threshold, mark the file as a hit. False positives are skipped
//! via allow-list paths and per-file markers.
//!
//! ## Scan targets
//!
//! Recursive walk relative to the cwd:
//!
//! - `apps/cli/templates/` (payload of `mustard init`)
//! - `apps/{cli,rt,dashboard}/src/`
//! - `packages/*/src/`
//! - `.claude/refs/`
//!
//! Excluded by default (allow-list):
//!
//! - `apps/cli/templates/refs/feature/spec-language.md` — documents PT examples.
//! - `apps/cli/templates-extras/` — opt-in payload; user freely picks the locale.
//! - `apps/rt/tests/fixtures/` — test fixtures intentionally carry legacy data.
//! - `.claude/spec/` — historical specs may be PT by design.
//! - `node_modules`, `.git`, `target`, `dist`, `.next`.
//!
//! ## Per-file opt-out
//!
//! A file whose first non-empty line contains the marker
//! `<!-- LANG: pt-allowed -->` (markdown) or `// SPEC LANG: pt-allowed` (Rust
//! / TS) is skipped regardless of content. Use this on artifacts that
//! intentionally hold Portuguese examples.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use mustard_core::io::fs;
use serde::Serialize;
use serde_json::json;

/// Distinctive PT-BR diacritic words used as the heuristic seed. Case- and
/// diacritic-insensitive matching is too noisy (catches PT loanwords across
/// EN text); we require the exact diacritic spelling so the false-positive
/// floor stays low.
///
/// Keep this list small and curated — three hits on the same file across this
/// vocabulary is the threshold for "definitely PT-BR".
const PT_MARKER_WORDS: &[&str] = &[
    "não",
    "está",
    "também",
    "função",
    "ação",
    "configuração",
    "porém",
    "então",
    "específico",
    "específica",
    "diretório",
    "execução",
    "padrão",
    "código",
    "estão",
    "será",
    "são",
    "deve",
    "através",
    "porque",
    "fluxo",
];

/// Number of distinct marker words a file must contain before it counts as a
/// hit. `3` keeps incidental PT terms in EN docs (e.g. a single quoted spec
/// title) from flagging the whole file.
const HIT_THRESHOLD: usize = 3;

/// Audit run options.
pub struct LanguageAuditOpts {
    /// Output format: `"text"` (default) or `"json"`.
    pub format: String,
    /// When true, exit with status `1` if any hit is found. Default `false`.
    pub strict: bool,
}

/// One per-file hit recorded in the report.
#[derive(Debug, Serialize)]
struct Hit {
    file: String,
    matches: usize,
    samples: Vec<String>,
}

/// Entry point. Walks every audit target, emits the report, and (under
/// `--strict`) exits non-zero when at least one hit is found.
pub fn run(opts: LanguageAuditOpts) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let report = audit(&cwd);

    let exit_code = if opts.strict && !report.hits.is_empty() {
        1
    } else {
        0
    };

    match opts.format.as_str() {
        "json" => {
            let body = json!({
                "scanned": report.scanned,
                "hits": report.hits,
                "ok": report.hits.is_empty(),
                "strict": opts.strict,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into())
            );
        }
        _ => {
            for h in &report.hits {
                println!("HIT  {}  ({} distinct PT words)", h.file, h.matches);
                for s in &h.samples {
                    println!("     - {s}");
                }
            }
            println!(
                "\nlanguage-audit: scanned={} hits={} strict={}",
                report.scanned,
                report.hits.len(),
                opts.strict
            );
        }
    }

    std::process::exit(exit_code);
}

#[derive(Debug)]
struct Report {
    scanned: usize,
    hits: Vec<Hit>,
}

/// Pure audit — walks the targets under `root`, returns the report. Split out
/// from [`run`] so the inline tests can assert against a tempdir without
/// touching stdout / exit.
fn audit(root: &Path) -> Report {
    let targets = audit_targets(root);
    let mut scanned = 0usize;
    let mut hits: Vec<Hit> = Vec::new();

    for target in &targets {
        walk(target, &mut |path| {
            if !is_scannable_ext(path) {
                return;
            }
            if is_allow_listed(path) {
                return;
            }
            let Ok(text) = fs::read_to_string(path) else {
                return;
            };
            scanned += 1;
            if has_pt_marker(&text) {
                return;
            }
            // Comentário segue o specLang do projeto: só o que está FORA
            // dele entra na contagem. Markdown não passa por aqui.
            let scored: Cow<'_, str> = if has_c_style_comments(path) {
                Cow::Owned(strip_comments(&text))
            } else {
                Cow::Borrowed(text.as_str())
            };
            let (count, samples) = score_pt(&scored);
            if count >= HIT_THRESHOLD {
                let display = path.strip_prefix(root).unwrap_or(path).display().to_string();
                hits.push(Hit {
                    file: display.replace('\\', "/"),
                    matches: count,
                    samples,
                });
            }
        });
    }

    // Stable sort for byte-stable JSON.
    hits.sort_by(|a, b| a.file.cmp(&b.file));

    Report { scanned, hits }
}

/// Recursively walk `dir`, invoking `visit` for every regular file. Skips
/// noisy/legacy directories (`node_modules`, `.git`, `target`, `dist`,
/// `.next`, `apps/cli/templates-extras`, `apps/rt/tests/fixtures`,
/// `.claude/spec`).
fn walk(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.path;
        if path.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            walk(&path, visit);
        } else if path.is_file() {
            visit(&path);
        }
    }
}

/// Audit targets resolved under `root`. Filters to ones that exist on disk so
/// tests using a tempdir do not need to materialise every layout.
fn audit_targets(root: &Path) -> Vec<PathBuf> {
    // Spec 2026-05-26-template-agnostic-audit line 149 declares scope as `.claude/refs/` only;
    // `.claude/commands/` and `.claude/skills/` were observed to cause stale-install false positives.
    let candidates = [
        "apps/cli/templates",
        "apps/cli/src",
        "apps/rt/src",
        "apps/dashboard/src",
        "apps/dashboard/server/src",
        "packages/core/src",
        // The compiled-in harness seeds (settings, injectable instruction
        // files) — moved from apps/cli/templates, still under the EN policy.
        "packages/core/templates",
        // The command/skill/ref prose moved to the plugin tree in F4 (2.0);
        // it must stay under the EN-only audit like the old `.claude/refs` did.
        "plugin",
        ".claude/refs",
    ];
    candidates
        .iter()
        .map(|c| root.join(c))
        .filter(|p| p.exists())
        .collect()
}

/// Return true for directories the walker must not descend into.
fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if matches!(
        name,
        "node_modules" | ".git" | "target" | "dist" | ".next" | "build"
    ) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    // Opt-in payload and test fixtures intentionally carry non-EN content.
    if s.contains("apps/cli/templates-extras") {
        return true;
    }
    if s.contains("apps/rt/tests/fixtures") {
        return true;
    }
    // Historical specs are user-narrative — outside the EN policy.
    if s.contains("/.claude/spec/") || s.ends_with("/.claude/spec") {
        return true;
    }
    false
}

/// Only audit text artifacts the policy applies to.
fn is_scannable_ext(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "md" | "rs" | "ts" | "tsx"
    )
}

/// Allow-list of fully-qualified paths that are exempt from the audit. Used
/// for the canonical pt-BR example doc the rest of the audit references.
fn is_allow_listed(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.ends_with("plugin/refs/feature/spec-language.md")
        || s.ends_with("apps/cli/templates/refs/feature/spec-language.md")
        || s.ends_with(".claude/refs/feature/spec-language.md")
}

/// Per-file opt-out marker. Looks at the first 5 non-empty lines for
/// `<!-- LANG: pt-allowed -->` (markdown) or `// SPEC LANG: pt-allowed`
/// (source) — covers shebangs / module headers without scanning the whole
/// file.
fn has_pt_marker(text: &str) -> bool {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .any(|l| {
            l.contains("LANG: pt-allowed") || l.contains("SPEC LANG: pt-allowed")
        })
}

/// Extensões cujo comentário o removedor entende. Markdown fica de fora de
/// propósito: lá a prosa é o próprio conteúdo, não um comentário.
fn has_c_style_comments(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(ext.to_ascii_lowercase().as_str(), "rs" | "ts" | "tsx")
}

/// Devolve `text` sem o conteúdo dos comentários — linha iniciada por `//`
/// (portanto também `///` e `//!`) e blocos `/* ... */`. Literais de string
/// (`"`, `'` e crase) são copiados na íntegra, para que uma URL como
/// `"https://x"` não seja lida como início de comentário. As quebras de linha
/// sobrevivem, de modo que os trechos do relatório continuem vindo da linha
/// certa.
///
/// Um `'` de lifetime pode deixar o varredor em modo string até a próxima
/// aspa; nesse caso o comentário deixa de ser removido — ou seja, o auditor
/// passa a ver MAIS texto, nunca menos.
fn strip_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match (c, chars.get(i + 1)) {
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i < chars.len() {
                    if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        i += 2;
                        break;
                    }
                    if chars[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
            ('"' | '\'' | '`', _) => {
                out.push(c);
                i += 1;
                while i < chars.len() {
                    let d = chars[i];
                    out.push(d);
                    i += 1;
                    if d == '\\' {
                        if let Some(esc) = chars.get(i) {
                            out.push(*esc);
                            i += 1;
                        }
                        continue;
                    }
                    if d == c {
                        break;
                    }
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Count distinct marker words present in `text` (case-insensitive on the
/// language letters, but the diacritic must match) and collect up to 5 sample
/// snippets for the report.
fn score_pt(text: &str) -> (usize, Vec<String>) {
    let lower = text.to_lowercase();
    let mut hits: Vec<&'static str> = Vec::new();
    let mut samples: Vec<String> = Vec::new();
    for word in PT_MARKER_WORDS {
        if lower.contains(word) {
            hits.push(word);
            if samples.len() < 5 {
                if let Some(sample) = grab_sample(text, word) {
                    samples.push(sample);
                }
            }
        }
    }
    (hits.len(), samples)
}

/// Return a single-line snippet around the first occurrence of `needle`. Used
/// for the JSON `samples[]` field so reviewers see the offending text.
fn grab_sample(text: &str, needle: &str) -> Option<String> {
    let needle_lower = needle.to_lowercase();
    for line in text.lines() {
        if line.to_lowercase().contains(&needle_lower) {
            let trimmed = line.trim();
            let cap = trimmed.chars().take(120).collect::<String>();
            return Some(cap);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn pure_pt_file_is_a_hit() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "apps/cli/templates/refs/foo.md",
            "Esta é a configuração padrão. A função não está disponível porém será corrigida.",
        );
        let report = audit(root);
        assert_eq!(report.hits.len(), 1, "expected 1 hit, got {:?}", report.hits);
    }

    #[test]
    fn pure_en_file_is_not_a_hit() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "apps/cli/templates/refs/foo.md",
            "This document describes the canonical configuration of the spec drafter.",
        );
        let report = audit(root);
        assert!(report.hits.is_empty(), "got hits: {:?}", report.hits);
    }

    #[test]
    fn below_threshold_is_not_a_hit() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Two distinct PT words only — below the threshold of 3.
        write(
            root,
            "apps/cli/templates/refs/foo.md",
            "Mostly English text but contains não and está somewhere.",
        );
        let report = audit(root);
        assert!(report.hits.is_empty(), "got hits: {:?}", report.hits);
    }

    #[test]
    fn allow_listed_path_is_skipped() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "apps/cli/templates/refs/feature/spec-language.md",
            "Configuração padrão da função: não está disponível porém será corrigida.",
        );
        let report = audit(root);
        assert!(report.hits.is_empty(), "allow-listed path leaked: {:?}", report.hits);
    }

    #[test]
    fn marker_opt_out_is_respected_md() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "apps/cli/templates/refs/example.md",
            "<!-- LANG: pt-allowed -->\nEsta é a configuração padrão. A função não está disponível.",
        );
        let report = audit(root);
        assert!(report.hits.is_empty(), "marker not honoured: {:?}", report.hits);
    }

    #[test]
    fn marker_opt_out_is_respected_rs() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "apps/rt/src/foo.rs",
            "// SPEC LANG: pt-allowed\n// Esta é a configuração padrão.\n// A função não está disponível porém será corrigida.",
        );
        let report = audit(root);
        assert!(report.hits.is_empty(), "marker not honoured: {:?}", report.hits);
    }

    #[test]
    fn skip_dirs_are_not_walked() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A PT file under the spec tree must not flag — specs are user narrative.
        write(
            root,
            ".claude/spec/some-spec/spec.md",
            "Esta é a configuração padrão. A função não está disponível porém será corrigida.",
        );
        // Also templates-extras is opt-in.
        write(
            root,
            "apps/cli/templates-extras/hallmark/foo.md",
            "Esta é a configuração padrão. A função não está disponível porém será corrigida.",
        );
        let report = audit(root);
        assert!(report.hits.is_empty(), "skip dirs leaked: {:?}", report.hits);
    }

    #[test]
    fn pt_only_inside_comments_is_not_a_hit() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // O comentário segue o specLang; o código ao lado é inglês puro.
        write(
            root,
            "apps/rt/src/greeter.rs",
            "//! Este módulo não está pronto, porém será corrigido.\n\
             /// A função de configuração padrão.\n\
             /* Também cobre bloco assim, com código e execução. */\n\
             pub fn build() -> usize {\n    3\n}\n",
        );
        let report = audit(root);
        assert!(
            report.hits.is_empty(),
            "comment-only PT flagged: {:?}",
            report.hits
        );
    }

    #[test]
    fn pt_outside_comments_is_still_a_hit() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Identificador, mensagem de log e constante de texto continuam em
        // inglês obrigatório — sem este caso, um verificador que simplesmente
        // parou de funcionar passaria no teste positivo acima.
        write(
            root,
            "apps/rt/src/greeter.rs",
            "pub fn configuração_padrão() -> String {\n\
             eprintln!(\"execução não está disponível\");\n\
             String::from(\"código de erro\")\n\
             }\n",
        );
        let report = audit(root);
        assert_eq!(
            report.hits.len(),
            1,
            "PT outside comments went unnoticed: {:?}",
            report.hits
        );
    }

    #[test]
    fn pt_in_both_is_a_hit_because_of_the_code() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "apps/rt/src/greeter.rs",
            "// Este comentário explica a função de configuração padrão.\n\
             pub fn run() {\n\
             eprintln!(\"execução não está disponível para o código\");\n\
             }\n",
        );
        let report = audit(root);
        assert_eq!(report.hits.len(), 1, "mixed file missed: {:?}", report.hits);
        // Só as 4 palavras de fora do comentário contam; as do comentário
        // (função, configuração, ação, padrão) ficariam de fora da soma.
        assert_eq!(
            report.hits[0].matches, 4,
            "comment words leaked into the count: {:?}",
            report.hits[0]
        );
        assert!(
            report.hits[0]
                .samples
                .iter()
                .all(|s| !s.trim_start().starts_with("//")),
            "sample came from a comment: {:?}",
            report.hits[0].samples
        );
    }

    #[test]
    fn url_inside_a_string_is_not_read_as_a_comment() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // O `//` de `https://` abre comentário para um removedor ingênuo, que
        // engoliria o resto da linha e perderia o português da constante.
        write(
            root,
            "apps/rt/src/links.rs",
            "pub const DOC: &str = \"https://x/não-está-no-código-padrão\";\n",
        );
        let report = audit(root);
        assert_eq!(
            report.hits.len(),
            1,
            "string content was swallowed as a comment: {:?}",
            report.hits
        );
    }
}
