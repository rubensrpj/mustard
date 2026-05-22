# SpecCard usa children_count em vez de useSpecChildren — eliminar N+1 invocações

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-21T01:00:00Z
### Lang: pt
### Parent: 2026-05-20-tactical-fix-via-sub-spec

## Contexto

Tactical fix derivado de [[2026-05-20-tactical-fix-via-sub-spec]]. O review frontend apontou que `SpecCard.tsx` chama `useSpecChildren(repoPath, data.spec)` por card renderizado. Em uma Sidebar com 20 specs ativas, são 20 invocações Tauri independentes (mitigadas por `refetchInterval: 30_000` mas ainda caras).

`SpecSummary` já carrega `children_count: u32` (populado pelo `SqliteSpecReader::list_specs` agora hoist'ado — ver [[2026-05-21-list-specs-hoist-replay]]). Basta o `SpecCard` consumir esse campo via prop em vez de despachar query própria. O `useSpecChildren` continua relevante e necessário em `SpecDrillDown.tsx` (contador da aba) e `SpecChildrenTab.tsx` (lista de filhas).

## Critérios de Aceitação

- [x] AC-1: Dashboard frontend compila — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: `SpecCard.tsx` NÃO importa `useSpecChildren` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(c.includes('useSpecChildren')?1:0)"`
- [x] AC-3: `SpecCard.tsx` referencia `children_count` para decidir badge — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(c.includes('children_count')?0:1)"`
- [x] AC-4: `SpecDrillDown.tsx` continua usando `useSpecChildren` (consumidor legítimo) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecDrillDown.tsx','utf8');process.exit(c.includes('useSpecChildren')?0:1)"`
- [x] AC-5: `SpecChildrenTab.tsx` continua usando `useSpecChildren` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecChildrenTab.tsx','utf8');process.exit(c.includes('useSpecChildren')?0:1)"`

## Arquivos

```
apps/dashboard/src/components/specs/SpecCard.tsx                — remove useSpecChildren, lê data.children_count
(eventualmente) apps/dashboard/src/lib/types/specs.ts           — confirmar children_count no shape consumido pelo card
(eventualmente) apps/dashboard/src-tauri/src/spec_views.rs      — garantir que o adapter usado por SpecCard popula children_count
```

## Tarefas

- [x] Auditar: SpecCard.tsx recebe dados via prop `data` de qual shape? Identificar se é `SpecSummary` (vindo de `dashboard_specs` list) ou outro shape (`dashboard_spec_card`). Se shape atual NÃO tem `children_count`, propagar do reader já existente até a prop.
- [x] Remover `useSpecChildren` + imports relacionados de `SpecCard.tsx`. Substituir `subSpecCount = children?.length ?? 0` (ou equivalente) por `subSpecCount = data.children_count ?? 0`.
- [x] Confirmar que `SpecDrillDown.tsx` e `SpecChildrenTab.tsx` continuam usando o hook (consumidores legítimos — não tocar).
- [x] `pnpm --filter mustard-dashboard build`.

## Limites

- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/lib/types/specs.ts` (apenas se faltar `children_count` no shape consumido pelo card)
- `apps/dashboard/src-tauri/src/spec_views.rs` (apenas se o adapter não emitir `children_count`)

**Fora dos limites:**
- Remover `useSpecChildren` de SpecDrillDown ou SpecChildrenTab.
- Mexer no hook em si.

## Checklist

- [x] AC-1 a AC-5 passam
- [x] Diff cirúrgico
