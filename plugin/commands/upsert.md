---
description: Install, update or diagnose Mustard; use if /mustard:* is blocked because mustard.json is absent.
argument-hint: [--doctor]
---
# /mustard:upsert — the installation door

## Install or update

1. Run `mustard-rt run upsert`. It updates local config, hooks, `session-map.md` and `agents/mustard`, without commits. Publish pages only on request; retain historical resources.
2. Relay `created`, `updated`, `preserved`, `migrated`. Settings and config preserve personal choices. For session-map and agents, every run lays the shipped text down again in `language.text`; equal copies are `preserved`.

3. Relay `installation.complete` and failures; false is incomplete. Index precedes language tools. Relay `pluginRefresh` version/restart or skip and `codeToolWarnings` with commands.
4. Relay `cleanup`/`cleaned` by file: obsolete `CLAUDE.md`/team settings entries removed, `cleaned.swapped` deny rules to commit, `cleaned.pending` Guards to test or drop. `unmarked` files were untouched; the person decides.
5. `localFilesFound`, while `mustard.json` lacks `localFiles`: ignored files outside ignored folders, like `.env`, that a wave copy lacks. Show it once, with the prepare command its lockfile or manifest suggests (`npm ci`, `dotnet restore`; none if nothing installs), and record what the person confirms: `mustard-rt run upsert --local-files <a,b> --prepare "<command>"`; an empty value records none.
6. After a first install, say that `mustard.json` takes `git.flow`, `git.protected`, `language.text`, `language.code` (the names in the code; English without it), `enabled` (off turns Mustard's hooks off here) and `rtk` (off drops rtk's hook from the local settings on the next upsert).

## Doctor

`mustard-rt run doctor` only reads and names fixes. `--check <name>` runs one check, `--residue` seeks dead references, `--json` answers in JSON.

`mustard-rt run config-read --file mustard.json --reader grep --args-json '["-n","-E","flow|protected"]'` filters masked keys. Every shell read of the key file is refused; a supported read's refusal ends with the masked command, ready to run.

## Upkeep

- `mustard-rt run scan` updates the map; `--full` refreshes scan-map texts. No git writes.
- `mustard-rt run clean` lists the throwaway copies agents left in the temp folder; deletes only when told.

The binary writes `.claude/settings.local.json`, `.claude/mustard/` and `.claude/agents/mustard/`. Never hand-edit them. Unreadable settings stay untouched.
