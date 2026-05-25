# W2 — Limpeza profunda da `.claude/` raiz do Mustard

## Contexto

T2.1 já foi entregue manualmente nesta sessão (8 paths movidos para `~/.mustard-backups/2026-05-25-claude-dir-prune-manual/`). Restam: subcomando agnóstico, janitor automático, contrato canônico em CLAUDE.md template.

## Tarefas

- [x] **T2.1** — Manual nesta sessão: removidos `scripts/`, `adapters/`, `plans/`, `agent-memory/`, `.agent-memory/`, `memory/`, `metrics/`, `.tmp/` (797 KB backup).
- [ ] **T2.2** — `mustard-rt run claude-dir-prune` em `apps/rt/src/run/claude_dir_prune.rs`. Output JSON com `path/classification/evidence/recommendation`. Flags `--dry-run`/`--apply`/`--json`. Auditoria: para cada subdir em `.claude/`, classifica (KEEP/STALE/ORPHAN/LEGACY) baseado em cross-check com rt/cli/dashboard.
- [ ] **T2.3** — Janitor SessionStart: hook `session_start.rs` chama `claude_dir_prune::check_orphans()` e emite WARN se >0 ORPHAN. Não bloqueia.
- [ ] **T2.4** — Contrato em `apps/cli/templates/CLAUDE.md`: "todo path em `.claude/` deve ter consumidor declarado em pelo menos uma das três subprojects, exceto caches `.X.json` e diretórios documentados (`worktrees/`, `.pipeline-states/`, `.qa-reports/`)".

## Critérios de Aceitação

- [x] **AC-W2.1** — Paths ORPHAN/LEGACY removidos. Validado 2026-05-25.
- [ ] **AC-W2.2** — `mustard-rt run claude-dir-prune --help` lista flags. Command: `rtk mustard-rt run claude-dir-prune --help`
- [ ] **AC-W2.3** — `session_start.rs` referencia `claude_dir_prune`. Command: `rtk node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/session_start.rs','utf8');if(!/claude_dir_prune/.test(t))process.exit(1)"`
- [ ] **AC-W2.4** — `CLAUDE.md` do template contém o contrato. Command: `rtk node -e "const t=require('fs').readFileSync('apps/cli/templates/CLAUDE.md','utf8');if(!/consumidor declarado/.test(t)&&!/declared consumer/.test(t))process.exit(1)"`

## Limites

`apps/rt/src/run/claude_dir_prune.rs` (novo), `apps/rt/src/run/mod.rs`, `apps/rt/src/hooks/session_start.rs`, `apps/cli/templates/CLAUDE.md`.

OUT: tudo fora.

## Role

mixed (rt + cli)
