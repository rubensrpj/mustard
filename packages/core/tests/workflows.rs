//! The verification, the release and the version workflows, read as files —
//! with the installer scripts the release runs.
//!
//! GitHub runs what is under `.github/workflows/`, and nothing on this side
//! ever executes it, so what those files promise is checked here by reading
//! them. What they promise today:
//!
//! - the verification (`ci.yml`) runs on every pull request and on every push
//!   to `dev` and `main`;
//! - only those push runs save the compiled build; pull requests, manual runs
//!   and the release (`release.yml`) restore it and save nothing, and every
//!   Rust cache step names the same shared key;
//! - no job of any workflow runs past half an hour;
//! - the release builds neither the dashboard nor the memory server;
//! - the three installers bring rtk in the fixed version of `checksums.txt`,
//!   checked against its sum, the release stops without it, and no step
//!   compiles rtk.
//!
//! Each promise is a function that answers with the problems it finds. A
//! fixture proves the function refuses what it should, and the test over the
//! real files only asks for an empty list.
//!
//! The workflows are read by a YAML library (`yaml-rust2`, YAML 1.2, so `on:`
//! stays a key), a test-only dependency. The installer scripts are shell and
//! PowerShell, and are read as text.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use yaml_rust2::{Yaml, YamlLoader};

/// The verification every pull request and every push to `dev` and `main` runs.
const VERIFICATION: &str = ".github/workflows/ci.yml";

/// The release: builds the installers and publishes them on a version tag.
const RELEASE: &str = ".github/workflows/release.yml";

/// Where every workflow lives. Each one found there is held to the time cap.
const WORKFLOWS_DIR: &str = ".github/workflows";

/// The only branches whose runs save the compiled build, sorted.
const SAVING_BRANCHES: [&str; 2] = ["dev", "main"];

/// The longest a job may run. Without a cap a hung step holds the runner for
/// GitHub's six-hour default; the slowest job measured took 16 minutes.
const LONGEST_JOB_MINUTES: i64 = 30;

/// The Rust cache action, compared without case: the two files spell its
/// owner differently.
const RUST_CACHE: &str = "swatinem/rust-cache@";

/// The toolchain action. It bundles the same Rust cache, and left on, that one
/// saves on every run whatever the cache step says.
const RUST_TOOLCHAIN: &str = "actions-rust-lang/setup-rust-toolchain@";

/// The general cache actions that save at the end of every run using them.
/// Only the `restore` half may appear.
const SAVING_CACHE_ACTIONS: [&str; 2] = ["actions/cache@", "actions/cache/save@"];

/// The verification cache's `save-if`: true on a push run, false otherwise.
const SAVE_ON_PUSH: &str = "${{ github.event_name == 'push' }}";

/// What only building the dashboard or the memory server needs, in a step's
/// action or command.
const DASHBOARD_OR_MEMORY: [&str; 5] = ["pnpm", "setup-node", "dashboard", "mustard-mcp", "memory-server"];

/// The file that fixes rtk's version and the sum of each of its packages.
const CHECKSUMS: &str = "checksums.txt";

/// The rtk packages the three installers download, one line each in
/// [`CHECKSUMS`].
const RTK_PACKAGES: [&str; 4] = [
    "rtk-x86_64-unknown-linux-musl.tar.gz",
    "rtk-x86_64-apple-darwin.tar.gz",
    "rtk-aarch64-apple-darwin.tar.gz",
    "rtk-x86_64-pc-windows-msvc.zip",
];

/// The Linux and the macOS installer scripts, and the one the Windows job and
/// the Linux job run.
const LINUX_SCRIPT: &str = "packaging/linux/build-deb.sh";
const MACOS_SCRIPT: &str = "packaging/macos/build-pkg.sh";
const PACKAGES_SCRIPT: &str = "packaging/build-packages.ps1";

// ---------------------------------------------------------------------------
// The reader
// ---------------------------------------------------------------------------

/// `node` when it is there: indexing a YAML node that has no such key answers
/// a bad value, never a panic.
fn present(node: &Yaml) -> Option<&Yaml> {
    (!node.is_badvalue()).then_some(node)
}

/// A scalar as the workflow means it: a string, a number or a boolean, in the
/// words they were written with.
fn scalar(node: &Yaml) -> Option<String> {
    match node {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(n) => Some(n.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A node read as a list of scalars: a sequence, or one scalar alone.
fn list(node: &Yaml) -> Vec<String> {
    match node {
        Yaml::Array(items) => items.iter().filter_map(scalar).collect(),
        other => scalar(other).into_iter().collect(),
    }
}

/// The keys of a mapping node, in file order.
fn keys(node: &Yaml) -> Vec<String> {
    node.as_hash().map(|hash| hash.keys().filter_map(scalar).collect()).unwrap_or_default()
}

/// One step of one job.
struct Step<'a> {
    job: String,
    node: &'a Yaml,
}

impl Step<'_> {
    /// The step's value under `key`, as text.
    fn text(&self, key: &str) -> Option<String> {
        present(&self.node[key]).and_then(scalar)
    }

    /// The action the step uses, lowercased.
    fn uses(&self) -> String {
        self.text("uses").unwrap_or_default().to_ascii_lowercase()
    }

    /// Everything the step says: its name, its action and its command.
    fn said(&self) -> String {
        ["name", "uses", "run"].iter().filter_map(|k| self.text(k)).collect::<Vec<_>>().join("\n")
    }
}

/// One workflow file, read into its YAML tree.
struct Workflow {
    /// Where the file lives, relative to the workspace root. Every problem
    /// names it.
    rel: String,
    doc: Yaml,
}

impl Workflow {
    /// Read the workflow at `rel`, relative to the workspace root.
    fn read(rel: &str) -> Self {
        Self::parse(rel, &read_text(rel))
    }

    /// Read `raw` as the workflow at `rel`. A file the reader cannot parse
    /// fails the read instead of passing quietly.
    fn parse(rel: &str, raw: &str) -> Self {
        let docs = YamlLoader::load_from_str(raw).unwrap_or_else(|e| panic!("{rel} is not YAML: {e}"));
        let doc = docs.into_iter().next().unwrap_or_else(|| panic!("{rel} is empty"));
        Self { rel: rel.to_string(), doc }
    }

    /// The top-level entry named `key`.
    fn get(&self, key: &str) -> Option<&Yaml> {
        present(&self.doc[key])
    }

    /// Every job, as `(id, node)`, in file order.
    fn jobs(&self) -> Vec<(String, &Yaml)> {
        self.doc["jobs"]
            .as_hash()
            .map(|jobs| jobs.iter().filter_map(|(id, job)| scalar(id).map(|id| (id, job))).collect())
            .unwrap_or_default()
    }

    /// Every step of every job.
    fn steps(&self) -> Vec<Step<'_>> {
        self.jobs()
            .into_iter()
            .flat_map(|(job, node)| {
                node["steps"].as_vec().into_iter().flatten().map(move |step| Step { job: job.clone(), node: step })
            })
            .collect()
    }

    /// The steps whose action starts with `action`, compared without case.
    fn steps_using(&self, action: &str) -> Vec<Step<'_>> {
        self.steps().into_iter().filter(|step| step.uses().starts_with(action)).collect()
    }
}

/// The workspace root: the nearest directory above this crate that holds the
/// verification workflow.
fn workspace_root() -> PathBuf {
    let mut dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    loop {
        if dir.join(VERIFICATION).is_file() {
            return dir.to_path_buf();
        }
        dir = dir
            .parent()
            .unwrap_or_else(|| panic!("no {VERIFICATION} above {}", env!("CARGO_MANIFEST_DIR")));
    }
}

/// A file of the workspace, as text.
fn read_text(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every workflow file, relative to the workspace root, sorted.
fn every_workflow() -> Vec<String> {
    let dir = workspace_root().join(WORKFLOWS_DIR);
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".yml") || name.ends_with(".yaml"))
        .map(|name| format!("{WORKFLOWS_DIR}/{name}"))
        .collect();
    found.sort();
    found
}

// ---------------------------------------------------------------------------
// The promises, each as the list of what breaks it
// ---------------------------------------------------------------------------

/// What keeps the verification from running where the saved build is made and
/// read: every pull request into `dev` and `main`, and pushes to exactly those
/// two branches — a push anywhere else, or a tag push, would be a saving run
/// too.
fn trigger_problems(ci: &Workflow) -> Vec<String> {
    let rel = &ci.rel;
    let Some(on) = ci.get("on") else {
        return vec![format!("{rel}: no `on:` block, so it runs on nothing")];
    };
    let mut problems = Vec::new();
    match present(&on["pull_request"]) {
        None => problems.push(format!(
            "{rel}: no `pull_request` trigger, so nothing is verified before it lands"
        )),
        Some(pr) => {
            let branches = list(&pr["branches"]);
            for branch in SAVING_BRANCHES {
                if !branches.is_empty() && !branches.iter().any(|b| b == branch) {
                    problems.push(format!("{rel}: pull requests into `{branch}` are not verified"));
                }
            }
        }
    }
    match present(&on["push"]) {
        None => problems.push(format!(
            "{rel}: no `push` trigger, so no run saves a build a pull request could restore"
        )),
        Some(push) => {
            let mut branches = list(&push["branches"]);
            branches.sort();
            if branches != SAVING_BRANCHES {
                problems.push(format!(
                    "{rel}: pushes must run on {SAVING_BRANCHES:?} and nowhere else, found {branches:?}"
                ));
            }
            for filter in keys(push).into_iter().filter(|key| key != "branches") {
                problems.push(format!(
                    "{rel}: the `push` trigger also filters on `{filter}`; it must name only its branches"
                ));
            }
        }
    }
    problems
}

/// What lets a run other than a push to `dev` or `main` save the compiled
/// build, or keeps the runs from sharing one cache.
fn cache_problems(ci: &Workflow, release: &Workflow) -> Vec<String> {
    let mut problems = Vec::new();
    let mut shared_keys = BTreeSet::new();
    if ci.steps_using(RUST_CACHE).is_empty() {
        problems.push(format!("{}: no Rust cache step, so no run saves the build", ci.rel));
    }
    for (workflow, save_if) in [(ci, SAVE_ON_PUSH), (release, "false")] {
        let rel = &workflow.rel;
        for step in workflow.steps_using(RUST_CACHE) {
            let with = &step.node["with"];
            let found = present(&with["save-if"]).and_then(scalar);
            if found.as_deref() != Some(save_if) {
                problems.push(format!(
                    "{rel}: job `{}` caches with `save-if: {}`; it must be `{save_if}`",
                    step.job,
                    found.as_deref().unwrap_or("(absent, which saves)")
                ));
            }
            shared_keys.insert(present(&with["shared-key"]).and_then(scalar).unwrap_or_default());
        }
        for step in workflow.steps_using(RUST_TOOLCHAIN) {
            if present(&step.node["with"]["cache"]).and_then(scalar).as_deref() != Some("false") {
                problems.push(format!(
                    "{rel}: job `{}` lets the toolchain action cache on its own, which saves on every run",
                    step.job
                ));
            }
        }
        for action in SAVING_CACHE_ACTIONS {
            for step in workflow.steps_using(action) {
                problems.push(format!("{rel}: job `{}` uses `{action}`, which saves on every run", step.job));
            }
        }
    }
    if shared_keys.len() != 1 || shared_keys.contains("") {
        problems.push(format!("the Rust cache steps must all name one `shared-key`, found {shared_keys:?}"));
    }
    problems
}

/// Every job of `workflow` without a numeric `timeout-minutes` of at most
/// [`LONGEST_JOB_MINUTES`].
fn timeout_problems(workflow: &Workflow) -> Vec<String> {
    let rel = &workflow.rel;
    let mut problems = Vec::new();
    let jobs = workflow.jobs();
    if jobs.is_empty() {
        problems.push(format!("{rel}: no jobs found"));
    }
    for (id, job) in jobs {
        match job["timeout-minutes"].as_i64() {
            Some(minutes) if (1..=LONGEST_JOB_MINUTES).contains(&minutes) => {}
            Some(minutes) => problems.push(format!(
                "{rel}: job `{id}` may run {minutes} minutes; the cap is {LONGEST_JOB_MINUTES}"
            )),
            None => problems.push(format!(
                "{rel}: job `{id}` has no numeric `timeout-minutes`, so a hung step holds the runner for hours"
            )),
        }
    }
    problems
}

/// Every step of the release that builds the dashboard or the memory server.
fn dashboard_problems(release: &Workflow) -> Vec<String> {
    let mut problems = Vec::new();
    for step in release.steps() {
        let said = step.said().to_ascii_lowercase();
        if let Some(word) = DASHBOARD_OR_MEMORY.iter().find(|w| said.contains(**w)) {
            problems.push(format!(
                "{}: job `{}` has a step about `{word}`; the installers build neither the dashboard nor the memory server",
                release.rel, step.job
            ));
        }
    }
    problems
}

/// The texts the installer check reads, by name, so a fixture can replace any
/// of them.
struct Installers {
    release: Workflow,
    checksums: String,
    linux: String,
    macos: String,
    packages: String,
}

impl Installers {
    fn read() -> Self {
        Self {
            release: Workflow::read(RELEASE),
            checksums: read_text(CHECKSUMS),
            linux: read_text(LINUX_SCRIPT),
            macos: read_text(MACOS_SCRIPT),
            packages: read_text(PACKAGES_SCRIPT),
        }
    }
}

/// What keeps the three installers from bringing rtk in the fixed version,
/// checked against its sum, with the release stopping without it and no step
/// compiling it.
fn installer_problems(set: &Installers) -> Vec<String> {
    let mut problems = Vec::new();
    let rel = &set.release.rel;

    // The file that fixes the version and the sums.
    let version = set
        .checksums
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("# rtk v"))
        .and_then(|rest| rest.split_whitespace().next())
        .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'));
    if version.is_none() {
        problems.push(format!("{CHECKSUMS}: the first line must say `# rtk v<version>`"));
    }
    for package in RTK_PACKAGES {
        let summed = set.checksums.lines().any(|line| {
            line.strip_suffix(package)
                .and_then(|head| head.strip_suffix("  "))
                .is_some_and(|sum| sum.len() == 64 && sum.chars().all(|c| c.is_ascii_hexdigit()))
        });
        if !summed {
            problems.push(format!("{CHECKSUMS}: no sum for `{package}`"));
        }
    }

    // No step compiles rtk, and none lets its absence pass.
    for step in set.release.steps() {
        let said = step.said();
        let lowered = said.to_ascii_lowercase();
        if lowered.contains("rtk") && (lowered.contains("cargo install") || lowered.contains("install.sh")) {
            problems.push(format!("{rel}: job `{}` compiles or installs rtk outside the fixed release", step.job));
        }
        if lowered.contains("rtk") && step.text("continue-on-error").as_deref() == Some("true") {
            problems.push(format!("{rel}: job `{}` lets the release go on without rtk", step.job));
        }
        if lowered.contains("command -v rtk") || lowered.contains("rtk ausente") {
            problems.push(format!("{rel}: job `{}` ships whatever rtk the runner happens to have", step.job));
        }
    }

    // Each installer takes its rtk from the checked download.
    let job_says = |job: &str, needle: &str| {
        set.release.steps().iter().any(|step| step.job == job && step.said().contains(needle))
    };
    for (job, needle) in [
        ("windows-installer", "build-packages.ps1 -Targets rtk"),
        ("windows-installer", "cp dist/_rtk/rtk.exe \"$PAYLOAD/bin/rtk.exe\""),
        ("macos-installer", "packaging/macos/build-pkg.sh"),
        ("linux-deb", "build-packages.ps1 -Targets linux"),
    ] {
        if !job_says(job, needle) {
            problems.push(format!("{rel}: job `{job}` no longer runs `{needle}`"));
        }
    }
    for (script, body, sum_check, package) in [
        (LINUX_SCRIPT, &set.linux, "sha256sum -c -", RTK_PACKAGES[0]),
        (MACOS_SCRIPT, &set.macos, "shasum -a 256 -c -", RTK_PACKAGES[1]),
    ] {
        for needle in [
            "set -euo pipefail",
            "SUMS=\"$REPO/checksums.txt\"",
            "releases/download/v$RTK_VERSION/",
            sum_check,
        ] {
            if !body.contains(needle) {
                problems.push(format!("{script}: no `{needle}`"));
            }
        }
        if package == RTK_PACKAGES[0] && !body.contains(package) {
            problems.push(format!("{script}: does not download `{package}`"));
        }
        for forbidden in ["install.sh | sh", "cargo install", "|| true\nfor p in"] {
            if body.contains(forbidden) {
                problems.push(format!("{script}: still has `{forbidden}`"));
            }
        }
        if body.lines().any(|l| l.contains(sum_check) && l.contains("||")) {
            problems.push(format!("{script}: the sum check may fail without stopping the build"));
        }
    }
    if !set.macos.contains("rtk-$arch-apple-darwin.tar.gz") {
        problems.push(format!("{MACOS_SCRIPT}: does not download both Mac packages"));
    }
    for needle in [
        "function Get-PinnedRtk",
        "checksums.txt",
        RTK_PACKAGES[3],
        "Get-FileHash -Algorithm SHA256",
        "if ($actual -ne $expected) { throw",
        "$rtk = Get-PinnedRtk",
    ] {
        if !set.packages.contains(needle) {
            problems.push(format!("{PACKAGES_SCRIPT}: no `{needle}`"));
        }
    }
    if set.packages.contains("Get-Command rtk") {
        problems.push(format!("{PACKAGES_SCRIPT}: ships whatever rtk the machine happens to have"));
    }
    problems
}

// ---------------------------------------------------------------------------
// The real files
// ---------------------------------------------------------------------------

/// The verification runs on every pull request and on pushes to `dev` and
/// `main`; only the push runs save the compiled build; no job of any workflow
/// runs past half an hour; and the release builds neither the dashboard nor the
/// memory server.
#[test]
fn the_verification_runs_on_pull_requests_and_on_pushes_to_dev_and_main() {
    let ci = Workflow::read(VERIFICATION);
    let release = Workflow::read(RELEASE);
    let workflows = every_workflow();
    assert!(workflows.len() >= 3, "the workflows were not found: {workflows:?}");
    let problems: Vec<String> = [trigger_problems(&ci), cache_problems(&ci, &release), dashboard_problems(&release)]
        .into_iter()
        .flatten()
        .chain(workflows.iter().flat_map(|rel| timeout_problems(&Workflow::read(rel))))
        .collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The three installers bring rtk in the version `checksums.txt` fixes,
/// checked against its sum; the release stops without it; and no step
/// compiles it.
#[test]
fn the_installers_bring_the_fixed_rtk_and_nothing_compiles_it() {
    let problems = installer_problems(&Installers::read());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---------------------------------------------------------------------------
// The checks themselves, over fixtures
// ---------------------------------------------------------------------------

/// A verification in the shape the checks accept. The block scalar under
/// `run` carries a line that looks like a step, which must stay text.
const GOOD_CI: &str = "\
name: CI
on:
  # a comment between triggers
  pull_request:
    branches: [main, dev]
  push:
    branches:
      - 'dev'
      - main   # a trailing comment
  workflow_dispatch:
jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          cache: false
      - name: Cache
        uses: Swatinem/rust-cache@v2
        with:
          shared-key: one
          save-if: ${{ github.event_name == 'push' }}
      - name: Test
        run: |
          - name: not a step
          cargo test
";

/// A release in the shape the checks accept.
const GOOD_RELEASE: &str = "\
on:
  push:
    tags:
      - 'v*'
jobs:
  build:
    timeout-minutes: 30
    steps:
      - uses: swatinem/rust-cache@v2
        with:
          shared-key: one
          save-if: false
      - name: Build
        run: cargo build --release --bin mustard
";

/// Every problem the checks find in the fixture pair.
fn fixture_problems(ci: &str, release: &str) -> Vec<String> {
    let ci = Workflow::parse("ci", ci);
    let release = Workflow::parse("release", release);
    [
        trigger_problems(&ci),
        cache_problems(&ci, &release),
        timeout_problems(&ci),
        timeout_problems(&release),
        dashboard_problems(&release),
    ]
    .concat()
}

#[test]
fn the_checks_accept_the_good_shape_and_read_it_as_written() {
    assert_eq!(fixture_problems(GOOD_CI, GOOD_RELEASE), Vec::<String>::new());

    let ci = Workflow::parse("ci", GOOD_CI);
    assert_eq!(ci.steps().len(), 4, "the line inside `run` became a step");
    let push = ci.get("on").map(|on| list(&on["push"]["branches"]));
    assert_eq!(push, Some(vec!["dev".to_string(), "main".to_string()]));
}

#[test]
fn the_checks_refuse_each_broken_piece() {
    // (what breaks, whether it is the release that changes, text of the good
    // shape, what that text becomes)
    let cases = [
        ("no pull request is verified", false, "  pull_request:\n    branches: [main, dev]\n", ""),
        ("pushes to `dev` do not run", false, "      - 'dev'\n", ""),
        ("a push to a work branch saves too", false, "      - main", "      - main\n      - work"),
        ("a tag push saves too", false, "  workflow_dispatch:", "    tags: ['v*']\n  workflow_dispatch:"),
        ("the pull request saves", false, SAVE_ON_PUSH, "true"),
        ("the cache saves by default", false, "          save-if: ${{ github.event_name == 'push' }}\n", ""),
        ("the toolchain caches on its own", false, "          cache: false\n", ""),
        ("a plain cache saves on every run", false, "uses: actions/checkout@v4", "uses: actions/cache@v4"),
        ("the release saves", true, "save-if: false", "save-if: true"),
        ("the release names another key", true, "shared-key: one", "shared-key: two"),
        ("a job has no cap", false, "    timeout-minutes: 30\n", ""),
        ("a job runs past half an hour", true, "timeout-minutes: 30", "timeout-minutes: 45"),
        ("the release builds the dashboard", true, "cargo build --release --bin mustard", "pnpm --filter mustard-dashboard build"),
    ];
    for (what, in_release, from, to) in cases {
        let good = if in_release { GOOD_RELEASE } else { GOOD_CI };
        assert!(good.contains(from), "the fixture for `{what}` no longer matches the good shape");
        let broken = good.replacen(from, to, 1);
        let problems = if in_release {
            fixture_problems(GOOD_CI, &broken)
        } else {
            fixture_problems(&broken, GOOD_RELEASE)
        };
        assert!(!problems.is_empty(), "nothing refused it when {what}");
    }
}

/// The installer check refuses each way the fixed rtk could stop reaching an
/// installer. Each case breaks one text of the real set, which the check
/// accepts as it is.
#[test]
fn the_installer_check_refuses_each_broken_piece() {
    assert_eq!(installer_problems(&Installers::read()), Vec::<String>::new(), "the real set must pass first");
    let release = read_text(RELEASE);
    // (what breaks, which text, text of the real file, what that text becomes)
    let cases: [(&str, &str, &str, &str); 9] = [
        ("the version is not fixed", CHECKSUMS, "# rtk v", "# rtk "),
        ("a package has no sum", CHECKSUMS, "  rtk-x86_64-pc-windows-msvc.zip", "  rtk-windows.zip"),
        ("the Windows job compiles rtk", RELEASE, "run: ./packaging/build-packages.ps1 -Targets rtk", "run: cargo install --git https://github.com/rtk-ai/rtk --locked"),
        ("the Windows job goes on without rtk", RELEASE, "        run: ./packaging/build-packages.ps1 -Targets rtk", "        continue-on-error: true\n        run: ./packaging/build-packages.ps1 -Targets rtk"),
        ("the payload takes the runner's rtk", RELEASE, "cp dist/_rtk/rtk.exe \"$PAYLOAD/bin/rtk.exe\"", "RTK=\"$(command -v rtk || true)\""),
        ("Linux skips the sum", LINUX_SCRIPT, "| sha256sum -c - )", ")"),
        ("Linux takes rtk from the unpinned script", LINUX_SCRIPT, "curl -fsSL -o", "curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh || true\ncurl -fsSL -o"),
        ("macOS skips the sum", MACOS_SCRIPT, "| shasum -a 256 -c - )", ")"),
        ("Windows ships the machine's rtk", PACKAGES_SCRIPT, "$rtk = Get-PinnedRtk", "$rtk = (Get-Command rtk).Source"),
    ];
    for (what, which, from, to) in cases {
        let mut set = Installers::read();
        let text = match which {
            CHECKSUMS => &mut set.checksums,
            LINUX_SCRIPT => &mut set.linux,
            MACOS_SCRIPT => &mut set.macos,
            PACKAGES_SCRIPT => &mut set.packages,
            _ => {
                assert!(release.contains(from), "the case `{what}` no longer matches {RELEASE}");
                set.release = Workflow::parse(RELEASE, &release.replacen(from, to, 1));
                assert!(!installer_problems(&set).is_empty(), "nothing refused it when {what}");
                continue;
            }
        };
        assert!(text.contains(from), "the case `{what}` no longer matches {which}");
        *text = text.replacen(from, to, 1);
        assert!(!installer_problems(&set).is_empty(), "nothing refused it when {what}");
    }
}
