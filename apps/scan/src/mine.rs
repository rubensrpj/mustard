//! Camada 4 — leitura do que se repete nos nomes (sem saber de framework
//! nenhum).
//!
//! Nenhum detector sabe o que é um "Controller" ou um "resolver GraphQL". O que
//! se explora é a única coisa que toda convenção tem: ela *se repete*.
//!
//! O que sai daqui é uma coisa só: os **contratos compartilhados** — os tipos
//! base que muitas entidades distintas estendem ou implementam. Para contar
//! "entidades distintas" é preciso saber que `OrderHandler` e `OrderRepository`
//! falam da mesma entidade `Order`, e é isso, e só isso, que a leitura de
//! afixo, de pasta e de nome aninhado abaixo serve para dizer: ela tira o papel
//! do nome para sobrar a entidade. Os grupos por sufixo do nome, que este
//! arquivo publicava no modelo (`roles` e `conventions`), saíram: ninguém os
//! lia, e era por eles que as skills fracas nasciam.

use crate::model::{Decl, Module};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// An affix must pair with at least this many distinct entities to be a role.
const MIN_ROLE_PARTNERS: usize = 2;
/// A folder name must recur under at least this many distinct parent dirs to
/// count as a role folder (DTOs/, Mappers/, Services/ — one under each module).
const MIN_ROLEFOLDER_PARENTS: usize = 3;
/// …and a role folder must offer a CHOICE. Recurring under many parents proves
/// a folder is systematic; it does not prove it is a ROLE. A build unit's code
/// root recurs exactly the same way — once under every unit — and passes that
/// test while naming nothing: it is the path you had to take, not a decision
/// anyone made. The line is whether the folder has SIBLINGS: picking `DTOs/`
/// over `Services/` is information, and a folder that is its parent's only
/// child offered no such pick.
///
/// So a candidate is dropped when it is a solo child in MORE than this share of
/// the places it appears. Measured across two unrelated real workspaces — one a
/// flat multi-unit repository, the other a layered one with an entity-per-module
/// backend — the two populations do not overlap: each workspace's code root
/// scored 59% and 100%, while every folder that did name a role scored between
/// 0% and 12%. A simple majority sits in that gap with room on both sides, so it
/// is a boundary rather than a tuned number.
const MAX_SOLO_SHARE: f32 = 0.5;
/// A bare (single-token) class name must recur across at least this many
/// distinct path-entities to count as a role (e.g. a nested `Validator`).
const MIN_BARE_ENTITIES: usize = 3;
/// A base type must be built on by at least this many distinct entities to be
/// reported as a shared contract.
const MIN_SHARED_CONTRACT: usize = 3;

struct Symbol {
    path: String,
    tokens: Vec<String>,
    supertypes: Vec<String>,
}

pub(crate) struct Mined {
    pub shared_contracts: Vec<crate::model::SharedContract>,
}

pub fn mine(modules: &[Module]) -> Mined {
    let symbols = collect_symbols(modules);

    // --- Leitura 1: o papel pelo sufixo, por frequência ---------------------
    let mut suffix_partners: HashMap<String, HashSet<String>> = HashMap::new();
    for s in &symbols {
        if s.tokens.len() >= 2 {
            let last = s.tokens.last().unwrap().clone();
            let remainder = s.tokens[..s.tokens.len() - 1].join("");
            suffix_partners.entry(last).or_default().insert(remainder);
        }
    }
    let role_suffixes: HashSet<String> = suffix_partners
        .iter()
        .filter(|(_, p)| p.len() >= MIN_ROLE_PARTNERS)
        .map(|(t, _)| t.clone())
        .collect();

    let mut prefix_partners: HashMap<String, HashSet<String>> = HashMap::new();
    for s in &symbols {
        if s.tokens.len() >= 2 && !role_suffixes.contains(s.tokens.last().unwrap()) {
            let first = s.tokens[0].clone();
            let remainder = s.tokens[1..].join("");
            prefix_partners.entry(first).or_default().insert(remainder);
        }
    }
    let role_prefixes: HashSet<String> = prefix_partners
        .iter()
        .filter(|(t, p)| p.len() >= MIN_ROLE_PARTNERS && t.len() > 1)
        .map(|(t, _)| t.clone())
        .collect();

    // --- Miner 1b: role FOLDERS (convention encoded in directory names) ------
    // A folder name is a role if it recurs as the immediate parent folder under
    // many DISTINCT parent dirs (one per entity): DTOs/, Mappers/, Services/.
    // This catches roles that file-name suffixes miss or fragment. A project
    // that instead centralises a type (e.g. Domain/Entities) has a single parent
    // for that folder, fails this test, and stays on the suffix path — so the
    // rule adapts to each layout without being told which one it is.
    // A folder is a role only if it sits under many DISTINCT parent *names*
    // (DTOs under ApiKeys, Banks, Contracts, …). A module folder like Contracts/
    // sits under only `v1`/`v2` — few distinct parent names — so it is NOT a
    // role, even though it appears under several full paths. Counting parent
    // names rather than paths keeps `Modules/v1` and `Modules/v2` from inflating
    // a module folder into a false role.
    let mut folder_parents: HashMap<String, HashSet<String>> = HashMap::new();
    for s in &symbols {
        let segs = path_segs(&s.path);
        if segs.len() >= 3 {
            let folder = segs[segs.len() - 2].to_string();
            let parent_name = segs[segs.len() - 3].to_string();
            folder_parents.entry(folder).or_default().insert(parent_name);
        }
    }
    // …and the sibling test above: a folder that is usually its parent's only
    // child named no role, it named the only way in. Built over EVERY directory
    // level (not just a file's immediate parent), because the code root sits
    // one level above the folders that do carry roles.
    let dir_children = directory_children(symbols.iter().map(|s| s.path.as_str()));
    let role_folders: HashSet<String> = folder_parents
        .iter()
        .filter(|(_, p)| p.len() >= MIN_ROLEFOLDER_PARENTS)
        .filter(|(f, _)| !is_solo_child(f, &dir_children))
        .map(|(f, _)| f.clone())
        .collect();

    // Entity inferred from the path is computed inline at assignment time
    // (folder above a role-folder, else the file's own folder).

    // --- Miner 1c: bare recurring class names (e.g. a nested `Validator`) -----
    // A single-token class declared inside a role-folder file, whose name recurs
    // across many path-entities, is a role whose entity lives in the surrounding
    // folder — the only way to surface roles declared as nested types.
    let mut bare_entities: HashMap<String, HashSet<String>> = HashMap::new();
    for s in &symbols {
        if s.tokens.len() == 1 {
            let segs = path_segs(&s.path);
            if segs.len() >= 3 && role_folders.contains(segs[segs.len() - 2]) {
                bare_entities
                    .entry(s.tokens[0].clone())
                    .or_default()
                    .insert(canonical_key(segs[segs.len() - 3]));
            }
        }
    }
    let bare_roles: HashSet<String> = bare_entities
        .iter()
        .filter(|(_, e)| e.len() >= MIN_BARE_ENTITIES)
        .map(|(n, _)| n.clone())
        .collect();

    // --- Tira o papel de cada símbolo; o que sobra é a entidade -------------
    //
    // A entidade é a chave que faz `OrderHandler` e `OrderRepository` contarem
    // como UM implementador do mesmo contrato, e não dois. É só para isso que o
    // papel é lido: ele não é publicado em lugar nenhum.
    let mut entity_keys: HashSet<String> = HashSet::new();
    let mut contract_entities: HashMap<String, BTreeSet<String>> = HashMap::new();

    for s in &symbols {
        let segs = path_segs(&s.path);
        let in_role_folder = segs.len() >= 3 && role_folders.contains(segs[segs.len() - 2]);

        let entity: String =
            if s.tokens.len() == 1 && bare_roles.contains(&s.tokens[0]) && in_role_folder {
                // Papel aninhado (um `Validator` dentro de um arquivo de Dto).
                segs[segs.len() - 3].to_string()
            } else if in_role_folder {
                // O papel é a pasta; a entidade é a pasta acima dela.
                segs[segs.len() - 3].to_string()
            } else if s.tokens.len() >= 2 && role_suffixes.contains(s.tokens.last().unwrap()) {
                entity_tokens(&s.tokens[..s.tokens.len() - 1])
            } else if s.tokens.len() >= 2 && role_prefixes.contains(&s.tokens[0]) {
                s.tokens[1..].join("")
            } else {
                s.tokens.join("")
            };

        if entity.is_empty() {
            continue;
        }
        let key = canonical_key(&entity);
        if key.is_empty() {
            continue;
        }
        entity_keys.insert(key.clone());
        for st in &s.supertypes {
            contract_entities.entry(st.clone()).or_default().insert(key.clone());
        }
    }

    // --- Shared contracts: base types many entities build on ----------------
    let mut shared_contracts: Vec<crate::model::SharedContract> = contract_entities
        .iter()
        .filter(|(name, ents)| ents.len() >= MIN_SHARED_CONTRACT && !entity_keys.contains(&canonical_key(name)))
        .map(|(name, ents)| crate::model::SharedContract { name: name.clone(), implementors: ents.len() })
        .collect();
    shared_contracts.sort_by(|a, b| b.implementors.cmp(&a.implementors).then(a.name.cmp(&b.name)));

    Mined { shared_contracts }
}

// --- symbol collection -----------------------------------------------------

fn collect_symbols(modules: &[Module]) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for m in modules {
        // Computed once per module: whether this file declares a single unit,
        // which is the second way its callables earn unit status ([`sole_unit`]).
        let sole = sole_unit(m);
        for d in &m.declarations {
            if !is_significant(d, &m.path, sole) || !seen.insert((m.path.clone(), d.name.clone())) {
                continue;
            }
            out.push(Symbol {
                path: m.path.clone(),
                tokens: strip_interface_i(split_tokens(&d.name)),
                supertypes: d.supertypes.clone(),
            });
        }
    }
    out
}

/// Kinds that are a unit on their own: a NAMED TYPE, which every language that
/// has one uses as the thing an architectural role hangs on (`UserService`,
/// `BankRepository`). Closed vocabulary — the generic `@definition.<kind>`
/// suffix the `.scm` layer emits, never a grammar node.
const UNIT_TYPE_KINDS: &[&str] =
    &["class", "interface", "record", "struct", "enum", "trait", "mixin", "extension", "type"];

/// Kinds that are a unit only when the declaration NAMES ITS OWN FILE.
///
/// A callable is not architecture by itself — a private helper buried among
/// forty others says nothing about how the project is built, and treating it as
/// a unit is what mints bare grammatical particles as roles. But a callable
/// that names its file IS the file's subject, the same fact a named type states
/// when it carries the name of the file that declares it. One rule, no
/// exception list: whether a project spells its units as types or as callables
/// is a property of the project, never a branch in this code.
const UNIT_CALLABLE_KINDS: &[&str] = &["function", "const"];

/// Whether a declaration is an architectural UNIT — the thing roles are mined
/// from. Everything else is a MEMBER (`method`, `field`, `property`,
/// `enum_member`): it lives inside a unit and feeds the digest's term index
/// only. The distinction is drawn upstream, by each query set's generic
/// `@definition.<kind>` vocabulary, and applied here identically for every one
/// of them — this function knows no dialect and must never learn one.
fn is_significant(d: &Decl, module_path: &str, sole: bool) -> bool {
    if d.name.len() < 3 {
        return false;
    }
    if UNIT_TYPE_KINDS.contains(&d.kind.as_str()) {
        return true;
    }
    UNIT_CALLABLE_KINDS.contains(&d.kind.as_str())
        && (names_its_file(&d.name, module_path) || sole)
}

/// Whether `m` declares exactly ONE unit candidate — the second way a callable
/// earns unit status ([`is_significant`]).
///
/// [`names_its_file`] rests on the file being NAMED by whoever wrote it. Where
/// a layout fixes the filename instead — position in the tree carrying the
/// meaning a name would carry — the author still names the declaration, after
/// the domain or the verb. The namesake test then answers "no" for every file
/// in that layout at once, and a whole area leaves the census with no symbol.
/// Measured on a real workspace: one request-handling tree of 47 modules went
/// from 45 symbols to 0, and every mold teaching its convention vanished with
/// them.
///
/// Counting recurrence would not fix it. How often a name repeats under a
/// shared stem has no gap between convention and noise — measured across two
/// unrelated workspaces, the population runs 4045 / 74 / 27 / 4 and 6562 / 22 /
/// 6 / 0, a continuous tail — so any floor drawn there is a curated number
/// waiting to be wrong in the next repository.
///
/// Singularity needs no floor. [`UNIT_CALLABLE_KINDS`] is justified by a helper
/// "buried among forty others"; the opposite of forty is ONE. A file declaring
/// a single unit IS that declaration, whatever name the layout imposed on the
/// file. The cleanup that motivated the namesake test survives untouched,
/// because a bare grammatical particle is never the only thing a file declares:
/// this readmitted 429 of 4626 dropped callables in one workspace and 23 of
/// 6653 in the other, with no particle among them.
fn sole_unit(m: &Module) -> bool {
    let mut names: HashSet<&str> = HashSet::new();
    for d in &m.declarations {
        if d.name.len() < 3 {
            continue;
        }
        if UNIT_TYPE_KINDS.contains(&d.kind.as_str())
            || UNIT_CALLABLE_KINDS.contains(&d.kind.as_str())
        {
            names.insert(d.name.as_str());
            if names.len() > 1 {
                return false;
            }
        }
    }
    names.len() == 1
}

/// Whether `name` is the file's namesake — identifier and file stem equal once
/// case and separators are dropped, so `close_gate`/`close_gate.x`,
/// `useProject`/`useProject.x` and `UserService`/`user_service.x` all match.
/// Separator- and case-blind on purpose: the convention is the WORD SEQUENCE,
/// and the joint between words is spelled differently from project to project.
/// No extension is inspected and no naming style is privileged.
fn names_its_file(name: &str, module_path: &str) -> bool {
    let file = module_path.rsplit(['/', '\\']).next().unwrap_or(module_path);
    let stem = file.split('.').next().unwrap_or(file);
    !stem.is_empty() && squash(stem) == squash(name)
}

/// Lowercase `s` keeping only ASCII alphanumerics (`close_gate` -> `closegate`,
/// `useProject` -> `useproject`).
fn squash(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

// --- text helpers ----------------------------------------------------------

fn split_tokens(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = name.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c == '_' || c == '-' || c == '.' || c == ' ' {
            if !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let next_lower = chars.get(i + 1).map(|n| n.is_lowercase()).unwrap_or(false);
        let prev_lower = cur.chars().last().map(|p| p.is_lowercase() || p.is_ascii_digit()).unwrap_or(false);
        if !cur.is_empty() && c.is_uppercase() && (prev_lower || next_lower) {
            tokens.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens.into_iter().filter(|t| !t.is_empty()).collect()
}

fn strip_interface_i(tokens: Vec<String>) -> Vec<String> {
    if tokens.len() >= 2 && tokens[0] == "I" && tokens[1].chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        tokens[1..].to_vec()
    } else {
        tokens
    }
}

/// Join entity tokens, dropping a leading ALL-LOWERCASE particle when a
/// capitalized token follows (`useBanksConfig` → entity `Banks`, never
/// `useBanks`): a camelCase head names the naming pattern (a hook/builder
/// verb), not the entity — and keeping it breaks the dir abstraction (the
/// folder `banks/` never matches the token `usebanks`, so every member lands
/// in a distinct literal dir and the recurrence floor drops the whole
/// convention). Case-shape rule only — no curated verb list (agnostic).
fn entity_tokens(tokens: &[String]) -> String {
    let rest = if tokens.len() >= 2
        && !tokens[0].is_empty()
        && tokens[0].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && tokens[1].chars().next().is_some_and(char::is_uppercase)
    {
        &tokens[1..]
    } else {
        tokens
    };
    rest.join("")
}

fn canonical_key(entity: &str) -> String {
    let lower = entity.to_lowercase();
    if lower.len() > 3 && lower.ends_with('s') && !lower.ends_with("ss") {
        lower[..lower.len() - 1].to_string()
    } else {
        lower
    }
}

fn path_segs(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// Every directory in the tree mapped to the set of directory names directly
/// beneath it. Built over ALL levels, not just each file's immediate parent:
/// the folders this feeds a verdict on sit at every depth, and a build unit's
/// code root sits one level ABOVE the folders that do carry roles. The root
/// itself is the empty key, so a top-level folder is measured too.
fn directory_children<'a>(paths: impl Iterator<Item = &'a str>) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in paths {
        let segs = path_segs(path);
        // The last segment is the file itself — a file is not a child folder.
        for i in 0..segs.len().saturating_sub(1) {
            let parent = segs[..i].join("/");
            out.entry(parent).or_default().insert(segs[i].to_string());
        }
    }
    out
}

/// Whether `folder` is its parent's ONLY child in more than [`MAX_SOLO_SHARE`]
/// of the places it appears — see that constant for why a solo child cannot be
/// a role. A folder that appears nowhere is not solo (an empty denominator is
/// missing evidence, never proof).
fn is_solo_child(folder: &str, dir_children: &BTreeMap<String, BTreeSet<String>>) -> bool {
    let mut seen = 0usize;
    let mut solo = 0usize;
    for children in dir_children.values() {
        if !children.contains(folder) {
            continue;
        }
        seen += 1;
        if children.len() == 1 {
            solo += 1;
        }
    }
    if seen == 0 {
        return false;
    }
    #[allow(clippy::cast_precision_loss)]
    let share = solo as f32 / seen as f32;
    share > MAX_SOLO_SHARE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um módulo com uma classe só, que estende `base`.
    fn class_on(path: &str, name: &str, base: &str) -> Module {
        Module {
            path: path.to_string(),
            declarations: vec![Decl {
                kind: "class".into(),
                name: name.to_string(),
                supertypes: vec![base.to_string()],
                ..Decl::default()
            }],
            ..Module::default()
        }
    }

    /// Um contrato compartilhado conta ENTIDADES distintas, não arquivos: os
    /// dois arquivos de papel de uma mesma entidade valem um implementador só.
    ///
    /// É esta a única razão pela qual a leitura de papel continua aqui depois
    /// que os grupos por sufixo saíram do modelo — ela tira o papel do nome
    /// para sobrar a entidade. Se ela parar de tirar, `OrderHandler` e
    /// `OrderRepository` viram duas entidades, e o número de implementadores
    /// dobra sem que nada no repositório tenha mudado.
    #[test]
    fn um_contrato_conta_entidades_distintas_e_nao_arquivos() {
        let mut modules = Vec::new();
        for entidade in ["Order", "Product", "Customer"] {
            let dir = entidade.to_lowercase();
            modules.push(class_on(
                &format!("src/{dir}/{dir}_handler.x"),
                &format!("{entidade}Handler"),
                "BaseThing",
            ));
            modules.push(class_on(
                &format!("src/{dir}/{dir}_repository.x"),
                &format!("{entidade}Repository"),
                "BaseThing",
            ));
        }

        let mined = mine(&modules);
        let contrato = mined
            .shared_contracts
            .iter()
            .find(|c| c.name == "BaseThing")
            .expect("o tipo base de seis arquivos é um contrato compartilhado");
        assert_eq!(
            contrato.implementors, 3,
            "três entidades o estendem, em seis arquivos: {:?}",
            mined.shared_contracts
        );
    }

    /// One module whose declarations are all callables — the shape
    /// [`sole_unit`] judges.
    fn callables(path: &str, names: &[&str]) -> Module {
        Module {
            path: path.to_string(),
            declarations: names
                .iter()
                .map(|n| Decl { kind: "const".into(), name: (*n).to_string(), ..Decl::default() })
                .collect(),
            ..Module::default()
        }
    }

    /// A layout that fixes the filename leaves the author no way to make a
    /// declaration its file's namesake: the file is named for its POSITION and
    /// the declaration for the verb it answers. Being the only unit in the file
    /// says the same thing the namesake test says, without needing the two
    /// names to agree — so the census keeps seeing an area that would otherwise
    /// leave it entirely.
    #[test]
    fn a_lone_callable_is_its_files_subject_whatever_the_file_is_called() {
        let m = callables("api/banks/x/route.ts", &["PUT"]);
        assert!(!names_its_file("PUT", &m.path), "the layout named the file, not the author");
        assert!(sole_unit(&m), "it is the only unit the file declares");
        assert!(
            is_significant(&m.declarations[0], &m.path, sole_unit(&m)),
            "so it is the file's subject and belongs in the census"
        );
    }

    /// …and the cleanup the namesake test was written for is untouched, because
    /// singularity is exactly what a buried helper does NOT have. No floor is
    /// consulted here: one is one in every repository.
    #[test]
    fn a_callable_buried_among_others_is_still_not_a_unit() {
        let m = callables("lib/helpers.ts", &["formatDate", "parseAmount", "clamp"]);
        assert!(!sole_unit(&m), "three units in one file — none of them is its subject");
        for d in &m.declarations {
            assert!(
                !is_significant(d, &m.path, false),
                "{} is a member of a helper bag, not architecture",
                d.name
            );
        }
    }

    /// A named type never needed either test, and must not start depending on
    /// one: a file carrying a type PLUS helpers still yields the type.
    #[test]
    fn a_named_type_is_a_unit_even_when_it_shares_its_file() {
        let m = Module {
            path: "domain/user_service.cs".into(),
            declarations: vec![
                Decl { kind: "class".into(), name: "UserService".into(), ..Decl::default() },
                Decl { kind: "const".into(), name: "DefaultPageSize".into(), ..Decl::default() },
            ],
            ..Module::default()
        };
        assert!(!sole_unit(&m), "two candidates — singularity does not apply");
        assert!(
            is_significant(&m.declarations[0], &m.path, false),
            "a named type stands on its own kind"
        );
        assert!(
            !is_significant(&m.declarations[1], &m.path, false),
            "…and the constant beside it does not ride along"
        );
    }

    /// A build unit's code root recurs under every unit — which is exactly the
    /// shape "recurs under many distinct parents" was written to catch — yet it
    /// names nothing. The sibling test separates them: the roles sit BESIDE
    /// each other under one parent, the code root is alone under its own.
    ///
    /// Both layouts here are transcribed from real measurements, so the test
    /// fails if the boundary ever stops separating them.
    #[test]
    fn a_folder_that_is_its_parents_only_child_is_not_a_role() {
        // A layered workspace: each unit has ONE code root; inside it the real
        // role folders sit side by side.
        let paths = [
            "apps/one/src/commands/a.x",
            "apps/one/src/hooks/b.x",
            "apps/one/src/shared/c.x",
            "apps/two/src/commands/d.x",
            "apps/two/src/hooks/e.x",
            "apps/three/src/commands/f.x",
            "apps/three/src/hooks/g.x",
        ];
        let kids = directory_children(paths.iter().copied());
        assert!(
            is_solo_child("src", &kids),
            "the code root is alone under each unit — it names the only way in"
        );
        assert!(!is_solo_child("commands", &kids), "a role folder has siblings");
        assert!(!is_solo_child("hooks", &kids), "…and so does the other one");
    }

    /// The entity-per-folder layout, where the roles recur once under every
    /// entity. This is the case `MIN_ROLEFOLDER_PARENTS` exists for, and the
    /// sibling test must leave it completely alone.
    #[test]
    fn roles_that_recur_under_every_entity_keep_their_siblings() {
        let paths = [
            "app/Modules/Banks/DTOs/a.x",
            "app/Modules/Banks/Services/b.x",
            "app/Modules/Banks/Repositories/c.x",
            "app/Modules/Cards/DTOs/d.x",
            "app/Modules/Cards/Services/e.x",
            "app/Modules/Cards/Repositories/f.x",
            "app/Modules/Loans/DTOs/g.x",
            "app/Modules/Loans/Services/h.x",
        ];
        let kids = directory_children(paths.iter().copied());
        for role in ["DTOs", "Services", "Repositories"] {
            assert!(!is_solo_child(role, &kids), "{role} is chosen over its siblings");
        }
    }

    /// An empty denominator is missing evidence, not proof of solitude.
    #[test]
    fn a_folder_that_appears_nowhere_is_never_solo() {
        let kids = directory_children(["a/b/c.x"].iter().copied());
        assert!(!is_solo_child("nowhere", &kids));
    }

    /// The share is a strict majority, so a folder split evenly between solo
    /// and accompanied keeps its role — the tie goes to keeping evidence.
    #[test]
    fn exactly_half_solo_is_not_enough_to_drop_a_folder() {
        let paths = [
            "one/roles/a.x",   // solo under `one`
            "two/roles/b.x",   // accompanied under `two`
            "two/other/c.x",
        ];
        let kids = directory_children(paths.iter().copied());
        assert!(!is_solo_child("roles", &kids), "1 of 2 is not MORE than half");
    }
}
