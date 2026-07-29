# Acceptance Criteria — Cross-Shell Pattern

> Detail for `/feature` spec authoring: what an AC command can rely on, and the one thing it still cannot.

`mustard-rt run qa-run` executes each AC command through the shell that
`crate::util::platform::build_shell_command` resolves: `sh -c` on Unix, and on
Windows the POSIX shell that ships beside `git`, located from the `git.exe` on
PATH. **Write AC commands in ordinary POSIX shell.** Single quotes, `test`,
`[ … ]`, `$(…)`, pipes, `&&`, `wc`, `grep` and heredocs all work on every
platform this harness runs on.

## Why this page used to say the opposite

The Windows shell was `cmd.exe`, and this page taught the workarounds for it —
`node -e "…"` wrappers, explicit `bash -c '…'` prefixes, and a list of POSIX
constructs to avoid. That guidance made a defect invisible instead of fixing it.
Under `cmd.exe` the single quote is **not** a quote character, so `rg 'token' path`
searched for a literal `'token'`, matched nothing in any tree state, exited 1
with an empty stderr — and `ac-negative-check`, whose whole red rule is
`exit != 0`, stamped it `proven: red`. A criterion that could never go green
entered the plan, and the failure resurfaced at QA looking like the
implementer's fault. Teaching authors to route around a shell is not the same as
giving them one.

## The one residual: backslash paths

`\` is an escape character in a POSIX shell, so `apps\rt\src\x.rs` collapses to
`appsrtsrcx.rs`. **Write paths with forward slashes** — they resolve on Windows
too, and every tool this project uses accepts them.

This failure is loud: the program names the mangled path on stderr and the
`stderr_excerpt` carries it. It degrades to a visible error, never to a silent
red.

## Two verdicts that are NOT failures

- **Spawn failure** — the OS could not start the command at all. Reported
  `skip`, carrying the OS error rather than a guess about its cause.

`skip` never counts as a proof: `ac-negative-check` records it `unproven`, so an
unrunnable criterion is never mistaken for a discriminating one.

## Exit 127 — one code, two correct verdicts

A command the shell cannot FIND is a case apart, and the two readers of that
result answer opposite questions, so they reach opposite verdicts on purpose:

- **`qa-run` fails on it.** A criterion nobody could run must block CLOSE.
  Grading it `skip` would let it ride along beside a passing criterion, because
  an external run tolerates a skip next to a pass — that regression shipped once.
- **`ac-negative-check` records it `unproven`.** Its red rule is exit≠0, so a
  missing program would otherwise be stamped `proven: red` and enter the plan
  carrying a proof about the shell rather than about the behaviour.

The remedy is the same in both readings and neither reading suggests the wrong
one: fix the program name, or install the tool.

## Still worth avoiding

- **Backslash regex escapes inside a `node -e` literal** (`\b`, `\d`, `\w`). The
  escape does not survive the markdown → shell → `node -e` round-trip, and the
  regex silently fails to match even when the output is correct. Use plain
  substrings (`/lsp/i`), character classes (`/[^a-z]lsp[^a-z]/i`), or build the
  `RegExp` from a string inside node. Prefer `rg` with an `Expect:` regex over a
  nested `node -e` in the first place.
- **A lone build-green** (`cargo build`, a bare `grep`). It verifies nothing
  about the behaviour; `analyze-validation` warns on it and `ac-negative-check`
  refuses it for not being able to fail. Only the trailing criterion is exempt.
