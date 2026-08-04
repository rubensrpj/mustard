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
    // the agent's block was trimmed, and swept fresh on the next scan.
    let out = super::origin::stamp(body);
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

/// The `paths:` list declared in the mold's frontmatter, in document order.
/// Reads only the frontmatter block, so a `paths:` word in the prose below
/// cannot be mistaken for the key.
fn declared_paths(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_frontmatter = false;
    let mut in_paths = false;
    for (i, line) in body.lines().enumerate() {
        let trimmed = line.trim_end();
        if trimmed.trim() == "---" {
            if i == 0 {
                in_frontmatter = true;
                continue;
            }
            if in_frontmatter {
                break;
            }
        }
        if !in_frontmatter {
            continue;
        }
        if trimmed.trim_start().starts_with("paths:") {
            in_paths = true;
            continue;
        }
        if in_paths {
            let item = trimmed.trim_start();
            if let Some(v) = item.strip_prefix("- ") {
                out.push(normalise(v.trim()));
            } else {
                in_paths = false;
            }
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
    /// `description` + `source: scan`, which the apply now requires.
    fn valid_mold(name: &str) -> String {
        format!(
            "---\nname: {name}\ndescription: Use when adding or refactoring an X.\nsource: scan\n---\n\n## Purpose\nbody"
        )
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
            "---\nname: api-service-pattern\ndescription: Use when adding or refactoring an X.\npaths:\n  - apps/api/services/**\nsource: scan\n---\n\n## Purpose\nbody",
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
            "{}\n\n## Examples\n- Ref: `apps/api/services/UserService.x`\n- Ref: `apps/api/services/GhostService.x`",
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
