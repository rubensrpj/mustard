# Wave 4 — Sessions index + área no dashboard

## PRD

### Contexto

A timeline per-spec (W3) cobre eventos enquanto uma pipeline está rodando. Mas há eventos que acontecem **fora** de uma pipeline ativa: `SessionStart`, `/clear`, `/mustard:status`, comandos avulsos. Hoje eles caem na tabela `events` global; depois das Waves 1-3 vão para NDJSON em `.claude/.session/{slug}/events/` (caminho já preparado em W1). Esta wave fecha o ciclo: (a) cria a tabela `sessions` em `mustard.db` (`id`, `slug`, `started_at`, `ended_at`, `spec_id` opcional, `tool_count`, `tokens_total`) atualizada por `session_start.rs` + `session_end.rs`; (b) entrega a rota `apps/dashboard/src/pages/Sessions.tsx` que lista todas as sessões (com/sem spec) num sidebar no padrão das imagens de claude-devtools que o usuário compartilhou — TODAY / YESTERDAY / PREVIOUS 7 DAYS; (c) clicar numa sessão abre o mesmo `SpecEventRenderers` do W3, mas lendo da pasta da sessão.

### Acceptance Criteria

- [ ] AC-W4-1: Tabela `sessions(id, slug, started_at, ended_at, spec_id, tool_count, tokens_total)` existe — Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \".schema sessions\"',{encoding:'utf8'});const need=['slug','started_at','ended_at','spec_id','tool_count','tokens_total'];process.exit(out.includes('CREATE TABLE')&&need.every(c=>out.includes(c))?0:1)"`
- [ ] AC-W4-2: Arquivo `apps/dashboard/src/pages/Sessions.tsx` existe — Command: `node -e "process.exit(require('fs').existsSync('apps/dashboard/src/pages/Sessions.tsx')?0:1)"`
- [ ] AC-W4-3: Rota `/sessions` está em `App.tsx` E entry no `Sidebar.tsx` E label no `Topbar.tsx` — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/App.tsx','apps/dashboard/src/components/layout/Sidebar/index.tsx','apps/dashboard/src/components/layout/Topbar/index.tsx'];const ok=files.every(f=>fs.readFileSync(f,'utf8').toLowerCase().includes('session'));process.exit(ok?0:1)"`
- [ ] AC-W4-4: Tauri commands `list_sessions` e `events_for_session` registrados — Command: `node -e "const fs=require('fs');const dir='apps/dashboard/src-tauri/src';const all=fs.readdirSync(dir).map(f=>fs.readFileSync(dir+'/'+f,'utf8')).join('');process.exit(all.includes('list_sessions')&&all.includes('events_for_session')?0:1)"`
- [ ] AC-W4-5: `cargo build -p mustard-rt && pnpm --filter mustard-dashboard build` passam — Command: `cargo build -p mustard-rt && pnpm --filter mustard-dashboard build`
- [ ] AC-W4-6: `cargo test -p mustard-rt --test sessions_table` passa (INSERT no SessionStart; UPDATE no SessionEnd) — Command: `cargo test -p mustard-rt --test sessions_table`

## Plano

### Arquivos

- `packages/core/src/store/sqlite_schema.sql` (edição) — `CREATE TABLE sessions ...` + `idx_sessions_started_at`
- `apps/rt/src/hooks/session_start.rs` (edição) — gerar slug (timestamp + 6 chars random ou primeira palavra do prompt se disponível); INSERT sessions; criar `.claude/.session/{slug}/events/`
- `apps/rt/src/hooks/session_end.rs` (edição) — UPDATE ended_at + tool_count + tokens_total (agregando do ndjson da sessão)
- `apps/rt/src/run/event_projections.rs` (edição) — view `sessions-list` paginada
- `apps/dashboard/src-tauri/src/sessions.rs` (novo) — Tauri commands `list_sessions`, `events_for_session`
- `apps/dashboard/src-tauri/src/lib.rs` (edição) — registrar commands
- `apps/dashboard/src/pages/Sessions.tsx` (novo) — página principal
- `apps/dashboard/src/features/sessions/SessionList/index.tsx` (novo) — sidebar
- `apps/dashboard/src/features/sessions/SessionDetail/index.tsx` (novo) — área principal (reutiliza `SpecEventRow` + renderers do W3)
- `apps/dashboard/src/features/sessions/_shared/use-sessions.ts` (novo) — TanStack Query hook
- `apps/dashboard/src/features/sessions/_shared/group-by-day.ts` (novo) — agrupa "TODAY / YESTERDAY / PREVIOUS 7 DAYS"
- `apps/dashboard/src/features/sessions/index.ts` (barrel)
- `apps/dashboard/src/App.tsx` (edição) — `<Route path="/sessions" element={<Sessions/>}/>`
- `apps/dashboard/src/components/layout/Sidebar/index.tsx` (edição) — entry "Sessões"
- `apps/dashboard/src/components/layout/Topbar/index.tsx` (edição) — LABELS map
- `apps/dashboard/src/lib/dashboard.ts` (edição) — wrappers `listSessions`, `eventsForSession`
- `apps/rt/tests/sessions_table.rs` (novo)

### Tarefas

#### General Agent (Wave 4)

- [ ] Adicionar `CREATE TABLE sessions ...` ao schema + idx
- [ ] Atualizar `session_start.rs`: gerar slug, INSERT sessions, criar pasta `.claude/.session/{slug}/events/`
- [ ] Atualizar `session_end.rs`: UPDATE ended_at + agregados (varre ndjson, conta tools, soma tokens)
- [ ] Criar Tauri commands `list_sessions(limit, offset)` e `events_for_session(slug)`
- [ ] Implementar `Sessions.tsx` (layout 2-col: sidebar + detalhe)
- [ ] Implementar `SessionList` agrupado por "TODAY/YESTERDAY/PREVIOUS 7 DAYS" via `group-by-day.ts`
- [ ] Implementar `SessionDetail` reusando `SpecEventRow` + renderers do W3
- [ ] Wire `App.tsx` + `Sidebar.tsx` + `Topbar.tsx`
- [ ] Adicionar wrappers em `lib/dashboard.ts`
- [ ] Testes: `cargo test -p mustard-rt --test sessions_table && pnpm --filter mustard-dashboard build && pnpm --filter mustard-dashboard lint`

### Dependências

Wave 2 (precisa do reader core estável; Wave 4 reusa pra eventos de sessão).

### Limites

- **Tocar:** `packages/core/src/store/sqlite_schema.sql`, `apps/rt/src/hooks/{session_start,session_end}.rs`, `apps/rt/src/run/event_projections.rs`, `apps/dashboard/src-tauri/src/{sessions,lib}.rs`, `apps/dashboard/src/{pages/Sessions.tsx,features/sessions/**,App.tsx,components/layout/{Sidebar,Topbar}/index.tsx,lib/dashboard.ts}`, `apps/rt/tests/sessions_table.rs`.
- **NÃO tocar:** `apps/dashboard/src/features/specs/**` (W3 já fechou), schema parts não relacionadas a `sessions`, `apps/cli/**`.
