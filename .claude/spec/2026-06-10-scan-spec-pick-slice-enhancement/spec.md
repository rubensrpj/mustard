# Tactical Fix: pick_slice penaliza pseudo-slice degenerado + --like por entidade primária + carve-out de enhancement na prosa

## Contexto

Mesma auditoria 2026-06-10 (memória `mustard-sialia-payables-audit`): `scan spec --entity Payable --ops update` no sialia escolheu o pseudo-slice `Request+Response` (132 pares de wrappers GraphQL, conf 0.94) sobre o vertical real (rec 30, conf 0.97) e o slice de UI; experimento provou que `--like Payable`/`--like Receivable` NÃO resgata (o filtro substring sobre entities casa os próprios wrappers, e recurrence-first vence de novo) — a mitigação documentada é ineficaz. O compilador é create-only por decisão documentada; a prosa do /feature força "ALWAYS compile" mesmo para Enhancement. Escopo deste TF: ranking do pick_slice + escape funcional do --like + carve-out na prosa (só unidade net-new compila o molde create; enhancement consome as âncoras do digest). Modo enhancement completo do compilador fica para spec futura.

## Critérios de Aceitação

- **AC-1** — pick_slice: com fixture sialia-like (pseudo-slice de 2 roles rec 132 vs vertical ≥3 roles rec 30), o vertical vence; `--like` com entidade primária resolve para o slice da entidade, substring só como fallback.
  Command: `cargo test -p scan pick_slice`
- **AC-2** — Workspace verde.
  Command: `cargo test --workspace`
- **AC-3** — Prosa do /feature condiciona o `scan spec` a unidade net-new (carve-out de enhancement presente no template).
  Command: `rg -n "net-new" apps/cli/templates/commands/mustard/feature/SKILL.md`

## Arquivos

- apps/scan/src/spec.rs — comparator do pick_slice: slices com ≥3 roles têm precedência de classe sobre pares degenerados de 2 roles (que viram fallback); caminho --like tenta igualdade de entidade primeiro, substring como fallback
- apps/scan/tests/ — fixture + testes pick_slice
- apps/cli/templates/commands/mustard/feature/SKILL.md (+ espelho local .claude/commands/mustard/feature/SKILL.md se existir) — carve-out: só unidade NET-NEW compila `scan spec`; unidade de enhancement consome as âncoras do digest do feature