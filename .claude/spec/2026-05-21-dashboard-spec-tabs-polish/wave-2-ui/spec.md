# Wave 2 — Ondas restruct: Onda #0 + pin + sub-specs aninhadas

### Parent: [[2026-05-21-dashboard-spec-tabs-polish]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T17:00:00Z

## Resumo

Três mudanças relacionadas na aba "Ondas": (3) remover o botão de markdown viewer do `<SpecCard>` (que abre modal pelo card) e mover esse acesso para a aba Ondas tratando a spec principal como "Onda #0"; (4) o drawer da onda ganha botão pin que alterna entre overlay (Sheet) e inline (embeddado no painel); (8) sub-specs deixam de ser uma aba separada e viram subnível de cada onda — uma onda expandível mostra as sub-specs que ela gerou.

## Contexto

**Item (3) — Onda #0 = parent spec.** Hoje cada `<SpecCard>` na lista tem um botão `<FileText>` que abre um modal `<SpecMarkdownViewer>`. Esse ícone polui o card e dá um caminho redundante para ver o markdown. Mover esse acesso para dentro da aba Ondas como "Onda #0" (linha 0 da lista, label "Spec principal" ou nome do parent, role = "parent") fechando o ciclo: tudo que é markdown vive na aba Ondas. Clicar na Onda #0 abre o drawer com o `wave-plan.md` (ou `spec.md` em single-spec).

**Item (4) — Pin no drawer.** Hoje o `<WaveMarkdownDrawer>` é um Sheet à direita (overlay). O usuário quer poder fixar o conteúdo INLINE dentro do painel da aba Ondas (lado a lado: lista de ondas à esquerda, markdown da onda selecionada à direita, sem overlay). Botão pin/unpin no header do drawer alterna entre os modos. Estado vive em `<SpecDetailDashboard>` (`drawerPinned: boolean`).

**Item (8) — Sub-specs aninhadas.** Hoje sub-specs vivem na aba "Sub-specs" do drill-down. O usuário quer que sub-specs apareçam como filhos da onda que as gerou (UX tipo árvore: Onda 3 ▾, dentro: tactical-fix-A, tactical-fix-B). A aba "Sub-specs" some. Correlação sub-spec ↔ wave: usa `started_at` do `ChildEntry` comparado com o intervalo `[wave.started_at, wave.completed_at]` da onda; sub-spec criada DURANTE o range é filha da onda. Sub-spec criada APÓS a última onda (ex.: tactical-fix no CLOSE) vira filha da última onda (ou de "Onda #0" no sentido genérico). Sub-spec sem `started_at` vai para um bucket "Sem onda correlacionada" no fim.

## Arquivos

```
apps/dashboard/src/components/specs/SpecCard.tsx                — remover botão FileText
apps/dashboard/src/components/specs/SpecDrillDown.tsx           — remover aba Sub-specs (TABS de 5 → 4)
apps/dashboard/src/components/specs/SpecDetailDashboard.tsx     — drawerPinned state + render condicional
apps/dashboard/src/components/specs/SpecWavesTab.tsx            — Onda #0 row + waves expandíveis com sub-specs
apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx      — pin/unpin button + modo inline
apps/dashboard/src/components/specs/SpecChildrenTab.tsx         — DELETADO (reaproveita lógica em SpecWavesTab)
apps/rt/src/run/spec_children.rs                                — adiciona campo wave: Option<u32> em ChildEntry
apps/dashboard/src-tauri/src/spec_views.rs                      — propagar wave no SpecChild
apps/dashboard/src/lib/types/specs.ts                           — SpecChild.wave: number | null
```

## Tarefas

- [ ] **(3) Remover FileText do SpecCard.** Em `SpecCard.tsx`: deletar import `FileText`, state `viewerOpen`, render do `<button>` que abre o viewer, e o `<SpecMarkdownViewer>` no JSX. Manter `<SpecActionMenu>` (kebab) intacto. Confirme que o card ainda tem o botão "Detalhes" (W1 da parent) — esse continua.
- [ ] **(3) Onda #0 no SpecWavesTab.** Em `SpecWavesTab.tsx`, prepende uma linha "Onda #0" no topo da lista:
  - `wave: 0`, `role: "parent"`, `status: "completed"` (assume parent já existe).
  - Label visual: `<span className="text-[12px] font-medium">Spec principal</span>` ou `<span>#0</span> <span>{specName}</span>`.
  - Cor distinta (e.g., border `--color-accent-mustard/40`).
  - Click abre o drawer com o `wave-plan.md` (ou `spec.md` para single-spec). Reusa o `useSpecWaveFiles` hook mas com `wave=0` — o command `dashboard_spec_wave_files` precisa aceitar `wave=0` e devolver o markdown do root (`wave-plan.md` ou `spec.md`).
  - Atualizar `mustard-rt run wave-files` para tratar `wave=0`: resolve `<repo>/.claude/spec/{spec}/wave-plan.md` (primeiro) ou `spec.md` (fallback). Adicionar test.
- [ ] **(4) Pin no drawer.** Em `WaveMarkdownDrawer.tsx`:
  - Receber prop `pinned: boolean` + `onPinChange: (p: boolean) => void`.
  - Quando `pinned === false`: comportamento atual (Sheet overlay).
  - Quando `pinned === true`: renderiza inline como `<aside className="border-l border-border bg-card/30 p-3">` à direita da lista de ondas dentro do painel da aba Ondas. NÃO Sheet.
  - Header do drawer tem botão `<Pin>` (lucide) que toggle. Tooltip "Fixar dentro do painel" / "Soltar como janela".
- [ ] **(4) State no SpecDetailDashboard.** `drawerPinned, setDrawerPinned: useState(false)`. Propaga pro `<SpecDrillDown>` → `<SpecWavesTab>` para o drawer renderizar inline.
- [ ] **(8) ChildEntry com wave em rt.** Em `apps/rt/src/run/spec_children.rs`:
  - Adicionar `pub wave: Option<u32>` em `ChildEntry`.
  - Em `list_children`, para cada entry vinda do SQLite (Set A), correlacionar com waves do parent: query SQLite para `pipeline.wave.complete` events do parent, montar mapa `wave → (started_at, completed_at)`. Para cada child, se `started_at` cai dentro de um intervalo, marca `wave = Some(N)`. Senão, `wave = None`.
  - Entry vinda apenas do header (Set B) sem `started_at` no SQLite → `wave = None`.
  - Atualizar testes.
- [ ] **(8) Propagar wave no SpecChild.** Em `apps/dashboard/src-tauri/src/spec_views.rs`: `ChildEntryRaw` deserializa `wave: Option<u32>`. `SpecChild` adiciona `wave: Option<u32>`.
- [ ] **(8) Tipo TS.** Em `apps/dashboard/src/lib/types/specs.ts`: `SpecChild.wave?: number | null`.
- [ ] **(8) SpecWavesTab — ondas expandíveis.** Refatorar `WaveLi`:
  - State `expanded: boolean` por linha. Chevron `>` (ou `▾`) à esquerda do número da onda.
  - Quando expanded: render sub-lista com sub-specs cuja `child.wave === wave.wave`. Item: pill de status + slug + duração.
  - Sub-specs sem wave correlacionada → bucket único no fim "Sem onda" (renderizado fora do loop principal).
- [ ] **(8) Remover aba Sub-specs.** Em `SpecDrillDown.tsx`: tirar `"Sub-specs"` do array `TABS`, remover o `<TabsContent value="Sub-specs">` e a query `useSpecChildren` daqui (que agora vive em `SpecWavesTab` ou no `SpecDetailDashboard` propagando pra `SpecWavesTab`).
- [ ] **(8) Deletar SpecChildrenTab.** Se nenhum consumer remanescente, deletar `SpecChildrenTab.tsx`. Caso contrário, manter mas remover do drill-down.
- [ ] Build: `cargo build -p mustard-rt && pnpm --filter mustard-dashboard build`

## Acceptance Criteria

- [ ] AC-W2-1: Build full passa — Command: `cargo check --workspace && pnpm --filter mustard-dashboard build`
- [ ] AC-W2-2: Testes rt passam — Command: `cargo test -p mustard-rt --bin mustard-rt --no-fail-fast`
- [ ] AC-W2-3: SpecCard NÃO renderiza FileText/SpecMarkdownViewer — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(/FileText|SpecMarkdownViewer/.test(s)?(console.error('FileText/Viewer ainda no SpecCard'),1):0)"`
- [ ] AC-W2-4: SpecDrillDown TABS array NÃO contém "Sub-specs" — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecDrillDown.tsx','utf8');const m=s.match(/const\s+TABS\s*=\s*\[([^\]]+)\]/);process.exit(m && !/Sub-specs/.test(m[1])?0:(console.error('aba Sub-specs ainda no array TABS'),1))"`
- [ ] AC-W2-5: WaveMarkdownDrawer expõe prop `pinned` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx','utf8');process.exit(/pinned\??:\s*boolean|onPinChange/.test(s)?0:1)"`
- [ ] AC-W2-6: SpecChild type tem campo wave — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/lib/types/specs.ts','utf8');process.exit(/wave\??:\s*number/.test(s)?0:1)"`

## Limites

- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/components/specs/SpecDrillDown.tsx`
- `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx`
- `apps/dashboard/src/components/specs/SpecWavesTab.tsx`
- `apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx`
- `apps/dashboard/src/components/specs/SpecChildrenTab.tsx` (deletado se possível)
- `apps/dashboard/src-tauri/src/spec_views.rs`
- `apps/dashboard/src/lib/types/specs.ts`
- `apps/rt/src/run/spec_children.rs`
- `apps/rt/src/run/wave_files.rs` (tratar wave=0)

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs-polish]]
- Depende: [[wave-1-ui]] (SpecWavesTab ganha mudanças nas duas waves; W1 vai primeiro)
