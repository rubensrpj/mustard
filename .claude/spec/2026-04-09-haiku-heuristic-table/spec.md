# Enhancement: haiku-heuristic-table
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Reescrever a seção "Model Selection" em `review/SKILL.md` como tabela de decisão imperativa (single source of truth). Substituir a prose em `feature/SKILL.md` e `complete/SKILL.md` por referência de 1 linha à tabela. DRY + copy-pasteable + inambígua.

## Why
Re-auditoria detectou que a heurística atual (prose distribuída em 3 SKILL.md) é descritiva, não prescritiva — orchestrator pode pular a decisão sob carga cognitiva. Uma tabela explícita com linhas imperativas é mais fácil de seguir e mais difícil de ignorar. Também elimina duplicação: hoje a mesma lógica aparece em 3 arquivos.

## Boundaries
- `templates/commands/mustard/review/SKILL.md`
- `templates/commands/mustard/feature/SKILL.md`
- `templates/commands/mustard/complete/SKILL.md`
- `.claude/commands/mustard/review/SKILL.md` (mirror)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)
- `.claude/commands/mustard/complete/SKILL.md` (mirror)

## Checklist
### templates-impl Agent
- [x] Localizar seção "Model Selection" (ou similar) em `templates/commands/mustard/review/SKILL.md`
- [x] Reescrever como imperative steps + decision table. Template abaixo.
- [x] Em `templates/commands/mustard/feature/SKILL.md`, substituir a prose existente de re-review dispatch por: `Before re-review dispatch: consult \`review/SKILL.md § Model Selection\` decision table. Set \`model: "haiku"\` if row 1 matches.`
- [x] Mesma substituição em `templates/commands/mustard/complete/SKILL.md`
- [x] Mirror para `.claude/commands/mustard/{review,feature,complete}/SKILL.md`
- [x] Build: `rtk npm run build` → PASS
- [x] Validar markdown: renderizar mentalmente, sem `|` desalinhado nem seção órfã

### Template da seção em review/SKILL.md

```markdown
## Model Selection

**Initial reviews**: always use default model (per `pipeline-config.md § Models`).

**Re-reviews**: apply this decision BEFORE dispatching the re-review Task:

1. Count lines in the previous review's return content matching `^\[(CRITICAL|WARNING)\]`. This is `issue_count`.
2. Count files in the pending fix step. This is `files_changed`.
3. Decision table:

   | issue_count | files_changed | model           |
   |-------------|---------------|-----------------|
   | ≤3          | <5            | `haiku`         |
   | else        | else          | default         |

4. Set `model: "..."` on the re-review Task dispatch per the matching row.
```

## Files (~6)
- `templates/commands/mustard/review/SKILL.md` (modify — main rewrite)
- `templates/commands/mustard/feature/SKILL.md` (modify — reference)
- `templates/commands/mustard/complete/SKILL.md` (modify — reference)
- `.claude/commands/mustard/review/SKILL.md` (mirror)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)
- `.claude/commands/mustard/complete/SKILL.md` (mirror)

## Acceptance
- Tabela existe em `review/SKILL.md` como single source of truth
- `feature.md` e `complete.md` têm APENAS referência (1 linha)
- Mirrors sync com templates
- Build limpo
- Markdown renderiza (tabela bem formada)

## Guards
- NÃO remover a seção "Model Selection" sem substituir
- NÃO alterar o modelo de reviews iniciais (só re-reviews)
- Threshold `issue_count ≤3 AND files_changed <5` é a regra — não flexibilizar
- Manter a referência a `pipeline-config.md § Models` para default model

## Result

### Files Modified
- `templates/commands/mustard/review/SKILL.md` lines 83–93 — Model Selection rewritten as imperative steps + decision table (single source of truth)
- `templates/commands/mustard/feature/SKILL.md` line 193 — prose replaced with 1-line reference
- `templates/commands/mustard/complete/SKILL.md` line 35 — prose replaced with 1-line reference
- `.claude/commands/mustard/review/SKILL.md` — mirror updated
- `.claude/commands/mustard/feature/SKILL.md` — mirror updated
- `.claude/commands/mustard/complete/SKILL.md` — mirror updated

### Build
- `npm run build` (tsc): PASS
