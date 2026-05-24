# W1 — worktree-gc (limpeza sistêmica de worktrees órfãos)

## Contexto

W0 limpou o worktree órfão atual manualmente, mas amanhã outro vai aparecer (cada Task com `isolation: "worktree"` cria um e o cleanup é frágil). Esta onda entrega o subcomando `worktree-gc` que enumera, identifica órfãos por idade, e remove com fail-open.

## Tarefas

- [x] **T1.1.** Novo `apps/rt/src/run/worktree_gc.rs` seguindo `rt-run-subcommand-pattern`. Função `run(args) -> Result<Value>` que enumera `.claude/worktrees/agent-*`, lê metadata (timestamp de `git worktree list` + mtime), filtra `> --age-days` (default 7).
- [x] **T1.2.** Flags: `--age-days <N>` (default 7), `--dry-run` (default true como `spec-clear`), `--apply` para realmente remover. Saída JSON `{ removed: [...], kept: [...], errors: [...] }`.
- [x] **T1.3.** Emit evento `worktree.gc.run` com payload `{ removed_count, kept_count }`.
- [x] **T1.4.** Hook `SessionStart` ganha chamada idempotente fail-open: se há mais que 3 worktrees órfãos > 7d, emite warning (não bloqueia).
- [x] **T1.5.** Registrar subcomando em `apps/rt/src/run/mod.rs`.
- [x] **T1.6.** Testes: in-memory fixture com 3 worktrees de idades diferentes; valida que só os > age-days são removidos com `--apply`.
- [x] **T1.7.** Emit `pipeline.economy.operation.invoked { operation: "worktree-gc", duration_ms }` para alimentar `/economia` (W12).

## Files

- `apps/rt/src/run/worktree_gc.rs` (novo)
- `apps/rt/src/run/mod.rs` (registrar)
- `apps/rt/src/hooks/session_start.rs` (chamada idempotente)

## Critérios de Aceitação

- [x] AC-W1-1: `--help` lista as flags `--age-days`, `--dry-run`, `--apply` — Command: `node -e "const{execSync}=require('child_process');const out=execSync('mustard-rt run worktree-gc --help',{encoding:'utf8'});for(const k of ['age-days','dry-run','apply'])if(!out.includes('--'+k)){console.error('missing flag',k);process.exit(1)}"`
- [x] AC-W1-2: saída padrão é JSON com chaves `removed`, `kept`, `errors` — Command: `node -e "const{execSync}=require('child_process');const out=execSync('mustard-rt run worktree-gc',{encoding:'utf8'});const j=JSON.parse(out);for(const k of ['removed','kept','errors'])if(!Array.isArray(j[k])){console.error('not array',k);process.exit(1)}"`
- [x] AC-W1-3: cargo test -p mustard-rt worktree_gc passa — Command: `cargo test -p mustard-rt worktree_gc --quiet`
- [x] AC-W1-4: hook SessionStart fail-open quando `.claude/worktrees/` ausente (coberto por session_start_probe tests) — Command: `cargo test -p mustard-rt session_start --quiet`
- [x] AC-W1-5: evento `pipeline.economy.operation.invoked` emitido após primeira execução — Command: `mustard-rt run verify-emit --event pipeline.economy.operation.invoked --since 10m --quiet`

## Notas

- Subcomando segue padrão `rt-run-subcommand-pattern` (Options struct + split entry-point + JSON byte-stable).
- Fail-open: erros de filesystem em worktrees individuais não abortam o batch.
- Paralelizável com W2.
