//! Deterministic SPEC compiler (no AI, no hardcoded domain or language).
//!
//! Turns a structured need — an entity to create, optionally mirroring an
//! existing one, plus the operations wanted — into a complete implementation
//! SPEC: the contract a subagent receives (and the checklist a reviewer uses).
//! Everything is templating over the mined `ProjectModel`: it never calls a
//! model, and it carries no language, framework, role, folder, or contract — all
//! of those come from the repo via mining. The spec is broken down BY PROJECT
//! (the compilation units `scan` already discovers), so each section is a unit a
//! subagent can own — its own directory and manifest kind — rather than a fixed
//! backend/frontend split.
//!
//! Output is ASCII-only so it prints on any console; pass `--out` to write a file.

use crate::model::{Convention, Exemplar, ProjectModel, ProjectUnit, RoleStat};
use regex::Regex;
use std::collections::{HashMap, HashSet};

pub fn compile(model: &ProjectModel, entity: &str, like: &str, ops: &[String], invariants: &[String]) -> String {
    let (conv, like_matched) = match pick_slice(model, like) {
        Some(p) => p,
        None => return "# (no vertical slice in the model -- nothing to compile)\n".to_string(),
    };
    let module_paths: HashSet<&str> = model.modules.iter().map(|m| m.path.as_str()).collect();
    let roles_meta: HashMap<&str, &RoleStat> = model.roles.iter().map(|r| (r.affix.as_str(), r)).collect();

    // Novelty: does the entity have ANY precedent in the repo? Matched on WHOLE
    // tokens (not raw substring), so a short name like "Tax" does not falsely
    // match "Taxonomy". With no sibling (`--like`) and no precedent, this is
    // net-new — the spec must say so loudly rather than let "gerada
    // deterministicamente" lend false confidence to an unmined shape.
    let etok = crate::digest::tokenize(entity);
    let has_precedent = !etok.is_empty()
        && model.modules.iter().any(|m| {
            token_seq_in(&crate::digest::tokenize(&m.path), &etok)
                || m.declarations.iter().any(|d| token_seq_in(&crate::digest::tokenize(&d.name), &etok))
        });
    // A `--like` that matched nothing in the model counts as not passed at all:
    // the fallback pattern is unverified, so novelty is judged as if `--like`
    // were absent.
    let novel = !like_matched && !has_precedent;

    // Operations select extra (optional) roles by name prefix. The base vertical
    // (core roles) is ALWAYS included, so no operation keyword is special-cased.
    let wanted_ops: Vec<String> = ops.iter().map(|o| o.trim().to_lowercase()).filter(|o| !o.is_empty()).collect();

    let mut lines: Vec<SpecLine> = Vec::new();
    for step in &conv.steps {
        let p = parse_step(step);
        let role_l = p.role.to_lowercase();
        let selected = !p.optional || wanted_ops.iter().any(|op| role_l.starts_with(op));
        if !selected {
            continue;
        }
        // Folder fallback for roles the miner couldn't place (e.g. Module).
        let folder = if p.folder.is_empty() {
            roles_meta.get(p.role.as_str()).map(|r| r.common_dir.clone()).filter(|d| !d.is_empty()).unwrap_or_default()
        } else {
            p.folder.clone()
        };
        let target = if folder.is_empty() {
            "(define location)".to_string()
        } else {
            subst(&format!("{folder}{}", p.example), entity)
        };
        let (mirror, real) = mirror_file(&folder, &p.example, &p.model, like, &conv.exemplar, &module_paths);
        let collabs = roles_meta.get(p.role.as_str()).map(|r| r.collaborators.clone()).unwrap_or_default();
        // Section + label come from the model's own project units (manifest kind),
        // by longest-prefix match — not a hardcoded backend/frontend rule.
        let probe = if real { mirror.as_str() } else { folder.as_str() };
        let proj = project_of(probe, &model.projects);
        let (kind, project, dir) = match proj {
            Some(p) => (p.kind.clone(), p.name.clone(), p.dir.clone()),
            None => ("outros".to_string(), String::new(), String::new()),
        };
        merge_into(&mut lines, SpecLine { roles: vec![p.role], target, mirror, real, collaborators: collabs, optional: p.optional, kind, project, dir, folder });
    }

    render(model, conv, entity, like, like_matched, &lines, &wanted_ops, &roles_meta, invariants, novel)
}

/// One acceptance criterion: the role(s) and the target folder (entity already
/// substituted). Same selection + target-dedup as `compile`, so `grain verify`
/// checks exactly what the spec promised.
pub(crate) struct Accept {
    pub roles: String,
    pub folder: String,
    pub optional: bool,
}

pub fn acceptance(model: &ProjectModel, entity: &str, like: &str, ops: &[String]) -> Vec<Accept> {
    let conv = match pick_slice(model, like) {
        Some((c, _)) => c,
        None => return Vec::new(),
    };
    let roles_meta: HashMap<&str, &RoleStat> = model.roles.iter().map(|r| (r.affix.as_str(), r)).collect();
    let wanted_ops: Vec<String> = ops.iter().map(|o| o.trim().to_lowercase()).filter(|o| !o.is_empty()).collect();
    let mut out: Vec<(String, Accept)> = Vec::new(); // (target key, item)
    for step in &conv.steps {
        let p = parse_step(step);
        let selected = !p.optional || wanted_ops.iter().any(|op| p.role.to_lowercase().starts_with(op));
        if !selected {
            continue;
        }
        let folder = if p.folder.is_empty() {
            roles_meta.get(p.role.as_str()).map(|r| r.common_dir.clone()).filter(|d| !d.is_empty()).unwrap_or_default()
        } else {
            p.folder.clone()
        };
        if folder.is_empty() {
            continue;
        }
        let target = subst(&format!("{folder}{}", p.example), entity);
        let folder_sub = subst(&folder, entity);
        if let Some((_, a)) = out.iter_mut().find(|(t, _)| *t == target) {
            if !a.roles.split('+').any(|r| r == p.role) {
                a.roles = format!("{}+{}", a.roles, p.role);
            }
            a.optional = a.optional && p.optional;
        } else {
            out.push((target, Accept { roles: p.role.clone(), folder: folder_sub, optional: p.optional }));
        }
    }
    out.into_iter().map(|(_, a)| a).collect()
}

struct SpecLine {
    roles: Vec<String>,
    target: String,
    mirror: String,
    real: bool,
    collaborators: Vec<String>,
    optional: bool,
    kind: String,
    project: String,
    dir: String,
    folder: String,
}

/// Merge a new line into an existing one when they resolve to the same target
/// file — collapses the many response-shape roles into a single deliverable.
fn merge_into(lines: &mut Vec<SpecLine>, new: SpecLine) {
    if let Some(e) = lines.iter_mut().find(|l| l.target == new.target && new.target != "(define location)") {
        for r in new.roles {
            if !e.roles.contains(&r) {
                e.roles.push(r);
            }
        }
        for c in new.collaborators {
            if !e.collaborators.contains(&c) {
                e.collaborators.push(c);
            }
        }
        if e.mirror.is_empty() || (!e.real && new.real) {
            e.mirror = new.mirror;
            e.real = new.real;
        }
        e.optional = e.optional && new.optional;
    } else {
        lines.push(new);
    }
}

#[allow(clippy::too_many_arguments)]
fn render(
    model: &ProjectModel,
    conv: &Convention,
    entity: &str,
    like: &str,
    like_matched: bool,
    lines: &[SpecLine],
    ops: &[String],
    roles_meta: &HashMap<&str, &RoleStat>,
    invariants: &[String],
    novel: bool,
) -> String {
    let mut o = String::new();
    let core: Vec<&str> = conv.roles.iter().map(|s| s.as_str()).filter(|r| *r != "(core)").collect();
    // A convention's short label (its core role affixes) and a couple of sample
    // entities — used to surface the chosen pattern AND the alternatives, so the
    // reader sees which shape was picked and can switch WITHOUT knowing CRUD/CQRS.
    let slice_label = |c: &Convention| c.roles.iter().map(|s| s.as_str()).filter(|r| *r != "(core)").collect::<Vec<_>>().join("+");
    let ex_inline = |c: &Convention| -> String {
        let ex: Vec<&str> = c.entities.iter().take(2).map(|s| s.as_str()).collect();
        if ex.is_empty() { String::new() } else { format!(", ex.: {}", ex.join(", ")) }
    };
    let richness = |c: &Convention| (c.roles.len() + c.optional_roles.len(), c.recurrence);
    let mut others: Vec<&Convention> = model.conventions.iter().filter(|c| c.is_slice && !std::ptr::eq(*c, conv)).collect();
    others.sort_by(|a, b| richness(b).cmp(&richness(a)).then(a.name.cmp(&b.name)));

    let title = if like.is_empty() {
        format!("# SPEC -- create **{entity}**")
    } else {
        format!("# SPEC -- create **{entity}** (a kind of **{like}**)")
    };
    o.push_str(&format!("{title}  _(generated deterministically, no AI)_\n\n"));
    // Pattern chosen + alternatives, ALWAYS in the file — this repo may build in
    // more than one shape, so the choice is made transparent and switchable here
    // (you point at a sibling via `--like`; you never declare "CRUD"/"CQRS").
    o.push_str(&format!(
        "> **Pattern chosen (automatic):** `{}` -- recurs across **{}** entities (conf {:.2}{}).\n",
        core.join("+"),
        conv.recurrence,
        conv.confidence,
        ex_inline(conv)
    ));
    if !others.is_empty() {
        let alts: Vec<String> = others.iter().take(3).map(|c| format!("`{}` ({}x{})", slice_label(c), c.recurrence, ex_inline(c))).collect();
        o.push_str(&format!("> Other patterns in this repo: {}.\n", alts.join(" - ")));
        o.push_str("> If your case is a different pattern, re-run with `--like <an example of it>` (point at an existing sibling; scan infers the pattern from where it lives).\n");
    }
    if !like.is_empty() {
        if like_matched {
            o.push_str(&format!("> Mirrored on the REAL files of **{like}** (verified in the model). `{entity}` follows the same shape.\n"));
        } else {
            // The sibling was NOT found among the mined slice entities: the
            // pattern below is the best fallback, never a verified mirror.
            o.push_str(&format!("> **like \"{like}\" not found in the model -- fallback pattern below, treat as UNVERIFIED.**\n"));
        }
    }
    o.push_str(&format!("> Operations: {}.\n", ops.join(", ")));
    o.push_str("> **ATTENTION — HYPOTHETICAL plan, NOT verified against the code.** Read the anchors and resolve the fork BEFORE creating any file. The per-project sections are hypotheses until then.\n");
    if novel {
        o.push_str("> **NO PRECEDENT — this unit was NOT mined in the repo (no `--like` sibling, no file with this name).** Treat it as DESIGN, not recomposition: the mold below is only a starting point from the dominant pattern; each file must be designed against the anchors, not cloned.\n");
    }
    o.push('\n');

    // Foundation: the contracts each selected role implements (mined supertypes).
    o.push_str("## Foundation (contracts to extend/implement)\n");
    let mut seen = HashSet::new();
    for l in lines {
        for r in &l.roles {
            if let Some(rm) = roles_meta.get(r.as_str()) {
                if let Some(impl_) = &rm.implements {
                    if seen.insert(impl_.clone()) {
                        o.push_str(&format!("- **{r}** implements `{impl_}`\n"));
                    }
                }
            }
        }
    }
    for sc in model.shared_contracts.iter().take(4) {
        if seen.insert(sc.name.clone()) {
            o.push_str(&format!("- shared base `{}` ({} implementors)\n", sc.name, sc.implementors));
        }
    }
    o.push('\n');

    // === Invariantes transversais (obedecer em TODA unidade) =================
    // Cross-cutting contracts the decomposition/feature step flagged (e.g. an
    // injected current-tenant accessor). These are NOT mined supertypes, so they
    // never show up in `shared_contracts`; we locate the real defining + consumer
    // files by graph fan-in + name and anchor on them, so the AI MIRRORS the
    // wiring instead of inventing it. The mechanism itself stays a design choice.
    let mut inv_anchors: Vec<String> = Vec::new();
    if !invariants.is_empty() {
        o.push_str("## Cross-cutting invariants to obey (do NOT invent the mechanism)\n");
        for inv in invariants {
            let files = invariant_anchors(model, inv);
            o.push_str(&format!("- **{inv}** — every unit of this spec must respect it; mirror the real files below (do not assume a base class/attribute).\n"));
            for f in files.iter().take(4) {
                o.push_str(&format!("    - [{}]({})\n", file_name(f), f));
            }
            inv_anchors.extend(files.into_iter().take(4));
        }
        o.push('\n');
    }

    // === Bifurcação (só quando há um irmão `--like` a confrontar) ============
    // The miner cannot tell "new entity" from "variant/type of an existing one"
    // (a row in a *Type table / N:N). When a sibling is given, surface the fork.
    let mut signals: Vec<&str> = Vec::new();
    if !like.is_empty() {
        let lk = like.to_lowercase();
        signals = model
            .modules
            .iter()
            .map(|m| m.path.as_str())
            .filter(|p| {
                let pl = p.to_lowercase();
                pl.contains(&format!("{lk}type")) || pl.contains(&format!("{lk}assignment")) || pl.contains(&format!("{lk}kind"))
            })
            .collect();
        signals.sort();
        signals.dedup();

        o.push_str("## Fork to resolve (READ first)\n");
        if signals.is_empty() {
            o.push_str(&format!("> Is `{entity}` a NEW entity, or a VARIANT/type of `{like}`? The plan below assumes a new entity — confirm by reading the anchors before creating files.\n\n"));
        } else {
            o.push_str(&format!("> Signs that `{entity}` may be a **variant/type** of `{like}` (a type table / N:N exists), NOT a new entity:\n"));
            for s in signals.iter().take(4) {
                o.push_str(&format!("- `{s}`\n"));
            }
            o.push_str(&format!("> If so, **do NOT clone the vertical**: `{entity}` becomes a row/type of `{like}`. Confirm by reading the anchors.\n\n"));
        }
    }

    // === Âncoras a ler (SEMPRE — a IA le SO estes, nunca o repo) =============
    // Mandatory regardless of `--like`: even a net-new unit gets real files to
    // read. Two tiers so the cap never drops what the user explicitly asked to
    // confront: MUST (the invariant files + variant signals — listed in full,
    // never truncated) then DISCRETIONARY (per-role mirrors + slice exemplars,
    // filling the remaining budget). The total stays ~a dozen, but the
    // invariant anchors can no longer be sorted out of the "read SO these" list.
    let mut must: Vec<String> = inv_anchors;
    must.extend(signals.iter().take(4).map(|s| s.to_string()));
    must.sort();
    must.dedup();

    let mut more: Vec<String> = lines.iter().filter(|l| l.real).map(|l| l.mirror.clone()).collect();
    for ex in &conv.exemplars {
        for f in ex.files.iter().take(2) {
            more.push(f.clone());
        }
    }
    more.sort();
    more.dedup();
    more.retain(|p| !must.contains(p));

    let budget = 12usize.saturating_sub(must.len());
    let shown: Vec<&String> = must.iter().chain(more.iter().take(budget)).collect();
    if !shown.is_empty() {
        let src = if like.is_empty() { "real exemplars of the pattern + invariants".to_string() } else { format!("`{like}` + exemplars + invariants") };
        o.push_str(&format!("## Read before planning — anchors ({src}); the AI reads ONLY these\n"));
        for p in shown {
            o.push_str(&format!("- [{}]({})\n", file_name(p), p));
        }
        o.push('\n');
    }

    // One section per project (the unit a subagent can own), in order of
    // appearance — which follows the dependency order of the recipe steps.
    let key = |l: &SpecLine| if l.project.is_empty() { "(no project)".to_string() } else { l.project.clone() };
    let mut order: Vec<String> = Vec::new();
    for l in lines {
        let k = key(l);
        if !order.contains(&k) {
            order.push(k);
        }
    }
    o.push_str("> Per-project sections (HYPOTHESIS — confirm after reading the anchors). Each is a **project** (`scan`) a subagent can own, with its directory.\n\n");
    for k in &order {
        render_block(&mut o, k, lines, roles_meta, &conv.exemplars);
    }

    // Registration hubs to EDIT (from the graph) + tooling/scripts — inlined so
    // the spec is self-contained. Scoped to the projects this spec touches.
    let mut spec_dirs: Vec<&str> = lines.iter().map(|l| l.dir.as_str()).filter(|d| !d.is_empty()).collect();
    spec_dirs.sort();
    spec_dirs.dedup();
    let mut tp_shown = false;
    for d in &spec_dirs {
        let here: Vec<_> = model.graph.touchpoints.iter().filter(|t| t.module.starts_with(&format!("{d}/"))).take(2).collect();
        if here.is_empty() {
            continue;
        }
        if !tp_shown {
            o.push_str("## Registration points to edit (hubs — confirm)\n");
            o.push_str("> When adding the entity, you usually **edit** (not create) one of these: DI, menu, barrel.\n");
            tp_shown = true;
        }
        for t in here {
            o.push_str(&format!("- [{}]({}) ({} modules, {} dirs)\n", file_name(&t.module), t.module, t.fan_out, t.breadth));
        }
    }
    if tp_shown {
        o.push('\n');
    }
    let mut scripts: Vec<&String> = Vec::new();
    for mf in &model.manifests {
        let mdir = mf.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let rel = mdir.is_empty() || spec_dirs.iter().any(|d| *d == mdir || mdir.starts_with(&format!("{d}/")) || d.starts_with(&format!("{mdir}/")));
        if rel {
            for s in &mf.scripts {
                if !scripts.contains(&s) {
                    scripts.push(s);
                }
            }
        }
    }
    if !scripts.is_empty() {
        o.push_str("## Tooling / scripts of the projects (verbatim)\n");
        o.push_str("> A codegen step (generate the client from the backend) shows up here; the ORDER is decided at lapidation.\n");
        for s in scripts.iter().take(15) {
            o.push_str(&format!("- `{s}`\n"));
        }
        o.push('\n');
    }

    // Acceptance checklist — the spec is also the gate.
    o.push_str("## Acceptance criteria (slice complete when)\n");
    for l in lines {
        let tag = if l.optional { " _(optional)_" } else { "" };
        let loc = if l.folder.is_empty() { "(define location)".to_string() } else { subst(&l.folder, entity) };
        o.push_str(&format!("- [ ] **{}**{tag} for {entity} in `{loc}`\n", l.roles.join("+")));
    }
    if !invariants.is_empty() {
        o.push_str(&format!(
            "\n> **`scan verify` checks ONLY file presence, NOT the invariants ({}).** A slice can be 100% and still ignore the invariant — needs review (human/AI) reading the invariant anchors above.\n",
            invariants.join(", ")
        ));
    }
    o.push('\n');
    o.push_str("> This spec is both the subagent's input AND the reviewer's checklist: the same document generates and verifies. ");
    o.push_str("The AI only fills the substance (fields, validations, rules) within this mold.\n");
    o
}

/// True if the `needle` token sequence appears contiguously inside `hay` — a
/// whole-token (not substring) containment, so "AccountReceivable" matches a
/// decl tokenized as [...,account,receivable,...] but "Tax" does not match
/// [taxonomy].
fn token_seq_in(hay: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && needle.len() <= hay.len() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Locate the real files for a cross-cutting invariant (e.g. an injected
/// contract): the module that DEFINES it, the highest fan-in hubs whose path
/// carries its token, and consumer modules whose declarations carry the token.
/// Ranked by graph degree so the most central wiring comes first. Deterministic;
/// the token is derived from the argument, never from a hardcoded vocabulary.
fn invariant_anchors(model: &ProjectModel, inv: &str) -> Vec<String> {
    use std::collections::BTreeMap;
    let invl = inv.to_lowercase();
    // Core token: drop a leading single uppercase prefix letter ONLY for the
    // "I"+PascalWord interface / "T"+PascalWord type-param convention — i.e. when
    // the 2nd char is upper AND the 3rd is lower ("ICurrentTenant" -> "currenttenant").
    // Do NOT strip on an all-caps acronym like "HTTPClient" (3rd char upper).
    let b = inv.as_bytes();
    let core = if b.len() > 2 && b[0].is_ascii_uppercase() && b[1].is_ascii_uppercase() && b[2].is_ascii_lowercase() {
        inv[1..].to_lowercase()
    } else {
        invl.clone()
    };

    fn bump(w: &mut BTreeMap<String, usize>, path: &str, by: usize) {
        let e = w.entry(path.to_string()).or_insert(0);
        if by > *e {
            *e = by;
        }
    }
    let mut weight: BTreeMap<String, usize> = BTreeMap::new();
    // 1) defining module (a declaration whose name IS the invariant) — ranks first.
    for m in &model.modules {
        if m.declarations.iter().any(|d| d.name.to_lowercase() == invl) {
            bump(&mut weight, &m.path, usize::MAX);
        }
    }
    // 2) high fan-in hubs carrying the token — ranked by their degree.
    for nd in &model.graph.top_fan_in {
        let pl = nd.module.to_lowercase();
        if pl.contains(&invl) || pl.contains(&core) {
            bump(&mut weight, &nd.module, nd.degree);
        }
    }
    // 3) consumers: modules whose declaration names carry the core token.
    for m in &model.modules {
        if m.declarations.iter().any(|d| d.name.to_lowercase().contains(&core)) {
            bump(&mut weight, &m.path, 1);
        }
    }
    let mut ranked: Vec<(String, usize)> = weight.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.into_iter().map(|(p, _)| p).collect()
}

fn render_block(o: &mut String, project_key: &str, lines: &[SpecLine], roles_meta: &HashMap<&str, &RoleStat>, exemplars: &[Exemplar]) {
    let key = |l: &SpecLine| if l.project.is_empty() { "(no project)".to_string() } else { l.project.clone() };
    let block: Vec<&SpecLine> = lines.iter().filter(|l| key(l) == project_key).collect();
    if block.is_empty() {
        return;
    }
    let first = block[0];
    let head = if first.project.is_empty() {
        "(no project)".to_string()
    } else {
        format!("{} ({}, `{}/`)", first.project, first.kind, first.dir)
    };
    o.push_str(&format!("## Project: {head}\n"));
    // Self-contained: target + contract + mirror + 3 examples + collaborators.
    for (i, l) in block.iter().enumerate() {
        let opt = if l.optional { "  _(optional)_" } else { "" };
        o.push_str(&format!("{}. **{}** -> `{}`{opt}\n", i + 1, l.roles.join("+"), l.target));
        if let Some(c) = l.roles.iter().find_map(|r| roles_meta.get(r.as_str()).and_then(|m| m.implements.clone())) {
            o.push_str(&format!("     implements: `{c}`\n"));
        }
        if !l.mirror.is_empty() {
            let how = if l.real { "mirror" } else { "model" };
            o.push_str(&format!("     {how}: [{}]({})\n", file_name(&l.mirror), l.mirror));
        }
        if !l.folder.is_empty() {
            let mut exs: Vec<String> = Vec::new();
            for ex in exemplars {
                let concrete = subst(&l.folder, &ex.entity);
                if let Some(f) = ex.files.iter().filter(|p| p.starts_with(&concrete)).min() {
                    exs.push(format!("{} [{}]({})", level_label(&ex.level), file_name(f), f));
                }
            }
            if !exs.is_empty() {
                o.push_str(&format!("     examples: {}\n", exs.join(" · ")));
            }
        }
        if !l.collaborators.is_empty() {
            let c: Vec<&str> = l.collaborators.iter().take(4).map(|s| s.as_str()).collect();
            o.push_str(&format!("     collaborates with: {}\n", c.join(", ")));
        }
    }
    o.push('\n');
}

// --- selection -------------------------------------------------------------

/// Minimum role count (core + optional) for a slice to rank as a real vertical
/// in `pick_slice`. Below it sits the degenerate class: 2-role pairs (the
/// miner's own floor), typically a wrapper family minted once per OPERATION
/// rather than once per entity, whose recurrence therefore dwarfs every true
/// vertical's without describing how an entity is actually built.
const MIN_VERTICAL_ROLES: usize = 3;

/// Pick the slice convention to compile against. Returns the convention plus
/// whether the `--like` filter actually matched it — `false` means the caller
/// fell back to the best overall slice and must NOT claim a verified mirror.
fn pick_slice<'a>(model: &'a ProjectModel, like: &str) -> Option<(&'a Convention, bool)> {
    // CLASS precedence first: a real vertical (>= MIN_VERTICAL_ROLES roles)
    // always outranks a degenerate 2-role pair, because recurrence cannot be
    // compared across classes — a per-operation wrapper pair out-recurs every
    // per-entity vertical by an order of magnitude. The pair stays a legitimate
    // fallback when it is the only slice shape the repo has.
    // WITHIN a class, recurrence and confidence outrank role count: a
    // convention proven across many entities (e.g. 11x at conf 0.94) beats a
    // wide pseudo-convention of a couple of wrapper families (7 roles at conf
    // 0.82). Name is the final tie-break so the choice is deterministic even
    // when two slices are equally rich (the model's Vec order isn't guaranteed
    // stable).
    let role_count = |c: &Convention| c.roles.len() + c.optional_roles.len();
    let richer = |a: &&Convention, b: &&Convention| {
        (role_count(a) >= MIN_VERTICAL_ROLES)
            .cmp(&(role_count(b) >= MIN_VERTICAL_ROLES))
            .then(a.recurrence.cmp(&b.recurrence))
            .then(a.confidence.total_cmp(&b.confidence))
            .then(role_count(a).cmp(&role_count(b)))
            .then(a.name.cmp(&b.name))
    };
    if !like.is_empty() {
        let likel = like.to_lowercase();
        // Exact entity equality first: `--like Foo` must land on the slice that
        // MINED Foo itself. Substring alone would also match wrapper entities
        // that merely carry the name (a pair whose members embed "foo") and let
        // their recurrence steal the pick. Substring remains the fallback for
        // partial pointers when no slice owns the entity verbatim.
        if let Some(c) = model
            .conventions
            .iter()
            .filter(|c| c.is_slice && c.entities.iter().any(|e| e.to_lowercase() == likel))
            .max_by(richer)
        {
            return Some((c, true));
        }
        if let Some(c) = model
            .conventions
            .iter()
            .filter(|c| c.is_slice && c.entities.iter().any(|e| e.to_lowercase().contains(&likel)))
            .max_by(richer)
        {
            return Some((c, true));
        }
    }
    model.conventions.iter().filter(|c| c.is_slice).max_by(richer).map(|c| (c, false))
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Map the mined complexity level (PT keys from the miner) to English labels.
fn level_label(level: &str) -> &str {
    match level {
        "simples" => "basic",
        "média" => "medium",
        "complexa" => "complex",
        other => other,
    }
}

/// The project unit a path belongs to: longest `dir` that prefixes it.
fn project_of<'a>(path: &str, projects: &'a [ProjectUnit]) -> Option<&'a ProjectUnit> {
    projects
        .iter()
        .filter(|p| !p.dir.is_empty() && (path == p.dir || path.starts_with(&format!("{}/", p.dir))))
        .max_by_key(|p| p.dir.len())
}

// --- step parsing ----------------------------------------------------------

struct ParsedStep {
    role: String,
    folder: String,
    example: String,
    model: String,
    optional: bool,
}

fn parse_step(step: &str) -> ParsedStep {
    let role = cap(r"\*\*(.+?)\*\*", step);
    let folder = cap(r" em `([^`]+?)`", step);
    let example = cap(r"ex\.: `([^`]+?)`", step);
    let model = cap(r"modelado em `([^`]+?)`", step);
    ParsedStep { role, folder, example, model, optional: step.contains("opcional") }
}

fn cap(pat: &str, text: &str) -> String {
    Regex::new(pat).ok().and_then(|re| re.captures(text)).and_then(|c| c.get(1)).map(|m| m.as_str().to_string()).unwrap_or_default()
}

// --- helpers ---------------------------------------------------------------

/// Substitute `<Name>` with the entity, following the repo's OWN pluralization
/// (English-style `<Name>s`), so generated paths match existing conventions
/// rather than imposing a foreign plural rule.
fn subst(text: &str, name: &str) -> String {
    let lower = name.to_lowercase();
    let upper = name.to_uppercase();
    text.replace("<Name>s", &format!("{name}s"))
        .replace("<name>s", &format!("{lower}s"))
        .replace("<NAME>S", &format!("{upper}S"))
        .replace("<Name>", name)
        .replace("<name>", &lower)
        .replace("<NAME>", &upper)
}

/// Pick the reference file to mirror, preferring the `like` entity's REAL file.
/// Returns (path, is_real_like_file).
fn mirror_file(folder: &str, example: &str, model_file: &str, like: &str, exemplar: &str, mods: &HashSet<&str>) -> (String, bool) {
    if !like.is_empty() && !folder.is_empty() && !example.is_empty() {
        // 1) the file `like` would have for this role
        let cand = format!("{}{}", subst(folder, like), subst(example, like));
        if mods.contains(cand.as_str()) {
            return (cand, true);
        }
        // 2) recenter the exemplar's real file onto `like`
        if !exemplar.is_empty() {
            let rec = model_file.replace(exemplar, like).replace(&exemplar.to_lowercase(), &like.to_lowercase());
            if mods.contains(rec.as_str()) {
                return (rec, true);
            }
        }
    }
    (model_file.to_string(), false)
}
