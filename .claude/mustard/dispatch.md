# Dispatch Rules

The router's second half: the question that OPENS a work unit, the gate that emits it, and the naming that comes out of it. `orchestrator.md` § Intent Routing classifies; this file is what you run once the classification is made. Why the router ships as two files: `refs/mustard/router-rationale.md`.

## Dispatch

**Every request that edits a file opens a unit, and a unit opens with ONE question.** Size is not the test: a one-line change lands on a branch the same way a rewrite does, and "too small to ask about" is how work ends up committed to whatever the checkout was standing on. The only requests that open no unit are the ones that WRITE NOTHING — a question answered by reading, a status command, a digest.

**The question is asked against a REAL list.** Get the candidates from git first. `mustard-rt run base-candidates` fetches every branch on `origin`, newest first, each marked `protected` (a direct commit is refused there) and `preselected` (`git.flow` names it; where the cursor opens). `measured:false` means git could not be asked: ask without a menu rather than showing an empty one as complete.

Ask the rows together, **`sai de` FIRST**. The operator settles where the unit STARTS before what it is CALLED. A type shown above the base makes the base read as the type's consequence.

```
Li seu pedido como: correção de defeito

  sai de:  [dev]   main   release/2026-Q3   squad-b/integration   …ou o seu
  tipo:    [fix]   feature   hotfix   chore   …ou o seu
  branch:  [fix/o-botao-de-login-quebrou]   …ou corrija o nome
```

**The rows are INDEPENDENT fields; asking them together NEVER means pairing them.** Never render combined options like `fix saindo de dev` / `hotfix saindo de main`: a pair-list hands back the cartesian product of two choices, and the operator who wants `hotfix` cut from the ordinary base finds no row to pick. **A question surface takes at most 4 options per field, plus the free one.** `sai de` shows `preselected` plus the newest of the catalogue; any other branch is typed in full. On `tipo`, `hotfix` is PINNED and is never the suggestion dropped to fit the ceiling, since that row exists precisely so an emergency can be named.

The pre-marked `tipo` is the reading you made in § Intent Routing (Bugfix to `fix`, else `feature`). **The type is an OPEN label:** any token that can be a git ref segment is accepted, and it decides nothing beyond the prefix — `hotfix/` no longer moves the base, because the base is chosen outright. `sai de` is skipped when the repository has ONE branch. Ask ONCE per unit; the answer is stored nowhere.

**`branch` is a CORRECTABLE field, not a notice.** It shows `{tipo}/{name derived from the request}` and an Enter accepts it. The operator who rewrites that name on purpose is the one person who knows what the unit should be called, and that correction wins. Editing the row edits `tipo` + name in ONE string: split at the first `/`, the head replaces the `tipo` answer, the tail is the corrected name. There is no third, free-standing name: a branch field allowed to disagree with `tipo` would resurrect the two-names defect. An untouched row is silence, and silence still means derived. Then:

```
mustard-rt run emit-pipeline --kind pipeline.kind --spec {slug} --intent "<short request>" --type {tipo} --base {base} --payload '{"kind":"<feature|bugfix|task|tactical-fix>","scope":"<light|full|lean>"}'
# …and ONLY when the operator corrected the `branch` row, append: --unit-name {name}
```

`--type` is the `tipo` answer; `--base` is the `sai de` one, and omitting it takes the primary base. **`--type` (the BRANCH) and the payload `kind` (the FLOW) are different vocabularies, both needed.** A `bugfix` flow on a `fix/` branch is the ordinary pairing, and neither goes in `--kind`, which names the EVENT. Kind to type, no hole: `feature`,`task` give `feature`; `bugfix`,`tactical-fix` give `fix`. `hotfix` only off the ordinary base, and that fork is YOURS. Omitted `--type` is no silent default: on the ordinary base the gate derives it from the payload `kind` and echoes `type`+`typeFrom`; elsewhere, or with no routing kind, it REFUSES, because a silent default may not name a durable artefact. A `--base` the remote lacks is refused, LISTING the branches that exist.

**`--unit-name` is the operator's correction, and the ONLY signal that outranks the name derived from `--intent`.** Pass it when, and only when, the `branch` row came back edited. `--spec` remains a caller's guess and still loses. `nameFrom` reports which side named the unit: `derived-from-intent` or `operator`.

That emit IS the **base gate**, the one check before ANALYZE, and every pipeline-opening path crosses it. A read-only answer opens no pipeline, so it never emits. It refuses with exit 2, before anything is written, when the base trails `origin`. Each refusal names the command that resolves it: run it and re-dispatch, never route around it. A stale census is re-mined right there, so `/scan` is not a step you run.

**That call is also where the unit is NAMED, and the name it returns is the only one.** The gate derives the canonical slug from `--intent`, or canonicalises `--unit-name` through that same derivation, so a correction still yields ONE spelling. It echoes the winner as `spec`, with `renamedFrom` when the `--spec` you passed was not it; that flag is a hint and never decides. Carry that `spec` value into every later step (`spec-draft --slug`, `--spec {slug}`, the spec directory), never the string you typed. `--intent` + `--type` compute the unit's `{kind}/{slug}` branch, echoed as `branch`, and fix the `/git` PR target: the base the unit was cut from, recorded at the cut, never re-derived from the prefix. **The branch IS the isolation, and it is cut at APPROVAL.** `spec-draft` checks `{kind}/{slug}` out in the MAIN checkout, so the whole unit is authored ON it: `spec.md`, the waves, the ceremony and the code alike. There is no `.claude/spec/` carve-out; a spec write on a bare integration base is DENIED like any other write. EXECUTE therefore finds the branch already checked out and reports the unit isolated IN PLACE (`inPlace:true`). `EnterWorktree name=<branch from the output>` still cuts a worktree from a fresh `origin/{base}` when the branch is NOT already out, the parallel-work case. An old `{base}_{slug}` name still reads as its unit. Every path emits. Read-only requests never branch or open a worktree.

**A decision the conversation settles is written down when it is settled**, never reconstructed from memory at draft time. What is not written before a compaction is lost — measured: two units shipped across two days of conversation and NEITHER carried material, because the rule named no door.

```
mustard-rt run material-add --spec {slug} --kind decision   --subject "<what>"  --detail "<why>"
mustard-rt run material-add --spec {slug} --kind definition --subject "<term>"  --detail "<what it means here>"
mustard-rt run material-add --spec {slug} --kind finding    --subject "<claim>" --detail "<file>" [--line N]
```

One call per item, at the moment it is settled. Each lands in the unit's `spec-material.json`, which is the file `spec-draft --material` reads.
