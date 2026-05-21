//! External cost adapters — placeholder, populated in W3.
//!
//! W3 (`wave-3-ingestion`) injects three submodules here:
//!
//! - `otel` — receiver for the Claude Code native OTLP stream.
//! - `jsonl` — parser for the OpenAI-compatible JSONL Anthropic exports.
//! - `rtk` — adapter that converts `rtk gain` output into [`SavingsRecord`]s.
//!
//! Each submodule must terminate in a call to one of the four writer
//! functions in [`super::writer`]; no submodule reads from the database
//! directly. See the parent spec `2026-05-20-economia-moat-unification` for
//! the full ingestion architecture.
