---
name: scan-stage-pattern
description: Use when adding or refactoring a flat `src/*.rs` stage or projection of the grain miner — a deterministic pipeline step that builds the model or projects from it without re-reading the repo
tags: [add, refactor]
appliesTo: [stage]
scope: [code-editing]
source: scan
metadata:
  generated_by: scan
  cluster:
    label: stage
---

<!-- mustard:generated -->
# stage pattern

## Purpose

Every `.rs` in `apps/scan/src/` is one deterministic stage or projection of the mining pipeline (`ingest → extract → classify → mine → graph → rank` build the model; `digest`/`facts`/`spec`/`purpose` project FROM the finished `ProjectModel`), each opening with a `//!` doc that states its single responsibility. `ingest.rs` (Layer 0) walks the tree via the `ignore` crate and detects manifests/languages purely from data (`manifests.toml`, the language registry), accumulating into `BTreeMap`s so iteration order is stable. `rank.rs` scores in fixed-point integer (×1024) so every ranking is byte-stable across runs and platforms; its k1/b/weights are DATA in `ranking.toml` (embedded via `include_str!`), and shared arithmetic delegates to `mustard_core::domain::ranking`. `facts.rs` folds the model into a tiny orchestrator-facing JSON (projects in stable model order, entities sorted + deduped) and never re-reads the repository — frequency ties resolve by first-appearance order, never alphabetically. `matching.rs`/`stemmers.rs` are shared pure primitives; `model.rs` holds the typed model; `main.rs` is the thin clap CLI that declares all 16 flat `mod`s and wires stages to subcommands.

## Convention

- Folder: `src/` (flat — no subdirectories; the flatness IS the convention)
- Extension: `.rs`
- Files: 16

## How to apply

- A new stage or projection is ONE new flat `.rs` in `apps/scan/src/`: declare it as `mod` in `main.rs`, wire it from the clap subcommand that needs it, and put its typed output in `model.rs`.
- Stages build the model; projections read only the finished `ProjectModel` — a projection never re-reads the repo or the filesystem.
- Determinism is mandatory: sort + dedup every collection, prefer `BTreeMap`/`BTreeSet` over `HashMap` in iteration (HashMap order changes between runs), use stable tiebreaks (first-appearance/manifest order when source order is meaningful), and never break `serde_json` `preserve_order`.
- No language, extension, grammar-node, or framework name in `src/`: that data lives in `languages.toml`, `manifests.toml`, `queries/<dir>/*.scm`, `ranking.toml` — adding a language, build-system, or tuning knob is a data change, never new logic.
- Degrade without panic: a tree-sitter pattern that fails to compile is dropped individually, a failing grammar skips that language with a warning, and the agnostic textual fallback is preserved — never abort the whole scan. The only allowed `expect` is on embedded-TOML parse (a programmer error any test run catches).

## Examples

- Ref: apps/scan/src/ingest.rs
- Ref: apps/scan/src/rank.rs
- Ref: apps/scan/src/facts.rs
