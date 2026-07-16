---
name: rt-redirect-pattern
description: Use when adding or refactoring a bash-command redirect stage that lexes a shell command by shape and denies, advises or falls through.
tags: [add, refactor]
appliesTo: [redirect]
scope: [code-editing]
source: scan
metadata:
  generated_by: scan
  cluster:
    label: redirect
---

# redirect pattern

## Purpose

The `redirect` modules are ordered stages of the Bash command gate: each exposes one `pub(super) fn bash_{name}_redirect(cmd: &str) -> Option<Verdict>` that analyses a shell command line by its textual shape alone — first token, flags, operand form, redirect operators — and either short-circuits with `Some(Deny/Inject/Allow)` or returns `None` to fall through to the next stage in `bash_command_gate.rs`. They are pure string functions: deterministic, no filesystem probe, no IO, and no `regex` crate — matching is byte-wise or `split_whitespace`-based. Quoted spans must never be read as shell operators (a `"foo|bar"` grep pattern is not a pipe), which is why the stages lean on the shared `super::lex` helpers (`mask_quoted_operators`, `strip_leading_rtk`, `is_cmd_separator`, `truncate`). A `Deny` carries a `[bracketed-tag]` reason that quotes the offending token and teaches the fix; a piped/composed command downgrades to an advisory `Inject` because denying a compound line is unsafe.

## Convention

- Folder: `apps/rt/src/hooks/bash/`
- Suffix: `redirect` (file `{name}_redirect.rs`, entry fn `bash_{name}_redirect`)
- Extension: `.rs`
- Declares: functions (one `pub(super)` entry + private shape-helpers)
- Count: 11

## How to apply

To add a new `redirect` stage:

- Create `apps/rt/src/hooks/bash/{name}_redirect.rs` with a `//!` module doc naming the failure mode it makes loud and any deliberate divergence from a JS-port ancestor.
- Expose exactly one `pub(super) fn bash_{name}_redirect(cmd: &str) -> Option<Verdict>`; keep every helper (`looks_like_*`, `is_*_shaped`, `*_target`) private and pure. Decide by argument FORM only — never probe the filesystem, so the stage stays deterministic and cheap.
- Declare it as a private `mod {name}_redirect;` in `apps/rt/src/hooks/bash/mod.rs` (only `bash_command_gate` is `pub`), then insert the call at the right point of the chain in `apps/rt/src/hooks/bash/bash_command_gate.rs` — order is the contract (`windows_redirect` runs before `native_redirect` before `rtk_rewrite`), and an unwired stage silently never runs.
- Run operator/segment detection on the `mask_quoted_operators` view of the command; strip `2>/dev/null`-style noise before analysis; see through a leading `rtk` only when the exemplars' rules demand it.
- Verdict grammar: `None` = not this stage's business; `Some(Verdict::Deny { reason })` = hard stop with a `[tag]`-prefixed, fix-carrying reason (truncate long commands via `lex::truncate`); `Some(Verdict::Inject { context })` = advisory for compound commands; `Some(Verdict::Allow)` = deliberately silence a downstream nudge.
- Cover the stage with in-file `#[cfg(test)]` parity tests: one per allow/deny/inject shape, plus the quoted-operator and env-var-prefix edge cases — the exemplars treat regressions here as test-named incidents.

## Examples

- Ref: apps/rt/src/hooks/bash/native_redirect.rs
- Ref: apps/rt/src/hooks/bash/windows_redirect.rs
