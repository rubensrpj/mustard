# Bugfix: Workspace Filtering + Switcher Placement + Watcher Coverage

### Status: completed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-05-13T01:00:00Z
### Lang: pt

## Contexto

A spec anterior (`2026-05-13-notion-style-workspace-redesign`) moveu o WorkspaceSwitcher para o topbar e supostamente filtrou todas as views pelo workspace ativo, mas o usuário reportou cinco regressões testando em produção: o switcher deveria estar no sidebar (a estética Notion canônica tem o picker no topo do sidebar, não no topbar do conteúdo), Activity continua mostrando eventos agregados de todos os workspaces (em `Activity.tsx:54` o feed recebe `projects ?? []` em vez do projeto ativo), a Telemetria mostra valores idênticos para todo workspace porque a seção RTK lê dados globais de `~/.rtk/` e o UI não comunica isso, a timeline trava e um `/scan` recém-executado não aparece porque o watcher só classifica mudanças em `events.jsonl`/`pipeline-states/`/`spec/active/` (ignora `mustard.db`, `knowledge.json`, `decisions.json`, `lessons.json`), e o Knowledge mostra `type` cru (`entity-cluster`, `naming-pattern`) com descrições técnicas sem contexto explicativo. O impacto: o usuário não confia que cada tela reflete o workspace selecionado e não consegue interpretar o que está vendo.

## Summary

Mover o WorkspaceSwitcher para o topo do sidebar (substitui o header "Mustard v0.1.0"), corrigir o filtro do feed em Activity, rotular RTK como global, estender o classifier do watcher para captar mudanças em `mustard.db` + projeções de memória, e adicionar header explicativo + labels PT em Knowledge.

## Boundaries

- `src/components/layout/Sidebar.tsx`
- `src/components/layout/Topbar.tsx`
- `src/components/layout/WorkspaceSwitcher.tsx`
- `src/pages/Activity.tsx`
- `src/pages/Telemetry.tsx`
- `src/pages/Knowledge.tsx`
- `src-tauri/src/watcher.rs`
- `src/lib/watcher.ts`

Out-of-boundary: tudo mais.

## Checklist

### Frontend Agent (Wave 1)

- [x] `Sidebar.tsx`: substituir o header `<div>Mustard ...</div>` (linhas 28-32) pelo `<WorkspaceSwitcher />` posicionado no topo, com `width: 100%` e `border-b border-sidebar-border` por baixo. Importar `useStore` + `useQuery(discoverProjects)` no Sidebar para alimentar o switcher; manter os 3 grupos atuais (Workspace/Tools/Settings).
- [x] `Topbar.tsx`: remover `<WorkspaceSwitcher />` do topbar. Restaurar uma versão simples: breadcrumb `Mustard / {pageLabel}` à esquerda + reload/theme à direita. Sem nome do workspace duplicado no topbar (o sidebar já mostra).
- [x] `WorkspaceSwitcher.tsx`: ajustar visual para sidebar: trigger com `w-full` (não fixo 220px), padding compatível com sidebar (`px-3 py-2`), borda inferior sutil, sem chevron flutuante se ficar desalinhado. Manter dropdown com Command + busca.
- [x] `Activity.tsx`: linha 54 — trocar `useActivityFeed(projects ?? [], LIMIT_PER_PROJECT)` por `useActivityFeed(activeProject ? [activeProject] : [], LIMIT_PER_PROJECT)`. Garantir que `row.projectName` no Raw tab seja sempre o workspace ativo (não exibir coluna projectName se redundante).
- [x] `Telemetry.tsx`: ajustar título da seção RTK para "RTK Token Savings — Global" + nota `text-xs text-muted-foreground` abaixo: "RTK lê dados globais de `~/.rtk/`; mesmo valor para todo workspace.". Não filtrar (não há como — é global por design).
- [x] `Knowledge.tsx`: adicionar bloco intro no topo (entre header e search), explicando o que é Knowledge — texto curto: "Padrões, convenções e lições extraídos automaticamente das pipelines deste workspace. A confiança (%) reflete quantas vezes o padrão foi observado." Criar mapa PT para `type` mais comuns (`entity-cluster` → "Cluster de entidade", `naming-pattern` → "Padrão de nomenclatura", `decision` → "Decisão", `lesson` → "Lição", `recipe` → "Receita") aplicado no badge — fallback para o type cru se não mapeado.

### Rust Agent (Wave 1, parallel-safe)

- [x] `watcher.rs`: estender `classify_kind` (linhas 31-42) para também detectar:
  - `.harness/mustard.db` → `"events"` (mesmo kind, dispara invalidate de recent-events/metrics/telemetry/activity)
  - `knowledge.json` → novo kind `"knowledge"`
  - `memory/decisions.json` OR `memory/lessons.json` → novo kind `"memory"`
- [ ] Cargo build limpo: `cargo build --manifest-path src-tauri/Cargo.toml`

### Frontend Agent (Wave 2, depende de Rust)

- [x] `src/lib/watcher.ts`: estender `subscribeFsChange` para os novos kinds:
  - `"knowledge"` → invalidar queries `["knowledge-browse", repoPath]` e `["knowledge-search", ...]`
  - `"memory"` → invalidar `["activity-feed", repoPath]` (lessons/decisions alimentam o feed)
- [ ] Build + tsc: `pnpm build` e `pnpm tsc --noEmit` clean.

## Files (~8)

- `src/components/layout/{Sidebar,Topbar,WorkspaceSwitcher}.tsx`
- `src/pages/{Activity,Telemetry,Knowledge}.tsx`
- `src-tauri/src/watcher.rs`
- `src/lib/watcher.ts`

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: tsc clean — Command: `pnpm tsc --noEmit`
- [ ] AC-2: Vite build limpo — Command: `pnpm build`
- [ ] AC-3: Cargo build limpo — Command: `node -e "const {execFileSync}=require('child_process');const p=require('path').join(process.env.USERPROFILE||process.env.HOME,'.cargo','bin','cargo.exe');execFileSync(p,['build','--manifest-path','src-tauri/Cargo.toml'],{stdio:'inherit'})"`
- [ ] AC-4: Sidebar tem WorkspaceSwitcher no topo — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src/components/layout/Sidebar.tsx','utf8');process.exit(s.includes('WorkspaceSwitcher')?0:1)"`
- [ ] AC-5: Topbar NÃO tem WorkspaceSwitcher — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src/components/layout/Topbar.tsx','utf8');process.exit(s.includes('WorkspaceSwitcher')?1:0)"`
- [ ] AC-6: Activity feed limitado ao activeProject — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src/pages/Activity.tsx','utf8');process.exit(s.includes('useActivityFeed(activeProject')?0:1)"`
- [ ] AC-7: Watcher classifica mustard.db + knowledge.json + memory/* — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src-tauri/src/watcher.rs','utf8');process.exit((s.includes('mustard.db')&&s.includes('knowledge.json')&&s.includes('memory'))?0:1)"`
