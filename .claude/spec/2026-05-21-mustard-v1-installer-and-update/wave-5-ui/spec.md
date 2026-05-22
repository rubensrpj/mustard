# Wave 5 — Frontend: Add Project adaptativo + banners + welcome + InstallLog

### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full (wave)
### Wave: 5
### Role: ui
### Checkpoint: 2026-05-21T18:00:00Z
### Lang: pt
### Parent: 2026-05-21-mustard-v1-installer-and-update

## PRD

## Contexto

O frontend hoje tem `addProject(path)` que só registra a pasta no workspace (memory `project_dashboard_project_as_workspace`); rodar `mustard_install` é ação separada. Esta wave refatora o fluxo "Adicionar projeto" pra ser adaptativo (Q5): user escolhe pasta → app chama `detect_project_mustard(path)` → se `.claude/` existe, registra silencioso com toast confirmatório; se não existe, mostra diálogo com 2 opções ("Instalar e adicionar" / "Só adicionar"); após install, mostra `<details>` expansível (`InstallLog.tsx`) com cada operação feita. Em paralelo, monta 3 banners persistentes no chrome do app — RTK/Claude Code ausentes do PATH, projetos out-of-sync, update disponível — e uma welcome screen quando o registry de projetos está vazio (primeira execução). Tudo usando componentes shadcn já presentes no projeto, dark-first (memory `feedback_design_aesthetic`).

## Métrica de sucesso

User abre o app pela primeira vez → vê WelcomeScreen com call-to-action "Adicionar primeiro projeto". Clica, seleciona pasta vazia → diálogo "Essa pasta não tem Mustard. [Instalar e adicionar] [Só adicionar (vazio)]". Clica "Instalar e adicionar" → install roda → vê `<details>` com 4 linhas (criou .claude/, RTK detectado, settings global skipped, stack detectada). App passa a mostrar o workspace. Quando há projetos com versão antiga, banner aparece no topo: "5 projetos com versão antiga. [Atualizar todos] [Caso a caso]".

## Critérios de Aceitação

- [ ] AC-W5-1: `AddProjectDialog.tsx` chama `detect_project_mustard` antes de exibir opções — Command: `node -e "const s=require('fs').readFileSync('apps/app/src/components/projects/AddProjectDialog.tsx','utf8');if(!/detect_project_mustard/.test(s)){process.exit(1)}"`
- [ ] AC-W5-2: `InstallLog.tsx` existe e usa `<details>` — Command: `node -e "const s=require('fs').readFileSync('apps/app/src/components/projects/InstallLog.tsx','utf8');if(!/details/.test(s)){process.exit(1)}"`
- [ ] AC-W5-3: `WelcomeScreen.tsx` existe — Command: `node -e "if(!require('fs').existsSync('apps/app/src/components/welcome/WelcomeScreen.tsx')){process.exit(1)}"`
- [ ] AC-W5-4: `PrereqBanner.tsx` referencia rtk e claude — Command: `node -e "const s=require('fs').readFileSync('apps/app/src/components/banners/PrereqBanner.tsx','utf8');if(!/rtk/i.test(s)||!/claude/i.test(s)){process.exit(1)}"`
- [ ] AC-W5-5: `ProjectSyncBanner.tsx` existe — Command: `node -e "if(!require('fs').existsSync('apps/app/src/components/banners/ProjectSyncBanner.tsx')){process.exit(1)}"`
- [ ] AC-W5-6: `UpdateAvailableBanner.tsx` existe — Command: `node -e "if(!require('fs').existsSync('apps/app/src/components/banners/UpdateAvailableBanner.tsx')){process.exit(1)}"`
- [ ] AC-W5-7: App.tsx monta os 3 banners + WelcomeScreen condicional — Command: `node -e "const s=require('fs').readFileSync('apps/app/src/App.tsx','utf8');const need=['PrereqBanner','ProjectSyncBanner','UpdateAvailableBanner','WelcomeScreen'];const miss=need.filter(x=>!s.includes(x));if(miss.length){console.error(miss);process.exit(1)}"`
- [ ] AC-W5-8: Dashboard tipa e builda — Command: `pnpm --filter mustard-app build`

## Plano

## Summary

Cria 7 componentes React novos + 3 hooks novos + 1 refator no `AddProjectDialog`. Hooks usam `useQuery` (TanStack Query v5) com `staleTime` apropriado — `usePrereqStatus` poll a cada 30s + on focus, `useUpdateCheck` 1h + on demand, `useOutOfSyncProjects` on workspace change. Banners renderizam condicionalmente — null quando não há problema. Welcome só mostra quando `projects.length === 0`.

## Component Contract

| Component | Props (minimal) | Reads/Writes |
|---|---|---|
| `AddProjectDialog` | `open: boolean, onClose: () => void` | invoke(`detect_project_mustard`), invoke(`mustard_install`), invoke(`add_project_to_registry`) |
| `InstallLog` | `operations: InstallOperation[]` | none (pure) |
| `WelcomeScreen` | none | reads zustand store (projects), opens AddProjectDialog |
| `PrereqBanner` | none | useQuery(prereq_status) |
| `ProjectSyncBanner` | none | useQuery(list_out_of_sync_projects), useMutation(update_project) |
| `UpdateAvailableBanner` | none | useQuery(check_for_updates), invoke(open_url) |

`InstallOperation` type:
```ts
type InstallOperation =
  | { kind: "created"; path: string }                          // ✓ Created .claude/ em /path
  | { kind: "rtk_detected"; version: string }                  // ✓ RTK v1.2.3 detected
  | { kind: "settings_global_skipped" }                        // ✓ Skipped global ~/.claude/settings.json (opt-in)
  | { kind: "stack_detected"; stacks: string[] }               // ✓ Detected: TypeScript + React
  | { kind: "warning"; message: string };                      // ⚠ <message>
```

## Checklist

### UI Agent

- [ ] Criar `apps/app/src/components/projects/AddProjectDialog.tsx`:
  - Estado: `step: "picker" | "decision" | "installing" | "done"`, `detectedVersion: string | null`
  - Flow:
    - `picker` → user escolhe pasta via `@tauri-apps/plugin-dialog::open`
    - chama `invoke('detect_project_mustard', { path })`
    - Se `installed === true` → registra silencioso, toast "Mustard v{version} detected", fecha
    - Se `installed === false` → step `decision`
    - `decision` → 2 botões: "Instalar e adicionar" (default), "Só adicionar (vazio)"
    - Click Install → step `installing` → `invoke('mustard_install', { path })`, capturar `Result<InstallOperation[], string>` (ajustar `mustard_install` retorno na Wave 4 ou nesta wave a coordenar)
    - step `done` → mostra `<InstallLog operations={...} />`
- [ ] Criar `apps/app/src/components/projects/InstallLog.tsx`:
  - `<details>` aberto por default (expanded={true} no primeiro render)
  - Render por `op.kind`: ícone (✓ ou ⚠) + texto formatado
  - Sem CSS pesado — Tailwind utility, monospace pros paths
- [ ] Criar `apps/app/src/components/welcome/WelcomeScreen.tsx`:
  - Logo Mustard centralizado (text-only por enquanto: "Mustard")
  - Heading "Bem-vindo ao Mustard v{version}"
  - 1 linha contexto: "Comandos disponíveis no terminal: mustard, mustard-rt"
  - CTA primary "[Adicionar primeiro projeto]" → abre AddProjectDialog
  - CTA secondary "[Documentação]" → open https://github.com/atiz-tech/mustard
- [ ] Criar `apps/app/src/components/banners/PrereqBanner.tsx`:
  - `usePrereqStatus()` → render quando `rtk.kind === "Missing"` OR `claude_code.kind === "Missing"`
  - Texto: "⚠ {binary} não encontrado no PATH — hooks do Claude Code não funcionarão. [Como instalar]"
  - Botão "[Como instalar]" → abre dialog com instruções por SO (deferred a outro componente se necessário, ou inline)
  - Botão dismiss "x" — persistir em zustand store por 24h
- [ ] Criar `apps/app/src/components/banners/ProjectSyncBanner.tsx`:
  - `useOutOfSyncProjects()` → render quando `length > 0`
  - Texto: "{N} projetos com versão antiga"
  - Botões: "[Atualizar todos]" (mutação batch), "[Caso a caso]" (abre painel), "[Lembrar depois]" (persistir 24h)
- [ ] Criar `apps/app/src/components/banners/UpdateAvailableBanner.tsx`:
  - `useUpdateCheck()` → render quando `needs_update === true`
  - Texto: "Nova versão {latest} disponível"
  - Botões: "[Ver release]" → `invoke('plugin:opener|open_url', { url })`, "[Mais tarde]" (dismiss 24h)
- [ ] Criar `apps/app/src/hooks/usePrereqStatus.ts`:
  - `useQuery({ queryKey: ['prereq_status'], queryFn: () => invoke('prereq_status'), staleTime: 30_000, refetchOnWindowFocus: true })`
- [ ] Criar `apps/app/src/hooks/useUpdateCheck.ts`:
  - `useQuery({ queryKey: ['update_check'], queryFn: () => invoke('check_for_updates', { force: false }), staleTime: 3600_000 })`
  - Expor `forceCheck()` mutation pra botão manual em Preferences
- [ ] Criar `apps/app/src/hooks/useOutOfSyncProjects.ts`:
  - `useQuery({ queryKey: ['out_of_sync_projects'], queryFn: () => invoke('list_out_of_sync_projects'), staleTime: 60_000 })`
  - `useMutation({ mutationFn: (path) => invoke('update_project', { path }), onSuccess: invalidate })`
- [ ] Atualizar `apps/app/src/lib/dashboard.ts` com wrappers tipados pros 5 novos commands
- [ ] Atualizar `apps/app/src/App.tsx`:
  - Mount banners no topo (acima do conteúdo): `<PrereqBanner /> <UpdateAvailableBanner /> <ProjectSyncBanner />`
  - Renderiza `<WelcomeScreen />` quando `projects.length === 0` (do zustand store)
  - Senão renderiza route content normal
- [ ] Atualizar `apps/app/src/pages/Preferences.tsx`:
  - Adicionar seção "Updates"
  - Botão "Check for updates" → `forceCheck()` do hook
  - Mostrar última versão checada + timestamp
- [ ] Build/type-check: `pnpm --filter mustard-app build`

## Files (~13)

```
apps/app/src/components/projects/AddProjectDialog.tsx       — REFATOR (existe? confirmar; se não, criar)
apps/app/src/components/projects/InstallLog.tsx             — NOVO
apps/app/src/components/welcome/WelcomeScreen.tsx           — NOVO
apps/app/src/components/banners/PrereqBanner.tsx            — NOVO
apps/app/src/components/banners/ProjectSyncBanner.tsx      — NOVO
apps/app/src/components/banners/UpdateAvailableBanner.tsx  — NOVO
apps/app/src/hooks/usePrereqStatus.ts                       — NOVO
apps/app/src/hooks/useUpdateCheck.ts                        — NOVO
apps/app/src/hooks/useOutOfSyncProjects.ts                  — NOVO
apps/app/src/lib/dashboard.ts                               — wrappers tipados (~5 funções)
apps/app/src/App.tsx                                        — mount banners + welcome
apps/app/src/pages/Preferences.tsx                          — botão Check for updates
```
