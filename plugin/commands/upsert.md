---
description: Use when the user runs /mustard:upsert, asks to install, set up, or update Mustard in the current project, or to diagnose the installation — and when any /mustard:* command is blocked because Mustard is not installed (no mustard.json at the project root). The installation door — install, update, doctor.
argument-hint: [--doctor]
---
# /mustard:upsert — the installation door

## Install or update

1. Run `mustard-rt run upsert`. It writes, hidden from the project's git, `.claude/settings.local.json`, `.claude/.gitignore`, `mustard.json`, the session map `session-map.md` and page templates under `.claude/mustard/`, and the agents under `.claude/agents/mustard/`. The local settings allow Mustard's commands, `ArtifactData` (the pages' database) and the folder of the wave copies, and keep the person's rules. Nothing is committed.
2. Relay `created`, `updated`, `preserved` and `migrated`, file by file. The settings, `.claude/.gitignore` and `mustard.json` are the person's: merged, never clobbered. The session map and the agents are Mustard's own text, in `language.text`: every run lays the shipped text down again, so an edited copy comes back in `updated`, and one already equal in `preserved`.
3. `pluginRefresh`: `refreshed` names the new version, which only a Claude Code restart loads; relay `skipped` with its reason.
4. `cleanup` and `cleaned`: what an older Mustard left in the `CLAUDE.md` files and the team's `.claude/settings.json` already left. Relay it file by file, and the Guards rules, now in the pending item `cleaned.pending`, each to become a test or be dropped; a file under `unmarked` was not touched: the person decides.
5. `localFilesFound`, while `mustard.json` lacks `localFiles`: ignored files outside ignored folders, like `.env`, that a wave copy lacks. Show it once, with the prepare command its lockfile or manifest suggests (`npm ci`, `dotnet restore`; none if nothing installs), and record what the person confirms: `mustard-rt run upsert --local-files <a,b> --prepare "<command>"`; an empty value records none.
6. After a first install, say that `mustard.json` takes `git.flow`, `git.protected`, `language.text`, `enabled` (off turns Mustard's hooks off here) and `rtk` (off drops rtk's hook from the local settings on the next upsert).

## Doctor

`mustard-rt run doctor` only reads; each failing check names its fix. `--check <name>` runs one, `--residue` also seeks dead references, `--json` answers in JSON.

## Upkeep

- `mustard-rt run scan` updates the project map from what changed, never writing to git; `--full` also rewrites each subproject's map.
- `mustard-rt run clean` lists the throwaway copies agents left in the temp folder; deletes only when told.

Never hand-edit `.claude/settings.local.json`, `.claude/mustard/` or `.claude/agents/mustard/`: the binary writes them. An unreadable settings file is reported and left untouched.
