//! The verification and the release workflows, read as files.
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
//! - no job of either file runs past half an hour.
//!
//! Each promise is a function that answers with the problems it finds. A
//! fixture proves the function refuses what it should, and the test over the
//! real file only asks for an empty list. Another promise about these files
//! (what the installers build, for one) belongs here as one more function of
//! the same shape, reading through the same [`Workflow`].
//!
//! No YAML parser is pulled into the dev-dependencies for this. [`Workflow`]
//! reads the indentation-shaped subset these files are written in: mappings,
//! `- ` sequence items, inline `[a, b]` lists, quoted scalars and `#`
//! comments. A file it cannot follow fails the read instead of passing
//! quietly.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The verification every pull request and every push to `dev` and `main` runs.
const VERIFICATION: &str = ".github/workflows/ci.yml";

/// The release: builds the installers and publishes them on a version tag.
const RELEASE: &str = ".github/workflows/release.yml";

/// The only branches whose runs save the compiled build, sorted.
const SAVING_BRANCHES: [&str; 2] = ["dev", "main"];

/// The longest a job may run. Without a cap a hung step holds the runner for
/// GitHub's six-hour default; the slowest job measured took 16 minutes.
const LONGEST_JOB_MINUTES: u32 = 30;

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

/// The key of a sequence item.
const ITEM: &str = "-";

// ---------------------------------------------------------------------------
// The reader
// ---------------------------------------------------------------------------

/// One entry of the YAML tree: `key: value`, or a sequence item (key `-`)
/// whose value is its scalar, with the entries nested under it.
struct Node {
    key: String,
    value: String,
    children: Vec<Node>,
}

impl Node {
    /// The first entry named `key` directly under this one.
    fn get(&self, key: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.key == key)
    }

    /// The value of the entry named `key` directly under this one.
    fn value_of(&self, key: &str) -> Option<&str> {
        self.get(key).map(|child| child.value.as_str())
    }

    /// The sequence items directly under this one.
    fn items(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter(|child| child.key == ITEM)
    }

    /// This entry read as a list: an inline `[a, b]`, or the `- ` items under it.
    fn list(&self) -> Vec<String> {
        match self.value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            Some(inline) => inline
                .split(',')
                .map(scalar)
                .filter(|v| !v.is_empty())
                .collect(),
            None => self.items().map(|item| item.value.clone()).collect(),
        }
    }
}

/// A scalar as YAML reads it: the quotes around a quoted one removed, or an
/// inline `# comment` dropped from a plain one.
fn scalar(raw: &str) -> String {
    let raw = raw.trim();
    for quote in ['\'', '"'] {
        if let Some(end) = raw.strip_prefix(quote).and_then(|rest| rest.find(quote)) {
            return raw[1..=end].to_string();
        }
    }
    if raw.starts_with('#') {
        return String::new();
    }
    raw.split_once(" #").map_or(raw, |(value, _)| value).trim_end().to_string()
}

/// `text` split as a mapping entry, `key: value` or `key:`, when it is one.
fn entry(text: &str) -> Option<(String, String)> {
    let (key, value) = match text.split_once(": ") {
        Some(pair) => pair,
        None => (text.strip_suffix(':')?, ""),
    };
    let plain = !key.is_empty()
        && !key.contains(' ')
        && !key.starts_with(['"', '\'', '$', '{', '[']);
    plain.then(|| (key.to_string(), scalar(value)))
}

/// The entry one line holds.
fn node(text: &str) -> Node {
    let (key, value) = if text == ITEM {
        (ITEM.to_string(), String::new())
    } else if let Some(rest) = text.strip_prefix("- ") {
        (ITEM.to_string(), scalar(rest))
    } else {
        entry(text).unwrap_or_else(|| (text.to_string(), String::new()))
    };
    Node { key, value, children: Vec::new() }
}

/// The lines that carry YAML, as `(indentation, text)`: blank and comment
/// lines dropped, and an item that opens a mapping (`- key: value`) split into
/// the item and its first entry two columns deeper, so the mapping nests like
/// any other.
fn yaml_lines(raw: &str) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let text = line.trim_start();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let indent = line.len() - text.len();
        match text.strip_prefix("- ") {
            Some(rest) if entry(rest).is_some() => {
                lines.push((indent, ITEM.to_string()));
                lines.push((indent + 2, rest.to_string()));
            }
            _ => lines.push((indent, text.to_string())),
        }
    }
    lines
}

/// The entries of the block starting at `*at`: every following line at the
/// block's own indentation, each carrying the deeper lines after it. Stops at
/// the first line indented less than the block.
fn block(lines: &[(usize, String)], at: &mut usize) -> Vec<Node> {
    let mut nodes: Vec<Node> = Vec::new();
    let Some(indent) = lines.get(*at).map(|(depth, _)| *depth) else {
        return nodes;
    };
    while let Some((depth, text)) = lines.get(*at) {
        match depth.cmp(&indent) {
            Ordering::Less => break,
            Ordering::Greater => {
                let nested = block(lines, at);
                if let Some(last) = nodes.last_mut() {
                    last.children.extend(nested);
                }
            }
            Ordering::Equal => {
                nodes.push(node(text));
                *at += 1;
            }
        }
    }
    nodes
}

/// One workflow file, read into its tree of entries.
struct Workflow {
    /// Where the file lives, relative to the workspace root. Every problem
    /// names it.
    rel: &'static str,
    top: Vec<Node>,
}

impl Workflow {
    /// Read the workflow at `rel`, relative to the workspace root.
    fn read(rel: &'static str) -> Self {
        let path = workspace_root().join(rel);
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        Self::parse(rel, &raw)
    }

    /// Read `raw` as the workflow at `rel`.
    fn parse(rel: &'static str, raw: &str) -> Self {
        let lines = yaml_lines(raw);
        let mut at = 0;
        let top = block(&lines, &mut at);
        if let Some((_, text)) = lines.get(at) {
            panic!("{rel}: the reader cannot follow `{text}`, indented less than the first line");
        }
        Self { rel, top }
    }

    /// The top-level entry named `key`.
    fn get(&self, key: &str) -> Option<&Node> {
        self.top.iter().find(|node| node.key == key)
    }

    /// Every job, keyed by its id.
    fn jobs(&self) -> &[Node] {
        self.get("jobs").map_or(&[], |jobs| jobs.children.as_slice())
    }

    /// Every step of every job, as `(job id, step)`.
    fn steps(&self) -> impl Iterator<Item = (&str, &Node)> {
        self.jobs().iter().flat_map(|job| {
            job.get("steps")
                .into_iter()
                .flat_map(Node::items)
                .map(move |step| (job.key.as_str(), step))
        })
    }

    /// The steps whose action starts with `action`, compared without case.
    fn steps_using<'a>(&'a self, action: &'a str) -> impl Iterator<Item = (&'a str, &'a Node)> {
        self.steps().filter(move |(_, step)| {
            step.value_of("uses")
                .is_some_and(|uses| uses.to_ascii_lowercase().starts_with(action))
        })
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

// ---------------------------------------------------------------------------
// The promises, each as the list of what breaks it
// ---------------------------------------------------------------------------

/// What keeps the verification from running where the saved build is made and
/// read: every pull request into `dev` and `main`, and pushes to exactly those
/// two branches — a push anywhere else, or a tag push, would be a saving run
/// too.
fn trigger_problems(ci: &Workflow) -> Vec<String> {
    let rel = ci.rel;
    let Some(on) = ci.get("on") else {
        return vec![format!("{rel}: no `on:` block, so it runs on nothing")];
    };
    let mut problems = Vec::new();
    match on.get("pull_request") {
        None => problems.push(format!(
            "{rel}: no `pull_request` trigger, so nothing is verified before it lands"
        )),
        Some(pr) => {
            let branches = pr.get("branches").map(Node::list).unwrap_or_default();
            for branch in SAVING_BRANCHES {
                if !branches.is_empty() && !branches.iter().any(|b| b == branch) {
                    problems.push(format!("{rel}: pull requests into `{branch}` are not verified"));
                }
            }
        }
    }
    match on.get("push") {
        None => problems.push(format!(
            "{rel}: no `push` trigger, so no run saves a build a pull request could restore"
        )),
        Some(push) => {
            let mut branches = push.get("branches").map(Node::list).unwrap_or_default();
            branches.sort();
            if branches != SAVING_BRANCHES {
                problems.push(format!(
                    "{rel}: pushes must run on {SAVING_BRANCHES:?} and nowhere else, found {branches:?}"
                ));
            }
            for filter in push.children.iter().filter(|child| child.key != "branches") {
                problems.push(format!(
                    "{rel}: the `push` trigger also filters on `{}`; it must name only its branches",
                    filter.key
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
    let mut keys = BTreeSet::new();
    if ci.steps_using(RUST_CACHE).next().is_none() {
        problems.push(format!("{}: no Rust cache step, so no run saves the build", ci.rel));
    }
    for (workflow, save_if) in [(ci, SAVE_ON_PUSH), (release, "false")] {
        let rel = workflow.rel;
        for (job, step) in workflow.steps_using(RUST_CACHE) {
            let with = step.get("with");
            let found = with.and_then(|w| w.value_of("save-if"));
            if found != Some(save_if) {
                problems.push(format!(
                    "{rel}: job `{job}` caches with `save-if: {}`; it must be `{save_if}`",
                    found.unwrap_or("(absent, which saves)")
                ));
            }
            keys.insert(with.and_then(|w| w.value_of("shared-key")).unwrap_or_default().to_string());
        }
        for (job, step) in workflow.steps_using(RUST_TOOLCHAIN) {
            if step.get("with").and_then(|w| w.value_of("cache")) != Some("false") {
                problems.push(format!(
                    "{rel}: job `{job}` lets the toolchain action cache on its own, which saves on every run"
                ));
            }
        }
        for action in SAVING_CACHE_ACTIONS {
            for (job, _) in workflow.steps_using(action) {
                problems.push(format!("{rel}: job `{job}` uses `{action}`, which saves on every run"));
            }
        }
    }
    if keys.len() != 1 || keys.contains("") {
        problems.push(format!(
            "the Rust cache steps must all name one `shared-key`, found {keys:?}"
        ));
    }
    problems
}

/// Every job of `workflow` without a numeric `timeout-minutes` of at most
/// [`LONGEST_JOB_MINUTES`].
fn timeout_problems(workflow: &Workflow) -> Vec<String> {
    let rel = workflow.rel;
    let mut problems = Vec::new();
    if workflow.jobs().is_empty() {
        problems.push(format!("{rel}: no jobs found"));
    }
    for job in workflow.jobs() {
        let id = &job.key;
        match job.value_of("timeout-minutes").map(str::parse::<u32>) {
            Some(Ok(minutes)) if (1..=LONGEST_JOB_MINUTES).contains(&minutes) => {}
            Some(Ok(minutes)) => problems.push(format!(
                "{rel}: job `{id}` may run {minutes} minutes; the cap is {LONGEST_JOB_MINUTES}"
            )),
            Some(Err(_)) | None => problems.push(format!(
                "{rel}: job `{id}` has no numeric `timeout-minutes`, so a hung step holds the runner for hours"
            )),
        }
    }
    problems
}

// ---------------------------------------------------------------------------
// The real files
// ---------------------------------------------------------------------------

#[test]
fn the_verification_runs_on_pull_requests_and_on_pushes_to_dev_and_main() {
    let problems = trigger_problems(&Workflow::read(VERIFICATION));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn only_the_push_runs_save_the_compiled_build() {
    let problems = cache_problems(&Workflow::read(VERIFICATION), &Workflow::read(RELEASE));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn no_job_of_the_verification_or_the_release_runs_past_half_an_hour() {
    let problems: Vec<String> = [VERIFICATION, RELEASE]
        .into_iter()
        .flat_map(|rel| timeout_problems(&Workflow::read(rel)))
        .collect();
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
";

/// Every problem the three checks find in the fixture pair.
fn fixture_problems(ci: &str, release: &str) -> Vec<String> {
    let ci = Workflow::parse("ci", ci);
    let release = Workflow::parse("release", release);
    [
        trigger_problems(&ci),
        cache_problems(&ci, &release),
        timeout_problems(&ci),
        timeout_problems(&release),
    ]
    .concat()
}

#[test]
fn the_checks_accept_the_good_shape_and_read_it_as_written() {
    assert_eq!(fixture_problems(GOOD_CI, GOOD_RELEASE), Vec::<String>::new());

    let ci = Workflow::parse("ci", GOOD_CI);
    assert_eq!(ci.steps().count(), 4, "the line inside `run` became a step");
    let push = ci.get("on").and_then(|on| on.get("push")).expect("the push trigger is read");
    assert_eq!(push.get("branches").map(Node::list), Some(vec!["dev".to_string(), "main".to_string()]));
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
