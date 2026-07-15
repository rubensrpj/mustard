//! Auto-branch name computation for the porta-unica `pipeline.kind` signal.
//!
//! Pure, self-contained git-ref helpers lifted out of `emit_pipeline`: given a
//! spec or intent plus the project's integration base, compute the
//! `{base}_{slug}` work-branch name the first file edit of a work unit checks
//! out. The only I/O is reading `mustard.json` for the slug locale.

use std::path::Path;

/// Resolve the effective integration base for the auto-branch prefix: the
/// caller-supplied `--base` when it names one of the project's integration
/// bases (`config.git.integration_bases()`), else the project's primary base
/// (`config.git.primary_base()`).
///
/// Agnostic — both the accepted set and the fallback come from `git.flow`; no
/// branch name is hardcoded here. Do NOT re-derive the base set ad hoc: the
/// core owns that derivation so `work_branch_gate` and this emitter agree.
pub(crate) fn resolve_base(requested: Option<&str>, config: &mustard_core::ProjectConfig) -> String {
    let bases = config.git.integration_bases();
    if let Some(b) = requested.map(str::trim).filter(|b| !b.is_empty()) {
        if bases.contains(b) {
            return b.to_string();
        }
    }
    config.git.primary_base()
}

/// Resolve the slug lang for the auto-branch from `mustard.json` — `lang`
/// (legacy) then `specLang`, defaulting to `pt-BR` (mirrors
/// [`mustard_core::ProjectConfig::i18n`] precedence). A branch is not
/// user-facing prose, but the slug helper still strips accents per-locale.
fn branch_lang(project: &str) -> String {
    let config = mustard_core::ProjectConfig::load(Path::new(project));
    config
        .lang
        .clone()
        .or(config.spec_lang.clone())
        .unwrap_or_else(|| "pt-BR".to_string())
}

/// A short, ref-safe fallback token from the session id. `unknown`/empty →
/// `work` so the branch always has a non-empty tail.
fn short_sid(sid: &str) -> String {
    let s = sid.trim();
    if s.is_empty() || s == "unknown" {
        return "work".to_string();
    }
    s.chars().take(8).collect()
}

/// Sanitise `{base}_{slug}` into a valid git ref: keep `[A-Za-z0-9-_./]`,
/// map everything else to `-`, collapse `..` runs (git forbids them), and trim
/// leading `-`/`.`/`/` and trailing `/`/`.`. Never empty — floors to `work`.
fn sanitize_git_ref(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '/' => ch,
            _ => '-',
        })
        .collect();
    while out.contains("..") {
        out = out.replace("..", "-");
    }
    let trimmed = out
        .trim_start_matches(|c| c == '-' || c == '.' || c == '/')
        .trim_end_matches(|c| c == '/' || c == '.');
    if trimmed.is_empty() {
        "work".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Compute the auto-branch name for a `pipeline.kind` work-type signal:
/// `{base}_{slug}`, sanitised to a valid git ref. The `{base}_` prefix records
/// the integration branch the work is cut from, so the gate (and `/git`) can
/// recover the PR-target from the name alone. Slug precedence:
/// 1. `--spec` when present (already a slug);
/// 2. else `--intent` slugified for the project's lang;
/// 3. else a date-based fallback (`YYYY-MM-DD` from the event `ts`) suffixed
///    with a short session id for uniqueness.
/// Never fails — every branch degrades to a valid ref.
pub(crate) fn compute_work_branch(
    base: &str,
    spec: &str,
    intent: Option<&str>,
    sid: &str,
    ts: &str,
    project: &str,
) -> String {
    let slug = if !spec.trim().is_empty() {
        spec.trim().to_string()
    } else if let Some(intent) = intent.map(str::trim).filter(|s| !s.is_empty()) {
        crate::commands::spec::spec_slug::for_lang(intent, &branch_lang(project))
    } else {
        // Date-based fallback from the shared event timestamp, plus a short
        // session id so two spec-less/intent-less runs on the same day differ.
        let date = ts.split('T').next().unwrap_or("").trim();
        if date.is_empty() {
            short_sid(sid)
        } else {
            format!("{date}-{}", short_sid(sid))
        }
    };
    sanitize_git_ref(&format!("{base}_{slug}"))
}

#[cfg(test)]
mod tests {
    // -----------------------------------------------------------------------
    // Auto-branch name computation (porta-unica)
    // -----------------------------------------------------------------------

    #[test]
    fn compute_work_branch_prefers_spec_slug_off_primary_base() {
        // base = the primary/`*` base → `{base}_{slug}`, kind dropped from name.
        let b = super::compute_work_branch("dev", "2026-07-02-my-spec", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "dev_2026-07-02-my-spec");
        // Task example.
        let b2 = super::compute_work_branch("dev", "parcelas-virtuais", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b2, "dev_parcelas-virtuais");
    }

    #[test]
    fn compute_work_branch_off_non_primary_base() {
        // base = a non-primary integration base (e.g. `main`) → prefix records it.
        let b = super::compute_work_branch("main", "close-gate-windows", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "main_close-gate-windows");
    }

    #[test]
    fn compute_work_branch_falls_back_to_intent_slug() {
        // No spec → the intent is slugified (pt-BR strips accents by default).
        let b = super::compute_work_branch("main", "", Some("Corrigir botão de login"), "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "main_corrigir-botao-login");
    }

    #[test]
    fn compute_work_branch_date_fallback_when_no_spec_or_intent() {
        // No spec, no intent → date-from-ts + short session id.
        let b = super::compute_work_branch("dev", "", None, "sess-abcdef1234", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "dev_2026-07-02-sess-abc");
    }

    #[test]
    fn compute_work_branch_sanitizes_unsafe_slug() {
        // A spec with unsafe chars is sanitised into a valid ref.
        let b = super::compute_work_branch("dev", "weird ..slug/", None, "unknown", "2026-07-02T10:00:00.000Z", "/no/project");
        // ".." collapsed, spaces mapped to '-', trailing '/' trimmed.
        assert_eq!(b, "dev_weird--slug");
        assert!(!b.contains(".."), "no `..` runs in a git ref");
        assert!(!b.starts_with('-'), "no leading dash");
    }

    #[test]
    fn resolve_base_honours_requested_when_in_bases_else_primary() {
        // Standard two-tier flow → integration bases {dev, main}, primary = dev.
        let mut config = mustard_core::ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "dev".to_string());
        config.git.flow.insert("dev".to_string(), "main".to_string());
        // A requested base that IS an integration base is used verbatim.
        assert_eq!(super::resolve_base(Some("main"), &config), "main");
        assert_eq!(super::resolve_base(Some("dev"), &config), "dev");
        // A requested base that is NOT an integration base → primary (flow["*"]).
        assert_eq!(super::resolve_base(Some("feature/x"), &config), "dev");
        // No request → primary.
        assert_eq!(super::resolve_base(None, &config), "dev");

        // Agnostic: a develop/master project resolves against ITS bases.
        let mut dm = mustard_core::ProjectConfig::default();
        dm.git.flow.insert("*".to_string(), "develop".to_string());
        dm.git.flow.insert("develop".to_string(), "master".to_string());
        assert_eq!(super::resolve_base(Some("master"), &dm), "master");
        assert_eq!(super::resolve_base(Some("dev"), &dm), "develop", "unknown base → primary");
    }
}
