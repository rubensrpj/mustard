# Enhancement: quality-grouped-side-panel

### Status: closed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-05-13T00:00:00Z
### Lang: pt

## Contexto

A página Quality lista todas as specs em uma tabela plana ordenada por nome, sem distinção visual entre specs ativas, fechadas e em rascunho — o usuário precisa ler a coluna Status linha por linha para entender o estado do trabalho. Além disso, cada spec passou por múltiplas waves (cada wave registra eventos como dispatch, qa.result, retry.attempt), mas a tabela só mostra o agregado da spec — quem investiga um problema específico de wave precisa abrir a página inteira da spec (`/project/{id}/spec/{name}`) e perder o contexto da página Quality. Como navegação por SPA recarrega a árvore inteira, qualquer comparação rápida entre specs vira ping-pong entre rotas.

## Resumo

Reorganizar a seção Specs da página Quality em três grupos por status (Active, Closed, Draft), exibir contagem de waves por spec, e ao clicar abrir um painel lateral direito (estilo Notion peek) com o markdown da spec + link para a página completa — sem perder o contexto da Quality.

## Limites

- `src/components/ui/sheet.tsx` (novo — wrapper Radix Dialog com animação lateral direita)
- `src/components/SpecSidePanel.tsx` (novo — conteúdo do painel: header, markdown, link "Abrir página completa")
- `src/pages/Quality.tsx` (refactor da seção "Specs" — grupos por status, contagem de waves, click → painel)
- **Fora do escopo:** os 4 cards KPI (Pass@1, fix-loop, etc.), Per-role breakdown, Slowest waves, Tokens by phase — ficam intactos

## Checklist

### Frontend Agent

- [x] Criar `src/components/ui/sheet.tsx`: re-exporta `Sheet`, `SheetTrigger`, `SheetContent`, `SheetHeader`, `SheetTitle`, `SheetClose` baseados em `radix-ui` Dialog. `SheetContent` posicionado `fixed right-0 top-0 h-screen w-[600px] max-w-[90vw]`, com animações slide-in-from-right (Tailwind `data-open:slide-in-from-right data-closed:slide-out-to-right`). Reuso `DialogPortal`/`DialogOverlay` pattern já existente em `src/components/ui/dialog.tsx`
- [x] Criar `src/components/SpecSidePanel.tsx`: props `{ open: boolean, onOpenChange: (o: boolean) => void, projectId: string | null, projectPath: string | null, specName: string | null }`. Render: header com nome (font-mono), badges phase/status, link "Abrir página completa →" para `/project/${projectId}/spec/${specName}`. Body: `<Markdown content={markdown} />` com fetch via `fetchSpecMarkdown` (TanStack Query, enabled quando `open && specName && projectPath`). Scroll vertical no body via `ScrollArea`
- [x] Refatorar a seção Specs em `src/pages/Quality.tsx`:
  - [x] Computar `wavesBySpec: Map<string, Set<number>>` a partir de `feedEvents` (rows que têm `event.spec` E `event.wave`)
  - [x] Em vez de uma tabela plana, agrupar specs em três seções por status: `Active` (status !== "closed" && !completed_at), `Closed` (status === "closed" || completed_at), `Draft` (status === "draft"). Header de cada seção: `## Active (N)` etc, font menor que o h2 da página
  - [x] Adicionar coluna "Waves" mostrando `{wavesBySpec.get(spec.name)?.size ?? 0}` — formato compacto, badge se > 0
  - [x] Trocar `onClick={navigate(...)}` por `onClick={openPanel(spec.name)}` que abre `<SpecSidePanel />` controlado por state local `[selectedSpec, setSelectedSpec]`
  - [x] Renderizar `<SpecSidePanel />` no final do JSX, sempre montado (controlled), state controla `open`
- [x] Manter Skeleton, empty state e error state da tabela existente
- [x] `bun run build` e `bun run typecheck` passam

## Arquivos (~3)

- `src/components/ui/sheet.tsx` (new)
- `src/components/SpecSidePanel.tsx` (new)
- `src/pages/Quality.tsx` (modify)

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Build passa sem erros — Command: `bun run build`
- [x] AC-2: Sheet component existe e exporta `Sheet`/`SheetContent` — Command: `node -e "const fs=require('fs');if(!fs.existsSync('src/components/ui/sheet.tsx'))process.exit(1);const c=fs.readFileSync('src/components/ui/sheet.tsx','utf8');if(!c.includes('Sheet')||!c.includes('SheetContent'))process.exit(2);console.log('ok')"`
- [x] AC-3: SpecSidePanel existe, consome Markdown e fetchSpecMarkdown — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/components/SpecSidePanel.tsx','utf8');if(!c.includes('Markdown'))process.exit(1);if(!c.includes('fetchSpecMarkdown'))process.exit(2);console.log('ok')"`
- [x] AC-4: Quality.tsx importa SpecSidePanel e tem agrupamento por status — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/pages/Quality.tsx','utf8');if(!c.includes('SpecSidePanel'))process.exit(1);if(!/Active|Ativas/.test(c)||!/Closed|Fechadas/.test(c))process.exit(2);console.log('ok')"`
- [x] AC-5: Quality.tsx calcula waves por spec — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/pages/Quality.tsx','utf8');if(!/wavesBySpec|wavesBy|wave_count|waveCount/i.test(c))process.exit(1);console.log('ok')"`

## Preocupações

- [WARN/layer-gap] `analyze-validation.js` reportou "Spec declares Frontend Agent but Files has no Frontend extensions" — falso positivo: todos os 3 arquivos são `.tsx`. Não bloqueia.
