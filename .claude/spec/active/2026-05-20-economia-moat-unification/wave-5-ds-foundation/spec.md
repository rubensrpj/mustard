# Wave 5 — Design System Foundation (Tailwind 4 @theme + primitivas caseiras)

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: queued
### Phase: PLAN
### Scope: full (wave)
### Checkpoint: 2026-05-20T23:59:00Z
### Lang: pt

## PRD

Hoje os tokens de design ficam cravados inline em ~40+ componentes (`bg-emerald-100`, `dark:bg-emerald-500/15` repetidos). Tailwind 4.3 já está no projeto — mas o recurso novo `@theme` (CSS vars first) não é usado. Esta wave estabelece o Design System Foundation em `apps/dashboard/src/components/ds/` com tokens centralizados via `@theme` em `apps/dashboard/src/styles/theme.css`, primitivas caseiras (`DiffViewer` com LCS, `CodeBlock` com syntax highlighter caseiro, `TreeNode` colapsável, `MetricsPill`, `BaseRow`) que alimentam o trace viewer da W6 e a página Economia da W7. Aesthetic: dark-first Linear+Notion, accent indigo/violet, Inter, status dots — alinhado com `feedback_design_aesthetic.md` da memória.

## Acceptance Criteria

- [ ] AC-1: Build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-2: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [ ] AC-3: Theme CSS com `@theme` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/styles/theme.css'))throw new Error('theme.css missing');const t=require('fs').readFileSync('apps/dashboard/src/styles/theme.css','utf8');if(!t.includes('@theme'))throw new Error('@theme directive missing')"`
- [ ] AC-4: 5 primitivas DS existem — Command: `node -e "['DiffViewer','CodeBlock','TreeNode','MetricsPill','BaseRow'].forEach(c=>{const p='apps/dashboard/src/components/ds/'+c+'.tsx';if(!require('fs').existsSync(p))throw new Error('missing '+p)})"`
- [ ] AC-5: DiffViewer não importa lib de diff — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/ds/DiffViewer.tsx','utf8');['diff','react-diff','diff2html','jsdiff'].forEach(lib=>{if(t.includes(\"from '\"+lib+\"'\")||t.includes('from \"'+lib+'\"'))throw new Error('imported '+lib)})"`
- [ ] AC-6: NOTICE.md menciona claude-devtools — Command: `node -e "if(!require('fs').existsSync('NOTICE.md'))throw new Error('NOTICE missing');const t=require('fs').readFileSync('NOTICE.md','utf8');if(!t.includes('claude-devtools'))throw new Error('attribution missing')"`

## Plano

Estrutura nova:
```
apps/dashboard/src/styles/
├── theme.css           # @theme { --ds-* } com tokens semânticos (status, intent, surface, text, accent)
└── tokens/             # color/space/radius/motion split (opcional)

apps/dashboard/src/components/ds/
├── DiffViewer.tsx      # LCS caseiro ~150 LOC, inspirado em claude-devtools (MIT, atribuído)
├── CodeBlock.tsx       # Tokenizer keyword caseiro ~100 LOC
├── TreeNode.tsx        # <details> aninhado + connectors via ::before
├── MetricsPill.tsx     # Pill monospace + tooltip de breakdown
├── BaseRow.tsx         # Átomo: ícone + label + summary + tokens + status + chevron
└── DS.md               # Documentação inline (sem Storybook)

NOTICE.md (raiz do repo, novo) — atribuição MIT para componentes adapted from claude-devtools
```

Tokens via `@theme` em CSS vars: `--ds-status-draft`, `--ds-status-implementing`, `--ds-status-awaiting-qa`, `--ds-status-completed`, `--ds-intent-success`, `--ds-intent-warning`, `--ds-intent-error`, `--ds-intent-info`, `--ds-surface-base`, `--ds-surface-elevated`, `--ds-text-primary`, `--ds-text-secondary`, `--ds-accent-primary` (indigo/violet). Light + dark via media query nativa do Tailwind 4.

## Dependências

Nenhuma. Paralelizável com [[wave-1-core-economy]], [[wave-2-hooks-real]], [[wave-3-ingestion]], [[wave-4-attribution]].

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Paralela a: TODAS as waves 1-4 (sem dep)
- Desbloqueia: [[wave-6-trace-viewer]], [[wave-7-economia-page]]
- Grava memória: `{tokens: [...], primitives: [...], theme_css_path: "..."}` para [[wave-6-trace-viewer]] e [[wave-7-economia-page]]

## Limites

Em escopo: `apps/dashboard/src/styles/theme.css` (novo), `apps/dashboard/src/components/ds/**` (todos novos), `NOTICE.md` (raiz, novo), `apps/dashboard/src/main.tsx` (1 linha importando o theme.css).

Fora de escopo: qualquer migração das ~40 páginas/componentes existentes (refactor lazy — só novas features consomem DS). Não tocar em `tailwind.config` (Tailwind 4 não usa). Não criar Storybook.
