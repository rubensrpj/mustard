# Feature: dashboard-home-real

### Status: closed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-12T00:00:00Z
### Lang: pt

## Contexto

A Mustard Dashboard é a interface desktop que deveria materializar, em segundos, o estado do pipeline-orchestrator rodando localmente — quantos specs estão ativos e em que fase estão, quanto o harness vem registrando, e quantos padrões o `/scan` capturou em `knowledge.json`. Esse é o ponto fundador de utilidade do produto: sem ele o app é só uma casca shadcn. O scaffold atual (`src/pages/Home.tsx`) entrega apenas três cards estáticos com texto literal sobre "Pipelines/Métricas/Knowledge"; mesmo rodando dentro do próprio repositório do Mustard, a tela home não cruza para o filesystem nem expõe um único número real. O impacto observável é que o usuário precisa abrir terminal e rodar `harness-views.js` (ou `cat` em arquivos) para descobrir se um pipeline está vivo, se o `events.jsonl` cresceu hoje, ou se já existe conhecimento capturado — frustrando a promessa de "dashboard pronto" e empurrando qualquer demo de volta para o terminal.

## Summary

Substituir o array literal mockado de `src/pages/Home.tsx` por dados reais lidos via três `#[tauri::command]` em Rust, expostos ao front por um wrapper TS tipado e um hook React único. Introduz o primeiro pattern "comando Rust → wrapper invoke tipado → hook" no projeto, consumindo `.claude/.pipeline-states/*.json`, `.claude/.harness/events.jsonl` e `.claude/knowledge.json` relativos ao `current_dir()` do processo Tauri.

## Entity Info

N/A — `entity-registry.json` está vazio. Não há CRUD nem nova entidade persistida; os tipos introduzidos (`PipelineSummary`, `MetricsSummary`, `KnowledgeSummary`) são DTOs de leitura, locais ao dashboard, não candidatos a registry.

## Boundaries

Caminhos intencionalmente tocados:
- `src-tauri/src/lib.rs`
- `src-tauri/Cargo.toml`
- `src/lib/dashboard.ts` (novo)
- `src/hooks/useDashboard.ts` (novo)
- `src/pages/Home.tsx`

Fora de escopo (qualquer edit aqui surface `[BOUNDARY WARNING]`):
- `src/components/layout/*` (AppShell/Sidebar/Topbar inalterados)
- `src/components/ui/*` (Card/Button não tocados)
- `src-tauri/capabilities/default.json` (commands customizados via `invoke_handler` não exigem entrada de capability em Tauri 2; só verificar, não editar — se EXECUTE descobrir que precisa, escalar como `CONCERN`)
- `src-tauri/src-tauri/` (duplicação herdada do CTA — fica para limpeza separada)

## Files (~5)

| Path | Operação |
|------|----------|
| `src-tauri/src/lib.rs` | modify (adicionar 3 commands, registrar no `invoke_handler!`, remover `greet`) |
| `src-tauri/Cargo.toml` | modify (garantir `serde`/`serde_json` se ausentes) |
| `src/lib/dashboard.ts` | create (tipos + 3 fetchers via `invoke`) |
| `src/hooks/useDashboard.ts` | create (hook único agregando os 3 fetchers) |
| `src/pages/Home.tsx` | modify (consumir o hook; render loading/error/data) |

## Component Contract

`Home` continua sendo um componente sem props (consumido em `App.tsx`).

| Prop | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| — | — | — | — | Sem props públicas |

Visual states (todos sob o mesmo grid `md:grid-cols-3`):
- **loading** — cada um dos 3 cards mostra título + `<CardDescription>Carregando…</CardDescription>`.
- **error** — header "Não foi possível ler o estado local" + a mensagem retornada pelo `invoke` no `<CardDescription>` com classe `text-destructive`.
- **data** — Pipelines: `{N} pipeline(s) ativos` + linha auxiliar com breakdown por phase no formato `ANALYZE: x • PLAN: y • EXECUTE: z • QA: w • CLOSE: v` (omite zeros). Métricas: `{total} eventos • {sessions} sessions • {agents} agents`. Knowledge: `{patterns} patterns • {high} alta-confiança`.

Sem variantes/tamanhos extras; usa o mesmo `Card` da pasta `ui/`. Acessibilidade: mantém `<CardTitle>` como heading e `<CardDescription>` para o número/legenda; sem `aria-live` (fora de escopo).

## Tasks

### Backend Agent (Wave 1)

- [x] Verificar/garantir em `src-tauri/Cargo.toml` que `serde = { version = "1", features = ["derive"] }` e `serde_json = "1"` estão presentes; se já vierem transitivamente, não adicionar (karpathy §3: surgical).
- [x] Em `src-tauri/src/lib.rs`, definir `#[derive(serde::Serialize)] struct PipelineSummary { spec_name: String, phase: String, scope: String, status: String, updated_at: Option<String> }`, idem `MetricsSummary { total_events: usize, sessions_recent: usize, agents_dispatched: usize, last_event_at: Option<String> }` e `KnowledgeSummary { patterns_count: usize, conventions_count: usize, high_confidence_count: usize }`.
- [x] Implementar `#[tauri::command] fn dashboard_pipelines() -> Result<Vec<PipelineSummary>, String>`: enumera `current_dir()/.claude/.pipeline-states/*.json`, faz `serde_json::from_str`, extrai os campos pedidos, ignora arquivos malformados (skip + log via `tauri_plugin_log`). Diretório ausente → `Ok(vec![])`.
- [x] Implementar `#[tauri::command] fn dashboard_metrics() -> Result<MetricsSummary, String>`: lê `.claude/.harness/events.jsonl`; para evitar custo em arquivos grandes, lê as últimas ~2000 linhas (read + take from end). Conta total de linhas no arquivo, distintos `sessionId` recentes, e `agent.start` ocorrências. Arquivo ausente → zeros (não erro).
- [x] Implementar `#[tauri::command] fn dashboard_knowledge() -> Result<KnowledgeSummary, String>`: lê `.claude/knowledge.json`; agrega entradas por tipo (`pattern`/`convention`) e conta `confidence >= 0.8` em `high_confidence_count`. Arquivo ausente → zeros.
- [x] Substituir `invoke_handler![greet]` por `invoke_handler![dashboard_pipelines, dashboard_metrics, dashboard_knowledge]` e remover por completo `fn greet`.
- [x] Validar: `cargo check --manifest-path src-tauri/Cargo.toml`. Retornar erros se houver.

### Frontend Agent (Wave 2)

- [x] Criar `src/lib/dashboard.ts`: tipa `PipelineSummary`/`MetricsSummary`/`KnowledgeSummary` espelhando o backend (snake_case → camelCase no consumidor TS é opcional; manter snake_case alinhado ao Rust simplifica). Exporta `fetchPipelines`, `fetchMetrics`, `fetchKnowledge` chamando `invoke<T>("dashboard_…")` de `@tauri-apps/api/core`.
- [x] Criar `src/hooks/useDashboard.ts`: hook React que no `useEffect` inicial dispara `Promise.all([fetchPipelines, fetchMetrics, fetchKnowledge])`, expõe `{ pipelines, metrics, knowledge, loading, error }`. Sem polling/refetch (fora de escopo). Em erro, captura a `string` retornada pelo `invoke` em `error: string | null`.
- [x] Refatorar `src/pages/Home.tsx`: remover array literal mockado e o card "scaffold ready". Consumir `useDashboard()`. Renderizar os 3 cards com os três visual states do Component Contract acima. Manter o grid `md:grid-cols-3 gap-4`.
- [x] Validar: `pnpm tsc --noEmit && pnpm lint`.

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript compila com hook e tipos novos — Command: `pnpm tsc --noEmit`
- [x] AC-2: Rust compila com os 3 commands registrados — Command: `node -e "const{execSync}=require('child_process');const path=require('path');const fs=require('fs');const candidates=['cargo',path.join(process.env.USERPROFILE||process.env.HOME||'','.cargo','bin','cargo.exe'),path.join(process.env.USERPROFILE||process.env.HOME||'','.cargo','bin','cargo')];const bin=candidates.find(c=>{try{execSync((c==='cargo'?c:JSON.stringify(c))+' --version',{stdio:'pipe'});return true}catch{return false}});if(!bin){console.error('cargo not found');process.exit(1)}execSync((bin==='cargo'?bin:JSON.stringify(bin))+' check --manifest-path src-tauri/Cargo.toml',{stdio:'inherit'})"`
- [x] AC-3: `lib.rs` expõe exatamente os 3 commands e removeu `greet` — Command: `node -e "const s=require('fs').readFileSync('src-tauri/src/lib.rs','utf8');const ok=['dashboard_pipelines','dashboard_metrics','dashboard_knowledge'].every(c=>s.includes('fn '+c));process.exit(ok && !s.includes('fn greet')?0:1)"`
- [x] AC-4: `Home.tsx` não tem mais o array literal mockado — Command: `node -e "const s=require('fs').readFileSync('src/pages/Home.tsx','utf8');process.exit(s.includes('Orquestra')||s.includes('Token savings, taxa')?1:0)"`
- [x] AC-5: `src/lib/dashboard.ts` expõe os 3 fetchers — Command: `node -e "const s=require('fs').readFileSync('src/lib/dashboard.ts','utf8');const ok=['fetchPipelines','fetchMetrics','fetchKnowledge'].every(f=>s.includes(f));process.exit(ok?0:1)"`
- [x] AC-6: Arquivos novos não suprimem regras de lint nem usam `any` — Command: `node -e "const fs=require('fs');const files=['src/lib/dashboard.ts','src/hooks/useDashboard.ts','src/pages/Home.tsx'];const bad=files.find(f=>{const s=fs.readFileSync(f,'utf8');return /eslint-disable|@ts-ignore|: any\b|<any>/.test(s);});process.exit(bad?1:0)"`

## Concerns

- Capabilities em Tauri 2: commands declarados via `invoke_handler` são alcançáveis pelo front sem entry em `capabilities/default.json`, mas o EXECUTE deve confirmar isso em runtime (chamada de smoke test). Se uma permission for necessária, escalar como `CONCERN` antes de editar `default.json` (está fora de Boundaries).
- Resolução do path: `current_dir()` reflete o cwd do processo Tauri — em `tauri dev` é a raiz do repo, o que basta para self-dogfooding. Build standalone que rode fora do repo é fora de escopo (vai virar spec de "project picker" depois).
- Performance do `events.jsonl`: o arquivo pode crescer indefinidamente. A heurística "últimas 2000 linhas" mantém leitura O(tail) sem manter ponteiros; aceitamos `total_events` aproximado pelo número de linhas reais (não amostra). Se virar gargalo, vira otimização separada.
- [VALIDATOR] WARN missing-file `src/lib/dashboard.ts` — marcado como `create` na coluna Operação da tabela de Files (falso positivo do parser, ignorável).
- [VALIDATOR] WARN missing-file `src/hooks/useDashboard.ts` — idem acima.
- [SCAFFOLD GAP] ESLint não está instalado nem configurado: `node_modules/.bin/eslint` ausente e nenhum `.eslintrc*` / `eslint.config.*` na raiz. O `"lint": "eslint ."` em `package.json` é stub herdado do CTA. AC-6 original (`pnpm lint`) foi substituída por verificação de qualidade local (sem `// eslint-disable`, sem `any`, sem `@ts-ignore`) até spec separada configurar ESLint.

## Non-Goals

- Project picker, persistência de path raiz com `plugin-store`
- Auto-refresh, polling ou file watching
- Nova rota/tela ou alterações em AppShell/Sidebar/Topbar
- Testes unitários (scaffold ainda não tem infra; `pnpm test` é stub)
- Limpeza do diretório duplicado `src-tauri/src-tauri/`
- Estilização aprofundada / variantes de Card / temas

## Dependencies

Nenhuma dep npm nova. No Rust, garantir `serde` (com `derive`) e `serde_json` no `[dependencies]` do `Cargo.toml` se ainda não estiverem expostos diretamente.
