# Wave 3 — Dashboard claude-devtools-style

## PRD

### Contexto

Com o backend (W1+W2) entregando eventos ricos via reader NDJSON estável, o `SpecTimelineTab` atual — que agrupa por fase e mostra só `kind:label` — fica subutilizado. Esta wave reescreve esse componente e o `PipelineTimeline` no padrão **claude-devtools**: lista flat de tool calls por wave (fase como sub-grouping opcional), cada linha mostrando ícone do tool, label curto (derivado do input), tokens in/out, duração e status dot. Expand vertical revela blocos `Input` (parâmetros) e `Output` (renderizado por tool). Cada tool ganha um renderer dedicado: `BashRenderer` em terminal style (comando + stdout + stderr), `ReadRenderer` alternando Code/Preview (syntax highlight + markdown preview), `EditRenderer` mostrando diff old/new lado-a-lado, `WriteRenderer` mostrando código novo, `GlobRenderer`/`GrepRenderer` listando matches, `TaskRenderer` renderizando recursivamente os eventos do subagent (filtra por `parent_id` e instancia novo `<SpecTimelineTab>` aninhado), `DefaultRenderer` como fallback JSON pretty. Live tail via `notify-rs` no `src-tauri` watcha a pasta `events/` da spec ativa, debounce 50ms, emite Tauri event `events:appended` que o React consome para invalidar TanStack Query e atualizar em tempo real.

### Acceptance Criteria

- [ ] AC-W3-1: `pnpm --filter mustard-dashboard build` passa (tsc + vite) — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W3-2: `pnpm --filter mustard-dashboard lint` passa — Command: `pnpm --filter mustard-dashboard lint`
- [ ] AC-W3-3: Existem todos os 8 renderers em `src/features/specs/SpecEventRenderers/{Name}/index.tsx` — Command: `node -e "const fs=require('fs');const dir='apps/dashboard/src/features/specs/SpecEventRenderers';const need=['BashRenderer','ReadRenderer','EditRenderer','WriteRenderer','GlobRenderer','GrepRenderer','TaskRenderer','DefaultRenderer'];const ok=need.every(n=>fs.existsSync(dir+'/'+n+'/index.tsx'));process.exit(ok?0:1)"`
- [ ] AC-W3-4: Tauri commands `events_for_spec`, `events_for_wave`, `read_blob` registrados — Command: `node -e "const fs=require('fs');const dir='apps/dashboard/src-tauri/src';const all=fs.readdirSync(dir).map(f=>fs.readFileSync(dir+'/'+f,'utf8')).join('');const ok=['events_for_spec','events_for_wave','read_blob'].every(c=>all.includes(c));process.exit(ok?0:1)"`
- [ ] AC-W3-5: `notify` listado em `apps/dashboard/src-tauri/Cargo.toml` — Command: `node -e "process.exit(require('fs').readFileSync('apps/dashboard/src-tauri/Cargo.toml','utf8').includes('notify')?0:1)"`
- [ ] AC-W3-6: `TaskRenderer` referencia `SpecTimelineTab` recursivamente — Command: `node -e "const fs=require('fs');process.exit(fs.readFileSync('apps/dashboard/src/features/specs/SpecEventRenderers/TaskRenderer/index.tsx','utf8').includes('SpecTimelineTab')?0:1)"`

## Plano

### Arquivos

- `apps/dashboard/src/features/specs/SpecTimelineTab/index.tsx` (rewrite)
- `apps/dashboard/src/features/specs/SpecEventRow/index.tsx` (novo) — uma linha colapsada
- `apps/dashboard/src/features/specs/SpecEventRenderers/{Bash,Read,Edit,Write,Glob,Grep,Task,Default}Renderer/index.tsx` (todos novos)
- `apps/dashboard/src/features/specs/SpecEventRenderers/index.ts` (barrel)
- `apps/dashboard/src/features/specs/_shared/event-types.ts` (novo) — tipo `EventV2` espelhando `model::view::timeline`
- `apps/dashboard/src/features/specs/_shared/use-spec-events.ts` (novo) — TanStack Query + subscribe a `events:appended`
- `apps/dashboard/src/features/specs/_shared/event-icon.tsx` (novo) — map `tool → lucide icon`
- `apps/dashboard/src/features/telemetry/PipelineTimeline/index.tsx` (rewrite) — reaproveita `SpecEventRow` + renderers
- `apps/dashboard/src/lib/dashboard.ts` (edição) — `eventsForSpec`, `eventsForWave`, `readBlob`
- `apps/dashboard/src/lib/types/specs.ts` (edição) — estende `SpecTimelineNode`
- `apps/dashboard/src-tauri/src/events_ndjson.rs` (novo) — Tauri commands chamam `mustard_core::reader::ndjson`
- `apps/dashboard/src-tauri/src/events_watcher.rs` (novo) — `notify-rs` watcher (debounce 50ms)
- `apps/dashboard/src-tauri/src/lib.rs` (edição) — registrar handlers + state do watcher
- `apps/dashboard/src-tauri/Cargo.toml` (edição) — add `notify = "8"` (ou versão atual estável)
- `apps/dashboard/package.json` (edição) — add `react-diff-viewer-continued` (ou alternativa equivalente leve) + `shiki` (ou `react-syntax-highlighter` se já presente, validar antes)

### Tarefas

#### UI Agent (Wave 3)

- [ ] Adicionar deps no `Cargo.toml` (notify) e `package.json` (diff viewer + syntax highlight) — `pnpm install` no final
- [ ] Estender `SpecTimelineNode` em `lib/types/specs.ts`; criar `_shared/event-types.ts`
- [ ] Implementar `_shared/use-spec-events.ts` (TanStack Query key por `[spec, wave]`; subscribe via `listen('events:appended', ...)`; invalidate query)
- [ ] Implementar `_shared/event-icon.tsx` (lucide map)
- [ ] Implementar `SpecEventRow` (colapsada → ícone, label, tokens, duração, status, chevron; expandida → renderer apropriado)
- [ ] Implementar cada renderer em pasta própria + barrel
  - [ ] `BashRenderer`: command + stdout + stderr blocos
  - [ ] `ReadRenderer`: toggle Code/Preview; preview via react-markdown se .md, syntax-highlight caso contrário
  - [ ] `EditRenderer`: diff lado-a-lado com `react-diff-viewer-continued`
  - [ ] `WriteRenderer`: bloco código novo
  - [ ] `GlobRenderer`/`GrepRenderer`: lista de paths/matches
  - [ ] `TaskRenderer`: renderiza header (Type/Duration/Model/ID/Context) + `<SpecTimelineTab events={children}/>` filtrado por `parent_id`
  - [ ] `DefaultRenderer`: JSON pretty
- [ ] Rewrite `SpecTimelineTab` (lista flat agrupada por wave; cada item `<SpecEventRow>`)
- [ ] Rewrite `PipelineTimeline` (mesmo padrão cross-wave; entrypoint do dashboard "spec timeline")
- [ ] Adicionar wrappers Tauri em `lib/dashboard.ts` (`eventsForSpec`, `eventsForWave`, `readBlob`)
- [ ] `apps/dashboard/src-tauri/src/events_ndjson.rs`: implementar 3 commands chamando `mustard_core::reader::ndjson::EventReader`
- [ ] `events_watcher.rs`: `RecommendedWatcher` em `.claude/spec/{ativa}/events/`; debounce 50ms; emit `events:appended` com payload `{spec, wave}`
- [ ] `lib.rs`: registrar handlers + setup do watcher quando spec ativa muda
- [ ] `pnpm --filter mustard-dashboard build && pnpm --filter mustard-dashboard lint`
- [ ] Teste manual: rodar `/mustard:status` ou outra pipeline real e ver timeline atualizar live

### Dependências

Wave 2 (reader core estável + shape novo no `model/view/timeline.rs`).

### Limites

- **Tocar:** `apps/dashboard/src/features/specs/{SpecTimelineTab,SpecEventRow,SpecEventRenderers,_shared}/**`, `apps/dashboard/src/features/telemetry/PipelineTimeline/**`, `apps/dashboard/src/lib/{dashboard,types/specs}.ts`, `apps/dashboard/src-tauri/src/{events_ndjson,events_watcher,lib}.rs`, `apps/dashboard/src-tauri/Cargo.toml`, `apps/dashboard/package.json`.
- **NÃO tocar:** `apps/dashboard/src/pages/Sessions.tsx` (W4), `apps/dashboard/src/features/specs/{SpecCard,SpecDrillDown,SpecEventsTab,SpecMarkdownViewer}/**` (não relacionado), `apps/rt/`, `packages/core/`.
