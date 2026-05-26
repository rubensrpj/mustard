# No SQLite — Git como fonte de verdade

### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full
### Checkpoint: 2026-05-26T00:00:00Z
### Lang: pt-BR

## PRD

## Contexto

Hoje o Mustard mantém dois bancos SQLite locais por projeto — `mustard.db` (~5.5 MB no repo do próprio Mustard, dos quais 5.1 MB são lixo de schema antigo) e `telemetry.db` (run_usage + usage_totals + economy_baselines/savings + run_attribution). Ambos são locais, não-versionados, e contêm o histórico de execução, telemetria, conhecimento e memória. Isso cria três problemas convergentes. Primeiro, o `mustard.db` ainda guarda a tabela morta `events` mais FTS5 (5.1 MB) que a spec W5 da unification (2026-05-24) declarou removida no schema mas nunca dropou do disco — leitores do dashboard em `apps/dashboard/src-tauri/src/` (105 ocorrências de `json_extract`/`GROUP BY`/`JOIN` em 5 arquivos) ainda consultam essa tabela morta, mantendo um zumbi vivo. Segundo, o desenho atual separa lifecycle (`pipeline_events` em SQLite) de eventos hot path (`.events/*.ndjson` per-spec), criando dois sinks pra manter sincronizados — fonte de bug em divergência. Terceiro, e mais importante: bancos locais não viajam em git. Se Rubens executa `/feature add-login` no Mac e Maria abre o repo no Windows, Maria não vê absolutamente nada do trabalho que aconteceu — todo histórico, decisão, conhecimento, savings de telemetria fica no `.harness/mustard.db` que está em `.gitignore`. O Mustard quer ser usado por múltiplos usuários no mesmo projeto, então memória do projeto precisa ser memória **compartilhada**, e a forma mais natural de compartilhar é via git. Esta spec elimina SQLite por completo: NDJSON local (raw, não-versionado) para eventos de execução granulares, `.summary.json` versionado por spec com o que importa pra outros usuários, knowledge e memory como markdown atômico versionado (uma decisão por arquivo). Reader Rust no Tauri lê filesystem direto, sem cache em disco, com cache em RAM por sessão de dashboard.

## Usuários/Stakeholders

Maintainer único atual (Rubens); usuários futuros que clonarão o repo. Indireto: qualquer projeto-alvo onde `mustard init` foi rodado. Memória [[project_db_bloat_per_spec_events]] e [[feedback_no_attach_sqlite]] confirmam que o caminho SQLite tem fricções históricas que não cedem.

## Métrica de sucesso

Após `mustard init` em projeto fresh, nenhum arquivo `.db` é criado em `.claude/.harness/`. Após executar `/feature` + `/mustard:close` em uma spec, o repo tem `.claude/spec/{name}/.summary.json` versionado (≤10 KB típico) com timeline, AC results, decisões, savings agregados e telemetria. Outro usuário clonando o repo e abrindo o dashboard vê todas as specs históricas pelo summary sem precisar dos NDJSON raw. Dashboard responde queries (lista de specs, detalhe de spec, agregado de economia) em <100ms em workspace típico (5-20 specs).

## Não-Objetivos

- Migrar dados existentes em `mustard.db` ou `telemetry.db` — dev phase, sem usuários em prod ([[feedback_no_migration_dev_phase]]). Apaga e recomeça.
- Versionar NDJSON raw em git — fica explicitamente local (`.gitignore`). Tamanho controlado.
- Substituir FTS5 por engine de busca textual — knowledge/memory ficam em markdown atomic com nomes/slugs descritivos; busca é via grep ou Tantivy (Rust) carregado em RAM por sessão de dashboard se preciso.
- Adicionar watcher de filesystem (notify-rs) pra invalidação de cache — cache em RAM invalidado por `mtime` no momento da query é suficiente.
- Manter compatibilidade com schemas antigos ou readers legacy — corte limpo.
- Tocar templates do `mustard init` (`apps/cli/templates/`) — só `.gitignore` deles atualiza. Toda mudança é em código Rust + dashboard React.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: nenhum arquivo `.db` em `.claude/.harness/` após `mustard init` greenfield — Command: `bash -c 'd=$(mktemp -d); cd $d && cargo run -q -p mustard-cli --manifest-path /caminho-do-mustard/Cargo.toml -- init --yes; test ! -f .claude/.harness/mustard.db && test ! -f .claude/.harness/telemetry.db'`
- [ ] AC-2: `packages/core/src/store/` não existe — Command: `node -e "process.exit(require('fs').existsSync('packages/core/src/store')?1:0)"`
- [ ] AC-3: `packages/core/src/telemetry/` não existe — Command: `node -e "process.exit(require('fs').existsSync('packages/core/src/telemetry')?1:0)"`
- [ ] AC-4: `Cargo.toml` dos crates `mustard-core`, `mustard-rt`, `mustard-cli`, `mustard-dashboard` (src-tauri) sem dependency de `rusqlite` — Command: `bash -c 'count=$(grep -l "^rusqlite" packages/core/Cargo.toml apps/rt/Cargo.toml apps/cli/Cargo.toml apps/dashboard/src-tauri/Cargo.toml 2>/dev/null | wc -l); test "$count" = "0"'`
- [ ] AC-5: `cargo build` workspace passa — Command: `cargo build`
- [ ] AC-6: `cargo test` workspace passa (excluindo pre-existing failures documentadas) — Command: `cargo test --workspace --no-fail-fast`
- [ ] AC-7: `mustard-rt run pipeline-summary --spec <qualquer-spec-fechada>` produz JSON com campo `version` numérico — Command: `bash -c 'cargo run -q -p mustard-rt -- pipeline-summary --self-test | node -e "let s=\"\";process.stdin.on(\"data\",c=>s+=c).on(\"end\",()=>{const j=JSON.parse(s);process.exit(typeof j.version===\"number\"?0:1)})"'`
- [ ] AC-8: Dashboard builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-9: Smoke: rodar `mustard-rt run active-specs` lista specs sem precisar de SQLite — Command: `cargo run -q -p mustard-rt -- run active-specs`
- [ ] AC-10: `.claude/knowledge/*.md` e `.claude/memory/*.md` existem (mesmo que vazias) — Command: `node -e "process.exit(require('fs').existsSync('.claude/knowledge')&&require('fs').existsSync('.claude/memory')?0:1)"`

## Plano

## Informações da Entidade

Não há entidade nova de domínio. Agregados tocados: storage layer do `mustard-core` (deletado), telemetry layer do `mustard-core` (deletado), todos os emitters do `mustard-rt` (NDJSON), readers do dashboard Tauri (filesystem), schema do `.summary.json` (novo artefato), formato de markdown atomic para knowledge/memory.

## Arquivos

→ Ver `references/files.md` (decomposição completa por wave; 55-65 arquivos totais)

## Schema do `.summary.json`

Estrutura proposta (a refinar na W1):

```json
{
  "version": 1,
  "spec": "2026-05-26-no-sqlite-git-source-of-truth",
  "title": "No SQLite — Git como fonte de verdade",
  "lang": "pt-BR",
  "tone": "didactic",
  "scope": "full",
  "stage": "Close",
  "outcome": "Completed",
  "timeline": {
    "draft_at": "2026-05-26T...",
    "approved_at": "...",
    "execute_started_at": "...",
    "review_at": "...",
    "qa_at": "...",
    "closed_at": "..."
  },
  "waves": [
    {
      "n": 1,
      "role": "core",
      "summary": "...",
      "status": "completed",
      "ac_results": [{ "id": "AC-W1-1", "pass": true, "command": "..." }],
      "review": "approved",
      "qa": "pass",
      "concerns": []
    }
  ],
  "acceptance_criteria": [
    { "id": "AC-1", "pass": true, "command": "..." }
  ],
  "decisions": [
    { "at": "...", "summary": "...", "memory_link": "knowledge/foo-bar.md" }
  ],
  "economy": {
    "total_savings_tokens": 12345,
    "by_source": { "rtk_rewrite": 8000, "model_routing": 4000 }
  },
  "telemetry": {
    "total_tokens": 500000,
    "total_cost_usd_micros": 1500000,
    "by_model": { "claude-opus-4-7": 350000, "claude-sonnet-4-6": 150000 },
    "by_agent": { "core-impl": 200000, "cli-impl": 150000 }
  },
  "files_affected": ["packages/core/src/i18n.rs", "..."]
}
```

## Tarefas

Decomposição detalhada em ACs por wave fica nas pastas `wave-1-core/` até `wave-8-mixed/` quando aprovado.

## Dependências

- Sem dependência externa nova
- Memórias relevantes: [[project_db_bloat_per_spec_events]], [[feedback_no_attach_sqlite]], [[feedback_no_migration_dev_phase]], [[feedback_everything_measurable]], [[feedback_clear_naming]], [[project_dashboard_value_over_features]]
- Não conflita com a `2026-05-26-template-agnostic-audit` (CONCERN). Não conflita com `2026-05-26-dashboard-i18n-migration` (followup).

## Débito pré-existente absorvido

Os 7 testes de integração failing identificados durante a audit (`mcp`, `spec_children_tree`, `spec_hygiene`, `migrate_to_meta`/`migrate_spec_headers`) são todos baseados em SQLite (memory_sqlite_test, fixtures de pipeline_events, mocks de migrations). Não foram introduzidos pela audit — confirmado via `git log` (origem em commit `189a414` da W5 deep-refactor). Resolução natural: W7 (dashboard reader migration) + W8 (cleanup + validação) desta spec rewrite esses tests pra mockar filesystem em vez de DB. Não tratar como fix tactical isolado — trabalho perdido.

## Limites

- DELETE: 2 diretórios inteiros em `packages/core/src/` (`store/`, `telemetry/`), `apps/rt/src/run/db_maintain.rs`, 5 `mustard.db` físicos, 5 `telemetry.db` físicos
- REWRITE: 5 arquivos do dashboard src-tauri, ~10 arquivos de tests
- MODIFY: ~40 arquivos Rust em `apps/rt/`, `apps/cli/`, `packages/core/`
- CREATE: `packages/core/src/summary/` (mod.rs, writer.rs, schema.md), `.claude/knowledge/`, `.claude/memory/`
- FORA: templates de spec (`spec.md`, `meta.json`), wave structure, recipes (já removidos), scan engine, i18n (já refatorado)
- BREAKING: rows existentes nos 2 bancos serão perdidos (dev phase, sem usuário em prod, [[feedback_no_migration_dev_phase]])

## Cobertura

| Crítica/Preocupação do usuário | Onde foi tratada |
|---|---|
| "mustard.db tem 5.1 MB de lixo da tabela events" | W2 elimina o store inteiro |
| "Eventos no banco quando deveriam estar em NDJSON per-spec" | W4 migra emitters para NDJSON |
| "Banco deve ficar leve, só conhecimento e memória" | Solução: nada SQLite. Tudo arquivo. |
| "Multi-usuário precisa funcionar" | Git como protocolo de sync; `.summary.json` versionado |
| "Dashboard precisa rápido (telemetria + eventos + status)" | Reader Rust no Tauri, cache em RAM por sessão, sem cache em disco |
| "Repo git ficaria inchado se NDJSON raw versiona" | NDJSON raw fica local (`.gitignore`); summary versionado é leve (~10 KB) |
| "Quem rodou tem tudo, outro user vê só o resumo" | Por design: NDJSON local vs summary versionado |
| "Telemetry.db também sai" | W3 elimina, mesma lógica |
| "Knowledge/memory atomic markdown (mesmo padrão de ~/.claude/projects/memory/)" | W6 |
| "Usar Rust onde IA não precisa" | Summary writer + readers do dashboard são código Rust determinístico, sem LLM |
