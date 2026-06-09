# Tactical Fix: qa-run pula AC de cargo silenciosamente: timeout 120s curto para suites e auto-invocacao -p nao tratada

## Contexto

Tactical fix derivado de [[menos-ia-mais-mustard-compor]]. Defeito medido: no fechamento daquela spec, o AC-8 (`cargo test -p mustard-rt`) saiu `skip` com QA `overall=pass` — um AC pulado silenciosamente corrói a confiança no gate. Causa-raiz dupla, verificada em `apps/rt/src/commands/review/qa_run.rs`:

1. **Timeout fixo curto** (`AC_TIMEOUT_SECS = 120`, linha 33): um AC de `cargo build/test` que precise recompilar (caso real: 2 arquivos do rt editados momentos antes) estoura 120s na compilação e vira `skip` ("timeout after 120000ms" fica só no `stderr_excerpt`).
2. **Catch-22 de auto-invocação incompleto** (`rewrite_self_invoked_cargo`, linhas 310-355): só trata `cargo build/test --workspace` (anexa `--exclude mustard-rt`); a forma direta `-p mustard-rt` / `--package mustard-rt` passa intocada e, executada de dentro do próprio `mustard-rt` (qa-run via complete-spec/close-pipeline), tentaria relinkar o `.exe` em execução — `Acesso negado (os error 5)` no Windows.

Fix (determinístico, no qa_run.rs):
- Timeout sensível a compilação: comandos contendo `cargo ` ganham um teto maior (ex. 600s) — o default de 120s permanece para o resto; ambos sobrescritíveis por env `MUSTARD_QA_AC_TIMEOUT_SECS` (documentar no doc-comment).
- Auto-invocação `-p`/`--package` do próprio crate: detectar quando `self_invoked=true` e o comando alveja `mustard-rt` diretamente → `skip` IMEDIATO com `stderr_excerpt` explícito ("self-invocation: cannot rebuild the running binary; run this AC externally") em vez de queimar o timeout e falhar com os error 5.

## Critérios de Aceitação

- **AC-1** — Timeout de AC cargo-aware + env override: comando com `cargo ` usa o teto maior; não-cargo mantém 120s; `MUSTARD_QA_AC_TIMEOUT_SECS` sobrescreve ambos
  Command: `cargo test -p mustard-rt qa_timeout`
- **AC-2** — Auto-invocação direta detectada: com `self_invoked=true`, AC `cargo test -p mustard-rt` vira skip imediato com razão explícita; com `self_invoked=false` o comando roda intocado
  Command: `cargo test -p mustard-rt qa_self_invoked`
- **AC-3** — Suíte do módulo verde
  Command: `cargo test -p mustard-rt qa_run`

## Arquivos

- `apps/rt/src/commands/review/qa_run.rs` — `AC_TIMEOUT_SECS` vira função timeout-por-comando (cargo-aware + env); `rewrite_self_invoked_cargo` (ou um irmão `detect_self_invoked_direct`) cobre `-p`/`--package mustard-rt`; testes `qa_timeout_*` e `qa_self_invoked_*`

<!-- wikilinks-footer-start -->
- [menos-ia-mais-mustard-compor](?) ⚠ unresolved
<!-- wikilinks-footer-end -->