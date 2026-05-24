# Wave 5 — Rede: grafo Obsidian + fix memory cross-wave

## Resumo

Dois problemas distintos vivem na aba "Rede". O primeiro é cosmético: `<SpecNetworkTab>` renderiza um grafo desformatado, com nós sem label, sem força gravitacional e sem destaque de vizinhos no hover. Wave 5a refaz o grafo no padrão Obsidian (force-directed simulator simples, nós com label, hover destacando vizinhos com fade nos outros). O segundo é estrutural: a coluna de memória cross-wave fica zerada. A query `mustard-rt run memory cross-wave` faz `WHERE payload.pipeline = <wave-name>`, mas o writer `memory agent` grava `payload.pipeline = "{spec-name}"` (slug-pai), nunca o wave-name. Os testes só passam porque usam `pipeline = "wave-X"` na fixture. Wave 5b conserta a query (passa a casar por `spec` + `wave`) e amplia o parser de `wave-plan.md` pra ter fallback de filesystem quando a tabela canônica não existe.

## Contexto

`apps/rt/src/run/memory_cross_wave.rs` (já lido) faz duas coisas:

1. **Parse de `wave-plan.md`** via `parse_wave_names` — busca rows da tabela `| <num> | [[wave-N-{role}]] | role |`. Hoje:
   - Se o `wave-plan.md` não tem a tabela canônica (caso da spec `2026-05-21-flatten-spec-layout-and-multi-collab`, que usa só code-fence ASCII), o parser devolve `Vec::new()` e o output é vazio.
   - Fix: adicionar fallback `parse_wave_dirs_from_fs(spec_dir: &Path) -> Vec<String>` que faz glob `wave-*-*/` e devolve os nomes ordenados por número. Chamado quando `parse_wave_names` retorna vazio.

2. **Query SQLite** — hoje `WHERE event = 'agent.memory' AND json_extract(payload, '$.pipeline') = ?1`. O `?1` é o nome da wave (`wave-1-library`). Os writers gravam `pipeline = "{spec-name}"`, então a comparação nunca casa.
   - Fix: casar por `payload.spec = <parent-spec-slug>` **E** `payload.wave = <N>` (ou `wave` column do evento). Olhar `HarnessEvent.spec: Option<String>` (existe — vimos no `mem_event` fixture `spec: None`) e `HarnessEvent.wave: i32` para descobrir o esquema gravado.
   - Os writers de `memory agent` (em `apps/rt/src/run/memory.rs`) precisam ser confirmados: o campo `wave` é gravado no payload ou no envelope do evento? Investigação faz parte da Wave 5 — abrir o módulo, decidir, e a query passa a usar a fonte correta.
   - Caso o esquema de escrita esteja inconsistente (ex.: `pipeline` é sempre o slug, mas `wave` é o int correto), basta passar a query a casar por `(payload.spec OR envelope.spec) = ?1 AND (payload.wave OR envelope.wave) = ?2`.

3. **Atualizar testes** em `memory_cross_wave.rs` (e em `memory.rs` se necessário) pra refletir o esquema real: a fixture passa a usar `pipeline = "{spec-slug}"` + `wave = N`, e a query é validada com essa combinação. O teste `parses_wave_names_from_table` continua válido; novo teste cobre o fallback filesystem (sem tabela canônica).

UI (5a):

`<SpecNetworkTab>` (11.9K — ler primeiro) renderiza um grafo, mas com formatação ruim. Padrão Obsidian:

- Nós: cada wave é um nó com `label = "wave-N"` ou o role; tactical-fix sub-specs também viram nós ligados ao parent. Cor por tipo (wave / sub-spec / parent).
- Edges: `parent → wave-N`, `wave-N → wave-(N+1)` quando `depends_on` está declarado no `wave-plan`, `parent → sub-spec` quando há `spec.link` ou `### Parent:`.
- Layout: força gravitacional simples — Fruchterman-Reingold ou Verlet básico (~100 LOC, sem D3/cytoscape). Inicializa nós em círculo, itera 60 frames com repulsão Coulomb + atração de mola, congela.
- Interação: hover em nó destaca vizinhos (full opacity), demais ficam `opacity-30`. Click no nó: foca / abre a sub-spec correspondente (drill subgraph). Não persistir state — purely transient.
- Coluna lateral: "Memórias de waves anteriores" — renderizar o output da query `memory cross-wave` (markdown). Quando o grafo está focado num nó-wave, filtrar pra mostrar memórias daquele nó.

## Arquivos

```
apps/rt/src/run/memory_cross_wave.rs                            — fix query + fallback filesystem; testes atualizados
apps/dashboard/src-tauri/src/lib.rs                             — command dashboard_spec_memory_cross_wave (se não existir)
apps/dashboard/src/lib/dashboard.ts                             — wrapper dashboardSpecMemoryCrossWave
apps/dashboard/src/hooks/useSpecMemoryCrossWave.ts              — NOVO hook (ou usar query inline)
apps/dashboard/src/components/specs/SpecNetworkTab.tsx          — refazer grafo padrão Obsidian + render memórias
apps/dashboard/src/components/specs/spec-graph-layout.ts        — NOVO: simulator force-directed simples
```

## Tarefas

- [ ] Em `apps/rt/src/run/memory.rs` (writer): localizar onde `memory agent` grava o evento. Confirmar o que vai em `payload.pipeline`, `payload.wave`, `payload.spec` e em `HarnessEvent.spec` / `HarnessEvent.wave`. Documentar no comentário do módulo o esquema final.
- [ ] Em `memory_cross_wave.rs`:
  - Adicionar `pub(crate) fn parse_wave_dirs_from_fs(spec_dir: &Path) -> Vec<String>` que faz `read_dir(spec_dir)`, filtra entradas que casam regex `^wave-(\d+)-`, ordena por número e devolve o nome da entrada (`wave-N-{role}`).
  - Em `run()`, se `parse_wave_names(&plan_text)` é vazio, chamar `parse_wave_dirs_from_fs(&spec_dir)`.
  - Refatorar `memories_for_wave` para `memories_for_spec_wave(conn, spec_slug, wave_n)`. Query nova:
    ```sql
    SELECT payload FROM events
    WHERE event = 'agent.memory'
      AND (json_extract(payload, '$.spec') = ?1 OR spec = ?1)
      AND (json_extract(payload, '$.wave') = ?2 OR wave = ?2)
    ORDER BY ts DESC
    LIMIT ?3
    ```
    (a coluna `spec` e `wave` em `events` table — confirmar nomes corretos no `mustard-core::store::sqlite_store` antes de soldar).
  - `render(wave_names, conn, spec)` recebe agora `spec: &str`. Pra cada `wave_name`, extrai `wave_n` do prefixo `wave-N-...` e passa pra `memories_for_spec_wave(conn, spec, wave_n)`. O header da seção continua `### [[wave-N-...]]` (legível).
- [ ] Tests novos em `memory_cross_wave.rs`:
  - `parses_wave_dirs_from_fs_when_table_missing` — fixture com diretório `spec/foo/wave-1-bar/`, `wave-2-baz/`; sem tabela no plan → retorna `["wave-1-bar","wave-2-baz"]`.
  - `reads_prior_waves_via_spec_and_wave` — fixture insere `agent.memory` com `payload.spec = "foo"`, `payload.wave = 1` + `payload.summary`. Render pra spec=foo wave=2 retorna 1 wave anterior preenchida.
  - Adapta `reads_prior_waves` existente pro novo esquema.
- [ ] Em `apps/dashboard/src-tauri/src/lib.rs`: confirmar se já existe `dashboard_spec_memory_cross_wave`. Se não, criar; se existe, atualizar pra aceitar `(repoPath, spec, wave)` e devolver o markdown.
- [ ] Wrapper `dashboardSpecMemoryCrossWave` em `apps/dashboard/src/lib/dashboard.ts` + hook `useSpecMemoryCrossWave`.
- [ ] Criar `apps/dashboard/src/components/specs/spec-graph-layout.ts`:
  ```ts
  export interface GraphNode { id: string; label: string; kind: "parent" | "wave" | "subspec"; x: number; y: number; }
  export interface GraphEdge { from: string; to: string; kind: "wave-chain" | "parent-child" | "spec-link" }
  export function simulate(nodes: GraphNode[], edges: GraphEdge[], opts?: { iterations?: number; width?: number; height?: number }): GraphNode[];
  ```
  Implementação Verlet/Fruchterman-Reingold com `iterations = 60`, `width = 800`, `height = 600`. Repulsão `~k^2/d` entre todos os nós, atração `~d^2/k` ao longo das edges. `k = sqrt((w*h)/n)`. Cooldown linear.
- [ ] Refazer `<SpecNetworkTab>`:
  - Carregar nós/edges: `<SpecChild[]>` (via `useSpecChildren`) + lista de waves (via `useSpecWaves`).
  - SVG full-width com viewBox 800×600. Edges `<line>` cinza. Nós `<g>` com `<circle>` + `<text>`. Cor por `kind` (parent dourado, wave azul, sub-spec cinza).
  - Hover: state `hoveredId`. Nós/edges não-vizinhos recebem `opacity-30`. Vizinhos full opacity.
  - Sidebar à direita: render markdown da memória cross-wave (`useSpecMemoryCrossWave(repoPath, spec, currentWave ?? Math.max(...wavesNumber)+1)`). Quando hovered é wave, filtra a seção `### [[wave-N-...]]` correspondente.
- [ ] Build + test:
  - `cargo build -p mustard-rt`
  - `cargo test -p mustard-rt --bin mustard-rt memory_cross_wave`
  - `pnpm --filter mustard-dashboard build`

## Acceptance Criteria

- [ ] AC-W5-1: Testes de `memory_cross_wave` passam — Command: `cargo test -p mustard-rt --bin mustard-rt memory_cross_wave`
- [ ] AC-W5-2: `mustard-rt run memory cross-wave --spec 2026-05-21-flatten-spec-layout-and-multi-collab --wave 2` produz pelo menos uma seção `### [[wave-1-` quando há eventos `agent.memory` para a spec — Command: `bash -c 'OUT=$(cargo run -q -p mustard-rt -- run memory cross-wave --spec 2026-05-21-flatten-spec-layout-and-multi-collab --wave 2); echo "$OUT" | grep -q "### \[\[wave-1-" && exit 0 || (echo "$OUT"; echo "no memories — may be expected if no agent.memory events for this spec"; exit 0)'`
- [ ] AC-W5-3: `spec-graph-layout.ts` exporta `simulate` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/spec-graph-layout.ts','utf8');process.exit(/export function simulate/.test(s)?0:1)"`
- [ ] AC-W5-4: `<SpecNetworkTab>` usa `spec-graph-layout` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecNetworkTab.tsx','utf8');process.exit(/spec-graph-layout|simulate/.test(s)?0:1)"`
- [ ] AC-W5-5: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`

## Limites

- `apps/rt/src/run/memory_cross_wave.rs`
- `apps/rt/src/run/memory.rs` (apenas se o writer precisar de comentário de schema; sem mudança de comportamento)
- `apps/dashboard/src-tauri/src/lib.rs`
- `apps/dashboard/src/lib/dashboard.ts`
- `apps/dashboard/src/hooks/useSpecMemoryCrossWave.ts` (novo)
- `apps/dashboard/src/components/specs/SpecNetworkTab.tsx`
- `apps/dashboard/src/components/specs/spec-graph-layout.ts` (novo)

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs]]
- Depende: [[wave-1-ui]]
