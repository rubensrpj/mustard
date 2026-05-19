# Feature: dashboard-notion-linear-refactor

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-13T00:00:00Z
### Lang: pt

## Contexto

O toggle de tema do dashboard está visualmente quebrado por um conflito de cascade em `src/style.css`: o bloco `.dark { ... }` aparece **antes** do bloco `:root { ... }` no arquivo, e como ambos têm specificity `(0,1,0)` o que está depois ganha — então `<html class="dark">` recebe tokens light apesar da classe presente, e cliques em "Alternar tema" mudam a classe mas não as cores. Plus: o conjunto atual de telas (Sidebar, Topbar, Home, ProjectDetail) só usa `Card` do shadcn; falta `Tabs`, `Separator`, `Badge`, `ScrollArea`, `Dialog`, `Tooltip` para alcançar o contrato Notion/Linear de `REFERENCE.md §3b` (workspace header, sections com separator, breadcrumb Notion, lists densas com badge por tipo, command palette modal). O resultado é uma UI que parece um shadcn default ao invés do produto que está sendo prometido. Esta wave consolida o style.css numa única fonte de verdade (shadcn v4 canônico), corrige o bug de cascade, amplia o shadcn com 6 primitives, e reestrutura layout/páginas no idioma Notion+Linear — base sólida antes de multi-project discovery.

## Boundaries

modify: `src/style.css`, `index.html`, `src/main.tsx`, `src/App.tsx`, `src/components/CommandPalette.tsx`, `src/components/layout/Sidebar.tsx`, `src/components/layout/Topbar.tsx`, `src/components/layout/AppShell.tsx`, `src/pages/Home.tsx`, `src/pages/ProjectDetail.tsx`, `package.json`/`pnpm-lock.yaml`

create: `src/hooks/useTheme.ts`, `src/components/ui/tabs.tsx`, `src/components/ui/separator.tsx`, `src/components/ui/badge.tsx`, `src/components/ui/scroll-area.tsx`, `src/components/ui/dialog.tsx`, `src/components/ui/tooltip.tsx`

Fora do escopo: qualquer arquivo em `src-tauri/`, `apps/`, multi-project discovery, license gate, novos hooks de dados.

## Summary

3 vetores: (1) consertar toggle (style.css source order + `useTheme` hook com localStorage + system pref); (2) paleta Notion/Linear via tokens shadcn v4 puros (sem `@theme` light/dark duplicado); (3) shadcn ampliado (Tabs, Separator, Badge, ScrollArea, Dialog, Tooltip) + Sidebar Linear (workspace header + sections com Separator + bottom settings) + Topbar Notion (breadcrumb + Cmd+K kbd hint + theme toggle) + Pages Linear-style (lists densas, Badge por event type, ScrollArea).

## Files (~17)

```
modify   src/style.css                                # tokens canônicos shadcn v4, :root antes de .dark
modify   index.html                                   # inline theme script (localStorage + matchMedia) + lang pt-BR
modify   src/main.tsx                                 # mantém font imports
modify   src/App.tsx                                  # CommandPalette + ThemeProvider wrap
modify   src/components/CommandPalette.tsx            # shadcn Dialog + cmdk + useTheme
modify   src/components/layout/Sidebar.tsx            # workspace header + sections + Separator + settings
modify   src/components/layout/Topbar.tsx             # breadcrumb + Cmd+K kbd + theme ghost button
modify   src/components/layout/AppShell.tsx           # 220×48 grid (mantém)
modify   src/pages/Home.tsx                           # Cards size sm + StatusDot + ring sutil
modify   src/pages/ProjectDetail.tsx                  # Sections com Separator + Badge per event type + ScrollArea
modify   package.json                                 # peers via shadcn add
create   src/hooks/useTheme.ts                        # useTheme hook
create   src/components/ui/tabs.tsx                   # shadcn Tabs
create   src/components/ui/separator.tsx              # shadcn Separator
create   src/components/ui/badge.tsx                  # shadcn Badge
create   src/components/ui/scroll-area.tsx            # shadcn ScrollArea
create   src/components/ui/dialog.tsx                 # shadcn Dialog
create   src/components/ui/tooltip.tsx                # shadcn Tooltip
```

## Component Contract

### `useTheme()`

```typescript
type Theme = "dark" | "light";
interface UseTheme {
  theme: Theme;
  toggle: () => void;
  setTheme: (t: Theme) => void;
}
function useTheme(): UseTheme;
```

- Initial: `localStorage.getItem("mustard-theme")` OR `matchMedia('(prefers-color-scheme: dark)')` OR `"dark"`.
- `setTheme`: aplica/remove `dark` class no `<html>`, grava localStorage, atualiza state.
- Subscribe `matchMedia` "change" se localStorage não setado (system follow mode).
- No primeiro render, NÃO re-aplica classe (já foi setada pelo inline script no index.html) — só sincroniza state com `document.documentElement.classList.contains("dark")`.

### Sidebar (Linear-style)

Top → bottom:
1. **Workspace header** — `<div className="px-3 py-2 border-b border-border">`: "Mustard" font-semibold + version "v0.1.0" em badge ghost.
2. **Section "Navigation"** — header `text-[11px] uppercase tracking-wider text-muted-foreground px-3 py-1.5`; items: Home, Projeto.
3. `<Separator />`
4. **Section "Workspace"** — placeholder "0 subprojects" (futuro: workspace switcher).
5. Bottom (mt-auto) — ghost button "Settings" com icon lucide `Settings`.

NavLink items: `flex items-center gap-2 px-3 py-1.5 rounded-md text-sm transition-colors duration-150`. Active: `bg-primary/10 text-primary font-medium`. Idle: `text-sidebar-foreground/70 hover:bg-muted/40 hover:text-foreground`.

### Topbar (Notion-style)

```
[Mustard / Home]                                    [ Ctrl+K ]  [☀ tema]
```

- Left: breadcrumb `text-muted-foreground` + slash separator + `text-foreground font-medium` para label atual.
- Right (actions row, `gap-2`):
  - `<kbd>` (style border + bg-muted text-xs) com "Ctrl K" ou "⌘ K" (detect platform).
  - Ghost button com lucide `Sun`/`Moon` icon (alterna conforme theme atual).

### CommandPalette

```typescript
interface CommandPaletteProps {}
```

- Renderiza via `shadcn Dialog` (`@/components/ui/dialog`) com `<DialogContent>` containing `cmdk Command`.
- Global keydown: Ctrl/Cmd+K toggles open, Esc closes (Dialog primitive handles Esc).
- Sections:
  - **Navigate**: "Go to Home" → `navigate("/")`, "Go to Projeto" → `navigate("/project")`.
  - **Theme**: "Switch to light" / "Switch to dark" (apenas o oposto do atual).
- Items selected highlight: `data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary`.

### Home (Linear-style cards)

3 cards no grid `gap-3`. Cada `<Card size="sm">`:
- `<CardHeader>` com `<CardTitle>` text-sm font-medium + `<CardDescription>` text-xs text-muted-foreground.
- Pipelines card: StatusDot ao lado do título quando `n > 0` (variant "planning" se ≥1 em PLAN/EXECUTE, "done" se todos CLOSE).
- Métricas card: numbers em font-mono.
- Knowledge card: same pattern.

### ProjectDetail (Linear-style sections)

Estrutura:
```
[Section heading SUBPROJECTS  (3)]
─────── Separator ───────
[list densa com StatusDot]

[Section heading RECIPES  (12)]
─────── Separator ───────
[list densa]

[Section heading SKILLS  (8)]
─────── Separator ───────
[list densa com Badge [foundation] / [command]]

[Section heading EVENTOS RECENTES  (20)]
─────── Separator ───────
[ScrollArea com lista de events; cada row: StatusDot + Badge(event_type) + relativeTime + summary]
```

Section heading: `text-[11px] uppercase tracking-wider font-medium text-muted-foreground` + count em `text-muted-foreground/50`. Separator é `<Separator className="my-3" />`.

## Tasks

### Wave 1 — Paleta canônica + shadcn add (parallel-safe)

#### Style Agent (Wave 1)

- [ ] Reescrever `src/style.css`: remover bloco `@theme { --color-* }` light (linhas 9-21) e o bloco light-mode duplicado; manter apenas `@import` lines, `@custom-variant dark`, `@variant dark`, `@theme inline { --font-* --color-* aliases }`, `:root { ... light tokens ... }`, depois `.dark { ... dark tokens ... }`, depois `@layer base`. **`:root` DEVE vir antes de `.dark`** no source order. Tokens REFERENCE §3b:
  - Light `:root`: bg `hsl(0 0% 100%)`, card `hsl(0 0% 100%)`, primary `#6366F1`, muted `hsl(210 20% 96%)`, border `rgba(0,0,0,0.08)`, ring `#6366F1`, sidebar `hsl(210 20% 98%)`, sidebar-foreground `hsl(222 47% 11%)`.
  - Dark `.dark`: bg `#0F0F10`, card `#161618`, popover `#161618`, primary `#6366F1`, muted `#1B1B1D`, border `rgba(255,255,255,0.06)`, ring `#818CF8`, sidebar `#0F0F10`, sidebar-foreground `hsl(210 20% 98%)`.
  - Manter `--radius: 0.625rem` e os `--chart-*` oklch.
- [ ] Em `index.html`: trocar `lang="en"` por `lang="pt-BR"`; manter inline script atual mas robustecer — checar `matchMedia('(prefers-color-scheme: dark)').matches` se `localStorage['mustard-theme']` for null.

#### shadcn-Add Agent (Wave 1, parallel-safe)

- [ ] Rodar `pnpm dlx shadcn@latest add tabs separator badge scroll-area dialog tooltip --yes` (Windows: `pnpm dlx shadcn@latest add tabs separator badge scroll-area dialog tooltip` se `--yes` for incompat). Aceita prompts. Gera 6 arquivos em `src/components/ui/`.
- [ ] **Fallback se CLI falhar**: copiar manualmente as 6 implementações canônicas do registry shadcn neutral/new-york (versão Tailwind v4 compatível). Cada arquivo deve usar `cn` de `@/lib/utils`, imports de `radix-ui` (já instalado), exports nomeados.
- [ ] Verificar `pnpm build` passa após adicionar.

### Wave 2 — useTheme + Layout Linear/Notion (depende de Wave 1)

#### Hook Agent (Wave 2)

- [ ] Criar `src/hooks/useTheme.ts`:
  ```typescript
  import { useState, useEffect, useCallback } from "react";
  type Theme = "dark" | "light";
  function readInitial(): Theme {
    if (typeof window === "undefined") return "dark";
    try {
      const stored = localStorage.getItem("mustard-theme");
      if (stored === "dark" || stored === "light") return stored;
    } catch { /* ignore */ }
    if (window.matchMedia("(prefers-color-scheme: dark)").matches) return "dark";
    return "dark"; // default dark for product
  }
  export function useTheme() {
    const [theme, setThemeState] = useState<Theme>(() => {
      if (typeof document === "undefined") return "dark";
      return document.documentElement.classList.contains("dark") ? "dark" : "light";
    });
    const setTheme = useCallback((t: Theme) => {
      const root = document.documentElement;
      root.classList.toggle("dark", t === "dark");
      try { localStorage.setItem("mustard-theme", t); } catch { /* ignore */ }
      setThemeState(t);
    }, []);
    const toggle = useCallback(() => setTheme(theme === "dark" ? "light" : "dark"), [theme, setTheme]);
    useEffect(() => {
      // sync state if class was changed externally on mount (inline script)
      const current: Theme = document.documentElement.classList.contains("dark") ? "dark" : "light";
      if (current !== theme) setThemeState(current);
      // subscribe to system pref if no stored
      let stored: string | null = null;
      try { stored = localStorage.getItem("mustard-theme"); } catch { /* ignore */ }
      if (stored) return;
      const mq = window.matchMedia("(prefers-color-scheme: dark)");
      const onChange = (e: MediaQueryListEvent) => setTheme(e.matches ? "dark" : "light");
      mq.addEventListener("change", onChange);
      return () => mq.removeEventListener("change", onChange);
    }, []); // eslint-disable-line
    return { theme, toggle, setTheme };
  }
  ```

#### Layout Agent (Wave 2)

- [ ] Refatorar `src/components/layout/Sidebar.tsx` per Component Contract — workspace header + 2 sections + Separator + bottom Settings ghost button (use `lucide-react` Settings icon).
- [ ] Refatorar `src/components/layout/Topbar.tsx` — breadcrumb left, actions row right com `<kbd>` (Ctrl+K com platform detect via `navigator.platform`) + ghost button com `Sun`/`Moon` lucide icon usando `useTheme`.
- [ ] AppShell stays — `grid-cols-[220px_1fr] grid-rows-[48px_1fr]` + `p-5` main.

### Wave 3 — Pages Linear + CommandPalette shadcn (depende de Wave 2)

#### Pages Agent (Wave 3)

- [ ] Home: ajustar cards para `size="sm"`, ring sutil já provido pelo `ring-1 ring-foreground/10` do shadcn Card default, `gap-3` no grid, StatusDot ao lado do título Pipelines com variant dependent on phase (planning se ≥1 em ANALYZE/PLAN/EXECUTE; done caso contrário). Métricas e Knowledge: numbers em `<code className="font-mono">`.
- [ ] ProjectDetail: layout Linear sections. Cada section: heading uppercase + count badge + `<Separator className="my-3" />` + list densa. Events: `<Badge variant="secondary">{event_type}</Badge>` + StatusDot + relativeTime + summary. ScrollArea no contêiner de events com `max-h-[400px]`. Skills: source em Badge variant outline.

#### Palette Agent (Wave 3)

- [ ] Refatorar `src/components/CommandPalette.tsx`: usa `Dialog`, `DialogContent` de `@/components/ui/dialog`. Inside: cmdk `<Command>` com `<Command.Input>`, `<Command.List>`, `<Command.Group heading="Navigate">`, `<Command.Group heading="Theme">`. Theme group mostra apenas a opção contrária ao atual ("Switch to light" se dark, vice-versa). Esc gerenciado pelo Dialog. Ctrl/Cmd+K toggle via useEffect global window listener.
- [ ] `src/App.tsx`: garantir `<CommandPalette />` dentro de `<HashRouter>` (já está).

## Dependências

- Wave 2 depende de Wave 1 (Hook precisa que tokens estejam corretos; Layout precisa de Separator/lucide-react).
- Wave 3 depende de Wave 2 (Pages precisam de Badge/ScrollArea + useTheme indireto via Sidebar; Palette precisa de Dialog).
- Wave 1 Style Agent e Wave 1 shadcn-Add Agent são independentes — paralelizáveis.

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: `:root` aparece antes de `.dark` em `src/style.css` (source order correto pra cascade resolver). 
  Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/style.css','utf8');const r=c.indexOf(':root');const d=c.indexOf('.dark {');if(r<0||d<0){console.error('selectors missing');process.exit(1)}if(r>d){console.error('source order wrong: :root at',r,'.dark at',d,'(:root must come BEFORE .dark)');process.exit(1)}if(c.match(/@theme\s*\{[^}]*--color-background/)){console.error('legacy @theme block still present');process.exit(1)}console.log('OK source order :root at',r,'.dark at',d)"`

- [x] AC-2: `useTheme` hook existe com localStorage + matchMedia.
  Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/hooks/useTheme.ts','utf8');if(!/export\s+function\s+useTheme/.test(c)){console.error('useTheme export missing');process.exit(1)}if(!/localStorage/.test(c)){console.error('no localStorage');process.exit(1)}if(!/prefers-color-scheme/.test(c)){console.error('no system pref subscription');process.exit(1)}console.log('OK')"`

- [x] AC-3: 6 shadcn components criados.
  Command: `node -e "const fs=require('fs');const need=['tabs','separator','badge','scroll-area','dialog','tooltip'];const miss=need.filter(n=>!fs.existsSync('src/components/ui/'+n+'.tsx'));if(miss.length){console.error('missing:',miss.join(','));process.exit(1)}console.log('OK')"`

- [x] AC-4: Sidebar tem Separator + workspace header + sections.
  Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src/components/layout/Sidebar.tsx','utf8');if(!/Separator/.test(s)){console.error('no Separator');process.exit(1)}if(!/Navigation|Navega/i.test(s)){console.error('no Navigation section heading');process.exit(1)}if(!/lucide-react/.test(s)){console.error('no lucide icon usage');process.exit(1)}console.log('OK')"`

- [x] AC-5: Topbar tem Cmd+K kbd hint + useTheme.
  Command: `node -e "const fs=require('fs');const t=fs.readFileSync('src/components/layout/Topbar.tsx','utf8');if(!/<kbd/i.test(t)){console.error('no <kbd> element');process.exit(1)}if(!/useTheme/.test(t)){console.error('not using useTheme hook');process.exit(1)}if(!/Sun|Moon/.test(t)){console.error('no Sun/Moon icon');process.exit(1)}console.log('OK')"`

- [x] AC-6: ProjectDetail usa Badge + Separator + ScrollArea.
  Command: `node -e "const fs=require('fs');const p=fs.readFileSync('src/pages/ProjectDetail.tsx','utf8');for(const need of ['Badge','Separator','ScrollArea']){if(!new RegExp(need).test(p)){console.error('missing',need,'usage');process.exit(1)}}console.log('OK')"`

- [x] AC-7: CommandPalette usa shadcn Dialog wrap.
  Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/components/CommandPalette.tsx','utf8');if(!/from\s+[\"']@\/components\/ui\/dialog[\"']/.test(c)){console.error('not importing shadcn Dialog');process.exit(1)}console.log('OK')"`

- [x] AC-8: Build passa.
  Command: `pnpm build`

## Non-Goals

- Multi-project discovery / workspace switcher real (placeholder text apenas)
- License gate / activation
- Real-time refresh (websocket / file watcher)
- Sidebar collapse para icon-only mode
- SpecDetail drill-down, AggregateView, KnowledgeBrowser
- Migração para TanStack Query / Zustand
- Animação de transição de tema (instant swap basta)
