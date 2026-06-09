//! End-to-end contract of `scan spec --like`: an unmatched sibling must be
//! called out as UNVERIFIED (never "verified in the model") and the novelty
//! banner must come back, while a real match keeps the Mirrored banner; the
//! fallback slice pick ranks recurrence/confidence above raw role count.

use std::process::Command;

/// Three slice conventions crafted so the old roles-first ranking and the new
/// recurrence/confidence-first ranking disagree:
/// - `wrapper-pseudo`: 7 roles but only 2 entities (conf 0.82) — the old winner.
/// - `billing-slice`: 3 roles, 11 entities, conf 0.94 — the rightful winner.
/// - `lowconf-slice`: 5 roles, same 11 recurrence but conf 0.50 — proves the
///   confidence tie-break (more roles must not rescue a low-confidence shape).
const MODEL: &str = r#"{
  "conventions": [
    {
      "name": "wrapper-pseudo",
      "roles": ["WrapA", "WrapB", "WrapC", "WrapD", "WrapE", "WrapF", "WrapG"],
      "recurrence": 2,
      "entities": ["AlphaWrap", "BetaWrap"],
      "confidence": 0.82,
      "is_slice": true,
      "steps": ["**WrapA** em `src/wraps/` (ex.: `<Name>WrapA.cs`)"],
      "examples": [],
      "exemplar": "AlphaWrap",
      "summary": ""
    },
    {
      "name": "billing-slice",
      "roles": ["Service", "Store", "Gate"],
      "recurrence": 11,
      "entities": ["Order", "Product"],
      "confidence": 0.94,
      "is_slice": true,
      "steps": ["**Service** em `src/services/` (ex.: `<Name>Service.cs`)"],
      "examples": [],
      "exemplar": "Order",
      "summary": ""
    },
    {
      "name": "lowconf-slice",
      "roles": ["RoleA", "RoleB", "RoleC", "RoleD", "RoleE"],
      "recurrence": 11,
      "entities": ["Thing", "Widget"],
      "confidence": 0.5,
      "is_slice": true,
      "steps": ["**RoleA** em `src/rolea/` (ex.: `<Name>RoleA.cs`)"],
      "examples": [],
      "exemplar": "Thing",
      "summary": ""
    }
  ]
}"#;

/// Write the fixture model and run `scan spec` on it, returning stdout.
/// `tag` keeps temp dirs distinct across tests of the same process.
fn run_spec(tag: &str, entity: &str, like: &str) -> String {
    let dir = std::env::temp_dir().join(format!("scan-spec-like-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    std::fs::write(&model, MODEL).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["spec", model.to_str().unwrap(), "--entity", entity, "--like", like])
        .output()
        .expect("run scan spec");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn spec_like_no_match_notes_unverified_and_restores_no_precedent() {
    // "Ghost" matches no slice entity: the draft must say so explicitly, must
    // NOT claim a verified mirror, and novelty is judged as if --like were
    // absent (the model has no modules, so "Tax" has no precedent either).
    let spec = run_spec("nomatch", "Tax", "Ghost");
    assert!(spec.contains("like \"Ghost\" not found in the model"), "explicit not-found note missing:\n{spec}");
    assert!(spec.contains("UNVERIFIED"), "fallback must be flagged UNVERIFIED:\n{spec}");
    assert!(!spec.contains("verified in the model"), "must not claim a verified mirror:\n{spec}");
    assert!(spec.contains("NO PRECEDENT"), "novelty banner must come back when --like matched nothing:\n{spec}");
}

#[test]
fn spec_like_match_prints_mirrored_banner() {
    // "Order" is a real entity of billing-slice: the Mirrored banner stays and
    // the not-found note / novelty banner must not appear.
    let spec = run_spec("match", "Tax", "Order");
    assert!(
        spec.contains("Mirrored on the REAL files of **Order** (verified in the model)"),
        "Mirrored banner missing for a real match:\n{spec}"
    );
    assert!(!spec.contains("not found in the model"), "no not-found note on a real match:\n{spec}");
    assert!(!spec.contains("NO PRECEDENT"), "a matched sibling is precedent enough:\n{spec}");
}

#[test]
fn spec_like_fallback_prefers_recurrence_and_confidence() {
    // No --like at all: the pick must be billing-slice (11x, conf 0.94) — high
    // recurrence beats wrapper-pseudo's 7 roles (2x), and at equal recurrence
    // the higher confidence beats lowconf-slice's 5 roles. Under the previous
    // roles-first ranking wrapper-pseudo would have won; this assertion pins
    // the deliberate ranking change.
    let spec = run_spec("fallback", "Tax", "");
    assert!(spec.contains("recurs across **11** entities (conf 0.94"), "expected billing-slice chosen:\n{spec}");
    assert!(!spec.contains("recurs across **2** entities"), "roles-first pseudo-convention must lose:\n{spec}");
}
