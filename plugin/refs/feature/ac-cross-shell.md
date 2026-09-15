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
searched for a literal `'token'`, matched nothing in any tree state and exited 1
with an empty stderr. A criterion that could never go green entered the plan,
and the failure resurfaced at QA looking like the implementer's fault. Teaching
authors to route around a shell is not the same as giving them one.

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

`skip` never counts as a pass, so an unrunnable criterion is never mistaken for
a discriminating one.

## Exit 127 — a command the shell cannot find

**`qa-run` fails on it.** A criterion nobody could run must block CLOSE. Grading
it `skip` would let it ride along beside a passing criterion, because an
external run tolerates a skip next to a pass — that regression shipped once. The
remedy: fix the program name, or install the tool.

## Still worth avoiding

- **Backslash regex escapes inside a `node -e` literal** (`\b`, `\d`, `\w`). The
  escape does not survive the markdown → shell → `node -e` round-trip, and the
  regex silently fails to match even when the output is correct. Use plain
  substrings (`/lsp/i`), character classes (`/[^a-z]lsp[^a-z]/i`), or build the
  `RegExp` from a string inside node. Prefer `rg` with an `Expect:` regex over a
  nested `node -e` in the first place.
- **A lone build-green** (`cargo build`, a bare `grep`). It verifies nothing
  about the behaviour, and `analyze-validation` warns on it. Only the trailing
  criterion is exempt.
