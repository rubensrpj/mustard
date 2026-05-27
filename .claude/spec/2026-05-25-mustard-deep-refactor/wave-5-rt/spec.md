# W5 — rt-new-subcommands (após W1)
### Stage: Plan
### Outcome: Active
### Flags: 

## Contexto

W1 já entrega `spec-draft`, `skill-resolve`, `spec-validate`. Aqui ficam os subcomandos restantes que substituem prosa em SKILL.md e habilitam W6 (cortes).

## Tarefas (16 subcomandos novos)

| # | Done | Subcomando | Substitui | Arquivo |
|---|---|---|---|---|
| T5.1 | [x] | `close-orchestrate` | passos imperativos em `close/SKILL.md` (verify → qa → docs-stale → summary → complete) | `apps/rt/src/run/close_orchestrate.rs` |
| T5.2 | [x] | `review-dispatch --pr <N>` | Steps em `review/SKILL.md` | `apps/rt/src/run/review_dispatch.rs` |
| T5.3 | [x] | `tactical-fix-create --parent X --description Y --scope Z` | Steps em `tactical-fix/SKILL.md` | `apps/rt/src/run/tactical_fix_create.rs` |
| T5.4 | [x] | `prd-build --intent "..."` | 167 linhas determinísticas em `prd/SKILL.md` | `apps/rt/src/run/prd_build.rs` |
| T5.5 | [x] | `skill-fetch --name X` + `skill-cache --check X` | install em `skill/SKILL.md` | `apps/rt/src/run/skill_fetch.rs` + `skill_cache.rs` |
| T5.6 | [x] | `adapt-cursor` | `templates/adapters/cursor/adapter.js` | `apps/rt/src/run/adapt_cursor.rs` |
| T5.7 | [x] | `maint-deps` + `maint-validate` | `maint/SKILL.md` (install + build/typecheck) | `apps/rt/src/run/maint_*.rs` |
| T5.8 | [x] | `task-checklist --domain X` | Domain Checklists em `task/SKILL.md` | `apps/rt/src/run/task_checklist.rs` |
| T5.9 | [x] | `bugfix-cache --hash X` | pseudo-código em `bugfix/SKILL.md` | `apps/rt/src/run/bugfix_cache.rs` |
| T5.10 | [x] | `context-budget --role X --spec Y --wave N` | planning de orçamento | `apps/rt/src/run/context_budget.rs` |
| T5.11 | [x] | `backup-specs --target <path> --filter all\|active --dry-run` | comando idempotente cross-platform | `apps/rt/src/run/backup_specs.rs` |
| T5.12 | [x] | `migrate-to-meta` | one-shot para legacy `### X:` → meta.json | `apps/rt/src/run/migrate_to_meta.rs` (pré-existente) |
| T5.13 | [x] | `i18n translate-heading --from "## Tasks" --to-lang pt-BR` | header translation | `apps/rt/src/run/i18n_translate.rs` |
| T5.14 | [x] | `spec-lang resolve --spec <path>` | resolução de idioma | `apps/rt/src/run/spec_lang_resolve.rs` |
| T5.15 | [x] | `economy capture-baseline --operation X --wave Y [--from-history]` + `economy reconcile --wave W` + `economy report --format json\|table` | métrica auditável | `apps/rt/src/run/economy_capture_baseline.rs` + `economy_reconcile.rs` + `economy_report.rs` |
| T5.16 | [x] | `pipeline-prelude --spec X --phase {ANALYZE\|PLAN\|EXECUTE}` | consolida spec-hygiene + diff-context + auto-sync (W6 chama 1 vez) | `apps/rt/src/run/pipeline_prelude.rs` |

## Tarefas comuns

- [x] Cada subcomando segue `rt-run-subcommand-pattern`: `Options` struct, `parse(args)`, `run(opts)`, JSON byte-stable.
- [x] Cada um registrado em `apps/rt/src/run/mod.rs`.
- [x] Cada um com `cargo test` (happy path + error path + JSON shape).
- [x] Cada um emite `pipeline.economy.operation.invoked { operation, duration_ms, tokens_used: 0, was_rust_only: true }`.
- [x] Doc-comments rustdoc em en-US.

## Critérios de Aceitação

- [x] **AC-W5.1** — `mustard-rt run --help` lista os 16 subcomandos. Command: validador.
- [x] **AC-W5.2** — Cada subcomando tem teste passando. Command: `rtk cargo test -p mustard-rt`
- [~] **AC-W5.3** — `rtk cargo clippy -p mustard-rt -- -D warnings` limpo nos 16 arquivos novos; 18 lints residuais em arquivos pré-existentes fora dos Limites (`scan_structural.rs`, `claude_dir_prune.rs`, `scan/interpret.rs`, `scan_md_validate.rs`).

## Limites

`apps/rt/src/run/mod.rs`, 16 arquivos novos em `apps/rt/src/run/`.

OUT: tudo fora.

## Role

rt
