//! Parity ratchet between `plugin/pipeline-config.md § Enforcement Hooks` and
//! the enforcement surface the runtime really has.
//!
//! Three silent failures this turns into a red build:
//!
//! - **A gate the runtime registry names and the table does not.** The table is
//!   what a reader consults to learn what can refuse a write. A gate missing
//!   from it still refuses; the reader just has nowhere to look, and nothing in
//!   the repository can tell that the omission happened.
//! - **A mode env var some hook reads that no plugin prose ever spells.** A knob
//!   whose name is written nowhere cannot be set — it is a switch that exists
//!   only for whoever wrote it.
//! - **A mode env var the registry names that NOTHING reads.** The inverse, and
//!   the worse half for an operator: `run status --harness` renders that name as
//!   the knob for the gate, so the name is not merely absent, it is wrong, and
//!   setting it looks accepted. Every such name must be declared in
//!   [`DEAD_REGISTRY_ENV`] with the name the hook really reads.
//! - **A dead name kept alive by being MENTIONED.** All three checks above pivot
//!   on one question — does anything read this name — so the measurement of
//!   "read" decides what they can see. Counting every quoted occurrence would
//!   let a doc-comment example, an advisory string or a test assertion vouch for
//!   a name nothing consults, and a dead name that looks live is invisible to
//!   every test here. So a read is a literal handed to an [`ENV_READERS`] call,
//!   comments excluded, and `a_mode_env_name_is_live_only_where_something_reads_it`
//!   holds the other end: a mention is fine, of a name something really reads.
//!
//! What this deliberately does NOT do is demand LITERAL parity with the
//! `hook_mode_env` map. That map names two vars **no hook reads**
//! (`MUSTARD_POST_EDIT_MODE`, `MUSTARD_KNOWLEDGE_MODE` — measured: they occur
//! nowhere but `status.rs`). A ratchet requiring the table to repeat them would
//! pin two dead names into the documentation, which is the same defect the
//! table was corrected to remove. So the env half is checked against the name
//! the runtime ACTUALLY reads, and the two dead names are declared dead in
//! [`DEAD_REGISTRY_ENV`] — with a sibling test that fails the day either stops
//! being dead, so the exception cannot outlive its reason.
//!
//! The name half is checked in ONE direction, registry → table. The reverse
//! would demand a `hook_mode_env` row for every documented gate, and the table
//! rightly documents gates that have no mode at all (`scan_gate`) or whose knob
//! is read by a `run` command rather than by the hook dispatcher
//! (`approve-spec`). Forcing them into the map would add rows nothing renders.
//!
//! `status.rs` and `pipeline-config.md` are read here and never written: this
//! file is the comparison, not a third copy of either side.
//!
//! Deterministic: walks the repo tree only (sorted), no network, no env vars.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The registry module. Its private `hook_mode_env` is the name→env map
/// `run status --harness` renders; it is read as TEXT because the function is
/// private and this ratchet has no business widening it.
const REGISTRY_SRC: &str = "apps/rt/src/commands/pipeline/status.rs";

/// The prose that carries the gate table.
const GATE_TABLE_DOC: &str = "plugin/pipeline-config.md";

/// The callees that READ an environment variable. A `"…_MODE"` literal handed
/// to one of these is the runtime consulting the knob; the same name anywhere
/// else is a MENTION, and a mention is not evidence that anything reads it.
///
/// `var` and `var_os` are `std::env`'s readers — the identifier scan stops at
/// the `::`, so `std::env::var(…)` reads as `var`. `resolve_mode`
/// (`apps/rt/src/shared/gate_mode.rs`) is the crate's own cascade and hands its
/// first argument straight to `std::env::var`.
///
/// A new reader helper belongs here the day it lands: until it does, every
/// knob it reads looks dead, and the sibling tests say so out loud rather than
/// quietly excusing it.
const ENV_READERS: &[&str] = &["resolve_mode", "var", "var_os"];

/// Registry entries whose env var NO hook reads. Each row names what the hook
/// really reads instead, so the exception carries its own correction.
///
/// A row here is a claim of deadness, and the sibling test re-measures it: the
/// day something starts reading one of these names, the row fails and has to go
/// (and the table then owes the name a row like any live knob). Kept sorted.
const DEAD_REGISTRY_ENV: &[(&str, &str, &str)] = &[
    (
        "post_edit",
        "MUSTARD_POST_EDIT_MODE",
        "no hook reads it; the only refusing half of post_edit reads \
         MUSTARD_GUARD_GATE_MODE (hooks/write/post_edit.rs), which is the name \
         the table's post_edit row carries. Setting this one changes nothing",
    ),
    (
        "session_knowledge_observer",
        "MUSTARD_KNOWLEDGE_MODE",
        "no hook reads it; session_knowledge_observer returns no verdict at all \
         (an Observer), so it has no enforcement level to set",
    ),
];

/// Live mode env vars that no plugin prose spells, kept deliberately.
///
/// The bar this list has to clear is not "it is minor" — it is that a reader
/// who never sets the name still meets no refusal the prose failed to explain.
/// Every row below is either a knob with no blocking mode at all, an observer
/// on/off switch, or a half of a gate whose BEHAVIOUR the table already
/// describes. A knob that can refuse a default install does NOT belong here:
/// document it instead. Kept sorted.
const PROSE_EXEMPT_MODE_ENV: &[(&str, &str)] = &[
    (
        "MUSTARD_AC_QUALITY_MODE",
        "the AC audit half of size_gate: `warn` by default and advisory ONLY - \
         hooks/write/size_gate.rs pushes its finding onto `warnings` and never \
         returns a Deny, so there is no refusal a reader could fail to anticipate",
    ),
    (
        "MUSTARD_DELEGATION_WARN_MODE",
        "hooks/task/delegation_advisory.rs declares `off` and `warn` and says so \
         in its own doc: there is deliberately no strict mode. A knob that cannot \
         block belongs in a table of what blocks only as noise",
    ),
    (
        "MUSTARD_FINDINGS_GATE_MODE",
        "the findings sub-gate IS documented, by behaviour, in the § Close - \
         Deterministic Gate Chain list of what `close-gates` runs; only the env \
         name is absent, and its refusal prints the exact `mark-finding` line \
         that settles each open finding",
    ),
    (
        "MUSTARD_HYGIENE_MODE",
        "the on/report/auto switch of a SessionStart Observer \
         (hooks/session/spec_hygiene_observer.rs). An observer returns no \
         verdict, so it refuses nobody and has no row to own",
    ),
    (
        "MUSTARD_MAX_ACTIVE_SPECS_MODE",
        "hooks/write/active_spec_limit_gate.rs is `warn` by default; it becomes \
         blocking only for an operator who sets `strict`, which is an operator \
         who already knows the name. Its advisory names the cap and the way past \
         it in the message",
    ),
    (
        "MUSTARD_MOLD_GATE_MODE",
        "hooks/write/mold_gate.rs declares `off` and `warn` and says so in its \
         own doc: no blocking mode by design",
    ),
    (
        "MUSTARD_QA_COMPOSITION_GATE_MODE",
        "`warn` by default and telemetry-only there (close_gates.rs) - a strict \
         default could deadlock the close, since a natural-language close prompt \
         is itself recorded as a change request, which is why it is not one",
    ),
    (
        "MUSTARD_REWAVE_OBSERVER_MODE",
        "the on/off switch of a fire-and-forget observer \
         (hooks/observe/rewave_observer.rs), which states there is deliberately \
         no strict mode: this is advisory restructuring, never a gate",
    ),
    (
        "MUSTARD_SKILL_SIZE_MODE",
        "a blocking half of size_gate the table's row does not describe - the row \
         names the spec cap and the frontmatter validation. Recorded here rather \
         than excused: the refusal itself carries the whole remedy \
         ('SKILL.md exceeds 500 lines (N lines) - split verbose sections into \
         references/examples.md'), so the blocked author is never sent to prose",
    ),
    (
        "MUSTARD_WAVE_COMPLETE_OBSERVER_MODE",
        "on/off switch of a fire-and-forget observer \
         (hooks/observe/wave_complete_observer.rs) - no verdict, nothing to gate",
    ),
    (
        "MUSTARD_WAVE_START_OBSERVER_MODE",
        "on/off switch of a fire-and-forget observer \
         (hooks/observe/wave_start_observer.rs) - no verdict, nothing to gate",
    ),
];

/// The repo root, resolved from this crate (`apps/rt`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Read a repo file, panicking with its path. In a ratchet an unreadable input
/// is a failure, never a silent pass.
fn read_repo(root: &Path, rel: &str) -> String {
    let path = root.join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("ratchet input {} is unreadable: {e}", path.display()))
}

/// Read a file as lossy UTF-8; unreadable files degrade to an empty string.
fn read_lossy(path: &Path) -> String {
    fs::read(path).map_or_else(|_| String::new(), |b| String::from_utf8_lossy(&b).into_owned())
}

/// Recursively collect files under `dir` in a deterministic (sorted) order.
fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name == "node_modules" || name == "target" || name == ".git" {
                continue;
            }
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// The `(hook, env)` pairs the `hook_mode_env` match arms declare, in source
/// order. Located by SHAPE — `"<hook>" => Some("<ENV>"),` inside that function —
/// so the surrounding module can be reorganised without touching this guard.
fn registry_pairs(src: &str) -> Vec<(String, String)> {
    let Some(start) = src.find("fn hook_mode_env(") else {
        return Vec::new();
    };
    let body = &src[start..];
    let end = body.find("\n}").unwrap_or(body.len());
    body[..end]
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.trim().strip_prefix('"')?.split_once("\" => Some(\"")?;
            let env = rest.strip_suffix("\"),")?;
            Some((name.to_string(), env.to_string()))
        })
        .collect()
}

/// The `## Enforcement Hooks` section of the doc — the table plus the prose
/// that qualifies it, up to the next heading.
fn enforcement_section(doc: &str) -> &str {
    let Some(at) = doc.find("\n## Enforcement Hooks") else {
        return "";
    };
    let body = &doc[at + 1..];
    let after_heading = body.find('\n').map_or(body.len(), |n| n + 1);
    let rest = &body[after_heading..];
    let end = rest.find("\n#").map_or(rest.len(), |n| n + 1);
    &rest[..end]
}

/// The cells of every table row in a markdown section, header and separator
/// rows included — callers filter by what they are looking for.
fn table_rows(section: &str) -> Vec<Vec<String>> {
    section
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

/// The module names the table's first column declares: the FIRST backticked
/// token of the cell, so a qualifier like `` `close_gate` (QA) `` still reads as
/// `close_gate`.
fn table_modules(section: &str) -> BTreeSet<String> {
    table_rows(section)
        .iter()
        .filter_map(|cells| {
            let cell = cells.first()?;
            let start = cell.find('`')? + 1;
            let end = cell[start..].find('`')? + start;
            Some(cell[start..end].to_string())
        })
        .collect()
}

/// Byte-indexed "this position sits in a comment" mask, computed line by line
/// and masking exactly ONE thing: a line whose first non-space is `//` (`//`,
/// `///` and `//!` alike). Nothing else is masked — there is no block-comment
/// tracking here, deliberately.
///
/// The obvious `/* … */` half was here, and it was worse than none.
/// `line.find("/*")` cannot tell a comment opener from the same two bytes
/// inside a string literal, and this crate really contains `"**/*.rs"` globs;
/// with no `*/` to close it, the mask latched on and ran to the END OF THE
/// FILE. Measured across `apps/rt/src`: eleven files had their tails hidden
/// that way — `hooks/write/post_edit.rs` and `hooks/bash/native_redirect.rs`
/// among them — so a live read written below such a glob was silently DROPPED,
/// the one failure a ratchet must never have. Proven by measurement before the
/// removal: a `std::env::var("MUSTARD_ALGUM_NOVO_MODE")` appended to the end of
/// `post_edit.rs` left all four tests green.
///
/// What remains cannot over-mask, and that is the whole design: it hides only a
/// whole line that STARTS with `//`. The crate documents with `///` and `//!`
/// and wraps no live read in a `/* … */` block; a `_MODE` literal parked inside
/// one now reads as code, and the worst that costs is a loud failure in
/// `a_mode_env_name_is_live_only_where_something_reads_it` — never a silent
/// pass. A trailing `// …` comment on a code line is unmasked too, and
/// [`ENV_READERS`] rejects that on its own: a name written in prose is no
/// call's first argument.
fn comment_mask(text: &str) -> Vec<bool> {
    let mut mask = vec![false; text.len()];
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let end = offset + line.len();
        if line.trim_start().starts_with("//") {
            mask[offset..end].fill(true);
        }
        offset = end;
    }
    mask
}

/// The identifier being CALLED on a literal whose opening quote sits at `at`:
/// walk back over whitespace, require a `(`, walk back over whitespace again,
/// and take the identifier ending there. Empty when the literal is not the
/// first argument of a call.
///
/// Walking backwards over ASCII-only predicates is byte-safe on UTF-8: a
/// continuation byte is `>= 0x80` and matches neither, so the walk stops at a
/// char boundary.
fn callee_before(text: &str, at: usize) -> &str {
    let bytes = text.as_bytes();
    let mut i = at;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] != b'(' {
        return "";
    }
    i -= 1;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    let end = i;
    while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        i -= 1;
    }
    &text[i..end]
}

/// Every bare `"…_MODE"` string literal outside a comment, paired with the
/// callee it is the first argument of — the shape an env-var name takes at the
/// point something reads it, and the shape it takes when it is merely named.
fn mode_env_literals(text: &str) -> Vec<(String, String)> {
    let mask = comment_mask(text);
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_uppercase() || bytes[end].is_ascii_digit() || bytes[end] == b'_')
        {
            end += 1;
        }
        if end > start && end < bytes.len() && bytes[end] == b'"' {
            let name = &text[start..end];
            if name.ends_with("_MODE") && !mask[i] {
                out.push((name.to_string(), callee_before(text, i).to_string()));
            }
            i = end + 1;
        } else {
            i = start;
        }
    }
    out
}

/// Every `.rs` file of the runtime crate, sorted.
fn rt_sources(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("apps/rt/src");
    assert!(dir.is_dir(), "runtime sources missing at {}", dir.display());
    let mut files = Vec::new();
    walk_files(&dir, &mut files);
    files.retain(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"));
    files
}

/// Every `(file, name, callee)` a `"…_MODE"` literal appears as, outside the
/// registry module — which renders names rather than consulting them.
fn rt_mode_literals(root: &Path) -> Vec<(PathBuf, String, String)> {
    rt_sources(root)
        .into_iter()
        .filter(|p| !p.ends_with("commands/pipeline/status.rs"))
        .flat_map(|p| {
            mode_env_literals(&read_lossy(&p))
                .into_iter()
                .map(move |(name, callee)| (p.clone(), name, callee))
        })
        .collect()
}

/// The mode env vars the runtime actually READS — a `"…_MODE"` literal handed
/// to one of the [`ENV_READERS`], never one merely written down.
///
/// The distinction is the whole point. "Live" is what the other three tests
/// pivot on: it decides whether a registry name owes the table a row or owes
/// [`DEAD_REGISTRY_ENV`] a confession, and whether a name owes plugin prose an
/// explanation. Counting every quoted occurrence would let a MENTION — an
/// example in a doc comment, a name in an advisory string, a test asserting a
/// message contains it — pass a dead name off as live, which is the exact
/// drift `dead_registry_env_names_stay_dead_and_declared` exists to catch.
/// `a_mode_env_name_is_live_only_where_something_reads_it` guards the other
/// side: a mention is allowed, but only of a name something really reads.
fn live_mode_envs(root: &Path) -> BTreeSet<String> {
    rt_mode_literals(root)
        .into_iter()
        .filter(|(_, _, callee)| ENV_READERS.contains(&callee.as_str()))
        .map(|(_, name, _)| name)
        .collect()
}

/// Every shipped plugin markdown file — the prose an operator can read.
fn plugin_prose(root: &Path) -> String {
    let dir = root.join("plugin");
    assert!(dir.is_dir(), "plugin tree missing at {}", dir.display());
    let mut files = Vec::new();
    walk_files(&dir, &mut files);
    files.retain(|p| p.extension().and_then(|e| e.to_str()) == Some("md"));
    files.iter().map(|p| read_lossy(p)).collect::<Vec<_>>().join("\n")
}

/// Every gate the runtime registry names must appear in the table, and every
/// LIVE mode var it maps must be spelled in that same section.
///
/// The table is the only place a reader learns what can refuse a write, and
/// nothing in the build has ever compared the two. The registry gained
/// `post_edit` and `session_knowledge_observer` while the table still carried a
/// `skills_advisory` row naming a module with zero occurrences in the source —
/// drift in both directions at once, for as long as it took someone to read
/// both files side by side.
#[test]
fn gate_table_matches_the_runtime_registry() {
    let root = repo_root();
    let pairs = registry_pairs(&read_repo(&root, REGISTRY_SRC));
    assert!(
        !pairs.is_empty(),
        "{REGISTRY_SRC} no longer exposes a `hook_mode_env` map of \
         `\"hook\" => Some(\"ENV\")` arms — this ratchet reads that shape, and \
         with no rows it would pass by measuring nothing"
    );

    let doc = read_repo(&root, GATE_TABLE_DOC);
    let section = enforcement_section(&doc);
    assert!(
        !section.is_empty(),
        "{GATE_TABLE_DOC} has no `## Enforcement Hooks` section — the gate table \
         this ratchet compares against is gone"
    );
    let modules = table_modules(section);
    let live = live_mode_envs(&root);

    let mut offenders = Vec::new();
    for (hook, env) in &pairs {
        if !modules.contains(hook) {
            offenders.push(format!(
                "gate `{hook}` is in the runtime registry and NOT in the table - \
                 it refuses writes that a reader of {GATE_TABLE_DOC} has no way to \
                 anticipate. Add its row (module, matcher, mode env, blocks on)"
            ));
        }
        if !live.contains(env) {
            // A dead name is not a documentation failure, it is a registry one,
            // and `dead_registry_env_names_stay_dead_and_declared` owns it in
            // BOTH directions — declared rows stay dead, and an undeclared dead
            // name fails there. Reporting it here too would bury the live gaps.
            continue;
        }
        if !section.contains(env.as_str()) {
            offenders.push(format!(
                "`{env}` is the knob `{hook}` really reads and the \
                 `## Enforcement Hooks` section never spells it - the row exists \
                 and the way to set it does not"
            ));
        }
    }
    assert!(
        offenders.is_empty(),
        "the gate table and the runtime registry have drifted:\n{}",
        offenders.join("\n")
    );
}

/// Every dead name in the registry is DECLARED here, and every declared one
/// stays dead, stays mentioned in the section, and never occupies a `Mode env`
/// cell.
///
/// The first half is what closes the hole a ratchet reading only its own list
/// would leave: `gate_table_matches_the_runtime_registry` skips a registry env
/// nothing reads (a dead name is a registry defect, not a documentation one),
/// so without the sweep below a NEW dead name — a row edited to
/// `MUSTARD_BOUNDARY_GATE_MODE` when the gate reads `MUSTARD_BOUNDARY_MODE`,
/// say — would pass every test in this file. It is exactly the drift this file
/// exists to catch, printed by `run status --harness` as the knob to set.
///
/// The last half is the one that matters most to a reader: the `Mode env`
/// column is read as *the knob to set*, so a dead name printed there is worse
/// than an absent row — it sends someone to configure a switch that is wired to
/// nothing, and the setting looks accepted.
#[test]
fn dead_registry_env_names_stay_dead_and_declared() {
    let root = repo_root();
    let pairs = registry_pairs(&read_repo(&root, REGISTRY_SRC));
    let live = live_mode_envs(&root);
    let doc = read_repo(&root, GATE_TABLE_DOC);
    let section = enforcement_section(&doc);
    let mode_cells: Vec<String> = table_rows(section)
        .iter()
        .filter_map(|cells| cells.get(2).cloned())
        .collect();

    for pair in DEAD_REGISTRY_ENV.windows(2) {
        assert!(
            pair[0].0 < pair[1].0,
            "DEAD_REGISTRY_ENV must stay sorted by hook: {} before {}",
            pair[0].0,
            pair[1].0
        );
    }

    // Registry → declaration. Every env the registry maps is either LIVE (some
    // hook reads it) or DECLARED dead below. A third state — named by the
    // registry, read by nothing, admitted by nobody — is the silent one: it is
    // rendered to the operator as the knob to set and wired to nothing.
    let undeclared: Vec<String> = pairs
        .iter()
        .filter(|(_, env)| !live.contains(env))
        .filter(|(hook, env)| !DEAD_REGISTRY_ENV.iter().any(|(h, e, _)| h == hook && e == env))
        .map(|(hook, env)| {
            format!(
                "`{hook}` => `{env}`: {REGISTRY_SRC} maps this name and NOTHING \
                 in the runtime reads it. Either fix the arm to name the var the \
                 hook really reads, or add a justified DEAD_REGISTRY_ENV row \
                 saying which name it reads instead"
            )
        })
        .collect();
    assert!(
        undeclared.is_empty(),
        "the runtime registry names mode env vars that are neither live nor \
         declared dead:\n{}",
        undeclared.join("\n")
    );

    for (hook, env, why) in DEAD_REGISTRY_ENV {
        assert!(
            pairs.iter().any(|(h, e)| h == hook && e == env),
            "DEAD_REGISTRY_ENV names `{hook}` => `{env}`, which {REGISTRY_SRC} no \
             longer maps - drop the row, there is nothing dead left to declare"
        );
        assert!(!why.trim().is_empty(), "DEAD_REGISTRY_ENV entry {env} carries no justification");
        assert!(
            !live.contains(*env),
            "`{env}` is no longer dead - something in the runtime reads it now. \
             Drop its DEAD_REGISTRY_ENV row; the table then owes `{hook}` the same \
             row any live knob gets"
        );
        assert!(
            section.contains(env),
            "`{env}` is a dead name the registry still renders, and the \
             `## Enforcement Hooks` section never mentions it. A reader who meets \
             it in `run status --harness` must be able to find out that setting it \
             does nothing: {why}"
        );
        assert!(
            !mode_cells.iter().any(|cell| cell.contains(env)),
            "`{env}` sits in a `Mode env` cell of the gate table. That column is \
             read as the knob to set, and this one is wired to nothing: {why}"
        );
    }
}

/// Every mode env var the runtime reads is spelled by some plugin prose, or
/// carries a justified exemption.
///
/// A knob nobody can name is not a knob. The exemption list exists because the
/// gap is wide today, and a ratchet that starts red is a ratchet somebody
/// deletes — but each row has to say why prose owes that name nothing, and the
/// sibling test drops the row the moment prose picks the name up.
#[test]
fn every_gate_mode_env_is_documented() {
    let root = repo_root();
    let prose = plugin_prose(&root);
    let live = live_mode_envs(&root);
    assert!(!live.is_empty(), "no `*_MODE` literals found in the runtime - the scan is broken");

    let mut undocumented = Vec::new();
    for env in &live {
        if prose.contains(env.as_str())
            || PROSE_EXEMPT_MODE_ENV.iter().any(|(name, _)| name == env)
        {
            continue;
        }
        undocumented.push(env.clone());
    }
    assert!(
        undocumented.is_empty(),
        "mode env vars the runtime reads that no plugin prose spells - an \
         operator cannot set a name that is written nowhere. Document them, or \
         add a JUSTIFIED PROSE_EXEMPT_MODE_ENV row saying why prose owes this one \
         nothing:\n{}",
        undocumented.join("\n")
    );
}

/// A `_MODE` name is never live by MENTION alone.
///
/// [`live_mode_envs`] counts only a literal handed to an [`ENV_READERS`] call,
/// so a name that appears solely in a message, an assertion or an example
/// cannot vouch for itself. This is the half that keeps that rule honest as the
/// crate grows: every remaining occurrence must name something the runtime
/// really reads. A name that is only ever written down is either a knob whose
/// read was deleted and whose mention outlived it, or a reader this file has
/// not been taught — and both are answered here rather than by a test quietly
/// treating the mention as proof.
#[test]
fn a_mode_env_name_is_live_only_where_something_reads_it() {
    let root = repo_root();
    let live = live_mode_envs(&root);
    let literals = rt_mode_literals(&root);
    assert!(!literals.is_empty(), "no `*_MODE` literals found in the runtime - the scan is broken");

    let mut orphans = Vec::new();
    for (path, name, callee) in literals {
        if ENV_READERS.contains(&callee.as_str()) || live.contains(&name) {
            continue;
        }
        let rel = path.strip_prefix(&root).unwrap_or(&path).display().to_string();
        let site = if callee.is_empty() { "no call at all".to_string() } else { format!("`{callee}(…)`") };
        orphans.push(format!(
            "`{name}` is written in {rel} under {site}, and NOTHING in the \
             runtime reads it. Either the read was removed and this mention \
             outlived it (delete the mention), or the thing that reads it is a \
             helper ENV_READERS has not been taught (add the callee there)"
        ));
    }
    assert!(
        orphans.is_empty(),
        "mode env names that exist only as mentions - a name nothing reads must \
         never be counted as a live knob:\n{}",
        orphans.join("\n")
    );
}

/// The exemption list stays sorted, stays live, and stays necessary.
#[test]
fn prose_exemptions_stay_sorted_live_and_not_redundant() {
    let root = repo_root();
    let prose = plugin_prose(&root);
    let live = live_mode_envs(&root);

    for pair in PROSE_EXEMPT_MODE_ENV.windows(2) {
        assert!(
            pair[0].0 < pair[1].0,
            "PROSE_EXEMPT_MODE_ENV must stay sorted: {} before {}",
            pair[0].0,
            pair[1].0
        );
    }
    for (env, why) in PROSE_EXEMPT_MODE_ENV {
        assert!(!why.trim().is_empty(), "PROSE_EXEMPT_MODE_ENV entry {env} carries no justification");
        assert!(
            live.contains(*env),
            "PROSE_EXEMPT_MODE_ENV entry {env} is read by nothing in the runtime - \
             drop the row, there is no knob left to exempt"
        );
        assert!(
            !prose.contains(env),
            "PROSE_EXEMPT_MODE_ENV entry {env} IS spelled in plugin prose now - the \
             row is redundant, drop it"
        );
    }
}
