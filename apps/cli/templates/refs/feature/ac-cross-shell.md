# Acceptance Criteria — Cross-Shell Pattern Reference

> Detail for `/feature` spec authoring: how to write AC commands that run correctly on both Windows (cmd.exe) and Unix shells.

### Acceptance Criteria — Cross-Shell Pattern

`qa-run.js` executes each AC command via `execSync` with `shell: true`. On Windows, that shell is `cmd.exe`, which does NOT understand bash syntax (`for`, `test $? -eq 0`, `[ $n -gt 200 ]`, single-quoted heredocs). To keep specs portable, write AC commands in one of these forms:

- **Single command, exit code is the verdict:** `mustard-rt run skills validate --json` — execSync throws on non-zero, passes on 0. No wrapper needed.
- **Multi-step assertion:** wrap the whole logic in `node -e "..."`:
  ```
  node -e "const fs=require('fs');for(const f of ['a.md','b.md']){if(fs.readFileSync(f,'utf8').split('\n').length>200)process.exit(1)}"
  ```
- **Need a real shell (grep, pipes):** prefix with `bash -c '...'` so cmd.exe spawns bash explicitly.
- **Avoid:** raw `for f in ...; do ...; done`, `test $? -eq 0`, `$(...)` substitution, `[ ... ]` brackets — all bash-only and silently fail on Windows with cryptic errors like "f foi inesperado neste momento".
