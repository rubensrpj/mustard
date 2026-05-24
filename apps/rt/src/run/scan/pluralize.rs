//! English pluralization helpers — a port of `registry/pluralize.js`.
//!
//! Converts `snake_case` plural database table names to `PascalCase` singular
//! entity names. Used by the entity scanners when deriving an entity name from
//! a table name. The JS module was extracted from `sync-registry.js` so scanners
//! could reuse it without depending on the top-level CLI script; the same
//! reuse goal holds here.
//!
//! This is a reusable library surface — no scanner derives entity names from
//! table names yet (the ones that do land in a later wave), so the public API
//! has no in-crate caller. `dead_code` is allowed module-wide rather than on
//! each item, matching the deliberate "ported library, future caller" intent.
#![allow(dead_code)]

/// Lookup for common irregular plurals.
///
/// Each tuple is `(lowercase plural form, PascalCase singular entity name)`.
/// Mirrors the `IRREGULAR_PLURALS` object in `pluralize.js` exactly.
const IRREGULAR_PLURALS: &[(&str, &str)] = &[
    ("people", "Person"),
    ("children", "Child"),
    ("men", "Man"),
    ("women", "Woman"),
    ("mice", "Mouse"),
    ("geese", "Goose"),
    ("teeth", "Tooth"),
    ("feet", "Foot"),
    ("data", "Datum"),
    ("indices", "Index"),
    ("matrices", "Matrix"),
    ("vertices", "Vertex"),
    ("analyses", "Analysis"),
    ("bases", "Base"),
    ("crises", "Crisis"),
    ("diagnoses", "Diagnosis"),
    ("hypotheses", "Hypothesis"),
    ("parentheses", "Parenthesis"),
    ("theses", "Thesis"),
    ("criteria", "Criterion"),
    ("phenomena", "Phenomenon"),
    ("media", "Medium"),
    ("statuses", "Status"),
    ("addresses", "Address"),
];

/// Look up an irregular plural by its full (lowercase) form.
fn irregular(word: &str) -> Option<&'static str> {
    IRREGULAR_PLURALS
        .iter()
        .find(|(plural, _)| *plural == word)
        .map(|(_, singular)| *singular)
}

/// Uppercase the first character of an ASCII word, leaving the rest unchanged.
///
/// Mirrors JS `word.charAt(0).toUpperCase() + word.slice(1)` for the ASCII
/// inputs the scanners produce (identifiers and table names).
fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Singularize a single lowercase English word using simple heuristics.
///
/// A faithful port of `singularize()` in `pluralize.js`: irregular lookup,
/// already-singular indicators, then the `-ies` / `-sses` / `-es` / generic
/// `-s` rules, in that order.
#[must_use]
pub fn singularize(word: &str) -> String {
    // Irregular lookup — the JS form lower-cases the PascalCase singular.
    if let Some(singular) = irregular(word) {
        return singular.to_ascii_lowercase();
    }

    // Already-singular indicators.
    if word.ends_with("ss") || word.ends_with("us") || word.ends_with("is") || word == "queue" {
        return word.to_string();
    }

    // -ies -> -y (companies -> company).
    if let Some(stem) = word.strip_suffix("ies") {
        return format!("{stem}y");
    }

    // -sses -> -ss (addresses -> address).
    if word.ends_with("sses") {
        return word[..word.len() - 2].to_string();
    }

    // -es after sh, ch, x, z -> remove -es (boxes -> box).
    if word.ends_with("shes")
        || word.ends_with("ches")
        || word.ends_with("xes")
        || word.ends_with("zes")
    {
        return word[..word.len() - 2].to_string();
    }

    // Generic -s removal (contracts -> contract).
    if word.ends_with('s') && !word.ends_with("ss") {
        return word[..word.len() - 1].to_string();
    }

    word.to_string()
}

/// Convert a `snake_case` plural table name to a `PascalCase` singular entity name.
///
/// A faithful port of `snakeToPascalSingular()`: irregular lookup on the full
/// compound name first, then split on `_` and singularize only the last part.
#[must_use]
pub fn snake_to_pascal_singular(snake_plural: &str) -> String {
    if let Some(singular) = irregular(snake_plural) {
        return singular.to_string();
    }

    let parts: Vec<&str> = snake_plural.split('_').collect();
    let last = parts.len().saturating_sub(1);
    parts
        .iter()
        .enumerate()
        .map(|(idx, part)| {
            let word = if idx == last {
                singularize(part)
            } else {
                (*part).to_string()
            };
            capitalize(&word)
        })
        .collect()
}

/// Convert a `snake_case` name to `PascalCase` without singularization.
///
/// A faithful port of `snakeToPascal()`.
#[must_use]
pub fn snake_to_pascal(snake_name: &str) -> String {
    snake_name.split('_').map(capitalize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singularize_regular_words() {
        assert_eq!(singularize("companies"), "company");
        assert_eq!(singularize("addresses"), "address");
        assert_eq!(singularize("boxes"), "box");
        assert_eq!(singularize("contracts"), "contract");
        assert_eq!(singularize("queue"), "queue");
        assert_eq!(singularize("churches"), "church");
    }

    #[test]
    fn singularize_already_singular() {
        assert_eq!(singularize("status"), "status");
        assert_eq!(singularize("analysis"), "analysis");
        assert_eq!(singularize("bus"), "bus");
    }

    #[test]
    fn singularize_irregular_lowercases() {
        assert_eq!(singularize("people"), "person");
        assert_eq!(singularize("children"), "child");
    }

    #[test]
    fn snake_to_pascal_singular_cases() {
        assert_eq!(snake_to_pascal_singular("contracts"), "Contract");
        assert_eq!(snake_to_pascal_singular("partner_types"), "PartnerType");
        assert_eq!(snake_to_pascal_singular("people"), "Person");
        assert_eq!(snake_to_pascal_singular("companies"), "Company");
        assert_eq!(
            snake_to_pascal_singular("product_categories"),
            "ProductCategory"
        );
        assert_eq!(snake_to_pascal_singular("email_queue"), "EmailQueue");
    }

    #[test]
    fn snake_to_pascal_no_singularization() {
        assert_eq!(snake_to_pascal("contract_status"), "ContractStatus");
        assert_eq!(snake_to_pascal("user"), "User");
    }
}
