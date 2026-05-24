# Design system unificado (Binance DESIGN.md aplicado, brand Mustard só na cor)

## PRD

## Contexto

O dashboard cresceu com 11 rotas, ~100 componentes, **dois** sistemas de tokens CSS rodando em paralelo (`src/style.css` com paleta Notion + amarelo Mustard `#dfab01`, e `src/styles/theme.css` com tokens `--ds-*` em índigo/violeta) e **três** barris de componentes sem regra clara (`components/ui/` shadcn, `components/page/` primitivas de página, `components/ds/` design-system tentativa). Cada nova página inventa seu próprio `flex flex-col gap-6 w-full`, importa de barris diferentes, repete espaçamento e radius à mão. Abrir Workspace, Specs e Economia parece três produtos distintos, e não temos um `DESIGN.md` na raiz como norte para o agente. A decisão de design é **adotar integralmente o pack `binance` do CLI `getdesign` (`npx getdesign@latest add binance` gera 1 markdown na raiz, sem código nem deps) — canvas escuro `#0b0e11`, surface card `#1e2329`, hairlines `#eaecef`, escala tipográfica modesta (display 600-700, body 400), 80px de banda editorial, semântica trading (`#0ecb81` up, `#f6465d` down), CTA "preto sobre amarelo" assinatura — preservando APENAS o amarelo Mustard `#dfab01` como brand color** (não o `#FCD535` da Binance). Fontes proprietárias (BinanceNova/BinancePlex) não têm licença pública, então usamos **Inter Variable** (Nova fallback, já carregada) e **IBM Plex Mono** (Plex fallback, swap do JetBrains Mono atual) — substituição documentada no `DESIGN.md`. A meta é virar **um** sistema de tokens, **dois** barris (`components/ui/` shadcn + `components/page/` composto Mustard) e 11 páginas que consomem o mesmo contrato sob o visual Binance.

## Usuários/Stakeholders

Quem usa o dashboard hoje (Rubens + qualquer engenheiro abrindo o app Tauri). Sem usuários externos em produção — refactor pode quebrar visualmente sem migração suave.

## Métrica de sucesso

Ao abrir as 11 páginas em sequência, o dashboard parece um produto Binance com a marca Mustard: canvas escuro `#0b0e11`, cards `#1e2329`, números em mono tabular, CTAs amarelos com texto preto, deltas em verde/vermelho de direção, ritmo editorial. Build e lint passam. Nenhum import de barril deletado fica vivo.

## Não-Objetivos

- **Não** trocar o amarelo Mustard `#dfab01` pelo `#FCD535` da Binance — única exceção à adoção integral.
- **Não** mudar comportamento funcional de nenhuma rota — só visual + estrutura de import. Sem renaming de rotas, sem mudança de dados, sem novo recurso.
- **Não** refatorar `components/{specs,workspace,telemetry,trace,prd,knowledge,amend}/` (componentes vinculados a página) — esses ficam onde estão, só re-mapeiam imports do barril composto e absorvem o novo visual automaticamente via tokens.
- **Não** preservar `styles/theme.css` por compatibilidade — em dev, deletar legado é a regra (memory `feedback_no_migration_dev_phase.md`).
- **Não** rodar `npx getdesign` em CI — execução é one-shot em Wave 1, `DESIGN.md` resultado entra no repo e nunca mais o CLI é tocado.
- **Não** licenciar BinanceNova/BinancePlex (proprietárias da Binance, sem distribuição pública) — usar Inter + IBM Plex Mono como fallback documentado.
- **Não** adicionar feature flag, banner de migração, ou wrapper de compatibilidade — deletar e reapontar imports é mais limpo.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: dashboard build passa após refactor — Command: `pnpm --filter mustard-dashboard build`
- [~] AC-2: DEFERRED → [[2026-05-23-tf-dashboard-eslint-baseline]] (lint passa via TF dedicado; baseline pré-existente sem eslint.config.js)
- [x] AC-3: `DESIGN.md` existe na raiz do app dashboard com o pack binance materializado — Command: `node -e "const fs=require('fs');const t=fs.readFileSync('apps/dashboard/DESIGN.md','utf8');if(!/binance/i.test(t))process.exit(1);if(t.length<800)process.exit(2);console.log('ok')"`
- [x] AC-4: token system unificado — arquivo legado `apps/dashboard/src/styles/theme.css` deletado e zero referência viva — Command: `node -e "const fs=require('fs');const p=require('path');if(fs.existsSync('apps/dashboard/src/styles/theme.css'))process.exit(1);const needle='styles/theme.css';const root='apps/dashboard/src';const exts=['.tsx','.ts','.jsx','.js','.mjs','.cjs','.css'];const hits=[];function walk(d){for(const e of fs.readdirSync(d,{withFileTypes:true})){if(e.name==='node_modules'||e.name==='.git'||e.name==='dist')continue;const f=p.join(d,e.name);if(e.isDirectory())walk(f);else if(exts.some(x=>e.name.endsWith(x))){if(fs.readFileSync(f,'utf8').includes(needle))hits.push(f)}}}walk(root);if(hits.length){console.error('matches:\\n'+hits.join('\\n'));process.exit(2)}console.log('ok')"`
- [x] AC-5: barril `components/ds/` absorvido em `components/page/`, diretório removido e nenhum import vivo — Command: `node -e "const fs=require('fs');const p=require('path');if(fs.existsSync('apps/dashboard/src/components/ds'))process.exit(1);const needle='@/components/ds';const root='apps/dashboard/src';const exts=['.tsx','.ts','.jsx','.js','.mjs','.cjs'];const hits=[];function walk(d){for(const e of fs.readdirSync(d,{withFileTypes:true})){if(e.name==='node_modules'||e.name==='.git'||e.name==='dist')continue;const f=p.join(d,e.name);if(e.isDirectory())walk(f);else if(exts.some(x=>e.name.endsWith(x))){if(fs.readFileSync(f,'utf8').includes(needle))hits.push(f)}}}walk(root);if(hits.length){console.error('matches:\\n'+hits.join('\\n'));process.exit(2)}console.log('ok')"`
- [ ] AC-6: todas as 11 páginas só importam de `@/components/ui`, `@/components/page`, `@/components/layout` ou subpastas page-bound — nenhuma importa de `@/components/ds` — Command: `node scripts/check-pages-imports.mjs apps/dashboard/src/pages`
- [x] AC-7: brand color Mustard preservada e canvas Binance adotado — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('apps/dashboard/src/style.css','utf8');if(!/--primary:\s*#dfab01/.test(c))process.exit(1);if(!/#0b0e11/.test(c))process.exit(2);if(!/#1e2329/.test(c))process.exit(3);console.log('ok')"`
- [x] AC-8: trading semantics Binance aplicadas (`#0ecb81` up, `#f6465d` down) — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('apps/dashboard/src/style.css','utf8');if(!/#0ecb81/.test(c))process.exit(1);if(!/#f6465d/.test(c))process.exit(2);console.log('ok')"`
- [x] AC-9: IBM Plex Mono carregada (Plex fallback) e JetBrains Mono removida do bundle — Command: `node -e "const pkg=require('./apps/dashboard/package.json');const deps={...pkg.dependencies,...pkg.devDependencies};if(!deps['@fontsource-variable/ibm-plex-mono']&&!deps['@fontsource/ibm-plex-mono'])process.exit(1);if(deps['@fontsource-variable/jetbrains-mono']||deps['@fontsource/jetbrains-mono'])process.exit(2);console.log('ok')"`
- [ ] AC-10: NADA inline nas pages — sem hex hardcoded, sem classes Tailwind de cor/border/bg/radius/elevation, sem `style={{...}}` com propriedades visuais. Apenas classes de layout estrutural (grid/flex/gap/w-*/h-*/max-w-*) e composição de primitivas — Command: `node scripts/check-pages-no-inline-visual.mjs apps/dashboard/src/pages`

## Plano

## Informações da Entidade

N/A — refactor visual + estrutural, sem entidade de domínio nova. Subprojeto único: `apps/dashboard` (role `ui`, stack React 19.1 + Tailwind 4.3 + TypeScript 5.8 + shadcn 4.7 + radix-ui + Tauri 2).

## Arquivos

**Wave 1 — DS foundation (general):**
- `apps/dashboard/DESIGN.md` (novo, gerado por `npx getdesign add binance` + header `## Mustard Overrides`)
- `apps/dashboard/src/style.css` (consolida tokens; adota canvas/surface/hairline/trading Binance; preserva `--primary: #dfab01`)
- `apps/dashboard/src/styles/theme.css` (DELETAR)
- `apps/dashboard/src/main.tsx` (remover import `theme.css`; remover import `jetbrains-mono`; adicionar import `ibm-plex-mono`)
- `apps/dashboard/package.json` (remover `@fontsource-variable/jetbrains-mono` + `@fontsource/jetbrains-mono`; adicionar `@fontsource-variable/ibm-plex-mono`)
- `apps/dashboard/.claude/CLAUDE.md` (apontar p/ `DESIGN.md`)
- `scripts/check-pages-imports.mjs` (novo, alimenta AC-6)

**Wave 2 — Primitives consolidados (ui):**
- `apps/dashboard/src/components/page/index.ts` (barril expandido — ~22 novas primitivas, ver Component Contract)
- `apps/dashboard/src/components/page/` novos: `PageSurface`, `EditorialBand`, `EditorialEyebrow`, `EditorialTitle`, `EditorialSubtitle`, `BrandMark`, `NavSection`, `NavItem`, `Crumb`, `CrumbSeparator`, `KPIRow`, `KpiValue`, `KpiLabel`, `KpiHint`, `StatPill`, `DeltaText`, `DataRow`, `CostBar`, `BarTrack`, `BarFill`, `LegendSwatch`
- `apps/dashboard/src/components/page/KPICard.tsx` (refit — passa a compor com `KpiValue`/`KpiLabel`/`KpiHint`)
- `apps/dashboard/src/components/page/` movidos de `ds/`: `DiffViewer`, `CodeBlock`, `TreeNode`, `BaseRow`; movidos de `components/`: `Markdown`, `StatusDot`
- `apps/dashboard/src/components/ds/` (DELETAR diretório)
- `apps/dashboard/.claude/skills/dashboard-page-primitives/SKILL.md` (atualizar inventário + regra "NADA inline em pages")
- `scripts/check-pages-no-inline-visual.mjs` (novo — AST-walk que alimenta AC-10)

**Wave 3 — Layout shell (ui):**
- `apps/dashboard/src/components/layout/AppShell.tsx` (canvas `#0b0e11`, ritmo Binance)
- `apps/dashboard/src/components/layout/Sidebar.tsx` (tipografia modesta, surface `#1e2329`, divisores `#eaecef`-equivalent)
- `apps/dashboard/src/components/layout/Topbar.tsx` (40px altura assinatura Binance, type scale)
- `apps/dashboard/src/components/layout/SplitDetail.tsx` (alinhar ritmo)
- `apps/dashboard/src/components/ui/button.tsx` (variant `primary` = preto sobre amarelo Mustard, 40px altura, 6px radius — assinatura "black on yellow")

**Wave 4 — Refator estrutural (ui):** folder-per-component em TODO `components/**` e migração de domínios para `src/features/{specs,workspace,economy,knowledge,prd,telemetry,amend,trace}`; 10 strays do root realocados; codemod `scripts/refactor-folder-per-component.mjs` + script AC-10 `scripts/check-pages-no-inline-visual.mjs` criados; tokens fantasmas (`--color-ok`, `--color-accent-mustard`, `text-red-*`) varridos.

**Wave 5 — Pages high-traffic (ui):** `Workspace.tsx`, `Specs.tsx`, `Economia.tsx`, `Knowledge.tsx`

**Wave 6 — Pages secondary (ui):** `ProjectDetail.tsx`, `SpecDetail.tsx`, `Prd.tsx`, `Commands.tsx`, `Settings.tsx`, `Preferences.tsx`, `Home.tsx`

Total: ~55 arquivos tocados, 2 deletados, ~28 novos/movidos.

## Component Contract

**Token surface (Wave 1 entrega):** Um único arquivo `apps/dashboard/src/style.css` consolida tudo. Hex map Binance aplicado integralmente com 1 exceção:

| Token | Valor Binance | Aplicado em Mustard | Override? |
|---|---|---|---|
| `--primary` | `#FCD535` | `#dfab01` (Mustard yellow) | **SIM — única exceção** |
| `--primary-foreground` | `#000000` | `#000000` (preto sobre amarelo) | adotado |
| `--background` (dark) | `#0b0e11` | `#0b0e11` | adotado |
| `--background` (light) | `#ffffff` | `#ffffff` | adotado |
| `--card` (dark) | `#1e2329` | `#1e2329` | adotado |
| `--border` (light) | `#eaecef` | `#eaecef` | adotado |
| `--intent-success` | `#0ecb81` (up green) | `#0ecb81` | adotado |
| `--intent-error` | `#f6465d` (down red) | `#f6465d` | adotado |
| `--font-sans` | BinanceNova | Inter Variable (Nova fallback) | substituído (licença) |
| `--font-mono` | BinancePlex | IBM Plex Mono Variable (Plex fallback) | substituído (licença) |
| `--radius` | 6px (button) / 8px (card) | mesmo | adotado |
| `--editorial-band-py` | 80px | 80px | adotado |

Regras do DESIGN.md aplicadas:
- **Single accent**: amarelo Mustard `--primary` é o único acento; nunca em body text ou superfícies grandes (restrição Binance).
- **CTA "black on yellow"**: variant `primary` do button = preto sobre amarelo, 40px altura, 6px radius.
- **Depth sem shadow**: profundidade vem do salto `#0b0e11 → #1e2329` (12 stops de luminância), não de drop shadow.
- **Type voltage**: tamanho + cor de acento como ênfase; weights display 600-700, body 400.
- **Editorial breathing**: `EditorialBand` provê 80px vertical padding para aberturas de página; data sections (cards, listas) ficam em 24-32px.
- **Trading semantics**: `DeltaText` aplica verde/vermelho **só para deltas/direção** (cost change, latency change). Sucesso/falha de AC seguem em verde/vermelho (já alinhados com Binance up/down).

**Regra de componentização (HARD):** Pages NÃO renderizam JSX inline com semântica visual. Permitido em page: classes de layout estrutural (`grid`, `flex`, `gap-*`, `w-*`, `h-*`, `max-w-*`, `col-span-*`) e composição de primitivas. Proibido em page: hex hardcoded, classes Tailwind de cor (`text-*`, `bg-*`, `border-*`), radius (`rounded-*`), elevation (`shadow-*`), `style={{...}}` com cor/border/bg/radius. Cada átomo visual (logo, eyebrow, value, swatch, dot, separator) é uma primitiva — não vive solto em `<span class="...">`.

**Composed primitives (Wave 2 entrega):** O barril `@/components/page` é a única importação de primitiva visual:

| Primitive | Provê | Status |
|---|---|---|
| **Layout / surface** | | |
| `PageSurface` | wrapper canônico de página (`flex flex-col gap-8 w-full max-w-screen-2xl mx-auto px-8 pb-20`) | NOVO |
| `EditorialBand` | abertura full-width com 80px py (Binance editorial rhythm) — slots `eyebrow`/`title`/`subtitle`/`actions` | NOVO |
| `SectionHeader` | título + ação opcional | existente |
| **Brand / shell atoms** | | |
| `BrandMark` | logo "M" amarelo 24×24 + brand name | NOVO |
| `NavSection` | rótulo uppercase + lista de items | NOVO |
| `NavItem` | item de sidebar (dot + label + active state) | NOVO |
| `Crumb` + `CrumbSeparator` | breadcrumb de topbar | NOVO |
| **KPI / number atoms** | | |
| `KPIRow` | grid 4-up (responsivo) | NOVO |
| `KPICard` | wrapper de card KPI com slots `label`/`value`/`hint` | existente (refit) |
| `KpiValue` | número grande em mono tabular (28px, weight 600, letter-spacing -0.02em) | NOVO |
| `KpiLabel` | label uppercase 11px tracking-wide | NOVO |
| `KpiHint` | linha de hint 12px secondary | NOVO |
| `StatPill` | pílula label+value mono (renomeia `MetricsPill`) | renomeado |
| `DeltaText` | delta numérico com trading semantics (`up`/`down`/`flat`) | NOVO |
| **Status / chips** | | |
| `StatusDot` | dot 8px colorido por status (`plan`/`execute`/`qa`/`close`/`cancelled`) | movido de `components/` |
| `PhaseChip`, `EventChip` | chips tintados de fase/evento | existente |
| **Data row atoms** | | |
| `DataCard` | wrapper de tabela/lista | existente |
| `DataRow` | linha grid com slots `lead`/`primary`/`meta`/`trailing` | NOVO |
| **Cost / bars** | | |
| `CostBar` | linha de barra horizontal (label + track + fill + value mono) | NOVO |
| `BarTrack` + `BarFill` | track 6px + fill (cor customizável via prop `intent`) | NOVO |
| **Editorial / legend** | | |
| `EditorialEyebrow` | label uppercase amarelo (sobre título de banda editorial) | NOVO |
| `EditorialTitle` | h1 32px weight 600 letter-spacing -0.02em | NOVO |
| `EditorialSubtitle` | parágrafo 15px secondary max-w-prose | NOVO |
| `LegendSwatch` | quadrado de cor + caption hex | NOVO |
| **Existentes mantidos** | | |
| `EmptyState`, `AcBreakdown`, `WaveRowLabel`, `CollapsibleGroup`, `DiffViewer`, `CodeBlock`, `TreeNode`, `BaseRow`, `Markdown` | já cobertos pelo barril | mantidos / movidos |

Convenção: import sempre `@/components/page`, nunca arquivo individual. Adicionar primitiva nova = arquivo + 1 linha em `index.ts`.

## Tarefas

### Wave 1 — DS foundation (general, model: opus)

- [ ] Rodar `npx getdesign@latest add binance --out apps/dashboard/DESIGN.md`; commitar bruto
- [ ] Anexar no topo de `apps/dashboard/DESIGN.md` header `## Mustard Overrides` explicitando: brand color preservada (`#dfab01` no lugar de `#FCD535`), fontes substituídas (Inter Variable + IBM Plex Mono no lugar de BinanceNova + BinancePlex por licença), resto adotado integralmente
- [ ] `apps/dashboard/package.json`: remover `@fontsource-variable/jetbrains-mono` e `@fontsource/jetbrains-mono`; adicionar `@fontsource-variable/ibm-plex-mono`
- [ ] `apps/dashboard/src/main.tsx`: remover imports de jetbrains-mono; adicionar `import '@fontsource-variable/ibm-plex-mono'`; remover import de `styles/theme.css` se houver
- [ ] Consolidar tokens em `apps/dashboard/src/style.css`:
  - Substituir `:root` (light) e `.dark` pelos valores do DESIGN.md Binance
  - **Exceto** `--primary: #dfab01` (light) e `--primary: #e6c84a` (dark, mantém lift atual) — único override
  - Dark canvas `#0b0e11`, card `#1e2329`, hairlines `#eaecef` (light) / equivalente dark
  - Trading semantics: `--intent-success: #0ecb81`, `--intent-error: #f6465d` (com escala light/dark)
  - Trocar `--font-mono` para `'IBM Plex Mono Variable'`
  - Adicionar `--editorial-band-py: 80px`
- [ ] Deletar `apps/dashboard/src/styles/theme.css`
- [ ] Criar `scripts/check-pages-imports.mjs` na raiz: varre `apps/dashboard/src/pages/*.tsx`, falha se algum import bate em `@/components/ds`
- [ ] Adicionar ao `apps/dashboard/.claude/CLAUDE.md` em "Where to read what": `apps/dashboard/DESIGN.md` é o norte visual — consultar antes de tocar UI
- [ ] `pnpm install` + build + lint local antes de retornar

### Wave 2 — Primitives consolidados (ui, model: opus)

- [ ] Mover `MetricsPill.tsx` (`ds/`) para `page/StatPill.tsx`; `DiffViewer`, `CodeBlock`, `TreeNode`, `BaseRow` de `ds/` para `page/`; `Markdown.tsx` e `StatusDot.tsx` de `components/` para `page/`
- [ ] Deletar `apps/dashboard/src/components/ds/` (diretório inteiro)
- [ ] Criar layout/surface atoms: `PageSurface`, `EditorialBand` (com slots `eyebrow`/`title`/`subtitle`/`actions`)
- [ ] Criar brand/shell atoms: `BrandMark`, `NavSection`, `NavItem`, `Crumb`, `CrumbSeparator`
- [ ] Criar KPI/number atoms: `KPIRow`, `KpiValue`, `KpiLabel`, `KpiHint`; refit `KPICard` para compor com eles via slots
- [ ] Criar number primitives: `DeltaText` (props `value: number`, `format?: 'pct'|'abs'`; usa `--intent-success`/`--intent-error`/`--text-tertiary` por sign)
- [ ] Criar data atoms: `DataRow` (slots `lead`/`primary`/`meta`/`trailing`)
- [ ] Criar editorial atoms: `EditorialEyebrow`, `EditorialTitle`, `EditorialSubtitle`
- [ ] Criar bar atoms: `BarTrack`, `BarFill`, `CostBar` (composição de label+BarTrack+BarFill+value)
- [ ] Criar legend atom: `LegendSwatch`
- [ ] Atualizar `apps/dashboard/src/components/page/index.ts` com todos os novos exports (sequência: layout → brand → kpi → status → data → cost → editorial → legacy)
- [ ] Find/replace em `apps/dashboard/src/`: `@/components/ds` → `@/components/page`; `@/components/Markdown` → `@/components/page`; `@/components/StatusDot` → `@/components/page`
- [ ] Atualizar `dashboard-page-primitives/SKILL.md` com inventário completo + a regra "NADA inline em pages"
- [ ] Criar `scripts/check-pages-no-inline-visual.mjs` na raiz: AST-walk em `apps/dashboard/src/pages/*.tsx`, falha se encontrar (a) `style={...}` com propriedades visuais (color/background/border/borderRadius/boxShadow), (b) `className` contendo classes Tailwind visuais (`text-{cor}`, `bg-{cor}`, `border-{cor}`, `rounded-*`, `shadow-*`), (c) hex string `#[0-9a-f]{3,8}` literal. Permite layout puro (grid/flex/gap/w/h/max-w/col-span)
- [ ] Build + lint

### Wave 3 — Layout shell + button signature (ui, model: opus)

- [ ] `AppShell.tsx`: canvas `bg-background` agora resolve para `#0b0e11` (dark); ajustar padding interno se DESIGN.md prescrever ritmo diferente
- [ ] `Sidebar.tsx`: surface `#1e2329`, weights modestos (≤600), divisores hairline; status indicator via `StatusDot` do barril
- [ ] `Topbar.tsx`: altura 40px (assinatura Binance), divisor hairline, type scale display
- [ ] `SplitDetail.tsx`: alinhar gap ao ritmo unificado
- [ ] `components/ui/button.tsx`: ajustar variant `default`/`primary` para assinatura "black on yellow" (bg `--primary`, text `--primary-foreground` = preto, 40px altura, 6px radius)
- [ ] Build + lint

### Wave 4 — Refator estrutural (ui, model: opus)

Detalhes em `[[wave-4-ui]]`. Folder-per-component + namespace `src/features/` para os 8 domínios; codemod determinístico + script AC-10; sweep de tokens fantasmas. Sem mudança visual nem de comportamento.

### Wave 5 — Pages high-traffic (ui, model: opus)

Detalhes em `[[wave-5-ui]]`. `Workspace`, `Specs`, `Economia`, `Knowledge` compõem `<PageSurface>` + `<EditorialBand>` + primitivas; imports via `@/features/*`; check-pages-no-inline-visual passa nas 4.

### Wave 6 — Pages secondary (ui, model: opus)

Detalhes em `[[wave-6-ui]]`. `ProjectDetail`, `SpecDetail`, `Prd`, `Commands`, `Settings`, `Preferences`, `Home` no mesmo padrão; AC-6 e AC-10 do parent ficam verdes ao fim — destrava CLOSE do wave plan.

## Dependências

- Wave 2 depende de Wave 1 (tokens unificados antes de consolidar barril)
- Wave 3 depende de Wave 2 (primitivas estabilizadas antes do shell)
- Wave 4 depende de Wave 3 (shell estável antes do refator estrutural)
- Wave 5 depende de Wave 4 (`features/*` existem antes das pages alto-tráfego migrarem)
- Wave 6 depende de Wave 5 (padrão `<PageSurface>` + `<EditorialBand>` validado primeiro nas 4 high-traffic)
- npm: `+@fontsource-variable/ibm-plex-mono`, `-@fontsource-variable/jetbrains-mono`, `-@fontsource/jetbrains-mono`. Wave 4 pode adicionar `acorn-jsx` devDep se necessário.

## Limites

Editar dentro de:
- `apps/dashboard/src/style.css`, `apps/dashboard/src/styles/theme.css` (deletar), `apps/dashboard/src/main.tsx`
- `apps/dashboard/src/components/{page,layout,ui}/**`, `apps/dashboard/src/components/ds/**` (deletar)
- `apps/dashboard/src/components/{Markdown,StatusDot}.tsx` (mover)
- `apps/dashboard/src/pages/**`
- `apps/dashboard/{DESIGN.md,package.json}`, `apps/dashboard/.claude/{CLAUDE.md,skills/dashboard-page-primitives/SKILL.md}`
- `scripts/check-pages-imports.mjs` (novo)

**Não tocar** (`[BOUNDARY WARNING]` se aparecer):
- `apps/dashboard/src/components/{specs,workspace,telemetry,trace,prd,knowledge,amend}/**` exceto trocar import de barril
- `apps/dashboard/src/{api,lib,hooks}/**`, `apps/dashboard/src-tauri/**`
- Qualquer coisa fora de `apps/dashboard/` exceto `scripts/check-pages-imports.mjs`

## Cobertura

- "produamente [sic] com quebra de padrões de layout" → Waves 4+5 (migração 11 pages)
- "centralizar os componentes" → Wave 2 (consolida `ds/` em `page/`, deleta duplicado)
- "extrair aqueles que podem ser reaproveitados" → Wave 2 (`PageSurface`, `EditorialBand`, `DeltaText`, `StatPill`)
- "componentes padronizados" → Wave 2 + Component Contract (barril único `@/components/page`)
- "shadcn (última versão) + tailwind" → mantido (shadcn 4.7 + tailwind 4.3)
- "design system definido" → Wave 1 (`DESIGN.md` na raiz)
- "npx getdesign@latest add binance" → Wave 1 (executado one-shot, output commitado)
- "Você é o design senior" → spec adota integralmente Binance hex/typography rules/canvas/spacing/trading-semantics; única decisão senior preservada é **manter `#dfab01`** (recalibração do user após primeira proposta) — Inter + IBM Plex Mono como fallback de fontes proprietárias é decisão de licença, não de gosto
- "ajuste todas as rotas" → Waves 4 (4 pages) + 5 (7 pages) = 11 rotas
- "manter apenas a cor o resto é pra fazer tudo igual a binance" (recalibração) → Override table no Component Contract documenta cada token: 1 exceção (`--primary`) + 2 substituições por licença (fonts); resto idêntico ao DESIGN.md Binance
- "quero tudo componetizado" (recalibração final) → Regra HARD no Component Contract: pages SÓ compõem primitivas + layout estrutural. Wave 2 cria ~22 átomos (BrandMark, NavItem, Crumb, KpiValue, EditorialEyebrow/Title/Subtitle, CostBar, BarTrack/Fill, LegendSwatch, DataRow, etc.). AC-10 enforça via AST-walk que falha em hex hardcoded, classes Tailwind visuais e `style={{}}` visual em pages
