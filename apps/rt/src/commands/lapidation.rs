//! `run scan-lapidation` — the convention map a request is lapidated AGAINST.
//!
//! ## Why this exists
//!
//! Every flow tells the orchestrator to "lapidate the intent to code vocabulary
//! YOURSELF" before querying the digest. That instruction lived only in prose,
//! so the lapidation was a model guessing which words a project uses — and a
//! guess is exactly as good as how well that model already knows the repo.
//! Measured on a real request, the raw prompt scored 14 of 32 terms and had its
//! planning fields withheld, while the same request expressed in this project's
//! own words scored 5 of 5 and pointed straight at the implementing modules.
//! The difference was never intelligence; it was vocabulary the asker had no
//! way to know.
//!
//! The scan already mines that vocabulary. `roles[]` records what a thing is
//! CALLED here and where its kind lives (`*Observer` under `hooks/observe`,
//! `*Cmd` under `commands/<name>`); `conventions[]` records which roles recur
//! TOGETHER, which is what a new entity in this codebase usually needs. All of
//! it is computed deterministically, written to the model, and then never
//! reaches the step that needs it most.
//!
//! ## What this is NOT
//!
//! It does not read the prompt and it never suggests. A previous attempt to
//! match prompt words against path tokens was measured at 1 useful suggestion
//! in 17 and removed from [`super::orient`], with the reason recorded there:
//! prompt words are PROBLEM vocabulary and path tokens are CODE vocabulary, and
//! they overlap by coincidence. That verdict stands and this module respects
//! it. The machine supplies the MENU; choosing from it is judgement, and
//! judgement is the one thing worth spending a model on.
//!
//! Fail-open like every projection here: a missing or unparseable model yields
//! an empty kit and exit 0, so a caller degrades to the behaviour it had before
//! this command existed. Byte-stable: everything sorted, no clock, no absolute
//! path.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde::Deserialize;

/// How many roles the kit carries. The map is a menu a human or a model reads
/// in one pass — past a few dozen entries it stops being a menu and becomes the
/// model file again, which the caller already has.
const MAX_ROLES: usize = 30;

/// How many co-occurrence slices ride along. These are rarer and denser than
/// roles; a handful covers the recurring shapes of a codebase.
const MAX_SLICES: usize = 12;

/// A role must recur at least this many times to be worth naming. Below it the
/// "convention" is a coincidence, and a menu of coincidences teaches a caller
/// to distrust the menu.
const MIN_ROLE_COUNT: usize = 3;

/// The slice of `grain.model.json` this projection reads. Additive
/// `#[serde(default)]` throughout — the JSON is the contract, not the grain
/// crate's internal types.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Model {
    roles: Vec<Role>,
    conventions: Vec<Convention>,
    projects: Vec<Proj>,
}

/// One mined role: what a thing is called and where its kind lives.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Role {
    affix: String,
    kind: String,
    count: usize,
    common_dir: String,
    decl_kind: String,
    implements: Option<String>,
}

/// One mined convention slice — the roles that recur together per entity.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Convention {
    roles: Vec<String>,
    optional_roles: Vec<String>,
    recurrence: usize,
    is_slice: bool,
}

/// A workspace subproject.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Proj {
    name: String,
    dir: String,
    kind: String,
}

/// Run `scan-lapidation`: print the convention map to stdout; exit 0 always.
pub fn run(root: &Path) {
    print!("{}", render(&load(root)));
}

/// Read the model, degrading to an empty one on any failure.
fn load(root: &Path) -> Model {
    std::fs::read_to_string(root.join(".claude").join("grain.model.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Model>(&raw).ok())
        .unwrap_or_default()
}

/// Render the kit. Pure over the model so the shape is unit-testable without a
/// repository.
fn render(model: &Model) -> String {
    let mut roles: Vec<&Role> =
        model.roles.iter().filter(|r| r.count >= MIN_ROLE_COUNT && !r.affix.is_empty()).collect();
    // Strongest convention first, affix breaking ties — byte-stable.
    roles.sort_by(|a, b| b.count.cmp(&a.count).then(a.affix.cmp(&b.affix)));
    roles.truncate(MAX_ROLES);

    let mut slices: Vec<&Convention> =
        model.conventions.iter().filter(|c| c.is_slice && !c.roles.is_empty()).collect();
    slices.sort_by(|a, b| b.recurrence.cmp(&a.recurrence).then(a.roles.join("+").cmp(&b.roles.join("+"))));
    slices.truncate(MAX_SLICES);

    let mut projects: Vec<&Proj> = model.projects.iter().filter(|p| !p.dir.is_empty()).collect();
    projects.sort_by(|a, b| a.dir.cmp(&b.dir));

    if roles.is_empty() && slices.is_empty() {
        return "LAPIDATION KIT: the model carries no mined convention — \
                lapidate from the terrain census and the request's own literals.\n"
            .to_string();
    }

    let mut out = String::new();
    out.push_str(
        "LAPIDATION KIT — how THIS project names things. Map the request onto these words\n\
         BEFORE querying the digest; do not invent vocabulary and do not copy this list\n\
         into the query wholesale. Derived by scan from the code itself.\n\n",
    );

    if !roles.is_empty() {
        out.push_str("ROLES — what a thing is called here, and where that kind lives:\n");
        for r in &roles {
            let where_ = if r.common_dir.is_empty() { "(no recurring home)" } else { r.common_dir.as_str() };
            let shape = match (r.kind.as_str(), r.decl_kind.as_str()) {
                ("", "") => String::new(),
                (k, "") => format!(" [{k}]"),
                ("", d) => format!(" [{d}]"),
                (k, d) => format!(" [{k} {d}]"),
            };
            let _ = writeln!(out, "  {:<14} {:>4}x  {where_}{shape}", r.affix, r.count);
            if let Some(base) = r.implements.as_deref().filter(|s| !s.is_empty()) {
                let _ = writeln!(out, "  {:<14}       builds on {base}", "");
            }
        }
        out.push('\n');
    }

    if !slices.is_empty() {
        out.push_str(
            "SHAPES — roles that recur TOGETHER. A new entity here usually needs the whole row:\n",
        );
        for c in &slices {
            let core: Vec<&str> = c.roles.iter().map(String::as_str).filter(|r| *r != "(core)").collect();
            let opt: Vec<&str> =
                c.optional_roles.iter().map(String::as_str).filter(|r| *r != "(core)").collect();
            let label = if core.is_empty() { "(core)".to_string() } else { core.join(" + ") };
            let tail = if opt.is_empty() { String::new() } else { format!("   (optional: {})", opt.join(", ")) };
            let _ = writeln!(out, "  {label:<44} {:>3} entities{tail}", c.recurrence);
        }
        out.push('\n');
    }

    if !projects.is_empty() {
        out.push_str("UNITS — where the work lands:\n");
        let mut by_kind: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for p in &projects {
            by_kind.entry(if p.kind.is_empty() { "?" } else { p.kind.as_str() }).or_default().push(&p.dir);
        }
        for (kind, dirs) in by_kind {
            let _ = writeln!(out, "  {kind:<10} {}", dirs.join(", "));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(affix: &str, count: usize, dir: &str) -> Role {
        Role {
            affix: affix.into(),
            kind: "suffix".into(),
            count,
            common_dir: dir.into(),
            decl_kind: "struct".into(),
            implements: None,
        }
    }

    #[test]
    fn the_kit_names_the_role_and_where_its_kind_lives() {
        let m = Model {
            roles: vec![role("Observer", 17, "src/hooks/observe"), role("Cmd", 16, "src/commands")],
            conventions: vec![Convention {
                roles: vec!["Cli".into(), "(core)".into(), "Cmd".into()],
                optional_roles: vec!["Summary".into()],
                recurrence: 7,
                is_slice: true,
            }],
            projects: vec![Proj { name: "rt".into(), dir: "apps/rt".into(), kind: "cargo".into() }],
        };
        let out = render(&m);
        assert!(out.contains("Observer"), "the role is named: {out}");
        assert!(out.contains("src/hooks/observe"), "and where that kind lives: {out}");
        assert!(out.contains("Cli + Cmd"), "(core) is dropped from the shape label: {out}");
        assert!(out.contains("optional: Summary"), "optional roles are marked: {out}");
        assert!(out.contains("apps/rt"), "units ride along: {out}");
    }

    #[test]
    fn a_coincidence_is_not_a_convention() {
        // Below the floor a "role" is two files that happen to rhyme. A menu of
        // coincidences teaches the reader to distrust the menu.
        let m = Model { roles: vec![role("Thing", 2, "src")], ..Model::default() };
        let out = render(&m);
        assert!(!out.contains("Thing"), "under the floor it never ships: {out}");
    }

    #[test]
    fn an_empty_model_says_so_instead_of_pretending() {
        let out = render(&Model::default());
        assert!(out.contains("no mined convention"), "the empty case is stated: {out}");
        assert!(!out.contains("ROLES"), "and no empty section is rendered: {out}");
    }

    #[test]
    fn output_is_byte_stable_across_runs() {
        let m = Model {
            roles: vec![role("Beta", 5, "b"), role("Alpha", 5, "a")],
            ..Model::default()
        };
        assert_eq!(render(&m), render(&m));
        // Equal counts break ties by affix, so order never depends on the map.
        let idx_a = render(&m).find("Alpha").expect("Alpha rendered");
        let idx_b = render(&m).find("Beta").expect("Beta rendered");
        assert!(idx_a < idx_b, "deterministic tie-break by affix");
    }

    #[test]
    fn missing_model_degrades_to_the_empty_kit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = render(&load(dir.path()));
        assert!(out.contains("no mined convention"), "fail-open, never an error: {out}");
    }
}
