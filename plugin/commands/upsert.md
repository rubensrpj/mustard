---
description: Install, update or diagnose Mustard; use if /mustard:* is blocked because mustard.json is absent.
argument-hint: [--doctor]
---
# /mustard:upsert — the installation door

## Install or update

1. Run `mustard-rt run upsert` (never commits).
2. Relay `created`, `updated`, `preserved`, `migrated`. Settings and config preserve personal choices. For `session-map.md` and agents, every run lays the shipped text down again in `language.text`; equal copies are `preserved`.
3. Relay `installation.complete` (false: incomplete), failures, `pluginRefresh` version/restart or skip and `codeToolWarnings` with commands: hints the person may run, never counted against `complete`.
4. Relay `cleanup`/`cleaned` by file: obsolete `CLAUDE.md` entries removed, `cleaned.pending` Guards to test or drop. `cleanup.team` (team settings lines and deny rules to swap) and `unmarked` files are untouched; the person decides.
5. `localFilesFound`, while `mustard.json` lacks `localFiles`: ignored files outside ignored folders (`.env`) a wave copy lacks. `prepareSuggested`, while it lacks `prepareCommand`: the prepare line its lock file asks for, with that file as trigger; nothing ran. Show both once and record the reply: `mustard-rt run upsert --local-files <a,b> --prepare "<command>" --prepare-files <a,b>`; empty records none.
6. After a first install, say `mustard.json` takes `git.flow`, `git.protected`, `language.text`, `language.code` (names in code; English without it), `enabled` (off disables Mustard's hooks here) and `rtk` (off drops its local hook on the next upsert).
7. Say `proofTimeoutSecs` caps a criterion proof in seconds; `MUSTARD_QA_AC_TIMEOUT_SECS` wins.

## Doctor

`mustard-rt run doctor` only reads and names fixes. `--check <name>` runs one check, `--residue` seeks dead references, `--json` prints JSON.

`mustard-rt run config-read --file mustard.json --reader grep --args-json '["-n","-E","flow|protected"]'` filters masked keys; a shell read of the file is refused with it.

## Upkeep

- `mustard-rt run scan` updates the map; `--full` refreshes scan-map texts. No git writes.
- `mustard-rt run clean` lists agents' temp copies, ended specs' copies and branches, merged branches; never a base, protected, current or unmerged branch. `--apply` deletes them, `--path <path>` one temp item.

The binary writes `.claude/settings.local.json`, `.claude/mustard/` and `.claude/agents/mustard/`. Never hand-edit them.
