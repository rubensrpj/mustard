# W6 — rt new subcommands (15 subcomandos novos)

## Contexto

Pré-requisito para o corte massivo dos `SKILL.md` (W7). Cada subcomando substitui um bloco grande de lógica que hoje vive em markdown e gasta tokens no agente. Padrão consolidado: `Options struct + split entry-point` (`cli-command-pattern`), `rt-run-subcommand-pattern`, saída JSON byte-stable. Todos seguem fail-open.

## Tarefas (15 subcomandos)

| # | Subcomando | Substitui | Arquivo |
|---|---|---|---|
| T6.1 | `spec-scaffold --kind {feature\|bugfix\|tactical-fix} --scope {light\|full} --lang {pt-BR\|en-US}` | header generator em feature/bugfix/tactical-fix SKILL.md | `apps/rt/src/run/spec_scaffold.rs` |
| T6.2 | `close-orchestrate` | steps imperativos em close/SKILL.md (verify → qa → docs-stale → summary → complete) | `apps/rt/src/run/close_orchestrate.rs` |
| T6.3 | `review-dispatch --pr <N>` | Steps 1-5 em review/SKILL.md | `apps/rt/src/run/review_dispatch.rs` |
| T6.4 | `tactical-fix-create --parent X --description Y --scope Z` | Steps 1-4 em tactical-fix/SKILL.md | `apps/rt/src/run/tactical_fix_create.rs` |
| T6.5 | `prd-build --intent "..."` | TODO prd/SKILL.md (167 linhas determinísticas: scope + entities + paths + JSON) | `apps/rt/src/run/prd_build.rs` |
| T6.6 | `skill-fetch --name X` + `skill-cache --check X` | Steps de install em skill/SKILL.md (sparse-clone + validate + cache) | `apps/rt/src/run/skill_fetch.rs` + `skill_cache.rs` |
| T6.7 | `adapt-cursor` | `apps/cli/templates/adapters/cursor/adapter.js` (bun shebang + CommonJS) | `apps/rt/src/run/adapt_cursor.rs` |
| T6.8 | `maint-deps` + `maint-validate` | maint/SKILL.md (install + build/typecheck em todos subprojects) | `apps/rt/src/run/maint_deps.rs` + `maint_validate.rs` |
| T6.9 | `task-checklist --domain X` | Domain Checklists em task/SKILL.md | `apps/rt/src/run/task_checklist.rs` |
| T6.10 | `bugfix-cache --hash X` | pseudo-código em bugfix/SKILL.md (hash + invalidation) | `apps/rt/src/run/bugfix_cache.rs` |
| T6.11 | `context-budget --role X --spec Y --wave N` | (novo — planning de orçamento ANTES do prompt) | `apps/rt/src/run/context_budget.rs` |
| T6.12 | `backup-specs --target <path> --filter all\|legacy-headers-only\|active-only --dry-run` | comando explícito de backup (idempotente, cross-platform, emit `backup.specs.created`) | `apps/rt/src/run/backup_specs.rs` |
| T6.13 | `i18n translate-heading --from "## Tasks" --to-lang pt-BR` | consulta a Header Translation Table no caso médio | `apps/rt/src/run/i18n_translate.rs` |
| T6.14 | `spec-lang resolve --spec <path>` | resolução de idioma em 2 SKILLs | `apps/rt/src/run/spec_lang_resolve.rs` |
| T6.15 | `economy capture-baseline --operation X --wave Y [--from-history]` + `economy reconcile --wave W` + `economy report --format json\|table` | métrica auditável | `apps/rt/src/run/economy_capture_baseline.rs` + `economy_reconcile.rs` + `economy_report.rs` |

## Tarefas comuns a todos

- [ ] **T6.A.** Cada subcomando segue `rt-run-subcommand-pattern`: `Options` struct, `parse(args)`, `run(opts)`, saída JSON byte-stable com `Report` helper.
- [ ] **T6.B.** Cada um registrado em `apps/rt/src/run/mod.rs` no enum `RunCmd` + dispatch.
- [ ] **T6.C.** Testes: cada subcomando com `cargo test` (happy path + error path + JSON shape).
- [ ] **T6.D.** Cada um emite `pipeline.economy.operation.invoked { operation: <name>, duration_ms, tokens_used: 0, was_rust_only: true }` para alimentar `/economia` (W12).
- [ ] **T6.E.** Documentação inline (doc-comments rustdoc) em en-US.

## Files

- `apps/rt/src/run/mod.rs` (registrar 15+ subcomandos)
- 15 arquivos novos em `apps/rt/src/run/` (ver tabela acima)
- `apps/cli/src/commands/add.rs` (W7 — extender; aqui só consume `skill-fetch`)

## Critérios de Aceitação

- [ ] AC-W6-1: `mustard-rt run --help` lista os 15 subcomandos novos. Command: `node -e "const{execSync}=require('child_process');const out=execSync('rtk mustard-rt run --help 2>&1',{encoding:'utf8'});for(const k of ['spec-scaffold','close-orchestrate','review-dispatch','tactical-fix-create','prd-build','skill-fetch','adapt-cursor','maint-deps','maint-validate','task-checklist','bugfix-cache','context-budget','backup-specs','economy']){if(!out.includes(k)){console.error('missing',k);process.exit(1)}}"`
- [ ] AC-W6-2: Cada subcomando tem teste passando. Command: `rtk cargo test -p mustard-rt 2>&1 | grep -E "(spec_scaffold|close_orchestrate|review_dispatch|tactical_fix_create|prd_build|skill_fetch|adapt_cursor|maint_deps|task_checklist|bugfix_cache|context_budget|backup_specs|economy)" | grep -q "ok"`
- [ ] AC-W6-3: `rtk cargo clippy -p mustard-rt -- -D warnings` limpo.
- [ ] AC-W6-4: `prd-build --intent "add user auth"` retorna JSON com `scope`, `entities`, `paths`, `acceptanceCriteria`. Command: `rtk mustard-rt run prd-build --intent "add user auth" --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);for(const k of ['scope','entities','paths','acceptanceCriteria']){if(!(k in j))process.exit(1)}})"`
- [ ] AC-W6-5: `backup-specs --dry-run --target ~/.mustard-backups/test/` lista todas as ~70 specs. Command: `rtk mustard-rt run backup-specs --dry-run --target /tmp/test-backup --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!Array.isArray(j.would_move)||j.would_move.length<50)process.exit(1)})"`

## Notas

- Bloqueia W7 (cortes em SKILL.md dependem desses subcomandos existirem).
- Cada subcomando é independente — podem ser entregues em paralelo se necessário (cada um é arquivo isolado em `apps/rt/src/run/`).
- `adapt-cursor` substitui `adapter.js` mas o `.js` físico só é removido em W7.
