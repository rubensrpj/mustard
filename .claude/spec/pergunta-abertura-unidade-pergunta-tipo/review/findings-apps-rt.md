Approved. Full suite: `cargo test --workspace` -> 3004 passed, 6 ignored, 0 failed.

## Verdict per claim

**Guards (apps/rt/CLAUDE.md)** — all six hold. No new `run` subcommand (only a flag), so the four-registration guard is not triggered; dispatch() destructures the new field and threads it (commands/event/cli.rs:173-193). No unwrap/expect outside #[cfg(test)] in the diff. Hooks/observers untouched except `unit_name: None` fillers; main.rs untouched. Output determinism verified live: two identical invocations produced byte-identical JSON.

**Molds** — rt-cmd-pattern is the only skill whose paths match a changed Rust file (commands/event/cli.rs): Option<String> field, #[arg(long = "unit-name")], help doc, dispatch arm in the same file. Conforms.

**AC-1..AC-6** — each ran, each `1 passed` (AC-6: `2 passed`); controls still pass.

**T3/T4 effectiveness (not just presence)** — real binary, isolated repo:
  --unit-name "Nome Escolhido/Pelo Operador" -> {"spec":"nome-escolhido-pelo-operador","branch":"fix/nome-escolhido-pelo-operador","renamedFrom":"palpite-do-chamador","nameFrom":"operator"}
  without it -> "spec":"corrigir-botao-login","nameFrom":"derived-from-intent"
  --kind pipeline.status -> byte-identical to before.
The operator's name reaches the branch because compute_work_branch prefers `spec`, which run() already replaced with the minted slug.

**CR#4 (structural split, no compression)** — confirmed non-redactional: diffing the pre-split `## Dispatch` section against dispatch.md shows ONE changed word (`above` -> `§ Intent Routing`); nothing was cut. Both delivered copies byte-identical to their seeds. The sessionStart path really delivers it: `mustard-rt on SessionStart` returns additionalContext of 8,321 chars containing `# Dispatch Rules`; UserPromptSubmit returns 5,852 chars with `Intent Routing` and no `## Dispatch`.

**Pre-split installs** — backfill_dispatch_inject is exercised by two green tests, is idempotent, and does not re-impose a router the operator dropped; ProjectConfig carries #[serde(flatten)] extra, so the load->write round-trip cannot drop foreign keys.

## Non-blocking observations

1. emit_pipeline.rs:592 — the `.filter(|slug| !slug.trim().is_empty())` after canonicalisation is dead code: `canonical` floors at "x" (spec_slug.rs:17). Measured: `--unit-name '///'` and `--unit-name '!!!'` both yield {"spec":"x","branch":"fix/x","nameFrom":"operator"}. Same floor the --intent path always had, and nameFrom still attributes it honestly — but no test pins it.
2. dispatch.md is 7,994 chars against a 10,000-char EVENT response that also carries the terrain census + drift/prune advisories (measured 8,321 total here). The 9,500 per-file cap does not model siblings, so a project with a fatter census has ~1.7k of real headroom.
3. The change request of 11:27 ("por que corte? ... ajuste isso no mustard agora") has no AC and no code. It was superseded 4 minutes later by the 11:31 request on the same thread, and the critical fix landed in this unit (commit 0f74a380) rather than being deferred — honoured in practice, but nothing records it.

Cleanup note: a temp-dir command mis-fired and wrote stray .claude/spec/{x,alguma,corrigir-botao} dirs plus a census refresh into the repo; the reviewer removed them and `git checkout --` the two grain files.
