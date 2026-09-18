---
description: Use when the user runs /mustard:upsert, asks to install, set up, or update Mustard in the current project, or to diagnose the installation — and when any /mustard:* command is blocked because Mustard is not installed (no mustard.json at the project root). The installation door — install, update, doctor.
argument-hint: [--doctor]
---
# /mustard:upsert — the installation door

## Install or update

1. Run `mustard-rt run upsert`. It writes `.claude/settings.local.json`, `.claude/.gitignore`, `mustard.json`, the session map `session-map.md` under `.claude/mustard/`, the two page templates under `.claude/mustard/pages/` and the three agents under `.claude/agents/mustard/`, all hidden from the project's git. The local settings allow Mustard's own commands and `ArtifactData`, the tool that writes the pages' database, and keep every rule the person already has. An older install's map, `mapa-inicio-sessao.md`, leaves the disk, and `mustard.json` then declares the new name. Nothing is committed.
2. Relay `created`, `updated`, `preserved` and `migrated`, file by file. The settings file, `.claude/.gitignore` and `mustard.json` are the person's: merged, never clobbered. The session map and the agents are Mustard's own text, in the language of `language.text`: every run lays the shipped text down again, so an edited copy comes back in `updated`, and one already equal comes back in `preserved`.
3. `pluginRefresh`: `refreshed` names the new version when the field is there, and this session keeps the plugin it loaded until Claude Code restarts; `skipped` is relayed with its reason.
4. `cleanup` and `cleaned`: what an older Mustard left in the `CLAUDE.md` files and in the team's `.claude/settings.json` already left, in this same call. Relay what left, file by file, and the Guards that became lessons; a file listed under `unmarked` was not touched, and is the person's to decide. The commit is the person's.
5. After a first install, say that `mustard.json` takes `git.flow`, `git.protected`, `language.text`, `enabled` (off turns every Mustard hook off here) and `rtk` (off takes rtk's hook out of the local settings on the next upsert).

## Doctor

`mustard-rt run doctor` only reads, and each failing check names its fix. `--check <name>` runs one check, `--residue` also looks for dead references, and `--json` answers in JSON.

## Upkeep

- `mustard-rt run scan` updates the project map, reading only what changed; it never writes to git, and `--full` also rewrites each subproject's map.
- `mustard-rt run clean` lists the throwaway copies agents left in the temp folder, and deletes nothing unless told to.

Never edit `.claude/settings.local.json`, `.claude/mustard/` or `.claude/agents/mustard/` by hand: the binary writes them. An unreadable settings file is reported and left untouched.
