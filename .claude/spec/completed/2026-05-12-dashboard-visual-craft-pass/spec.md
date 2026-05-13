# Enhancement: dashboard-visual-craft-pass

### Status: completed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-05-12T00:00:00Z
### Lang: pt

## Contexto

O dashboard já tem a estrutura de páginas correta (Home, ProjectDetail, Settings, CommandPalette, descoberta multi-projeto), mas o craft visual ficou atrás da intenção declarada em REFERENCE.md — densidade Linear + hierarquia Notion + dark default real. Hoje o app abre em light em Windows com tema de sistema claro porque o script inline de `index.html` honra `prefers-color-scheme` antes do localStorage; cards somem em light mode porque `--card` é idêntico a `--background`; o StatusDot de 6px é invisível em densidade de lista; a Sidebar tem um stub "Workspace" sem itens reais; e os cards/seções carecem de ícones que ajudem a leitura. A wave faz polish cirúrgico nesses pontos — sem novas rotas, sem nova feature.

## Summary

Refinar tokens visuais (light off-white, dark contraste sutil, radius 8px), forçar dark como default real, densificar Sidebar com lista dinâmica de projetos, adicionar refresh + ícones em Topbar/Home/ProjectDetail e empty states com ícone discreto.

## Boundaries

- `index.html` (script inline do tema)
- `src/style.css` (tokens `:root`, `.dark`, `--radius`)
- `src/components/StatusDot.tsx` (tamanho + prop opcional + ring active)
- `src/components/layout/Sidebar.tsx` (estrutura Navigation + Workspace + Settings)
- `src/components/layout/Topbar.tsx` (botão refresh)
- `src/pages/Home.tsx` (empty state com ícone + lucide na lista)
- `src/pages/ProjectDetail.tsx` (ícone por seção + empty state)

NÃO tocar: SQLite reader, novas rotas, novas tabs, license/CI, animações novas, nenhum arquivo Rust, nenhum arquivo de scaffold (`.claude/*`).

## Nota de divergência

O usuário citou `Sidebar.tsx` linha 22 com `<div>0 subprojects</div>` hardcoded. O arquivo atual (commit em `working tree`) **não** tem essa string — linha 22 é `<div className="mt-auto" />`. A estrutura existente, porém, segue sendo refatorada conforme o spec (Navigation group + Workspace group dinâmico).

## Checklist

### Frontend Agent

1. **Dark default real em `index.html`** — reescrever o script inline:
   - `if (stored === 'light')` → light
   - `else if (stored === 'dark')` → dark (explícito)
   - `else` (sem preferência) → dark sempre (ignorar `prefers-color-scheme`)
   - Manter try/catch; fallback do catch continua adicionando `dark`
   - Adicionar comentário curto: `// Dark is the default by design; user can toggle via Settings or Cmd+K.`

2. **Tokens em `src/style.css`** (substituir valores existentes; não adicionar novas variáveis fora do conjunto):
   - `:root`: `--card: hsl(210 20% 99%)`; `--popover: hsl(210 20% 99%)`; `--border: rgba(0, 0, 0, 0.10)`; `--input: rgba(0, 0, 0, 0.10)` (acompanha border); `--sidebar-border: rgba(0, 0, 0, 0.10)`; `--sidebar: hsl(210 20% 98%)` (mantém)
   - `.dark`: `--popover: #18181B`; `--border: rgba(255, 255, 255, 0.08)`; `--sidebar-border: rgba(255, 255, 255, 0.08)`; `--sidebar: #0B0B0C`
   - Global: `--radius: 0.5rem` (era `0.625rem`)
   - Não tocar `--background` light (já é `hsl(0 0% 100%)`) nem `--card` dark (já é `#161618`).

3. **`src/components/StatusDot.tsx`**:
   - Adicionar prop opcional `size?: "sm" | "md"`; default `"md"`
   - `sm` → `w-1.5 h-1.5`; `md` → `w-2 h-2`
   - Quando `variant === "active"`: aplicar `ring-1 ring-emerald-500/30` (somado às classes existentes)
   - Manter `animate-pulse` quando `pulse && variant === "active"`

4. **`src/components/layout/Sidebar.tsx`** — refatorar estrutura:
   - Importar `useStore` (de `@/lib/store`), `useQuery` (TanStack) e o tipo de projeto descoberto (espelhar o que `CommandPalette` faz)
   - Estrutura: `Navigation` (Home + Knowledge disabled + Activity disabled) → `<Separator />` → `Workspace` (header com label + badge de count, lista de NavLink por projeto, empty state inline) → `<Separator />` no rodapé → Settings
   - Itens disabled = `<div>` (não `<NavLink>`) com `cursor-not-allowed text-sidebar-foreground/40` e ícone `lucide-react` (`BookOpen` p/ Knowledge, `Activity` p/ Activity)
   - Workspace header: `<div className="flex items-center justify-between px-3 py-1.5"><span className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground">Workspace</span><Badge variant="secondary">{count}</Badge></div>`
   - Lista: `projectsRoot` do store + `useQuery({ queryKey: ['discover', projectsRoot], queryFn: ..., enabled: !!projectsRoot })` reaproveitando o mesmo padrão de `CommandPalette` (consultar para não duplicar lógica)
   - Cada projeto = `<NavLink to={`/project/${p.id}`}` com `<StatusDot variant={...} size="md" />` + name truncado (`truncate`)
   - Empty state inline: `<div className="text-xs text-muted-foreground/70 px-3 py-2">Configure root em Settings</div>` quando `!projectsRoot`; quando `projectsRoot` existe mas lista é `[]` → `<div className="text-xs text-muted-foreground/70 px-3 py-2">Nenhum projeto encontrado.</div>`

5. **`src/components/layout/Topbar.tsx`**:
   - Importar `RefreshCw` (lucide) e `useQueryClient` (TanStack) e `useStore`
   - Antes do botão de tema, adicionar botão "Refresh":
     - `h-7 w-7 rounded-md text-muted-foreground hover:bg-muted hover:text-foreground transition-colors duration-150 inline-flex items-center justify-center`
     - Wrap em `<Tooltip>` com label "Reload projects"
     - `onClick`: `queryClient.invalidateQueries({ queryKey: ['discover', projectsRoot] })` (se houver `projectsRoot`)
     - Ícone `<RefreshCw className="h-3.5 w-3.5" />`
   - Container direito: trocar `gap-2` por `gap-1.5`

6. **`src/pages/Home.tsx`**:
   - Importar `BarChart3`, `Brain`, `FolderGit2` de lucide
   - Empty state dos cards (sem `selectedProject`): substituir `CardDescription` "Selecione um projeto" pelo bloco visual `<div className="flex flex-col items-center justify-center gap-2 py-4 opacity-60">` com o ícone do card (`BarChart3` p/ Métricas, `Brain` p/ Knowledge) + `<span className="text-xs">Selecione um projeto</span>`
   - Lista "Projetos": antes do `StatusDot`, adicionar `<FolderGit2 className="h-3.5 w-3.5 text-muted-foreground" />`; manter o `StatusDot` em `size="md"`

7. **`src/pages/ProjectDetail.tsx`**:
   - Importar `Layers`, `ChefHat`, `BookOpen`, `Activity` de lucide
   - `SectionHeading`: aceitar prop `icon?: LucideIcon` opcional; quando passada, renderizar `<Icon className="h-4 w-4 text-foreground" />` à esquerda do título (gap-2). Título passa de `text-muted-foreground` para `text-foreground` quando ícone presente
   - Usar: Subprojects→`Layers`; Recipes→`ChefHat`; Skills→`BookOpen`; Eventos→`Activity`
   - Empty states de seção: quando array vazio, renderizar bloco `<div className="flex flex-col items-center gap-2 py-3 opacity-40"><Icon className="h-5 w-5" /><span className="text-xs">{texto}</span></div>` em lugar do `<p>` atual

8. **Validações**:
   - `pnpm build` (Vite + tsc -b) sem erros
   - `pnpm tsc --noEmit` clean
   - App abre em dark mode em sistema light após limpar localStorage

## Files (~7)

- `index.html`
- `src/style.css`
- `src/components/StatusDot.tsx`
- `src/components/layout/Sidebar.tsx`
- `src/components/layout/Topbar.tsx`
- `src/pages/Home.tsx`
- `src/pages/ProjectDetail.tsx`

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Build TS + Vite passa sem erros — Command: `pnpm build`
- [x] AC-2: Dark é default real (script de `index.html` ignora `prefers-color-scheme`) — Command: `node -e "const fs=require('fs');const h=fs.readFileSync('index.html','utf8');if(h.includes('prefers-color-scheme')){console.error('prefers-color-scheme ainda presente');process.exit(1)}if(!h.includes('Dark is the default by design')){console.error('comentário dark default ausente');process.exit(1)}console.log('ok')"`
- [x] AC-3: Token `--radius` é `0.5rem` e `--card` light é `hsl(210 20% 99%)` — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/style.css','utf8');const okR=c.includes('--radius: 0.5rem');const okC=c.includes('--card: hsl(210 20% 99%)');if(!okR||!okC){console.error('tokens faltando',{okR,okC});process.exit(1)}console.log('ok')"`
- [x] AC-4: StatusDot default agora é `md` (w-2 h-2) — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/components/StatusDot.tsx','utf8');if(!c.includes('w-2 h-2')||!c.includes('size?')){console.error('StatusDot não atualizou');process.exit(1)}console.log('ok')"`
- [x] AC-5: Sidebar usa store + query de discover e tem `Badge` no header Workspace — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/components/layout/Sidebar.tsx','utf8');const checks=[c.includes('useStore'),c.includes('discover'),c.includes('Badge')];if(checks.some(v=>!v)){console.error('Sidebar incompleta',checks);process.exit(1)}console.log('ok')"`
- [x] AC-6: Topbar tem botão Refresh com `invalidateQueries` e ícone `RefreshCw` — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/components/layout/Topbar.tsx','utf8');const checks=[c.includes('RefreshCw'),c.includes('invalidateQueries')];if(checks.some(v=>!v)){console.error('Topbar incompleta',checks);process.exit(1)}console.log('ok')"`
- [x] AC-7: ProjectDetail importa `Layers`, `ChefHat`, `BookOpen`, `Activity` — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/pages/ProjectDetail.tsx','utf8');const names=['Layers','ChefHat','BookOpen','Activity'];const miss=names.filter(n=>!c.includes(n));if(miss.length){console.error('ícones faltando',miss);process.exit(1)}console.log('ok')"`

## Non-Goals

- SQLite reader (próxima wave)
- Novas rotas (Knowledge/Activity ficam disabled, sem `to=`)
- Animações ou transições novas
- Mexer em qualquer arquivo `src-tauri/*` ou `.claude/*`
