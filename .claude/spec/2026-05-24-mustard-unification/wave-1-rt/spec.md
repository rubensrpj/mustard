# W1 — worktree-gc (limpeza sistêmica de worktrees órfãos)

### Stage: Plan
### Outcome: Active
### Phase: PLAN
### Scope: light
### Checkpoint: 2026-05-24T19:30:00Z
### Lang: pt-BR

## Contexto

W0 limpou o worktree órfão atual manualmente, mas amanhã outro vai aparecer (cada Task com `isolation: "worktree"` cria um e o cleanup é frágil). Esta onda entrega o subcomando `worktree-gc` que enumera, identifica órfãos por idade, e remove com fail-open.

## Tarefas

- [ ] **T1.1.** Novo `apps/rt/src/run/worktree_gc.rs` seguindo `rt-run-subcommand-pattern`. Função `run(args) -> Result<Value>` que enumera `.claude/worktrees/agent-*`, lê metadata (timestamp de `git worktree list` + mtime), filtra `> --age-days` (default 7).
- [ ] **T1.2.** Flags: `--age-days <N>` (default 7), `--dry-run` (default true como `spec-clear`), `--apply` para realmente remover. Saída JSON `{ removed: [...], kept: [...], errors: [...] }`.
- [ ] **T1.3.** Emit evento `worktree.gc.run` com payload `{ removed_count, kept_count }`.
- [ ] **T1.4.** Hook `SessionStart` ganha chamada idempotente fail-open: se há mais que 3 worktrees órfãos > 7d, emite warning (não bloqueia).
- [ ] **T1.5.** Registrar subcomando em `apps/rt/src/run/mod.rs`.
- [ ] **T1.6.** Testes: in-memory fixture com 3 worktrees de idades diferentes; valida que só os > age-days são removidos com `--apply`.
- [ ] **T1.7.** Emit `pipeline.economy.operation.invoked { operation: "worktree-gc", duration_ms }` para alimentar `/economia` (W12).

## Files

- `apps/rt/src/run/worktree_gc.rs` (novo)
- `apps/rt/src/run/mod.rs` (registrar)
- `apps/rt/src/hooks/session_start.rs` (chamada idempotente)

## Critérios de Aceitação

- [ ] **AC-1.1.** `rtk mustard-rt run worktree-gc --help` lista flags `--age-days`, `--dry-run`, `--apply`. Command: `rtk mustard-rt run worktree-gc --help 2>&1 | grep -E "(age-days|dry-run|apply)" | wc -l | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{if(parseInt(s)<3)process.exit(1)})"`
- [ ] **AC-1.2.** Em projeto com 0 worktrees, retorna JSON `{ removed: [], kept: [], errors: [] }`. Command: `rtk mustard-rt run worktree-gc --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!Array.isArray(j.removed)||!Array.isArray(j.kept))process.exit(1)})"`
- [ ] **AC-1.3.** `cargo test -p mustard-rt worktree_gc` passa.
- [ ] **AC-1.4.** Hook SessionStart não trava nem fail se `.claude/worktrees/` não existe (fail-open).
- [ ] **AC-1.5.** Evento `pipeline.economy.operation.invoked` aparece em `mustard.db` após primeira execução. Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \"SELECT count(*) FROM events WHERE event=\\\"pipeline.economy.operation.invoked\\\"\"',{encoding:'utf8'});if(parseInt(out.trim())<1)process.exit(1)"`

## Notas

- Subcomando segue padrão `rt-run-subcommand-pattern` (Options struct + split entry-point + JSON byte-stable).
- Fail-open: erros de filesystem em worktrees individuais não abortam o batch.
- Paralelizável com W2.
