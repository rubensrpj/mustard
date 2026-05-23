# Wave 3 — Layout shell refit (AppShell, Sidebar, Topbar, SplitDetail, button signature)

### Parent: [[2026-05-23-dashboard-design-system]]
### Stage: Close
### Outcome: Completed
### Flags:
### Scope: full (wave 3 of 5)
### Lang: pt
### Checkpoint: 2026-05-23T00:00:00Z

## Resumo

Aplicar o pack Binance do `DESIGN.md` na camada de shell do dashboard, fechando o último gap visual antes das pages migrarem (Waves 4 e 5). Hoje o `AppShell` declara um grid `[220px_1fr] [48px_1fr]` com `py-6` interno (ritmo Notion legado), o `Topbar` é `h-12` (48px) com `border-b border-border`, e o `Sidebar` usa `bg-sidebar` mas com weights pesados (`font-medium`), badges `bg-[--color-accent-mustard]/10` hardcoded e indicadores `bg-[--color-ok]` que apontam para tokens removidos na Wave 1. O `button` `variant=default` já é `bg-primary text-primary-foreground` mas mora em `h-8` (32px) — fora da assinatura Binance "black on yellow 40px". Esta wave aterrissa a assinatura Binance em 5 arquivos: grid passa a `[40px_1fr]` (Topbar 40px), `Topbar` perde o `border-b` em favor do degrau `#0b0e11 → #1e2329` (depth sem shadow, regra `DESIGN.md` §Elevation), `Sidebar` consome `bg-card` + weights ≤500, `SplitDetail` re-alinha `top-[40px]` para casar o grid novo, e `button` ganha `size=default` em `h-10` + `rounded-md` (6px) para a assinatura "preto sobre amarelo" exigida pelo `DESIGN.md` §CTA Button. Risco surfaceiado: as referências vivas `--color-ok` (Sidebar L119/229, Topbar L67) e `--color-accent-mustard` (Sidebar L121/358) são tokens fantasma de `styles/theme.css` deletado na Wave 1 — esta wave reaponta os usos dentro dos 5 arquivos para `--intent-success`/`--primary`, mas pode haver consumidores fora do escopo; se aparecerem, Waves 4/5 limpam.

## Network

- Parent: [[2026-05-23-dashboard-design-system]]
- Depende de: [[wave-2-ui]] (`PageSurface` no barril `@/components/page` está disponível; `StatusDot` migrado de `components/` para `page/` — disponível para inline replacement do `StatusDotInline` em Sidebar se valer a pena, mas fora do escopo desta wave)
- Habilita: [[wave-4-ui]] (Workspace/Specs/Economia/Knowledge montam dentro de um shell estável com 40px topbar + canvas `#0b0e11` + sidebar surface `#1e2329`); [[wave-5-ui]] idem

## Component Contract

Os 5 arquivos da wave passam a operar sob estes contratos. Cada bullet referencia tokens já consolidados em `apps/dashboard/src/style.css` (Wave 1) — nenhum token novo é introduzido.

### AppShell.tsx
- Grid muda de `grid-cols-[220px_1fr] grid-rows-[48px_1fr]` para `grid-cols-[220px_1fr] grid-rows-[40px_1fr]` (Topbar 40px, regra `DESIGN.md` §Component Patterns / Binance signature).
- Canvas continua `bg-background` (resolve `#0b0e11` em `.dark` via tokens consolidados na Wave 1, AC-7 do parent já confirmou).
- Coluna central perde `py-6`; o ritmo vertical vira responsabilidade de `<PageSurface>` (já oferece `gap-8` + `py-20` editorial) entregue na Wave 2. AppShell mantém `px-6` no wrapper interno apenas como fallback para rotas que não compõem `PageSurface`; pages que compõem ignoram esse padding porque já têm o seu.
- Sem `border` interno entre Topbar e main — a separação Binance é por degrau de luminância (`#0b0e11` canvas vs `#1e2329` Topbar), não por hairline (`DESIGN.md` §Elevation: "depth without drop shadow").

### Sidebar.tsx
- Surface continua `bg-sidebar` (resolve `#1e2329` em `.dark`); `border-r border-sidebar-border` mantido — em dark, `--sidebar-border: #2b3139` é o degrau correto da escala Binance, não hairline `#eaecef`.
- `groupHeaderClass` perde `font-medium` (passa a weight default 400) — regra Binance "weights modestos ≤600 display, 400 body".
- `toolNavItemClass` active state troca `bg-primary/10 text-primary font-medium` por `bg-primary/15 text-foreground font-medium` — type voltage Binance ("ênfase por cor de acento + tamanho", não por preencher todo o item com amarelo: o texto ativo deve ser `--foreground` para legibilidade).
- `leafItemClass` idem: active = `bg-primary/15 text-foreground font-medium`.
- `StatusDotInline` substitui as 3 referências mortas a `--color-ok` (linhas 119, 229) por `--intent-success` (token vivo pós-Wave 1); referência a `--color-accent-mustard` (linhas 121, 358) vira `--primary`; ring opacity preservada.
- Bloco "stale badge" (linhas 350-363) substitui as 3 ocorrências de `--color-accent-mustard` por `--primary`.
- `statusLineColor` (linhas 218-230) — `text-red-400` (Tailwind raw color) vira `text-[--intent-error]`; `text-[--color-ok]` (linhas 223, 229) vira `text-[--intent-success]`. Isso elimina 2 hex-equivalentes fora dos tokens.

### Topbar.tsx
- Altura sobe de `h-12` (48px) para `h-10` (40px) — assinatura Binance.
- `border-b border-border` REMOVIDO — separação Topbar↔main é por degrau de luminância (regra `DESIGN.md` §Elevation). Mantém `bg-background` (canvas `#0b0e11` em dark, então não há separação Topbar↔canvas sem o `border-b`); ALTERNATIVA: trocar `bg-background` por `bg-card` (`#1e2329`) — Topbar fica como "barra elevada" e o degrau aparece sozinho. Vence a alternativa: `bg-card` (mais alinhado ao DESIGN.md §Elevation; Topbar é parte do shell elevado igual à Sidebar).
- `px-5` mantido; `gap-3` interno preservado.
- Breadcrumb perde `font-medium` no label final — type voltage por cor (`text-foreground` vs `text-muted-foreground` no prefixo) basta; sem peso adicional.
- "live" indicator (linha 67) — `bg-[--color-ok]` (token morto) vira `bg-[--intent-success]`; classe `animate-pulse` preservada.
- Botões `h-7 w-7` (28px) ficam — são quiet action chrome, não CTA. Não atendem a regra "40px black on yellow" porque não são `variant=default` do button (são `<button>` raw); permanecem como estão.

### SplitDetail.tsx
- `top-[48px]` vira `top-[40px]` (acompanha o grid novo do AppShell).
- `left-[220px]` preservado (sidebar não muda de largura nesta wave).
- `border-l border-border` no painel direito mantido — aqui hairline FAZ sentido, é divisor estrutural entre conteúdo e painel lateral, não Topbar↔main.
- `px-6 py-6` da metade esquerda mantido (ritmo de leitura para pages que não usam `PageSurface` no modo split).

### button.tsx
- `size=default` muda de `h-8 gap-1.5 px-2.5` para `h-10 gap-2 px-4` — assinatura Binance "40px altura, padding generoso" (`DESIGN.md` §CTA Button).
- `rounded-md` na base já resolve para `--radius` (6px) via `@theme inline` Wave 1; nenhuma mudança necessária na base classe.
- `variant=default` continua `bg-primary text-primary-foreground` — em `.dark`, `--primary` é `#e6c84a` (Mustard yellow lifted) e `--primary-foreground` é `#000000` — "preto sobre amarelo" automático.
- Adicionar `hover:bg-primary-pressed` ao `variant=default` (linha 12) — hoje só tem `[a]:hover:bg-primary/80` para anchors-as-button. Token `--primary-pressed` já existe (`#b88c00` light / `#c8a92e` dark, Wave 1).
- Demais sizes (`xs`/`sm`/`lg`/`icon`/`icon-*`) preservados — são chrome interno, não CTA. Regra "40px black on yellow" se aplica só ao CTA primário, e `size=default` é a porta de entrada para isso.

## Arquivos

- `apps/dashboard/src/components/layout/AppShell.tsx`
- `apps/dashboard/src/components/layout/Sidebar.tsx`
- `apps/dashboard/src/components/layout/Topbar.tsx`
- `apps/dashboard/src/components/layout/SplitDetail.tsx`
- `apps/dashboard/src/components/ui/button.tsx`

(Estritamente 5. Tokens-fantasma fora desses 5 — se existirem — viram risco de Waves 4/5.)

## Informações da Entidade

N/A — refactor visual em camada de layout. Sem entidade de domínio.

## Tarefas

### Wave 3 — Layout shell (ui, model: opus)

- [ ] `AppShell.tsx` linha 17: trocar `grid-rows-[48px_1fr]` por `grid-rows-[40px_1fr]`. Manter `grid-cols-[220px_1fr]`, `h-screen`, `bg-background`, `text-foreground` como estão.
- [ ] `AppShell.tsx` linhas 20-23: remover `py-6` do `<div className="mx-auto w-full max-w-screen-2xl px-6 py-6">` — o ritmo vertical agora pertence ao `<PageSurface>` (Wave 2). Resultado: `<div className="mx-auto w-full max-w-screen-2xl px-6">`. Pages que ainda não migraram (Waves 4/5) absorvem o degrau temporário; smoke test confirma que o degrau não quebra visualmente.
- [ ] `Sidebar.tsx` linha 74 (`groupHeaderClass`): remover `font-medium`. Resultado: `"text-xs uppercase tracking-wider text-muted-foreground px-3 py-1.5"`.
- [ ] `Sidebar.tsx` linha 80 (`toolNavItemClass` active branch): trocar `"bg-primary/10 text-primary font-medium"` por `"bg-primary/15 text-foreground font-medium"`.
- [ ] `Sidebar.tsx` linha 89 (`leafItemClass` active): trocar `"bg-primary/10 text-primary font-medium"` por `"bg-primary/15 text-foreground font-medium"`.
- [ ] `Sidebar.tsx` linha 119: trocar `"bg-[--color-ok] ring-[--color-ok]/30"` por `"bg-[--intent-success] ring-[--intent-success]/30"`.
- [ ] `Sidebar.tsx` linhas 120-122: trocar `"bg-[--color-accent-mustard] ring-[--color-accent-mustard]/30"` por `"bg-[--primary] ring-[--primary]/30"`.
- [ ] `Sidebar.tsx` linha 222: trocar `statusLineColor = "text-red-400"` por `statusLineColor = "text-[--intent-error]"`.
- [ ] `Sidebar.tsx` linha 224: trocar `statusLineColor = "text-[--color-ok]"` por `statusLineColor = "text-[--intent-success]"`.
- [ ] `Sidebar.tsx` linha 229: trocar `statusLineColor = updateAvailable ? "text-red-400" : "text-[--color-ok]"` por `statusLineColor = updateAvailable ? "text-[--intent-error]" : "text-[--intent-success]"`.
- [ ] `Sidebar.tsx` linhas 357-359 (stale badge classNames): trocar as 3 ocorrências de `--color-accent-mustard` por `--primary`. Resultado: `"border border-[--primary]/30 bg-[--primary]/10 text-[--primary]"`.
- [ ] `Topbar.tsx` linha 58: trocar `"h-12 sticky top-0 bg-background border-b border-border"` por `"h-10 sticky top-0 bg-card"`. Sem `border-b` (depth por degrau de luminância).
- [ ] `Topbar.tsx` linha 63: remover `font-medium` do `<span>` do label do crumb. Resultado: `<span className="text-foreground truncate">{label}</span>`.
- [ ] `Topbar.tsx` linha 67: trocar `"bg-[--color-ok]"` por `"bg-[--intent-success]"`.
- [ ] `SplitDetail.tsx` linha 24: trocar `top-[48px]` por `top-[40px]`. Resto do JSX preservado.
- [ ] `button.tsx` linha 12 (`variant=default`): trocar `"bg-primary text-primary-foreground [a]:hover:bg-primary/80"` por `"bg-primary text-primary-foreground hover:bg-primary-pressed [a]:hover:bg-primary-pressed"`.
- [ ] `button.tsx` linha 24-25 (`size=default`): trocar `"h-8 gap-1.5 px-2.5 has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2"` por `"h-10 gap-2 px-4 has-data-[icon=inline-end]:pr-3 has-data-[icon=inline-start]:pl-3"`. Demais sizes intactos.
- [ ] `pnpm --filter mustard-dashboard build` passa (TypeScript não-quebra; tokens referenciados todos existem em `style.css` Wave 1).
- [ ] Visual smoke: `pnpm --filter mustard-dashboard dev` — abrir `/workspace` e `/specs`. Confirmar (a) canvas `#0b0e11` em dark, (b) Topbar 40px com surface `#1e2329` (degrau visível contra canvas), (c) Sidebar mesmo `#1e2329` (continuidade lateral), (d) CTA primário (botão "Add project" se houver em algum lugar, ou qualquer `Button` default) com fundo amarelo Mustard e texto preto, altura 40px.

## Dependências

- Wave 2 entregou `PageSurface` no barril `@/components/page` com `gap-8` + `py-20` editorial (`apps/dashboard/src/components/page/PageSurface.tsx`) — Wave 3 NÃO consome PageSurface diretamente (shell vive um nível acima), mas o redesign do `AppShell` (remoção de `py-6`) ASSUME que pages das Waves 4/5 vão compor `<PageSurface>` em vez de herdar ritmo do shell.
- Tokens consumidos (todos existentes em `apps/dashboard/src/style.css` pós-Wave 1):
  - `--background` (`#0b0e11` dark canvas)
  - `--card` (`#1e2329` dark surface)
  - `--sidebar` (alias de `#1e2329`)
  - `--sidebar-border` (`#2b3139` dark divisor)
  - `--primary` (`#e6c84a` dark / `#dfab01` light — Mustard brand)
  - `--primary-foreground` (`#000000` — preto sobre amarelo)
  - `--primary-pressed` (`#c8a92e` dark / `#b88c00` light — hover)
  - `--intent-success` (`#0ecb81` Binance up green — substitui `--color-ok` fantasma)
  - `--intent-error` (`#f6465d` Binance down red — substitui `text-red-400` Tailwind raw)
- Sem nova dependência npm.

## Limites

Editar dentro de:
- `apps/dashboard/src/components/layout/{AppShell,Sidebar,Topbar,SplitDetail}.tsx`
- `apps/dashboard/src/components/ui/button.tsx`

**Não tocar** (`[BOUNDARY WARNING]` se aparecer):
- `apps/dashboard/src/style.css` — Wave 1 consolidou tokens; Wave 3 só consome
- `apps/dashboard/src/components/page/**` — Wave 2 consolidou primitivas; Wave 3 só observa o contrato
- `apps/dashboard/src/pages/**` — Waves 4 e 5
- `apps/dashboard/src/components/{specs,workspace,telemetry,trace,prd,knowledge,amend,economy}/**` — page-bound, ficam onde estão
- `apps/dashboard/src/components/ui/*` exceto `button.tsx` — shadcn intacto
- Hooks (`src/hooks/**`), lib (`src/lib/**`), api (`src/api/**`) — sem mudança de comportamento ou tipo
- `apps/dashboard/src-tauri/**`, `apps/dashboard/package.json`, `apps/dashboard/DESIGN.md`, `.claude/**`
- Qualquer arquivo fora de `apps/dashboard/src/components/{layout,ui}/`

## Critérios de Aceitação

- [ ] AC-W3-1: dashboard build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W3-2: `Topbar.tsx` declara altura 40px (`h-10`) e NÃO contém `h-12` no JSX root — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/layout/Topbar.tsx','utf8');if(!/h-10/.test(c))process.exit(1);if(/h-12/.test(c))process.exit(2);console.log('ok')"`
- [ ] AC-W3-3: `AppShell.tsx` grid usa `grid-rows-[40px_1fr]` (acompanha Topbar) e NÃO contém `48px` literal — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/layout/AppShell.tsx','utf8');if(!/grid-rows-\[40px_1fr\]/.test(c))process.exit(1);if(/48px/.test(c))process.exit(2);console.log('ok')"`
- [ ] AC-W3-4: `SplitDetail.tsx` ancora o overlay em `top-[40px]` (em sincronia com o grid novo) e NÃO contém `top-[48px]` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/layout/SplitDetail.tsx','utf8');if(!/top-\[40px\]/.test(c))process.exit(1);if(/top-\[48px\]/.test(c))process.exit(2);console.log('ok')"`
- [ ] AC-W3-5: `button.tsx` `size=default` declara `h-10` (40px CTA Binance) e variant `default` aponta para `bg-primary text-primary-foreground` (assinatura "black on yellow") — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/ui/button.tsx','utf8');if(!/h-10 gap-2 px-4/.test(c))process.exit(1);if(!/bg-primary text-primary-foreground/.test(c))process.exit(2);console.log('ok')"`
- [ ] AC-W3-6: zero referências vivas aos tokens fantasma `--color-ok` e `--color-accent-mustard` nos 5 arquivos da wave — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/components/layout/AppShell.tsx','apps/dashboard/src/components/layout/Sidebar.tsx','apps/dashboard/src/components/layout/Topbar.tsx','apps/dashboard/src/components/layout/SplitDetail.tsx','apps/dashboard/src/components/ui/button.tsx'];for(const f of files){const c=fs.readFileSync(f,'utf8');if(/--color-ok\b/.test(c)){console.error('color-ok still in',f);process.exit(1)}if(/--color-accent-mustard\b/.test(c)){console.error('accent-mustard still in',f);process.exit(2)}}console.log('ok')"`
- [ ] AC-W3-7: `Sidebar.tsx` não usa mais `text-red-400` (Tailwind raw color) — substituído por token `--intent-error` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/layout/Sidebar.tsx','utf8');if(/text-red-400/.test(c))process.exit(1);if(!/--intent-error/.test(c))process.exit(2);console.log('ok')"`
- [ ] AC-W3-8: `Topbar.tsx` consome `bg-card` (Topbar como surface elevada) e NÃO carrega `border-b` no header root (depth por degrau, não hairline) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/layout/Topbar.tsx','utf8');if(!/bg-card/.test(c))process.exit(1);const header=c.match(/<header[^>]*>/);if(header&&/border-b\b/.test(header[0]))process.exit(2);console.log('ok')"`

## Checklist

- [ ] Build dashboard passa (`pnpm --filter mustard-dashboard build`)
- [ ] Visual smoke OK em 2 rotas (Workspace + Specs) com Topbar 40px, canvas `#0b0e11`, sidebar `#1e2329`, CTA amarelo+texto preto 40px
- [ ] Sem regressão em pages existentes (Workspace, Specs ainda abrem; SplitDetail abre alinhado ao novo topo de 40px)
- [ ] Nenhuma referência viva a `--color-ok`, `--color-accent-mustard` ou `text-red-400` dentro dos 5 arquivos da wave
- [ ] `button` variant `default` em "black on yellow 40px" — `bg-primary` resolve `#e6c84a` (dark) / `#dfab01` (light), `text-primary-foreground` resolve `#000000`, `h-10` confirma altura
