# Workspace Root — âncora única para escrita do harness

### Stage: Close
### Outcome: Cancelled
### Flags: superseded
### Checkpoint: 2026-05-26T04:00:00Z
### Superseded-by: [[2026-05-26-claude-paths-single-source]]

## Status

**Cancelada em 2026-05-26.** Escopo absorvido por [[2026-05-26-claude-paths-single-source]] (já em PLAN/Active), que ataca o mesmo problema por ângulo complementar (catálogo de paths via `ClaudePaths`). **Não executar.**

## Por que foi cancelada

Esta spec foi rascunhada antes de descobrirmos que já existia
[[2026-05-26-claude-paths-single-source]] atacando parte do mesmo problema. A
spec ativa entrega `ClaudePaths` (catálogo) mas o construtor `for_project(root)`
é API neutra — recebe a raiz como parâmetro. Faltava o **walker** que produz
essa raiz a partir do `cwd` cru: era o conteúdo desta spec.

## Como foi fundida

Os entregáveis desta spec foram absorvidos na spec mãe:

| Aqui | Na spec mãe |
|---|---|
| `workspace_root()` walker | W1 (T1.5-T1.8): mesmo arquivo do `ClaudePaths`, módulo irmão |
| Invariante I1 (`.claude/.claude/` proibido) | W1 (guard no walker + guard em `ClaudePaths::for_project`) + W3 (doctor `--check i1`) |
| Propagação via `dispatch.rs` | W2 (T2.9): newtype `WorkspaceRoot` no `Ctx`, resolução única em `build_ctx()` |
| `env::project_dir()` substituído | W2 (T2.10) |
| Fixture `test_workspace()` | W2 (helper em `apps/rt/tests/common/mod.rs`) |
| Doctor `--check workspace-leaks` | W3 (T3.8) |
| Doctor `--check i1` | W3 (T3.9) |
| Doctor agregador default | W3 (T3.10) |
| Limpeza retroativa one-shot | W4 (nova wave inteira) |

## Causa-raiz preservada (frase principal)

> O bug não é "o Rust não faz o que o TS fazia". É "o Rust precisa de algo que o
> TS não precisava ter". O TS vivia dentro do `.claude/scripts/` da raiz, então
> `path.resolve(__dirname, "..", "..")` sempre acertava por posição estrutural.
> O binário Rust em `$PATH` perdeu essa âncora — e ninguém adicionou o walker
> para compensar.

Referência: `c:\Atiz\sialia\.claude\scripts\sync-detect.js:25` (código JS legado
ainda presente como cópia stale) e `apps/rt/src/run/env.rs:15` (Rust atual sem
walker).

## Links

- [[2026-05-26-claude-paths-single-source]] — spec que absorve este escopo
- [[project_no_bun_rust_only]] — migração que originou a regressão
- [[feedback_no_attach_sqlite]] — proibição de múltiplos `mustard.db`
- [[feedback_mustard_self_scripts_stale]] — `.claude/scripts/` é cópia stale,
  fonte viva é `templates/scripts/` (relevante para achar o código JS original)
