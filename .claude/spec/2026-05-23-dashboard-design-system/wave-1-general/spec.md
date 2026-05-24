# Wave 1 — DS foundation (Binance tokens + fonts swap)

## Resumo

Materializar o `DESIGN.md` Binance na raiz do `apps/dashboard/`, consolidar os tokens CSS em **um único** arquivo `src/style.css` adotando os hex Binance (canvas `#0b0e11`, card `#1e2329`, hairline `#eaecef`, trading `#0ecb81`/`#f6465d`, 80px editorial, CTA black-on-yellow) com **uma exceção única**: `--primary` preservado como amarelo Mustard `#dfab01`. Trocar fontes: remover JetBrains Mono (substituto de BinancePlex) e adicionar IBM Plex Mono (mais próximo do espírito Binance). Deletar o arquivo legado `src/styles/theme.css` (sistema de tokens duplicado) e criar o script `scripts/check-pages-imports.mjs` que alimenta AC-6 no QA final.

## Network

- Parent: [[2026-05-23-dashboard-design-system]]
- Depende de: nenhuma (wave inicial)
- Habilita: [[wave-2-ui]] (precisa dos tokens estabilizados antes de criar primitivas)

## Component Contract — tokens consolidados em `src/style.css`

Substituir os blocos `:root` (light) e `.dark` por valores derivados do DESIGN.md Binance. Mapeamento exato:

| Token CSS | Light | Dark | Origem |
|---|---|---|---|
| `--primary` | `#dfab01` | `#e6c84a` | **Mustard (override)** |
| `--primary-foreground` | `#000000` | `#000000` | Binance |
| `--primary-pressed` | `#b88c00` | `#c8a92e` | Mustard lift |
| `--background` | `#ffffff` | `#0b0e11` | Binance |
| `--foreground` | `#1e2329` | `#eaecef` | Binance |
| `--card` | `#ffffff` | `#1e2329` | Binance |
| `--card-foreground` | `#1e2329` | `#eaecef` | Binance |
| `--popover` | `#ffffff` | `#1e2329` | Binance |
| `--muted` | `#f5f5f5` | `#2b3139` | Binance card-elevated |
| `--muted-foreground` | `#848e9c` | `#848e9c` | Binance secondary |
| `--accent` | `#f5f5f5` | `#2b3139` | Binance card-elevated |
| `--border` | `#eaecef` | `#2b3139` | Binance hairline + dark equivalent |
| `--input` | `#eaecef` | `#2b3139` | Binance |
| `--ring` | `#dfab01` | `#e6c84a` | Mustard ring |
| `--destructive` | `#f6465d` | `#f6465d` | Binance down (trading) |
| `--intent-success` | `#0ecb81` | `#0ecb81` | Binance up (trading) |
| `--intent-error` | `#f6465d` | `#f6465d` | Binance down (trading) |
| `--intent-warning` | `#f0b90b` | `#f0b90b` | Binance amber |
| `--intent-info` | `#1e88e5` | `#1e88e5` | Binance blue |
| `--font-sans` | Inter Variable, ui-sans-serif, system-ui | mesmo | substituto Nova (licença) |
| `--font-mono` | IBM Plex Mono Variable, ui-monospace | mesmo | substituto Plex (licença) |
| `--radius` | 6px | 6px | Binance button |
| `--radius-card` | 8px | 8px | Binance card |
| `--editorial-band-py` | 80px | 80px | Binance editorial |

**Preservar do código atual:** as cores semânticas de fase (`--color-phase-analyze`, `-plan`, `-execute`, `-qa`, `-backlog`, `-close`) e de evento (`--color-event-fail`) ficam — não são "trading direction", são "lifecycle state". Apenas `--intent-success` e `--intent-error` adotam os hex Binance.

**Remover do código atual:** todos os tokens "Notion-inspired" (`--brand-pink/orange/purple/teal/green/yellow/brown`, `--tint-peach/rose/mint/lavender/sky/yellow/cream/gray`, `--ink-deep/charcoal/slate/steel/stone`, `--brand-navy/-deep/-mid`, `--link-blue/-pressed`, `--color-accent-mustard`, `--color-ok/error/paper/ink-subtle`). São paleta de marketing Notion que não está mais alinhada com o DESIGN.md Binance. Se algum componente legado usar — Wave 2/3/4/5 reapontam para tokens equivalentes do mapa acima.

**Preservar bloco `@layer base`:** o pattern de scrollbar (`scrollbar-width`, `::-webkit-scrollbar-*`) e o reset (`* { border-border; outline-ring/50 }`) ficam. Apenas valores de `body { bg-background text-foreground }` herdam os novos hex.

## Arquivos

- `apps/dashboard/DESIGN.md` (NOVO — gerado por `npx getdesign@latest add binance --out apps/dashboard/DESIGN.md`, com header `## Mustard Overrides` anexado no topo)
- `apps/dashboard/src/style.css` (REESCRITO — adota tokens Binance com 1 override Mustard, drop tokens Notion legados)
- `apps/dashboard/src/styles/theme.css` (DELETAR)
- `apps/dashboard/src/main.tsx` (EDIT — remover imports `jetbrains-mono`, adicionar `import '@fontsource-variable/ibm-plex-mono'`, remover import `./styles/theme.css` se houver)
- `apps/dashboard/package.json` (EDIT — `dependencies`: remover `@fontsource-variable/jetbrains-mono` e `@fontsource/jetbrains-mono`; adicionar `@fontsource-variable/ibm-plex-mono: "^5.2.8"`)
- `apps/dashboard/.claude/CLAUDE.md` (EDIT — adicionar linha em "Where to read what" apontando para `apps/dashboard/DESIGN.md` como norte visual)
- `scripts/check-pages-imports.mjs` (NOVO — script Node.js que varre `apps/dashboard/src/pages/*.tsx` e falha exit 1 se algum import bate em `@/components/ds`, `../components/ds`, `./components/ds`; alimenta AC-6 no QA)

## Tarefas

- [x] **getdesign**: rodar `npx getdesign@latest add binance --out apps/dashboard/DESIGN.md` (one-shot, output commitado); se o arquivo já existir, usar `--force`
- [x] **DESIGN.md header**: anexar no topo do `apps/dashboard/DESIGN.md` um bloco `## Mustard Overrides` (~20 linhas) listando: (1) brand color preservada `#dfab01` no lugar de `#FCD535`, (2) fontes substituídas por Inter + IBM Plex Mono (Nova/Plex são proprietárias da Binance, sem licença pública), (3) tokens de fase preservados (não-trading), (4) resto adotado integralmente do pack
- [x] **package.json**: remover `@fontsource-variable/jetbrains-mono` e `@fontsource/jetbrains-mono` de `dependencies`; adicionar `@fontsource-variable/ibm-plex-mono` (versão `^5.2.8` para casar com Inter)
- [x] **pnpm install**: rodar `pnpm install` para baixar `@fontsource-variable/ibm-plex-mono`
- [x] **main.tsx**: Grep antes; remover linha(s) com `@fontsource-variable/jetbrains-mono` e `@fontsource/jetbrains-mono`; adicionar `import '@fontsource-variable/ibm-plex-mono';` na mesma seção que Inter; remover `import './styles/theme.css'` se aparecer
- [x] **style.css**: reescrever `:root` e `.dark` com o mapa de tokens acima; preservar `@theme inline`, `@layer base`, `@custom-variant dark`, e o pattern de scrollbar; drop os tokens Notion legados listados
- [x] **Delete theme.css**: deletar `apps/dashboard/src/styles/theme.css`; após deletar, rodar Grep para confirmar zero referência viva no projeto
- [x] **check-pages-imports.mjs**: criar `scripts/check-pages-imports.mjs` na raiz do repo, Node.js puro (sem deps externas), shebang `#!/usr/bin/env node`. Lógica: ler todos os arquivos `apps/dashboard/src/pages/*.tsx`, regex `from\s+['"](@/components/ds|\.\.?/components/ds)`, exit 1 + lista de violações se encontrar match, exit 0 caso contrário. Aceita arg posicional `apps/dashboard/src/pages` como override
- [x] **CLAUDE.md dashboard**: editar `apps/dashboard/.claude/CLAUDE.md` na tabela "Where to read what" — adicionar linha:  `| \`apps/dashboard/DESIGN.md\` | Norte visual do design system (Binance pack via getdesign) | Manual — atualizar via `npx getdesign add binance --force` |`
- [x] **Build local**: `pnpm --filter mustard-dashboard build` deve passar (TypeScript não quebra com novos tokens, Vite resolve os imports de fonts novos)
- [x] **Lint local**: DEFERRED → [[2026-05-23-tf-dashboard-eslint-baseline]] — baseline pré-existente (repo não tem `eslint.config.js`; ESLint v9 exige flat config). Não é regressão da Wave 1; tactical-fix linkada cuida do flat config + violações

## Critérios de Aceitação (Wave 1)

- [x] AC-W1.1: `apps/dashboard/DESIGN.md` existe e contém ≥800 chars, com palavra "binance" — Command: `node -e "const fs=require('fs');const t=fs.readFileSync('apps/dashboard/DESIGN.md','utf8');if(!/binance/i.test(t))process.exit(1);if(t.length<800)process.exit(2);console.log('ok')"`
- [x] AC-W1.2: `apps/dashboard/src/styles/theme.css` não existe — Command: `node -e "if(require('fs').existsSync('apps/dashboard/src/styles/theme.css'))process.exit(1);console.log('ok')"`
- [x] AC-W1.3: brand Mustard + canvas Binance no `style.css` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/style.css','utf8');if(!/--primary:\s*#dfab01/.test(c))process.exit(1);if(!/#0b0e11/.test(c))process.exit(2);if(!/#1e2329/.test(c))process.exit(3);if(!/#0ecb81/.test(c))process.exit(4);if(!/#f6465d/.test(c))process.exit(5);console.log('ok')"`
- [x] AC-W1.4: IBM Plex Mono in (variable ou static), JetBrains Mono out — Command: `node -e "const pkg=require('./apps/dashboard/package.json');const deps={...pkg.dependencies,...pkg.devDependencies};if(!deps['@fontsource-variable/ibm-plex-mono']&&!deps['@fontsource/ibm-plex-mono'])process.exit(1);if(deps['@fontsource-variable/jetbrains-mono']||deps['@fontsource/jetbrains-mono'])process.exit(2);console.log('ok')"`
- [x] AC-W1.5: build passa — Command: `pnpm --filter mustard-dashboard build`
- [~] AC-W1.6: DEFERRED → [[2026-05-23-tf-dashboard-eslint-baseline]] (lint passa via TF dedicado; baseline pré-existente sem eslint.config.js, fora do escopo desta wave)
- [x] AC-W1.7: `scripts/check-pages-imports.mjs` existe e roda (exit 1 esperado pré-wave-2; o que valida é que o script roda) — Command: `node -e "const fs=require('fs');if(!fs.existsSync('scripts/check-pages-imports.mjs'))process.exit(1);try{require('child_process').execSync('node scripts/check-pages-imports.mjs apps/dashboard/src/pages',{stdio:'pipe'})}catch(e){if(e.status!==0&&e.status!==1)process.exit(2)}console.log('ok')"`

## Limites

Editar dentro de:
- `apps/dashboard/DESIGN.md` (novo)
- `apps/dashboard/src/style.css`
- `apps/dashboard/src/styles/theme.css` (deletar)
- `apps/dashboard/src/main.tsx`
- `apps/dashboard/package.json`
- `apps/dashboard/.claude/CLAUDE.md`
- `scripts/check-pages-imports.mjs` (novo)

**Não tocar** (`[BOUNDARY WARNING]` se aparecer):
- Qualquer arquivo `.tsx`/`.ts` em `apps/dashboard/src/components/` ou `apps/dashboard/src/pages/` — refit visual é responsabilidade de Wave 3/4/5
- `apps/dashboard/src/components/ds/` — Wave 2 deleta
- `apps/dashboard/src-tauri/`, `apps/dashboard/src/{api,lib,hooks}/`
- Qualquer coisa fora de `apps/dashboard/` exceto `scripts/check-pages-imports.mjs`

## Modelo

opus (token economy: Wave 1 envolve decisões de design que precisam de raciocínio; downgrade vetado por memory `feedback_no_routing_downgrade.md`)
