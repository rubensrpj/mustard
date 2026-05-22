# Wave 3 — Dashboard: aba "Network" no SpecDrillDown renderiza grafo wikilink

### Parent: [[2026-05-20-mustard-wave-network-standard]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full (wave)
### Checkpoint: 2026-05-20T21:05:00Z
### Lang: pt

## PRD

## Contexto

Com `mustard-rt run wikilink-extract` produzindo JSON estruturado (de [[wave-1-rt-infra]]) e o pattern Tauri command existente em `spec_views.rs`, esta wave expõe a rede pra UI. `SpecDrillDown` ganha uma aba nova ("Network") que renderiza dois blocos: (a) grafo das wikilinks da spec corrente (parent, children, paralelas, sucessoras), (b) lista de memórias por wave (lê via novo Tauri command que faz bridge para `memory cross-wave`).

Grafo: começa simples — render SVG estático com nós (specs e waves) e arestas (links), sem layout dinâmico. Suficiente pra eliminar a invisibilidade atual; força-direcionado fica pra futuro.

## Métrica de sucesso

Operador abre `SpecDrillDown` de uma spec wave-decomposed, clica em "Network", vê o grafo com parent/waves/dependências e o conteúdo de memória de cada wave. Cada nó é clicável e abre o `SpecDrillDown` daquela spec.

## Não-Objetivos

- Não implementar layout force-direcionado (SVG estático com posicionamento manual por níveis basta).
- Não permitir edição do grafo pela UI.
- Não tocar outras abas do `SpecDrillDown`.
- Não criar nova rota — só nova aba dentro do drill-down.

## Acceptance Criteria

- [x] AC-1: Build do dashboard passa — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: `SpecNetworkTab.tsx` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/components/specs/SpecNetworkTab.tsx'))throw new Error('missing')"`
- [x] AC-3: `SpecDrillDown.tsx` importa a aba — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecDrillDown.tsx','utf8');if(!t.includes('SpecNetworkTab'))throw new Error('not wired')"`
- [x] AC-4: Tauri command `dashboard_wikilink_extract` registrado — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/main.rs','utf8');if(!t.includes('dashboard_wikilink_extract'))throw new Error('missing handler')"`
- [x] AC-5: Cargo check passa no crate Tauri — Command: `cargo check -p dashboard --manifest-path apps/dashboard/src-tauri/Cargo.toml`

## Plano

## Arquivos (~4)

```
apps/dashboard/src-tauri/src/spec_views.rs          (modify — +2 commands bridge: wikilink_extract, memory_cross_wave)
apps/dashboard/src-tauri/src/main.rs                (modify — register handlers)
apps/dashboard/src/lib/dashboard.ts                 (modify — 2 invoke wrappers)
apps/dashboard/src/components/specs/SpecNetworkTab.tsx   (new)
apps/dashboard/src/components/specs/SpecDrillDown.tsx    (modify — add tab)
```

## Tarefas

### Frontend Agent

- [ ] Em `apps/dashboard/src-tauri/src/spec_views.rs`:
  - `#[tauri::command] fn dashboard_wikilink_extract(spec_dir: String) -> Result<WikilinkExtract, String>` — invoca `mustard-rt run wikilink-extract --spec-dir <dir>` via `std::process::Command` e parseia JSON
  - `#[tauri::command] fn dashboard_memory_cross_wave(spec: String, wave: u32) -> Result<String, String>` — invoca `mustard-rt run memory cross-wave --spec <s> --wave <N>` e retorna stdout (markdown)
  - Structs `WikilinkExtract { wikilinks: Vec<Wikilink>, orphans: Vec<String> }`, `Wikilink { from, to, file, line }`
- [ ] Em `main.rs`, registrar os 2 handlers
- [ ] Em `lib/dashboard.ts`: 2 wrappers tipados + interfaces espelho
- [ ] `apps/dashboard/src/components/specs/SpecNetworkTab.tsx`:
  - Prop: `specDir: string`, `specName: string`
  - Hook `useQuery` consumindo `dashboardWikilinkExtract(specDir)`
  - Render bloco 1 — Grafo: SVG simples com 3 camadas (parent no topo, waves no meio agrupadas por número, dependentes embaixo). Nós = `<rect>` + `<text>`, arestas = `<line>`. Click no nó: `navigate('/specs#'+nodeName)`.
  - Render bloco 2 — Memórias por wave: para cada wave detectada, chamar `dashboardMemoryCrossWave(specName, wave_number+1)` e renderizar o markdown via `react-markdown` (já existe).
  - Estado vazio: `<EmptyState title="Sem wikilinks detectados" description="Esta spec não tem wave-files ou referências [[name]]." />`
- [ ] Em `SpecDrillDown.tsx`:
  - Adicionar `"network"` à enum de tabs
  - Render condicional do `<SpecNetworkTab specDir={spec.dir} specName={spec.name} />`
- [ ] `pnpm --filter mustard-dashboard build`

## Dependências

- [[wave-1-rt-infra]]: precisa dos subcomandos `wikilink-extract` e `memory cross-wave`.

## Network

- Parent: [[2026-05-20-mustard-wave-network-standard]]
- Depende de: [[wave-1-rt-infra]]
- Paralela a: [[wave-2-skill-template]]
- Recebe memória: [[wave-1-rt-infra]] (JSON shape do `wikilink-extract`, formato markdown do `memory cross-wave`).
- Grava memória: `{components_created: ['SpecNetworkTab'], commands_added: [...], notes: '...'}`.

## Limites

Em escopo: `apps/dashboard/src-tauri/src/{spec_views,main}.rs`, `apps/dashboard/src/lib/dashboard.ts`, `apps/dashboard/src/components/specs/{SpecNetworkTab,SpecDrillDown}.tsx`.

Fora de escopo: outras abas do drill-down, outras pages, layout, mustard-rt (já feito).
