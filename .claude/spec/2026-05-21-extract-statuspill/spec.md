# Extract StatusPill — DRY entre SpecCard e SpecChildrenTab

### Status: completed
### Phase: CLOSE
### Scope: light
### Checkpoint: 2026-05-21T01:10:00Z
### Lang: pt
### Parent: 2026-05-20-tactical-fix-via-sub-spec

## Contexto

Tactical fix derivado de [[2026-05-20-tactical-fix-via-sub-spec]]. O review frontend apontou que `StatusPill` (com seus mapas `STATUS_LABELS` e `STATUS_CLASSES`) está duplicado verbatim entre `SpecCard.tsx` e `SpecChildrenTab.tsx`. Duas cópias significam dois lugares para atualizar quando um novo status string aparecer. Hoje as cópias estão em sincronia, mas a próxima adição vai derivar (e bugs vão emergir só quando um status novo for renderizado).

Solução: extrair para `apps/dashboard/src/components/specs/spec-status.tsx` (módulo compartilhado), importar em ambos.

## Critérios de Aceitação

- [x] AC-1: Dashboard frontend compila — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: `spec-status.tsx` existe e exporta `StatusPill` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/spec-status.tsx','utf8');process.exit((c.includes('export')&&c.includes('StatusPill'))?0:1)"`
- [x] AC-3: `SpecCard.tsx` importa `StatusPill` do módulo novo — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(c.includes(\"from './spec-status'\")||c.includes('from \"./spec-status\"')?0:1)"`
- [x] AC-4: `SpecChildrenTab.tsx` importa `StatusPill` do módulo novo — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecChildrenTab.tsx','utf8');process.exit(c.includes(\"from './spec-status'\")||c.includes('from \"./spec-status\"')?0:1)"`
- [x] AC-5: `SpecCard.tsx` NÃO declara `STATUS_LABELS` localmente — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');const m=c.match(/const STATUS_LABELS/);process.exit(m?1:0)"`
- [x] AC-6: `SpecChildrenTab.tsx` NÃO declara `STATUS_LABELS` localmente — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecChildrenTab.tsx','utf8');const m=c.match(/const STATUS_LABELS/);process.exit(m?1:0)"`

## Arquivos

```
apps/dashboard/src/components/specs/spec-status.tsx              — novo: exporta StatusPill, STATUS_LABELS, STATUS_CLASSES
apps/dashboard/src/components/specs/SpecCard.tsx                 — importa em vez de redeclarar
apps/dashboard/src/components/specs/SpecChildrenTab.tsx          — importa em vez de redeclarar
```

## Tarefas

- [x] Criar `spec-status.tsx` exportando `StatusPill` componente + (eventualmente) `STATUS_LABELS` e `STATUS_CLASSES` maps.
- [x] Remover declarações locais de `StatusPill`/`STATUS_LABELS`/`STATUS_CLASSES` de `SpecCard.tsx` e `SpecChildrenTab.tsx`. Adicionar import.
- [x] `pnpm --filter mustard-dashboard build`.

## Limites

- 3 arquivos: o novo `spec-status.tsx`, `SpecCard.tsx`, `SpecChildrenTab.tsx`. Nada além.

**Fora dos limites:**
- Renomear `StatusPill` ou mudar API pública do componente.
- Mudar visual/cores.

## Checklist

- [x] AC-1 a AC-6 passam
- [x] Visual idêntico ao anterior (pixel parity)
