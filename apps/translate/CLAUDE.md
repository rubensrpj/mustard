@.claude/scan-map.md

# Translate

> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/CLAUDE.md](../../.claude/CLAUDE.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=cargo; frameworks=candle-core, candle-nn, candle-transformers, tokenizers, hf-hub, lingua, serde, serde_json, anyhow, clap -->
[critical] never println! in apps/translate/src/**
Stdout is protocol, not a console: exactly one JSON line per input line, in the SAME order (blank line → `{"en":"","detected":"unknown"}`), written only through `emit`; every diagnostic goes to stderr — a stray print corrupts the positional zip callers depend on.
Fail-open is the crate's contract: any new failure path must emit the ORIGINAL text as `en`, warn on stderr, and exit 0; route new candle/tokenizers calls through `load_guarded`/`translate_guarded` (tokenizers 0.21 panics instead of returning Err), and keep `unwrap`/`expect` under `#[cfg(test)]` only.
Decoding must stay deterministic: greedy `greedy_pick` (pad masked, NaN skipped, ties break to the lowest token id) — never add sampling, beam search, or temperature; `determinism_two_fresh_loads` must keep proving byte-equal output across fresh loads.
Keep the empty `[workspace]` table in Cargo.toml AND the root manifest's `exclude` — this sidecar deliberately lives outside the workspace so candle/lingua never bloat `mustard-rt` (runs on every tool call) or the deterministic `mustard-scan`.
Model swaps must stay commercially licensed: OPUS-MT is CC-BY-4.0; NLLB and Tower are CC-BY-NC and banned from the product. Weights load from `refs/pr/4` (the safetensors conversion — upstream `main` only ships pickle) into the per-MACHINE cache; never vendor weights into the repo or pin them per project.
<!-- /mustard:guards -->
