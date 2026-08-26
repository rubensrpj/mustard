//! `scan-patterns-apply` — write the enrich agent's authored pattern-skill mold
//! to `{subproject}/.claude/skills/{slug}-pattern/SKILL.md`, create-only.
//!
//! The pattern-mold twin of `scan-guards-apply`. Mustard-generated molds are
//! swept before generation ([`super::sweep`]), so by the time apply runs the
//! target does not exist and this is a plain CREATE. It refuses to overwrite an
//! existing mold — whatever survived the sweep is hand-authored/adopted and must
//! be preserved. Every write is stamped with the origin notice
//! ([`super::origin::stamp`]), makes the parent directories, and lands
//! atomically via the same primitive `scan_claude` uses.
//!
//! What a mold CLAIMS is checked before it is written ([`grounding_defects`]) and
//! so is its SHAPE ([`structure_defects`]); its `paths:` key is tolerated in any
//! YAML form on the way in ([`paths_key`]) and rewritten to one canonical form on
//! the way out ([`canonical_paths_form`]), because the form is not what the check
//! measures but IS what the platform reads back.
//!
//! Routing the write THROUGH this command (rather than the orchestrator's own
//! Write tool) is the point: it is path-shape-guarded and — like every
//! `mustard-rt run` — outside the background-isolation gate, so the mold enrich
//! no longer stalls when the orchestrator runs as a background job.
//!
//! Fail-open per the `mustard-rt run` contract: a recoverable error (blank body,
//! IO failure, already-present mold) prints a clear stderr line and exits 0. The
//! only non-zero exit is a flat refusal of a path that is not a mold SKILL.md — a
//! caller bug worth surfacing.

use std::path::Path;

use mustard_core::io::fs as mfs;

/// The path shape a mold must have — guards this command from being used to
/// write anywhere else. A valid mold lives at `…/.claude/skills/<x>-pattern/SKILL.md`.
const SKILLS_SEGMENT: &str = "/.claude/skills/";
const MOLD_SUFFIX: &str = "-pattern/SKILL.md";

/// The ONE phrasing of "there is no body to write", so the blank case reads the
/// same whichever side reaches it first — [`resolve_content`] (the CLI face,
/// which stops there) or the [`Applied::Empty`] arm (any other caller of
/// [`apply_one`]). Two spellings of one event is how a caller starts guessing
/// whether they mean different things.
const EMPTY_BODY: &str = "scan-patterns-apply: empty mold body — nothing to write";

/// What happened to ONE mold — the verdict [`apply_one`] returns instead of
/// printing and exiting.
///
/// The CLI face still prints and exits; this exists because a caller holding
/// MANY molds (see [`super::relay`]) must not have the first bad block kill the
/// eleven good ones behind it. Splitting the decision from its reporting is
/// what lets both callers share exactly one copy of the rules.
#[derive(Debug, PartialEq)]
pub(crate) enum Applied {
    /// Written.
    Created,
    /// A `source: scan` mold was already there — written by THIS run, so two
    /// candidates collided on one path and this block was discarded.
    Collision,
    /// A hand-authored/adopted mold holds the path; it is preserved.
    Preserved,
    /// The block carried no body.
    Empty,
    /// Not a `…/.claude/skills/<slug>-pattern/SKILL.md` path.
    BadPath,
    /// The mold claims something the machine checked and refuted.
    Refused(Vec<String>),
    /// The write itself failed.
    IoError(String),
}

/// Decide and perform ONE mold write. Pure of stdout/stderr and of
/// `process::exit`, so it composes; see [`Applied`].
pub(crate) fn apply_one(path: &Path, body: &str, root: &Path) -> Applied {
    if !is_mold_path(path) {
        return Applied::BadPath;
    }

    // Create-only: never overwrite. Two very different things reach this branch,
    // and calling both "hand-authored" made one of them invisible.
    //
    // The sweep deletes every `source: scan` mold BEFORE any authoring, so a
    // survivor carrying that marker cannot be a leftover — it was written by
    // THIS run, seconds ago, which means two candidates resolved to one mold
    // path and this block is being thrown away. That is a worklist defect (see
    // `list::fold_collisions`), not a preserve: an agent burned a read and an
    // authoring pass for nothing. It must never again be reported as if a human
    // owned the file.
    if path.exists() {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        return if super::origin::is_mustard_generated(&existing) {
            Applied::Collision
        } else {
            Applied::Preserved
        };
    }

    let body = body.trim();
    if body.is_empty() {
        return Applied::Empty;
    }

    // Validate the frontmatter BEFORE writing. A mold that lands without
    // frontmatter-first or without `source: scan` is never swept again and
    // blocks its cluster forever — a permanent orphan. Better a loud refusal
    // now than a silent orphan a scan or two later. (`stamp` re-injects the
    // notice idempotently, but it cannot invent a `name:` or a `source:` the
    // agent never wrote.)
    let mut defects: Vec<String> =
        super::origin::frontmatter_defects(body).into_iter().map(|d| d.to_string()).collect();
    defects.extend(structure_defects(body));
    defects.extend(grounding_defects(body, path, root));
    if !defects.is_empty() {
        return Applied::Refused(defects);
    }

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Applied::IoError(format!("cannot create {}: {e}", parent.display()));
        }
    }

    // Normalised body + injected origin notice: byte-stable regardless of how
    // the agent's block was trimmed, and swept fresh on the next scan. The
    // `paths:` key is rewritten into the canonical block-list form on the way
    // out ([`canonical_paths_form`]) — the read side tolerates the other YAML
    // shapes, but only ONE shape may reach disk.
    let out = super::origin::stamp(&canonical_paths_form(body));
    match mfs::write_atomic(path, out.as_bytes()) {
        Ok(()) => Applied::Created,
        Err(e) => Applied::IoError(format!("cannot write {}: {e}", path.display())),
    }
}

/// Run `scan-patterns-apply`.
///
/// - `path`: the mold `SKILL.md` to write (`{subproject}/.claude/skills/{slug}-pattern/SKILL.md`).
/// - `content`: the agent's authored SKILL.md body, `-` to read it from stdin,
///   or `@<path>` to read it from that file — the SAME three channels
///   [`super::relay`] accepts, through the same reader, so the two commands
///   cannot drift apart on what `--content` means.
///
/// Create-only: an existing mold is left untouched (the sweep already removed
/// the generated ones; a survivor is hand-authored). On a successful write
/// prints a one-line confirmation and exits 0. A path that is not a mold
/// SKILL.md exits 1; every other recoverable error is fail-open.
pub fn run(path: &Path, content: &str, root: &Path) {
    // Nothing to write, and [`resolve_content`] already named WHY — the IO
    // failure of an unreadable `@<path>`, or the blank body. Falling through
    // with an empty string instead would hand `apply_one` a body the agent
    // never authored, and STACK `empty mold body` on top of the IO failure —
    // blaming the AGENT for what the FILE did. Two reasons for one event is
    // one reason too many, and the wrong one reads last.
    let Some(body) = resolve_content(content) else { return };
    match apply_one(path, &body, root) {
        Applied::Created => println!("scan-patterns-apply: created {}", path.display()),
        Applied::BadPath => {
            eprintln!(
                "scan-patterns-apply: refusing {} — not a `…/.claude/skills/<slug>-pattern/SKILL.md` path",
                path.display()
            );
            std::process::exit(1);
        }
        Applied::Collision => eprintln!(
            "scan-patterns-apply: COLLISION at {} — a `source: scan` mold was already written \
             there BY THIS RUN (the sweep removes them all before authoring), so this block is \
             DISCARDED. Two candidates share one mold path: the worklist should have folded \
             them. Nothing was hand-authored here.",
            path.display()
        ),
        Applied::Preserved => eprintln!(
            "scan-patterns-apply: mold already exists at {} — left unchanged (hand-authored/adopted; the sweep only removes `source: scan`)",
            path.display()
        ),
        // Unreachable from this face — `resolve_content` stops a blank body
        // above — but `apply_one` is shared with the relay, so the arm keeps
        // the verdict answerable rather than silent.
        Applied::Empty => eprintln!("{EMPTY_BODY}"),
        Applied::Refused(defects) => {
            eprintln!(
                "scan-patterns-apply: refusing {} — malformed mold, NOT written:\n  - {}",
                path.display(),
                defects.join("\n  - ")
            );
            std::process::exit(1);
        }
        Applied::IoError(e) => eprintln!("scan-patterns-apply: {e}"),
    }
}

/// Everything a mold CLAIMS that the machine can check, checked.
///
/// The authoring agent is good at the thing only reading finds — that a row
/// component carries the full keyboard-accessibility quartet, that a directory
/// entry exposes fields where a caller would reach for methods. It is
/// measurably bad at the thing a program computes exactly: counts, orders and
/// paths. Measured over one real enrich: every wrong claim was a tally or a
/// path, every right one was a behaviour. So the tally and the path stop being
/// the agent's job here.
///
/// Two checks, both fatal — a mold is written once and then auto-loads into
/// every future edit of its folder, so a false claim is not a typo, it is a
/// lesson taught forever:
///
/// 1. **Every `Ref:` resolves.** A mold saying "see how X does it" about a file
///    that does not exist does not teach, it misdirects. Measured on the molds
///    this repo carried before the check: 9 of 54 references pointed at files
///    that had been deleted or never existed.
/// 2. **`paths:` is the worklist's own value.** The frontmatter `paths:` is the
///    ONE key the platform reads to decide when a mold loads, and the agent is
///    told to copy it verbatim from the worklist — but nothing verified the
///    copy. A widened glob silently scopes the mold to a whole subproject; a
///    narrowed one silently kills it. The value is re-derived here from the
///    same [`super::list::collect`] the prompt was rendered from, so agreement
///    is proven rather than trusted.
///
/// A cluster the worklist no longer proposes yields no `paths:` expectation —
/// the mold is still written (its `Ref:`s are still checked), because refusing
/// there would block a legitimate re-author of an adopted mold.
fn grounding_defects(body: &str, path: &Path, root: &Path) -> Vec<String> {
    let mut out = Vec::new();

    for cited in cited_refs(body) {
        if !root.join(&cited).exists() {
            out.push(format!(
                "`Ref: {cited}` does not exist under {} — a mold may only cite files it read",
                root.display()
            ));
        }
    }

    let mold_rel = normalise(&path.to_string_lossy());
    if let Some(expected) =
        super::list::collect(root).into_iter().find(|c| normalise(&c.mold_path) == mold_rel || mold_rel.ends_with(&normalise(&c.mold_path)))
    {
        let declared = declared_paths(body);
        if declared != expected.paths {
            out.push(format!(
                "`paths:` must be the worklist's value copied verbatim — expected {:?}, got {:?}",
                expected.paths, declared
            ));
        }
        // 3. The census-owned lead of `## Convention`. Folder, extension and
        //    tally are facts the model holds exactly; the agent is handed the
        //    line and copies it, so a mold can no longer claim "9 files" over a
        //    house of 10.
        let line = super::list::convention_line(&expected);
        if !body.contains(&line) {
            out.push(format!(
                "the `## Convention` lead must be the census line copied verbatim — expected `{line}`"
            ));
        }
    }
    out
}

/// Every path a `Ref:` line cites, forward-slashed. Tolerates the backtick and
/// em-dash decoration the canonical mold format uses around the path.
fn cited_refs(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in body.lines() {
        let Some(rest) = line.split_once("Ref:").map(|(_, r)| r) else { continue };
        let token = rest
            .trim()
            .trim_start_matches('`')
            .split(['`', ' ', ','])
            .next()
            .unwrap_or("")
            .trim_end_matches([':', '.', ','])
            .trim();
        // A trailing `)` closes the citation only when nothing opened it: Next.js
        // route groups carry parentheses INSIDE the path itself, so splitting on
        // `)` truncated `app/(dashboard)/x.tsx` to `app/(dashboard` and refused
        // every mold that cited a real route.
        let token = trim_unbalanced_close(token);
        // A citation is a path: it must carry a separator or an extension dot.
        if !token.is_empty() && (token.contains('/') || token.contains('.')) {
            out.push(normalise(token));
        }
    }
    out
}

/// Drops trailing `)` characters that no `(` in the token opened — the prose
/// decoration `(see …)` around a citation, never the parentheses of a path.
fn trim_unbalanced_close(mut token: &str) -> &str {
    while token.ends_with(')') && token.matches(')').count() > token.matches('(').count() {
        token = &token[..token.len() - 1];
    }
    token
}

/// The frontmatter `paths:` key as the mold actually wrote it: where it sits and
/// what it declares.
///
/// The span exists so [`canonical_paths_form`] can rewrite exactly those lines
/// without re-finding the key under a second set of rules.
struct PathsKey {
    /// Index of the `paths:` line.
    start: usize,
    /// One past the last line the key owns — its block items when it has them,
    /// otherwise `start + 1`.
    end: usize,
    /// The declared globs, in document order, whichever form declared them.
    values: Vec<String>,
}

/// Locate and read the frontmatter `paths:` key in ANY of the three YAML forms
/// it has been seen to arrive in. Reads only the frontmatter block, so a
/// `paths:` word in the prose below cannot be mistaken for the key.
///
/// The three forms are the block list (`paths:` + `  - v`), the inline scalar
/// (`paths: v`, and `paths: a, b` — what a comma-joined worklist line produced
/// when copied verbatim) and the flow sequence (`paths: [a, b]`).
///
/// Reading all three is deliberate: this key is checked to prove the VALUE
/// copied from the worklist, and the YAML form is not what the check measures.
/// It used to read the block list only, so a mold that carried the right globs
/// in another shape was refused for its punctuation — 19 of 79 molds in one
/// enrich, three re-dispatches, and nothing proven by any of them. Tolerating
/// the shape on the way IN is safe only because [`canonical_paths_form`]
/// rewrites it on the way OUT: exactly one shape reaches disk.
fn paths_key(body: &str) -> Option<PathsKey> {
    let lines: Vec<&str> = body.lines().collect();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return None;
    }
    for (i, line) in lines.iter().enumerate().skip(1) {
        let trimmed = line.trim();
        if trimmed == "---" {
            return None; // frontmatter closed without the key
        }
        let Some(rest) = trimmed.strip_prefix("paths:") else { continue };
        let rest = rest.trim();
        // Flow sequence: `paths: [a, b]`. The closing bracket is optional so a
        // truncated line still surrenders its values instead of reading as one
        // scalar that happens to open with `[`.
        if let Some(inner) = rest.strip_prefix('[') {
            let inner = inner.strip_suffix(']').unwrap_or(inner);
            return Some(PathsKey { start: i, end: i + 1, values: split_values(inner) });
        }
        // Inline scalar: `paths: a` — and `paths: a, b`, which is one YAML
        // string but carries the worklist's whole value, so it is split back
        // into the globs it joined. A glob from `list::globs_for` is always
        // `<dir>/**`, so a comma inside one is not a shape this can meet.
        if !rest.is_empty() {
            return Some(PathsKey { start: i, end: i + 1, values: split_values(rest) });
        }
        // Block list — the canonical form.
        let mut values = Vec::new();
        let mut end = i + 1;
        for item in lines.iter().skip(i + 1) {
            let item = item.trim();
            if item == "---" {
                break;
            }
            let Some(v) = item.strip_prefix("- ") else { break };
            let v = normalise(v.trim());
            if !v.is_empty() {
                values.push(v);
            }
            end += 1;
        }
        return Some(PathsKey { start: i, end, values });
    }
    None
}

/// Split one inline `paths:` value into its globs: comma-separated, unquoted,
/// forward-slashed, blanks dropped.
fn split_values(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|v| normalise(v.trim().trim_matches(['"', '\'']).trim()))
        .filter(|v| !v.is_empty())
        .collect()
}

/// The `paths:` list declared in the mold's frontmatter, in document order.
fn declared_paths(body: &str) -> Vec<String> {
    paths_key(body).map(|k| k.values).unwrap_or_default()
}

/// Rewrite the frontmatter `paths:` key into the block-list form every mold on
/// disk already carries — `paths:` at the top level, one `  - <glob>` per line.
///
/// [`paths_key`] tolerates three shapes so a mold is never refused over its
/// punctuation; without this the tolerated shape would REACH disk, and the
/// platform reads that file, not this parser. Tolerate on the way in, normalise
/// on the way out. Returns `body` untouched when there is no key to rewrite (or
/// it declares nothing) — an absent key stays absent.
fn canonical_paths_form(body: &str) -> String {
    let Some(key) = paths_key(body) else { return body.to_string() };
    if key.values.is_empty() {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len() + 16);
    for (i, line) in body.lines().enumerate() {
        if i == key.start {
            out.push_str("paths:\n");
            for v in &key.values {
                out.push_str("  - ");
                out.push_str(v);
                out.push('\n');
            }
        } else if i < key.start || i >= key.end {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The four `## ` sections the mold prompt contracts, in the order it names
/// them.
const CANONICAL_SECTIONS: [&str; 4] =
    ["## Purpose", "## Convention", "## How to apply", "## Examples"];

/// The ways the mold's SECTIONS depart from the shape the prompt contracts:
/// exactly [`CANONICAL_SECTIONS`], each once, in that order, and no other
/// `## ` heading. Empty when the mold has that shape.
///
/// The prompt has always named these four titles; nothing verified them, and
/// two molds reached disk with `## How to apply` twice, no `## Convention` and
/// `## Examples` in the middle. A mold is authored ONCE and then auto-loads into
/// every later edit of its folder, so a shape defect is not a typo the next pass
/// fixes — it is permanent. Structure is exactly the thing a program checks
/// exactly, so it stops being the agent's job here, like the tally and the path
/// before it ([`grounding_defects`]).
fn structure_defects(body: &str) -> Vec<String> {
    let found = level_two_headings(body);
    if found == CANONICAL_SECTIONS {
        return Vec::new();
    }
    let contract = CANONICAL_SECTIONS.join(", ");
    let mut out = Vec::new();
    for want in CANONICAL_SECTIONS {
        match found.iter().filter(|h| h.as_str() == want).count() {
            1 => {}
            0 => out.push(format!("missing `{want}` — a mold carries exactly {contract}")),
            n => out.push(format!("`{want}` appears {n} times — each section appears exactly once")),
        }
    }
    let mut reported: Vec<&str> = Vec::new();
    for heading in &found {
        let heading = heading.as_str();
        if CANONICAL_SECTIONS.contains(&heading) || reported.contains(&heading) {
            continue;
        }
        reported.push(heading);
        out.push(format!("`{heading}` is not a mold section — a mold carries exactly {contract}"));
    }
    // Right sections, wrong order: nothing above fires, so the order is the
    // whole defect and it must still be named.
    if out.is_empty() {
        out.push(format!(
            "the sections are out of order — expected {contract}, got {}",
            found.join(", ")
        ));
    }
    out
}

/// Every level-two heading of `body`, in document order. Fenced code is skipped:
/// a `## ` line inside a ``` block is a sample, not a section, and refusing a
/// mold that shows one would punish the molds that teach best.
fn level_two_headings(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in body.lines() {
        let line = line.trim_end();
        let lead = line.trim_start();
        if lead.starts_with("```") || lead.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if !fenced && line.starts_with("## ") {
            out.push(line.to_string());
        }
    }
    out
}

/// Forward-slash a path so a value authored on either platform compares equal.
fn normalise(s: &str) -> String {
    s.replace('\\', "/")
}

/// Apply's OWN blank-check, and nothing more, on top of the shared envelope
/// reader ([`super::read_envelope`]): `-` reads stdin, `@<path>` reads that
/// file, anything else is the literal body. `None` when there is nothing to
/// write — a blank body, or a `@<path>` that could not be read.
///
/// The resolution itself is NOT duplicated here. This command and
/// [`super::relay`] each used to keep a copy with a divergent contract, so the
/// file face would have had to be written twice; now the reader is one and only
/// the trim is local. The IO failure is named on stderr rather than folded into
/// the blank case, because "the file could not be read" and "the agent authored
/// an empty body" are different things to fix.
///
/// Every `None` leaves stderr already carrying its reason — exactly one line,
/// either the IO failure or [`EMPTY_BODY`] — which is what lets [`run`] stop on
/// it without adding a second. It used to `unwrap_or_default()` into
/// [`apply_one`], so an unreadable path printed its IO error and then
/// `empty mold body` underneath it: the true reason first, the misleading one
/// last, and the last is the one a reader keeps (found in review, 2026-08-04).
fn resolve_content(content: &str) -> Option<String> {
    let raw = match super::read_envelope(content) {
        super::Envelope::Raw(text) | super::Envelope::Json(text) => text,
        super::Envelope::Unreadable(e) => {
            eprintln!("scan-patterns-apply: {e}");
            return None;
        }
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        eprintln!("{EMPTY_BODY}");
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Whether `path` has the mold shape: it lives under a `.claude/skills/` segment
/// and ends in `<slug>-pattern/SKILL.md`. Backslashes are normalised so a Windows
/// path passes the same check.
fn is_mold_path(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains(SKILLS_SEGMENT) && s.ends_with(MOLD_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mold(root: &Path, rel: &str) -> std::path::PathBuf {
        root.join(rel)
    }

    #[test]
    fn is_mold_path_accepts_the_shape_and_rejects_others() {
        assert!(is_mold_path(Path::new("apps/api/.claude/skills/api-service-pattern/SKILL.md")));
        // Windows separators normalise.
        assert!(is_mold_path(Path::new(r"apps\api\.claude\skills\api-service-pattern\SKILL.md")));
        // Wrong file, wrong folder, missing skills segment — all refused.
        assert!(!is_mold_path(Path::new("apps/api/.claude/skills/api-service-pattern/README.md")));
        assert!(!is_mold_path(Path::new("apps/api/.claude/agents/x-pattern/SKILL.md")));
        assert!(!is_mold_path(Path::new("apps/api/src/service.rs")));
        assert!(!is_mold_path(Path::new("CLAUDE.md")));
    }

    #[test]
    fn resolve_content_blanks_are_none() {
        assert!(resolve_content("   \n  ").is_none());
        assert_eq!(resolve_content("# mold").as_deref(), Some("# mold"));
        // A `@<path>` that cannot be read is the OTHER `None`, and it is the
        // one `run` must stop on: it has already named the IO failure, so
        // carrying an empty body onward would print `empty mold body` under it
        // and blame the agent for the file's failure.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-return.json");
        assert!(resolve_content(&format!("@{}", missing.display())).is_none());
    }

    /// AC-4 — the apply takes the SAME `@<path>` face the relay does, through
    /// the SAME reader. Two copies of the resolution is how the relay grew a
    /// file face the apply lacked, so the shared reader is the guard: a body
    /// too large for an argv reaches either command the same way.
    #[test]
    fn apply_reads_the_mold_body_from_a_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let on_disk = dir.path().join("authored-mold.md");
        std::fs::write(&on_disk, valid_mold("api-service-pattern")).unwrap();

        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        run(&path, &format!("@{}", on_disk.display()), dir.path());

        let written = std::fs::read_to_string(&path).expect("the mold is written from the file");
        assert!(written.contains("## Purpose"), "the body survives whole: {written}");
        assert!(super::super::origin::is_mustard_generated(&written), "still stamped as generated");
        // The literal face is untouched: a body passed inline still resolves.
        assert_eq!(
            resolve_content(&valid_mold("api-x-pattern")).as_deref(),
            Some(valid_mold("api-x-pattern").as_str())
        );
    }

    /// A well-formed generated mold body — frontmatter-first, `name` +
    /// `description` + `source: scan`, and the four canonical sections in
    /// order, all of which the apply now requires.
    fn valid_mold(name: &str) -> String {
        format!(
            "---\nname: {name}\ndescription: Use when adding or refactoring an X.\nsource: scan\n---\n\n{}",
            canonical_sections().trim_end()
        )
    }

    /// The four `## ` sections in the contracted order, each with a line of
    /// body — the shape [`structure_defects`] requires.
    fn canonical_sections() -> String {
        CANONICAL_SECTIONS
            .iter()
            .map(|s| format!("{s}\nbody\n"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn run_writes_and_marks_generated() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        run(&path, &valid_mold("api-service-pattern"), dir.path());
        assert!(path.exists(), "mold written");
        let got = std::fs::read_to_string(&path).unwrap();
        assert!(got.contains("## Purpose"), "body preserved: {got}");
        assert!(got.contains("<!-- mustard:generated"), "origin notice injected");
        assert!(super::super::origin::is_mustard_generated(&got), "reads as generated");
    }

    #[test]
    fn run_is_create_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "HAND AUTHORED — keep me").unwrap();
        // A write over an existing mold must NOT clobber it — survivors are
        // hand-authored (the sweep removed the generated ones already).
        run(&path, "---\nname: x\nsource: scan\n---\nregenerated", dir.path());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "HAND AUTHORED — keep me");
    }

    #[test]
    fn a_collision_is_not_reported_as_a_preserve() {
        // Both cases leave the file untouched, so behaviour alone cannot tell
        // them apart — the REPORT is the whole product here. A survivor marked
        // `source: scan` was written by this very run (the sweep clears them
        // all first), so it is a discarded authoring pass, never a human's file.
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-report-pattern/SKILL.md");
        run(&path, &valid_mold("api-report-pattern"), dir.path());
        let first = std::fs::read_to_string(&path).unwrap();
        // Second block for the SAME mold path — the collision.
        run(&path, &valid_mold("api-report-pattern"), dir.path());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first, "create-only still holds");
        assert!(super::super::origin::is_mustard_generated(&first), "the survivor is this run's own");
    }

    #[test]
    fn a_malformed_mold_is_refused_never_written() {
        // The whole point: a mold without `source: scan` (or without
        // frontmatter at all) that reached disk would be an orphan the sweep
        // can never reclaim. The gate refuses it, and nothing is written.
        let cases = [
            ("no frontmatter", "## Purpose\njust prose, no `---`"),
            ("no source", "---\nname: api-x-pattern\ndescription: Use when …\n---\nbody"),
            ("no name", "---\ndescription: Use when …\nsource: scan\n---\nbody"),
        ];
        for (label, body) in cases {
            let defects = super::super::origin::frontmatter_defects(body);
            assert!(!defects.is_empty(), "{label}: should be rejected, got no defects");
        }
        // A valid one has zero defects — the gate is not over-eager.
        assert!(
            super::super::origin::frontmatter_defects(&valid_mold("api-x-pattern")).is_empty(),
            "a canonical mold must pass"
        );
    }

    /// A mold whose authored body carries `paths:` must reach disk with the key
    /// and its glob intact. The command writes the agent's block verbatim, so
    /// this is a regression guard rather than new behaviour — but the scoping
    /// key is the whole reason the mold is scoped, and a normalisation added
    /// later that dropped an unknown key would silently unscope every mold.
    #[test]
    fn run_preserves_the_paths_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        run(
            &path,
            &format!(
                "---\nname: api-service-pattern\ndescription: Use when adding or refactoring an X.\npaths:\n  - apps/api/services/**\nsource: scan\n---\n\n{}",
                canonical_sections()
            ),
            dir.path(),
        );
        let written = std::fs::read_to_string(&path).expect("mold written");
        assert!(written.contains("paths:"), "the scoping key must survive: {written}");
        assert!(
            written.contains("apps/api/services/**"),
            "the glob itself must survive: {written}"
        );
    }

    #[test]
    fn a_ref_to_a_file_that_does_not_exist_is_a_defect() {
        // The agent's weak spot, made fatal: a mold that says "see how X does
        // it" about a deleted file teaches the wrong thing forever, because the
        // mold auto-loads into every later edit of its folder.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("apps/api/services")).unwrap();
        std::fs::write(dir.path().join("apps/api/services/UserService.x"), "real").unwrap();

        let body = format!(
            "{}\n- Ref: `apps/api/services/UserService.x`\n- Ref: `apps/api/services/GhostService.x`",
            valid_mold("api-service-pattern")
        );
        let defects = grounding_defects(&body, Path::new("apps/api/.claude/skills/api-service-pattern/SKILL.md"), dir.path());
        assert_eq!(defects.len(), 1, "only the ghost is a defect: {defects:?}");
        assert!(defects[0].contains("GhostService.x"), "the offender is named: {defects:?}");
    }

    #[test]
    fn a_widened_or_narrowed_paths_value_is_a_defect() {
        // `paths:` is the ONE key that decides when a mold loads. The agent is
        // told to copy the worklist's value verbatim; this proves the copy
        // instead of trusting it. A model with one real cluster gives the
        // expectation something to disagree with.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(
            dir.path().join(".claude/grain.model.json"),
            r#"{"projects":[{"name":"api","dir":"apps/api"}],
                "roles":[{"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services"}],
                "modules":[{"path":"apps/api/services/UserService.x"},{"path":"apps/api/services/OrderService.x"}]}"#,
        )
        .unwrap();

        let mold_rel = "apps/api/.claude/skills/api-service-pattern/SKILL.md";
        let expected = super::super::list::collect(dir.path())
            .into_iter()
            .find(|c| c.mold_path == mold_rel)
            .expect("the fixture proposes this cluster");
        assert_eq!(expected.paths, vec!["apps/api/services/**"], "worklist value");

        // The census line the agent is handed — asserted through the same
        // function the renderer and the apply use, so the test cannot drift
        // from the contract it is guarding.
        let census = super::super::list::convention_line(&expected);
        assert!(census.contains("Files of this role in this subproject: 2"), "tally is the census's: {census}");
        let with_correct = format!(
            "---\nname: api-service-pattern\ndescription: Use when adding or refactoring an X.\npaths:\n  - apps/api/services/**\nsource: scan\n---\n\n## Purpose\nbody\n\n## Convention\n{census}\nVisibility is pub by habit."
        );
        assert!(
            grounding_defects(&with_correct, Path::new(mold_rel), dir.path()).is_empty(),
            "the verbatim copy passes: {:?}",
            grounding_defects(&with_correct, Path::new(mold_rel), dir.path())
        );

        // The tally reworded — the exact class of error this check exists for.
        let reworded = with_correct.replace("this subproject: 2", "this subproject: 9");
        let defects = grounding_defects(&reworded, Path::new(mold_rel), dir.path());
        assert_eq!(defects.len(), 1, "a wrong tally is refused: {defects:?}");
        assert!(defects[0].contains("Convention"), "the reason names the section: {defects:?}");

        // Widened to the whole subproject — the silent failure this catches.
        // Only the frontmatter value is widened, so the census line stays
        // correct and the `paths:` defect is the only one reported.
        let widened = with_correct.replacen("  - apps/api/services/**", "  - apps/api/**", 1);
        let defects = grounding_defects(&widened, Path::new(mold_rel), dir.path());
        assert_eq!(defects.len(), 1, "the widened glob is refused: {defects:?}");
        assert!(defects[0].contains("verbatim"), "the reason names the contract: {defects:?}");

        // Dropped entirely — an unscoped mold loads on every edit of the house.
        let dropped = with_correct.replace("paths:\n  - apps/api/services/**\n", "");
        assert!(
            grounding_defects(&dropped, Path::new(mold_rel), dir.path())
                .iter()
                .any(|d| d.contains("`paths:`")),
            "a missing paths: is the same defect as a wrong one"
        );
    }

    /// The `paths:` check exists to prove the VALUE the worklist handed the
    /// agent, and the YAML form is not what it measures. Reading the block list
    /// only cost 19 refusals over 79 molds and three re-dispatches, none of
    /// which proved anything — so every form that carries the right value
    /// passes, and exactly one form reaches disk.
    #[test]
    fn an_inline_paths_value_is_accepted_and_written_as_a_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(
            dir.path().join(".claude/grain.model.json"),
            r#"{"projects":[{"name":"api","dir":"apps/api"}],
                "roles":[{"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services"}],
                "modules":[{"path":"apps/api/services/UserService.x"},{"path":"apps/api/services/OrderService.x"}]}"#,
        )
        .unwrap();

        let mold_rel = "apps/api/.claude/skills/api-service-pattern/SKILL.md";
        let expected = super::super::list::collect(dir.path())
            .into_iter()
            .find(|c| c.mold_path == mold_rel)
            .expect("the fixture proposes this cluster");
        let census = super::super::list::convention_line(&expected);
        let body = |paths: &str| {
            format!(
                "---\nname: api-service-pattern\ndescription: Use when adding or refactoring an X.\n{paths}\nsource: scan\n---\n\n## Purpose\nbody\n\n## Convention\n{census}\n\n## How to apply\nbody\n\n## Examples\nbody\n"
            )
        };

        for form in [
            "paths:\n  - apps/api/services/**",
            "paths: apps/api/services/**",
            "paths: [apps/api/services/**]",
            "paths: [\"apps/api/services/**\"]",
        ] {
            let defects = grounding_defects(&body(form), Path::new(mold_rel), dir.path());
            assert!(defects.is_empty(), "`{form}` carries the worklist value: {defects:?}");
        }

        // The exact shape the old worklist produced when copied verbatim: one
        // scalar joining the globs by comma. It is the VALUE, so it reads.
        assert_eq!(
            declared_paths(&body("paths: apps/api/services/**, apps/api/handlers/**")),
            ["apps/api/services/**", "apps/api/handlers/**"]
        );
        // A widened value is still refused — tolerance is of the form, never of
        // the glob.
        assert!(
            !grounding_defects(&body("paths: apps/api/**"), Path::new(mold_rel), dir.path())
                .is_empty(),
            "a widened inline value is still a defect"
        );

        // ...and the tolerated form is NOT what reaches disk: the platform reads
        // the file, not this parser.
        let path = mold(dir.path(), mold_rel);
        run(&path, &body("paths: apps/api/services/**"), dir.path());
        let written = std::fs::read_to_string(&path).expect("mold written");
        assert!(
            written.contains("paths:\n  - apps/api/services/**\n"),
            "an inline value is normalised to the canonical block list: {written}"
        );
        assert!(!written.contains("paths: apps"), "the inline form is gone: {written}");
    }

    /// The prompt has always contracted four sections and nothing checked them,
    /// so two molds reached disk with `## How to apply` twice, no
    /// `## Convention` and `## Examples` in the middle. A mold auto-loads into
    /// every later edit of its folder, so that defect is permanent.
    #[test]
    fn a_mold_whose_headings_are_wrong_is_refused() {
        assert!(
            structure_defects(&valid_mold("api-x-pattern")).is_empty(),
            "the canonical shape passes"
        );
        let cases = [
            ("a missing section", "## Purpose\na\n\n## How to apply\nb\n\n## Examples\nc", "missing `## Convention`"),
            ("a duplicated section", "## Purpose\na\n\n## Convention\nb\n\n## How to apply\nc\n\n## How to apply\nd\n\n## Examples\ne", "appears 2 times"),
            ("the wrong order", "## Purpose\na\n\n## Convention\nb\n\n## Examples\nc\n\n## How to apply\nd", "out of order"),
            ("a section nobody contracted", "## Purpose\na\n\n## Convention\nb\n\n## How to apply\nc\n\n## Examples\nd\n\n## Notes\ne", "`## Notes` is not a mold section"),
        ];
        for (label, sections, needle) in cases {
            let defects = structure_defects(sections);
            assert!(
                defects.iter().any(|d| d.contains(needle)),
                "{label}: the defect must name it — got {defects:?}"
            );
        }

        // The refusal reaches the write: a broken mold never lands.
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-x-pattern/SKILL.md");
        let broken = valid_mold("api-x-pattern").replace("## Convention", "## How to apply");
        assert!(matches!(apply_one(&path, &broken, dir.path()), Applied::Refused(_)));
        assert!(!path.exists(), "a mold with a broken section list is never written");

        // A `## ` line inside fenced code is a sample, not a section — refusing
        // it would punish the molds that teach with an example.
        let fenced = format!("{}\n```md\n## Not A Section\n```\n", valid_mold("api-x-pattern"));
        assert!(structure_defects(&fenced).is_empty(), "fenced samples are not sections");
    }

    /// The section check run against every `-pattern` mold this repository
    /// already carries. They were measured as conforming, so a red here means
    /// the CHECK is wrong, not the molds. Reads outside the crate fail open
    /// (skip) per this codebase's test convention.
    #[test]
    fn every_mold_this_repository_carries_passes_the_new_checks() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let Some(workspace) = manifest_dir.parent().and_then(Path::parent) else {
            eprintln!("[skip] cannot resolve workspace root from CARGO_MANIFEST_DIR");
            return;
        };
        let molds = repo_molds(workspace);
        if molds.is_empty() {
            eprintln!("[skip] no `-pattern` mold found under {}", workspace.display());
            return;
        }
        for m in &molds {
            let Ok(text) = std::fs::read_to_string(m) else { continue };
            let defects = structure_defects(&text);
            assert!(defects.is_empty(), "{}: {defects:?}", m.display());
            // The corpus is also already in the form the write normalises to,
            // so normalising can never churn a mold that was fine.
            assert_eq!(
                canonical_paths_form(&text).trim_end(),
                text.trim_end(),
                "{} is already in the canonical `paths:` form",
                m.display()
            );
        }
    }

    /// Every `…/.claude/skills/<slug>-pattern/SKILL.md` under `root`, by a
    /// bounded walk — the corpus the check above is measured against.
    fn repo_molds(root: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![(root.to_path_buf(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if depth > 4 {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if matches!(name.as_str(), "target" | "node_modules" | ".git") {
                    continue;
                }
                if name.starts_with('.') && name != ".claude" {
                    continue;
                }
                if name.ends_with("-pattern") {
                    let skill = path.join("SKILL.md");
                    if skill.is_file() {
                        out.push(skill);
                    }
                    continue;
                }
                stack.push((path, depth + 1));
            }
        }
        out.sort();
        out
    }

    #[test]
    fn cited_refs_reads_the_canonical_decoration() {
        let body = "- Ref: `apps/api/x.rs` — the shape\n- Ref: apps/api/y.rs\n- not a ref line\n- Ref: `pkg/z.ts`, and prose";
        assert_eq!(cited_refs(body), vec!["apps/api/x.rs", "apps/api/y.rs", "pkg/z.ts"]);
    }

    #[test]
    fn cited_refs_keeps_parentheses_that_belong_to_the_path() {
        // Next.js route groups live inside the path; prose parentheses do not.
        let body = "- Ref: `app/(dashboard)/banks/[id]/loading.tsx` (the canonical four lines)\n\
                    - Ref: app/(auth)/register/page.tsx\n\
                    - Ref: pkg/z.ts)";
        assert_eq!(
            cited_refs(body),
            vec![
                "app/(dashboard)/banks/[id]/loading.tsx",
                "app/(auth)/register/page.tsx",
                "pkg/z.ts"
            ]
        );
    }
}
