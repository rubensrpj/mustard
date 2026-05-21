# Wave 6 — Trace viewer (dashboard_spec_trace + ExecutionTrace component)

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: queued
### Phase: PLAN
### Scope: full (wave)
### Checkpoint: 2026-05-20T23:59:00Z
### Lang: pt

## PRD

Hoje a Visão Geral mostra eventos numa lista linear chapada (`WorkspaceEventsFeed`) e a página `/specs` tem timeline horizontal + lista de eventos separadas. Cinco crítica: nenhuma das duas mostra hierarquia spec→wave→agent→tool, nem renderiza diffs inline, nem mostra tokens por nível, nem permite colapsar/expandir. Esta wave entrega o `<ExecutionTrace>` — componente recursivo (não pelo nó, mas pelo container) inspirado em claude-devtools, que pivota o `mustard.db` em árvore navegável: spec colapsável → wave colapsável → agent colapsável → tool event com preview + diff inline. Backend novo `dashboard_spec_trace(projectPath, specName) -> TraceNode` faz o pivot via reader da W4. Frontend usa primitivas da W5 (`BaseRow`, `TreeNode`, `DiffViewer`, `CodeBlock`, `MetricsPill`). Substitui `WorkspaceEventsFeed` na Visão Geral e timeline+events na `/specs`.

## Acceptance Criteria

- [ ] AC-1: Build do dashboard passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-2: Cargo check do tauri passa — Command: `cargo check -p mustard-dashboard --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [ ] AC-3: Tauri command `dashboard_spec_trace` registrado — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');if(!t.includes('dashboard_spec_trace'))throw new Error('command not registered')"`
- [ ] AC-4: Componente `<ExecutionTrace>` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/components/trace/ExecutionTrace.tsx'))throw new Error('component missing')"`
- [ ] AC-5: Hook `useSpecTrace` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/hooks/useSpecTrace.ts'))throw new Error('hook missing')"`
- [ ] AC-6: Workspace.tsx removeu EventsFeed — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');if(t.includes('WorkspaceEventsFeed'))throw new Error('still importing EventsFeed')"`

## Plano

Backend: `dashboard_spec_trace` em `apps/dashboard/src-tauri/src/telemetry.rs` chama `core::economy::reader::per_agent_costs(scope=Spec)` + reader de events para reconstruir hierarquia. Devolve `TraceNode` shape: `{ kind: 'spec'|'wave'|'agent'|'tool', label, tokens?: TokenBreakdown, duration_ms?, children: TraceNode[] }`. Frontend: `<ExecutionTrace>` em `apps/dashboard/src/components/trace/` renderiza recursivamente usando `BaseRow` (W5) + `MetricsPill` para tokens + `DiffViewer` para tool events de Edit + `CodeBlock` para Read/Bash output. Substitui `WorkspaceEventsFeed` em `Workspace.tsx`. Adiciona nova tab "Trace" em `Specs.tsx` (ou substitui timeline existente — confirmar com user durante implementação).

## Dependências

- [[wave-4-attribution]]: reader devolve dados pivotados por agente.
- [[wave-5-ds-foundation]]: primitivas DS (BaseRow, TreeNode, DiffViewer, CodeBlock, MetricsPill).

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Depende de: [[wave-4-attribution]], [[wave-5-ds-foundation]]
- Paralela a: [[wave-7-economia-page]]
- Grava memória: `{trace_node_shape: "...", tauri_command: "dashboard_spec_trace", replaced: ["WorkspaceEventsFeed"]}`

## Limites

Em escopo: `apps/dashboard/src-tauri/src/telemetry.rs` (extend), `apps/dashboard/src-tauri/src/lib.rs` (registrar command), `apps/dashboard/src/components/trace/**` (novo), `apps/dashboard/src/hooks/useSpecTrace.ts` (novo), `apps/dashboard/src/pages/Workspace.tsx` (remover EventsFeed import + render), `apps/dashboard/src/pages/Specs.tsx` (substituir/adicionar tab Trace).

Fora de escopo: backend além do telemetry.rs, novas Tauri commands fora desta wave, DS primitivas (já entregue em W5), economia.tsx (W7).
