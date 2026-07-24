---
id: spec.harness-safety-instruments
---

# The harness kill-switch destroys what it should keep, and two instruments report on sources nothing writes

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

## Context

Mustard installs a settings file in every project it manages. That file carries two very different kinds of content: the harness wiring the tool needs to observe a session, and a safety net the developer relies on — a list of shell commands that must never run, such as recursive deletes, force pushes and hard resets, plus a list of private files that must never be read, such as keys and credentials. The kill-switch exists so a developer can silence the harness for a while without uninstalling it. Today that switch takes the whole settings file out of play at once, which means asking to stop being observed also silently removes the safety net — and, because the harness wiring now arrives from the installed plugin rather than from that file, the switch does not even achieve what it was asked to do. Separately, two reporting commands answer questions about the project's own history by reading a location the tool stopped writing to; both return an empty answer, exit successfully, and give no sign that they read nothing. A verification that reports zero when it means "I could not look" is worse than one that fails, because it is trusted.

## Acceptance Criteria

- **AC-1** — when the kill-switch runs, then the harness stops firing and the safety rules stay in place
  Command: `cargo test -p mustard-rt unhook_disables_hooks_without_dropping_permissions -- --exact --nocapture` Expect: `1 passed`
- **AC-2** — when a hook event is added or removed from what ships, then the health check follows it without anyone editing a second list
  Command: `cargo test -p mustard-rt known_events_match_shipped_hooks -- --exact --nocapture` Expect: `1 passed`
- **AC-3** — when the reporting command is asked about a project that has history, then it reports that history instead of zero
  Command: `cargo test -p mustard-rt metrics_collect_reports_specs_from_events -- --exact --nocapture` Expect: `1 passed`
- **AC-4** — when the shipped settings template is inspected, then it grants no permission the platform refuses to honour
  Command: `rg --files-without-match "Edit\(\*?\*?/?\.claude" packages/core/templates/settings.json` Expect: `settings.json`
- **AC-5** — the workspace builds and its suite stays green
  Command: `cargo build --workspace && cargo test --workspace` Expect: `test result: ok`
- **AC-6** — when a counter cannot be derived from any readable source, then the report says so instead of publishing a number that means nothing
  Command: `cargo test -p mustard-rt metrics_counters_declare_unknown_when_underived -- --exact --nocapture` Expect: `1 passed`
- **AC-7** — when a spec is drafted into a directory that already holds its event log, then the draft proceeds without an overwrite flag
  Command: `cargo test -p mustard-rt spec_draft_accepts_an_events_only_directory -- --exact --nocapture` Expect: `1 passed`
- **AC-8** — when the drafted skeleton is validated, then its context section carries no file path and no bullet list
  Command: `cargo test -p mustard-rt drafted_context_is_prose_only -- --exact --nocapture` Expect: `1 passed`
- **AC-9** — when the approval gate declines to record an answer, then it states which condition failed
  Command: `cargo test -p mustard-rt approval_refusal_names_the_unmet_condition -- --exact --nocapture` Expect: `1 passed`
- **AC-10** — when the glossary scores an intent, then a single word stem is counted once, not once per inflection
  Command: `cargo test -p mustard-rt glossary_terms_collapse_inflections -- --exact --nocapture` Expect: `1 passed`

## Root cause

Four independent defects, one shared shape: a component acting on a source that no longer matches reality.

1. **The kill-switch renames the settings file.** `apps/rt/src/commands/maint/unhook.rs:182` moves `settings.json` to `settings.json.disabled-<timestamp>`. That file holds `permissions.deny` (55 rules), `permissions.allow`, `statusLine` and the telemetry `env` block. It holds no `hooks` key — `packages/core/templates/settings.json` never wrote one, because Mustard's hooks arrive through `plugin/hooks/hooks.json`. Official documentation: *"Plugin hooks are disabled by `disableAllHooks`"* and *"There is no built-in way to disable only a plugin's hooks"*. So the rename removes the safety net and leaves the hooks running. With `--scope all --confirm` (`unhook.rs:114`) it reaches `~/.claude/settings.json`, doing the same to every session on the machine.

2. **The shipped allowlist grants a protected path.** `packages/core/templates/settings.json:30-31` lists `Edit(.claude/**)` and `Edit(**/.claude/**)`. The permission-modes reference names that exact string as its example of a rule with no effect: *"`permissions.allow` rules in settings files do not pre-approve protected-path writes… an entry such as `Edit(.claude/**)`… does not change the per-mode outcome."* A bare `Edit` on line 29 already covers the intent, so the two entries are redundant on a second, independent count.

3. **The health check validates against a hand-kept list.** `apps/rt/src/commands/doctor/doctor.rs:95` declares eight event names. `plugin/hooks/hooks.json` registers nine. The two disagree in both directions: the list carries `PreCompact`, which is never registered, and omits `Stop` and `WorktreeCreate`, which are. Thirty lines below, `known_run_subcommands` solves the identical drift by deriving from the live command tree — the fix pattern already lives in the same file.

4. **The reporting command reads a directory nothing writes.** `apps/rt/src/commands/economy/metrics.rs:205` calls `collect_specs`, which reads `pipeline_states_dir()` — `.claude/.pipeline-states`. That directory does not exist, and three sites say why: `apps/rt/src/shared/context.rs:290` ("no longer written"), `apps/rt/src/hooks/write/scope_guard.rs:152` ("not written"), `apps/rt/tests/pipeline_state_projection_test.rs:16` ("the old … path"). The read fails, an empty list is returned, and zero is published. Measured on this repository the same moment: the command reports `tracked: 0`; the live event projection reports 21 specs, 52 agents and 3,048 tool calls.

## Files

- `apps/rt/src/commands/maint/unhook.rs` — replace the whole-file rename with a surgical `disableAllHooks: true` write; keep the volatile-state wipe and the report shape.
- `apps/rt/src/commands/maint/rehook.rs` — mirror the inverse: remove the key. Keep reading a legacy `settings.json.disabled-*` snapshot so an already-unhooked project can still be restored.
- `packages/core/templates/settings.json` — drop the two protected-path allow entries.
- `.claude/settings.json` — same removal in this project's own copy.
- `apps/rt/src/commands/doctor/doctor.rs` — derive the event list from the shipped hooks manifest instead of declaring it.
- `apps/rt/src/commands/economy/metrics.rs` — project spec state from the event log, reusing the existing projection; when no source is readable, say so instead of publishing zero.

## Boundaries

IN: the kill-switch pair, the shipped and local settings templates, the health-check event list, the pipeline half of the reporting command, and a regression test per defect. Extended after approval to cover every defect this unit surfaced while working (see `## Scope extension`): the vacuous first-pass counter, the pull-request under-count, the drafter refusing a directory holding only its own event log, the drafter writing paths into a prose-only section, the approval gate refusing without saying why, and the glossary counting one word stem once per inflection.

OUT: the retrieval, wave, spec-graph and scan families. Any change to which hooks ship or what the safety rules contain. Any change to what the approval gate accepts — only what it explains.

## Scope extension

The original boundary pushed four of these to "a separate unit". That was the wrong call and it was reversed while the unit was still open: a defect found with the file already in hand and the diagnosis fresh costs less to fix now than to rediscover later, and a deferred defect reads as progress without being progress. Each addition below is a defect this unit surfaced by running the tool, not a new idea.

1. The first-pass counter reports 100% on every project because the retry count it divides by is never filled — a number published without being measured, the same defect as the dead-directory read, one line away in the same file.
2. The pull-request report under-counts what opened and reports nothing merged over a period with nine merges.
3. Creating the work unit writes its event log into the spec directory; drafting the spec then refuses because that directory exists. Two steps of one sequence, the first blocking the second.
4. The drafter writes anchor file paths and a bullet list into the context section, which the shipped spec-language law forbids in that section.
5. The approval gate declines to record an answer whose option label lacks a specific word stem, and says nothing — the author of the question is never told the condition exists.
6. The glossary scores one word stem once per inflection, reporting eighteen open terms where there are about nine.

<!-- signals: layers,files -->