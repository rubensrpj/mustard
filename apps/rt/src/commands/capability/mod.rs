//! `mustard-rt run capability` — author and read durable capability docs.
//!
//! A [`Capability`](mustard_core::domain::capability::Capability) is the
//! living "what the system does" record (see the core domain module for why it
//! is not a decaying `Knowledge`). This rt module owns its **on-disk markdown
//! form** and the parse↔render pair, mirroring how `spec_sections` / `qa_run`
//! keep spec/QA markdown in rt while the *types* live in core. The pure domain
//! type and the single `[[ ]]` scanner ([`scan_links`]) are reused — this
//! module never defines a parallel `Capability`, a parallel `AcceptanceCriterion`,
//! or a second wikilink scanner.
//!
//! ## Markdown form (`.claude/capabilities/{slug}.md`)
//!
//! A capability is a NEW artifact, not a spec, so unlike `spec.md` (pure
//! narrative) its frontmatter may carry structured fields:
//!
//! ```text
//! ---
//! id: cap.{slug}
//! status: active
//! ---
//!
//! # {title}
//!
//! ### Requirement: {statement}
//!
//! #### Scenario: {name}
//! - when: {when}
//! - then: {then}
//! - command: `{cmd}`        (optional line)
//!
//! ## Covers
//! - [[entity.X]]
//!
//! ## Specs
//! - [[spec.Y]]
//!
//! ## Related
//! - [[cap.Z]]
//! ```
//!
//! The structural tokens — the `### Requirement:` / `#### Scenario:` headings,
//! the `when:` / `then:` / `command:` field labels, and the `## Covers` /
//! `## Specs` / `## Related` link-section headings — are the **machine
//! contract**: language-agnostic, stable, EN. The parser keys off them
//! verbatim and the renderer emits them verbatim, so a doc round-trips
//! regardless of the project's narrative locale. Only the H1 title is free
//! prose. Link bullets are parsed by reusing [`scan_links`] (so
//! `- [[entity.X]]` yields the inner token `entity.X`), never a second
//! scanner.
//!
//! ## Fail-open
//!
//! [`parse`] never panics: a malformed document degrades to a best-effort
//! [`Capability`] (missing sections → empty vecs, a header with no value →
//! empty string). [`create`] reports usage errors as JSON and returns; nothing
//! here uses `unwrap`/`expect` outside `#[cfg(test)]`.

pub mod cli;

use crate::shared::context::project_dir;
use mustard_core::domain::capability::{Capability, Requirement, Scenario};
use mustard_core::io::atomic_md::scan_links;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs as mfs;
use mustard_core::Scan;
use serde_json::json;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Structural tokens — the machine contract. Single source for parse + render.
// ---------------------------------------------------------------------------

/// Heading prefix opening one requirement block (text after it = statement).
const REQUIREMENT_PREFIX: &str = "### Requirement:";
/// Heading prefix opening one scenario block (text after it = scenario name).
const SCENARIO_PREFIX: &str = "#### Scenario:";
/// Field-label prefix for a scenario's `when` line.
const WHEN_PREFIX: &str = "- when:";
/// Field-label prefix for a scenario's `then` line.
const THEN_PREFIX: &str = "- then:";
/// Field-label prefix for a scenario's optional `command` line.
const COMMAND_PREFIX: &str = "- command:";
/// H2 opening the `covers` link section.
const COVERS_HEADING: &str = "## Covers";
/// H2 opening the `specs` link section.
const SPECS_HEADING: &str = "## Specs";
/// H2 opening the `related` link section.
const RELATED_HEADING: &str = "## Related";

/// H2 heading that opens a SPEC's optional capability-link section
/// (`## Capabilities` in `<spec>/spec.md`). This is the spec-side counterpart to
/// the capability doc's own `## Covers` / `## Specs` / `## Related` headings:
/// machine contract, language-agnostic, stable, EN. Single source so the
/// merge-on-close (`complete-spec`) reader and the QA reader (`qa-run`) detect
/// the section identically and can never drift.
pub(crate) const CAPABILITIES_HEADING: &str = "## Capabilities";

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run capability create`.
pub struct CapabilityCreateOpts {
    /// Capability slug (the `{slug}` in `cap.{slug}` and the file name).
    pub slug: String,
    /// Human-readable title (narrative locale, free prose).
    pub title: String,
    /// Lifecycle word (defaults to `active`). Free string — kept agnostic.
    pub status: String,
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch the verb (`create` / `show` / `sync-nodes`). An unknown subcommand
/// emits a JSON error and returns — never panics.
pub fn dispatch(subcommand: Option<&str>, slug: &str, opts: CapabilityCreateOpts) {
    match subcommand.unwrap_or("create") {
        "create" => create(opts),
        "show" => show(slug),
        "sync-nodes" => sync_nodes(slug),
        other => emit_error("unknown subcommand", other),
    }
}

fn create(opts: CapabilityCreateOpts) {
    create_with_root(opts, &PathBuf::from(project_dir()));
}

/// Same as [`create`] but with an explicit project root, so tests inject a temp
/// directory without mutating the process environment.
fn create_with_root(opts: CapabilityCreateOpts, project: &Path) {
    if opts.slug.trim().is_empty() {
        emit_error("missing --slug", "");
        return;
    }
    let dir = match capabilities_dir(project) {
        Ok(d) => d,
        Err(e) => {
            emit_error("invalid project path", &e);
            return;
        }
    };
    if let Err(e) = mfs::create_dir_all(&dir) {
        emit_error("could not create capabilities directory", &e.to_string());
        return;
    }
    let target = dir.join(format!("{}.md", opts.slug));
    if target.exists() {
        emit_error("capability exists", &target.display().to_string());
        return;
    }

    // A freshly created capability has the title + status + an empty body;
    // requirements/links are filled in later (next task) or by hand.
    let status = {
        let s = opts.status.trim();
        if s.is_empty() { "active".to_string() } else { s.to_string() }
    };
    let cap = Capability {
        id: format!("cap.{}", opts.slug),
        title: opts.title.clone(),
        status,
        ..Capability::default()
    };
    let body = render(&cap);
    if let Err(e) = mfs::write_atomic(&target, body.as_bytes()) {
        emit_error("write failed", &e.to_string());
        return;
    }

    let report = json!({
        "ok": true,
        "id": cap.id,
        "slug": opts.slug,
        "path": target.display().to_string(),
    });
    print_json(&report);
}

/// Read `.claude/capabilities/{slug}.md`, parse it, and print the
/// [`Capability`] as byte-stable JSON. A missing / unreadable file degrades to
/// a JSON error (exit 0) — fail-open like every `run` emitter.
fn show(slug: &str) {
    show_with_root(slug, &PathBuf::from(project_dir()));
}

fn show_with_root(slug: &str, project: &Path) {
    if slug.trim().is_empty() {
        emit_error("missing --slug", "");
        return;
    }
    let dir = match capabilities_dir(project) {
        Ok(d) => d,
        Err(e) => {
            emit_error("invalid project path", &e);
            return;
        }
    };
    let target = dir.join(format!("{slug}.md"));
    let Ok(md) = mfs::read_to_string(&target) else {
        emit_error("capability not found", &target.display().to_string());
        return;
    };
    let cap = parse(&md);
    // Byte-stable: struct field order is fixed and the body carries no
    // timestamps/paths — `to_string` is deterministic across runs.
    match serde_json::to_string(&cap) {
        Ok(s) => println!("{s}"),
        Err(e) => emit_error("serialise failed", &e.to_string()),
    }
}

/// Resolve `<project>/.claude/capabilities/` via the typed accessor, mapping
/// the path error to a `String` so callers report it uniformly.
fn capabilities_dir(project: &Path) -> Result<PathBuf, String> {
    ClaudePaths::for_project(project)
        .map(|p| p.capabilities_dir())
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// sync-nodes — materialize the entity nodes a capability covers
// ---------------------------------------------------------------------------
//
// A capability's `## Covers` carries entity wikilink ids (`entity.{name}`). The
// EXISTING resolver (`atomic_md::wikilink::resolve`) resolves a `[[id]]` to any
// `.claude/graph/*.md` whose frontmatter declares `id: {id}` — so the ONLY
// thing needed to make a cover link resolvable (in Obsidian AND in Mustard) is
// a node file carrying that frontmatter. This verb writes one such node per
// covered entity that actually exists in the registry; nothing here touches the
// resolver, the spec generators, the memory feature, or the dashboard.
//
// The "registry" is grain's `grain.model.json` (~498KB). We never load it: the
// membership lookup goes through the in-process `mustard_core::read_entity_names`
// — a thin `scan facts` projection (the subproject list + the deduped
// declaration-name set), NOT the model's own schema, and NOT a self-spawn of
// `mustard-rt`. A cover whose entity name is absent from that set is SKIPPED, so
// a wrong / typo'd cover surfaces as `⚠ unresolved` rather than gaining an
// invented node.

/// The entity-node id convention prefix. A capability covers an entity via the
/// wikilink id `entity.{name}`; the `{name}` after this prefix is the bare
/// declaration name the registry knows. Agnostic — no `rt.` / language / role
/// assumption is baked in; the name is whatever grain mined as a declaration.
const ENTITY_NODE_PREFIX: &str = "entity.";

/// `mustard-rt run capability sync-nodes --slug X`.
///
/// Parse `.claude/capabilities/{X}.md`, then for each `covers` id of the form
/// `entity.{name}` look `{name}` up in the entity registry; for every hit write
/// (or refresh) `.claude/graph/{id}.md` carrying `id: {id}` so the resolver
/// dereferences the cover link. A miss is skipped. Idempotent + byte-stable +
/// atomic. Fail-open: errors emit JSON (exit 0) — never a panic.
fn sync_nodes(slug: &str) {
    sync_nodes_with_root(slug, &PathBuf::from(project_dir()));
}

/// Same as [`sync_nodes`] but with an explicit project root, so tests inject a
/// temp directory without mutating the process environment.
fn sync_nodes_with_root(slug: &str, project: &Path) {
    if slug.trim().is_empty() {
        emit_error("missing --slug", "");
        return;
    }
    let paths = match ClaudePaths::for_project(project) {
        Ok(p) => p,
        Err(e) => {
            emit_error("invalid project path", &e.to_string());
            return;
        }
    };
    let cap_path = paths.capabilities_dir().join(format!("{slug}.md"));
    let Ok(md) = mfs::read_to_string(&cap_path) else {
        emit_error("capability not found", &cap_path.display().to_string());
        return;
    };
    let cap = parse(&md);

    // The registry slice (grain `facts`): the deduped declaration-name set.
    // Empty on a missing model (no scan yet) — every cover then misses and is
    // skipped, which is the honest behaviour (nothing to resolve against yet).
    let model = paths.claude_dir().join("grain.model.json");
    let known: BTreeSet<String> = mustard_core::read_entity_names(&model)
        .into_iter()
        .collect();

    let graph_dir = paths.graph_dir();
    // The source path is pulled from the per-name digest slice — injected as a
    // closure so the materialize core is unit-tested without spawning `scan`.
    let outcome = match materialize_nodes(&cap, &known, &graph_dir, &|name| {
        source_path_for(&model, name)
    }) {
        Ok(o) => o,
        Err(e) => {
            emit_error("write failed", &e);
            return;
        }
    };

    let report = json!({
        "ok": true,
        "slug": slug,
        "id": cap.id,
        "graphDir": graph_dir.display().to_string(),
        "written": outcome.written,
        "skipped": outcome.skipped,
    });
    print_json(&report);
}

/// The result of [`materialize_nodes`]: the cover ids that produced a node and
/// the cover ids that were skipped (not `entity.*`, or absent from the
/// registry). Both sorted (the input is sorted) so the report is byte-stable.
struct SyncOutcome {
    written: Vec<String>,
    skipped: Vec<String>,
}

/// Pure-ish core of `sync-nodes`: for each `entity.{name}` cover of `cap` whose
/// `{name}` is in `known`, write `graph_dir/{id}.md` (atomic) and record it;
/// otherwise skip it. `source_of` resolves the optional source line for a found
/// entity (injected so the registry digest spawn stays out of the unit tests).
///
/// Deterministic: covers are deduped + sorted before iteration, so re-running
/// produces byte-identical files in a stable order. Returns `Err(detail)` only
/// on a filesystem failure (the caller maps it to a JSON error).
fn materialize_nodes(
    cap: &Capability,
    known: &BTreeSet<String>,
    graph_dir: &Path,
    source_of: &dyn Fn(&str) -> Option<String>,
) -> Result<SyncOutcome, String> {
    let mut written: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // Dedup + sort so the side effects and the report are byte-stable
    // regardless of source order / repetition in `## Covers`.
    let covers: BTreeSet<String> = cap.covers.iter().cloned().collect();

    for id in &covers {
        let Some(name) = entity_name_of(id) else {
            // Not an `entity.*` cover (e.g. a `spec.*` that slipped into
            // Covers) — leave it for its own node kind; not our concern.
            skipped.push(id.clone());
            continue;
        };
        if !known.contains(&name) {
            // Unknown / typo'd entity — DO NOT invent a node. The `[[id]]`
            // stays unresolved so the wrong cover is visible.
            skipped.push(id.clone());
            continue;
        }
        // Found: materialize the node. The source path (if the registry slice
        // can name it) is best-effort navigation, not a correctness signal.
        let source = source_of(&name);
        let body = render_entity_node(id, &name, source.as_deref());
        let target = graph_dir.join(format!("{id}.md"));
        mfs::create_dir_all(graph_dir).map_err(|e| e.to_string())?;
        mfs::write_atomic(&target, body.as_bytes()).map_err(|e| e.to_string())?;
        written.push(id.clone());
    }

    Ok(SyncOutcome { written, skipped })
}

/// Strip the `entity.` id prefix to the bare declaration name the registry
/// knows. Returns `None` for an id that is not an `entity.*` cover (so a
/// `spec.*` / `cap.*` cover is left alone). Agnostic — no `rt.` assumption.
fn entity_name_of(id: &str) -> Option<String> {
    id.strip_prefix(ENTITY_NODE_PREFIX)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

/// Best-effort source path for `name` from the registry — the file where the
/// entity is DECLARED. Read via the in-process digest **slice** for that one
/// name (`scan digest --query`, not the full model): we keep a file only from
/// an `exact`-tier report term whose token equals `name` (case-insensitive), so
/// a fuzzy/stem/lexicon hit never attaches a misleading path. Returns the first
/// such file (the report orders them), or `None` when the slice names none.
/// Fail-open: any spawn/parse error yields `None` — the node still resolves,
/// just without the convenience line.
fn source_path_for(model: &Path, name: &str) -> Option<String> {
    if !model.is_file() {
        return None;
    }
    let q = Scan::locate().digest_query(model, &[name.to_string()]).ok()?;
    let needle = name.to_ascii_lowercase();
    q.report
        .terms
        .iter()
        .find(|t| t.tier == "exact" && t.term.eq_ignore_ascii_case(&needle))
        .and_then(|t| t.files.first())
        .cloned()
}

/// Render one entity-node markdown file. The frontmatter `id:` is the load-
/// bearing field — it is what `atomic_md::wikilink::resolve` keys on to
/// dereference `[[{id}]]`. The H1 + the optional source line are navigation
/// only. Byte-stable: no timestamps, fixed field order, trailing newline.
fn render_entity_node(id: &str, name: &str, source: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "id: {id}");
    out.push_str("---\n\n");
    let _ = writeln!(out, "# {name}\n");
    out.push_str("> Entity node materialized from the grain registry so a ");
    out.push_str("capability's `[[");
    out.push_str(id);
    out.push_str("]]` cover resolves. Edit the source, not this file.\n");
    if let Some(path) = source.map(str::trim).filter(|p| !p.is_empty()) {
        out.push('\n');
        let _ = writeln!(out, "- Source: `{path}`");
    }
    out
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Render a [`Capability`] to its canonical markdown form. The inverse of
/// [`parse`]: `parse(render(c))` equals `c` for every field the format carries
/// (id, status, title, requirements with scenarios, and the three link lists).
#[must_use]
pub fn render(cap: &Capability) -> String {
    let mut out = String::new();
    // Frontmatter — id + status (capability is NOT bound by spec.md purity).
    out.push_str("---\n");
    let _ = writeln!(out, "id: {}", cap.id);
    let _ = writeln!(out, "status: {}", cap.status);
    out.push_str("---\n\n");

    // Title.
    let _ = writeln!(out, "# {}\n", cap.title);

    // Requirements, each with its scenarios.
    for req in &cap.requirements {
        let _ = writeln!(out, "{REQUIREMENT_PREFIX} {}\n", req.statement);
        for sc in &req.scenarios {
            let _ = writeln!(out, "{SCENARIO_PREFIX} {}", sc.name);
            let _ = writeln!(out, "{WHEN_PREFIX} {}", sc.when);
            let _ = writeln!(out, "{THEN_PREFIX} {}", sc.then);
            if let Some(cmd) = sc.command.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                // Backticked so the command renders as inline code; the parser
                // strips the backticks symmetrically.
                let _ = writeln!(out, "{COMMAND_PREFIX} `{cmd}`");
            }
            out.push('\n');
        }
    }

    // Link sections — always emitted (an empty list is a bare heading), so the
    // shape is stable for both humans and the auto-footer observer.
    render_link_section(&mut out, COVERS_HEADING, &cap.covers);
    render_link_section(&mut out, SPECS_HEADING, &cap.specs);
    render_link_section(&mut out, RELATED_HEADING, &cap.related);

    out
}

/// Render one `## Heading` + a `- [[id]]` bullet per link.
fn render_link_section(out: &mut String, heading: &str, links: &[String]) {
    let _ = writeln!(out, "{heading}");
    for link in links {
        let _ = writeln!(out, "- [[{link}]]");
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------
// Parse
// ---------------------------------------------------------------------------

/// Parse a capability markdown document into a [`Capability`]. Fail-open: a
/// malformed document yields a best-effort value (missing pieces → defaults),
/// never a panic. The inverse of [`render`].
///
/// Link sections (`## Covers` / `## Specs` / `## Related`) are parsed by
/// reusing [`scan_links`] over the section body, so a `- [[entity.X]]`
/// bullet contributes the inner token `entity.X` — there is no second
/// `[[ ]]` scanner.
#[must_use]
pub fn parse(md: &str) -> Capability {
    let mut cap = Capability::default();
    cap.id = frontmatter_field(md, "id");
    cap.status = frontmatter_field(md, "status");

    let body = strip_frontmatter(md);

    // Title — the first `# ` H1 (not `##`/`###`).
    for line in body.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("# ") {
            cap.title = rest.trim().to_string();
            break;
        }
        // Stop scanning for the title once a structural section opens.
        if t.starts_with(REQUIREMENT_PREFIX) || t.starts_with("## ") {
            break;
        }
    }

    cap.requirements = parse_requirements(body);
    cap.covers = parse_link_section(body, COVERS_HEADING);
    cap.specs = parse_link_section(body, SPECS_HEADING);
    cap.related = parse_link_section(body, RELATED_HEADING);
    cap
}

/// Fold the `### Requirement:` / `#### Scenario:` blocks into typed
/// requirements. A scenario before any requirement is ignored (no anchor);
/// `when` / `then` / `command` lines attach to the open scenario.
fn parse_requirements(body: &str) -> Vec<Requirement> {
    let mut reqs: Vec<Requirement> = Vec::new();
    for raw in body.lines() {
        let line = raw.trim_start();
        if let Some(rest) = line.strip_prefix(REQUIREMENT_PREFIX) {
            reqs.push(Requirement {
                statement: rest.trim().to_string(),
                scenarios: Vec::new(),
            });
        } else if let Some(rest) = line.strip_prefix(SCENARIO_PREFIX) {
            if let Some(req) = reqs.last_mut() {
                req.scenarios.push(Scenario {
                    name: rest.trim().to_string(),
                    ..Scenario::default()
                });
            }
        } else if let Some(rest) = line.strip_prefix(WHEN_PREFIX) {
            if let Some(sc) = current_scenario(&mut reqs) {
                sc.when = rest.trim().to_string();
            }
        } else if let Some(rest) = line.strip_prefix(THEN_PREFIX) {
            if let Some(sc) = current_scenario(&mut reqs) {
                sc.then = rest.trim().to_string();
            }
        } else if let Some(rest) = line.strip_prefix(COMMAND_PREFIX) {
            if let Some(sc) = current_scenario(&mut reqs) {
                let cmd = strip_inline_code(rest.trim());
                if !cmd.is_empty() {
                    sc.command = Some(cmd);
                }
            }
        }
    }
    reqs
}

/// The scenario currently being filled — the last scenario of the last
/// requirement, if any.
fn current_scenario(reqs: &mut [Requirement]) -> Option<&mut Scenario> {
    reqs.last_mut().and_then(|r| r.scenarios.last_mut())
}

/// Extract every `[[ ]]` link inside the `heading` section — from the heading
/// line until the next `## ` H2 (or end of body) — by reusing [`scan_links`].
fn parse_link_section(body: &str, heading: &str) -> Vec<String> {
    let mut collecting = false;
    let mut section = String::new();
    for raw in body.lines() {
        let line = raw.trim_end();
        if collecting {
            // A new H2 ends the section.
            if line.trim_start().starts_with("## ") {
                break;
            }
            section.push_str(line);
            section.push('\n');
        } else if line.trim_start() == heading {
            collecting = true;
        }
    }
    scan_links(&section)
}

// ---------------------------------------------------------------------------
// Spec-side `## Capabilities` scanner — the SINGLE source for "which
// capabilities does a spec link?". Reused by `complete-spec` (merge-on-close)
// and `qa-run` (run the linked capabilities' scenario ACs). No parallel scanner
// lives in either of those modules.
// ---------------------------------------------------------------------------

/// Slice the `## Capabilities` section of a spec's markdown (from its heading to
/// the next `## ` H2 or end of document) and return the `cap.*` wikilink ids
/// inside it. Pure + agnostic; reuses [`scan_links`] so there is no second
/// `[[ ]]` scanner, and a stray `entity.*` / `spec.*` bullet is ignored. A
/// document with no `## Capabilities` section yields an empty vec.
#[must_use]
pub(crate) fn scan_capabilities_section(md: &str) -> Vec<String> {
    let mut collecting = false;
    let mut section = String::new();
    for raw in md.lines() {
        let line = raw.trim_end();
        if collecting {
            if line.trim_start().starts_with("## ") {
                break; // next H2 ends the section.
            }
            section.push_str(line);
            section.push('\n');
        } else if line.trim_start() == CAPABILITIES_HEADING {
            collecting = true;
        }
    }
    scan_links(&section)
        .into_iter()
        .filter(|id| id.starts_with("cap."))
        .collect()
}

/// Read `<spec>/spec.md` and return the `cap.*` ids linked in its
/// `## Capabilities` section, via [`scan_capabilities_section`]. Returns an
/// empty vec when the spec / file / section is absent (fail-open) — every error
/// path degrades to "no links" rather than propagating.
#[must_use]
pub(crate) fn linked_capability_ids(project: &Path, spec: &str) -> Vec<String> {
    let Ok(spec_md) = ClaudePaths::for_project(project)
        .and_then(|p| p.for_spec(spec))
        .map(|sp| sp.spec_md_path())
    else {
        return Vec::new();
    };
    let Ok(md) = mfs::read_to_string(&spec_md) else {
        return Vec::new();
    };
    scan_capabilities_section(&md)
}

// ---------------------------------------------------------------------------
// Frontmatter helpers (tolerant, no panic)
// ---------------------------------------------------------------------------

/// Return everything after a leading `---\n…\n---` frontmatter block, or the
/// whole input when there is no well-formed block.
fn strip_frontmatter(md: &str) -> &str {
    let Some(rest) = md.strip_prefix("---\n") else {
        return md;
    };
    match rest.find("\n---") {
        Some(end) => {
            // Skip past the closing `---` line.
            let after = &rest[end..];
            after
                .strip_prefix("\n---")
                .map_or(md, |a| a.trim_start_matches(['\r', '\n']))
        }
        None => md,
    }
}

/// Read a single `key: value` line from the leading frontmatter block. Returns
/// an empty string when the block or the key is absent (fail-open).
fn frontmatter_field(md: &str, key: &str) -> String {
    let Some(rest) = md.strip_prefix("---\n") else {
        return String::new();
    };
    let Some(end) = rest.find("\n---") else {
        return String::new();
    };
    let needle = format!("{key}:");
    for line in rest[..end].lines() {
        if let Some(value) = line.trim().strip_prefix(&needle) {
            return value.trim().to_string();
        }
    }
    String::new()
}

/// Strip a symmetric pair of backticks from `s` (the inline-code wrapper the
/// renderer adds to a command). Leaves un-backticked input untouched.
fn strip_inline_code(s: &str) -> String {
    let trimmed = s.trim();
    trimmed
        .strip_prefix('`')
        .and_then(|t| t.strip_suffix('`'))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------
// JSON output
// ---------------------------------------------------------------------------

fn emit_error(reason: &str, detail: &str) {
    print_json(&json!({ "ok": false, "error": reason, "detail": detail }));
}

fn print_json(value: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into())
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample() -> Capability {
        Capability {
            id: "cap.living-spec".into(),
            title: "Living capability spec".into(),
            status: "active".into(),
            requirements: vec![
                Requirement {
                    statement: "The system SHALL persist capabilities durably.".into(),
                    scenarios: vec![
                        Scenario {
                            name: "round trip".into(),
                            when: "a capability is declared".into(),
                            then: "it survives a reload".into(),
                            command: Some("rtk cargo test -p mustard-core".into()),
                        },
                        Scenario {
                            name: "documentary".into(),
                            when: "no command is given".into(),
                            then: "it is doc-only".into(),
                            command: None,
                        },
                    ],
                },
                Requirement {
                    statement: "The system SHALL link covered entities.".into(),
                    scenarios: vec![],
                },
            ],
            covers: vec!["rt.entity.CapabilityStore".into(), "rt.entity.Other".into()],
            specs: vec!["spec.living-capability-spec".into()],
            related: vec!["cap.knowledge".into()],
        }
    }

    #[test]
    fn render_emits_machine_contract_tokens() {
        let md = render(&sample());
        assert!(md.starts_with("---\nid: cap.living-spec\nstatus: active\n---\n"));
        assert!(md.contains("# Living capability spec"));
        assert!(md.contains("### Requirement: The system SHALL persist capabilities durably."));
        assert!(md.contains("#### Scenario: round trip"));
        assert!(md.contains("- when: a capability is declared"));
        assert!(md.contains("- then: it survives a reload"));
        assert!(md.contains("- command: `rtk cargo test -p mustard-core`"));
        // Documentary scenario has no command line.
        assert!(md.contains("#### Scenario: documentary"));
        // Link sections present with their bullets.
        assert!(md.contains("## Covers\n- [[rt.entity.CapabilityStore]]\n- [[rt.entity.Other]]"));
        assert!(md.contains("## Specs\n- [[spec.living-capability-spec]]"));
        assert!(md.contains("## Related\n- [[cap.knowledge]]"));
    }

    #[test]
    fn parse_render_round_trips() {
        let cap = sample();
        let back = parse(&render(&cap));
        assert_eq!(back, cap, "parse(render(c)) must equal c");
    }

    #[test]
    fn parse_links_reuse_scan_links() {
        let md = render(&sample());
        let cap = parse(&md);
        // Inner tokens of `- [[..]]` bullets, in source order.
        assert_eq!(cap.covers, vec!["rt.entity.CapabilityStore", "rt.entity.Other"]);
        assert_eq!(cap.specs, vec!["spec.living-capability-spec"]);
        assert_eq!(cap.related, vec!["cap.knowledge"]);
    }

    #[test]
    fn parse_is_fail_open_on_garbage() {
        // No frontmatter, stray scenario before any requirement, half a field.
        let md = "# Title only\n\n#### Scenario: orphan\n- when: x\n\nrandom text\n";
        let cap = parse(md);
        assert_eq!(cap.title, "Title only");
        assert!(cap.id.is_empty());
        assert!(cap.status.is_empty());
        // A scenario with no owning requirement is dropped (no anchor).
        assert!(cap.requirements.is_empty());
        assert!(cap.covers.is_empty());
    }

    #[test]
    fn parse_empty_string_is_default() {
        assert_eq!(parse(""), Capability::default());
    }

    #[test]
    fn create_writes_doc_and_show_round_trips() {
        let dir = tempdir().unwrap();
        create_with_root(
            CapabilityCreateOpts {
                slug: "durable-store".into(),
                title: "Durable capability store".into(),
                status: String::new(), // defaults to active
            },
            dir.path(),
        );
        let target = dir
            .path()
            .join(".claude")
            .join("capabilities")
            .join("durable-store.md");
        assert!(target.exists(), "create must write the doc");
        let parsed = parse(&std::fs::read_to_string(&target).unwrap());
        assert_eq!(parsed.id, "cap.durable-store");
        assert_eq!(parsed.title, "Durable capability store");
        assert_eq!(parsed.status, "active", "empty --status defaults to active");
    }

    #[test]
    fn create_refuses_existing_doc() {
        let dir = tempdir().unwrap();
        let caps = dir.path().join(".claude").join("capabilities");
        std::fs::create_dir_all(&caps).unwrap();
        std::fs::write(caps.join("x.md"), "---\nid: cap.x\n---\n# X\n").unwrap();
        // Second create must not clobber — the file is unchanged.
        let before = std::fs::read_to_string(caps.join("x.md")).unwrap();
        create_with_root(
            CapabilityCreateOpts {
                slug: "x".into(),
                title: "Clobber attempt".into(),
                status: "active".into(),
            },
            dir.path(),
        );
        let after = std::fs::read_to_string(caps.join("x.md")).unwrap();
        assert_eq!(before, after, "existing capability must not be overwritten");
    }

    #[test]
    fn show_byte_stable_and_deterministic() {
        let dir = tempdir().unwrap();
        let caps = dir.path().join(".claude").join("capabilities");
        std::fs::create_dir_all(&caps).unwrap();
        std::fs::write(caps.join("d.md"), render(&sample())).unwrap();
        // `show` reads + parses + emits compact JSON; emitting twice from the
        // same parse is identical (field order fixed, no timestamps).
        let cap = parse(&std::fs::read_to_string(caps.join("d.md")).unwrap());
        let a = serde_json::to_string(&cap).unwrap();
        let b = serde_json::to_string(&cap).unwrap();
        assert_eq!(a, b);
        // Round-trips back through serde to the same Capability.
        let from_json: Capability = serde_json::from_str(&a).unwrap();
        assert_eq!(from_json, sample());
    }

    // --- sync-nodes -------------------------------------------------------

    use mustard_core::io::atomic_md::wikilink;

    /// A capability covering a KNOWN entity materializes
    /// `.claude/graph/entity.{name}.md` whose `id:` resolves via the EXISTING
    /// resolver; an UNKNOWN entity is skipped (no node ⇒ stays unresolvable).
    #[test]
    fn sync_nodes_materializes_known_entity_and_skips_unknown() {
        let dir = tempdir().unwrap();
        let graph = dir.path().join(".claude").join("graph");

        // Covers: one known entity, one typo'd (unknown) entity. Agnostic — the
        // names are bare declaration names, no `rt.`/language assumption.
        let cap = Capability {
            id: "cap.invoicing".into(),
            covers: vec![
                "entity.InvoiceService".into(),
                "entity.Typooo".into(), // not in the registry ⇒ skipped
            ],
            ..Capability::default()
        };
        let known: BTreeSet<String> =
            ["InvoiceService"].into_iter().map(str::to_string).collect();

        let outcome = materialize_nodes(&cap, &known, &graph, &|_| None).expect("materialize ok");
        assert_eq!(outcome.written, vec!["entity.InvoiceService"], "known ⇒ written");
        assert_eq!(outcome.skipped, vec!["entity.Typooo"], "unknown ⇒ skipped, no invented node");

        // The node file exists and carries the load-bearing frontmatter id.
        let node = graph.join("entity.InvoiceService.md");
        assert!(node.exists(), "node file written");
        let body = std::fs::read_to_string(&node).unwrap();
        assert!(body.starts_with("---\nid: entity.InvoiceService\n---\n"), "frontmatter id:\n{body}");
        assert!(body.contains("# InvoiceService"), "H1 carries the entity name:\n{body}");

        // The EXISTING resolver dereferences `[[entity.InvoiceService]]` to the
        // node via its frontmatter `id:` — the whole point of the verb.
        let resolved = wikilink::resolve("entity.InvoiceService", &[graph.as_path()]);
        assert_eq!(resolved.as_deref(), Some(node.as_path()), "id: resolves in mustard");

        // The skipped (typo) cover has NO node ⇒ it does not resolve ⇒ surfaces
        // as `⚠ unresolved` to a human.
        assert!(
            wikilink::resolve("entity.Typooo", &[graph.as_path()]).is_none(),
            "unknown cover stays unresolved"
        );
    }

    /// Re-running over the same inputs produces byte-identical node files
    /// (idempotent + stable), and a source line (when the slice names one) is
    /// rendered deterministically.
    #[test]
    fn sync_nodes_is_idempotent_and_byte_stable() {
        let dir = tempdir().unwrap();
        let graph = dir.path().join(".claude").join("graph");
        let cap = Capability {
            id: "cap.x".into(),
            covers: vec!["entity.Order".into()],
            ..Capability::default()
        };
        let known: BTreeSet<String> = ["Order"].into_iter().map(str::to_string).collect();
        let src = |name: &str| (name == "Order").then(|| "apps/api/src/order.rs".to_string());

        materialize_nodes(&cap, &known, &graph, &src).expect("first run");
        let first = std::fs::read_to_string(graph.join("entity.Order.md")).unwrap();
        materialize_nodes(&cap, &known, &graph, &src).expect("second run");
        let second = std::fs::read_to_string(graph.join("entity.Order.md")).unwrap();
        assert_eq!(first, second, "re-running is byte-identical");
        assert!(first.contains("- Source: `apps/api/src/order.rs`"), "source line rendered:\n{first}");
    }

    /// `sync_nodes_with_root` fails open: a missing capability doc emits a JSON
    /// error (no panic), and with no grain model present every `entity.*` cover
    /// misses the (empty) registry slice ⇒ nothing is written.
    #[test]
    fn sync_nodes_with_root_fail_open_and_empty_registry_skips_all() {
        let dir = tempdir().unwrap();
        // No capability doc yet ⇒ JSON error, no panic.
        sync_nodes_with_root("absent", dir.path());

        // Doc exists but there is no grain.model.json ⇒ read_entity_names is
        // empty ⇒ the cover misses ⇒ no node materialized.
        let caps = dir.path().join(".claude").join("capabilities");
        std::fs::create_dir_all(&caps).unwrap();
        let cap = Capability {
            id: "cap.demo".into(),
            covers: vec!["entity.Whatever".into()],
            ..Capability::default()
        };
        std::fs::write(caps.join("demo.md"), render(&cap)).unwrap();
        sync_nodes_with_root("demo", dir.path());
        let node = dir.path().join(".claude").join("graph").join("entity.Whatever.md");
        assert!(!node.exists(), "empty registry ⇒ cover skipped, no node");
    }

    /// `entity_name_of` is agnostic: it strips ONLY the `entity.` prefix (no
    /// `rt.`/language assumption), keeps the rest verbatim, and rejects a
    /// non-`entity.*` id or an empty name.
    #[test]
    fn entity_name_of_is_agnostic() {
        assert_eq!(entity_name_of("entity.InvoiceService").as_deref(), Some("InvoiceService"));
        assert_eq!(entity_name_of("entity.snake_case_thing").as_deref(), Some("snake_case_thing"));
        // A namespaced name survives whole — we do not assume a single segment.
        assert_eq!(entity_name_of("entity.App.Module.Resolver").as_deref(), Some("App.Module.Resolver"));
        // Not an entity cover ⇒ None (left for its own node kind).
        assert!(entity_name_of("spec.foo").is_none());
        assert!(entity_name_of("cap.bar").is_none());
        // Empty / whitespace name ⇒ None.
        assert!(entity_name_of("entity.").is_none());
        assert!(entity_name_of("entity.   ").is_none());
    }

    // --- shared `## Capabilities` scanner --------------------------------

    /// The section parser is precise: it keeps only `cap.*` ids inside
    /// `## Capabilities`, stops at the next H2, and ignores other link kinds.
    /// (Moved here from `complete_spec` so the scanner has ONE home + ONE test.)
    #[test]
    fn scan_capabilities_section_keeps_only_cap_ids_in_section() {
        let md = "\
# Title

## Capabilities
- [[cap.one]]
- [[entity.NotACap]]
- [[cap.two]]

## Other
- [[cap.should-not-count]]
";
        let ids = scan_capabilities_section(md);
        assert_eq!(ids, vec!["cap.one".to_string(), "cap.two".to_string()]);
        // No section at all ⇒ empty.
        assert!(scan_capabilities_section("# Only narrative\n").is_empty());
    }

    /// `linked_capability_ids` reads `<spec>/spec.md` and returns the section's
    /// `cap.*` ids; a missing spec / file is fail-open (empty), not a panic.
    #[test]
    fn linked_capability_ids_reads_spec_md_and_fail_opens() {
        let dir = tempdir().unwrap();
        // No spec dir yet ⇒ empty (fail-open).
        assert!(linked_capability_ids(dir.path(), "ghost").is_empty());

        let spec_dir = dir.path().join(".claude").join("spec").join("feature-x");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Feature X\n\nNarrative.\n\n## Capabilities\n- [[cap.alpha]]\n- [[cap.beta]]\n",
        )
        .unwrap();
        assert_eq!(
            linked_capability_ids(dir.path(), "feature-x"),
            vec!["cap.alpha".to_string(), "cap.beta".to_string()]
        );
    }

    /// The node renders without a source line when the registry slice names
    /// none — the link still resolves (the `id:` is what matters).
    #[test]
    fn render_entity_node_omits_absent_source() {
        let body = render_entity_node("entity.Order", "Order", None);
        assert!(body.starts_with("---\nid: entity.Order\n---\n"));
        assert!(body.contains("# Order"));
        assert!(!body.contains("- Source:"), "no source line when none known:\n{body}");
    }
}
