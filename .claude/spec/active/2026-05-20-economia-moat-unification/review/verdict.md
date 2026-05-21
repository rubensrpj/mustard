# Review Verdict — economia-moat-unification

### Phase: REVIEW (Wave 9)
### Timestamp: 2026-05-21T06:30:00Z
### Round: 1/2

## Verdict consolidado

**REJECTED** (1 CRITICAL real em core, 1 CRITICAL cosmético em dashboard) → fix-loop 1/2.

| Reviewer | Verdict | CRITICAL | WARNING | NOTE | Tactical-fix |
|----------|---------|----------|---------|------|--------------|
| core | REJECTED | 1 (projects[0] bug) | 5 | 3 | 4 |
| rt | APPROVED | 0 | 2 | 4 | 3 |
| dashboard | APPROVED-with-fixes | 1 (cosmetic) | 3 | 5 | 3 |

## CRITICAL findings (devem fechar)

### Core

1. **`reader.rs`: fan_out closures passam `projects[0]` em todas as 6 branches de `AllProjects`** (linhas 43, 121, 197, 274, 348, 423).
   - Hoje não corrompe dados (porque `scope_where(Project)` emite `None, None` params), mas silenciosamente rompe se um filtro por path for adicionado em `scope_where` futuro.
   - Fix: expor o `ProjectPath` corrente para a closure (capturar da iteração interna do `fan_out`).
   - LOC estimado: ~12.

### Dashboard

2. **`i18n.ts`: import `useSyncExternalStore` não usado** (linhas 21 e 116 com `void` suppressor).
   - Não quebra tsc hoje, mas trip-eará lint `no-unused-vars`. Comment de "referenced via zustand selector under the hood" é falso (zustand não usa `useSyncExternalStore` direto).
   - Fix: deletar import + suppressor.
   - LOC: 3.

## WARNING confirmados (devem fechar — silenciam correctness)

### Core

3. **`reader.rs:640,643,650`: tautologia `?2 = ?2` em `scope_where(Wave)` em `economy_summary` + `savings_breakdown`** — Wave scope vira no-op silencioso. Callers de `EconomyScope::Wave` para essas 2 fns recebem todos os spans do spec, não só da wave. Hole de correctness.
   - Fix: implementar filtragem real por wave_id (já existe no CTE de `per_agent_costs`; replicar nas outras 2).
   - LOC: ~15.

### Dashboard

4. **`NOTICE.md`: placeholders `<year>` e `<authors>` literais** — atribuição MIT incompleta legalmente; deve ser substituída por best-effort (`2023-2025 Anthropic, Inc.`) antes do CLOSE.
   - LOC: 1.

## WARNING + tactical-fix recomendados (bundlar no fix-loop p/ entregar 100% limpo)

### Core

5. **`now_iso` + `epoch_secs_to_ymdhms` duplicados 3x** em `sources/{otel,transcript,rtk}.rs` — extrair para `economy::sources::time` privado. ~90 LOC dedupe.

6. **Adicionar 1 teste cobrindo `EconomyScope::Wave` em economy_summary** — `tests/economy_basic.rs` ou `economy_attribution.rs`. ~15 LOC.

### RT

7. **`bash_guard.rs:1470` — `BashGuardBlock` → `RtkRewrite`** no site rtk-rewrite. ~1 LOC. Resolve W2 Concern + WARNING do reviewer.

8. **`bash_guard.rs:1466` — `estimate_input_tokens(&cmd, "")` → thread `ctx.model` ou env `CLAUDE_MODEL`**. ~3 LOC.

### Dashboard

9. **`formatTokens` duplicado** em `ExecutionTrace.tsx`/`BaseRow.tsx`/`economy.ts` — consolidar em `lib/types/economy.ts` (já exporta lá). ~10 LOC.

10. **`CodeBlock`: grid bug quando `showLineNumbers={false}`** — usar `grid-cols-[1fr]` quando sem gutter. ~5 LOC.

## NOTE (não precisam ser fixados agora, mas vão pra Concerns)

- Core: `eprintln!` em vez de `tracing::warn!` em adapters (motivado por `mustard-core` não ter `tracing` dep)
- Core: `context_routing_quality` usa `f64` para ratios (aceitável; documentar)
- Core: `insert_span_row` grava `project_path` como `None` (writer não thread o param)
- Core: `AttributeView` linear scan (aceitável até ~50 attrs)
- RT: 5 cópias do helper Connection (W3a delivered `open_for` mas W2 hooks não foram refatorados — follow-up wave)
- RT: `home_dir()`+`encode_cwd()` duplicados em session_cleanup.rs + transcript_watcher.rs
- RT: `/v1/traces` sem doc do env var necessário
- RT: `transcript_watcher` sem PID file (multi-spawn risk com `MUSTARD_TRANSCRIPT_WATCH=1`)
- Dashboard: `WorkspaceStatusBar.tsx` virou orphan (importado em nenhum lugar)
- Dashboard: `ScopeBar` fetcha specs internamente (não é reusable fora da Economia)
- Dashboard: `EconomySummary.top_agents_by_cost` cap=3 no core, mas `PerAgentTable.limit=10`

## Próximo passo

Fix-loop 1/2: dispatch 3 fix agents paralelos (core-impl, rt-impl, dashboard-impl) com a lista CRITICAL+WARNING+tactical-fix do seu subprojeto. Re-dispatch dos 3 reviewers após retorno. Se APPROVED em todos → avança pra QA (Wave 10). Se algum CRITICAL persistir → loop 2/2; se ainda assim falhar → STOP + AskUserQuestion.
