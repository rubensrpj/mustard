//! `git_provider` — who hosts this repository's pull requests, read from the
//! remote instead of asked at install time.
//!
//! ## Why this exists
//!
//! `mustard init` used to ask "Git provider" from a three-item menu and freeze
//! the answer into `mustard.json#git.provider`. That is the same shape of
//! defect the branch catalogue removed one unit earlier: a fact answered once,
//! on the day of the install, about a repository the operator had often just
//! met — and never revisited afterwards.
//!
//! It is even less necessary here, because the answer is one command away:
//!
//! | remote | provider |
//! |---|---|
//! | `https://github.com/org/repo.git` | `github` |
//! | `https://dev.azure.com/org/proj/_git/repo` | `azure` |
//! | `git@bitbucket.org:team/repo.git` | `bitbucket` |
//!
//! ## Why the setting WINS here, and lost there
//!
//! For branches, git won and `git.flow` was demoted to a hint. Here it is the
//! other way round, and the asymmetry is deliberate: the two facts fail
//! differently.
//!
//! A branch list goes stale because the repository changes every week. The
//! provider hardly ever changes — but detection genuinely FAILS on a
//! self-hosted instance, where the hostname says nothing about the product
//! (a GitHub Enterprise at `git.suzano.com.br` looks like nothing at all).
//! A setting that lost to detection would be useless in exactly the case that
//! justifies its existence.
//!
//! So: an explicit setting wins; otherwise the remote answers; otherwise
//! [`FALLBACK`], which is what every install already assumed.
//!
//! **This only makes the FACT right.** Nothing here changes how Mustard talks
//! to a provider — the pull-request paths still shell out to `gh` directly.
//! Getting the fact right is what an adapter would stand on.
//!
//! ## Contracts honoured
//!
//! - No IO in `domain/`: this is a git probe, so it lives in `platform/`
//!   alongside [`super::git_branches`].
//! - Never errors and never blocks: no git, no remote, an unrecognised host —
//!   each degrades to [`FALLBACK`], the behaviour every install had before.
//! - No `unwrap`/`expect` outside tests; no `println!`.

use std::path::Path;
use std::process::Command;

/// What every install assumed before the provider could be detected, and what
/// an unrecognised remote still answers. Changing this would silently re-route
/// projects that never declared anything.
pub const FALLBACK: &str = "github";

/// The hosts that name a provider, longest-qualified first so a subdomain match
/// cannot be stolen by a shorter one.
///
/// Deliberately a small, literal table: a host either belongs to the vendor's
/// own service or it does not, and a heuristic that guessed beyond this is what
/// would route a self-hosted GitLab to `github`. What the table cannot answer is
/// answered by the setting, not by a cleverer rule.
const KNOWN_HOSTS: &[(&str, &str)] = &[
    ("dev.azure.com", "azure"),
    ("ssh.dev.azure.com", "azure"),
    ("visualstudio.com", "azure"),
    ("github.com", "github"),
    ("gitlab.com", "gitlab"),
    ("bitbucket.org", "bitbucket"),
];

/// The host part of a git remote URL, lower-cased — `None` when the string is
/// not a shape this understands.
///
/// Handles the three spellings a remote really takes: `https://host/path`,
/// `ssh://git@host/path`, and the scp-like `git@host:path` that has no scheme
/// at all and is what most SSH remotes look like.
fn host_of(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let after_scheme = match url.split_once("://") {
        Some((_, rest)) => rest,
        // scp-like: `git@host:org/repo.git`. The colon separates host from
        // path here, which is why it cannot be parsed as a URL.
        None => url,
    };
    let authority = after_scheme
        .split(['/', ':'])
        .next()
        .filter(|s| !s.is_empty())?;
    let host = authority.rsplit('@').next().filter(|s| !s.is_empty())?;
    Some(host.to_ascii_lowercase())
}

/// The provider a remote URL names, or `None` when its host is not one this
/// recognises — a self-hosted instance, or no remote at all.
#[must_use]
pub fn provider_of_url(url: &str) -> Option<&'static str> {
    let host = host_of(url)?;
    KNOWN_HOSTS.iter().find_map(|(known, provider)| {
        (host == *known || host.ends_with(&format!(".{known}"))).then_some(*provider)
    })
}

/// The provider detected from this repository's `origin` remote, or `None` when
/// git could not answer or the host is not recognised.
#[must_use]
pub fn detect_provider(root: &Path) -> Option<&'static str> {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    provider_of_url(&String::from_utf8_lossy(&out.stdout))
}

/// The provider in force for `root`, given whatever `mustard.json` declared.
///
/// The ONE place the precedence is spelled, so a caller cannot implement half
/// of it: `declared` (non-empty, trimmed) → the remote → [`FALLBACK`].
#[must_use]
pub fn resolve_provider(root: &Path, declared: &str) -> String {
    let declared = declared.trim();
    if !declared.is_empty() {
        return declared.to_ascii_lowercase();
    }
    detect_provider(root).unwrap_or(FALLBACK).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_with_remote(dir: &Path, url: &str) -> bool {
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
        git(&["init", "-q", "."]) && git(&["remote", "add", "origin", url])
    }

    /// AC-1 — the fact comes from the repository, and the case that motivated
    /// the unit is the one the old three-item menu could not even offer.
    #[test]
    fn the_provider_comes_from_the_remote_url() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        if !repo_with_remote(root, "https://dev.azure.com/suzano/florestal/_git/portal") {
            return; // no usable git here — every path degrades to the fallback
        }
        assert_eq!(resolve_provider(root, ""), "azure");
        assert_eq!(detect_provider(root), Some("azure"));
    }

    /// AC-2 — the setting wins, which is the whole reason it survives. A
    /// self-hosted instance is unrecognisable by host, so the operator's word
    /// is the only source there is.
    #[test]
    fn an_explicit_setting_overrides_detection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        if !repo_with_remote(root, "https://github.com/org/repo.git") {
            return;
        }
        assert_eq!(resolve_provider(root, ""), "github", "detected when nothing is declared");
        assert_eq!(
            resolve_provider(root, "gitlab"),
            "gitlab",
            "a declared value wins even when the remote says otherwise",
        );
        assert_eq!(resolve_provider(root, "  AZURE  "), "azure", "trimmed and lower-cased");

        // The case the override exists FOR: a host that names no product.
        let hosted = tempfile::tempdir().unwrap();
        if repo_with_remote(hosted.path(), "https://git.suzano.com.br/time/repo.git") {
            assert_eq!(detect_provider(hosted.path()), None, "an unknown host answers nothing");
            assert_eq!(
                resolve_provider(hosted.path(), ""),
                FALLBACK,
                "…and nothing degrades to what every install already assumed",
            );
            assert_eq!(resolve_provider(hosted.path(), "github"), "github", "declared, so known");
        }
    }

    /// The three URL spellings a remote really takes, including the scp-like
    /// one that has no scheme and cannot be parsed as a URL.
    #[test]
    fn every_remote_spelling_yields_its_host() {
        for (url, expected) in [
            ("https://github.com/org/repo.git", Some("github")),
            ("http://github.com/org/repo.git", Some("github")),
            ("ssh://git@ssh.dev.azure.com/v3/org/proj/repo", Some("azure")),
            ("git@bitbucket.org:team/repo.git", Some("bitbucket")),
            ("git@gitlab.com:team/repo.git", Some("gitlab")),
            ("https://org.visualstudio.com/proj/_git/repo", Some("azure")),
            ("https://git.suzano.com.br/time/repo.git", None),
            ("/caminho/local/sem/remoto", None),
            ("", None),
        ] {
            assert_eq!(provider_of_url(url), expected, "for {url:?}");
        }
    }

    /// A host that merely CONTAINS a known name is not that provider — the
    /// match is on the host or one of its subdomains, never on a substring.
    #[test]
    fn a_lookalike_host_is_not_the_provider() {
        assert_eq!(provider_of_url("https://github.com.evil.example/org/repo"), None);
        assert_eq!(provider_of_url("https://notgithub.com/org/repo"), None);
        // …but a real subdomain is.
        assert_eq!(provider_of_url("https://vs-ssh.visualstudio.com/v3/org/p/r"), Some("azure"));
    }
}
