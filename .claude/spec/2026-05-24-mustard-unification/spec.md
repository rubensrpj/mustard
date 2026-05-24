# Unificação do Mustard — refatoração profunda e metrificação real

## PRD

## Contexto

O Mustard amadureceu em Rust ao longo de 2026-05. As 55+ specs entre `b1-monorepo-merge` e `2026-05-22-project-profiler` estão fechadas e a base técnica é sólida: monorepo pnpm + Cargo, CLI/runtime/dashboard em Rust, scan paralelo via passada única (rayon) com cold-path interpretativo, vault Obsidian gerado automaticamente. Mas a inspeção arquivo-a-arquivo de `apps/cli/templates/` revelou que o payload em markdown ainda carrega peso de duas eras: trechos antigos quando o pipeline era em JS/bun, duplicação de regras em três ou mais arquivos, skills de terceiros grandes que ninguém invoca por padrão, e `SKILL.md` inflados (feature 353 linhas, bugfix 240, close 220) que poderiam delegar para os 87 subcomandos `mustard-rt run` que já existem.

Ao mesmo tempo, persistem lacunas críticas no runtime:

1. `verify-pipeline` da fase CLOSE roda `npm test` em projeto Rust e dá timeout (gate genérico que não respeita a stack).
2. `entity-registry.json` saiu vazio do último scan porque o cold-path interpret ainda invoca o modelo via SDK Anthropic direto, exigindo `ANTHROPIC_API_KEY`. A regra é que TODO acesso a modelo no Mustard passe por subprocess do binário `claude` (Claude CLI — onde a assinatura do usuário já paga). Sem SDK paralelo, sem validar chave.
3. A memória entre agentes existe (`mustard.db` com `knowledge_patterns`, `memory_decisions`, `memory_lessons`) mas é injetada read-only, sem feedback bidirecional, sem escopo por spec, sem auto-injeção de memória cross-wave.
4. A injeção de contexto em `SessionStart` é grande (top-5 patterns + top-5 decisions + top-5 lessons ≈ 1500 tokens) sem filtrar pela spec ativa.
5. O `mustard.db` está inchando indefinidamente pela tabela `events` (com FTS5) que cada hook escreve a cada tool call. Hot path penalizado por `SQLite open + INSERT + commit` (~100-500 µs) e múltiplos subagentes brigando pelo lock único. Existe spec ativa `2026-05-23-per-spec-event-log-claude-devtools` (5 ondas planejadas) que dropa essa tabela e move para NDJSON per-spec — essa spec é absorvida aqui.
6. A página `/specs` do dashboard ficou lenta. O componente de grafo interno é o gargalo de render e duplica o vault Obsidian que o `mustard-rt` já gera. Decisão: remover o grafo do dashboard; wikilinks `[[X]]` abrem no Obsidian via URI scheme.
7. `telemetry.db` foi separado em `2026-05-22-telemetry-separation` (já fechada), mas os subdirs `review/spec.md` e `qa/spec.md` ficaram pendentes — esta unificação fecha esses gates e adiciona retention configurável.

Esta mega-spec consolida tudo num único contrato de 14 ondas paralelizáveis. Absorve as specs ativas `2026-05-24-meta-sidecar`, `2026-05-24-config-idioma-tom` e `2026-05-23-per-spec-event-log-claude-devtools` como ondas internas; aposenta `2026-05-20-economia-moat-unification` como superseded (escopo era dashboard, não unificação); deixa `2026-05-20-dashboard-prd-ai-lapidator` rodando em paralelo (UI redesign desacoplado).

Plano completo aprovado em `C:\Users\ruben\.claude\plans\o-mustard-vem-passando-humble-whale.md`.

## Usuários/Stakeholders

- **Rubens** (usuário único atual) — operador do Mustard via CLI + dashboard local; quer Mustard como referência de desenvolvimento com SDD; quer ver economia real medida, não estimada.
- **O próprio Mustard** (`mustard-rt` hooks, `mustard-cli` comandos, dashboard Tauri) — passa a consumir 87 subcomandos novos/existentes em vez de monólitos `.md`.
- **Quem mantém o código** — herda template enxuto (≤1100 linhas de `SKILL.md` no total, vs ~2630 hoje), `mustard.db` redesenhado do zero, memória cross-session com feedback.

## Métrica de sucesso

A página `/economia` do dashboard mostra economia real medida (não estimada em ADR) das 12 ondas internas (W2-W11). Soma cumulativa do indicador "Unification savings (total)" não-zero e maior que 20.000 tokens por sessão típica (uma feature + um bugfix + um close). Build verde em todo o workspace (`cargo build --workspace && cargo clippy --workspace -- -D warnings` + `pnpm --filter mustard-dashboard build`). Tamanho do `mustard.db` em projeto canário menor que 1 MB pós-cleanup. Página `/specs` interativa em menos de 200 ms para vault com 100 specs.

## Não-Objetivos

- **Reabsorver o que já foi entregue**: `2026-05-22-project-profiler` (5 ondas fechadas — scan paralelo, vault Obsidian, BFS de fecho mínimo) é citado como fundação consumida, nunca reimplementado.
- **Refatorar o redesign de UI do dashboard** (`/dashboard/lapidator/PRD`): roda em paralelo na spec `2026-05-20-dashboard-prd-ai-lapidator`.
- **Migrar dados de specs antigas para o novo schema do `mustard.db`**: padrão fase-dev — drop limpo, sem migration formal (cf. [[feedback_no_migration_dev_phase]]).
- **Reintroduzir SDK Anthropic em Rust**: todo LLM call é via subprocess `claude` CLI; sem `reqwest` para `api.anthropic.com`; sem validar `ANTHROPIC_API_KEY`.
- **Manter o componente de grafo interno do dashboard**: Obsidian é o canonical viewer; wikilinks redirecionam via `obsidian://` URI.
- **Estimar economia em prosa**: nenhum delta declarado sem antes virar `baseline + post` mensurado em `economy_baselines` / `economy_savings` do `telemetry.db`.
- **Cobrir hooks Claude Code que o enum `Trigger` não modela hoje além de `Stop` e `Notification`**: outros hooks futuros ficam fora.

## Critérios de Aceitação

ACs autoritativos vivem em cada `wave-N-{role}/spec.md` (14 ondas). `/mustard:qa` agrega no momento da execução. ACs globais agregados:

- [ ] **AC-G1.** `cargo build --workspace && cargo clippy --workspace -- -D warnings` passa após todas as ondas. Command: `rtk cargo build --workspace && rtk cargo clippy --workspace -- -D warnings`
- [ ] **AC-G2.** `pnpm --filter mustard-dashboard build && pnpm --filter mustard-dashboard lint` passa. Command: `rtk pnpm --filter mustard-dashboard build && rtk pnpm --filter mustard-dashboard lint`
- [ ] **AC-G3.** Tabela `events` foi dropada de `mustard.db`. Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \".schema events\"',{encoding:'utf8'}).trim();process.exit(out===''?0:1)"`
- [ ] **AC-G4.** Schema final do `mustard.db` em `packages/core/src/store/sqlite_schema.sql` não contém `events`, `events_fts`, `knowledge` (legacy JS) nem `metrics_projection`. Command: `node -e "const fs=require('fs');const t=fs.readFileSync('packages/core/src/store/sqlite_schema.sql','utf8');for(const k of ['CREATE TABLE events','CREATE VIRTUAL TABLE events_fts','CREATE TABLE knowledge ','CREATE TABLE metrics_projection']){if(t.includes(k)){console.error('still has',k);process.exit(1)}}"`
- [ ] **AC-G5.** Tabela `agent_memory` existe em `mustard.db` com colunas `spec`, `wave`, `confidence`, `status`. Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \".schema agent_memory\"',{encoding:'utf8'});for(const k of ['spec','wave','confidence','status']){if(!out.includes(k)){console.error('missing column',k);process.exit(1)}}"`
- [ ] **AC-G6.** Toda spec sob `.claude/spec/**` tem um `meta.json` válido (consumido por dashboard via `read_spec_meta`). Command: `node -e "const fs=require('fs'),path=require('path');const root='.claude/spec';for(const d of fs.readdirSync(root)){const p=path.join(root,d);if(!fs.statSync(p).isDirectory())continue;const m=path.join(p,'meta.json');if(!fs.existsSync(m)){console.error('missing',m);process.exit(1)}}"`
- [ ] **AC-G7.** `mustard.json` tem `lang` em formato BCP-47 (`pt-BR` ou `en-US`). Command: `node -e "const fs=require('fs');const j=JSON.parse(fs.readFileSync('mustard.json','utf8'));if(!/^(pt-BR|en-US)$/.test(j.lang||''))process.exit(1)"`
- [ ] **AC-G8.** `mustard-rt run doctor` reporta presença do binário `claude` no PATH (não exige `ANTHROPIC_API_KEY`). Command: `rtk mustard-rt run doctor --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.checks?.find(c=>c.name==='claude_cli'))process.exit(1)})"`
- [ ] **AC-G9.** Soma de linhas dos `SKILL.md` em `apps/cli/templates/commands/mustard/**` ≤ 1100. Command: `node -e "const{execSync}=require('child_process');const out=execSync('find apps/cli/templates/commands/mustard -name SKILL.md -exec wc -l {} +',{encoding:'utf8'});const total=out.split('\\n').reverse().find(l=>l.includes('total'));const n=parseInt(total.trim().split(/\\s+/)[0]);process.exit(n<=1100?0:1)"`
- [ ] **AC-G10.** Skills opt-in (`hallmark`, `design-craft`, `react-best-practices`, `grill-me`) NÃO estão em `apps/cli/templates/skills/` (movidas para `apps/cli/templates-extras/skills/`). Command: `node -e "const fs=require('fs');for(const s of ['hallmark','design-craft','react-best-practices','grill-me']){if(fs.existsSync('apps/cli/templates/skills/'+s))process.exit(1)}"`
- [ ] **AC-G11.** Dashboard `/specs` não tem dependência de força-grafo no `package.json`. Command: `node -e "const j=JSON.parse(require('fs').readFileSync('apps/dashboard/package.json','utf8'));for(const k of ['react-force-graph','react-force-graph-2d','d3-force','vis-network']){if(j.dependencies?.[k]||j.devDependencies?.[k]){console.error('still has',k);process.exit(1)}}"`
- [ ] **AC-G12.** `telemetry.db` tem tabelas `economy_baselines` e `economy_savings` populadas com ≥ 1 row por onda W2-W11. Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.telemetry/telemetry.db \"SELECT DISTINCT wave_id FROM economy_savings\"',{encoding:'utf8'}).trim().split('\\n');const want=['W2','W3','W4','W5','W6','W7','W8','W9','W10','W11'];for(const w of want){if(!out.includes(w)){console.error('missing wave',w);process.exit(1)}}"`
- [ ] **AC-G13.** Triggers `Stop` e `Notification` modelados no enum em `packages/core/src/model/contract.rs`. Command: `node -e "const t=require('fs').readFileSync('packages/core/src/model/contract.rs','utf8');if(!/Stop\\b/.test(t)||!/Notification\\b/.test(t))process.exit(1)"`
- [ ] **AC-G14.** Backup de specs em `~/.mustard-backups/2026-05-24-pre-unification/` existe com `MANIFEST.json`. Command: `node -e "const fs=require('fs'),os=require('os'),path=require('path');const p=path.join(os.homedir(),'.mustard-backups/2026-05-24-pre-unification/MANIFEST.json');if(!fs.existsSync(p))process.exit(1)"`
- [ ] **AC-G15.** Adapter `adapters/cursor/adapter.js` removido; `mustard-rt run adapt-cursor` existe. Command: `node -e "const fs=require('fs');if(fs.existsSync('apps/cli/templates/adapters/cursor/adapter.js'))process.exit(1);const t=fs.readFileSync('apps/rt/src/run/mod.rs','utf8');if(!/adapt_cursor|adapt-cursor/.test(t))process.exit(1)"`

## Plano

Ver `wave-plan.md`. Tabela resumida:

| W | Nome curto | Role | Depende | Paralelizável com |
|---|---|---|---|---|
| 0 | stop-the-bleeding | mixed | — | — |
| 1 | worktree-gc | rt | 0 | 2 |
| 2 | scan-cold-path-bootstrap | rt | 0 | 1 |
| 3 | spec-meta-sidecar (absorve `2026-05-24-meta-sidecar`) | rt | 2 | — |
| 4 | language-and-tone (absorve `2026-05-24-config-idioma-tom`) | mixed | 3 | 5 |
| 5 | mustard-db-redesign + per-spec-event-log + dashboard-fast (absorve `2026-05-23-per-spec-event-log-claude-devtools`) | mixed | 3 | 4 |
| 6 | rt-new-subcommands | rt | 4, 5 | — |
| 7 | templates-cuts + opt-in-skills | cli | 6 | 8 |
| 8 | shared-memory-hardening | rt | 5, 6 | 7 |
| 9 | context-injection-optimization | rt | 8 | — |
| 10 | stop-and-notification-triggers | rt | 6 | 11 |
| 11 | verify-pipeline-multistack | rt | 0 | 10 |
| 12 | telemetry-perf-followup + economy-dashboard-wiring | mixed | 6, 9 | — |
| 13 | close-and-archive | mixed | 0-12 | — |

## Cobertura — críticas e pedidos

| Crítica / pedido | Onde está |
|---|---|
| Todos os comandos e skills de template abordados | Seção "Cobertura total dos templates" no plano (cobre 18 commands + 13 skills + 24 refs + 5 recipes + settings.json + CLAUDE.md). Implementada por W7. |
| Análise cruzada entre as frentes | Seção "Consistência cruzada — overlaps controlados" no plano. 9 arquivos compartilhados entre ondas com sequência segura documentada. |
| mustard-rt + CLI evitando IA para reduzir custo | Tabela "Economia de IA" no plano + protocolo de métrica. 26 operações substituídas. AC-G12 garante dados reais em `/economia`. |
| Tudo metrificável em `/economia` | Objetivo #4 do contexto + seção "Protocolo de métrica" + W12 (3 subcomandos novos: `economy capture-baseline`, `reconcile`, `report`). AC-G12. |
| pt-BR/en-US (não pt/en abreviado) | W4 entrega `i18n.rs` com enum `Locale { PtBr, EnUs }`. AC-G7. |
| `mustard.db` redesenhado para performance | W5.T5.6 — schema refeito do zero, não ALTER. Drop legacy. Índices auditados. AC-G3, AC-G4, AC-G5. |
| Dashboard lento na área de specs | W5.T5.7 — remover grafo interno; wikilinks via `obsidian://`; lista virtualizada; cache. AC-G11. |
| Timeline spec/wave estilo claude-devtools | W5.T5.3 — rewrite `SpecTimelineTab` + `PipelineTimeline` + per-tool renderers + recursão Task via `parent_id`. |
| Backup das specs antes da unificação | W0 chama `mustard-rt run backup-specs` (subcomando entregue em W6). Cópia para `~/.mustard-backups/2026-05-24-pre-unification/`. AC-G14. |
| LLM call via `claude` CLI, não SDK | W2 reescreve cold-path para subprocess. Sem `ANTHROPIC_API_KEY` em `doctor`. AC-G8. |
| Skills 3rdparty opt-in | W7 move `hallmark`, `design-craft`, `react-best-practices`, `grill-me` para `templates-extras/`. `mustard add skill:nome` instala on-demand via `skill-fetch`. AC-G10. |
| Compartilhamento de memória entre agentes cross-session | W8 (agent_memory + feedback + scope-by-spec) + W9 (auto-capture summary + scoped inject). |
| Otimizar carga de contexto | W9 (`context-budget`, `context-slice` estendido para CLAUDE.md, `--budget-tokens` em `agent-prompt-render`). |
| Cada comando eficiente | W6 (12+3 subcomandos novos) + W7 (cortes nos SKILL.md). |

## Plano completo

Documento detalhado em `C:\Users\ruben\.claude\plans\o-mustard-vem-passando-humble-whale.md` (decisões estratégicas, política Rust vs Markdown, mapa de absorção, dependências, política de acesso a modelo, protocolo de métrica, mapeamento de issues).
