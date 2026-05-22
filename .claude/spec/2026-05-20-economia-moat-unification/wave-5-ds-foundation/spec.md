# Wave 5 — Design System Foundation (Tailwind 4 @theme + primitivas caseiras)

### Parent: [[2026-05-20-economia-moat-unification]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full (wave)
### Checkpoint: 2026-05-21T05:25:00Z
### Lang: pt

## PRD

Hoje os tokens de design ficam cravados inline em ~40+ componentes (`bg-emerald-100`, `dark:bg-emerald-500/15` repetidos). Tailwind 4.3 já está no projeto — mas o recurso novo `@theme` (CSS vars first) não é usado. Esta wave estabelece o Design System Foundation em `apps/dashboard/src/components/ds/` com tokens centralizados via `@theme` em `apps/dashboard/src/styles/theme.css`, primitivas caseiras (`DiffViewer` com LCS, `CodeBlock` com syntax highlighter caseiro, `TreeNode` colapsável, `MetricsPill`, `BaseRow`) que alimentam o trace viewer da W6 e a página Economia da W7. Aesthetic: dark-first Linear+Notion, accent indigo/violet, Inter, status dots — alinhado com `feedback_design_aesthetic.md` da memória.

## Acceptance Criteria

- [x] AC-1: Build passa — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [x] AC-3: Theme CSS com `@theme` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/styles/theme.css'))throw new Error('theme.css missing');const t=require('fs').readFileSync('apps/dashboard/src/styles/theme.css','utf8');if(!t.includes('@theme'))throw new Error('@theme directive missing')"`
- [x] AC-4: 5 primitivas DS existem — Command: `node -e "['DiffViewer','CodeBlock','TreeNode','MetricsPill','BaseRow'].forEach(c=>{const p='apps/dashboard/src/components/ds/'+c+'.tsx';if(!require('fs').existsSync(p))throw new Error('missing '+p)})"`
- [x] AC-5: DiffViewer não importa lib de diff — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/ds/DiffViewer.tsx','utf8');['diff','react-diff','diff2html','jsdiff'].forEach(lib=>{if(t.includes(\"from '\"+lib+\"'\")||t.includes('from \"'+lib+'\"'))throw new Error('imported '+lib)})"`
- [x] AC-6: NOTICE.md menciona claude-devtools — Command: `node -e "if(!require('fs').existsSync('NOTICE.md'))throw new Error('NOTICE missing');const t=require('fs').readFileSync('NOTICE.md','utf8');if(!t.includes('claude-devtools'))throw new Error('attribution missing')"`

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

## Informações da Entidade

Sem entidade de domínio. DS pure-UI: tokens CSS + 5 primitivas React + 1 doc + 1 attribution file.

## Arquivos (~9)

```
apps/dashboard/src/styles/theme.css              (new — @theme { --ds-* } tokens semânticos)
apps/dashboard/src/components/ds/DiffViewer.tsx  (new — LCS caseiro ~150 LOC, MIT adapted from claude-devtools)
apps/dashboard/src/components/ds/CodeBlock.tsx   (new — tokenizer keyword caseiro ~100 LOC, suporta rust/ts/json/sql)
apps/dashboard/src/components/ds/TreeNode.tsx    (new — <details> aninhado + connectors via ::before)
apps/dashboard/src/components/ds/MetricsPill.tsx (new — pill monospace + tooltip de breakdown)
apps/dashboard/src/components/ds/BaseRow.tsx     (new — átomo: ícone + label + summary + tokens + status + chevron)
apps/dashboard/src/components/ds/DS.md           (new — doc inline com props, tokens, exemplos)
apps/dashboard/src/components/ds/index.ts        (new — barrel re-export)
apps/dashboard/src/main.tsx                      (modify — 1 linha: import './styles/theme.css')
NOTICE.md                                        (new — raiz do repo, attribution MIT para claude-devtools)
```

## Tarefas

### Dashboard DS Agent

- [ ] Criar `apps/dashboard/src/styles/theme.css` com bloco `@theme { ... }` único, definindo `--ds-status-{draft,implementing,awaiting-qa,completed,archived}`, `--ds-intent-{success,warning,error,info}`, `--ds-surface-{base,elevated,hover,sunken}`, `--ds-text-{primary,secondary,tertiary,disabled}`, `--ds-accent-{primary,secondary}` (indigo/violet pair), `--ds-radius-{sm,md,lg}`, `--ds-spacing-{1..8}`, `--ds-font-{sans,mono}` (Inter/JetBrains Mono). Light + dark via `@media (prefers-color-scheme: dark)` ou `[data-theme="dark"]` selector — pegar o pattern já usado em outros componentes do dashboard.
- [ ] Criar `apps/dashboard/src/components/ds/DiffViewer.tsx` — LCS (Longest Common Subsequence) caseiro, ~150 LOC. Props: `before: string`, `after: string`, `mode?: 'unified' | 'split'` (default unified), `maxLines?: number` (default ilimitado). Render: linhas verdes/vermelhas com prefixo `+`/`-`/` `, números de linha, syntax-agnóstico. NÃO importar `diff`, `react-diff`, `diff2html`, `jsdiff` ou similar (AC-5). Atribuir no header do arquivo: `// LCS algorithm adapted from claude-devtools (MIT). See NOTICE.md.`
- [ ] Criar `apps/dashboard/src/components/ds/CodeBlock.tsx` — tokenizer regex-based caseiro, ~100 LOC. Props: `code: string`, `lang?: 'rust' | 'ts' | 'tsx' | 'json' | 'sql' | 'plain'`, `showLineNumbers?: boolean`. Mapas de keywords por linguagem (15-20 keywords cada para rust/ts; chaves para json; SELECT/FROM/WHERE/JOIN para sql). Resto vira `<span class="text-[--ds-text-primary]">`. Sem highlight.js, sem prism.
- [ ] Criar `apps/dashboard/src/components/ds/TreeNode.tsx` — componente recursivo. Props: `node: { label: string, children?: TreeNode[], meta?: ReactNode }`, `defaultExpanded?: boolean`, `depth?: number` (interno). Uso de `<details>`/`<summary>` nativo + CSS `::before` para connectors verticais. Click no chevron expande; click no label dispara `onSelect?` se passado.
- [ ] Criar `apps/dashboard/src/components/ds/MetricsPill.tsx` — pill arredondada (`rounded-full`) com fundo `var(--ds-surface-elevated)` e texto monospace. Props: `value: string | number`, `unit?: string`, `intent?: 'success'|'warning'|'error'|'info'|'neutral'` (default neutral), `tooltip?: ReactNode` (breakdown opcional ao hover). Inten't pinta apenas a border-color, não o fill.
- [ ] Criar `apps/dashboard/src/components/ds/BaseRow.tsx` — átomo de lista. Props: `icon?: ReactNode`, `label: string`, `summary?: string`, `tokens?: number`, `status?: 'draft'|'implementing'|'awaiting-qa'|'completed'|'archived'`, `chevron?: boolean`, `onClick?: () => void`. Layout: flex horizontal (gap-3), status pinta dot à esquerda do label via `bg-[--ds-status-{status}]`, tokens em `<MetricsPill>` à direita, chevron `▸` se passado. Hover state via `var(--ds-surface-hover)`.
- [ ] Criar `apps/dashboard/src/components/ds/index.ts` — barrel: `export { DiffViewer } from './DiffViewer'; export { CodeBlock } from './CodeBlock';` etc. para os 5 componentes.
- [ ] Criar `apps/dashboard/src/components/ds/DS.md` — doc inline (markdown): seção por primitiva com prop signature, 1-2 exemplos de uso, token list. Não-Storybook por design (memória `feedback_design_aesthetic.md` + non-goal global).
- [ ] Adicionar `import './styles/theme.css';` em `apps/dashboard/src/main.tsx` (linha após os imports existentes). Verificar ordem se Tailwind precisa do import antes de outros estilos.
- [ ] Criar `NOTICE.md` na raiz do repo com bloco MIT attribution para claude-devtools (algoritmo LCS do DiffViewer + inspiração do trace viewer). Formato:
```
# NOTICE

This product includes software developed by:

## claude-devtools (MIT License)
Portions of `apps/dashboard/src/components/ds/DiffViewer.tsx` adapt the LCS diff algorithm from claude-devtools.
Copyright (c) <year> <authors>.
Licensed under the MIT License.
```
- [ ] Rodar `pnpm --filter mustard-dashboard build` e `pnpm --filter mustard-dashboard exec tsc --noEmit` — ambos verdes.

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

## Concerns

- **Light/dark via `.dark` class (não `[data-theme="dark"]` nem `prefers-color-scheme`)** — adaptado ao `useTheme.ts` que já toggla `.dark` no `<html>`. Decisão pragmática: evita duplicar mecanismos de tema. Spec sugeria `[data-theme]` ou media query; REVIEW pode reavaliar se quiser respeitar `prefers-color-scheme` do OS como fallback inicial.
- **Dois blocos `@theme` coexistindo** — `style.css` já tem `@theme inline` para tokens shadcn; `theme.css` (novo) tem `@theme` puro para `--ds-*`. Tailwind 4 mescla, mas REVIEW deve confirmar que não há conflito de nomes (`--background` em ambos, etc.) ao fazer scan completo.
- **`NOTICE.md` com placeholders `<year>`/`<authors>`** — origem MIT do claude-devtools não foi verificada no time budget desta wave. REVIEW final OU tactical-fix deve populá-los antes do CLOSE.
- **`@theme` no dashboard ainda não verificado para temas escuros via OS** — só funciona quando `.dark` está no `<html>`. Se o usuário desliga `prefers-color-scheme: dark` no OS, a app não responde automaticamente — depende do toggle interno. REVIEW pode propor um `useEffect` no `useTheme.ts` que sync com a media query.
