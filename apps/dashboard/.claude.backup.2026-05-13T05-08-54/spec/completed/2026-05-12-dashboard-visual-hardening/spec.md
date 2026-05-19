# Feature: dashboard-visual-hardening

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-12T23:20:00Z
### Lang: pt

## Contexto

O dashboard Tauri já lê pipelines, métricas, knowledge, recipes, skills e eventos do projeto auto-hospedado, mas a primeira impressão diverge do contrato visual estabelecido em `REFERENCE.md §3b` (dark-mode-first Linear/Notion, Inter 13px, indigo `#6366F1`, cards densos com bordas finas, status dots, command palette `Cmd+K`). Hoje o app abre em light mode com fontes Geist Variable, cards arredondados demais, e o topbar duplica o título quando a Home já o anuncia. Além disso, `dashboard_recent_events` em `src-tauri/src/lib.rs:326` lê `v["type"]` quando o schema real do `.claude/.harness/events.jsonl` grava o tipo em `v["event"]` — o resultado é uma lista com 20 entradas todas rotuladas `unknown`, escondendo a única visão temporal que o usuário tem da pipeline. A mesma falha contamina `dashboard_metrics` (linha 99), fazendo `agents_dispatched` ficar zerado mesmo quando há `agent.start` no log. Antes de avançar com multi-project discovery, SpecDetail e KnowledgeBrowser, é necessário consertar o parser e alinhar a casca visual — caso contrário toda feature nova herda essa dívida.

## Boundaries

Arquivos que esta wave intencionalmente toca:

- `src-tauri/src/lib.rs` (parser fix)
- `index.html` (dark default + title)
- `src/main.tsx` (font imports)
- `src/style.css` (color tokens + typography)
- `package.json` (deps: cmdk, dayjs, @fontsource/inter, @fontsource/jetbrains-mono)
- `src/App.tsx` (mount CommandPalette + theme bootstrap)
- `src/components/StatusDot.tsx` (novo)
- `src/components/CommandPalette.tsx` (novo)
- `src/lib/time.ts` (novo)
- `src/components/layout/Sidebar.tsx`
- `src/components/layout/Topbar.tsx`
- `src/components/layout/AppShell.tsx`
- `src/pages/Home.tsx`
- `src/pages/ProjectDetail.tsx`

Fora do escopo: qualquer arquivo em `apps/`, `packages/`, `.claude/skills/`, multi-project discovery, novas tabs SpecDetail/Aggregate/KnowledgeBrowser, license gate.

## Summary

Wave de hardening visual + bugfix de parser de eventos: corrige campo `event` (não `type`) no backend Rust, ativa dark mode como default com paleta REFERENCE.md §3b, troca fonte para Inter 13px, introduz `StatusDot` e command palette `Cmd+K` (cmdk), timestamps relativos via dayjs, e enxuga o cromo (breadcrumb no Topbar, hover suave na sidebar, cards densos).

## Files (~14)

```
modify   src-tauri/src/lib.rs                          # parser fix (2 sites)
modify   index.html                                    # class="dark" + title
modify   src/main.tsx                                  # font imports
modify   src/style.css                                 # tokens + Inter + 13px base
modify   package.json                                  # deps: cmdk, dayjs, @fontsource/*
modify   src/App.tsx                                   # CommandPalette mount + theme init
modify   src/components/layout/AppShell.tsx            # grid/spacing pass
modify   src/components/layout/Sidebar.tsx             # hover transition + denser nav
modify   src/components/layout/Topbar.tsx              # breadcrumb-style header
modify   src/pages/Home.tsx                            # denser cards, subtitle zinc-400
modify   src/pages/ProjectDetail.tsx                   # relativeTime + StatusDot por event
create   src/components/StatusDot.tsx                  # variants idle/active/planning/blocked/done
create   src/components/CommandPalette.tsx             # cmdk-based Cmd+K palette
create   src/lib/time.ts                               # dayjs relativeTime helper
```

## Component Contract

### `StatusDot`

```typescript
type StatusDotVariant = "idle" | "active" | "planning" | "blocked" | "done";
interface StatusDotProps {
  variant: StatusDotVariant;
  pulse?: boolean;       // active state only
  className?: string;
}
```

Render: `<span className="inline-block w-1.5 h-1.5 rounded-full {variantBg} {pulseClass}" />`.

Mapping:
- `idle` → `bg-zinc-500`
- `active` → `bg-emerald-500` (+ `animate-pulse` se `pulse`)
- `planning` → `bg-amber-500`
- `blocked` → `bg-rose-500`
- `done` → `bg-zinc-400`

### `CommandPalette`

```typescript
interface CommandPaletteProps {} // no props — uses internal open state + hotkey
```

Comportamento: monta listener global `keydown` (`Ctrl+K` em Windows/Linux, `Cmd+K` em Mac) que toggla `open`. Render: dialog `cmdk` com `<Command.Input>` + `<Command.List>` + 3 ações iniciais — "Go to Home", "Go to Projeto", "Toggle theme". Navegação por teclado (↑/↓/Enter) cortesia do `cmdk`. Esc fecha. Backdrop com `bg-black/50`.

### `relativeTime(iso: string): string`

Wrapper sobre `dayjs(iso).fromNow()` com locale pt-BR. Retorna strings como "agora", "5 min atrás", "2 h atrás", "ontem", "3 dias atrás". Lida com input inválido retornando `iso` original (fail-soft).

## Tasks

### Wave 1 — Backend bugfix + setup (parallel-safe)

#### Rust Agent (Wave 1)

- [ ] Em `src-tauri/src/lib.rs`, `dashboard_recent_events` (linha ~326): trocar `v["type"].as_str().unwrap_or("unknown")` por `v["event"].as_str().unwrap_or("unknown")`.
- [ ] Mesma função: também aceitar legacy `v["type"]` como fallback (compat: `v["event"].as_str().or_else(|| v["type"].as_str()).unwrap_or("unknown")`).
- [ ] Em `dashboard_metrics` (linha ~99): trocar `v["type"].as_str() == Some("agent.start")` por `v["event"].as_str() == Some("agent.start")` (mesmo fallback `v["type"]`).
- [ ] Rodar `cd src-tauri && cargo check` — deve passar sem warnings novos.

#### Frontend Setup Agent (Wave 1, parallel-safe)

- [ ] `pnpm add cmdk dayjs @fontsource/inter @fontsource/jetbrains-mono` (atualiza `package.json` + `pnpm-lock.yaml`).
- [ ] Em `src/main.tsx`: importar `@fontsource/inter/400.css`, `@fontsource/inter/500.css`, `@fontsource/inter/600.css`, `@fontsource/jetbrains-mono/400.css` antes de `./style.css`.
- [ ] Em `index.html`: adicionar `class="dark"` em `<html lang="en" class="dark">`; trocar `<title>` para `Mustard Dashboard`.
- [ ] Em `src/style.css`: substituir `--color-background`/`--color-foreground`/`--color-sidebar`/`--color-border`/`--color-primary` (ambos `@theme` e `.dark` selectors) pelos hex da REFERENCE.md §3b — bg `#0F0F10`, card `#161618`, border dark `rgba(255,255,255,0.06)`, border light `rgba(0,0,0,0.08)`, primary `#6366F1` (indigo-500), primary-hover `#818CF8` (indigo-400). Manter compat com tokens `oklch` existentes para shadcn.
- [ ] Em `src/style.css` `@theme inline`: trocar `--font-sans: 'Geist Variable', sans-serif` por `--font-sans: 'Inter', system-ui, sans-serif` e adicionar `--font-mono: 'JetBrains Mono', monospace`.
- [ ] Em `src/style.css` `@layer base { html { ... } }`: adicionar `font-size: 13px` (era default 16px).

### Wave 2 — Frontend componentes novos (depende de Wave 1)

#### Frontend Components Agent (Wave 2)

- [ ] Criar `src/components/StatusDot.tsx` conforme Component Contract.
- [ ] Criar `src/lib/time.ts`: importar `dayjs` + plugin `relativeTime` + locale `pt-br`; exportar `relativeTime(iso: string): string` com try/catch retornando `iso` em falha.
- [ ] Criar `src/components/CommandPalette.tsx`: usa `cmdk` (`Command`, `Command.Input`, `Command.List`, `Command.Item`); estado local `open`; `useEffect` registra `keydown` global (`(e.metaKey || e.ctrlKey) && e.key === 'k'` toggla); três ações — `navigate('/')`, `navigate('/project')`, `toggleTheme()` (lê `document.documentElement.classList`, alterna `dark`); fecha após executar.
- [ ] Estilizar o overlay do palette: backdrop `bg-black/50 backdrop-blur-sm`, dialog `bg-card border border-border rounded-lg shadow-lg w-[min(560px,90vw)]`, items com hover `bg-muted` e selected `bg-primary/10`.

### Wave 3 — Integração + polish (depende de Wave 2)

#### Frontend Integration Agent (Wave 3)

- [ ] Em `src/App.tsx`: importar e renderizar `<CommandPalette />` dentro de `<HashRouter>` (precisa do contexto de router para `useNavigate`).
- [ ] Em `src/components/layout/Topbar.tsx`: converter em breadcrumb estilo Notion — renderizar `Mustard / {pageLabel}` usando `useLocation()` (`/` → "Home", `/project` → "Projeto"); separador slash em `text-muted-foreground`; remover qualquer título grande "Mustard Dashboard" que duplicasse a Home.
- [ ] Em `src/components/layout/Sidebar.tsx`: adicionar `transition-colors duration-150` aos `NavLink`; aplicar hover sutil `hover:bg-muted/40`; reduzir espaçamento (`gap-0.5` ao invés de `gap-1`); marcar `font-medium` apenas no estado ativo.
- [ ] Em `src/components/layout/AppShell.tsx`: reduzir `p-6` do `<main>` para `p-5`; manter grid existente.
- [ ] Em `src/pages/Home.tsx`: aplicar `gap-3` ao invés de `gap-4`; `CardDescription` em `text-muted-foreground text-xs` (não preto); cards com `rounded-md` (8px) garantido; opcional `StatusDot` ao lado do título "Pipelines" baseado na fase dominante.
- [ ] Em `src/pages/ProjectDetail.tsx`: importar `relativeTime` e renderizar `relativeTime(e.ts)` no lugar de `@ {e.ts}`; importar `StatusDot` e renderizar dot colorido por `event_type` (mapping: `tool.use` → idle, `pipeline.phase` → planning, `qa.result` → done, `agent.start` → active, default → idle).
- [ ] Rodar `pnpm build` — deve passar sem erros TS nem warnings de import faltante.

## Dependências

- Wave 2 depende de Wave 1 (Frontend Setup precisa estar feito para que as novas deps existam).
- Wave 3 depende de Wave 2 (Integration consome `StatusDot`, `CommandPalette`, `relativeTime`).
- Wave 1 Rust e Wave 1 Frontend Setup são totalmente independentes — podem rodar em paralelo.

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Parser fix aplicado — `dashboard_recent_events` e `dashboard_metrics` leem `v["event"]` (não `v["type"]`).
  Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src-tauri/src/lib.rs','utf8');const a=s.match(/v\[\"event\"\]\.as_str\(\)/g)||[];if(a.length<2){console.error('expected >=2 v[event] reads, got',a.length);process.exit(1)}console.log('OK',a.length,'sites')"`

- [x] AC-2: Rust compila — `cd src-tauri && cargo check --quiet`

- [x] AC-3: Build frontend passa — `pnpm build`

- [x] AC-4: Dark mode default + paleta correta — `<html>` tem `class="dark"`, tokens REFERENCE.md presentes em `style.css`.
  Command: `node -e "const fs=require('fs');const h=fs.readFileSync('index.html','utf8');const c=fs.readFileSync('src/style.css','utf8');if(!/<html[^>]*class=\"dark\"/.test(h)){console.error('html missing dark class');process.exit(1)}if(!/0F0F10/i.test(c)){console.error('style.css missing #0F0F10 background token');process.exit(1)}if(!/6366F1/i.test(c)){console.error('style.css missing #6366F1 indigo accent');process.exit(1)}console.log('OK')"`

- [x] AC-5: Inter font + 13px base wired — main.tsx importa `@fontsource/inter`, style.css usa Inter como `--font-sans` e define `font-size: 13px` em html.
  Command: `node -e "const fs=require('fs');const m=fs.readFileSync('src/main.tsx','utf8');const c=fs.readFileSync('src/style.css','utf8');if(!/@fontsource\/inter/.test(m)){console.error('main.tsx missing @fontsource/inter import');process.exit(1)}if(!/'Inter'/.test(c)){console.error('style.css missing Inter as font-sans');process.exit(1)}if(!/font-size:\s*13px/.test(c)){console.error('style.css missing 13px base');process.exit(1)}console.log('OK')"`

- [x] AC-6: Novos componentes + helper existem com export correto — `StatusDot`, `CommandPalette`, `relativeTime`.
  Command: `node -e "const fs=require('fs');const files=['src/components/StatusDot.tsx','src/components/CommandPalette.tsx','src/lib/time.ts'];for(const f of files){if(!fs.existsSync(f)){console.error('missing',f);process.exit(1)}}const sd=fs.readFileSync('src/components/StatusDot.tsx','utf8');const cp=fs.readFileSync('src/components/CommandPalette.tsx','utf8');const tm=fs.readFileSync('src/lib/time.ts','utf8');if(!/export\s+function\s+StatusDot|export\s+const\s+StatusDot/.test(sd)){console.error('StatusDot export missing');process.exit(1)}if(!/export\s+function\s+CommandPalette|export\s+const\s+CommandPalette/.test(cp)){console.error('CommandPalette export missing');process.exit(1)}if(!/export\s+function\s+relativeTime|export\s+const\s+relativeTime/.test(tm)){console.error('relativeTime export missing');process.exit(1)}console.log('OK')"`

- [x] AC-7: Deps instaladas — `cmdk`, `dayjs`, `@fontsource/inter`, `@fontsource/jetbrains-mono` em `package.json`.
  Command: `node -e "const p=require('./package.json');const need=['cmdk','dayjs','@fontsource/inter','@fontsource/jetbrains-mono'];const have={...p.dependencies,...p.devDependencies};const miss=need.filter(d=>!have[d]);if(miss.length){console.error('missing deps:',miss.join(','));process.exit(1)}console.log('OK')"`

- [x] AC-8: Parser fix produz types reais ao ler events.jsonl — zero ocorrências de `unknown` E ≥3 tipos distintos nos últimos 100 eventos (janela maior para amortizar `tool.use` dominante).
  Command: `node -e "const fs=require('fs');const lines=fs.readFileSync('.claude/.harness/events.jsonl','utf8').trim().split('\n').slice(-100);const s=new Set();for(const l of lines){try{const v=JSON.parse(l);s.add(v.event||v.type||'unknown')}catch{}}if(s.has('unknown')){console.error('parser produced unknown — schema mismatch');process.exit(1)}if(s.size<3){console.error('only',s.size,'distinct types in last 100:',[...s].join(','));process.exit(1)}console.log('OK',s.size,'types:',[...s].join(','))"`

## Non-Goals

- Multi-project discovery (próxima wave de feature)
- SpecDetail drill-down ou AggregateView ou KnowledgeBrowser
- License gate / activation flow
- Tema light polishing (toggle existe via Cmd+K mas paridade visual completa fica deferida)
- Real-time refresh (websocket / file watcher) — Home segue snapshot on mount
- Migração para TanStack Query / Zustand
