---
description: Install, update or diagnose Mustard in this project; also use when /mustard:* is blocked because mustard.json is absent.
argument-hint: [--doctor]
---
# /mustard:upsert — the installation door

## Install or update

1. Run `mustard-rt run upsert`. It writes `mustard.json`, `.claude/settings.local.json`, `.claude/.gitignore`, `.claude/mustard/session-map.md`, `.claude/agents/mustard/wave.md` and `.claude/agents/mustard/review.md`, hidden from this clone's git. Nothing is committed. External pages require explicit publication; historical resources remain readable.
2. Relay `created`, `updated`, `preserved`, `migrated`. Settings and config preserve personal choices. Session-map and agents always match the shipped text in `language.text`; unchanged copies are `preserved`.

3. Relay `pluginRefresh` version/restart or skip reason, and `codeToolWarnings` commands/failures/timeouts.
4. Relay `cleanup`/`cleaned` by file: obsolete `CLAUDE.md`/team settings entries removed, `cleaned.swapped` deny rules to commit, `cleaned.pending` Guards to test or drop. `unmarked` files were untouched; the person decides.
5. `localFilesFound`, while `mustard.json` lacks `localFiles`: ignored files outside ignored folders, like `.env`, that a wave copy lacks. Show it once, with the prepare command its lockfile or manifest suggests (`npm ci`, `dotnet restore`; none if nothing installs), and record what the person confirms: `mustard-rt run upsert --local-files <a,b> --prepare "<command>"`; an empty value records none.
6. After a first install, say that `mustard.json` takes `git.flow`, `git.protected`, `language.text`, `language.code` (the names in the code; English without it), `enabled` (off turns Mustard's hooks off here) and `rtk` (off drops rtk's hook from the local settings on the next upsert).

## Doctor

`mustard-rt run doctor` only reads; each failing check names its fix. `--check <name>` runs one, `--residue` also seeks dead references, `--json` answers in JSON.

`mustard-rt run config-read --file mustard.json --reader grep --args-json '["-n","-E","flow|protected"]'` filters masked keys. Supported shell reads rewrite automatically; other readers are refused.

## Upkeep

- `mustard-rt run scan` updates changed files in the map; `--full` rebuilds it. Neither writes to git.
- `mustard-rt run clean` lists the throwaway copies agents left in the temp folder; deletes only when told.

The binary writes `.claude/settings.local.json`, `.claude/mustard/` and `.claude/agents/mustard/`. Never hand-edit them. Unreadable settings stay untouched.
