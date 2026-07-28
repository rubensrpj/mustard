---
id: spec.make-self-invocation-guard-decide
---

# Make the self-invocation guard decide by path instead of by crate name: refuse a QA criterion only when the command would overwrite the very binary running it, so a spec whose criteria test the harness itself can be verified and closed

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Make the self-invocation guard decide by path instead of by crate name: refuse a QA criterion only when the command would overwrite the very binary running it, so a spec whose criteria test the harness itself can be verified and closed.

Why now. The spec that just shipped was reviewed by two adversarial readers,
verified criterion by criterion, and came back green on three operating systems.
It still could not be closed. Twelve of its thirteen criteria were refused as
self-invocation, one refusal is enough to deny the whole run, and the close only
completes on a pass. The work was done and the record could not say so.

The refusal protects against a real hazard — a command that overwrites the
executable running it — but it asks the wrong question to detect that hazard. It
matches the command text against two hardcoded crate names, so it refuses by
spelling rather than by fact.

An experiment settled it. With the build cache invalidated, the QA was re-run:
the build criterion went from 763ms to 9626ms — a genuine recompilation — and
came back green, with the installed binary executing the whole time. The two
files are simply different: the process runs from one path, cargo writes another.

## Users/Stakeholders

Anyone whose acceptance criteria test the tool that runs them — today that is
this project itself, whose specs cannot close by the official route. Also the
reader of a closed spec, for whom "QA skipped" and "QA passed" must not be the
only two outcomes available for work that was genuinely verified.

## Success Metric

A criterion is refused when, and only when, running it would overwrite the file
this process is executing from. Concretely: the spec that shipped before this one
could be re-run through QA and reach a recorded pass, and a harness genuinely
started from its own build directory is still refused.

## Non-Goals

Loosening the guard into always-allow stays out: running from the build directory
is a real configuration, and trading a false refusal for a genuine failure is not
a fix. Recording a pass from evidence taken outside the process stays out too —
that would be a door for asserting a verdict nobody's run produced, which is the
habit the previous spec removed. The remaining findings from that spec (the
record that loses memories, the missing door to add a criterion) stay out: they
are their own work, argued in their own specs.

## Acceptance Criteria

Each criterion names the test that proves it and demands a non-zero pass count:
a filter matching nothing exits 0 and prints "0 passed", and `[1-9][0-9]*` is
what refuses to read that as success.

- **AC-1** — when the command under judgement would rebuild a crate but writes to
  a file other than the one this process is executing from, then it is run
  instead of being refused as self-invocation
  Command: `cargo test -p mustard-rt guard_allows_when_build_target_is_not_the_running_binary`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-2** — when the running binary IS the file the command would overwrite,
  then the refusal stands and names that file as the reason
  Command: `cargo test -p mustard-rt guard_refuses_when_build_target_is_the_running_binary`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-3** — the project build passes green
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — Ask the guard the path question: resolve the running executable and the
      build target the command would write, and decide by comparing them.
- [ ] T2 — Keep the refusal honest when the paths do coincide, naming the file
      rather than the crate as the reason.
- [ ] T3 — Re-run the previous spec through QA and record whether it now reaches a
      pass — the measure this spec exists for.

## Definitions

- **self-invocation** — a QA command that would rebuild the very binary currently running the QA — the reason qa-run refuses to execute it
- **running binary** — the executable this process IS, answered by std::env::current_exe — on this machine ~/.cargo/bin/mustard-rt
- **build target** — the file cargo writes when it compiles the crate — target/debug/mustard-rt.exe, a different file from the running binary
- **text match** — the current guard's question: does the command string mention one of two hardcoded crate names
- **path match** — the question the guard should ask instead: would this command overwrite the exact file I am executing from

## Decisions

- base is dev with PR #121 already merged
  Reason: the confirmation pass this spec unblocks was delivered there; cutting from an unmerged branch would stack work on work and collide in qa_run/runner.rs
- the guard decides by comparing paths, not by matching crate names in the command text
  Reason: an experiment settled it: with the build cache invalidated, cargo rebuilt target/debug/mustard-rt.exe from inside a running mustard-rt and came back green — the conflict the text match protects against does not exist when the two paths differ
- no second fix for the unreachable confirmation pass
  Reason: it is a consequence, not a separate defect: once QA stops skipping, close completes, and close is what takes the confirmation
- the guard must still refuse when the paths DO coincide
  Reason: running from target/debug is a real configuration; loosening the guard into always-allow would trade a false refusal for a genuine failure

## Evidence

- The self-invocation guard decides by matching the command TEXT against two hardcoded crate names, so every criterion spelled `cargo test -p mustard-rt` is refused regardless of where the running binary actually lives.
  Evidence: `apps/rt/src/commands/review/qa_run/runner.rs:150`
- SELF_CRATES is a fixed two-name list, which means the guard's accuracy depends on how a criterion is spelled rather than on any fact about the machine.
  Evidence: `apps/rt/src/commands/review/qa_run/runner.rs:32`
- overall_verdict returns a non-pass whenever any single criterion skipped, so one refused criterion is enough to deny the whole run.
  Evidence: `apps/rt/src/commands/review/qa_run/mod.rs:585`
- The close only completes on a QA pass, and MUSTARD_QA_GATE_MODE does not reach this: that variable governs the write gate, not the close composition. There is no switch, and there should not be one.
  Evidence: `apps/rt/src/commands/pipeline/close_pipeline.rs:1`
- EXPERIMENT that settles the hypothesis: with the build cache invalidated by touching apps/rt/src/main.rs, qa-run was re-run and AC-9 (cargo build --workspace) went from 763ms to 9626ms — a real recompilation — and came back PASS, with the installed mustard-rt executing the whole time. The rebuild of the crate succeeds from inside a running instance.
  Evidence: `.claude/spec/make-harness-stop-asserting-what/qa/report.md:1`
- The two files are distinct: the process runs from /c/Users/ruben/.cargo/bin/mustard-rt while cargo writes target/debug/mustard-rt.exe, so no Windows executable lock can arise between them.
  Evidence: `apps/rt/src/commands/review/qa_run/runner.rs:150`
- Consequence measured on the previous spec: 12 of its 13 criteria were refused as self-invocation and the close was denied twice, leaving a fully reviewed and CI-green spec stuck at QaPending with no way to record the pass its evidence had earned.
  Evidence: `.claude/spec/make-harness-stop-asserting-what/qa/report.md:1`