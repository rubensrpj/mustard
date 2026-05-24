# Wave 3 — Rede: mapa mental radial full-painel

## Resumo

O grafo da aba "Rede" entregue na parent ficou pequeno (`aspect-[4/3]`, ~600px de altura no melhor caso) e o layout force-directed não ficou legível com ~6-8 nós. Refazer como **mapa mental radial**: parent no centro, waves em órbita ao redor (anel 1), sub-specs como filhos das waves no anel 2. SVG ocupa o painel inteiro (altura via `100%` flexível, sem `aspect-*`). Layout determinístico (sem simulação iterativa), legível por hierarquia.

## Contexto

`spec-graph-layout.ts` hoje implementa Fruchterman-Reingold. Pra mapa mental, a fórmula é direta:
- Parent node: `(cx, cy) = (width/2, height/2)`.
- N waves: distribuídos em círculo de raio `R1 = min(w, h) * 0.3`. Ângulo `θ_i = 2π * i / N`.
- M sub-specs por wave: distribuídos em arco ao redor da wave-parent. Raio `R2 = min(w, h) * 0.15`. Ângulo centrado no `θ_i` da wave, com spread total ≤ π/3 (60°).

Layout determinístico, sem RNG, sem iteração. Fácil de raciocinar e responsivo (recalcula on resize via `ResizeObserver`).

Edges:
- Parent → wave-N: linha curva ou reta. Estilo principal.
- wave-N → sub-spec: linha mais fina, cor neutra.
- wave-(N-1) → wave-N (chain): arco fino tracejado opcional.

Hover destaca neighbors (mesma lógica do force-directed atual).

## Arquivos

```
apps/dashboard/src/components/specs/spec-graph-layout.ts        — adicionar exportação `radialLayout(nodes, edges, opts)`
apps/dashboard/src/components/specs/SpecNetworkTab.tsx          — trocar simulate por radialLayout, SVG full-size com ResizeObserver
```

## Tarefas

- [ ] Em `spec-graph-layout.ts`, adicionar:
  ```ts
  export function radialLayout(
    nodes: GraphNode[],
    edges: GraphEdge[],
    opts?: { width?: number; height?: number; r1Ratio?: number; r2Ratio?: number; subspecSpread?: number }
  ): GraphNode[];
  ```
  Implementação:
  1. Encontra o parent node (`kind === "parent"`).
  2. Lista wave nodes ordenados por id ascendente.
  3. Mapeia sub-spec parents: pra cada sub-spec edge `wave-N → sub-spec`, agrupa sub-specs por wave.
  4. Coloca parent em centro.
  5. Para cada wave i (1..N): `θ_i = 2π * (i-1) / N - π/2` (começa no topo). Posição: `(cx + R1*cos(θ), cy + R1*sin(θ))`.
  6. Para cada sub-spec de wave-i: posiciona em arco ao redor da wave. Centro do arco = `θ_i`; spread = `subspecSpread` (default π/3); raio R2.
  7. Retorna novos `GraphNode[]` com x/y atualizados. Nodes sem categoria conhecida ficam onde estão (input passthrough).
  - Sem mutação do input. Determinístico (sem `Math.random`).
- [ ] Em `SpecNetworkTab.tsx`:
  - Trocar `simulate(...)` por `radialLayout(...)`.
  - SVG: tirar `aspect-[4/3]`, usar `height: 100%` num container `flex-1 min-h-[500px]` (ou maior). Adicionar `<ResizeObserver>` (via `useRef` + `new ResizeObserver`) que lê `clientWidth/clientHeight` e re-roda layout. Reactor `useMemo([clientSize, nodes, edges])`.
  - viewBox dinâmico: `0 0 ${w} ${h}`.
  - Sidebar de memórias mantida (escala média).
  - Nó parent: maior (`r=10`), label maior (`text-[13px]` em vez de `text-[11px]`).
  - Edges curvas em vez de retas: `<path d={`M${a.x},${a.y} Q${midX},${midY} ${b.x},${b.y}`} />` com `midX/midY` ligeiramente desviado pra fora do raio. Opcional — começa com retas, se ficar feio promove pra curvas.
- [ ] Manter o destacamento de hover (mesma lógica de `neighbors` set).
- [ ] Build: `pnpm --filter mustard-dashboard build`

## Acceptance Criteria

- [ ] AC-W3-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W3-2: `spec-graph-layout.ts` exporta `radialLayout` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/spec-graph-layout.ts','utf8');process.exit(/export function radialLayout/.test(s)?0:1)"`
- [ ] AC-W3-3: `SpecNetworkTab.tsx` usa `radialLayout` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecNetworkTab.tsx','utf8');process.exit(/radialLayout/.test(s)?0:1)"`
- [ ] AC-W3-4: SVG NÃO tem aspect-[4/3] — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecNetworkTab.tsx','utf8');process.exit(/aspect-\\[4\\/3\\]/.test(s)?(console.error('aspect ainda hard-coded'),1):0)"`

## Limites

- `apps/dashboard/src/components/specs/spec-graph-layout.ts`
- `apps/dashboard/src/components/specs/SpecNetworkTab.tsx`

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs-polish]]
- Depende: [[wave-1-ui]] (paralelizável com W2/W4)
