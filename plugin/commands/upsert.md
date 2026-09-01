---
description: Use when the user runs /upsert, asks to install, set up, or update Mustard in the current project, to disable or re-enable the harness hooks, or to diagnose the installation — and when any /mustard:* command is blocked because Mustard is not installed (no mustard.json at the project root). The installation door — install, update, off, on, doctor.
argument-hint: [--off | --on | --doctor] [--scope this|monorepo|all] [--confirm]
---
<!-- mustard:generated -->
# /upsert — The Installation Door

## Trigger

`/mustard:upsert [--off | --on | --doctor] [--scope this|monorepo|all] [--confirm]`

## Description

One subject, one door: **the state of Mustard's installation in this project**. With no flag it installs or updates. The three flags are the other three questions you can ask about that same state — turn it off, turn it back on, and is it healthy. They were three separate doors once; splitting one subject across four commands was division without a reason.

| Flag | What it does |
|------|--------------|
| *(none)* | Install or update. Seeds `.claude/settings.local.json` (the install is always private-mode, so the hook wiring lands in your local settings file, never the shared `.claude/settings.json`), the injectable instruction files under `.claude/mustard/`, `.claude/.gitignore`, and the project-root `mustard.json`. Idempotent and merge-only — a file you already have is preserved; only what is missing is created. The one exception is `.claude/mustard/orchestrator.md` and `.claude/mustard/dispatch.md`: those are the harness's own rules, not your configuration, so every run lays the shipped text down again, and a copy that had diverged is reported as updated, never as preserved. A legacy Mustard-planted `.claude/CLAUDE.md` (and the old import/breadcrumb lines in the root `CLAUDE.md`) is migrated away in the same pass. Until this has run, every other `/mustard:*` command is disabled. |
| `--off` | Harness kill-switch. Sets `"disableAllHooks": true` in `.claude/settings.json` and wipes volatile state — `.agent-state/` and `.cluster-cache.json`, and nothing else. **Worktrees are NOT touched.** The units under `.claude/worktrees/` hold uncommitted work, and silencing the harness is not a reason to destroy it; `mustard-rt run worktree-gc` is the only door that reaps them, and it is dry-run until `--apply`. The rest of the settings file — `permissions.deny`, `permissions.allow`, `statusLine`, `env` — is left untouched, so silencing the harness never removes the safety rules. Plugin-provided hooks are covered too. Reversible with `--on`. |
| `--on` | Reverses `--off`. For each `.claude/` in scope, removes the `"disableAllHooks"` key from the live `settings.json`. With no live file the legacy path still applies: the most recent `settings.json.disabled*` snapshot is renamed back, so a project unhooked by an older build still recovers. Volatile state directories are **not** recreated — the runtime regenerates them on the next run. |
| `--doctor` | Read-only installation health report. Never writes. |

Use `--off` when: harness misbehaviour and you want a clean baseline; handing the project to someone without `mustard-rt`; a sensitive operation you want without hook overhead.

## Action

### Install or update (no flag)

```bash
mustard-rt run upsert
```

Print nothing raw — read the JSON report and relay it in clear language:

1. `installedBefore: false` → this was a **first install**; `true` → an update over an existing installation.
2. Walk the four lists — `created`, `updated`, `preserved`, `migrated` — and say plainly what each file got. `.claude/mustard/orchestrator.md` and `.claude/mustard/dispatch.md` are never the operator's to keep: every run lays the shipped text down again, so a copy that had diverged comes back in `updated` (e.g. "your router files were rewritten from the shipped rules — if you had edited a copy, that edit is gone"), and a copy that already matched the shipped text comes back in `preserved` because there was nothing left to write. Every OTHER name in `preserved` is a file you own — the settings file, `.claude/.gitignore`, `mustard.json` — merged, never clobbered.
3. `pluginRefresh` — the run's last step updates **the plugin itself**, so there is no "now go to /plugin and reload" left over.
   - `state: "refreshed"` → name the resulting `version` **when the field is there** (it is optional: the refresh ran, but the registry could not be read back — say the refresh succeeded and the version could not be confirmed, never invent one), and then say the other half plainly: **this session keeps running the plugin it loaded at start; only restarting Claude Code picks up the new one.** Never imply the new version is already active here — the host loads a plugin once per session and nothing inside the session changes that.
   - `state: "skipped"` → relay the `skipped` reason as it comes. The project install still succeeded; only the plugin update did not run.
4. After a **first install**, add: the defaults work out of the box, and nothing needs a branch declared — bases come from git itself, and protection from `origin/HEAD`. `git.flow` (an OPTIONAL promotion map, which also pre-selects where a base picker opens), `git.protected` and `specLang` can be adjusted anytime by editing `mustard.json` at the project root.
5. Next step: describe the work you want done. The router opens the pipeline, and the base gate mines the repo census for you on the way in — there is no separate mapping step to run.

### Off / on

```bash
mustard-rt run unhook --scope this
mustard-rt run rehook --scope this
```

Print stdout verbatim. The `unhook` report's `revert_with` field tells the user exactly how to restore.

| Scope | What it touches |
|-------|-----------------|
| `this` | Only `<repo>/.claude/settings.json` (default) |
| `monorepo` | `<repo>/.claude/` + every `apps/*/.claude/` + `packages/*/.claude/` |
| `all` | `monorepo` plus the user-global `~/.claude/settings.json` (requires `--confirm`) |

Without `--confirm`, `all`-scope reports the global target as `state: "skipped"` and leaves it alone.

Report each entry's `state`. For `--off`: `disabled` / `missing` / `skipped` / `error`. For `--on`: `restored` (the key removed, or a legacy `settings.json.disabled-<ts>` renamed back) / `already-active` (the file carries no `disableAllHooks` — hooks were never off) / `no-snapshot` (`.claude/` exists, no live `settings.json` and no snapshot) / `missing` / `skipped` / `error` (unreadable or unparseable settings, or the rename failed — the OS message is in the report and the file was left untouched).

### Doctor

```bash
mustard-rt run doctor
```

Read-only. Relay the report as it comes; a failing check names its own remediation. `--residue` audits leftover state and `--check <name>` narrows to one check.

## INVIOLABLE RULES

- Never create or edit `.claude/settings.json`, `.claude/mustard/*.md`, `.claude/.gitignore` or `mustard.json` by hand, and never rename a `settings.json.disabled*` snapshot yourself — the binary is the only writer. An unparseable settings file is reported as `error` and left byte-for-byte untouched; that file is the safety net, so a blind overwrite is the one outcome worse than not acting.
- Relay every list and every per-entry `state` from the report; if the JSON carries an `error` field, surface it verbatim — never mask it.
- Never say the refreshed plugin is in effect for this session, and never offer to reload it for the user. The update lands on disk; applying it is a restart, which nothing inside a session can perform. A `pluginRefresh` that came back `skipped` is reported, not retried by hand.
- Never pass `--confirm` automatically — the user types it for `--scope all`.
- After `--off`, name `--on --scope <same>` as the reversal. If every `--on` entry comes back `already-active`, say so: the user may have meant `--off`.
