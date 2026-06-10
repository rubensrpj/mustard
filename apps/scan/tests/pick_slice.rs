//! Contract of the spec compiler's slice selection (`pick_slice`), pinned
//! end-to-end through the `scan spec` binary on a neutral fixture that mirrors
//! the failure shape seen in the field: a DEGENERATE 2-role wrapper pair whose
//! recurrence (one pair per operation) dwarfs every real vertical's (one slice
//! per entity), and whose wrapper entities carry the target entity's name as a
//! substring — so neither recurrence-first ranking nor a substring `--like`
//! could escape it. The new contract: class precedence (>= 3 roles beats a
//! 2-role pair, which stays a fallback) and `--like` equality-before-substring.

use std::process::Command;

/// Three slice conventions in the field's proportions:
/// - `shell-pair`: 2 roles, recurrence 132, conf 0.94 — the degenerate pair.
///   Its entities are per-operation wrappers (`MakeBrick`, `DropBrick`, ...)
///   that contain the real entity name "Brick" as a substring.
/// - `vertical-slice`: 3 core + 2 optional roles, recurrence 30, conf 0.97 —
///   the real vertical; "Brick" is one of ITS mined entities, verbatim.
/// - `panel-slice`: 3 roles, recurrence 12, conf 0.97 — a second real vertical
///   (UI-shaped), reachable only by pointing `--like` at one of its entities.
const MODEL: &str = r#"{
  "conventions": [
    {
      "name": "shell-pair",
      "roles": ["Inbound", "Outbound"],
      "recurrence": 132,
      "entities": ["MakeBrick", "DropBrick", "MakeStone", "DropStone"],
      "confidence": 0.94,
      "is_slice": true,
      "steps": ["**Inbound** em `src/shells/` (ex.: `<Name>Inbound.cs`)"],
      "examples": [],
      "exemplar": "MakeBrick",
      "summary": ""
    },
    {
      "name": "vertical-slice",
      "roles": ["Shaper", "Keeper", "Gate"],
      "optional_roles": ["Probe", "Mapper"],
      "recurrence": 30,
      "entities": ["Brick", "Stone", "Plank"],
      "confidence": 0.97,
      "is_slice": true,
      "steps": ["**Shaper** em `src/shapers/` (ex.: `<Name>Shaper.cs`)"],
      "examples": [],
      "exemplar": "Brick",
      "summary": ""
    },
    {
      "name": "panel-slice",
      "roles": ["Panel", "Hook", "View"],
      "recurrence": 12,
      "entities": ["Widget", "Gadget"],
      "confidence": 0.97,
      "is_slice": true,
      "steps": ["**Panel** em `src/panels/` (ex.: `<Name>Panel.tsx`)"],
      "examples": [],
      "exemplar": "Widget",
      "summary": ""
    }
  ]
}"#;

/// The degenerate pair alone — the only slice the model has.
const PAIR_ONLY_MODEL: &str = r#"{
  "conventions": [
    {
      "name": "shell-pair",
      "roles": ["Inbound", "Outbound"],
      "recurrence": 132,
      "entities": ["MakeBrick", "DropBrick"],
      "confidence": 0.94,
      "is_slice": true,
      "steps": ["**Inbound** em `src/shells/` (ex.: `<Name>Inbound.cs`)"],
      "examples": [],
      "exemplar": "MakeBrick",
      "summary": ""
    }
  ]
}"#;

/// Write the fixture model and run `scan spec` on it, returning stdout.
/// `tag` keeps temp dirs distinct across tests of the same process.
fn run_spec(tag: &str, model_json: &str, entity: &str, like: &str) -> String {
    let dir = std::env::temp_dir().join(format!("scan-pick-slice-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    std::fs::write(&model, model_json).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["spec", model.to_str().unwrap(), "--entity", entity, "--like", like])
        .output()
        .expect("run scan spec");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// The "Pattern chosen" banner carries recurrence + confidence — together they
/// identify which fixture slice won the pick.
fn chosen(spec: &str, recurrence: usize, conf: &str) -> bool {
    spec.contains(&format!("recurs across **{recurrence}** entities (conf {conf}"))
}

#[test]
fn pick_slice_vertical_class_beats_degenerate_pair_recurrence() {
    // No --like: the >= 3-role vertical (30x) must win even though the 2-role
    // pair out-recurs it 132x — recurrence is not comparable across classes.
    // Under the previous recurrence-first ranking the pair would have won.
    let spec = run_spec("noflag", MODEL, "Slab", "");
    assert!(chosen(&spec, 30, "0.97"), "expected vertical-slice chosen:\n{spec}");
    assert!(!chosen(&spec, 132, "0.94"), "degenerate pair must lose to a real vertical:\n{spec}");
}

#[test]
fn pick_slice_like_exact_entity_wins_over_wrapper_substring() {
    // --like Brick: "Brick" is a verbatim entity of vertical-slice, while the
    // pair's wrappers (MakeBrick, DropBrick) only CONTAIN it. Substring-only
    // matching used to route the pick to the pair (recurrence 132 won the
    // tie); equality-first must land on the slice that mined Brick itself.
    let spec = run_spec("exact", MODEL, "Slab", "Brick");
    assert!(chosen(&spec, 30, "0.97"), "expected vertical-slice for an exact entity match:\n{spec}");
    assert!(
        spec.contains("Mirrored on the REAL files of **Brick** (verified in the model)"),
        "exact match must keep the verified-mirror banner:\n{spec}"
    );
}

#[test]
fn pick_slice_like_exact_entity_routes_to_ui_slice() {
    // --like Widget: exact entity of panel-slice only — the pointer must reach
    // the smaller vertical (12x) even though a richer one (30x) exists. This is
    // the documented escape hatch ("point at a sibling of the other pattern").
    let spec = run_spec("ui", MODEL, "Slab", "Widget");
    assert!(chosen(&spec, 12, "0.97"), "expected panel-slice for --like Widget:\n{spec}");
}

#[test]
fn pick_slice_like_exact_on_wrapper_entity_honors_the_pointer() {
    // --like MakeBrick: the user pointed verbatim at a wrapper entity, so the
    // degenerate pair IS the requested pattern — class precedence must not
    // override an exact pointer (it only governs picks among non-exact picks).
    let spec = run_spec("wrapper", MODEL, "Slab", "MakeBrick");
    assert!(chosen(&spec, 132, "0.94"), "an exact wrapper-entity pointer must be honored:\n{spec}");
}

#[test]
fn pick_slice_like_substring_fallback_when_no_exact_entity() {
    // --like Bri: no slice owns "Bri" verbatim, so substring fallback kicks in;
    // it matches both Brick (vertical) and MakeBrick/DropBrick (pair), and the
    // class rule resolves the tie toward the vertical. Still a real match
    // (like_matched), so no UNVERIFIED note.
    let spec = run_spec("substr", MODEL, "Slab", "Bri");
    assert!(chosen(&spec, 30, "0.97"), "substring fallback must still prefer the vertical class:\n{spec}");
    assert!(!spec.contains("not found in the model"), "substring fallback is a match, not a miss:\n{spec}");
}

#[test]
fn pick_slice_degenerate_pair_is_the_fallback_when_alone() {
    // A repo whose ONLY slice shape is the 2-role pair: the pair must still be
    // compilable — class precedence demotes it, never disqualifies it.
    let spec = run_spec("alone", PAIR_ONLY_MODEL, "Slab", "");
    assert!(chosen(&spec, 132, "0.94"), "the pair must be picked when it is the only slice:\n{spec}");
}
