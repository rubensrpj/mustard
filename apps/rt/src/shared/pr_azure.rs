//! `pr_azure` — the Azure DevOps adapter behind the
//! [`crate::shared::pr_provider::PrProvider`] port, speaking the Git REST API
//! (7.1) directly instead of shelling out to a CLI nobody has installed.
//!
//! ## The three load-bearing choices
//!
//! - **The credential is the git credential.** The PAT comes from
//!   `AZURE_DEVOPS_EXT_PAT` when set (the `az` CLI's own convention, kept as a
//!   deliberate override), otherwise from `git credential fill` with
//!   `GIT_TERMINAL_PROMPT=0` — the SAME vault that authenticates a `git push`,
//!   so wherever the push works, this adapter works with zero new
//!   configuration (with `credential.https://dev.azure.com.useHttpPath=true`
//!   the vault keys per repository path, which is why the fill is asked with
//!   the full https remote URL). With neither source, the refusal NAMES both.
//! - **Every URL is DERIVED from the `origin` remote** — the REST base, the
//!   https remote the credential is asked for, and the PR web URL a door
//!   prints. Response fields that carry URLs are ignored on purpose: the
//!   remote is the local fact we already have, and reading the response would
//!   create a second path of truth.
//! - **There is NO merge/complete operation** — not here and not on the port.
//!   That mirrors the operator's real permission boundary (open and update a
//!   PR, never merge it), so the restriction is architecture, not
//!   configuration.
//!
//! ## Transport
//!
//! The adapter talks to the REST API through the [`AzureTransport`] trait: the
//! real implementation uses `ureq` (already a workspace dependency — a
//! blocking client with no async runtime), the tests use a table-driven fake
//! that records every request — the same design as `branch_state`'s `FakePr`.
//! No test ever touches the network.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

use crate::shared::pr_provider::{
    short_ref, status_from_azure, PrOpened, PrProvider, PrToOpen, PrView, HEADS, PROVIDER_AZURE,
};

/// The Azure DevOps REST API version every call pins. One spelling, so a bump
/// is one edit.
const API_VERSION: &str = "7.1";

/// The env var that overrides the git credential — the official `az devops`
/// CLI's own convention, so an operator who already exports it for `az` needs
/// nothing new. `pub(crate)` because the AC-pinned tests in
/// [`crate::shared::pr_provider`] assert the refusal names it.
pub(crate) const PAT_ENV: &str = "AZURE_DEVOPS_EXT_PAT";

/// HTTP deadline per call. Generous enough for a PR create on a slow proxy,
/// short enough that a dead network answers as an error instead of a hang.
const HTTP_TIMEOUT_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// Base64 — the ~20 encode-only lines Basic auth needs
// ---------------------------------------------------------------------------

/// RFC 4648 base64, ENCODE ONLY. The workspace has no base64 crate and the
/// one need is encoding `:PAT` for the `Authorization: Basic` header — a
/// dependency would be more surface than these lines.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk.first().copied().unwrap_or(0);
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        let sextet = |shift: u32| ALPHABET[((n >> shift) & 63) as usize] as char;
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// The `Authorization` header value for a PAT: Basic auth with an empty user,
/// exactly what the Azure DevOps REST reference prescribes.
fn basic_auth(pat: &str) -> String {
    format!("Basic {}", base64(format!(":{pat}").as_bytes()))
}

// ---------------------------------------------------------------------------
// The remote — org / project / repo, read out of `origin`
// ---------------------------------------------------------------------------

/// The one Azure repository the `origin` remote names, plus the web base its
/// spelling implies. Every URL the adapter uses is derived from THIS — never
/// read back out of a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AzureRemote {
    /// `https://dev.azure.com/{org}` or `https://{org}.visualstudio.com` —
    /// whichever host family the remote itself spoke.
    base: String,
    project: String,
    repo: String,
}

impl AzureRemote {
    /// Parse the three spellings an Azure remote really takes:
    ///
    /// - https: `https://dev.azure.com/{org}/{project}/_git/{repo}`
    ///   (optionally `{user}@` before the host)
    /// - ssh v3: `git@ssh.dev.azure.com:v3/{org}/{project}/{repo}` and the
    ///   `ssh://` spelling of the same path (plus the legacy
    ///   `vs-ssh.{...}.visualstudio.com` host)
    /// - legacy https: `https://{org}.visualstudio.com/[DefaultCollection/]{project}/_git/{repo}`
    ///
    /// Anything else — another provider, a bare path, an empty string — is
    /// `None`: this adapter refuses to guess an org out of a host it does not
    /// recognise.
    pub(crate) fn parse(url: &str) -> Option<Self> {
        let url = url.trim();
        let (authority, path) = match url.split_once("://") {
            Some((_, rest)) => rest.split_once('/')?,
            // scp-like: `git@host:path` — no scheme, colon splits host/path.
            None => url.split_once(':')?,
        };
        let host = authority.rsplit('@').next()?.to_ascii_lowercase();
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        if host == "ssh.dev.azure.com" || host.starts_with("vs-ssh.") {
            // v3/{org}/{project}/{repo}
            if segments.first() != Some(&"v3") || segments.len() != 4 {
                return None;
            }
            let (org, project, repo) = (segments[1], segments[2], segments[3]);
            let base = if host == "ssh.dev.azure.com" {
                format!("https://dev.azure.com/{org}")
            } else {
                // vs-ssh.visualstudio.com — the legacy host family keeps its
                // own web base so derived URLs match where the repo lives.
                format!("https://{org}.visualstudio.com")
            };
            return Some(Self { base, project: project.to_string(), repo: strip_git(repo) });
        }

        if host == "dev.azure.com" || host.ends_with(".visualstudio.com") {
            // {org}/{project}/_git/{repo} or [DefaultCollection/]{project}/_git/{repo}
            let git_at = segments.iter().position(|s| *s == "_git")?;
            let repo = segments.get(git_at + 1)?;
            let project = segments.get(git_at.checked_sub(1)?)?;
            let base = if host == "dev.azure.com" {
                format!("https://dev.azure.com/{}", segments.first()?)
            } else {
                format!("https://{host}")
            };
            return Some(Self {
                base,
                project: (*project).to_string(),
                repo: strip_git(repo),
            });
        }

        None
    }

    /// The REST collection every PR call addresses.
    pub(crate) fn api_pulls(&self) -> String {
        format!(
            "{}/{}/_apis/git/repositories/{}/pullrequests",
            self.base, self.project, self.repo
        )
    }

    /// The canonical https remote — what `git credential fill` is asked for,
    /// so a `useHttpPath=true` vault keys on the same path the push uses.
    pub(crate) fn https_remote(&self) -> String {
        format!("{}/{}/_git/{}", self.base, self.project, self.repo)
    }

    /// The PR's web URL — DERIVED, the one a door prints for the operator.
    pub(crate) fn pr_url(&self, number: u64) -> String {
        format!("{}/{}/_git/{}/pullrequest/{number}", self.base, self.project, self.repo)
    }
}

/// A repo segment without a trailing `.git` — rare on Azure remotes, but a
/// hand-edited remote may carry it.
fn strip_git(repo: &str) -> String {
    repo.strip_suffix(".git").unwrap_or(repo).to_string()
}

// ---------------------------------------------------------------------------
// The credential — env override, then the git vault, then an honest refusal
// ---------------------------------------------------------------------------

/// The precedence, pure so it is testable without touching env or git:
/// `env_pat` (non-blank) wins as the deliberate override; otherwise whatever
/// `fill` answers; otherwise a refusal that NAMES both sources, so the
/// operator knows the two ways to fix it. `pub(crate)` for the AC-pinned test
/// in [`crate::shared::pr_provider`].
pub(crate) fn pat_from(
    env_pat: Option<String>,
    fill: impl FnOnce() -> Option<String>,
    remote_url: &str,
) -> Result<String, String> {
    if let Some(pat) = env_pat {
        let pat = pat.trim().to_string();
        if !pat.is_empty() {
            return Ok(pat);
        }
    }
    if let Some(pat) = fill() {
        return Ok(pat);
    }
    Err(format!(
        "azure-credential-missing: set {PAT_ENV} or store a git credential for {remote_url} \
         (git credential fill answered nothing)"
    ))
}

/// Ask the git credential vault for `url` — the SAME lookup a `git push` to
/// that remote performs. `GIT_TERMINAL_PROMPT=0` so an empty vault answers
/// as a failure instead of an interactive prompt hanging the command.
fn credential_fill(root: &Path, url: &str) -> Option<String> {
    let mut child = Command::new("git")
        .args(["credential", "fill"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(format!("url={url}\n\n").as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("password="))
        .map(str::to_string)
        .filter(|pat| !pat.is_empty())
}

/// The credential in force for this remote: [`PAT_ENV`] → the git vault →
/// refusal. The one place the real sources are wired to [`pat_from`].
fn resolve_pat(root: &Path, remote_url: &str) -> Result<String, String> {
    pat_from(std::env::var(PAT_ENV).ok(), || credential_fill(root, remote_url), remote_url)
}

// ---------------------------------------------------------------------------
// The transport — injectable, so no test touches the network
// ---------------------------------------------------------------------------

/// How the adapter reaches the REST API. `method` is `GET`/`POST`/`PATCH`
/// (the ONLY verbs the port's operations need — no DELETE, and deliberately
/// nothing that could complete a merge), `auth` the ready `Authorization`
/// value, `body` the JSON document for the writing verbs. Answers the parsed
/// response body, or the reason as a stable-prefixed string.
pub(crate) trait AzureTransport {
    fn call(
        &self,
        method: &str,
        url: &str,
        auth: &str,
        body: Option<&Value>,
    ) -> Result<Value, String>;
}

/// The real transport: `ureq`, blocking, one agent per call (each command
/// invocation performs one or two calls — pooling would outlive the process).
/// Non-2xx statuses are read for Azure's `message` field so the operator sees
/// the server's own words, not just a number.
struct UreqTransport;

impl AzureTransport for UreqTransport {
    fn call(
        &self,
        method: &str,
        url: &str,
        auth: &str,
        body: Option<&Value>,
    ) -> Result<Value, String> {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(HTTP_TIMEOUT_SECS)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let sent = match (method, body) {
            ("GET", None) => agent
                .get(url)
                .header("Authorization", auth)
                .header("Accept", "application/json")
                .call(),
            ("POST", Some(doc)) => agent
                .post(url)
                .header("Authorization", auth)
                .header("Content-Type", "application/json")
                .send_json(doc),
            ("PATCH", Some(doc)) => agent
                .patch(url)
                .header("Authorization", auth)
                .header("Content-Type", "application/json")
                .send_json(doc),
            _ => return Err(format!("azure-transport: unsupported request shape {method}")),
        };
        let mut response = sent.map_err(|e| format!("azure-transport: {e}"))?;
        let status = response.status();
        let doc = response.body_mut().read_json::<Value>().unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(doc);
        }
        let message = doc.get("message").and_then(Value::as_str).unwrap_or_default();
        Err(if message.is_empty() {
            format!("azure-http-{}", status.as_u16())
        } else {
            format!("azure-http-{}: {message}", status.as_u16())
        })
    }
}

// ---------------------------------------------------------------------------
// The operations — pure over (remote, transport, auth), so the fake proves them
// ---------------------------------------------------------------------------

/// Read one `GitPullRequest` document into the port's [`PrView`]. Refs come
/// out short, the status word is reduced to the canonical vocabulary,
/// `mergeStatus` travels verbatim — and the URL is DERIVED from the remote,
/// never read from the response.
fn view_from_azure(remote: &AzureRemote, row: &Value) -> Result<PrView, String> {
    let Some(number) = row.get("pullRequestId").and_then(Value::as_u64) else {
        // A document without an id is not a pull request — never "PR 0".
        return Err("parse-error".to_string());
    };
    let text = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default();
    Ok(PrView {
        number,
        title: text("title").to_string(),
        head: short_ref(text("sourceRefName")).to_string(),
        base: short_ref(text("targetRefName")).to_string(),
        status: status_from_azure(text("status")),
        merge_status: row.get("mergeStatus").and_then(Value::as_str).map(str::to_string),
        draft: row.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        url: remote.pr_url(number),
    })
}

/// POST the create. Branch names arrive short (the port's contract) and leave
/// as the FULL refs the REST contract demands; the answered URL is derived.
/// `pub(crate)` for the AC-pinned test in [`crate::shared::pr_provider`].
pub(crate) fn do_open(
    remote: &AzureRemote,
    transport: &dyn AzureTransport,
    auth: &str,
    pr: &PrToOpen,
) -> Result<PrOpened, String> {
    let url = format!("{}?api-version={API_VERSION}", remote.api_pulls());
    let body = json!({
        "sourceRefName": format!("{HEADS}{}", pr.head),
        "targetRefName": format!("{HEADS}{}", pr.base),
        "title": pr.title,
        "description": pr.body,
        "isDraft": pr.draft,
    });
    let doc = transport.call("POST", &url, auth, Some(&body))?;
    let number = doc
        .get("pullRequestId")
        .and_then(Value::as_u64)
        .ok_or_else(|| "parse-error".to_string())?;
    Ok(PrOpened { number, url: remote.pr_url(number) })
}

/// PATCH one field of PR `number`. `edit_body` and `ready` are both this —
/// the REST contract updates a PR by PATCHing the fields that change.
fn do_patch(
    remote: &AzureRemote,
    transport: &dyn AzureTransport,
    auth: &str,
    number: u64,
    body: &Value,
) -> Result<(), String> {
    let url = format!("{}/{number}?api-version={API_VERSION}", remote.api_pulls());
    transport.call("PATCH", &url, auth, Some(body)).map(|_| ())
}

/// GET PR `number`, normalised. `pub(crate)` for the AC-pinned test in
/// [`crate::shared::pr_provider`].
pub(crate) fn do_view_number(
    remote: &AzureRemote,
    transport: &dyn AzureTransport,
    auth: &str,
    number: u64,
) -> Result<PrView, String> {
    let url = format!("{}/{number}?api-version={API_VERSION}", remote.api_pulls());
    view_from_azure(remote, &transport.call("GET", &url, auth, None)?)
}

/// GET the PR whose head is `branch` — the port's `view(None)` shape, asked
/// via `searchCriteria.sourceRefName` with the FULL ref. `status=all` so a
/// just-completed PR is still viewable, matching what `gh pr view` answers on
/// GitHub; the first row is the newest, which is the one a door wants.
/// Percent-encode one query-string VALUE. Without this, a git-legal branch
/// carrying `+` or `%` is misdecoded server-side and answers a false
/// `no-pr-for-branch` — a plus sign reads as a space under form decoding.
/// `/` stays literal: it is legal in a query value and the full ref reads
/// better in logs.
fn query_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn do_view_branch(
    remote: &AzureRemote,
    transport: &dyn AzureTransport,
    auth: &str,
    branch: &str,
) -> Result<PrView, String> {
    let url = format!(
        "{}?searchCriteria.sourceRefName={}&searchCriteria.status=all&api-version={API_VERSION}",
        remote.api_pulls(),
        query_encode(&format!("{HEADS}{branch}")),
    );
    let doc = transport.call("GET", &url, auth, None)?;
    let rows = doc
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "parse-error".to_string())?;
    let row = rows.first().ok_or_else(|| format!("no-pr-for-branch: {branch}"))?;
    view_from_azure(remote, row)
}

// ---------------------------------------------------------------------------
// The adapter — context resolution wired onto the pure operations
// ---------------------------------------------------------------------------

/// Run `git` in `root` and return its trimmed stdout — the same degradation
/// shape as the GitHub adapter's `gh_out`, for the same reason.
fn git_out(root: &Path, args: &[&str]) -> Result<String, String> {
    let Ok(out) = Command::new("git").args(args).current_dir(root).output() else {
        return Err("git-not-found".to_string());
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() { "git-failed".to_string() } else { stderr });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The Azure DevOps adapter — REST over the injectable transport, credential
/// from the git vault (or the env override), every URL derived from `origin`.
///
/// Deliberately NO merge/complete operation, matching both the port (which
/// does not offer one) and the operator's real permission boundary: open and
/// update a PR, never merge it.
pub(crate) struct AzurePrRest {
    repo: PathBuf,
    transport: Box<dyn AzureTransport>,
}

impl AzurePrRest {
    /// Bind the adapter to one repository root with the real transport.
    pub(crate) fn new(repo: &Path) -> Self {
        Self { repo: repo.to_path_buf(), transport: Box::new(UreqTransport) }
    }

    /// Resolve what every operation needs: the remote (parsed out of
    /// `origin`) and the ready `Authorization` value. Each failure names its
    /// own stable token, so a door can tell "not an Azure remote" apart from
    /// "no credential".
    fn context(&self) -> Result<(AzureRemote, String), String> {
        let url = git_out(&self.repo, &["remote", "get-url", "origin"])
            .map_err(|e| format!("azure-remote-unreadable: {e}"))?;
        let remote = AzureRemote::parse(&url)
            .ok_or_else(|| format!("azure-remote-unrecognized: '{url}' is not an Azure DevOps remote"))?;
        let pat = resolve_pat(&self.repo, &remote.https_remote())?;
        Ok((remote, basic_auth(&pat)))
    }
}

impl PrProvider for AzurePrRest {
    fn provider(&self) -> &str {
        PROVIDER_AZURE
    }

    fn open(&self, pr: &PrToOpen) -> Result<PrOpened, String> {
        let (remote, auth) = self.context()?;
        do_open(&remote, self.transport.as_ref(), &auth, pr)
    }

    fn edit_body(&self, number: u64, body: &str) -> Result<(), String> {
        let (remote, auth) = self.context()?;
        do_patch(&remote, self.transport.as_ref(), &auth, number, &json!({ "description": body }))
    }

    fn ready(&self, number: u64) -> Result<(), String> {
        let (remote, auth) = self.context()?;
        do_patch(&remote, self.transport.as_ref(), &auth, number, &json!({ "isDraft": false }))
    }

    fn view(&self, number: Option<u64>) -> Result<PrView, String> {
        let (remote, auth) = self.context()?;
        match number {
            Some(n) => do_view_number(&remote, self.transport.as_ref(), &auth, n),
            None => {
                let branch = git_out(&self.repo, &["rev-parse", "--abbrev-ref", "HEAD"])
                    .map_err(|e| format!("azure-branch-unreadable: {e}"))?;
                do_view_branch(&remote, self.transport.as_ref(), &auth, &branch)
            }
        }
    }
}

/// Test-only fixtures shared with the AC-pinned tests that live in
/// [`crate::shared::pr_provider`]'s tests module — ONE fake transport, never
/// two copies drifting apart.
#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use serde_json::Value;

    use super::{AzureRemote, AzureTransport};

    /// One request as the fake saw it — everything the adapter chose.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct Recorded {
        pub(crate) method: String,
        pub(crate) url: String,
        pub(crate) auth: String,
        pub(crate) body: Option<Value>,
    }

    /// The table-driven fake transport: answers by exact `(method, url)` key
    /// and records every request — `branch_state::FakePr`'s design, applied
    /// to HTTP. A request outside the table is an error naming itself, so a
    /// drifted URL fails loudly instead of passing vacuously.
    pub(crate) struct FakeTransport {
        responses: BTreeMap<(String, String), Value>,
        pub(crate) calls: RefCell<Vec<Recorded>>,
    }

    impl FakeTransport {
        pub(crate) fn of(rows: &[(&str, &str, Value)]) -> Self {
            Self {
                responses: rows
                    .iter()
                    .map(|(m, u, v)| (((*m).to_string(), (*u).to_string()), v.clone()))
                    .collect(),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl AzureTransport for FakeTransport {
        fn call(
            &self,
            method: &str,
            url: &str,
            auth: &str,
            body: Option<&Value>,
        ) -> Result<Value, String> {
            self.calls.borrow_mut().push(Recorded {
                method: method.to_string(),
                url: url.to_string(),
                auth: auth.to_string(),
                body: body.cloned(),
            });
            self.responses
                .get(&(method.to_string(), url.to_string()))
                .cloned()
                .ok_or_else(|| format!("unexpected request: {method} {url}"))
        }
    }

    /// The canonical parsed remote every operation test stands on.
    pub(crate) fn remote() -> AzureRemote {
        AzureRemote::parse("https://dev.azure.com/suzano/florestal/_git/portal")
            .expect("a canonical https remote parses")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{remote, FakeTransport};
    use super::*;
    use crate::shared::branch_state::PrStatus;

    /// T4 — the RFC 4648 vectors, plus the one composition the module exists
    /// for: Basic auth of `:PAT`.
    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "for {input:?}");
        }
        assert_eq!(basic_auth("abc"), "Basic OmFiYw==", "empty user, colon, PAT");
    }

    /// A remote that is NOT Azure is refused, never guessed at — the factory
    /// only routes here on an azure provider, but a declared override can
    /// point it at any remote.
    #[test]
    fn a_foreign_remote_is_refused() {
        for url in [
            "https://github.com/org/repo.git",
            "git@gitlab.com:team/repo.git",
            "https://dev.azure.com/suzano",
            "/local/path/no/remote",
            "",
        ] {
            assert_eq!(AzureRemote::parse(url), None, "for {url:?}");
        }
    }

    /// T3 — edit_body and ready are both one PATCH of the field that changes,
    /// addressed to the PR by number.
    #[test]
    fn edit_body_and_ready_patch_one_field_each() {
        let remote = remote();
        let patch_url = format!("{}/7?api-version=7.1", remote.api_pulls());
        let fake = FakeTransport::of(&[("PATCH", &patch_url, json!({ "pullRequestId": 7 }))]);

        do_patch(&remote, &fake, "a", 7, &json!({ "description": "new body" }))
            .expect("edit succeeds");
        do_patch(&remote, &fake, "a", 7, &json!({ "isDraft": false })).expect("ready succeeds");

        let calls = fake.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].body, Some(json!({ "description": "new body" })));
        assert_eq!(calls[1].body, Some(json!({ "isDraft": false })));
    }

    /// T3 — view(None)'s shape: the current branch is asked for via
    /// `searchCriteria.sourceRefName` with the FULL ref, the first row
    /// answers, and an empty answer is "no PR", distinct from a broken one.
    #[test]
    fn a_hostile_branch_name_is_percent_encoded_in_the_search() {
        assert_eq!(query_encode("refs/heads/hot+fix%2 x"), "refs/heads/hot%2Bfix%252%20x");
        assert_eq!(query_encode("refs/heads/feature/plain-1.2~ok"), "refs/heads/feature/plain-1.2~ok");
    }

    #[test]
    fn view_by_branch_searches_the_full_source_ref() {
        let remote = remote();
        let search_url = format!(
            "{}?searchCriteria.sourceRefName=refs/heads/feature/my-unit&searchCriteria.status=all&api-version=7.1",
            remote.api_pulls()
        );
        let fake = FakeTransport::of(&[(
            "GET",
            &search_url,
            json!({ "count": 1, "value": [{
                "pullRequestId": 11,
                "title": "t",
                "status": "completed",
                "sourceRefName": "refs/heads/feature/my-unit",
                "targetRefName": "refs/heads/dev",
            }]}),
        )]);
        let view =
            do_view_branch(&remote, &fake, "a", "feature/my-unit").expect("search finds the PR");
        assert_eq!(view.number, 11);
        assert_eq!(view.status, PrStatus::Merged, "status=all keeps a landed PR viewable");

        let empty = FakeTransport::of(&[("GET", &search_url, json!({ "count": 0, "value": [] }))]);
        assert_eq!(
            do_view_branch(&remote, &empty, "a", "feature/my-unit"),
            Err("no-pr-for-branch: feature/my-unit".to_string()),
            "an empty answer is 'no PR', not a parse error",
        );

        let broken = FakeTransport::of(&[("GET", &search_url, json!({ "count": 0 }))]);
        assert_eq!(
            do_view_branch(&remote, &broken, "a", "feature/my-unit"),
            Err("parse-error".to_string()),
            "a document without `value` could not be read — never 'no PR'",
        );
    }

    /// A transport failure travels to the caller untouched — the operations
    /// add nothing to what the transport already named.
    #[test]
    fn a_transport_failure_travels_verbatim() {
        let remote = remote();
        let fake = FakeTransport::of(&[]);
        let answer = do_view_number(&remote, &fake, "a", 1).expect_err("nothing in the table");
        assert!(answer.starts_with("unexpected request: GET"), "{answer}");
    }
}
