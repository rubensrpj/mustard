# Acceptance Criteria — Cross-Shell Pattern Reference

> Detail for `/feature` spec authoring: how to write AC commands that run correctly on both Windows (cmd.exe) and Unix shells.

### Acceptance Criteria — Cross-Shell Pattern

`mustard-rt run qa-run` executes each AC command via `execSync` with `shell: true`. On Windows, that shell is `cmd.exe`, which does NOT understand bash syntax (`for`, `test $? -eq 0`, `[ $n -gt 200 ]`, single-quoted heredocs). To keep specs portable, write AC commands in one of these forms:

- **Single command, exit code is the verdict:** `mustard-rt run verify-pipeline` — execSync throws on non-zero, passes on 0. No wrapper needed.
- **Multi-step assertion:** wrap the whole logic in `node -e "..."`:
  ```
  node -e "const fs=require('fs');for(const f of ['a.md','b.md']){if(fs.readFileSync(f,'utf8').split('\n').length>200)process.exit(1)}"
  ```
- **Pipes/grep needed:** keep the pipe inside node via `execSync` instead of relying on the shell pipe — `node -e "const o=require('child_process').execSync('mycmd',{encoding:'utf8',stdio:['ignore','pipe','ignore']});if(!/needle/.test(o))process.exit(1)"`. The shell-level `|` and stdio redirects (`2>&1`) on cmd.exe combined with `node -e "..."` quoting often mangle nested quotes; pulling the exec inside node sidesteps the problem entirely.
- **Need a real shell (heredocs, complex pipelines):** prefix with `bash -c '...'` so cmd.exe spawns bash explicitly.
- **Avoid backslash regex escapes (`\b`, `\d`, `\w`) in inline regex literals:** the escape does not survive the markdown → cmd.exe → `node -e` round-trip; the regex silently fails to match even when the output is correct. Use plain substring (`/lsp/i`), character classes (`/[^a-z]lsp[^a-z]/i`), or build the `RegExp` from a string inside node where parsing happens once.
- **Avoid:** raw `for f in ...; do ...; done`, `test $? -eq 0`, `$(...)` substitution, `[ ... ]` brackets — all bash-only and silently fail on Windows with cryptic errors like "f foi inesperado neste momento".
