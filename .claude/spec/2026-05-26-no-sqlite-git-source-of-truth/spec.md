# Refactor de fundação Rust — eliminar SQLite + aplicar SOLID e reuso em todos os `.rs`

### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full
### Checkpoint: 2026-05-27T09:00:00Z
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

Testable, binary (pass/fail) criteria. Each MUST be executable and independent. Comandos shell são POSIX (`bash -c '…'`); checagens de filesystem são `node -e "…"` para portabilidade.

- [ ] AC-1: nenhum arquivo `.db` em `.claude/.harness/` após `mustard init` greenfield — Command: `bash -c 'd=$(mktemp -d); cd $d && cargo run -q -p mustard-cli --manifest-path $OLDPWD/Cargo.toml -- init --yes; test ! -f .claude/.harness/mustard.db && test ! -f .claude/.harness/telemetry.db'`
- [ ] AC-2: `packages/core/src/store/` não existe — Command: `node -e "process.exit(require('fs').existsSync('packages/core/src/store')?1:0)"`
- [ ] AC-3: `packages/core/src/telemetry/` sem `store.rs`, `writer.rs`, `reader.rs`, `schema.sql` — Command: `node -e "const f=require('fs');const dir='packages/core/src/telemetry';if(!f.existsSync(dir))process.exit(0);const stale=['store.rs','writer.rs','reader.rs','schema.sql'].filter(n=>f.existsSync(dir+'/'+n));process.exit(stale.length?1:0)"`
- [ ] AC-4: `Cargo.toml` dos crates `mustard-core`, `mustard-rt`, `mustard-cli`, `mustard-dashboard` (src-tauri) sem dependency de `rusqlite` — Command: `bash -c 'set -e; for f in Cargo.toml packages/core/Cargo.toml apps/rt/Cargo.toml apps/cli/Cargo.toml apps/dashboard/src-tauri/Cargo.toml; do if grep -E "^rusqlite" "$f" >/dev/null 2>&1; then echo "rusqlite in $f"; exit 1; fi; done; exit 0'`
- [ ] AC-5: zero matches do regex `SqliteEventStore|sqlite_store|sqlite_schema|memory_sqlite` em arquivos `.rs`/`.sql`/`.toml` sob `packages/` e `apps/` — Command: `bash -c 'count=$(grep -rE "SqliteEventStore|sqlite_store|sqlite_schema|memory_sqlite" packages apps --include="*.rs" --include="*.sql" --include="*.toml" 2>/dev/null | wc -l); test "$count" = "0"'`
- [ ] AC-6: zero `use .*::rusqlite` / `rusqlite::` em arquivos `.rs` sob `packages/` e `apps/` — Command: `bash -c 'count=$(grep -rE "(^use [^;]*::)?rusqlite::" packages apps --include="*.rs" 2>/dev/null | wc -l); test "$count" = "0"'`
- [ ] AC-7: nenhum arquivo `.rs` sob `packages/` ou `apps/` contém `sqlite` no nome — Command: `bash -c 'count=$(find packages apps -name "*sqlite*.rs" -o -name "*sql_*.rs" 2>/dev/null | wc -l); test "$count" = "0"'`
- [ ] AC-8: `cargo build --workspace` passa — Command: `cargo build --workspace`
- [ ] AC-9: `cargo test --workspace --no-fail-fast` passa — Command: `cargo test --workspace --no-fail-fast`
- [ ] AC-10: `mustard-rt run pipeline-summary --self-test` produz JSON com campo `version` numérico — Command: `bash -c 'cargo run -q -p mustard-rt -- run pipeline-summary --self-test | node -e "let s=\"\";process.stdin.on(\"data\",c=>s+=c).on(\"end\",()=>{const j=JSON.parse(s);process.exit(typeof j.version===\"number\"?0:1)})"'`
- [ ] AC-11: Dashboard builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-12: Smoke: `mustard-rt run active-specs` lista specs sem precisar de SQLite — Command: `cargo run -q -p mustard-rt -- run active-specs`
- [ ] AC-13: `.claude/knowledge/*.md` e `.claude/memory/*.md` existem como diretórios — Command: `node -e "const f=require('fs');process.exit(f.existsSync('.claude/knowledge')&&f.existsSync('.claude/memory')?0:1)"`
- [ ] AC-14: primitivos `EventReader` e `MarkdownStore` existem em `mustard-core` — Command: `bash -c 'test -f packages/core/src/events/reader.rs && test -f packages/core/src/atomic_md/store.rs'`
- [ ] AC-15: benchmarks de performance dos primitivos passam — Command: `bash -c 'cargo test -p mustard-core --release events::reader::bench atomic_md::store::bench 2>&1 | grep -E "(test result: ok|all .* tests passed)"'`
- [ ] AC-16: `apps/rt/src/run/wikilink.rs` não existe — Command: `node -e "process.exit(require('fs').existsSync('apps/rt/src/run/wikilink.rs')?1:0)"`
- [ ] AC-17: `mustard_core::atomic_md::wikilink::render_footer` resolve links + marca órfãos + idempotente — Command: `cargo test -p mustard-core atomic_md::wikilink::test_render_footer`
- [ ] AC-18: hook `wikilink_footer` aplica rodapé em `.md` salvo (integration) — Command: `cargo test -p mustard-rt --test wikilink_footer_hook`

## Plano

Ver `wave-plan.md` para a decomposição completa em **39 sub-specs** distribuídas em 9 ondas. **Wave 9 (T1-T3, 17 sub-specs)** aplica auditoria SOLID + reuso ao restante dos ~155 `.rs` que não tocam SQLite — integrado após directive do usuário (2026-05-26) de que SOLID/reuso é regra de projeto, não de uma spec. W1 foi partido em **W1A (summary) + W1B (EventReader NDJSON) + W1C (MarkdownStore atomic)** para entregar os **primitivos compartilhados** que todas as sub-specs downstream consomem (SOLID/DRY/performance — sem reimplementar parser de NDJSON ou de markdown em cada módulo). Sub-specs respeitam **cap rígido de ≤5 arquivos cada** para caberem no orçamento de contexto de uma sessão Opus sem overflow. Cada sub-spec commita com `cargo build --workspace` verde, e o invariante de decrescimento monotônico (count de matches `SqliteEventStore|sqlite_store|memory_sqlite` cai estritamente após cada commit) trava regressão.

| W | Nome | Role | Depende | Status |
|---|---|---|---|---|
| 1A | summary-foundation | core | — | 📋 |
| 1B | ndjson-event-reader-primitive | core | — | 📋 |
| 1C | markdown-store-primitive | core | — | 📋 |
| 2A | rt-readers-active-and-projections | rt | W1 | 📋 |
| 2B | rt-readers-resume-and-metrics | rt | W1 | 📋 |
| 2C | rt-emit-pipeline-and-route | rt | W1 | 📋 |
| 3A | rt-hooks-savings-and-budget | rt | W2A,W2B,W2C | 📋 |
| 3B | rt-hooks-session-and-stop | rt | W2A,W2B,W2C | 📋 |
| 3C | rt-hooks-amend-and-spec-hygiene | rt | W2A,W2B,W2C | 📋 |
| 3D | rt-hooks-misc-cleanup | rt | W2A,W2B,W2C | 📋 |
| 3E | rt-hook-wikilink-footer | rt | W1C | 📋 |
| 4A | rt-run-spec-and-skills-and-verify | rt | W3A-D | 📋 |
| 4B | rt-run-memory-and-knowledge-md | rt | W3A-D | 📋 |
| 4C | rt-run-complete-and-epic | rt | W3A-D | 📋 |
| 5A | rt-otel-rewrite-ndjson | rt | W4A-C | 📋 |
| 5B | rt-mcp-and-orphan-tests | rt | W4A-C | 📋 |
| 6A | dashboard-spec-and-economy-readers | dashboard | W4A-C | 📋 |
| 6B | dashboard-telemetry-and-amend-readers | dashboard | W4A-C | 📋 |
| 7 | core-economy-ndjson | core | W4A-C | 📋 |
| 8A | delete-store-and-telemetry-modules | core | W2-W7 verdes | 📋 |
| 8B | delete-rusqlite-deps-and-orphan-tests | mixed | W8A | 📋 |
| 8C | smoke-and-final-validation | mixed | W8B | 📋 |
| 9A | T1.1 refactor-stdin-hook-io-primitive | core | W8C | 📋 |
| 9B | T1.2 refactor-hook-helpers-and-gate-mode | rt | W8C | 📋 |
| 9C | T1.3 refactor-time-and-fs-primitives-to-core | core | W8C | 📋 |
| 9D | T1.4 refactor-process-helpers-cross-platform | mixed | W8C | 📋 |
| 9E | T1.5 refactor-mustard-json-and-config-readers | mixed | W8C | 📋 |
| 9F | T2.1 refactor-run-mod-clap-split | rt | W8C | 📋 |
| 9G | T2.2 refactor-bash-substring-matchers | rt | W8C | 📋 |
| 9H | T2.3 refactor-doctor-checks-split | rt | W8C | 📋 |
| 9I | T2.4 refactor-emit-pipeline-fold | rt | W8C | 📋 |
| 9J | T2.5 refactor-scan-cluster-discovery-shrink | rt | W8C | 📋 |
| 9K | T2.6 refactor-claude-paths-spec-shrink | core | W8C | 📋 |
| 9L | T3.1 cleanup-dead-protocol-rs | rt | W9A | 📋 |
| 9M | T3.2 cleanup-sha256-helpers | rt | W8C | 📋 |
| 9N | T3.3 cleanup-emit-error-helper | rt | W8C | 📋 |
| 9O | T3.4 cleanup-unwrap-and-expect-non-test | mixed | W8C | 📋 |
| 9P | T3.5 cleanup-redundant-clones-and-allocations | mixed | W8C | 📋 |
| 9Q | T3.6 cleanup-spec-md-header-readers | mixed | W8C | 📋 |

## Informações da Entidade

Não há entidade nova de domínio. Agregados tocados: storage layer do `mustard-core` (deletado), telemetry layer do `mustard-core` (deletado), todos os emitters do `mustard-rt` (NDJSON), readers do dashboard Tauri (filesystem), schema do `.summary.json` (novo artefato), formato de markdown atomic para knowledge/memory.

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

Decomposição detalhada em ACs por wave/sub-spec fica nas pastas `wave-1-core/`, `wave-2a-rt/`, `wave-2b-rt/`, …, `wave-8c-mixed/` (cada sub-spec tem seu próprio `spec.md` com ACs binários, lista exata de ≤5 arquivos, e comandos de verificação shell-executáveis).

## Dependências

- Sem dependência externa nova
- Memórias relevantes: [[project_db_bloat_per_spec_events]], [[feedback_no_attach_sqlite]], [[feedback_no_migration_dev_phase]], [[feedback_everything_measurable]], [[feedback_clear_naming]], [[project_dashboard_value_over_features]]
- Não conflita com a `2026-05-26-template-agnostic-audit` (CONCERN). Não conflita com `2026-05-26-dashboard-i18n-migration` (followup).

## Débito pré-existente absorvido

Os 7 testes de integração failing identificados durante a audit (`mcp`, `spec_children_tree`, `spec_hygiene`, `migrate_to_meta`/`migrate_spec_headers`) são todos baseados em SQLite (memory_sqlite_test, fixtures de pipeline_events, mocks de migrations). Não foram introduzidos pela audit — confirmado via `git log` (origem em commit `189a414` da W5 deep-refactor). Resolução natural: as ondas 5, 6 e 8 desta spec rewrite esses tests pra mockar filesystem em vez de DB. Não tratar como fix tactical isolado — trabalho perdido.

## Limites

- DELETE: 2 diretórios inteiros em `packages/core/src/` (`store/`, partes de `telemetry/`), `apps/rt/src/run/db_maintain.rs`, `apps/rt/src/run/backfill_run_usage_*.rs`, `apps/rt/src/run/otel/store.rs`, `packages/core/src/reader/sqlite.rs`, 5 `mustard.db` físicos, 5 `telemetry.db` físicos
- REWRITE: 6 arquivos do dashboard src-tauri (src), 6 arquivos de tests do dashboard, ~10 arquivos de tests do core+rt
- MODIFY: ~55 arquivos Rust em `apps/rt/`, `apps/cli/`, `packages/core/`
- CREATE: `packages/core/src/summary/` (mod.rs, writer.rs, schema.md — entregue na W1 do trabalho anterior, validar e preservar), `packages/core/src/reader/fs.rs` (filesystem reader, substituto do `sqlite.rs`), `.claude/knowledge/.gitkeep`, `.claude/memory/.gitkeep`
- FORA: templates de spec (`spec.md`, `meta.json`), wave structure, recipes (já removidos), scan engine, i18n (já refatorado)
- BREAKING: rows existentes nos 2 bancos serão perdidos (dev phase, sem usuário em prod, [[feedback_no_migration_dev_phase]])

## Cobertura

| Crítica/Preocupação do usuário | Onde foi tratada |
|---|---|
| "mustard.db tem 5.1 MB de lixo da tabela events" | W8A deleta o store inteiro |
| "Eventos no banco quando deveriam estar em NDJSON per-spec" | W2C migra emitters; W3A-D + W4A-C migram readers para NDJSON |
| "Banco deve ficar leve, só conhecimento e memória" | Solução: nada SQLite. Tudo arquivo. |
| "Multi-usuário precisa funcionar" | Git como protocolo de sync; `.summary.json` versionado (W1) |
| "Dashboard precisa rápido (telemetria + eventos + status)" | W6A+W6B reescrevem readers do Tauri para filesystem, cache em RAM por sessão |
| "Repo git ficaria inchado se NDJSON raw versiona" | NDJSON raw fica local (`.gitignore`); summary versionado é leve (~10 KB) |
| "Quem rodou tem tudo, outro user vê só o resumo" | Por design: NDJSON local vs summary versionado |
| "Telemetry.db também sai" | W5 (OTEL) + W7 (economy) + W8A (telemetry/store, writer, reader) eliminam |
| "Knowledge/memory atomic markdown (mesmo padrão de ~/.claude/projects/memory/)" | W4B |
| "Usar Rust onde IA não precisa" | Summary writer + readers do dashboard são código Rust determinístico, sem LLM |
| "Sem nome `sqlite` em arquivo, módulo, tipo, struct, função ou comentário" | AC-5, AC-6, AC-7 (binários e verificáveis em qualquer shell) |
| "Sem stubs preservando o nome" | Política de plano: callers migram **antes** de o módulo morrer; W8A só remove código já órfão |