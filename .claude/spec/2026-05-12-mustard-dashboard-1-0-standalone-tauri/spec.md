# Mustard Dashboard 1.0 — Remove Legacy Dashboard (Pre-Standalone)

- **Lang**: ptbr
- **Checkpoint**: 2026-05-12T22:22:56Z
- **Scope**: Full (this pipeline: removal only; standalone app continues in dedicated repo)
- **Type**: feature
- **Model**: opus
- **Depends on**: Mustard 2.0 Phase 1 (SQLite schema é o contrato)
- **Unlocks**: produto comercializável separado em repo dedicado `C:\Atiz\mustard-dashboard`

> **Repo decision (2026-05-12):** Dashboard standalone vai para repo **dedicado** (`C:\Atiz\mustard-dashboard`), NÃO monorepo. Esta spec no Mustard core executa apenas a **remoção** do dashboard legacy. O build Tauri propriamente acontece em sessão Claude Code separada nesse repo. Esta spec preserva as decisões de arquitetura (Tauri, SQLite direct read, Linear+Notion) como contrato para a próxima spec.

## Summary

Desktop app standalone (Tauri + Vite + React + TypeScript) que descobre e monitora múltiplos projetos Mustard no filesystem do usuário. Distribuído como **produto pago separado** do Mustard core (que continua open-source/free). Lê SQLite diretamente de cada projeto — não requer Claude Code rodando.

## Por que isso existe

**Estratégico**: Mustard (engine) é open source; Dashboard (visualização) tem valor pago.

**Técnico**: Hoje cada projeto carrega 60KB+ de dashboard.js embedded. Erros são corrigidos em cada projeto separadamente. UI fica refém de Node.js no projeto.

**Comercial**: Solo developers + teams pagam por:
- Visão multi-projeto consolidada
- Histórico além de N dias
- Métricas avançadas (cost forecasting, model comparison)
- UI moderna substituindo dashboard 60KB JS-puro

## Decisões de arquitetura

### 1) Tauri (NÃO Electron)
- Binary 10MB vs 100-150MB
- RAM 30-50MB vs 200-400MB
- Rust backend = `rusqlite` nativo, sem Node deps
- Modelo de segurança rígido (relevante pra vender)
- CDN cost 10x menor em escala
- `@tauri-apps/api` cobre fs/dialog/notification/clipboard sem escrever Rust

### 2) Direct SQLite read (NÃO MCP)
- Não exige Claude Code ativo no projeto
- Funciona offline / em projetos arquivados
- Dependência única: schema da Phase 1 do Mustard 2.0
- MCP fica como **opcional** pra live data (Phase 2 do dashboard, se justificar)

### 3) Frontend: Vite + React + TypeScript + Tailwind + shadcn/ui
- Stack padrão 2026, biggest ecosystem
- shadcn/ui = componentes copiados (sem dep runtime, customizáveis)
- Tailwind = consistência sem CSS-in-JS overhead

### 3b) Visual style: Linear + Notion
- **Dark-mode-first**: bg `#0F0F10`, surface `#161618-#1B1B1D`, borders `rgba(255,255,255,0.06)`
- **Typography**: Inter (UI 13px base), JetBrains Mono / Geist Mono (code)
- **Accent**: indigo `#6366F1` (primary action) / violet `#8B5CF6` (active state)
- **Layout primário**: sidebar Linear-style (workspace switcher + seções colapsáveis: Projects, Aggregate, Knowledge, Settings) + main panel com tabs internas (Specs, Metrics, Knowledge, Timeline)
- **Listas (specs, events, knowledge)**: estilo Linear — densas, status dot colorido (gray=backlog/active, yellow=in-progress/PLAN, green=done, red=blocked), hover sutil 1px outline
- **Conteúdo de spec (visualização)**: estilo Notion — blocos (callout, code, table, toggle), breadcrumb topo, line-height generoso, max-width legível
- **Command palette**: Cmd+K obrigatório (cmdk lib) — navegação, busca cross-project, ações rápidas
- **Animações**: 150ms ease-out em hover/expand/transition. Sem motion gratuito.
- **Light mode**: paridade visual com dark, mas dark é o default. Toggle em Settings.

### 4) Estado: TanStack Query + Zustand
- TanStack Query: cache + dedup das queries por projeto
- Zustand: estado UI leve (projetos selecionados, filtros, theme)
- Sem Redux

### 5) Distribuição
- GitHub Releases (free tier limitado)
- Gumroad ou Lemon Squeezy (license keys)
- Tauri built-in updater
- Code signing Win/Mac (~$100-300/yr — custo de negócio, não eng)

## Acceptance Criteria (this pipeline)

> This pipeline only runs **AC #9** below (removal verification). All other AC are preserved as the contract for the next spec in `mustard-dashboard` repo.

### Active (this pipeline)

**AC: Old dashboard removed from core** — see #9 below.

### Deferred to standalone-app spec (in `mustard-dashboard` repo)

1. **Build cross-platform funciona**
   ```bash
   cd packages/dashboard-app && bun run tauri:build && ls -lh src-tauri/target/release/bundle/
   ```
   Bundle existe pra Win/Mac/Linux. Binary <15MB.

2. **Onboarding: configurar root directory**
   ```bash
   cd packages/dashboard-app && bun run test:e2e -- onboarding.spec.ts
   ```
   Playwright/Tauri test: usuário clica "Add projects root", seleciona dir, scanner roda, lista projetos descobertos com `.claude/`.

3. **Discovery: scanner encontra projetos Mustard**
   ```bash
   bun run test src-tauri/tests/discovery.rs
   ```
   Rust integration test: dado fixture com 3 dirs (2 com `.claude/.harness/mustard.db`, 1 sem), retorna exatamente 2 projetos.

4. **Leitura SQLite per-project**
   ```bash
   bun test packages/dashboard-app/src/api/__tests__/project-reader.test.ts
   ```
   Mock DB com schema Phase 1: leitor retorna `{specs, metrics, knowledge, spans}` corretos.

5. **Multi-project view**
   ```bash
   bun run test packages/dashboard-app/src/views/__tests__/multi-project.test.ts
   ```
   Component test: dados de 3 projetos → sidebar lista 3 + agregação (total tokens, total specs, top knowledge entries).

6. **License gate skeleton**
   ```bash
   bun test packages/dashboard-app/src/license/__tests__/gate.test.ts
   ```
   Sem license: máx 1 projeto. Com license válida (mock): unlimited. Validação **offline** (HMAC de key) — sem chamar servidor no MVP.

7. **Performance: cold start <2s, 10 projetos load <1s**
   ```bash
   bun run bench packages/dashboard-app/bench/startup.bench.ts
   ```
   Tauri window visible em <2s. 10 projetos × 1000 events cada carregam em <1s (read-only SQLite paralelo).

8. **Auto-update funciona**
   ```bash
   bun test packages/dashboard-app/src-tauri/tests/updater.rs
   ```
   Mock release server → app detecta nova versão, baixa, instala on restart.

9. **Old dashboard removido do core**
   ```bash
   node -e "const fs=require('fs');const gone=['templates/scripts/dashboard.js','templates/scripts/dashboard-ui.js','templates/scripts/dashboard-commands-catalog.js','templates/scripts/dashboard-env-catalog.js','templates/scripts/dashboard-prd-template.js','templates/commands/mustard/dashboard/SKILL.md'];for(const f of gone){if(fs.existsSync(f)){console.log('STILL EXISTS',f);process.exit(1)}}process.exit(0)"
   ```
   Os 5 scripts + slash command do dashboard legacy não existem mais em `templates/`. README + `docs/upgrade-to-2.0.md` atualizados.

## Implementation

### Repo structure

Sugestão: **separar do Mustard core** em `packages/dashboard-app/` (monorepo) OU repo dedicado `mustard-dashboard`.

```
mustard-dashboard/  (ou packages/dashboard-app/)
├── src/                        # Vite + React
│   ├── views/
│   │   ├── ProjectList.tsx
│   │   ├── ProjectDetail.tsx
│   │   ├── AggregateView.tsx
│   │   ├── KnowledgeBrowser.tsx
│   │   └── Settings.tsx
│   ├── api/                    # Tauri command bridge
│   │   ├── project-reader.ts
│   │   ├── discovery.ts
│   │   └── license.ts
│   ├── components/             # shadcn/ui copies
│   ├── lib/
│   │   ├── query-client.ts     # TanStack Query setup
│   │   └── store.ts            # Zustand
│   └── main.tsx
├── src-tauri/                  # Rust backend
│   ├── src/
│   │   ├── main.rs
│   │   ├── discovery.rs        # scan filesystem for .claude/
│   │   ├── reader.rs           # rusqlite DB read
│   │   ├── license.rs          # HMAC license validation
│   │   └── updater.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── package.json
├── vite.config.ts
└── tsconfig.json
```

### Tauri commands (Rust ↔ TS bridge)

```rust
// src-tauri/src/main.rs
#[tauri::command]
fn discover_projects(root: String) -> Result<Vec<Project>, String> { /* walk for .claude/ */ }

#[tauri::command]
fn read_project_metrics(db_path: String) -> Result<Metrics, String> { /* rusqlite query */ }

#[tauri::command]
fn read_project_knowledge(db_path: String, query: Option<String>) -> Result<Vec<KnowledgeEntry>, String> { /* FTS5 */ }

#[tauri::command]
fn read_project_spans(db_path: String, filter: SpanFilter) -> Result<Vec<Span>, String> { /* spans table */ }

#[tauri::command]
fn validate_license(key: String) -> Result<License, String> { /* HMAC + features */ }
```

### Frontend wiring

```typescript
// src/api/project-reader.ts
import { invoke } from '@tauri-apps/api/core';
export async function readMetrics(dbPath: string): Promise<Metrics> {
  return invoke('read_project_metrics', { dbPath });
}
```

```typescript
// src/views/ProjectDetail.tsx
import { useQuery } from '@tanstack/react-query';
export function ProjectDetail({ project }: { project: Project }) {
  const { data } = useQuery({
    queryKey: ['metrics', project.dbPath],
    queryFn: () => readMetrics(project.dbPath),
    staleTime: 12_000,  // matches Mustard 12s poll cadence
  });
  return <MetricsView data={data} />;
}
```

### Discovery algorithm

```rust
// src-tauri/src/discovery.rs
fn walk(root: &Path, max_depth: usize) -> Vec<Project> {
    let mut found = vec![];
    // BFS up to max_depth (default 5)
    // For each dir: check `.claude/.harness/mustard.db` exists
    // Skip: node_modules, .git, dist, target, .next, vendor
    // Return: { name, path, db_path, last_activity_ms }
    found
}
```

### License gate (MVP — local validation)

```rust
// src-tauri/src/license.rs
// Format: base64(payload) + "." + hex(hmac_sha256(payload, MUSTARD_DASH_SECRET))
// Payload: {"plan":"pro","max_projects":-1,"expires":"2027-01-01","email":"x@y.com"}
// Secret embedded at build time (different per platform/release)
pub fn validate(key: &str) -> Result<License, LicenseError> { /* ... */ }
```

License keys sold via Gumroad webhook → emite key assinada. Validação 100% local (offline). Anti-piracy é **dissuasão**, não fortaleza.

### Pricing skeleton

```typescript
// src/lib/features.ts
export const FEATURES = {
  free: { maxProjects: 1, historyDays: 7 },
  pro:  { maxProjects: Infinity, historyDays: Infinity, costAnalytics: true },
  team: { ...pro, sharing: true, customBranding: true },
};
```

Sugestão de preço (revisar depois):
- **Free**: 1 projeto, 7 dias histórico
- **Solo lifetime**: $39 — unlimited projetos, full history
- **Team lifetime** (5 seats): $149
- Subscription tier opcional pra updates >ano 1

## Removal scope (in this spec)

O dashboard atual sai do Mustard core **nesta mesma execução** (não em spec separada):

**Source files (templates/):**
- ✗ `templates/scripts/dashboard.js`
- ✗ `templates/scripts/dashboard-ui.js`
- ✗ `templates/scripts/dashboard-commands-catalog.js`
- ✗ `templates/scripts/dashboard-env-catalog.js`
- ✗ `templates/scripts/dashboard-prd-template.js`
- ✗ `templates/commands/mustard/dashboard/` (SKILL.md + diretório)

**Synced copies (.claude/, dentro deste repo):**
- ✗ `.claude/scripts/dashboard.js`
- ✗ `.claude/scripts/dashboard-ui.js`
- ✗ `.claude/scripts/dashboard-commands-catalog.js`
- ✗ `.claude/commands/mustard/dashboard/` (SKILL.md + diretório)

**Docs:**
- `README.md` — remover linha do command table, remover seção `## Dashboard`, atualizar tree em `templates/scripts/` removendo as 5 entradas dashboard*
- `docs/upgrade-to-2.0.md` — substituir referências a `dashboard.js --check` por instruções de instalação do Mustard Dashboard standalone; ajustar troubleshooting (linha 150)

**Comentários (não bloqueiam):** `templates/hooks/metrics-tracker.js:109` e `templates/hooks/_lib/harness-event.js:114` mencionam "dashboard" em comentários — atualizar para apontar para o Mustard Dashboard standalone.

**Hooks/settings:** nenhuma referência em `templates/settings.json` (verificado). Nenhum hook importa `dashboard*.js` (verificado).

**Total que sai: ~80KB de código mantido por projeto + 1 slash command.**

## Risks endereçados por design

- **Pirataria** → license é dissuasor, não fortaleza. Aceita perda de N% pra evitar friction em pagantes legítimos.
- **Tauri Windows code-signing caro** → primeira release pode ser unsigned com aviso explícito; signing em release 1.1 quando receita justificar.
- **Schema do Mustard mudar e quebrar dashboard** → Phase 1 do Mustard 2.0 define schema versionado; dashboard checa version no startup, exibe warning se incompat.
- **Multi-platform CI complexo** → GitHub Actions matrix builds; cada platform um job paralelo. Tauri docs cobrem isso.

## Out of scope (release 1.0)

- Cloud sync entre máquinas (release 2.0 se demanda existir)
- Real-time live update via MCP (release 1.x se útil)
- Team collaboration features (Team tier — release 1.5)
- Mobile companion app (longe)
- AI insights ("seu pipeline X gasta 30% mais que projeto Y") — fase futura, requer telemetria de mais usuários

## Checklist

### Active (this pipeline — Removal in Mustard core) — COMPLETED 2026-05-12
- [x] Delete `templates/scripts/dashboard.js` + 4 sibling files (dashboard-ui, dashboard-commands-catalog, dashboard-env-catalog, dashboard-prd-template)
- [x] Delete `templates/commands/mustard/dashboard/` directory (SKILL.md)
- [x] Delete `.claude/scripts/dashboard.js` + 4 sibling files — synced copies in this repo (originally hinted 2 siblings; actual was 4)
- [x] Delete `.claude/commands/mustard/dashboard/` directory (synced SKILL.md)
- [x] Update `README.md`: remove `/dashboard` row from commands table, remove `## Dashboard` section, update structure tree under `templates/scripts/`
- [x] Update `docs/upgrade-to-2.0.md`: replace `dashboard.js --check` references with Mustard Dashboard install instructions; update troubleshooting line 150
- [x] Update comments in `templates/hooks/metrics-tracker.js:109` + `templates/hooks/_lib/harness-event.js:114` — replace "dashboard" wording with "Mustard Dashboard (standalone)"
- [x] Update `docs/mcp-tools.md` Dashboard deprecation section (review WARNING fix)
- [x] Verify build still passes: `npm run build && bun test templates/hooks/__tests__/hooks.test.js`

### Deferred to `mustard-dashboard` repo
- [ ] Tauri init + Vite + React + TS scaffold
- [ ] Tailwind + shadcn/ui setup
- [ ] Design tokens Linear/Notion (tailwind.config, CSS vars, base components Sidebar/StatusDot/CommandPalette/Block/Tabs)
- [ ] Rust: discovery.rs + reader.rs + license.rs + updater.rs
- [ ] TS bridge: api/* commands
- [ ] Views: ProjectList, ProjectDetail, Aggregate, KnowledgeBrowser, Settings
- [ ] Command palette Cmd+K (cmdk)
- [ ] TanStack Query + Zustand wiring
- [ ] License gate (mock keys pra dev)
- [ ] CI: cross-platform matrix build
- [ ] Auto-updater configurado contra GitHub releases

### Out of scope (separate efforts)
- [ ] Landing page (separado deste spec)
- [ ] Gumroad/Lemon Squeezy setup (separado)

## Next steps após CLOSE desta spec

1. Criar repo dedicado: `cd C:\Atiz && mkdir mustard-dashboard && cd mustard-dashboard && git init`
2. Rodar `mustard init` (CLI deste repo Mustard) para gerar `.claude/` structure no novo repo
3. Iniciar nova spec lá: `/mustard:feature dashboard-tauri-scaffold` — usar este spec como referência de contrato (decisões já tomadas)
4. As decisões preservadas: Tauri, Direct SQLite, Linear+Notion style, 5 views, license HMAC, GitHub Releases distribution
