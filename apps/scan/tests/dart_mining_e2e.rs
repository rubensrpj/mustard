//! End-to-end contract: the generic tree-sitter engine mines a real Flutter
//! project and the registry-driven stack engine names it — with NO Dart- or
//! Flutter-specific logic in `src/`. Dart is wired purely as data
//! (languages.toml + queries/dart/*.scm), the `flutter` stack is data
//! (core's stacks.toml), and the `*.g.dart` generated convention is data
//! (generated-markers.toml). This guards three things at once over one
//! committed fixture (`tests/fixtures/flutter_app`):
//!
//!   (a) the hand-written `lib/main.dart` is MINED — non-empty declarations
//!       (the `MyApp` / `CounterPage` widget classes, copied from the generic
//!       `@definition.class` capture) and the `package:flutter/material.dart`
//!       import — i.e. it did NOT fall through to the empty agnostic fallback;
//!   (b) the `flutter` stack is detected at high confidence (all three signal
//!       classes converge: the pubspec `flutter` dep + the `lib/main.dart`
//!       path marker + the `StatelessWidget`/`runApp(` code signatures);
//!   (c) `lib/counter.g.dart` is classed `generated` by the `**/*.g.dart`
//!       path marker, so it is excluded from the source/stack mining surface.

use std::path::PathBuf;
use std::process::Command;

use mustard_core::domain::vocabulary::stacks::CONFIDENCE_TWO_CLASSES;

/// A committed fixture root, resolved from the crate manifest dir so the test
/// is location-independent.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// Scan a fixture into a temp `grain.model.json` and return (temp dir, parsed
/// model). Mirrors `stack_detection_e2e.rs`: a temp dir owned by the test,
/// removed at the end.
fn scan_fixture(name: &str, label: &str) -> (PathBuf, serde_json::Value) {
    let dir = std::env::temp_dir().join(format!("scan-dart-{}-{}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", fixture(name).to_str().unwrap(), "--out", model.to_str().unwrap()])
        .output()
        .expect("run scan over fixture");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&model).expect("read model")).expect("valid model JSON");
    (dir, v)
}

fn module<'a>(v: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    v["modules"]
        .as_array()
        .expect("model carries modules")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("module {path} present in model"))
}

fn strings(v: &serde_json::Value) -> Vec<&str> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect()
}

#[test]
fn dart_module_is_mined_with_nonempty_declarations_and_imports() {
    let (dir, v) = scan_fixture("flutter_app", "mine");

    // The hand-written entrypoint, mined by the generic Dart query (NOT the
    // empty agnostic fallback).
    let main = module(&v, "lib/main.dart");
    assert_eq!(main["language"], "dart", "selected the dart analyzer: {main}");
    assert!(main.get("file_class").is_none(), "hand-written: no machine class: {main}");

    // (a.1) Declarations are non-empty and carry the widget classes copied
    // verbatim from the `@definition.class` capture suffix.
    let decls = main["declarations"].as_array().expect("module carries declarations");
    assert!(!decls.is_empty(), "lib/main.dart mined to non-empty declarations: {main}");
    let myapp = decls.iter().find(|d| d["name"] == "MyApp").expect("MyApp class declaration mined");
    assert_eq!(myapp["kind"], "class", "kind copied verbatim from the @definition.class capture: {myapp}");
    assert!(
        decls.iter().any(|d| d["name"] == "CounterPage" && d["kind"] == "class"),
        "the second (StatefulWidget) class is mined too: {decls:?}"
    );
    // The Flutter base types are captured as supertypes (extends StatelessWidget).
    let supers = strings(&myapp["supertypes"]);
    assert!(supers.contains(&"StatelessWidget"), "extends base captured as supertype: {supers:?}");

    // (a.2) The import path survives clean_import (leading `import ` keyword +
    // surrounding quotes stripped), proving the import query landed.
    let imports = strings(&main["imports"]);
    assert!(
        imports.contains(&"package:flutter/material.dart"),
        "the material import is mined: {imports:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn flutter_stack_detected_with_high_confidence() {
    let (dir, v) = scan_fixture("flutter_app", "stack");

    // (b) The registry-driven engine names exactly `flutter`, with confidence
    // at least the two-class tier (here all three classes converge → the high
    // tier; assert the floor so the test survives a tier-constant tweak).
    let stacks = v["detected_stacks"].as_array().expect("model carries detected_stacks");
    let flutter = stacks
        .iter()
        .find(|s| s["name"] == "flutter")
        .unwrap_or_else(|| panic!("flutter detected among {stacks:?}"));
    let confidence = flutter["confidence"].as_f64().unwrap();
    assert!(
        confidence >= f64::from(CONFIDENCE_TWO_CLASSES) - 1e-6,
        "flutter detected with reasonable confidence ({confidence} >= {CONFIDENCE_TWO_CLASSES}): {flutter}"
    );

    // The detection is explainable — at least one signal from each evidence
    // class fired (the pubspec dep, the path layout, a code signature).
    let signals = strings(&flutter["signals"]);
    assert!(signals.iter().any(|s| s.starts_with("dep:")), "a manifest dep fired: {signals:?}");
    assert!(signals.iter().any(|s| s.starts_with("path:")), "a path marker fired: {signals:?}");
    assert!(signals.iter().any(|s| s.starts_with("code:")), "a code signature fired: {signals:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generated_dart_file_is_classed_generated() {
    let (dir, v) = scan_fixture("flutter_app", "generated");

    // (c) The `**/*.g.dart` path marker classes the build_runner output as
    // generated, with the matching marker recorded as provenance — so it is
    // demoted out of the source/stack mining surface.
    let generated = module(&v, "lib/counter.g.dart");
    assert_eq!(generated["file_class"], "generated", "*.g.dart classed generated: {generated}");
    assert!(
        generated["marker"].as_str().unwrap().contains(".g.dart"),
        "the path glob is recorded as provenance: {generated}"
    );

    // And the generated file never contributes its stack evidence: the only
    // detected stack stays the real project's, never inflated/duplicated by the
    // machine-written sibling.
    let stacks = v["detected_stacks"].as_array().expect("model carries detected_stacks");
    assert_eq!(stacks.len(), 1, "exactly one stack, from hand-written code: {stacks:?}");
    assert_eq!(stacks[0]["name"], "flutter");

    let _ = std::fs::remove_dir_all(&dir);
}
