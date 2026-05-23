# Wave 2 — Primitives consolidados (ds/* → page/*, PageSurface novo, MetricsPill→StatPill)

### Parent: [[2026-05-23-dashboard-design-system]]
### Stage: Close
### Outcome: Completed
### Flags:
### Scope: full (wave 2 of 5)
### Lang: pt
### Checkpoint: 2026-05-23T00:00:00Z

## Resumo

Consolidar fisicamente o barril legado `components/ds/` dentro do barril canônico `components/page/`, criar a primitiva `PageSurface` (wrapper de 1ª camada com ritmo editorial 80px), renomear `MetricsPill` para `StatPill` (nome alinhado ao DESIGN.md Binance), e reapontar todos os 22 arquivos consumidores. `ds/` é resíduo de uma tentativa de design system pré-Binance (índigo/violeta com tokens `--ds-*` já removidos na Wave 1); `page/` é a única fonte de primitivas pós-DESIGN.md. Sem mudança de comportamento, sem refit visual, sem novas primitivas além do `PageSurface` — apenas move + rename + create-wrapper + find/replace. No fim da wave: zero referências vivas a `@/components/ds` ou `MetricsPill`, `components/ds/` deletado, e `PageSurface` disponível em `@/components/page` para Wave 3 montar o shell.

## Network

- Parent: [[2026-05-23-dashboard-design-system]]
- Depende de: [[wave-1-general]] (tokens Binance precisam estar em vigor — `--editorial-band-py: 80px`, `--background: #0b0e11`)
- Habilita: [[wave-3-ui]] (shell precisa do PageSurface), [[wave-4-ui]] (pages high-traffic compõem PageSurface + StatPill), [[wave-5-ui]] (pages secondary idem)

## Component Contract — PageSurface

**Arquivo novo:** `apps/dashboard/src/components/page/PageSurface.tsx`

```tsx
import { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface PageSurfaceProps {
  children: ReactNode;
  className?: string;
  /**
   * Aplica o ritmo editorial Binance (80px vertical padding via --editorial-band-py).
   * Default true. Use false apenas em sub-páginas embedadas (split-detail content).
   */
  editorial?: boolean;
}

export function PageSurface({ children, className, editorial = true }: PageSurfaceProps) {
  return (
    <div
      className={cn(
        "flex flex-col gap-8 w-full max-w-7xl mx-auto px-6",
        editorial && "py-20",
        className,
      )}
    >
      {children}
    </div>
  );
}
```

**Justificativa (por que essa primitiva merece existir):**
Hoje cada uma das 11 páginas reescreve um wrapper hand-rolled (`<div className="flex flex-col gap-6 w-full">`, `<div className="space-y-6 max-w-6xl">`, `<main className="container py-8">` — três padrões diferentes só nas 4 pages high-traffic). O DESIGN.md Binance prescreve banda editorial de 80px (`--editorial-band-py`) e largura máxima `max-w-7xl` com `px-6` para o ritmo de leitura. `PageSurface` ancora isso em uma primitiva única — a Wave 3 troca o shell para apontar para ela, e Waves 4/5 substituem o wrapper hand-rolled de cada página por `<PageSurface>`. **Um tamanho só** (sem `variant`/`size`) — memory `feedback_no_size_variants` é explícita.

**O que esta wave NÃO faz com PageSurface:**
- Não migra nenhuma página para usá-la (isso é Wave 4/5).
- Não cria `EditorialBand`, `EditorialEyebrow`, `EditorialTitle`, `EditorialSubtitle` (parent spec lista esses, mas para escopo mínimo desta wave ficam fora — Wave 3 ou Wave 4 cria conforme demanda real).
- Não toca em `KPICard`, `PageHeader`, ou qualquer primitiva existente.

## Mapeamento ds/* → page/*

| Arquivo origem (ds/) | Arquivo destino (page/) | Renaming? |
|---|---|---|
| BaseRow.tsx | BaseRow.tsx | não (import interno `./MetricsPill` → `./StatPill`) |
| CodeBlock.tsx | CodeBlock.tsx | não |
| DiffViewer.tsx | DiffViewer.tsx | não |
| MetricsPill.tsx | StatPill.tsx | **SIM** — arquivo + `export function MetricsPill` → `export function StatPill` + `MetricsPillProps` → `StatPillProps` + `Intent as MetricsIntent` → `Intent as StatIntent` |
| TreeNode.tsx | TreeNode.tsx | não |
| index.ts | (merge com page/index.ts existente) | merge |
| DS.md | (DELETAR — DESIGN.md substitui) | deletar |

**Pós-merge `page/index.ts` deve re-exportar TODOS os 16 primitives** (10 atuais + 5 vindos de ds/ com StatPill já no novo nome + 1 novo PageSurface):

Existentes (10): `PageHeader`, `SectionHeader`, `KPICard`, `EmptyState`, `DataCard`, `PhaseChip`, `EventChip`, `AcBreakdown`, `WaveRowLabel`, `CollapsibleGroup`

Novos via merge (5): `BaseRow`, `CodeBlock`, `DiffViewer`, `StatPill` (renomeado), `TreeNode`

Novo via criação (1): `PageSurface`

Total final: **16 primitives exportadas** pelo barril.

## Arquivos

**Criados:**
- `apps/dashboard/src/components/page/PageSurface.tsx` (novo)
- `apps/dashboard/src/components/page/StatPill.tsx` (vindo de `ds/MetricsPill.tsx`, renomeado: classe + interface + arquivo)
- `apps/dashboard/src/components/page/BaseRow.tsx` (vindo de `ds/`, import interno `./MetricsPill` → `./StatPill`)
- `apps/dashboard/src/components/page/CodeBlock.tsx` (vindo de `ds/`)
- `apps/dashboard/src/components/page/DiffViewer.tsx` (vindo de `ds/`)
- `apps/dashboard/src/components/page/TreeNode.tsx` (vindo de `ds/`)

**Modificados:**
- `apps/dashboard/src/components/page/index.ts` (adiciona re-exports: `PageSurface`, `StatPill` + type `StatPillProps` + type `Intent as StatIntent`, `BaseRow` + types `BaseRowProps`/`RowStatus`, `CodeBlock` + types `CodeBlockProps`/`CodeLang`, `DiffViewer` + types `DiffViewerProps`/`DiffMode`, `TreeNode` + types `TreeNodeProps`/`TreeNodeData`)
- `apps/dashboard/src/components/page/README.md` (atualizar bloco de exemplo de import para listar as 16 primitivas; uma linha por sessão)

**19 arquivos consumidores reais** atualizam imports `@/components/ds` → `@/components/page` E `MetricsPill` → `StatPill`:

Pages (6):
1. `apps/dashboard/src/pages/Economia.tsx` (importa `@/components/ds` E usa `MetricsPill`)
2. `apps/dashboard/src/pages/Workspace.tsx` (importa `@/components/page`)
3. `apps/dashboard/src/pages/Specs.tsx` (importa `@/components/page`)
4. `apps/dashboard/src/pages/Settings.tsx` (importa `@/components/page`)
5. `apps/dashboard/src/pages/Prd.tsx` (importa `@/components/page`)
6. `apps/dashboard/src/pages/Knowledge.tsx` (importa `@/components/page`)

Economy (2):
7. `apps/dashboard/src/components/economy/PerAgentTable.tsx` (importa `@/components/ds` E usa `MetricsPill` 6×)
8. `apps/dashboard/src/components/economy/SavingsBreakdownCard.tsx` (importa `@/components/ds`)

Workspace (7):
9. `apps/dashboard/src/components/workspace/WorkspaceTokenSummary.tsx` (importa `@/components/page`)
10. `apps/dashboard/src/components/workspace/WorkspaceStatusCounters.tsx` (importa `@/components/page`)
11. `apps/dashboard/src/components/workspace/WorkspaceSpecsByStatus.tsx` (importa `@/components/page`)
12. `apps/dashboard/src/components/workspace/WorkspaceMonthCalendar.tsx` (importa `@/components/page`)
13. `apps/dashboard/src/components/workspace/WorkspaceHero.tsx` (importa `@/components/ds` E `@/components/page` E usa `MetricsPill`)
14. `apps/dashboard/src/components/workspace/WorkspaceFilesRanking.tsx` (importa `@/components/ds` E `@/components/page` E usa `MetricsPill`; também comentário JSDoc menciona `<MetricsPill>` — atualizar)
15. `apps/dashboard/src/components/workspace/WorkspaceEventsFeed.tsx` (importa `@/components/page`)

Trace (2):
16. `apps/dashboard/src/components/trace/ToolEventRow.tsx` (importa `@/components/ds`)
17. `apps/dashboard/src/components/trace/ExecutionTrace.tsx` (importa `@/components/ds` E usa `MetricsPill` 2×)

Specs (2):
18. `apps/dashboard/src/components/specs/SpecNetworkTab.tsx` (importa `@/components/page`)
19. `apps/dashboard/src/components/specs/SpecEventsTab.tsx` (importa `@/components/page`)

**Deletados:**
- `apps/dashboard/src/components/ds/` (diretório inteiro: BaseRow.tsx, CodeBlock.tsx, DiffViewer.tsx, MetricsPill.tsx, TreeNode.tsx, index.ts, DS.md)

## Tarefas

- [x] Criar `apps/dashboard/src/components/page/PageSurface.tsx` com a API do Component Contract (1 export, `editorial?: boolean = true`, sem variant/size)
- [x] Copiar `ds/MetricsPill.tsx` → `page/StatPill.tsx` e renomear dentro do arquivo: `export function MetricsPill` → `export function StatPill`, `interface MetricsPillProps` → `interface StatPillProps`, `: MetricsPillProps` → `: StatPillProps` (assinatura). `Intent` interno permanece `Intent`; apenas o re-export no barril vira `Intent as StatIntent`
- [x] Copiar `ds/BaseRow.tsx` → `page/BaseRow.tsx` preservando comportamento; trocar o import interno `import { MetricsPill } from "./MetricsPill"` por `import { StatPill } from "./StatPill"` e a chamada JSX `<MetricsPill ...>` por `<StatPill ...>`
- [x] Copiar `ds/CodeBlock.tsx`, `ds/DiffViewer.tsx`, `ds/TreeNode.tsx` → `page/` (sem alteração de conteúdo)
- [x] Atualizar `apps/dashboard/src/components/page/index.ts` adicionando os 6 re-exports novos (PageSurface, StatPill+types, BaseRow+types, CodeBlock+types, DiffViewer+types, TreeNode+types) — manter a ordem das seções existentes e adicionar nova seção "Primitives migrated from ds/"
- [x] Find/replace global em `apps/dashboard/src/`: `@/components/ds` → `@/components/page` (não existem imports relativos `../components/ds` — confirmado por Grep prévio)
- [x] Find/replace global em `apps/dashboard/src/`: identifier `MetricsPill` → `StatPill` (apenas dentro dos 5 arquivos consumidores que usam: Economia.tsx, PerAgentTable.tsx, WorkspaceHero.tsx, WorkspaceFilesRanking.tsx, ExecutionTrace.tsx; inclui import + JSX + comentário JSDoc em WorkspaceFilesRanking.tsx linha 22)
- [x] Atualizar `apps/dashboard/src/components/page/README.md` para listar as 16 primitivas no bloco de exemplo (já cobre 10; adicionar `PageSurface`, `StatPill`, `BaseRow`, `CodeBlock`, `DiffViewer`, `TreeNode`)
- [x] Deletar o diretório `apps/dashboard/src/components/ds/` (7 arquivos: 5 .tsx + index.ts + DS.md)
- [x] Rodar `pnpm --filter mustard-dashboard build` — deve passar (TypeScript não pode quebrar com identifier renomeado nem com imports reapontados)
- [~] Rodar `node scripts/check-pages-imports.mjs apps/dashboard/src/pages` — exit 1 com 5 violações pré-existentes (`@/components/Markdown` + `@/components/StatusDot`); zero violações novas de `@/components/ds` (a 1 que existia foi removida)
- [x] Grep final: zero referências vivas a `@/components/ds`, `MetricsPill`, `MetricsPillProps`, `MetricsIntent`, `components/ds`

## Critérios de Aceitação (Wave 2)

- [x] AC-W21: diretório `apps/dashboard/src/components/ds/` não existe — Command: `node -e "if(require('fs').existsSync('apps/dashboard/src/components/ds'))process.exit(1);console.log('ok')"`
- [x] AC-W22: `PageSurface.tsx` e `StatPill.tsx` existem em `apps/dashboard/src/components/page/` e exportam os símbolos esperados — Command: `node -e "const fs=require('fs');const p='apps/dashboard/src/components/page/PageSurface.tsx';const s='apps/dashboard/src/components/page/StatPill.tsx';if(!fs.existsSync(p))process.exit(1);if(!fs.existsSync(s))process.exit(2);const pc=fs.readFileSync(p,'utf8');if(!/export\s+function\s+PageSurface/.test(pc))process.exit(3);const sc=fs.readFileSync(s,'utf8');if(!/export\s+function\s+StatPill/.test(sc))process.exit(4);if(/MetricsPill/.test(sc))process.exit(5);console.log('ok')"`
- [x] AC-W23: `page/index.ts` re-exporta `PageSurface`, `StatPill`, `BaseRow`, `CodeBlock`, `DiffViewer`, `TreeNode` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/page/index.ts','utf8');const must=['PageSurface','StatPill','BaseRow','CodeBlock','DiffViewer','TreeNode'];for(const sym of must){if(!c.includes(sym)){console.error('missing:',sym);process.exit(1)}}console.log('ok')"`
- [x] AC-W24: zero imports vivos de `@/components/ds` em todo `apps/dashboard/src/` — Command: `node -e "const {execSync}=require('child_process');try{const r=execSync('node -e \"const{readdirSync,readFileSync,statSync}=require(\\'fs\\');const{join}=require(\\'path\\');function walk(d,out){for(const e of readdirSync(d)){const p=join(d,e);const s=statSync(p);if(s.isDirectory())walk(p,out);else if(/\\\\.(tsx?|jsx?|mjs)$/.test(e))out.push(p)}return out}const files=walk(\\'apps/dashboard/src\\',[]);const hits=files.filter(f=>/@/components/ds/.test(readFileSync(f,\\'utf8\\')));if(hits.length){console.error(hits.join(String.fromCharCode(10)));process.exit(1)}console.log(\\'ok\\')\"',{encoding:'utf8'});console.log(r.trim())}catch(e){process.exit(e.status||1)}"`
- [x] AC-W25: zero referências a identifier `MetricsPill`, `MetricsPillProps` ou `MetricsIntent` em `apps/dashboard/src/` — Command: `node -e "const{readdirSync,readFileSync,statSync}=require('fs');const{join}=require('path');function walk(d,out){for(const e of readdirSync(d)){const p=join(d,e);const s=statSync(p);if(s.isDirectory())walk(p,out);else if(/\\.(tsx?|jsx?|mjs)$/.test(e))out.push(p)}return out}const files=walk('apps/dashboard/src',[]);const hits=files.filter(f=>/\\bMetricsPill(Props|Intent)?\\b/.test(readFileSync(f,'utf8')));if(hits.length){console.error('still referenced:',hits.join(','));process.exit(1)}console.log('ok')"`
- [x] AC-W26: dashboard build passa — Command: `pnpm --filter mustard-dashboard build`
- [~] AC-W27: `check-pages-imports.mjs` sai com exit 1 — 5 violações pré-existentes (`@/components/Markdown` + `@/components/StatusDot` em 5 pages); zero novas violações de `@/components/ds`. Estas duas barris (`Markdown`, `StatusDot`) estão fora do escopo da Wave 2 — o parent spec lista move para `page/` mas Wave 2 child spec se limita a ds/*. Fica para Wave 4/5 ou TF dedicado.

## Limites

Editar dentro de:
- `apps/dashboard/src/components/page/` (criar `PageSurface.tsx`, criar `StatPill.tsx`, copiar 4 outros .tsx de ds/, editar `index.ts`, editar `README.md`)
- `apps/dashboard/src/components/ds/` (deletar diretório inteiro ao final)
- 19 arquivos consumidores reais (lista em ## Arquivos itens 1-19) para atualizar imports e renomear `MetricsPill` → `StatPill`
- Nenhum outro arquivo

**Não tocar** (`[BOUNDARY WARNING]` se aparecer):
- `apps/dashboard/src/style.css` (Wave 1 finalizou tokens Binance)
- `apps/dashboard/src/main.tsx`, `apps/dashboard/package.json`, `apps/dashboard/DESIGN.md` (Wave 1 finalizou)
- `apps/dashboard/src/components/layout/` (Wave 3 toca shell)
- `apps/dashboard/src/components/ui/` (shadcn intacto)
- `apps/dashboard/src/components/{prd,knowledge,amend}/` (Wave 5 toca pages que consomem; estes componentes não importam de `@/components/ds`)
- Lógica de componente alguma — apenas mover arquivo + renomear identifier + reapontar imports. JSX/comportamento de `BaseRow`, `CodeBlock`, `DiffViewer`, `MetricsPill`/`StatPill`, `TreeNode` permanece byte-equivalente exceto o rename
- Páginas: zero refit visual nesta wave — apenas trocar `MetricsPill` por `StatPill` no Economia.tsx (mesma JSX, identifier diferente). Wave 4/5 substitui wrapper hand-rolled por `<PageSurface>`
- `apps/dashboard/src-tauri/` e tudo fora de `apps/dashboard/` (exceto rodar o script `scripts/check-pages-imports.mjs` que já existe)

## Modelo

opus (UI consolidação grande com 19 arquivos de impacto + criação de primitiva canônica + boundary de imports cross-paths; identifier rename precisa de raciocínio para não quebrar BaseRow.tsx que usa MetricsPill internamente; downgrade vetado por memory `feedback_no_routing_downgrade`)
