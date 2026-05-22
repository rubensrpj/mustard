# Wave 6 — Trace viewer (dashboard_spec_trace + ExecutionTrace component)

### Parent: [[2026-05-20-economia-moat-unification]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full (wave)
### Checkpoint: 2026-05-21T06:00:00Z
### Lang: pt

## PRD

Hoje a Visão Geral mostra eventos numa lista linear chapada (`WorkspaceEventsFeed`) e a página `/specs` tem timeline horizontal + lista de eventos separadas. Cinco crítica: nenhuma das duas mostra hierarquia spec→wave→agent→tool, nem renderiza diffs inline, nem mostra tokens por nível, nem permite colapsar/expandir. Esta wave entrega o `<ExecutionTrace>` — componente recursivo (não pelo nó, mas pelo container) inspirado em claude-devtools, que pivota o `mustard.db` em árvore navegável: spec colapsável → wave colapsável → agent colapsável → tool event com preview + diff inline. Backend novo `dashboard_spec_trace(projectPath, specName) -> TraceNode` faz o pivot via reader da W4. Frontend usa primitivas da W5 (`BaseRow`, `TreeNode`, `DiffViewer`, `CodeBlock`, `MetricsPill`). Substitui `WorkspaceEventsFeed` na Visão Geral e timeline+events na `/specs`.

## Acceptance Criteria

- [x] AC-1: Build do dashboard passa — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: Cargo check do tauri passa — Command: `cargo check -p mustard-dashboard --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [x] AC-3: Tauri command `dashboard_spec_trace` registrado — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');if(!t.includes('dashboard_spec_trace'))throw new Error('command not registered')"`
- [x] AC-4: Componente `<ExecutionTrace>` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/components/trace/ExecutionTrace.tsx'))throw new Error('component missing')"`
- [x] AC-5: Hook `useSpecTrace` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/hooks/useSpecTrace.ts'))throw new Error('hook missing')"`
- [x] AC-6: Workspace.tsx removeu EventsFeed — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');if(t.includes('WorkspaceEventsFeed'))throw new Error('still importing EventsFeed')"`

## Plano

Backend: `dashboard_spec_trace` em `apps/dashboard/src-tauri/src/telemetry.rs` chama `core::economy::reader::per_agent_costs(scope=Spec)` + reader de events para reconstruir hierarquia. Devolve `TraceNode` shape: `{ kind: 'spec'|'wave'|'agent'|'tool', label, tokens?: TokenBreakdown, duration_ms?, children: TraceNode[] }`. Frontend: `<ExecutionTrace>` em `apps/dashboard/src/components/trace/` renderiza recursivamente usando `BaseRow` (W5) + `MetricsPill` para tokens + `DiffViewer` para tool events de Edit + `CodeBlock` para Read/Bash output. Substitui `WorkspaceEventsFeed` em `Workspace.tsx`. Adiciona nova tab "Trace" em `Specs.tsx` (ou substitui timeline existente — confirmar com user durante implementação).

## Informações da Entidade

`TraceNode` (frontend-only DTO; backend retorna mesmo shape via serde): `{ kind, label, tokens?: { input, output, cache_read, cache_creation, cost_usd_micros? }, duration_ms?, ts?: string, payload?: any, children: TraceNode[] }`. `kind` ∈ `'spec'|'wave'|'agent'|'tool'`. `payload` opcional carrega `tool_input`/`tool_response` para `kind=tool`.

## Arquivos (~6)

```
apps/dashboard/src-tauri/src/telemetry.rs        (extend — fn dashboard_spec_trace + struct TraceNode serde)
apps/dashboard/src-tauri/src/lib.rs              (modify — registrar dashboard_spec_trace no .invoke_handler)
apps/dashboard/src/components/trace/ExecutionTrace.tsx       (new — render recursivo com BaseRow + TreeNode + tokens MetricsPill)
apps/dashboard/src/components/trace/ToolEventRow.tsx         (new — kind=tool render: DiffViewer p/ Edit, CodeBlock p/ Read/Bash, plain fallback)
apps/dashboard/src/hooks/useSpecTrace.ts                     (new — useQuery wrapper sobre invoke('dashboard_spec_trace'))
apps/dashboard/src/pages/Workspace.tsx                       (modify — remover import e render de WorkspaceEventsFeed, adicionar <ExecutionTrace /> ou link p/ /specs)
apps/dashboard/src/pages/Specs.tsx                           (modify — adicionar tab "Trace" usando <ExecutionTrace />; pode manter timeline existente OU substituir — decisão fina)
```

## Tarefas

### Tauri Backend Agent (6a)

- [ ] Em `apps/dashboard/src-tauri/src/telemetry.rs`, adicionar struct `TraceNode { kind: String, label: String, tokens: Option<TokenBreakdown>, duration_ms: Option<i64>, ts: Option<String>, payload: Option<serde_json::Value>, children: Vec<TraceNode> }` + `TokenBreakdown { input: i64, output: i64, cache_read: i64, cache_creation: i64, cost_usd_micros: Option<i64> }`. Derive `Serialize, Deserialize`.
- [ ] Adicionar `#[tauri::command] pub fn dashboard_spec_trace(project_path: String, spec_name: String) -> Result<TraceNode, String>`. Implementação: abrir conn via `mustard_core::economy::store::open_for(&project_path)`, query 1) `agent.start`/`agent.stop` events filtrados por `spec=spec_name`, 2) `per_agent_costs(EconomyScope::Spec)` do reader W4, 3) `tool.use` events do mesmo spec. Construir hierarquia: nó raiz `spec` → 1 filho `wave` por wave_id distinto → 1 filho `agent` por agent_id distinto naquela wave → 1 filho `tool` por evento `tool.use`. Tokens agregados sobem do tool → agent → wave → spec.
- [ ] Registrar `dashboard_spec_trace` no `tauri::generate_handler![]` em `lib.rs`.

### Frontend Trace Agent (6b — DEPENDE de 6a)

- [ ] Criar tipo `TraceNode` em `apps/dashboard/src/lib/types/trace.ts` (espelha o serde do backend). Export tipos `TokenBreakdown` e `TraceNode`.
- [ ] Criar `apps/dashboard/src/hooks/useSpecTrace.ts`: `export function useSpecTrace(projectPath, specName) { return useQuery({ queryKey: ['spec-trace', projectPath, specName], queryFn: () => invoke<TraceNode>('dashboard_spec_trace', { projectPath, specName }) }) }`.
- [ ] Criar `apps/dashboard/src/components/trace/ExecutionTrace.tsx`: componente recursivo (helper interno `<TraceNodeRow node depth />`). Para cada nó: usa `<BaseRow>` (W5) com icon variando por kind (`Square` p/ spec, `Layers` p/ wave, `Cpu` p/ agent, `Wrench` p/ tool — Lucide), `MetricsPill` p/ tokens, badge p/ duration. Children aninhados via `<TreeNode>` (W5). Click expand/collapse via `<details>` nativo. Memo com `React.memo` para evitar re-render em árvore grande.
- [ ] Criar `apps/dashboard/src/components/trace/ToolEventRow.tsx`: especialização para `kind=tool`. Baseado em `payload.tool_name`: `Edit`/`Write` → `<DiffViewer before={payload.before} after={payload.after} mode="unified" />`; `Read` → `<CodeBlock code={payload.content_excerpt} lang={detectLang(payload.path)} />`; `Bash` → `<CodeBlock code={payload.command + '\n---\n' + payload.stdout} lang="plain" />`; default → JSON `<CodeBlock code={JSON.stringify(payload, null, 2)} lang="json" />`. Tudo lazy: só renderiza payload quando o `<details>` está aberto.
- [ ] Editar `apps/dashboard/src/pages/Workspace.tsx`: remover import + render de `WorkspaceEventsFeed`. Substituir por `<ExecutionTrace projectPath={...} specName={primarySpecName} />` SE houver spec ativo; senão, empty state "Sem pipeline ativo — veja /specs".
- [ ] Editar `apps/dashboard/src/pages/Specs.tsx`: adicionar tab/seção "Trace" no SpecDrillDown que renderiza `<ExecutionTrace>` para o spec selecionado. Pode coexistir com timeline atual ou substituí-la — decidir baseado no que faz mais sentido visualmente (default: coexistir, marcar timeline como `(legacy)` ou movê-la pra "Eventos linear" tab).
- [ ] Rodar `pnpm --filter mustard-dashboard build` + `cargo check -p mustard-dashboard --manifest-path apps/dashboard/src-tauri/Cargo.toml` — ambos verdes.

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

## Concerns

- **`TokenBreakdown` parcial: W4 reader retorna tokens combined (input+output) sem split** — agente surface o total no campo `input` e deixa `output`/`cache_read`/`cache_creation` em 0. Tooltip na `<MetricsPill>` é honesto sobre isso. Rollup per-token-type pode encher os outros campos sem reshape do DTO.
- **Timeline existente foi mantida (coexiste)** — adicionou nova tab "Trace" no `SpecDrillDown` entre "Ondas" e "Qualidade", deixou "Eventos"/"Timeline" intactos. Linear views respondem "o que aconteceu em ordem"; Trace responde "como o trabalho foi decomposto". REVIEW pode decidir aposentar a timeline.
- **Primary active spec heuristic reusada** — `<ExecutionTrace>` no Workspace.tsx usa o mesmo first-non-terminal-track que o `PipelineTimeline` já usava. Bom: trace e timeline sempre concordam sobre "o que está rodando". Risco: se houver 2+ specs ativas, só uma vira foco no Workspace; usuário precisa ir em /specs pra ver outras.
